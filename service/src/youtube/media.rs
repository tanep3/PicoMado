//! Slow YouTube effects. All filesystem scans, JPEG decoding, child processes,
//! network requests, and device sends must run outside the application lock.

use super::*;
use image::{GenericImageView, ImageFormat};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

const FIT: &str = "scale=160:80:flags=bicubic:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1,pad=160:80:(ow-iw)/2:(oh-ih)/2:color=black";

#[derive(Clone, Debug)]
pub struct MediaTools {
    pub root: PathBuf,
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub node: Option<PathBuf>,
}
impl MediaTools {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            yt_dlp: "yt-dlp".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            node: None,
        }
    }
}

fn storage(error: std::io::Error) -> YoutubeError {
    YoutubeError::Storage(error.to_string())
}
fn tool_error(name: &str, stderr: &str) -> YoutubeError {
    let text = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.contains("403 Forbidden")
        || text.contains("HTTP Error 403")
        || text.contains("HTTP error 403")
    {
        YoutubeError::Tool("YouTubeが動画の取得URLを拒否しました（HTTP 403）".into())
    } else {
        // Never return tool output: it may contain signed source URLs or cookies.
        YoutubeError::Tool(format!("{name} の処理に失敗しました"))
    }
}
fn send(
    progress: &UnboundedSender<Progress>,
    job: &PrepareJob,
    stage: JobStage,
    percent: u8,
    message: &str,
) {
    let _ = progress.send(Progress {
        generation: job.generation,
        stage,
        percent,
        message: message.to_owned(),
        metadata: None,
        source: None,
        conversion: None,
    });
}
fn send_metadata(progress: &UnboundedSender<Progress>, job: &PrepareJob, metadata: &VideoMetadata) {
    let _ = progress.send(Progress {
        generation: job.generation,
        stage: JobStage::Metadata,
        percent: 0,
        message: "動画情報を確認しました".into(),
        metadata: Some(metadata.clone()),
        source: None,
        conversion: None,
    });
}
fn send_source(progress: &UnboundedSender<Progress>, job: &PrepareJob, source: &SourceCache) {
    let _ = progress.send(Progress {
        generation: job.generation,
        stage: JobStage::Download,
        percent: 0,
        message: "元素材を保存しました".into(),
        metadata: None,
        source: Some(source.clone()),
        conversion: None,
    });
}
fn send_conversion(progress: &UnboundedSender<Progress>, job: &PrepareJob, item: &ConversionCache) {
    let _ = progress.send(Progress {
        generation: job.generation,
        stage: JobStage::Ready,
        percent: 99,
        message: "完成キャッシュを保存しました".into(),
        metadata: None,
        source: None,
        conversion: Some(item.clone()),
    });
}
fn safe_root(root: &Path) -> Result<PathBuf, YoutubeError> {
    fs::create_dir_all(root).map_err(storage)?;
    root.canonicalize().map_err(storage)
}
fn video_dir(root: &Path, id: &str) -> Result<PathBuf, YoutubeError> {
    if !valid_id(id) {
        return Err(YoutubeError::InvalidUrl);
    }
    let parent = root.join("videos");
    if parent
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(YoutubeError::InvalidFrame);
    }
    let path = parent.join(id);
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(YoutubeError::InvalidFrame);
    }
    Ok(path)
}
fn source_path(root: &Path, source: &SourceCache) -> Result<PathBuf, YoutubeError> {
    check_cache_ids(&source.video_id, &source.key)?;
    if source.extension.len() > 8 || !source.extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(YoutubeError::InvalidFrame);
    }
    let parent = video_dir(root, &source.video_id)?.join("sources");
    if parent
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(YoutubeError::InvalidFrame);
    }
    Ok(parent.join(format!("{}.{}", source.key, source.extension)))
}
fn converted_path(root: &Path, item: &ConversionCache) -> Result<PathBuf, YoutubeError> {
    check_cache_ids(&item.video_id, &item.key)?;
    let parent = video_dir(root, &item.video_id)?.join("converted");
    if parent
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(YoutubeError::InvalidFrame);
    }
    Ok(parent.join(&item.key))
}
fn frame_name(index: u64) -> String {
    format!("frame{:08}.jpg", index + 1)
}

fn frame_number(name: &std::ffi::OsStr) -> Option<u64> {
    let name = name.to_str()?;
    if name.len() != 17
        || !name.starts_with("frame")
        || !name.ends_with(".jpg")
        || !name.as_bytes()[5..13].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    name[5..13].parse::<u64>().ok().filter(|n| *n > 0)
}

