# D-03 単位A：起動復旧・保護参照の分離

先行契約は`83a7ec6`の[plugin-lifecycle-boundary-contract.md](plugin-lifecycle-boundary-contract.md)。D-02の追加4件は`551445a`で別に固定しており、この変更へ混ぜない。

`PluginStartupExecutor`を外部bootstrapとして登録する。Appは登録順でPluginPathsとSQLite接続を配送し、すべての復旧成功後に保存snapshotを読んでcontentへ復元する。共通constructorの風刺画名・保存先・削除gate分岐を除いた。風刺画のRetentionRecoveryが既存gate、artwork/cacheの相対パス、pending-delete復旧を所有する。起動エラーはAppを返さず、IOエラーを保持する。既存の端末選択・有効化・共通順序・保存schemaは変えない。

保護参照の共通Queryは、指定pluginの未完了jobだけを渡す。共通層はpayloadのキーを解釈しない。ContentPluginのdefaultは従来の表示参照を返し、風刺画は自分の`artwork_id`を自分で解釈する。Cancellingも未完了に含める。

## 検証

`d03a-targeted.log`: 新5件成功、失敗0、0.09秒。実SQLite bodyを書き換える二つの起動hookを登録順で実行し、その後に新しい状態がcontentへ復元されること、正しいdata/cacheパス、失敗時の未公開、未知/重複hook拒否を確認した。独自`opaque_asset`キーの拡張pluginで、他plugin/完了jobを混ぜず保護参照を返せることを確認した。実風刺画でCancellingの履歴表示jobを保護し、完了/他pluginのjobを除いた。

`d03a-retention-regression.log`: 既存期限・保護・pending-delete復旧4件成功、失敗0、0.68秒。

`d03a-all-features.log`: D-02追加修正を含む全feature unit283件成功、失敗0、81.98秒。

`d03a-default-features.log`: 通常unit262件成功、失敗0、87.07秒。

`d03a-default-check.log`: 通常lib/bins型チェック成功、12.51秒。

## 未完了範囲

App内の風刺画専用sweep、JST03時の日次制御、workerからの専用呼出しは単位Bに残る。実ファイル削除を確定snapshot公開後に行う調整もBの対象であり、Aの通過だけでD-03全体を完了としない。E-inkの描画中pending受信(D-04)も別項目である。

このA変更はサーバ側だけで、ファーム更新は不要。実機、本番サービス/DB/設定、SSH、共有SDKへの書込み、push、インストールは行っていない。実機の間欠遅延の解消も未検証である。
