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
