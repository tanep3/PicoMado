# BT-AUDIO-20261107 / MUSIC-PLAYER-20261107 follow-up evidence

## Applied and observed

The earlier pipeline LCD build b85e8d985312b12c3c648894075b7a33b28a50ff91b8b11a87ba097968ac2b34 and server 5ee400c740f00f5205d9c83ba1112a621c2045066fbed3b79debfac103790059 were the applied artifacts before the later authorized update recorded below. A three-second tone completed (144000 frames, failed=0) and the user confirmed hearing it clearly. A following uploaded-music run sent 1331 packets / 1533312 frames before intentional pause at about32 seconds. These successes are limited trials.

Subsequent user playback failed in a later connection generation: PCM and negotiated SBC both48000Hz, full9-frame packets, about2944 frames still buffered, but can-send wait increased from about0.9ms/packet in the successful tone to18.5ms/packet. A250ms lateness guard stopped the stream. Connection role remains a hypothesis, not a confirmed cause.

The user reported unexpectedly loud music at night. The synthetic quiet tone did not validate real-track loudness. Playback was paused. No further automated sound, hardware update or service deployment is authorized in this follow-up without explicit permission.

## Implemented and host-tested before deployment

- Per-device digital volume0–100, initially0/muted for old and new settings; squared gain so10 means1% amplitude,100 is unity. Original assets are unchanged. Existing buffered audio and speaker-owned announcements are not immediately controlled by PCM mute.
- Live volume updates preserve radio commands, connection generation and audio-stream identity. Invalid values and stale sessions fail closed.
- Physical music Sub input now follows session-checked PluginInput and a same-device PluginDataChanged candidate. Normal Up/Down retain channel selection. End-of-track replay needs one press.
- An incoming local-master request matches the official SDK source demo, with actual role/mode diagnostics. Candidate LCD hash a053c4cf83f2f716711608b8fbad32affa0f106fd9765a1311776626ded88186. No audible effectiveness claim.
- LBS volume-button behavior is not established. Current AVRCP supports playback/navigation operations, not volume or absolute-volume synchronization.

## Host checks

Volume/domain/application4 tests, PCM5 standalone tests, actual transport sink mute/live-volume test1, and application music input integration7 tests passed. Firmware host suite, pipeline structural regression, LCD/E-ink cross-builds, UI JavaScript syntax and generated observation drift check passed.

The first volume all-features library run had373 passes,1 timeout failure and1 ignored. The same compiled failing loopback test passed when isolated; no assertion/timeout was relaxed. The final combined all-features serial run passed: 374 library, 2 server, 7 local-music input integration and 4 official-JMA tests, total 387 passed / 0 failed / 2 intentionally ignored. Log: `/tmp/picomado-final-volume-music-all.log`. The previously failing loopback test passed both isolated and in this final full run, without relaxed assertions or timeouts. Independent review, actual volume listening, AVRCP hardware behavior and role-policy improvement remain unverified.

## Source snapshot (before final combined regression)

- `service/src/audio_transport.rs`: `6ee6da983a1dd048c519be34a0c98fe81e5e93d0e0ead2c2ae92ff3729c628c4`
- `service/src/transport_audio.rs`: `0c8ecbf84eb5eacd73dda25774aeda21c78de35146474abe09a03a2d6fe35ae4`
- `service/src/bluetooth.rs`: `0e5975e8ee086f0d3a916e2a20acd059d235478c208ef81f92503ad88d67daa3`
- `service/src/application_bluetooth.rs`: `aa5390b7e3bd1f59dcf6a1db53ba6146e60ef22bf37dbfa19599aa1f515f825f`
- `service/src/local_music.rs`: `e2a24cfda9b376230de4cf118f3e916338376583b6fc8b6af395218ec716edfc`
- `service/tests/local_music_input.rs`: `a987d012c5aa06fad1304666f194aaf1504c263190bf21df9ba8e848bc09bd46`
- `firmware/src/bluetooth_runtime.c`: `c329a79637a2bf4961194d4bfe23340f232deeb4bac4936ed49e08365912804c`
- `firmware/src/bluetooth_media.inc`: `4c29b7f74e12fea3c01d4462496f542b6f0ccd8008a7f3e8661bb59a16899744`

## Deployment / rollback boundary

