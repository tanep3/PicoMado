> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# 書込み済みLCDのSDK使用法監査（2026-10-21）

最後に書込み/verify・再接続を確認したLCDのUF2 fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3aを基準とする。SDK原本・アプリruntime・実機を変更せず、保存証拠、当該source、実SDK、隔離host試験で照合した。秒単位の画面遅延の根因を確定するものではない。

## ビルドとSDKの同定

| 項目 | 照合結果 |
| --- | --- |
| accepted app source | f2bdc7b2e17cc620e5d568c16842ee6ea871e7b0。build sourceのC/header/CMake/lwipoptsは同commitと一致 |
| SDK実パス | $HOME/dev/PicoW/pico-sdk。gap-p2-build/example-lcd.private.log:1、LCD CMakeCache.txt:300で一致 |
| SDK | 2.3.1、079c6f39023649b154152db30f1d781e884879bc |
| CYW43 checkout | c1075d4bc440422cf2b2fd12c64a1f53f77660ee |
| lwIP checkout | 2.2.0、0a0452b2c39bdd91e252aef045c115f88f6ca773 |
| SDK2.3.1のgitlink | CYW43 055d64274b014dd7b1c2fc94d26e8a18face7124、lwIP 77dcd25a72509eb83f72b033d219b1d40cd8eb95（2.2.1） |
| LCD app arch | pico_cyw43_arch_lwip_threadsafe_background、CYW43_LWIP=1、PICO_CYW43_ARCH_THREADSAFE_BACKGROUND=1 |
| build | Release / pico_w / RP2040、stack4096B、NDEBUG、USB stdout timeout0、deadlock timeout0 |
| compile ON | EVENT_STREAM / STREAM_DIAGNOSTICS / ACK_PATH_PROBE / USB_BUTTON_SIM / TX_LINK_PROBE / CYW43_INNER_PROBE / AWAY_EEX1。EEX1 port48116、TRACE_STATS1、TRACE_VERBOSE0 |

実パスはフォルダ名から推測していない。artifact ELF SHA827f556716e32d4afbf78a78406ae3963d85a5ef07eb1c83c76983626af2edd8、UF2 SHA、ready.json、20261021T013421Z/flash-complete.jsonのcandidate SHAが対応する。archived ELFにはbackground初期化、EV1、CYW内probeの実symbolがある。

**現CMakeCache/compile_commandsは後のEV1 OFF試験で再利用済み**で、現ELF SHA5c83262c…は書込み済みON版ではない。ON flagsはgap-p2-build/results.jsonとready.jsonの退避済みON記録、ON ELF symbolで確認した。今回のpreprocessor結果は現compile databaseからEVENT_STREAMだけONに戻した再導出であり、元ビルド時の独立snapshotとは呼ばない。compile databaseには別target picomado_backlight_polarityのpoll TUもあるが、picomado_lcd.dirを選ぶとbackgroundで一致する（firmware/CMakeLists.txt:154,180）。

SDK reflogは2025-07-21 clone（当時9a4113…）、2026-10-11T11:32:44+09:00 pull→079c6f…、両submoduleは2025-07-23T14:54:16+09:00 checkoutのまま。両headはSDK **2.1.1のgitlinkと一致**する。記録上、SDK本体更新後に両submoduleが追随していない。誰がなぜその構成にしたかという意図は不明。build時git HEADを別保存した証拠は見つからず、現在checkout・それ以前のreflog・build依存パス・原本/生成source hashを組み合わせた同定である。

SDKのtracked source改変はなく、dirty表示はsubmodule head差を含む。CYW43原本LL SHAa4e6baa503b9332a0a445c8b22deb4bee62793c441dc74a200b0fe8065c397c7はビルド時pinと現在HEAD内容で一致する。計測版はbuild/private-cyw43だけの生成TU、SHA769485c707d80f7841a60f626a735d6001eef1948d8a1dfa425538764010746d。共有SDKへのpatch適用ではない（firmware/CMakeLists.txt:93–120）。

## lwipopts / asyncの契約

firmware/lwipopts.h:2–31はNO_SYS1、MEM_LIBC_MALLOC0、MEM_SIZE16000、MEMP_NUM_TCP_SEG32、PBUF_POOL_SIZE24、socket/netconn0、MSS1460、WND23360、SND_BUF11680、SND_QUEUELEN32、NETIF_TX_SINGLE_PBUF1。background initのNO_SYS/MEM_LIBC条件に適合する（SDK src/rp2_common/pico_cyw43_arch/cyw43_arch_threadsafe_background.c:21–47）。lwIPの既定LWIP_ASSERT_CORE_LOCKEDは空macroなので、assert未発生をlock正しさの証拠にしない。

