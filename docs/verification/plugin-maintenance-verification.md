# D03B plugin maintenance の実装・検証

先行契約は`69d6187`の[plugin-maintenance-contract.md](../design/plugin-maintenance-contract.md)。D03AとD02の保存・一時view境界を保ち、D04 `bae0575`の差分へ干渉しない。

## 実装境界

`maintenance.rs`の任意executorはopaqueなprepare/permit、stage、公開後finishを受け持つ。`application_maintenance.rs`は現在のmanaged revision、同pluginの未完了job、clone上の純粋計画、SQLite snapshot保存を扱う。`shared.rs`の明示的公開で通知を確定してからfinishへ渡す。pluginへ可変Appは渡さない。登録のないjob executorはdefault Noneで従来動作を保つ。

共通workerは登録済みhandlerだけを分ごとにblocking workerへ起動し、同じpluginの重複を抑止する。停止時は進行中maintenanceも待つ。共通層に風刺画ID、期限日、payloadのartwork key、保護台帳、日次成功日の処理はない。

風刺画側はJSTの期限・03時の時間窓・root別成功日・最大32件・再開印とdirectory同期を所有する。prepareでは削除せず、現在参照を凍結後の計画で再検査する。SQLや公開失敗ではfinishを呼ばず、finish途中失敗では公開済み論理削除と残存印を保持して再試行する。起動時も保護・履歴・latest・自plugin未完了jobを照合する。

整理中は対象pluginの参照変更入力とtime/debt描画を保留する。ACKは一時経路で進み、他pluginの準備済み入力・HTTP/query・時計・配信も独立する。mutable所有権はfinish終了まで保持するため、永続変更は待ち得る。新しいtimeoutや保存前の先行表示は追加しない。

## 隔離試験

`application_maintenance_contract_tests.rs`は独自のshelf/opaque_assetキーのpluginで、prepare→plan→stage→SQL→公開通知→finish内の公開query・DB照合→unlinkを確認する。実SQLite transaction内の停止とfinish停止の双方でHTTP/query、天気SubとACK、非対象時計の配信が進み、対象入力が一時経路へ入らず、解除後のtime refreshが1回だけ再開することを確認する。

stage失敗、SQL失敗、公開lockのpoison、外部finish失敗・panicを注入し、ファイル/印・DB/公開snapshot・成功prefix通知の境界を検査する。prepare後の改訂変更や所有者busyではstage/finishを呼ばない。現在の同plugin Queued/Running/Cancellingを参照に含め、完了jobや他pluginのpayloadを混ぜない。

`retention_maintenance_tests.rs`では既定gate、JST時間窓・root別重複抑止、latest/temporary/browse各参照、32件上限、削除途中IO失敗と時間窓外の再開、保護/解除、未完了jobの起動復旧、不明snapshotの拒否、未確定印の取消し、論理削除済み残存ファイルの新規保護拒否、legacy archive保護を合成tempファイルだけで確認する。既存の期限・WebP・一式削除復旧・実Appのview移動試験も維持する。

最終自己レビューで、保護済みの確定再開印だけが残る場合にも別の期限切れ作品を整理でき、成功日以後は空の公開を繰り返さない回帰条件を追加した。保護済み印が新規期限整理を恒久的に後回しにしない。最終実行結果：

| 確認 | 結果と証拠 |
| --- | --- |
| 重点試験 | 10/10、0.79秒（`d03b-final-targeted.log`）。最終の保護済み印の条件は下記全体回帰にも含めた。 |
| 全feature library | 294/294、89.81秒（`d03b-final-policy-all-features.log`）。 |
| 通常feature library | 273/273、87.61秒（`d03b-final-policy-default-offline.log`）。 |
| 通常lib/bins/tests型チェック | 成功、24.69秒（`d03b-final-check.log`）。 |
| binary / 同梱JMA integration | 1/1、4/4（JMA 2.31秒）、上記通常構成log。実HTTPS試験は起動していない。 |

実機遅延の改善・物理描画の受入を示す試験ではない。

## 反映条件

D03Bはサーバ側のみで追加ファーム更新は不要。削除gateは既定OFFを維持し、本番設定を変えない。採用には独立レビュー後、通常構成でserviceをbuildし、既存バックアップ・配置・再起動手順の承認範囲で反映する。D04を採用する場合のE-inkファーム更新は別工程である。

今回の変更は隔離worktreeだけ。本番source/DB/素材、共有SDK、実USBに触れず、SSH、service再起動、install、pushを行っていない。コミットは隔離branchのレビュー固定点としてのみ作成する。

承認後の本番反映手順（今回は未実施）：

1. 独立レビューで確定したサーバ側commitを通常featureのままrelease buildする。`caricature-retention-delete`を追加せず、削除環境変数も変更しない。
2. 現在のbinary、設定、schema 2のDB、各pluginのdata/cacheを既存の権限で退避する。DBと素材の整合した退避は、承認されたservice停止で生成・公開を止めて行う。稼働中WALのDB本体だけを単純copyしない。
3. [既存のuser-service配置](../../service/deploy/README.md)に従い、`/home/tane/bin/picomado-server`だけをレビュー済みbuildへ更新して既存user unitを起動する。既存configと端末の選択・有効化・共通順序を上書きしない。
4. Web/API、再接続、保存済み選択、静止画/動画/時計の通常動作とjournalを確認する。問題時は退避binaryへ戻し、DB/素材の復元が必要な場合は反映後の変更を確認してから判断する。

E-inkのD04 firmwareを同時に書き込むことは前提にしない。採用する場合は別の承認範囲で実board IDと既存端末設定を使い、端末ごとにfirmware更新と物理受入を行う。今回保存したsample UF2は実機に使用しない。

## 独立レビューの受入

`52d8fdd`を独立43試験で確認し、追加指摘なし。SQL失敗時の参照/実ファイル保持と取消し後再試行、台帳lockを待ったHTTP保護更新が公開状態を再検査すること、登録された実起動hookによる復旧も確認された。D03Bのソフトウェア範囲は受入済み。永続変更がfinish終了まで待つことは既存契約どおりであり、実機遅延への効果は未検証である。
