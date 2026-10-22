# 干渉抑制値の限定GET — INTERFERENCE-GET-20261022

目的は現機の値を得て、一次報告の mode 4→3 比較が成立するか判断すること。現在値は未取得。SET、任意IOCTL、SDK更新、無線再初期化、設定永続化、新しい観測基盤は追加しない。正式USB＋200ms案内、Nagle OFF、SDK 2.3.1 / CYW43 055d642、個体設定を保持する。

## 一次根拠

[公式PR151](https://github.com/georgerobotics/cyw43-driver/pull/151) の固定実装 `889e4ccc892327c5b6c1a83552811d70d84ccba0` は、GET `0x1a6` / SET `0x1a9`（符号化済み）、uint32_t 4B、STA ifaceを使用する。現SDKの公開 `cyw43_ioctl` でGETだけ表現する。専用helperやSDK全体更新は不要。設定は無線全体に作用し、PM・Nagle・roamingとは別である。ヘッダの既定4は現機の実測値ではない。

[2835の2026-10-20保守者報告](https://github.com/raspberrypi/pico-sdk/issues/2835) はUDP試験で干渉抑制4→3の改善、roamingイベント不在を支持する。PicoMadoの低頻度TCP遅延の原因・改善は未証明。今回のGETだけで遅延修復や比較完了とはしない。

## 責務と呼出構造

- 純粋な値/結果: `pm_interference_candidate` は0初期化を無効とする候補値型。raw 0..4の範囲判定だけであり、CDC成功を検証したモード型ではない。結果は unavailable / context / busy / sdk_error / unknown / transport_returned を区別する。SDKが返すrcとpayloadからCDC statusの正否は判別不能。
- SDK adapter: GET専用、固定IOCTL/サイズ/ifaceだけ。出力を `UINT32_MAX` で初期化し、rc!=0は無効、rc==0でも既知範囲外はunknown。mode0は有効な値であり、エラーの代用にしない。
- USB adapter: `RADIO INTERFERENCE GET <boot-hex>` だけを受ける。`INPUT CAPS`で得るbootと照合する。既存single owner/parser/部分送信serializerを使い、診断flagsとは独立。SETや余分な引数は副作用前に拒否。
- USB受付は固定2件返信queueの1枠を予約して要求だけ保持する。parserや一般pollからSDKを呼ばない。mainの既存USB snapshot経路だけが予約要求をサービスする。予約枠は未完成のまま送らず、切断で要求を取り消す。満杯時は新規GETを実行しない。
- mainがWiFi有効・ACTIVE・既描画画像あり・link UPであることを確認する。未初期化/待機/接続準備中は unavailable。既存の switching.waiting、未応答入力、await_selection、standby/SUB取消し待ち、組立中、描画ACK送信待ち、E-ink描画/保留画像/driver busy、LCD動画/描画待ちは busy としてSDKを呼ばない。GETのensure_upで端末を勝手に起こさない。
- SDK adapterはcore0/threadコンテキストだけを許し、IRQやcore1を拒否する。ネットワーク/表示callbackには読出し入口を接続しない。受付と実行の分離を実main経路試験で確認する。

## 値の意味と限界

現SDK `cyw43_ll.c` のCONTROL_HEADER処理にはエラーstatus伝播TODOがあり、rc=0だけで無線内部の成功を保証しない。公開APIは応答長を返さず最小長をcopyする。UINT32_MAX初期化により短いcopyをmode0と誤認しない。非zero CDC statusでもpayloadが0..4なら公開APIは同じrc=0/payloadを返し得る。このため既知範囲の返却値にも必ず cdc_status=unverified を付け、transport_returned / candidate と呼ぶ。現機の確定mode、CDC成功、RF正常性、設定成功、再起動後持続は証明しない。後日SETする場合、現在APIのGET一致だけではCDC成功を保証できないという限界も残す。今回はSETを追加しない。

GETはbus wake/control通信と同期待ちを伴い、現SDKのIOCTL応答待ちは最大500ms（その前の送信/lock取得待ち等の全所要上限ではない）。待ち中に受信callbackが実行される。運用は操作・描画のない時に既存ownerから明示1回だけとし、応答まで新しい切替操作を入れない。周期poll、自動再試行、入力ごとのGETはしない。既存の切替が進行中ならSDK実行前にbusyを返して取消し、後で自動実行しない。ただしチェック後/IOCTL実行中の新たな物理押下やネット受信を将来にわたり排除できず、その間の200ms案内を保証しない。

USB応答は `radio interference b=<hex> status=<...> rc=<int>` とし、SDK実行時は `cdc_status=unverified`、未実行時は `cdc_status=not_read`。rc0では `raw=<8hex>`、範囲内だけ `candidate=<decimal>` を付ける。`observed` や `mode=` は返さない。candidate0と未取得は別である。最大行は既存144Bに収める。

## 先行試験表

| 条件 | SDK呼出し/期待結果 |
| --- | --- |
| 全mode0..4、rc0 | GETのみ、固定ID/4B/STA、transport_returned+同じcandidate、cdc_status=unverified |
| rc負値、rawが0でも | sdk_error、candidateなし、cdc_status=unverified |
| rc0+未知値、出力未変更、1..3Bコピー | unknown、candidateなし、cdc_status=unverified |
| 未初期化/WIFI/RETRY/STANDBY/STA off/link down/E-ink standby排出 | SDK0回、unavailable |
| 有効payload0..4でもCDC status非zeroの可能性 | transport_returnedの候補だけ。CDC未検証を応答へ明示し、成功検証を主張しない |
| 既存の切替/入力応答/画像/描画/ACK/動画が進行中 | SDK0回、busy。操作APIと200ms表示は通常経路を進める |
| IRQ/core1 | SDK0回、context |
| parserへ完全/byte分割入力、一般pollだけ | SDK0回。明示main service後に1回 |
| stale boot/余分な引数/SET/不正文字/長すぎ | SDK0回、構文/boot拒否 |
| pending中切断、返信queue満杯 | 取消し/拒否、後で勝手に実行しない |
| 0/partial USB write、既存INPUTと混在 | 行を混ぜず応答は1回、通常INPUT IDは変更しない |
| SDKエラー後の次のGET、再接続 | 新しい要求として処理、前の値を再利用しない |

既存host/USB回帰と根拠のある境界試験を実施し、E-ink_1を同じ設定・診断全OFFで通常buildする。UF2設定sector非包含と構成SHAを照合。独立レビュー/実機readback/遅延効果は未完了として引き渡す。USB・書込み・サービス・SSHは行わない。

## 独立確認からの限定補正（2026-10-22）

レビュー `interference-get-independent-review-20261022.json` の2反例を取り込んだ。混入原因は、公開API返却値の範囲検査をradioの成功検証と同じ observed/mode の語彙で表現したことと、mainでSDK同期待ちする機能を表示期限と同時実行し得る設計のままにしたこと。範囲検査済みの**候補**と未確認CDC statusを別々に表し、既存の切替状態からGETを拒否する。SDKのstatus伝播改造や新しい非同期IOCTL機構へは拡大しない。実行中に来る新入力の将来予測はできないため、操作なしの明示1回という運用上の限界を残す。
