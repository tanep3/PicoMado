# MUSIC-PLAYER-20261107: streaming source candidate

Specification commits: `3985647`, `7ad5768`, `b924181`, `7ced3a2`, `8b21668`.
Contract: [owned streaming source decode](../design/local-music-streaming.md).

## Implemented candidate

- Automatic bounded ffprobe metadata registration on Scan/upload. Original compressed files are decoded per transfer, without a new full-track PCM cache. Existing PCM-only tracks and old files are retained.
- One cancel-safe reader/ffmpeg process per socket transfer; private compressed input copy and SHA validation; bounded pipe/read/replay; explicit normal exit versus decoder error; Drop kills/waits only the owned child and removes its private work.
- Prime the first 256 canonical sample frames before sending Start. Initial absolute 10-second deadline survives control interruption; Start then begins existing Ready/credit/device prefill deadlines. Subsequent reads retain the existing100ms limit.
- Real EOF, at most127 padding frames, unchanged volume-before-SBC and encoder history. Finite legacy PCM preserves its previous length validation.
- Streaming position follows validated consumed credit, including credit arriving during a source read. EOF replay restarts at zero; same-session reconnect preserves consumed position without autoplay. Metadata duration never rewinds a streaming failure position; known EOF padding is excluded.
- The music player remains above the library. Preparation/conversion buttons are removed. Per-device playback and confirmed deletion remain.

## Review corrections

A separate read-only review found EOF button replay/EOF-pause-play could restart at the old end, and that the old failure-position clamp still used estimated metadata duration. Both were corrected and regression cases added. Review also identified delayed credit publication during reads and loss of consumed position on session reset. Current-session publication was added at credit admission and session reset now retains observed position. Duplicate consumed positions are not republished, and legacy PCM does not incur this new update path.

Natural EOF does not introduce a new physical display-refresh effect. Web state and the next normal render reflect completion. This is separate from audible playback confirmation.

## Validation record

Initial, pre-review snapshot: all-features library416 passed,1 failed,1 ignored in391.68s (`/tmp/picomado-stream-lib.log`). The failure was `host::tests::loopback_host_runs_initial_and_repeated_fetch_then_stops`, `Runner(Conflict("plugin runner shutdown timed out"))` at host.rs247. The UI worker separately observed the same test fail with Elapsed on the earlier0c35bd9 candidate under high IO load. These are different failure details in the same existing test, not proof of one cause. This initial run is not acceptance of the later corrections.

Current JavaScript: music7/7 passed. Combined UI43 tests:37 passed,6 failed, all six in the pre-existing video FPS fixture (`renderEinkVideoControls is not defined`), `/tmp/picomado-stream-node-all.log`. No unrelated fixture changes were made. Static domain/boundary gate and git diff whitespace check passed.

The next intermediate all-features run passed the previously flaky host loopback and all integration tests: library418 passed,1 failed,1 ignored; other targets22 passed,1 ignored. Its sole failure was the newly added zero-length EOF test driver, which continued polling after the transfer correctly completed and was removed, then unwrapped None. The driver was corrected to inspect at Finish. This was not an audio decoder failure. `/tmp/picomado-stream-final-all.log` and `/tmp/picomado-stream-eof-repro.log` retain evidence.

Scan/delete review then added cancellation both at request and completion, same-name metadata removal for an intervening Scan, and compatible prepare cancellation. This prevents both completion orders from recreating a deleted track, including a request with no prior key or a changed same-name key. A concurrent different-track upload can require an explicit list refresh after deletion; its original and existing tracks are retained. Regression cases cover these boundaries.

The post-Scan-fix intermediate run passed444 tests with0 failures and2 ignored (library422 plus other targets22), `/tmp/picomado-stream-acceptance.log`. It preceded the final legacy-EOF persistence correction, so it is not the final source acceptance.

The legacy migration correction normalizes only stopped legacy selections already at their known exact PCM EOF to position0 during migration. It preserves partial pauses and is tested across save/load. That snapshot was frozen, with169 service input hashes captured in `/tmp/picomado-stream-frozen-inputs.json` (manifest SHA256 `35e8825a677b182d91c24243b7f1f94b15a3d3fc1eb2d53c6742f5645b8440c1`).

