# LCD baseline investigation: source-only result, 2026-10-21

## Finding

The first hardware natural-heartbeat attempt stopped at host baseline admission, before observer START or DTR low. Pico `AP2 ARM_HB` fixed send-attempt origin 46. Host cleanup later showed processed ordinal 40, not the rejection-time value. The exact six-message cause remains unknown. The real C→Rust isolated path proves that six pending application messages alone can produce 46/40 refusal and, once processed on the same connection, 46/46 acceptance and matching HB47 session/CRC. The firmware's first failed COPY marks the connection FAILED, so six independent COPY failures in one uninterrupted ready generation are unsupported. ARM `o/g` values cannot rule out PREPARED sends.

The original approximately 1.497 s switch delay is separate: saved TCP evidence identifies a 99-byte missing interval `[8064,8163)` and approximately 1.344 s until retransmission released later notifications. The first transmission was outside the observation window. The heartbeat payload and loss location remain hypotheses, not a diagnosed or repaired cause.

## Implemented on isolated worktree

The existing runner now saves a monotonic controller bracket for host status before/after a refused one-shot arm, Pico ARM origin and existing `AP2 LIVE` binding values. The existing firmware can support this host-side bracket; no new firmware is required for it. A candidate firmware additionally exposes current send-attempt ordinal as read-only `AP2 LIVE.message`, allowing a PREPARED-send check while preserving `AP2 TRIGGER` snapshot meanings. No admission or capture quality rule was changed, and no automatic retry was added.

Local checks: `heartbeat_capture_test.py` 24 passed, `heartbeat_owner_runner_test.py` 12 passed, and compiled `ack_probe_heartbeat_test.c` passed including PREPARED live ordinal versus unchanged `o/g`. `git diff --check` passed. `tools/project_gate.py check` rejected the modified branch with `implementation: wrong evidence stage/subject` because OBS-20261021-01's pinned evidence belongs to an older subject; this investigation did not alter that prior evidence or claim root-gate success. The new code and firmware have not been built for or run on the board.

Read-only local production check: `systemctl --user show picomado.service` returned active/running PID 1850719; `/home/tane/bin/picomado-server` SHA256 is `e7b52c29d776bf9b9fbfea9125216c7b5c05613f47fadc9660618dedb76c727c`. Existing `/api/state` GET returned HTTP 200 and all three devices online. The target firmware's current hash, EV1/probe values and USB ownership were not read live because this phase did not open USB; the handoff's last confirmation remains the only evidence for them.

Next operation for the parent to coordinate: run the existing finite capture once on the current firmware with one local USB owner, after confirming the remaining device state and selecting a static plugin outside the window. Save the new host bracket even if admission refuses. Only if PREPARED activity remains relevant should the additive `AP2 LIVE.message` candidate be built and its firmware SHA, settings backup, preflight, one-load verification and rollback path reviewed before any flash. No SSH, service switch, USB operation, flash or push occurred in this investigation.
