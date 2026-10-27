# PUBLIC-REVIEW-20261026 — 公開準備の独立レビュー補正

## 既存との差分と指摘

先行 PUBLIC-PORTABILITY-20261026 は文字列による profile include 検査と旧 config の定義行存在検査を行う。有効なマクロを比較していないため、include 後の `#undef` / 再定義や旧 config の重複で、選択した検証済み配線と実際の GPIO が食い違い得る。先行の静的ゲートとホスト試験はこの抜けを検出しなかった。

## 補正契約

1. profile は MCU board、画面・画素形式・寸法・全 GPIO・ボタン数を所有する。個体 config が変えてよいハード項目は LCD の `PM_LCD_BLK_CONNECTED` / `PM_LCD_BLK_ACTIVE_HIGH` と E-ink の `PM_EINK_PANEL_CONNECTED` だけ。個体 ID、Wi-Fi、接続先、診断項目は個体側に残す。
2. 新形式 config は profile を一度だけ include し、保護対象マクロの `#define` / `#undef` を直接持たない。旧形式 config は一つの検証済み profile を選択したまま、保護対象の各定義をちょうど一度だけ持ち、`#undef` や余分なハード定義を持たない。旧形式の秘密ファイルは変更しない。
3. 両形式で対象 C compiler のプリプロセッサ出力を profile 単体と比較し、有効な保護対象マクロの集合・値が完全一致しなければ configure を拒否する。任意の文字列を shell コマンドへ渡さない。検査器と profile は追跡ソースで、変更すると既存ビルド証拠は無効になる。
4. 一時ファイルの回帰試験で include 後 GPIO 再定義、`#undef`、旧形式の重複、間接 include からの上書き、許可した個体パラメータを検証する。既存サンプルと旧形式実設定の一致を秘密値を表示せず確認する。
5. 作者向けの最小 plugin 例は現在の API 境界を示し、動作・実機対応を誇張しない。公開導線には版の固定、証拠、バックアップ、復旧の順序と未承認操作を明記する。

先行の ACK、世代、400 ms 動作は変更しない。独立レビュー・運用適用・効果確認は親の証拠を待つ。
