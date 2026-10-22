# INPUT-CONTINUITY-20261022 — sampled physical gesture continuity

## Problem and scope

At fixed normal USB/200ms firmware98fc95fa, two physical100ms holds can generate SUB/POWER when the intervening release cannot finish debounce because main does not sample it. The independent reproduction is saved in `independent-unexpected-sub-review-20261022` under the E-ink_1 incident results. This is a confirmed conditional software counterexample; the incident GPIO timing is absent and the same live cause is unproved. Server arrival spacing1965ms is not physical hold duration.

Reuse the existing binding data and gesture FSM. GPIO observation owns sampling quality; the shared operation API owns semantic actions. Do not change long-press thresholds, button mappings, USB commands, wire, PM/Nagle/SDK, hint deadline or panel ownership. No device operations are authorized.

## Language, ownership and invariant

A **physical sample** is the raw GPIO value with a current local monotonic millisecond timestamp. A **known virtual signal** is owned by diagnostic SIM's scheduled press/release source. A **cancelled gesture** has no short/long action and must see a debounced release before another press may arm it.

For physical GPIO, no long operation may be constructed using elapsed time across an untrusted sample interval. Define the existing debounce duration as35ms and the maximum trusted gap between physical samples as the same35ms. Main normally polls every1/5ms; the bound is an admission rule, not a claim about device scheduling. A gap strictly greater than35ms cancels the affected physical gesture and suppresses both short and long outcomes until35ms of newly sampled release. This intentionally loses an ambiguous operation rather than manufacturing SUB/POWER. Exactly35ms is admissible. Repeated gaps restart release qualification. uint32 subtraction preserves ordinary clock wrap; a backward/nonordinary timestamp becomes a large gap and cancels safely.

Cancellation is within the pure input aggregate in core.c. Main's GPIO adapter must use the sampled entry and capture its timestamp at the GPIO scan, after display polling, not reuse a pre-display clock. Every physical button has independent sample/gesture state. Formal USB submits operations directly and cannot mutate physical gesture history. Diagnostic SIM continues to use the known virtual signal entry and its own state/lease/cancel rules; it does not claim physical samples. Both entries use the existing binding/FSM, with no second action dispatcher.

## State table

| Previous observation | New physical observation | Transition / output |
|---|---|---|
| no prior sample | any level | establish sample clock; existing debounce/FSM |
| trusted stream, gap<=35ms | any level | existing debounce/short/long rules |
| any stream, gap>35ms | any level | cancel gesture; no operation; require release |
| release-required | pressed | remain cancelled; no operation |
| release-required | released for<35ms of trusted samples | remain cancelled; no operation |
| release-required | released for>=35ms of trusted samples | clear cancellation; no operation |
| release-qualified | new press | existing debounce/FSM may arm anew |
| any button | invalid binding/output argument | reject without state mutation |

This is not proof of electrical continuity between samples. Existing35ms debounce remains the contact-noise contract. Physical refresh BUSY alone must not suppress sampling; the driver remains cooperative. Startup keeps normal debounce; no inherited gesture exists after reset.

## Fixed acceptance before implementation

1. Keep permanent real-main counterexamples: upper/lower100ms taps separated by an unsampled release, and one released sample without completed debounce;1500/1800/2000ms thresholds. Old source must fail the no-SUB/no-POWER assertion; corrected source must pass.
2. After any gap, held raw input must not generate delayed long or short on release. A fresh35ms sampled release must allow the next ordinary short and continuous long operation, once each. Cover multiple gaps, exact35/36ms boundary, startup and clock wrap.
3. At regular5ms sampling preserve existing short/long/debounce, threshold snapshot, standby, simultaneous independent buttons,0/2/4 layouts, and binding data. Test short release bounce, no repeat and no short after long.
4. Run real-main GPIO+USB cases and existing formal USB/SIM/TX regressions. USB operations do not modify physical state. Diagnostic virtual hold semantics and lease/cancellation remain unchanged. Retain200ms hint cancellation/image-priority regressions; exercise physical short presses around actual driver hint phases/BUSY.
5. Run root check, host regression and meaningful ASan/UBSan cases. Build three existing device identities through the existing controlled build route using unchanged private configuration and SDK variants/diagnostic flags. Verify full artifact hashes, configuration binding, reserved settings sector exclusion, Nagle continuity and LCD PR155 source inheritance. These are candidate builds for independent review; no parent approval or device effect is asserted.

## Limitations / follow-up

An unexpected long operation can also arise from unobserved input origin or electrical faults. This change only closes the reproduced untrusted-sampling path. No observer firmware, new gate, automatic retry, or new process framework is introduced. Parent owns independent acceptance, operational application and closure.

## Scope correction for direct press operations

[INPUT-CADENCE-20261022](INPUT-CADENCE-20261022.md) narrows gap cancellation to release/hold bindings. `on_press` operations retain their existing debounce/edge behavior and never synthesize a long operation. The previous all-physical wording above is superseded for these direct bindings; false-SUB/POWER protection for held gestures remains.
