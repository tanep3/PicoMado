# LCD の小さい送信と ACK 待ちを分離する候補

2026-10-19、基準 commit `492a710121c009ee2a32cf5e2abd3443a167b5d0` から隔離した候補。製品コードの変更は `src/net.c` のみで、新しい LCD 用 PCB を取得した直後に `tcp_nagle_disable(pcb)` を呼ぶ。`core.h` の追加は端末の display type を既存 config から取得するため。サーバー、SDK、端末設定、動画 FPS、描画処理、タイマー、ロック範囲は変更しない。

この資料の候補作成・オフライン検証時点では本番反映・実機書込み・新規採取・SSH は行っていない。後続の反映履歴は末尾に追記する。観測用候補にも重ねていない。本候補は通常版ソースからの独立したパッチであり、ACK observer を含まない。

## 根拠と限界

直前の単発実機観測では、同一接続の `rcv_nxt=584278074` に対する ACK_NOW から TCP ヘッダー生成まで、Pico 時計で 1.299200 秒空いた。RX=0、app=434、uncredited=0、window=23360。今回の実描画は約4.35fps、最大描画間隔10.80秒であり、改善していない。観測負荷と環境変動の寄与は未分離。

インストール済み SDK2.3.1 の `tcp_output` は、unacked データがあり、unsent の先頭が MSS 未満1個で、Nagle の送出条件を満たさない場合、送出ループを抜けて ERR_OK を返す。この場合は ACK_NOW があってもヘッダーを作らない経路がある。アプリの小さい status/制御送信と組み合わさる条件として、同じ SDK ソースを直接コンパイルするローカルテストで再現した。SDK のソース修正はしていない。

実機 dump は unsent/unacked/cwnd/NODELAY の状態を記録していない。従って Nagle が実機の当該経路を通ったとは確定しておらず、最初の packet loss も未特定。`tcp_nagle_disable` は実機原因の確定修正ではなく、再現した送出待ち条件をアプリ側で取り除く候補である。

## 設定する時点と対象

`pm_net_begin` が既存 SDK lock を取った状態で `begin_unlocked` を呼び、古い接続を閉じ、新しい `tcp_new_ip_type` が成功した後、keepalive/callback 登録と `tcp_connect` より前に設定する。NULL PCB には触らない。

設定は PCB ごとで、プロセス全体や SDK の既定値には作用しない。SDK の `tcp_alloc` は PCB をゼロ初期化するため、古い接続の値を引き継がず毎回設定する。現在の SDK の `tcp_connect` と SYN_SENT→ESTABLISHED の処理には TF_NODELAY を消す処理がなく、接続前の設定を維持する（この接続内部の判断はソース確認。新テストの connect/radio 自体はモック）。

| 経路・操作 | 候補の設定と確認内容 |
|---|---|
| LCD HOME | server:16153 への TCP 開始前から NODELAY。HTTP upgrade と制御メッセージを確認 |
| LCD AWAY | SOCKS proxy への TCP 開始前から NODELAY。同じ PCB 上の METHOD/AUTH/CONNECT、HTTP upgrade、制御メッセージを確認 |
| HOME↔AWAY・再接続 | 同じ PCB アドレスをゼロ初期化して再使用する試験で、毎回 connect 前に設定済みであることを確認 |
| 割当失敗・connect 失敗 | NULL を操作せず、既存 FAILED 状態と再接続経路を維持 |
| 静止画・heartbeat・status・ping/pong | LCD の同じ接続全体が対象。送信 byte/kind/JSON、マスク、keepalive の既存値を確認。動画画面への切替待ちで設定しない |
| E-ink_0 / E-ink_1 | 設定しない。各端末の新規接続・再接続・HOME/AWAY/制御試験で flags=0 を確認 |
| EEX1 endpoint discovery | UDP 側のコードは変更しない。TCP の宛先決定や資格情報は変更しない |

小さい送信をまとめて待つ条件が変わるため、LCD からの小 packet 数や無線利用・CPU が増える可能性がある。再送、cwnd、受信側 ACK、電波の損失そのものは残る。実機の改善幅・負荷・安定性は未測定。

## 修正前後を比較したローカル試験

`tests/run_nagle_sdk_tests.py` は保存した修正前の `net.c` と候補の実 `net.c` をそれぞれ使い、接続開始時の flags をファイルへ保存する。その値を、同じインストール済み `tcp_out.c` を直接リンクしたテストへ入力する。app のソケット/無線/割当と SDK の IP/netif/pbuf は決定的なホスト用モックで、実機・ネットワーク・USBは開かない。

