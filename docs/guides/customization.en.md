# Customize PicoMado

PicoMado combines a screen, buttons and a Pico W into a device you can make your own. Normal builds currently accept only the [checked configurations](hardware-support.en.md). This guide shows where to work when changing the hardware. A new combination earns a physically verified status only after its build, host tests, wiring, display and input have been checked.

## Choose the layer to change

| Goal | Starting point | Current boundary |
| --- | --- | --- |
| Set your Wi-Fi, server address and device name | `firmware/devices/<device>/config.h`, `board-id.txt` | Individual device setup in the [first setup guide](first-setup.en.md) |
| Set LCD backlight connection and polarity, or E-ink panel connection | The same `config.h` | Allowed per-device hardware options; a disconnected panel does not prove drawing |
| Choose plugins, caricature topics and drawing styles | PicoMado Web UI | Does not change firmware hardware definitions |
| Move GPIO or same-kind button wiring | `firmware/profiles/*.h` and the validation/build path | Requires a new tracked profile and tests; cannot be overridden in device `config.h` |
| Change display size/controller, button count/behavior, or MCU | Profile, firmware driver/input, server rendering and protocol | Needs code and contract work; changing profile numbers alone is insufficient |

Adding screen, SPI or button macros, an `#include`, or conditional directives to a new `config.h` is rejected by the [profile validator](../../firmware/tools/validate_hardware_profile.py). Keep verified profiles intact and name a new configuration separately until it has evidence of its own.

## Pico 2 W

The current Pico W profiles identify their board as `pico_w`. `firmware/CMakeLists.txt`, `firmware/tools/build.sh` and `flash.sh` also restrict builds and flashing to the three checked Pico W profiles. A Pico 2 W needs its SDK board setting, CYW43/network path, Flash ID and UF2 checks, memory and timing checked on that board, followed by a distinct profile and physical acceptance. Parts of the display and server message contract may be reusable; the device port has not been done.

## Another screen or size

First identify the module's controller, supply voltage, pinout, SPI mode, dimensions and refresh sequence. The current LCD path assumes a 160×80 ST7735S RGB565 panel; E-ink assumes a 250×122 V4 mono1 panel. The [LCD driver](../../firmware/src/lcd.c) and [E-ink driver](../../firmware/src/display_eink.c) contain geometry, SPI and version guards plus panel-specific drawing. Even a similar module with different pins needs review of fixed SCK/MOSI pins and orientation.

Changing size or pixel format also affects frame length and buffers in firmware, the [wire protocol](../../service/src/protocol.rs), and server rendering or conversion for weather, radar, clock, video and caricatures. Another E-ink revision needs its own initialization, waveform and partial-refresh checks. A new profile file alone does not make an arbitrary panel work.

## Different buttons

Moving pins without changing the button count still requires a new profile and a wiring check. The current four buttons map to SUB/UP/POWER/DOWN; the two-button version has UP/DOWN; the zero-button version has no device buttons. `firmware/src/main.c` handles only 0, 2 and 4 and maps them to logical operations; the LCD backend requires four. Changing the number or meaning of presses also requires input, standby/menu behavior, server input semantics and host tests to be updated. Verify active-low wiring to common GND.

## ESP32 or another MCU

An ESP32 cannot use the Pico SDK, Pico W CYW43 path, SPI and USB CDC integration, Flash ID and UF2 checks, or build/flash scripts unchanged. It needs a device port covering display, buttons, Wi-Fi/WebSocket, persistent state, build and recovery. The server message format and plugin roles can guide the design, but no ESP32 implementation or physical acceptance exists here yet.

## Validate a new configuration

1. Record the exact parts, voltage, wiring, pixel format and controls, and compare them with a checked profile. For code changes, define a change ID and contract first under the [development process](../development-process.md).
2. Add a distinct profile and any needed driver, server rendering and input changes. Extend the allowlists in `firmware/CMakeLists.txt`, `firmware/tools/build.sh` and `flash.sh` only for a configuration that has been verified; do not disable the checks.
3. Run `python3 tools/project_gate.py check`, the relevant host tests and a checked build/artifact review. The current gate evidence is not portable to a fresh computer; see the [first setup limitation](first-setup.en.md#5-check-whether-you-can-build-this-source).
4. After the owner has backed up and completed preflight, physically check supply/GND/GPIO, first drawing, buttons, HOME connection, plugin images and recovery. Record the build result separately from physical acceptance.

For a checked starting build, use [first setup](first-setup.en.md) and the [current hardware table](hardware-support.en.md).
