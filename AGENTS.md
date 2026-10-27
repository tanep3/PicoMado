# Development entry

All parent and worker tasks read [the versioned standard](development/standard/v1/standard.md), [development process](docs/development-process.md), and [domain contract](docs/contracts-observation-design.md#obs-20261021-01-observation-domain) before changing governed code.

Associate each change with an open ID in `docs/changes/`. Fix the domain model/contract first; generate representations; pass static boundaries; implement through aggregate APIs. `python3 tools/project_gate.py check` is available as an optional development check. Never equate local tests, a document commit, or an isolated branch with operational completion. The parent owns final closure and independent approval.

No device/service/SSH/permission/push operations are authorized by this file. Test fixtures use temporary paths and fresh PTYs only. Direct build/flash/deploy commands do not require the root gate. See the process for known enforcement limits and the approved work root.

開発の正本は、このリポジトリの `main` とする。別cloneを無断で増やさない。作業分離が必要なら、開始前に正本との関係と統合先をユーザーへ説明する。完了時は正本 `main` への統合と検証を確認し、コミットの所在と未反映差分を報告する。既存cloneはこの指針だけを理由に削除しない。

## 調査と委任の共通参照

既に動く類似機能・過去の解決策と今回との差分を調査の起点にし、ユーザーが示した仮説と比較軸を既存の問題記録に保持する。着手・方向変更・委任では同じ記録を参照し、各仮説の支持・反証・未検証と根拠を引き継ぐ。未検証を反証扱いして調査から落とさない。誤判断が出たら、脱落した前提、是正、次の実作業での確認結果を同じ記録へ戻す。今回の適用例と未完了の比較検証は[YouTubeのACK非依存化を起点にしなかった件のPDCA](docs/development-process.md#pdca-youtube-ack)を参照する。新しい管理形式や作業開始ゲートは追加しない。
