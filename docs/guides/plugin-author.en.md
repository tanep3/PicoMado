# Make a favorite words plugin

[日本語](plugin-author.md) · [First setup](first-setup.en.md)

Put “おはよう” in white on a dark blue 160×80 color LCD. Change the image to display your own favorite words. No external service or API key is needed. This example is for the LCD; E-ink and other screen sizes need a different renderer.

## 1. Prepare

Set up a server and LCD device using [first setup](first-setup.en.md), then run the commands below from the repository root. You need Rust, Cargo, Python 3, Pillow and a Japanese font. Check Pillow with `python3 -c 'from PIL import Image'` and find a font with `fc-list :lang=ja file | head`. If needed, install Pillow in a Python virtual environment with `python3 -m pip install Pillow`.

**To try the bundled example:** The repository already contains the finished example: `service/src/word.rs` is the plugin, `service/examples/make-word-card.py` generates the image, and `service/assets/word-card.png` is the image it uses. Use these files as they are, generate the image in step 2, and proceed to the build in step 4. Do not add the registrations again.

**To learn how to create it:** Follow steps 2–4 in order. The repository already contains these files and registrations, so inspect the existing entries instead of adding duplicates.

## 2. Make the image

Here is the complete `service/examples/make-word-card.py`:

```python
#!/usr/bin/env python3
"""Make a 160x80 LCD card from a phrase using an installed Japanese font."""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('words')
    parser.add_argument('--font', required=True, type=Path)
    parser.add_argument('--size', type=int, default=24)
    parser.add_argument('--x', type=int, default=8)
    parser.add_argument('--y', type=int, default=22)
    parser.add_argument('--output', type=Path, default=Path('service/examples/word-card.png'))
    args = parser.parse_args()
    if not 8 <= args.size <= 48 or not args.font.is_file():
        parser.error('choose an installed font and size 8..48')
    font = ImageFont.truetype(str(args.font), args.size, index=0)
    card = Image.new('RGB', (160, 80), '#102030')
    draw = ImageDraw.Draw(card)
    box = draw.textbbox((args.x, args.y), args.words, font=font, anchor='lt')
    if box[0] < 0 or box[1] < 0 or box[2] > 160 or box[3] > 80:
        parser.error('text does not fit 160x80; shorten it or change size/position')
    draw.text((args.x, args.y), args.words, font=font, fill='#ffffff', anchor='lt')
    card.save(args.output)
    print(args.output)


if __name__ == '__main__':
    main()
```


This draws text with a Japanese font and saves a 160×80 PNG. Replace the example font path with one found on your own computer.

```sh
PM_FONT=/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
python3 service/examples/make-word-card.py 'おはよう' --font "$PM_FONT" --output service/assets/word-card.png
```

Open the generated PNG in an image viewer and check that the text is readable.

## 3. The plugin

This is the complete code needed to run `service/src/word.rs` (the repository file also contains a test afterwards):

```rust
//! Optional LCD phrase example. The PNG is rendered before compilation.
use crate::content::{ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use std::collections::BTreeSet;
use std::sync::LazyLock;

static FRAME: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let image = image::load_from_memory(include_bytes!("../assets/word-card.png"))
        .expect("word card PNG").to_rgb8();
    assert_eq!(image.dimensions(), (160, 80), "word card must be 160x80");
    let mut frame = Vec::with_capacity(160 * 80 * 2);
    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        let color = ((u16::from(r) >> 3) << 11) | ((u16::from(g) >> 2) << 5) | (u16::from(b) >> 3);
        frame.extend_from_slice(&color.to_be_bytes());
    }
    frame
});

pub struct WordRules;
impl Plugin for WordRules {
    fn id(&self) -> &str { "my-words" }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool { caps.contains("lcd") }
    fn candidates<'a>(&'a self, _: &'a dyn WorldQuery, _: &'a Event) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone, Default)]
pub struct WordContent;
impl ContentPlugin for WordContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> { Box::new(self.clone()) }
    fn id(&self) -> &'static str { "my-words" }
    fn publish(&mut self, _: &PluginContext, _: &[u8], _: &str, _: &str) -> Result<(), ContentError> { Err(ContentError::Unsupported) }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> { None }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> { Err(ContentError::Unsupported) }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> { Some(FRAME.clone()) }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
}
```


`FRAME` loads the PNG and converts its pixels to RGB565 for the LCD. `WordRules` declares the ID `my-words` and LCD support. `WordContent` returns the image from `current_frame`. This example has no button actions or data storage.

## 4. Register and display it

To create the plugin yourself, connect the parts in this order. The bundled example already has all these entries; inspect them instead of adding duplicates.

1. Add this to `[features]` in `service/Cargo.toml` so the feature can be selected at build time:

   ```toml
   word-tutorial = []
   ```

2. Add this to the module declarations in `service/src/lib.rs` to load the module when the feature is enabled:

   ```rust
   #[cfg(feature = "word-tutorial")]
   pub mod word;
   ```

3. In `service/src/plugins.rs`, add this inside `builtins()` before `debug_assert_eq!`. It registers the rules and content together:

   ```rust
   #[cfg(feature = "word-tutorial")]
   set.register(Box::new(crate::word::WordRules), Box::new(crate::word::WordContent)).expect("unique plugin");
   ```

4. In the same file, define `DEFAULT_ORDER` as follows to include `my-words` in the selection order when enabled:

   ```rust
   #[cfg(not(feature = "word-tutorial"))]
   pub const DEFAULT_ORDER: [&str; 5] = ["youtube", "caricature", "weather", "rain-radar", "clock"];
   #[cfg(feature = "word-tutorial")]
   pub const DEFAULT_ORDER: [&str; 6] = ["youtube", "caricature", "weather", "rain-radar", "clock", "my-words"];
   ```

5. Add `'my-words':'好きな言葉'` to the `labels` object in `service/src/ui.html`, separated from the other entries by a comma. This supplies the name shown in the management screen.

Check that the ID is `my-words` throughout. If you changed the image or code, review and commit only the necessary files to local Git. The gate requires a committed, clean worktree for the build. Do not add private configuration or device IDs.

When building the server in [first setup](first-setup.en.md), add `--features word-tutorial` to the server build command. For example, use a new empty build directory:

```sh
PM_RUN=$(mktemp -d /tmp/picomado-word.XXXXXX)
python3 tools/project_gate.py check --out "$PM_RUN/check"
python3 tools/project_gate.py build --kind server --features word-tutorial --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/build"
```

Follow the server installation steps in first setup using this build’s `"$PM_RUN/build/release/picomado-server"`. Start the server, connect the LCD device on HOME, and use its next control until “好きな言葉” is selected. A white “おはよう” on a dark blue background is the finished result. If the item is absent, check that the running server was built with `word-tutorial`.

## 5. Make it yours

Replace the quoted phrase in step 2. Add options such as `--size 20 --x 12 --y 28` to set the font size and top-left position. If the text does not fit, shorten it or adjust the size and position. The image is embedded at server build time, so regenerate the PNG and rebuild and reinstall after changing it. Use a new empty build directory each time, for example:

```sh
PM_RUN=$(mktemp -d /tmp/picomado-word.XXXXXX)
python3 tools/project_gate.py check --out "$PM_RUN/check"
python3 tools/project_gate.py build --kind server --features word-tutorial --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/build"
```

Commit the changed PNG to local Git before running these commands, then install `"$PM_RUN/build/release/picomado-server"` following first setup.
