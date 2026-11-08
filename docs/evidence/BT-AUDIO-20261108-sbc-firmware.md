# BT-AUDIO-20261107: SBC receiver implementation evidence

2026-11-08. Specification: `808a838` / [SBC transport](../design/bluetooth-sbc-transport.md). Integrates bounded rebuffer `b5d46ac` and host encoder fixture `f64dd8c`. This is implementer verification, not independent approval, deployment or continuity acceptance. No SSH, USB, service change, SDK edit, playback or push was performed.

## Implementation

- Advertise `audio-sbc-v1`; admit kind28 Start and kind30 binary without changing kind22/24 PCM. Ready kind29 contains the actual negotiated tuple, fixed chosen bitpool, nonzero codec revision and16384 canonical capacity. Ready uses the existing bounded COPY retry/report owner, and is cancelled on stop, display/session invalidation, runtime failure or timeout.
- `bluetooth_sbc.h` validates supported tuple, exact1..8-frame payload, sync/header/bitpool, protected-header CRC-8 and joint reserved bit. Full validation precedes atomic ring admission. Wholly old chunks cannot grow credit, partial overlaps/future gaps/current malformed data terminate.
- `bluetooth_audio.h` shares exactly16384 payload bytes between PCM and SBC. Separate byte and canonical bounds include prepared-but-unsent RTP in credit. Whole SBC frames pass unchanged into the existing MTU/15-frame/40ms packet policy. The SDK encoder is not called for SBC input. PCM retains its encoder path.
- Immutable pinned codec tuple/epoch are checked on ingress, preparation and send. Reconfiguration or link replacement cancels old queued/prepared work. Per-format full-prefill and bounded rebuffer preserve canonical and RTP positions. Successful RTP send alone releases credit. Final short EOF preserves the existing final-packet drain interval.

## Verification

- `bash firmware/tests/run_sbc_tests.sh`: all three suites pass natively and with ASan/UBSan.208 tuple combinations; exact-length/CRC/header/reserved-bit negatives; capacity/byte wrap; prepared-credit retention; unsupported tuple/zero epoch/u64 overflow. Twelve host-generated real codec fixtures are checked by the C validator and copied bit-for-bit through production prepare/send with zero encoder calls.
- Production runtime fixtures exercise PCM regression, SBC aggregation, full refill, timeout, changed tuple/epoch at ingress/prepare/pending send, arbitrary first frame and finite EOF. Wire bridge fixtures exercise Ready fields, retry/COPY/no Ready retransmission, report deadline, early data, whole duplicate versus partial overlap, exact-current identity with each truncated length32..40, terminal CRC tail, startup capacity, stale identities, and cancellation.
- `bash firmware/tests/run_rebuffer_tests.sh`:15 focused PCM/policy/bridge/structure checks pass after SBC integration.
- `PICO_SDK_PATH=/home/tane/dev/PicoW/pico-sdk bash firmware/tests/run_host_tests.sh`: full firmware host suite exits0, including E-ink and all52 net-EOF recovery scenarios. A subsequent redundant runtime CRC-pass removal and additional negative assertions were covered by the final native/sanitized SBC suites and final LCD rebuild. Parent final integrated-head regression remains distinct.
- ARM syntax checks pass for `bluetooth_runtime.c`, `main.c`, `net.c` using the existing pinned LCD compile commands.
- Full LCD and non-Bluetooth E-ink_1 builds pass. Final LCD recompilation used `CMAKE_BUILD_PARALLEL_LEVEL=1` and `-j1`. Final build directories: `/tmp/picomado-sbc-integrated-lcd` and `/tmp/picomado-sbc-integrated-eink`.
- `git diff --check` passes.
- Fresh-output project gate is **not passed**:49 structural tests contain one failure in pre-existing `tests/test_gate_routes.py:46`, which expects removed approval to reject but sees return0. No gate/flash tooling was modified. Receipt folder `/tmp/picomado-sbc-firmware-gate-1794106055`; log `structural.log`. This must not be described as successful integrated approval.

