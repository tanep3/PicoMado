# MUSIC-PLAYER-20261107: 画面進入時の停止解除

2026-11-08、実装前契約。端末操作でローカル音楽画面に遷移した時、停止中なら再生する。一時停止は利用者の明示意図なので維持する。既存の画面描画・session_resetから再生を始めず、純粋なナビゲーション境界を追加する。

## 既存との差分と状態

既存Selectionはplayingをvolatileに保持し、pause/stopの違いを保存していない。端末別に停止理由RestingState (Stopped/Paused/Failed)を保存する。再生要求は従来通りvolatileであり、loadだけでは再生しない。旧保存データに理由がない時はPausedとして保守的に読む。旧pauseかstopかをpositionから推測しない。明示PLAY/STOP後は新契約に移る。音源、metadata、原本、復号/SBC/音量は変更しない。

| 入力と前提 | 遷移 |
| --- | --- |
| 端末のNext/Previous/Power等で別画面からNormal音楽へ進入、選曲済みStopped | 保存位置から再生要求。既知EOFなら先頭。新generation。 |
| 同じ進入、Paused | 一時停止・位置・generation維持。 |
| 同じ進入、Failed | 失敗を保持。自動再試行しない。明示PLAYのみ再試行。 |
| 同じ進入、選曲なし/曲なし/別incarnation | 無音。勝手に曲を選ばない。 |
| 同じ進入、既に再生要求中 | 再起動・巻戻し・generation変更なし。 |
| 再生中の画面離脱 | 配信対象から外れ、確定消費位置を固定してStopped。legacy PCMは従来の時間由来位置。EOFなら次回先頭。 |
| 明示pause/Web一時停止/AVRCP toggleによるpause | Paused。位置維持。離脱・再進入・再起動をまたいで維持。 |
| 明示stop | Stopped、位置0。音楽画面に留まるだけでは再開しない。 |
| AVRCP toggle/Web PLAY | 既存の明示再生規則。pauseから再開。EOFなら先頭。 |
| Hello/再接続/サーバ再起動/設定同期/再描画/ACK/周期処理 | 画面進入ではない。自動再生しない。 |

EOF直後の自動リピートは追加しない。音楽画面から離れて入り直した時のみStoppedとして扱う。pause後に次曲選択してもPausedを保つ。失敗理由は再起動で詳細が失われてもFailedを保持し、画面進入でリトライしない。streamingの再生位置は既存の検証済みconsumedを用い、古いstreamのprogress/failureはgeneration/spec fenceで拒否する。Web状態・LCD/E-inkでPausedをStoppedと分ける。

## 所有と発火境界

ContentPluginに既定no-opの純粋なnavigation_changed(context, active)を加える。Appは受理可能な端末ナビゲーションのkernel判断前後で、Normalかつ選択中という状態が本当に変わったpluginだけに通知する。具体plugin IDの分岐、描画時副作用、観測が再生を許可する経路を作らない。plugin cloneとkernel遷移、snapshot、managed revisionを同じ永続化で採用し、保存失敗時は全て未採用にする。別端末・古いsession・同一画面の再通知は他端末や新世代を変更しない。音量0・muteを解除せず、BT設定も変更しない。

## 無音host受入

停止→進入、pause→進入、EOF→再進入、再生中の重複、未選曲、失敗、離脱、旧状態互換、再起動/Hello、複数端末・incarnation、stale進捗、保存失敗atomicity、音量/mute不変、AVRCP toggle再開を確認する。focused Cargo/実App経路/Node UI/static検査を実行する。全Cargoは親と調整する。本番サービス、原本、USB、実音声は変更しない。独立受入、運用反映、効果確認、変更クローズは親が所有する。
