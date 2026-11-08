# BT-AUDIO-20261107: bounded startup timing diagnostic

Specification was committed first as `df83047`. This is implementation and local
verification evidence, not independent acceptance, firmware deployment, a repair
claim or operational closure. Source remains the canonical `main`; unrelated
music/server work was not staged by this change. Git author configuration was not
changed.

## Implemented

`PM_BT_STARTUP_DIAGNOSTIC` accepts 0 (default/off), 1 (observation A), or 2
(observation and ACK-wait B). `firmware/cmake/bluetooth_startup.cmake` validates the
cache value. BT-disabled CMake targets do not receive the diagnostic definition.
The normal mode0 controller path has no new runtime queries/logs/gates.

The pure rule is in `firmware/src/bluetooth_startup.h`; the main-loop adapter is
`bluetooth_startup_control.inc`. For a pending first-start command, mode2 samples
at most once per100ms. It requires current ACTIVE/READY/IP-UP, a valid same
connection, forward ACK change, at least5000ms after that change, and current
`lastack==snd_lbb` with no unsent/unacked data. The100ms sampling interval can add
up to one sample interval to the exact rule boundary. Loss/connection changes and
superseding commands reset the evidence. Sequence/time wrap and ambiguous or
regressive ACK are covered. USB visibility/DTR never participates in the decision.
No sleep or extra network traffic was introduced.

A pending uninitialized stop completes as a no-op in mode2 without powering HCI.
The ordinary stop behavior remains in modes0/1. The original25s local deadline is
checked before gate admission and is never extended. Expired/cancelled work cannot
start later; late readiness cannot revive the expired operation. A newer command
gets only its own existing received-time budget, not old gate progress.

Runtime captures actual HCI_POWER_ON begin/return and first WORKING in fixed
storage under its existing lock. The SDK run-loop clock is
`to_ms_since_boot(get_absolute_time())`, the same MCU clock used by the control
adapter. Main formats the captured times outside the lock; a later flush is not
misrepresented as event time. First-start connection/generation remain bound even
if a newer command arrives before WORKING. Command token records bind incarnation,
session, generation and command ID. Gate records include age, ACK/end and queue
flags. No keys, addresses, audio or packet payload are logged.

There are at most96 diagnostic text records per boot; the last marks further
records unreported. Existing trace queue/drop accounting still applies, and a
96-record cap cannot prevent earlier trace-queue loss. Every line is under144bytes,
including worst-case identity/counter values. Fully captured HCI markers stop
further lifecycle snapshot reads. This instrumentation is diagnostic, not a claim
of unmeasured zero timing cost.

## Local checks

1. `python3 firmware/tests/run_bluetooth_startup_tests.py` passed serially.
   Log: `/tmp/picomado-bt-startup-host-20261108.log`.
   - Modes0/1 execute the existing real-control fixture unchanged in behavior.
   - Mode2 tests pure boundaries4999/5000 and local24999/25000, fixed ACK,
     unsent/unacked/end position, IP/READY/PCB loss, connection replacement,
     generation/session/incarnation replacement, stop, sequence/clock wrap,
     HCI readiness, no repeated init, init failure, late ready after expiry,
     preserved first-start marker identity and log bounds/suppression.
   - All three modes compile/run with PM_BLUETOOTH_AUDIO=0. The production
     main-loop fake also passes BT-off with diagnostic mode2, including Wi-Fi
     timeout/retry, preserved healthy link, settings/menu, standby, heartbeat,
     trace-full progress and frame-report regressions.
   - CMake default/0/1/2 accepted; -1/3/01/ON/empty/1.5 rejected. C rejects -1/3.
   - Two initial fixture compiler errors (misleading indentation and a renamed
     main without explicit return) were corrected before the successful runs.
2. `python3 firmware/tests/bluetooth_pipeline_test.py` passed.
3. `git diff --check` passed for the working tree at verification.
4. Small ARM check compiled main.c, bluetooth_runtime.c and net.c once each per
   mode, strictly sequential, using existing E-ink_1 flags. Each mode then linked
   in an isolated output directory with the existing dependency objects.
   - Log: `/tmp/picomado-bt-startup-arm-20261108.log`.
   - Exact argv, source hashes and unchanged original-object hashes:
     `/tmp/picomado-bt-startup-arm.e7j6mxft/report.json`.
   - Mode0 text553768 / BSS166608; mode1 text555336 / BSS166660;
     mode2 text556192 / BSS166688 bytes. Relative to mode0, A adds1568/52 and
     B adds2424/80 bytes of text/BSS.
   - `arm-none-eabi-objdump -dr` after excluding only the filename heading is
     identical for mode0 main and Bluetooth runtime versus the existing E-ink_1
     objects. This establishes equal instructions/relocations for those two
     objects, not all debug/data bytes or whole-image identity. net.c separately
     contains the earlier approved terminal-TCP cleanup, so it was not claimed
     identical to that older object's code.
   - Existing E-ink_1 build objects were hash-checked unchanged. No UF2 was
     created. These mixed-dependency ELFs validate syntax/link compatibility;
     they are **not deployable clean-build/provenance receipts**.