SDK src/rp2_common/pico_async_context/include/pico/async_context.h:188–225は、blocking lockをworkerまたはnon-IRQから使用する契約。任意のapp IRQからは使わず、IRQ通知はset_work_pending（:370–382）を使う。アプリの入力はmainのgpio_get polling（main.c:1011–1034,1116）で、appが任意IRQからpm_net_*を呼ぶ経路や追加async workerは見つからない。

この版は低優先IRQに加え、同じcoreの最外lock releaseでもprocess_under_lockを実行する（SDK async_context_threadsafe_background.c:106–130,289–303）。callbackを常にIRQだけと断言しない。lock保持中は低優先handlerが処理を見送り、release時にworkerを処理する。background型の.pollは0（:360–371）なので、main.c:1123 / net.c:712のcyw43_arch_pollはこのモードでは実質no-op。poll呼出回数をnetwork処理進行の代用にしない。

## アプリとの照合

| 対象 | 判定と具体的根拠 |
| --- | --- |
| 初期化 | net.c:438–440でcyw43_arch_initを1回、main.c:1054–1061でSTA→panel初期化。独自async_contextへの差替えなし |
| raw TCP / PCB | net.c:708–757でmain側close/begin/poll/state/send/takeをSDK lockで囲む。new/connect/recv/err登録とclose/abortはその内部（:441–482）。callback内はSDKの既存lock文脈 |
| receive / pbuf chain | net.c:350–401はtot_lenを使うpbuf_copy_partialで全chainを所有rxへcopyしてfree。callback内でJSON解析/描画/USB待ちなし。copy戻り値を検査しないが、正常pbuf chainのtot_len/capacity契約下では短いcopyを生む根拠は見つからない |
| tcp_recved | net.c:240–254,368–394、rx_flow.h:18–36。copy済みowned領域の安全余裕まで早期creditし、残りはconsume時に返す。受信量＝返却credit累積＋uncredited、不正な二重返却を避ける。RX_CAP32768>WND23360、JSON最大fragmentがWND内 |
| tcp_err / PCB寿命 | lwIP tcp.h:110–120ではcallback時PCBは解放済み。net.c:344–348はpcb=NULLを先に設定し、後続snapshotもNULLを検査。旧PCBを参照しない |
| 送信buffer寿命 | net.c:280,329の全tcp_writeはCOPY。tx_packet、SOCKS/upgrade、Close/pongのstack入力はenqueue成功後に再使用可能。SOCKS credential copyもCOPY後に消去（:496–497） |
| enqueue / output / ERR_MEM | net.c:263–321はsndbuf事前確認に加えtcp_write戻り値を検査。ERR_MEMでもstage FAILEDへ進み、同dataを保持してsent/poll callbackで再試行する設計ではない。可用性上のtradeoff。sndbufのみで成功を保証しない。tcp_output戻り値は記録するがboolはCOPY enqueue成功を維持 |
| Nagle / window / input中 | LCDはnet.c:470–474でNagle無効。実lwIP tcp_out.c:1262はtcp_input_pcb一致時ERR_OKでも延期、:1266,1336はsnd_wnd/cwnd、:1347はNagle条件。ERR_OKは到着/ACKを保証しない。今回保存された欠落heartbeatは実linkoutputまで進んでおり、enqueue延期だけの説明にはならない |
| mainのlock内作業 | take内にJSONへの最大18KB copy、app/rx memmove（net.c:639–647,654–662,252）、poll内にupgrade header走査、send内に最大1KB encodingとtcp_output。描画/JSON数値配列解析はlock外。ただしlock内所要がゼロ/一定とはしない |
| UDP endpoint | endpoint_udp_sdk.c:16–23は固定2slotへのbounded copy/free、main操作は:27–72でSDK lock。任意IRQからのblocking raw APIなし |

**競合疑い（今回の遅延との因果未確認）**: main.c:282–287のwifi_offはpm_net_closeのlockを解放した後にcyw43_arch_disable_sta_modeを呼ぶ。実SDK cyw43_arch.c:36–44は外側lockなしでcyw43_cb_tcpip_deinitへ入り、CYW43側cyw43_lwip.c:243–263がdhcp_stop/netif_remove/ip/flags更新を行う。この部分にserializeが見当たらず、background受信との重なりをレビューする必要がある。公式arch helper内部の契約/実装の疑いとして扱い、アプリが直接raw APIを無保護で呼んだと断定しない。通常ACTIVEの上下切替はwifi_offを呼ばず（main.c:972–974）、保存heartbeat窓にも再接続はない。今回この箇所を修正していない。

## 長いcallback・lock・IRQ禁止とbus

保存済みUSB-off統計のうち今回DTR窓に対応するepoch10–12/15–16ではrx_cb最大207–220µs、sdk_wait最大8–53µs、rx_take最大2448–2491µs、net_send最大1290–1332µs、draw最大19804–20392µs、loop_gap最大38772–40029µs。過去区間を含むepoch9はrx_take8995µs/net_poll13157µs。これらは各snapshot期間の包括時間で、純粋なlock保持/IRQ禁止時間や各操作だけの最大値ではない。秒単位callbackやlock待ちをこれらの保存範囲では示していない。

