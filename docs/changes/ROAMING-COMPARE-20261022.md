# ローミングだけの可逆比較 — ROAMING-COMPARE-20261022

基点はLCD ef953cb6440c5418e0ffeb88779039d2a3df1b2f42c9ed4335d8a2f677a1df61のソース6049a757、文書更新後983721a。既存RADIO INTERFERENCE、正式USB入力、200ms案内、EEX1、PM NONE、Nagle OFF、ACK非依存server、private CYW43/PR155を維持する。実機/USB/flash/サービス/SSHは担当範囲外。候補は自動SETしない。設定適用はhardware ownerが行う。

[公式PR151](https://github.com/georgerobotics/cyw43-driver/pull/151) の固定実装 [889e4ccc cyw43_ctrl.c](https://github.com/georgerobotics/cyw43-driver/blob/889e4ccc892327c5b6c1a83552811d70d84ccba0/src/cyw43_ctrl.c) に従う。roamingはinterferenceと別設定。roam_offというNUL終端名とLE32の値をWLC_GET_VAR/SET_VARでSTAへ渡す。0=enabled、1=disabled。SDK原本と無線firmwareは更新しない。現機roaming値は未確認。[SDK2835](https://github.com/raspberrypi/pico-sdk/issues/2835)のmode4→3改善はroaming OFFの実証ではない。

## 型と責務

roam_offは干渉抑制candidateと別の型にする。GETのrc、raw、範囲内候補を区別。baselineへ採用するのはcanonicalな0/1だけで、他の非zero値をOFFに丸めない。SDKはCDC status/応答長を返さないので、payload一致をRF設定成功とは呼ばずcdc_status=unverifiedを必ず付ける。未返却・部分返却が0初期値に化けないよう、GET要求名の先頭4Bを返却時のrawとして検査する。名前未置換/4B未満ならcanonical値にならない。

純粋なbaseline→固定比較値→保存baselineの判定を既存比較モデルの共通判定へ抽出して再利用する。roamingとinterferenceは型付き境界/保存値を分ける。SDK adapterだけが固定名IOを行い、USB adapterは既存の2枠返信queue、partial serializer、boot/id重複防止を両設定で共用する。任意IOCTL/iovar/数値SETは公開しない。mainがquiet判定後にだけSDKへ入る。callback/IRQ/core1からは実行しない。

## 操作と状態

| 操作/状態 | 処理 |
| --- | --- |
| GET | mainの通常quiet gateで1回読出し。設定せず候補0/1またはunknown/errorを返す。 |
| OFF、baselineなし | GET。失敗/未知ならSETなし。既に1ならalready_offでSETなし。0ならSET前にbaselineをRAM保存し、固定1をSETし、別GETを返す。 |
| OFF、baselineあり | baselineを上書きしない。再GETが元値なら固定1へ。既に1なら再SETしない。別値/STA世代変更なら拒否。 |
| SET失敗/後GET失敗・不一致 | 適用不明としてbaselineを保持。write/read rc、raw、目標、一致を個別に返す。自動retryなし。 |
| RESTORE、baselineなし | SETせずno_base。既定0を捏造しない。 |
| RESTORE、baselineあり | 元GET候補だけをSETして別GET。TCP未接続でもSTA初期化済みなら既存USBから復元できる。STA無効化を挟んだ場合も、元の値へ戻す明示操作としてだけ許可し、OFFの比較継続はstale拒否を維持する。新世代で元値だったと推測しない。 |
| USB/TCP切断のみ | baselineと処理済み返信は保持。未実行pendingだけ取消し。USB再接続でCAPSから次idを取得できる。 |
| STA無効化 | baselineをstaleにする。自動SETしない。明示RESTOREのみ現STAへ元候補を設定できるが、元の比較区間の連続性は失われたと扱う。 |
| reset/電源断 | RAM baseline/cache消失。ホスト保存が必要。再起動だけで復旧済みとしない。 |

GET/OFFは既存radio_allowedと同じgate。RESTOREだけはTCP/link UP/ACTIVEを要件にせず、main/core0・wifi_enabled・非MENU/STANDBY・描画/入力/画像組立等busyでないことを確認する。無線未初期化をensure_upで勝手に起こさない。無線再初期化中など拒否された場合、復元未確認として保持し安全な状態で明示再要求する。干渉抑制RESTOREの条件は変えない。

I/Oは最大500msの同期response待ちを各呼出に持ち、GET+SET+GET全体やlock取得が500ms以内とは保証しない。操作中は入力を重ねず、200ms応答保証を主張しない。自動poll・自動設定・新しい観測機構は作らない。

## USB契約

`RADIO ROAMING CAPS` / `GET <boot>` / `OFF <boot> <id>` / `RESTORE <boot> <id>`だけを追加。radio変異idはINTERFERENCEと共有、INPUTとは別。直前同id/同family/同opはcache再送だけ、別family/別opはconflict。busy確定応答もcacheし、再送でSDKを再実行しない。返信枠を先に確保してからI/Oする。既存INTERFERENCEの行形式は維持。

GETはrawとroam_off_candidateを返す。変異は`radio roam ... op=O|R s=... w=... r=... x=... t=... base=... m=... cdc_status=...`。t/baseはroam_offの0/1でありinterference modeではない。mはpayload一致のみ。全返信は既存144B内。

## 先行検証と適用条件

実main/USB/SDK fixtureで、0→OFF→0、初期1の無SET、未知値・SDKエラー・0〜3B返却、LE32/name/GET_VAR/SET_VAR/STA/lock、失敗後baseline保持、同id再送/別family衝突/古いid、byte分割、queue満杯/partial/zero write/切断、busy/IRQ/未初期化拒否、TCP未接続RESTOREとSTA再初期化後の明示復元を検証する。既存RADIO/INPUT/200ms回帰も実行。新型とinterference candidateの混同はコンパイル拒否で確認する。

LCD通常候補を既存root buildで作り、個体設定、診断flags、private CYW43、UF2予約sector非包含を照合。今回の候補はレビュー待ちであり適用済みではない。更新の再起動でruntimeのinterference mode3が維持される保証はないため、hardware ownerは既存GET/TRY3でmode3を再確認してからroaming比較を行う。PM/Nagle/EEX/server等を同時変更しない。

実機は元GET/比較前通信条件をホスト保存→明示OFF→別GET照合→同条件有限操作→必要時保存元候補RESTORE→GET確認。番号/SDK/未知値/不一致や接続再初期化では比較不成立として保存。RF改善、実復元とCDC成功はホスト試験だけで認定しない。
