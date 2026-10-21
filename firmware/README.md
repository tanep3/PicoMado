# PicoMado firmware (LCD / E-ink)

> Current development entry: [AGENTS](../AGENTS.md). Run the root gate before builds. New artifact staging uses `tools/project_gate.py record`; deployment requires `preflight` and parent approval. Direct cargo install/cmake/picotool commands below are historical/manual bypass paths, not substitutes for this gate.


2026-10-20のソース保存点で採用済みの動画protocol、世代・ACK・取消し・EOF、Pico通常値と診断variantの区別は [採用済み通信と設定](adopted-protocol-and-configuration.md) にまとめています。過去の「現在」「未転送」は各記録時点であり、ソース更新を実機書込みと混同しません。

LCD 接続で小さい制御送信が ACK 待ちに重なる条件に対し、新規 PCB の接続開始前に Nagle を無効にする候補を隔離検証しました。HOME/AWAY/再接続と E-ink の設定維持、同 SDK の送出挙動はローカル試験済みです。既知正常版も Nagle は有効だったため、実機悪化の原因確定や改善確認とは扱いません。後続の承認済み LCD 版へ反映し、ソースも保存しました。実機経過と残る制約は検証記録の末尾にあります。[候補と検証範囲](nagle-candidate-verification-2026-10-19.md)。

160×80 ST7735S LCD・4 ボタンと、250×122 Waveshare Pico-ePaper V4・2／0 ボタンの Pico W 用です。`picomado_selftest` はネット資格情報を使わない LCD/ボタン診断、`picomado_lcd` は端末通信を含むファームです。既存 YouTube 端末の配線・ガンマ・180 度画素回転を照合して実装しました。

通常 `picomado_lcd` は `PM_NORMAL_BUILD=1` で診断画像/段階待ち/バッファ回収を無効にします。個体configを編集せず、制御速度を `PM_LCD_SPI_BAUD` と同じ20MHz要求へ結線し、窓指定から全25,600B画素完了までCSを保持します。同速度の描画中再設定はしません。既存125MHz周辺クロックでは実速度は15.625MHzになる見込みで、起動USBの `lcd ready` がSDK読戻し値を記録します。40MHz要求は今回採用しません。通常USBログは診断画像ではありません。

2026-10-13、ユーザーははんだ不良修理後、診断の最高速度で表示の乱れがないことを確認しました。診断UPの最高値は20MHz要求です。通常版はこの同速度/CS保持条件を引き継ぎ、プラグイン画像の実表示とサーバ側で拡大した日本語文字の読みやすさもユーザー確認済みです。2026-10-13の後続試験では、AWAYからTailSocksと既存subnet routing経由で `192.168.0.120:16153` に接続し、Hello・画像受信／描画ACKまで確認しました。追加listenerは不要でした。HOME全操作の再接続・長期安定性の受入は継続し、ACK修正の実機証拠と区別します。[今回の接続記録](../experiments/tether-tailnet/results-2026-10-13-picomado-away-lan.md)。[現時点の検証・残件](verification-2026-10-12.md)を参照してください。今回変更した個体設定はAWAYのサーバ宛先だけです。Wi-Fi/SOCKS資格情報、GPIO、offset24、180度、色補正、ガンマは維持しました。

## 配線

SPI0: CS GP17, SCK GP18, MOSI GP19, DC GP20, RESET GP21。表示窓は 160×80、Y offset 24、MADCTL 0x60、INVON。左上 GP12 はサブ/決定、右上 GP13 は上/前、左下 GP14 は待機/取消、右下 GP15 は下/次で、全ボタンは pull-up・押下 LOW・35 ms 安定判定です。

2026-10-13にユーザーがBLKをGP22（physical pin29）へ接続したと報告し、対象個体の `PM_LCD_BLK_CONNECTED=1` を設定しました。`PM_LCD_BLK_ACTIVE_HIGH=1` は維持し、待機時はLCD黒表示後にGP22 LOW、復帰時はHIGHで既定のONへ戻します。GPIOは一度だけSIO出力へ初期化し、PWMは使いません。修理後の診断版でユーザーがHIGH約3Vで点灯、LOW 0Vで消灯を確認し、実モジュールはactive HIGHと確定しました。07:47 UTC、ユーザーが通常版の待機／復帰による消灯／点灯も受入済みとしました。サンプルのCONNECTED=0は未配線用の初期値です。待機時はWi-Fiも停止します。

