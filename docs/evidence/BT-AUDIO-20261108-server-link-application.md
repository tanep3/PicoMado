# BT-AUDIO-20261107: Welcome MAC hint operational trial

## Applied artifacts and protection

On 2026-11-08 at 18:00 JST (09:00 UTC), the approved HOME/HOME2
session-scoped hint service was applied. Tested service source is `c7bd652`,
with final regression evidence `96cd9b6`: 424 tests passed, zero failed,
three intentionally ignored; the actual Linux resolver additionally passed.
Production release used `caricature-retention-delete`, `--locked`, and `-j1`.
The 170 tracked service inputs were unchanged throughout the 4m47s build.

Deployed server SHA256:
`6510486f5a3e6b0eeb27e9f54a8bfe9ffd962978fe435c7cb6cfd3d3ae696dcd`.
The current streaming-compatible predecessor, config, consistent SQLite backup
(integrity_check=ok), state snapshots and hashes are retained privately under
`/home/tane/picomado-backups/20261108-pre-server-link-hint-180015`.
The running service recovered active. All three devices' desired configuration,
Bluetooth volume/mute/target/enabled values were unchanged. Music was not played.
Music revision/import discovery bookkeeping changed during restart; device
playback and asset data were preserved. Do not roll back to a pre-streaming
server without regenerating its old PCM dependencies.

E-ink_1 serial `E6614864D34F8E22` runs the staged opt-in firmware from
`86775cd`, SHA256
`e06fdfbd9cfe59cec02c54cfcca71a607d9127082cfd310c96c17b837f670262`.
The original normal UF2 and private configuration remain preserved. Only this
firmware is opted in: default builds remain OFF, LCD and E-ink_0 have not been
updated to advertise this capability. Their old protocol compatibility was
observed online after server replacement. No SSH, router access, AWAY test,
push, audio playback or permanent host-network configuration was performed.

## Three bounded observations

Private immutable logs, arrival timing and five-second state snapshots:
`/home/tane/picomado-staging/20261108-server-link-c7bd652/live/`.
`summary.json` contains file SHA256 values and extracted TCP snapshots.
Directory timestamps below are JST, not UTC.

1. `20261108-180001`: captured the server replacement and reconnect. On conn2,
   Welcome hint=2 and installed result=0 selected `192.168.0.120` /
   `2c:cf:67:11:72:f3`. Frame2440274 reached server Rendered. All nine conn2
   snapshots had ACK equal to send end, no queued/unacknowledged data, and no
   TCP error. The old long-lived firmware trace had cumulative dropped=152;
   this is not a complete zero-loss timing trace and is not presented as one.
2. `20261108-180258`: same candidate flashed/verified for cold application
   startup. Wi-Fi first reported BADAUTH and retried. Welcome at MCU25.733s
   installed the correct MAC on conn1. A snapshot first showed ACK=end7199,
   with no unsent/unacknowledged data. Frame2441318 completed at32.749981s and
   its server Rendered was visible in the35-second sample. **Subsequently the
   same conn1 stalled at ACK7199 while send end reached7679 then7777, despite
   every observed ARP MAC still being the correct server MAC.** TCP error -13
   occurred and mapping cleanup succeeded at71.417s. Later SYN attempts did
   not finish within150s. Dropped=0. This is a counterexample to claiming that
   correct Welcome-time MAC installation eliminates every startup failure.
3. `20261108-180657`: identical firmware and server, another verified startup.
   No BADAUTH appeared. Welcome installed the same correct MAC at5.745s,
   frame2443942 completed at12.429645s. All ten valid TCP snapshots over150s
   stayed on conn1 with ACK=end (7375 through8463), unsent=0 and unacked=0.
   No TCP errors or log drops. This trial supports correct application and
   successful operation in this condition, not universal reliability.

Final API read showed E-ink_0, E-ink_1 and LCD online; all music playback false.
E-ink_1 desired volume38/unmuted was retained, but its Bluetooth observed state
was failed (also present before this change). This silent network trial does
not establish Bluetooth reconnection or audible playback success.

