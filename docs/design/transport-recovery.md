# 動画送信と正常FINの有限な回復処理

この候補は未反映。新しい実機採取・SSH・サービス操作・ファーム書き込みは行わず、隔離ホスト試験と既存SDKオブジェクトを用いたビルドで検証する。既存PM無効化、EEX、wire形式、端末選択、有効化、共通並び順は変更しない。

## サーバの進捗と期限

進捗は**同じ接続のAsyncWriteが正の書き込みバイト数を返したこと**。TcpStreamを包むTrackedIoでpoll_write/poll_write_vectoredを計数し、ConnectInfoでWebSocketのwriterへ渡す。カーネルへの受付であり、TCP ACK、AP到達、端末受信、描画の確認ではない。Pending・0B・read・flushだけでは進捗にならない。同じ接続の自動pong等も含む接続単位の値であり、特定の画像だけの到達計ではない。

| 制限 | 候補値 | 意味と根拠 |
| --- | --- | --- |
| 無進捗 | 3秒 | 正の書き込みを25ms刻みで観測して更新。観測した約1.79秒の停滞回復に余裕を持たせる初期値。3秒以上受信できたという意味ではない |
| 動画1画像全体 | 10秒 | 既存5秒から延長するが、微小な進捗で永久に延長しない。設定・応答の時間も含む。実機で最適化済みの値ではない |
| 静止画全体 | 既存のwire量由来、最大20秒 | JSON/nativeの送信量の違いを維持。断片内の無進捗だけ3秒・バイト進捗へ変更 |
| 制御応答到着後の進行中メッセージ | 最大250ms | メッセージ境界まで完了できれば応答を送る。完了できなければ接続全体を閉じる。途中のWSへ制御応答を差し込まない |
| 制御応答自体・終了Close | 各既存250ms | 従来の上限を維持 |

期限種別は待機時に選んだ期限で分類し、遅いexecutor起床で別の期限種別に置き換えない。これらは協調実行の期限でありOSスケジュールや既存同期処理の停止を強制中断するリアルタイム保証ではない。

writerは追加でpending commandを最大1個だけ保持し、既存32個のchannelを無制限にdrainしない。active画像1枚とwatch内latest1枚、WS buffer、SO_SNDBUF、Appのキャッシュ/置換規則を維持する。新しいsessionを25ms刻みの確認で検出した場合も旧送信を終える。制御応答を優先する代わりに、極端に遅い接続は再接続になる。入力による状態変更が既に適用されていても、応答自体を届けられる保証はしない。

本番host入口はTrackedListenerを使用する。独自にrouterを直接serveする埋め込みでConnectInfoがない場合、断片内のバイト進捗は取得できず、無進捗3秒と全体上限で打ち切る保守的な動作になる。

## Picoの正常EOF

正常FINはREADYからDRAININGへ進め、同じ接続でコピー済みのRX/app/WS状態を保持する。hard TCP error、overflow、protocol failure、明示的なローカルcloseは救済対象にしない。正常WebSocket Closeも有限救済の対象とする。旧PCBのrecvを防御的に拒否し、close/beginでは従来どおりSDK lock内でcallback登録を解除・バッファを初期化する。

DRAININGでも既存の1ループ8メッセージ（動画開始前は2）の予算で処理し、物理入力・ネット処理の機会を維持する。take=falseでもpingを消費した場合は処理が進んでいるため、ただちに終了しない。RX/app/残りWS長が変化しないdecode passでDRAINEDへ進む。EOF後なので不足したheader/bodyを待ち続けない。不完全画像やCRC不一致は表示しない。

有効な完成画像は既存latest1枚へ置換し、既存pacingで表示してからretryする。最大1fpsの周期1秒と通常のdecode/SPIに余裕を与えるため、**FINから最大1.5秒**で打ち切る。重複FINやparser進捗でこの全体上限を更新しない。上限を超えた画像は未表示でも破棄する。正常EOF後はownerを直ちに無効化し、物理pixelsの保持を独立した状態で引き継ぐ。HOMEのTCP/WS準備、AWAYのEEX探索・target接続、Wi-Fi再join、接続/handshakeのtimeout、welcomeのWAIT IMAGEを越えて、次のCRC・token検証済み画像の描画成功まで前のpixelsを残す。保留中に新しいEOFで有効画像がなかった場合や、EOF処理の1.5秒上限に達した場合も、前の表示済みpixelsは保持する。期限とtokenを延命しているわけではなく、各試行は有限で、古いフレームを再描画することもない。物理画像の表示年齢を制限する仕様ではない。

