# 実機観測で残った境界と欠測の修正前契約

状態は**実装前の契約案**。runtime／firmwareの変更、追加実機操作、採取、再起動を許可する文書ではない。[既存設計](contracts-observation-design.md)、[因果契約](observation-causality-contract.md)、[収集器契約](observation-collector-contract.md)の純粋Rules・状態遷移・既存wire／ACKと観測の分離を維持する。

## 実測からの出発点

2026-10-20のLCD手押し4切替を [実測照合](manual-switch-analysis-20261020.md) に記す。時計→YouTubeは入力受付→初回描画1,578.827ms、通知送出受理→制御応答受付1,478.404ms。サーバAPI受付後の返信完了は39.925ms以内、初回frame送信完了は66.022ms以内と、同一server Sourceの事実とwriter内順序で上限を置ける。

端末sourceの欠測増分885は、現在の `pm_observe` が32枠の共有queueの16枠閾値で予約を拒否した数と一致する。ホスト保存のqueue／write欠落は0。4回の入力→初回描画の正の事実は連続seqで残るが、採取全体と正規解析はincompleteのまま保持する。

遅延の根因修正はまだ選定しない。以下は、欠測対策と不足境界を実装する場合の契約である。

## R-01: 送出結果の意味を保持する

- 既存 `pm_net_send` のboolは、validな送信状態・serializationと `tcp_write(COPY)` 成功に依存する。`tcp_output`の戻り値は現在診断用で、domainの成功条件にはしない。新しい観測のためにfalse／再送／切断へ変えない。
- 同期通知処理の開始・終了、TCP COPY受理byte-range、output戻り値、unsent／unacked／snd_nxt／lastack／cwnd／snd_wndを固定値として区別する。netif／CYW43／bus／RF／TCP ACKを同一の「送信成功」にまとめない。
- cause→connection generation＋TCP byte-rangeを有界に対応付け、後続のlinkoutputで一意に一致したときだけ結ぶ。IRQにmainの最新causeを流用しない。再接続、wrap、曖昧な重複、capacity超過はunknown。
- 相関表はCOPY成功後にpublishし、**tcp_outputを呼ぶ前に**参照可能にする。同じ呼出し内の同期linkoutputでも、後日の再送でも同じ範囲を照合できること。COPY失敗時に新しい範囲をpublishしない。既存SDK lockの所有を守り、hookから再取得しない。
- 既存AP2／TX／CYのARM・DTR・run所有は維持する。継続EV1が旧detached試験を暗黙armしたり、旧controllerと2つのUSB ownerになったりしない。RF／SDK内部の詳細記録が必要なら、その所有契約を別途設計する。
- AP2の既存linkoutput hookと新観測は1つの転送経路で共存し、hookの付替え・解除の所有を変えず、元関数へ同じpacketを必ず1回だけ渡す。観測失敗や相関unknownでも転送回数・戻り値を変えない。

## R-02: 受信／返信の不足境界を記録する

- serverのraw read／WS完了、decode開始／終了、現API Received、返信enqueue開始／完了、writer dequeue、sink開始／完了を同じserver Source時計で区別する。`Frame Pending`の静止画App登録点と動画writer準備点も区別し、どちらもTCP送信完了としない。
- raw受信にcauseがまだなければconnection＋byte-range等のopaqueキーで持ち、後のdecodeと一意に結ぶ。payloadや資格情報を記録しない。入力1件を近い時刻のpacketへ割り当てない。
- raw read／WS完了はkernelへの到着時刻ではない。**最初の実観測点以前はunknown**とし、kernel到着後の未読待ちと到着前の待ちはこの最小構成だけでは分離できない。fixtureだけが知る到着時刻を実機の識別能力へ転用しない。この不足を理由にkernel観測を暗黙追加しない。
- Pico側もRX callback受領、mainでのtake／decode、API受付を同じPico時計で区別する。対向との時計同期を前提とせず、各endpoint内の待ちを測る。
- 返信／画像の通常処理とwriter順序は変えない。今回の4操作は返信先着だったが、FrameStart先着・遅れた同世代応答・混在fragmentも正しい既存経路として保持する。
- 現在server coverageは0xebでNetwork境界を含まない。追加事実に合わせcatalog／coverage／schemaを明示し、旧sourceを新しい網羅性へ格上げしない。

