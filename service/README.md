# PicoMado service library

The crate contains a pure transition engine, settings model, SQLite application boundary, Web/API/UI shell, versioned control and image protocol, WebSocket adapter, bounded queue model, cached JMA weather and rain-radar renderers, plugin fetch cycle, and replaceable effect runner. Importing the library never starts a listener or background worker. `picomado-server` reads its dedicated user-service configuration, while `temporary_lan_host` supports an explicit trial; both bind only `192.168.0.120:16153`. No PicoMado API key or Web login is added.

## Current behavior

- `application.rs` stores world state, device settings, shared plugin settings, plugin-owned snapshots, and effect outbox in one SQLite transaction using WAL and schema version 2. It assumes one application writer; cross-process compare-and-swap is not implemented. A restarted application restores registered devices offline and retains pending fetch work. Sessions, physical frame state, and input mode are transient. This new-project draft has no schema migration from version 1.
- `App::hello` automatically registers the device through the existing Engine. Device registration and settings use the same persisted incarnation. Web writes require expected revisions; device writes and deletion also require the incarnation. Offline edits persist. A new session receives the desired settings snapshot even when the previous session had confirmed it.
- `App::settings_stored` records the Pico NVM acknowledgement. `App::heartbeat` requires the current session and confirms application separately. `App::terminal_event` distinguishes receipt of a frame from completion of physical drawing. Send completion only marks the outbox effect complete; it does not claim either ACK.
- `runner.rs` processes persisted effects through an injected executor. A fetch result and removal of its outbox item commit together. An executor must be idempotent by outbox ID and generation because a crash after an external action can replay it. The older generic fetch outbox has a placeholder executor. `fetch.rs` plans both JMA plugins through their content contracts, groups jobs by plugin/source/date/policy so devices using one source share one HTTPS fetch, and applies the result against each device generation. Different source groups run with a concurrency limit of two. The host checks for newly due work every second; each plugin uses its last successful fetch for the one-hour freshness deadline and a separate 60-second cooldown after failure. `host.rs` stops the listener when the worker fails and bounds graceful shutdown. The live adapter uses cancellable async HTTPS outside the application lock. Weather HTTP accepts only catalogued office codes; radar HTTP uses fixed public JMA nowcast paths.
- `protocol.rs` frames WebSocket application messages with a 4-byte big-endian length, version, kind, and JSON body. LCD frames use `FrameStart` (160×80, RGB565 big-endian, 25,600 bytes, CRC32, full frame token) plus ordered `FrameChunk` messages of at most 4,096 pixel bytes. The mock receiver combines format, length, offset, token, and CRC checks with `PicoScreen` generation and local-owner gates. It emits receipt ACK only for a complete current frame and rendered ACK only after simulated drawing finishes under the current owner. Dispatch-path tests deliver a partially received frame across plugin switch, sub-mode cancellation, standby, and reconnect; valid old bytes complete in the raw assembler but the gated terminal emits no ACK. JSON numeric arrays are a provisional wire encoding for chunks; Pico memory and bandwidth must be measured. The decoder accepts partial and consecutive frames and rejects bad lengths, versions, kinds, and JSON. `transport.rs` handles Hello, settings synchronization, heartbeat, generic plugin input, images, and ACKs over WebSocket. Weather input interpretation stays in `WeatherContent`.
- `queue.rs` bounds encoded bytes per device, prioritizes control, drops stale video, and supports round-robin selection. The WebSocket sender now queues one completed LCD frame per dispatch, with its start in the control lane and fragments in the image lane. Frames are transient assets outside SQLite; unsupported/unknown handles stay pending. WebSocket receiving and writing run in independent tasks. Image writes have a provisional 2 s stall limit. The full image has a bounded deadline calculated from encoded wire bytes at 12 KiB/s plus 2 s setup time, capped at 20 s; a measured weather image expands from 25,600 pixel bytes to about 100,670 wire bytes and receives an 11 s deadline. The outbox/display generation is rechecked before every fragment. Start, completion, and failure logs record the frame ID, payload/wire/sent byte counts, duration, and outcome. A stalled writer ends the whole WebSocket session, so the Pico can reconnect and receive a fresh cached frame; an in-process test verifies the new frame reaches both ACKs. Other mocks exercise a blocked terminal receiving input while a healthy peer receives two complete images. These timing limits and queue caps require device measurement.
- `plugins.rs` composes built-in Rule, content, and HTTP source adapters; application and transport dispatch without branches for individual plugins. `web.rs` provides the home and four plugin page shells, device list and changes, shared order, and plugin settings through a 16 KiB-limited JSON API. `content.rs` defines plugin-owned content, calendar observation, and input planning. Weather renders seven JMA forecast days from the bundled nationwide location catalogue. It rebases the normal view to the actual JST date on input/connection and shows an error image when that date is absent from the cached set; a browsing snapshot stays pinned. Rain radar selects only observed/forecast timestamps actually listed by JMA at exact hourly offsets around the latest observation. It combines JMA precipitation PNG tiles with the saved JMA-hosted GSI base map and JMA no-data coverage into RGB565 LCD frames. Strong coast strokes preserve the Japanese outline at 160×80; a packed 250×122 monochrome preview is available for E-ink design checks, but the transport has no E-ink frame path yet. Rain-radar and weather retain the viewed set during background updates, stop at timeline ends, and return to the newest default view on cancel. Fetch failure shows the plugin error frame even during browsing without moving the pinned position. Only YouTube and caricature content executors remain stubs.

