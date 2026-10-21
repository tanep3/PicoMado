# LCD-ACKLESS-20261021: bounded latest-frame sending for LCD and E-ink

Status: design for a server-only change. The running Pico firmware, wire messages and receipt/rendered ACK transmissions remain unchanged. This revision precedes the common implementation. Latest-frame delivery applies to push-capable LCD and the current E-ink protocol; a legacy LCD without the required capability retains its existing ACK-gated path. The deployed service and physical terminals are not changed by this commit.

## Existing contract gap and cause

`8e41fd6` added bounded latest-frame delivery for LCD video because per-image rendered ACK credit stalled playback. Ordinary images on LCD and E-ink still use `RequestFrame`/`pending_frame_id`, send one frame, and admit another only after `RenderedAck` (`service/src/lib.rs`, `application.rs`, `application_jobs.rs`). A missing Pico→Pi TCP range can therefore delay later images after the screen has already drawn. Two saved LCD roundtrip frames completed Pico drawing within 162 ms, while server ACK handling waited 4.28 s and 1.22 s for retransmission. This change addresses the *application's ACK-dependent send permission*, not packet loss or possible TCP head-of-line blocking of a later button notification. The measured delays are LCD evidence; E-ink's expected benefit remains to be verified.

## Proposed policy and ownership

| Boundary | Push-capable LCD and E-ink | Legacy LCD |
|---|---|---|
| Admission | Validate current session, remote owner, normal/sub mode, selected plugin or logo, and terminal-specific image format/length. Allocate a unique frame ID; use the existing transient latest-frame slot, not `pending_frame_id` or a durable per-image outbox effect. | Keep `RequestFrame`, pending frame and ACK credit. |
| Buffer | One immutable frame already being sent plus one replaceable latest unsent image per device. Do not replace a started frame's remaining chunks. Preserve bounded socket writes/deadlines and finish or close on failure. | Existing bounded outbox. |
| Wire/Pico | Ordinary `FrameStart` with no `stream` metadata and existing binary chunks. LCD uses 160×80 RGB565; E-ink uses 250×122 mono1 with its existing refresh policy. Pico still validates CRC/token and sends both ACKs. E-ink copies an accepted image to its display queue and retains at most one pending image while the panel draws. No firmware, SDK or capability change. | Existing contract. |
| ACK | Accept only matching known session/generation/frame IDs. Record current receipt/render observation if available, but neither grants sending credit nor creates an ACK timeout. Ignore late old-generation ACK state changes. | ACKs continue to grant credit and preserve timeout. |
| Selection | A new display epoch invalidates unsent old content; started frame finishes at complete application-message boundaries. Writer checks current generation before starting the latest image. Old ACK must not restore an old selection. | Existing invalidation. |
| Video | Preserve LCD video's `stream` metadata, playback clock, replacement, and optional StreamStatus. E-ink's sampled YouTube images use the same bounded ordinary latest slot, terminal format and refresh policy; they do not claim LCD stream capability. Static latest images are distinguishable in the server's delivery type. | Existing fallback. |

The server's selected plugin and epoch remain domain state. The latest pixel slot is transient transport state; ACK status is observation, not permission. A successful socket write does not claim a physical draw. The sender keeps one immutable active image and one replaceable unsent image, including when an E-ink physical draw continues; it must respect socket bounds and cannot enqueue indefinitely. The firmware's assembly buffer is reused only after completed receipt, while its display queue owns a copy of the drawn pixels. No new management gate or SDK change is introduced.

## Regression obligations and limitations

Check legacy LCD fallback, LCD and E-ink latest replacement under slow writer, E-ink draw-busy/pending and refresh metadata, single images, continuous UP/DOWN and stale generations, delayed duplicate ACKs, current ACK status, video playback, all-OFF logo, disconnect/rebind, and complete-frame/CRC on the wire. A frame already in the kernel cannot be retracted; Pico token/epoch rejection remains necessary. Existing Pico RX capacity and backpressure must be exercised with a slow receiver fixture before deployment. Physical switch delay and original packet-loss location remain unproven until live verification after the server build is reviewed.
