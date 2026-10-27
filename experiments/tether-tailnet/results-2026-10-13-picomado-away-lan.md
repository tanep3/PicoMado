> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# PicoMado AWAY → TailSocks → LAN宛先の実機検証

日付: 2026-10-13。以下の時刻はUTC。対象はLCD端末 `example-lcd` １台と、その時点の既存ネットワーク環境。

## 結果と適用範囲

AWAYのTailSocks SOCKS5経由で `192.0.2.120:16153` への接続が成功した。端末CDCのMETHOD/AUTH/CONNECT成功、AWAY/ACTIVE/READYと、RustサーバのWebSocket Hello・受信ACK・描画ACKを同時刻で照合した。**本環境では既存LAN listenerだけで成立し、追加Tailnet listenerは不要。**

LAN宛先への到達は、本環境で既に利用できるsubnet routingに依存する。Tailnetに参加するだけで全LANへ届くとは扱わない。別環境では対象subnetの広告・承認、TailSocks側の経路利用、アクセス許可、対象IP/portの待受を確認し、同じSOCKS5から実際に接続する。通常のスマホTailscaleアプリとTailSocksは別ノード・設定であり、通常アプリの到達結果やPi自身のrouteだけではTailSocksの経路受入を証明できない。今回の試験は経路広告元やACLの全構成を再監査したものではない。

資格情報・所在地は本記録に含めない。ユーザーの承認によりファームのAWAYサーバIPだけをLAN宛先へ変更した。サーバport16153、Wi-Fi/SOCKS資格情報とLCD条件を保持した。OS・ルータ・Tailscale/スマホのネットワーク設定、他サービス、listenerを変更していない。接続試験中のSSH、commit/pushなし。

## 使用版

- ファーム: `example-lcd-away-lan-test.uf2`、SHA256 `c37ce90bee98b009f249679acdaaaff30210f83682e2ccbb8a8f8ddec83f7c8a`。05:06:46にFlash load/verify100%完了。実config・UF2・生ログはGit管理外。
- Rustサーバ: ACK競合修正版。95試験通過、fmt・clippy・releaseビルド成功。05:05:57に `picomado.service` のみ再起動、PID `1069457`。導入SHA256 `6bbf1a1978c1929c6ad50c0f3e64bf5a511aaf43986aa0190baeb2f50df171ae`。
- 既存待受: `192.0.2.120:16153` のみ。Web `/` と `/api/state` はHTTP200。端末incarnation3、設定改訂1、利用設定改訂4を保持。

## 時刻付き照合

| 時刻 UTC | 端末CDC | サーバ | 判定範囲 |
| --- | --- | --- | --- |
| 05:06:48–51 | AWAY開始、IP取得後にTCP ERR_RST(-14) | 新Helloなし | 初期失敗。subnet経路の不成立とは断定しない。 |
| 05:15:57–16:00 | METHOD/AUTH成功後peer close、CONNECT成功なし | 新Helloなし | 認証成功とアプリ接続成功を区別する。 |
| 05:16:30–31 | METHOD/AUTH、05:16:30.904 CONNECT成功 | Hello/session35、画像217の受信・描画ACK | SOCKS・WebSocket・画像ACKまで成立。 |
| 05:16:45–46 | AWAY/ACTIVE/READY後に一度切断 | Power入力直後、session35終了 `read_error` | ACK拒否・送信stallはなし。端末操作との因果は全ログで評価する。 |
| 05:16:52–56 | AWAY再決定、Wi-Fi/IP・METHOD/AUTH/CONNECT再成功 | Hello/incarnation3/session36、画像218の両ACK | 同環境で再接続と画像ACK成立。 |
| 05:17:00–07 | AWAY/ACTIVE/READY、IP維持 | DOWNで雨雲→天気、画像219/220の両ACK | 操作と別画像配送の成立。 |
| 05:18:11 | 後続CDCでもAWAY/ACTIVE/READYを維持 | session36 heartbeat | この観測窓で継続。長期安定の合格を意味しない。 |

05:17:45のAPIはonline=true、last_received_frame=220、last_rendered_frame=220。サーバTCPの接続元は `192.0.2.160`、宛先は `192.0.2.120:16153`。接続元だけでTailSocksのidentityや経由全ホップを断定せず、AWAYのCDCと照合する。

## ACK修正の実機証拠と残件

- 05:13:09、05:13:21に、表示世代が更新済みの旧receipt ACK（frame200/210）が `ack_ignored` となった。session34は切れず、後続入力を処理した。旧実装では同じRule no-opを `stale_frame` として接続終了する。これは遅延ACK修正が実端末の経路でも働いた証拠。
- 最後の画像片送信中にeffectが取消済みになる競合は、旧実装で失敗・修正版で次画像配送まで成功するin-process試験で確認。今回の実機ログだけでは、その競合が実際に起きたとは断定しない。
- ACKは端末側の受信完了・SPI描画完了通知。実画面の全表示品質・視認性をACKだけで証明しない。
- HOMEの全操作中再接続が解消したとは断定しない。過去の診断不足の切断は未帰属。長時間接続、回線切替・スマホ省電力後の復帰、動画帯域、複数実機、E-inkとBLKの受入は残る。

- BLK接続後も電源OFF時に発光が残るとのユーザー報告あり。ファーム担当が接続済みフラグ0によるバックライト関数の即returnを確認した。修正は疎通checkpoint後、実機受入待ち。

## 最後の一回のセッション確認

確認時刻 `2026-10-13T05:31:15.577854+00:00`。サービス・設定を変更せず、最新ジャーナルとAPIを一度取得した。HTTP 200、example-lcd online=false、最新session=36、selected=weather、受信ACK=None、描画ACK=None。最新sessionの終了ログは1件。最後のheartbeatは05:22:56 UTC、05:23:11 UTCに `read_error` で終了した（接続時間375,199ms、終了直前のidle105ms）。この終了にはACK拒否・送信stallの記録はない。端末操作との因果は未断定。約6分15秒の継続と、その後の終了を確認した結果であり、長期・全操作の受入完了とは扱わない。

## 証拠の参照先

- [接続技術ガイド](../../docs/guides/home-away-connectivity-technical-guide.md)
- [実機検証の経過](../../firmware/verification-2026-10-12.md)
- [2026-10-11の別実験](results-2026-10-11-tailsocks.md)
- ファーム担当の照合報告: 作業領域 `task-5/away-lan-physical-test.md`。CDCの資格情報は非出力。
- サーバ診断: 作業領域 `task-4/connection-review/post-install-device.log`。受信内容・認証値を含めない診断行だけを抽出。
