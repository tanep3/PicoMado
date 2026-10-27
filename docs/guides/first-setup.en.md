# First setup for PicoMado

[日本語](first-setup.md) · [Project overview](../../README.md)

This guide assumes a Linux server and Raspberry Pi Pico W. Run its commands from the repository root. Start by checking [supported parts and wiring](hardware-support.en.md). Do not overwrite an existing configuration or database with the examples.

## 1. Prepare the tools

Install [Rust and Cargo](https://rust-lang.org/tools/install/) for the server, and a C/C++ toolchain, CMake, Python 3 and the [Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html) for firmware. Install [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) for UF2 conversion and board identification. Follow the official Raspberry Pi setup instructions for a matching SDK and C/C++ toolchain. Check `rustc --version`, `cargo --version`, `cmake --version`, `python3 --version` and `picotool version`. Point `PICO_SDK_PATH` at your installed SDK; for example, `export PICO_SDK_PATH="$HOME/pico-sdk"` if it is really there.

From the repository root, run `cargo fetch --locked --manifest-path service/Cargo.toml` once to download the locked dependencies. This requires network access. Subsequent gate checks and builds use `--offline --locked`.

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

The real `board-id.txt` must contain the selected board's 16-digit Flash ID. Put only one Pico W in BOOTSEL mode and inspect `picotool info -d`; do not guess the ID. Keep `config.h` and the real board ID out of Git.

## 4. Start with HOME

Put the server and Pico W on a LAN where they can reach each other. `PM_HOME_SERVER_IP` in firmware must be the same address as the server's `bind`; allow TCP port `16153` between them and the browser you use for the web interface. The current server does not provide a built-in web login or PicoMado API key, so use a trusted LAN.

## 5. Check whether you can build this source

Source builds use the project gate. If you create a new wiring profile, keep its `.h` and `.board` files under `firmware/profiles/`, select it in `profile.txt`, and commit the new profile before building. The gate checks the source and binds the build to that profile; it does not certify your wiring on hardware. A separate review is required before installation.
To check and build the source:

```sh
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --build-dir /tmp/picomado-server-build
python3 tools/project_gate.py record --build-receipt /tmp/picomado-server-build/build-receipt.json --artifact /tmp/picomado-server-build/release/picomado-server --kind server --out /tmp/picomado-server-build/candidate.json
PM_DEVICE=my-pico
firmware/tools/build.sh --device "$PM_DEVICE"
```

Use a fresh empty build directory each time. The server candidate is `/tmp/picomado-server-build/release/picomado-server`; the device candidate is `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2`. `record` records a candidate; it does not approve one. Once a separate reviewer supplies `review.json` tied to that candidate and artifact, check the server with `python3 tools/project_gate.py preflight --candidate /tmp/picomado-server-build/candidate.json --review /tmp/picomado-server-build/review.json --artifact /tmp/picomado-server-build/release/picomado-server`, and the device with `flash.sh --verify-only`. Do not manufacture an approval file yourself. A passing build is not a physical display check.

## 6. Install a checked candidate on your own new device

The commands here are for the owner of the Linux server and Pico W **after** a matching candidate, independent review and `project_gate.py preflight` have succeeded.

```sh
PM_RELEASE_BIN=/tmp/picomado-server-build/release/picomado-server
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 "$PM_RELEASE_BIN" "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

The example unit's `ExecStart` points to `~/.cargo/bin/picomado-server`; edit your unit if you install elsewhere. Expect `active` from `is-active`. If startup fails, use `systemctl --user status picomado.service`, then check the TOML path, bind address and port. Open the web page at your configured IPv4 and port `16153`.

For the firmware, make sure the matching `candidate.json` and `review.json` are present and offline preflight succeeded. Connect **one** target Pico W in BOOTSEL mode. The first command only checks the board and artifact; the second writes and verifies the UF2:

```sh
firmware/tools/flash.sh --device "$PM_DEVICE" --check-only
firmware/tools/flash.sh --device "$PM_DEVICE"
```

Stop if the board ID, manifest, candidate or review does not match. After restart, check HOME Wi-Fi, server Hello, an actual image on the display and, where present, button input as separate observations. The zero-button E-ink profile has no button check.

## 7. Updating an existing device

Begin with [backup and recovery](release-and-recovery.md#更新前の退避), not with the new-install copy commands. Keep a consistent copy of the old executable, config, SQLite database, plugin `data/`, and the old UF2 and manifest for the same device. Never copy just a live SQLite DB file. After preflight, update the server and device in separate steps and repeat web, HOME connection, drawing and input checks. Restore the saved combination if the update fails.

## 8. Optional AWAY connection

Confirm HOME first. A production firmware build requires EEX1 discovery for its AWAY SOCKS5 endpoint. AWAY also needs a separate Android EEX1 app, phone tethering, TailSocks and a route that can actually reach the server. That Android app is not part of this repository. See the [EEX1 design and remaining checks](../design/eex1-endpoint-discovery.md). If you only use HOME, leave AWAY unselected.

## 9. Troubleshooting

For a server configuration failure, check TOML syntax, file paths and bind IP. For a connection failure, compare the server and device IP, HOME SSID and port. For a blank display, compare panel version, profile, power, common GND and GPIO. Do not paste Wi-Fi passwords or a real Flash ID into public issue logs.
