# Make a favorite words plugin

[日本語](plugin-author.md) · [Content API](../../service/src/content.rs)

The finished 160×80 LCD card shows “おはよう” in white on a dark background. It uses no external service or API key. A Japanese font renders the words to PNG; the `my-words` plugin converts that image to an RGB565BE frame. Check the image and delivery path locally before trying your own device.

## 1. Prepare

From the repository root, install Rust/Cargo, Python 3, Pillow and a Japanese font such as Noto Sans CJK. Check Pillow with `python3 -c 'from PIL import Image; print(Image.__version__)'` and locate your font with `fc-list :lang=ja file | head`. PicoMado's built-in LCD font contains only a limited character set; rendering a PNG first avoids replacing unsupported Japanese characters with `?`.

## 2. Make the image

[make-word-card.py](../../service/examples/make-word-card.py) creates the 160×80 card. Replace `PM_FONT` with your font path.

```sh
PM_FONT=/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
python3 service/examples/make-word-card.py 'おはよう' --font "$PM_FONT" --output service/assets/word-card.png
```

Open `service/assets/word-card.png` and check that the text is legible. Change the quoted phrase to your own. Use `--size 20`, `--x 12`, and `--y 28` for size and position. The script rejects text outside the card. Commit the image to your local Git branch after checking it.

## 3. Read and adapt the complete code

[word.rs](../../service/src/word.rs) is the complete plugin. `WordRules` declares its ID and LCD support; `WordContent.current_frame` converts PNG pixels into a 25,600-byte RGB565BE frame. With the `word-tutorial` feature, `service/src/plugins.rs` registers rules and content under the same ID and adds them to selection order. `service/src/ui.html` labels the item “好きな言葉”. Copy these pieces to start another plugin, then change its ID and renderer. For input or persistence, see the [ContentPlugin API](../../service/src/content.rs).

## 4. Verify delivery in isolation

This test creates a temporary server-side database, connects an LCD device in the app, selects `my-words` with button actions and inspects the outgoing frame. It does not contact a running service or physical device.

```sh
cargo test --offline --manifest-path service/Cargo.toml --features word-tutorial word::tests::registered_phrase_reaches_server_send_effect
```

Look for `test result: ok`. To view the browser UI with an empty local database, build a checked server and run it on loopback. Choose a fresh `/tmp/picomado-word-build` name for each build.

```sh
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --features word-tutorial --build-dir /tmp/picomado-word-build
mkdir -p /tmp/picomado-word-run/data
cat > /tmp/picomado-word-run/config.toml <<'CONFIG'
bind = "127.0.0.1:16154"
database_path = "/tmp/picomado-word-run/data/app.db"
plugins_root = "/tmp/picomado-word-run"
CONFIG
/tmp/picomado-word-build/release/picomado-server --config /tmp/picomado-word-run/config.toml
```

In another terminal, run `curl http://127.0.0.1:16154/` and open that address in a browser. The UI shows “好きな言葉”. Stop the local server with Ctrl+C. This uses a separate database on your PC; the isolated test above checks the outgoing frame.

## 5. View it on your own device

Follow [first setup](first-setup.en.md) and [backup and recovery](release-and-recovery.md) to install a checked server candidate built with `word-tutorial`. Connect a compatible 160×80 LCD device on HOME and choose “好きな言葉” using its next button or the web selection controls. Check the physical display and wiring yourself. The isolation test does not establish physical drawing. E-ink and other display sizes need a different renderer.

## Troubleshooting

`No module named PIL` points to Pillow; `choose an installed font` points to the font path. `text does not fit` means the phrase, size or position exceeds the card. If the plugin is absent, check that your server build enabled `word-tutorial`. Regenerate the PNG and rebuild the server after changing words.
