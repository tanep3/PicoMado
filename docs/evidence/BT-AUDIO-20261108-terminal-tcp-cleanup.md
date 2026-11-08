# BT-AUDIO-20261107: terminal-send resource cleanup

Specification precedes implementation: `843c972`. This is recovery from a
secondary resource-pressure mechanism, not correction of the initial ARP/TCP
stall. The old/new firmware comparison and original hypothesis remain in
[cross-device startup](BT-AUDIO-20261108-cross-device-startup.md) and
[old/new comparison](BT-AUDIO-20261108-old-new-firmware-comparison.md).
No physical device, network, USB, SSH, service, settings, or push operation was
performed by this implementation worker.

## Actual SDK allocator/PCB reproduction

Unmodified SDK lwIP revision `77dcd25a72509eb83f72b033d219b1d40cd8eb95`;
`mem.c`, `memp.c`, `pbuf.c`, `tcp.c`, `tcp_out.c`, checksum and support sources
are compiled directly. The real firmware lwipopts is used. Only IP output,
routing and single-thread lock/platform services are host fixtures. Established
PCBs are registered directly; no handshake, packet ingress/ACK, retransmission
timing, Wi-Fi, ARP or RF delivery is simulated. Output returns OK while no ACK
is delivered, intentionally isolating resource retention.

On this64-bit host, PBUF_RAM costs1548 heap bytes for each98-byte notification
when single-pbuf output reserves1460 payload bytes. Ten output/unacknowledged
writes use15480/16000 heap bytes and10/32 segment-pool slots. The eleventh write
returns ERR_MEM, sndbuf10700 and queue10. Heap allocation errors increment;
segment-allocation errors remain zero. This differentiates heap exhaustion
from both the byte and segment/queue limits in this fixture.

- Graceful tcp_close returns ERR_OK, adds a FIN, and leaves15568 heap bytes and
  eleven segments in FIN_WAIT_1. A newly allocated established PCB has queue0
  and sndbuf11680, but its first164-byte tcp_write returns ERR_MEM.
- Explicit tcp_abort releases the old heap, segment pool and PCB immediately.
  The new164-byte write succeeds. Final fixture cleanup returns all three
  resource counts to zero.
- Reserving an additional480 bytes before close makes FIN allocation fail.
  tcp_close still returns ERR_OK, retains the old PCB/queues and sets
  TF_CLOSEPEND. Therefore abort-on-tcp_close-error alone cannot guarantee release.
- Control: when one receive byte remains uncredited, tcp_close sends RST and
  purges the old queues. Graceful close does not always retain resources; the
  observed retaining case requires fully returned receive credit.
- Control: only in a temporary options wrapper, SINGLE_PBUF=0 permits eleven
  separate98-byte output/unacknowledged writes using2068 bytes. The actual
  SINGLE_PBUF=1 control fails after ten at15480 bytes. Production options and
  SDK were not changed; this comparison is not a proposed radio-driver fix.
- A synthetic331+9x98-byte sequence reproduces sndbuf10467/queue10 at ERR_MEM,
  followed by the empty-new-PCB164-byte failure. Its1213-byte total matches the
  observed byte counter, but this is not a reconstruction of the device's
  individual notification sizes.
- Actual tcp_abort invokes an attached error callback after freeing its PCB.
  Clearing arg/recv/error callbacks first suppresses that notification.

These are code-supported, measured host reproductions of the secondary
amplification, not observed device heap counters. Host pbuf/segment sizes are
24/32 bytes versus target16/16, verified with an ARM compiler-only layout
fixture. The target layout implies a different per-allocation cost; the host
1548/15568 figures are not asserted as device values. The native-memory fixture
passes ASan/UBSan with alignment checks disabled because production4-byte heap
alignment is below this host's8-byte pointer alignment. LeakSanitizer is disabled
under the known ptrace restriction. A temporary8-byte-alignment host control
also passes full ASan/UBSan (no alignment exclusion): ten writes use15520
bytes, graceful close retains15608 and rejects the next164-byte write; abort
releases all resources and the next write succeeds. Thus the mechanism also
reproduces with pointer-correct host alignment. Production adapter fixtures use full
ASan/UBSan, also with LeakSanitizer disabled.

