# PicoMado を初めて設定する

この手順は Linux のサーバと Raspberry Pi Pico W を想定します。作業はリポジトリのルートで行います。まず[対応表](hardware-support.md)で画面とボタンの組合せを確認してください。既存の設定、DB、素材を新しい例で上書きしないでください。

## 1. 自分の環境を確認する

サーバ用の [Rust と Cargo](https://rust-lang.org/tools/install/)、ファーム用の C/C++ コンパイラ、CMake、Python 3.11 以上、Git、[Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html)、UF2 変換とボード確認用の [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) を用意します。C/C++ ツールと Pico SDK の導入は Raspberry Pi の公式手順に従い、利用する SDK の要件に合う版を選んでください。`rustc --version`、`cargo --version`、`cmake --version`、`python3 --version`、`git --version`、`picotool version` で実行できるか確認します。端末で `echo "$HOME"` を実行し、自分のホームディレクトリも確認します。Pico SDK の実際の場所を `PICO_SDK_PATH` に指定します。例: `export PICO_SDK_PATH="$HOME/pico-sdk"`。ファームビルドでは SDK とサブモジュールの版を記録します。サブモジュールを初期化した Git checkout を使い、SDK とサブモジュールをクリーンに保ってください。Git 情報のない SDK はこの出自検査を通りません。ビルド前に `git -C "$PICO_SDK_PATH" status --porcelain` と `git -C "$PICO_SDK_PATH" submodule status --recursive` を確認します。風刺画プラグインは追加の実行環境が必要です。使う場合だけ [サービスの説明](../../service/README.md) を参照してください。

全体のオフライン検査には `ffmpeg`、`ffprobe`、GNU `nice` も必要です。FFmpeg は試験が使う MPEG-4 encoder を含むものを用意し、`ffmpeg -version`、`ffprobe -version`、`nice --version` で確認します。`ffmpeg -hide_banner -encoders | grep -E "(^|[[:space:]])mpeg4[[:space:]]"` で encoder の行が表示されることも確認します。YouTube 再生には別途 `yt-dlp`、FFmpeg、FFprobe が必要です。yt-dlp の環境で JavaScript runtime が必要な場合はそれも用意します。

初回はリポジトリのルートで `cargo fetch --locked --manifest-path service/Cargo.toml` を実行し、ロックされた依存を取得します。ネットワークが必要です。以後の gate とビルドは `--offline --locked` を使います。

配布物のルートで `python3 tools/public_gate.py verify-source` を実行すると、同梱 manifest とファイルが一致するか確認できます。配布元が示すアーカイブの SHA256 も信頼できる方法で照合してください。設定やソースを変更する前に実行します。

## 2. サーバの設定を作る

新規導入の場合、次のコマンドで設定例をコピーします。既存の設定がある場合、`cp -n` は上書きしません。既存ファイルを別途確認してください。

```sh
mkdir -p "$HOME/.config/PicoMado"
cp -n service/deploy/config.toml "$HOME/.config/PicoMado/config.toml"
```

コピーした `config.toml` の `bind` を、サーバとなる Linux マシン自身の LAN IPv4 アドレスとポート `16153` に変更します。`192.0.2.20` は説明用アドレスで、そのままでは使えません。`ip -4 addr show` で候補を確認できます。全アドレスを受け付ける `0.0.0.0` は拒否されます。`database_path` は保存する SQLite DB、`plugins_root` はプラグインの保存場所です。例の `~/` は実行ユーザーの HOME に展開されます。既存 DB を使う場合は、設定を変更する前に[退避手順](release-and-recovery.md#更新前の退避)を確認してください。

実行ファイルは既定でこの設定を読みます。別の設定を試す場合は `picomado-server --config /絶対パス/config.toml` と指定できます。設定の読み込みに失敗したら、先に TOML の書式と IP/パスを直してください。

## 3. ファームの個体設定を用意する

`firmware/devices/sample-a/config.sample.h` はダミーの Wi-Fi 値を持つ LCD 例です。対応表にある自分の構成を選び、`firmware/devices/<自分の端末名>/` に `profile.txt`、`config.h`、`board-id.txt` を用意します。`profile.txt` はプロファイル名を 1 行で指定します。`config.h` はサンプルの `PM_DEVICE_ID` をディレクトリ名に、HOME の SSID・パスワード・サーバ IPv4 を自分の値に変えます。`main.c` は AWAY のマクロも常に参照するため、HOME だけで使う場合もサンプルと同じ AWAY 項目をすべて定義します。未使用の AWAY 項目にはサンプルの説明用値を残し、AWAY を選択しません。実際に AWAY を使う場合は Wi-Fi と SOCKS5 の値を個別に設定します。サンプルの予約済み IP アドレスやパスワードでは接続できません。`board-id.txt` は対象の実機から確認した Flash ID を入れるファイルです。BOOTSEL で対象の Pico W を 1 台だけ接続し、`picotool info -d` の `flash id` が 16 桁であることを確認します。値を推測してはいけません。自分のテキストエディタで `firmware/devices/<端末名>/board-id.txt` を作り、確認した16桁だけを１行で保存します。秘密の `config.h` と実機 ID を公開しないでください。

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

## 5. 検査してビルドする

配布物のルートで実行します。検査はオフラインでホスト試験とローカル loopback を使う Rust 試験を行います。出力・ログにはローカルパスを含むため公開しません。新しい配線は `firmware/profiles/` に `.h` と `.board` を作り `profile.txt` で選びます。検査は選択したプロファイルとビルドを結び付けますが、実物の配線を保証しません。

```sh
PM_DEVICE=my-pico
PM_RUN=$(mktemp -d /tmp/picomado.XXXXXX)
python3 tools/public_gate.py check --out "$PM_RUN/check"
python3 tools/public_gate.py build --kind server --report "$PM_RUN/check/check.json" --build-dir "$PM_RUN/server"
python3 tools/public_gate.py build --kind firmware --device "$PM_DEVICE" --report "$PM_RUN/check/check.json" --build-dir "$PWD/firmware/build/$PM_DEVICE"
python3 tools/public_gate.py preflight --receipt "$PM_RUN/server/public-build.json"
python3 tools/public_gate.py preflight --receipt "firmware/build/$PM_DEVICE/public-build.json"
```

ファームのビルド先は未使用である必要があります。再ビルドの前に旧ディレクトリと `firmware/build/$PM_DEVICE-configure.log`、`firmware/build/$PM_DEVICE-build.log` を私的な場所へ退避してください。preflight はソース、ツール、SDK、設定と成果物の一致を確認します。ローカル記録は改変検知用であり、署名や独立レビューではありません。成功は実機表示の確認を意味しません。

## 6. 自分のサーバと端末に入れる

両方の preflight が成功した後、検査済みのサーバ成果物を導入します。

```sh
PM_RELEASE_BIN="$PM_RUN/server/release/picomado-server"
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 "$PM_RELEASE_BIN" "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

ユニットは `~/.cargo/bin/picomado-server` を参照します。`active` でなければ `systemctl --user status picomado.service` と TOML を確認します。信頼できる LAN から設定した IPv4 とポート `16153` を開きます。

Pico W を１台だけ BOOTSEL で接続します。最初のコマンドは対象と成果物の照合だけ、次のコマンドは照合を繰り返して UF2 を書き込み verify します。

```sh
python3 tools/public_gate.py flash --receipt "firmware/build/$PM_DEVICE/public-build.json" --device "$PM_DEVICE" --check-only
python3 tools/public_gate.py flash --receipt "firmware/build/$PM_DEVICE/public-build.json" --device "$PM_DEVICE"
```

不一致なら中止します。再起動後に HOME Wi-Fi、サーバへの Hello、画面表示、ボタンを個別に確認します。0 ボタンの E-ink はボタン確認不要です。

## 7. 既存端末を更新する場合

新規導入の `cp -n` や新しい DB 作成に進まず、[更新前の退避](release-and-recovery.md#更新前の退避)から始めます。現在の実行ファイル、設定、SQLite DB、プラグインの `data/`、対象個体の旧 UF2・manifest を同じ時点の組合せで保存します。新候補の preflight 後にサーバと端末を一つずつ更新し、Web、HOME 接続、表示、入力を再確認します。失敗時は保存した組合せで復旧します。DB 本体だけを稼働中にコピーしないでください。

## 8. AWAY を使う場合

AWAY は HOME の初回動作を確認してから設定します。製品用ファームでは EEX1 による SOCKS5 接続先探索が必須です。別プロジェクトの Android 側 EEX1 アプリ、テザリング、TailSocks と利用できる経路、認証値の準備が必要です。このリポジトリだけで AWAY を完成させる手順はありません。[接続方式と未確認事項](../design/eex1-endpoint-discovery.md)を読み、各環境で宛先と経路を確認してください。HOME だけ使う場合は AWAY を選択しません。

## 9. 困ったとき

困ったときは、サーバ設定を読めない場合は TOML の書式と指定パス、接続しない場合はサーバと端末に設定した IPv4、SSID、ポート、画面が出ない場合はプロファイルと実際の画面型番・配線を順に確認します。確認用ログや質問に Wi-Fi パスワードや実機 ID を貼り付けないでください。