/// Constant-memory manifest scan. Distinct directory entry names and a count
/// equal to the maximum index prove that every number in 1..=count exists.
fn inspect_manifest(
    path: &Path,
    cancel: &Cancellation,
) -> Result<Option<(u64, u64)>, YoutubeError> {
    let meta = match path.symlink_metadata() {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(storage(e)),
    };
    if !meta.file_type().is_dir() {
        return Ok(None);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(storage(e)),
    };
    let mut count = 0u64;
    let mut highest = 0u64;
    let mut bytes = 0u64;
    for entry in entries {
        cancel.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage(e)),
        };
        let Some(number) = frame_number(&entry.file_name()) else {
            return Ok(None);
        };
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage(e)),
        };
        if !meta.file_type().is_file() || !(4..=MAX_JPEG_BYTES).contains(&meta.len()) {
            return Ok(None);
        }
        count = count.checked_add(1).ok_or(YoutubeError::Overflow)?;
        highest = highest.max(number);
        bytes = bytes
            .checked_add(meta.len())
            .ok_or(YoutubeError::Overflow)?;
    }
    cancel.check()?;
    if count == 0 || count != highest {
        Ok(None)
    } else {
        Ok(Some((count, bytes)))
    }
}

pub fn validate_conversion_cancellable(
    root: &Path,
    item: &ConversionCache,
    cancel: &Cancellation,
) -> Result<bool, YoutubeError> {
    if item.frames == 0 || item.frames > 99_999_999 || item.fps.value() == 0 {
        return Ok(false);
    }
    Ok(inspect_manifest(&converted_path(root, item)?, cancel)? == Some((item.frames, item.bytes)))
}
/// Checks the entire completed manifest without decoding every JPEG again.
/// Use `validate_conversion_cancellable` for an active user job.
pub fn validate_conversion(root: &Path, item: &ConversionCache) -> Result<bool, YoutubeError> {
    validate_conversion_cancellable(root, item, &Cancellation::default())
}
fn valid_source(root: &Path, item: &SourceCache) -> bool {
    source_path(root, item)
        .ok()
        .and_then(|p| p.symlink_metadata().ok())
        .is_some_and(|m| m.file_type().is_file() && m.len() == item.bytes && m.len() > 0)
}