The outbox cap of 1,024 effects, 4 KiB control-message cap, 64 KiB WebSocket binary chunk cap, 256 KiB per-device queue, 80 KiB control reserve, 200 ms dispatch poll, and eight control sends per dispatcher tick are **provisional safety limits**, not approved acceptance thresholds. They require Pico memory and multi-device measurements. The user-approved port, connection, weather, button, and FPS defaults are listed separately in the requirements.

## Verification

From the repository root:

```sh
CARGO_TARGET_DIR=/tmp/picomado-core-target cargo test --offline --manifest-path service/Cargo.toml
cargo fmt --manifest-path service/Cargo.toml --check
CARGO_TARGET_DIR=/tmp/picomado-core-target cargo clippy --offline --manifest-path service/Cargo.toml --all-targets -- -D warnings
```

The public-network test runs only when explicitly requested:

```sh
CARGO_TARGET_DIR=/tmp/picomado-core-target cargo test --manifest-path service/Cargo.toml --test live_https -- --ignored
```

## User-service deployment

The install root in the existing Cargo configuration is `/home/tane`, so
`cargo install --path .` from `service/` installs the binaries in
`/home/tane/bin`. The dedicated layout and unit templates are in
[`deploy/`](deploy/README.md). The service uses
`/home/tane/.config/PicoMado/config.toml`, the common database at
`data/picomado.db`, and plugin-specific `data/` and `cache/` directories under
`youtube/`, `caricature/`, `weather/`, and `rain-radar/`. Logs use the user
journal. Existing configuration, data, and unrelated services must be checked
and retained during installation.

```sh
cd service
cargo install --offline --path .
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

## Explicit LAN trial

The standalone trial host uses an isolated `/tmp` DB and cannot share the port
with the user service. A read-only check on 2026-10-12 found
`192.168.0.120/24` on `wlan0` and no earlier TCP 16153 listener. Recheck
both before a new trial because either can change. No SSID, password, or SOCKS
credential belongs in this command or database setup.

```sh
ip -4 addr show
ss -ltn '( sport = :16153 )'
mkdir -p /tmp/picomado-lan-trial
CARGO_TARGET_DIR=/tmp/picomado-task4-target cargo run --offline --manifest-path service/Cargo.toml --bin temporary_lan_host -- --temporary --bind 192.168.0.120:16153 --db /tmp/picomado-lan-trial/state.db
```

The host accepts that address only, starts the HTTP/WebSocket service and JMA fetch worker together, and stops on Ctrl+C. Stop it before starting the user service, after copying only its own SQLite state when continuity is needed. The firmware initiates `Hello` at `/device`, which creates its device record and default settings.

Tests use temporary SQLite files, fake clocks and fetchers, and in-process HTTP calls. Mock-terminal integration tests start HTTP/WebSocket listeners on OS-selected `127.0.0.1` ports and abort them at the end; they never bind a LAN address or the production port. No Pico, existing project, or service is accessed.

## Remaining integration work

Real device communication, durable transport session ownership, production deployment, and source migration remain unverified. The bundled nationwide weather catalogue is validated against a saved official forecast snapshot for each of its 139 selectable mappings; the two excluded areas and reason are recorded in `assets/README.md`. An opt-in live test exercised the service's Tokyo HTTPS cycle, but a one-shot live JMA radar fetch on 2026-10-12 retrieved five time slices and rendered them successfully after bounded gzip expansion of the no-data GeoJSON; the saved generation and mocked terminal path are also tested. The GSI base-map snapshot and simplified JMA no-data polygons are deterministic test inputs; the live adapter fetches current JMA times, rain tiles, and no-data layers. Coastline clarity at final LCD/E-ink resolution and physical draw timing still require device inspection. E-ink transport frames remain unimplemented. Unsupported generic show/fetch effects remain pending until a real executor reports the proper result. Completed LCD frame sends clear their own outbox effect without claiming a terminal ACK.


## 2026-10-13 checkpoint

The local suite passes 89 tests (one live HTTPS test remains opt-in), with fmt and warning-free clippy. Native Japanese glyph/metrics consistency, all 139 selectable weather locations, text bounds, missing/signed temperatures, cached radar captions, and mock terminal delivery are covered. The user confirmed actual LCD plugin images and the enlarged Japanese text after deployment. This is a checkpoint, not completion of all acceptance work: HOME button operations still have intermittent WebSocket disconnects and AWAY has not connected. The failure stage and reconnect causes are not yet established. See the current checkpoint and remaining issues in [firmware verification](../firmware/verification-2026-10-12.md). YouTube/caricature executors, E-ink transport/physical tests, and multi-device performance remain incomplete.
