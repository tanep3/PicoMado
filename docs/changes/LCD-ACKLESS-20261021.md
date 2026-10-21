# LCD-ACKLESS-20261021: ordinary LCD latest-frame sending

Status: design for a server-only change. The running Pico firmware, its wire messages and its mandatory receipt/rendered ACK transmissions remain unchanged. This document is committed before the implementation. The intended scope is a terminal that advertises `lcd`, `frame-bytes-v1`, and `lcd-push-v1`; other LCD capabilities and E-ink retain their present ACK-gated contract. The deployed service and physical LCD are not changed by this commit.

## Existing contract gap and cause

`8e41fd6` added bounded latest-frame delivery for LCD video because per-image rendered ACK credit stalled playback. Ordinary LCD still uses `RequestFrame`/`pending_frame_id`, sends one frame, and admits another only after `RenderedAck` (`service/src/lib.rs`, `application.rs`, `application_jobs.rs`). A missing Pico→Pi TCP range can therefore delay later ordinary images after the screen has already drawn. Two saved roundtrip frames completed Pico drawing within 162 ms, while server ACK handling waited 4.28 s and 1.22 s for retransmission. This change addresses the *application's ACK-dependent send permission*, not the packet loss or possible TCP head-of-line blocking of a later button notification.

## Proposed policy and ownership

| Boundary | Push-capable ordinary LCD | Legacy LCD / E-ink |
|---|---|---|
| Admission | Validate current session, remote owner, normal/sub mode, selected plugin or logo, and image length. Allocate a unique frame ID; use the existing transient latest-frame slot, not `pending_frame_id` or a durable per-image outbox effect. | Keep `RequestFrame`, pending frame and ACK credit. |
| Buffer | One immutable frame already being sent plus one replaceable latest unsent image per device. Do not replace a started frame's remaining chunks. Preserve bounded socket writes/deadlines and finish or close on failure. | Existing bounded outbox. |
| Wire/Pico | Ordinary `FrameStart` with no `stream` metadata and existing binary chunks. Pico still validates CRC/token, sends both ACKs and draws synchronously. No firmware, SDK or capability change. | Existing contract. |
| ACK | Accept only matching known session/generation/frame IDs. Record current receipt/render observation if available, but neither grants sending credit nor creates an ACK timeout. Ignore late old-generation ACK state changes. | ACKs continue to grant credit and preserve timeout. |
| Selection | A new display epoch invalidates unsent old content; started frame finishes at complete application-message boundaries. Writer checks current generation before starting the latest image. Old ACK must not restore an old selection. | Existing invalidation. |
| Video | Preserve its `stream` metadata, playback clock, replacement, and optional StreamStatus. Static latest images are distinguishable in the server's delivery type. | Existing video fallback. |

The server's selected plugin and epoch remain domain state. The latest pixel slot is transient transport state; ACK status is observation, not permission. A successful socket write does not claim a physical draw. The existing Pico assembly/draw is sequential, but the server must keep its own active+latest and socket bounds; this is not an unlimited enqueue loop. No new management gate or SDK change is introduced.

## Regression obligations and limitations

Check capability fallback and E-ink credit, static latest replacement under slow writer, continuous UP/DOWN and stale generations, delayed duplicate ACKs, current ACK status, video playback, all-OFF logo, disconnect/rebind, and complete-frame/CRC on the wire. A frame already in the kernel cannot be retracted; Pico token/epoch rejection remains necessary. Existing Pico RX capacity and backpressure must be exercised with a slow receiver fixture before deployment. Physical switch delay and original packet-loss location remain unproven until live verification after the server build is reviewed.
