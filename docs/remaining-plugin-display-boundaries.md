# D-03 / D-04 着手時点の確認

D-02のサーバ実装保存点は`7ff1480`。全feature272件、通常251件、bin1件、offline integration 4件、通常lib/bins型チェックが成功した。本書の調査でソースは変更しておらず、D-03/D-04を完了扱いにしない。SSH・実機・サービス・共有SDKへの操作は行わない。

## D-03 風刺画ポリシーの所有境界

現在の`application.rs`には、起動時の`recover_pending_deletes`、`caricature_retention_pins`、`sweep_caricature_retention`、JST 3時の日次実行が残る。Appが`jobs.payload.artwork_id`を読み、履歴・保護ledger・期限・cache削除の意味を知っている。`workers.rs`も風刺画専用の日次メソッドを呼ぶ。自動削除featureは既定OFFのまま保つ。

実装前に確定する境界は、plugin所有の候補選定・参照保護・実ファイル作用と、共通の改訂確認・snapshot保存・結果公開である。起動時復旧も登録済みpluginのhookとして配送し、共通Appへ具体ID分岐を増やさない。現在の保護ledgerの排他区間、pending-delete作成→アプリsnapshot確定→削除完了の順序を維持する。ファイルを移して同じAppの私有状態へアクセスし続けるだけでは、責務を分離したことにしない。

回帰対象は`caricature/retention.rs`の既存試験と、表示中・browse参照・進行中jobの保持、設定変更/取消し/削除競合、途中失敗・再起動復旧、feature既定OFF、日次重複抑止。D-02の公開snapshot・一時view合流と干渉せず、保存失敗時に参照中素材を消さないことを追加確認する。

## D-04 E-ink描画中の受信

現在の`main.c`はE-inkのFrameStartで`drawing`中なら`PM_API_REJECTED`を返す。正常画像の受信完了後は`drawing_token`で描画を追跡し、`poll_display`が物理完了と現在tokenを検査してRendered ACKを作る。この既存の完了/取消し区別は維持する。「描画中なら新Startを捨てる」ことを要件へ変更しない。

新しい受信を許す前に、描画中bufferと受信assemblyの所有をドライバまで確認する。SPI送出やbusy待ち中にどの画素領域を参照するかは、この調査ではまだ確定していない。最新pending画像はtoken・更新metadata・CRC検証済み画素を同じ所有単位として有界に保持し、古い描画結果で新しいpendingを完了扱いにしない。取消し・切断・待機・新sessionでは旧pendingを失効させ、busy解除後に現在の最新画面へ進む契約を先に定める。

受入は純粋モデルと実mainを使うC試験で、busy中の新Start/chunk/完了、複数世代、部分受信、不正CRC、取消し/切断/再接続、旧ACK、時計更新metadataとAPI_PENDING/COMPLETED/REJECTEDの対応を確認する。既存`eink_app_test.c`、`eink_test.c`、API結果試験を維持し、ARMのRAM/stackとLCD非影響も確認する。物理パネルの受入とファーム反映は別途であり、今回実施していない。
