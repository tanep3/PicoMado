# BT-AUDIO-20261107: E-ink Bluetooth/SBC update

2026-11-08 JST. The user requested E-ink firmware updates and a commit afterward.
Existing private device configuration was preserved. No SSH, push, automatic
music playback, speaker pairing or server/network configuration change was
performed by this update operation.

## Candidates

Both real-device profiles were freshly built with Bluetooth audio and EEX1 ON,
production diagnostic switches OFF. Source5407f2d, SDK079c6f39023649b154152db30f1d781e884879bc.
Candidate/config/source verification manifest:
`/tmp/picomado-eink-audio-evidence/build-manifest.json`.

- E-ink_0 serial E6614864D3813227, UF2 SHA-256
  270d1630d3b1e733704e6fdb6b87689cfa659bb5d82ade69165cf8514dfc3e57.
  Text553448, BSS166480 bytes. Reserved settings/bonding sectors are untouched.
- E-ink_1 serial E6614864D34F8E22, UF2 SHA-256
  3307b0b52dfd5a6136f5b7d838643dd2f7903e8a51a3026c042f09b420561fe8.
  Text553640, BSS166608 bytes. Reserved settings/bonding sectors are untouched.

HOME2 is not configured in these existing E-ink config files and remains hidden;
this update does not copy LCD credentials or change saved network selection.

## E-ink_0 application

The connected serial matched the existing registered manifest. Full physical
flash was saved before writing at
`/home/tane/picomado-backups/20261108-eink0-before-update/flash-before.uf2`.
The same private backup contains old candidate, config, current server binary,
SQLite backup with integrity_check=ok, and API snapshots. No source assets were
changed. picotool verified the new flash100% OK and returned to application mode;
log `/tmp/picomado-eink0-audio-flash.log`.

The updated device advertises audio-sbc-v1, bluetooth-audio-v1, media-input-v1
and its existing buttons0/E-ink capabilities. USB confirmed V4 panel ready and
actual rendering. The server eventually confirmed matching Received/Rendered
frame2440103 and online state. Observer folders:
`/tmp/picomado-eink-update-observe-20261108-130347` and
`/tmp/picomado-eink-update-observe-20261108-130521`.

An initial communication stall also reproduced on this device: frame2440098
was received at13:03:41.962, but its Rendered event arrived at13:05:54.700,
about132.738seconds later and stale for the then-current display. USB shows
render completion earlier, successful TCP enqueue, retransmissions and router
MAC changing to actual-host MAC. Following reconnect, ACK progress recovered.
This is recorded as a separate unresolved transport problem, not concealed as
a clean first-connection success or attributed to failed panel rendering.

The user's UI settings changed during the operation (selected local-music,
Bluetooth enabled, target absent, volume0); the updater did not restore older
snapshots over those changes. No pairing or audio acceptance was attempted.

## E-ink_1 application

The physical serial matched E6614864D34F8E22. Before replacing anything,
full flash was saved privately in
`/home/tane/picomado-backups/20261108-eink1-before-investigation/flash-before.uf2`.
Every one of1533 program blocks matched the prior11/5 a2f191ee... artifact;
the old manifest named a different artifact and was not trusted as identity.
The backup also contains the existing config, old candidate, current server,
consistent SQLite backup and before-state. The user requested retaining this
unit as a diagnostic subject, so old-firmware passive and reboot observations
were completed before the new firmware was written.

New UF2 verification completed100% OK; flash log
`/tmp/picomado-eink1-audio-flash.log`. Observer
`/tmp/picomado-eink-update-observe-20261108-131934` confirmed the same HOME
profile, clock selection, buttons2, new audio-sbc/media-input capabilities,
online state, and matching Received/Rendered2440140. Bluetooth remained disabled,
muted with volume0 and no target. No sound or pairing was initiated.

In this new-firmware trial, first Receipt13:19:44.941 to Rendered13:19:51.637
was6.696seconds, consistent with panel drawing rather than the earlier large
network delay; subsequent clock update ACK arrived normally. The previous
old-firmware reboot on this same unit had a43.991second host interval despite
6.635seconds of device rendering. This is evidence of intermittent conditions,
not proof that the new firmware cures the transport issue or that the old
firmware was always bad. See
[controlled comparison](BT-AUDIO-20261108-old-new-firmware-comparison.md).

## Build and verification receipts

Both actual-device Bluetooth-ON ARM builds completed with zero warnings.
Full firmware host suite passed (EOF cases1..52), as did rebuffer native/ASan/
UBSan and both E-ink profiles' three SBC suites in native and sanitizer variants.
Source/config hashes were unchanged. Final build/test manifests and fixed UF2s
are privately staged at `/home/tane/picomado-staging/20261108-eink-audio`.
That directory is700 and files600 because compiled UF2s contain private device
configuration; raw config files were not copied into the stage.

A same-current-source Bluetooth-OFF E-ink_1 comparison artifact was also built
and preserved but not flashed in this update. No independent approval or final
closure was performed. Audio pairing/playback on the E-ink devices is not yet
verified by these silent update checks.