That frozen-source all-features regression passed445 tests with0 failures and2 ignored (library423 plus other targets22), `/tmp/picomado-stream-frozen-all.log`. Its release build completed in5m56s, but neither is final acceptance: a standalone state reproduction then demonstrated the remaining metadata clamp below. The old release is excluded from deployment.

### Final position correction

A compiled-library reproduction confirmed a real resume regression, not only display truncation: a stopped streaming position6000 frames with estimated duration4800 displayed100ms, and a repeated PAUSE saved4800; PLAY resumed4800 rather than6000. The failing assertion is retained in `/tmp/picomado-stream-position-probe.log`. Production corrections are limited to the Web position and PAUSE call sites using Track.limit(), preserving exact limits for legacy PCM. New regressions cover consumed6000, repeated pause, save/load, resume6000, actual EOF6500, EOF replay0, and legacy4800 limits.

After this correction, the only service input changed from the preceding freeze is `service/src/local_music.rs`. Final169 input hashes are captured in `/tmp/picomado-stream-position-fixed-inputs.json` (SHA256 `b5b760cbed02b7a36cf2e0835b24693c758e17497bbe0dadc2c1bd954fe2ffd6`). Compile, targeted music/SBC tests, then full regression use the same frozen inputs and test binaries, with `PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px`, `-j1`, and serial test execution. The targeted run passed21 music tests and9 SBC sender/sink tests, including the new position regressions. Combined music/home UI tests passed15/15 and the static domain/boundary gate passed. The full run completed with446 passed,1 failed,2 ignored: library424 passed,1 failed,1 ignored in381.09s; other targets22 passed,1 ignored. Its sole failure was the existing host loopback at host.rs247, `Runner(Conflict("plugin runner shutdown timed out"))`, the same error detail as the first intermediate run. The exact same test binary reran that test successfully (1 passed in39.03s). This rerun does not turn the full run into a clean pass. No unrelated source or test was changed. Every streaming, music input, SBC transport and integration test passed.

Logs: `/tmp/picomado-stream-position-{compile,music,sbc,all,host-rerun,ui,static}.log`. All169 service input hashes and all six compiled test-binary hashes were verified unchanged after the complete run; binary hashes are in `/tmp/picomado-stream-position-test-binaries.json`. The source retains home UI commit5cc4d4b. Production release is built only after the implementation commit and rechecked against this manifest; its artifact, embedded UI verification and SHA are recorded with the staged candidate.

## Staged release

Implementation is committed on main as `94fde75e383a8e438a2a3f668dedd6c50b21c19e`. The production `--release --locked --features caricature-retention-delete --bin picomado-server -j1` build succeeded. All169 frozen input hashes were unchanged before and after the build. Home UI commit5cc4d4b is an ancestor.

Candidate directory: `/home/tane/picomado-staging/20261108-streaming-94fde75`.

- Binary SHA256: `a8f944282ef96b78c34cc0e0fd548352b8070ebef7eb896d6c4e48eb18807ccd`.
- Embedded UI SHA256: `705c1c3069a4bd7ff57310949390aae1c732fe8de7603945c47254a4c365bef9`.
- Full source HTML was found byte-for-byte in the binary and extracted. The extracted HTML passed all15 music/home UI tests, including removed preparation/conversion controls, player-first order and collapsed device settings.
- `manifest.json`, `source-inputs.json`, `test-binaries.json`, extracted HTML and validation logs are retained beside the binary. The older pre-correction release is not the deployment candidate.

The read-only pre-migration inventory found12 tracks with originals and exact-length PCM caches, totaling684,484,456 PCM bytes. All were still legacy metadata, so none is approved for removal yet. No hash/full-song decode or delete/move was performed. Rollback and later recoverable cleanup follow [the PCM retirement guide](../guides/local-music-pcm-retirement.md): preserve matching old binary, consistent SQLite/config backup and PCM until the new reader is verified. This inventory is not evidence of successful migration or audible playback.

## Not established

No server install/restart, USB/device action, SSH, push or audio playback was performed by this implementation task. Final independent acceptance, deployment and hardware continuity/performance remain parent-owned. A100ms read timeout still becomes a terminal failure rather than fabricated silence. Loaded-host continuity is not proven; startup waiting is separated from device deadlines, not a relaxation of normal read/credit timing. Long resume offsets decode/discard from the start for sample-exact positioning and can hit the10-second startup limit.
