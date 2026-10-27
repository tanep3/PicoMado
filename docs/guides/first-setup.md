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

新しく作る `config.h` には画面・ボタンの定義も `#include` も書きません。ビルドが `profile.txt` に対応する追跡済みプロファイルを読み込みます。個体設定で選べるハード項目は LCD バックライトの接続・極性、または E-ink パネル接続の有無だけです。旧 config はプロファイルとハード定義が完全に一致する場合だけ利用できます。

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

## 5. 変更を検証し、成果物を作る

このリポジトリの現行工程では、変更 ID、先行する設計コミット、clean なソース、root gate の検査記録が必要です。下記は検査・ビルドの入口を示すもので、現在の未承認ブランチから実機へ導入する許可ではありません。`check` が成功した同じコミットで、空のビルド先を指定します。

```sh
python3 tools/project_gate.py check
python3 tools/project_gate.py build --kind server --build-dir /tmp/picomado-server-build
PM_DEVICE=my-pico  # 手順3で作った端末ディレクトリ名
firmware/tools/build.sh --device "$PM_DEVICE"
```

サーバ実行ファイルは指定したディレクトリの `release/picomado-server`、端末の UF2 は `firmware/build/$PM_DEVICE/$PM_DEVICE.uf2` にできます。`build.sh` はサンプルではない場合、個体設定・ボード ID に結び付けた候補記録も作ります。ビルド成功は実機動作や導入承認を示しません。

## 6. 承認された成果物を導入して確認する

[復旧手順](release-and-recovery.md)に従って旧サーバ実行ファイル、設定、DB、各プラグインの `data/`、端末の旧 UF2 を対応付けて退避します。独立レビューを受け、`project_gate.py preflight` で同じ成果物と承認記録を照合します。サーバ実行ファイルを配置する場合は [user-service 設定](../../service/deploy/README.md) の `ExecStart` が実際の配置先を指すようにします。既存サービスの停止・差替え・起動は所有者の承認を得た別操作です。端末の書込みも、対象を BOOTSEL で識別し、`firmware/tools/flash.sh --device "$PM_DEVICE" --check-only` で一致を確認した後の別操作です。直接 `cargo install`、CMake、`picotool load` で gate を迂回しないでください。

サーバが起動したら設定した IPv4 のポート `16153` にブラウザから接続し、画面が表示されることを確認します。端末を起動し、HOME Wi-Fi への接続、サーバへの Hello、画面の表示、ボタン入力を順に確認します。確認できない段階を次段階の成功と扱わないでください。E-ink 0 ボタン構成にはボタン入力の確認はありません。

## 7. AWAY を使う場合

AWAY は HOME の初回動作を確認してから設定します。製品用ファームでは EEX1 による SOCKS5 接続先探索が必須です。別プロジェクトの Android 側 EEX1 アプリ、テザリング、TailSocks と利用できる経路、認証値の準備が必要です。このリポジトリだけで AWAY を完成させる手順はありません。[接続方式と未確認事項](../design/eex1-endpoint-discovery.md)を読み、各環境で宛先と経路を確認してください。HOME だけ使う場合は AWAY を選択しません。

## 8. 確認と復旧

開発者向けのオフライン確認は `python3 tools/project_gate.py check` です。既存端末の設定や DB を残したまま新バージョンを検討し、変更前の実行ファイル、設定、DB、plugin の `data/` を整合する時点でバックアップします。戻すときは保存した同じ組合せを復元し、端末ファームも対象個体に一致するものを使います。サービス停止や実機書込みはこの文書だけで許可されません。

期待される設定結果は、サーバ設定ファイルに自分の LAN アドレスと保存先が入り、個体ディレクトリに選んだプロファイル、秘密の `config.h`、実機と照合した ID が揃うことです。これは起動・描画の確認ではありません。root gate と所有者の承認を経たビルド・導入の手順は [開発工程](../development-process.md) と [復旧手順](release-and-recovery.md) に従います。

困ったときは、サーバ設定を読めない場合は TOML の書式と指定パス、接続しない場合はサーバと端末に設定した IPv4、SSID、ポート、画面が出ない場合はプロファイルと実際の画面型番・配線を順に確認します。確認用ログや質問に Wi-Fi パスワードや実機 ID を貼り付けないでください。E-ink 2 ボタン完成端末の実機動作はまだ未確認です。
