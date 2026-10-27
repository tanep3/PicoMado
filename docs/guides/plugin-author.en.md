# Build a plugin that shows your words from the Web UI

[日本語](plugin-author.md) · [First setup](first-setup.en.md)

Enter a phrase in the Web UI, preview it, and save it to display on a 160×80 LCD. This example includes the UI, durable settings, image creation, and delivery to the device. It is enabled only by the `word-tutorial` feature; the normal five plugins stay unchanged. E-ink is not supported by this example.

## 1. Try the included sample

Prepare the server and LCD as described in [first setup](first-setup.en.md), and work from the repository root. The sample and its registrations are already present: do not add the self-build code a second time. If you changed source files, review and commit the necessary files to local Git so the build worktree is clean. Do not commit secrets or device IDs.

```sh
PM_RUN=$(mktemp -d /tmp/picomado-word.XXXXXX)
python3 tools/project_gate.py check --out "$PM_RUN/check"
python3 tools/project_gate.py build --kind server --features word-tutorial --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/build"
python3 tools/project_gate.py record --kind server --report "$PM_RUN/check/check.json" --build-receipt "$PM_RUN/build/build-receipt.json" --artifact "$PM_RUN/build/release/picomado-server" --out "$PM_RUN/build/candidate.json"
```

Follow [first setup](first-setup.en.md) for candidate recording, independent review, preflight, and installation using `"$PM_RUN/build/release/picomado-server"`. The first-install gate currently has an unresolved dependency on the author's local evidence. If it rejects the build or approval, inspect the error and missing evidence; do not treat installation as completed.

On the Web home page, open “編集” next to “好きな言葉”. Enter a phrase, check that the preview drawn with the browser's Japanese font is legible, and select “保存して端末に反映”. Shorten a phrase that does not fit. If the preview shows missing glyphs, install a Japanese font on the browser's system. Select “好きな言葉” on a connected LCD; a device already showing it receives the update on save. The setting survives restart, and editing the words does not require a rebuild.

## 2. Register your own implementation

To build it yourself, connect the following pieces in order. They already exist in the included sample; do not register the same rows twice. Use the ID `my-words` consistently.

```toml
# service/Cargo.toml, [features]
word-tutorial = []
```

```rust
// service/src/lib.rs
#[cfg(feature = "word-tutorial")]
pub mod word;
```

```rust
// service/src/plugins.rs: builtins(), before debug_assert_eq!
#[cfg(feature = "word-tutorial")]
set.register(Box::new(crate::word::WordRules), Box::new(crate::word::WordContent::default())).expect("unique plugin");

// service/src/plugins.rs: DEFAULT_ORDER
#[cfg(not(feature = "word-tutorial"))]
pub const DEFAULT_ORDER: [&str; 5] = ["youtube", "caricature", "weather", "rain-radar", "clock"];
#[cfg(feature = "word-tutorial")]
pub const DEFAULT_ORDER: [&str; 6] = ["youtube", "caricature", "weather", "rain-radar", "clock", "my-words"];
```

In `service/src/ui.html`, add `'my-words':'好きな言葉'` to `labels`, a link to `/plugins/my-words` in the home order row, the `renderWords()` branch in `renderPlugin`, and `GET /api/plugins/my-words` in `loadManaged`. Inspect those locations in the included UI.

## 3. Plugin, storage, and delivery

This is the complete implementation in `service/src/word.rs`, excluding tests at the end of the file. It validates the submitted phrase and PNG and converts pixels to RGB565 big-endian. `web_state` supplies current values, while `save_state` / `load_state` restore them after restart. `plan_command` returns `RefreshTargets::Selected`. The shared `POST /api/plugins/my-words/commands` checks the revision, commits state, and then delivers the frame to selected devices. `current_frame` also supplies the saved pixels when a device selects the plugin.

