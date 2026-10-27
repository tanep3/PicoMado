# PicoMado — a small window of your own

[日本語で読む](README.ja.md)

Build a little device for your desk, then keep making it yours. PicoMado puts a tiny screen and physical buttons in your hands. Its plugins give it something new to show long after the hardware is assembled.

PicoMado already has five built-in experiences. Pick one on the device or manage it in a browser:

| Plugin | What you can do |
| --- | --- |
| **YouTube** | Prepare a video on the server and play it on the small LCD. Browse and manage saved sources from the web interface. |
| **Weather** | Read a Japanese weather forecast and move through its days with the device controls. |
| **Rain radar** | Look at a compact Japanese precipitation map and step through available times. |
| **Caricature** | Generate a drawing from a news topic or your own text, then browse saved drawings. This needs the separate generation tools configured on the server. |
| **Clock** | Keep a date, analog clock, and 24-hour time in view. |

The images below are **software-rendered previews**, not photos of a finished device. The repository does not yet contain publishable device photos or a demo video.

![Clock LCD render preview](service/assets/clock-lcd-preview.png) ![Rain radar LCD render preview](service/assets/radar-lcd-preview.png)

## Make it yours

The display, buttons, firmware, server and web controls form one device you can use at your desk. Add another use by implementing a server plugin and registering it with the shared plugin API; [the minimal plugin example](docs/guides/plugin-author.md) shows the starting point. That example is not installed by default. This repository does not include an enclosure design, so choose or build a housing for your particular screen and wiring.

## How it works

```text
Web controls ──> Linux server: plugins, content, settings and saved media
                            │ Wi-Fi / WebSocket
                            ▼
                  Pico W: screen and physical buttons
```

The server prepares images and media; the Pico W receives them, draws them and reports input. The server and device each have their own configuration. Some plugins need internet access or additional local tools.

## Choose a build

The supported profiles are Pico W with a 160×80 ST7735S LCD and four buttons, or Pico W with a 250×122 V4 E-ink display and zero or two buttons. The LCD four-button and E-ink zero-button builds have physical verification records. A complete E-ink two-button device has not yet passed physical acceptance. [Check exact parts, wiring and status](docs/guides/hardware-support.md) before buying or connecting components. Pico 2 W, other screens and arbitrary button layouts are not supported.

## Get started

1. Choose parts and wiring from the [hardware guide](docs/guides/hardware-support.md).
2. Follow the [first setup guide](docs/guides/first-setup.md) for the Linux server, device configuration, gated build and verification. The guide is currently in Japanese.
3. Use the [recovery guide](docs/guides/release-and-recovery.md) before updating an existing device.

For deeper detail, see the [firmware](firmware/README.md), [server](service/README.md) and [documentation index](docs/README.md). Development rules and evidence are in [AGENTS.md](AGENTS.md).

Code is licensed under [MIT](LICENSE), copyright Tane Channel Technology. Fonts and other third-party assets have [separate notices](service/assets/NOTO-COPYRIGHT).
