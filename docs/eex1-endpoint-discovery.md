# AWAY接続先のEEX1取得

Androidアプリは別プロジェクト `/home/tane/dev/Android/endpoint-exchange`（SuperTane WSL、引継ぎcommit09f111350773547115ef3ded09f4e54dd913d136）。PicoMadoにはクライアントと参照契約だけを置く。

## 実装範囲

`PM_AWAY_EEX1=ON`のLCD候補は、HOMEのdirect TCPを維持し、AWAYだけEEX1でSOCKS接続先を取得する。build既定はOFFで、他端末の設定・有効プラグイン・選択・共通並び順は変更しない。Wi-Fi/SOCKS認証情報とPi宛先は既存profileのまま使い、取得IP/portだけを今回の接続へ適用する。自動取得値はRAMのみで、手動設定やflash保存値を上書きしない。USBは引き続きログ専用である。

取得はUDP観測(adapter)、純粋なcodec/交換状態機械、endpoint選択、TCP/SOCKSの副作用に分離。`endpoint_policy`はSDK非依存の固定/Gateway/Exchange契約。今回mainへ接続する方式はExchangeであり、gatewayをSOCKSサーバと同一視しない。gatewayは探索Dの送信先候補として使い、limited broadcastにも同じDを送る。

初回はIP取得→ENDPOINT→D/O→A/K→SOCKS認証/CONNECT→R/K。通知アプリは後から開いても120秒の探索中に応答できる。探索は0.5秒から×1.7、最大5秒、0.85〜1.15のjitter。A/Rは別々に最大1.5秒、300ms毎に再送し、AのKが無くてもprobeへ進む。

SOCKS認証・CONNECTまで成功したendpointは同じWi-Fi接続世代のTCP再接続に再利用する。offer TTLは交換/probe開始期限であり、成功済みRAM cacheの期限ではない。Wi-Fi開始/切断、IPv4/gateway/netmask変更で無効化する。prefix等が変わらない通常DHCP更新やTCP再接続だけでは再探索しない。未検証候補で失敗した場合は既存retry後に新nonceで再探索する。受領済みだけの候補はTTLを越えて新probeを開始しない。

SDK UDP受信callbackは最大512Bを2固定slotへコピーしてpbufを解放するだけ。mainが最大4datagram/loopを解析する。TCPや描画のcallbackに探索処理を混ぜず、完了後はUDP PCBを閉じる。ネットワークfingerprintは250ms間隔で確認し、新しいIRQ/alarm/timerは予約しない。

## Wireの出典と互換

下記はAndroid担当者が既存作業文脈から再現して親経由で渡した仕様。元SPEC.md/commitから再抽出した資料ではなく、当方による母艦アクセスや元ファイルとの照合はしていない。この出典を確定済みartifactと混同しない。受領した24固定codecベクタは`firmware/tests/eex_wire_test.c`に同じ文字列で収録した。ユーザーがAndroid16 REDMI/Windows PCでgateway unicastとlimited broadcastのO/A/R往復を確認したとの引継ぎがある。Pico実機とのEEX1動作は別途確認する。

- IPv4/UDP、探索既定48116（`PM_EEX1_PORT`で変更可能）。SOCKSのTCP port初期48115とは別。
- 最大512B、ASCII、単一SPACE区切り。末尾LF0/1個を許し、送信は1個付ける。
- CR/TAB/高位byte/二重SPACE/先頭末尾SPACE/途中LF/余剰field/未知version,type,serviceを破棄。
- request/offer/generationは小文字hex32文字。clientは英数/`_`/`-`の1〜32文字。
- hostは正規形IPv4または`-`。codecは先頭octet1〜223でloopbackも受理、物理Picoの候補選択ではloopbackを使わない。
- port1〜65535、ttl1〜30、attempt1〜9。十進に先頭ゼロを認めない。

