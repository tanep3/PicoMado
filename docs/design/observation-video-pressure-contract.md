# 動画負荷下の共通観測対策契約（レビュー用、未実装）

[実測](../investigations/lcd-sim-recapture-20261021.md)と[TCP追解析](../investigations/tcp-retransmission-followup-20261021.md)に基づく。以下は候補の合格条件であり、実機の上限/達成値ではない。設計確定前に実装しない。最大USB zero期間・最低排出能力は現counterから確定できず、実測gateが必要。追加flashなし。

## 保持するもの・採る最小方式

アプリwire、COPY bool、ACK/描画/動画policy、AP2元hook1回転送、設定/選択/有効化/共通順序を維持し、plugin名で分岐しない。input、COPY/TCP範囲、output、初回/再送linkoutputと結果、制御API、frame開始/CRC/完了/失敗/取消、描画開始/完了/失敗/取消、source開始/終了/欠測を全て保持する。無入力の未知エラーも共通境界から追う。

候補をlossless binary＋有界queue/公平serializerに限定する。件数集約やnetwork詳細の選別削除は採らない。元76B事実の全field、64bit timestamp/seq/operation/parent/session/frame/dropped/boot、32bit a/b、event/component/phase、catalog3/coverage0x9f、従来完全性判定を保存。集約は対象外で、後日採用するなら別catalogと識別能力/完全性の契約を先に定義する。counter更新からEV1を再帰生成しない。

## 実証burstと合法producer/drain順序

原動画seq18→43（us191867379→191885594）は18.215ms内に少なくとも26 offer、drop0→18。最終seq7804→7836（us814889814→814906984）にも17.170ms内に少なくとも33 offerがある。26を実機最大とせず、モデルには50同時offerを含める。さらにseq8071→8160は69.890ms中90試行で、R600/sならburst項は少なくとも48.066必要。旧B40候補はこの原証拠を覆わないため撤回しB50に補強する。lost offerの個別時刻は不明で、両端間の均等/先頭集積/末尾集積を仮定別fixtureにし、実機時刻と呼ばない。

SDK lock/IRQ内でdrainしない。必須順序は「26 offerを18.215ms中drainなし」「33 offer/17.170ms」「50同時offer」→SDK解放→mainの有限drain→message/19ms描画→有限drain。SDK連続占有20ms、既存sleep5ms、zero/partial、各予算打切りを最悪順に重ねる。必要な新排出点はmainがSDKを解放した直後とmessage/同期描画前後だけ。SDK20ms超もstressへ含め、性能範囲外と明示する。

## 数値のある負荷・service条件

任意区間t秒の到着上限を次のtoken-bucket曲線とする。平均だけで判定しない。

| class | arrival上限 | payload/wire上限 |
| --- | --- | --- |
| 観測O | 50＋600t records | 76B / 84B |
| control C（SIM/AP2/OBS返信全部） | 10＋80t records | 143B / 151B |
| 通常text/timing T | 2＋20t records | 143B / 151B |

AP2 READ8はheader＋各recordのidentity/data 16行＋end＝**18行**（ack_probe.c:168–179）。元の25ms/行を保持し、AP2全返信の到着曲線をA(t)≤1＋40t、SIM/OBS/TX/CY等の他controlをU(t)≤9＋40tとする。C合算は10＋80t。READ8の18行を瞬時burst18にせず、0,25,…,425msの合法生成をfixtureに含める。18行の最短生成span425msは、page全体の最大待ちではない。

生成してqueueへ投入した単一C frameの最終byteまで150ms条件を維持。AP2 poll間隔≤50msも追加の未実証gateに置き、command受理から各line offerまで最悪75ms、18行のend出力まで18×75＋150＝1500msをpage条件とする。既存4秒host timeoutを変えず、25ms pacingを削除/まとめ生成して合格させない。

これらは許容負荷/試験曲線で、現実の無制限command spamが自動でこの上限に収まる主張ではない。SIM/OBS等をAP2に独立加算した上で検査し、C全体と各内訳の両方を満たすこと。DTR大量snapshotは観測OFFのpreludeへ保存。text40行/s、U control80行/s、C burst11/18瞬時、O burst51/64/128も範囲外stressで試す。

実測518.362/sの84B化だけで43,542.411B/s＋他出力。検証範囲R600ではO50,400＋C12,080＋T3,020＝65,500B/s。最低物理排出能力を全class backlog時80,000B/s、最大連続USB zeroを10msとする。これは未測定の合格条件で、累積zero回数から導出した値ではない。

