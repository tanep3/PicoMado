# 通知TCP再送の追跡と追加ファームなしの最小試験（2026-10-21）

本追跡は保存済み証拠と現行コードだけを使用した。追加USB操作、再採取、サービス操作、firmware更新、packet capture、容量対策実装はしていない。前回の[全体報告](lcd-sim-recapture-20261021.md)と同じログで、global device incomplete/server completeという品質判定を維持する。

## 同一通知の5回のlinkoutput

Pico boot12871963686048374921、input op6203→notify6205→COPY6206、TCP範囲[20176,20322)の146B。app session2296、EV1接続operation6138。親operationと範囲の両方が一致し、全eventはsource seq6203–6254の欠測0 prefix内。次表は同一Pico時計の差。

| 呼出しseq→完了seq | 初回呼出しから | 関数所要 | 戻り値 | PCB nrtx |
| --- | ---: | ---: | ---: | ---: |
| 6209→6210 | 0 | 343µs | ERR_OK=0 | 0 |
| 6214→6215 | 1176.465ms | 347µs | ERR_OK=0 | 1 |
| 6216→6217 | 4176.533ms | 337µs | ERR_OK=0 | 2 |
| 6231→6232 | 10176.356ms | 338µs | ERR_OK=0 | 3 |
| 6237→6238 | 10187.704ms | 234µs | ERR_OK=0 | 4 |

1.176/4.177/10.176秒に加えて、さらに約11.35ms後にも同範囲を呼び出している。nrtxはPCB共有counterで、operationごとの回数ではない。同一rangeの実反復が再送の根拠で、counter単独から推定していない。1.176秒、その3秒後、その6秒後という間隔はRTO系に整合するが、RTO/fast retransmitの理由を直接記録していない。11ms後の追加分も原因を決めつけない。

初回COPYは入力1159µs後、最初のlinkoutputは1456µs後、通知受理1962µs後。初回tcp_output後seq6211はresult0/flags13（PCB有、unsent無、unacked有、NODELAY有）。net.cのhookは元関数を同一pbufで1回転送し、戻り値後はpbufを再読しない。この5回の各関数内に秒単位の待ちを観測した事例ではない。ERR_OKはdriver呼出し受理を示し、RF送信・相手到達を保証しない。

## ACKが分かる位置と分からない位置

次input op6218は前inputから8220.151ms後。そのCOPY後/出力前seq6223でsnd_nxt20322/lastack20176、新COPY範囲[20322,20468)、cwnd1400/snd_wnd64076を保持。前通知146Bを越える累積ACKがその時点でPCBへ受理・反映されていない。新ACKを受理する現行lwIPのtcp_receiveはlastackをACK番号へ進めnrtxを0へ戻す（tcp_in.c:1234–1258）。同接続の後のseq6347ではsnd_nxt=lastack=21120へ進んだが、進行時刻そのものは未記録。

現行firmwareのrecv_cb/observe_rxはアプリpayloadのpbufを扱う。ACK専用パケットの到着・lwIP ACK検査・受理を直接記録しない。netif linkoutput parserもpayload長0を除外する。従って「ACKパケットが一切戻らない」とは言わない。失効/重複/不正なACK、driver/FW内滞留、lwIPへ配送されないACK、戻り経路で失われたACKは今回分離できない。

前inputから最初のpayload callback(seq6239)までは10264.546ms。seq6239–6254は16回のpayload callbackが13.830ms内に並ぶ。この直後にsource欠測が始まり、制御応答やframe/drawのfactが失われた。SDK callback群の間に排出できないburst仮説を支持するが、各drain時刻/SDK critical区間は未記録なので単一SDK pollで全16件が発生したとまでは断定しない。payloadがない間にもACK専用受信はあり得る。

## CYW43診断との照合

ready.jsonはAP2/TX/CYW43_INNER compile ON。ただし今回の監査済みserial commandにAP2 ARM、TX1、CY1はなく、CY1/TX1 exportもない。新boot初期のAP2はIDLEで、CYW43 probe.activeはAP2 RECORDING・run/世代一致を条件にする。compile ONだけで有効なWAIT/CRC/bus記録があると扱わない。

USB接続中のAP2 ARMはPREPAREDに入り、未開始runはUSB切断までARMEDにならない。connected SIM +持続EV1 ownerと現在のAP2/CYW43採取gateは両立しない。既存のdetached COMMITや第2readerを抜け道として使わない。

保存されたwifi_pm行はrequest/readback result0、raw0x10/mode0/none1。ただし接続時に取得したcached値の繰返し表示であり、全試行のRF正常性やPM値の連続検査ではない。

既存のCYW43共有buffer/credit待ちの条件付きhostモデルは、今回の実機でのWAIT/CRC変化を証明しない。短いlinkoutputでも短いcredit-pollの内部変化は排除できず、今回をそのモデルの再現と断定しない。

## server raw readとの対応と限界

cause op6203のApi385→decode383→WS382は親で結べ、accept-order接続21。op6218のApi410→decode408→WS407も同接続。同じserver時計の表：

| server seq | 境界 | source us | byte範囲（各層固有） |
| --- | --- | ---: | --- |
| 363 | RawRead完了 | 39399929 | raw1379–1478 |
| 381 | 次RawRead完了 | 55729009 | raw1478–1869、391B |
| 382 | op6203のWS完了 | 55729012 | WS1248–1386、138B |
| 384/385 | decode完了/API受付 | 55729023/55729024 | op6203 |
| 407 | op6218のWS完了 | 55798354 | WS1386–1524、138B |
| 409/410 | decode完了/API受付 | 55798369/55798370 | op6218 |
| 431 | 次WS完了 | 55849984 | WS1524–1617、93B |
| 445 | 次RawRead完了 | 55894326 | raw1869–2053、184B |

