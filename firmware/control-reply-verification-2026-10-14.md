> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# ボタン応答と受信画像の競合 — 2026-10-14

受信画像破棄の論理バグは実mainを用いて再現し、修正・回帰・UF2ビルドまで完了した。サーバの応答コンテキスト追加は175試験・clippy・Release後に19:47:48 JSTに本番導入した。LCDは新しいBOOTSEL連絡後、20:53:30 JSTにb817e5a4修正版のload／verify／起動を完了した。write_stallは別事象として原因未確定を維持する。

## 確定したログ

- 19:09:45：LCD session129／YouTube frame914は91043 wire bytes中58558Bまで進み、write_stallで終了。19:10:12にsession131へ再接続し、frame916の両ACK。
- 19:15:05〜09：YouTube frame952〜959を送信。frame959は90695B中87006Bまで進み、19:15:09.846に2秒write_stall、19:15:10にsession134終了、19:15:18に135へ再接続。ソケット送信待ちの期限であり、画像生成タイムアウトではない。
- 19:16:38.158：天気SUBのUpを受理（App処理20.178ms、画像準備128us）。frame978は19:16:38.186に99371Bをソケットへ送信完了したが、両ACK・heartbeatが続かず、19:17:32の新Helloでsession136を失効。サーバが先にタイムアウトを宣言したログはない。端末側の切断理由やframe978の破棄を断定できない。
- 19:17:41以降：session137のframe981は両ACK。19:19:15〜19:20と19:26:04〜19:27:04のUSB採取はHOME／ACTIVE／READY／IP取得、heartbeat送信を確認。後から開いたUSBの先頭は過去のリング内容なので、採取時刻とeventの起動後usを混同しない。以前の診断983〜1002件は未接続中に欠落しており、障害直前の全順序は残っていない。
- E-ink_0の最後の描画は19:08:41。LCDの後の停止区間にE-ink物理描画はない。

証跡はtask-3/eink-evidence/freeze-20261014/server-journal.txt、lcd-usb.txt、lcd-usb-live.txt。生の受信JSONや秘密設定は採取していない。

## 原因の確定範囲と修正

旧mainのkind10 InputAcceptedは同じdisplay_epochでもpm_reset、kind17 PluginPositionは無条件pm_resetしていた。制御応答とフレーム送信は別タスクで、FrameStart→応答→FrameChunkの順も起こり得る。旧mainをホストで実行すると、応答→Start→ChunkはReceipt／Rendered各1、Start→応答→ChunkはACTIVEのまま各0になることを確認した。これは論理バグの確定で、ログ不足のframe978が同じ原因だったという確定ではない。

両応答に任意input_context={incarnation,session_epoch,display_epoch,mode}を追加。Picoは旧応答を拒否し、同じ世代の進行中assemblyを保持、新しい世代だけ旧assemblyを破棄する。選択応答より先に来た正しい新世代FrameStartも受理する。異なるフレームの遅延Chunkが現assemblyを消さないようにした。待機中／ローカル所有／古いセッション・表示・ownerは従来のゲートを維持。秘密値を含まない返信世代／保持判定、受信バッファ異常の診断を追加した。

## write_stallの切り分け

各接続は個別writer、FairQueue、sinkを持ち、ネットワークawaitやE-inkの物理Rendered待ちでApp mutexを保持しない。App／SQLiteの短い同期処理は共有する。採取範囲でLCDのlock_waitは最大12.231ms、処理51.217ms、画像準備194usで、2秒停止の説明にはならない。

LCD動画はFrameSent（OSへの送信）で次位置を許可し、端末のReceipt／Renderedを待たない。E-inkはRenderedAckで許可する。ログでは約90KBのJSON画像が短い間隔で先行送信され、送信待ちが1.26秒へ伸びた後にstallした。LCD側受信やTCPのバックプレッシャーは候補だが、障害時のTCP window／再送やPico RX状態を残せていないため原因は確定しない。タイムアウト・Wi-Fi・OS設定は変更していない。

## 回帰・互換・展開

