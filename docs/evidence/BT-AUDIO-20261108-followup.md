# BT-AUDIO-20261107 / MUSIC-PLAYER-20261107 follow-up evidence

## Applied and observed

The earlier pipeline LCD build b85e8d985312b12c3c648894075b7a33b28a50ff91b8b11a87ba097968ac2b34 and server 5ee400c740f00f5205d9c83ba1112a621c2045066fbed3b79debfac103790059 remain the last applied artifacts. A three-second tone completed (144000 frames, failed=0) and the user confirmed hearing it clearly. A following uploaded-music run sent 1331 packets / 1533312 frames before intentional pause at about32 seconds. These successes are limited trials.

Subsequent user playback failed in a later connection generation: PCM and negotiated SBC both48000Hz, full9-frame packets, about2944 frames still buffered, but can-send wait increased from about0.9ms/packet in the successful tone to18.5ms/packet. A250ms lateness guard stopped the stream. Connection role remains a hypothesis, not a confirmed cause.

The user reported unexpectedly loud music at night. The synthetic quiet tone did not validate real-track loudness. Playback was paused. No further automated sound, hardware update or service deployment is authorized in this follow-up without explicit permission.

## Implemented, not applied

- Per-device digital volume0–100, initially0/muted for old and new settings; squared gain so10 means1% amplitude,100 is unity. Original assets are unchanged. Existing buffered audio and speaker-owned announcements are not immediately controlled by PCM mute.
- Live volume updates preserve radio commands, connection generation and audio-stream identity. Invalid values and stale sessions fail closed.
- Physical music Sub input now follows session-checked PluginInput and a same-device PluginDataChanged candidate. Normal Up/Down retain channel selection. End-of-track replay needs one press.
- An incoming local-master request matches the official SDK source demo, with actual role/mode diagnostics. Candidate LCD hash a053c4cf83f2f716711608b8fbad32affa0f106fd9765a1311776626ded88186. No audible effectiveness claim.
- LBS volume-button behavior is not established. Current AVRCP supports playback/navigation operations, not volume or absolute-volume synchronization.

## Host checks

Volume/domain/application4 tests, PCM5 standalone tests, actual transport sink mute/live-volume test1, and application music input integration7 tests passed. Firmware host suite, pipeline structural regression, LCD/E-ink cross-builds, UI JavaScript syntax and generated observation drift check passed.

The first volume all-features library run had373 passes,1 timeout failure and1 ignored. The same compiled failing loopback test passed when isolated; no assertion/timeout was relaxed. The final combined all-features serial run passed: 374 library, 2 server, 7 local-music input integration and 4 official-JMA tests, total 387 passed / 0 failed / 2 intentionally ignored. Log: `/tmp/picomado-final-volume-music-all.log`. The previously failing loopback test passed both isolated and in this final full run, without relaxed assertions or timeouts. Independent review, actual volume listening, AVRCP hardware behavior and role-policy improvement remain unverified.

## Source snapshot (before final combined regression)

- `service/src/audio_transport.rs`: `6ee6da983a1dd048c519be34a0c98fe81e5e93d0e0ead2c2ae92ff3729c628c4`
- `service/src/transport_audio.rs`: `0c8ecbf84eb5eacd73dda25774aeda21c78de35146474abe09a03a2d6fe35ae4`
- `service/src/bluetooth.rs`: `0e5975e8ee086f0d3a916e2a20acd059d235478c208ef81f92503ad88d67daa3`
- `service/src/application_bluetooth.rs`: `aa5390b7e3bd1f59dcf6a1db53ba6146e60ef22bf37dbfa19599aa1f515f825f`
- `service/src/local_music.rs`: `e2a24cfda9b376230de4cf118f3e916338376583b6fc8b6af395218ec716edfc`
- `service/tests/local_music_input.rs`: `a987d012c5aa06fad1304666f194aaf1504c263190bf21df9ba8e848bc09bd46`
- `firmware/src/bluetooth_runtime.c`: `c329a79637a2bf4961194d4bfe23340f232deeb4bac4936ed49e08365912804c`
- `firmware/src/bluetooth_media.inc`: `4c29b7f74e12fea3c01d4462496f542b6f0ccd8008a7f3e8661bb59a16899744`

## Deployment / rollback boundary

The old DesiredBluetooth uses `deny_unknown_fields`. Once the new binary persists `volume` (including default0), the old binary cannot deserialize StoredState and startup fails. Stored schema version remains2, so this is not a supported binary-only downgrade. Before any authorized application, save a fresh consistent SQLite backup using its backup API (or a stopped-service snapshot) together with the current binary. Preserve later user changes when planning a later rollback; do not blindly overwrite a newer database. The old binary also lacks attenuation, so rollback does not authorize audible playback. No new-format volume state has been written to production; the earlier authorized playback/pause actions used the currently deployed server.
