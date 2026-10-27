# PicoMado server

The Linux server prepares plugin content, keeps device settings and media, and serves the browser controls and Pico W WebSocket connection. Start with [first setup (English)](../docs/guides/first-setup.en.md) or [初回設定 (日本語)](../docs/guides/first-setup.md). For an existing installation, read [backup and recovery](../docs/guides/release-and-recovery.md) before replacing anything.

## What runs here

`picomado-server` loads the user-service configuration, starts the HTTP/WebSocket host, and runs plugin fetch and job workers. The library crate itself does not start a listener when imported. Built-in plugins provide a clock, JMA weather, JMA rain radar, LCD YouTube playback, and caricature generation and history. YouTube conversion and caricature generation need additional server-side tools; their availability depends on the local installation. The Pico W receives prepared frames and sends button and display acknowledgements.

The browser interface and device endpoint have no built-in login or PicoMado API key. Configure the server for a trusted LAN and check its bind address before starting it. A separate `temporary_lan_host` exists for an isolated operator trial, with an explicit unicast bind address; it is not the normal installation route.

## Configuration and installation

The service reads `$HOME/.config/PicoMado/config.toml` by default; [the generic example](deploy/config.toml) documents its fields. The `bind` address must be the Linux server's actual LAN IPv4 and port, not the documentation address in the example. SQLite state and plugin `data/` and `cache/` directories are configured separately. Protect an existing database and saved media before changing these paths.

The [first setup guide](../docs/guides/first-setup.en.md) covers dependencies, configuration, the root gate, a checked build candidate, preflight, installation, startup and verification. The [user-service notes](deploy/README.md) describe the unit and file layout. A fresh external clone cannot yet pass the development gate from this repository alone because older change evidence uses machine-local absolute paths; the setup guide identifies this public self-build blocker. Do not treat a local test pass as a completed installation.

## Code and verification

The main paths are `src/application.rs` for persistent application state, `src/plugins.rs` and plugin modules for content, `src/transport.rs` and `src/protocol.rs` for device communication, and `src/web.rs` for browser controls. SQLite snapshots and effect outbox work are separated from physical frame state; receipt of a frame and completion of drawing are separate acknowledgements. Development checks run through `python3 tools/project_gate.py check` from the repository root, following [the development process](../docs/development-process.md).

Physical LCD and zero-button E-ink runs are recorded, while long-duration and multi-device acceptance remain open. The two-button E-ink build has no completed physical acceptance. See [hardware support](../docs/guides/hardware-support.en.md) for exact profiles and status.

## Historical record

The former long README, including dated observations and old `cargo install` / `systemctl` examples, is preserved in the [historical development record](history/development-record-before-20261027.md). Those examples describe past operations and are not current installation instructions.
