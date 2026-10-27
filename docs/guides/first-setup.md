# PicoMado を初めて設定する

この手順は Linux のサーバと Raspberry Pi Pico W を想定します。作業はリポジトリのルートで行います。まず[対応表](hardware-support.md)で画面とボタンの組合せを確認してください。既存の設定、DB、素材を新しい例で上書きしないでください。

## 1. ツールを用意する

サーバー用の [Rust と Cargo](https://rust-lang.org/tools/install/)、ファーム用の C/C++ ツール、CMake、Python 3.8 以上、[Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html)、UF2 変換用の [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) を用意します。SDK は Raspberry Pi の公式手順で導入し、実際の場所を `PICO_SDK_PATH` に設定します。Cargo は初回ビルド時に不足する依存を取得します。後でオフラインでビルドしたい場合は `cargo fetch --locked --manifest-path service/Cargo.toml` で先に取得できます。

YouTube 再生には `yt-dlp`、FFmpeg、FFprobe が必要です。yt-dlp の環境によっては JavaScript runtime も必要です。全体テストを任意で行う場合は FFmpeg、FFprobe、`nice` が必要です。

## 2. サーバの設定を作る

新規導入の場合、次のコマンドで設定例をコピーします。既存の設定がある場合、`cp -n` は上書きしません。既存ファイルを別途確認してください。

```sh
mkdir -p "$HOME/.config/PicoMado"
cp -n service/deploy/config.toml "$HOME/.config/PicoMado/config.toml"
```

コピーした `config.toml` の `bind` を、サーバとなる Linux マシン自身の LAN IPv4 アドレスとポート `16153` に変更します。`192.0.2.20` は説明用アドレスで、そのままでは使えません。`ip -4 addr show` で候補を確認できます。全アドレスを受け付ける `0.0.0.0` は拒否されます。`database_path` は保存する SQLite DB、`plugins_root` はプラグインの保存場所です。例の `~/` は実行ユーザーの HOME に展開されます。既存 DB を使う場合は、設定を変更する前に[退避手順](release-and-recovery.md#更新前の退避)を確認してください。

実行ファイルは既定でこの設定を読みます。別の設定を試す場合は `picomado-server --config /絶対パス/config.toml` と指定できます。設定の読み込みに失敗したら、先に TOML の書式と IP/パスを直してください。

## 3. ファームの個体設定を用意する

`firmware/devices/sample-a/config.sample.h` はダミーの Wi-Fi 値を持つ LCD 例です。対応表から構成を選び、`firmware/devices/<自分の端末名>/` に `profile.txt` と `config.h` を用意します。`config.h` の `PM_DEVICE_ID` をディレクトリ名に、HOME の SSID・パスワード・サーバ IPv4 を自分の値に変えます。HOME だけで使う場合もサンプルの AWAY 項目をすべて残し、AWAY を選択しません。接続値や `config.h` は公開しないでください。

新しく作る `config.h` には画面・ボタンの定義も `#include` も書きません。ビルドが `profile.txt` に対応する選んだプロファイルを読み込みます。個体設定で選べるハード項目は LCD バックライトの接続・極性、または E-ink パネル接続の有無だけです。旧 config はプロファイルとハード定義が完全に一致する場合だけ利用できます。

LCD の新しい端末なら次のように例を複製できます。`my-pico` を自分で決めた端末名に変え、複製した `config.h` の `PM_DEVICE_ID` と接続値を編集してください。E-ink の場合は、対応表で選んだボタン数に合う `firmware/devices/E-ink_0` または `E-ink_1` の例を使います。

```sh
PM_DEVICE=my-pico
mkdir -p "firmware/devices/$PM_DEVICE"
cp firmware/devices/sample-a/config.sample.h "firmware/devices/$PM_DEVICE/config.h"
cp firmware/devices/sample-a/profile.txt "firmware/devices/$PM_DEVICE/profile.txt"
```

各端子は [ファームウェアの説明](../../firmware/README.md#配線) を確認します。画面の型番、電圧、端子名を配線前に照合し、通電したまま配線を変更しないでください。サンプルビルドは実機用ではありません。

## 4. HOME 接続を準備する

まず HOME 接続だけを使います。Linux サーバと Pico W が同じ LAN 上で通信でき、端末の `PM_HOME_SERVER_IP` とサーバの `bind` が同じ IPv4 を指すようにします。ファイアウォールで TCP 16153 とサーバの画面を使う端末からのアクセスを確認します。サーバの Web 画面や端末接続には組込みのログイン・API キーがありません。信頼できる LAN の範囲で運用してください。

## 5. ビルドする

ソースのルートで実行します。

```sh
PM_DEVICE=my-pico
cargo build --release --locked --manifest-path service/Cargo.toml
firmware/tools/build.sh --device "$PM_DEVICE"
```

サーバーは `service/target/release/picomado-server`、UF2 は `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2` です。配線を変更する場合は `firmware/profiles/` にプロファイルを作り、`profile.txt` で選びます。ビルドは MCU・画面ドライバ・GPIO の矛盾を検出しますが、実際の配線は確認できません。ソースや設定を編集した後も同じコマンドで再ビルドできます。テストは任意で、例えばサーバーは `cargo test --locked --all-features --manifest-path service/Cargo.toml` で確認できます。

## 6. 自分のサーバーと端末に入れる

新規導入の例です。

```sh
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 service/target/release/picomado-server "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

ユニットは `~/.cargo/bin/picomado-server` を参照します。`active` でなければ `systemctl --user status picomado.service` と TOML を確認します。信頼できる LAN から設定した IPv4 とポート `16153` を開きます。

Pico W を BOOTSEL モードにし、`firmware/build/$PM_DEVICE/$PM_DEVICE.uf2` を `RPI-RP2` ドライブへコピーします。コピー後に自動で再起動します。HOME Wi-Fi、サーバー接続、画面表示、ボタンを個別に確認します。Flash ID ファイルや承認記録は不要です。

## 7. 既存端末を更新する場合

新規導入の `cp -n` や新しい DB 作成に進まず、[更新前の退避](release-and-recovery.md#更新前の退避)から始めます。現在の実行ファイル、設定、SQLite DB、プラグインの `data/`、旧 UF2 を同じ時点の組合せで保存します。サーバと端末を一つずつ更新し、Web、HOME 接続、表示、入力を再確認します。失敗時は保存した組合せで復旧します。DB 本体だけを稼働中にコピーしないでください。

## 8. AWAY を使う場合

AWAY は HOME の初回動作を確認してから設定します。製品用ファームでは EEX1 による SOCKS5 接続先探索が必須です。別プロジェクトの Android 側 EEX1 アプリ、テザリング、TailSocks と利用できる経路、認証値の準備が必要です。このリポジトリだけで AWAY を完成させる手順はありません。[接続方式と未確認事項](../design/eex1-endpoint-discovery.md)を読み、各環境で宛先と経路を確認してください。HOME だけ使う場合は AWAY を選択しません。

## 9. 困ったとき

困ったときは、サーバ設定を読めない場合は TOML の書式と指定パス、接続しない場合はサーバと端末に設定した IPv4、SSID、ポート、画面が出ない場合はプロファイルと実際の画面型番・配線を順に確認します。確認用ログや質問に Wi-Fi パスワードや実機 ID を貼り付けないでください。