The old DesiredBluetooth uses `deny_unknown_fields`. Once the new binary persists `volume` (including default0), the old binary cannot deserialize StoredState and startup fails. Stored schema version remains2, so this is not a supported binary-only downgrade. Before any authorized application, save a fresh consistent SQLite backup using its backup API (or a stopped-service snapshot) together with the current binary. Preserve later user changes when planning a later rollback; do not blindly overwrite a newer database. The old binary also lacks attenuation, so rollback does not authorize audible playback. This was the pre-deployment boundary. The authorized deployment below now writes new-format volume state; use its paired backup for any rollback.

## Authorized deployment, 2026-11-08 00:40–00:43 JST

The user explicitly approved updating the server and LCD after being told LBS could announce its connection. No song or test tone was played. Consistent SQLite backup via the backup API passed integrity_check, with matching old server binary/config and uploaded source files saved under `/home/tane/picomado-backups/20261108-pre-volume-003640`. Source/cache SHA-256 values were recorded and verified unchanged after application.

- Server applied SHA-256 `0742d7b512bdd1145aacc3e738d97b26f044869f5950f9a9c2b4bae59a79c27e`; service active and updated WebUI served.
- LCD FlashID `E661640843239D21` matched the registered `PicoMado-lcd1`; candidate UF2 `a053c4cf83f2f716711608b8fbad32affa0f106fd9765a1311776626ded88186` flashed and verified100%, then booted in fresh session2469. The experimental HFP device was untouched.
- All three devices online, all volume values0, music playing=false. Core settings, enabled plugin lists and BT target/enabled intent preserved. No audio_start_sent appeared after the server restart.
- Initial LBS auto-reconnect timed out waiting for A2DP discovery. One bounded disable/re-enable of the saved LBS connection (volume remained0) succeeded; final BT revision34, connected. This is not proof that automatic reconnect is fixed.
- Actual reduced-volume playback, AVRCP physical-button behavior and the role policy's effect on audio timing remain unverified. Independent review/acceptance is still the parent's responsibility.

## DISCOVER stall: diagnostic and bounded recovery design (not implemented or deployed)

This extends BT-AUDIO-20261107. Evidence is `/tmp/picomado-volume-role-connect.log`: generation4 reports status11 (ACL already exists), then successful signaling on CID2, followed repeatedly by `conn=7 source=3 initiator=9 outgoing=1 discover=0 configured=0`. In this SDK these mean signaling OPENED, A2DP waiting for SEP DISCOVER, initiator waiting for an answer, no postponed discovery, and no configured endpoint. The incoming ownership stage has completed. This does not establish missing bonding keys or distinguish a failed local send from an unanswered or mishandled response.

### Observation boundary and minimal project-side diagnostics

- SDK `classic/avdtp_initiator.c`, `avdtp_initiator_stream_config_subsm_run_signaling`, changes to W4_ANSWER before invoking DISCOVER send. Its helper returns `l2cap_send`, but this caller ignores that result. No DISCOVER-specific response timeout was found in this path. `avdtp_discover_stream_endpoints` rejects a new request while the initiator is not IDLE. Do not overwrite these private states or inject a duplicate DISCOVER while waiting.
- With the SDK unchanged, project code can observe arrival at the send path and continued response-wait state, but **cannot directly obtain the DISCOVER send return value**. Logs must say `discover_wait`, not `send_success` or `peer_did_not_reply`. An observed state transition proves progress, not a received packet's provenance. The existing source demo uses this same SDK machinery; it does not supply an independent recovery contract.
- Record generation, owned CID, signaling channel, source/initiator state, elapsed unchanged time, recovery attempt, and synchronous return from project-issued establish/disconnect calls. Emit on state/action changes and a bounded low-frequency wait summary from the main loop through the event queue; no printing in BT callbacks. No PCM, SBC contents, authentication material, or keys.
- Actual DISCOVER send-result instrumentation would require a separately reviewed SDK hook or integration seam. It is deferred, not silently approximated by a state snapshot. Future HCI traces require explicit authorization and narrow capture/retention: they can contain addresses, link keys, authentication exchanges, and private traffic. Never publish raw traces or include secrets in normal logs. No trace capture is authorized by this design.

### Recovery state machine and ownership

