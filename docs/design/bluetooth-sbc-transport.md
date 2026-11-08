# BT-AUDIO-20261107: SBC transport on the host-to-Pico link

Specification, 2026-11-08. This supplements the [change authority](../changes/BT-AUDIO-20261107.md), not a separate change or completion claim. Implementation, independent verification, deployment and effectiveness remain separate. No SSH, USB, service operation, playback, SDK modification or push is authorized here.

## Problem, comparison and decision

The current transport carries 48 kHz signed-16 stereo PCM: 192000 bytes/s before framing. Its 4096-frame / 16384-byte device ring holds 85.33 ms. The parent's saved AWAY streams show first-PCM-to-first-credit intervals of 123–142 ms, and failure with server sent=7552, device accepted=4096, consumed=3456. These are different observations, not a measured network RTT. A larger time reservoir and less Wi-Fi traffic address a different hypothesis from the bounded rebuffer repair; neither proves a radio/root-cause fix.

Use host SBC encoding and Pico pass-through for newly negotiated peers. Preserve source PCM assets, the existing PCM transport, 48 kHz stereo and the existing Bluetooth packet scheduler. Do not introduce 44.1 kHz, resampling, SBC decode/re-encode on Pico, a second Bluetooth runtime or a compressed-asset cache. SBC is lossy; this uses the already selected Bluetooth encoder settings, rather than an extra codec stage.

The selected v1 buffer guarantees **16384 canonical sample frames / 341.33 ms**, even at the largest supported SBC frame, within the existing 16 KiB audio storage. At bitpool32 the raw storage could hold about568 ms, but v1 deliberately exposes one conservative capacity rather than variable credit. The existing bounded rebuffer policy remains a finite safety net; it does not make insufficient sustained throughput continuous.

## Language and ownership

- A canonical sample frame is one simultaneous left/right sample at48 kHz. All first/sent/accepted/consumed/final positions retain this unit. One SBC frame represents128 canonical frames. SBC byte counts and RTP packet counts must not enter those counters.
- `StreamIdentity` remains incarnation/session_epoch/display_epoch/stream_id. A source generation remains part of `AudioPlaybackSpec`, not a replacement for the transport identity.
- A `SbcConfiguration` is an immutable value for one admitted stream: rate48000, channels2, blocks16, subbands8, channel_mode, allocation and bitpool. Wire channel_mode uses SBC header values2=stereo,3=joint_stereo; allocation uses0=loudness,1=SNR. Never expose an unchecked BTstack enum/cast as a wire value.
- `codec_epoch` is a nonzero monotonic runtime configuration revision. It changes/invalidate on codec renegotiation or link replacement. It is bound to the current transport identity and is not a sample counter. Overflow fails closed. Refresh of the same established link without reconfiguration need not change it.
- The pure sender/session owns format, credit and lifecycle. The host encoder adapter owns one encoder state per live SBC stream. The firmware audio aggregate owns one tagged storage union, counters and pinned configuration. Adapters do framing/SDK/C calls, not unchecked successful domain transitions.

## Compatibility and start handshake

New firmware advertises `audio-sbc-v1` in the current online Hello, alongside existing Bluetooth/PCM capabilities. New server selects SBC only when that current session advertises it and the server encoder is built in. Old firmware continues kind22/24 PCM with the existing2048/4096 capability logic. A new firmware talking to an old server receives PCM normally and sends no unsolicited SBC messages. Thus Welcome and generic BluetoothReport need no extension.

Reserve the following kinds (kind27 is already MediaInput):

| Kind | Type and direction | Fields / meaning |
| --- | --- | --- |
|28|`audio_sbc_start`, host→Pico JSON|incarnation, session_epoch, display_epoch, stream_id, first_frame, rate. Same identity/start field semantics as PCM AudioStart; rate must be48000. Requests a snapshot of the current Bluetooth codec, not a guessed codec configuration.|
|29|`audio_sbc_ready`, Pico→host JSON|The same four identity fields; first_frame; codec_epoch; rate; channels; blocks; subbands; channel_mode; allocation; bitpool; capacity_frames. Configuration is the actual accepted A2DP setting plus the selected fixed bitpool; capacity_frames must be16384.|
|30|SBC binary, host→Pico|Same length/version/identity/first_frame header layout as PCM kind24, followed by1..8 whole SBC frames; see below.|