## Bounded ownership change

`net.c` records a current-connection terminal-send latch for mandatory sndbuf
shortage, terminal tcp_write failure, or the existing matching send deadline.
The normal main-side locked poll/checkpoint/close/begin boundary chooses abort
only for this latch. The owner clears pcb, removes callbacks, then retires it;
the latch clears with that connection. Already-SDK-freed PCB=NULL is not freed
again. The connection's new allocation occurs after cleanup.

No abort occurs in the connected or receive callbacks; a failed handshake send
returns ERR_OK with the PCB still live, then main cleanup retires it after the
callback unwinds. No new ERR_ABRT return is introduced. Existing callback rules
still apply: an actual callback-local abort would require ERR_ABRT and no later
PCB access. Retryable WAIT, copied bytes despite tcp_output error, optional
Close shortage, normal EOF/drain and display salvage retain their meanings.
Queued old notifications may be discarded after terminal failure, intentionally.
This does not repair the original network path or establish successful delivery.

## Validation and limits

- Updated `run_backpressure_sdk_tests.py`: eight executable cases, native and
  ASan/UBSan SDK-write, production net, production cleanup and Bluetooth media;
  all passed. Includes callback unwinding/detachment, SDK-owned free, repeated
  cleanup, explicit close/begin (abort before allocation), stale timeout/receive,
  fresh connection, and optional Close byte/heap shortage.
- Full `firmware/tests/run_host_tests.sh` with pinned PICO_SDK_PATH: two runs exit0, including the final test-strengthened tree.
  Includes52 existing EOF/display-salvage groups and their boundary matrices,
  SBC/backpressure, Bluetooth rebuffer/recovery, LCD and E-ink host fixtures.
- Actual ARM net.c translation-unit compile: LCD and E-ink_0, each with
  PM_BLUETOOTH_AUDIO=0 and1, four passes. Existing target flags were read, output
  objects went to the private diagnostic directory. No UF2 was built or flashed.
- Replacing terminal abort with the historical graceful close in a temporary
  source copy is rejected by the new production cleanup fixture.
- Optional project gate: NOT passed. Fresh-out structural suite ran49 tests;
  the already recorded `test_actual_firmware_build_sdk_flags_and_offline_flash`
  missing-approval expectation still fails (expected nonzero, observed0).
  This change does not modify or weaken that route/test. Initial default-out
  attempt refused an existing immutable receipt; the fresh-out run is the
  substantive result. `git diff --check` passes.

Private reproduction source, build commands, logs and hash manifest:
`/home/tane/picomado-staging/tcp-recovery-memory-20261108/`.
Working logs: `/tmp/picomado-tcp-recovery-memory-20261108/`.
Core files are `probe.c`, `build.sh`, `close.log`, `abort.log`,
`close-no-fin.log`, `close-held-rx.log`, `mixed-close.log`, `single-on.log`,
`single-off.log`, `callback-control.c`, `callback-control.log`,
`final-backpressure/summary.json`, `all-host-final.log`, `target-tu-results.json`,
`graceful-mutation-test.log`, and `gate-fresh/structural.log`.
A separate read-only source/log review reported no blocking implementation issue
for net.c SHA256 `e8afcae8f960c311f84a48b4b17f489412b8805c28c8eec871cf08d413762fcc`.
The reviewer did not execute tests. Its two nonblocking suggestions, optional
Close sndbuf coverage and abort-before-successor-allocation assertions, were
added and the affected tests/full host suite rerun successfully.
Parent independent acceptance, deployment and hardware effectiveness remain open.
