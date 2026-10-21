# 自然heartbeat開始の診断契約案

## 現行ファームでできないこと

対象は書込み済みSDK整合LCD UF2 SHA `11a11556e6e5c71b2f90fb98aeabca15132aabb5914b27d6ccebd217d7a5c485`。[先行TCP hole解析](sdk-aligned-sequence-followup-20261021.md)で必要になった「入力前heartbeat初回COPY/TX/CY」を、現行USB機能だけで一式採取することはできない。以下は候補の固定source `SDK-align-20261021T032923Z/source/firmware` で確認した。

| 操作/状態 | AP2 | TX1/CY1 | 根拠 |
| --- | --- | --- | --- |
| AP2 ARM、USB接続あり | PREPARED、trigger0 | inactive | ack_probe.c:130–133 |
| DTR/USBを切断 | ARMED、trigger0 | inactive | ack_probe.c:52–60 |
| 自然heartbeat送信、入力なし | ARMEDのまま、記録0 | inactive | appendはRECORDING限定、captureも同状態限定（42–44、80–95行） |
| 物理buttonまたはSIM dispatch | RECORDINGに遷移 | 次のtx_syncでactive | ack_probe.c:66–78、net.c:162–175 |
| AP2 FREEZE | FROZEN | inactive | 開始経路ではない（137行） |
| TX1/CY1 STATUS/READ | 参照のみ | AP2状態に追従 | net.c:179–191、usb_control.c:180–207 |

USB command一覧にSTART/periodic triggerはなく、DTRの再開閉でもRECORDINGにはならない。hostもDecoded kind6/7/14/15/16の入力だけで20秒を開始し、heartbeat kind5はARMED prehistoryへ保存するだけ（service/src/ack_probe.rs:161–172）。Piヘッダを早く開始するだけでは初回Pico COPY/CYは得られない。

偽の物理/SIM入力で始めると、それ自体がアプリの切替・frame/ACK・TCP未確認状態を作る。自然heartbeatからの無入力試験ではなくなるため、今回実機で代用しない。現行が不可能と判定したのでUSBを開かず、無駄な再試行は0。

## 最小変更案（未実装・未書込み）

選ぶ案は**次の自然heartbeatでの開始**。DTR切断即開始よりheartbeatの位相待ちを記録窓から外せ、初回COPY直前から既存20秒を使用できる。アプリのheartbeat周期・TCP送出・選択・設定・SDK・計測patchは変更しない。新しいUSB STARTで接続中に記録を始める案、疑似入力を開始合図にする案、容量による早期freezeや既存20秒の短縮は採らない。

通常の `AP2 ARM <boot> <run>` と従来入力triggerはそのまま残す。診断専用 `AP2 ARM_HB <boot> <run>` を追加し、runにmodeを固定する。古いtoolが対応したと誤認しないよう、`AP2 CAPS` と別の有界 `AP2 TRIGGER` replyにmode/trigger種別/message/generationを出す。既存144B reply/96B command上限内で、status行を無制限に延長しない。

| 状態/条件 | 遷移/記録 | アプリへの影響 |
| --- | --- | --- |
| IDLE、ARM_HB、正しいboot・新run | PREPAREDまたはARMED、mode=heartbeat | なし |
| PREPARED、heartbeat/入力 | triggerなし | 送信/入力は通常どおり |
| PREPARED、DTR low | ARMED、gateを固定 | なし |
| ARMED/HB、kind5のbefore_lock、DTR low、active session | RECORDINGへ1回遷移。新event25 PERIODIC_TRIGGERを先頭に保存し、そのheartbeatのBEFORE_LOCKへ続く | heartbeatを生成/早送り/再送しない |
| 同heartbeatのSDK lock取得 | 既存tx_syncがCOPY/tcp_outputより先にTX/CYを有効化 | lock/callback所有を変更しない |
| HBモード待機中の物理/SIM入力 | HB開始としては扱わず、unexpected input品質失敗を保持 | 入力処理を遮断しない |
| RECORDING、追加heartbeat | 通常の記録のみ。起点/期限を延ばさない | なし |
| RECORDING、物理/SIM入力 | 別由来として記録、無入力試験の品質失敗 | 入力処理を遮断しない |
| trigger+20秒、または明示FREEZE | 既存FROZEN/digest/READ/RELEASE | なし |
| FROZEN/partial出力中の別ARM | 既存retained/busy。保存内容を消さない | なし |

heartbeatトリガのrecordはbutton/SIM eventを流用せず、physical/SIM counterも増やさない。mode、event25、kind5、当該message、boot/run/generation/sessionを保存し、初回COPY範囲と初回TX/CYまで同じrunで対応する。before_lock時点の開始とSDK lock内tx_syncには時差があるが、後者はそのheartbeatのCOPYより先。lock取得前の境界以前はunknownで、捕捉したと主張しない。

初回WRITE_BEGIN/TX_POSITIONのsnd_lbb/lastack/unsent/unackedも使い、**開始時点ですでに未確認だったデータがある場合は、そのデータの初回を今回捕捉したとはしない**。TCPをflush/resetしたりACKを待って送信を変えたりせず、初期状態を品質/範囲として明示する。初回heartbeatのCOPY拒否・接続失敗も失敗として保持し、成功扱いしない。