op6203のWS→APIは12µs、op6218は16µs（decode call内は11/14µs）。API間隔69.346ms、返信sink完了はAPI後69.428/52.938ms。両API間に同接続の新RawRead完了factはない。first readからfirst APIまでの順序差15µsは観測事実だが、raw381の391Bへ特定message全byteを正確に結ぶ消費provenanceはない。raw byteとWS payload byteのoffset領域をそのまま比較しない。

Picoでは再送時に146B、146B、99BのTCP payload範囲を出力し、合計391Bがraw381のsizeと一致する。WSも138B、138B、93Bと続く。興味ある収支だが、payload/transport消費の直接対応を保存していないので、サイズ一致だけで到着TCP segmentやraw→WSのexact joinとしない。

RawReadはTrackedIo::poll_readのReady(Ok)後のuserspace完了で、kernel/NIC到着ではない。今回のserver ACK-probeはARMしておらずRawPoll/Pending/Wakeの詳細も未保存。server初回read以前はunknown。Picoとserverの時計絶対値は引かず、各source内の間隔を比較するに留める。

## 仮説別の判定

| 仮説 | 観測済み | 未観測/結論 |
| --- | --- | --- |
| 初回COPY/output前のアプリ待ち、Nagleでunsent停留 | 入力→linkoutput1.456ms、NODELAY有/unsent無 | 当該初回の秒単位待ちの説明には合わない |
| 同じlinkoutput関数内で長時間待つ | 5回とも234–347µs、OK | 当該呼出し内の秒単位待ちは観測なし |
| TCP未ACK・再送 | 同range反復、次入力でもlastack未進行 | 確定。ただし原因箇所は未確定 |
| Pico driver/FW/RF/AP→Pi往路で欠落/滞留 | linkoutput後にも未ACK | Pi NIC到着なし、CY1記録なし。候補として残る |
| Pi ACK復路/Pico driver/lwIP受信処理 | lastack未進行 | ACK専用の到着/受理を未観測。候補として残る |
| Pi kernel既着・reader未poll | RawRead完了後は速い | kernel到着とRawPollを未観測。往復ACK不達と併存もあり得る |
| server decode/App永続化だけで数秒待つ | decode/返信はµs/数十ms | API以降だけで長さを説明できない |
| 観測/USB制御の摂動 | EV1 source drop、SIM GET233回/約30秒 | 影響の大きさ不明。4,136件欠測試行はSIM0でも発生 |

## 追加firmwareなしの最小の次試験（提案のみ）

### 1試行の構成

現行LCD/通常serverを維持し、EV1 OFFで単一ownerのconnected SIMを1操作だけ送る。現在のserver ACK-probeはcompile/config済みで、IDLE中も対象sessionを登録し、ローカルIPC ARMに再接続/再起動を要しない。親が次試験へ進むと決めた後、まずstatusでtarget/boot/現session/非残留runを確認する。未保存runは勝手にreleaseしない。

Piの対象物理NICで、検証したLCD↔Pi:16153の正確な4-tupleに限定してTCP header metadata（時刻、方向、SEQ/end、ACK、flags/window/length）のみを追加取得する。既存tcpdumpが利用可能。payload/raw pcap、他端末、HTTP全体を保存せず、promiscuous/OS/network設定を変更しない。これは既存観測では足りないNIC境界を1点追加する提案であり、今回未実行。権限・drop/statistics・checksum/offload・重複の条件を記録し、NIC観測を無線/RFやTCP受理そのものと扱わない。

server側だけACK-probe ARM→同owner SIM PRESS/RELEASE→最長15秒は次入力を送らず待つ→freeze→全page/digest/overwritten/contended保存→保存SHA検査付きrelease。SIM受付/dispatch/releaseを確認し、GETは解放確認と終了確認に限定する。前回の233回GETを再現条件の一部として隠さず、最初は低頻度で行う。開始前後のphysical button counterも読める範囲で照合し、混入はunknownとする。

1試行で目的の遅延が出なければ、保存済みrunごとに新runで反復し、遅い1件と速い対照、試行数/方向/操作間隔を残す。未再現を修正成功としない。途中cap/欠測ならその試行をunknown/incompleteで保存して停止。

### 何を切り分けられるか

- Pi NICに初回dataが見えず後続SEQだけ見えるなら、少なくともそのNIC観測前の区間が残る（capture dropも検査）。Picoで初回送信した正の証拠はEV1 OFFではないため、必要なら同じ静止画1操作だけEV1 ONの対照を別採取し、COPY/linkoutput prefixを照合する。動画観測全体の完全性を要求しない代わりに、欠測を成功へ読み替えない。
- dataを早く観測し累積ACKを早くPiから出しても、Picoで再送が続けばACK復路/Pico受理側を疑う。Pi送出ACKはPico到達を保証しない。
- dataは早いがPiのACKがない場合、TCP検査/受理以前とcapture条件が残る。headerだけでchecksumやTCP受理を証明しない。
- NIC受信が早くRawPollが長く発生しなければserver reader未pollを支持。RawPending/Readyがあっても、最初のkernel到着をそれだけで復元しない。
- NIC時刻とAP2のprocess相対monotonic時計は直接差を取らない。ARM RPC前後のhost monotonicとarmed_usでsource offsetを有界化し、NIC timestamp clockとの対応を採取開始/終了/中間で検査する。未観測clock step等を排除できなければ時間差は条件付き/unknown。TCP SEQ/ACK対応と各source内の時間だけでも判定可能な範囲を優先する。

これで不足する場合だけ、USBなしAP2/CYW43 run（物理操作または別途認められた方式）を次段として検討する。現在の禁止されたdetached SIMやFW gate変更へ勝手に進まない。容量対策の独立レビュー待ち実装には入らない。
