# SDK整合後LCD実機比較（2026-10-21）

SDK 2.3.1の公式gitlinkに揃えたLCDでも、風刺画→雨データの切替で入力から描画完了まで1,496,540µsの遅延を記録した。通知TCPデータはPiに到着していたが、それより前のsequence gapが再送で埋まるまで累積ACKが1,344,373.226µs進まなかった。SDK整合化だけでは間欠遅延の解消を確認できなかった。先行データの元の送信は採取開始前で、欠落の場所・原因は未確定。

## 書込み・現在状態

[比較候補の記録](sdk-alignment-results-20261021.md)に続き、独立レビュー `/tmp/picomado-sdk-alignment-review-3p1g7x5k/review-outcome.json` の受入れとユーザーのBOOTSEL報告を受けて実施した。証拠rootは `SDK-live-20261021T040833Z`（以下R）、共通親dirは `/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020`。

- 対象LCDのFlash ID E661640843239D21、個別設定、binding、復旧UF2、候補SHAを再照合した。既存flash.shによるload/verify/rebootは1回だけ。2026-10-21 04:09:24 UTCにverify 100%、04:09:40 UTCにUSB再認識とframe1021493の受信・描画ACKを確認。
- 現在のUF2は `11a11556e6e5c71b2f90fb98aeabca15132aabb5914b27d6ccebd217d7a5c485`。設定sectorを書かず、容量対策WIPを混ぜていない。
- 通常サーバPID1780574、binary SHA `e7b52c29d776bf9b9fbfea9125216c7b5c05613f47fadc9660618dedb76c727c` を維持した。サービス更新・再起動0、E-ink・ai-news・OS/ネットワーク変更0、SSH・pushなし。
- 最終Pico AP2状態0/host probe idle、EV1 OFF、単一USB owner終了・FD閉鎖を確認。新SDKのLCDファームを維持している。

証拠はR/flash-complete.json、flash-reconnect.json、inner-cleanup.json。直前LCDの復旧先は `gap-p2-build/artifacts/PicoMado-lcd1/PicoMado-lcd1.uf2`、SHA `fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3a`。さらに前のf0c6a15aとは区別する。SDK自体の復旧は先行比較契約のSDK-before.tar/旧HEAD一覧に従い、DB/configを巻き戻さない。今回復旧操作は不要だった。

## 比較条件と停止

EV1 OFF、既存AP2/CYW43診断、対象LCDのTCP 4-tuple限定ヘッダ観測を使用した。単一TIOCEXCL USB FDを保持し、SIMはUP 150msを1回。観測中35.6秒はUSB通信せず、次入力を入れなかった。物理ボタンの入口・debounceはこのSIM試験に含まれず、共通イベント経路以降を確認した。

計画は最大8操作だったが、最初の風刺画→雨データでCYW43容量8行に対しTX9件、over=1となった。保存・digest検査・release/IDLEを確定し、既存品質条件に従って次のSIMを中止した。時計→YouTube、同じ新SDKの速い対照、発生率・分布は未取得。短時間正常や命令の受付を描画成功とは扱わない。

初期提案は診断品質失敗後の継続を許していたため、自動承認レビューが実行前に拒否した。USB owner未開始を確認し、元の全品質停止条件を戻した実装だけを実行した。R/auto-review-correction.jsonとrejected-comparison-proposal.txtに記録した。1件のsyntax errorもUSB開始前で、構文確認後に訂正済み。拒否案や構文失敗によるSIM/書込みは0。

## 因果IDと区間

run `18e0701e35d08e10`、Pico boot `5ae570e360f434a5`、session2299、SIM job1、Pico connection1/host connection431、入力message15/kind15/fingerprint2177506005、描画frame1021500で対応した。以下のrecords添字は0始まり。

| 区間 | 実測 | 時計・証拠 |
| --- | ---: | --- |
| SIM共通入力→送出関数return | 1,812µs | Pico、pico.json records[0]→[9] |
| 入力→frame受信開始 | 1,422,294µs | Pico、[0]→[20] |
| frame開始→CRC完了 | 53,291µs | Pico、[20]→[21] |
| 描画開始→完了 | 19,248µs | Pico、[31]→[32] |
| 入力→描画完了 | 1,496,540µs | Pico、[0]→[32] |
| raw poll→ready | 14µs | host、host.json [2]→[3] |
| 入力decode→処理完了 | 62,119µs | host、[18]→[20] |
| 入力decode→応答queue完了 | 62,124µs | host、[18]→[22] |
| Pi通知到着→先行gap再送到着 | 1,344,373.226µs | 同一kernel realtime、headers.jsonl 1→5行 |
| gap再送到着→累積ACK進行 | 49.241µs | 同一kernel realtime、5→6行 |

