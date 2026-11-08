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
