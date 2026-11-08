# MUSIC-PLAYER-20261107: terminal delivery feedback

Specification was committed before implementation as `1518703` in the existing
MUSIC-PLAYER-20261107 change. This remains an open change. Independent acceptance,
server application and physical audio effectiveness belong to the parent.

## Cause and correction

The transport held terminal audio failure in its own transfer but never returned
it to the owning content plugin. Consequently local-music's playback intent and
clock-derived Web position kept advancing after transmission stopped. This is
separate from the cause of device underflow or radio/transport failure.

The common ContentPlugin callback defaults to no action. App validates the exact
current session/incarnation/display, selected plugin and full AudioPlaybackSpec
before adopting the plugin's cloned plan through existing accept_work_plan.
There is no concrete plugin-ID branch in this new common path. The playback start
time is fixed, so elapsed time does not invalidate feedback. Volume/mute do not
change the audio offer. Local music owns stopping, its generation, saved consumed
position, per-device public error and rendering. Failure does not restart audio,
change Bluetooth settings, mutate source files or submit jobs. Socket write/send
IO errors and connection loss still use the existing writer/session teardown;
this correction does not add a dedicated public error or consumed-position
feedback for those paths. Its scope is exactly the five reason codes described below.

The socket owner validates terminal accepted/consumed counters through the same
AudioSender rules as ordinary credit. Invalid/reversed counters retain the last
valid consumed position. AudioSender returns Ok(changed): equal or reversed credit
returns Ok(false), so duplicate reports do not refresh last_credit or extend the
existing three-second no-progress deadline. The first terminal result freezes
that position. App
receives failure before a potentially unsuccessful AudioStop write. Device
failure, invalid credit, credit deadline, source error and source deadline have
bounded public reason codes; raw IO errors are not exposed. Duplicate/stale
feedback cannot overwrite a later playback intent. Padding is clamped to track
length by local music; consumed PCM is not proof of audible speaker output.

## Host verification

- `cargo test --manifest-path service/Cargo.toml --no-default-features --test local_music_input -j 1`: 16 passed. Includes failure at consumed 6912 frames = 144 ms, position fixed across time, other-device state preserved, duplicate/new generation/new track/new session/display mismatch rejection, explicit resume from consumed position and EOF-padding clamp.
- `cargo test --manifest-path service/Cargo.toml --no-default-features --lib audio -j 1`: 13 passed. Includes production service_audio with a fake sink, terminal status after mute/volume changes, source PCM unchanged, exactly one stop and no new start/PCM after failure, explicit resume, existing credit timeout and missing temporary PCM failure. Pure tests cover invalid/reversed credit, stale stream/session/display/incarnation and failure interrupting a pending source read.
- `node service/tests/local_music_ui_test.js`: passed. Executes the real renderer with a minimal DOM facade, checks failure label/reason, 0.144-second position, unaffected second device and filename text safety. This is not a browser visual acceptance test.
- Extracted `service/src/ui.html` script passed `node --check`.
- `git diff --check`: passed at the reviewed implementation stage.
- `python3 tools/project_gate.py check --out /tmp/picomado-music-failure-gate-20261108`: rejected at structural tests, 48 passed / 1 failed. Failure is the previously documented `test_gate_routes.py:46` expectation that absent approval rejects flash check-only; actual return is zero. No firmware tool or approval behavior was changed by this service correction. Log: `/tmp/picomado-music-failure-gate-20261108/structural.log`.

Final verification used the frozen 166-file service/identity input set, digest
`293331abda3cc4526858897212c956cbf0001fee33f07aa90aa9189f1bdc0478`.
After another worker briefly edited/restored protocol inputs during the first
full run, the crate was forced to reread the final source; the first run is not
used as acceptance evidence. The fixed-input rerun used:

`PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px cargo test --manifest-path service/Cargo.toml --locked --all-features -j 1 -- --test-threads=1`

It passed **409 tests / 0 failed / 2 intentionally ignored** (387 library, 2
server, 16 local-music integration, 4 official JMA fixtures). Compile: 3m20s;
library run: 348.07s. Full run 2026-11-08 02:51:25–03:00:42 UTC. Log:
`/tmp/picomado-music-failure-final-all-features.log`. Input hashes before/after
were identical; Node renderer/syntax and whitespace checks also passed again.

The separately committed f64dd8c SBC files are included in the input inventory
but not compiled: no Cargo/build.rs/lib module wiring or Cargo-auto-detected Rust
integration test was added there. The runtime of this candidate remains PCM.

No config or database schema changed. Existing saved selection fields retain
the same serialization; new failure is runtime-only with serde(skip). The old
state loader and restart-without-autoplay tests pass, as do dedicated server
config tests. No production DB/config was opened or changed to verify this.

No SSH, USB, firmware flash, service restart, production playback or push was
performed. The separate release candidate is built after fixing the commit;
its staging manifest binds the exact commit, input hashes, features and artifact.
