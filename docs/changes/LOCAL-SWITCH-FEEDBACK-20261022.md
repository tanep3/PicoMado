# ローカル切替案内（LCD / E-ink、400 ms）

2026-10-22 のユーザー訂正により開始閾値を400 msへ変更した。通常切替とSUBで共通。過去の200 msの再現記録は当時値として保持し、現行境界は399/400 msとする。詳細は[閾値変更](SWITCH-FEEDBACK-400MS-20261022.md)。

基準は `6db62d8`（LCD PR155候補 `77c40a06…90e9` と同じ製品コード）。根治調査とは独立した入力へのフィードバック。既存の[局所/サーバ世代](../history/system-design-draft.md)、[E-ink pending](../design/eink-pending-frame-contract.md)、[採用済み通信](../../firmware/adopted-protocol-and-configuration.md)を保つ。実機・USB・サーバ・SSHは変更しない。独立受入済みE-ink NODELAY差分 `34b4849` も既存成果として継承し、LCDのPR155 overlay・診断flags・個別設定を保持する。

## 要件と意味

- ACTIVEで受理したUP/DOWN（通常切替とサブ内操作）とSUBを対象に、押下受理の単調時刻を記録する。入力通知のCOPY/outputを先に行い、表示のために送信を遅らせない。物理ボタンとUSB SIMは同じ入口を通す。POWER・メニュー・待機操作は既存処理を優先する。
- 押下後 **200,000 µs未満** に対応画像が到着すれば案内を描かない。200,000 µs以上で未到着なら、1行目「Pico窓」、2行目「切り替え中」を出す。通常の速い切替でE-inkの余計な描画・ちらつきを発生させず、遅いときだけ案内するためのユーザー指定期限である。sleepや新しいACK待ちは追加しない。
- 到着は、現session/local ownerの有効なFrameStartに続き全画素がCRC検証を通った時点。header・部分chunk・受信ACK・描画完了を混同しない。到着後は未開始の案内を取り消し、本画像を優先する。既に見えている案内は対応画像の描画まで保持する。
- 新しい押下は最新要求に置き換え、400 msをその受理時刻から数える。表示済み案内を同じ文言で再描画しない。旧世代の画像、旧動画の継続frame、旧応答は最新要求の到着・完了にしない。

## 共通状態と責務

純粋な切替案内モデルは要求/応答の接続内序数、要求時刻、表示世代の下限、CRC済み候補と案内の表示状態だけを持つ。IO・plugin ID・画素bufferを持たない。CRC済み候補の画素は、組立の置換/破棄、videoへのcopy、描画adapterへの移譲に対応する寿命を持つ。組立bufferを上書きするときは、まだ組立内にある候補だけを失効し、video/adapterへ移譲済みの候補は保持する。失われた候補を到着/描画完了に使わない。mainが入力送信→モデル遷移→既存受信検証→描画adapterの順を所有し、network callback内で描画しない。

現サーバは各入力へInputAcceptedまたはPluginPositionを1回返し、同じTCP列の応答順を保つ。既存input_contextのsession/epochと接続内応答序数を使い、連打時はまだ返答されていない要求が残す表示世代下限を保持する。正常な切替はepochを進め、画像を返さないno-opはepochを進めない。FrameStartが応答より先でもCRC済み候補を保持し、下限が確定したら描画する。新しいwire識別子やサーバ変更を追加しない。任意の外部更新と入力を完全なaction IDで因果同定する保証は既存wireにはなく、現行session/owner/epochと応答順の範囲で判定する。

