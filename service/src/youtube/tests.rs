use super::*;

fn sample_meta(id: &str) -> VideoMetadata {
    VideoMetadata {
        id: id.into(),
        url: format!("https://www.youtube.com/watch?v={id}"),
        title: "sample".into(),
        channel: "author".into(),
        uploader: "author".into(),
        duration_ms: 2000,
        thumbnail: None,
        thumbnail_bytes: 0,
        updated_unix_ms: 1,
        played_unix_ms: None,
    }
}
fn sample_result(id: &str, fps: Fps) -> PrepareResult {
    let clip = Clip::resolve(None, None, 2000).unwrap();
    PrepareResult {
        metadata: sample_meta(id),
        source: None,
        conversion: ConversionCache {
            key: conversion_key(id, clip, fps),
            video_id: id.into(),
            clip,
            fps,
            frames: 12,
            bytes: 100,
            created_unix_ms: 1,
        },
        reused: false,
    }
}
fn request(id: &str) -> PrepareRequest {
    PrepareRequest {
        url: format!("https://youtu.be/{id}"),
        fps: None,
        start: None,
        end: None,
    }
}

#[test]
fn urls_times_and_cache_conditions() {
    let id = "abcdefghijk";
    for value in [
        format!("https://youtu.be/{id}"),
        format!("https://www.youtube.com/watch?v={id}"),
        format!("https://m.youtube.com/shorts/{id}"),
        format!("https://youtube.com/embed/{id}"),
    ] {
        assert_eq!(VideoUrl::parse(&value).unwrap().id, id);
    }
    for value in [
        "https://evil.youtube.com/watch?v=abcdefghijk",
        "https://youtu.be/abcdefghijk.evil",
        "https://youtube.com:443/watch?v=abcdefghijk",
        "file:///watch?v=abcdefghijk",
        "https://youtube.com/watch?v=abcdefghijk&v=abcdefghijk",
    ] {
        assert!(VideoUrl::parse(value).is_err(), "{value}");
    }
    assert_eq!(parse_time(Some("01:02.345")).unwrap(), Some(62345));
    assert!(parse_time(Some("01:60")).is_err());
    assert!(Clip::resolve(Some(1000), Some(1000), 2000).is_err());
    assert!(Clip::resolve(Some(0), Some(2001), 2000).is_err());
    let whole = Clip::resolve(None, None, 2000).unwrap();
    let segment = Clip::resolve(Some(100), Some(2000), 2000).unwrap();
    assert_ne!(
        conversion_key(id, whole, Fps::new(6).unwrap()),
        conversion_key(id, segment, Fps::new(6).unwrap())
    );
    assert_ne!(
        conversion_key(id, whole, Fps::new(6).unwrap()),
        conversion_key(id, whole, Fps::new(12).unwrap())
    );
    assert!(serde_json::from_str::<Fps>("7").is_err());
}

#[test]
fn generation_cancel_failure_and_shared_clock() {
    let id = "abcdefghijk";
    let mut content = YoutubeContent::default();
    let first = content.request_prepare(request(id), 10).unwrap();
    assert!(content.request_prepare(request(id), 11).is_err());
    assert!(content.complete_prepare(first.generation, Ok(sample_result(id, first.fps)), 1000, 20));
    assert_eq!(content.frame_due(1000).unwrap().index, 0);
    assert_eq!(content.frame_due(1500).unwrap().index, 3);
    assert_eq!(content.frame_due(3500).unwrap().index, 3);
    let previous = content.playback.as_ref().unwrap().conversion.key.clone();
    let mut second_input = request(id);
    second_input.fps = Some(12);
    let second = content.request_prepare(second_input, 30).unwrap();
    assert!(content.cancel());
    assert!(second.cancel.is_cancelled());
    assert!(content.complete_prepare(
        second.generation,
        Ok(sample_result(id, second.fps)),
        4000,
        40
    ));
    assert_eq!(content.playback.as_ref().unwrap().conversion.key, previous);
    assert_eq!(content.job.as_ref().unwrap().status, JobStatus::Cancelled);
    let cached_after_cancel =
        conversion_key(id, Clip::resolve(None, None, 2000).unwrap(), second.fps);
    assert!(content.videos[id]
        .conversions
        .contains_key(&cached_after_cancel));
    let third = content.request_prepare(request(id), 50).unwrap();
    assert!(!content.complete_prepare(
        second.generation,
        Ok(sample_result(id, second.fps)),
        5000,
        60
    ));
    assert!(content.complete_prepare(
        third.generation,
        Err(YoutubeError::InvalidMetadata),
        5000,
        60
    ));
    assert_eq!(content.playback.as_ref().unwrap().conversion.key, previous);
    assert_eq!(content.job.as_ref().unwrap().status, JobStatus::Failed);
    content.stop();
    assert!(content.frame_due(6000).is_none());
}

