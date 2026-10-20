# 取得完了のprepareと原子的適用（D-01）

既存のpure Rules→状態遷移→外部効果の境界を保つ。取得後の画像decode・データparse・renderを共有App mutex下で実行している現状を是正する。D-02の同期保存は別の未解決事項であり、この単位で背景化したことにしない。

1. pluginはfetch jobと、自分が所有する最小限の不変設定からprepare関数を構築する。共通層はplugin IDで処理を分岐しない。ネットワーク結果のparse/renderはこの関数内で行い、共有App mutexを保持せず、async executorではblocking workerを使う。
2. prepareの結果はplugin所有のopaque型とする。共通層は内容を検査・編集せず、対応pluginへ渡す。prepare関数に可変App、DB connection、現在のplugin全体のcloneを渡さない。後着したcloneで現在のbrowse/configを上書きしない。
3. 適用は現在のdevice incarnation/settings revisionとplugin側のconfig revision/in-flight generationを再検証する。削除、設定変更、再接続、取消し、新取得の後着はstaleとして適用しない。現在のbrowse状態へ新データをpublishし、commit失敗時は公開しない。既存のerror表示、成功時刻、retry cooldown、共有取得groupの意味を保つ。
4. 既存の同期complete APIはprepare→applyへ委譲して互換性を保つ。legacy拡張pluginには既存完了へ委譲するdefaultを用意し、組込みweather/radarは重い処理をprepareへ移す。defaultのある未知pluginまで隔離済みと報告しない。
5. 共有取得groupをまとめてApp lock下で完了させない。jobごとにprepare取得→lock解放→prepare完了待ち→短い再照合/applyとする。取得結果や設定のsource bytesを観測ログへ出力しない。

受入：意図的にprepareを待たせるplugin doubleでも、別端末の入力と既に用意された動画packet処理が進むこと。待機中に設定変更/削除/再取得を行い、旧結果をstaleにすること。weather/radarの既存fetch回帰（正常・error・SUB browse・日付・世代・設定・共有取得・取消し）を維持すること。DB serialize/fsync待ちがApp lockから外れたとは主張しない。