正常Closeは未マスク・FIN=1・RSV=0・payload125B以下の制御フレームを検証し、空payload、またはstatus1000/1001と正しいUTF-8 reasonだけを受け入れる。Close直前までに完成した有効latest画像だけを残し、途中app/WS・Close後の入力は破棄する。新しいアプリデータも送らず、Close応答をqueueへ置ける場合だけマスクしたCloseを返す。sndbufの空きだけではsegment queue/heapの余裕は保証されないため、Close専用送信はsndbuf不足またはtcp_writeのERR_MEMで応答を一度だけ省略し、有限救済を続ける。再試行・空き待ちはしない。tcp_outputのERR_MEMも救済を維持するが、write/outputのその他のエラーと既にラッチされたhard errorはFAILEDとし、救済状態へ戻さない。Close後のpayloadは捨てるが、同じcallbackに付く受信エラーは捨てずに直ちにFAILEDとする。これは受付でありACK/到達確認ではない。最初に正常Closeをdecodeした時点（応答write/outputの前）、またはFINを受けた時点を1.5秒上限に使い、重複Close/FINでは更新しない。CloseだけでFINが来なくても無限待ちしない。不正・断片化・truncated Close、エラー/未対応status、hard error後は救済しない。EOF時にframe境界へ残ったCloseの先頭1バイト（opcode8）も不完全headerとして拒否する。未完のbinary body内にある0x88はClose headerと取り違えない。受信容量超過もcallback時点でFAILEDにし、次のpollを待たずmainの既存FAILED判定で描画を中止する。

保持を終了するのは次の有効画像の描画成功、SUB/UP/DOWN/POWER等の有効なローカル操作、menu/standby、確立済み接続のhard TCP/protocol error。接続前のrefused/timeoutは通常の復旧試行として保持する。接続準備中の無効なUP/DOWNなどは保持を解除しない。Webの選択更新では新しい有効画像を待ち、旧session/ownerを再び有効にしない。

standby・明示的切替・新session/ownerのtoken検査を緩めない。EOFをdrainしながら新しい接続を同じ共有バッファへ開始しない。E-inkのmainはEOFで従来どおりretryし、LCDだけが画像を救済する。SDKのcallback取消し・割込み実装そのものはホストmock試験では証明できない。

## 検証と制限

transport_recovery.rsは正常、3.2秒の部分write進捗、無進捗3秒、進捗継続中の全体10秒、制御の完了/打切り、session交代を同じwriterで比較する。socket_progressの試験は0/Pending/flushが進捗にならないことと、実際のloopback WebSocket upgradeまで同じcounterが渡ることを確認する。

fin_recovery_test.cの52ケースは本番net/mainを同時に使い、20,433B（未返却credit11,025B）、app内の完全メッセージ、部分WS、CRC、ready画像、take直後FIN、hard error前後、再接続、1fps pacing、重複FINと1.5秒上限、latest置換、standby、ping、decoder失敗を検証する。Close＋FIN・Close単独・最終chunk直後のClose・不正Close・Close後hard error・後続payload拒否、実mainのretry→handshake timeout、welcomeから次の有効画像、ユーザー操作による保持終了も検証する。app_endpoint_test.cではHOME/AWAYの状態表示抑制とPMの継続を確認する。run_host_tests.shへ組み込む。

追加の境界監査は次の組合せを診断ON/OFF/ASan・UBSanのそれぞれで実行する。全CコンパイルはWall/Wextra/Werror。LSanはexecutorの制約で無効。