- サービス175成功（lib170、binary1、integration4）、外部取得opt-in1件ignored。応答コンテキストが入力後のモード・世代と一致し、旧形式をencode／decodeできることをLCD／E-inkで検証。
- ファーム26実行PASS。両応答、前後・途中の順序、旧形式、古い世代／incarnation／session、別フレーム、連続切替、Web変更、SUB取消し、待機、設定同期、再接続、不正offsetをsample LCD／E-ink_0／E-ink_1で検証。
- 実際の旧main（7cf7e7b）を別の一時ホストビルドで使い、新コンテキスト付き応答と従来の画像／両ACKを3構成で確認。kind／version／画像契約は変更せず、追加項目は任意。新LCD＋旧サーバは旧形式の試験で確認。旧サーバの位置応答は世代を持たないため完全な古さ判定はできず、サーバ先行を推奨する。
- サーバを先行導入済み。SHA256 f0024aeaa3e4913de7360d3fc739957f8f105cb055ff75c309fe86f94d6fb3d3、15103032B、PID33784。caricature-retention-delete featureと起動完了後の実PICOMADO_CARICATURE_DELETE=1を維持。旧E-ink_0はframe982、旧LCDは983で両ACK・設定維持を確認。旧サービス／timer active。
- 新しいBOOTSEL連絡後、LCDの単一Flash ID E661640843239D21を照合し、20:53:30 JSTに転送・Flash verify100% OK・起動を完了した。E-ink書込み、OS／ネットワーク／xrdp変更、SSHは行っていない。

## 準備済みUF2

| 構成 | パス | bytes | 用途 |
| --- | --- | ---: | --- |
| LCD | build/PicoMado-lcd1/PicoMado-lcd1.uf2 | 719872 | load／verify済み |
| E-ink_1 V4表示有効 | build/.unbound-panel/E-ink_1/E-ink_1.uf2 | 722432 | UNBOUND-PANEL |
| E-ink_1 表示なし | build/.unbound/E-ink_1/E-ink_1.uf2 | 722432 | UNBOUND |

LCD SHA256: `b817e5a44ee0a95e8f4004b272b208ce5f9ca65091f02a1bbb7117ed21a717d0`

E-ink_1 V4表示有効 SHA256: `8acc3242c1aa65065c001fe1519606f346208f8adf6a3646bbb389ea441bed26`

E-ink_1 表示なし SHA256: `ecaa6c89c725c7f5a1a408c7230b1bff7caa18381e3e025fd7ed7690193a49fd`

E-ink_1の上GP20（物理26）・下GP21（物理27）・共通GND物理28、内蔵pull-up・LOW押下はkernel.mdの決定値と一致。診断BOOT／STAGEは0。表示有効候補は既存V4コードのフル更新を含む通常コードで、表示無効版とは別の明確な出力にした。

`tools/build.sh --device E-ink_1 --unbound-panel` はビルド内の0600派生configだけCONNECTED=1にする。元の秘密個体configはCONNECTED=0のまま、10通信マクロはLCDと同一。派生configとUF2もGit除外。Flash ID未結合なのでmanifestはUNBOUND-PANEL、実機へ転送禁止。実物パネルはユーザーが 2026-10-14 20:50 JST に V4 を確認済み。Flash ID確認後、元configのCONNECTED=1、正しいboard-id.txtで通常ビルドを作り直す。

V3ドライバはこのプロジェクトに実装していない。display_eink.cはVERSION!=4をコンパイル時に拒否する。ダミーV3設定の負のビルドでもこの明示ゲートの拒否を確認した。E-ink_1実物はユーザーが 2026-10-14 20:50 JST に V4 を確認した。V3へVERSIONを変えるだけでは対応しない。Flash ID未結合、E-ink_1の実機描画・配線・2ボタン押下試験は未実施。AWAY、長期運転・電流の確認も残る。

## 応答競合修正版LCDの実機転送後

20:52 JSTのユーザーBOOTSEL連絡を受け、期待SHA256 b817e5a44ee0a95e8f4004b272b208ce5f9ca65091f02a1bbb7117ed21a717d0と単一対象E661640843239D21を照合。20:53:30 JSTにload／Flash verify100% OK／アプリ起動、終了コード0を確認。ログは/tmp/picomado-lcd-control-fix-flash.txt。

20:53:36にincarnation3／session139へ再接続。frame986のReceiptAckとRenderedAckは20:53:39、USBもCRC25600B・Receipt投入成功・Rendered投入成功、HOME／ACTIVE／READYを確認。接続前のリング欠落12件はカウントされているが、両ACKの実到着はサーバで確認した。

