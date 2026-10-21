# 動画負荷下の共通観測対策契約（設計のみ）

[実測](lcd-sim-recapture-20261021.md)のsource lossは既存モデルの予測外で、静止画1画像でも発生する。以下を実装前の契約とし、追加flashはまだ行わない。

## 保つ事実と動作

品質基準・catalog/coverage・causal_completeを緩和しない。特定plugin名へ分岐しない。アプリwire、COPY bool、ACK/描画/動画policy、AP2 hook元関数1回転送、設定・選択・有効化・共通順序を保つ。

input受付/dispatch、通知COPY/TCP範囲、tcp_output、同範囲linkoutput初回/再送/結果、制御API受付、frame開始/CRC/受信完了/失敗/取消、描画開始/完了/失敗/取消、source開始/終了/欠測を保持。無入力時の未知エラーも共通境界から追えること。counter更新でEV1を再帰生成しない。

## 識別できる範囲

offer/loss、保存、FIFO zero/partial、drain budget停止までは確定。SDK解放間の瞬間offer、format/CRC単独時間、USB write単独時間、FIFO zero連続spanは未計測で、原因を1要素へ固定しない。max_drainをUSB write時間と扱わない。kernel観測を安易に追加せず、現最小構成の初回read以前はunknown。

## 優先する比較

1. 実負荷モデルを先に拡張。518facts/s、16件閾値、19ms描画、zero/partial、SDK解放までのburst、control返信を別変数とする。保存分布から欠測種類を推定せず、実測と仮定をfixtureへ明記。12fps nominal合格を実機保証にしない。
2. 必要事実を失わない転送最適化を先に評価。観測専用binary framing/CRCや複数recordのbounded出力を候補とする。166B→約84Bでも必要約43.5KB/sなので、単独で足りるとは限らない。collector互換・CRC・部分入力・旧catalog拒否・固定RAM・control公平性を先に検証する。
3. burst容量は固定RAMとstack予算で評価。LCD static251064B、linker heap可能11080B、stack4096Bに、新buffer/IRQ重なりを加えたmapと実使用上限が必要。単なるqueue拡大は継続offer>drainを解消しない。control領域を奪わない。
4. 反復RX/chunkを集約するならbyte range、最初/最後の実source timestamp、件数、失敗/不連続/ambiguity/overflowを有界に保持。個別到着を知ったように装わず、frame/input/未知error境界を削らない。失う識別能力を明記し独立レビューする。詳細を捨てて完全coverageを主張する方式は禁止。

## 最小追加counterが必要な場合

SDK解放間の最大offer、高水位、format max、USB write max、FIFO zero連続span maxを要求時pageへ追加する候補。固定サイズ・非再帰・非待機で、各時間の割込み包含を定義。counter追加・encoding/queue変更はLCD firmware変更が必要で、書込み済み候補へserver設定だけでは追加不能。

追加flash前に理由・対象LCD1台・修正範囲・SHA・直前復旧UF2・個体binding/設定・資源・host回帰・必要BOOTSEL操作を親へ報告する。現在は追加flash未実施。

## 検証と終了条件

- nominal、実測約1.96倍offer、burst、zero/partial/recovery、SDK占有、control返信、無入力排出の独立/混合試験。CPU/byte/write上限とcontrol予約を検査。
- 容量内は全必須事実/seq/CRC/timestamp/cause/frameを保存。容量外はunknown/incompleteでfail closed。source lossを0に見せない。
- 固定RAM/stack map、観測OFF build、既存27collector試験、ON/OFF atomic IRQ、catalog/parser一致、TX同期hook/再送/1回転送、既存host回帰を通す。
- 実機でstatic ready、初回画像、時計→動画、動画→static、無入力動画継続。遅い例・速い対照・試行数とSIM/GPIO差を記録。全区間と正のprefixを区別し、欠測はcanonical incompleteのまま。
- server変更は隔離build/test後、承認済み範囲で整合退避・設定維持・復帰検証。TCP再送の原因未特定でNagle/timeout/OS/network設定を変更しない。