#[test]
fn completed_cache_progress_survives_cancelled_outcome() {
    let id = "abcdefghijk";
    let mut content = YoutubeContent::default();
    let initial = content.request_prepare(request(id), 1).unwrap();
    content.complete_prepare(
        initial.generation,
        Ok(sample_result(id, initial.fps)),
        1000,
        2,
    );
    let playing = content.playback.as_ref().unwrap().conversion.key.clone();
    let mut input = request(id);
    input.fps = Some(18);
    let job = content.request_prepare(input, 3).unwrap();
    assert!(content.cancel());
    let ready = sample_result(id, job.fps);
    let key = ready.conversion.key.clone();
    assert!(content.progress(Progress {
        generation: job.generation,
        stage: JobStage::Ready,
        percent: 99,
        message: "published".into(),
        metadata: Some(ready.metadata),
        source: None,
        conversion: Some(ready.conversion)
    }));
    assert!(content.complete_prepare(job.generation, Err(YoutubeError::Cancelled), 2000, 4));
    assert!(content.videos[id].conversions.contains_key(&key));
    assert_eq!(content.playback.as_ref().unwrap().conversion.key, playing);
}

#[test]
fn recipients_share_clock_but_queue_independently() {
    let id = "abcdefghijk";
    let mut content = YoutubeContent::default();
    let job = content.request_prepare(request(id), 1).unwrap();
    content.complete_prepare(job.generation, Ok(sample_result(id, job.fps)), 1000, 2);
    let fast = Viewer {
        device_id: "lcd",
        selected_youtube: true,
        online: true,
        max_fps: 30,
    };
    let slow = Viewer {
        device_id: "eink",
        selected_youtube: true,
        online: true,
        max_fps: 1,
    };
    let inactive = Viewer {
        device_id: "weather",
        selected_youtube: false,
        online: true,
        max_fps: 30,
    };
    assert!(content.plan_delivery(inactive, 1000).is_none());
    let first_fast = content.plan_delivery(fast.clone(), 1000).unwrap();
    let first_slow = content.plan_delivery(slow.clone(), 1000).unwrap();
    assert_eq!(first_fast.frame.index, first_slow.frame.index);
    assert!(content.mark_enqueued(&first_fast, 1000));
    assert!(content.mark_enqueued(&first_slow, 1000));
    assert!(content.plan_delivery(slow.clone(), 1500).is_none());
    assert_eq!(
        content
            .plan_delivery(fast.clone(), 1500)
            .unwrap()
            .frame
            .index,
        3
    );
    assert_eq!(
        content
            .plan_delivery(slow.clone(), 2100)
            .unwrap()
            .frame
            .index,
        6
    );
    content.forget_viewer("eink");
    assert_eq!(content.plan_delivery(slow, 1500).unwrap().frame.index, 3);
}

#[test]
fn common_playback_spec_uses_the_plugin_clock() {
    let id = "abcdefghijk";
    let mut content = YoutubeContent::default();
    let job = content.request_prepare(request(id), 1).unwrap();
    content.complete_prepare(job.generation, Ok(sample_result(id, job.fps)), 42_000, 2);
    let spec = content.playback_spec().unwrap();
    assert!(spec.valid());
    assert_eq!(spec.started_mono_ms, 42_000);
    assert_eq!(spec.generation, content.frame_due(42_000).unwrap().epoch);
    let at = 42_500;
    let common_index = ((at - spec.started_mono_ms) * spec.fps as u64 / 1000) % spec.frame_count;
    assert_eq!(common_index, content.frame_due(at).unwrap().index);
    assert_eq!(
        YoutubeContent::conversion_from_spec(&spec).unwrap().key,
        content.playback.as_ref().unwrap().conversion.key
    );
    let mut changed = spec.clone();
    changed.frame_count += 1;
    assert!(YoutubeContent::conversion_from_spec(&changed).is_err());
    let restarted = YoutubeContent::restore(content.save_state(), 90_000);
    assert_eq!(restarted.playback_spec().unwrap().started_mono_ms, 90_000);
}

