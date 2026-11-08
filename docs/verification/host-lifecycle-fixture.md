# Finite host lifecycle fixture (BT-AUDIO-20261107 / MUSIC-PLAYER-20261107)

## Failure evidence before correction

At integrated service source `0318b5e` with production feature
`caricature-retention-delete`, the full library regression ended413 passed,
1 failed,2 ignored. The sole failure was
`host::tests::loopback_host_runs_initial_and_repeated_fetch_then_stops`,
`src/host.rs:247`, `Runner(Conflict("plugin runner shutdown timed out"))`.
Normal Cargo continuation stopped there, so later targets were not run.
Retained log:
`/home/tane/picomado-staging/20261108-remembered-autoplay-0318b5e/full-regression.log`.
The same-source isolated exact test passed in50.67s; its log is
`/tmp/picomado-remembered-autoplay-host-isolated.log`. This does not convert the
failed full run into a clean pass. Earlier occurrences with the same detail
are preserved in `../evidence/MUSIC-PLAYER-20261108-streaming.md`.

## Source-supported diagnosis and limits

The injected **test** runner uses a30ms Tokio interval with default missed-tick
Burst behavior. Each tick synchronously runs `fetch::run_cycle` while holding
its App owner, then advances fake time by a whole hour. The synchronous path
calls native preparation, including multiple radar image renders and blocking
pic2gray4px subprocess waits. The fixture uses a current-thread runtime.
Therefore delayed ticks can remain ready, more expensive fake refreshes are
eligible indefinitely, and an unbiased select can choose them over an already
ready stop signal. Synchronous work also prevents timely polling of other
futures/timers on that executor. The HTTP request phase has no explicit overall
test deadline, so the isolated50.67s total alone does not identify which phase
was slow. The logs do not contain enough polling timestamps to prove the exact
branch sequence of the failed run.

This is a concrete fixture scheduling defect and a supported explanation for
intermittent shutdown starvation, not proof of a new production regression.
The test does not use a device WebSocket Hello, remembered-MAC lookup, or music
navigation. Production uses a different async periodic runner with shutdown
selection during its fetch cycle and blocking preparation offloaded. Do not
change production runner behavior or increase host shutdown timeouts on this
evidence.

## Test-only correction contract (fixed before implementation)

The fixture's purpose is initial fetch, one later scheduled refresh, HTTP state
availability, and explicit host shutdown, not indefinite accelerated rendering.

- Perform exactly two actual fetch/apply cycles. Advance fake time by1hour once,
  before the second cycle. Keep a small async interval between these cycles,
  without a perpetual missed-tick backlog.
- Check shutdown before starting each cycle and prioritize it during the async
  wait. After the two cycles finish, notify the test and wait for shutdown;
  normal completion must not exit the runner before the requested stop.
- Assert each cycle's actual planned/attempted/applied fetch counts and total
  four fetches. Wait for completed cycles, not merely the fetch-entry counter.
- Preserve HTTP state success, successful host join, unchanged count after
  stop, and closed listener. Add a fixture-level already-stopped case proving
  that no new cycle/fetch starts.
- Keep the existing8s setup wait,12s outer shutdown assertion, and production
 10s drain deadline unchanged. Do not relax assertions, skip the failing test,
  change product scheduling, or claim a timing/RF repair.

Parent approval permits this narrow fixture correction. After implementation,
run focused host tests and record the result. The parent owns the subsequent
frozen-source full Cargo regression and release decision; the earlier failed
run and isolated pass remain separate evidence.

## Implementation and focused verification

The change is confined to `host.rs`'s `#[cfg(test)]` module. A shared fixture
helper performs the two real fetch/apply cycles, asserts each report is
planned2/attempts2/applied2/stale0, then sends a completion notification and
waits for stop. The loopback test requires total fetches4 and confirms the host
has not exited before stop. A new already-stopped fixture case requires zero
fetches and no false completion notification. HTTP, successful shutdown join,
post-stop stable count and listener closure assertions remain.

The production host source prefix before `#[cfg(test)]` is byte-identical to
the pre-correction source, SHA256
`7a73d612a4b8a285e298b0539aa220600ba71f26d932326e1cdab7ced41b009a`.
No product timer, runner or deployment code changed;8s/12s fixture timeouts and
10s production drain were not increased. The two synchronous fixture cycles
are still non-preemptible while executing, so the8s Tokio wait is not a hard
wall-clock limit on native rendering. This correction bounds scheduled work
and makes stop admission deterministic; it does not claim preemption or improve
production/RF timing.

Focused command with `PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px`:
`cargo test --manifest-path service/Cargo.toml -j1 --features caricature-retention-delete --lib host::tests -- --test-threads=1 --nocapture`.
All3 host tests passed in11.68s. The same final feature-specific test binary
then reran the original exact loopback test three times:3/3 passed in
11.29s,11.23s and11.38s. No source changed between those runs.
`git diff --check` passes. These focused results do not replace the parent's
required final integrated full regression.

Logs:
- `/tmp/picomado-host-finite-focused.log`, SHA256
  `8ddf40c87b22b6a12a98670d55f3cba7d5e624b0cbaecdc9c6dee216c598aeb9`.
- `/tmp/picomado-host-finite-repeats.log`, SHA256
  `27b0bd6f9d0ade1b0f97056a6a629b2fa312d2b0deee821501ef85b8bef3ac05`.
