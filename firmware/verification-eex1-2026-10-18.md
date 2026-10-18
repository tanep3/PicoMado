# LCD EEX1実装・転送確認（2026-10-18 UTC）

完了時刻: 2026-10-18T15:09:18.795198+00:00 / 2026-10-19T00:09:18.795198+09:00。対象PicoMado-lcd1、USB Flash ID E661640843239D21。ユーザー承認に基づき、完成候補を**一回だけ**転送した。

## ソフトウェア確認

- ホストrunner43実行ケースが全通過。PASS出力は49行で、先行の「49件」という報告は出力行数だった。24個のAndroid担当再現codecベクタを含む。
- EEX1 framing/範囲、D/O/A/R/K、tuple/nonce、AとRの別確認、重複O、TTL、ACK欠落、アプリ後起動、DHCP変更、失敗時再探索、検証済みendpointのTCP再利用、HOME/PM/既存描画経路をmockで確認。
- codec/交換状態機械/clientはASan・UBSan通過。runnerのptrace制約でLeakSanitizerは使用できず無効化した（新コードに独自mallocはない）。
- 既存SDK2.3.1でReleaseビルド成功、警告0。PM初回要求・一回の読戻し・cached USB表示を維持。start_wifiはendpoint reset hookを除く本文が従前と一致し、PM関数2つは完全一致。

## 候補と転送

- UF2 SHA256: `7cdc47bfa596ee9459e49e6db6a957b55f2ebf0a84083416fc70fc840e5cdf80`
- UF2: 773632B。text392764B / data0 / BSS230872B。
- 直前の実機受信診断版b22041e0...からtext+9128B、固定RAM+2708B。既存受信診断3728Bも維持した。
- リンカの__end__→__HeapLimit余裕26032B。実行時free heapの実測ではない。
- 最大の関数単体stackはmain888B、新EEX関数最大368B。全.su項目static。4096Bのmain stackに対する実行時high-water/IRQ込みの最大は未測定。
- buildはPM_AWAY_EEX1=1 / PM_EEX1_PORT=48116 / PM_NORMAL_BUILD=1 / PM_TRACE_STATS=1 / PM_TRACE_VERBOSE=0。既定はOFFで他端末buildを勝手に有効化しない。
- flash.shが一台のみのBOOTSELとID一致、UF2manifest/ハッシュ/PicoWを照合。load -v -xはexit0、Flash検証100% OK、正常起動を確認。

## 自動ログで確認した実機範囲

転送後に5秒USB取得を2回行い、両方で完全snapshot・DTR低下・FD閉鎖を確認。ボタン・テザリング・アプリの操作は当方で行っていない。

- AWAYでIP取得成功。Pico IPv4は10.244.95.120、gateway10.244.95.133、netmask255.255.255.0。
- EEX1の受領と結果のKを確認: receipt=1 / result=1、verified=1、UDPは終了済み(udp_open=0)。candidate10.244.95.133:48115、報告UDP port48116、tx_error0。
- 既存SOCKS METHOD/AUTH/CONNECTが全てresult1。PicoはACTIVE/READY。認証を省略していない。
- PM読戻しraw0x10/mode0/none1、SDK戻り値0を維持。
- Pi APIでもLCD online / selected youtube。受信・描画ACKのframe IDが421188→421249へ更新。これは最新IDの進行であり、差61を描画枚数や実効FPSとは扱わない。
- E-ink2台はoffline。端末選択/有効化・共通順序・動画/FPS設定の書込みはしていない。サーバ/VPN/ルータ変更・再起動・SSHなし。

この実機では、案内されたproxy IPと今回取得gatewayが一致した。一般に常に一致すると保証する結果ではない。情報はEEX1 Oから選択し、gatewayを無条件にSOCKS接続先へ固定する実装ではない。

## ユーザー操作で残す確認

1. Android画面上で対象端末の受領とSOCKS結果が表示されること。
2. 案内アプリ終了後の継続、同一Wi-FiでのTCP再接続・接続先再利用。
3. テザリング再開/ネットワーク切替後の再探索と取得し直し。
4. 動画の長時間品質、以前の自宅経路の停滞との180秒比較。

今回の短い自動ログを上記の全合格や動画性能改善の証明とはしない。追加BOOTSELを反復せず、必要な実機操作はユーザー本人が行う。

## 記録の所在

作業フォルダ `endpoint-preparation/` のfinal-host-tests.log、final-sanitizer-results.json、final-sdk-result.json、final-candidate-analysis.json、flash-result.json/flash.log、post-flash-followup-summary.jsonに根拠を保存。UF2/実端末config/生USBはGit管理外。参照仕様・出典の限界は[接続先交換の説明](../docs/eex1-endpoint-discovery.md)。Android仕様は担当者の文脈から再現された引継ぎで、当方が元commitから抽出・照合したものではない。