## R-03: queue欠測対策は有界・非待機で行う

- hot producerは固定値・固定RAM・一定の有限処理。整形、USB／file IO、sleep、wait、malloc、SDK再入を持ち込まない。full時もdomain処理を止めず、seqと欠測を可視化する。
- control返信用の予約と1行のpartial write所有を維持し、EV1／旧AP2／SIM／通常ログの出力を行内で混ぜない。欠測counterをリセットして採取を完全に見せない。
- drop reason、queue高水位、producer試行数、drainのrecord／byte数、partial／zero-write、loop内の排出時間を有限counterで示す。新counter自体の欠測・飽和も明示する。
- counterを更新するたびにEV1を生成しない。counterは固定値を更新し、明示要求や有限のsnapshotだけで取り出す。排出を計測したことが新しい排出対象を作る再帰を避け、入力・通信・通常log・要求が止まればqueueが排出し切れること。
- cold drainの回数・byte・時間budget、呼出し位置、またはqueue配分を変更するなら、対応する上限と根拠を先に提示する。無条件にflushを増やしたり、空くまでdrainしたりしない。受信budget、入力poll、既存描画／ACK・動画policyを観測都合で変えない。
- このLCD全診断版のリンカ上heap可能領域は11,712B。queueを増やす案はRAM差・map／stackと実行時余裕の受入計画を先に出す。過去sampleの値や静的余裕を実heap高水位へ読み替えない。
- 正常な12fps動画＋通常切替を、既存の生産点を維持したまま検証する。無期限切断・無期限ゼロUSB容量で無欠測を保証しない。観測を間引くなら別schema／coverageとして示し、従来の完全性を名乗らない。

## R-04: 採取開始と品質を分離する

- DTR立上りの既存統計snapshotは、同じexclusive ownerで読み終えてから採取区間を始める。snapshotは私有preludeとして保全する。開始待ちを、待機前に作った古いnowからの返信期限へ算入しない。
- 前の変更コマンドがuncertainなら、独立nonceのSTATUSで実状態を照合する。必要な正規OFFを確定してから別の新しい区間として開始する。結果不明のON／SIM操作を同じ要求として再送しない。
- 利用者へ開始を知らせる条件は、ON応答だけでなく、対応boot／seqの実begin fact、catalog／coverage、CRC、保存durable prefixが揃うこと。既存statusのreadyだけで因果採取の準備完了と宣言しない。
- 生涯dropの非ゼロbaselineは保存し、区間内増分と区別する。global complete=falseと、連続した局所範囲の正の測定値は別々に示す。境界を捏造した部分ファイルでglobal completeをtrueにしない。
- 正規stop→OFF→末尾watermark／保存worker／両summaryを確認する。CLI終了やstate=stoppingだけを保存確定としない。単一owner、有限上限、再接続回数、既存SIM COMMIT拒否を維持する。

## R-05: 時計と推論の扱い

Pico／server Sourceのtimestampを直接減算しない。確定の所要時間は同一boot／source内、または関数内の既存durationから得る。program orderで得た上限は点推定へ変換しない。

Pi上のUSB受領 `host_mono_ns` とserver SystemTimeを対応させる場合も、同じhostでのclock pairing、ms丸め、offset範囲、clock stepの仮定を保存する。USB受領はPicoのevent生成より後なので、正の残時間は下限である。USB転送・queue遅延を片道ネットワーク時間としない。

## 実装を始める前の区切り

1. 欠測対策と不足境界を別の変更単位にし、上の契約・資源差・検証ケースをレビュー可能にする。
2. [検証条件](observation-runtime-gap-verification.md)を満たすローカルfixtureと互換根拠を整える。現時点ではruntime修正・その新テストは未実施。
3. 実機負荷、動的heap／stack、RF、物理表示を確認する操作は別の範囲として扱う。本契約をその承認に流用しない。

未知の根因を確定した扱いにせず、既存の選択・有効化・順序・個別設定・HOME/AWAY・flagsを変更しない。
