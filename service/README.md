# PicoMado server

The Linux server prepares plugin content, keeps device settings and media, and serves the browser controls and Pico W WebSocket connection. Start with [first setup (English)](../docs/guides/first-setup.en.md) or [初回設定 (日本語)](../docs/guides/first-setup.md). For an existing installation, read [backup and recovery](../docs/guides/release-and-recovery.en.md) before replacing anything.

## What runs here

`picomado-server` loads the user-service configuration, starts the HTTP/WebSocket host, and runs plugin fetch and job workers. The library crate itself does not start a listener when imported. Built-in plugins provide a clock, JMA weather, JMA rain radar, LCD YouTube playback, and caricature generation and history. YouTube playback needs `yt-dlp`, `ffmpeg` and `ffprobe`; a JavaScript runtime may also be required by the installed yt-dlp. Caricature generation needs its separately configured provider and tools. The Pico W receives prepared frames and sends button and display acknowledgements.

The browser interface and device endpoint have no built-in login or PicoMado API key. Configure the server for a trusted LAN and check its bind address before starting it. A separate `temporary_lan_host` exists for an isolated operator trial, with an explicit unicast bind address; it is not the normal installation route.

## Configuration and installation

The service reads `$HOME/.config/PicoMado/config.toml` by default; [the generic example](deploy/config.toml) documents its fields. The `bind` address must be the Linux server's actual LAN IPv4 and port, not the documentation address in the example. SQLite state and plugin `data/` and `cache/` directories are configured separately. Protect an existing database and saved media before changing these paths.

The [first setup guide](../docs/guides/first-setup.en.md) covers dependencies, configuration, public checks, build, preflight, installation and verification. The [user-service notes](deploy/README.md) describe the unit and file layout.

## Code and verification

The main paths are `src/application.rs` for persistent application state, `src/plugins.rs` and plugin modules for content, `src/transport.rs` and `src/protocol.rs` for device communication, and `src/web.rs` for browser controls. SQLite snapshots and effect outbox work are separated from physical frame state; receipt of a frame and completion of drawing are separate acknowledgements. Public source checks run through `python3 tools/public_gate.py check` as described in first setup.

LCD and both zero- and two-button E-ink configurations are supported as listed in [hardware support](../docs/guides/hardware-support.en.md). Long-duration and multi-device acceptance remain open.

The bundled weather and radar glyph assets and the default word card are derived from Noto Sans CJK JP; see [Noto copyright and license](assets/NOTO-COPYRIGHT) and [asset sources](assets/README.md). PicoMado itself is distributed under the [MIT License](../LICENSE).
