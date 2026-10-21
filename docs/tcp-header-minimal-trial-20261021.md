# EV1 OFF・TCPヘッダ最小試験（2026-10-21）

現行firmware/serverを変えず計6操作。全て単一USB owner、EV1 OFF、RELEASE応答後15秒以上は次入力なし。5件はSIM要求→server decoded約45–69ms、1件は雨データ→風刺画で1416.368–1436.961ms。範囲はARM RPCによる同hostのsource origin校正幅を含む。物理GPIO入口は試していない。

遅い例ではUSB PRESS応答27.768ms、RELEASE応答198.493ms。4ボタンの成功dispatchはRELEASEを処理する前に発生する現行state machineで、step dispatched/released=1,cancelled=0。初回の対象TCP payloadはPi kernel timestampで要求後1436.271–1436.275msに観測され、73B/SEQ[41774,41847)。PiからACK41847が70.337µs後に出た。

対象socketのRecvQは要求後1353.293msまでの約100ms標本で0、TCP_INFO.bytes_receivedは14285のまま。1457.457ms標本で14358へ73B増加。server RawPoll→RawReadyは15µs、WS/decodeも短い。この事例の主な待ちはPi userspaceのApp処理や既受信bytesをreaderが長く放置した区間ではなく、Pi NIC/カーネル観測点より前にある。NIC timestampは無線到着やTCP受理そのものではなく、Pico内部のTCP/driver/FW、RF/AP/経路、Pi NIC/driverのどこかは残る。今回EV1 OFFでは初回Pico linkoutputを直接採っていないため、1.4秒をTCP再送タイマと確定しない。前回の再送証拠とは別試行。

既存observerのBPFは正確なLCD4-tupleをカーネルで照合しIHL+TCP固定20Bだけをcopy。payload/pcapや他通信を保存せず、interface/promiscuous/権限設定を変更していない。全6件header kernel drop0・scope valid・socket cookie継続・sample gap200ms以内。server probeは全件overwrite/contended0でdigest保存後releaseしIDLE。EV1 OFF、USB owner終了、同じ通常server PIDを確認。

初回2操作の後、可変latest進捗fileをexclusive evidence保存関数で更新して停止した。2操作の証拠は全て保存・probe復帰済みと照合し、再送せず別rootで残りを開始。次rootでは遅い1件と速い対照が揃った4件で正規終了。権限拒否・ツール不足はなく、再起動/flash/SSH/E-ink/ai-news変更なし。

原証拠はこのSUMMARY.jsonの各root、host.json、headers.jsonl、recvq.jsonl、observer-result.jsonとSHA。RT↔monotonicのoffsetは観測範囲幅4.241µsに収まったが、未観測clock stepを排除したとは言わない。2つのsource時計の絶対値を直接引いていない。