|型|ASCII field列|
|---|---|
|D|`EEX1 D request client socks5`|
|O|`EEX1 O request client offer generation host port ttl socks5`|
|A|`EEX1 A request client offer`|
|R|`EEX1 R request client offer attempt status stage`|
|K(A)|`EEX1 K request client offer A 0`|
|K(R)|`EEX1 K request client offer R attempt`|

Androidのtransaction keyは要求元IPv4/UDP port/request/client。Picoは同じUDP socketを交換中維持し、A/RをOの送信元UDP tupleへ返す。`host=-`だけO送信元IPをSOCKS候補にし、明示hostがあればそれを優先する。O内portはTCP用で、報告UDP portには使わない。

重複Oで新probeや期限延長を行わない。request/client/offer/generation/tupleを照合し、KのA0/R attemptを区別する。Rを受け取ったことからA成功を推定しない。nonceは対応付けであり暗号認証ではない。通信範囲は利用者管理のテザリングと手動起動アプリを前提とし、資格情報を探索電文へ載せない。

許される結果はOK/NEGOTIATED、OK/TARGET_CONNECTED、FAIL/TCP_FAILED、AUTH_REQUIRED、PROTOCOL_ERROR、TARGET_REJECTED、TIMEOUTだけ。**現PCのNEGOTIATEDはnoauth選択成功。認証付きPicoはNEGOTIATEDを送らず、認証＋CONNECT成功でOK TARGET_CONNECTEDを送る。** AUTH拒否/形式不正/途中I/O異常はPROTOCOL_ERROR、CONNECT拒否はTARGET_REJECTED、TCP失敗はTCP_FAILED、期限切れはTIMEOUT。詳細は既存の非秘密USB phaseログで区別する。WebSocket/Welcome/画像ACKはEEX1 v1結果に含まれず、TARGET_CONNECTEDをPiアプリ/描画成功と誤認しない。

Android側のtransaction期限は最大30秒と受付終了時刻の早い方、重複D抑制300ms、同時64件/100packets毎秒との引継ぎ。Picoはclient1件・新nonceの交換として扱い、結果attempt=1を使う。

## ビルド・ホスト試験

```sh
bash firmware/tests/run_host_tests.sh
cmake -S firmware -B firmware/build/PicoMado-lcd1 -DPM_DEVICE=PicoMado-lcd1 -DPM_USE_SAMPLE=OFF -DPM_PREPARE_UNBOUND=OFF -DPM_PREPARE_UNBOUND_PANEL=OFF -DPICO_BOARD=pico_w -DCMAKE_BUILD_TYPE=Release -DPM_AWAY_EEX1=ON -DPM_EEX1_PORT=48116
cmake --build firmware/build/PicoMado-lcd1 --target picomado_lcd -j4
```

ホストは実ネットワークを使わず、24ベクタ/framing、O/K対応、重複/期限、ACK欠落時probe、TTL後完了と成功cache、遅延アプリ起動、DHCP変化、失敗時再探索、HOME/PM/既存LCD/E-ink経路を確認する。SDK実装はSTAのIPv4/UDP socketを明示して送信する。スマホ側VPN経路・ユーザー環境の実動作はホストmockだけで保証しない。

現在実機に入っていた受信診断版b22041e0...のソース差分も保存し、今回の候補で意図せず取り除かない。通常のPM要求/読戻しは維持する。関連データ・UF2はGit管理外の端末固有build/作業記録へ保存し、認証情報をcommitしない。

## 実機確認の順序

ユーザー操作は本人が行う。転送後にAWAY/テザリングの状態を確認し、Android案内アプリを開く。端末の受領、SOCKS TARGET_CONNECTED、PiのWelcome/online/画像ACKを別々に照合する。アプリを閉じた後の動画/接続継続、同一Wi-FiでのTCP再接続、テザリング再開時の再取得はそれぞれ未検証項目として記録する。本作業でルータ/VPN/スマホネットワークを自動変更せず、SSHも使わない。
