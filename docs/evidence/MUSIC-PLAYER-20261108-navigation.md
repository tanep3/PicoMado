# MUSIC-PLAYER-20261107: navigation autoplay host evidence

2026-11-08. Specification commits `dc8bf45` and `4be913a` precede implementation. Work is in the authoritative `main` checkout, with the existing author configuration unchanged. No push, SSH, service installation/restart, firmware operation, production database/source/PCM change, or physical playback was performed.

## Implementation

- `ContentPlugin::navigation_changed` is a pure default-no-op hook. `App::terminal_event` invokes it only for actual Normal/selected edges caused by accepted terminal navigation. Hello, reconnect, settings, ACKs, renderer refresh and scheduling cannot start playback.
- Navigation, changed plugin snapshots and managed Web revisions are one persistence transaction. Plugin clones publish only after it succeeds. No concrete music ID check was added to the application.
- Local music owns persisted `RestingState` (Stopped/Paused/Failed), volatile playback and saved known EOF. Missing legacy reasons migrate conservatively to Paused; positions are not used to guess user intent. Play/stop explicitly establish new behavior. No stored source/track schema is rewritten beyond additive selection fields.
- Stopped selections resume on entry; paused, failed, missing selection and other incarnation do not. Leaving active music freezes validated consumed position (legacy PCM keeps its previous time-based position model), invalidates the old generation, and prevents streaming outside the selected screen. Known EOF restarts from zero only after a new entry or explicit play. Existing AVRCP toggle semantics are unchanged.
- Web/LCD/E-ink distinguish explicit pause from stop. A saved Failed state still displays failure after restart even if its transient detailed reason is gone. Deletion stops playback and inhibits navigation autoplay while its job is pending. No volume/mute/BT setting changes.

## Verification

Initial focused local-music run: 25 passed, 0 failed before the final EOF/deletion follow-up. `/tmp/picomado-music-nav-unit.log`.

Final frozen library binary `service/target/debug/deps/picomado_core-3482d2578402c7e5`, SHA256 `1213f31774b55d59a87e605fa7a981509e3173c50468c9564eafbe011f62ab22`, passed:

- `local_music`: 26 passed (including streaming decode/seek and 5 new pure navigation tests). `/tmp/picomado-music-nav-unit-final.log`.
- `music_navigation_tests`: 3 passed. Real App persistence failure on entry/exit, frozen consumed position/stale progress/EOF, restart and same-channel no-op. `/tmp/picomado-music-nav-app-final.log`.
- `media_input_contract_tests`: 3 passed; `sbc_sender_sink_tests`: 9 passed; `audio_barrier_sink_tests`: 1 passed. Existing wire controls, pause/new generation, source/credit/volume and start ordering are retained. `/tmp/picomado-music-nav-{media_input_contract_tests,sbc_sender_sink_tests,audio_barrier_sink_tests}.log`.

`cargo test --manifest-path service/Cargo.toml --test local_music_input -- --test-threads=1`: final run 19 passed, 0 failed. `/tmp/picomado-music-nav-input-final2.log`. This includes multi-device isolation, stop vs pause, AVRCP resume, muted/zero-volume preservation, failures, no selection, standby/Hello boundary and legacy EOF. Initial test-only mistakes were corrected: BluetoothAction's import module, a fixture without the required media-input capability, and an assumption that Power in Standby wakes the device. The existing Standby Power no-op and subsequent Hello were retained; the corrected test checks that Hello alone never starts playback. Failed attempts are retained in `/tmp/picomado-music-nav-input{,-final}.log` and are not counted as passing evidence.

UI: `node --test service/tests/local_music_ui_test.js`, 9 passed, 0 failed. Includes pause vs stop, retained failure after restart, repeated rendering without commands, and selecting a different Web track without starting until the explicit Play action. `/tmp/picomado-music-nav-ui.log`.

`node --check` of the complete embedded script, `git diff --check`, and `python3 tools/project_gate.py static` passed. Static log: `/tmp/picomado-music-nav-static.log`.

The full all-features Cargo regression, independent review/acceptance, operational deployment and physical effect confirmation remain owned by the parent; focused checks do not establish those stages.

## 2026-11-08 production application / E-ink_1 navigation trial

Server source `6ee9602` (includes navigation implementation `0318b5e`) applied after full Cargo regression 440 PASS / 0 FAIL / 3 existing ignored. Binary SHA256 `1da0433b3ad3237a622f62ad1398c807af4894aa343d3f3719acf246dfd8e196`. Deployment retained source files, 12 registered tracks, and device settings. Legacy states migrated conservatively to Paused; no playback on Hello/restart.

With user-announced permission and HOME on E-ink_1, volume temporarily 30:

1. Explicit STOP produced Stopped at position 0. USB UP left music for local-image; DOWN returned. Actual accepted navigation started playback without Web PLAY.
2. After eight further seconds, API showed playing, position 10,800 ms; firmware emitted 464 SBC packets / 534,528 consumed PCM frames (11.136 seconds including command/stop tail). No audio failure; rebuffer duration zero (one terminal boundary counter increment at stop). Frame 2495716 rendered.
3. Explicit PAUSE produced Paused at 10,800 ms. UP to local-image and DOWN to music preserved Paused, position and no-play state. Still paused after four further seconds; frame 2495891 rendered.
4. Finally paused and restored prior volume **38**; mute unchanged. Bluetooth stayed connected and device online. No AWAY traffic/test initiated.

Private evidence: `picomado-staging/20261108-remembered-autoplay-6ee9602/navigation-live/{before.json,results.json,usb.log}`. This verifies accepted device-input and packet/consumption behavior, not independent subjective sound quality or physical AVRCP button reception. Independent verification/closure remains with the parent/user.
