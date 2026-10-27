# LCD固定RAM計測の準備と検証（2026-10-18）

省電力修正は維持する。通信方式、フレーム形式、送信/接続期限、サーバ、端末選択、有効化設定は変更しない。旧版復帰案は撤回済み。実装・ホスト試験・LCD実configビルド後、明示許可されたLCD書込みと180秒実機測定まで完了。改善は確認できず、USB閉鎖中も送信stallが8回発生した。

## 計測と観測負荷

追加RAMは固定448B（active/snapshot各2区分、12項目のuint32回数/最大us、少数の時刻と状態）。ヒープ不使用、回数/最大値はUINT32_MAXで飽和。ホットパスは時計読取りと短いIRQ保護下での整数更新だけ。文字列化やUSB送信は行わない。E-ink通常版はPM_TRACE_STATS既定0のままで、追加RAM集計や出力をしない。

通常LCD版はPM_TRACE_ENABLE=1を保ち、PM_TRACE_VERBOSE=0で毎フレーム/毎送信のtiming行を抑える。エラー、ボタン、PM状態等のテキストは既存の有界リングを使う。PM_TRACE_ENABLE=0にして直接vprintf経路へ戻す変更はしない。

| metric | 範囲 |
| --- | --- |
| loop | 前景ループ全体、USB flushを含みsleepは除く。早期continueも回数/最大を記録 |
| loop_gap | 前回ループ開始から次回開始。sleep、間に走るIRQも含む |
| sdk_wait | net_poll/take/sendのSDKロック取得まで |
| net_poll | 公開net_poll全体、SDK待ち/解放も含む |
| rx_take | SDKロック取得、RX/appコピー/消費、解放まで |
| net_send | 公開必須/任意sendの試行全体。キュー投入と相手到達は異なる |
| process | 受け取った1アプリメッセージの処理。最終chunkのCRC等も含む |
| draw | LCDの画素変換とSPI全体。再接続等のローカル画面も含む |
| frame_rx | 有効FrameStartからCRC完了までの経過。ネット待ちも含む |
| flush | traceの1回flush全体。文字列化とUSB backend呼出しを含む |
| usb_write | backendのFIFO確認、write/flush、IRQ保護から戻るまで |
| rx_cb | アプリTCP受信callback全体。pbufコピー/解放/受信credit更新 |

既存slow_loopもflush後に計測するよう修正。従来net poll冒頭にあった無意味なslow_poll計測も公開pollの終わりへ移す。quiet版ではこれらの毎回のtiming行は出さず、RAMへ集計する。

## 採取方式

USB CDC/DTRの開放を検知した時だけ、active統計をIRQ保護下でsnapshotへコピーしてactiveをリセットする。snapshotは出力中に変化しない。集計後の出力処理が、その出力自身のsnapshotへ混ざらない。USB状態はMCUが検知した開放/閉鎖ごとに別bankへ記録。

出力はbegin、12項目×2bank、endの計26行。最大50msに1行で一度だけ出し、その後は再開放まで出さない。通常再生中の毎フレーム出力は増やさない。途中で閉じたdumpは打切りとなり次の開放でepochが更新される。hostは最新epochの24行とendが揃った場合だけ有効と判定する。

```text
stats begin epoch=2 start_us=... end_us=... metrics=12 banks=2
stats epoch=2 usb=off metric=flush calls=... max_us=...
stats epoch=2 usb=on metric=usb_write calls=... max_us=...
stats end epoch=2 trace_dropped=...
```

## 自動検証

作業コピーのホスト試験35ケースPASS。既存31ケースにRAM統計単体、統計有効の実main、統計有効の実net、quiet統計有効の実parser/動画/入力の4ケースを追加。模擬3秒のUSB処理をusb_write/flush/loopの最大へ保存し、USB状態別bank、immutable snapshot、paced output、再開放、USB満杯10,000回、64bit時計、count/max飽和、frame受信時間、quiet抑制を検証した。省電力の初回要求/読戻し、失敗、不一致、LCD/E-ink、trace無効も引き続きPASS。

ホスト採取parserもcomplete/latest/incompleteを自己試験済み。ホストmockは実際のTinyUSB/無線IRQの時間を証明しない。