| 入力/事実 | 状態と表示 |
| --- | --- |
| 対象入力を送信受理 | 最新要求・期限を設定。旧動画の表示候補を抑止。描画しない |
| 期限前の対応CRC完了 | 到着済み。案内不要。本画像を既存経路で描く |
| 期限到達、対応CRC未完了 | 案内要求を一度だけ出す。通信・入力処理を継続 |
| 期限後の対応CRC完了 | 未開始の案内を取消し、本画像を優先。描画待ちと到着済みを別に保持 |
| 現在要求の同世代no-op応答 | 無機能SUBやサブ端の操作は完了。案内未表示なら何もしない。表示済みなら保持している元画像へ戻す |
| 連打/旧応答/旧frame | 最新要求の期限と世代下限を維持。古い到着で解除しない |
| 対応画像の物理描画成功 | 案内を完了。既存RenderedAckの意味・経路は保つ |
| 送信/描画失敗、切断、再接続、POWER、standby、local owner変更 | 案内を取消し、既存の接続/待機/復旧表示を優先。新しい再試行タイマーは作らない |

LCD/E-inkは同じ案内要求・取消しを受け、描画速度、画素表現、物理busyはadapterが所有する。固定の2行bitmapを共有し、既存ASCII状態表示の意味は変えない。フル画素bufferを追加せず、既存の表示/組立bufferを使う。案内自体をremote frameとしてACKしない。

E-inkは案内を未開始jobとして合流し、remote画像を最優先する。案内の準備段階は本画像到着時に取消し可能とし、物理refresh開始後だけは既存の中断不能性を守る。開始済みfull refreshが本画像を最大1回分遅らせる可能性は残る（既存BUSY期限内）。新しいpartial/LUTや無根拠な短縮を導入しない。no-op復元も局所表示であり、新しいremote完了ACKを生成しない。

## 検証

純粋モデルと実mainの試験で、199,999 / 200,000 / 200,001 µs、期限直前のCRC完了、headerのみ/部分/CRC不正、期限後到着、連打・応答順・frame先行・CRC済候補の次FrameStartによる置換/破棄・旧世代・旧session、SUB正常/no-op・サブ端、動画継続、送信/描画失敗・再接続を確認する。E-ink実adapterでbusy中の案内、未開始/準備中の取消し、remote優先、物理refresh後の合流、元画像復元、ACK非生成を検証する。LCD実adapterでbitmap・復元とSPI順序を確認する。

既存host/USB-domain/表示変換・V4更新・native wire・世代回帰を保ち、LCD（既存PR155 flags）と両E-ink（通常版、NODELAY）をARM buildする。個別設定/board bindingとUF2設定予約領域非包含を照合する。描画サンプルを目視する。実機の400 ms精度・文字視認性・ちらつき・入力/動画への影響は未検証として引き渡し、書込みは別承認とする。

## P2-01: 案内取消し後の再要求（独立レビュー反例）

独立レビュー `2a5693e` の反例: 旧E-ink画像の描画中にSUB→200 msで案内要求→対応画像CRC完了で未開始案内取消し→候補の次FrameStartによる置換、または次の入力→未到着が継続。adapterは案内を取消した一方、モデルの `hint_requested` がtrueのまま残り、案内を再要求できなかった。両個体×trace有無×2経路の8件で再現。既存188 PASSはこの区間を含まず、受入れ成功ではない。

発生原因は「要求済み」を物理表示中と同じように保持し、外部作用の取消しをモデルへ戻さなかったこと。混入原因はモデルとadapterを各々検証した一方、旧描画中の要求→CRCによる取消し→候補喪失/再入力という合成遷移の回帰がなかったこと。表示済みの同文再描画抑止はadapterが持つ既存責務であり、モデルが取消し済み要求を保持して代行しない。

不変条件: `hint_requested` はモデルが現在adapterへ出している要求を意味し、物理表示済みを意味しない。対応画像が案内より優先されるとき、純粋モデルの取消し遷移とadapterのREPLACEを一つの共通呼出経路で必ず実行する。受信完了・応答で候補が有効になる時・pollのどの入口も同じ経路を使う。SHOWの要求確定もモデルAPIを通す。新しいstate/wire/timerは増やさない。