Reuse kind23 AudioStop,25 AudioStatus and26 AudioFinish unchanged. Do not broaden the PCM kind24 decoder or send SBC bytes under it.

Start sequence:

1. Require the current source spec itself to be valid48kHz/stereo; a source/spec/Ready rate mismatch is a terminal unsupported-format error, never a sample-clock reinterpretation or automatic resample. Retain the current display ReceiptAck barrier. Choose a fresh stream_id, record the SBC transport's fixed capacity and enter WaitingReady. Send kind28. No source read/encode/chunk/EOF is permitted until matching Ready is admitted.
2. Pico validates current incarnation/session/display, monotonically new stream_id, Bluetooth readiness and the negotiated48k/16×8/stereo-or-joint tuple. It cancels previous audio through the normal owner, binds its audio session to the current codec_epoch, resets ring/counters to first_frame, and prepares the corresponding Ready. No AVDTP START is requested yet.
3. A failed start uses the existing failed AudioStatus for that identity. A valid Ready is a bounded pending control report: retry only until TCP COPY acceptance or the existing1500ms report bound; inability to report cancels the new session through existing connection failure handling. Never block the callback or queue an unbounded report history.
4. Host admits Ready only for the exact WaitingReady identity and first_frame. Require the full supported tuple, bitpool2..53, nonzero codec_epoch and exactly16384 capacity. Pin the value and initialize one host encoder. A duplicate identical Ready is harmless; a conflicting Ready for an admitted stream is a terminal protocol/configuration error. Stale identities cannot configure or restart a stream.
5. WaitingReady has a non-extending3s deadline from start. Wrong/stale/duplicate messages do not extend it. Only after acceptance may the host send SBC and use normal credit progress. No automatic infinite restart or silent fallback after failure.
6. Firmware checks the pinned configuration/epoch before receiving, preparing and sending. A real reconfiguration/link replacement cancels the audio, prepared RTP and timers through the normal owner; old frames/Ready/callbacks cannot be reinterpreted under the new codec. A later play creates a new stream and handshake.

A known absence of SBC capability or of the host encoder selects PCM **before** start. A failed SBC handshake/encode/session is surfaced through the normal terminal audio failure path, not hidden by switching format within the identity. A deliberate retry using PCM must use a fresh stream and cannot loop automatically. No dual transmission.

## Binary framing, validation and ring

Kind30 uses the same46-byte outer header as kind24: u32be length excluding itself; version1:u8; kind30:u8; incarnation/session/display/stream/first_frame as five u64be values. Following bytes contain raw SBC frames, with no RTP or SBC media-count prefix. Do not serialize a C struct.

For the admitted tuple, encoded length is `12 + (channel_mode == 3 ? 1 : 0) + 2 * bitpool`, range16..119 bytes. Each frame represents128 canonical frames. Validate the complete application message before any state mutation:

- The payload is nonempty, an exact multiple of that fixed length and contains1..8 frames; maximum payload952 bytes and maximum complete message998 bytes.
- Every frame has sync0x9c (not mSBC), the exact rate/blocks/mode/allocation/subbands header byte and the exact pinned bitpool. No variable bitpool or mode within a stream.
- Check the SBC CRC-8, initial0x0f / polynomial0x1d, over header bytes1–2 and the joint/scale-factor bits after byte3. For the supported8-subband stereo layouts these are bytes4..11, or bytes4..12 for joint stereo. The joint-stereo reserved bit (byte4 bit0) must be zero. CRC covers the SBC protected header, not every audio bit; do not call it whole-payload integrity.
- Check current full identity and pinned codec epoch, exact contiguous first_frame, checked `first_frame + count*128`, remaining canonical credit and remaining byte storage independently, and declared EOF bound. Partial stale overlap is not new data; fully old duplicate packets may be ignored without credit growth. Future gaps/malformed current-stream data fail, never skip ahead.

