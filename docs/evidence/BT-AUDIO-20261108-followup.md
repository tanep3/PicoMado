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

## DISCOVER recovery host implementation, not deployed (specification2896889)

Implemented the pure recovery policy and project-owned SDK effect coordinator. DISCOVER response-wait stagnation is limited to five seconds; recovery reserves at most two shared attempts across ACL-collision retry and DISCOVER recovery. Reservations cancelled before execution still consume that command's retry allowance conservatively. Both matching release callbacks and disappearance of the selected peer's SDK AVDTP owner are required before a500ms cooldown and new establishment. Teardown and cooldown remain within the three-second teardown budget. Explicit OFF, session cancellation, new generation/target, effect failure, or expiry cancels pending work. Existing-owner reuse is confined to the pre-existing collision path, not DISCOVER recovery.

The control adapter now passes its original local receipt timestamp through `pm_bt_runtime_connect_at`, so recovery does not start a fresh25-second clock after initialization or teardown. Deadline checks precede success acceptance and retry effects. The server's issue-time expiry is not changed or mapped to the MCU's clock: the existing host30-second versus firmware25-second budget can still diverge after more than five seconds of delivery delay. This residual boundary is not solved or extended here. Session invalidation and explicit cancellation remain required safeguards.

Internal teardown suppresses user-facing DISCONNECTED. A late STREAM_ESTABLISHED during teardown/cooldown is disconnected rather than accepted, and an SDK state snapshot reporting OPENED cannot disarm recovery in those phases. Actual matching stream success outside recovery teardown disarms setup monitoring; media suspension does not rearm it. Consistent failure handling suppresses repeated terminal failure for late callbacks. SDK source is unchanged; normal diagnostics report `discover_wait ... evidence=state_only`, not a guessed DISCOVER send result.

The eight-entry event queue now admits at most four ordinary diagnostic/candidate/input events, reserving space for terminal/control results. New terminal events evict nonterminal entries first at saturation. If all eight entries are terminal, the oldest terminal is evicted to retain the newest state, with a saturating drop counter. This is explicit bounded-state retention, not lossless logging; current terminal results cannot be crowded out by diagnostic chatter.

Host verification completed:

- Full `firmware/tests/run_host_tests.sh` passed; log `/tmp/picomado-recovery-host-final.log` (final release-notification fix included). The new recovery, actual effect-coordinator, and actual stream-completion-handler tests are part of this suite. Final control fixture re-run also passed.
- Pure policy cases include five-second threshold, meaningful progress, CID change, both cancellation and changed target/generation, clock rollover, shared two-attempt budget in both recovery/collision orders,25-second deadline including success at the deadline, terminal failure once, and diagnostic/terminal queue pressure.
- Fake-SDK tests execute the same included effect coordinator and release/completion handlers used by firmware: both release orders, missing SDK-owner cleanup, duplicate/old release CID, cooldown boundaries, cancellation just before retry, synchronous disconnect/establish failure, internal notification suppression, and late success during teardown/cooldown or after failure. They do not reproduce controller scheduling or RF reception. The smaller ownership test and policy tests are not represented as a full BTstack simulation.
- New three tests passed AddressSanitizer and UndefinedBehaviorSanitizer. LeakSanitizer was disabled because the execution environment does not support it under ptrace; no leak-check claim is made.
- LCD and E-ink cross-builds passed: `/tmp/picomado-recovery-lcd-build.log`, `/tmp/picomado-recovery-eink-build.log`. LCD text553544/BSS235760; E-ink text540376/BSS166248. Candidate LCD UF2 SHA-256 `53ada45454ff8d7daacd40c2220e430dafdf2ba09839414fb07b28acc6b5215b`; candidate E-ink UF2 SHA-256 `2f38c56ff733a4dec9ca6e305906afb0a81ca622729755b8e9ab5acd587fdc98`.
- `git diff --check` passed. Read-only `git status --short` for `/home/tane/dev/PicoW/pico-sdk` and its `lib/btstack` repository returned empty output. No SDK edits, device access, USB operations, SSH, service actions, commits, or pushes were performed in this implementation task.

These candidate artifacts have **not** replaced the production firmware or server. Connection recovery effectiveness, speaker announcements, competing phone/manual operations, and reconnect sound/timing remain for a separately authorized hardware trial. Do not reconnect the currently playing device to validate this host-only change.

### Separate user-led listening observation of the already deployed version

