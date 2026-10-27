# 好きな言葉を表示するプラグインを作る

[English](plugin-author.en.md) · [初回設定](first-setup.md)

160×80 のカラー LCD に、濃い青の背景と白い「おはよう」を表示します。画像を変えれば、好きな言葉を自分の PicoMado に飾れます。外部サービスや API キーは不要です。この例は LCD 用です。E-ink や別の画面サイズには別の描画処理が必要です。

## 1. 準備

[初回設定](first-setup.md)に従ってサーバーと LCD 端末を用意し、リポジトリのルートで以下の操作をします。Rust、Cargo、Python 3、Pillow、日本語フォントが必要です。`python3 -c 'from PIL import Image'` で Pillow、`fc-list :lang=ja file | head` でフォントを確認できます。Pillow がなければ Python の仮想環境などに `python3 -m pip install Pillow` でインストールしてください。

**同梱サンプルを試す場合:** このリポジトリには完成版が入っています。手順2で画像を生成し、手順4のビルドへ進んでください。登録コードは追加しないでください。

**仕組みを学んで自分で作る場合:** 手順2〜4を順に読み、各ファイルと登録を用意します。既存のリポジトリには同じコードがあるので、重複して追加せず対応箇所を確認してください。プラグイン本体は `service/src/word.rs`、画像生成プログラムは `service/examples/make-word-card.py`、生成画像の置き場所は `service/assets/word-card.png` です。既存ファイルを使えるので、コピーを作る必要はありません。コードを自分で書く場合は、それぞれ次の内容を同じ場所に保存します。

## 2. 画像を作るコード

`service/examples/make-word-card.py` の全コードです。

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


日本語フォントで文字を描画し、160×80 の PNG として保存します。次のフォントパスは例です。`fc-list` の結果にある自分の環境のパスに置き換えてください。

```sh
PM_FONT=/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
python3 service/examples/make-word-card.py 'おはよう' --font "$PM_FONT" --output service/assets/word-card.png
```

生成された PNG を画像ビューアで開き、文字が読めることを確認します。

## 3. プラグイン本体

`service/src/word.rs` の実行に必要な全コードです（リポジトリのファイルには続けてテストもあります）。

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


`FRAME` は PNG を読み、各画素を LCD 用の RGB565 に変換します。`WordRules` は ID `my-words` と LCD 対応を宣言します。`WordContent` の `current_frame` がその画像を返します。この例にはボタン操作やデータ保存はありません。

## 4. 登録して表示する

自分で作る場合は、以下の接続を順に用意します。同梱サンプルではすべて登録済みです。既存行を確認し、同じ行を二重に追加しないでください。

1. `service/Cargo.toml` の `[features]` に次を追加します。機能を指定してビルドできるようになります。

   ```toml
   word-tutorial = []
   ```

2. `service/src/lib.rs` のモジュール宣言に次を追加します。機能を有効にしたときだけ本体を読み込みます。

   ```rust
   #[cfg(feature = "word-tutorial")]
   pub mod word;
   ```

3. `service/src/plugins.rs` の `builtins()` 内で `debug_assert_eq!` の前に次を追加します。同じ ID を持つルールと表示内容を組にして登録します。

   ```rust
   #[cfg(feature = "word-tutorial")]
   set.register(Box::new(crate::word::WordRules), Box::new(crate::word::WordContent)).expect("unique plugin");
   ```

4. 同ファイルの `DEFAULT_ORDER` を次のようにします。機能有効時の選択順に `my-words` が入ります。

   ```rust
   #[cfg(not(feature = "word-tutorial"))]
   pub const DEFAULT_ORDER: [&str; 5] = ["youtube", "caricature", "weather", "rain-radar", "clock"];
   #[cfg(feature = "word-tutorial")]
   pub const DEFAULT_ORDER: [&str; 6] = ["youtube", "caricature", "weather", "rain-radar", "clock", "my-words"];
   ```

5. `service/src/ui.html` の `labels` オブジェクトに `'my-words':'好きな言葉'` を追加します。管理画面で ID の代わりに名前が表示されます。既存の項目とカンマで区切ってください。

各箇所で ID が `my-words` に揃っていることを確認してください。

[初回設定](first-setup.md)のサーバービルドでは、サーバーの `build` コマンドに `--features word-tutorial` を加えてください。例えば次の通りです。ビルド先は空の新しいディレクトリを指定します。

```sh
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --features word-tutorial --build-dir /tmp/picomado-word-build
```

初回設定のサーバー導入手順で、このビルドの `release/picomado-server` を使います。サーバーを起動し、LCD 端末を HOME に接続したら、本体の次へ進む操作で「好きな言葉」まで切り替えます。濃い青の画面に白い「おはよう」が見えれば完成です。選択肢がない場合は、実行中のサーバーが `word-tutorial` 付きでビルドされたか確認してください。

## 5. 自分の言葉に変える

手順 2 の引用符内を好きな言葉に変え、`--size 20 --x 12 --y 28` のように文字サイズと左上の位置を指定できます。画像からはみ出すとエラーになります。文字を短くするか、サイズや位置を調整してください。画像はサーバーのビルド時に埋め込まれるため、変更後は PNG を生成し直し、毎回新しい空のビルドディレクトリを使ってビルドと導入をもう一度行います。例えば次のコマンドは一意の空ディレクトリを作ります。

```sh
PM_BUILD=$(mktemp -d /tmp/picomado-word-build.XXXXXX)
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --features word-tutorial --build-dir "$PM_BUILD"
```

生成された `"$PM_BUILD/release/picomado-server"` を初回設定の手順で導入してください。
