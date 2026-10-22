# 設計・契約

設計・契約と検討中の案。**現行の製品仕様は [requirements](../requirements/README.md) と実装・該当変更記録で確認する。** 契約案には未実装のものがあるため、各文書の状態と日付を読む。観測ドメインの固定パスは [共通契約](../contracts-observation-design.md)。

## 文書一覧

- [D-02 保存中の読取り・一時入力・配信](application-live-state-design.md)
- [D-02 独立レビュー後の追加契約](application-live-state-review-corrections.md)
- [アプリ状態の永続化と公開（D-02）](application-persistence-isolation-contract.md)
- [D-02 の要件照合とレビュー是正](application-persistence-review-corrections.md)
- [風刺画の保存・保護・期限整理](caricature-storage-retention.md)
- [AWAY接続先のEEX1取得](eex1-endpoint-discovery.md)
- [D-04 E-ink描画中の最新pending](eink-pending-frame-contract.md)
- [取得完了のprepareと原子的適用（D-01）](fetch-completion-isolation-contract.md)
- [D-01 独立レビュー後の追加契約](fetch-completion-review-corrections.md)
- [heartbeat証拠品質：追加P2の修正契約](heartbeat-evidence-review-fix-contract.md)
- [ARM_HBの単一owner統合契約](heartbeat-owner-integration-contract.md)
- [古い接続の遅延 Hello によるセッション置換の防止](hello-accept-order.md)
- [main の状態・期限・画面保持の監査（TR5/TR6修正前に整理）](main-loop-recovery.md)
- [境界事実と入力→描画の因果（第2-B単位）](observation-causality-contract.md)
- [第2-B 独立レビュー後の是正契約](observation-causality-review-corrections.md)
- [継続観測の収集・区間確定（既存設計第8節の具体化）](observation-collector-contract.md)
- [観測queueの資源再配分比較（設計のみ、2026-10-21・レビュー補正）](observation-resource-reallocation.md)
- [実機観測で残った境界と欠測の修正前契約](observation-runtime-gap-contract.md)
- [動画負荷下の共通観測対策契約（レビュー用、未実装）](observation-video-pressure-contract.md)
- [自然heartbeat開始の診断契約案](periodic-heartbeat-capture-contract.md)
- [D-03 plugin所有の復旧・保護参照](plugin-lifecycle-boundary-contract.md)
- [D-03 単位B：保存・公開後のplugin外部作用](plugin-maintenance-contract.md)
- [SDK 2.3.1 submodule整合化と比較契約（2026-10-21）](sdk-alignment-comparison-contract.md)
- [動画送信と正常FINの有限な回復処理](transport-recovery.md)