実リポジトリでも35ケースPASS、git diff --check成功。LCD実configの通常版ビルド成功。UF2 730112B、SHA256 `92fd51e45967874c37f1c06af1d6c3e4d7380f35eaaedd43b8973619a0417dc9`、manifest OFF / example-lcd / 0123456789ABCDE0 / pico_wを照合。ARM ELFのstats symbolは0x1c0＝448B。PM_TRACE_ENABLE=1 / PM_TRACE_STATS=1 / PM_TRACE_VERBOSE=0を実コンパイル定義で確認。config.hとboard-id.txtはビルド前後SHA一致、lcd-push-v1とframe-bytes-v1も保持。下記の実機測定で書込み済み。直前の省電力修正版44565624もローカルに保全している。

## 実機での手順（実施済み）

1. ビルド報告を確認後、新しいLCD BOOTSELが必要。単一ボード0123456789ABCDE0と指定UF2のSHA/manifest OFF/pico_wを照合してから書込む。他端末へ書かない。
2. 起動後に同じ動画・6fps・選択のまま、3秒だけUSBを開いて最初のdumpとPM mode=0 none=1を確認する。初回がreadback=pendingなら接続成功後の初期snapshotから準備し直し、pendingを成功扱いしない。採取用capture_stats_snapshot.pyは3秒の有限期限、finallyでDTRを明示解除してFD closeする。baud変更や端末へのデータ送信はしない。
3. 180秒、USBシリアルを開かず同じ動画を維持。journalだけを読み取る。ユーザーはこの間、動画/fps/プラグインを変更しない。変更した場合は結果へ明記し、こちらから元へ戻さない。
4. もう一度3秒だけ開いて最新epochの全24行とendを取得して閉じる。usb=offは閉鎖中の計測、usb=onは前の短い観測期間の計測。GET状態と正確なDTR解除/再開放時刻も保存し、閉鎖中journalと対比する。

以前の採取はFD closeを記録したが、DTRの実機状態は記録していなかった。新しい採取器はDTRを明示解除し、新ファームもMCUが見た状態別に記録することで曖昧さを減らす。上記方式で実機採取を完了した。

## 測定できないもの・解釈の制限

- 値は割込み/待ち込みの実経過時間で、CPU専有時間ではない。範囲は重なるので足し算してCPU負荷にしない。
- USB状態の分類はmainが状態を確認して更新した後、計測完了時点で決まる。境界を跨ぐ処理やIRQは厳密には分離できない。frame_rxも完了した側に入る。
- 完了しない永久停止は最大値へ保存されず、dump自体を取れない場合もある。集計出力やSDK stderrの最大時間に新しい期限は設けない。
- Wi-Fiの電波/AP側再送、全TinyUSB IRQ、全SDK内部処理を個別には測らない。loop_gapや呼出し全体には含まれるが、原因の特定には追加根拠が必要。
- init前景外のPM設定、getter、TCP接続callback内の直接send等の全APIを個別計測するわけではない。省電力修正は既存の試験とcached読戻しで維持を確認する。
- 3秒で最新dumpが揃わなければ不完全と報告する。通信停止や欠落を成功扱いせず、観測時間やタイムアウトを勝手に延ばさない。

## 実機書込みと180秒測定結果（18:22 JST完了）

ユーザーから新しいLCD BOOTSEL連絡を受け、個体0123456789ABCDE0、730112B、SHA256 `92fd51e45967874c37f1c06af1d6c3e4d7380f35eaaedd43b8973619a0417dc9`を再照合。既存flash.shの対象確認、load、100% verify、通常起動すべて成功。通常USB列挙とサーバへの接続も確認した。省電力修正を維持し、初回・最終USB出力のcached読戻しはどちらも `sdk_result=0 raw=0x00000010 mode=0 none=1`。これは起動後の成功したgetter値であり、USB開放ごとにgetterを再実行した値ではない。

初期USB開放18:19:50.319〜18:19:53.321、DTR明示解除後の閉鎖18:19:53.321〜18:22:53.472（180.151秒）、最終開放18:22:53.472〜18:22:56.474。両方3秒で最新dump全24行とendを取得し、finallyのDTR解除・FD closeも成功。journalの取得範囲は正確な閉鎖期間に一致させた。すべて日本時間、2026-10-18。

4回のGET（初期/最終の各前後）で、LCD youtube選択、動画 `dFf4AgBNR1E`、変換key `9a8a64a8baa0b7e745d7dd41`、6fps、epoch=33を確認。全端末の選択・有効化・各revisionと共通並び順も4回一致。途中すべての瞬間を連続監視した証明ではないが、こちらから設定・サーバ・他端末を変更していない。

