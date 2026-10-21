# 正式な端末操作APIと入力adapter

基点は `5cff47a`。200 ms切替案内の受入済みworktree/成果物/receiptを変更せず、別worktreeで組み合わせ可能にする。USB/実機書込み/SSH/サービス操作は行わない。

## 目的と確認済みの既存構造

ユーザーは、将来WEBからも端末を操作する中核として、ボタン種別/数をデータ、操作の意味と処理を共通API、GPIO/USB/将来WEBを入力adapterに分けるよう指示した。今回はWEBエンドポイント、ネットワーク公開、認証方式を追加しない。

現行のGPIOとSIMは既に `pm_button_input_step` → `button_with_origin` → `pm_input_decide` → `apply_local_input` へ合流する。この共通判断/実行と200 ms表示を再利用する。SIMは `PM_USB_BUTTON_SIM` のcompile guardで受付/状態が除外され、0物理ボタンではunsupportedとなる。設計書 `usb-button-tx-diagnostics.md` は低頻度遅延を調べる任意診断として既定OFFを明記している。SIMがAP2を必須とするCMake依存はない（TXだけがAP2を要する）。通常の操作入口までこの診断adapterの存在に依存する点が今回の是正対象である。

また既存GPIOの短押し/長押し対応は `core.c` で2/4個による処理分岐に埋め込まれ、共通判断もSTANDBYでボタン数を参照する。ボタン数0と操作能力なしは別概念である。既存serverは四つの操作メッセージを処理し、Helloのbuttons0は操作APIを拒否する判定には使っていない。

## 責務と不変条件

- 正式な操作は `SUB / UP / POWER / DOWN`。生の整数とは区別する値型を使い、不正値を共通API境界で拒否する。現在の3個体は四操作を持ち、GPIO数とは独立した能力maskをデータとして渡す。能力のない操作は副作用なしでunsupported。
- 純粋な操作判断が、型/能力/現在状態/入力可能状態から拒否・無視・実行計画を一箇所で返す。操作の実行、入力通知送信、200 ms案内は既存 `apply_local_input` 経路へ集約する。GPIO数やUSB/診断flagで操作の意味を変えない。
- GPIO adapterはraw押下/解放、debounce、長押し判定を所有する。既存の35 ms debounceと閾値snapshot、短押しの発火時点、長押し後に短押しが出ない動作、STANDBY復帰を保持する。対応表（press/release発火、短/長操作、復帰可否）をデータにし、処理本体は個数で分岐しない。既存0/2/4 profileを登録し、1/3個の試験データでも同じ処理を動かせる。
- 通常USBは完成した意味上の操作を一回要求する。物理ボタンの押下途中を通常APIへ持ち込まない。PRESS/RELEASEの片割れを表す型/commandを通常APIには作らず、不明な動詞として拒否する。押下時間や未解放、長押しを再現する既存SIMは別の診断adapterとして保持し、同じ共通操作APIへ合流する。
- 正常な連打は異なる要求IDの独立した操作。同ID同操作の再送は前の結果を返し再実行せず、同ID別操作はconflict、過去IDはstale、0/不正IDは拒否する。boot照合で別起動の古い要求を拒否。IDの整形/順序はUSB境界、操作可否/状態遷移は共通APIが所有する。
- 結果のdispatchedは共通APIを呼んで既存処理/送信へ渡した事実であり、TCP ACK、サーバ受付、画像到着/描画完了ではない。送信失敗はfailed、現在状態で動作なしならignored。自動再送、待機sleep、独自の画像待ちtimerを追加しない。
- 通常USB受付/返信をAP2/SIM/TX/CYW43/EV1やTRACE計測の有無へ依存させない。既存の単一USB readerと行単位の非待機/部分write serializerを再利用する。計測OFFでも共通の返信輸送は残し、観測producerと分離する。既存診断のwire形式、有限SIM、DTR/run結合、固定RAM/排他は保持する。

| 入力/状態 | 判定と副作用 |
| --- | --- |
| 未知操作、不正/0 ID、別boot | 拒否、操作しない |
| 能力なし / 入力禁止中 | unsupported / busy、操作しない |
| 有効操作だが現状態で対象動作なし | ignored、接続/画面/送信を変えない |
| 新IDの有効操作 | 共通の純粋判断→既存実行、dispatchedまたはfailedを記録 |
| 同ID同操作 / 同ID別操作 / 過去ID | 保存済み結果 / conflict / stale、再実行しない |
| GPIO保持 / 解放 / bounce / 長押し | adapterの既存状態遷移。1操作の多重発火なし |
| 物理ボタン0個 | GPIOイベントなし。能力を持つUSB操作は同じAPIで実行 |
| 通常USBのPRESS/RELEASE | 通常APIの語彙外として拒否。押下状態を残さない |
| USB切断/再接続、返信のpartial/zero write | 未完成行は破棄、操作の自動再実行なし。行のバイトを他producerと混ぜない |

## 通常USB wire（診断SIMとは別名前空間）

`INPUT CAPS` と `INPUT DO <boot> <request-id> <SUB|UP|POWER|DOWN>`。整数は既存制御と同じ符号/0xなし16進64 bit。CAPSはboot、操作能力mask、GPIO数、最後に処理した要求IDを返し、再接続したclientが次のIDを決められるようにする。DOの返信はboot/ID/操作/結果を持つ。bootと要求IDの両方を照合してから実行する。直近1件の結果を有界に保持し、過去要求を再実行しない。既存SIMの高度なjob/hold/切断後実行は変更しない。

USB parserは語彙を型へ変換し、mainが登録した正式な操作handlerへ委譲する。main thread内の同じhandlerをGPIO/SIMも通り、network callbackやUSB IRQで描画/送信処理を実行しない。通常USBのために診断全体をONにしない。

## 検証とPDCA

混入原因は、任意診断として導入した押下シミュレータを通常の遠隔操作APIと混同し、入力adapterの不在を「端末操作の無効」と説明したこと。共有済みの正しい処理は保持し、操作の型/能力/判断と物理入力の対応表を分け、通常USBを独立したadapterとしてそのAPIへ接続する。

型/能力/状態の表から、不正値・未対応操作・無効状態・busy・GPIO/USBの同一操作効果、0/2/4 GPIOとカスタム1/3対応表、press/release/bounce/hold/連打、同ID再送/conflict/stale/boot変更、未知/長過ぎる行、切断/再接続、partial/zero writeを恒久回帰へ追加する。全診断OFFとtrace ON/OFFで通常USBが動き、legacy SIM/AP2/EV1/TXの既存回帰、200 ms表示/取消し/画像優先、動画/世代/EOF/復帰を維持する。

仕様コミット後に限定実装し、host/root回帰と個体別ARM build、設定/SDK/PR155/Nagle/設定sector非包含・RAM/stackを確認する。ここでの結果はソフトウェア検証で、実機の操作・タイミング・表示効果を認定しない。元の200 ms候補と通常書込み資料はそのworktreeで保持し、新候補の承認を流用しない。
