# main の状態・期限・画面保持の監査（TR5/TR6修正前に整理）

対象は隔離候補のみ。LCDのEOF救済以外の状態設計は維持する。実SDKのIRQスケジュール・進行中SPIの強制停止は検証対象外。

## 不変条件

1. 最初のFINまたは正常Close decodeから1500000us未満の間だけ、新しいネットワーク画像の描画を開始できる。等号は期限切れ。READYの通常再生にはEOF用SDK pollを増やさない。
2. transportの期限判定はSDK lock内で行う。hard error latchはEOF_TIMEOUTより優先し、EOF_TIMEOUTをhard/protocol errorに上書きして画面を消してはならない。
3. FAILEDを復旧へ送るすべてのmain checkpointは同じ判定を使う。LCDのEOF_TIMEOUT→retry_eof、接続前のTCP失敗→retry_recovery、その他のhard/protocol→retry。E-inkは従来のretry。
4. retry_eofは前の保持画像、または現contextで描画済みの有効画像を保持する。毎回ownerを無効化し、未表示画像を破棄する。物理pixelsの保持と論理tokenの有効性は独立。
5. 有効なlocal操作、menu/standby、実display失敗、hard/protocol errorは保持を解除。CRC/token不一致と正常EOF終了は、既に保持している有効pixelsを解除しない。
6. 期限未満で始めた同期SPIが期限を越える場合は完了させる。期限後に次の描画を始めない。実機の厳密な時間上限ではなく協調実行の開始判定である。

## 状態遷移表

| 現状態 | 事象 | 次状態 | 画面保持・描画 |
| --- | --- | --- | --- |
| WIFI | link up / timeout | SERVERまたはENDPOINT / RETRY | 保持中はstatus抑制、PM readback維持 |
| SERVER | READY / TCP refused / hard error | SESSION / RETRY / RETRY | hello送信、接続前失敗は保持、確立後errorは解除 |
| SESSION | welcome / app timeout | ACTIVE / RETRY | 保持中はWAIT IMAGE抑制、前owner無効 |
| SERVER・SESSION | FIN/Close | RETRY | 古いsessionとしてdecode/描画しない、既存保持だけ継承 |
| ACTIVE+READY | 完成したcurrent画像 | ACTIVE | CRC/token検証後のみ描画、成功で保持解除 |
| ACTIVE+DRAINING/DRAINED | 期限未満、完成画像 | ACTIVE→RETRY | pacing後に最大latest1枚を描画、owner無効化して保持 |
| ACTIVE+DRAINING/DRAINED | 期限ちょうど・超過 | RETRY | 新規描画0、既存有効pixels保持 |
| SERVER・SESSION・ACTIVE | hard/protocol/overflow | RETRY | EOFより優先、保持解除 |
| RETRY | backoff終了 / Wi-Fi喪失 | SERVER/ENDPOINT/WIFI | pixelsを維持し毎試行owner無効 |
| ENDPOINT | target発見 / 探索失敗 / network変更 | SERVER / RETRY / RETRY | 保持中のNETWORK status抑制、既存EEX手順 |
| ACTIVE | SUB/UP/DOWN/POWER | ACTIVE/RETRY/STANDBY | 有効操作は保持解除。UP/DOWN/POWER待機中は旧ready画像を抑止、SUBの世代更新は既存control reply処理 |
| WIFI・SERVER・SESSION・RETRY・ENDPOINT | SUB / POWER / UP・DOWN | MENU / STANDBY / そのまま | menu/standbyで解除、無効UP/DOWNは保持 |
| MENU / STANDBY | network選択 / wake | WIFI | 明示画面は許可、旧画像は再描画しない |

## poll と draw の走査結果・修正方針

