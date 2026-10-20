# D-01の実装・検証

設計を先に `8e4c875` として確定し、因果追跡 `94df8e7` の後に実装した。`ContentPlugin` はopaqueなPreparedFetchとprepare関数を供給し、共通App/fetch層はplugin IDや内容型で分岐しない。weather/radarのsource parse、画像decode、セット全体のrenderと出力画素の準備をblocking workerへ移した。

prepareには不変jobと必要なplugin設定だけを渡す。適用時は現在のpluginをcloneし、device incarnation/settingsとplugin config/in-flight generationを再検証する。prepare時点のplugin全体を後から上書きせず、現在のbrowse/pinへpublishする。共有取得groupを一つのApp lockで完了する構造も廃止した。従来の同期complete APIはprepare→applyへ委譲する。

`d01-service-regression-local-loopback.log`：Rust unit **240件成功、67.73秒**。追加のprepare待機試験では、別端末のDown→動画開始と実VideoTransfer packet生成が進むことを確認した。待機中の設定変更・端末削除・取得取消しで旧結果を拒否し、変更なしでは受理した。待機は5秒の有限channelで、テスト失敗時も無限のbarrierを残さない。既存のweather/radarの正常/error/SUB browse/日付/世代/設定/共有取得とHTTP/video/ACK/再接続の回帰を含む。外部ネットワーク・実機・USBは使用していない。

限界：既存拡張pluginのdefault complete委譲までは重い処理の隔離を保証しない。SUBの現在画面選択・小さいmode合成、plugin snapshot生成とアプリ状態serialize/SQLiteは別経路で、D-02の対象を含めてすべてロック外になったとはしない。観測JSONL保存とアプリSQLite保存を混同しない。実機間欠遅延の原因特定・解消は未確認。

## 独立レビューの2点への修正

追加契約を `4f673b1` で先に記録した。FetchJobにsession epochを加え、prepare取得前とapply時の両方で照合する。weather/radarの再接続時には旧in-flightとその再試行待ちを解消する。実組込みの両pluginで、session 1でprepareした結果をsession 2で拒否し、新sessionの新generationで取得を計画できることを確認した。

weatherは7日分のLCD画素とnative E-ink画素をprepare内で生成し、applyでは現在のbrowseに対応する画素を選択する。cacheはForecastSetの不変Arcを識別し、同じdataset IDを再利用しても旧pinと新latestを混同しない。保持はlatestとpinの最大2セット。画素は1セット205,891 bytes、別途error画像29,413 bytesで、prepare中の一時領域はこれに加わる。cacheはSQLite snapshotへ保存しない。復元時の生成はサービス公開前に行う。

実E-ink能力のAppをmutexで保持したapply試験では、normal/SUBの両方でfull render関数の呼出しが0回、出力3813 bytesが既存native rendererの画素と一致した。SUB中の新取得でも旧pinの画素・datasetが維持された。別試験では同じIDの新セットを6回publishしてもcacheが2セット以内で、cancel後は新latestへ移り1セットになることを確認した。画像の差分を確認する入力には、元の晴天と異なる雨天の符号300を用いる。

現在のコード全体：`d01-review-final-full-local-loopback.log`、Rust unit **245件成功、失敗0、74.63秒**。ローカルloopbackと保存済みデータだけを使い、実機・USB・外部HTTPSは使用していない。D-02のserialize/SQLite待ちは引き続き未修正。今回の追加修正はサーバのRustコードだけで、ファーム更新を必要としない。本番への取り込み・サービス再起動は未実施。

通常featureのlib/bins型チェックは成功（11.39秒、`d01-review-feature-off-check.log`）。観測feature付きのbin試験1件と保存済みJMAデータによるoffline integration 4件も成功（`d01-review-offline-integration.log`）。公開HTTPSを使うlive試験は実行していない。
