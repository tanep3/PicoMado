# 観測不足への実装とローカル検証

設計先行コミットは `d70dd84`。その後の隔離workで [契約](observation-runtime-gap-contract.md) のR-01〜R-04を実装した。これは遅延の根因修正・実機改善の受入ではない。保存済み実測1,478.404msの内訳と欠測885件は、[既存解析](manual-switch-analysis-20261020.md) のまま扱う。

## 実装した境界

EV1の76B body、CRC、通常API、送信bool、ACK、選択・設定・並び順は維持する。新catalogは3。Pico coverageは0x9f、serverはNetwork追加で0xef。parserはcatalog2も読めるが、旧sourceへ新しい境界の網羅性を付与しない。

Pico Networkのevent 256〜263はconnection、COPY、window、TCP position、output結果、linkoutput、RX callback、API ingress。288+kindはmainのtake/decode完了。これらのsession欄はconnection cookieで、従来Network eventのアプリsessionとは区別する。COPY parentは同期通知operation、linkoutput parentは一意なCOPY、API ingress parentは直前のtake。native FrameBytesも実frame IDを持つingressとして記録し、domainでの受理成功とはしない。

COPY成功後、tcp_output前に固定8枠のbyte範囲をpublishする。packet全体が単一範囲に含まれる場合だけ対応付ける。複数cause同居、未記録byteの混在、eviction、wrap、別flowはunknown。同期linkoutputと後の再送は同じ表を見る。AP2と共通のhookから元関数へ同一packetを1回渡し、戻り値を保持する。tcp_output errorで既存のCOPY成功boolをfalseへ変更しない。再接続と採取ON世代の変更で対応表を破棄する。

RX callbackはconnection内のraw範囲、takeはWS headerを除いたapp workspaceの有界spanを記録する。envelopeの全範囲を含むRXが一意なときだけparentを持つ。複数callbackにまたがる場合やspan容量超過・32bit offset overflowはunknown。時刻の近いcallbackを代用しない。

server Factのdetailは1 Registered、2 RawRead、3 WsComplete、4 Decode、5 ReplyQueue、6 WriterDequeue、7 ReplySink。connectionはTCP accept order（custom hostは既存WS開始order）。RawReadのfirst/endはraw読出し累積位置、WsComplete/DecodeはWS binary payloadの累積位置で、座標系を混ぜない。Decodeは前回から持ち越したapp bytesを含む範囲。API Received→Dispatchedと返信queue→writer→sinkをoperation/parentで結ぶ。queue完了はsend/reserve受理後の観測で、writerが先に動く競合も順序変更せず記録する。Frame Pendingのargument=0はApp静止画登録、1は動画writer準備であり、どちらもTCP送信完了ではない。

**RawReadはuserspaceでreadが完了した点で、kernel到着時刻ではない。最初の実観測以前は常にunknown。** 到着前待ちと到着後未読待ちは識別しない。fixtureだけが持つ到着oracleを保存事実へ渡していない。HTTP/upgrade以前の未観測prefixをRegisteredで示す。tungstenite内部の消費範囲が得られないため、raw→WSの個別parentもunknownのままとする。connectionが同じという理由だけで各APIへraw readを割り当てない。kernel観測・SDK/RF追加は行っていない。

## 排出の上限とcounter

共有queueは32枠、観測producerの閾値16枠、control予約はそのまま。固定RAMのcounterはoffered、高水位、drain/write回数・byte、partial/zero write、budget停止、最大drain時間、飽和を持ち、counter更新からEV1を生成しない。要求時の `OBS METRICS <cid-hex> <page-hex>`（page 0〜3）だけで取得する。source dropは既存の観測queue予約拒否、旧trace dropとは別で、baselineをリセットしない。

drain自体の絶対上限は8 write・1024B・500µs。1loop末尾は4 write・768B・500µs、処理済み受信message後と同期通知前後は6 write・768B・200µs。既存SDK所有を解放した位置からだけ呼ぶ。時間をwrite前に検査し、最終の有限・非待機write分の超過はあり得る。zero writeは即終了し、queueが空くまで待たない。受信8/2message budget、input poll、描画・ACK・動画policyは変更しない。

新Network事実を加えた22 facts/frameの12fpsモデルでは、当初2write/messageの案は欠測を発生させた。このため上の有限6writeへ修正した。64B partial write、7message/frame、19ms同期描画、100frameの同じモデルでsource欠測0、idleでqueue/partialの排出完了を確認する。実USBのroom推移やIRQ競合、実機での無欠測を保証する結果ではない。