fn process_group_signal(pid: u32, signal: i32) {
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

fn tail_text(path: &Path, limit: u64) -> Result<String, YoutubeError> {
    let mut file = File::open(path).map_err(storage)?;
    let len = file.metadata().map_err(storage)?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit)))
        .map_err(storage)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(storage)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_command(
    mut command: Command,
    work: &Path,
    label: &str,
    timeout: Duration,
    cancel: &Cancellation,
    progress: Option<(&UnboundedSender<Progress>, &PrepareJob, u64)>,
) -> Result<Vec<u8>, YoutubeError> {
    cancel.check()?;
    let out_path = work.join("stdout.log");
    let err_path = work.join("stderr.log");
    let stdout = File::create(&out_path).map_err(storage)?;
    let stderr = File::create(&err_path).map_err(storage)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    command.process_group(0);
    let mut child = command.spawn().map_err(storage)?;
    let began = Instant::now();
    let mut last_percent = 0;
    let status = loop {
        if cancel.is_cancelled() || began.elapsed() > timeout {
            process_group_signal(child.id(), libc::SIGTERM);
            for _ in 0..10 {
                if child.try_wait().map_err(storage)?.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            process_group_signal(child.id(), libc::SIGKILL);
            let _ = child.wait();
            return if cancel.is_cancelled() {
                Err(YoutubeError::Cancelled)
            } else {
                Err(YoutubeError::Tool(format!(
                    "{label} の処理期限を超えました"
                )))
            };
        }
        if let Some(status) = child.try_wait().map_err(storage)? {
            break status;
        }
        if let Some((sender, job, expected)) = progress {
            if let Ok(data) = tail_text(&out_path, 4096) {
                if let Some(count) = data.lines().rev().find_map(|line| {
                    line.strip_prefix("frame=")
                        .and_then(|n| n.parse::<u64>().ok())
                }) {
                    let percent = (count.saturating_mul(95) / expected.max(1)).min(95) as u8;
                    if percent != last_percent {
                        send(
                            sender,
                            job,
                            JobStage::Convert,
                            percent,
                            "160×80 JPEGへ変換しています",
                        );
                        last_percent = percent;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(150));
    };
    cancel.check()?;
    let err = tail_text(&err_path, 8192)?;
    if !status.success() {
        return Err(tool_error(label, &err));
    }
    let size = fs::metadata(&out_path).map_err(storage)?.len();
    if size > 8_000_000 {
        return Err(YoutubeError::Tool(format!("{label} の出力が大きすぎます")));
    }
    fs::read(out_path).map_err(storage)
}

fn yt_base(tools: &MediaTools) -> Command {
    let mut c = Command::new(&tools.yt_dlp);
    c.args([
        "--ignore-config",
        "--no-playlist",
        "--no-warnings",
        "--socket-timeout",
        "15",
        "--retries",
        "2",
    ]);
    if let Some(parent) = tools.ffmpeg.parent().filter(|p| !p.as_os_str().is_empty()) {
        c.arg("--ffmpeg-location").arg(parent);
    }
    if let Some(node) = &tools.node {
        c.arg("--js-runtimes")
            .arg(format!("node:{}", node.display()));
    }
    c
}
fn duration_ms(value: &Value) -> Result<u64, YoutubeError> {
    let d = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0);
    if !d.is_finite() || d <= 0.0 || d > u64::MAX as f64 / 1000.0 {
        return Err(YoutubeError::InvalidMetadata);
    }
    Ok((d * 1000.0).round() as u64)
}
fn metadata(
    tools: &MediaTools,
    job: &PrepareJob,
    work: &Path,
    now: i64,
) -> Result<VideoMetadata, YoutubeError> {
    if let Some(previous) = &job.previous {
        return Ok(previous.metadata.clone());
    }
    let mut command = yt_base(tools);
    command
        .args(["--skip-download", "--dump-single-json"])
        .arg(&job.url.canonical);
    let output = run_command(
        command,
        work,
        "yt-dlp",
        Duration::from_secs(180),
        &job.cancel,
        None,
    )?;
    let info: Value = serde_json::from_slice(&output).map_err(|_| YoutubeError::InvalidMetadata)?;
    if info["id"].as_str() != Some(&job.url.id)
        || info["_type"].as_str() == Some("playlist")
        || info["is_live"].as_bool() == Some(true)
    {
        return Err(YoutubeError::InvalidMetadata);
    }
    let title = info["title"]
        .as_str()
        .unwrap_or("タイトル未取得")
        .chars()
        .take(500)
        .collect();
    let uploader: String = info["uploader"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect();
    let channel = info["channel"]
        .as_str()
        .unwrap_or(if uploader.is_empty() {
            "投稿者不明"
        } else {
            &uploader
        })
        .chars()
        .take(300)
        .collect();
    let thumb = info["thumbnail"]
        .as_str()
        .and_then(|url| download_thumbnail(url, tools, &job.url.id).ok().flatten());
    let thumbnail_bytes = if thumb.is_some() {
        fs::metadata(video_dir(&tools.root, &job.url.id)?.join("thumbnail.jpg"))
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };
    Ok(VideoMetadata {
        id: job.url.id.clone(),
        url: job.url.canonical.clone(),
        title,
        channel,
        uploader,
        duration_ms: duration_ms(&info["duration"])?,
        thumbnail: thumb,
        thumbnail_bytes,
        updated_unix_ms: now,
        played_unix_ms: None,
    })
}
fn download_thumbnail(
    url: &str,
    tools: &MediaTools,
    id: &str,
) -> Result<Option<String>, YoutubeError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| YoutubeError::InvalidUrl)?;
    if parsed.scheme() != "https"
        || !matches!(parsed.host_str(), Some("i.ytimg.com" | "img.youtube.com"))
    {
        return Ok(None);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| YoutubeError::Tool(e.to_string()))?;
    let response = client
        .get(parsed)
        .send()
        .map_err(|e| YoutubeError::Tool(e.to_string()))?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1_000_000) {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    response
        .take(1_000_001)
        .read_to_end(&mut bytes)
        .map_err(storage)?;
    if bytes.len() > 1_000_000 {
        return Ok(None);
    }
    let image = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
        .map_err(|_| YoutubeError::InvalidFrame)?;
    let image = image.thumbnail(480, 270);
    let folder = video_dir(&tools.root, id)?;
    fs::create_dir_all(&folder).map_err(storage)?;
    let staged = folder.join("thumbnail.tmp");
    image
        .save_with_format(&staged, ImageFormat::Jpeg)
        .map_err(|_| YoutubeError::InvalidFrame)?;
    fs::rename(staged, folder.join("thumbnail.jpg")).map_err(storage)?;
    // The web layer resolves this public asset ID through its own route.
    Ok(Some(format!("thumbnail-{id}")))
}

#[derive(Debug)]
struct Probe {
    duration_ms: u64,
    width: u64,
    height: u64,
}
fn probe(
    tools: &MediaTools,
    job: &PrepareJob,
    work: &Path,
    path: &Path,
) -> Result<Probe, YoutubeError> {
    let mut command = Command::new(&tools.ffprobe);
    command
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,start_time,duration:format=duration,start_time",
            "-of",
            "json",
        ])
        .arg(path);
    let output = run_command(
        command,
        work,
        "ffprobe",
        Duration::from_secs(180),
        &job.cancel,
        None,
    )?;
    let info: Value = serde_json::from_slice(&output).map_err(|_| YoutubeError::InvalidMetadata)?;
    let stream = info["streams"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or(YoutubeError::InvalidMetadata)?;
    let width = stream["width"]
        .as_u64()
        .ok_or(YoutubeError::InvalidMetadata)?;
    let height = stream["height"]
        .as_u64()
        .ok_or(YoutubeError::InvalidMetadata)?;
    let duration = if stream["duration"].is_null() {
        &info["format"]["duration"]
    } else {
        &stream["duration"]
    };
    Ok(Probe {
        width,
        height,
        duration_ms: duration_ms(duration)?,
    })
}

fn download(
    tools: &MediaTools,
    job: &PrepareJob,
    work: &Path,
    clip: Clip,
    metadata_duration: u64,
    progress: &UnboundedSender<Progress>,
) -> Result<SourceCache, YoutubeError> {
    let full = clip.is_full(metadata_duration);
    if !full {
        let mut command = Command::new(&tools.ffmpeg);
        command.args(["-hide_banner", "-protocols"]);
        let protocols = run_command(
            command,
            work,
            "ffmpeg",
            Duration::from_secs(30),
            &job.cancel,
            None,
        )?;
        if !String::from_utf8_lossy(&protocols)
            .lines()
            .any(|s| s.trim() == "https")
        {
            return Err(YoutubeError::Tool(
                "指定区間の取得に必要なffmpegのHTTPS機能がありません".into(),
            ));
        }
    }
    send(
        progress,
        job,
        JobStage::Download,
        0,
        if full {
            "全編を取得しています"
        } else {
            "指定区間を取得しています"
        },
    );
    let origin = if full {
        0
    } else {
        clip.start_ms.saturating_sub(1000)
    };
    let covered_end = if full {
        metadata_duration
    } else {
        clip.end_ms.saturating_add(1000).min(metadata_duration)
    };
    let mut attempt = 0;
    loop {
        let mut command = yt_base(tools);
        command.args([
            "--check-formats",
            "--fragment-retries",
            "2",
            "--no-progress",
            "--no-part",
            "-f",
        ]);
        command.arg(if attempt == 0 {
            "bv/b"
        } else {
            "bv[protocol^=m3u8]/b[protocol^=m3u8]"
        });
        command
            .args(["-S", "res:240,vcodec:h264", "-o"])
            .arg(work.join("source.%(ext)s"));
        if !full {
            command
                .arg("--download-sections")
                .arg(format!("*{}-{}", format_ms(origin), format_ms(covered_end)))
                .args([
                    "--force-keyframes-at-cuts",
                    "--downloader-args",
                    "ffmpeg_o:-threads 2",
                ]);
        }
        command.arg(&job.url.canonical);
        match run_command(
            command,
            work,
            "yt-dlp",
            Duration::from_secs(7200),
            &job.cancel,
            None,
        ) {
            Ok(_) => break,
            Err(YoutubeError::Tool(msg)) if attempt == 0 && msg.contains("HTTP 403") => {
                attempt += 1;
                send(
                    progress,
                    job,
                    JobStage::Download,
                    0,
                    "取得URLが拒否されたためHLSで1回再試行しています",
                );
                for entry in fs::read_dir(work).map_err(storage)? {
                    let path = entry.map_err(storage)?.path();
                    if path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("source."))
                        && path.is_file()
                    {
                        fs::remove_file(path).map_err(storage)?;
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    let sources: Vec<_> = fs::read_dir(work)
        .map_err(storage)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("source."))
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|s| s != "part" && s != "ytdl")
                && p.is_file()
        })
        .collect();
    if sources.len() != 1 {
        return Err(YoutubeError::Tool(
            "取得した動画ファイルを特定できません".into(),
        ));
    }
    let source_file = &sources[0];
    let extension = source_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_owned();
    if extension.len() > 8 || !extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(YoutubeError::InvalidFrame);
    }
    let measured = probe(tools, job, work, source_file)?;
    if measured.width == 0 || measured.height == 0 {
        return Err(YoutubeError::InvalidMetadata);
    }
    let actual_end = origin.saturating_add(measured.duration_ms);
    let tolerance = if clip.end_ms == metadata_duration {
        1000
    } else {
        100
    };
    if actual_end.saturating_add(tolerance) < clip.end_ms {
        return Err(YoutubeError::Tool(
            "取得区間が指定終了位置まで届いていません".into(),
        ));
    }
    let key = source_key(&job.url.id, origin, covered_end, full);
    let folder = video_dir(&tools.root, &job.url.id)?.join("sources");
    fs::create_dir_all(&folder).map_err(storage)?;
    let target = folder.join(format!("{key}.{extension}"));
    job.cancel.check()?;
    fs::rename(source_file, &target).map_err(storage)?;
    let bytes = fs::metadata(target).map_err(storage)?.len();
    if bytes == 0 {
        return Err(YoutubeError::InvalidFrame);
    }
    Ok(SourceCache {
        key,
        video_id: job.url.id.clone(),
        coverage: Clip {
            start_ms: origin,
            end_ms: if full {
                covered_end
            } else {
                actual_end.min(covered_end)
            },
        },
        full,
        extension,
        bytes,
        origin_ms: origin,
        duration_ms: measured.duration_ms,
    })
}