The focused suite is also called by the existing host-test runner. The entire
host regression and root project gate were not run in this task; the root check
invokes Cargo, which was expressly outside scope. No Cargo, USB, SSH, service,
network setting, playback, firmware write or push operation occurred.

## Remaining interpretation and real-device steps

The original hypothesis and negative/positive evidence remain in the preceding
specification and old/new comparison. BT compile-out/old-image reproductions
exclude BT as necessary for that whole failure class, not as an amplifier.
SDK shared initialization and BT-capable WLAN firmware selection remain unchanged
in both diagnostic modes. Delaying the application HCI controller does not test
those earlier layers.

The host's30s issue deadline and MCU's25s receipt deadline are independent clocks.
No conversion or freshness claim is made. A delayed incoming command may already
be stale at the host; mode2 waiting consumes additional budget. Retain gate-expired
no-HCI boots as failed diagnostic trials, not successful BT connections, and never
weaken stale-report checks. The alternative of issuing a fresh host generation
only after independently observed stable traffic was not implemented.

Parent-owned next step is a regular provenance-bound A/B build and separately
authorized backup/flash/cold-start/capture of the same E-ink_1, same server/AP/profile,
saved target and stopped audio. Change mode1 versus2 only. Record the full boot,
MCU command/HCI/ACK markers, host issue/send/report/expiry, achieved delay and
missing/suppressed records. A gate that simply waits for independently recovering
TCP does not establish that delaying BT repaired the earlier stall. One boot per
mode does not establish failure incidence; repeated controlled starts and
independent review/effectiveness remain pending.

## Pre-deployment deadline correction

Review after initial implementation8ae8205 found that a shared snapshot/lock
wait could cross25s after the original timeout check. Mode2 now refreshes time
after the network query and rechecks under the acquired runtime lock immediately
before initialization. If the bound was crossed it performs no initialization;
the next main poll reports the existing expiry. First-start identity is bound
only after this admission, so an expired attempted wait cannot steal a later
command's HCI markers. Modes0/1 keep their behavior.

The fixture advances the fake clock24.9s→25.0s inside each wait separately.
The8ae8205 implementation fails that new assertion, saved at
`/tmp/picomado-bt-startup-old-deadline.f843zt0u/failure.log`; the corrected implementation
passes the complete focused suite. All nine ARM TU compiles/three isolated links
were repeated; the size and report above are the corrected results. Mode0
instructions/relocations still match the unchanged normal E-ink_1 objects.
An in-progress A build was stopped with exit130 before source changes; no
pre-correction deployable pair was released. A/B staging is separate below.

## Regular deployment artifacts, source88f2301

After the deadline correction, the parent requested regular E-ink_1 A/B UF2
builds. These completed through ordinary CMake configure/build in separate
private directories, strictly sequential with `-j1`. The interrupted A directory
was resumed using normal dependency tracking; the changed main/control source
was rebuilt. B was configured separately. No manual object substitution was used
for these deployment artifacts.

Staging: `/tmp/picomado-bt-startup-deploy.y0su76_x/` (directory0700, artifacts0600).

- A: `artifacts/A-observe-E-ink_1.uf2`, diagnostic1,1098752bytes, SHA256
  `998a5a225bce39a06f1434c8542246377a63316e2cd40438d226da6a931a673a`.
- B: `artifacts/B-ack-wait-E-ink_1.uf2`, diagnostic2,1100288bytes, SHA256
  `ac789cb7b47964b69c27c3c4c2216d376f39ee191eaea202f9365fc44374724c`.
- `manifest.json`, SHA256
  `69b6c60881f818cd617e2e6093586f4fbb3af4962a280b3057636a7a8d257118`,
  binds source88f230142061e083b4dbce9fe50c8225a0adaaa8, all tracked firmware input
  hashes, private config hash (not values), SDK/submodule IDs, exact configure/
  build argv, ELF/UF2/log/flags hashes and completion checks.
- Both builds' effective C definitions/includes/flags match the normal E-ink_1
  build after normalizing build-directory paths and removing only the diagnostic
  mode definition. A/B use the same source and private configuration.
- The original `/tmp/picomado-eink-audio-E-ink_1` UF2/cache/flags are hash-verified
  unchanged; source/config did not change during builds, and SDK/submodules remain
  unchanged/clean. No ordinary build option or config value was silently reset.
- Offline UF2 validation in `uf2-validation.json` verifies block magic, RP2040
  family,256-byte payloads, complete sequential block numbering, unique addresses
  and no overlap with the final12KiB reserved settings/BT storage. No USB command
  is part of this validation.

These artifacts and hashes were returned to the parent, which owns all backup,
flash, restart and capture actions. Parent-reported live observations and any
A/B causal conclusion remain separate from this build-only evidence. The producer
did not operate a device or service, and private configuration values were not
printed or copied into this document.
