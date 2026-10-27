# Web から好きな言葉を表示するプラグインを作る

[English](plugin-author.en.md) · [初回設定](first-setup.md)

Web 画面に言葉を入力し、プレビューを見て保存すると、160×80 の LCD に表示します。プラグインは Web 操作、設定の保存、画像生成、端末への配信までを含みます。サンプルは `word-tutorial` を指定した場合だけ有効になり、通常の五つのプラグインは変わりません。E-ink は対象外です。

## 1. 同梱サンプルを試す

[初回設定](first-setup.md)の準備を済ませ、リポジトリのルートで作業します。同梱コードと登録は完成済みです。以下の自作手順のコードを重複して追加しないでください。ソースを変更した場合は必要なファイルをローカル Git にコミットし、作業ツリーをクリーンにします。秘密設定や実機 ID は追加しません。

```sh
PM_RUN=$(mktemp -d /tmp/picomado-word.XXXXXX)
python3 tools/project_gate.py check --out "$PM_RUN/check"
python3 tools/project_gate.py build --kind server --features word-tutorial --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/build"
```

[初回設定](first-setup.md)の候補記録と別の確認者によるレビュー・preflight を行い、このビルドの `"$PM_RUN/build/release/picomado-server"` を導入します。現在、初回導入の gate には作者のローカル証拠に依存する未解決の問題があります。その場合は導入済みとせず、エラーと不足している証拠を確認してください。

Web ホームの「好きな言葉」の「編集」を開き、言葉を入力します。ブラウザの日本語フォントで描かれたプレビューが読めることを確認して「保存して端末に反映」を押します。画面に収まらない文字列は短くしてください。プレビューに豆腐が出る場合はブラウザ側に日本語フォントを用意します。接続済み LCD で「好きな言葉」を選ぶと、保存した画像が表示されます。表示中なら保存時に更新されます。設定は保存され、再起動後も残ります。毎回ビルドする必要はありません。

## 2. 自分で作る場合の登録

既存ファイルを参照して作る場合は、以下の接続を順に用意します。同梱版にはすでにあります。同じ行を二重登録しないでください。`my-words` が全箇所で一致する必要があります。

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

`service/src/ui.html` の `labels` に `'my-words':'好きな言葉'` を入れます。ホームの順序行に `/plugins/my-words` への「編集」リンクを置き、`renderPlugin` が `renderWords()` に分岐し、`loadManaged` が `GET /api/plugins/my-words` を取得します。これらは同梱 UI 内の該当箇所で確認できます。

## 3. プラグイン本体と保存

`service/src/word.rs` の本体全体です（ファイル末尾のテストを除く）。ブラウザから受け取る文字列と PNG を検証して RGB565 big-endian に変換します。`web_state` は UI に現在値を返し、`save_state` / `load_state` が再起動後の復元を担当します。`plan_command` は変更を計画し `RefreshTargets::Selected` を返します。共通の `POST /api/plugins/my-words/commands` は revision を照合し、保存を確定してから選択中の端末へ配信します。端末が選択した際にも `current_frame` が保存済みの画素を返します。

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

## 4. Web UI と画像生成

`service/src/ui.html` の UI 実装です。`textField`、`section`、`managedViews`、`managedCommand` は同ファイルの共通ヘルパーです。編集開始時の revision を下書きとともに保持し、競合時も入力を残します。再読み込みすると最新の保存値を取得します。`managedCommand` は `{expected_revision, command}` を既存 API に送り、競合時にはエラーを表示します。文字列は HTML として挿入せず入力値として扱います。

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

ブラウザが PNG を作るため、実際の字形は利用するブラウザのフォントに依存します。サーバーは寸法・データ・文字列を検証しますが、画像と文字列が一致するかを証明しません。信頼できる LAN で利用し、保存前にプレビューを確認してください。

## 5. 変更して学ぶ

Web 画面の言葉は再ビルドせず何度でも変更できます。文字サイズや位置を変えるには `renderWords()` の `24px`、`x`、`y` を変更し、160×80 の境界確認とプレビューも合わせて調整します。コード変更はコミットしてから手順 1 で再ビルドし、初回設定のレビュー・導入手順を踏みます。ブラウザで保存後、端末の表示と、リロード後にも言葉が残ることを確認してください。