fn validate_staged_frames(
    stage: PathBuf,
    cancel: Cancellation,
    generation: u64,
    fps: u8,
    expected: u64,
    available_ms: u64,
    progress: UnboundedSender<Progress>,
) -> Result<(u64, u64), YoutubeError> {
    // Linux applies this to the validation worker thread. It exits after the
    // scan, so a pooled worker is never left at a modified scheduling priority.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
    let mut frames = 0u64;
    let mut highest = 0u64;
    let mut bytes = 0u64;
    let mut last_percent = 95u8;
    for entry in fs::read_dir(&stage).map_err(storage)? {
        cancel.check()?;
        let entry = entry.map_err(storage)?;
        let number = frame_number(&entry.file_name()).ok_or(YoutubeError::InvalidFrame)?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(storage)?;
        if !meta.file_type().is_file() || !(4..=MAX_JPEG_BYTES).contains(&meta.len()) {
            return Err(YoutubeError::InvalidFrame);
        }
        let jpeg = fs::read(&path).map_err(storage)?;
        let image = image::load_from_memory_with_format(&jpeg, ImageFormat::Jpeg)
            .map_err(|_| YoutubeError::InvalidFrame)?;
        if image.dimensions() != (WIDTH, HEIGHT) {
            return Err(YoutubeError::InvalidFrame);
        }
        frames = frames.checked_add(1).ok_or(YoutubeError::Overflow)?;
        highest = highest.max(number);
        bytes = bytes
            .checked_add(meta.len())
            .ok_or(YoutubeError::Overflow)?;
        let percent = 95 + (4 * frames / expected.max(1)).min(4) as u8;
        if percent != last_percent {
            let _ = progress.send(Progress {
                generation,
                stage: JobStage::Validate,
                percent,
                message: "変換結果を検証しています".into(),
                metadata: None,
                source: None,
                conversion: None,
            });
            last_percent = percent;
        }
    }
    cancel.check()?;
    let tolerance_ms = 110u64.max(1100 / fps as u64);
    let actual_ms = frames.saturating_mul(1000) / fps as u64;
    if frames == 0
        || frames > 99_999_999
        || highest != frames
        || actual_ms.abs_diff(available_ms) > tolerance_ms
    {
        return Err(YoutubeError::InvalidFrame);
    }
    Ok((frames, bytes))
}