The audio payload storage is a **union** of current PCM `int16_t[4096*2]` and a16384-byte SBC ring, not two allocated rings. Keep the tag, immutable configuration and bounded indices/counters outside it. In SBC mode permit at most128 queued SBC frames (16384 canonical frames), regardless of unused bytes. At119bytes/frame that consumes15232bytes. The existing one prepared RTP buffer remains separately bounded as today; it is not additional advertised credit. Firmware builds must measure the total BSS/stack change, not merely assert the union saves memory.

A chunk is admitted atomically after validating every frame. The aggregate must also enforce `accepted-consumed <= capacity`, including a prepared-but-not-successfully-sent RTP packet. Copying frames out of the ring into that packet does not free **sender credit**. Only successful SDK RTP send advances consumed. Ring wrap, full/empty, prepared packet and malformed tail cannot partially advance accepted.

## Credit, EOF, pacing and rebuffer

The existing invariant remains `start <= consumed <= accepted <= sent`, with `sent-consumed <= capacity` at the sender. Credit is absolute, monotonic and identity-bound; duplicates/reordering do not enlarge allowance. The only legal capacities are the negotiated PCM2048/4096 or SBC16384 under their corresponding format. Public APIs must not let arbitrary capacity values bypass mode negotiation.

SBC allowance is aligned down to128 canonical frames, at most1024 per chunk (8 SBC frames). Permit at most2 chunks /2048 canonical frames per current5ms writer turn, leaving control/credit cancellation points. PCM retains its existing2×256 limit. The plugin read bound of4800 is sufficient; source reads still survive ordinary credit events and remain interrupted by stop/selection changes, with their existing100ms deadline.

- Startup waits for the full advertised capacity, or a fully received finite short EOF with at least one SBC frame. A declared empty EOF ends without waiting for a packet. StartSbc/Ready alone never means sound has begun.
- Keep the current source/end semantics. Pad only the last partial128-sample block with at most127 zero frames, then encode it. `final_frame = first_frame + ceil((source_end-first_frame)/128)*128`; all arithmetic checked. AudioFinish may precede the final chunk as today. No padding on temporary shortage. End positions are relative to first_frame, which need not itself be a multiple of128. Seek starts at the exact requested PCM frame with a fresh encoder; do not round the seek backward/forward. For finite source_end the transport end must never exceed source_end+127 (less than2.667ms); the visible track duration/progress stays bounded by the real source duration, while transport drain waits for the padded canonical end. Reject a source read extending beyond its declared frame_count rather than expanding that duration.
- Pico constructs the existing SBC media-count byte and copies whole validated SBC frames into its one prepared RTP packet. Use the existing actual-MTU/15-frame/40ms limits and full-packet rule. LBS MTU704/joint32 gives9frames,694bytes,24ms; joint53 gives5frames,596bytes,13.33ms. Do not send each Wi-Fi chunk as one RTP packet.
- The can-send callback only sends the prepared packet. Successful send advances consumed and RTP timestamp by count×128. No Pico SBC encode or decode occurs for SBC transport. PCM keeps its current SDK encoder path.
- Apply the existing bounded rebuffer contract to both formats; its capacity/prefill query becomes format-specific (PCM4096 or SBC16384). Preserve the1500ms episode bound,3-episode budget,5s stable reset and explicit wall-clock reanchor after resumed successful send. Retain canonical/RTP positions, never resample, accelerate catch-up, or fabricate silence. Peer-suspend/disconnect remains cancellation.
- New capability and compressed buffer do not relax the250ms scheduling failure, no-credit timeout, control/report bounds, or terminal failure feedback. Rebuffering is still an audible gap, not uninterrupted success.

## Host encoder, assets, volume and cancellation

Keep `AudioPlaybackSpec` and plugin `read_audio` in canonical48k s16le stereo PCM. Local music PCM files remain unchanged; no per-volume compressed disk cache or asset regeneration. The common transport executor reads a bounded PCM block, checks current selection/identity, applies the latest per-device squared gain/mute exactly as today, then encodes it with the stream's persistent state. Preserve that state across chunks and volume changes; do not initialize per frame.

