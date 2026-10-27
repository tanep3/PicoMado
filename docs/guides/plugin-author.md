# 好きな言葉を表示するプラグインを作る

[English](plugin-author.en.md) · [API と仕組み](../../service/src/content.rs)

このチュートリアルの完成画面は、160×80 LCD の濃い背景に白字で「おはよう」と表示したものです。外部サービスや API キーは不要です。文字を画像にしてから `my-words` プラグインが画面用の RGB565BE フレームを作ります。まず自分の PC 上で画像と配信経路を確認し、最後に自分の端末で表示します。

## 1. 準備する

リポジトリのルートで作業します。Rust/Cargo、Python 3、Pillow、Noto Sans CJK などの日本語フォントが必要です。`python3 -c 'from PIL import Image; print(Image.__version__)'` と `fc-list :lang=ja file | head` で確認できます。既存 LCD 内蔵フォントは文字集合が限定されるため、好きな日本語をそのまま渡すと `?` に置換されることがあります。この例は日本語フォントで PNG を作るため、その問題を避けます。

## 2. 表示画像を作る

[make-word-card.py](../../service/examples/make-word-card.py) が 160×80 の画像を作ります。`PM_FONT` を `fc-list` で見つけたフォントパスに変えます。

```sh
PM_FONT=/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc
python3 service/examples/make-word-card.py 'おはよう' --font "$PM_FONT" --output service/assets/word-card.png
```

`service/assets/word-card.png` を画像ビューアで開き、文字が読めることを確認します。好きな言葉へ変えるには引用符の中を編集します。`--size 20`、`--x 12`、`--y 28` で大きさと位置を調整できます。画面からはみ出すとエラーになります。生成画像を確認してからローカル Git コミットに含めます。

## 3. コードを読む・変更する

省略のない実装は [word.rs](../../service/src/word.rs) です。`WordRules` は ID と LCD 対応を宣言します。`WordContent` の `current_frame` は PNG の各画素を RGB565BE に変換して 25,600 バイトを返します。`service/src/plugins.rs` は `word-tutorial` 機能が有効なときに規則と内容を同じ ID で登録し、選択順に加えます。`service/src/ui.html` は選択肢を「好きな言葉」と表示します。これらのファイルを複製して ID と描画を変えれば、自分用の別プラグインの土台になります。入力操作や保存が必要なら [ContentPlugin の API](../../service/src/content.rs) を確認してください。

## 4. 隔離して配信まで確認する

`word-tutorial` を有効にしたテストは、一時 DB のサーバー側アプリへ LCD 端末を接続し、ボタンで `my-words` を選択して、送出対象のフレームを検査します。実機や稼働中のサービスを使いません。

```sh
cargo test --offline --manifest-path service/Cargo.toml --features word-tutorial word::tests::registered_phrase_reaches_server_send_effect
```

`test result: ok` が成功の目印です。さらに別の空ディレクトリでブラウザ画面を見たい場合は、次のように検査済みビルドを作ります。`/tmp/picomado-word-build` は毎回新しい名前にしてください。

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

別の端末で `curl http://127.0.0.1:16154/` を実行し、ブラウザで同じアドレスを開きます。選択名「好きな言葉」が見えます。終了は起動した端末で Ctrl+C。これは自分の PC 内の別 DB を使う確認です。送信フレームの確認は上の隔離テストで行います。

## 5. 自分の端末で表示する

[初回設定](first-setup.md)と[更新・復旧](release-and-recovery.md)に沿って、`word-tutorial` を有効にした検査済みサーバー候補を自分の環境へ導入します。対応する 160×80 LCD 端末を HOME で接続し、端末の次へ進むボタンまたは Web の選択操作で「好きな言葉」を選びます。実際の画面に文字が出るか、電源・配線・接続も含めて確認してください。今回の隔離テストは物理表示の成功を証明しません。E-ink や別サイズの画面には専用レンダラーが必要です。

## 困ったとき

`No module named PIL` は Pillow、`choose an installed font` はフォントパスを確認します。`text does not fit` は文字を短くするかサイズ・位置を調整します。選択肢がない場合は `word-tutorial` を有効にしたサーバーを使っているかを確認します。画像が更新されない場合は PNG を作り直した後にサーバーを再ビルドしたかを確認します。
