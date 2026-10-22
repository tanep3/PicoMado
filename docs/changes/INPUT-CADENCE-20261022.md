# INPUT-CADENCE-20261022 — limit continuity cancellation to held gestures

## Confirmed regression and live limits

Current LCD UF2 e3941513eca5ccafa38260062a1dc9a49491fbf60d559274227efa6840051595 was built from53d8bdd2b425d81ee36170b8bb8d03857a098d06, which inherits INPUT-CONTINUITY-20261022. Its sampled input wrapper resets the gesture on every gap>35ms, including while released. If such gaps repeat, release qualification never completes and all physical operations are suppressed. A host cadence model confirms all four keys produce zero events at36/40/50/100ms intervals, while the unchanged original binding FSM emits one event per held press. At5/20/35ms, both emit one. This is a deterministic application regression introduced by the continuity change. The actual incident's GPIO sample timing has not been measured here; video playback alone does not prove that timing.

Main scans once before receive/decode/draw, and poll_video synchronously calls display_frame. The LCD SPI write and transforms are synchronous, while the loop's1ms sleep is only the final sleep, not a maximum input sample interval. A video FPS/render gap is not itself a GPIO sampling gap. No new claim about SDK, RF, USB ownership or live wiring is made.

## Correct domain scope

`on_press` bindings have one operation when the existing debounce confirms a pressed level. They never transform an elapsed hold into another operation. Existing four-key bindings all have on_press=true. The short/long release-based bindings have on_press=false; the two-key bindings can transform elapsed hold into SUB/POWER and still require the physical continuity guard.

The pure sampled entry validates arguments, then routes by existing binding semantics. For on_press=true, call the unchanged binding FSM directly. For on_press=false, retain the exact current gap cancellation, release qualification, threshold snapshot and rearm rules. No PM_BUTTON_COUNT or display/plugin branch is added. The same dispatcher and GPIO/USB/SIM responsibilities remain. Direct operations retain the existing polling debounce semantics; this does not prove electrical continuity between sparse samples or recover a press entirely missed between them.

| Binding | Sample gap | Rule |
|---|---|---|
| on_press=true | any | original debounce/press-edge FSM; one semantic operation, no long synthesis |
| on_press=false | <=35ms | existing debounce and short/long rules |
| on_press=false | >35ms | unchanged cancellation; no action until newly sampled release qualifies |
| either | invalid binding/output | reject without mutation |

This correction narrows the over-broad original continuity contract. It does not increase any long threshold or relax the false-SUB/POWER prevention for held gestures. Repeating long gaps can still cancel release-based gestures; those inputs cannot safely infer a hold without sufficient samples. A future independent scheduling change would require its own evidence and contract, not a hidden threshold increase here.

## Acceptance fixed before implementation

1. Permanent model: on_press bindings at5/20/35/36/40/50/100ms polling, all four operations, one event per press and no repeats. Release then second press works. Old guarded code must fail the36ms+ acceptance assertion.
2. Real main+video cadence model: execute the production GPIO adapter, video offer/draw and shared notification path for each key. Under40/100ms supplied work cadence, old code has advancing rendered frames and no operation; corrected code sends the expected kind once. Modelled duration is not claimed measured draw latency. Check identical short/long two-key bindings preserve all original12 false-SUB/POWER refusal/rearm cases.
3. Re-run full host and USB/SIM/TX/root checks and relevant ASan/UBSan. Existing200ms image/feedback ownership tests remain. A controlled LCD candidate must retain current per-device identity/configuration, radio comparison features, seven diagnostic flags, SDK/private LL, Nagle and settings-sector exclusion.
4. Independent verification, parent acceptance and live application/effectiveness remain separate and pending. No device/USB/SSH/service/network-setting/push operation is part of this worker's task.

## Introduction / escape cause

The previous fix used one physical sampling rule for both elapsed-hold and direct-press bindings. Its acceptance covered steady5ms GPIO sampling, single long gaps and E-ink feedback BUSY, but lacked sustained slow/alternating video cadence for LCD direct-press inputs. Preserve those successful false-SUB tests and add the missing cadence acceptance to the existing host/USB-domain route; no new process framework or observer firmware.