SDK/描画/zero/packet化を全て含め、classごとにS_i(t)≥R_i×max(0,t−0.050) byteを要求。R_O=56,000、R_C=16,000、R_T=8,000 B/s。work-conserving DRR重み7:2:1、候補byte quanta1120/320/160、record境界だけで切替。deficitは持越すが2quantum＋最大frame以内。重み設定だけでserviceを証明せず、合法interleave全てで下限を検査する。

drain1回の8write/1024B/500µs上限とzero時即戻りを維持。時間はwrite前予算判定で、割込みを含む絶対wall-time上限とは呼ばない。上のserviceからbacklog上限O80/C14/T3。必要容量候補O84（通常82＋begin/end専用2）、C16、T4。Cはbacklog14に対し1枠をbusy/拒否の確定返信用へ予約し、残り1枠を保持余裕とする。予約済みC slotなしにmode/owner変更を適用しない。queue→最終byteの待ち条件は通常control150ms（50ms＋1510/16000＝144.375ms）、O130ms、T90ms。ON/OFF/FORMATはwatermark barrier込み400msを別上限とする。

zero11/50/100ms、service79,999B/sなども試す。USB不通で有限wire応答を保証できない場合、アプリを待たせず既存host4秒timeoutでunknown/uncertainとし再送しない。性能条件違反と完全性を混同しない。事実が全保存できた場合の完全性は実証で判定し、loss/CRC/境界/返信不明は従来どおりincomplete。範囲外性能を合格に偽装しない。

## 単一serializer・partial所有・形式

通常ログ/timing/AP2/OBS/SIM/観測の全出力はmain所有serializerだけがUSBへ書く。IRQ producerは固定値予約/copyのみ。1 pending frame＋offsetがpartialを排他的に所有し、完了前に他class/formatのbyteを挿入しない。

envelopeはmagic A5 5A＋version/type1B＋payload長1B＋payload＋header/payloadを覆うCRC32 4B。v1のO/T/Cを区別し、O長は厳密に76、T/Cは143以下、最大frame151B。通常ログにmagicや改行があってもtype/長さ/CRCで識別。collectorはCを元ASCII lineとしてnonce/command muxへ、Tを通常ログへ、Oを従来fact schemaへ渡す。

parser buffer304B、readは残空き以内、1回scan/CRCは4候補まで。invalid tag/length/CRCはsticky品質エラーとし1byte進めて再探索。単発破損後clean frame開始から302B以内の復帰を検査し、512B消費しても復帰不能ならsegment打切り。無限再同期/無限bufferは不可。CRC偶然一致を数学的完全性と呼ばない。

旧textを既定とし、OFF時のnonce付きFORMAT ACKを旧形式で最後まで書いてから全出力をbinaryへ切替。逆はbinary C ACK完了後にtextへ戻す。途中mode変更/複数FORMAT/lifecycleはbusyで拒否しpartialを捨てない。新ownerは有界preludeで形式/nonceを照合するまでONしない。DTRで保持run/partialをresetしない。正常終了はOFF確定後textへ戻す。

ON/OFFのenable・境界fact・watermarkは従来の同一短時間IRQ禁止区間で確定。専用begin/end2枠を維持。lifecycle返信は対象watermarkまでのO frameが全byte出力済みになるまで非blocking保留し、OFF ACKがend factより先に出る退行を防ぐ。global barrierは1件だけ。OFF後もqueue排出を継続し、アプリはUSBを待たない。400ms条件外で応答不明なら再実行しない。

## AP2 / EV1 overlayと保持の追加条件

[資源比較](observation-resource-reallocation.md)のAP2/EV1 ring排他overlayを推奨候補とする。AP2 ARM/PREPARED/RECORDING/FROZEN/READ中はAP2所有。FROZENは保存digest照合済みRELEASEまで保持し、READ18行生成中・C queue内・serializer partialのいずれかにそのrunの未完了frameがあれば、RELEASE/EV1移譲をbusy確定応答で拒否する。新ownerのために自動破棄しない。

EV1 OFF後もend watermarkまでのO queue/partialとlifecycle ACKの出力完了まではEV1所有。AP2 ARMとEV1 ONは相互busyとし、拒否返信用C枠を確保してから判定を確定する。busyを単なる無応答/timeoutで代用せず、元nonce/runに対応付ける。USB不通は別途unknownで、state適用済みと推測しない。