A small pure recovery policy owns one current command generation, observed CID, unchanged-state timestamp, phase, and attempt count. Runtime remains responsible for SDK effects. It is activated only by an explicit connect command to the saved/selected peer; it is not a general background reconnect loop.

1. **Observe:** For the current generation, wanted target and owned nonzero CID, detect source WAIT_DISCOVER plus initiator W4_ANSWER. Any meaningful progress or CID change restarts the five-second unchanged-state measurement. A successful stream establishment disarms recovery. Other setup phases are not retried by this policy.
2. **Disconnect pending:** After five unchanged seconds, if fewer than two recovery attempts have been used, increment the count, issue a normal A2DP disconnect, and retain the same command generation/target. Never call establish while that owned link has not completed teardown. Drain the associated AVRCP owner too if one exists. A synchronous disconnect failure is an explicit failure, not a completed teardown.
3. **Cooldown:** Only after matching release callbacks clear the owned A2DP and AVRCP CIDs, wait at least500ms, then attempt establishment of that exact target. Do not treat an existing ACL status or request acceptance as a successful connection. The current bounded ACL-collision handling remains distinct and cannot reset the recovery count.
4. **Failure:** Stop recovery after two attempts, after three seconds without confirmed teardown, or on an unrecoverable SDK effect error. Emit one final failure. Keep unresolved link ownership so subsequent explicit OFF can still clean it up. Do not free or fabricate SDK connection objects.
5. **Cancellation:** Explicit OFF, changed target, superseding generation, session loss, or overall command timeout cancels the recovery timer and pending effects. A late callback for an old CID/generation cannot reconnect or clear a newer owner. No retry survives a new command.

The existing firmware25-second total command budget and server command expiry are unchanged. Internal retries use the remaining budget; none starts a new user-visible command or resets its receive time. Internal teardown must not emit the normal user-facing DISCONNECTED/STOPPED completion, which would prematurely fail the upper-level command or create another generation. Success remains the actual matching STREAM_ESTABLISHED event. If the owning command expires during any phase, normal cancellation wins immediately.

### Safety and unresolved behavior

Every additional link attempt may make LBS play its own connection announcement even with PCM volume0. It can also compete with a user's manual pairing, phone connection, OFF action, or target change. Therefore this policy is bounded, cancellable, scoped to the explicit connect intent, and must not be deployed or exercised on the current device without user approval that includes possible speaker announcements. It neither changes volume nor starts audio. The current live connection and muted/stopped state must remain untouched during host work.

The one successful manual OFF→ON is evidence that fresh teardown/establishment can recover this occurrence, not proof of the root cause or guaranteed retry effectiveness. Peer nonresponse, local send failure, and response handling remain separate hypotheses. Unrelated runtime/CPU and audio timing findings remain valid and are not explained away by this recovery policy.

### Required host verification and later authorized validation

Before code integration, freeze this design in the existing change's specification commit. Then test the pure policy with fake clock/effects: no retry before five seconds; progress resets only the stall timer, not the attempt budget; same-CID stall disconnects; no establish before both matching releases;500ms cooldown; maximum two attempts; three-second teardown failure; stale callback ignored; OFF/session loss/changed target/new generation cancel in every phase; final failure once; wraparound clock arithmetic; actual stream success disarms recovery. Adapter tests must verify asynchronous teardown, synchronous effect errors, no user DISCONNECTED during recovery, and unchanged25-second outer timeout.

For diagnosis, verify main-loop-only bounded metadata logging and that a send-path state is never reported as a successful send. Host tests cannot prove RF delivery or LBS behavior. Later explicitly authorized device validation should compare restart/direct connect, equal-duration wait without discovery, discovery then connect, and manual OFF→ON, while tracking role/mode and actual setup progress. No audible playback is needed to test setup, but connection announcements remain possible.

Recovery integration clarification: before the new establishment effect, confirm not only both matching release callbacks but also that the SDK's AVDTP owner for the selected peer is absent. A lingering owner is not silently reused by DISCOVER-stall recovery; remain within the same teardown deadline or fail. Existing ACL-collision retry and DISCOVER recovery must cancel one another's pending timers and share one bounded retry budget, rather than multiplying nested retry loops. Initial establishment is not counted as a retry; no more than two subsequent establishment attempts are allowed across these paths. PM_BT_CONNECTED permanently disarms setup recovery for that command; an ordinary media suspend cannot rearm it.