#[test]
fn deletion_guards_and_restart_selection() {
    let id = "abcdefghijk";
    let mut content = YoutubeContent::default();
    let job = content.request_prepare(request(id), 10).unwrap();
    assert!(content.complete_prepare(job.generation, Ok(sample_result(id, job.fps)), 1000, 20));
    assert!(matches!(
        content.request_delete(id, "wrong"),
        Err(YoutubeError::ConfirmationRequired)
    ));
    assert!(matches!(
        content.request_delete(id, id),
        Err(YoutubeError::Busy)
    ));
    let snapshot = content.save_state();
    let restored = YoutubeContent::restore(snapshot, 5000);
    assert_eq!(restored.frame_due(5000).unwrap().index, 0);
    assert_eq!(restored.frame_due(6000).unwrap().index, 6);
    content.stop();
    let deleting = content.request_delete(id, id).unwrap();
    assert!(matches!(
        content.request_delete(id, id),
        Err(YoutubeError::Busy)
    ));
    content.complete_delete(deleting, Ok(())).unwrap();
    assert!(content.history().is_empty());
}

#[test]
fn source_reuse_coverage_and_eof_rounding() {
    let id = "abcdefghijk";
    let full = SourceCache {
        key: source_key(id, 0, 2000, true),
        video_id: id.into(),
        coverage: Clip {
            start_ms: 0,
            end_ms: 2000,
        },
        full: true,
        extension: "mp4".into(),
        bytes: 100,
        origin_ms: 0,
        duration_ms: 2000,
    };
    assert!(full.covers(
        Clip {
            start_ms: 500,
            end_ms: 1500
        },
        2000
    ));
    let partial = SourceCache {
        full: false,
        key: source_key(id, 0, 2000, false),
        coverage: Clip {
            start_ms: 0,
            end_ms: 1900,
        },
        ..full
    };
    assert!(partial.covers(
        Clip {
            start_ms: 0,
            end_ms: 2000
        },
        2000
    ));
    assert!(!partial.covers(
        Clip {
            start_ms: 0,
            end_ms: 1950
        },
        2000
    ));
}

#[test]
fn rgb565_panel_transform() {
    use std::io::Cursor;
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        WIDTH,
        HEIGHT,
        image::Rgb([255, 0, 0]),
    ));
    let mut jpeg = Cursor::new(Vec::new());
    image.write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
    let plain = rgb565_from_jpeg(jpeg.get_ref(), ColorTransform::None).unwrap();
    let legacy = rgb565_from_jpeg(jpeg.get_ref(), ColorTransform::LegacyPanel).unwrap();
    assert_eq!(plain.len(), FRAME_BYTES);
    assert_eq!(u16::from_be_bytes([plain[0], plain[1]]), 0xf800);
    assert_eq!(u16::from_be_bytes([legacy[0], legacy[1]]), 0xffe0);
}

#[test]
fn manifest_scan_is_order_independent_and_cancellable() {
    let temp = tempfile::tempdir().unwrap();
    let id = "abcdefghijk";
    let clip = Clip::resolve(None, None, 2000).unwrap();
    let item = ConversionCache {
        key: conversion_key(id, clip, Fps::new(6).unwrap()),
        video_id: id.into(),
        clip,
        fps: Fps::new(6).unwrap(),
        frames: 2000,
        bytes: 8000,
        created_unix_ms: 1,
    };
    let folder = temp
        .path()
        .join("videos")
        .join(id)
        .join("converted")
        .join(&item.key);
    std::fs::create_dir_all(&folder).unwrap();
    for index in (0..item.frames).rev() {
        std::fs::write(folder.join(format!("frame{:08}.jpg", index + 1)), b"1234").unwrap();
    }
    assert!(validate_conversion(temp.path(), &item).unwrap());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        validate_conversion_cancellable(temp.path(), &item, &cancel),
        Err(YoutubeError::Cancelled)
    );
    std::fs::write(folder.join("unexpected"), b"1234").unwrap();
    assert!(!validate_conversion(temp.path(), &item).unwrap());
}

#[tokio::test]
async fn usage_scan_tolerates_concurrent_removal_and_retains_last_good_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let target = root.join("videos/abcdefghijk/converted/cache");
    std::fs::create_dir_all(&target).unwrap();
    for index in 0..500 {
        std::fs::write(target.join(format!("{index:04}")), b"1234").unwrap();
    }
    let last = scan_usage(root.clone()).await.unwrap();
    assert_eq!(last.converted, 2000);
    let delete = target.parent().unwrap().parent().unwrap().to_path_buf();
    let remover = std::thread::spawn(move || std::fs::remove_dir_all(delete).unwrap());
    let during = scan_usage(root.clone()).await.unwrap();
    remover.join().unwrap();
    assert!(during.converted <= 2000);
    let invalid_root = temp.path().join("regular-file");
    std::fs::write(&invalid_root, b"x").unwrap();
    assert_eq!(scan_usage_or_last(invalid_root, last).await, last);
}

