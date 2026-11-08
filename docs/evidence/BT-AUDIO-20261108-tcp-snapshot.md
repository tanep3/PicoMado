# BT-AUDIO-20261107: bounded TCP status snapshot

Specification: eae9f33. Implementation is diagnostic only and has not been deployed or exercised on a device by this task.

net.c/net.h expose a lock-protected copy of current PCB state, byte positions (snd_lbb/snd_nxt/lastack), window/cwnd, unsent/unacked presence, retransmission/keepalive counters and signed rtime. A saturating boot-local connection ID increments at begin. Invalid snapshots after close have no stale PCB fields. Per-connection write/output success/failure counters saturate; last output result has an explicit valid flag. Queued writes and output returns are not acknowledged delivery.

main.c emits three bounded lines only in the existing USB-connected initial/15-second status path. No new feature flag, polling, packet, allocation, credential or audio payload logging is introduced. Existing send return semantics, timeout/recovery and standard INPUT controls are unchanged. The optional tcp_ticks/private-lwIP idle diagnostic was not introduced.

Tests extend the actual net.c fake-socket test across profiles: unconnected/connected/close/rebind, distinct byte positions, state/retransmission/keepalive values, negative rtime, write success/output failure, write failure without output, signed error, saturating counts, reset on new connection, and snapshot PCB equality/no output side effect. Main fixture stubs return invalid snapshots. The first suite attempt found missing fixture API symbols; the second found test counter contamination from added setup. Both were corrected in fixtures, not by changing production behavior or relaxing existing assertions.

Final complete firmware host suite exited0: `/tmp/picomado-tcp-snapshot-host-final.log`. Earlier successful suite: `/tmp/picomado-tcp-snapshot-host.log`. Release builds exited0: `/tmp/picomado-tcp-snapshot-build.log`. Fresh build directories preserve prior deployed artifacts. The initial LCD build used default BT-off and passed as a compatibility check; the final LCD was explicitly reconfigured BT-on and rebuilt.

Final artifacts:
- `/tmp/picomado-tcp-snapshot-lcd/PicoMado-lcd1.uf2`: SHA256 `b0d15de3699bd9abe6a508a761209bb58d5af987f905f118b50b901e2745a330`, PM_BLUETOOTH_AUDIO=ON, PM_BT_HCI_DRAIN_LIMIT=0.
- `/tmp/picomado-tcp-snapshot-eink/E-ink_0.uf2`: SHA256 `30fd8dc31e73e6090d2b1f629ccb5cdbb2d72b212ec4e5c3130597eb1fdf4214`, PM_BLUETOOTH_AUDIO=OFF, PM_BT_HCI_DRAIN_LIMIT=0.

Both use pico_w and Release. SDK/BTstack worktrees remained clean and git diff whitespace check passed. No USB, radio, service, SSH, playback, push or deployment occurred. These checks validate diagnostic construction, not the intermittent fault's root cause or independent acceptance.