| 対象 | 通り数・境界数 | 確認内容 |
| --- | ---: | --- |
| Close header/prefix | 12,304 | FIN/RSV/mask/7-bit length全4096組合せ×FIN前後/なしと1バイトprefix16種 |
| Close分割/EOF | 275 | payload0/2/125Bの全分割位置、途中FIN、binary内0x88 |
| Close/FIN/送信/エラー | 6,804 | sndbuf 0/5/6/7/8/9/65535、write/output OK/ERR_MEM/hard(-14)、受信errorの前後、エラーcallback latch |
| RX capacity/credit | 32 | RX_CAP直前/一致/超過、既存usedとの足算、Close後の大きなpayloadとhard error |
| App/WS/JSON領域 | 24 | APP_CAPの最小/最大/超過、出力NUL1B、最大32758BのWS、拡張lengthと不完全body |
| Close status/UTF-8 | 65,558 | 全65536 status、UTF-8上下端・overlong・surrogate・範囲外・部分列 |
| Close応答時間/期限 | 36 | write/outputの模擬経過0/1499999/1500000usとERR_MEM、FIN先行。最初の期限を保持 |

case37/38は独立レビューの追加再現、case39/40は実mainでClose出力hard error・Close後の受信hard errorが物理描画0回になる回帰試験。case47は応答呼出しの時間が期限を延長しないことを確認する。これら5件は修正前の候補でも失敗を再現した。エラーcallbackを送信mock内から発生させる検査はラッチ保持の防御試験であり、実SDKの割込み順序を再現したものではない。

これらは確認できたソフト側の切断・破棄の扱いを直す候補であり、最初のネットワーク停滞の原因を確定したものではない。12fpsへの回復、AP/RF/CYW43/lwIP/ACK戻り経路の健全性、実機RAMの最大使用量は未検証。定型画像との比較、追加の観測、診断OFFの実機比較はこの変更へ混ぜない。

## 反映計画（未実行）

1. 候補diffと試験結果を独立レビューし、現在の本番との差分・artifact hashを照合する。
2. 承認後に既存サーバ実行ファイルを保全し、候補を設置して1回再起動する。端末の選択・有効化・共通順序には触れない。サーバ単独でも新しい期限処理は有効だが、EOF救済にはLCDファーム更新も必要。
3. LCDのBOOTSELと唯一のflash ID 0123456789ABCDE0を確認し、承認済み候補UF2を既存guard付き手順で1回書き込み・検証する。E-inkのファーム更新は不要。
4. 通常の接続・表示・PM readback・EEXを確認する。性能改善は別途承認された同条件の通常→定型→通常比較で評価する。新しい実機packet captureは今回の計画に含めない。
5. 問題が出たら、保全した直前のサーバとUF2を使って対で復旧する。既存設定をrollbackしない。復旧も実行前に親からの指示を確認する。

既存OFFプロセス群終了試験は独立比較で旧新版とも200回中1回同じassert失敗となり、この変更前からの問題と確認された。対象外としてコードを変更しない。以前のOFF一括217/218と単独再試験PASSという記録を保持し、OFF全件一括成功とは表現しない。


## main全体の期限・保持チェック（TR5/TR6）

全状態遷移・poll/draw点・保持の不変条件は[main-loop-recovery.md](main-loop-recovery.md)に記録した。transportのEOF判定をSDK lock内の軽いcheckpointで再使用し、mainのすべてのtransport FAILED復旧を同じhelperで分類する。take=falseでClose応答に時間を使った場合、メッセージ処理後のpollで期限切れになった場合、静止画receipt_ackまたは動画rendered_ackが遅れた場合も、同じEOF_TIMEOUT→retry_eofを使う。動画/静止画とも描画開始直前に再確認し、1500000usちょうどから新規drawを始めない。既に始めたSPIは協調実行として完了させ、その画像を保持して再接続する。通常再生へ新たなSDK pollは追加しない。

独立の実main再現2件と、追加284シナリオを含む52ケースをON/OFF/ASan・UBSanで全件検証する。前記47ケース内のtransport行列も維持する。画像保存用の追加領域を設けず、元のlatest1枚と物理pixels保持だけを使う。Wi-Fi喪失とEEX network changeによる先行resetも同じ分類へ通し、EEX側の3経路を統合試験で確認する。
