# 実機観測不足への検証条件（実装前）

[契約案](observation-runtime-gap-contract.md)の受入計画。ここに列挙する新しいruntime試験はまだ実行していない。既存採取の解析確認と、今後の修正検証を混同しない。

| 条件 | 必須の検証 |
| --- | --- |
| V-01 送信の意味 | 実`send_raw`／pm_net_sendのfixtureでtcp_write成功＋tcp_output ERR_MEM／ERR_IF、window保留、実linkoutputなしを作る。従来bool／retry／wireは不変で、新観測だけが結果を区別する |
| V-02 TCP相関 | COPY成功後かつoutput呼出し前に相関表が見えることを、output中の同期linkoutputで検証。1入力→複数segment、複数入力同居、後日の再送、wrap、再接続、別flow、map容量超過を扱い、一意でなければunknown。AP2 hookと共存時も元関数への同一packet転送が1回・戻り値不変で、私有LLの出自と元SDK不変も照合 |
| V-03 受信境界 | read済みだがdecode前、decode後input lock、reply capacity、writer dequeue、sink待ちを別々に注入して正しい段を測る。到着前待ちとkernel到着後未読待ちは、同じ最初のread観測を作れば共にpre-first-observation unknownのままとする。fixtureの到着oracleは解析入力から外し、実機で識別可能と判定しない。causeのない境界へ後続causeを流用しない |
| V-04 返信／画像 | 返信先着・FrameStart先着・画像組立中の同世代応答・旧token／旧session・初回動画置換を回帰。queue／フレームのbusiness順序やACKを観測で変更しない |
| V-05 drain負荷 | 実producer／serializer／consumerで12fpsのnative frame、最大8messageのburst、同期LCD描画、入力・log・control返信の混在を再現。byte／time／call budget、高水位、loss、入力poll、net／draw進行を確認。counter更新だけではEV1／queueを増やさず、無入力・通信停止後にqueueとpartialが有限回で空になり、追加flushでも新記録が湧かないことを検証 |
| V-06 障害 | USB不在、zero room、1byte partial、storage slow/error、full、counter飽和、切断、停止中のproducerを注入。待たない、行を混ぜない、欠測を隠さない。通常機能の進行と結果をbaseline比較 |
| V-07 cold owner | DTR統計burst、開始fact予約拒否、port準備が返信期限より長い、前のON応答遅着／不明、非ゼロの過去drop、begin欠落を試す。全て新規PTY／fake portだけで実USBを使わない |
| V-08 完全性 | global incompleteを維持したまま局所の正の測定を示す。欠測を必要事実の不発証拠にしない。生ログ＋summary＋durable prefixとSHA、実boundaryのprovenanceを検査 |
| V-09 時計 | Pico/serverの原点を大きくずらすfixtureで結果不変。Pi clock pairingの誤差幅・ms丸め・clock stepを注入し、下限を点推定にしない。未対応clockはunknown |
| V-10 資源と互換 | 同じLCD個別設定・既存AP2/SIM/TX/CY/STREAM/EEX1のON構成と、観測OFF構成をARM build。map／.su／flash／RAM差を出し、USB／AP2 export／旧SIM／TCP／cause／server全feature回帰を確認 |

V-05は平均帯域だけで合格にしない。1loop当たりのproducer burstと排出位置を再現し、queue拡大だけで長時間安定を宣言しない。実IRQ、USB負荷、動的heap／stack、RF、物理パネルは別の実機受入であり、host fixtureの数値を転用しない。

今回完了しているのは、保存ログの全4切替のcause/frame照合、同一sourceの区間和、journal同一stream順序、server上限、Pi clock pairingの条件付き下限、source欠番885とdrop増分885／host保存欠落0の一致の確認である。[実測照合](manual-switch-analysis-20261020.md)とcheckpoint JSONを参照する。

設計コミット `d70dd84` 後の実装・ローカル試験と識別限界は [実装検証](observation-runtime-gap-implementation.md) に追記する。上記「未実施」は設計先行時点の記録であり、実機受入は引き続き別工程。
