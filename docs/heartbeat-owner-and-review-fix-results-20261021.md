# heartbeat追加P2修正・単一owner統合の検証結果

独立レビューで指摘された3件を修正し、ARM_HBを既存の単一USB ownerへ統合した。隔離試験とreleaseサーバ候補の準備は完了した。**実機USB、ファーム書込み、サービス操作、SSH、pushは今回すべて0回**。間欠遅延の解消や実機観測の成功はまだ示していない。

| 固定対象 | commit |
| --- | --- |
| owner契約先行 | `8e3f93908a9ac9482693c4e634b00f89baa918b2` |
| P2修正契約先行 | `b1318fc465d653699aa3f812bdf76a31a8ad30c6` |
| P2 3件修正 core | `4b7987e0ab0912cad5b1e5132020fa9729e73a38` |
| runner・PTY統合 | `0b8bbe41217b8ca91667dcceb8e85869777f1d9b` |

契約は[owner統合](heartbeat-owner-integration-contract.md)と[証拠品質修正](heartbeat-evidence-review-fix-contract.md)。今回の3件に限り旧固定coreの変更が許可された。firmware runtime、SDK、個別設定、通常heartbeatと入力処理は旧レビュー対象 `660583b52f6d42d86289e663fade3472e347d10a` と同じ。元workの容量対策WIP6ファイルは元hashで保全し、候補に混ぜていない。

## 修正結果

1. `StreamStatus` のtokenからsession/frameを抽出する。同sessionの通常statusでheartbeat captureを停止しない。sessionを持たない未知messageの0と、実session変更を区別し、後者は従来どおり品質停止する。実WireMessageのencode→Decoder→identity→State回帰を追加した。
2. controllerとobserverの同じPi `CLOCK_MONOTONIC` の開始・終了bracketを保存する。START確認前にDTRをlowにしない。DTRから待機16秒＋観測20秒＋0.1秒を包含しない区間、1秒summary、早期終了、順序逆転、bracket欠落を拒否する。既存workerの`started`、`stop_command`、終了応答で区間を囲み、Pico時刻とhost時刻の単純差分を使わない。
3. offline CLI、collect、releaseを共通directory validatorへ統一した。`saved.json`、identity/ready等の必須JSON、raw headers/RecvQ、全SHA、frozen digest、品質を再検査する。5JSONだけの保存、必須原本欠落、改変、manifest不一致では完了を宣言せず、releaseもしない。保存できたoverflowは不合格のまま解放でき、未開始のpartial schemaは決してcompleteにならない。

runnerは既存`SerialPort`/`Commands`の排他FDを1回だけ開く。自然heartbeatを待ち、SIMや開始用の偽入力は送らない。観測中は同じFDを保持してUSB read/writeを0にし、対象tupleのヘッダobserverだけをpumpする。DTR前後の実bitとFD identityを照合し、失敗・timeout・partial commandを盲目的に再送しない。

正常・異常とも所有boot/runに限ってfreezeし、全原本の保存・検証後にdigest releaseする。保存失敗、USB状態不明、observer終了未確認ではRAMを保持する。再接続後の新bootへ旧runの操作を転用しない。保存できた品質不良は不合格のまま両端IDLEへ戻す。自動rearmはない。FDの実closeと最終IDLEを確認し、結果ファイル保存失敗も成功にしない。

## 試験と証拠

証拠root Hはworkの親 `heartbeat-capture-20261021`。新規PTYを使い、DTR bitと時計はfixtureでモデル化した。実機のioctl成否・負荷・容量の保証とは区別する。

| 確認 | 成功結果 | H内の証拠 |
| --- | --- | --- |
| Rust全features | 304 passed、失敗0 | `host-all-features-P2.log` |
| native USB/TX回帰 | 20 PASS行 | `runner-integrated-suite.log` |
| 既存Python export | 28件 | 同上 |
| heartbeat準備・原本・品質 | 22件 | 同上、`heartbeat-P2-python-final.log` |
| 共通証拠検証・実CLI拒否 | 4 test methods、欠落・改変matrix | 同上、`evidence-contract-final.log` |
| 単一owner runner | 12 test methods、26 scenario | 同上、`runner-PTY-final.log` |
| 既定planの無操作 | 出力dir作成0、USB open0 | `runner-plan-smoke.log` |
| 出自・保全・成果物照合 | 46項目成功、失敗0 | `verify-runner-candidate.py`、`runner-validation.json` |