同日07:24 UTC、ユーザーがBLK配線の断線を現物で確認しました。修理後の07:38 UTC、同じ極性診断版で右上UPにより点灯・約3V、左下POWERのトグルで消灯・0Vとなったと報告しました。通常版UF2を保全し、新たなBOOTSEL応答を受け07:44 UTCに通常版へ復帰転送しました。load／verify成功とHOME ACTIVE／READYのUSB状態を確認し、07:47 UTCに通常待機／復帰の消灯／点灯もユーザー確認済みとなりました。その後、ユーザーがLCD上の天気・雨雲・YouTube・風刺画の4プラグインすべての動作を確認しました。経過は `verification-2026-10-12.md` 末尾を参照してください。

## 待機復帰と遅延診断

4ボタン版はPOWERだけで復帰し、既存の35msチャタリング判定と短押し動作を維持します。SUB／UP／DOWNでは復帰しません。2ボタンは短押しの安定した離上で前／次、上長押しでSUB／決定、下長押しで待機／取消／復帰を共通コードから通知します。押下ごとに長押し閾値を固定し、長押し後の短押しと連続発火を抑止します。E-inkバックエンドは実装・ホスト検証済みですが、今回のC版ファームの実機表示は未確認です。

通常ビルドは `PM_TRACE_ENABLE=1` で、ボタン、CRC完了、受信／描画ACK投入前後、実SPI転送開始終了、TCP戻り値、50ms以上のpollを `timing us=... event=... frame=... a=... b=... dropped=...` として記録します。固定RAMキューを使い、USBが未接続／満杯なら待たず、欠落数を示します。ACK投入・SDKのSPI戻り値はサーバ到達・LCD可視表示の証明とは区別します。診断入り修正版78571088版は08:26 UTCに転送・verify成功し、時刻イベントの出力を確認しています。元f61版は復旧用として保全しています。実機の押下遅延の原因・解決は操作ログとの照合待ちです。

ダミーconfigによる再現試験：`firmware/tests/run_host_tests.sh`。個体設定・資格情報・実機を使いません。ビルド／ホスト試験と実機受入の区別は検証記録末尾を参照してください。

## ビルド

既存 SDK `/home/tane/dev/PicoW/pico-sdk` を使います。ローカル picotool 2.1.1 は現 SDK が要求する 2.3 より古いため、SDK の UF2 後処理を切り、ローカル picotool で RP2040 ELF を UF2 へ変換します。インストールは不要です。

共有ソースは `src/` に 1 セットだけ置きます。個体設定は `devices/<端末名>/config.h`、同じ個体の Flash ID は `devices/<端末名>/board-id.txt` に分けます。実設定と `build/` は Git 除外です。`PM_DEVICE_ID` はユーザーが決めた安定名（今回 `PicoMado-lcd1`）で、Flash ID を名前に含める必要はありません。ビルド時には端末名を必須とし、`config.h` の `PM_DEVICE_ID` と選択名が異なればファームは接続しません。起動時には別ファイルの Flash ID と実ボードを照合します。

`config.h` には画面方式、解像度、画素形式、LCD 制御 IC、配線、回転、オフセット、4 ボタン配置も定義します。画像長検査、表示初期化・描画、ボタン入力、Hello の能力申告に使用します。LCD は160×80 RGB565BE、E-ink は250×122 mono1（MSB-first、白=1、行間paddingなし、3813B）を受けます。E-ink は旧実動V4の全画面更新だけを実装し、未知のパネル版・寸法・形式はビルド時に拒否します。現 LCD は既存 YouTube 送信側が RGB565 に反転と赤青交換をかけてから送っていたため、標準 RGB565BE を送る PicoMado では `PM_LCD_COMPENSATE_INVERT_BGR=1` を LCD 転送直前に適用します。ローカル画面とサーバ画像の両方に一度だけ適用します。

```sh
firmware/tools/build.sh --device PicoMado-lcd1
# 出力: firmware/build/PicoMado-lcd1/PicoMado-lcd1.uf2
```

サンプル端末は `devices/sample-a` と `devices/sample-b` にダミー値だけを置いています。`firmware/tools/build.sh --device sample-a --sample` で `build/.samples/sample-a/` にビルドします。サンプル UF2 は実機へ書き込まないでください。資格情報や認証値をログ・Git に入れず、端末名で成果物を取り違えないようにしてください。

## 通信と状態

