# 正式USB操作API + 200 ms案内のソフトウェア検証

仕様先行 `1e622a9` / `170864a` / `fc25aac`、固定実装 `98fc95fa3cb6e8b1a94215b72621ab7086be4a3e`。独立実装確認・親受入れ・実機適用は未完了である。受入済みの200 ms案内だけの候補へ新APIの承認を流用しない。

## 実際の経路と変更範囲

GPIOはbindingデータからgestureを意味上の`pm_operation`へ変換し、通常USBは`INPUT DO`を同じ値型へ変換する。両者はmain threadの`perform_operation` → 純粋な`pm_input_decide` → 既存`apply_local_input`を通る。未知値・能力なし・busyは画素保持/owner/ネットワーク等の副作用より前に返す。USB parserは状態判断や描画を所有しない。legacy SIMも同じ意味上の実行へ合流し、物理入力の長押し再現は診断adapterに残す。

GPIO数と操作能力maskは独立したデータである。0 GPIOでも四つの正式操作が利用できる。処理は個数で分岐せず、既存2/4個の対応表と任意1/3/5個の試験データを同じbinding処理に渡す。通常USBのPRESS/RELEASEは未知語彙として拒否する。将来WEBを接続できる操作APIが中核だが、今回はWEB入口を追加しない。

通常USBは診断OFFでも既存の単一reader/有界serializerを使う。TRACE OFFでは計測producerを作らず返信用の2行queueだけを保持する。TRACE ONの従来queue/EV1は保持する。正常系の`dispatched`は既存操作へ渡した事実であり、TCP ACK/サーバ受領/描画完了の意味ではない。

`INPUT CAPS`はboot/能力/GPIO数/`last_request_id`を返す。`INPUT DO <boot> <request-id> <SUB|UP|POWER|DOWN>`の数値は16進、IDは1以上。直近同ID同操作は保存結果、同ID別操作はconflict、過去IDはstale。再接続しても直近結果を保持し、UINT64_MAXはwrapしない。不完全な行を切断時に破棄し、自動再実行しない。

200 ms案内、取消し→再要求の受入済み修正、Nagle無効化、LCDの私有PR155 overlay、個別設定は基点から保持する。GPIO/USBのどちらも既存の単調時刻/案内モデルを使う。SDK原本、送信制御、観測機能、サーバは変更しない。

## 独立設計反例と実経路回帰の対応

反例正本は作業結果領域の`independent-design-review-20261022/counterexamples.json`。以下は実装者の確認であり、独立実装受入れの代用ではない。

| 反例 | 恒久試験と確認範囲 |
| --- | --- |
| C01 / C03 | `normal_usb_api_test.c`、診断全OFF、3種×TRACE 0/1、0 GPIOでも四操作 |
| C02 | 同試験AP2構成、ARM/FREEZE/RELEASE後の同ID再送で送信増加なし |
| C04 / C07 | `button_bindings_test.c`任意1/3/5対応表・保持閾値、既存`input_test.c`の時間wrap |
| C05 / C06 | 同binding試験と既存`input_test.c`/`wake_test.c`、2/4個のpress/release/35 ms/長押し |
| C08 / C09 | `run_usb_tx_tests.sh`のlegacy取消し/物理優先/expiry、normal試験のPRESS/RELEASE拒否・保持GPIO切断 |
| C10 / C17 | normal試験の実GPIO POWER→USB UP、STANDBYの意味上POWER復帰 |
| C11 | normal試験の不正型/boot/ID/能力拒否とretained/owner不変、`device_contract_test.c`の全状態表 |
| C12 / C18 | normal試験の再送/conflict/stale/CAPS再接続/UINT64_MAX、送信失敗/ignored/busy結果 |
| C13 | normal試験のpartial/zero write、full queue、長行/不正byte/切断回復とlegacy回帰 |
| C14 | normal試験E-ink standby drain busy・後追い実行なし、既存SIM expiry |
| C15 | 実GPIOとUSBのwire操作比較、198/200 msの完成画像CRC受信で未開始案内抑制 |
| C16 | normal試験E-inkの取消し→新SUB→再要求、`switch_feedback_cancel_test.c`両個体×TRACE 0/1×3経路 |

`input_types_test.py`は正規factoryの成功と、生整数/不正初期化/異なるcallback型のコンパイル拒否3件を確認する。既存API結果・動画/EOF/復帰・EV1境界も保持する。

## 完了済みの実行結果と限界

保存先は`/home/tane/Documents/Codex/2026-10-18/task/picomado-usb-input-20261022/results/`。

- `host-verified.log`: host一式成功、先頭`PASS`行221件（内部scenario数とは別）。
- `usb-domain.log`: 既存USB/SIM/TX診断回帰成功。
- `contracts.log` / `contracts-sanitized.log`: EV1契約回帰とASan/UBSan実行成功。
- `sanitized.log` / `run-sanitized.sh`: 正式操作/型判断/GPIO/200 ms取消しとLCD/E-ink adapter、20 binary・28実行成功。
- `api-result.log`: 3構成の実main parser/API結果回帰成功。
- `host-final.log`は追加AP2 fixtureで失敗した記録を保持する。fixtureが既存AP2 exportの25 ms間隔を進めなかったためで、30 ms進行と正規digest RELEASEへ修正後`normal-usb-ap2.log`とhost一式が成功した。製品の判定/間隔を緩和していない。

root check、3個体の正規ARM build、設定/SDK/overlay/UF2 sector/実リンク/RAM検査の結果とhashは同結果領域へ保存する。build receipt/candidateは各`firmware/build/<device>/`に正規入口が作成する。候補の独立受入れ前にreview済みと記録しない。

実機の入力応答、視認性、ちらつき、CPU/IRQ時間、USB実hostとの往復は未検証。単なる返信成功を画面切替成功とせず、実機担当が同一要求から対象画像の到着/描画までを確認する。今回のAPI追加はパケット欠落の根治を主張しない。