hostは専用arm引数 `trigger=heartbeat` を追加し、対象の現行bindingで最初のDecoded kind5だけを起点にする。defaultは従来input。最初のheartbeatがPiに届かなければhostはARMEDのまま、有限controller期限でfreezeして未到着/未triggerを示す。Pico/hostの20秒は別時計・別起点で、同一時刻の完全窓とは呼ばない。入力eventを偽装してhostを起動しない。

### P2補正：host開始は仮の起点、対象は番号とCOPYで確定

独立レビュー `/tmp/picomado-heartbeat-review-ost82_dy/review-outcome.json` のP2を反映する。最初のDecoded kind5だけではPREPARED中のheartbeatや古い未確認heartbeatと区別できないため、以下を全て満たして初めて同じ対象と認定する。

1. Pico ARM_HB時点の送出ordinalをorigin Mとして保存する。hostは対象の現行connection/sessionの正常処理済みordinalを、既存診断feature内の非待機metadataとして保持する。hostのheartbeat armはbaseline=M、expected_message=M+1を必須にし、現在watermark=Mとの一致を同じstate lock内で検査する。遅れた古いheartbeatが未処理ならarmを拒否する。counterのwrap/connection不明/番号不一致も拒否する。
2. DTR low時のPico gate ordinalも固定する。origin=gate=Mかつevent25のmessage=M+1を要求する。PREPARED中にheartbeatまたは他messageが送出されていれば、hostが先に仮開始してもgateまたは対象番号の不一致で品質停止する。物理/SIM入力も独立に品質失敗とする。
3. 対象の最初のWRITE_BEGINでunacked/unsentがなく、TX_POSITIONのfirst=lastackであることを要求する。古い未確認データ/ACK遅れがあれば、その初回を今回捕捉したとせず品質停止する。接続やTCPをリセット/flushしない。
4. event25→BEFORE_LOCK→ENCODED→成功COPY位置→send returnを一意に対応し、hostでexpected_messageのDecoded kind5/session/JSON長/CRCが一意に一致することを要求する。hostのtrigger messageもこの番号に一致させる。番号はconnectionごとの送受信順序として扱い、途中COPY失敗・reconnect・欠測・prehistory overwrite・仮開始より前の不明なdecoded messageは曖昧として停止する。JSONが同じだけの後続heartbeatへ対応をずらさない。
5. hostの20秒開始はexpected_messageのkind5だけに限定する。古い番号はbounded prehistoryに残すが対象とはしない。期待番号の別kindや期待番号を越えたdecodeはorigin不一致としてfreezeする。未到着なら未triggerのまま有限終了する。両端の時刻差やfixtureだけが知る送信時刻で同一性を補完しない。

この方法はapp heartbeat payload、周期、COPY/TCP bool、送信動作を変更しない。host watermark更新も対象connectionだけ、try_lock・固定整数metadataで、hot-pathのformat/IO/待機を追加しない。ロック競合でwatermarkが更新できなければ準備拒否または既存contention品質失敗とし、安全側で停止する。host readerが新connectionを認定したときwatermarkをリセットし、旧close/旧readerが新bindingの値を書き換えない。

検証にはPREPARED中heartbeatのhost先行開始、ARM/DTR前後のmessage、古いunacked、ACK遅れ、遅着/未到着、同じCRCの複数HB、COPY失敗、ordinal不整合/overflow、再接続と旧reader、metadata lock競合を追加する。単なる最初のkind5やCRC一致では受け入れない。

新toolはheartbeatモードを明示し、Caps未対応ならUSB準備段階で拒否する。単一owner、DTR lowの間のUSB通信0、対象tupleヘッダのみ、設定/flags不変、有限採取・保存digest・正規release/IDLEの既存手順を使用する。heartbeat triggerの由来条件を追加した専用validatorを作り、既存input validatorの失敗を無視して流用しない。試験開始のtriggerと測定対象入力を混同しないため、最初の試験は測定対象入力自体を0回とする。

## 容量・資源gate

既存容量はAP2 96件、TX32件、CY8件、host512件。今回の独立実装では単純な全量CY15へ拡張し、15行すべてで対象tupleのdataの初回・全再送・異常を記録する。選択範囲による除外やserializer/overlayは導入しない。16件目は既存dropを増やし品質停止する。20秒窓と全品質停止条件は維持する。LCDの通常heartbeat周期15秒で、trigger直後の1件と15秒後の1件が通常2件。heartbeat1件のAP2送信境界は8件、20回程度の1秒snapshotとtrigger1件を加えた無再接続・無入力の基礎負荷は約37件。ただしfreeze端点・他イベント・TCP再送/自然なframe更新を含む完全上限ではない。

