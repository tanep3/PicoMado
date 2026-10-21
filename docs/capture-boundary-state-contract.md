# 採取区間の境界状態遷移契約

`4563976` の独立レビュー追加P2に対する、コード修正前の契約。ON/OFF原子性と同seq重複の既存修正を維持し、同じbootのbegin1→begin2→end3を完全とした退行を区間全体の不変条件で修正する。

## 不変条件と範囲

一つのWindowは一つのON応答 `(boot, start_seq)` にだけ結び付き、区間内に受理できるbeginは**ちょうど一つ**。最初のbeginもON応答と同じboot/seqでなければ受理しない。受理後のbeginはseq・catalogが同じでも異なっても追加開始であり、品質エラーとなる。最初のbeginとhost sequenceを上書きせず、失った品質を後の正常境界で回復しない。再接続・やり直しには新Windowを使う。

ON応答前は既存32件の有界preludeへ元host sequence付きで保存し、応答確定後に同じ状態機械へ順番どおり渡す。区間前と証明できる、同じbootかつstart_seqより小さいpreludeは除外する。別bootはseq比較で区間前と推定しない。DTR/STATUS/OFF中の私有preludeは従来どおり採取入力から除外する。

最初のendより後の、同bootでend_seqより大きい事実や別bootの次区間suffixは現区間から除外する。ただしOFF応答はその最初のend自身と同じboot/seqでなければ不完全。end前の低いseqのreorder、別bootや追加beginを、区間外として捨てない。終了後に同じ/低いseqの境界が再来しても品質エラーとする。

## 状態遷移表

品質エラーは状態と独立のstickyな集合で、以後解除しない。source seq/time/drop検査も別途維持する。境界はcomponent=Observation/event=1、begin=Received、end=Completedで識別する。catalog/coverageの不正も品質不成立である。

| ID | 状態 | 入力・条件 | 次状態 | 判定・作用 |
| --- | --- | --- | --- | --- |
| T01 | UNANCHORED | fact | 同じ | 有界preludeへ保存。準備不可 |
| T02 | UNANCHORED | 正常ON応答 | WAIT_BEGIN | anchorを1回確定しpreludeを同じacceptへ再生 |
| T03 | WAIT_BEGIN | 同bootの区間前fact（seq<start、またはseq=startの非begin） | 同じ | 除外。区間内のbegin数に含めない |
| T04 | WAIT_BEGIN | anchor一致のbegin、先行する区間内factなし | OPEN | 最初のbegin/host sequenceを保存 |
| T05 | WAIT_BEGIN | anchor不一致のbegin、または先行区間内factの後のbegin | 同じ／OPEN | unexpected_begin／begin_reordered。品質不成立。後から正常beginが来ても品質は戻らない |
| T06 | OPEN | begin（同seq、異seq、低seq、異catalogを含む） | 同じ | duplicate_beginまたはunexpected_begin。最初の開始を上書きしない |
| T07 | WAIT_BEGIN / OPEN | 区間内の別boot | 同じ | boot_changed。別bootのbeginで新区間へ切り替えない |
| T08 | OPEN | 同bootの最初のend | ENDED | 最初のendを保存。通常seq/time/drop検査を通す |
| T09 | WAIT_BEGIN | end | ENDED | unexpected_end。begin無しでは完全にしない |
| T10 | ENDED | 証明できる次区間suffix | 同じ | 現区間から除外。OFF watermarkとの照合は必要 |
| T11 | ENDED | 同じ/低いseqのbegin/end、reorder | 同じ | duplicate/unexpected boundaryまたはsource reorder。再開しない |
| T12 | WAIT_BEGIN / OPEN | 通常のfact | 同じ | source検査のみ。WAIT_BEGIN中は準備不可。遅いbeginの判断にsource進行を残す |
| T13 | ENDED | 正常OFF応答が最初のendに一致、品質エラー無し、実begin/endと保存が有効 | 同じ | 因果区間の完全性を認める。準備通知は既に終了した区間へ新発行しない |
| T14 | 任意 | 切断・timeout・partial line・OFF欠落/不一致、または境界の欠落でfinalize | ABORTED | 不完全。既存transport項目と因果完全性は別判定 |
| T15 | ABORTED | fact、再ON応答 | 同じ | event_after_abort／unexpected_start。不完全を維持、新Windowが必要 |
| T16 | anchor確定後 | 再ON応答（Window.begin再呼出し） | 同じ | unexpected_start。anchor/source進行をリセットしない |

`causal_ready` はOPENかつ品質有効、実beginとdurable prefixが揃ったときだけtrue。追加beginが届く前に有効だった過去通知は捏造して消さず、到着後のstatusと最終結果をfalseにする。`causal_complete` はENDED・実begin/end・ON/OFF照合・保存・品質が全て有効な場合のみtrue。legacy transportの完全性だけから因果完全性を作らない。

## 解析器とテストの導出

canonical interval_sourceも同じWindowの状態機械へ**実beginを含む**保存factを順に渡す。beginを検査対象から抜いて単にusだけ設定する経路を廃止し、同じ範囲除外と一度限りの開始を使う。root抽出の返却範囲は従来どおり維持する。

T01〜T16をcase ID付きfixtureへ写し、正常・同seq/異seqの追加開始・低seq/reorder・異catalog・別boot・end先行/重複・前後区間除外・境界欠落・異常終了・再anchor拒否を検証する。各fact列をON応答前/後/途中に置き、単一chunkと1byte分割で再生してCollector/Windowとcanonicalの因果判定を照合する。品質エラー後に後続の正常境界を足しても回復しないことを検査する。

新規PTYではbegin1→begin2→end3を応答前後・分割で保存し、fsync後も通知0回、causal_complete=false、unexpected_beginが残ることを確認する。late追加begin、別boot、異常終了も保存summaryと解析結果で検査する。状態表からの正常ケースは準備・完全性がtrueになることも確認し、全拒否で代用しない。

変更はhost collector/解析器とその試験に限定する。firmware C・TCP/hook・server・個別設定・候補artifactのSHAを照合し、未変更なら以前のhost/ARM/server検証を継承する。collector/因果parser、境界状態表、実IRQ境界byte列の読み戻しを回帰し、固定コードと証拠をコミットする。実機・USB・SSH・サービス・pushは対象外。
