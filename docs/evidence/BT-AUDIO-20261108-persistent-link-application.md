# BT-AUDIO-20261107: persistent server link application

2026-11-08 JST. This records implementation/operator checks, not independent verification or final closure. It supersedes neither the earlier mixed RAM-only trials nor the unresolved broader network hypotheses.

## Tested and applied artifacts

- Service source `6ee960225537785bcecff2b7b7ff06b8f109c9f1`, production feature `caricature-retention-delete`. Full locked Cargo regression: 440 passed, 0 failed, 3 existing ignored, including 415 library tests. Tracked service inputs unchanged. The former lifecycle fixture failure and its test-only correction remain separately recorded in `../verification/host-lifecycle-fixture.md`.
- Applied server SHA256 `1da0433b3ad3237a622f62ad1398c807af4894aa343d3f3719acf246dfd8e196`, 19:25 JST. Consistent SQLite backup passed integrity_check; old binary/config/state retained under private `picomado-backups/20261108-pre-remembered-autoplay-192521`. Service active/API restored. Device settings, volume/mute and 12 registered tracks preserved. No audio started by deployment.
- E-ink_1 firmware source `ad20ad3`, SHA256 `a30bd867bd3a4b8a8690b9be402e516700b1f45668f372ac3601c60815883e31`. ARM/stack/SDK evidence is bound by `76c5255`. Only the identified E-ink_1 was flashed; LCD and E-ink_0 unchanged.
- Full pretrial flash backup retained privately at `picomado-backups/20261108-eink1-before-remembered-mac-191255`. Legacy settings CRC valid, saved profile HOME. Cache sector initially all FF. No credentials or Bluetooth keys are included in this document.

## Cold trial

150-second USB/API capture starting 19:25:50 JST. `load empty` at 3 ms; valid Welcome learned at 11,198 ms; `save ok` at 23,324 ms, erase=0, measured adapter duration 2,142 us. This duration is not a general worst-case flash/IRQ bound.

All ten valid TCP snapshots used connection 1 and had ACK equal to last buffered byte; no TCP error/reconnect and no reported dropped trace records. Rendered frame 2490657 reached host state. Bluetooth connected; no music was requested.

Independent readback of the reserved final 16 KiB confirmed a valid committed current HOME record and CRC for the server address and verified NIC MAC. Existing Bluetooth 8 KiB and settings 4 KiB were byte-identical to pretrial backup.

## Warm trial

The readback operation rebooted the device. The following 150-second capture starting 19:28:36 JST observed `installed_before_tcp result=0` at 11,096 ms and current Welcome `learn unchanged` at 11,260 ms. Server independently logged `remembered=value comparison=matched` for session 214, versus `comparison=new` in cold session 213. `welcome_prepared` is server preparation, not a standalone delivery claim; device install/learn provide the corresponding device evidence.

All ten valid TCP snapshots again used connection 1 with ACK equal to buffered end; no TCP error or reported trace loss. Frame 2492653 rendered. The startup `load` line preceded USB capture and was not recorded: the helper's full warm log-order assertion is therefore **inconclusive**, not passed. The retained record, pre-TCP installation, and independently read-back byte-identical entire cache sector establish persisted-value reuse without a changed saved value. No save/changed-learning event occurred during capture. Bluetooth/settings regions remained byte-identical too.

This is software-restart persistence, not a physical power-removal/power-loss trial. Two 150-second observations do not prove that all first-connection stalls are eliminated. HOME2, AWAY, physical NIC replacement/stale-MAC recovery, and all-device deployment remain outside these physical trials. Host tests cover profile fences and bounded stale-value fallback; no live incorrect-MAC injection or router/network change was performed.

## Private reproducible evidence

`picomado-staging/20261108-server-link-cache-ad20ad3/` contains candidate/manifest, `live/cold`, `live/warm`, `live/summary.json`, cold/warm readbacks and parser reports. The read-only parser was cross-checked against production codec on 4,653 cases. `picomado-staging/20261108-remembered-autoplay-6ee9602/` retains full test/release manifests and source hashes. Raw flash stays private; only non-secret hashes and conclusions are suitable for repository evidence.

## E-ink_0 application (19:47 JST)

User connected and requested E-ink_0 update. Its own hardware profile/config were retained; the E-ink_1 image was not reused. Candidate source `a01be4f`, UF2 SHA256 `40638ed37e58cc60f1e60486c43be33be8884a433d8b62882d42297becfe65c2`; stable build/manifest and layout proof at private `picomado-staging/20261108-eink0-server-link-cache-a01be4f/`. ARM checks, actual main eight-step tests, final-16-KiB boundary and SDK flash safety checks passed. The feature-OFF net object difference from its older installed normal build was traced by matching machine code to existing TCP cleanup commit `412cc83`, not an unexplained cache side effect.