#[tokio::test]
async fn offline_media_conversion_reuses_source_and_publishes_complete_frames() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    for (tool, version_arg) in [
        ("ffmpeg", "-version"),
        ("ffprobe", "-version"),
        ("nice", "--version"),
    ] {
        let output = Command::new(tool)
            .arg(version_arg)
            .output()
            .expect("required local test tool");
        assert!(output.status.success(), "{tool} is required for this test");
    }
    let temp = tempfile::tempdir().unwrap();
    let fixture = temp.path().join("fixture.mp4");
    let output = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=320x240:r=12:d=2",
            "-an",
            "-c:v",
            "mpeg4",
            "-y",
        ])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stub = temp.path().join("mock-yt-dlp");
    let script = format!("#!/usr/bin/env python3\nimport json, shutil, sys\na=sys.argv[1:]\nif '--dump-single-json' in a:\n print(json.dumps({{'id':'abcdefghijk','duration':2,'title':'Local fixture','channel':'Test author'}}))\nelse:\n shutil.copyfile({:?}, a[a.index('-o')+1].replace('%(ext)s','mp4'))\n", fixture.to_str().unwrap());
    std::fs::write(&stub, script).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut tools = MediaTools::new(temp.path().join("cache"));
    tools.yt_dlp = stub.clone();
    let mut content = YoutubeContent::default();
    let first = content.request_prepare(request("abcdefghijk"), 10).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ready = run_prepare(tools.clone(), first.clone(), tx, 10)
        .await
        .unwrap();
    assert_eq!(ready.conversion.frames, 12);
    assert!(validate_conversion(&tools.root, &ready.conversion).unwrap());
    assert!(rx.try_recv().is_ok());
    let frame = read_frame(
        tools.root.clone(),
        ready.conversion.clone(),
        0,
        ColorTransform::None,
    )
    .await
    .unwrap();
    assert_eq!(frame.len(), FRAME_BYTES);
    let side = u16::from_be_bytes(frame[(40 * 160 * 2)..][..2].try_into().unwrap());
    let middle = u16::from_be_bytes(frame[(40 * 160 * 2 + 80 * 2)..][..2].try_into().unwrap());
    assert_eq!(side, 0);
    assert_ne!(middle, 0);
    assert!(content.complete_prepare(first.generation, Ok(ready), 1000, 20));
    std::fs::write(&stub, "#!/bin/sh\nexit 7\n").unwrap();
    let second = content.request_fps(12, 30).unwrap();
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut cached_tools = tools.clone();
    cached_tools.yt_dlp = temp.path().join("missing-downloader");
    let ready = run_cached_prepare(cached_tools.clone(), second.clone(), tx, 30)
        .await
        .unwrap();
    assert_eq!(ready.conversion.frames, 24);
    assert!(content.complete_prepare(second.generation, Ok(ready), 2000, 40));
    let third = content.request_fps(12, 50).unwrap();
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    cached_tools.ffmpeg = temp.path().join("missing-converter");
    let ready = run_cached_prepare(cached_tools, third.clone(), tx, 50)
        .await
        .unwrap();
    assert!(ready.reused);
    assert!(content.complete_prepare(third.generation, Ok(ready), 3000, 55));
    let fourth = content.request_fps(18, 60).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let worker = tokio::spawn(run_prepare(tools.clone(), fourth.clone(), tx, 60));
    let published = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(update) = rx.recv().await {
            if update.conversion.is_some() {
                return true;
            }
        }
        false
    })
    .await
    .unwrap();
    assert!(published);
    assert!(content.cancel());
    let completed_after_cancel = worker.await.unwrap().unwrap();
    let published_key = completed_after_cancel.conversion.key.clone();
    assert!(content.complete_prepare(fourth.generation, Ok(completed_after_cancel), 4000, 65));
    assert_eq!(content.job.as_ref().unwrap().status, JobStatus::Cancelled);
    assert!(content.videos["abcdefghijk"]
        .conversions
        .contains_key(&published_key));
    assert_eq!(
        content.playback.as_ref().unwrap().conversion.fps.value(),
        12
    );
    // Simulate a lost registration event: the complete directory is still
    // discoverable by its condition key, with no downloader or ffmpeg needed.
    content
        .videos
        .get_mut("abcdefghijk")
        .unwrap()
        .conversions
        .remove(&published_key);
    let fifth = content.request_fps(18, 70).unwrap();
    let mut disabled_tools = tools.clone();
    disabled_tools.ffmpeg = temp.path().join("missing-ffmpeg");
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let recovered = run_prepare(disabled_tools, fifth, tx, 70).await.unwrap();
    assert!(recovered.reused);
    assert_eq!(recovered.conversion.key, published_key);
    let usage = scan_usage(tools.root.clone()).await.unwrap();
    assert!(
        usage.source > 0 && usage.converted > 0 && usage.total >= usage.source + usage.converted
    );
    let snapshot = content.save_state();
    let restored = YoutubeContent::restore_checked(snapshot.clone(), tools.root.clone(), 5000)
        .await
        .unwrap();
    assert_eq!(restored.frame_due(5000).unwrap().index, 0);
    let selected = restored.playback.unwrap().conversion;
    std::fs::remove_file(
        tools
            .root
            .join("videos")
            .join(&selected.video_id)
            .join("converted")
            .join(&selected.key)
            .join("frame00000001.jpg"),
    )
    .unwrap();
    let damaged = YoutubeContent::restore_checked(snapshot, tools.root, 6000)
        .await
        .unwrap();
    assert!(damaged.playback.is_none());
}