```rust
//! Optional LCD phrase example: a browser-authored card saved through plugin commands.
use crate::content::{ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use std::collections::BTreeSet;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::plugin_jobs::{CommandContext, PluginWorkPlan, RefreshTargets};

const DEFAULT_PNG: &[u8] = include_bytes!("../assets/word-card.png");
const MAX_PNG: usize = 100_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Card { words: String, png: String }

fn decode_card(card: &Card) -> Result<Vec<u8>, ContentError> {
    if card.words.trim().is_empty() || card.words.len() > 160 ||
        card.words.chars().count() > 40 || card.words.chars().any(char::is_control) ||
        card.png.len() > (MAX_PNG + 2) / 3 * 4 {
        return Err(ContentError::InvalidSource);
    }
    let png = STANDARD.decode(&card.png).map_err(|_| ContentError::InvalidSource)?;
    if png.len() > MAX_PNG || png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" ||
        &png[12..16] != b"IHDR" || u32::from_be_bytes(png[16..20].try_into().unwrap()) != 160 ||
        u32::from_be_bytes(png[20..24].try_into().unwrap()) != 80 {
        return Err(ContentError::InvalidSource);
    }
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|_| ContentError::InvalidSource)?.to_rgb8();
    if image.dimensions() != (160, 80) { return Err(ContentError::InvalidSource); }
    let mut frame = Vec::with_capacity(160 * 80 * 2);
    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        let color = ((u16::from(r) >> 3) << 11) | ((u16::from(g) >> 2) << 5) | (u16::from(b) >> 3);
        frame.extend_from_slice(&color.to_be_bytes());
    }
    Ok(frame)
}

pub struct WordRules;
impl Plugin for WordRules {
    fn id(&self) -> &str { "my-words" }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool { caps.contains("lcd") }
    fn candidates<'a>(&'a self, _: &'a dyn WorldQuery, _: &'a Event) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone)]
pub struct WordContent { card: Card, frame: Vec<u8> }
impl Default for WordContent {
    fn default() -> Self {
        let card = Card { words: "おはよう".into(), png: STANDARD.encode(DEFAULT_PNG) };
        let frame = decode_card(&card).expect("bundled word card");
        Self { card, frame }
    }
}
impl ContentPlugin for WordContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> { Box::new(self.clone()) }
    fn id(&self) -> &'static str { "my-words" }
    fn publish(&mut self, _: &PluginContext, _: &[u8], _: &str, _: &str) -> Result<(), ContentError> { Err(ContentError::Unsupported) }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> { None }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> { Err(ContentError::Unsupported) }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> { Some(self.frame.clone()) }
    fn web_state(&self) -> Value { json!({"words": self.card.words, "png": self.card.png}) }
    fn save_state(&self) -> Value { serde_json::to_value(&self.card).expect("card serialization") }
    fn load_state(&mut self, state: Value) -> Result<(), ContentError> {
        if state.is_null() { return Ok(()); }
        let card: Card = serde_json::from_value(state).map_err(|_| ContentError::InvalidSource)?;
        let frame = decode_card(&card)?;
        self.card = card; self.frame = frame; Ok(())
    }
    fn plan_command(&mut self, _: &CommandContext, value: &Value) -> Result<PluginWorkPlan, ContentError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Command { kind: String, words: String, png: String }
        let command: Command = serde_json::from_value(value.clone()).map_err(|_| ContentError::InvalidSource)?;
        if command.kind != "save" { return Err(ContentError::Unsupported); }
        let card = Card { words: command.words, png: command.png };
        let frame = decode_card(&card)?;
        self.card = card; self.frame = frame;
        Ok(PluginWorkPlan { refresh: RefreshTargets::Selected, ..Default::default() })
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
}
```

## 4. Web UI and image creation

This is the UI function in `service/src/ui.html`. `textField`, `section`, `managedViews`, and `managedCommand` are shared helpers in the same file. The draft retains the revision from the first edit; conflicts preserve the input. Reload to fetch the latest saved value.

`managedCommand` posts `{expected_revision, command}` to the existing API and displays conflicts. User text stays in input values rather than being inserted as HTML.

```javascript
    function renderWords(){
      const view=managedViews['my-words'];if(!view)return;
      const panel=section('好きな言葉','入力してプレビューを確認し、保存すると表示中の LCD に反映されます。','plugin-card');
      const form=el('form'), input=textField(form,'言葉（40文字まで）','text',drafts.words?.value??view.data.words,true);
      input.maxLength=40;input.autocomplete='off';
      const canvas=el('canvas');canvas.width=160;canvas.height=80;canvas.style.width='320px';canvas.style.maxWidth='100%';canvas.style.imageRendering='pixelated';
      canvas.setAttribute('aria-label','表示プレビュー');form.append(canvas);
      const message=el('p','','plugin-note');form.append(message);
      const save=button('保存して端末に反映','primary');save.type='submit';form.append(save);panel.append(form);
      const ctx=canvas.getContext('2d');
      function preview(){
        const words=input.value;
        ctx.fillStyle='#102030';ctx.fillRect(0,0,160,80);ctx.font='bold 24px "Noto Sans CJK JP", "Noto Sans JP", sans-serif';ctx.textBaseline='alphabetic';
        const m=ctx.measureText(words),x=8,y=40;
        const valid=words.trim().length>0 && Array.from(words).length<=40 && new TextEncoder().encode(words).length<=160 && !/\p{Cc}/u.test(words) &&
          x-m.actualBoundingBoxLeft>=0 && x+m.actualBoundingBoxRight<=160 && y-m.actualBoundingBoxAscent>=0 && y+m.actualBoundingBoxDescent<=80;
        const error=/\p{Cc}/u.test(words)?'制御文字は使用できません。':'文字が空か長すぎます。画面に収まる言葉を入力してください。';
        input.setCustomValidity(valid?'':error);
        ctx.fillStyle='#ffffff';ctx.fillText(words,x,y);
        message.textContent=valid?'文字が正しく表示されているか確認してください。':error;
        save.disabled=!valid;return valid;
      }
      input.addEventListener('input',()=>{drafts.words={value:input.value,revision:drafts.words?.revision??view.revision};preview()});preview();
      form.onsubmit=async event=>{event.preventDefault();if(!preview()||!form.reportValidity())return;
        const png=canvas.toDataURL('image/png').split(',')[1],revision=drafts.words?.revision??view.revision;
        drafts.words??={value:input.value,revision};
        input.disabled=save.disabled=true;
        if(await managedCommand('my-words',{kind:'save',words:input.value,png},'言葉を保存しました',revision)){delete drafts.words;render()}
        else{input.disabled=false;preview()}
      };
    }
```

The browser creates the PNG using its available Japanese font, so glyphs vary by browser. The server checks dimensions, data and text, but cannot prove that submitted pixels match the submitted words. Use a trusted LAN and check the preview before saving.

## 5. Make it yours

Change words from the Web UI without rebuilding. To change size or position, edit `24px`, `x` and `y` in `renderWords()` and adjust the 160×80 bounds check and preview together. Commit code changes and rebuild as in step 1, then follow the review and installation steps. After saving from the browser, verify the LCD and that the phrase remains after reloading.