**改善していない。USB閉鎖中にstream_write_stallが8回、再helloが7回。** サーバは送信処理が断片2秒/フレーム全体5秒の期限までに完了しない場合、またはsink.sendがエラーを返した場合を同じstream_write_stallとしてwriterを終了し、その後reader/sessionが終了する。現行ログだけではこれら3条件の内訳を分離できない。独立したcontrol_write_failedは0回。session_end 8件はすべてtrigger=writer_ended/detail=stream_write_stallで、別の故障8件として二重計上しない。stall時刻は18:20:29.126、18:20:51.966、18:21:08.626、18:21:37.291、18:21:59.796、18:22:22.130、18:22:32.823、18:22:41.304。

閉鎖中にサーバframe_start 483件（すべてfps=6）、frame_sent 475件、stream_status 72件（すべてgeneration=33）。statusのrendered_framesから各session最初の未知区間を除いて得た増分は356。全描画数や実画面の正確なfpsを示す値ではなく、最終ACK前に切断した描画も含まれない。MCUのdraw 474回にはローカル再接続画面も含む。

| 項目 | USB閉鎖側 回数 | 最大ms | 初回USB約3秒側 回数 | 最大ms |
| --- | ---: | ---: | ---: | ---: |
| loop | 90986 | 65.292 | 1887 | 54.900 |
| loop_gap | 90986 | 66.304 | 1887 | 55.915 |
| sdk_wait | 173461 | 11.902 | 4053 | 0.011 |
| net_poll | 86785 | 13.785 | 2029 | 0.058 |
| rx_take | 86546 | 14.100 | 2021 | 3.639 |
| net_send | 130 | 0.896 | 3 | 0.708 |
| process | 3790 | 49.386 | 142 | 21.668 |
| draw | 474 | 33.367 | 18 | 19.532 |
| frame_rx | 469 | 4068.483 | 17 | 166.377 |
| flush | 90978 | 0.945 | 1887 | 0.227 |
| usb_write | 65390 | 0.942 | 67 | 0.150 |
| rx_cb | 9529 | 0.129 | 360 | 0.119 |

MCU集計epoch2の全範囲は183.148秒で、初期USB開放約3秒と閉鎖約180秒を状態別bankに分けている。最終dump出力自身の負荷はこのsnapshotへ入らない。USB境界の1ループ/IRQや境界を跨ぐ処理は完了側に分類され、閉鎖期間との厳密な端点一致には小さな差がある。

今回の完了済み処理では、USB出力最大0.942ms、flush 0.945ms、描画33.367ms、前景loop 65.292ms、loop間隔66.304ms。SDKロック待ち11.902ms、RX取出し14.100ms、TCP受信callback 0.129ms。それに対してFrameStart〜CRC完了には最大4.068483秒かかっている。前景ループが数秒停止したという根拠は得られず、受信データが揃うまでの経路に残る長い待ちを絞り込む必要がある。TinyUSB/SDK内部全IRQや無線/AP再送の内訳は測っていない。最大値だけの集計なので、4.068秒のフレームを8件のどのstallへ対応づけるかは特定できない。

USBを閉じてもstallが出るため、この計測版の悪化を「USBを読み続けたせい」で説明することはできない。一方、USB開放側は健康な約3秒だけで、閉鎖180秒と同等の比較試験ではない。初期USB側が良好だったことからUSBが改善させたとも結論できない。旧verbose版と今のquiet版は出力量が異なり、過去版でUSBが与えた影響も今回だけでは否定できない。

前回の省電力修正版でホスト読取りを閉じた180秒はstall 4回、今回は8回。改善と報告できない。ただし前回はDTR明示解除/MCU状態分類がなく、時間帯とファームの計測実装も異なるため、増加をPM修正や統計追加の因果と断定しない。統計更新自体の観測負荷もゼロではない。初回dumpのtrace_dropped=7、最終=9で、テキストリングの完全な履歴は残らない。journalを切断時系列の根拠とし、再開放で受信したテキストのホスト時刻を障害発生時刻とは扱わない。

原因は未確定。次の切り分けに必要なのは、同じframe_idについて、サーバ送信停滞、MCUへの受信到着/credit、アプリ取出しを対応づける根拠。今回タイムアウト延長、再試行条件、TCP/WS形式や省電力設定の変更はしていない。旧版復帰も行わない。

証跡（ローカル、UF2や秘密設定は共有しない）:

