# 2026-10-21 LCD USB SIM再採取

## 結果と本番状態

LCDのみUF2 fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3aを1回書込み、verify100%、個体binding・設定・同個体USB/HTTP再接続を確認。E-ink firmware、ai-news 3 unit、OS/network、SSH、push操作なし。

最終採取は observation-recapture-20261021T014716Z。EV1 OFFの同一USB ownerで静止画を確認し、server切替・全端末初回描画を区間外に置いた。その後、新ONのcatalog3 begin、CRC、durable prefixとcausal_ready=trueを保存してから、同じowner IPCのconnected SIM PRESS/RELEASEで5操作を行った。物理GPIOは通らずbutton state machine/common dispatch以降を試した。SIM受付だけで描画成功としない。

正規owner stop、OFF応答/end、保存worker終了、両summary、観測server正常終了を確認。通常server e7b52c29d776bf9b9fbfea9125216c7b5c05613f47fadc9660618dedb76c727cへ復帰し、3端末online・received/renderedを確認。元unit/config/flagsを維持、終了直前の永続状態との差分0、DB巻戻しなし。許可されたSIMによるLCD選択は最終時計。LCDは新firmware、EV1 OFF。直前UF2 f0c6a15a...を私有復旧先に保持。

## 準備是正と中間失敗

- DTR統計snapshotが約6.4秒続き、固定collectorの2秒quiet上限ではSTATUS/ON前に終了した。固定受入済みcoreを保存し、今回用copyだけ上限10秒/512KiBへ変更。quiet150ms・行境界・nonce・品質/境界FSMは不変。旧warm wrapper未使用。実copy SHA40673ecac3267bd85c2e65dfd8bee800004b7ecaccbc24b821b3867363b3866a。既存27試験と、2.3秒snapshot・SIM/EV1混在・単一owner排他・禁止COMMIT/EV1転送拒否・正規保存のPTY試験成功。
- 動画ONで開始直後に欠測。正規OFFし、追加許可に従い観測OFFで静止画へ移動。この移動は計測外。
- 静止画beginは成立したが、その後のserver再接続で35件欠測。通常復帰後、最終試行では新ONを再接続完了後へ移した。
- 中間試行で前回DB snapshotを新退避先へ持ち越し、exclusive createに阻止された。上書きせず通常復帰。継承物をunit・旧binary・LCD復旧UF2へ限定した。ゲート待ちownerのPID消滅・serial FD所有者0を確認してから次へ進んだ。
- 全試行は別rootで保持。flash1回のみ。中間失敗のためservice停止/起動は合計5組（観測切替/通常復帰2試行各2組、退避衝突の通常起動1組）。不確かな結果のまま再送していない。

## 品質と区間表

最終device seq6134–8169はbegin後からendを含む期待2035件、749受信、source loss増分1286、host queue/write loss0、storage durable。全区間causal_complete=false。server catalog3/coverage0xefはsource/保存とも完全。canonical解析は5件すべてincompleteのまま。

| SIM job / cause op | 観測した選択 | Pico入力→通知受理 | server API→返信sink完了 | 保持された事実 |
| --- | --- | ---: | ---: | --- |
| 2 / 6135 | 風刺画→雨データ | 1.994ms | 49.441ms | TCP再送1回、描画完了欠測 |
| 3 / 6203 | 雨データ→次へ（8秒時点は雨のまま） | 1.962ms | 69.428ms | 同TCP範囲の再送、詳細は下記 |
| 4 / 6218 | 雨データ表示→時計（前要求と近接処理） | 2.039ms | 52.938ms | 前要求の未ACK残存、frame1021238受付は入力2168.093ms後 |
| 5 / 6342 | 時計→YouTube | 2.001ms | 34.152ms | 通知受理→制御応答45.776ms、frame1021244開始まで443.134ms |
| 6 / 7681 | YouTube→時計 | 欠測 | 67.448ms | SIM dispatch/releaseとserver commit、描画unknown |