During this host work, the parent reported the user's own listening feedback: 「浜崎がクリアに聞こえる」 and 「今のところ大丈夫（テンポ/途切れ）」. This is a limited favorable observation of the previously deployed version, not of this unflashed recovery implementation. It does not establish the cause of the earlier intermittent stutter or prove that requesting the master role fixed it. AVRCP physical-button behavior and automatic reconnect robustness remain unverified. No agent-triggered audio or reconnection was performed for this observation.

## Independent mute setting: implementation and rollback boundary

The slider/checkbox change preserves legacy effective volume: missing muted becomes true only at volume0, otherwise false; new settings default to0/true. Muting retains the selected volume and changing the slider does not unmute. Host-only validation is in progress; no production operation or audible test is performed by this work.

After this version persists muted, the currently deployed strict DesiredBluetooth reader will reject that unknown field. Binary-only rollback is again unsupported. Before an authorized deployment, create a fresh consistent SQLite backup and matching currently deployed binary, rather than reusing the earlier pre-volume backup. A later rollback must also account for user changes made since that backup.

Final integration safeguard: after a setup failure or explicit cancellation, a later signaling release no longer emits DISCONNECTED, preventing a terminal Failed state from being converted to Idle and triggering host reconciliation. Normal unexpected disconnection while wanted remains observable, and explicit OFF still completes through STOPPED. The actual release-handler fake-SDK tests cover all three paths. The final full host suite and both cross-builds above include this fix. The SDK async-context clock uses `to_ms_since_boot(get_absolute_time())`, matching the control adapter's receipt timestamp used by `connect_at`.

Mute/slider host results: targeted migration/application/API tests4 passed (`/tmp/picomado-mute-targeted.log`). The final all-features serial library run had376 passed,1 failed,1 ignored (`/tmp/picomado-mute-all.log`). The sole failure was the existing loopback host test at host.rs245: the12-second shutdown-task wait expired, not a PCM/mute assertion. The same compiled library binary's exact isolated test passed in44.75seconds (`/tmp/picomado-mute-host-isolated.log`); no timeout/assertion was relaxed. This is not a clean full-suite pass.

All mute/domain/API tests and the actual recording-sink test passed in that full run. The sink confirms default silence, explicit unmute to saved10, mute retaining10, and unmute restoring sample100 from source10000, with one AudioStart and unchanged stream/source. Subsequent already-compiled server2, local-music integration7 and official-JMA4 tests passed; live HTTPS1 was intentionally ignored (`/tmp/picomado-mute-integration.log`). UI JavaScript syntax and git diff whitespace checks passed. These results do not establish device listening behavior or deployment acceptance. No deployment, service restart, device operation, SSH or playback was performed for this change.

## AVRCP dedicated media input firmware, host-only (specification 6f02af5)

The firmware maps PLAY/PAUSE/FORWARD/BACKWARD to independent media operations, not GPIO Sub/Down/Up. Hello advertises `media-input-v1`; only Welcome integer `media_input_version:1` for the current incarnation/session enables kind27. The main loop sends `{type:media_input,data:{incarnation,session_epoch,operation}}`, with operation play/pause/next/previous. Unsupported negotiation, non-ACTIVE state and stale identities cannot send. Failed sends are not replayed or replaced with GPIO actions. Existing physical input code is unchanged; the server owns selected-local-music/Normal checks and the documented old-Bluetooth-firmware toggle compatibility rule.

Changed firmware files: `src/bluetooth_input.h`, `src/bluetooth_control.inc`, `src/main.c`, `tests/bluetooth_input_test.c`, `tests/bluetooth_control_test.c`. The new value type prevents accidentally treating AVRCP operations as GPIO operations. Press/release, duplicate suppression, old connection generation, all four mappings, unknown opcode, absent/unknown negotiation, session mismatch and send-failure non-replay passed pure/actual-control-adapter fixtures. No runtime SBC/radio/volume changes were made in this step. Service wire decoding/generated identity classification are a separate coordinated implementation and require their own tests.

- Full host suite: `/tmp/picomado-media-input-host.log`, exit0.
- LCD `-j1` crossbuild: `/tmp/picomado-media-input-lcd-build.log`, exit0. UF2 SHA256 `0277c4deabf5275783ffc934dd13e5b07546aa283383f0ace09c0b4c9664a661`; ELF text554144, BSS235784 bytes.
- E-ink `-j1` crossbuild: `/tmp/picomado-media-input-eink-build.log`, exit0. UF2 SHA256 `6b4f82fbdf4e54cb023a981115c95137bdcf25cfb09e9c17bd90e9eb615eb368`; ELF text540800, BSS166272 bytes.
- `git diff --check` passed. SDK and SDK/lib/btstack `git status --short` are empty; SDK remains unmodified.

