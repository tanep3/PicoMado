# D-03 単位B：保存・公開後のplugin外部作用

先行契約は[plugin-lifecycle-boundary-contract.md](plugin-lifecycle-boundary-contract.md)の単位Bと[風刺画保存要件](requirements/plugins/caricature.md)。D03A `53cb612`、D02 `a6c7cf4`、D04 `034fb2d`を保持する。この文書をBの実装前に保存する。

## 所有境界

共通層はplugin ID、managed revision、opaque proposal/outcome、保存snapshotを扱う。期限、JSTの日次時刻、保護台帳、artwork IDのpayload key、パス、削除gate、再開印の意味はpluginが所有する。純粋Rule/QueryにIOを入れず、pluginへ可変Appを渡さない。

任意のjob executorからmaintenance executorを登録できるdefault Noneの境界を用意する。共通workerは登録済みhandlerを分ごとに起動し、pluginごとの重複実行を避ける。外部prepare/finishはblocking workerで実行し、通常jobやJMAのqueueを占有しない。時刻の可否はhandlerが判断する。風刺画の実削除はfeatureと環境変数の両方がONの場合だけで、既定OFFを維持する。

## 一回の処理順

1. 外部prepareが履歴・保護・再開印を読み、台帳guardとopaqueな候補を返す。ここで削除はしない。重い履歴読込みはApp所有権の外で行う。
2. 台帳を保持した状態ではApp所有権をtryで取得する。busyならguardを解放して後の周期へ回す。台帳を待つ他の所有者とのlock順序を循環させない。
3. 公開lock下で最新一時viewを所有Appへ合流してから、対象pluginの参照を変更する一時入力を凍結する。対象pluginの時刻更新・再描画義務は保留する。ACKは進め、無関係なpluginの準備済み入力・query/HTTP/配信を止めない。公開lockをSQLやファイルIOへ持ち越さない。
4. 純粋なcontent clone上で、現在改訂、表示/browse参照と同じpluginの未完了jobを再検査する。Cancellingを含む。pluginが候補を絞り、変更snapshotとopaque outcomeを作る。
5. 外部stageが削除対象の再開印を先に原子的に保存する。その後に共通層がsnapshotをSQLiteへ保存する。保存失敗なら既存表示・index・ファイルを保持し、削除を実行しない。
6. 共通状態管理が最新ACKを合流して確定snapshotを明示的に公開し、対応通知を送る。公開失敗なら実削除を呼ばない。
7. 公開成功後、外部finishへ確定snapshot/outcomeを渡す。pluginが実ファイル群を整理し、親directoryを同期してから再開印を最後に消す。
8. 参照凍結を解除し、保留した最新frameを現在scopeで処理して公開する。

ファイル完了までmutable所有権を保持し、durableな新参照の追加も直列化する。pluginへはその所有者を渡さない。この間も公開snapshotとACK/非対象pluginの一時経路は独立して動く。永続変更は待ち得る。この判断は、公開前にファイルを消す現状と、公開後に新参照が追加される競合を両方なくすためである。新しい所有者timeoutは導入しない。

## 失敗・再起動・保護

SQL成功後のcleanup失敗では論理的なretirementを戻さない。公開結果とDBは一致させ、残ったファイルと再開印から再試行する。成功日の記録はfinish成功後とし、IO失敗を成功扱いにしない。panicや公開不能では既存D02のunavailable/成功prefix通知を保ち、未公開の削除作用を実行しない。

再開印のある対象も、古いprepare snapshotだけを根拠に削除しない。現行snapshotに履歴/最新/表示/browse/未完了jobの参照があれば保つ。未確定だった印は参照を保ったまま取り消す。確定済み・未参照だけをfinish対象とする。保護された確定済み対象は印と残存ファイルを保持し、解除後に再評価する。起動時復旧も保護と未完了durable jobを検査する。不明な保存状態を安全な未参照と扱わない。

保護APIは台帳lock取得後に現在公開snapshotを読み直す。印があり、すでに論理retirement済みのIDを、残存ファイルがあるという理由だけで新規保護可能に戻さない。未確定で参照が残る対象の保護は許す。印のない既存legacy archiveの保護は維持する。先に確定した保護が削除候補へ入らない順序を守る。

## 風刺画の日次・有界処理

期限は従来の生成日JSTから3暦月（末日丸め）。新しい期限整理は03:00〜04:00 JSTに行う。1回の削除候補は最大32件とし、残りがある間は成功日を確定せず次の周期へ回す。再開印の処理を先に行い、確定済みcleanupの再開は新規期限整理と区別する。protected印だけを繰り返しdomain更新しない。最終成功日はexecutorインスタンス内のroot別状態として保持し、共通Appの風刺画専用fieldをなくす。保存schema 2、端末選択/有効化/共通順序は変えない。

## 受入

opaqueな独自キーの拡張pluginでprepare→純粋計画→印→save→publish→finishの順と改訂拒否を確認する。実SQLite停止/外部作用停止中もquery/他pluginのACK/browse/配信/時計が進み、対象参照が途中で増えないことを確認する。

合成した隔離ファイルだけで、印作成失敗、SQL失敗、公開不能、削除途中失敗、再試行・再起動、参照復帰、保護/解除、latest/temporary/browse/未完了job、default gate、JST時間窓と重複抑止を検証する。公開後のfinishから実際の公開queryを読んで順序を確認する。従来の保護・期限・復旧、D01/D02/D03Aの全体/通常回帰を維持する。

本番DB/本番実ファイル削除、service再起動、SSH、実USB、install、pushは今回も行わない。D03Bはサーバ側の変更であり、D04のfirmware反映とは別である。