## Interpreting the remaining failure

Code/SDK read-only inspection distinguishes hypotheses from findings:

- if=2 in the failed trial is lwIP netif num+1 after STA removal/re-addition;
  CYW43 output still uses the same STA interface. Earlier ordinary failures
  also happened at if=1, so re-JOIN is not a necessary cause of all failures.
- Static insertion updates the existing ARP entry and flushes pending ARP
  packets using the new MAC. TCP retransmission constructs Ethernet output
  using the current ARP entry. A forever-cached old-MAC TCP segment is not
  supported by that code or by the fully-ACKed snapshot after installation.
- `remove reason=link_scope` follows tcp_error setting PCB NULL/FAILED here;
  it is not independent proof of a Wi-Fi link change.
- The earlier before-TCP static-MAC A/B/A success remains valid evidence,
  but differs in installation timing and startup conditions. The present
  counterexample prevents interpreting that single earlier success as proof
  that competing ARP replies are the sole cause.

Further useful comparisons are repeated starts with explicit JOIN/re-JOIN
stratification and paired transport observations distinguishing Pico data not
reaching the server, server ACK emission, and return ACK loss. Existing USB
logs alone cannot locate those losses. No new privileged packet capture or
network configuration was started to fill that gap. Physical stability and
final independent approval remain open; no stage is self-promoted to verified.

## Third startup (18:11 JST)

An additional same-artifact trial `20261108-181132` did not report BADAUTH.
Hint installation succeeded at5.836s and physical render report was generated
at12.868546s. The first valid TCP snapshot nevertheless showed send end7375
with ACK7199 and one unacknowledged segment, while ARP still named the correct
server MAC. The next snapshot recovered to ACK=end7679 on the same conn1;
the remaining eight snapshots stayed caught up, with no TCP error or trace
loss through150s. Server Rendered2447237 appeared in the30-second state sample.
Thus the remaining transient stall also occurs without the re-JOIN condition.

Read-only `ss -tin` observations of the existing E-ink_1 socket
(.120:16153/.174:49793) were saved in `ss-repeat3.jsonl` beside the retained logs.
Received bytes progressed from689 at the initial read to1267 after recovery;
`rcv_ooopack=2` appeared. This supports out-of-order reception during this
interval, but does not identify where the missing packet or ACK was lost.
No packet capture, network configuration change, or sound was introduced.
The currently applied firmware remains the opt-in Welcome-hint version.

## Repeated before-TCP control and restoration

The previously authorized hardcoded, HOME-only diagnostic was repeated against
this same new service, then the approved Welcome-hint firmware was restored.
`20261108-181500` used the original fixed UF2 SHA256
`d8d78feccb40965e8188e4f21e13921abee43d33a6abaa22cf065bd86a0be204`.
It reported Wi-Fi FAIL then successful re-JOIN, installed before TCP at25.591s,
and showed all eight valid snapshots ACK=end, unsent/unacked=0 over150s.
No TCP error or trace loss occurred. Thus a re-JOIN can also precede success.

Restoration `20261108-181756` reverified the current Welcome UF2 `e06fdfbd…`.
It installed the correct MAC at11.280s and rendered frame2451898 at17.944792s.
ACK first reached7199, then7375 but stopped there while send end advanced7681,
7779 and7877. TCP error -13 occurred at64.970s and cleanup succeeded. The
100-second observation ended during connection retry. This is another failure
with correct observed MAC, not a successful reliability acceptance.

A subsequent read-only host check showed the .174 neighbor as the device MAC
`28:cd:c1:0b:f5:55` on eth0, and source-.120 route lookup was direct eth0.
The old server socket had bytes_received865 and bytes_acked5036. This does not
prove the earlier complete packet path, but the host neighbor was not the
router MAC at this read. Neither the neighbor nor routing settings were changed.
The result strengthens the case for testing a previously learned mapping before
TCP establishment; it still does not establish the causal network mechanism.
The temporary hardcoded image is no longer active. Current firmware is the
Welcome implementation, and persistent learned-MAC storage is not implemented
or applied by these trials.