fn convert(
    tools: &MediaTools,
    job: &PrepareJob,
    work: &Path,
    clip: Clip,
    source: &SourceCache,
    progress: &UnboundedSender<Progress>,
    now: i64,
) -> Result<ConversionCache, YoutubeError> {
    send(
        progress,
        job,
        JobStage::Convert,
        0,
        "160×80 JPEGへ変換しています",
    );
    let key = conversion_key(&job.url.id, clip, job.fps);
    let stage = work.join("frames");
    fs::create_dir(&stage).map_err(storage)?;
    let input = source_path(&tools.root, source)?;
    let expected = ((clip.end_ms - clip.start_ms) as u128 * job.fps.value() as u128 / 1000) as u64;
    let mut command = Command::new("nice");
    command
        .arg("-n")
        .arg("10")
        .arg(&tools.ffmpeg)
        .args([
            "-v",
            "error",
            "-nostdin",
            "-threads",
            "2",
            "-filter_threads",
            "1",
            "-ss",
        ])
        .arg(format_ms(clip.start_ms - source.origin_ms))
        .arg("-i")
        .arg(&input)
        .arg("-t")
        .arg(format_ms(clip.end_ms - clip.start_ms))
        .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf"])
        .arg(format!("{FIT},fps={},setpts=PTS-STARTPTS", job.fps.value()))
        .args([
            "-c:v",
            "mjpeg",
            "-q:v",
            "5",
            "-pix_fmt",
            "yuvj420p",
            "-f",
            "image2",
            "-progress",
            "pipe:1",
        ])
        .arg(stage.join("frame%08d.jpg"));
    run_command(
        command,
        work,
        "ffmpeg",
        Duration::from_secs(7200),
        &job.cancel,
        Some((progress, job, expected)),
    )?;
    send(
        progress,
        job,
        JobStage::Validate,
        95,
        "変換結果を検証しています",
    );
    let available_ms = (clip.end_ms - clip.start_ms).min(
        source
            .duration_ms
            .saturating_add(source.origin_ms)
            .saturating_sub(clip.start_ms),
    );
    let stage_for_worker = stage.clone();
    let cancel = job.cancel.clone();
    let generation = job.generation;
    let fps = job.fps.value();
    let progress_for_worker = progress.clone();
    let (frames, bytes) = thread::Builder::new()
        .name("youtube-jpeg-validate".into())
        .spawn(move || {
            validate_staged_frames(
                stage_for_worker,
                cancel,
                generation,
                fps,
                expected,
                available_ms,
                progress_for_worker,
            )
        })
        .map_err(storage)?
        .join()
        .map_err(|_| YoutubeError::Tool("JPEG検証が停止しました".into()))??;
    let conversion = ConversionCache {
        key,
        video_id: job.url.id.clone(),
        clip,
        fps: job.fps,
        frames,
        bytes,
        created_unix_ms: now,
    };
    let folder = video_dir(&tools.root, &job.url.id)?.join("converted");
    fs::create_dir_all(&folder).map_err(storage)?;
    let target = converted_path(&tools.root, &conversion)?;
    job.cancel.check()?;
    if target.exists() {
        let previous = work.join("previous-conversion");
        fs::rename(&target, &previous).map_err(storage)?;
        if let Err(error) = fs::rename(&stage, &target) {
            let _ = fs::rename(previous, target);
            return Err(storage(error));
        }
        fs::remove_dir_all(previous).map_err(storage)?;
    } else {
        fs::rename(&stage, &target).map_err(storage)?;
    }
    Ok(conversion)
}

