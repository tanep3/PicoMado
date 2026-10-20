# D-01の実装・検証

設計を先に `8e4c875` として確定し、因果追跡 `94df8e7` の後に実装した。`ContentPlugin` はopaqueなPreparedFetchとprepare関数を供給し、共通App/fetch層はplugin IDや内容型で分岐しない。weather/radarのsource parse、画像decode、セット全体のrenderと出力画素の準備をblocking workerへ移した。

prepareには不変jobと必要なplugin設定だけを渡す。適用時は現在のpluginをcloneし、device incarnation/settingsとplugin config/in-flight generationを再検証する。prepare時点のplugin全体を後から上書きせず、現在のbrowse/pinへpublishする。共有取得groupを一つのApp lockで完了する構造も廃止した。従来の同期complete APIはprepare→applyへ委譲する。

`d01-service-regression-local-loopback.log`：Rust unit **240件成功、67.73秒**。追加のprepare待機試験では、別端末のDown→動画開始と実VideoTransfer packet生成が進むことを確認した。待機中の設定変更・端末削除・取得取消しで旧結果を拒否し、変更なしでは受理した。待機は5秒の有限channelで、テスト失敗時も無限のbarrierを残さない。既存のweather/radarの正常/error/SUB browse/日付/世代/設定/共有取得とHTTP/video/ACK/再接続の回帰を含む。外部ネットワーク・実機・USBは使用していない。

限界：既存拡張pluginのdefault complete委譲までは重い処理の隔離を保証しない。SUBの現在画面選択・小さいmode合成、plugin snapshot生成とアプリ状態serialize/SQLiteは別経路で、D-02の対象を含めてすべてロック外になったとはしない。観測JSONL保存とアプリSQLite保存を混同しない。実機間欠遅延の原因特定・解消は未確認。
