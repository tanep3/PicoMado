# PicoMado LCD firmware

初版の 160×80 ST7735S・4 ボタン Pico W 用です。`picomado_selftest` はネット資格情報を使わない LCD/ボタン診断、`picomado_lcd` は端末通信を含むファームです。既存 YouTube 端末の配線・ガンマ・180 度画素回転を照合して実装しました。

通常 `picomado_lcd` は `PM_NORMAL_BUILD=1` で診断画像/段階待ち/バッファ回収を無効にします。個体configを編集せず、制御速度を `PM_LCD_SPI_BAUD` と同じ20MHz要求へ結線し、窓指定から全25,600B画素完了までCSを保持します。同速度の描画中再設定はしません。既存125MHz周辺クロックでは実速度は15.625MHzになる見込みで、起動USBの `lcd ready` がSDK読戻し値を記録します。40MHz要求は今回採用しません。通常USBログは診断画像ではありません。

2026-10-13、ユーザーははんだ不良修理後、診断の最高速度で表示の乱れがないことを確認しました。診断UPの最高値は20MHz要求です。通常版はこの同速度/CS保持条件を引き継ぎ、プラグイン画像の実表示とサーバ側で拡大した日本語文字の読みやすさもユーザー確認済みです。2026-10-13の後続試験では、AWAYからTailSocksと既存subnet routing経由で `192.168.0.120:16153` に接続し、Hello・画像受信／描画ACKまで確認しました。追加listenerは不要でした。HOME全操作の再接続・長期安定性の受入は継続し、ACK修正の実機証拠と区別します。[今回の接続記録](../experiments/tether-tailnet/results-2026-10-13-picomado-away-lan.md)。[現時点の検証・残件](verification-2026-10-12.md)を参照してください。今回変更した個体設定はAWAYのサーバ宛先だけです。Wi-Fi/SOCKS資格情報、GPIO、offset24、180度、色補正、ガンマは維持しました。

## 配線

SPI0: CS GP17, SCK GP18, MOSI GP19, DC GP20, RESET GP21。表示窓は 160×80、Y offset 24、MADCTL 0x60、INVON。左上 GP12 はサブ/決定、右上 GP13 は上/前、左下 GP14 は待機/取消、右下 GP15 は下/次で、全ボタンは pull-up・押下 LOW・35 ms 安定判定です。

2026-10-13にユーザーがBLKをGP22（physical pin29）へ接続したと報告し、対象個体の `PM_LCD_BLK_CONNECTED=1` を設定しました。`PM_LCD_BLK_ACTIVE_HIGH=1` は維持し、待機時はLCD黒表示後にGP22 LOW、復帰時はHIGHで既定のONへ戻します。GPIOは一度だけSIO出力へ初期化し、PWMは使いません。修理後の診断版でユーザーがHIGH約3Vで点灯、LOW 0Vで消灯を確認し、実モジュールはactive HIGHと確定しました。07:47 UTC、ユーザーが通常版の待機／復帰による消灯／点灯も受入済みとしました。サンプルのCONNECTED=0は未配線用の初期値です。待機時はWi-Fiも停止します。

同日07:24 UTC、ユーザーがBLK配線の断線を現物で確認しました。修理後の07:38 UTC、同じ極性診断版で右上UPにより点灯・約3V、左下POWERのトグルで消灯・0Vとなったと報告しました。通常版UF2を保全し、新たなBOOTSEL応答を受け07:44 UTCに通常版へ復帰転送しました。load／verify成功とHOME ACTIVE／READYのUSB状態を確認し、07:47 UTCに通常待機／復帰の消灯／点灯もユーザー確認済みとなりました。その後、ユーザーがLCD上の天気・雨雲・YouTube・風刺画の4プラグインすべての動作を確認しました。経過は `verification-2026-10-12.md` 末尾を参照してください。

## 待機復帰と遅延診断

4ボタン版はPOWERだけで復帰し、既存の35msチャタリング判定と短押し動作を維持します。SUB／UP／DOWNでは復帰しません。2ボタンの下長押し復帰は共通判定とホスト試験に追加しましたが、E-inkバックエンドと2ボタン実機の完成を意味しません。