overlayではAP2とEV1を同時採取できない。上のO＋C最大値の合算65,500B/sは安全側のservice試験用上限で、同時実機採取可能性を示さない。AP2 modeではO=0、EV1 modeではAP2取得不可・拒否返信はUへ含める。別runのSDK観測とEV1事実を同じ操作の因果証拠として合成しない。共通条件の対照比較は別trialとしてboot/run・観測方式・限界を表示する。

## RAM・counterの実装前gate

候補layoutはO native68B（17×32bit word、boot共有、64bit値は2wordでIRQ内copy）×84、C148B×16、T既存native160B×4＝8720B。旧32×160＝5120Bとの差3600B、新管理/format/barrier/検証counterを96B以下、総増分3696B。unaligned pointer参照/IRQ内formatを避け、mainでwire変換する。sizeof/static_assert/mapで実寸確認。

現在static251064B/heap可能11080Bから計算上残7384Bで、要求heap8192Bへ808B不足する。独立layoutはRAM gate不合格。overlay候補はAP2 record5376BとO ring5712Bをunion化し、C2368B/T640B/管理上限256Bを含め旧比1520B回収、heap計算上12600B。ただしsizeof/map・実heap/stack・80KB/s/50ms service・AP2 poll間隔は未実証gateであり、設計最終受入れ/実装は保留。stack予約4096Bは保持する。下限を勝手に下げず、レビューへ戻す。動的heap/stack使用の確認も必要で、linker値だけで保証しない。

必要counterは最大input-poll間隔、observer IRQ禁止、USB zero連続span、format/write時間を既存pageへ有界・非再帰に追加し、overlay候補では全管理上限256Bへ含める。独立layout試算は96Bだった点を区別する。per-poll/per-byteログを増やさず、未計測値をhost mockから補わない。

## observer effect・受入条件

旧ON＝直前f0c6a15a、候補新ON＝本方式source有効、候補新OFF＝同じcandidateのsource無効。同LCD/config/clock/SDK/diagnostic flags、asset SHA、server/操作script/間隔、配置/経路で各3回以上。同一trace（static切替4操作＋動画30秒＋idle10秒）。新ON/OFFは交互。旧ONとの差は順序/RF変動の限界を残し、今回勝手に旧firmwareへ戻さない。初期GETは解放/終了確認のみ、追加で旧233 GET/約30秒も同条件評価。

| 指標 | 旧ONと新OFFの両方に対する候補許容差 |
| --- | --- |
| 実input poll間隔 | p99増分2ms以下、最大50ms以下。loop_gapを代用しない |
| CRC完了→ACK enqueue | p95増分1ms以下、最大増分5ms以下 |
| CRC完了→描画開始 | p95増分2ms以下、最大増分5ms以下 |
| LCD描画所要 | p95増分1ms以下、最大増分2ms以下 |
| ACK/描画進行 | 新規ACK timeout/CRC後500ms超の新規停止なし。30秒の描画数差1枚以下。policy取消/dropは別記 |
| observer producer IRQ禁止 | 最大10µs以下、p99増分2µs以下 |
| USB FIFO IRQ禁止 | 最大100µs以下、最大増分10µs以下。SDK外側を含む連続禁止増分10µs以下 |
| CPU | 同window main active時間比（既存loop sum/window、CPU全体ではない）増分5 percentage points以下。IRQ/format/writeは別記 |

最大/分位/試行数/欠測を保存。基線が欠測なら該当指標unknownで合格にしない。通信再送の待ちと受信完了後処理を分ける。原seq/必須境界/CRC/64bit時刻/cause/frame/canonical完全性を照合し、範囲内無欠測、範囲外fail closed、idle排出を検査。

host回帰は1byte partial/zero復旧、class飽和、ON/OFF IRQ atomic、形式境界/偽magic/CRC/再同期、AP2/SIM混在、begin応答前後/分割/重複、TX同期/再送/1回転送、観測OFF互換。実機はstatic ready→初回画像→動画→static→無入力動画。追加flash前に理由/対象LCD1台/変更範囲/SHA/復旧UF2/binding/設定/資源/host回帰/必要BOOTSEL操作を親へ提示する。容量実装・追加実機操作は設計確定後。未確定原因に対してNagle/timeout/OS/network設定を変更しない。
