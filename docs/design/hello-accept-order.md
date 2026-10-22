# 古い接続の遅延 Hello によるセッション置換の防止

これは `492a710121c009ee2a32cf5e2abd3443a167b5d0` を基にしたサーバ側変更。反映履歴は末尾に記録する。ファーム、wire 形式、設定、端末の選択・有効化・共通順序は変更しない。Nagle 無効化のファーム候補とは別の差分として扱う。

## 問題と保護する条件

従来は同じ端末 ID の Hello を処理するたびに現在の session を置き換えた。先に受け付けた接続 A の HTTP/WebSocket/Hello が遅れ、後に受け付けた接続 B が既に動画を送っている場合でも、A の Hello が B を失効させる。A への Welcome 書き込みに失敗すると、新旧両方の接続が終了して offline になる。

各端末 ID について、現在の App が受理したプロトコル上有効な Hello 試行の最大受付番号を H とする。受付番号 n が `n <= H` の Hello は、World・session・設定・outbox を変更する前に `stale_connection` として拒否する。`n > H` の場合は、既存の App mutex の中で比較・H の更新・従来の Hello 遷移を連続して行う。別 ID の番号は互いに影響しない。端末側 owner epoch の大小は接続順の判定に使わないため、再起動で epoch が戻っても新しい接続は許可する。

番号は TrackedListener が TCP accept を返した直後、HTTP/WebSocket/Hello の処理前に割り当てる。既存の WriteProgress と同じ接続情報として引き継ぐ。プロセス全体の AtomicU64 を使い、同一プロセス内で listener を作り直しても番号を再使用しない。上限で wrap しない。従来の `connection_id` は WebSocket handler 開始順であり、その意味は変えず、open ログに `accept_order` と `order_source` を追加する。

H は disconnect・Welcome 失敗・端末削除でも戻さず、DB に保存しない。プロトコル上無効な Hello は H を進めない。有効な Hello の後続アプリ処理や永続化が失敗した場合は保守的に H を保持し、古い保留接続の復活を許さない。新しい受付番号での再接続は可能。

## 適用範囲と残る制約

- 本番 host は既に TrackedListener を使うため、TCP accept の番号を利用できる。独自 host が接続情報なしで router/serve_socket を使う場合は WebSocket handler 開始時の番号に退避し、`order_source=websocket_start` と記録する。この場合、HTTP/WebSocket 前で順番が入れ替わるケースまでは防げない。
- 接続の「新しさ」はサーバが accept した順番。クライアントの接続試行開始順と一致する保証はない。プロトコルに端末の試行番号がないため、後から accept された古い試行を識別する変更ではない。
- より新しい有効 Hello は、従来どおり Welcome 成功を待たず現在 session を置き換える。新しい側の Welcome が失敗すると旧 session には戻さず offline になり、さらに新しい接続で回復する。Welcome 成功まで旧 session を維持する二段階切替は今回含めない。
- 新しいプロセスでは古い socket と H は消える。DB の incarnation/session epoch は従来どおり保持し、新しい Hello の session epoch を進める。App オブジェクトだけを再作成し古い handler を残す独自ホットリロードは対象外。
- `App::hello` は従来の信頼された内部呼出し用 API として残す。ネットワーク経由の Hello は受付番号付き API を使う。Hello 前の接続期限、一般的なネットワーク停滞、再接続バックオフは変更しない。

## 検証

同じ回帰テストを修正前と修正後へ追加する。A を先に accept した状態で B の WebSocket handler を先に開始し、B で Welcome・Settings・動画フレーム送信を確認する。その後 A から同じ ID の Hello を送り、A の応答 sink を失敗させる。修正前は B の session が失効する assert で失敗し、修正後は A を拒否して B の追加動画送信まで成功する。

追加ケースは、正当な同 ID 再接続、現在接続の切断後の回復、切断後の古い/同じ受付番号の拒否、無効 Hello、別 ID、DB を再利用した App 再起動、同時 Hello の競合、最新側 Welcome 失敗後の回復を検証する。競合は mutex 取得順を固定せず 24 組を実行する。listener の試験は HTTP/WS の前に番号が付くこと、clone と listener 再作成で番号が逆行しないことを確認する。既存の localhost WebSocket 試験は実際の upgrade へ同じ接続情報が届くことを検証する。

stream-diagnostics ON/OFF の両方で実行する。回帰試験の I/O は in-memory、DB は一時ディレクトリ。実機・サービス・USB・SSH・外部ネットワークを使用しない。コンパイラと既存キャッシュ依存関係を使った正確なコマンド、終了コード、ログ、hash は候補ルートの `validation/` に保存する。

## 既存の設計欠陥と今回の観測

保存済み記録では、稼働中の session 2237 に対し、先に WebSocket handler が開始した接続の Hello が遅れて session 2238 を作り、Welcome 書き込み失敗と旧 session 失効が続いた。約 8 秒後に session 2239 で復旧した。この置換動作は従来の Hello 遷移にあり、今回の回帰試験でも修正前のコードで再現する。

ただし当時のログには TCP accept 番号がない。handler 開始順と TCP accept 順は区別し、当時の実 TCP accept 順まで確定したとは表現しない。遅延 Hello を生んだネットワーク条件や、Nagle 変更がその条件を生んだかは未確定。今回のガードが実機の悪化全体を解消したという証拠はまだない。

## 反映手順

差分・試験ログと不変条件をレビューし、現在のソースとの整合と実行 artifact の hash を確認する。親からの実行指示後にサーバ実行ファイルを保全・置換して再起動する。既存設定・DB の値は書き換えず、schema migration は不要。ファーム更新は不要。新しいログの受付番号と session 遷移を使う実機確認は別途指示後に行う。問題が出た場合は保全したサーバ実行ファイルへ戻し、設定や DB 全体を過去の snapshot に戻さない。

## 2026-10-19 の本番反映と確認

ユーザー承認と独立レビュー後、18:45:55 JST に既存サーバを保全・置換し、picomado.service を 1 回再起動した。既存 default/caricature-retention-delete/stream-diagnostics feature と opt-level=3 を維持した。配置ファイルと実行中実体の SHA256 はともに `80257dea3c6b3f529b0fcca557351c0a81af60095703ee2665ac93b1931e6740`。systemd unit・config・端末の選択/有効化/共通並び順は変更していない。

3 台の再接続と LCD 動画開始後から 210.1 秒、自然稼働だけを確認した。全台 online、E-ink の受信/描画 ACK と時計の毎分画像更新、LCD の動画送信 2517 件と端末申告の描画進捗を確認した。新プロセスの session_end/control_write_failed 等は 0 件、サービス自動再起動 0 回。最新の端末申告は描画 2504 枚/経過 209583 ms、累計最大描画間隔 252044 us。固定長の物理描画性能試験や目視判定とは区別する。

実機で古い Hello を人工的に注入しておらず、その条件での拒否はオフライン回帰試験で確認した範囲。今回の観測で再発しなかったことは長期安定性や Hello 遅延の根本原因の確定を意味しない。LCD ファーム書込み、USB、packet 採取、SSH は行っていない。反映前バイナリは個別保全し、設定/DB の巻戻しを伴わず復旧できる。