#[tokio::test]
async fn cancellation_terminates_local_child_and_preserves_playback() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let stub = temp.path().join("slow-yt-dlp");
    std::fs::write(&stub, "#!/bin/sh\nsleep 20\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut content = YoutubeContent::default();
    let id = "abcdefghijk";
    let initial = content.request_prepare(request(id), 1).unwrap();
    content.complete_prepare(
        initial.generation,
        Ok(sample_result(id, initial.fps)),
        1000,
        2,
    );
    let old_key = content.playback.as_ref().unwrap().conversion.key.clone();
    let job = content.request_prepare(request("lmnopqrstuv"), 3).unwrap();
    let mut tools = MediaTools::new(temp.path().join("cache"));
    tools.yt_dlp = stub;
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let started = std::time::Instant::now();
    let worker = tokio::spawn(run_prepare(tools, job.clone(), tx, 3));
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert!(content.cancel());
    let result = worker.await.unwrap();
    assert!(matches!(&result, Err(YoutubeError::Cancelled)));
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    content.complete_prepare(job.generation, result, 2000, 4);
    assert_eq!(content.playback.as_ref().unwrap().conversion.key, old_key);
}

#[tokio::test]
async fn deletion_effect_is_scoped_to_confirmed_video() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let target = root.join("videos/abcdefghijk");
    let sibling = root.join("videos/lmnopqrstuv");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(target.join("dummy"), b"target").unwrap();
    std::fs::write(sibling.join("dummy"), b"keep").unwrap();
    let mut content = YoutubeContent::default();
    let job = content.request_prepare(request("abcdefghijk"), 1).unwrap();
    content.complete_prepare(
        job.generation,
        Ok(sample_result("abcdefghijk", job.fps)),
        1000,
        2,
    );
    content.stop();
    let delete = content
        .request_delete("abcdefghijk", "abcdefghijk")
        .unwrap();
    let effect = run_delete(root, delete.clone()).await;
    content.complete_delete(delete, effect).unwrap();
    assert!(!target.exists());
    assert_eq!(std::fs::read(sibling.join("dummy")).unwrap(), b"keep");
}

#[tokio::test]
async fn metadata_history_survives_failed_download() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let stub = temp.path().join("metadata-only-yt-dlp");
    std::fs::write(&stub, "#!/bin/sh\nfor arg in \"$@\"; do\n if [ \"$arg\" = '--dump-single-json' ]; then\n  printf '%s\\n' '{\"id\":\"abcdefghijk\",\"duration\":2,\"title\":\"Known title\",\"channel\":\"Known author\"}'\n  exit 0\n fi\ndone\nexit 7\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut tools = MediaTools::new(temp.path().join("cache"));
    tools.yt_dlp = stub;
    let mut content = YoutubeContent::default();
    let job = content.request_prepare(request("abcdefghijk"), 1).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let result = run_prepare(tools, job.clone(), tx, 2).await;
    assert!(result.is_err());
    while let Ok(update) = rx.try_recv() {
        content.progress(update);
    }
    content.complete_prepare(job.generation, result, 1000, 3);
    assert_eq!(content.job.as_ref().unwrap().status, JobStatus::Failed);
    assert_eq!(content.history()[0].metadata.title, "Known title");
}