Gain changes do not alter the source generation, display token or transport identity. Already transmitted audio can take up to the advertised341ms plus controller/sink buffering to drain; disclose/measure this added response latency. Immediate retroactive muting is not promised. Future chunks use the latest gain. Encoding exact-zero PCM may retain codec filter history briefly, so bytewise PCM silence must not be advertised as immediate speaker silence.

The encoder adapter checks all lengths/configuration before crossing its C boundary and returns the actual whole-frame length and bounded bytes. On any error, stop rather than send a partial frame. One stream owns one opaque C context. No unsafe C state/pointers enter the pure domain. The selected encoder has global joint-stereo scratch arrays: serialize all its init/encode calls with a short shared mutex across streams, even though each stream owns a context. Do not hold the mutex across IO/await, or assume the library is reentrant.

After every asynchronous read and before publication, recheck current session, display, source generation, connection/stream/config. Stop, pause, seek, track/display replacement, socket loss or codec invalidation discards the ring, prepared packet and encoder context. Late read/Ready/credit/callback completion cannot restart it. No helper process or unbounded encode queue is needed; if an implementation chooses a worker, cancellation must own and drain its bounded lifetime.

## Reproducible dependency and license boundary

Host proof used the installed Pico SDK's BTstack checkout at `eb0bb8b5ea6d234ccb940313b47f7a5c3b4e20ec`, upstream `https://github.com/bluekitchen/btstack.git`. SDK itself was `079c6f39023649b154152db30f1d781e884879bc`. No SDK files were changed.

Recommended distributable dependency: vendor only `3rd-party/bluedroid/encoder/srce/*.c` (8 files) and `include/*.h` (4 files),139838 bytes at that revision, under a clearly named service vendor directory. All12 inspected files carry Broadcom1999–2012 Apache-2.0 notices. Preserve those notices, add the Apache-2.0 license text and provenance manifest (upstream URL/commit, original relative paths, per-file SHA256, local patches), inspect applicable upstream NOTICE material, and label any local changes. Do not apply PicoMado's MIT license to these files. Actual vendoring/packaging still requires implementation review of the selected files and obligations; this document is not a legal assurance.

Do **not** copy/link the `src/classic/btstack_sbc_bluedroid.c` host wrapper or the full BTstack core: that wrapper carries an additional noncommercial-use clause. A small project-owned C ABI wrapper can call the Apache-2.0 `SBC_Encoder_Init` / `SBC_Encoder` directly. The firmware's pre-existing SDK relationship is unchanged.

Compile the vendored C statically from the service build with an explicit pinned build dependency (the current Cargo.lock already contains registry `cc`1.4.5). A normal checkout plus normal Rust/C toolchain must build without a local Pico SDK, `/home/tane` paths, online build-time downloads or a runtime helper. Do not discover and link an arbitrary user SDK. Reproducible source/license manifest and host codec fixtures belong in CI. C toolchain requirements are already familiar to this service's bundled SQLite build, but must be documented explicitly.

Alternatives are not selected: installed FFmpeg CLI can produce SBC but does not expose the full exact mode/bitpool configuration here; installed `libsbc.so.1` exists on this host but development headers were absent, and a system-library binding would add deployment/ABI/license obligations. No new registry SBC crate was reviewed, and no unverified crate should be described as a drop-in solution.

## Feasibility evidence and required acceptance

Read-only/small temporary host checks, no device/service/playback:

- Installed FFmpeg advertises the SBC encoder. Three128-frame inputs at228000bps produced3×76byte frames, header9cf920 (48k/16blocks/stereo/loudness/8subbands/bitpool32). At231000bps it selected bitpool33/78bytes. A bitrate request alone therefore does not enforce the current joint-stereo32/77byte contract.
- Direct SDK encoder build on this aarch64 host exercised208 configurations (2modes×2allocations×52bitpools),4frames each:832 frames passed exact header, length, output sentinel, independent CRC-8 and joint reserved-bit checks. Joint32=77bytes, joint53=119; stereo32=76, stereo53=118.
- A single-loop37500-frame encode (100s of PCM) took about0.172 CPU seconds. This is feasibility evidence, not concurrent production latency or real-device effectiveness.
- Temporary fixture `/tmp/picomado-sbc-design.szbPE8/probe.c` SHA256 `d5f5947b39b6e946cf2ec3085698c6f755cfd594c586fe0191ae32ce94b03933`; no implementation is sourced from a disposable path. Promote reproducible tests during implementation.

