# 風刺画の保存・保護・期限整理

## 実際の格納場所

SQLiteは`app_state(id, schema_version, body TEXT)`の1行を使う。画像専用BLOB列はないが、`body.plugin_states.caricature.latest`には最新作品のE-ink PNGとLCD RGB565がJSON数値配列で入り、`history`は作品ID・日時・概要・設定の索引である。`state.jobs`には生成要求と公開済み作品IDが残る。古い`artwork/archive/<id>.json`にもE-inkとLCDの配列がある。`<id>.png`は表示用E-ink画像で、原画は新規作品では`<id>.original.webp`、旧形式では`<id>.original.png`である。変換キャッシュは`cache/artwork_variants/<id>-<key>.bin`に置く。

新規原画は生成PNGを縦横半分へ縮小し、`libwebp`の`WebPEncodeRGB`へ品質90を渡して非可逆VP8として保存する。生成PNGから現行LCD/E-inkの表示画像を先に作る。原画WebPのハッシュ、復号、再読込が通ってから公開ポインタを更新する。失敗時は作業PNGを残す。旧アーカイブJSONの表示画像を今すぐ移行・削除しない。現行160×80/250×122の表示画像はそのJSONをキャッシュとして使い、新たなキャッシュファイルを重複作成しない。別解像度や別設定のみ個別の変換キャッシュを作る。DB最新画像とJSONを参照方式へ大きく移す改訂は別途設計する。

## 保護と期限

カレンダーから複数作品を選び、保護・解除を`protection.json`へ原子的に保存する。改訂番号を照合し、同時更新を競合として返す。保護ファイルは作品IDのみを保持し、画像を複写しない。原画、画像入りJSON、表示PNG、キャッシュ、DB履歴は保護時に一式残る。

期限日は作品の日本時間生成日から3暦月後で、月末はその月の最終日に丸める。保護中、最新表示、一時表示、履歴閲覧の開始・表示・移動先、未完了の履歴表示ジョブが参照する作品を候補から外す。参照は保護登録とは別で、切断や表示切替後の次回整理で再評価する。実行時はplugin所有のmaintenance executorが保護台帳guardを取得し、共通層が現在改訂・表示参照・同pluginの未完了ジョブ（Cancellingを含む）で純粋計画を作る。再開印→DB履歴の確定→公開snapshotと通知→作品ファイル群の整理→directory同期→印の削除、の順を守る。重い履歴読込みとファイル作用へ公開lock/SQLite lockを持ち越さない。対象pluginの参照を変える一時入力と時刻再描画を整理中だけ保留し、ACKと他pluginの準備済み入力・query・配信は進める。永続変更は完了まで待ち得る。途中停止印は、新しい参照・保護・未完了ジョブを再検査してから処理する。未確定の印は取り消し、保護された確定済みの印と残存ファイルは解除まで残す。不明な保存状態では削除しない。短期ゴミ箱の期間は追加しない。

毎日03:00〜04:00 JSTに、公開後1日以上たち、保存WebPを再検証できるジョブの作業フォルダを整理する。未公開、再試行中、失敗、旧作品のフォルダは自動整理から除外する。掃除は原画アーカイブやプロンプト設定を触らない。

作品と作業フォルダの**実削除は既定OFF**。有効化には、`caricature-retention-delete` Cargo feature付きのバイナリをビルドし、実行環境に`PICOMADO_CARICATURE_DELETE=1`を明示設定する両方が必要。片方だけでは削除しない。作品整理は03:00〜04:00 JSTに実行する。作業フォルダ整理ジョブはスケジュールされてもゲートOFFでは0件として終了する。対象パスは`caricature/data/artwork/publications/*.json`、`archive/<id>.{json,png,original.png,original.webp}`、`caricature/cache/artwork_variants/<id>-*.bin`と、検証済み公開ジョブの`caricature/data/work/<job_id>`のみ。設定・プロンプト・他プラグインは対象外。

期限整理は任意の登録済みmaintenance handlerとして分ごとに起動し、日付・時刻・候補の意味は風刺画側が判断する。1回の削除候補は最大32件。残りやIO失敗がある間は成功日を確定せず、再開印は時間窓外でも先に処理する。保護済みの確定印はそのまま保持し、別作品の日次期限整理を妨げない。成功日はexecutor内で保存rootごとに保持し、共通Appに風刺画用の日付fieldを置かない。gate OFFではhandlerを起動せず、既存workerの負荷を増やさない。

再開印のあるIDの新規保護は、台帳lock取得後の公開snapshotと現在参照で検査する。論理削除済みの残存ファイルを新規保護で再導入しない。未確定で履歴や参照が残るIDと、印のないlegacy archiveの保護操作は維持する。処理順・失敗条件の契約は[plugin maintenance](plugin-maintenance-contract.md)、隔離試験の結果は[検証記録](../verification/plugin-maintenance-verification.md)を参照する。
