# Bounded PCM rebuffer: host implementation evidence

Change BT-AUDIO-20261107. Specification commits `ddd3154`, `1ac5d2e` precede
implementation. The existing 4096-frame PCM buffer is unchanged.

## Implementation and limits

The private runtime owns the same stream's shortage/rebuffer state through
`bluetooth_rebuffer.h`. Production timer/prepare/send and PCM APIs are factored
into `bluetooth_audio_runtime.inc`, included by both firmware and deterministic
host fixtures. No SDK body was modified.

A shortage before the next packet's deadline can recover normally. Crossing
that deadline enters rebuffer: preserve samples, accepted/consumed positions,
and RTP timestamp; wait for the full4096 PCM buffer or fully received declared
EOF. No transient AVDTP SUSPEND/START is added. On the first successful resumed
RTP send only, reanchor wall time to the current sample position. Following
packets retain normal24ms cadence for the measured9-SBC-frame packet. There is
an audible gap and longer wall-clock duration; this is not time-stretch,
accelerated catch-up, or proof of uninterrupted playback.

The first shortage's1500ms deadline includes blocked resumed can-send. At most
three rebuffer episodes are permitted until five seconds of successful normal
playback resets that budget. Timeout or exhaustion emits one UNDERFLOW with
status0xff04, reason, elapsed milliseconds and remaining frames, then existing
failed status/UI flow takes over. Changed absolute credits continue while the
runtime stays active. No status wire field was added. The server's existing3s
no-credit timeout remains independent; its already-expired stop is respected.

Stop, peer suspension, release, invalid generation and current-stream teardown
clear prepared data, pending callbacks, timer and rebuffer owner. Validated
same-peer/CID/ACL/endpoint refresh alone transfers the owner's Bluetooth command
generation without changing stream, deadline or budget. Deferred grants replace
the pending watchdog timer rather than adding the same SDK timer twice.

EOF waits at least the final successfully sent packet's sample duration before
local shutdown, so the new completion branch cannot immediately pause its tail.
This does not measure the physical speaker's playout buffer.

## Executed checks

`bash firmware/tests/run_rebuffer_tests.sh`: seven focused executables in native
and ASan+UBSan variants, plus the production pipeline structural test, all
passed. Log: `/tmp/picomado-rebuffer-focused-final.log`. LeakSanitizer is disabled
because this executor uses ptrace; ASan/UBSan remain enabled.

Coverage includes actual production tick/prepare/send with deterministic codec,
timer and RTP adapters: ordered samples;640-residual starvation; timely refill;
full-buffer recovery; preserved RTP/consumed positions;24ms resumed cadence;
no catch-up burst; final short EOF and tail interval; exact1500ms boundary;
blocked and late grants; asynchronous timer replacement; cancellation in three
wait phases; repeated shortage limit; healthy-budget reset; unsigned-clock wrap;
verified-refresh ownership preservation and invalid refresh refusal. Existing
PCM, packet-policy, bridge credit and Bluetooth-control fixtures also pass.

Pinned SDK target runtime syntax check reused the existing integrated build's
compile command with `-fsyntax-only`, without changing its artifact. Exit0,
log `/tmp/picomado-rebuffer-arm-syntax.log`. Build configuration points at the
repository's pinned SDK. The first full-host attempt accidentally selected the
other installed SDK and correctly failed the reviewed cyw43-source hash guard;
the corrected pinned run is logged in
`/tmp/picomado-rebuffer-all-host-pinned.log` and is reported separately.

Root gate attempt `/tmp/picomado-rebuffer-gate.log` stopped on generated
observation representation drift while SBC protocol work was in progress in
the shared tree. This is not a passed gate or independent acceptance. Full
integration/build must be rerun after all workers freeze the final source.

No physical device, playback, service, network setting, SDK source, SSH or push
operation was performed. Hardware AVDTP gap tolerance, audible effectiveness,
full integrated firmware and independent review remain with the parent.