20:56:13〜16に更新LCDのWeb切替を試験。試験前は4機能ON／風刺画。未選択YouTube・雨雲を先にOFFにし、動画を開始せず風刺画OFF→天気frame987、天気OFF→中央ロゴ988、風刺画ON→復元989で両ACKを確認。同じsession139を維持し、最後は4機能ON／選択風刺画へ復元（enabled_revision61）。E-ink側設定は操作していない。

証跡はfreeze-20261014/lcd-control-fixed-usb.txt、lcd-control-fixed-server-journal.txt、lcd-control-fixed-web-acks.json。サーバはPID33784／19:47:48 JST起動のままで追加再起動なし。通常V4 E-ink_1候補の8acc3242版は変更せず、未結合・未転送・実機描画／ボタン試験未実施。

物理ボタンの競合再現とwrite_stallの受入は未実施。次のユーザー操作を一度に依頼し、操作前からUSB／サーバログを採る：天気でSUB→上下を数回→電源取消し→雨雲でも同じ操作、続いてキャッシュ済み動画を30秒ほど再生して下で退出。固まった場合は直前操作・画面・時刻を記録し、再起動せず保持する。これまでのWeb切替とホスト回帰の成功を、物理連打や動画停止原因の解消と混同しない。

## 動画転送と背景受信の移植 — 22時以降の記録

21:02の物理操作では動画の接続復旧画面が再発した。旧YouTube実装はraw RGB565バイナリ、同じ2秒socket timeout、過ぎた位置を飛ばす共有時計、`pico_cyw43_arch_lwip_threadsafe_background`、描画中に受信可能な保護済みready領域を使っていた。今回、共通ファームへこの受信方式を移した。受信callbackはSDKロック内のtransport領域だけを書き、mainの画像コピーを保持する。JSON解釈・CRC・LCD SPI・E-inkの更新はロック外。traceのIRQ/main共用リングも短いIRQ保護にし、USB待ちを入れていない。SDKがIRQ中libc mallocを禁止するため`MEM_LIBC_MALLOC=0`にし、既存の16000B内部heapと他のbuffer/window値を維持した。LCDのリンク時heap余白42252Bは静的値であり、実行時heap使用量は未測定。

新Helloの`frame-bytes-v1`能力がある場合のみ画像断片をkind18へ切替。version1の4B BE長さ＋version/kindを保ち、bodyはdevice ID長1B・ID・5つのu64 BE世代/ID・u32 BE offset・最大4096Bのraw pixels。制御とFrameStartはJSON、古い能力の端末は従来JSONを受ける。新Picoも旧サーバJSONを受ける。token/offset/CRC/ownerゲートは両経路共用。実LCDの25600Bは約26314〜26315 wire Bとなった。JSON短縮やbase64化ではない。

LCD/E-inkともRenderedAck後に次の共有再生位置を採用する。端末ごとに未描画1フレームまでで、遅いE-inkには欠落位置を再送しない。他端末のcredit、input、heartbeatを待たせず、network await/物理描画待ちでApp mutexを保持しない。ACK欠落時は対象接続だけを10秒LCD/15秒E-inkで閉じて復旧する。socket stall2秒・control250ms・wire長からの送信期限は増やしていない。ACK監視は最終断片の成功時に登録する。最初の回帰が誤った登録位置を検出し、修正して通過した。

サービス全targetは178成功=lib173+binary1+integration4、失敗0・filtered0。従来175=170+1+4に対し、バイナリprotocol/実WebSocketの2件とACK欠落1件を追加した。外部HTTPS opt-in1件だけ従来どおりignored、example2本/host binaryは各0件。遅いE-ink対速いLCDの既存試験を強化し、LCDが4位置進む間E-inkは1pending、描画後は最新位置から復帰することも確認した。clippy全target・fmt・Release（caricature-retention-delete）成功。ファーム26実行PASSにはSDKロック、split >4KiB/NUL binary、描画中のcopy保持、native/旧JSON混在・古い世代・入力割込みが含まれる。旧7cf7e7b mainもLCD/E-ink_0/E-ink_1の3構成で追加context＋JSONフレーム/両ACKを確認。