runnerの26 scenarioには正常、PREPARED中の番号変化、DTR直前変化、古い未確認HB、COPY失敗、未到着、AP2/TX/CY/host/headerの各overflow、START未確認、observer終了timeout、DTR low/high失敗と到達済みbit・GET失敗、USB command timeout、host arm応答timeout、host障害、reconnect、保存失敗、最終report保存失敗を含む。Commands既存回帰でpartial command poisoningも確認する。新PTYの応答を7byteずつ分割し、open1回、観測中USB read/write0、FD終了を確認した。

旧coreで実施したfirmware host 175 PASS行、CY15 native/ASan/UBSan 8variant、CY collector13件、SDK実4関数18scenario、USB互換16件はfirmware不変のため継承する。件数を新規実行と合算しない。詳細は[旧実装結果](periodic-heartbeat-capture-results-20261021.md)。新規追加試験のred log、最初のfixture調整時の失敗logもHへ保持し、最終成功logと区別した。

## release候補と復旧先

| 成果物 | SHA256 |
| --- | --- |
| 新releaseサーバ `H/artifacts/picomado-server.heartbeat` | `cf452882458329c152f977906420eb52dcb2f44770ecc2432cd88e769588a761` |
| 継承LCD UF2 `H/artifacts/PicoMado-lcd1.uf2` | `9d4f66d1b71e07ef5c6e85b802201d0b6712b6290cd1c9133b40bdbf36479687` |
| 退避した通常サーバ `H/rollback/picomado-server.normal` | `e7b52c29d776bf9b9fbfea9125216c7b5c05613f47fadc9660618dedb76c727c` |
| 直前LCD復旧UF2 `SDK-align-20261021T032923Z/artifacts/aligned-observed/PicoMado-lcd1.uf2` | `11a11556e6e5c71b2f90fb98aeabca15132aabb5914b27d6ccebd217d7a5c485` |

新サーバはcore `4b7987e` から再buildしたaarch64 release。featuresは既存観測候補と同じdefault/stream-diagnostics/boundary-observation/ack-path-probe/caricature-retention-delete。動的依存が解決し、不正usageは設定・DB・listener開始前にexit1で終了する。`server-ready.json`とbuild/ldd/file/usage各logへ記録した。修正前に完成していた旧サーバ候補は `H/server-before-review-fix` へ区別して保存し、使用しない。

firmware sourceはレビュー済みUF2と同一のため、ARM成果物SHAを継承し再buildしなかった。静的RAM252072B、linker heap余地10072B、stack予約4096B、設定sector非書込みの証拠も継承する。動的heap・実stack高水位をこの数値で保証しない。SDK2.3.1の公式gitlink6件、private設定hash、容量WIP6hashを再照合済み。現配置の通常サーバbinaryと設定hashも変化なし。

## 残るgateと次工程

新しい固定core/runnerの独立レビュー、実機のobserver effect、動的資源、CY15の任意20秒全量収容は未成立。16行目以降のoverflowを無視せず不合格とする。根因調査の追加観測用候補であり、遅延修正・解消を主張しない。

将来の承認された実機工程では、対象LCD Flash ID `E661640843239D21`、候補・復旧UF2、設定sector非書込みを再照合する。サーバは既存runbookに従って停止時の設定・DB・素材を整合退避し、設定とruntime flagsを保ったまま候補へ切り替える。新binaryと接続を確認してから `H/runner-config.template.json` のPID/owned tupleを再取得する。templateのPIDはnullで、そのまま実行できない。observer2ファイルは `H/runner-observer` の既存版SHAへ固定した。

runnerは既定がplanで、実採取は明示`--execute`の1回有限窓。採取後は正規freeze/save/両端IDLE/owner終了を確認し、既存の承認範囲で通常サーバへ復帰する。サーバ異常時は上表の旧binaryへ戻す。DBの任意巻戻しはせず、LCD復旧が必要なら上表11a11556…を使う。E-ink、ai-news、OS/ネットワーク設定は対象外。今回これらの本番工程は実行していない。

機械可読の最新引渡しは `H/runner-ready.json` と[checkpoint](checkpoints/20261021/heartbeat-owner-review-fixes.json)。旧 `ready.json` / `handoff.json` は660583b当時の履歴として保存し、旧文書の「runner未統合・server未build」という準備状況を今回結果で更新する。
