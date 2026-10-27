# PicoMado 文書案内

初めて使う方は [README](../README.md) → [対応表](guides/hardware-support.md) → [初回設定](guides/first-setup.md) の順に読んでください。以下は開発・調査用の記録です。履歴内のホームパスとネットワーク値は公開用の置換値であり、当時の生データの再現値でも、現在の接続先でもありません。構造化されたチェックポイントには当時のローカル証跡パスが残るため、公開前の別途整理が必要です。

[公開範囲と私的証拠](publication-scope.md)に、現時点で公開から分離すべき記録をまとめています。

正本はこのリポジトリの `main`。読む順番は次の通り。文書の表題が同じ機能を示していても、設計案・検証結果・過去の観測は役割が異なる。

1. [要件・端末別仕様](requirements/README.md) — 承認時点の基準。後日の変更は対応する変更記録と実装で照合する。
2. [共通契約と観測設計](contracts-observation-design.md)、[設計一覧](design/README.md) — 実装境界と検討案。未実装の案は現行仕様と区別する。
3. [開発工程](development-process.md)、[操作ガイド](guides/README.md) — 作業の進め方と手順。
4. [変更記録](changes/README.md)、[検証記録](verification/README.md) — 変更ID、検証対象と適用段階。
5. [調査記録](investigations/README.md) — 症状、保存証拠、未確定事項。[パケット欠落・切替遅延](investigations/packet-loss-investigation-20261022.md)はこちら。
6. [保存証拠](checkpoints/README.md)、[過去の草案](history/README.md) — 当時の状態を参照するための資料。

`changes/` の台帳、`checkpoints/` の証拠、および機械参照のあるルート契約は既存パスを維持する。古い文書を現行仕様として引用する前に、対応する要件・実装・変更段階を確かめる。生pcap、認証情報、個体の設定ファイルはGitに入れない。
