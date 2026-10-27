> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# 自宅・外出先接続 技術検証ガイド

更新: 2026-10-13 UTC。対象は新しい Pico W と Pi5 の通信経路。仕様の決定事項は [要件定義書](../requirements/README.md) に置き、この文書は接続方式、実測、再現上の注意を記録する。

## 結論と適用範囲

2026-10-11 は、Pico W → スマートフォンの TailSocks SOCKS5 → Pi5 の Tailnet IPv4 で HTTP POST と持続 TCP の PUSH/ACK を実測した。2026-10-13 は、LCD端末 `example-lcd` の AWAY プロファイルから、同じ SOCKS5 経路で **Pi5 の LAN 宛先 `192.0.2.120:16153`** へ接続できた。端末側の METHOD/AUTH/CONNECT 成功・ACTIVE/READY と、サーバ側の WebSocket Hello・画像受信ACK・描画ACKを時刻で照合した。本環境の既存 subnet routing を利用して LAN 宛先へ到達できたため、追加の Tailnet listener は不要だった。[今回の検証記録](../../experiments/tether-tailnet/results-2026-10-13-picomado-away-lan.md)

自宅 direct TCP もLCD１台でHelloと画像ACKまで確認済み。HOMEの全操作中の再接続問題が解消したとは断定しない。動画帯域、長時間維持、回線切替・省電力後の復帰、複数実機は受入未完了。

