# BT-AUDIO-20261107: HOME2 host-SBC deployment and finite playback

2026-11-08 JST. Implementer operational record, not independent approval or
final closure. The user changed to HOME2 to avoid mobile-data use. No AWAY
playback or network comparison was performed in this validation.

## Applied artifacts and preservation

- Server source:31fc2422b0630da098a0c539b036a66f99222281.
- Running server SHA-256:5ef8104fc465bfcb04ae23a2a7e934092c98071a7d8583a55b0025a39e4fe688.
- Fixed candidate and manifest: `/home/tane/picomado-staging/20261108-sbc-sender-31fc242`.
- Existing LCD receiver source:68ebc52; UF2 SHA-256:105654b2705e575436dfd386eecbabadb4caff8b81a50fe9af55db7b08a0d23d; serial E661640843239D21.
- Before server replacement, SQLite backup with successful integrity_check,
  old binary, config and before/after API snapshots were kept together at
  `/home/tane/picomado-backups/20261108-pre-sbc-server-124937`.
- Service restart succeeded and running-binary hash matched. USB observation
  `/tmp/picomado-power-observe-20261108-124946` confirmed HOME2, TCP READY,
  LCD online and Bluetooth connected. No automatic music playback occurred.

## Finite user-authorized playback

After an explicit playback cue, the test rechecked HOME2 from USB, set volume30,
played the selected existing asset for30 one-second observation intervals, then
paused and restored volume36. HOME2 was checked throughout; the test would stop
if another profile appeared. Evidence:
`/tmp/picomado-sbc-live-20261108-125036/{usb.log,state.json}`.

The server recorded AudioSbcStart, valid AudioSbcReady and first SBC packet,
not the legacy PCM send path. Negotiation:48000Hz,2 channels,16 blocks,
8 subbands,joint stereo(mode3),loudness allocation(0),bitpool32,
capacity16384 canonical frames,codec_epoch5. Stream1/session2521/display6673
started at canonical source frame742224.

- Sent onward to A2DP:1253 packets,1443456 canonical frames, approximately30.07s.
- Rebuffer count0, total wait0ms, failed0 at the end.
- All30 API samples:online,playing=true,error absent. Playback position moved
  from16541 to45678ms. This position is a UI clock, not an acoustic measurement.
- Last normal RTP payload:694bytes for9 SBC frames, hence77bytes per codec
  frame of128 samples. Audio payload rate is231000bits/s versus1536000bits/s
  for stereo48k s16le PCM, about85% less. At full1024-sample application chunks,
  the46-byte application header makes248250bits/s. TCP/WebSocket/link headers
  are excluded; this is a format calculation, not a router traffic-meter reading.
- User listening report, relayed verbatim by the parent:「いい感じで鳴ったよ」.

This establishes a successful finite HOME2 run and positive listening feedback.
It does not establish AWAY continuity, unlimited-duration playback, all speakers,
AVRCP behavior, or screen-switching stability under the new compressed path.
The earlier ARP/TCP observations remain unresolved and must not be attributed to
one access point solely from this success.

## Regression qualification

The fixed source has431 distinct passing test evidence and2 ignored tests.
One all-features run had an existing causal capture incomplete failure; the
same-input isolated test passed4 times afterward, and saved repeat captures
reported complete=true with no loss/drop/storage error. This is not a claim of
one fully green run. The existing structural gate result is48/49, with the
unchanged flash-route expectation failure. Full evidence is in the staged
manifest and `BT-AUDIO-20261108-sbc-sender.md`. No push, SSH or independent
stage closure was performed.

## Subsequent user-operated AWAY check

After the HOME2 record above, the user explicitly chose to try AWAY personally.
The implementation worker only read USB and existing server logs; it did not
switch profiles, start/stop music or extend playback. Passive evidence:
`/tmp/picomado-power-observe-20261108-125247` (contains the HOME2-to-AWAY
transition), server journal at12:53:47 JST. USB confirmed profile=AWAY,
SOCKS setup succeeded, and the new session2523/display6696/stream1 received
valid SBC Ready with codec_epoch7 and the same48k joint-stereo/bitpool32 tuple.

The stream ended with1052928 canonical frames (approximately21.94seconds),
914 RTP packets, rebuffer count0, total wait0ms and failed0. This is positive
short-duration AWAY transport evidence replacing the prior 'not yet tried'
status, not long-duration or acoustic-quality acceptance. The end was not
initiated by the worker; no cause beyond failed0 is inferred from that fact.
No further AWAY playback or mobile-data traffic test is authorized by this
observation alone.

The parent then relayed the user's listening confirmation verbatim:
「やった！AWAYでもいい感じに音がでてるぞ。いけてるんちゃう？」.
Together with the finite transport counters, this supports successful short
AWAY playback and positive listening feedback. It still does not establish
long-duration stability or authorize automatic further AWAY tests.
