# LCD heartbeat baseline investigation (OBS-20261021-02)

This is an investigation change on the isolated `investigate-lcd-latency` worktree. It does not change the existing same-prefix admission rule, measurement quality, or the device and service in use.

## Preserved facts and limits

The saved hardware attempt ended before observer START and before DTR low. `AP2 ARM_HB` captured Pico send-attempt ordinal 46; the host baseline arm refused. Host `last_message=40` came from cleanup, not the rejection reply. `AP2 TRIGGER`'s `o` and `g` remained 46 while USB was open because `g` is only updated at gate close; those fields do not show whether the live ordinal advanced during PREPARED. Real C send to the Rust reader reproduced refusal at 46/40 and acceptance after the six held messages were processed on the same connection. It does not establish six lost sends in hardware. A COPY failure moves the firmware send path to FAILED, preventing six independent encoded retries in one ready generation. Neither TCP ACK nor an unchanged ARM snapshot establishes application processing.

The earlier LCD switch delay remains approximately 1.497 s. The stored packet evidence shows head-of-line blocking behind absent TCP sequence `[8064,8163)` (99 bytes), released after approximately 1.344 s by retransmission. The missing payload is consistent with a heartbeat, but its initial transmission and loss location were outside the capture. Heartbeat cause and any fix remain unconfirmed.

## Diagnostic contract before implementation

The host `status.last_message` is a processed ordinal, scoped to its accepted connection and session. The Pico `AP2 LIVE` diagnostic may expose its *current* encoded send-attempt ordinal as an additive hexadecimal field (`message`); `AP2 TRIGGER.o` remains the immutable ARM snapshot and `g` remains the gate-close snapshot. The live field is read-only and does not reset, arm, trigger, or change send behavior. A read has only its own endpoint's timing; controller monotonic times bracket the request/response and must not be subtracted from Pico or host clocks.

On a host arm refusal, the preparation adapter saves the existing pre-arm host status, the Pico origin and live reading, the single arm request outcome, and a new post-refusal host status and Pico live reading where the owner remains trustworthy. Each sample carries controller monotonic request/response brackets. A status failure is recorded as unavailable, never translated into a baseline value. Existing freeze/save/release cleanup remains authoritative. No automatic retry or subtract-six adjustment is introduced.

Interpretation is bounded: equal pre/post host ordinals with unchanged boot, connection and session establish the rejection-time host watermark if the arm refusal is inside that bracket. A changed host ordinal gives only a range. Pico live samples before and after can show PREPARED sends, but do not locate an individual send between endpoint clocks. If bindings or generation differ, the counters cannot be compared. Even equal counters alone remain a candidate until same ordered prefix, no failed COPY/reset/wrap, and the existing target quality checks are established.

## Acceptance and next physical observation

Pure tests must show a baseline refusal writes a diagnostic bracket while retaining the Pico partial arm and calling host arm once. Model a host watermark that drains during the bracket and one that stays fixed; label exact versus bounded accordingly. A PREPARED send must change the live field while `o/g` stay at the ARM value; no trigger or recording may be implied. The existing 46/40 refusal, 46/46 acceptance and HB47 CRC/session proof remain unchanged.

After a separate explicit physical-operation decision: confirm the normal server binary, three online devices, target firmware identity, EV1 OFF, both probes IDLE and USB owner closed; use only local raspi5 operation with one owner. Preselect one static plugin outside the measurement and wait for stable connection and rendered frame. Take the diagnostic bracket during one bounded runner attempt. If the same-prefix gate accepts, start the observer before DTR low, then collect natural heartbeat TX/CY and host/header evidence through the existing finite window. If it refuses or any binding changes, preserve the partial attempt and stop. Do not flash, switch service, touch USB, or use SSH in this source-only phase.
