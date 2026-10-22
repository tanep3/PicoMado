# 干渉抑制の同一バイナリ比較 — INTERFERENCE-COMPARE-20261022

基点は限定受入のGET `7b363e4cae7cdfc0379be0c4514f512f73a96c3e`。目的は統合する一つのファームで baseline → 3 → baseline を比較できるようにすること。個別ファームを何度も適用しない。EEX1必須化・正式USB・200ms表示・誤SUB修正の統合は別担当。本差分は干渉抑制だけで、接続復旧の動作、入力規則、PM、Nagle、roaming、SDK原本、設定sectorは変えない。今回は文書・実装・ホスト検証までで、実機SET/flash/USB/ネット設定変更をしない。

[公式PR151](https://github.com/georgerobotics/cyw43-driver/pull/151) 固定実装 `889e4ccc892327c5b6c1a83552811d70d84ccba0` は既存公開 `cyw43_ioctl` に GET=0x1a6 / SET=0x1a9（符号化済み）・uint32_t 4B・STA ifaceを渡す。[公式2835の10/20報告](https://github.com/raspberrypi/pico-sdk/issues/2835) はUDPで4→3を改善候補とするが、PicoMadoの低頻度TCP問題と同一原因かは未証明。現在値は未取得。既定4を現機値と決めない。

## 保証範囲

SDKはCDC statusと応答長を公開しない。GETの範囲内payloadもbaseline**候補**であり確定モードではない。SET rc=0と別GETのpayload一致もCDC成功/実無線設定/復旧の証明ではない。全I/O応答に cdc_status=unverified を付け、write/readの戻り値、raw、目標、payload一致を別に残す。自動設定、自動retry、NVRAM/flash永続化、任意IOCTL/任意modeのUSB入口は作らない。

一連のGET+SET+GETは同期待ちを伴う。既存GETのquiet gate（ACTIVE、既描画画像、STA/link UP、切替/入力返信/画像受信/描画/ACK/動画が進行中でない）を全操作で共有。IRQ/core1/未初期化/待機を拒否。parserは要求と返信枠だけ予約し、mainが実行する。IOCTL待ちは1回最大500msだが全呼出上限ではなく、3呼出ならその分待ち得る。応答まで新しいボタン操作を入れない。実行中の新入力に200ms表示は保証しない。

## 小さい状態モデル

mainが所有するRAMに baseline候補 / held / stale だけを保持する。純粋モデルが候補の範囲、前提、結果の意味を定義し、SDK adapterが固定GET/SET、USB adapterが文字列とidempotencyを扱う。

| 操作/状態 | 動作と保存 |
| --- | --- |
| TRY3、baseline未保持 | GET。SDK失敗/未知値ならSETしない。候補3なら差がないと返しSETしない。0/1/2/4ならSET前にbaseline候補を一度保存する。 |
| TRY3、baseline保持 | staleなら拒否。同じbaseline候補が再GETされた場合のみ3へSET。既に3ならSETせずalready_3。別候補ならbase_changedとして拒否。baselineを上書きしない。 |
| SET3後 | rcがエラーでも反映不明なのでbaselineを保持。別GETを行いwrite/read rc、raw、target3、payload一致を返す。エラー/未知/不一致を成功にしない。 |
| RESTORE、baselineなし/stale | SETせずno_baseline/stale。4を捏造しない。 |
| RESTORE、同じSTA稼働区間のbaselineあり | 保存候補へSETし、別GETを返す。一致してもCDC未検証。baselineは消さず、次のA/B/Aや不確かな復旧の再判断に残す。 |
| USB再接続/TCP再接続のみ | RAMを保持。quiet gateが満たされるまでSDKを実行しない。 |
| STA無効化 | 既存wifi_off境界で、保持baselineをstaleにするだけ。接続復旧の動作は変えない。再有効化後の自動再設定はない。stale baselineへ盲目にSETしない。 |
| reset/電源断 | RAM baselineとcommand cacheが失われる。ホスト保存が必要で、再起動だけで復旧済みとしない。 |

## 限定USBプロトコル

既存GETはそのまま。同じsingle owner、2件固定返信queue、部分送信serializerを使う。

- `RADIO INTERFERENCE CAPS` : SDKなし。boot、last_id、last_opを返し、別クライアントも次IDを決められる。
- `RADIO INTERFERENCE TRY3 <boot> <id>` : 3だけへ比較設定する。
- `RADIO INTERFERENCE RESTORE <boot> <id>` : RAMのbaseline候補だけへ戻す。

idはboot内のradio変異操作だけの単調増加hex非zero。INPUT IDとは別。直前の同一id/同一opは保存済み返信を再送しSDKを繰り返さない。古いid/同id別opは拒否。busy等の確定返信も同じidでは再実行しない。未実行pendingは切断で取消し、処理済みcacheとbaselineはUSB切断で消さない。返信枠確保前に変異をしない。

変異返信は既存144B以内の短い行 `radio change b=<boot> id=<id> op=3|R s=<status> w=<write-rc|-> r=<read-rc> x=<raw8hex> t=<target|-> base=<baseline|-> m=<0|1> cdc_status=unverified`。mはAPI返却payloadの一致だけ。busy/context/unavailableでは cdc_status=not_read。SDK rc失敗、unknown、mismatchは区別する。SETされたと断定するverified/success表現は使わない。

## 先行試験

baseline候補0/1/2/4→3→同じbaseline、初期3/未知値/GET失敗の無SET、write失敗・read失敗・不一致でもbaseline保持、再TRY3で上書き禁止、STA無効化後stale拒否、USB再接続保持、同id再送/古いid/衝突/別クライアントCAPS、queue満杯/partial/zero write/切断取消し、quiet gate/IRQ拒否を実main+USB+SDK fixtureで検証する。全応答のCDC未検証表示、GET-only回帰、通常INPUT/200ms表示を維持する。実SDK/無線効果をfixture成功と混同しない。

## 後の実機比較（今回実行しない）

統合版を一度適用後、hardware ownerがquiet時にCAPSとGETを取得して保存する。Aの既存切替試験、quiet時TRY3、Bの同じ試験、quiet時RESTORE、A2の同じ試験。操作列/間隔/位置/AP/サーバ/PM/Nagle/roaming/表示/SDKを固定。baseline候補4なら4→3の候補比較と表現するがCDCは未検証と付記する。設定のAPI整合性と遅延分布の改善を別に評価する。未知/エラー/再初期化/staleなら新規試行を止め、復旧未確認を報告する。先にホストへbaselineとreplyを保存し、USB/電源断で元値を失わない。再起動後の復旧は別判断が必要。
