//! Bounded local-only decode and sample IO. No application lock is held.
use super::*;
use futures_util::future::BoxFuture;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub struct LocalMusicExecutor;
fn fail(message: &str) -> JobFailure {
    JobFailure::new("music_failed", message)
}
struct CancelOnDrop(crate::youtube::Cancellation);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel()
    }
}
struct WorkDir(PathBuf);
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn directory(p: &Path) -> Result<(), JobFailure> {
    fs::create_dir_all(p).map_err(|_| fail("保存先を作成できません"))?;
    if !fs::symlink_metadata(p)
        .map_err(|_| fail("保存先を確認できません"))?
        .file_type()
        .is_dir()
    {
        return Err(fail("保存先が不正です"));
    }
    Ok(())
}
fn source(p: &Path, max: u64) -> Result<File, JobFailure> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p)
        .map_err(|_| fail("音源が見つからないか読めません"))?;
    let m = f.metadata().map_err(|_| fail("音源を確認できません"))?;
    if !m.is_file() || m.len() == 0 || m.len() > max {
        return Err(fail("音源サイズまたは種類が不正です"));
    }
    Ok(f)
}
fn pcm_path(paths: &PluginPaths, key: &str) -> Result<PathBuf, JobFailure> {
    if !good_key(key) {
        return Err(fail("音源IDが不正です"));
    }
    let d = paths.cache.join("audio");
    directory(&d)?;
    Ok(d.join(format!("{key}.pcm")))
}
impl PluginJobExecutor for LocalMusicExecutor {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome> {
        Box::pin(async move {
            let work: Work = match serde_json::from_value(payload) {
                Ok(w) => w,
                Err(_) => return JobOutcome::Failed(fail("要求が不正です")),
            };
            let token = crate::youtube::Cancellation::default();
            let _guard = CancelOnDrop(token.clone());
            let mut cancel = context.cancel;
            if *cancel.borrow() {
                return JobOutcome::Cancelled;
            }
            let worker_token = token.clone();
            let mut worker =
                tokio::task::spawn_blocking(move || run(work, context.paths, &worker_token));
            let result = tokio::select! {r=&mut worker=>r,_=async{while !*cancel.borrow(){if cancel.changed().await.is_err(){break}}}=>{token.cancel();worker.await}};
            if token.is_cancelled() {
                return JobOutcome::Cancelled;
            }
            match result {
                Ok(Ok(v)) => JobOutcome::Succeeded(v),
                Ok(Err(e)) => JobOutcome::Failed(e),
                Err(_) => JobOutcome::Failed(fail("音声処理が終了しました")),
            }
        })
    }
    fn read_audio(
        &self,
        spec: AudioPlaybackSpec,
        offset: u64,
        max: u32,
        paths: PluginPaths,
    ) -> BoxFuture<'static, Result<Vec<u8>, JobFailure>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                if !spec.valid() || max == 0 || max > 4800 {
                    return Err(fail("音声読取り範囲が不正です"));
                }
                let t: Track = serde_json::from_value(spec.source.clone())
                    .map_err(|_| fail("音源情報が不正です"))?;
                if !t.valid() || t.key != spec.asset_id || spec.frame_count != Some(t.frames) {
                    return Err(fail("音源情報が一致しません"));
                }
                let mut f = source(&pcm_path(&paths, &t.key)?, MAX_FRAMES * 4)?;
                if f.metadata()
                    .map_err(|_| fail("音源検証に失敗しました"))?
                    .len()
                    != t.frames * 4
                {
                    return Err(fail("音源が破損しています"));
                }
                if offset >= t.frames {
                    return Ok(vec![]);
                }
                let count = (t.frames - offset).min(max as u64);
                let mut bytes = vec![0; (count * 4) as usize];
                f.seek(SeekFrom::Start(offset * 4))
                    .map_err(|_| fail("音源読取りに失敗しました"))?;
                f.read_exact(&mut bytes)
                    .map_err(|_| fail("音源読取りに失敗しました"))?;
                Ok(bytes)
            })
            .await
            .map_err(|_| fail("音声読取りが終了しました"))?
        })
    }
}
fn run(
    work: Work,
    paths: PluginPaths,
    cancel: &crate::youtube::Cancellation,
) -> Result<Value, JobFailure> {
    let imports = paths.data.join("imports");
    directory(&imports)?;
    match work {
        Work::Scan => {
            let mut names = vec![];
            for entry in fs::read_dir(&imports).map_err(|_| fail("一覧を取得できません"))?
            {
                let e = entry.map_err(|_| fail("一覧を取得できません"))?;
                let n = e.file_name().to_string_lossy().into_owned();
                if good_name(&n)
                    && fs::symlink_metadata(e.path())
                        .is_ok_and(|m| m.is_file() && m.len() > 0 && m.len() <= MAX_SOURCE)
                {
                    names.push(n)
                }
            }
            names.sort();
            Ok(json!(names))
        }
        Work::Delete {
            name,
            key,
            keep_cache,
        } => {
            if !good_name(&name) {
                return Err(fail("音源名が不正です"));
            }
            let p = imports.join(&name);
            if p.exists() {
                let _ = source(&p, MAX_SOURCE)?;
                fs::remove_file(p).map_err(|_| fail("削除できません"))?;
            }
            if let Some(key) = key {
                if !keep_cache {
                    let p = pcm_path(&paths, &key)?;
                    if p.exists() {
                        let _ = source(&p, MAX_FRAMES * 4)?;
                        fs::remove_file(p).map_err(|_| fail("変換済み音源を削除できません"))?;
                    }
                }
            }
            Ok(Value::Null)
        }
        Work::Prepare { name, generation } => {
            if !good_name(&name) {
                return Err(fail("音源名が不正です"));
            }
            let mut input = source(&imports.join(&name), MAX_SOURCE)?;
            directory(&paths.cache)?;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| fail("時刻が不正です"))?
                .as_nanos();
            let work = paths.cache.join(format!(
                "music-work-{}-{generation}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&work).map_err(|_| fail("作業先を作れません"))?;
            let _cleanup = WorkDir(work.clone());
            let mut copy =
                File::create(work.join("input")).map_err(|_| fail("音源コピーに失敗しました"))?;
            let mut hash = Sha256::new();
            hash.update(b"local-music-pcm48-v1");
            hash.update(name.as_bytes());
            let mut buf = [0u8; 65536];
            let mut total = 0u64;
            loop {
                if cancel.is_cancelled() {
                    return Err(fail("取り消しました"));
                }
                let n = input.read(&mut buf).map_err(|_| fail("音源が読めません"))?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > MAX_SOURCE {
                    return Err(fail("音源が大きすぎます"));
                }
                copy.write_all(&buf[..n])
                    .map_err(|_| fail("音源コピーに失敗しました"))?;
                hash.update(&buf[..n]);
            }
            drop(copy);
            let key = format!("{:x}", hash.finalize());
            let target = pcm_path(&paths, &key)?;
            if target.exists() {
                let f = source(&target, MAX_FRAMES * 4)?;
                let bytes = f
                    .metadata()
                    .map_err(|_| fail("音源を確認できません"))?
                    .len();
                if bytes % 4 != 0 {
                    return Err(fail("音源が破損しています"));
                }
                return Ok(json!(Track {
                    key,
                    name,
                    frames: bytes / 4
                }));
            }
            let output = work.join("output.pcm");
            let child = Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-format_whitelist",
                    "mp3,wav,flac,ogg,mov,aac",
                    "-i",
                ])
                .arg(work.join("input"))
                .args([
                    "-map",
                    "0:a:0",
                    "-vn",
                    "-sn",
                    "-dn",
                    "-ac",
                    "2",
                    "-ar",
                    "48000",
                    "-c:a",
                    "pcm_s16le",
                    "-f",
                    "s16le",
                    "-t",
                    "7201",
                    "-fs",
                    &(MAX_FRAMES * 4 + 4096).to_string(),
                ])
                .arg(&output)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| fail("ffmpegを起動できません"))?;
            let mut child = ChildGuard(child);
            let started = Instant::now();
            loop {
                if cancel.is_cancelled() || started.elapsed() > Duration::from_secs(850) {
                    return Err(fail("変換を取り消したか時間切れです"));
                }
                if fs::metadata(&output).is_ok_and(|m| m.len() > MAX_FRAMES * 4) {
                    return Err(fail("音源は2時間以内にしてください"));
                }
                if let Some(status) = child
                    .0
                    .try_wait()
                    .map_err(|_| fail("変換状態を取得できません"))?
                {
                    if !status.success() {
                        return Err(fail("音源を変換できません。対応形式を確認してください"));
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let f = source(&output, MAX_FRAMES * 4)?;
            let bytes = f
                .metadata()
                .map_err(|_| fail("音源を検証できません"))?
                .len();
            if bytes % 4 != 0 {
                return Err(fail("音源が破損しています"));
            }
            f.sync_all().map_err(|_| fail("音源を保存できません"))?;
            if cancel.is_cancelled() {
                return Err(fail("取り消しました"));
            }
            match fs::hard_link(&output, &target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if source(&target, MAX_FRAMES * 4)?
                        .metadata()
                        .map_err(|_| fail("音源を検証できません"))?
                        .len()
                        != bytes
                    {
                        return Err(fail("保存済み音源が一致しません"));
                    }
                }
                Err(_) => return Err(fail("音源を保存できません")),
            };
            Ok(json!(Track {
                key,
                name,
                frames: bytes / 4
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pcm_reads_are_bounded_and_validate_length() {
        let d = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: d.path().join("data"),
            cache: d.path().join("cache"),
        };
        let key = "a".repeat(64);
        let t = Track {
            key: key.clone(),
            name: "x.wav".into(),
            frames: 100,
        };
        fs::write(pcm_path(&paths, &key).unwrap(), vec![3u8; 400]).unwrap();
        let spec = AudioPlaybackSpec {
            asset_id: key.clone(),
            generation: 1,
            sample_rate: 48000,
            channels: 2,
            frame_count: Some(100),
            start_frame: 0,
            started_mono_ms: 0,
            source: json!(t),
        };
        assert_eq!(
            LocalMusicExecutor
                .read_audio(spec.clone(), 95, 10, paths.clone())
                .await
                .unwrap(),
            vec![3; 20]
        );
        assert!(LocalMusicExecutor
            .read_audio(spec.clone(), 100, 10, paths.clone())
            .await
            .unwrap()
            .is_empty());
        assert!(LocalMusicExecutor
            .read_audio(spec.clone(), 0, 4801, paths.clone())
            .await
            .is_err());
        fs::write(pcm_path(&paths, &key).unwrap(), b"short").unwrap();
        assert!(LocalMusicExecutor
            .read_audio(spec, 0, 10, paths)
            .await
            .is_err());
    }
    #[test]
    fn scan_rejects_symlinks_and_cancel_cleans_work() {
        let d = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: d.path().join("data"),
            cache: d.path().join("cache"),
        };
        let imports = paths.data.join("imports");
        directory(&imports).unwrap();
        fs::write(imports.join("real.wav"), b"invalid audio").unwrap();
        std::os::unix::fs::symlink(imports.join("real.wav"), imports.join("link.wav")).unwrap();
        let token = crate::youtube::Cancellation::default();
        assert_eq!(
            run(Work::Scan, paths.clone(), &token).unwrap(),
            json!(["real.wav"])
        );
        token.cancel();
        assert!(run(
            Work::Prepare {
                name: "real.wav".into(),
                generation: 1
            },
            paths.clone(),
            &token
        )
        .is_err());
        assert_eq!(fs::read_dir(&paths.cache).unwrap().count(), 0);
    }
    #[test]
    fn wav_decodes_to_stereo48k_and_reuses_cache() {
        if Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .status()
            .is_err()
        {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: d.path().join("data"),
            cache: d.path().join("cache"),
        };
        let imports = paths.data.join("imports");
        directory(&imports).unwrap();
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(36u32.wrapping_add(1600).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(1600u32.to_le_bytes());
        wav.extend(vec![0; 1600]);
        fs::write(imports.join("tone.wav"), wav).unwrap();
        let token = crate::youtube::Cancellation::default();
        let a = run(
            Work::Prepare {
                name: "tone.wav".into(),
                generation: 1,
            },
            paths.clone(),
            &token,
        )
        .unwrap();
        assert_eq!(a["frames"], 4800);
        assert_eq!(
            a,
            run(
                Work::Prepare {
                    name: "tone.wav".into(),
                    generation: 2
                },
                paths.clone(),
                &token
            )
            .unwrap()
        );
        assert_eq!(fs::read_dir(&paths.cache).unwrap().count(), 1);
    }
}
