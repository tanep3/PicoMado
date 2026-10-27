# PicoMado firmware

PicoMado firmware runs on Raspberry Pi Pico W. It receives prepared images from the Linux server, draws them on a supported display, and sends physical button input and display acknowledgements. Begin with [初回設定](../docs/guides/first-setup.md) or [first setup in English](../docs/guides/first-setup.en.md); select parts from the [hardware support table](../docs/guides/hardware-support.md) before wiring or building.

## 対応構成

現行プロファイルは Pico W の 160×80 ST7735S LCD・4 ボタンと、250×122 V4 E-ink・0／2 ボタンです。LCD 4 ボタンと E-ink 0 ボタンには実機確認記録があります。E-ink 2 ボタン完成端末は実機受入が未完了です。Pico 2 W、別寸法・別版の表示器、任意のボタン配置は対応対象ではありません。

## 配線

現行プロファイルごとの LCD・E-ink の GPIO、電源、共通 GND、ボタン配線は[ハードウェア対応表](../docs/guides/hardware-support.md)にまとめています。表示器の電源電圧は使用するモジュールの仕様を確認してください。不明な端子や電圧を推測して接続しないでください。配線変更は電源を切って行います。

## 個体設定

`firmware/devices/<端末名>/profile.txt` が表示方式と GPIO を選び、Git に含めない `config.h` が個体の Wi-Fi・サーバ接続値を保持します。`board-id.txt` には対象 Pico W から読み取った Flash ID を入れます。新規 `config.h` に画面やボタンの定義を追加せず、選択したプロファイルと一致させます。HOME だけを使う場合でも AWAY マクロの定義はコンパイルに必要です。ダミー値の扱いとファイルの作り方は[初回設定](../docs/guides/first-setup.md#3-ファームの個体設定を用意する)を参照してください。実際の資格情報や Flash ID を Git に追加しないでください。

## ビルド・書込み・確認

ソースからのビルドは root gate に結び付く `firmware/tools/build.sh`、対象確認と書込みは `firmware/tools/flash.sh` を使用します。依存ツール、候補記録、独立レビュー、offline preflight、BOOTSEL 照合、画面とボタンの確認は[初回設定](../docs/guides/first-setup.md#5-ビルドできる配布物か確認する)の順に進めてください。既存端末には先に[退避と復旧](../docs/guides/release-and-recovery.md)を適用します。サンプルの Wi-Fi 値と UF2 は実機用ではありません。

Pico W は HOME でサーバの `/device` WebSocket に接続します。AWAY には別プロジェクトの Android EEX1 アプリ、テザリング、TailSocks/SOCKS5 経路が必要です。このリポジトリだけでは AWAY の一式は揃いません。実機未確認の構成をビルド成功だけで対応済みとしません。

## 過去の記録

旧 SDK 場所、直接変換・書込み例、日付付きの実機観測は[開発履歴](history/development-record-before-20261027.md)に保全しました。そこにある「現在」は各記録時点の状態です。現行の導入手順として実行しないでください。