| 事実 | モデル | adapter / 次の期限 |
| --- | --- | --- |
| 対応CRC画像が保管され到着条件を満たす | hint要求をfalseへ、候補は保持 | REPLACEで未開始/準備中だけ取消し、表示済みは本画像まで保持 |
| 取消し後に未移譲候補の画素を破棄 | 候補だけ失効、取消し済み要求はfalseのまま | 元の押下から400 ms以上なら再SHOW可能 |
| 取消し後にSUB/切替を再入力 | 候補失効、新しい期限、要求はfalse | 新押下の399999 µsでは出さず400000 µsで再SHOW可能 |
| 案内が既に見えている状態で再SHOW | 要求をtrueへ | 同文の再描画は既存adapterが抑止 |
| 対応画像が保持される / no-opが確定 / owner失効 | 各既存状態に従い案内を抑止/完了/取消し | 正常・no-op復元・復旧表示を変更しない |

恒久回帰は独立反例そのものを実main/parserへ取り込み、両E-ink×trace有無×候補置換/次入力に加え、次SUB・期限境界も確認する。実adapterでは旧画像描画中の未開始取消し→再要求、準備中取消し→再要求、表示済みの再要求で余計な更新が出ないことを確認する。旧実装で失敗し、修正後に通ることを保存する。独立レビューの原本・manifest・再現C/ログ等は結果領域 `review-fix-20261022/original-review/` へSHA照合して保存し、以前の候補も保持する。

## 受入済み候補を通常build/flash入口へ接続する限定対応

親 `01a1570e-43cf-74e4-b507-318bfec760c5` は、独立再レビュー（`review-fix-20261022/independent-re-review-20261022/review-outcome.json`）を根拠に、製品実装 `38bd24d` / 最終 `0ced9c1` を受け入れた。実機の視認性・ちらつき・負荷は未検証。実機適用/効果確認は未実施のまま、通常書込みに必要な実build出自とcandidate/親受入れの記録準備のみを委任された。USB/実機書込み/SSH/サービス操作は対象外で、既存flashの `--verify-only` までを行う。

既存build adapter `tools/build_lineage.py` だけへ、既存7 flags (`PM_ACK_PATH_PROBE`, `PM_USB_BUTTON_SIM`, `PM_TX_LINK_PROBE`, `PM_CYW43_INNER_PROBE`, `PM_CYW43_SPI_REAL_SIZE`, `PM_EVENT_STREAM`, `PM_STREAM_DIAGNOSTICS`) の環境変数入力を追加する。値は `ON/OFF` に限定し、未指定はすべて `OFF`、任意CMake引数やshell文字列を受け取る汎用口は追加しない。7値を実configureへ `-Dname=value` で明示し、configure後の既存snapshotに記録された実値との一致を検証してからbuildする。既存のsource/config/SDK/receipt/親受入れ検査は維持し、検証器やflash処理を弱めない。flags依存関係は既存CMake契約に従う。

LCDは7値を `ON`、E-ink両個体は7値を `OFF` として既存 `firmware/tools/build.sh` を使い、fresh build dirで実controlled buildを行う。AWAY_EEX1、sample/unboundは引き続き `OFF`。現在の個体設定とSDK/PR155/Nagleを維持し、生成cache・実compile flags・private LL・UF2全hashと設定sector非包含を受入済み候補と比較する。成果物へreceiptを手書きで追加しない。

最小回帰: 全OFF既定値、7値ON入力、許容外の値の拒否、private fixtureを使う既存root build経路への引数/receipt反映、生成cacheが指定値を変えた場合のbuild前拒否。fixtureは実機成果物の認定とは区別し、実3個体のcontrolled build/照合を別に行う。

親受入れの原レビューは改変せずSHA付きで参照する。新成果物のUF2が受入済み完全SHAと一致する場合だけ、その同一性と本メッセージによる委任を根拠に、既存形式の親受入れを現在subjectと各candidate/receiptへ束縛する。hashが変わった場合は差分・理由を報告し、再照合前に受入済みとしない。通常入口の `--verify-only` はpicotool呼出し前に終了することを維持する。