ユーザーが22:04 JSTにraspi5の192.0.2.120を有線へ移した。22:05の読取りでeth0を確認し、ネット設定は操作していない。21時のWi-Fi記録と22時以降の有線記録を直接の性能比較に使わない。有線の旧本番でも22:15〜18にwrite_stallを再現した。証跡は`task-3/eink-evidence/freeze-20261014/stream-native-v1/`に分離した。

本番サーバは2026-10-14T22:21:53.754188+09:00に反映。SHA256 `629afb3a292ba84c5c95b8bddcc27e5649991d7fece4a9e2a723d845b95867a4`、15105696B、PID105348。unit hash不変、実削除env=1、feature有効、API200と旧風刺画service/timer稼働を確認。旧E-ink_0はsession14/frame7666でReceiptとRendered、heartbeatを確認し、風刺画のみONを維持した。

新LCD BOOTSEL連絡後、単一Flash ID E661640843239D21とbb57f71eの全SHAを照合し、22:22:16 JSTに719872Bをload/Flash verify100% OK/起動した。Hello能力にframe-bytes-v1、frame7667から動画の両ACK、ignored ACK0を確認。しかしsession312 frame7678、313 frame7702、314 frame7761でACK待ち期限切れと再接続が再現した。USBではこれらのCRC/SPI/ACKのTCP投入が完了し、サーバに届かない例がある。TCPは再送・cwnd低下を記録している。タイムアウトを長くして解決としない。動画連続再生と物理ボタンの受入は未完了。

旧実動playerはSTA開始後にSDKロック内で`CYW43_NONE_PM`を設定していたが、bb57f71e移植には含めていなかった。SDK標準はPERFORMANCEでもPM2である。旧playerと同じ接続中radio方針を共通mainへ補完し、SDK結果だけログへ出す。失敗でも接続処理を保ち、待機では従来どおりSTAを停止する。OS/ルータ/SSID/HOME/AWAY/プロキシ/端末IPの設定変更はしていない。この差分の効果はまだ実機未検証で、PM2が障害の唯一の原因という確定ではない。接続中消費電流は未測定。

次が最新の省電力方針補完候補。26ホスト実行/3個体ビルド通過、いずれも未転送。既に書いたbb57f71eとは異なる。

| 端末 | パス（firmwareから） | UF2 B | text B | bss B | 区分 |
| --- | --- | ---: | ---: | ---: | --- |
| PicoMado-lcd1 | `build/PicoMado-lcd1/PicoMado-lcd1.uf2` | 720384 | 366132 | 214652 | NORMAL |
| E-ink_0 | `build/E-ink_0/E-ink_0.uf2` | 719872 | 365876 | 175108 | NORMAL |
| E-ink_1 | `build/.unbound-panel/E-ink_1/E-ink_1.uf2` | 722432 | 367244 | 175204 | UNBOUND-PANEL |

PicoMado-lcd1 SHA256: `c5c9fc741c94cc77fbdde036548fc17e905a9637d0313d4d29692f5db2ab1d56`

E-ink_0 SHA256: `5d51e67578e72c9e91c5890da3920825b8ef1c69c60e83609063e2be5d368617`

E-ink_1 SHA256: `3a3fc50da9131a7330243c5bbd8697f128b6f49d11a44844bb0547967a44796b`

UF2/config.hは0600・Git除外。E-ink_0/1の10通信マクロはLCDと一致。E-ink_0はID結合済み・V4表示有効・0入力。E-ink_1はV4確認済み、GP20/GP21の2入力、元config CONNECTED=0、派生だけ1、Flash ID未確認のUNBOUND-PANELで実機へ転送禁止。LCDの追加転送には新たなBOOTSEL連絡、E-ink_0には個体ごとのBOOTSEL連絡、E-ink_1には個体確認/ID結合/通常再ビルドとBOOTSELが必要。追加commit/pushはしていない。

## 旧YouTubeとの差分レビュー — 22:38 JST

追加書込み待ちの間、旧CMakeLists.txt・pico_player.c・lcd_display.h・lwipopts.h・streamer.py・開発履歴と現コードを読み取りで比較した。以下はc5c9fc74候補に対するレビューで、実機効果の確定ではない。レビューによるソース/UF2の追加変更はしていない。