| 条件 | 修正前 LCD | 候補 LCD | 候補 E-ink |
|---|---|---|---|
| connect 前 NODELAY | 0 | 0x40 | 0 |
| 小さい unacked＋小さい unsent＋ACK_NOW | 出力0、ACK_NOW保持 | データ＋ACK出力1、ACK_NOW解除 | 従来同様出力0、ACK_NOW保持 |
| unsent が空の純 ACK（対照） | 出力成功 | 出力成功 | 出力成功 |

修正前に新しい LCD の期待値を適用する回帰検出テストは、connect 前の flag assertion で意図どおり失敗する。native / ASan / UBSan の比較は全42 step通過（うち意図した回帰検出の失敗2件を確認）。ASan/UBSan は有効、ptrace 環境のため LeakSanitizer は無効。これは CPU 命令の所要時間や実 timer/RTT の測定ではない。

追加の3端末試験を含め、既存 `tests/run_host_tests.sh` 全体が成功（PASS172行）。既存の52種の EOF/error/世代回復、SOCKS split reply、制御応答、描画と受信の所有権の試験を含む。サンプル設定を使った LCD/E-ink_0/E-ink_1 の `net.c` は既存 ARM compiler/SDK flags で警告をエラー扱いとして object 生成に成功した。新しい実機用 UF2 の生成・書込みは行っていない。

再実行例（`--baseline-net` は変更前の保存ソース、出力は隔離ディレクトリを指定）：

```sh
python3 tests/run_nagle_sdk_tests.py \
  --sdk /home/tane/dev/PicoW/pico-sdk \
  --baseline-net /path/to/baseline/net.c \
  --out /path/to/isolated/results
bash tests/run_host_tests.sh
```

## 既知正常版との設定比較

保存された 2026-10-15 の検証記録には、Thriller 12fps の実描画11.996062fps・切断0の180秒観測がある。この記録と完全に同一の SDK/toolchain/binary 設定をソース履歴だけから復元したものではない。

| ソース基準 | Nagle 無効化 | background SDK library | lwipopts | 1秒周期 StreamStatus |
|---|---|---|---|---|
| 5224c30（push導入前） | なし | threadsafe_background | 同一 | なし |
| 8e41fd6（push導入） | なし | 同じ | 同一 | あり |
| 37d8e78（時計・統計追加） | なし | 同じ | 同一 | あり |
| 3830f3c（EEX1） | なし | 同じ | 同一 | あり |
| 492a710（今回の基準通常版） | なし | 同じ | 同一 | あり |
| この候補 | LCDだけあり | 同じ | 同一 | あり |

全基準で Wi-Fi PM none の要求処理があり、現在は読戻しも確認する。統計追加以降は診断処理・メモリ利用が増えているが、その差が悪化の原因とは確定していない。SDK version はアプリ CMake に固定されていない。今回使用するインストール済み SDK2.3.1 の出力関連ソースとヘッダーは試験前後の hash 一致を確認した。

**既知正常版も Nagle が有効だったため、Nagle 設定が後から悪化したという差分はない。** この候補は既知正常版の設定へ戻す変更ではない。現在再現できるアプリと SDK 出力の相互作用を避ける変更として評価し、実機の原因確定・改善とは区別する。

詳細な command / log / SDK hash / source hash / 回帰結果は隔離候補の `validation/` と manifest に保存する。本番適用、実機書込み、追加観測は別の明示指示まで行わない。

## 後続の反映履歴（2026-10-19）

別途ユーザー承認後、通常版に NODELAY だけを加えた LCD UF2 を対象 PicoMado-lcd1 に 1 回 load/verify して使用している。UF2 SHA256 は `a455b5bb75672b187bc0a1d0ccdb8644c99eb881c51ebe9d980ee6d00928fa82`。ACK observer は含まず、既存 PM/EEX/EOF 回復と通常診断を維持する。SDK ソースは変更していない。通常版との全 211 object 比較では net.o 以外が同一であることを確認した。

実機では遅い Hello による稼働 session の置換も観測されたため、NODELAY 単独で通信障害全般を解決したとは扱わない。保存ログの解析とサーバ側受付順ガードは別の変更として [サーバ検証記録](../docs/design/hello-accept-order.md) に分離した。今回のソース保存・コミット作業では再書込み、USB、新規 packet 採取をしていない。既存の端末固有 config、資格情報、生成バイナリ、private 採取物はコミットに含めない。