Pico 起点で direct TCP または TailSocks SOCKS5 CONNECT を行い、TCP 16153 `/device` の WebSocket へ Upgrade します。サーバの現 `protocol.rs` に合わせ、binary WebSocket に version/kind/length envelope と JSON を載せます。TCP 分割と WebSocket 分割をバッファリングし、長さ・種別・画像寸法／設定形式（LCD RGB565BE または E-ink mono1）・連続 offset・最大 4096 B の断片・CRC32・device/session/display/screen owner 世代を検査します。受信バッファは 32 KiB、TCP 受信窓は 16 MSS です。安全なバッファへ取り込んだ分はすぐ受信枠を戻し、バッファ末尾側に 1 受信窓分の空きを残します。現サーバの 4096 バイト画像断片を JSON 配列にした 1 メッセージ全体を保持できます。サーバの画像断片サイズや符号化を変えた場合は再評価が必要です。完全受信後に ReceiptAck、SPI 描画後に RenderedAck を別々に返します。古い世代の受信や再接続後の残片は捨てます。

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

## LCDの固定RAM計測（2026-10-18）

通常LCD版は448Bの固定RAMにUSB flush、受信、処理、描画など12項目の回数/最大usを保存します。省電力修正と固定リングを維持し、毎フレームの時刻ログは出しません。USB CDCを開いた時に直前の集計をスナップショットし、USB閉鎖/開放の2区分を50msに1行ずつ一度だけ出します。測定用スクリプトはDTRを明示的に下げて閉じ、通常再生中の連続ログ採取を避けます。E-ink版はこの計測を有効化しません。

2026-10-18に対象LCDへ書込み・verifyし、USB閉鎖180秒を測定済みです。同じ動画6fpsで送信stallが8回発生し、改善は確認できていません。測定範囲、制約、受信待ちの追加調査は [固定RAM計測の検証記録](runtime-stats-verification-2026-10-18.md) を参照してください。省電力バグ版へ戻す案は撤回済みです。

## Wi-Fi省電力の初回接続と診断（2026-10-18）

通常起動ではLCD初期化前にSTAが有効なため、`start_wifi()` はSTAの有効化済みにかかわらず `CYW43_NONE_PM` を要求します。Wi-Fi接続成功時に一度だけPMを読み戻し、USBに要求結果・読戻し結果・raw値・mode・`none` を記録します。後からUSBを開いた際と15秒毎の状態表示は保存結果だけを再掲し、無線への追加問い合わせはしません。設定失敗・読戻し失敗・PM2との不一致は成功扱いせずログに残し、既存の接続・再試行は保ちます。サーバだけの再接続ではPM設定/読戻しを繰り返しません。

31件のホスト試験では初回起動、待機復帰、設定/読戻し失敗、不一致、遅いUSB、LCD/E-inkの既存処理を確認しました。実機の通信改善は未確認です。変更と実機比較手順は [Wi-Fi PM検証記録](wifi-pm-verification-2026-10-18.md) を参照してください。

## 今回のLCD実機確認（2026-10-13）

ユーザーが4プラグインの実機動作を確認しました。統合サーバの最終回帰は137件成功（実HTTPSのopt-in1件は未実行）、fmt・全target clippyも成功しています。風刺画は実生成・履歴・frame408の描画ACKまで担当報告で確認済みです。上の時刻付き診断記録は各時点の観測であり、通信遅延全般の解決や2ボタンE-ink実機の完成を示すものではありません。


## E-ink 端末（2026-10-14）

`src/main.c`／`net.c`／`core.c` は共通で、CMake が LCD の `display.c`＋`lcd.c` または E-ink の `display_eink.c` を選びます。E-ink_0 は0ボタン、E-ink_1 は上GP20（物理26番）・下GP21（物理27番）・共通GND物理28番、内蔵pull-up・押下LOWです。表示のSPI1／4MHz、CS9・SCK10・MOSI11・DC8・RESET12・BUSY13は旧V4実装と公式設定を継承しています。配線・世代の根拠と未確認点は [E-ink 検証記録](eink-verification-2026-10-14.md) を参照してください。

個体config.hはGit除外・0600です。HOME/AWAY等の10通信マクロはLCD個体の値をそのまま流用し、固定端末IPを追加していません。E-ink_1の元個体configは初回準備時の`PM_EINK_PANEL_CONNECTED=0`を維持し、別の表示有効V4候補を用意しています。ユーザーが 2026-10-14 20:50 JST に V4 を確認しました。この状態でもWi-Fi／WebSocket／ボタン処理を続け、ReceiptAckは完全受信後に返します。表示していない画像にRenderedAckは返しません。Flash IDの確認・結合後、通常転送用にはCONNECTED=1で再ビルドしてください。実機描画・ボタン試験は未実施です。V3の実機受入は未実施です。