Pico区間は同じPico boot、server区間は同じserver Instant内の差。別時計の絶対値を引かない。frame1021244は最初の動画frameとは限らず、開始が実保持されたframe。全5件SIM step accepted/dispatched/released=1、cancelled=0。総描画遅延を欠測から補わない。

欠測前の最大連続source prefixはop6135:6135–6164、op6203:6203–6254、op6218:6218–6254、op6342:6342–6366、op7681:7681のみ。正の証拠範囲であり、操作/全区間の完全性の代替ではない。

## 遅い通知のTCP証拠

op6203はCOPY成功後TCP範囲[20176,20322)を公開し、同範囲のlinkoutputとOKを保持。最初の呼出しは入力1.456ms後、完了1.799ms後、nrtx=0。同copyをparentとする完了を入力1178.268ms後(nrtx=1)、4178.326ms後(nrtx=2)、10178.150ms後(nrtx=3)、10189.394ms後(nrtx=4)にも保持した。初回から約1.176、4.177、10.176秒の再送で、全てgap-free prefix内。nrtxは接続PCBのcounterであり、操作ごとの再送回数ではない。同一copy/TCP範囲の反復からこの通知の再送を確認している。

8220.151ms後のop6218では、新COPY範囲[20322,20468)、送信前snapshot snd_nxt=20322,lastack=20176,cwnd=1400,snd_wnd=64076。前通知146byteの累積ACKが進んでいないことが確定。linkoutput OKは無線送信・相手到達・ACK到達の証明ではない。

serverのop6203/6218 API受付間隔は69.346ms。Pico入力間隔8220.151msより大幅に短い。各時計内の間隔の比較で共通offsetを使わない。API前WS decodeは12/15µs、API後の返信は数十msで、長い部分をApp永続化だけで説明できない。

raw read/WS消費の正確な対応とkernel到着時刻はunknown。RF往路、AP/経路、相手kernel、ACK復路、Pico受信処理の場所は確定しない。ACK未観測からserver kernel未到着と断定しない。観測ONの摂動・SIMとGPIOとの差が残り、旧1.478秒事例との根因一致も未確定。

## 4136件欠測の収支とモデル差

動画開始失敗root observation-recapture-20261021T013754Zは11.667521秒でoffer6048、保存1912、source loss4136（68.386%）、host loss0。非飽和pm_observeの唯一のloss条件は共有queueの受入閾値16への到達。最初のseq21–30欠測は開始51.291–59.078msの間。

平均offer518.362件/s、保存163.874件/s。EV1固定166Bで必要86048B/sに対し保存27203B/s。nominalモデルは22facts/frame×12fps=264件/s、43824B/s、常に64B room/3µs write。実offerは約1.96倍で、実USBのzero/partial・callback burstを再現していない。保存内訳はRX callback918、binary chunk take684など。欠測種類の分布は不明で、保存内訳をoffer内訳へ外挿しない。

OFF後の累積metricsはhighwater16、drain3019、write8975、bytes331862、partial4610、zero2224、budget_stops720、max_drain18686µs。writeはOFF後通常trace/snapshotも含むため11.667秒で割って正確な動画USB速度としない。max_drainは割込み/先取りも含み、個別writeが18.686msブロックした証明ではない。

最終試験の別page snapshot差はoffer+2034、drop+1286、drain+6102、write+3917、bytes+140747、partial+1989、zero+930、budget_stops+268。生涯highwater17、max_drain18832µs。4pageは同時snapshotではない。FIFO空きなしと予算打切りの存在、producer/drain不均衡は確定。host read間隔、USB IRQ、SDK/描画占有、format/CRC負荷の時間別責任は現counterでは分離不能。

## 証拠・対策

私有最終rootのanalysis/{trial-intervals,slow-control-retransmissions,metrics-delta,evidence-sha256}.json、operation別JSON、原JSONL/summary、SIM受付、backup、通常復帰証拠を保持。秘密設定/生ログはGitへ入れない。[対策契約](observation-video-pressure-contract.md)は設計のみで、追加firmware/server runtime修正は未実施。必要server更新は承認済みだが、未特定原因に対する挙動変更はしない。
