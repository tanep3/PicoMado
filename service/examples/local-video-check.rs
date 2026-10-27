//! Scoped operator check: reuse the user-approved JPEG folder, without network
//! access or modifying the legacy project. Outputs cache plus an import snapshot;
//! it neither opens a listener nor changes the live application's database.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use picomado_core::youtube::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: local-video-check INPUT_JPEG_DIR CACHE_ROOT SNAPSHOT_OUTPUT".into());
    }
    let input = Path::new(&args[1]);
    let root = PathBuf::from(&args[2]);
    let snapshot = PathBuf::from(&args[3]);
    let count = std::fs::read_dir(input)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jpg"))
        .count() as u64;
    if count != 588 {
        return Err("approved fixture must contain exactly 588 JPEG frames".into());
    }
    let id = "voltesVtest"; // Local verification ID, never sent to a downloader.
    let duration_ms = count * 1000 / 6;
    let clip = Clip::resolve(None, None, duration_ms)?;
    let key = source_key(id, 0, duration_ms, true);
    let folder = root.join("videos").join(id);
    let source_dir = folder.join("sources");
    std::fs::create_dir_all(&source_dir)?;
    let source_path = source_dir.join(format!("{key}.mp4"));
    if !source_path.exists() {
        let status = Command::new("nice")
            .args([
                "-n",
                "10",
                "ffmpeg",
                "-v",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-framerate",
                "6",
                "-i",
            ])
            .arg(input.join("frame%04d.jpg"))
            .args(["-an", "-c:v", "mpeg4", "-q:v", "2", "-threads", "1", "-n"])
            .arg(&source_path)
            .status()?;
        if !status.success() {
            return Err("local JPEG source conversion failed".into());
        }
    }
    std::fs::copy(input.join("frame0001.jpg"), folder.join("thumbnail.jpg"))?;
    let source = SourceCache {
        key: key.clone(),
        video_id: id.into(),
        coverage: clip,
        full: true,
        extension: "mp4".into(),
        bytes: source_path.metadata()?.len(),
        origin_ms: 0,
        duration_ms,
    };
    let metadata = VideoMetadata {
        id: id.into(),
        url: format!("https://youtu.be/{id}"),
        title: "Local verification: voltesV_6frame".into(),
        channel: "Local JPEG fixture".into(),
        uploader: "Local JPEG fixture".into(),
        duration_ms,
        thumbnail: None,
        thumbnail_bytes: folder.join("thumbnail.jpg").metadata()?.len(),
        updated_unix_ms: 0,
        played_unix_ms: None,
    };
    let mut content = YoutubeContent::default();
    content.videos.insert(
        id.into(),
        VideoRecord {
            metadata,
            sources: BTreeMap::from([(key, source)]),
            conversions: BTreeMap::new(),
        },
    );
    let mut tools = MediaTools::new(root.clone());
    tools.yt_dlp = root.join("NO_NETWORK_DOWNLOADER");
    let mut six_key = String::new();
    for fps in [6, 12] {
        let job = content.request_prepare(
            PrepareRequest {
                url: format!("https://youtu.be/{id}"),
                fps: Some(fps),
                start: None,
                end: None,
            },
            0,
        )?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let generation = job.generation;
        let ready = run_cached_prepare(tools.clone(), job, tx, 0).await?;
        while let Ok(update) = rx.try_recv() {
            content.progress(update);
        }
        if !validate_conversion(&root, &ready.conversion)? {
            return Err("published local conversion validation failed".into());
        }
        let pixels = read_frame(
            root.clone(),
            ready.conversion.clone(),
            0,
            ColorTransform::None,
        )
        .await?;
        if pixels.len() != 160 * 80 * 2 {
            return Err("invalid RGB565 frame length".into());
        }
        println!(
            "local_conversion fps={} frames={} bytes={} reused={} key={} rgb565_crc32={:08x}",
            fps,
            ready.conversion.frames,
            ready.conversion.bytes,
            ready.reused,
            ready.conversion.key,
            crc32fast::hash(&pixels)
        );
        if fps == 6 {
            six_key = ready.conversion.key.clone();
        }
        if !content.complete_prepare(generation, Ok(ready), 0, 0) {
            return Err("local conversion state result rejected".into());
        }
    }
    let mut stored = content.save_state();
    stored.selected_key = Some(six_key);
    let usage = scan_usage(root).await?;
    let value = serde_json::json!({"stored":stored,"job":null,"deleting":[],"usage":usage});
    std::fs::write(snapshot, serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}
