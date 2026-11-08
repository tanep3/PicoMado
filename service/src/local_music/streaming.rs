//! One transfer owns one decoder; a cancelled read retains its partial bytes.
use super::*;
use std::os::fd::AsRawFd;

pub(super) struct Decoder {
    child: ChildGuard,
    _work: WorkDir,
    output: std::process::ChildStdout,
    offset: u64,
    pending: Vec<u8>,
    replay: Option<(u64, Vec<u8>)>,
    eof: bool,
    failed: bool,
}
impl Decoder {
    pub(super) fn open(
        spec: AudioPlaybackSpec,
        paths: PluginPaths,
        cancel: &crate::youtube::Cancellation,
    ) -> Result<Self, JobFailure> {
        let track = validated_track(&spec)?;
        let (work, key) = private_input(&paths, &track.name, spec.generation, cancel)?;
        if key != track.key {
            return Err(fail(
                "原本が登録時から変更されています。一覧を更新してください",
            ));
        }
        let mut child = ChildGuard(
            Command::new("ffmpeg")
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
                .arg(work.0.join("input"))
                .args([
                    "-map",
                    "0:a:0",
                    "-vn",
                    "-sn",
                    "-dn",
                    "-af",
                    &format!("aresample=48000,atrim=start_sample={}", spec.start_frame),
                    "-ac",
                    "2",
                    "-ar",
                    "48000",
                    "-c:a",
                    "pcm_s16le",
                    "-f",
                    "s16le",
                    "pipe:1",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| fail("ffmpegを起動できません"))?,
        );
        let output = child
            .0
            .stdout
            .take()
            .ok_or_else(|| fail("復号出力を開けません"))?;
        // Only this decoder owns the descriptor. O_NONBLOCK makes cancellation safe.
        let fd = output.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(fail("復号出力を設定できません"));
        }
        Ok(Self {
            child,
            _work: work,
            output,
            offset: spec.start_frame,
            pending: Vec::new(),
            replay: None,
            eof: false,
            failed: false,
        })
    }
}
impl AudioSourceReader for Decoder {
    fn read(&mut self, offset: u64, max: u32) -> BoxFuture<'_, Result<Vec<u8>, JobFailure>> {
        Box::pin(async move {
            if self.failed || max == 0 || max > 4800 {
                return Err(fail("復号読取り範囲が不正です"));
            }
            if let Some((previous, bytes)) = &self.replay {
                if *previous == offset && bytes.len() <= max as usize * 4 {
                    return Ok(bytes.clone());
                }
            }
            if offset != self.offset {
                return Err(fail("復号位置が一致しません"));
            }
            let target = (max as usize * 4).min(MAX_FRAMES.saturating_sub(offset) as usize * 4);
            // Do not truncate an unexpectedly long source into successful EOF.
            if target == 0 && !self.eof {
                let mut extra = [0u8; 4];
                loop {
                    match self.output.read(&mut extra) {
                        Ok(0) => break,
                        Ok(_) => {
                            self.failed = true;
                            return Err(fail("音源は2時間以内にしてください"));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            tokio::time::sleep(Duration::from_millis(2)).await
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => {
                            self.failed = true;
                            return Err(fail("音声の復号に失敗しました"));
                        }
                    }
                }
                self.eof = true;
            }
            while !self.eof && self.pending.len() < target {
                let mut buffer = [0u8; 19200];
                let want = (target - self.pending.len()).min(buffer.len());
                match self.output.read(&mut buffer[..want]) {
                    Ok(0) => self.eof = true,
                    Ok(n) => self.pending.extend_from_slice(&buffer[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(Duration::from_millis(2)).await
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        self.failed = true;
                        return Err(fail("音声の復号に失敗しました"));
                    }
                }
            }
            if self.eof {
                loop {
                    match self
                        .child
                        .0
                        .try_wait()
                        .map_err(|_| fail("復号状態を確認できません"))?
                    {
                        Some(status) => {
                            if !status.success() || self.pending.len() % 4 != 0 {
                                self.failed = true;
                                return Err(fail("音声の復号が正常終了しませんでした"));
                            }
                            break;
                        }
                        None => tokio::time::sleep(Duration::from_millis(2)).await,
                    }
                }
            }
            let bytes: Vec<_> = self
                .pending
                .drain(..self.pending.len().min(target))
                .collect();
            self.offset += bytes.len() as u64 / 4;
            self.replay = Some((offset, bytes.clone()));
            Ok(bytes)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PluginPaths) {
        let temp = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: temp.path().join("data"),
            cache: temp.path().join("cache"),
        };
        directory(&paths.data.join("imports")).unwrap();
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(1636u32.to_le_bytes());
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
        for n in 0..800i16 {
            wav.extend((n.wrapping_mul(29)).to_le_bytes());
        }
        fs::write(paths.data.join("imports/tone.wav"), wav).unwrap();
        (temp, paths)
    }
    fn spec(paths: &PluginPaths, name: &str, start: u64) -> AudioPlaybackSpec {
        let track = run(
            Work::Prepare {
                name: name.into(),
                generation: 1,
            },
            paths.clone(),
            &Default::default(),
        )
        .unwrap();
        AudioPlaybackSpec {
            asset_id: track["key"].as_str().unwrap().into(),
            generation: 1,
            sample_rate: 48000,
            channels: 2,
            frame_count: None,
            start_frame: start,
            started_mono_ms: 0,
            source: track,
        }
    }
    async fn collect(reader: &mut dyn AudioSourceReader, start: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::time::timeout(
                Duration::from_secs(2),
                reader.read(start + bytes.len() as u64 / 4, 137),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(chunk.len() <= 137 * 4);
            assert_eq!(chunk.len() % 4, 0);
            if chunk.is_empty() {
                break;
            }
            bytes.extend(chunk);
        }
        bytes
    }
    #[tokio::test]
    async fn original_formats_stream_seek_and_leave_no_pcm() {
        let (_temp, paths) = fixture();
        for extension in ["wav", "mp3", "flac", "ogg", "m4a", "aac"] {
            let name = format!("tone.{extension}");
            if extension != "wav" {
                assert!(Command::new("ffmpeg")
                    .args(["-nostdin", "-v", "error", "-i"])
                    .arg(paths.data.join("imports/tone.wav"))
                    .arg(paths.data.join("imports").join(&name))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success());
            }
            let s = spec(&paths, &name, 0);
            let mut a = LocalMusicExecutor
                .open_audio(s.clone(), paths.clone())
                .await
                .unwrap()
                .unwrap();
            let first = a.read(0, 137).await.unwrap();
            assert_eq!(a.read(0, 137).await.unwrap(), first, "same offset replay");
            assert!(
                a.read(3, 137).await.is_err(),
                "nonsequential seek requires new owner"
            );
            let tail = collect(a.as_mut(), 137).await;
            let mut all = first;
            all.extend(tail);
            assert!(!all.is_empty());
            let mut b = LocalMusicExecutor
                .open_audio(
                    AudioPlaybackSpec {
                        start_frame: 123,
                        ..s
                    },
                    paths.clone(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                collect(b.as_mut(), 123).await,
                &all[123 * 4..],
                "canonical sample seek {extension}"
            );
            drop(a);
            drop(b);
            assert_eq!(
                fs::read_dir(&paths.cache).unwrap().count(),
                0,
                "private compressed copies cleaned, no PCM cache"
            );
        }
    }
    #[tokio::test]
    async fn independent_decoders_drop_reap_and_keep_originals() {
        let (_temp, paths) = fixture();
        let s = spec(&paths, "tone.wav", 0);
        let mut a = Decoder::open(s.clone(), paths.clone(), &Default::default()).unwrap();
        let mut b = Decoder::open(s, paths.clone(), &Default::default()).unwrap();
        let (pid_a, pid_b) = (a.child.0.id(), b.child.0.id());
        assert_ne!(pid_a, pid_b);
        let expected = a.read(0, 100).await.unwrap();
        assert_eq!(b.read(0, 100).await.unwrap(), expected);
        drop(a);
        assert_eq!(unsafe { libc::kill(pid_a as i32, 0) }, -1);
        assert_eq!(b.read(100, 100).await.unwrap().len(), 400);
        drop(b);
        assert_eq!(unsafe { libc::kill(pid_b as i32, 0) }, -1);
        assert!(paths.data.join("imports/tone.wav").is_file());
        assert_eq!(fs::read_dir(&paths.cache).unwrap().count(), 0);
    }
    fn synthetic(script: &str) -> (tempfile::TempDir, Decoder) {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("private");
        fs::create_dir(&work).unwrap();
        let mut child = ChildGuard(
            Command::new("python3")
                .args(["-c", script])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let output = child.0.stdout.take().unwrap();
        let fd = output.as_raw_fd();
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) },
            0
        );
        (
            temp,
            Decoder {
                child,
                _work: WorkDir(work),
                output,
                offset: 0,
                pending: Vec::new(),
                replay: None,
                eof: false,
                failed: false,
            },
        )
    }
    #[tokio::test]
    async fn cancelled_partial_read_preserves_bytes_and_deadline_drop_reaps() {
        let (_temp, mut reader) =
            synthetic("import os,time; os.write(1,b'ab'); time.sleep(.15); os.write(1,b'cdefgh')");
        assert!(
            tokio::time::timeout(Duration::from_millis(80), reader.read(0, 2))
                .await
                .is_err()
        );
        assert_eq!(reader.read(0, 2).await.unwrap(), b"abcdefgh");
        assert!(reader.read(2, 2).await.unwrap().is_empty());
        let (_temp, mut reader) = synthetic("import time; time.sleep(30)");
        let pid = reader.child.0.id();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), reader.read(0, 2))
                .await
                .is_err()
        );
        drop(reader);
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
    #[tokio::test]
    async fn abnormal_exit_partial_frame_and_overlong_are_errors_not_silence() {
        for script in [
            "import os; os.write(1,b'abcd'); os._exit(7)",
            "import os; os.write(1,b'abc')",
        ] {
            let (_temp, mut reader) = synthetic(script);
            assert!(reader.read(0, 2).await.is_err());
            assert!(reader.read(0, 2).await.is_err());
        }
        let (_temp, mut reader) = synthetic("import os; os.write(1,b'abcd')");
        reader.offset = MAX_FRAMES;
        assert!(reader.read(MAX_FRAMES, 1).await.is_err());
    }
    #[tokio::test]
    async fn changed_original_and_symlink_fail_without_leaking_work() {
        let (_temp, paths) = fixture();
        let s = spec(&paths, "tone.wav", 0);
        fs::write(paths.data.join("imports/tone.wav"), b"changed").unwrap();
        assert!(LocalMusicExecutor
            .open_audio(s.clone(), paths.clone())
            .await
            .is_err());
        fs::remove_file(paths.data.join("imports/tone.wav")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", paths.data.join("imports/tone.wav")).unwrap();
        assert!(LocalMusicExecutor
            .open_audio(s, paths.clone())
            .await
            .is_err());
        assert_eq!(fs::read_dir(&paths.cache).unwrap().count(), 0);
    }
}