## ARM resources and artifact identity

LCD configuration matches parent baseline `/tmp/picomado-home2-integrated-lcd`: Bluetooth ON, HCI drain limit0, ARP/CREDIT/SLEEP probes ON, INNER/SPI_REAL_SIZE/TX_LINK/ACK_PATH/EVENT_STREAM OFF, AWAY_EEX1 ON, standard SDK. Existing device configuration is used without copying or publishing credential values. E-ink uses the existing E-ink_1 production flags with Bluetooth explicitly OFF.

| Artifact | text | data | BSS |
| --- | ---: | ---: | ---: |
| Existing parent LCD baseline |562656|0|238236|
| SBC + bounded-rebuffer LCD candidate |569904|0|238372|
| Non-Bluetooth E-ink_1 candidate |400724|0|162432|

LCD BSS grows136 bytes over the parent's earlier artifact, including concurrent bounded-rebuffer metadata, not16384 bytes. The ARM audio aggregate grows from16416 to16456 bytes; its PCM/SBC payload union remains16384 bytes. `bt_media` grows96→120 bytes; codec configuration12, epoch8 and rebuffer32 bytes are separate bounded metadata.

Compiler static stack reports: `prepare_audio`952 bytes, `tick.part.0`80, SBC begin56, SBC push72, `api_audio`168, `main`904. The earlier baseline reports inlined `tick`968 and `main`928; inlining changes prevent treating these as directly comparable peak call-chain totals. These are static per-function observations, not measured runtime stack high-water evidence.

UF2 SHA-256:

- LCD `105654b2705e575436dfd386eecbabadb4caff8b81a50fe9af55db7b08a0d23d`
- E-ink_1 `16b7649af19d6209803c55aa8aa48a3b68798d7660049696b0da8e1f7ab48bb7`

Remaining: parent integrated sender/receiver verification and independent approval; authorized device/server application; finite HOME/AWAY playback and actual listening, shared-radio/control/display behavior, volume latency and repeated recovery effectiveness.341ms queued audio is capacity, not proof of uninterrupted playback.

## Parent deployment and old-server PCM observation (2026-11-08)

The parent implementation worker subsequently applied the LCD artifact above to
serial E661640843239D21. picotool verification completed at100% with OK and
returned the device to application mode. Previous LCD artifact and manifest were
preserved in `/home/tane/picomado-backups/20261108-pre-sbc-firmware`.
Flash log: `/tmp/picomado-sbc-firmware-flash.log`; passive observation:
`/tmp/picomado-power-observe-20261108-115245`. The device reported
`audio-sbc-v1`, online, AWAY EEX1/TCP READY, Bluetooth connected, and matching
received/rendered frame IDs. The deployed server remained the previous PCM-only
binary; these observations do not establish an SBC handshake or SBC playback.

A user-started PCM stream at11:54:04 JST failed at11:54:05 with accepted14464,
consumed13824 and server-sent17920 canonical frames. USB counters show three
rebuffer episodes,335ms cumulative wait and114ms maximum, followed by exhausted
budget. A later authorized finite trial at volume30 reproduced accepted14464 /
consumed13824 and three episodes totaling355ms (maximum126ms). Evidence:
`/tmp/picomado-sbc-pcm-compat-20261108-115856/{usb.log,state.json}`. The trial was
explicitly paused afterward and original volume36 restored. The remaining640
frames cannot fill the normal1152-frame RTP packet. Rebuffer does operate and
recover temporarily; repeated PCM starvation still exhausts the bounded budget.
This is not continuous-playback acceptance. Host-side SBC sender integration and
its actual negotiated end-to-end playback remain pending. Old-server UI still
shows playing after the terminal device failure; the separate failure-feedback
change is under regression and is not yet deployed.