Both artifacts are unflashed candidates. The running production firmware/service and playback were untouched. No hardware, USB, SSH, service operations, sound generation, commit or push was performed by this firmware task. LBS button opcode behavior, current-channel real effects and mixed-version deployment remain unverified on hardware and need separately authorized acceptance.

### Final generated-observation crossbuild binding

After the service MediaInput session-classification generation stabilized, both firmware targets were rebuilt sequentially with `-j1`; both exited0. Logs: `/tmp/picomado-media-input-final-lcd-build.log` and `/tmp/picomado-media-input-final-eink-build.log`. The generated-header dependency caused recompilation (including ack_probe.c); the UF2 hashes and ELF sizes remain exactly those recorded above. These are the single final unflashed media-input candidates, not an additional deployed version.

Build source binding: base HEAD `c4e823c5878e0512da4b30b47d9a90043d79c220`, plus firmware diff snapshot `/tmp/picomado-media-input-final-firmware.patch` SHA256 `40892cc2a758369d24e59711d5e2f19c593f9054fb554003dea9ec160c340459`. Generated `firmware/src/observation_values.h` SHA256 `d1ff2efc1c6951687e6210557e18cdfab0906a0c0056fb190de09179f98870fd`. SDK and BTstack worktrees were again clean. No device/service operation or commit occurred in this rebuild.

## Replacement six-operation AVRCP candidate (specification 13a03f5)

The prior four-operation candidate was never deployed and is superseded, not a second accepted control mode. Current kind27 operations are toggle_playback/channel_next/channel_previous/cancel/volume_up/volume_down. Both PLAY and PAUSE complete on release: below1800ms toggles, 1800–9999ms cancels; expired/missing/unmatched releases do nothing. NEXT/PREV and volume41/42 act once on first press. The firmware never changes mute or volume itself; service owns persistence, five-point clamping and mute preservation. Physical input code and PCM/SBC paths remain unchanged.

SDK callback receive timestamps determine holds, with uint32 wrap-safe differences and ten-second expiration. Duplicate holds do not refresh their timestamp. AVRCP-owner epochs prevent a replacement control connection from completing an old hold while A2DP generation survives. A dropped edge due to bounded queue pressure also advances the input epoch, so a subsequent release cannot turn missing input into a false long action. Runtime's old indefinite pressed array was removed; edge deduplication and expiry have one main-loop policy owner. Unknown opcodes and wrong/off remote owners remain ignored; SDP/absolute-volume behavior was not expanded.

Pure input tests cover both PLAY/PAUSE at0/1799/1800/9999/10000ms, duplicate/unmatched release, queue delay, stale event, missing release expiry, re-press, clock wrap, generation/session reset, all six opcodes and unknown codes. The actual main control fixture tests negotiated kind27 sends, short versus long operation, owner epoch reset, failed-send non-replay, stale identity and expiry. A new actual callback fixture includes `bluetooth_remote_input.inc` and tests six-opcode admission, timestamp/epoch propagation, wrong owner/OFF, bounded queue and lost-edge invalidation. The pure input fixture also passes ASan+UBSan (leak detection disabled for this environment; no leak-check claim).

Final full host suite `/tmp/picomado-media-sixop-final-host.log` exits0. An earlier in-progress rerun was discarded after editing the running test shell file shifted its read position; the final rerun used stable source and completed all scenarios without weakening assertions. Both sequential `-j1` crossbuilds exit0: `/tmp/picomado-media-sixop-final-lcd-build.log`, `/tmp/picomado-media-sixop-final-eink-build.log`.

- LCD UF2 SHA256 `396f8df5c486ed358eb2e088604ece88d37123e987ef70a253fe6896abd88b9d`; ELF text554736/BSS235752.
- E-ink UF2 SHA256 `17abf271fd00e959a2deef19859920682de199f6f6b01db0f69307b6f55e6da1`; ELF text541392/BSS166240.
- Source base `13a03f5fb990612644f6e19e2d098e5ca576b079`; changed firmware tracked/untracked content manifest `/tmp/picomado-media-sixop-source.sha256`, SHA256 `9262d546cee497d2b0974546d9acd598cf99c77b4ca95396dce2466404c45c54`. All entries reverified after build, including generated observation header and new callback/test files.
- `git diff --check` passes; SDK/BTstack worktrees remain clean.

