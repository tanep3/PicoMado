# 境界事実と入力→描画の因果（第2-B単位）

[既存設計第2・5・8節](../history/system-design-draft.md) と [C-08](../contracts-observation-design.md) の具体化である。ルール・遷移・表示内容・ACK方針は変更せず、観測のためにアプリ処理を拒否・待機・再送しない。第2-Aのtransport_completeと、操作の結果は分ける。

## 既存IDで足りる範囲と追加契約

既存のsession/display/owner/frame tokenは画像の所有世代を特定できる。一方、複数入力、先着FrameStart、非同期準備があるため、時刻の近さや順番だけでは特定の入力が生成した画像を確定できない。そこで任意の**観測用opaque cause**を渡す。

- JSONの既存type/dataに、任意の `observation:{version:1,boot:u64,operation:u64}` を追加する。bootは入力を採ったsource、operationはそのsource内の入力operation。値を業務判断に使わず、credentialsや入力本文を入れない。
- 通信のversion/kind/length、既存WireMessageの構築API、token、画素、画像CRC、native kind18は維持する。metadataなしの送信は従来bytesと同一。既存decoderが未知のtop-level項目を無視することを回帰で確認する。
- metadataの未知version・不正値・重複等は観測の相関不明として扱い、正常な既存messageの処理を拒否しない。既存の全体長・構文・業務検証は維持する。
- PicoのEV1採取中だけ、物理/仮想入力の受理事実から得たcauseを当該入力通知へ付ける。サーバが観測拡張を処理しなければ通常動作し、追跡は不明となる。ソース間の時計を同期したと仮定しない。
- サーバは受理スコープ内でcauseを保持し、生成したframeと返信へ返す。純粋Engineへ可変observerを入れない。遅れた旧画像に後続入力のcauseを付けない。frameのcauseは割当時に固定し、上限256件のtransient mapで保持する。失効・eviction・再起動で不明になったら推測しない。

## 共通の採取点

| component | 事実と相関 |
| --- | --- |
| Input | debounce/FSMが受理した入力、判断・実行終了。physical/syntheticとlongを固定値で区別。アプリの成功・描画完了とはしない。 |
| Network | 入力通知の送信開始、既存送信関数の結果。成功はローカルenqueue/送出受理であり、対向到着やTCP ACKではない。mainのcauseを明示的に渡し、IRQの受信を勝手に同じ入力へ結ばない。 |
| API | 受信、dispatch、実結果。可能なら要求frame IDとcauseを記録する。拒否時もframe処理成功へ変換しない。 |
| State / Effect（サーバ） | decode/受理、判断、commit、frame割当、外部実行/結果。source内operationとparent、source間cause、frame IDを区別する。 |
| Frame | 妥当なFrameStart受理、全量/CRC完了、拒否。Picoは自分のbootに由来するcauseだけをlocal parentへ解決し、分からないcauseは0のままにする。 |
| Display | static/videoで実際に選ばれたtokenの描画開始、同期完了、E-ink pendingと後の完了/失敗。受信中のglobal tokenを流用して待機中の別画像を誤記しない。 |
| Observation | capture開始/終了、catalog/coverage、source欠測。観測metadataの拒否はアプリAPIの拒否と区別する。 |

EV1の固定bodyは維持し、component8をObservation、event1をcapture境界とする。ON時の開始事実をsnapshotより前、OFF時の終了事実を無効化より前に採り、終了source時刻を得る。catalog2は上の入力/通知送信/API/frame/display境界を持つ。capture境界のaにcatalog、bにcoverage maskを入れる。境界自体がdropした区間も不完全とし、専用の症状検出freezeを作らない。旧catalog1のファイルはtransportとして読めるが、この用途の欠落判定には使わない。

## サーバsourceの保存

サーバsourceもboot、capture sequence、monotonic us、component/event/phase、operation/parent、任意cause/frameを持つ固定値のproducerとする。callerの共有App mutex内でfile IOをしない。cold consumer一つが有界バッファをdrainし、begin/end watermarkとdrop、catalog、coverage、終了時刻をJSONLへ保存する。

任意featureと明示pathでのみ保存し、通常動作の条件にしない。専用threadは既定64MiB上限、排他的create、fsyncを持つ。producerはtry-lock/有界copyのみ。停止時は新規producerを無効化し、in-flight producerを有限に確認して最終snapshotを採る。disk障害・join期限超過・counter飽和・欠測は成功にせず、App処理は継続する。server shutdown時も保存完了と実機処理完了を混同しない。

## ホスト解析の判定

一つのroot causeごとに、device Input→通知送信→server受理/commit/frame割当→対応FrameStart→CRC完了→同じframe/tokenのDisplay完了を結ぶ。causeの一致とframeの一致が必要で、最寄り時刻やプラグインIDで結ばない。

- required事実が揃った場合は対応描画を確認できる。遅延は同じsourceの入力/描画時刻だけで測る。hostでのロック待ち等はhostの時計で別に測る。
- required事実が欠けた場合、catalog/coverage・開始/終了境界・source間cause・sequence/dropが揃った区間だけ、最後に満たされた境界から先の拒否/失敗/未完了を判定する。
- 欠測、CRC破損、途中終了、cause未対応、旧catalog等がある区間はincomplete/unknownとし、欠けた事実を「処理されなかった」としない。
- timeout/slowの閾値、完了待ち期限、何を描画完了まで要求する操作とするかはホストの解析policyである。firmwareには症状別判定を入れない。no-opや待機操作を勝手に「画像が来ない不具合」にしない。

## 受入

- T-01：metadataなしのbytes不変、旧decoder互換、断片/混在/native bytes、invalid metadataでも正常domain messageを処理する。旧Hello・旧token・ACKの拒否規則を維持する。
- T-02：実serverの入力・純粋Engine・App・frame packet生成と、実Pico main/assembly/draw IO doublesを横断し、1入力と対応frameの事実をホストで結ぶ。controllerから業務状態を書き換えて代用しない。
- T-03：複数入力・旧frame/制御応答先着、static/video、E-ink pendingと完了を扱い、後続causeを旧画像へ付けない。map evictionは不明にする。
- T-04：拒否/CRC/IO失敗、full/drop、catalog不足、metadata不足、seq欠測、完了前の有限終了を注入する。missingとincompleteを区別し、同source時間だけでslow/deadlineを判定する。
- T-05：server保存はApp lockと独立、有界・一consumer・排他的file・正常終了/故障を試験する。既存232件、Pico全体、AP2/SIM/TX、NODELAY/SDK出自、実ARMの回帰を確認する。

実USB/IRQ負荷、RF送信、物理画素、E-ink波形・残像、未登録の将来境界は引き続き別受入。ソフトウェアの因果追跡成立を、間欠遅延の原因特定や実機解消と報告しない。

実装上のsource coverageはPico `0x9f`、server `0xeb`。serverのNetwork境界はまだ採取しないため、そのbitを立てない。ホスト解析は入力通知のPico側結果、server受理・commit・frame割当、Picoのframe/CRC/drawを必須とし、server socket送出内の未実装境界を推測しない。server JSONLには正常fsync後にSHA-256付き `.summary.json` を排他的保存し、解析時に対応を確認する。通常のSQLiteアプリ状態保存は本契約の観測ログ保存とは別で、D-02に残る。
