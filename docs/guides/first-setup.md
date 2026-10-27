# PicoMado を初めて設定する

この手順は Linux のサーバと Raspberry Pi Pico W を想定します。作業はリポジトリのルートで行います。まず[対応表](hardware-support.md)で画面とボタンの組合せを確認してください。既存の設定、DB、素材を新しい例で上書きしないでください。

## 1. 自分の環境を確認する

サーバ用の [Rust と Cargo](https://rust-lang.org/tools/install/)、ファーム用の C/C++ コンパイラ、CMake、Python 3、[Pico SDK](https://www.raspberrypi.com/documentation/microcontrollers/c_sdk.html)、UF2 変換とボード確認用の [picotool](https://github.com/raspberrypi/picotool/blob/master/BUILDING.md) を用意します。C/C++ ツールと Pico SDK の導入は Raspberry Pi の公式手順に従い、利用する SDK の要件に合う版を選んでください。`rustc --version`、`cargo --version`、`cmake --version`、`python3 --version`、`picotool version` で実行できるか確認します。端末で `echo "$HOME"` を実行し、自分のホームディレクトリも確認します。Pico SDK の実際の場所を `PICO_SDK_PATH` に指定します。例: `export PICO_SDK_PATH="$HOME/pico-sdk"`。風刺画プラグインは追加の実行環境が必要です。使う場合だけ [サービスの説明](../../service/README.md) を参照してください。

## 2. サーバの設定を作る

新規導入の場合、次のコマンドで設定例をコピーします。既存の設定がある場合、`cp -n` は上書きしません。既存ファイルを別途確認してください。

```sh
mkdir -p "$HOME/.config/PicoMado"
cp -n service/deploy/config.toml "$HOME/.config/PicoMado/config.toml"
```

コピーした `config.toml` の `bind` を、サーバとなる Linux マシン自身の LAN IPv4 アドレスとポート `16153` に変更します。`192.0.2.20` は説明用アドレスで、そのままでは使えません。`ip -4 addr show` で候補を確認できます。全アドレスを受け付ける `0.0.0.0` は拒否されます。`database_path` は保存する SQLite DB、`plugins_root` はプラグインの保存場所です。例の `~/` は実行ユーザーの HOME に展開されます。既存 DB を使う場合は、設定を変更する前に[退避手順](release-and-recovery.md#更新前の退避)を確認してください。

実行ファイルは既定でこの設定を読みます。別の設定を試す場合は `picomado-server --config /絶対パス/config.toml` と指定できます。設定の読み込みに失敗したら、先に TOML の書式と IP/パスを直してください。

## 3. ファームの個体設定を用意する

`firmware/devices/sample-a/config.sample.h` はダミーの Wi-Fi 値を持つ LCD 例です。対応表にある自分の構成を選び、`firmware/devices/<自分の端末名>/` に `profile.txt`、`config.h`、`board-id.txt` を用意します。`profile.txt` はプロファイル名を 1 行で指定します。`config.h` はサンプルの `PM_DEVICE_ID` をディレクトリ名に、HOME の SSID・パスワード・サーバ IPv4 を自分の値に変えます。`main.c` は AWAY のマクロも常に参照するため、HOME だけで使う場合もサンプルと同じ AWAY 項目をすべて定義します。未使用の AWAY 項目にはサンプルの説明用値を残し、AWAY を選択しません。実際に AWAY を使う場合は Wi-Fi と SOCKS5 の値を個別に設定します。サンプルの予約済み IP アドレスやパスワードでは接続できません。`board-id.txt` は対象の実機から確認した Flash ID を入れるファイルです。BOOTSEL で対象の Pico W を 1 台だけ接続し、`picotool info -d` の `flash id` が 16 桁であることを確認します。値を推測してはいけません。秘密の `config.h` と実機 ID を Git に追加しないでください。

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

## 5. ビルドできる配布物か確認する

ソースからのビルドはプロジェクトの gate を通します。配線を変更する場合は `firmware/profiles/` に別名の `.h` と `.board` を作り、`profile.txt` で選びます。新しい profile はビルド前にローカル Git コミットへ入れてください。gate は使用したソースと設定を照合しますが、実物の配線や表示を保証しません。導入前には別の確認者によるレビューが必要です。

検査とビルドは次の順です。
```sh
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --build-dir /tmp/picomado-server-build
python3 tools/project_gate.py record --build-receipt /tmp/picomado-server-build/build-receipt.json --artifact /tmp/picomado-server-build/release/picomado-server --kind server --out /tmp/picomado-server-build/candidate.json
PM_DEVICE=my-pico  # 手順3で作った端末ディレクトリ名
firmware/tools/build.sh --device "$PM_DEVICE"
```

サーバ候補は `/tmp/picomado-server-build/release/picomado-server`、端末候補は `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2` です。`record` は候補を記録するだけで承認しません。別の確認者が対象の候補・成果物・設定に結び付けた `review.json` を渡したら、サーバは `python3 tools/project_gate.py preflight --candidate /tmp/picomado-server-build/candidate.json --review /tmp/picomado-server-build/review.json --artifact /tmp/picomado-server-build/release/picomado-server`、端末は `flash.sh --verify-only` で offline preflight を確認します。自分で承認ファイルを作ったことにして進めないでください。gate の検査成功だけでは表示を確認できません。

## 6. 新しい端末へ入れる

以下は有効な候補・レビュー記録と offline preflight が揃った後に、**自分のサーバと端末の所有者が実行する操作例**です。`PM_RELEASE_BIN` は検査済みサーバ成果物の実際の場所に変更します。

```sh
PM_RELEASE_BIN=/tmp/picomado-server-build/release/picomado-server
mkdir -p "$HOME/.cargo/bin" "$HOME/.config/systemd/user"
install -m 755 "$PM_RELEASE_BIN" "$HOME/.cargo/bin/picomado-server"
cp -n service/deploy/picomado.service "$HOME/.config/systemd/user/picomado.service"
systemctl --user daemon-reload
systemctl --user enable --now picomado.service
systemctl --user is-active picomado.service
```

`active` と表示されたら、設定した `bind` の IPv4 とポート `16153` で Web 画面を開きます。表示されない場合は `systemctl --user status picomado.service` で起動エラーを見て、設定ファイルのパス、TOML、待受 IP とポートを確認します。ユニットの `ExecStart` は `~/.cargo/bin/picomado-server` の例です。別の場所へ配置した場合は自分のユニットを編集してください。

端末の UF2 は、候補記録と `review.json` が同じビルドディレクトリにあり、offline preflight が成功したことを確かめてから扱います。対象 Pico W を 1 台だけ BOOTSEL で接続し、最初に照合だけを行います。

```sh
firmware/tools/flash.sh --device "$PM_DEVICE" --check-only
firmware/tools/flash.sh --device "$PM_DEVICE"
```

後者は対象ボードへの書込みと verify を行います。照合に失敗した場合は書き込まず、個体名、Flash ID、UF2 と候補記録を見直します。再起動後に HOME Wi-Fi、サーバへの Hello、画面の表示、ボタンの反応を順に確認します。E-ink 0 ボタン構成にボタン確認はありません。実機を使う段階はこの文書のオフライン検査とは別です。

## 7. 既存端末を更新する場合

新規導入の `cp -n` や新しい DB 作成に進まず、[更新前の退避](release-and-recovery.md#更新前の退避)から始めます。現在の実行ファイル、設定、SQLite DB、プラグインの `data/`、対象個体の旧 UF2・manifest を同じ時点の組合せで保存します。新候補の preflight 後にサーバと端末を一つずつ更新し、Web、HOME 接続、表示、入力を再確認します。失敗時は保存した組合せで復旧します。DB 本体だけを稼働中にコピーしないでください。

## 8. AWAY を使う場合

AWAY は HOME の初回動作を確認してから設定します。製品用ファームでは EEX1 による SOCKS5 接続先探索が必須です。別プロジェクトの Android 側 EEX1 アプリ、テザリング、TailSocks と利用できる経路、認証値の準備が必要です。このリポジトリだけで AWAY を完成させる手順はありません。[接続方式と未確認事項](../design/eex1-endpoint-discovery.md)を読み、各環境で宛先と経路を確認してください。HOME だけ使う場合は AWAY を選択しません。

## 9. 困ったとき

困ったときは、サーバ設定を読めない場合は TOML の書式と指定パス、接続しない場合はサーバと端末に設定した IPv4、SSID、ポート、画面が出ない場合はプロファイルと実際の画面型番・配線を順に確認します。確認用ログや質問に Wi-Fi パスワードや実機 ID を貼り付けないでください。