- main.c:263–266はflash erase/program全体のIRQ禁止。**menu acceptだけでなく、ACTIVEのsettings_message:627–645 / api_settings:793–795からも実行される**。同revisionでも保存する。秒単位の遅延を直接示す計測はなく、上下入力から直接は呼ばれない。設定配信/再接続と重なる可能性を残す。今回5 AP2入力窓のhost decodeにSettings/kind3はない。
- trace.c:63–77等は固定record値copy、:133–142はformatをmask外へ出している。一方stats snapshot:266–279 / rx_stats.c:175–187は複数recordコピーを同一mask内で行う。USBはtrace_usb.c:9–16でCDC FIFO/flushまでmask。各最大IRQ禁止時間は未観測で、既存関数maxやhost fixtureを実測値として使わない。
- app sourceにPIO/DMA固定claim・全PIO clear/reset・IO_IRQ_BANK0 handler置換なし。LCDはhardware SPI0（lcd.c:43–69,113–148）、GPIO17/18/19/20/21、BL22。SDK CYW43はPIO SPI（CYW43_SPI_PIO1/CYW43_USE_SPI1）、unused SMとDMA2channelをclaim（SDK cyw43_bus_pio_spi.c:119–155）。通常spi0同士の競合ではない。
- CYW43 SDK GPIO IRQはpending通知だけ（cyw43_driver.c:50–60）、post_pollで再enable（:82–86）。PIO writeはDMA完了とTXSTALLを同期的に待つ（cyw43_bus_pio_spi.c:309–331）、WLAN FIFO-ready pollingもある（:512–538）。アプリが独自DMA/IRQを横取りする証拠はない。bus API成功はchipのRF送出や802.11 ACKを意味しない。

SDK内部の長い占有候補も区別する。private LL:646–695のcredit待ちは元実装で最大約1秒のpoll/waitを含み、DATAは再入回避のため配送しない。その間の共用spid_buf変化は既存18シナリオの条件付きモデルで再現するが、[今回25件](cyw43-existing-trials-20261021.md)は全てcredit待ち0で、その4欠落送信の根因に転用できない。SDK pin版との当該send common関数には差分がない。正しいapp lockを使っていてもSDK内部の待ちが短い保証にはならない。

## 電源管理と版差

実CYW43 cyw43.h:638–653でDEFAULT=PERFORMANCE=PM2、NONEはmode0。app main.c:313–320はSTA有効化後にNONE_PMを要求し、join後:185–192で1回読戻す。既存USB logにはrequest_sdk_result=0/readback_sdk_result=0/raw0x10/mode0/none1が保存済みで、再attachはcached値の再表示。今回radio設定変更や追加IOCTLはしていない。キャッシュを各欠落送信瞬間のradio内部状態とは呼ばない。

SDK pinまではCYW43 15 / lwIP94commit先で分岐なし。今回使用するTCP enqueue/output/Nagle部分は同じで、tcp.c/timeouts.cは不変。tcp_out.c差はtypoとNDEBUG時未使用引数抑制、tcp_in.cはwindowでtrim済みooseq末尾の置換guard追加。CYW43ではSTA/AP teardown時tcpip deinitとitf_state clear、host-wake pin/極性対応、任意RF-switch pin名等に差がある。PicoMadoはSTA arch helperを使用し、AP enable/disable APIは呼ばない。混在を根因や自動版上げの根拠にしない。

## 計測patchの影響と検証

private LL差分はcopy戻り長の観測、common入口/credit待ち/poll/read/元bus直前/戻りの固定RAM記録。元pbuf copy、poll、bus writeを同じ引数で1回実行し、RX配送/credit/timeout/再送/buffer復元を変更しない。TX wrapperも元linkoutputを同一pbufで1回転送し、その後pbufを読まない（tx_link_probe.c:28–39）。CRC等の時間増分はあり、wire trace一致だけでobserver effectがないとはしない。

実SDKのsend_ethernet/common/SPI poll/SDPCM parserを抽出した18シナリオをnativeとASan/UBSanでON/OFF比較し、bus bytes・呼出順・戻り値・pbuf semanticsが一致した。RX flowの既存hostテストも成功。RF・実IRQ timing・SDK全体のABIや実機neutralityをこれで保証しない。

監査証拠はSDK-usage-audit-20261021T030739Z直下のenvironment-early.json、git-provenance-history.json、source-evidence-manifest.json（78file照合）、effective-macros.json、archived-ELF-symbols.txt、pinned差分、SDK-driver-ON-OFF/result.json、saved-runtime-stats.json。SDK source/共有submodule・本番・USB・サービスには変更なし。容量案の実装も保留し、[容量契約](../design/observation-video-pressure-contract.md)へ別途レビュー補正する。