通常ビルドは `PM_TRACE_ENABLE=1` で、ボタン、CRC完了、受信／描画ACK投入前後、実SPI転送開始終了、TCP戻り値、50ms以上のpollを `timing us=... event=... frame=... a=... b=... dropped=...` として記録します。固定RAMキューを使い、USBが未接続／満杯なら待たず、欠落数を示します。ACK投入・SDKのSPI戻り値はサーバ到達・LCD可視表示の証明とは区別します。診断入り修正版78571088版は08:26 UTCに転送・verify成功し、時刻イベントの出力を確認しています。元f61版は復旧用として保全しています。実機の押下遅延の原因・解決は操作ログとの照合待ちです。

ダミーconfigによる再現試験：`firmware/tests/run_host_tests.sh`。個体設定・資格情報・実機を使いません。ビルド／ホスト試験と実機受入の区別は検証記録末尾を参照してください。

## ビルド

既存 SDK `/home/tane/dev/PicoW/pico-sdk` を使います。ローカル picotool 2.1.1 は現 SDK が要求する 2.3 より古いため、SDK の UF2 後処理を切り、ローカル picotool で RP2040 ELF を UF2 へ変換します。インストールは不要です。

共有ソースは `src/` に 1 セットだけ置きます。個体設定は `devices/<端末名>/config.h`、同じ個体の Flash ID は `devices/<端末名>/board-id.txt` に分けます。実設定と `build/` は Git 除外です。`PM_DEVICE_ID` はユーザーが決めた安定名（今回 `PicoMado-lcd1`）で、Flash ID を名前に含める必要はありません。ビルド時には端末名を必須とし、`config.h` の `PM_DEVICE_ID` と選択名が異なればファームは接続しません。起動時には別ファイルの Flash ID と実ボードを照合します。

`config.h` には画面方式、解像度、画素形式、LCD 制御 IC、配線、回転、オフセット、4 ボタン配置も定義します。画像長検査、LCD 初期化・描画、ボタン入力、Hello の能力申告に使用します。初版の受入対象は ST7735S 160×80 RGB565 LCD だけです。E-ink や異なる画素形式・寸法はビルド時に拒否します。現 LCD は既存 YouTube 送信側が RGB565 に反転と赤青交換をかけてから送っていたため、標準 RGB565BE を送る PicoMado では `PM_LCD_COMPENSATE_INVERT_BGR=1` を LCD 転送直前に適用します。ローカル画面とサーバ画像の両方に一度だけ適用します。

```sh
firmware/tools/build.sh --device PicoMado-lcd1
# 出力: firmware/build/PicoMado-lcd1/PicoMado-lcd1.uf2
```

サンプル端末は `devices/sample-a` と `devices/sample-b` にダミー値だけを置いています。`firmware/tools/build.sh --device sample-a --sample` で `build/.samples/sample-a/` にビルドします。サンプル UF2 は実機へ書き込まないでください。資格情報や認証値をログ・Git に入れず、端末名で成果物を取り違えないようにしてください。

## 通信と状態

Pico 起点で direct TCP または TailSocks SOCKS5 CONNECT を行い、TCP 16153 `/device` の WebSocket へ Upgrade します。サーバの現 `protocol.rs` に合わせ、binary WebSocket に version/kind/length envelope と JSON を載せます。TCP 分割と WebSocket 分割をバッファリングし、長さ・種別・画像 160×80 RGB565 BE・連続 offset・最大 4096 B の断片・CRC32・device/session/display/screen owner 世代を検査します。受信バッファは 32 KiB、TCP 受信窓は 16 MSS です。安全なバッファへ取り込んだ分はすぐ受信枠を戻し、バッファ末尾側に 1 受信窓分の空きを残します。現サーバの 4096 バイト画像断片を JSON 配列にした 1 メッセージ全体を保持できます。サーバの画像断片サイズや符号化を変えた場合は再評価が必要です。完全受信後に ReceiptAck、SPI 描画後に RenderedAck を別々に返します。古い世代の受信や再接続後の残片は捨てます。

前回選択ネットワークとサーバ同期済み接続時間設定を Flash 末尾 4 KiB の先頭 1 ページに CRC 付きで保存します。資格情報自体はファーム個体設定に含みます。新ファームの Wi-Fi IPv4 は DHCP を使い、旧 YouTube ファームの固定 IP ビルド指定は引き継ぎません。接続中のサブは現在の試行を中断して家/外メニュー、サブで保存/接続、電源で破棄/元の経路へ戻ります。稼働中の上/下はサーバに操作を渡し、通常画面の電源はサーバ応答後に待機します。サブモードの電源はサーバへ取消しを渡します。Wi-Fi または WebSocket 断では同じプロファイルを再試行し、初期の接続 15/10 秒、再試行 5→30 秒、heartbeat 15 秒をサーバ設定で更新します。TCP keepalive も切断検知の補助に使います。