- `stats-fix/stats-flash-result.json`、`stats-flash-check.log`、`stats-flash.log`
- `stats-fix/run-20261018T091950Z/result.json`、`analysis.json`、`journal.jsonl`
- 同ディレクトリ `initial/`・`final/` のmetadata.json、stats.json、usb.jsonl
- 初期/最終USBはどちらも閉鎖済み。収集プロセス終了コード0。SSH、旧版転送、サービス操作、他端末操作、commit/pushなし。

サーバ稼働確認: `ActiveState=active SubState=running MainPID=907828 NRestarts=0`、起動時刻14:43:27 JST。測定前と同一PIDで稼働中。

## 受信4.068秒の内訳：現ソース・既存ログ・限定ssによる追加調査

### 計測区間の定義

`main.c`の有効FrameStart処理が`pm_start`で組立を開始し、stream情報の検証と`pm_video_prepare`等を済ませた直後に`pm_stats_frame_start`を呼ぶ。最後の断片を`pm_chunk`へ渡し、全25,600BのコピーとCRC32一致が確認された直後に`pm_stats_frame_done`を呼ぶ（`main.c:579`、`main.c:417`〜419、`core.c:36`、`trace.c:118`〜123）。

したがって4.068483秒は、TCP/WSから次の断片が揃うまでの待ち、アプリが断片を取り出すまでの待ち、断片処理と最終CRC、割込み等を含む実経過時間。最初のFrameStartメッセージがTCPへ到着するまでの時間、そのメッセージの大半のparse/検証、完了後のpm_video_offer/pacing/SPI描画は含まない。CPUを4秒使ったという意味ではない。完了した469フレームの最大で、断念した未完了フレームの時間は記録されない。frame_idや最大値の発生時刻も保存されていない。

### サーバ：生成、待ち、ソケット

| 段階 | ソースで確かめた範囲 | 今回の実測と限界 |
| --- | --- | --- |
| 生成 | playback.rsは16ms tickでsequenceを計算し、YouTubeのread_frameはspawn_blockingでJPEGファイル読取りとRGB565変換を行う。25,600Bが完成してからAppへpublishする。IO/変換はApp mutex外 | 生成開始/終了の時刻ログがないため処理時間を数値化できない。ただし同じフレームの生成はPicoのFrameStartより前であり、今回の4.068秒へ直接含まれない |
| App待ち | queue_lcd_videoは完成済み画素をwatchの最新1件へ置換。writerは転送中1件と最新1件のみ保持し、古い未開始フレームは置換する。フレーム単位ACK待ちはしない | 無限の動画キューはない。publish〜writer採用時刻はログがなく未計測。queue前の待ちはFrameStart前で、4.068秒に含まれない。後続断片のwriterスケジュール待ちは含まれ得る |
| エンコード/転送 | VideoTransfer採用後のstream_frame_start〜stream_frame_sentは、FrameStart＋4096B単位の7断片の生成、yield、sink.send完了を含む。画素は既に全部メモリ上にある | 完了475件、最小88us、中央値424us、95%点1250us、最大1,505,315us。これはソケット以降の到達/描画ACKではない |
| 停滞/期限 | 各断片のsink.sendを2秒とフレーム全体5秒の早い方で囲う。期限とsinkエラーを同じstream_write_stallにまとめる | 未完了8件すべてstart〜writer_exitが2,000.114〜2,038.160ms。断片2秒期限との整合性が高いが、ログが送信offset/timeout-vs-errorを区別しないため確定できない。5秒フレーム期限に達した証拠はない |
| OS以降 | WS書込み上限32KiB、Linux SO_SNDBUF要求32KiB（実skmem tb=65536）。sink.send完了後でも未ACK/未送信データは残る | App最新1件制限だけではOSキューの待ちを除けない。send完了をPicoへの到達と扱えない |

上記のソースは`service/src/playback.rs:24`、`youtube/media.rs:1184`、`application_jobs.rs:528`、`application.rs:1369`、`lcd_stream.rs:42`と67、`transport.rs:677`〜704、`host.rs:38`。数値は元の6fps・epoch33の180秒journalから算出。失敗frame_idは346605/346655/346673/346805/346890/346957/346988/347006。それぞれの断片offsetや4.068秒の最大フレームとの対応は不明。

同じ6fpsログには、frame_id=346513、rendered_frames=213のまま、Picoのelapsed_ms=42186/43187/44188という約1秒ごとのstatusがあり、3件がサーバへ18:20:14.916814 JSTにまとめて届いている。Picoがこの間もstatus生成を継続した根拠であり、主ループ全体がその数秒間ずっと停止した説明とは合わない。これは受信最大4.068秒のframe_idを特定する根拠ではない。初期segmentのframe_start情報やstatus到着にもネット待ちとkernel bufferingの影響がある。