表示はメインループから段階的に進み、BUSY待ちはsleepループを使いません。1 pollのRAM送信は最大128B、初期化から更新まで10秒の期限を設けます。待機要求では未開始の再描画を破棄し、進行中の物理更新だけを完了させて画像を保持します。sleep後の100msも非同期です。フレームの世代が変わった更新には描画ACKを返しません。

実機Flash IDが未確認の間は、次のコマンドでビルド検証のみ行えます。

```sh
firmware/tools/build.sh --device E-ink_0 --unbound
firmware/tools/build.sh --device E-ink_1 --unbound
```

成果物は`build/.unbound/<端末ID>/`に隔離し、manifestをUNBOUNDとします。実機への書込みは禁止され、ファームも実ボードに接続できません。本人確認後の16桁Flash IDをGit除外の`devices/<端末ID>/board-id.txt`へ設定し、通常ビルドを作り直してください。その後`flash.sh --device E-ink_0 --check-only`で対象を照合し、親側の確認済み対象にのみ通常のflash.shを使います。実機の本人確認／BOOTSEL確認前に書き込まないでください。

サーバ側は3813B mono1の生成・配信・検証を追加しました。風刺画は既存250×122の1bit PNGを使い、他の既存LCD描画は白余白を付けて1bitへ変換します。E-inkの動画はRenderedAck後だけ次の共有再生位置を取り込み、LCDの送信を待たせません。この変更はユーザーの承認後、既存削除featureと運用設定を維持してPicoMadoサーバへ反映済みです。E-ink_0のframe817でReceipt／Renderedの両ACKを確認しました。旧風刺画サービス・履歴は保持しています。


2026-10-14の実機試行：E-ink_0のローカル表示崩れは旧V4の最終SPI転置漏れと特定し、4000B完全一致のe8409d06版へ再書込み・verify済みです。本番反映は初回auto-review拒否後にユーザー承認を受け、同じ操作の一度の再試行で成功しました。HOME／Hello／Receipt／Rendered（frame817）、LCDの再接続とframe818描画も確認しました。旧風刺画サービス・履歴は維持。E-ink_0の実表示成功はユーザー確認済みです。詳細・SHA256・サイズ・残件は [E-ink 検証記録](eink-verification-2026-10-14.md) を参照してください。

2026-10-14追加修正：天気はLCDの色・形を保ち、E-inkのアイコンだけ黒く描きます。Webで現在機能をOFF／全OFFにした直後の新画面を同じ接続へ送り、古い動画・取得・ACKを拒否します。FrameStartの任意mode=normal/subにより、WebでSubを離れた際の局所入力状態も同期します。旧メッセージにmodeが無ければ従来どおりです。LCDの同期版は再BOOTSELで個体IDを照合し、19:05:43 JSTにload／verify／再起動済みです。E-ink_0は0入力なので再書込み不要。E-ink_1の未結合準備UF2には同期修正を含みます。

本番サーバは18:53:43 JSTに今回の修正版へ更新・再起動済み。既存削除feature／設定、旧サービスを維持しました。実Web保存で天気→全OFFロゴ→元の雨雲を試し、E-ink_0のframe889〜891とLCDのframe892〜894で両ACKを確認。同じセッションを維持し、終了後の両端末設定は試験前の雨雲のみONへ復元しました。サービス174成功、ファーム23実行PASS、fmt／clippy／Release成功。詳細は検証記録を参照してください。

LCD同期版の実機書込み後、HOME再接続とframe904の両ACKを確認。更新LCDだけのWeb保存で天気905→全OFFロゴ906→元の雨雲907を同じsession129へ送り、両ACKを確認しました。設定は雨雲のみONへ復元。物理SUB押下後のWeb解除操作全体はユーザー目視確認待ちです。サーバ追加再起動・E-ink書込みは行っていません。

## 受信中画像とボタン応答の保護（2026-10-14追加）

新応答のinput_contextでセッション・表示世代・モードを確認し、同じ世代の受信画像を保持します。古い応答／別フレームの断片は現画像を消しません。旧形式とも互換ですが、位置応答の世代を使えるサーバを先に展開してください。最新LCDのb817e5a4版は新しいBOOTSEL連絡後、20:53:30 JSTにload／verify／起動済みです。実機ログ、回帰、SHA、write_stallの未確定点は[検証記録](control-reply-verification-2026-10-14.md)を参照してください。