## 現時点の検証範囲・残件

- `tests/core_test.c` は画像長・offset・CRC・世代とボタンのチャタリングをホスト上で検査します。`tests/ws_accept_test.c` は WebSocket 応答検証の RFC 6455 例を検査します。`tests/rx_flow_test.c` は TCP 分割、連続フレーム、有界バッファ、途中切断の受信枠を検査します。
- 実機の自己診断は画面の向き・色を目視で確認してください。4 ボタンの反応はユーザーが確認済みです。USB CDC にはボタン番号のみ出力し、秘密値は出しません。
- 現サーバには汎用メニュー項目を Pico へ送るメッセージがありません。ローカルの家/外設定メニューは実装済みですが、サーバ駆動の共通メニューは契約追加後に実装します。
- 臨時 LAN ホストとの direct 接続、WebSocket Hello/Welcome、設定同期、ボタン操作の往復は実機確認済みです。最初の書込み版では TCP 受信窓が画像断片 1 通より小さく、画像転送が停滞しました。受信窓を拡大した版の実機再試験、画像 ACK、再接続、外出先 SOCKS5 経路は未確認です。WebSocket の `Sec-WebSocket-Accept` はホスト試験でも検証します。
- ユーザー作成の実 `config.h` は `devices/PicoMado-lcd1/` に内容を変えず移動済みです。秘密値は表示していません。外出先テザリング側 IP は変わり得るため、実機試験前に現行値を確認します。
- 外出先の資格情報が未設定の間はローカル設定メニューから外出先を選択できません。
- LCD は既存端末と同じ 180 度画素回転を採用しました。BLK は修理後の実機診断で active HIGH と確認済みです。
- 接続中の日本語ラベルは、ローカルの Noto Sans CJK JP から生成した小さな固定ビットマップを内蔵します。再生成手順は `tools/make_status_bitmaps.py` です。
- 個体configの診断フラグにかかわらず、通常ビルドでは診断BOOT/STAGEを0として扱います。診断/回収用ファームは別ターゲットです。色補正、Yオフセット24、180度回転は保持します。過去の受信/描画ACKと、修理後の診断目視合格は検証記録にあり、通常版の転送後の表示/通信確認はまだ必要です。

## 書込みと復旧

USBログはGPIO UARTではなくUSB CDC（個体serialで識別したttyACM）へ出します。サーバ/プロキシの失敗時は正常Wi-Fi/IPを保持し、再試行中にリンクを失った場合はWi-Fi再接続へ戻ります。CDCを後から開いた直後と15秒毎にprofile/app/transport/Wi-Fi/TCPIP/IP取得有無を出し、SSID/password/tokenや受信JSONは出しません。SOCKSはMETHOD(version5)→AUTH(version1)→CONNECT(version5)を別々に検証します。実main/netのhost回帰試験は `tests/app_timing_test.c` と `tests/net_socks_test.c` です。2026-10-13にこの修正とAWAYのLAN宛先設定を含む版を転送し、METHOD/AUTH/CONNECT、Hello、画像ACKまで実機で確認しました。

書込みには `firmware/tools/flash.sh --device PicoMado-lcd1` を使います。端末名を必須とし、同名のビルド出力と manifest の端末名・ボード ID・UF2 SHA256 を照合し、BOOTSEL が 1 台かつ `board-id.txt` の ID と一致したときだけ `picotool load -v -x` を実行します。`--check-only` を付けると書込みなしで一致だけを確認できます。接続ファームは当該 Pico に書込み・Flash verify 済みです。既存 YouTube の復旧用 UF2 は `PicoW_Youtube_Player/diagnostics/artifacts/buttons-rotation-20261010/video-rotate180.uf2` です。書込み前の現 Flash に対する復旧 UF2 の verify は確認済み。戻すときは同一機を手動 BOOTSEL にして復旧用 UF2 を `picotool load -v -x` で書きます。

## 今回のLCD実機確認（2026-10-13）

ユーザーが4プラグインの実機動作を確認しました。統合サーバの最終回帰は137件成功（実HTTPSのopt-in1件は未実行）、fmt・全target clippyも成功しています。風刺画は実生成・履歴・frame408の描画ACKまで担当報告で確認済みです。上の時刻付き診断記録は各時点の観測であり、通信遅延全般の解決や2ボタンE-ink実機の完成を示すものではありません。
