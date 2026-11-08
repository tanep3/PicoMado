# PicoMado firmware

PicoMado firmware runs on Raspberry Pi Pico W. It receives prepared images from the Linux server, draws them on a supported display, and sends physical button input and display acknowledgements. Begin with [初回設定](../docs/guides/first-setup.md) or [first setup in English](../docs/guides/first-setup.en.md); select parts from the [hardware support table](../docs/guides/hardware-support.md) before wiring or building.

## 対応構成

現行プロファイルは Pico W の 160×80 ST7735S LCD・4 ボタンと、250×122 V4 E-ink・0／2 ボタンです。LCD 4 ボタンと E-ink 0／2 ボタン構成は動作確認済みです。同じ表示器の配線を変える場合は新しいプロファイルを作れます。別の画面やマイコンを使う場合は[カスタマイズガイド](../docs/guides/customization.md)で必要な変更と検証を確認してください。

## 配線

現行プロファイルごとの LCD・E-ink の GPIO、電源、共通 GND、ボタン配線は[ハードウェア対応表](../docs/guides/hardware-support.md)にまとめています。表示器の電源電圧は使用するモジュールの仕様を確認してください。不明な端子や電圧を推測して接続しないでください。配線変更は電源を切って行います。

## 個体設定

`firmware/devices/<端末名>/profile.txt` が表示方式と GPIO を選び、Git に含めない `config.h` が個体の Wi-Fi・サーバ接続値を保持します。新規 `config.h` に画面やボタンの定義を追加せず、選択したプロファイルと一致させます。HOME だけを使う場合でも AWAY マクロの定義はコンパイルに必要です。ダミー値の扱いとファイルの作り方は[初回設定](../docs/guides/first-setup.md#3-ファームの個体設定を用意する)を参照してください。実際の資格情報を Git に追加しないでください。

## ビルド・書込み・確認

ソースからのビルドと BOOTSEL ドライブへの UF2 コピーは [初回設定](../docs/guides/first-setup.md#5-ビルドする) の順に行います。既存端末は先に[退避と復旧](../docs/guides/release-and-recovery.md)を確認します。

Pico W は HOME と任意の HOME2 でサーバの `/device` WebSocket に接続します。AWAY には別プロジェクトの Android EEX1 アプリ、テザリング、TailSocks/SOCKS5 経路が必要です。このリポジトリだけでは AWAY の一式は揃いません。実機未確認の構成をビルド成功だけで対応済みとしません。
HOME2 は `PM_HOME2_SSID`、`PM_HOME2_PASSWORD`、`PM_HOME2_SERVER_IP` の３つを `config.h` に設定すると利用できます。未定義・空値なら選択時に飛ばします。接続作業中または再試行中に SUB で NETWORK を開き、DOWN は HOME → HOME2 → AWAY、UP は逆順です。SUB で保存して接続、POWER で変更を破棄します。通常接続中の SUB の役割は変更しません。HOME/HOME2 は direct TCP、AWAY だけが EEX1／SOCKS5 です。

**旧ファームに戻す前に:** HOME2 を保存したまま旧版を起動すると、旧版はプロファイル ID 2 を拒否してその他の設定も初期値に戻します。必ず新ファームの NETWORK で HOME または AWAY を選択・保存し、選択結果を確認してから旧 UF2 に戻してください。[保存互換性と試験範囲](../docs/changes/HOME2-PROFILE-20261108.md)も参照してください。