fn run_prepare_blocking(
    tools: MediaTools,
    job: PrepareJob,
    progress: UnboundedSender<Progress>,
    now: i64,
    cached_only: bool,
) -> Result<PrepareResult, YoutubeError> {
    let root = safe_root(&tools.root)?;
    let work_parent = root.join("work");
    fs::create_dir_all(&work_parent).map_err(storage)?;
    let work = work_parent.join(format!(
        "youtube-{}-{}-{}",
        std::process::id(),
        job.generation,
        now
    ));
    fs::create_dir(&work).map_err(storage)?;
    let result = (|| {
        job.cancel.check()?;
        let metadata = if cached_only {
            let stored = job.previous.as_ref().ok_or(YoutubeError::MissingCache)?;
            if stored.metadata.id != job.url.id
                || VideoUrl::parse(&stored.metadata.url)?.id != job.url.id
                || stored.metadata.duration_ms == 0
            {
                return Err(YoutubeError::InvalidFrame);
            }
            stored.metadata.clone()
        } else {
            metadata(&tools, &job, &work, now)?
        };
        send_metadata(&progress, &job, &metadata);
        let clip = Clip::resolve(job.start_ms, job.end_ms, metadata.duration_ms)?;
        let key = conversion_key(&job.url.id, clip, job.fps);
        if let Some(previous) = &job.previous {
            if let Some(item) = previous.conversions.get(&key) {
                if validate_conversion_cancellable(&root, item, &job.cancel)? {
                    job.cancel.check()?;
                    return Ok(PrepareResult {
                        metadata,
                        source: None,
                        conversion: item.clone(),
                        reused: true,
                    });
                }
            }
        }
        // A worker can publish a valid directory and then be cancelled before
        // its state event is persisted. Recover it by its exact condition key.
        let candidate = ConversionCache {
            key,
            video_id: job.url.id.clone(),
            clip,
            fps: job.fps,
            frames: 0,
            bytes: 0,
            created_unix_ms: now,
        };
        if let Some((frames, bytes)) =
            inspect_manifest(&converted_path(&root, &candidate)?, &job.cancel)?
        {
            let conversion = ConversionCache {
                frames,
                bytes,
                ..candidate
            };
            send_conversion(&progress, &job, &conversion);
            return Ok(PrepareResult {
                metadata,
                source: None,
                conversion,
                reused: true,
            });
        }
        let source = job.previous.as_ref().and_then(|record| {
            record
                .sources
                .values()
                .filter(|s| s.covers(clip, metadata.duration_ms) && valid_source(&root, s))
                .min_by_key(|s| s.bytes)
                .cloned()
        });
        let source = match source {
            Some(source) => source,
            None if cached_only => return Err(YoutubeError::MissingCache),
            None => {
                let source = download(&tools, &job, &work, clip, metadata.duration_ms, &progress)?;
                send_source(&progress, &job, &source);
                source
            }
        };
        let conversion = convert(&tools, &job, &work, clip, &source, &progress, now)?;
        // The directory is complete and public now. Cancellation may suppress
        // playback, but it must not erase knowledge of this valid cache.
        send_conversion(&progress, &job, &conversion);
        Ok(PrepareResult {
            metadata,
            source: Some(source),
            conversion,
            reused: false,
        })
    })();
    let _ = fs::remove_dir_all(work);
    result
}