| 対象 | 旧実動コード | 今回の対応/差分 |
| --- | --- | --- |
| 画像のwire形式 | PWF2 20Bヘッダ＋raw RGB565 25600B | raw kind18約26315B、制御/世代とCRCを共通契約に載せる。旧JSON端末と段階配備可能 |
| SPI中の受信 | threadsafe_background＋2ready slots、readyを描画終了まで保護 | 同じbackground、SDKロック内のRX/appコピーとmainの描画用copyを分離。callbackは描画pixelsを書かない。streamは端末ごとに未描画1枚まで。旧2slotと同一構造ではない |
| フレーム蓄積 | 両ready満杯なら破棄し、受信creditは返す | 未描画creditを待ち、共有時計の最新位置へ復帰。遅いE-inkがLCD/heartbeat/JMAを阻害しない回帰あり |
| 送信/制御 | sendall、elapsed時刻で過ぎた位置を飛ばす、reverse status別thread | monotonic＋missed tick skip、writer/reader別task、個別queueとsink。RenderedAck信用と期限で無制限先行を抑止 |
| radio省電力 | STA開始時のNONE要求と初回join後のGET_PM読戻し | NONE要求をSDKロック内へ補完。読戻し診断は未移植。SDK戻り値だけで実モードを断定しない |
| TCP復旧 | socket stall2sとPicoのvideo受信空白5sで閉じる。正常Wi-Fiは保持 | socket stall2sを維持、Wi-Fi/IP保持。静止画にも使うため5s無受信closeは移さず、heartbeat/アプリ期限と個別描画ACK期限を使う |
| LCD描画 | SPI0、20MHz、RGB565、色補正/180度/offset/gamma | 対応済み。実測SPI約15.6ms、hostで全25600B/CS/BSY確認。画像ごとのSPI中のIRQを止めていない |
| lwIP | window16*MSS、snd8*MSS、TCPseg32、pool32、heap8192 | window/snd/seg同等。pool24とheap16000は現既存値、IRQ互換allocatorへ変更。新機でpool枯渇統計は未測定なので、枯渇なしとは断定しない。根拠なしのpool増量はしていない |
| main待ち | 1ms周期 | 最大5ms sleep、2受信/回までで入力機会を戻す。frame CRCと約15.6ms SPIはmain。USB非接続/満杯でも10,000 polling回帰あり |
| 診断/旧固有 | JPEG/RX-only切分け、PWS統計、RSSI任意（既定無効）、isolated CYW43 driver計装（既定OFF）、固定IP | JPEGや診断SDKは今回のraw共通配信要件外。RSSI周期ioctlは追加せず、SDK/OS/ルータを改変していない。固定IPは移さずユーザー指示のDHCP/個体IDを維持 |

旧履歴にも、NONE要求後に2秒socket期限で長い停止が減った一方、4.88fps程度/状態報告途切れ/再送が残り、安定6fps未達という記録がある。旧状態の計測はPBUF max18/32・割当失敗0・TCP drop/memerr0だったが、現在の条件/コードで同じという証拠ではない。特にraspi5有線化前後、今回の1credit、旧2slot、reverse statusと毎frame両ACKでは比較条件が異なる。

ローカルSDKのcyw43_ll.c:2002以降は複数のiovar設定の戻り値を確認せず、最後のPM設定ioctlも結果を確認せず0を返す。旧pico_player.c:329以降の一度だけWLC_GET_PMは、そのための実状態診断であり、動作設定を変更しない。現候補のログは意図どおりrequested=noneという要求/SDK結果であり、actual=noneとは表示していない。まず現候補の連続動画/ボタン試験を行い、残ればreadback・pool/TCP統計などを根拠付きで検討する。候補変更と再書込みを推測で連続要求しない。

## 20分採取の終了 — 22:42:40 JST

自作collector PID105715は22:22:40〜22:42:40に自然終了、journal childも終了コード0で停止した。USB 13934行・server 7041行・TCP 1151snapshot。`stream-native-v1/post-flash/summary.json`に集計を保存した。