Pico、host probe、Pi packetの時計を単純減算していない。host raw readは368Bで、先行heartbeat2件と入力のdecodeが続く。個々のWS messageへのraw byte provenanceは記録されていないため、raw read完了と入力message境界を同一視しない。reply queue完了はネットワーク送信・無線送信完了ではない。

## TCP順序待ちの観測

以下の相対時刻は、PiがIPID46を観測した時刻を0とする同一kernel realtime内の差。SEQは半開区間。

| 時刻ms | 事実 | headers.jsonl行 |
| ---: | --- | ---: |
| 0 | 通知IPID46、SEQ[8262,8333)到着 | 1 |
| 0.042074 | Pi累積ACK8064、通知より前のデータを要求 | 2 |
| 1336.660117 | 後続IPID47、SEQ[8333,8432)到着 | 3 |
| 1336.763598 | 依然ACK8064 | 4 |
| 1344.373226 | IPID48、SEQ[8064,8163)の再送到着 | 5 |
| 1344.422467 | 累積ACK8432へ進行 | 6 |
| 1412.787022 | Pi応答データ181B | 7 |
| 1413.960948 | 後続frameデータ開始 | 8 |

通知のCOPY位置はPico records[7]の[8262,8333)。TX index0とCY row0とPi IPID46が対応する。Piには通知が届いても、先行TCP holeが埋まるまでアプリへ順番どおり渡せない状態だったことをACK推移が裏づける。1.344秒をサーバアプリが読み取り可能な通知を放置した時間とは解釈しない。先行[8163,8262)を含む採取前の到着状況と、[8064,8163)の初回送信・欠落位置はunknown。

描画後にも同じ形の穴がある。受信ACK message17相当の初回TX index3/IPID61/SEQ[8432,8615)はPi観測にない。描画ACK message18相当のIPID62/SEQ[8615,8799)は+1497.882795msに到着したが、PiはACK8432を返した。IPID63の先行再送が+2754.959082msに届くとACK8799へ進行した（31～36行）。IPID62まで欠落したとはしない。描画完了そのものはPico records[32]で確認する。

## CYW43・品質限界

Pico AP2 70件/overwrite0、host AP2 63件/overwrite0/contention0、TX9件/drop0/malformed0/generation変化0。ヘッダ40件・queue sample355件、kernel drop0、最大sample間隔116.728ms、payload保存0、scope全件一致、observer worker/reaper正常終了。SIM受付/dispatch/release各1、cancel0、物理入力counter不変、USB観測overlap0。

CYW43は8件を保存、over=1、inc/changed/sat/depth/rejectは0。TX index0～7は各1件のCY rowに対応し、index8（入力+17,756,104µs）のCY rowがない。これは描画完了後だが、全20秒区間の完全性を回復させる理由にはならない。既存collectorの全8分類はincompleteのまま保持した。

保存済み通知IPID46とgap再送IPID48では、CY entry/prebus key一致、FULL/BUS/DONE、covered=domain、entry/prebus CRC一致、credit待ち/poll0、bus戻り値0を観測した。通知のCRCは3701199909、coverage129B。これらは保存行の局所事実であり、「全TXでdriver正常」「RF送信完了」「チップ/無線/APで欠落なし」は保証しない。nrtx4/5はPCB共有counterであり、その通知自身の再送回数ではない。採取前の先行packetにはこれらのCY証拠もない。

## 比較の結論と残り

旧混在SDKのAP2/CY静止画5試行は入力→描画140.427～170.673ms（中央値162.221ms）、新整合SDKは今回1試行1496.540ms。同時刻・同素材・対になった条件ではないため、悪化率やSDK原因を推定しない。旧版ヘッダ限定試験にも約1.416～1.437秒の遅い例があった。今回確定したのは整合SDKでも再発したことと、今回の支配的待ちが先行TCP gapに対応したこと。

未観測は初回先行packetのdriver以前/チップ/RF/802.11 ACK/AP/Piまでの経路、最終TXのCY内部、新SDKの速い対照・分布。次の採取で先行packetを含めるには現在のCY8行上限を含む品質契約を先に解決する必要がある。容量WIPをこの比較へ混ぜたり、別runを同時因果証拠に合成したり、品質基準を緩めて追加操作したりしていない。今回の計画内の実機操作は終了し、追加flash/service操作は行わない。

R/analyze.pyによりdigest・因果ID・相関・欠測・正規解放等21照合成功。R/analysis/results.json、validation.json、evidence-sha256.jsonに再現可能な数値・原証拠SHAを保存した。ホストsuite/ARM buildの結果は先行候補記録から変更なし。今回の文書追加に合わせた再buildは不要。

[機械可読checkpoint](checkpoints/20261021/sdk-aligned-live.json)。
