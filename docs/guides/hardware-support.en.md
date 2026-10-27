# Supported hardware and wiring

[日本語](hardware-support.md) · [Project overview](../../README.md)

Choose a profile before buying parts or wiring the board. For different wiring with an implemented display and button count, create a new profile. PicoMado currently supports Raspberry Pi **Pico W**, with these display and input combinations:

| Profile | Display | Buttons | Status |
| --- | --- | --- | --- |
| `pico_w_lcd_160x80_4` | ST7735S LCD, 160×80, RGB565 | 4 | Verified |
| `pico_w_eink_250x122_0` | 2.13-inch V4 E-ink, 250×122 | 0 | Verified |
| `pico_w_eink_250x122_2` | Same V4 E-ink | 2 | Verified |

Pico 2 W, ESP32, other display versions or sizes, speakers, microphones and servos need added implementation. New button wiring can be built with a new profile; the [customization guide](customization.en.md) shows what would need to change and be tested. Match the display controller and panel version to the profile. Do not change wiring while power is connected.

The GPIO values below come from the tracked files in `firmware/profiles/`. **GPIO numbers are not physical pin numbers.** The profile does not specify a particular module's power-pin label or safe supply voltage. Read your module's specification, connect it to an appropriate Pico W supply, and share GND with the Pico W. Do not power an unidentified module.

| LCD profile | Pico W GPIO |
| --- | --- |
| LCD CS / SCK / MOSI / DC / RESET | GP17 / GP18 / GP19 / GP20 / GP21 |
| LCD BLK, if wired | GP22; set `PM_LCD_BLK_CONNECTED` and polarity to match the actual module |
| SUB / UP / POWER / DOWN buttons | GP12 / GP13 / GP14 / GP15; each button closes to common GND |
| Display power / GND | Supply specified for your module / common Pico W GND |

| V4 E-ink profile | Pico W GPIO |
| --- | --- |
| Display CS / SCK / MOSI / DC / RESET / BUSY | GP9 / GP10 / GP11 / GP8 / GP12 / GP13 |
| UP / DOWN, two-button version | GP20 / GP21; each button closes to common GND |
| Zero-button version | No button wiring; it cannot be switched by a device button |
| Display power / GND | Supply specified for your V4 module / common Pico W GND |

The tracked profile owns display geometry, controller, SPI and button GPIO. A device's `config.h` may set only the LCD backlight connection and polarity, or whether the E-ink panel is connected. `PM_EINK_PANEL_CONNECTED=0` allows a no-panel check but proves nothing about physical drawing. Older flat hardware definitions are accepted only when they match the selected profile exactly. See the [first setup guide](first-setup.en.md) for device configuration.

An enclosure design is not included. Choose or design a housing for your screen, buttons and wiring.
