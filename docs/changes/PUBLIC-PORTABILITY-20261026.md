# PUBLIC-PORTABILITY-20261026 — 公開用可搬性と検証済みハード構成

## 既存との比較と対象

既存のサーバは bind、DB、plugin root と HOME を一つの個人環境に固定している。ファームは個体 config に表示、入力、配線も重複し、CMake と build lineage と flash 検査は pico_w を固定している。既存の LCD 160×80/4 ボタン、E-ink 250×122/0・2 ボタン、ACK・世代・400 ms フィードバックは維持する。

## 契約

1. サーバ設定の既定場所は実行ユーザーの HOME 以下の `.config/PicoMado/config.toml`。明示 `--config` はそのファイルを読む。bind は具体的な IPv4/IPv6 アドレスと有効な非ゼロ port に限り、wildcard、multicast、unspecified は拒否する。DB と plugin root は絶対パス、`..` による脱出や同一・逆親子関係を拒否し、既存ファイルを移動・上書きしない。旧固定値の明示設定は受け入れて移行不要にする。
2. ハード構成は検証済み profile のみを許可し、個体 config は ID、ネットワーク、個体向けパラメータを所有する。プロファイルは MCU board、表示、サイズ、入力数・配線を一体として宣言する。sample-a/b は Pico W LCD 160×80/4、E-ink_0/1 は Pico W E-ink 250×122/0・2 の既存組合せを表す。未検証の組合せ・Pico 2 W・ESP32・スピーカー/マイク/サーボは拒否する。
3. CMake、build lineage、receipt、manifest、offline flash preflight は同じ profile/board を照合し、profile ファイル変更後に旧 receipt を再利用できない。任意の環境文字列を CMake 引数や shell として実行しない。USB 接続試験や実機書込を検証の前提にしない。
4. 公開文書は現行対応・未対応を区別し、日本語の初心者導入と復旧を提供する。MIT は当プロジェクトの権利者 Tane Channel Technology を表記し、同梱の第三者素材の表示を維持する。秘密設定・個人パスを公開サンプルに含めない。

## 段階

A. この契約と変更 ID を確定。B. サーバ可搬性と拒否試験。C. profile と個体設定の分離、build lineage/flash の拒否試験、A→B→A のホスト試験。D. 公開 README、サポート表、MIT、初回導入。E. 最小 plugin 例と安定版・復旧手順。各段階は root gate と対象回帰で検証し、独立承認・運用適用・効果確認は親の証拠を待つ。

## 未検証の比較

既存機能とのフレーム/入力挙動比較、実機での対応可否、新 MCU/表示/部品は未検証。先行する ACK 非依存化の効果比較も既存記録の未完了状態を維持する。
