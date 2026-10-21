# 採取境界の単一開始・状態遷移検証

`4563976` の再レビューで、元のON/OFF原子性・同seq重複P2は解消を確認された一方、begin1→begin2→end3が完全になる追加P2が見つかった。本書はその著者側修正の結果で、独立再レビュー待ちである。

- 文書先行：[状態遷移表](capture-boundary-state-contract.md) の `c85e8d3`、区間前を厳密なseq<startに限定した補強 `7cdcd91`。
- 固定実装：`f2bdc7b2e17cc620e5d568c16842ee6ea871e7b0`。
- [checkpoint](checkpoints/20261021/capture-boundary-state.json) にreview/source/log、生JSONLとsummaryのSHA、未変更ソース・候補artifactの照合を保存した。

## 実装

Windowの境界状態をUNANCHORED→WAIT_BEGIN→OPEN→ENDEDと、異常終了のABORTEDで明示した。全begin/endを一つの遷移処理へ渡す。beginを受理できるのはWAIT_BEGINでanchor一致の最初の1件だけであり、受理後のbeginはseqやcatalogを問わず品質エラーとなる。同seqはduplicate_begin、他はunexpected_beginとして区別するが、拒否条件は状態の不変条件で共通である。最初のbeginとhost sequenceを保持し、後の正常endで品質を回復させない。

canonical interval_sourceも実beginを含むfact列を同じWindowへ渡す。開始を抜いてusだけ設定していた経路を廃止した。区間前と証明できる同bootのseq<start、終了後と証明できるsuffixだけを同じ状態機械で除外する。anchorと同seqの通常factを区間前と推定せずsource重複として拒否する。旧応答fixtureは、過去fact1/2→実begin3/ON応答3→end4/OFF応答4という現行の原子的境界へ揃えた。

readyはOPENかつ実begin・durable prefix・品質有効時のみ。異常終了後のfact/再anchorも、同じWindowを初期化し直して通さない。legacy transportと因果完全性の区別を保持し、区間中に現れた別bootを近いseqへ対応付けない。

## 状態表からの試験

fixture `firmware/tests/capture_boundary_cases.py` は実装をimportせず、表の期待値を持つ。`capture_boundary_state_test.py` は文書T01〜T16の全行にcaseがあることも検査する。

| 検証 | 結果 |
| --- | --- |
| 状態表5試験 | 成功。fact/ON応答位置/単一chunk・1byte分割の192通り、T01〜T16全行を含む |
| 保存PTY | 表由来40通り＋異常切断/準備通知後のlate begin 2通り、全成功。オンラインとcanonicalの因果判定が一致 |
| 既存collector | 27試験成功 |
| 既存因果parser | 10試験成功。旧/新catalog、実CのFrameStart、前後区間除外などを維持 |
| 実IRQ境界byte列 | 以前保存したbegin1/Network2/ON応答1/end3/OFF応答3を読み戻し、ready/completeとcanonicalが引き続き成立 |
| 独立レビューの元再現 | 固定実装で新規PTY、1.15秒採取・fsync後に通知0回、transport/causal completeともfalse、unexpected_begin=1、保存欠落0。canonicalも不完全 |

元再現の生JSONLとsummaryはtaskの `gap-state-evidence/distinct-final/`、集約ログは `gap-state-*.log` に保存した。late beginが有効な準備通知の後に初めて届く試験では、その過去1通知を保持し、到着後のstatusと最終判定をfalseにする。未来の異常を事前に予知した扱いにはしない。

正常、同seq/異seq/低seq・異catalogの追加begin、先行fact後の初回begin、別boot、end先行/重複、境界/OFF欠落、不正OFF、partial、再anchor、ABORTEDの再開拒否、32件prelude overflow、前後区間除外を検査した。正常な前後区間除外・単一開始では完全性が成立し、全拒否で代用していない。

## 変更範囲と継承

変更はhostのevent_stream.py・event_causality.pyと試験のみ。firmware C、TCP/COPY表、AP2 hook、server source、設定・bindingは未変更。以前の2世代の全ARM ELF SHAと未変更source SHAを再照合した。ARM・server release・175 host項目・長時間Rust試験は今回は再実行せず、固定SHAの既存結果を継承する。従ってLCDの静的RAM251,064B、リンカheap可能11,080B等も前回候補の値であり、新しい実機計測ではない。

実機・USB open/command・SSH・サービス操作・pushは行っていない。全検証処理は終了し、最終失敗・権限待ちは0。物理遅延改善、RF、動的heap/stack、最初の観測以前と厳密なraw→WS対応が未検証/unknownである限界を維持する。
