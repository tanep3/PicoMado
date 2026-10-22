# システム設計の状態遷移モデル検証

- 実施日：2026-10-12 UTC
- 対象：[システム設計書ドラフト v0.4](../../docs/history/system-design-draft.md)
- 実装：[model.rs](model.rs)。Rust 標準ライブラリだけを使う隔離モデルであり、製品サーバや Pico ファームではない。

## 独立レビュー７件への対応

| 指摘 | 設計書の修正 | ローカルモデルで確認したこと |
| --- | --- | --- |
| 1. 電源入力の状態差 | §6 で通常時だけ待機、時系列操作中は既定表示へ取消し、項目一覧中はメニュー取消しに分離。 | `power_is_mode_scoped_and_menu_never_standby`。 |
| 2. 削除後の旧完了混入 | §3・§5・§6 に永続非再利用 `incarnation` と `(device_id, incarnation)` の検査、削除時の設定・保留 effect の原子的失効を追加。 | `deletion_incarnation_rejects_old_save_send_ack_even_with_same_revision`、`reordered_old_fetch_after_new_registration_has_no_effect`。 |
| 3. Pico 局所画面と Pi 表示世代 | §5 で `screen_owner_epoch`、保留 frame 破棄、描画開始／完了時の再検査、物理描画が中断不能な場合の復帰を追加。E-ink は描画完了まで待機成立を保留。 | `queued_frame_rejected_by_local_takeover_and_new_server_epoch`、`noninterruptible_draw_repaints_local_screen_without_old_ack`、`eink_standby_waits_for_noninterruptible_draw_then_holds_image`。 |
| 4. 選択中プラグインの Web OFF | §6 で設定変更時に選択を即時再評価し、サブ破棄・表示世代更新・全 OFF 表示・オフライン保存を追加。 | `disabling_selected_plugin_immediately_reselects_and_clears_submode`。 |
| 5. 設定・取得セット・日時表示の一体性 | §7 で改訂・取得要求世代・セット ID・対象日・地域ラベルを一つの表示スナップショットに束ね、地域変更中・最終失敗・日跨ぎ・閲覧中保持・取消しを具体化。 | `region_change_rejects_old_result_and_keeps_dataset_label_atomic`、`browse_snapshot_survives_refresh_and_midnight_then_cancel_uses_new_today`、`midnight_without_new_dataset_and_final_failure_do_not_mislabel_old_day`、`older_request_same_incarnation_and_revision_cannot_replace_new_dataset`。 |
| 6. 複数端末の公平性 | §7 で worker 並列上限、端末別バイト上限、動画間引き、順番処理、短い DB トランザクションを明記。仮の 2/1 worker・256 KiB は測定前の初期値。 | `byte_bounded_queue_drops_stale_video_and_reserves_control`、`worker_permits_and_round_robin_prevent_busy_device_starvation`。 |
| 7. Web 設定と物理能力 | §8 でファーム申告能力と Web 管理プロファイルを分け、再接続時の照合・不一致表示・変更反映時点を明記。 | `web_cannot_enable_unsupported_firmware_capability`。 |

## 再現コマンドと結果

```bash
rustc --edition=2021 --test experiments/design-state-model/model.rs -o /tmp/picomado-state-model-tests
/tmp/picomado-state-model-tests
```

既存 `rustc 1.98.1` を使用し、追加インストールとネットワーク取得は行っていない。実行結果は **15 passed、0 failed**。`Rule` 群が読み取り専用 `WorldQuery` から候補を生成し、Engine が候補を選択して純粋な状態更新と effect 命令を返す形をモデル化した。旧世代の入力順序を入れ替えた反例でも、旧取得・旧保存／送信／ACK を無効として拒否した。同じ端末登録・設定改訂内で取得要求だけを更新した場合も、古い取得結果を拒否した。

モデルは状態と識別子の不変条件を確認する。現実の WebSocket／SOCKS5 通信、SQLite の原子性や障害復旧、実際の worker 性能、Pico のメモリ・LCD／E-ink 物理描画、BLK 極性、気象庁の取得継続性は検証していない。ポートと７組の初期値はユーザー確認済みで、[初期値・検証範囲](../../docs/history/initial-values-proposal.md)に記録した。worker・送信待ちなどの技術仮値は未承認で、合否閾値ではない。既存の接続・気象実験や稼働サービス、ファームは再実行・変更していない。