E-ink_1の通常表示を含むV4候補は`tools/build.sh --device E-ink_1 --unbound-panel`で作ります。成果物は`build/.unbound-panel/E-ink_1/E-ink_1.uf2`、表示無効版は従来の`.unbound`です。元configは変更せず、ビルド内の派生configだけ表示を有効にします。実物V4はユーザーが 2026-10-14 20:50 JST に V4 を確認済みですが、Flash ID未結合のため通常再ビルド前には転送できません。実機描画・ボタン試験は未実施、V3は未実装です。

+応答競合修正版LCDはHOME再接続とframe986の両ACK、Webの天気987→全OFFロゴ988→風刺画989の両ACKを確認しました。試験前の4機能ON／風刺画へ復元済み。物理SUB／連打と動画のwrite_stallの再現・受入はまだ未実施です。
+
## 動画バイナリ転送・背景受信（2026-10-14 22時以降）

LCD/E-ink_0/E-ink_1の共通通信は、能力frame-bytes-v1でraw pixel kind18を受け、制御/FrameStartと旧サーバの画像はJSONで扱います。旧YouTubeの背景受信方式を移植し、SDKロック内でtransportをコピーして描画前に解放します。3端末ともRenderedAckで最新再生位置へ進み、遅いE-inkの未描画画像を蓄積しません。

### 2026-10-15 LCD動画の追加能力

新しいLCDは `lcd-push-v1` を追加申告し、動画だけ描画ACKを送信許可に使わない最新画像方式へ進みます。静止画・旧LCD・E-inkのACK契約は維持します。組立中と描画用の領域を分け、1秒ごとの観測統計を任意送信します。実装・メモリ・6/18/24/12fpsの測定限界は [検証記録](lcd-push-verification-2026-10-15.md)を参照してください。

コミットする最新ソースには、機能時計を診断トレースから独立させる修正が含まれます。現在のLCD実機は10:23 JST書込版で、時計修正候補はビルド・ホスト試験済みですが未書込です。ソース更新やコミットだけでは実機のファームは変わりません。個体の `config.h` と `board-id.txt`、bindingを含むmanifest/UF2は引き続きGit除外です。

本番サーバは22:21:53 JSTに629afb3a版を反映、LCDは22:22:16 JSTにbb57f71e版のload/verifyを完了しました。動画は約26.3KB/frameですがACK待ち期限切れが再現し、実機受入は未完了です。旧playerのCYW43_NONE_PM方針も補完したc5c9fc74版を22:48:42 JSTにLCDへ転送・verify済みです。20分比較は約0.95fps/ACK待ち切れ13回で受入未完了。LCD宛送信経路がwlan0であることも確認し、有線比較の前提を訂正しました。E-ink向け候補は未転送です。最新SHA/サイズ、転送済み版との区別、有線化条件、残件は[検証記録](control-reply-verification-2026-10-14.md)を参照してください。E-ink_0も今回の更新対象です。

## 時計のV4部分更新候補（2026-10-18）

新しいE-inkファームは `eink-clock-v1` を申告し、時計だけに部分更新を許可します。初回・切替・復帰時に基準画像を作り、時計連続表示中の全面更新はサーバの端末別設定（既定10分）に従います。他プラグインとLCDは既存経路を維持します。公式V4のbase／partialとローカル横向き順序が根拠です。コード・ホスト試験・ビルドと実機受入を区別した詳細は[時計V4更新記録](clock-refresh-verification-2026-10-18.md)を参照してください。今回のサンプルUF2は書込み禁止です。

## AWAY接続先のEEX1取得

Androidの手動起動アプリを使う接続先交換クライアントを追加しました。HOMEは既存direct TCPのまま、`-DPM_AWAY_EEX1=ON`を指定した端末buildのAWAYだけに適用します。既定はOFF、探索UDP portは`PM_EEX1_PORT=48116`。認証成功とSOCKS CONNECT完了で検証した接続先をWi-Fi接続世代中のRAMへ保持し、TCP再接続で再利用します。取得IP/portを永続設定へ書き戻しません。

[参照仕様・出典・ビルド方法・実機確認範囲](../docs/eex1-endpoint-discovery.md)を参照してください。`TARGET_CONNECTED`はSOCKSの宛先接続成功を表し、PiのWelcomeや画像描画成功は別に確認します。Androidアプリは別プロジェクトで管理します。

## Controlled build provenance

Use the root gate receipt with the actual controlled builder. The build directory must be fresh (`PM_BUILD_DIR` may select one); an existing accepted image is never retroactively relabelled by `record`. `build-receipt.json` binds source, SDK/gitlinks, actual flags/compile commands, compiler and artifact. `flash.sh --verify-only` is offline; parent approval binds the exact candidate and build receipt. USB and service operation still require their own authorization.
