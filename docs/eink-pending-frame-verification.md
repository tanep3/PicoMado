# D-04 描画中の最新pending：実装と検証

先行契約は`83a7ec6`と補足`eccd285`の[eink-pending-frame-contract.md](eink-pending-frame-contract.md)。D02の固定版`a6c7cf4`、D03Aの`53cb612`を保持して実装した。

## 実装

純粋な`eink_pending.h`はEmpty/Receiving/Readyとtoken/更新metadataだけを保持する（64B以下をstatic assert）。画素は既存assemblyに置く。主loopがdriverへ渡すときに既存queued bufferへコピーされるため、古い物理描画中もassemblyを最新の受信へ再利用できる。追加のフル画素bufferは作っていない。

有効な新Startだけが古いpendingを置換する。寸法・format・byte length・CRCの整数幅を副作用なしに検査し、不正headerでready/受信途中の画像を破壊しない。CRC完了まで描画しない。旧描画の結果は旧drawing tokenで処理してから、現在scopeの最新readyを一度だけdispatchする。局所画面、再接続、待機、selection待ちでpendingを失効させる。Receipt ACK、API_PENDING、物理完了とRendered ACKを区別し、既描画tokenの再送は再描画しない。

driverのqueued/active trace contextも分けた。局所描画中にremoteをqueueしても、古いSPI factに新frame IDを付けない。Clock更新のinterval/minuteもpending画像に束縛し、後続ordinary画像へ引き継がない。plugin名による分岐は追加していない。

## ローカル試験

| 検証 | 結果・証拠 |
| --- | --- |
| 既存LCD/E-ink・ネットワーク・描画回帰 | `d04-host-trace-final.log`：host script全体成功、PASS行175。純粋pendingモデル、実mainの描画中latest、旧/native chunks、部分受信、不正header/CRC、旧失敗、metadata、重複、局所/切断再試行/再接続/待機を含む。 |
| driverの所有・trace | 0/2ボタンE-inkでLOCALのSPI begin/endは0、後続REMOTEは9901のまま。caller画素を書換えてもdriverのコピーから正しい画像を出力。上記scriptへ登録。 |
| Address + Undefined sanitizer | `d04-sanitizers/results.json`：5/5成功。純粋モデルと0/2ボタンの実main・実driver。Leak検査は対象外。 |
| API結果 | `d04-api-results.log`：LCD/0/2ボタンの実handlerでREJECTED/PENDING/COMPLETED/ERROR、offset/u64/stale/CRC/描画・送信失敗の既存script成功。 |
| 観測・共通契約 | `d04-observation-contracts.log`：EV1 ON/OFF、境界catalog、共有USB parser、部分書込み、u64飽和、metadataの既存script成功。 |
| ARM | dummy sample LCD/0/2ボタンE-inkの変更前後、計6build成功。`d04-arm-baseline`、`d04-arm-changed`にELF/map/.suを保存。観測ON E-ink_1も成功し`d04-arm-observed`へ保存。 |

## 初回実装（034fb2d）のRAM・stack

`d04-arm-comparison.json`はmapとRAM addressに属するsectionを集計した。初期化dataも含め、heap/stack予約は別計上する。Berkeley形式のsize出力だけを静的RAMと見なしていない。

| dummy構成 | 静的RAM 前→後 | 増分 | Flash増分 |
| --- | ---: | ---: | ---: |
| LCD sample-a | 233416→233416B | 0B | 72B |
| E-ink_0（0ボタン） | 163900→163960B | 60B | 880B |
| E-ink_1（2ボタン） | 164000→164060B | 60B | 1104B |

全構成のstack予約は4096B、最小heap予約は2048B。GCC `.su`の自己frameでは、追加の`dispatch_pending_eink`が128B。E-ink_1のmainは480→496B、api_frameは184→256B、receive_chunkは136→168B、driver pollは72B不変。E-ink_0はmain 408→416B、api_frame 184→248B。LCDのmain 552B/api_frame 216B/receive_chunk 128Bは不変。観測ONのE-ink_1静的RAMは164556BでARM linkも成功した。