### Pico：callback、pbuf、credit、バッファ

| 段階 | 確認した実装と実測 | まだ区別できないこと |
| --- | --- | --- |
| TCP callback間隔 | threadsafe_background版でlwIP/CYW43が背景IRQで動作。アプリrecv_cbは9,529回、1回本体の最大129us | 前回callbackから次回までのgap、受信byte数、フレームとの対応は保存していない。9,529回という総数から均等に届いたとは言えない。TCP順序待ちでcallbackが来ない場合と、driver/AP側で届かない場合を区別できない |
| pbuf解放 | recv_bodyは受信pbufをRX配列へコピーし、その場でpbuf_freeし、アプリ組立/描画まで持ち越さない。pbuf_freeも上記129usの範囲内 | pbuf_free単体時間・解放数・refcountは未記録。lwIPの順序待ちキュー等に残るpbufはこのアプリcallbackでは観測しない |
| callback前の割当 | ローカルSDK cyw43_lwip.c:275でPBUF_POOLから割当し、成功時にnetif->inputを呼ぶ。割当NULLならアプリcallbackもエラーログも出さず破棄する。PBUF_POOL_SIZE=24、MEM_SIZE=16000 | この無ログ経路が存在する事実と、今回実際に割当失敗したという事実は別。現ELFにlwip_stats(216B)はあるが、現在のUSB dumpは値を出さないため割当失敗は確認できない |
| tcp_recved | RX_CAP=32768、TCP_WND=16×1460=23360。append時は先頭9408B分まで早期creditし、それ以降の保留分はアプリconsume時に返す。consumeはWS bodyをapp_rxへ部分コピーした段階でも進む。tcp_recvedは窓更新が閾値5840B以上ならACK/tcp_outputし、それ未満は通常の送信時機を待つ | 呼出し回数/byte数、保留credit最大、rcv_wnd時系列は未記録。tcp_recvedを呼んだことと、ACK/窓更新がサーバへ到達したことは異なる |
| RX/appバッファ | TCPコピー配列32768B、app_rx18006B。overflow/異常はreceive_failedをログしsocket_deadにする。pm_net_takeはSDKロック内でWS/アプリbodyコピーとconsumeを行い、pixel組立/CRC/描画はロック外。今回take最大14.100ms、process最大49.386ms、lock待ち最大11.902ms | used/uncredited/app_usedの最大/停止時値はない。満杯が発生したか数値で確定できない。USBリングには欠落もあるためreceive_failedが見当たらないことだけでは否定できない |

TCP窓23360Bがフレーム25600Bより小さいことだけでは、この詰まりの証明にならない。現在のframe-bytes-v1は4096B単位の断片を途中から消費してcreditを返す。過去の「17KiB JSON断片が旧11680B窓を超えた」問題とは条件が異なる。受信フローのホスト試験も既存35ケースに含まれるが、実無線のpbuf枯渇やACK到達を証明しない。

### 追加ssの結果：6fps試験とは別条件

既存LCDのアドレス203.0.113.10373との対応はfirmware/verification-2026-10-12.md等の既存証跡に基づく。ssはサーバ16153番とこのpeerに限定し、約20.48秒・11回、既存TCP状態だけを読み取り、同期journalも保存。USBは開かず、全コマンドは終了済み。

**この追加観測時には利用者側の再生が変更されていた。** journalの全132件のstartは12fps。直後GETはMichael Jackson「Thriller」、video_id=`sOnqjkJTMaA`、変換key=`6f98153b479103e3ddfd1270`、epoch34。前の6fps・AKB48・epoch33の試験に混ぜない。LCDの選択youtube・有効化revision72等は維持されており、こちらから再生条件を変更/復元していない。

| JST時刻（18:29） | peer port/状態 | Send-Q B | notsent B | RTT ms | RTO ms / backoff | cwnd | 再送B累計 | サーバが最後に受けた広告窓B |
| --- | --- | ---: | ---: | --- | --- | ---: | ---: | ---: |
| 01.771 | 62000 ESTAB | 0 | 未表示 | 15.724 | 216 / 未表示 | 18 | 42347 | 23348 |
| 03.825 | 62000 ESTAB | 18003 | 未表示 | 16.645 | 217 / 未表示 | 18 | 42347 | 23360 |
| 05.881 | 62000 ESTAB | 47600 | 42000 | 16.524 | 868 / 2 | 4 | 44707 | 23360 |
| 07.930 | 62000 FIN-WAIT-1 | 42001 | 39201 | 16.524 | 1736 / 3 | 2 | 46107 | 23360 |
| 14.044 | 62001 ESTAB | 1316 | 未表示 | 13.133 | 214 / 未表示 | 26 | 未表示 | 23360 |
| 18.150 | 62001 ESTAB | 57400 | 42000 | 9.376 | 210 / 未表示 | 11 | 2068 | 20497 |
| 20.202 | 62001 ESTAB | 27955 | 22355 | 11.779 | 212 / 未表示 | 17 | 2068 | 6560 |