/// Runs blocking tools on a dedicated worker. The application must release its
/// mutex before awaiting and apply the returned generation only once.
pub async fn run_prepare(
    tools: MediaTools,
    job: PrepareJob,
    progress: UnboundedSender<Progress>,
    now_unix_ms: i64,
) -> Result<PrepareResult, YoutubeError> {
    tokio::task::spawn_blocking(move || {
        run_prepare_blocking(tools, job, progress, now_unix_ms, false)
    })
    .await
    .map_err(|e| YoutubeError::Tool(e.to_string()))?
}

/// Prepare an explicitly selected cached video without fetching metadata or
/// downloading a source. Missing/damaged cache fails instead of accessing the
/// network. FPS changes can still convert a valid saved source.
pub async fn run_cached_prepare(
    tools: MediaTools,
    job: PrepareJob,
    progress: UnboundedSender<Progress>,
    now_unix_ms: i64,
) -> Result<PrepareResult, YoutubeError> {
    tokio::task::spawn_blocking(move || {
        run_prepare_blocking(tools, job, progress, now_unix_ms, true)
    })
    .await
    .map_err(|e| YoutubeError::Storage(e.to_string()))?
}

pub async fn run_delete(root: PathBuf, job: DeleteJob) -> Result<(), YoutubeError> {
    tokio::task::spawn_blocking(move || {
        let root = safe_root(&root)?;
        let folder = video_dir(&root, &job.video_id)?;
        if !folder.exists() {
            return Ok(());
        }
        let trash_parent = root.join("work");
        fs::create_dir_all(&trash_parent).map_err(storage)?;
        let trash = trash_parent.join(format!("delete-{}-{}", job.video_id, std::process::id()));
        fs::rename(&folder, &trash).map_err(storage)?;
        if let Err(error) = fs::remove_dir_all(&trash) {
            let _ = fs::rename(&trash, &folder);
            return Err(storage(error));
        }
        Ok(())
    })
    .await
    .map_err(|e| YoutubeError::Storage(e.to_string()))?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTransform {
    None,
    LegacyPanel,
}
pub fn rgb565_from_jpeg(bytes: &[u8], transform: ColorTransform) -> Result<Vec<u8>, YoutubeError> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
        .map_err(|_| YoutubeError::InvalidFrame)?;
    if image.dimensions() != (WIDTH, HEIGHT) {
        return Err(YoutubeError::InvalidFrame);
    }
    let rgb = image.to_rgb8();
    let mut output = Vec::with_capacity(FRAME_BYTES);
    for pixel in rgb.pixels() {
        let [r, g, b] = pixel.0;
        let mut word = ((r as u16 & 0xf8) << 8) | ((g as u16 & 0xfc) << 3) | (b as u16 >> 3);
        if transform == ColorTransform::LegacyPanel {
            word ^= 0xffff;
            word = ((word & 0x001f) << 11) | (word & 0x07e0) | (word >> 11);
        }
        output.extend_from_slice(&word.to_be_bytes());
    }
    Ok(output)
}