サーバjournal（USB開始前の22:22:22からの起動直後ログを含む）のLCDはReceipt 1133・Rendered 1124・ACK待ち切れ14・write_stall0・ignored ACK0。描画ACK間隔中央値0.165秒、平均0.93枚/秒、最大空白28.29秒、2秒超空白118。SPI 1128例は中央値15671us・最大17189us、Start→CRC最大7.46秒。USB受信overflow/send_failedログ0、button event0。これはTCP到達/描画完了通知の集計で、目視・物理ボタンの受入ではない。

旧E-ink_0は同じsessionの両ACK各1・heartbeat82・ACK待ち切れ0。今回の集計は転送済みbb57f71eであり、最新c5c9fc74候補は未転送、試験効果未確認。追加BOOTSEL連絡は未受信。ネット・デスクトップ設定変更、稼働サービスの追加再起動、候補の追加変更はしていない。

## 省電力方針補完版LCD転送 — 22:48 JST

22:47 JSTの新BOOTSEL連絡後、単一Flash ID E661640843239D21・最新c5c9fc741c94cc77fbdde036548fc17e905a9637d0313d4d29692f5db2ab1d56・720384B・通常LCD/4入力/個体config/manifestを照合。22:48:42 JSTにload/Flash verify100% OK/起動、22:48:49にsession328、YouTube frame9109のReceipt/Renderedを確認した。22:50:08まで両ACK各115・ACK待ち切れ0、短時間の観測であり改善の受入ではない。PM設定要求の初期USBログは採取開始前にリングから欠落し、実PMモード読戻しも未実装のため、PM実状態は未確認。

同じraspi5有線環境・同じ本番PID105348のまま、post-pm-flashへ別採取を開始。collector PID115154、22:49:21から20分予定。旧E-ink_0は再転送せず同じ接続/設定を維持。E-ink_1未結合、追加E-ink転送なし。Flash logは/tmp/picomado-stream-pm-lcd-flash.txt、証跡lcd-pm-flash.json。連続動画/物理ボタンの受入は未完了。

## 経路の確認による比較条件訂正 — 23:06 JST

23:05〜06の読み取りで、実LCDのDHCPアドレス192.168.0.173へ、サーバのbind送信元192.0.2.120を指定した`ip -4 route get`もdev wlan0を返した。eth0のIPv4 routeは空、wlan0に192.168.0.0/24とdefault（metric600）がある。TCP socketは192.0.2.120:16153→LCDである。eth0にIPがあるという22:05確認だけでは、端末向け送信が有線だとは確定できなかった。前半区間の経路は採っていないので、この20分を純粋な有線区間とは記載しない。停止の唯一の原因と断定せず、route-evidence.jsonを保存した。ネット設定変更は禁止のため一切変更していない。

## 最新LCD継続試験終了 — 23:09 JST

20分collectorは23:09:21に正常終了（USB13764行、server8376行、TCP1150 snapshot）。負荷測定は23:09:22終了、比較処理も終了し全自作プロセス残存なし。本番PID105348と旧サービス/削除実フラグは維持。

同一の選択区間秒を分母に揃えると動画fpsはbb57f71eの0.92からc5c9fc74の0.95、描画間隔p95は4.49→4.39秒、最大28.29→21.99秒。新ACK待ち切れ13・write_stall1・再接続14で、解決/高FPSとは判断できない。新区間はユーザーが物理ボタン50コマンドを実行しており、動画以外の操作区間を分離した。32件をログ順序でACKへ関連づけると受信→ACK中央値203ms/p95 1.72秒/最大4.64秒、8件置換・9件表示変更なし・1件対応なし/接続中断。FrameStartに表示epochがないため関連付けは推定で、物理押下時刻を測ったものではない。

サーバCPU単一core換算中央値2.98%/p95 15.89%/最大60.88%、RSS中央値52.73MiB/最大55.07MiB。host loadavgとは分けた。旧CPU負荷は未採取。比較表と限界はtask-3/eink-evidence/freeze-20261014/stream-native-v1/post-pm-flash/comparison.mdとcomparison.json。

重要な条件訂正: 実送信元指定のLCD宛routeはwlan0、eth0 routeなし（eth0 link/carrier自体はUP）。この区間を純粋な有線とみなさない。OS/ネット設定の修正はユーザー指示で禁止されており実行していない。次の経路確認/設定方針は親に報告する。追加ファーム変更や転送は行っていない。