Full pretrial backup: private `picomado-backups/20261108-eink0-before-remembered-mac-194055`. Every old application UF2 page matched installed flash. Legacy HOME settings valid. The future cache sector was not erased but had no valid current record. Old ELF and all direct write paths confirmed BT store in -12KiB..-4KiB and settings in last4KiB, without use of -16KiB..-12KiB; origin of the residual values is unknown. RAM-only production-adapter testing using that exact sector seed verified erase/program/readback and adjacent 12KiB unchanged before application.

Cold 150 seconds from19:47:15: load empty; learn changed at9,709ms; save OK with erase=1 at22,470ms, measured34,142us. Nine valid TCP snapshots all connection1, ACK=end, no TCP errors or reported trace loss. Caricature frame2506021 rendered. Independent readback validated current record/CRC and byte-identical previous BT/settings12KiB.

Warm 150 seconds from19:50:03 after the readback-triggered software restart: load OK at3ms; saved MAC installed before TCP at11,093ms; current Welcome unchanged at11,272ms. Nine valid TCP snapshots again all connection1/ACK=end, no errors/drops; frame2508052 rendered. Full warm log ordering was captured. Second independent readback confirmed cache4KiB and existing BT/settings12KiB byte-identical to cold-after state; no same-value save.

After final readback/restart, API confirmed online, both hint/cache capabilities, caricature received/rendered frame2510033. Bluetooth remained disabled, volume42/mute=false unchanged, no music requested. Private `picomado-staging/20261108-eink0-server-link-cache/` stores capture/readback/helper checks and final state. Only HOME/software restarts tested; physical power-removal, HOME2 and stale-NIC fault recovery remain untested. This extends application to E-ink_0, not LCD, and is not final independent closure.

## LCD application (20:07 JST)

User connected registered PicoMado-lcd1 and requested the same update. Its saved profile was HOME2, retained throughout; only the displayed channel was temporarily moved by standard USB input from YouTube to caricature for idle-save verification. Video mode deliberately defers flash writes. Full backup is private `picomado-backups/20261108-lcd-before-remembered-mac-195814`.

Candidate source `bd48c78`, UF2 SHA256 `bffd96bfb5a41f7cd305af84a12ba8561dffd4623bcef0031f794d65c2168caf`, stable `picomado-staging/20261108-lcd-server-link-cache-bd48c78/`. Own LCD config/profile and existing BT/SBC/USB/CREDIT/SLEEP/ARP_RX settings retained. Old backup matched all2,203 application pages and ELF/BIN. Main and nine existing runtime/display/control objects matched old instructions/relocations; net difference was reconstructed as known cleanup412cc83. HOME/HOME2 main host16steps, ARM build and reserved16KiB protection passed. Static heap gap14,864B (old17,452B); cache call-path stack estimate1,080B/4,096B. Actual heap minimum/IRQ-inclusive stack high-water remain unmeasured. No increase to existing diagnostic capacities, no new dynamic cache allocation.

Cold150seconds from20:07:17: load empty at3ms, HOME2 learned at11,755ms, save OK erase=0 at24,647ms, measured2,141us. Nine valid TCP snapshots all connection1 and ACK=end; no TCP errors/reported trace drops. Caricature frame2514125 rendered. Readback confirmed valid HOME2 record/CRC and existing BT/settings12KiB byte-identical to backup.

Warm150seconds from20:10:04: load OK at3ms; a Wi-Fi BADAUTH occurred before TCP and existing retry recovered. Saved MAC installed before TCP at26,093ms; Welcome unchanged at26,224ms. Eight valid TCP snapshots all connection1/ACK=end, no TCP error/trace drop; frame2514129 rendered. Complete warm marker order captured. Second readback confirmed cache4KiB and existing BT/settings12KiB unchanged, no same-value save. Wi-Fi authentication retry is an explicitly remaining observation, not hidden by the successful post-join TCP check.

During the operation desired Bluetooth changed externally from revision80/OFF to81/ON; updater did not issue that change, respected current ON/volume36/mute=false, and observed connection. No music was started by the updater. At final restoration the selected channel had independently changed from caricature to local-music (enabled_revision95→96); the before-selection guard stopped the YouTube restore without sending input. Final API showed online, both cache/hint capabilities, local-music received/rendered2514141, Bluetooth connected. This report does not claim YouTube was restored; the newer selection was preserved.

Private `picomado-staging/20261108-lcd-server-link-cache/` contains captures, readbacks, checks and final state. LCD tested HOME2/software restart only; physical power removal and stale-NIC failure recovery remain untested. All three requested devices now have the new cache capability, while the per-device observed network coverage remains HOME on E-ink devices and HOME2 on LCD.