## 採取開始

同じexclusive ownerでDTR preludeを読み、150ms quiet（最大2s／64KiB到達で打切り）後に独立nonceのSTATUSを送る。既にONなら正規OFFを確認してから新ONに進む。port factory後に時計を更新し、起動待ちを古いcommand deadlineへ算入しない。ON応答不明時はONを再送せず、新STATUS→OFF→新nonce ONで別区間にし、先の区間とglobal incompleteは保持する。再接続の準備待ちは残りのデータ採取時間を消費させない。

既存 `ready`/callbackはtransport準備の互換項目。ユーザーへ操作開始を伝える側は **`causal_ready`** または新 `causal_ready` callbackを使う。対応boot/seq、CRCを通った実begin、catalog/coverage、beginを含むdurable prefix、保存欠落なしが条件。ON応答だけでは通知しない。通知時prefixをsegmentとjournalへ保存する。begin欠落で準備完了を偽造しない。stop/OFF、watermark、保存worker、summaryの確認と単一owner、SIM COMMIT拒否は保持する。

summaryの `complete` は既存transport項目の互換aliasであり、source境界の完全性は新 `causal_complete` で判定する。実begin/endのboot/seq/catalog/coverageが揃わなければfalseとなる。application/render完了は引き続き別判定である。

## 検証証拠と限界

ローカル証拠はtask直下 `architecture-refactor-20261020/gap-*.log`、`gap-build/results.json`、最終checkpoint JSONに保存する。新規PTY、in-memory IO、temporary DB、loopback fixtureのみを使った。通常sandboxのUnix/loopback socket EPERMは、実機アクセスを含まないfixture限定実行へ切り替えて分類した。ASan/UBSanはptrace下で利用できないLSanを無効にして検証する。

採取・再フラッシュ・USB open・サービス操作・SSH・pushはこの実装段階では実施していない。新境界を実機で取得するには、レビュー後に別途承認されたLCDファーム更新と観測server/collectorの反映が必要。E-ink用ビルド成功は書込み承認ではない。通常serverへ戻す従来手順と退避先は維持し、データは巻き戻さない。

静的RAM、リンカheap範囲、関数単位stackは候補artifact/map/.suで照合する。これらを実行時heap/stack高水位、実IRQ負荷、RF、物理パネル、遅延解消の証拠へ転用しない。既存ログを新catalogで取得した扱いにも変更しない。

## 2026-10-21 ローカル完了結果

| 検証 | 結果 |
| --- | --- |
| server全feature library | 297成功、0失敗（追加3件を含む）。最終338.88s、動画変換を含む |
| server通常feature library / server設定 | 273成功 / 1成功、0失敗 |
| firmware既存host | 175 PASS項目。EOF、実main、LCD/E-ink、native/legacy、再接続を含む |
| firmware契約＋ASan/UBSan | 成功。COPY公開、両観測を有効にしたAP2 hook、同期/再送、RX/ingress、capture切替、22 facts/frame burst、idle排出、counter飽和を含む。LSanは環境制限で無効 |
| API結果、USB/TX、CYW43 inner回帰 | 成功。USB/TX Python28件、inner Python13件を含む |
| collector / 因果parser | 新規PTY等24件 / 10件成功。前ON応答不明、5秒相当のfactory遅延、DTR burst、実begin保存前の通知拒否、catalog2/3を含む |
| ARM | 同じ個別設定を保ったLCD観測ON/OFF、E-ink_0観測ON、E-ink_1観測OFFが全て成功 |
| server offline release | 全feature成功、CLI不正引数の安全な起動前smokeと動的依存照合も成功。サービスとして起動していない |

| 候補 | 静的main RAM | リンカheap可能範囲 | stack予約 |
| --- | ---: | ---: | ---: |
| LCD観測ON | 251,064B | 11,080B | 4,096B |
| LCD観測OFF | 250,376B | 11,768B | 4,096B |
| E-ink_0観測ON | 181,200B | 80,944B | 4,096B |
| E-ink_1観測OFF | 180,612B | 81,532B | 4,096B |

旧採取LCD観測版の250,432Bから632B増。LCDの観測OFF RAMは旧通常診断版250,376Bと一致する。新しいON/OFF差は688B。queue拡大はしていない。ONの関数単位 `.su` はdrain 56B、flush_one 376B、pm_observe 64Bで、call chain全体・割込み併用時の高水位ではない。共有SDK LLは既存pinのSHAと一致し、私有設定・bindingの比較も一致した。