These artifacts are unflashed and replace the previous four-op hashes as candidates. No service/device/USB/SSH/audio operation, commit or push was performed. LBS actual press/release/long-hold and volume notification behavior remain unverified; a local-only hardware volume change cannot be claimed as observed or synchronized. Production listening on the earlier deployed version is separate evidence, not acceptance of this candidate.

## Integrated, committed and staged while the user sleeps

Implementation commits are `d94e25c` (bounded DISCOVER recovery), `c4e823c` (slider and independent mute), and `b5cfa6c` (latest six-operation AVRCP and physical Sub cycling). Source/contract generation checks passed. The final configured Cargo suite passed400 tests with0 failures and2 existing ignored tests; the earlier missing-converter run and the separately observed shutdown timeout remain recorded rather than hidden. Existing unrelated UI fixture failures and the optional gate's known offline-flash boundary failure remain unresolved, as listed in the MUSIC change record.

The user explicitly authorized completing the work, server restarts and LCD firmware updates without asking again before sleeping. Read-only state inspection found local music stopped, but LCD/LBS still connected with Bluetooth enabled and saved volume20. PCM mute cannot suppress LBS's own disconnect/reconnect announcements. Therefore no restart, disconnect, flash or audio playback was performed during this overnight preparation. Deployment authorization exists; operational application is deferred to avoid waking the user, not represented as completed.

A fresh SQLite backup via the backup API passed integrity_check and was paired with the currently installed binary/config and all4 local-music source/cache files at `/home/tane/picomado-backups/20261108-pre-controls-013932`. This is a preparation snapshot: take a fresh consistent snapshot again before a later deployment if state has changed. The current installed server SHA256 remains `0742d7b512bdd1145aacc3e738d97b26f044869f5950f9a9c2b4bae59a79c27e`.

Release build with `--release --locked --features caricature-retention-delete -j1` succeeded in4m33s. Staged candidates and their manifest are at `/home/tane/picomado-staging/20261108-controls`, bound to source commit `b5cfa6c49729c70c9e75b28d7a90b0a5def83868`:

- Server SHA256 `d4fc9a03c51e30738ef01278c272736a9ae30ec8b08ca5efc42a93f9ee8d4865`.
- Registered LCD `E661640843239D21` candidate SHA256 `396f8df5c486ed358eb2e088604ece88d37123e987ef70a253fe6896abd88b9d`.
- E-ink build-only candidate SHA256 `17abf271fd00e959a2deef19859920682de199f6f6b01db0f69307b6f55e6da1`; no E-ink flash was requested or performed in this update.

No push, SSH, production service change or hardware write was performed. Current branch author remains tane. Actual LBS long-hold/volume events, automatic recovery effectiveness and operational acceptance remain unverified on hardware; the user's favorable earlier music listening is separate evidence.

## Authorized application completed, 2026-11-08 08:15–08:17 JST

After the user explicitly directed immediate application again, the already-authorized server and LCD update was performed without SSH. A new consistent SQLite snapshot (integrity_check OK), matching prior server/config, and all4 music source/cache files were saved under `/home/tane/picomado-backups/20261108-apply-controls-081520` before changes.

The existing install-server.sh route installed the validated release while preserving the unmanaged pic2gray4px. User service picomado was restarted and active. Installed server SHA256 is now `d4fc9a03c51e30738ef01278c272736a9ae30ec8b08ca5efc42a93f9ee8d4865`.

USB serial/FlashID `E661640843239D21` matched registered LCD PicoMado-lcd1, not the experimental Bluetooth Pico. picotool loaded candidate `396f8df5c486ed358eb2e088604ece88d37123e987ef70a253fe6896abd88b9d`, verified100% and rebooted to application. `/tmp/picomado-controls-applied-flash.log` records success. Fresh LCD session2471 advertised media-input-v1; all3 devices were online. Web content contained the new range slider and mute checkbox. Saved volume20 migrated to muted=false without changing its effective value. Music remained playing=false, source/cache hashes matched, and no audio_start_sent appeared in the update journal. No music or test tone was initiated. E-ink firmware was not written.

The saved LBS target connection subsequently ended in observed=failed, pending=null while LCD stayed online. Thus server/firmware application is complete, but automatic Bluetooth recovery is not accepted as effective. Speaker availability/pairing state and the failure's immediate cause were not established by this read-only final check. Actual AVRCP short/long/volume effects and music playback remain unverified on the new firmware. No push was performed; independent verification and closure remain with the user.