これはcompilerの関数別frameとlink配置の確認である。割込みとの重なりを含む実機のstack高水位は測っていない。物理パネルの残像・ちらつき・RF/USB/間欠遅延も未検証。

## 反映

D04はE-ink firmwareの変更なので、採用時には対象端末用firmwareの更新が必要。wire/APIとLCDの有効な受信規則は保つ。今回のUF2はdummy sampleであり実機へ書き込まない。実機反映は独立レビュー後、既存の端末設定・board IDでbuildして識別を確認し、別途承認された書込みと物理受入を行う。

本番source/DB/設定、実USB、SSH、service再起動、install、push、共有SDKへの書込みは行っていない。SDK既存差分を保持し、cyw43_ll.cの基準SHA-256 `a4e6baa503b9332a0a445c8b22deb4bee62793c441dc74a200b0fe8065c397c7`と一致した。D03Bの期限整理・公開後ファイル作用は別の未完了項目として残る。

## 観測ONの実E-ink経路の追加確認

`eink_pending_observation_test.c`で実main/APIと実EV1 encoderを同時に動かし、decoderでframe/operation/resultを照合した。0/2ボタンの双方で、frame101の物理描画中に受信したframe202はAPI_PENDINGのままで、旧101の完了時に202を完了扱いにしない。無効header303はREJECTEDで202を破壊せず、202のdispatchは旧完了後、実完了とRendered ACKは次の完了後にだけ発生する。APIのReceived/Pending operation IDも同じで、観測dropは0。

`d04-observed-pending-contracts.log`の通常実行と`d04-observed-pending-sanitized.log`のAddress/Undefined実行が成功した。既存の`run_contract_tests.sh`へ追加しており、既存観測契約も同時に回帰している。この追加確認でfirmware本体は変更していない。

## 独立レビューの描画中再送修正

契約`dcbb78a`に従い、同じtokenを元の描画中に再受信してReadyになった場合を追加した。dispatch直前のlast_rendered照合でpendingを消費してACKへ合流する。元の描画が失敗した場合はCRC済み再送を一度試行できる。既知の描画中/既描画tokenの再送は、別tokenの新しいpartial/Readyを破壊しない。

`d04-replay-host.log`は全体成功（先頭PASS行175、本文の新規0/2ボタンcaseも成功）。`d04-replay-contracts.log`と`d04-replay-contracts-sanitized.log`では実main+EV1をdecodeし、frame900に別々のAPI operationが2組、物理dispatch/completionは各1回、ACK1回、drop0を確認。API3構成は`d04-replay-api-results.log`、model/main/driverのASAN+UBSAN5/5は`d04-replay-sanitizers/results.json`。Leak検査は行っていない。

再修正後のdummy ARM3構成は`d04-arm-replay`、section集計は`d04-replay-memory.json`。静的RAMはLCD233416B、E-ink_0 163960B、E-ink_1 164060Bで、最初の基準との差は0/+60/+60B。stack予約4096Bは不変。dispatch_pending_einkの自己frameは128→192B、api_frameはE-ink0/1とも248B（修正前のE-ink1は256B）、receive_chunkは168B、mainは416/496B。これらはcompilerが出した関数ごとのframeであり、実機高水位や割込みを含む最大同時stackの証明ではない。

native FrameBytes受信の機能・CRC・世代は実mainで回帰しているが、既存native入口には独立したAPI operation factがない。API operation/frameの同時照合はJSON API経路についての証拠であり、native入口まで新設した証拠ではない。実パネルと間欠遅延は今回も未検証。

観測ONのE-ink_1 ARMも再build成功し、ELF/.suを`d04-replay-arm-observed`へ保存した。通常OFFへ戻した再buildも成功（`d04-replay-arm-observed.log`）。修正後のFlash増分は元基準比LCD+72B、E-ink0+1192B、E-ink1+1424B。
