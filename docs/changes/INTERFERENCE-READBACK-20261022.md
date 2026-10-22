# GET状態ビットとSET設定値の分離 — INTERFERENCE-READBACK-20261022

親からLCD実機のGETが3回 raw=0x84/rc=0となり、現行境界でunknown、TRY3のSETは0回と報告された。担当は実機/USBへアクセスしない。基点 `5b3629df7485be270bbb1f16c403b7f1459379db` から読出し表現だけを直し、hardware ownerの統合版へ小さな差分として渡す。SDK、IOCTL要求、接続復旧、入力、表示機能、SET対象の選択は変更しない。

## 一次根拠と確定/推論の境界

Broadcom著作の[Android公開wlioctl_defs.h](https://android.googlesource.com/kernel/common/+/bcmdhd-3.10/drivers/net/wireless/bcmdhd/include/devctrl_if/wlioctl_defs.h)（公開blob `5468634070bc29a13804401167de591094fb940c`）には、WLC_GET_INTERFERENCE_MODE=211 / SET=212 と、同じ干渉抑制定義群の WLAN_AUTO_W_NOISE=4 / AUTO_ACTIVE=(1<<7) があり、後者の説明は自動処理が現在稼働中であるというもの。[別のBroadcom公開版](https://android.googlesource.com/kernel/msm/+/android-msm-hammerhead-3.4-m-preview/drivers/net/wireless/bcmdhd/include/wlioctl.h)も同じ定義を持つ。したがって0x84はこのABIのmode4と稼働状態ビット0x80に一致する。根拠なく下位4bit/byteへ丸める修正ではない。

[公式PR151の固定ヘッダ](https://github.com/georgerobotics/cyw43-driver/blob/889e4ccc892327c5b6c1a83552811d70d84ccba0/src/cyw43.h#L248-L259)と[実装](https://github.com/georgerobotics/cyw43-driver/blob/889e4ccc892327c5b6c1a83552811d70d84ccba0/src/cyw43_ctrl.c#L436-L440)はuint32_tの4Bを直接呼出側へ返す。GET=0x1a6は211<<1、SET=0x1a9は212<<1|1。現在のadapterは同じ要求/STA ifaceであり、GET引数を変える根拠はない。RP2040はlittle endian、0x00000084は84 00 00 00。byte順を誤れば0x84000000であり、既知値として扱わない。

[公式issue2835](https://github.com/raspberrypi/pico-sdk/issues/2835)の4→3改善は比較の根拠で、0x84の説明やPicoMadoの根因確定ではない。Broadcom ABIを現chip返却値へ当てはめる解釈とCDC成功は別。SDKのCDC status/応答長未検証という限界は残し、全SDK返却候補に cdc_status=unverified を維持する。

## 型と不変条件

1. SET用 `pm_interference_candidate` は引き続き0..4だけ。0x84からこの型を直接作ることは禁止する。0初期化は無効で、mode0は明示変換が必要。
2. GET用 `pm_interference_readback` はSET用候補と auto_active を別フィールドに持つ。raw全体は既存 `pm_radio_read.raw` に保持する。既知のAUTO_ACTIVE bitだけを分離し、残りが0..4の場合だけ復号する。未知bit/不正modeを捨てない。
3. baselineはGET readback型として保存し、RESTOREにはその設定候補だけを使う。稼働状態ビットをSETして再現しようとしない。mode一致とauto_active一致を混同しない。
4. `pm_radio_transport_candidate` はGETの型付き復号から設定候補だけを取得する。TRY3のbaseline保存、既に3の判定、RESTORE後payload比較はこの共通境界を通す。0x83と3、0x84と4は同じ設定候補で、稼働状態は別の観測である。
5. 既知bitを分離してもCDCエラー、RF正常、設定/復元成功を認定しない。rc失敗なら既知形式のrawでもsdk_error。

## 応答と回帰

GETの既存raw/candidateに `auto_active=0|1` を追加する。unknownにはrawだけを残す。変異返信は既存x(raw)/t(設定目標)/base(設定baseline)/m(設定候補一致)を維持し、xの状態bitが変わっても設定候補が一致すればm=1とする。CDC品質は未検証のまま。既存144B行上限を守る。

反例は旧境界が0x84を拒否する実コードを隔離コンパイルして再現し、新版では設定候補4＋auto_active trueと復号する。0x84→SET3→GET0x83、baselineへのSET4→GET0x84/0x04を実main/USB/SDK fixtureで検証し、SET値が0x84/0x83にならないことを確認する。未知bit0x100/0x40、0x85/0x88/UINT32_MAX、byte逆順、短い返却、rc失敗はunknown/errorのまま。通常0..4と既存quiet/ID/再接続境界を維持する。実機再確認と統合ARMビルドはhardware ownerへ引き渡し、ここでは実機/USB/SSHを行わない。

混入原因は、PR151が返すGET表現とSET許容値が同じ0..4列挙だと見なし、返却側の状態フラグを一次ABIまで照合しなかったこと。値域を広げて両者を同一型へ押し込まず、観測と設定を別の型・変換に分離する。
