# 要件・設計と実装の対応確認（2026-10-20）

## 対象と保存点

正本は [全体要件](requirements/README.md)、[共通カーネル](requirements/kernel.md)、各プラグイン要件、[設計第2・4・8節](system-design-draft.md)。既存の良い設計を変更せず、実装の適合・逸脱・未実装を区別する。

- 本番リポジトリ：`/home/tane/dev/PicoW/PicoMado`、`ffb62d6dcfe35b85191672d686fcc65c6acb8b50`。既存 ACK 診断の27ファイルを保存。作業ツリーは空。
- 実機で試した候補：別リポジトリの `c1304d2a5c6084fb1aba94cd88c3c88e10c8dc3c`。本番チェックポイントにUSB入力試験・TX・CYW43診断の差分を加え、元ソースのmanifest、私有SDKパッチの出自を `docs/checkpoints/20261020` に保存。これを本番コミットだけで復元できるとは扱わない。
- 秘密設定の内容、UF2/ELF、採取記録はGitに追加せず、既存証拠と権限を絞った設定バックアップに保持する。共有SDKは変更していない。
- 文書整理より先に始めてしまったリファクタは stash `3c4bdbd74814b7176abb2e36aa14d962cc96f941` に隔離した。文書コミットには含めない。元ファイルを削除・破棄せず、文書整理の後に設計修正・実装として扱う。

## 既に適合している構造

| 設計契約 | 実装根拠 | 判定 |
| --- | --- | --- |
| 読み取り専用Query、遅延Rule候補、純粋Transition、状態適用後のEffect | `service/src/lib.rs` の `Plugin::candidates`、`Candidate`、`Engine::handle`、`apply` | 第2節の契約を実装済み。エンジンにIOを持ち込まない。 |
| 外部仕事をロック外で実行し、世代を確認して結果を戻す | `workers.rs::run_work`、`runner.rs::run_pending`、`plugin_jobs.rs` | 部分的に適合。下記の取得完了経路まで全て適合しているとはしない。 |
| Picoにプラグインの内容判断を置かない | `firmware/src/main.c::process`、`core.c::pm_start/pm_chunk`、`video.c` | プラグインIDによる条件分岐は見つからない。受信バッファ、世代検証、表示処理は端末の責務。 |
| 動画を静止画ACK待ちやDB保存から分離 | `lcd_stream.rs`、`playback.rs`、`transport.rs`、`video.c` | 最新画像キュー、独立writer、送信中画像の不変性を維持する。 |
| 内容・固有状態・設定をプラグインに置く | `content.rs`、`plugins.rs`、`clock.rs`、`weather.rs`、`radar.rs`、`youtube.rs` | 主経路は共通契約で配送。固有の描画を共通層へ移す理由にはならない。 |

## 設計からの逸脱・不足（原因の断定ではない）

| 項目 | 現在の実装 | 守るべき契約と次の検証 |
| --- | --- | --- |
| 取得結果の重い適用 | `fetch.rs::run_jma_cycle` が共有Appロック内で `complete_plugin_fetch` を呼ぶ。`radar.rs::complete_fetch` は複数画像のdecode/renderを伴う。 | 内容準備と適用を分け、ロック外で準備、短い世代確認とcommitをロック内で行う。現行worker試験はこの経路の重さを覆っていない。 |
| 永続化の待ち | `application.rs::persist` が全StoredStateを同期serializeしSQLiteへcommit。App操作のロック保持時間に入る。 | 原子的な保存を保ったまま、準備・永続化・公開の境界と競合を設計する。単に非同期化して未保存状態を公開しない。 |
| 風刺画の保存・削除を共通Appが知る | `application.rs` にID、保存先、期限・保護・削除処理がある。自動削除featureは既定OFF。 | 固有ポリシーをプラグイン側、共通層は改訂付き状態の受理と永続化に限定する。今回のLCD遅延原因とする証拠はない。 |
| ファーム受信関数への集中 | `main.c::process` がtype検証、状態判定、設定、応答、フレームを一関数で扱う。 | API表、能力ハンドラ、pending/completed/error結果へ分ける。既存wire/状態・副作用順を維持する。プラグイン判断が存在するという意味ではない。 |
| 継続観測 | `diagnostics.rs` と設計第8節、非待機USB traceは既存。候補のAP2/SIM/統計採取はDTR開閉と結合。 | 事実・欠測・transport・実験制御・永続化を分け、継続採取の有界性と負荷の受入条件を定める。欠測を未実行の証拠にしない。 |
| E-ink描画中の新FrameStart | `main.c::process` は描画中なら新FrameStartを捨てる。 | 非同期表示のpending/完了と新旧世代の契約を確認する。LCDの遅延とは別の未検証事項であり、今回E-inkを操作しない。 |

## 追加機能と文書の整理

| 追加・現在動作 | 正本・既存記録 | 今回の文書対応 |
| --- | --- | --- |
| 時計：JST、毎分境界、可視端末だけ更新、LCD/E-ink、旧順序と有効化を保存 | [時計要件](requirements/plugins/clock.md)、`clock.rs`、`time_frames.rs` | 全体要件の目次と範囲表が旧4種のままだったため、追加承認と初回／既存端末の違いへの参照を追記。 |
| E-inkの毎分更新／全面更新間隔 | 時計要件の能力 `eink-clock-v1`、既存ドライバ試験 | サーバ画像生成は旧ファームでも利用できる。更新間隔には新能力が必要で、全てサーバだけで完結とはしない。物理残像・ちらつきは未検証。 |
| ACK経路の有限採取 | [ACK診断](ack-path-probe.md)、`ack_probe.c/.rs` | 実験機能であり通常機能や原因修正とは区別。boot/run/digest、欠測、凍結後exportの契約を参照。 |
| USB合成入力・TX・SDK内部診断（別候補） | [USB入力/TX](usb-button-tx-diagnostics.md)、[CYW43診断](cyw43-inner-diagnostics.md) | 本番保存点との違いとSDK私有コピーの出自を明示。実機への新規反映は行わない。 |
| LCD動画push、native frame bytes、EOF復旧 | 設計第7節、LCD検証記録、既存main/transport試験 | 既存の正常挙動を比較基準として保持。観測ON/OFF、制御応答の混在を追加試験の対象とする。 |

## 未実装要件と検証限界

汎用サーバメニューは [firmware/protocol-proposal.md](../firmware/protocol-proposal.md) が明示する未実装提案である。ローカルHOME/AWAYメニューやプラグインのサブ画面を、この共通メニューの実装済み証拠にしない。要件から削除して実装に合わせることもしない。

設計・状態モデル試験は実transport、DB待ち、物理描画を全て含まない。既存worker分離試験も重い取得完了と全状態永続化の遅延を保証しない。正常な短い採取から、間欠遅延の不存在や原因を結論しない。観測による時間・IRQ・USB負荷、ARMメモリ、高水位、実機画面は別の受入証拠を要する。

次段階は各逸脱の設計／受入条件を先に更新し、その単位で実装と回帰を行う。既存の端末選択・有効化・共通並び順、秘密設定、動画の正常挙動を変えない。SSH、push、ファーム書込み、サービス再起動、E-ink操作はこの文書整理には含めない。