この方式で tailnet のノードになるのは **スマートフォンの TailSocks** と Pi5 であり、Pico W 自体ではない。Pico がテザリングで得る IPv4 と、Pi5 の Tailscale IPv4 は異なるネットワークのアドレスである。[TailSocks 公式 README](https://github.com/bropines/tailsocks) は、SOCKS5 プロキシと LAN 上の機器への公開を機能として説明している。

## 経路とアドレス

### 自宅: 直接接続（LCD１台で実測）

```text
Pico W ──自宅 Wi-Fi──> Pi5 の自宅 LAN IPv4:アプリのポート
          Pico 起点の持続 TCP 上で Pi5 が PUSH、Pico が応答
```

今回の既知の Pi5 自宅 LAN IPv4 は `192.0.2.120`。運用時の値を確認する。2026-10-12〜13のLCD端末試験では `192.0.2.120:16153` の WebSocket、Hello、画像ACKを確認した。2026-10-11の A/C 実験とは別の実績であり、HOMEの全ボタン操作・長期安定性の受入は継続する。[実機記録](../../firmware/verification-2026-10-12.md)

### 外出先: 2026-10-11に成功した Tailnet IPv4 宛の経路

```text
Pico W 198.51.100.11 (テザリング DHCP)
    └─SOCKS5 接続─> スマホ 198.51.100.232:48115
                         TailSocks ノード 203.0.113.18
                              └─tailnet─> Pi5 203.0.113.9:18180 / :18182
```

上記は 2026-10-11 の試験値であり、DHCP アドレスやプロキシの待受値は毎回確認する。`198.51.100.232` はこの時のスマホのテザリング側アドレス、`203.0.113.18` はユーザー申告の TailSocks ノードアドレス、`203.0.113.9` は Pi5 の Tailscale IPv4。Pi5 は A/C の接続元を `203.0.113.18` と観測した。SOCKS5 は Pico が指定した **TCP 接続**をスマホが中継するもので、テザリング子機の全通信を自動で tailnet に通す設定ではない。今回の実装は宛先を IPv4 リテラルで指定し、DNS を使っていない。

### 外出先: 2026-10-13に成功した LAN IPv4 宛の経路

```text
example-lcd (AWAY / テザリング)
    └─SOCKS5認証・CONNECT─> スマホのTailSocks独立ノード
        └─既存subnet routing─> Pi5 192.0.2.120:16153 /device
            WebSocket Hello → 画像片 → 受信ACK / 描画ACK
```

サーバの待受は既存の `192.0.2.120:16153` だけで成功した。この試験で route の広告・承認・受入、ACL、OS、ルータ、スマホのネットワーク設定を変更していない。**Tailnetに参加するだけで全LANへ到達できるという意味ではない。** LAN宛先を使うには、対象subnetの経路が既存環境で広告・承認され、TailSocks側で利用でき、アクセスが許可されている必要がある。別環境では同じTailSocksから対象IP/portへの実通信で確認する。

通常のスマホTailscaleアプリとTailSocksは独立したノード・設定として扱う。通常アプリでのLAN到達やPi自身のroute出力だけでは、TailSocksの経路受入を証明できない。今回の証拠はAWAYの端末CDCと同時刻のサーバセッション・画像ACKである。サーバは接続元 `192.0.2.160` を観測したが、これだけでsubnet routerの全構成や経由ホップを断定しない。

## 試験の証拠

2026-10-13のWebSocket・画像ACK・ACK競合修正の実機証拠は[今回の検証記録](../../experiments/tether-tailnet/results-2026-10-13-picomado-away-lan.md)に記載する。以下のA/B/C表は2026-10-11の実験結果である。

すべて小さな `trial-001` 付きデータで検証した。詳しいログは [TailSocks 成功記録](../../experiments/tether-tailnet/results-2026-10-11-tailsocks.md) と [実験手順](../../experiments/tether-tailnet/README.md) にある。

| 経路 | 操作と ACK | 2026-10-11 の結果 |
| --- | --- | --- |
| A | Pico 起点 HTTP POST → Pi5、Pi5 が `ACK A trial-001` | SOCKS5 認証と CONNECT 成功、Pi5 が POST を受信し Pico が ACK を確認。 |
| B | Pi5 が Pico のテザリング DHCP IPv4 へ**新規** TCP 接続し PUSH | プロキシ成功試験では再実施せず。先行する直接接続の一回はタイムアウト。逆向きの経路が必要。 |
| C | Pico 起点の持続 TCP 上で Pi5 が `PUSH C trial-001`、Pico が `ACK C trial-001` | SOCKS5 CONNECT 成功。Pi5 が Pico の ACK を確認し、アプリデータの双方向性を実証。 |

B の不成立は C の PUSH を否定しない。C が維持できれば同じ接続の上で Pi5→Pico のコマンドや応答を送れる。ただし C の短時間成功は、スリープ後、携帯回線の切替後、長時間の接続維持を保証しない。

### 先行する直接接続試験

- スマホ Tailscale OFF、Pi5 自宅 LAN `192.0.2.120` 宛: Pico の Wi-Fi/DHCP は成立したが A/C の TCP 接続は `ERR_ABRT`、Pi5 への到達と ACK は観測なし。[記録](../../experiments/tether-tailnet/results-2026-10-11.md)
- スマホ Tailscale ON とユーザーが申告した条件で、同じ Pi5 自宅 LAN 宛: 受信側待受を先に開始した再試験でも A/C は `ERR_ABRT`、到達なし。別の再起動では Wi-Fi 自体が失敗したため通信試験は成立しなかった。[記録](../../experiments/tether-tailnet/results-2026-10-11-tailscale-on.md)
- 同じくスマホ Tailscale ON と申告された条件で、Pi5 Tailscale `203.0.113.9` **直接**宛: A/C 各 3 回が TCP 接続段階で `ERR_ABRT`。Pi5 に着信なし。[記録](../../experiments/tether-tailnet/results-2026-10-11-pi-tailscale-ip.md)

これらは各条件での到達失敗を示す。Android テザリングと VPN の転送挙動、ルート、フィルタのどれが原因かまでは切り分けていない。「スマホ自身が Pi5 に接続できる」ことから「テザリング子機も Pi5 に接続できる」とは推定しない。B の宛先は Pico の **その回の** DHCP IPv4 であり、Pi5 側に逆経路が要る。先行 B 試験では Pi5 の経路が自宅ルータ向きで、一回の接続がタイムアウトした。ルートを変更して通した結果ではない。

## TailSocks / Headscale の再現ポイント

以下は今回のスマートフォン操作で使われた設定の記録であり、アプリの全バージョンへの手順保証ではない。使用版は TailSocks v4.9.1、Headscale はユーザー申告で Docker 上の 0.28.0。スマホはユーザー申告で Xiaomi REDMI Note 15 Pro 5G、Android 16 / HyperOS 3.0.302.0.WPPJPXM.C07。版の存在は [TailSocks v4.9.1 リリース](https://github.com/bropines/tailsocks/releases/tag/v4.9.1) でも確認できる。

1. TailSocks の Headscale 用 **LoginServer** に自身の Headscale URL を設定し、SOCKS5 を使う **Proxy mode** で起動する。登録後は TailSocks が独立したノードとして Headscale に現れることと、Pi5 へ到達できることを確認する。Headscale の URL や登録キーは本書に記入しない。
2. Headscale の preauthkey は、登録先のユーザー ID を確認して、そのユーザー向けにローカルで作成する。今回の運用ではユーザーを数値 ID で指定した。キーは既定で **1 時間有効・1 回使用**。[Headscale 公式手順](https://docs.headscale.org/usage/getting-started/) にある。表示された実キーは端末内だけで扱い、チャット、文書、リポジトリ、ログに載せない。Headscale 管理 API キーはこのノード登録には不要。
3. 今回は TailSocks Main 画面の **Use a key** から preauthkey を入力し、起動し直した後に TailSocks ノードの登録と通信を確認した。設定画面に戻った時は Headscale の LoginServer が読み戻されているか確認する。**Login** をキーなしで選んだ試行では公式 Tailscale 側のログイン URL が出たため、Headscale 登録成功とは扱わなかった。TailSocks の [公式 README](https://github.com/bropines/tailsocks) は、ログイン URL が有効な間は設定更新を止める設計を記述している。今回の表示との関連はあり得るが、内部原因は実測から確定できない。
4. **Settings → Local proxies** で LAN へのプロキシ公開を有効にし、SOCKS5 の待受ポート、ユーザー名、パスワードをローカルで設定する。今回の接続先はテザリング側 `198.51.100.232:48115`。TailSocks の [公式 README](https://github.com/bropines/tailsocks) は LAN 公開時の SOCKS5 パスワード設定を促している。HTTP プロキシとローカル DNS はこの試験では不要。DNS の `192.0.2.160` やポート `1053` はこの通信に使っていない。
5. Pico のローカル設定は [空の設定サンプル](../../experiments/tether-tailnet/test_config.h.sample) から作り、SSID/パスワード、SOCKS5 認証情報を**手元だけ**で入力する。`test_config.h` とビルド成果物は Git 管理外。サンプル、文書、チャットに実値を書かない。SOCKS5 のユーザー名/パスワードはプロキシ利用の認証であり、アプリ内の論理 `device_id` の代わりにはならない。

TailSocks に既に接続したスマホ上の通常 Tailscale アプリだけで、テザリング子機が tailnet に到達するという前提は置かない。今回成功したのは、Pico のファームが明示的に SOCKS5 へ接続し、認証と CONNECT を行う経路である。

## 製品への取り込み方針と未決事項

- Wi-Fi の接続先と Pi5 への経路を一組のプロファイルとして扱う案。自宅プロファイルは自宅 SSID と Pi5 LAN への直接 TCP、外出プロファイルはスマホのテザリング SSID、TailSocks のテザリング側 SOCKS5、同ノードから到達を確認した Pi5 宛先を持つ。本環境では既存 subnet routing で Pi5 LAN IPv4 を利用できた。Tailnet IPv4を使う別構成では、その宛先のサービス待受も確認する。各プロファイルのアドレスは現地値に依存する。
- アプリ層では Pi5 と Pico の間に一つの双方向セッションを設け、接続時の `device_id`、メッセージ ID、ACK、再送・再接続を共通に扱う案。SOCKS5 認証情報だけでデバイス識別やアプリ認証を済ませたことにはしない。
- 家／外出先プロファイルの手動選択、選択中プロファイルでの再試行、サーバ不達時の表示と本体設定操作は[要件定義書](../requirements/README.md)で決定済み。試行間隔・設定保存方式などは U-18、セッション復帰の実装詳細は U-02。HOME操作の通信安定性、外出経路の長時間維持、スマホの省電力・再起動後の復帰、映像配信の帯域・遅延は追加検証が必要。

このガイドと実験プログラムは既存の YouTube プレーヤーおよび E-ink 端末を変更しない。Pi5 の一時受信プロセスは試験後に終了し、常駐設定やルート・VPN・ACL 変更は行っていない。
