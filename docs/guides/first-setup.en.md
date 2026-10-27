# First setup for PicoMado

[日本語](first-setup.md) · [Project overview](../../README.md)

This guide assumes a Linux server and Raspberry Pi Pico W. Run its commands from the repository root. Start by checking [supported parts and wiring](hardware-support.en.md). Do not overwrite an existing configuration or database with the examples.

## 1. Prepare the tools

Install [Rust and Cargo](https://rust-lang.org/tools/install/) for the server, and a C/C++ toolchain, CMake, Python 3.11 or newer, Git and the [Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html) for firmware. Install [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) for UF2 conversion and board identification. Follow the official Raspberry Pi setup instructions for a matching SDK and C/C++ toolchain. Check `rustc --version`, `cargo --version`, `cmake --version`, `python3 --version`, `git --version` and `picotool version`. Point `PICO_SDK_PATH` at your installed SDK; for example, `export PICO_SDK_PATH="$HOME/pico-sdk"` if it is really there.
The firmware build records the SDK checkout and its submodules. Use a complete Git checkout with initialized submodules, and keep the SDK and its submodules clean; an SDK installed without Git metadata cannot pass this build provenance check. Check `git -C "$PICO_SDK_PATH" status --porcelain` and `git -C "$PICO_SDK_PATH" submodule status --recursive` before building.

The full offline check also requires `ffmpeg`, `ffprobe`, and GNU `nice`; FFmpeg must provide the MPEG-4 encoder used by the tests. Check `ffmpeg -version`, `ffprobe -version`, and `nice --version`. `ffmpeg -hide_banner -encoders | grep -E "(^|[[:space:]])mpeg4[[:space:]]"` should show an encoder line. YouTube playback additionally requires `yt-dlp`, FFmpeg and FFprobe; install a JavaScript runtime if your yt-dlp setup requires one.

From the repository root, run `cargo fetch --locked --manifest-path service/Cargo.toml` once to download the locked dependencies. This requires network access. Subsequent gate checks and builds use `--offline --locked`.

Before changing files, run `python3 tools/public_gate.py verify-source` from the distribution root to check the included manifest. Also compare the archive SHA256 through a trusted channel.

## 2. Configure the Linux server

For a new installation, copy the example without replacing an existing file:

```sh
mkdir -p "$HOME/.config/PicoMado"
cp -n service/deploy/config.toml "$HOME/.config/PicoMado/config.toml"
```

Edit `bind` in the copied TOML file to your Linux server's own LAN IPv4 address and port `16153`. The example `192.0.2.20` is a documentation address and will not work unchanged. `ip -4 addr show` can help identify the server address. `0.0.0.0` is rejected. `database_path` is the SQLite state file; `plugins_root` contains plugin data and cache. The example `~/` expands under the running user's HOME. Protect existing state before changing those paths. The executable reads this file by default or accepts `picomado-server --config /absolute/path/config.toml`.

## 3. Configure one device

Choose a profile from the [hardware guide](hardware-support.en.md). Give the device a stable name containing letters, digits, `_` or `-`; the directory name must equal `PM_DEVICE_ID` in `config.h`. The LCD sample can be copied like this:

```sh
PM_DEVICE=my-pico
mkdir -p "firmware/devices/$PM_DEVICE"
cp firmware/devices/sample-a/config.sample.h "firmware/devices/$PM_DEVICE/config.h"
cp firmware/devices/sample-a/profile.txt "firmware/devices/$PM_DEVICE/profile.txt"
```

Edit the copied `config.h`: replace `PM_DEVICE_ID`, HOME Wi-Fi SSID and password, and `PM_HOME_SERVER_IP` with your device name and LAN settings. The sample addresses and credentials cannot connect. Firmware also references every `PM_AWAY_*` macro even when you only use HOME, so keep all the dummy AWAY definitions for now and do not select AWAY. If you intend to use AWAY later, configure those values separately as described in step 8. For E-ink, use the matching `E-ink_0` or `E-ink_1` sample and profile instead of the LCD sample.

The `profile.txt` selects display type, SPI and button pins. A new `config.h` must not define those hardware macros or include another header. Only an LCD backlight's connection and polarity, or the E-ink panel connection, may vary by individual device. Connect the appropriate display power and common GND as specified by its module maker; check all GPIO against the [wiring table](hardware-support.en.md) before powering it.

The real `board-id.txt` must contain the selected board's 16-digit Flash ID. Put only one Pico W in BOOTSEL mode and inspect `picotool info -d`; do not guess the ID. Create `firmware/devices/<device>/board-id.txt` in a text editor and save only the confirmed 16 digits on one line. Keep `config.h` and the real board ID private.

## 4. Start with HOME

Put the server and Pico W on a LAN where they can reach each other. `PM_HOME_SERVER_IP` in firmware must be the same address as the server's `bind`; allow TCP port `16153` between them and the browser you use for the web interface. The current server does not provide a built-in web login or PicoMado API key, so use a trusted LAN.

## 5. Check and build

From the distribution root, run the following after configuring the device. The check runs offline, including firmware host tests and Rust tests that use local loopback. The output and logs are private; they may contain local paths. Choose a fresh directory. For custom wiring, add the `.h` and `.board` profile files under `firmware/profiles/` and select them in `profile.txt`. The checks bind the build to the selected profile; they cannot verify your physical wiring.

```sh
PM_DEVICE=my-pico
PM_RUN=$(mktemp -d /tmp/picomado.XXXXXX)
python3 tools/public_gate.py check --out "$PM_RUN/check"
python3 tools/public_gate.py build --kind server --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/server"
python3 tools/public_gate.py build --kind firmware --device "$PM_DEVICE" --report "$PM_RUN/check/check.json" --build-dir "$PWD/firmware/build/$PM_DEVICE"
python3 tools/public_gate.py preflight --receipt "$PM_RUN/server/public-build.json"
python3 tools/public_gate.py preflight --receipt "firmware/build/$PM_DEVICE/public-build.json"
```

The firmware directory must be fresh. Before rebuilding, privately back up the earlier build directory and its sibling `firmware/build/$PM_DEVICE-configure.log` and `firmware/build/$PM_DEVICE-build.log`. Preflight checks source, toolchain, SDK, configuration and artifact identity. The files are local integrity records, not a security signature or independent review. A successful build does not establish physical operation.

## 6. Install on your own server and device

After both preflights succeed, install the checked server executable:

```sh
PM_RELEASE_BIN="$PM_RUN/server/release/picomado-server"
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 "$PM_RELEASE_BIN" "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

The unit points to `~/.cargo/bin/picomado-server`. Expect `active`; otherwise inspect `systemctl --user status picomado.service` and your TOML configuration. Open the configured IPv4 and port `16153` on your trusted LAN.

Connect exactly one Pico W in BOOTSEL mode. The first command checks the current device and artifact; the second repeats those checks, writes and verifies the UF2:

```sh
python3 tools/public_gate.py flash --receipt "firmware/build/$PM_DEVICE/public-build.json" --device "$PM_DEVICE" --check-only
python3 tools/public_gate.py flash --receipt "firmware/build/$PM_DEVICE/public-build.json" --device "$PM_DEVICE"
```

Stop on any mismatch. After restart, check HOME Wi-Fi, server Hello, the display and, where present, buttons separately. The zero-button E-ink profile has no button check.

## 7. Updating an existing device

Begin with [backup and recovery](release-and-recovery.en.md), not with the new-install copy commands. Keep a consistent copy of the old executable, config, SQLite database, plugin `data/`, and the old UF2 and manifest for the same device. Never copy just a live SQLite DB file. After the public preflight, update the server and device in separate steps and repeat web, HOME connection, drawing and input checks. Restore the saved combination if the update fails.

## 8. Optional AWAY connection

Confirm HOME first. A production firmware build requires EEX1 discovery for its AWAY SOCKS5 endpoint. AWAY also needs a separate Android EEX1 app, phone tethering, TailSocks and a route that can actually reach the server. That Android app is not part of this repository. See the [EEX1 design and remaining checks](../design/eex1-endpoint-discovery.md). If you only use HOME, leave AWAY unselected.

## 9. Troubleshooting

For a server configuration failure, check TOML syntax, file paths and bind IP. For a connection failure, compare the server and device IP, HOME SSID and port. For a blank display, compare panel version, profile, power, common GND and GPIO. Do not paste Wi-Fi passwords or a real Flash ID into public issue logs.
