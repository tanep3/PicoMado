# LOCAL-VIDEO-20261105

Status: implemented on the repository's main working tree; automated verification complete. Independent review and operational application pending.

The user requested a generic non-YouTube local video plugin, beginning with documentation. The existing YouTube path already converts finite video into JPEG frames and feeds the common PlaybackSpec/`run_playback` delivery path. The local source is the changed boundary; device input, transport and playback admission remain common.

Requirements: [local video requirements](../local-video-requirements.md). Design and state table: [local video design](../local-video-design.md).

The Web upload accepts a selected local file, streams at most 512 MiB to a private temporary leaf, refuses an existing filename and publishes only after completion. The plugin scans imports, can remove an uploaded original independently of completed conversions, owns its durable history and playback choice, and calls the existing YouTube finite ffmpeg/JPEG validator for conversion. The common `PlaybackSpec` dispatcher and RGB565 decoder deliver frames. Existing YouTube URLs and its five FPS choices remain unchanged.

Verification on raspi5: the Library reference `sentinel2_japan_2fps_wide_long_clean.mp4` (230 s, 2 fps, 1280×720) converted to 460 frames; the first and last RGB565 frames were each 25,600 bytes. Synthetic MP4 conversion, restart validation, symlink refusal, upload size and collision checks, independent original removal, and the five-plugin order migration passed. Default Rust library suite: 286 passed, 1 ignored; final all-feature suite: 320 passed, 1 ignored. The ignored reference test passed when run explicitly. UI video tests: 2 passed; `git diff --check`, observation generator drift and standalone dependency boundary checker passed.

The repository-wide `project_gate.py check` did not complete: its structural suite's unmodified `test_actual_firmware_build_sdk_flags_and_offline_flash` expected `flash.sh --verify-only` to refuse after a fixture approval was removed, but observed exit 0. This is outside this plugin change and must be resolved before a complete gate receipt can be claimed. No device or service was operated, and no user cache was swept.

Independent verification, operational application and effectiveness confirmation remain pending.
