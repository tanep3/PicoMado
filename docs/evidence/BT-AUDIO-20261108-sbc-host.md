# BT-AUDIO-20261107 host SBC encoder evidence

Specification commit: `808a838`; [contract](../design/bluetooth-sbc-transport.md).
This is host feasibility/implementation evidence, not independent acceptance,
deployment or real-speaker continuity. No SSH, USB, service operations, audio
playback, SDK modification or push was performed.

## Implemented, before service integration

- The selected 8 encoder C files and 4 headers are fixed, unmodified Apache-2.0
  vendor files with per-file notices, complete license and provenance hashes.
  SDK and BTstack core/wrapper are not build/runtime dependencies of the host
  adapter. See `service/vendor/bluedroid-sbc/README.md` and `provenance.json`.
- The project C ABI validates parameters, converts s16le bytes explicitly,
  encodes exactly128 sample frames and retains no caller/stack pointers.
- `service/src/sbc_encoder.rs` owns one opaque state per stream; the adapter
  serializes all init/encode calls to protect global C scratch. It validates
  output length/header/CRC, retains filter history and bounds input to1..8 blocks.
  Invalid input is rejected before state mutation; codec/output errors poison
  the stream. It performs no IO, asset caching, gain change or sample resampling.
- The initial module is deliberately not declared in `lib.rs` and has no Cargo
  build integration while another worker freezes/builds the preceding UI fix.
  Standalone tests compile only these sources. A later integration commit must
  add the fixed `cc` build dependency, build script and module declaration;
  source presence alone does not make the service send compressed audio.

## Reproducible checks (aarch64 host)

`bash service/tests/run_sbc_encoder_tests.sh`:

-12 vendor source hashes and retained Apache-2.0 notices: PASS.
- C ABI invalid configuration/input/output capacity: PASS; output sentinels for
  208 supported tuples ×4 encoded frames =832 frames: PASS.
- Rust7 tests: PASS, including208 configurations ×8 frames, exact CRC/length,
  invalid input without state advance, continuous versus split encode,
  independently interleaved states,4 concurrent threads, corruption rejection
  and12 fixed cross-language golden vectors.

`python3 service/tests/verify_sbc_vectors.py`: all12 representative mode ×
allocation × bitpool tuples decoded independently with installed FFmpeg to
exactly128 stereo sample frames each. Output goes only to memory, not a speaker.
The fixed vectors are `service/tests/fixtures/sbc-v1-frames.txt`; the firmware
worker separately reported its production C validator accepted all12.

The earlier temporary SDK feasibility fixture exercised208 tuples and independently
checked CRC; bitpool32 gives76/77bytes for stereo/joint,53 gives118/119bytes.
Encoding100seconds of PCM took about0.172CPU seconds in a single loop. This is
not a bound on production concurrent latency. Installed FFmpeg's SBC CLI used
stereo rather than joint and mapped231000bps to bitpool33, confirming that a
bitrate argument is not an exact negotiated-codec contract.

## Not yet established

Service wire/handshake/cancellation integration, firmware pass-through and
rebuffer integration, aggregate/full regressions, ARM BSS/stack, independent
review and actual HOME/AWAY uninterrupted playback remain separate work.