CY15の基礎2件に対し余裕は13件。Rを再送、Xをそれ以外の対象data送信とすると `2+R+X <= 15` が必要。旧試行から「入力/ACKを引けば収まる」とする反実仮想は容量証明にしない。正規の再送・server側frame更新を含む最悪順序で15以内とする証明は未完了。**CY15でも20秒全量完全性を保証済みとはしない**。16件以上ならfull-window incompleteのまま停止。IRQ/CRC/pollの既存境界、対象tupleのscopeとskip/drop/reject等の全counterを保存する。

CY/TX/AP2/hostのdrop・malformed・generation変化・digest不一致、全TX/CY join不足、観測USB overlap、初期/途中binding不整合等の全停止条件を残す。溢れた窓をcleanにしない。未知の初期未確認データを今回の初回に読み替えず、各runを独立保存する。容量で早期終了した窓を従来20秒の完全性として報告しない。

追加常駐RAMはCY7行×144B=1008B、mode/由来等32B以内、追加stack64B以内を上限案とする。整合候補heap11096Bからの概算は10056Bであり、実装後のARM mapで照合して既存heap8192B/stack4096B下限を維持する。初期値OFF、IRQ writer/待機/動的allocation/新USB hot-path出力を追加しない。CY LL生成patchやSDK原本は変更しない。

## 実装前に固定する検証条件

以下は実装の受入条件であり、現時点の成功実績とはしない。

1. 現行sourceの特性確認：ARM→DTR low→heartbeatを複数回呼んでもAP2 count0/trigger0/capture=false、CY inactive/count0。物理/SIM入力だけがRECORDINGを作る。未知START commandやFREEZEは開始代用にならない。
2. 新状態表の全遷移：旧input modeは同一挙動、HB modeはPREPARED時に始まらず、DTR low後のkind5で一度だけ開始。kind5以外・物理/SIMではheartbeat開始にならない。前後・同時境界・再ARM・別boot/run・古いrunを含む。
3. 周期起点の順序：PERIODIC_TRIGGER→BEFORE_LOCK→LOCKED→COPY→同期linkoutput→CY entry/prebus/finish。生成された最初のdataも再送もkey/CRC/範囲を対応する。before_lock中のIRQ/lock待ち以前はunknown。COPY失敗/ERR_MEM/output失敗を保持しbool意味を変えない。
4. windowは最初の自然heartbeatから20秒固定、後続heartbeat/入力は延長しない。triggerなしのARM待機には偽の20秒成功を作らない。世代変化/初期unacked/予期しない入力は品質・観測範囲として保存する。
5. host default/inputとheartbeat mode、到着・未到着・遅れて到着・違うbinding・別kindを検証。両端の起点と時計を別に保存し、heartbeatの先頭不達でも欠けたhost triggerを捏造しない。
6. 有限controllerのCaps不一致・USB断・返信timeout・部分READ・保存失敗・busy/FROZEN保持をPTYで検証。実USBなし。同一owner排他、正規freeze/save/release/IDLE、全既存品質停止条件をテストする。
7. 合法なproducer/drain順序とTCP再送/自然frame更新を含む容量モデルで、AP2/TX/CY/hostの各上限を満たす。未達なら実機gateを開かず、数字や窓・品質を黙って変えない。
8. ARM資源、通常app/heartbeat packet・送信bool・設定sector・SDK/private patch・flags不変、既存host回帰を確認。observer effectは新mode未使用時と使用時で入力poll/ACK/描画/lock保持を比較し、許容値を実施前に別途固定する。

## 隔離実装の範囲と現在位置

基準は受入れ済みf2bdc7bのappとSDK整合候補の診断適合5ファイル。次の実装用隔離sourceを作る際にこの出自を固定し、容量WIP6ファイルは現在のworkに元hashで保持する。主な対象はfirmware ack_probe.c/h、host ack_probe.rs、専用collector/validatorとそのtests。net.cのtx_sync/共有hookや既存LL patchはそのまま使える見込みだが、順序試験で確認する。

P2契約補正を先行コミットした後、この契約を基準に独立worktreeで実装・ホスト回帰・ARM buildへ進む。元workの容量WIP6ファイルは含めない。実機開始gateとobserver effect実測は未成立のままで、新規採取・USB/実機再flash・サービス/設定/SSH/push操作は行わない。

## 現行版の特性確認結果

設計先行コミット44323f5の後、書込み済み候補と同じ未改変ack_probe.c / cyw43_inner_probe.cをnative ccでcompileした。時刻・USB出力・transportをstubにし、PREPARED heartbeat、ARMEDで45秒/3 heartbeat、未対応START、FREEZE、host内物理入力fixture、host内SIM fixtureの6ケースが成功した。ARMED中のheartbeatはtrigger0/count0/capture=false/CY inactive、入力fixtureだけが記録を開始した。

証拠はR/analysis/trigger-feasibility/current_trigger_test.c、build.log、run.log、validation.json。Rは先行実機証拠rootと同じ。source/harness SHAを保存した。これは現行開始条件の確認であり、新トリガ実装・ARM build・実機試験の成功ではない。fixtureの物理/SIM呼出しはホスト内だけで、USB open/実機入力は0。新モードの受入条件と容量/資源gateは未検証のまま。