| 場所 | 問題・必要な扱い |
| --- | --- |
| network状態でのWi-Fi喪失 | 先に同じfailure checkpointを適用。正常EOFならretry_eofで保持し、未表示画像を破棄。通常のWi-Fi再joinは既存処理 |
| main先頭のcyw43_arch_poll | callbackがFIN/errorをラッチ。続くnetwork checkpointで共通復旧 |
| main接続処理先頭のpm_net_poll | 全SDK処理。既存EOF_TIMEOUT特別扱いを共通helperへ移す |
| take成功直後 | SDK lock解放後に時刻/errorが変わる。処理前にも共通checkpoint |
| メッセージ処理直後のpm_net_poll | TR6の通常retry分岐を共通helperへ。static drawは既にprocess内で起こり得るためここだけでは不十分 |
| take=false後 / 受信予算終了後 | Close write/outputやpingの同期時間が経過し得る。SDK poll不要の期限checkpointと共通復旧 |
| pending rendered_ack送信後 | 同期送信時間が経過し得る。次のvideo開始点で再判定。失敗も共通routing |
| poll_video→pm_video_begin_draw→pm_display_frame | TR5。論理draw開始直前の軽い期限checkpointを必須とする |
| receive_chunk→receipt_ack→LCD pm_display_frame | staticも送信/CRC/診断で時刻が進む。描画直前の同じcheckpointを使う |
| E-ink clock/frame/poll_display | 既存非救済retryを維持。新しいLCD描画gateは適用しない |
| local/status/boot/config/menu/standby/diagnostic | ネットワーク画像ではない。明示local/error画面は許可、保持中の接続statusは抑制 |
| heartbeat / optional stream status | 後続drawはない。エラーは次の共通checkpointで回復。通常再生へ重いpollを追加しない |
| endpoint poll / diagnostic_hold poll | EEX復旧と明示診断。画像drawを起こさない。既存状態遷移を維持 |

実装は期限/エラーの軽量checkpoint APIとmainの共通復旧helperに限定する。画像キャッシュ、wire、server、描画driver、接続設定は変更しない。実main試験ではpoll/take/ack/output/CRC後の経過、FIN/Close/エラー/期限、前画像の有無、ユーザー操作を組み合わせ、追加pollで欠陥を隠さない。


## 最終実装と証拠

- net.cのcheck_failure_unlockedを既存pm_net_pollと新しいpm_net_checkpointで共有する。checkpointはSDK lock内でerror/EOF期限だけを確認し、SDK poll・decode・application writeは行わない（期限終了の既存close cleanupは行う）。通常READY再生へcyw43_arch_pollを追加しない。
- main.cのrecover_transport_failureが全transport FAILED回復点の分類を担当する。endpoint_proxy_progressのFAILED観測はEEX報告のみで、画面回復を行わない。通常pollより先に接続をresetするWi-Fi喪失とEEX network changeにはrecover_transport_lossを共有し、同じfailure分類を先に適用する。process内で不正type/設定として明示的に行うretryやdisplay失敗によるretryは、独立したprotocol/local失敗として維持する。token不一致を無視する既存検査も維持する。
- 動画のpm_video_begin_draw前と、静止画のreceipt_ack/診断処理後のLCD draw前に同じ軽いcheckpointを使う。take=false後にも確認し、送信遅延で期限を越えたClose/pongを見落とさない。
- 独立の実main再現48/49はbefore-tr56-reviewで失敗、候補で成功。main-independent-48.log / main-independent-49.logに結果を保存。
- fin_recovery_test.cのcase48-52は実entry/main loopを各回実行し、テスト側の追加期限pollを挟まない。Close/pong/FIN送信遅延84、poll/take/process/static receipt/render ACK80、各状態でのuser操作36、hard/overflow/protocol/expiry/Wi-Fi loss64、開始済みSPI完了/display failure20の計284シナリオを、診断ON/OFF/ASan・UBSanで実行した。
- 各シナリオで描画開始時刻、保持flag、status描画回数、owner無効化、RX/app解放、旧video再呼出しでも再描画しないことを検査した。期限後に到着した未decodeの不正Closeはdecodeせず期限終了する。期限時点でラッチ済みのhard errorは保持を解除する。
- 通常再生・pacing、HOME/AWAY・PM・EEX・E-ink・入力/応答の既存全host試験も実行する。host mockの時間注入は境界の論理検証であり、実機のSDK実行時間、IRQ、SPI所要時間、12fps回復の証明ではない。

app_endpoint_test.cではEEXのnetwork changeがmain pollより先に起きる3経路（EOF_TIMEOUT、hard/protocol、まだretained flagへ移していないcurrent表示済み画像＋EOF）も検査し、保持/解除が同じ分類になることを確認した。