Required tests before deployment: all208 codec settings and CRC/malformed negatives; exact wire lengths/truncation/extra bytes; unsupported tuple/capacity; handshake timeout/duplicate/conflicting/stale Ready; both directions of old/new compatibility; union capacity/wrap/overflow and prepared-inflight credit; partial EOF/nonzero starting offset/u64 overflow; cancelled source read/encode/Ready/rebuffer/send at each boundary; two streams/devices with independent gain and serialized C scratch use; no per-frame encoder reset; output decode sanity with an independent decoder; PCM regression; SBC production prepare/send and no-encode checks; existing rebuffer tests under both capacities; existing terminal UI failures; full service/firmware host regression and LCD/E-ink builds/BSS.

Hardware evidence remains separate: authorized HOME/AWAY finite playback, observed accepted/consumed/underflow and scheduler/credit delays, real speaker continuity/quality/volume response, display/control responsiveness, disconnect/reconnect and bounded recovery. Do not call a host codec test, spec commit, flashed artifact or a connected speaker continuous-playback acceptance.

## Implementation ownership handoff

1. **Host codec/transport worker:** vendor/provenance/build/opaque encoder wrapper; pure SBC config/frame codec and mode-aware sender; kind28/29 wire and kind30 codec; writer handshake/encoding/credit/cancel integration. Reuse plugin PCM assets and the current terminal-failure owner. Coordinate `transport_audio.rs` with the concurrent UI/failure worker before editing.
2. **Firmware worker:** shared pure SBC validation/storage union; kind28/30 ingress and bounded Ready report; pinned config/epoch; pass-through prepare/send and mode-specific prefill. Coordinate `bluetooth_runtime.c` with the concurrent rebuffer worker before editing.
3. **Integrator:** generated wire identity representations, cross-language fixtures, capability/old-peer matrix, independent review and full regressions. Do not mark independent/operational/effectiveness stages from worker self-tests. Parent owns closure.
### Host source admission and failure-query responsibility

Keep the existing public `audio_playback_for` query valid-only, so its callers
retain the canonical-format guarantee. Add one crate-private raw-offer query
for the socket's format admission and the application's terminal-failure
matching. Both queries share the same current online session, remote owner,
normal mode, selected plugin and display identity checks. Raw intent is not
permission to read or transmit audio.

The socket validates the complete `AudioPlaybackSpec` before AudioStart or
AudioSbcStart, source IO, EOF or encoding. Invalid input produces one
UnsupportedFormat terminal result at the unchanged source start. It does not
resample, silently disappear, send a start, or automatically retry. The public
valid-only query remains unchanged for other consumers.

Terminal feedback must match the current raw display/plugin/spec exactly.
For an invalid spec only UnsupportedFormat at `consumed == start_frame` may
reach the plugin's failure callback; other reasons or progressed positions are
rejected. A stale identity, source generation or different invalid spec cannot
fail a newer selection. The content owner retains responsibility for exposing
and ending its playback intent, just as for existing terminal delivery errors.
Tests cover rejected stale/different spec feedback, invalid-rate admission
without start/read/EOF, one terminal callback, and the unchanged valid-only
public query. This explicitly relocates format admission for the socket rather
than weakening the old query contract.

### Original-source streaming extension

The later [local-music streaming contract](local-music-streaming.md) removes the
manual full-track PCM conversion prerequisite. It keeps the reader output in
canonical48k s16le stereo PCM and preserves gain-before-SBC ordering, per-stream
encoder history and negotiated transport. Source priming occurs before Start,
so decoder startup does not consume the device Ready/prefill budget. Existing
PCM-only assets remain supported and are not automatically deleted.