pub async fn read_frame(
    root: PathBuf,
    conversion: ConversionCache,
    index: u64,
    transform: ColorTransform,
) -> Result<Vec<u8>, YoutubeError> {
    tokio::task::spawn_blocking(move || {
        if index >= conversion.frames {
            return Err(YoutubeError::InvalidFrame);
        }
        let path = converted_path(&root, &conversion)?.join(frame_name(index));
        let meta = path.symlink_metadata().map_err(storage)?;
        if !meta.file_type().is_file() || !(4..=MAX_JPEG_BYTES).contains(&meta.len()) {
            return Err(YoutubeError::InvalidFrame);
        }
        let bytes = fs::read(path).map_err(storage)?;
        rgb565_from_jpeg(&bytes, transform)
    })
    .await
    .map_err(|e| YoutubeError::Storage(e.to_string()))?
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub source: u64,
    pub converted: u64,
    pub thumbnail: u64,
    pub other: u64,
    pub total: u64,
    pub free: u64,
}
/// A concurrent cache removal is expected: vanished entries are omitted from
/// this snapshot. Other I/O errors leave the previous published usage intact
/// when callers use `scan_usage_or_last`.
pub async fn scan_usage(root: PathBuf) -> Result<Usage, YoutubeError> {
    tokio::task::spawn_blocking(move || {
        let root = safe_root(&root)?;
        let mut usage = Usage::default();
        let mut stack = vec![root.clone()];
        while let Some(folder) = stack.pop() {
            let entries = match fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(storage(e)),
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(storage(e)),
                };
                let path = entry.path();
                let meta = match fs::symlink_metadata(&path) {
                    Ok(meta) => meta,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(storage(e)),
                };
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.file_type().is_dir() {
                    stack.push(path);
                    continue;
                }
                if !meta.file_type().is_file() {
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .map_err(|_| YoutubeError::InvalidFrame)?;
                let parts: Vec<_> = relative.components().collect();
                let size = meta.len();
                if parts.len() >= 4
                    && parts[0].as_os_str() == "videos"
                    && parts[2].as_os_str() == "converted"
                {
                    usage.converted = usage.converted.saturating_add(size);
                } else if parts.len() >= 4
                    && parts[0].as_os_str() == "videos"
                    && parts[2].as_os_str() == "sources"
                {
                    usage.source = usage.source.saturating_add(size);
                } else if path.file_name().is_some_and(|n| n == "thumbnail.jpg") {
                    usage.thumbnail = usage.thumbnail.saturating_add(size);
                } else {
                    usage.other = usage.other.saturating_add(size);
                }
            }
        }
        usage.total = usage
            .source
            .saturating_add(usage.converted)
            .saturating_add(usage.thumbnail)
            .saturating_add(usage.other);
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_path =
            CString::new(root.as_os_str().as_bytes()).map_err(|_| YoutubeError::InvalidFrame)?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return Err(storage(std::io::Error::last_os_error()));
        }
        let stat = unsafe { stat.assume_init() };
        usage.free = stat.f_bavail.saturating_mul(stat.f_frsize);
        Ok(usage)
    })
    .await
    .map_err(|e| YoutubeError::Storage(e.to_string()))?
}

/// Publish a new measurement only on success. A transient scan error never
/// replaces the last known capacity with zero or a partial result.
pub async fn scan_usage_or_last(root: PathBuf, previous: Usage) -> Usage {
    scan_usage(root).await.unwrap_or(previous)
}
