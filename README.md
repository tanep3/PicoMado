# PicoMado — a small window of your own

[日本語で読む](README.ja.md)

A glance at the weather in the morning. A clock while you work. A video or a new drawing when you want a change of pace. PicoMado is a little desktop device you build and give a purpose of your own. Assembly is only the beginning: add plugins and keep making it yours.

![Three PicoMado devices: a color LCD playing video, an E-ink clock, and an E-ink drawing](docs/media/picomado-devices.jpg)

*Three built devices bring small screens and physical buttons onto a desk.*

## Five plugins, five things to enjoy

### YouTube — a tiny video window for your desk

Pick a video in PicoMado's Web UI and play it on the little LCD. Keep a favorite scene or moving view beside you. The fun is seeing it move on a device this small.

![Animated recording of YouTube video playing on a PicoMado LCD device](docs/media/picomado-youtube.gif)

*About seven seconds of playback on the real LCD device, looping here.*

### Weather — a glance before going out

Keep today's forecast and temperature on your desk. Step through the days you want to check. The color LCD and E-ink each give the forecast a different look.

![PicoMado weather forecast on LCD and E-ink devices](docs/media/weather-lcd-eink.jpg)

*The same weather plugin on color LCD and E-ink.*

### Rain radar — the sky on a little map

Display a precipitation map and move through its available times to follow the rain. It is another way to look at the weather from your desk.

![PicoMado rain radar on LCD and E-ink devices](docs/media/rain-radar-lcd-eink.jpg)

*Rain radar on both screens. The time and map are examples from the photo.*

### Caricatures — turn a topic or thought into a picture

Generate a drawing from a news topic or your own words and put it on PicoMado's screen. Set it up easily in PicoMado's Web UI. Revisit saved work and make the little display your own gallery.

![PicoMado caricature drawings on LCD and E-ink devices](docs/media/caricature-lcd-eink.jpg)

*Saved artwork shown on the two screen types.*

### Clock — a familiar place for the time

See the date and time in analog and digital form. It stays useful on your desk between the other things you enjoy.

![PicoMado clock on LCD and E-ink devices](docs/media/clock-lcd-eink.jpg)

*Two clocks running on real devices. Their dates and times are examples from the photo.*

## Keep making it yours

Start with the five included plugins. You can turn another idea into a plugin and give the device a new role. Choosing what to show and how to control it becomes your next build. The [plugin guide](docs/guides/plugin-author.md) shows where to start.

## Build your own version

Start with a color LCD or E-ink example and shape a device around the screen and controls you want. The [hardware guide](docs/guides/hardware-support.en.md) shows checked parts and wiring. If you want to change the display, buttons or microcontroller, the [customization guide](docs/guides/customization.en.md) explains the work and checks involved.

## How it works

```text
Web controls ──> Linux server: plugins, content, settings and saved media
                            │ Wi-Fi / WebSocket
                            ▼
                  Pico W: screen and physical buttons
```

The server prepares images and media; the Pico W receives them, draws them and reports input.

## Get started

1. Choose a starting build from the [hardware guide](docs/guides/hardware-support.en.md).
2. Follow [first setup](docs/guides/first-setup.en.md) to prepare the Linux server and device.
3. Before updating an existing device, read [backup and recovery](docs/guides/release-and-recovery.md).

For deeper detail, see the [firmware](firmware/README.md), [server](service/README.md) and [documentation index](docs/README.md). Development rules and evidence are in [AGENTS.md](AGENTS.md).

Code is licensed under [MIT](LICENSE), copyright Tane Channel Technology. Fonts and other third-party assets have [separate notices](service/assets/NOTO-COPYRIGHT).
