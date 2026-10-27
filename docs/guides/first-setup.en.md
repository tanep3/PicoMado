# First setup for PicoMado

[日本語](first-setup.md) · [Project overview](../../README.md)

This guide assumes a Linux server and Raspberry Pi Pico W. Run its commands from the repository root. Start by checking [supported parts and wiring](hardware-support.en.md). Do not overwrite an existing configuration or database with the examples.

## 1. Prepare the tools

Install [Rust and Cargo](https://rust-lang.org/tools/install/) for the server, and a C/C++ toolchain, CMake, Python 3.8 or newer, the [Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html), and [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) for firmware. Follow Raspberry Pi's SDK instructions and set `PICO_SDK_PATH` to its actual location. Cargo downloads missing dependencies when you first build; `cargo fetch --locked --manifest-path service/Cargo.toml` can be used in advance if you want an offline build later.

YouTube playback needs `yt-dlp`, FFmpeg and FFprobe; yt-dlp may also require a JavaScript runtime. The optional full test suite uses FFmpeg, FFprobe and `nice`.

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

Keep your `config.h` and Wi-Fi credentials private.

## 4. Start with HOME

Put the server and Pico W on a LAN where they can reach each other. `PM_HOME_SERVER_IP` in firmware must be the same address as the server's `bind`; allow TCP port `16153` between them and the browser you use for the web interface. The current server does not provide a built-in web login or PicoMado API key, so use a trusted LAN.

## 5. Build

From the source root, build the server and firmware:

```sh
PM_DEVICE=my-pico
cargo build --release --locked --manifest-path service/Cargo.toml
firmware/tools/build.sh --device "$PM_DEVICE"
```

The server is `service/target/release/picomado-server`; the UF2 is `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2`. For different wiring, create a profile in `firmware/profiles/` and select it in `profile.txt`. The firmware build checks the selected MCU, display driver and pin conflicts, but it cannot verify your physical wiring. You can rerun the commands after editing source or settings. Tests are optional; for example, run `cargo test --locked --all-features --manifest-path service/Cargo.toml` for the server.

## 6. Install on your own server and device

For a new server installation:

```sh
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 service/target/release/picomado-server "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

The unit points to `~/.cargo/bin/picomado-server`. Expect `active`; otherwise inspect `systemctl --user status picomado.service` and your TOML configuration. Open the configured IPv4 and port `16153` on your trusted LAN.

Put the Pico W in BOOTSEL mode, then copy `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2` to the `RPI-RP2` drive. It restarts after the copy. Check HOME Wi-Fi, server connection, the display and, where present, buttons separately. No Flash ID file or approval record is required.

## 7. Updating an existing device

Begin with [backup and recovery](release-and-recovery.en.md), not with the new-install copy commands. Keep a consistent copy of the old executable, config, SQLite database, plugin `data/`, and the old UF2. Never copy just a live SQLite DB file. Update the server and device in separate steps and repeat web, HOME connection, drawing and input checks. Restore the saved combination if the update fails.

## 8. Optional AWAY connection

Confirm HOME first. A production firmware build requires EEX1 discovery for its AWAY SOCKS5 endpoint. AWAY also needs a separate Android EEX1 app, phone tethering, TailSocks and a route that can actually reach the server. That Android app is not part of this repository. See the [EEX1 design and remaining checks](../design/eex1-endpoint-discovery.md). If you only use HOME, leave AWAY unselected.

## 9. Troubleshooting

For a server configuration failure, check TOML syntax, file paths and bind IP. For a connection failure, compare the server and device IP, HOME SSID and port. For a blank display, compare panel version, profile, power, common GND and GPIO. Do not paste Wi-Fi passwords or identifying device information into public issue logs.
