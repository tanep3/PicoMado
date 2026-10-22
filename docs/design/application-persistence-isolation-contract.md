# アプリ状態の永続化と公開（D-02）

既存設計第2・3節の「純粋な判断→遷移適用→永続化→公開→外部効果」を保つ。D-01は取得結果のparse/render、観測保存はEV1/sourceログであり、本書のアプリ状態SQLiteとは別経路である。

**レビュー状態：本書の所有権貸出だけの案は未受入。** 後続の一時入力まで保存待ちに直列化する問題、複数commitの途中成功後のerror/panic、元要件における永続選択と一時状態の違いは[追加是正契約](application-persistence-review-corrections.md)を優先する。退避した試作は採用・完了していない。

## 所有と公開

変更処理はmutable Appの所有権を一つだけ借りる。共有mutexは所有権を受け渡す短区間だけで保持し、JSON serialize、SQLite transaction、plugin処理、呼出側の待機中には保持しない。並行queryと送信は、最後に保存・公開済みのimmutable App snapshotを読む。snapshotの公開lockもIO中には保持しない。

一つのworld改訂を更新する変更処理は従来どおり順序付ける。後続の変更は最新状態を取得して既存のincarnation/session/settings/config/generation検証を行う。競合中のsnapshotを勝手に編集して後で古い全状態を上書きしない。保存待ちで後続の状態変更が必要な順序を待つことと、無関係の読取り・既存動画を止めることは分ける。待機はasync executorを塞がないよう所有権取得と保存のblocking区間を扱う。

保存失敗では未保存worldを公開しない。成功後にcontent・frame cacheを含むsnapshotを一括公開し、その後dispatch/workの通知を送る。通知だけ先行して旧snapshotを消費させない。commit後・公開前にprocessが落ちれば、再起動で保存済みworld/outboxから復帰する。結果不明な操作は自動再送しない。panicした所有者の状態は後続の変更へ黙って貸さない。

## 動画とframe ID

既存LCD pushのcurrent+latest pendingとACKなしcreditを維持する。動画のper-device slotとframe ID採番は保存待ちの所有者に依存させない。最後に公開したsession/display/ownerを短い読取り区間で再検証してenqueueする。送信側も各packet境界で最新の公開世代を検証し、旧世代を送らない。

static/動画でIDが衝突しない共通採番を使い、pure transitionのframe予約と採番の短区間だけを直列化する。SQLite IO中に採番lockを保持しない。保存失敗で予約IDを再使用しない。次の保存では最新採番境界を取り込み、再起動後は既存session世代の失効規則を保つ。動画自体を毎frame永続化しない。

## 受入

- 実SQLiteへの保存待ちを注入し、queryが旧保存済み状態を返し、別端末の動画生成と実writer packet送出が進む。未保存の選択・画像・effectは公開しない。
- 保存成功後だけ新選択と対応frame/effectが見え、通知が公開より先行しない。保存失敗・所有者panic・再起動/outbox復帰を検証する。
- D-01のprepare待ち中の設定変更・削除・取消しの後着拒否を維持する。複数端末/入力順序、旧Hello・旧ACK、設定改訂、static/video/E-ink、HTTP UIの既存回帰を維持する。
- query snapshotのcopy/参照共有と保存待ちの作用範囲を測定可能な境界で示す。IOが別threadに移っただけで、共有query lockを保持したまま待つ構成は合格としない。

実機のRF・USB・panel遅延解消、OS上の無限IO停止や無限query保持に対する無欠測保証は、このソフトウェア単位の完了から推定しない。