同じ観測journalでは18:29:07.170553にsession1071がstream_write_stall、その251ms後にsession_end、18:29:13.134010にsession1072がhello。ssの同じ旧port62000は05.881までESTAB、07.930にはFIN-WAIT-1で未送信データも残り、その後消失し、新62001が接続した。従って「サーバwriter停止時点で全データが即座に消えてPicoも直ちに検知する」とは扱えない。

旧portの再送Bは短い観測内で42,347→46,107へ増加し、RTO backoffとcwnd縮小、約42KiBの未送信滞留が同時に見える。少なくともこの別条件ではカーネルTCP以降の進捗低下が実在する。RTT推定16msだけを見て健全とは言えない。停止直前の最後の広告窓は23360Bで、常時zero-windowという説明にもならない。ただし広告窓は最後に到達したACKの情報であり、瞬時のPicoバッファ空きを直接読んだ値ではない。

同connectionのrwnd_limited累計も増えており、新62001では窓6560Bも見えたため、受信窓制約が一切無関係とも言えない。これは接続全体の累計で、各stallの原因時刻や実機RX配列満杯を直接証明しない。再送の増加はデータ/ACKの未確認を示すが、無線/AP、往路データ欠落、逆路ACK欠落、Pico driver/lwIP内での破棄・遅延のどれかまではssで決められない。2秒間隔の観測で、間の短いzero-windowも除外できない。

先に取得した1回のssにも別connection61997でSend-Q=60243、notsent=57443、RTO=1688ms/backoff3/cwnd2/広告窓23360があった。ただしその1回は現在のvideo/fpsを同時記録していないので、6fps条件の測定として扱わない。

### 最小の次の切り分け案（未実施）

既存データだけで4.068秒を「CPU処理何秒、受信待ち何秒」と分解することはできない。現在の最大値同士は異なるフレーム/異なる時刻かもしれず、引き算しない。

次に必要な最小単位は、遅い完了フレーム1件のframe_idと受信状況を固定RAMに残すこと。現在の448B集計へ、遅いframe_id/開始終了時刻、フレーム内のTCP callback最大gapと受信byte数、RX used/app_used/uncreditedの最大・終了値、tcp_recvedのcallback/consume別credit byte数、および既存lwip_statsのPBUF_POOL/heap割当失敗カウンタの前後差を追加する案。常時の文字列化/USB出力は増やさず、今回と同じDTR開放時の有限snapshotへ載せる。新たなSDK内packetログやtcpdumpは要らない。callback gapを待ち時間とし、各callback/CRC等の本体時間とは別に保存する。

その1件にサーバ既存frame_idのstart/sentと限定ssを対応させる。低いRX使用量のままcallback gapが伸び、再送も伸びるなら到着/順序待ち側を優先。RX/保留creditが高くtake/consumeが止まればアプリ消費/credit側を優先。PBUF_POOL.errが増えればcallback前の割当失敗が実際にあった根拠になる。増えないだけで無線以外の全経路が健全とはしない。

サーバだけでも、失敗時frame_id・断片offset・sink.sendのtimeout/実エラー・断片開始/完了を有界に記録すれば、今の共通stream_write_stallを分けられる。ただしそれだけではPico callback間隔/バッファ/pbuf失敗を知れず、4.068秒の最大フレームもまだ特定できない。いずれも今は提案のみ。コード変更、追加ビルド/書込み、サーバ再起動、タイムアウト延長は行っていない。追加ファーム計測を行うなら、新しい明示指示とBOOTSELが必要。

追加証跡: `stats-fix/tcp-initial-ss.json`、`tcp-target-22s.json`、`tcp-target-analysis.json`、`tcp-target-22s-journal.jsonl`、`post-ss-state.json`、元180秒フォルダの`send-breakdown.json`。稼働確認は同一PID907828 / NRestarts0 / active-running。
