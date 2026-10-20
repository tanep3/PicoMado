# 共通契約と観測の段階的整理（2026-10-20）

本書は既存の [共通カーネル要件](requirements/kernel.md#プラグイン-web-画面と責務境界) と [遷移設計](system-design-draft.md#2-遷移を契約で組み立てる) の実装補足である。soukoban の読み取り専用 Query、遅延候補、純粋な遷移、外部 Effect という既決方針を維持する。プラグインの内容生成を共通コードへ移す設計ではない。

## 着手前の保存と適合状況

本番リポジトリの既存 ACK 診断は `ffb62d6dcfe35b85191672d686fcc65c6acb8b50`、実機で試した別候補は `c1304d2a5c6084fb1aba94cd88c3c88e10c8dc3c` に保存した。後者の `docs/checkpoints/20261020` にソースハッシュ、元 SDK と私有パッチの対応を置く。端末秘密設定は Git 外の権限を絞った保存先に保全し、生成物と採取データは元の証拠ディレクトリを保持する。

| 既存契約 | 現状 | 今回の整理 |
| --- | --- | --- |
| Query / Rule / Transition | `Engine::handle` は Query から候補を選び、複製状態と Effect を返す。既に適合。 | 純粋エンジンにはログ IO や可変 Observer を入れない。 |
| 遷移適用 / 外部実行 | `App::change` が永続化後に状態を公開、outbox ID で実行結果を戻す。 | 入力・判断・コミット・副作用の境界を共通語彙で観測する。 |
| プラグイン | `ContentPlugin` が状態・規則・描画・再生指定を提供する。基盤は capability で配送する。 | 登録を差し替え可能な構成境界に集め、静止画・動画・追加プラグインを同じ基盤で試験する。 |
| Pico | プラグイン ID での選択はしないが、受信検証と各機能の実行が `process` に集中。 | 既存 kind/type を API 表へ登録し、受信・検証・振分け・結果を分離する。ローカル接続・ボタン・表示世代は残す。 |
| USB 診断 | 固定 RAM と部分送信は存在。AP2/SIM/統計の採取開始が DTR に結合。 | 汎用イベントは独立した ON/OFF と連続 sequence。既存 AP2/SIM の意味は変更しない。 |

文書更新の前に隔離ブランチへ API 表、入力判断、観測キューの初期変更を行った。これらは消去せず保持し、本書との対応と試験を確認してから実装を進める。本番ソースや実機へ反映していない。

## 責務と境界

1. サーバのプラグインは自身のドメイン状態、規則、内容生成を所有する。データ取得・動画変換・端末 IO は外部実行器に任せ、その結果を入力として受ける。共通実行経路にプラグイン ID の条件分岐を増やさない。登録箇所と既存データの移行規則は具体 ID を扱える。
2. Pico は既存の端末 API を実行する。transport が envelope を受け、型を検証し、API 表から能力ハンドラへ渡す。ローカル入力は純粋な判断と端末操作の実行を分ける。サーバのコンテンツ選択を Pico に複製しない。
3. 観測は事実の採取であり、アプリケーションの可否判断や実行の条件ではない。固定値を有界キューにコピーする。producer は printf、malloc、USB、filesystem、SDK ロック再取得をしない。同一 core の main/IRQ を対象とする。core 間対応を暗黙に保証しない。
4. USB は一つの入出力所有者に集約する。既存コマンドとイベントは行単位で多重化し、部分送信中に別の行を差し込まない。ホストは一つの reader で返信とイベントを配送し、既存試験器はその IPC を利用する。独立した複数 reader を開かない。
5. ホスト保存は独立した有限キューと保存 worker を持つ。USB reader 内で fsync しない。受信、保存済み、実機の処理完了を別の状態とする。ディスク停止・満杯時も MCU を待たせず、欠測を明示する。

USB が本質なのではない。共通契約の各境界で、何を受け、どの世代・状態で判断し、何を出し、いつ完了・拒否したかを結べることが本質である。同じ事実をファイル、USB、テスト sink へ運べるようにする。

## 汎用イベント EV1

初期範囲は入力、API の受信・dispatch・結果、ネットワーク送信、フレーム完成、表示開始・完了、およびサーバの判断・コミット・effect 境界。event 種別、component、phase、source/boot、64-bit capture sequence、採取元の単調時刻、operation/parent、session/frame、固定2引数、欠測数を持つ。規則やプラグインごとの専用リングは追加しない。

Pico は既存 trace の32件キューを共用する。EV1 有効後は通常記録を前半16件まで、control reply に残りを確保し、満杯時は新規を落とす。sequence は採取試行時に採番し、drop、最初と最後の欠測 sequence、飽和を保持する。OFF 後も既存記録と sequence は消さない。IRQ mask は固定値コピーの区間だけ。既存 text の可変長整形は mask 外へ出す。

送出は既存 main の1回1レコード・1回の非待機 write を利用する。EV1 は `ev1 ` + 76 bytes の big-endian 値の hex152桁 + 空白 + CRC32 hex8桁 + LF（166 bytes）。body は component:u8 / event:u16 / phase:u8、続いて boot/sequence/us/operation/parent/session/frame/dropped:u64、a/b:u32。構造体 padding は送らない。上限192 bytes の既存 pending 領域に収まる。文字列行との混在は prefix と長さ・CRC で判別する。別々の行は失われ得るので、sequence の欠落と STATUS を照合する。

`EV1 ON`、`EV1 OFF`、`EV1 STATUS` は採取のみを制御する。STATUS は boot、最後の試行 sequence、欠測数と範囲、有効状態、飽和を返す。OFF の応答はそれ以前の共有キューの後ろに置かれるので、その watermark までの受信・欠測確認とホスト fsync をもって保存区間を閉じる。ACK の紛失・保存側の欠測がある区間を complete としない。

**互換性**：端末の既存 TCP/WS packet、ACK、世代番号、表示順序を変えない。EV1 はビルド時に任意、有効後も明示 ON が必要。既存 AP2/SIM/TX/CY の形式と DTR 動作は維持する。既存の detached SIM COMMIT を連続接続のまま動かせるとはしない。ホストの任意コマンド中継と有限ジョブの実行は別の権限である。

## 欠陥位置と欠測を区別する

### API結果の事実性（第1段階レビュー修正）

`FrameChunk`の結果は実際の組立・実行結果から返す。文脈不一致、offset/長さ/CRC不正はREJECTED、受理したが組立中・動画キュー待ち・非同期E-ink描画中ならPENDING、必要な同期描画が成功したLCDまたは同じ既描画tokenならCOMPLETED、受信ACK送信や描画の実行失敗はERRORとする。COMPLETEDは対応する端末側処理の完了であり、サーバが描画ACKを受信した意味ではない。E-inkの開始成功と物理完了も区別する。

`receive_chunk`がvoidで結果を捨て、`api_frame`が無条件COMPLETEDを返す第1段階の実装は誤った観測事実だった。戻り値を伝播し、実mainを含む試験で不正offset・u32範囲超過・古いtoken・CRC不正・部分受理・同期完了・E-ink pending/完了を検証する。既存の有効入力に対する副作用順、ACK、retryは変えず、不正offsetを縮小変換して受理しない。

最後に満たされた境界から、次の入力・判断・出力・完了のどこで遅延・拒否・不成立になったかを調べる。対象 event が実装・有効化され、同じ boot/operation/世代で、区間末尾 watermark と全 sequence が揃い、欠測がない場合だけ「期待した事実がなかった」と判定する。それ以外は不明とする。既存 wire に端から端までの action ID はないため、session/frame/序数以上の厳密な因果を捏造しない。将来その保証を足す場合は任意 metadata の能力交渉を別途設計する。

境界の内側の詳細、RF 上の実送信、物理画素、未登録の将来イベントまで観測できる保証はない。Pico と Pi の時計を直接引き算しない。無期限切断や無期限の低速保存に対する無欠測保証もない。CPU 時間・IRQ mask・RAM・USB 負荷は実測前に無影響と扱わない。

## 受入条件

本書は既存の正しい設計意図を実装へ戻す**契約の明確化**であり、方針変更ではない。最初の実装単位は端末API・局所入力判断・共通観測・登録契約である。次表は後続修正を反映した現在の追跡状態である。段階ごとの完了と全体の実機受入を区別する。

| 追跡項目 | 修正範囲・完了条件 |
| --- | --- |
| D-01 取得完了の重い処理 | `6c9fe32`を独立受入済み。組込みweather/radarのprepare/apply、session照合、native画素事前生成。未知拡張の同期defaultは別。 [検証](fetch-completion-isolation-verification.md)。 |
| D-02 全状態の同期保存 | `a6c7cf4`まで独立受入済み。保存後公開を維持し、一時query/ACK/browse/配信/時計を分離、表示identityと成功prefixを保持。全JSON保存・永続writer待ちは残る。[検証](application-live-state-verification.md)。 |
| D-03 共通Appの風刺画固有処理 | A `53cb612`は独立受入済み。B `52d8fdd`でplugin所有の期限/保護/再開と、共通の保存→明示公開→外部finishを実装し全体294/273件成功、独立43試験で受入済み・追加指摘なし。[検証](plugin-maintenance-verification.md)。 |
| D-04 E-inkの描画中受信 | `bae0575`まで独立受入済み。最新pending、世代/CRC/metadata、描画中同一token再送のdispatch時合流を0/2ボタンで確認。実パネルは未受入。[検証](eink-pending-frame-verification.md)。 |
| D-05 実用的な切替追跡 | 収集/保存/因果解析と是正`1cbb992`をソフトウェア検証し、5是正を独立確認済み。実USB/IRQ/RF/物理描画、server socket等の未採取境界は残りD05全体の実機受入は未完了。[最新照合](refactor-final-acceptance-20261020.md)。 |

既存挙動は [採用済み通信・Pico設定](../firmware/adopted-protocol-and-configuration.md) を比較基準とする。

- C-01：全ローカル状態×0/2/4ボタン×サブ状態×入力で旧判断と一致。既存 LCD/E-ink、動画、EOF、世代・ACK 回帰を通す。
- C-02：未知 kind / kind-type 不一致を拒否。既知ハンドラの副作用順と pending/completed を保つ。追加テスト API を表へ登録するだけで router を変更しない。
- C-03：観測無効、満杯、USB不在・ゼロ容量・部分writeでも機能の結果と進行を変えない。event producer での整形・IO・SDK 再入がない。
- C-04：sequence の欠落、キュー容量境界、64-bit時刻、飽和、CRC破損・断片化・混在行、boot変更と再接続を検出する。stop watermark と fsync の成功を区別する。
- C-05：ホストは唯一の USB owner。クライアント切断・低速保存・保存容量上限・disk errorでも有限メモリを維持し、不完全な採取を成功としない。ツール試験は擬似端末だけを使う。
- C-06：追加の静止画プラグインと再生プラグインを共通登録契約で注入し、入力・frame・video配送に個別 ID 分岐が不要なことを試験する。
- C-07：実 ARM のサイズ・stack、既存ソースとSDKの不変性を確認する。実 USB の時間・高水位、負荷比較、遅延再発の採取は今回の実機未反映段階では未検証とする。
- C-08：ホストで一つの入力から同じsession/display/frameの受信、判断、出力、CRC完了、対応する描画開始・完了まで追えること。既存IDで入力と出力が一意にならなければ不明とする。欠測を注入した区間はincomplete、完備した区間の拒否・期限超過は該当境界の結果として区別する。遅延閾値・完了待ち期限はホスト解析の設定とし、ファームに症状別freeze検出器を追加しない。有限期限・明示停止・保存watermarkで閉じ、欠測を成功扱いしない。

### 回帰と証拠の対応

| 契約 | 実際の試験・証拠 |
| --- | --- |
| LCDだけのNODELAY、HOME/AWAY・再接続、E-ink不変 | `net_nagle_test.c`、`run_nagle_sdk_tests.py`（実lwIP関数、変更前後、SDK SHA） |
| 遅れた旧Helloが現動画を置換しない | `service/tests/support/hello_order.rs`、`hello_order_matrix.rs`、transport試験 |
| FrameStart先着・chunks間の制御応答、旧ACK、動画取消し | `control_reply_test.c`（trace ON/OFF、LCD/E-ink）、protocol/transport/queue試験 |
| pending ACK取得・取消し・重複と再送方針 | `ack_pending_semantics_test.c`、`ack_net_semantics_test.c`、`run_usb_tx_tests.sh` |
| 正常EOF、Close、部分画像、既描画の保持 | `fin_recovery_test.c`（`run_host_tests.sh`の52モード）、実main回帰（`run_host_tests.sh`から実行） |
| 旧run COMMIT再発火禁止、DTR取消し、実入力優先 | `usb_run_binding_test.c`、`button_sim_batch_test.c`、`usb_main_input_test.c`。EV1有効の組合せも追加実行する。 |
| 混在・部分write・返信枠・digest | `usb_multiplex_test.c`、`ack_probe_v2_test.py`、`usb_button_trial_test.py`、追加EV1試験。controllerは16件。 |
| 私有SDK出自と送信・エラーの保持 | `run_cyw43_inner_tests.py`、`run_inner_tests.sh`、source/SDK/UF2 manifest。共有SDK不変、ON/OFFのbus trace比較。 |

用途別の回帰であり、実機の絶対的な無回帰保証ではない。重い処理と保存、実USBの時間・高水位、物理パネル、C-08は結果を別記する。

### 既存試験fixtureの接続補正

標準cargo testで既存の `transport_recovery.rs` が旧negative比較用cfgを選び、実write進行probeをwriterへ渡していない。これにより「進行あり3.2秒」「進行継続10秒」の2試験が、実際は進行なし3秒で落ちることを確認した。productionのtimeoutを緩めず、採用済みWriteProgressと同じprobeをfixtureへ明示的に接続する。無進行3秒・連続進行10秒・制御250ms・置換session取消しの既存assertは維持する。ローカルTCPを使う試験はsandboxのEPERMで別に失敗するため、既存許可範囲のローカルloopbackだけを使う実行で区別して確認する。
