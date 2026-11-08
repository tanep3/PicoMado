# BT-AUDIO-20261107: host SBC sender verification, 2026-11-08

This implements the transport contract fixed by `808a838`, with the raw-source
admission/failure-query responsibility fixed separately by `df244fc`. It builds
on the receiver `68ebc52`, host codec `f64dd8c` / Cargo integration `bacf317`, and
terminal failure feedback `6c1598a`. This record is not deployment or evidence
of uninterrupted AWAY playback.

## Implementation

- Current online `audio-sbc-v1` capability selects kind28 Start, kind29 Ready
  and kind30 SBC. Legacy peers keep kind22/24 PCM and 2048/4096 capacity.
- The pure sender admits the exact identity/start, nonzero codec epoch,
  48kHz/stereo/16-block/8-subband supported tuple, bitpool2..53 and exactly16384
  canonical-frame credit. Stale Ready cannot configure a stream; exact duplicate
  Ready neither reinitializes the codec nor renews timeouts. Conflicts fail.
- WaitingReady allows no source read, EOF or payload and has a non-extending3s
  deadline. One successful Ready log records the actual tuple and identity.
- One persistent host encoder receives latest squared gain/mute applied to
  bounded disposable PCM. Two1024-frame SBC chunks per5ms turn are permitted;
  PCM retains two256-frame chunks. CRC, frame size and every protected header
  are checked before kind30 publication.
- Source length/position arithmetic is validated before any EOF/padding. Only
  the final partial128-frame block is padded. Partial reads, including temporary
  empty reads, retain their raw tail. Nonaligned seeks retain the exact source
  start. Original PCM assets and plugin read_audio remain unchanged.
- Both post-read and post-Finish-await gates process bounded control events.
  Stop, failed credit, conflicting Ready and a disconnected command channel
  prevent publication. An ordinary reply preserves raw pending PCM and the
  already-declared final position; resumption rereads neither source nor EOF,
  advances encoder history only once and consumes raw pending data only after
  successful chunk enqueue. Ambiguous/send failure exits the writer.
- The public audio_playback_for query remains valid-only. A crate-private raw
  query supports current-offer validation and terminal feedback. An invalid spec
  can report only UnsupportedFormat at its untouched start, with exact current
  display/plugin/spec matching. New failures use the existing UI failure path.

## Fixed input and qualified regression evidence

The168-file service/identity inventory is unchanged before and after the final
runs, including isolated retests and remaining targets:

`197873ae3df9250d9597d1aa51cf652cffb99bf6b0e666d238d55d911e4f8434`

The command uses `PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px`, Cargo
`--offline --locked --all-features --jobs 1`, and `--test-threads=1`.

- The full library run completed408 passed /1 failed /1 ignored. The sole failure
  was the existing Rust-to-C causal fixture at causal_contract_tests.rs:341,
  Capture::finish returning `observation capture incomplete`. Its image, input
  and ACK path completed. The discarded failure fixture prevents identifying
  which capture completeness condition failed; no root-cause or flaky-test
  claim is made.
- That exact test subsequently passed4 isolated runs without source changes.
  Three repeated LCD/E-ink captures were preserved. All six footers report
  complete=true, storage_lost=0, dropped=0, saturated=false and error=null.
- Remaining binary/integration/doc targets passed22 tests, with1 ignored.
  Thus431 distinct Rust tests have passing same-input evidence and2 are ignored,
  but this is qualified combined evidence, **not a clean single full-suite run**.
- All21 SBC-named tests passed in the full run. Six real-sink tests cover
  Ready-gated missing source, finite EOF boundaries and nonaligned seeks,
  gain/mute history and credit capacity, pause/new stream/reconnect, invalid
  source and mismatched Ready UI failures, arbitrary partial reads (64/0/64 and
  1/127/1/127/17), and blocked Finish cancellation/control-yield scenarios.
  The final ordinary-reply case uses nonzero PCM and verifies the final SBC
  equals one pristine encode and differs from a double history advance.
- Static domain/boundary gate, generated-model drift, diff whitespace and the
  local-music UI renderer test pass. Root structural Python tests remain48/49:
  test_gate_routes.py:46 expects missing-approval refusal from flash.sh
  --verify-only. These files were not changed; the UI predecessor records the
  same pre-existing failure.
- An earlier run omitted the converter environment and failed9 fetch/radar and
  derived transport tests. All9 pass after supplying the required CLI path with
  unchanged source. Logs retain that environmental failure history.

Raw final logs and inventories are staged with the release candidate. The
causal retest captures are under `/tmp/picomado-sbc-causal-repeats` and copied
into the candidate evidence. The release manifest supplies exact source commit,
production feature set, input hashes and binary SHA256.

## Limits and next step

The sender bounds queued transport audio to16384/48000 seconds (341.33ms).
Already transmitted old-volume audio, codec history and controller/sink buffering
mean immediate physical muting is not promised. Tests prove software ownership
and cancellation, not the sink's audible latency or Wi-Fi continuity.

The parent owns independent acceptance, candidate deployment and real-device
Ready/throughput/continuity/volume/cancellation checks. This worker did not use
SSH, USB, change SDK/configuration, install/restart a service, play audio or push.
