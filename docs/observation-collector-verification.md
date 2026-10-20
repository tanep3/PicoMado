# 継続収集器のオフライン受入（第2-A単位）

対象は [収集契約K-01〜K-05](observation-collector-contract.md)。第1段階の未受入prototypeから、区間確定・単一所有・再接続・保存・不確定操作の区別を実装した。**入力から描画までの因果追跡D-05全体はまだ閉じない**。server source、採取点、因果情報と解析は次の契約単位である。

## 実装

- `event_stream.py` の `Collector` 一つがserialとUnix IPCを所有し、`Window` が有界のsequence区間を検証する。開始応答が無効ならreadyにしない。
- EV1に任意のwire CID付き形式を加え、旧形式は維持。未送信取消し、送信後の結果不明、部分行のdirty接続、tag付きOFFを区別する。アプリ操作・AP2 arm・合成入力を自動で実行しない。
- 切断を含む採取全体は不完全。明示した有限再接続だけで新segmentを作り、旧コマンドは再送しない。同bootのsequence後退も拒否する。
- `Journal` は有限queue/byte上限、専用writer、定期/終了fsync、独立drop counterとdurable prefixを持つ。disk待ちでreaderを止めず、join期限超過や保存障害で成功を返さない。
- 破損行・途中行は上限内の元データと理由を保存する。既存output、summary、IPC pathを上書きしない。

## 試験と証拠

Git外の証拠は `/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/`。

| 試験 | 結果 |
| --- | --- |
| `event_collector_test.py` | 16件成功。実kernelの新規PTY/Unix socketを含む。断片受信、第二open拒否、IPC、client切断、通常終了/区間/fsync、seq欠落・重複、CRC破損・過長行、途中EOF、同boot/新boot再接続、同boot seq後退、低速保存/queue/drop/join超過、write/fsync障害、既存path保持、旧返信不明とtag付きOFF、未送信/部分コマンド、開始異常を確認。`collector-pty-tests.log`。 |
| `run_contract_tests.sh` | 480入力ケース、EV1/旧制御共存、984drop、容量0/7-byte write、source時刻/seq/終了、C encoder由来のhost解析6件が成功。`stage2-contracts.log`。 |
| 上記ASan/UBSan | 成功。`stage2-contracts-sanitize.log`。新規のreal-C境界試験でu64 source seqの飽和・非wrap、最大値でのtag付き返信144B未満を確認。 |
| `run_host_tests.sh` | 既存全体成功。実main・LCD/E-ink・動画・旧応答/ACK・EEX1・EOF52モード等。`stage2-firmware-regression.log`。 |
| ARM | `PICO_BOARD=pico_w`、ダミーsample-a、EV1 ON、AP2/SIM/TX/CY/stream/EEX1 OFFで成功。text390428B、BSS228664B。`stage2-arm-build.log`、`stage2-arm-size.txt`。 |

全PTYは試験内の `os.openpty()` で生成し、DTRを模擬するadapterだけを注入した。実USB、Pico/E-ink、ライブサービスには接続していない。OS sandboxのPTY/Unix socket制限のためローカル試験だけ実行範囲を変更した。失敗した試験結果もログで隠さず、最終再実行で確認した。

transport_completeは「この採取sourceの開始/終了境界とsequence、欠測、保存が揃った」という意味であり、操作成功・物理描画完了ではない。application_completeは未判定ならnull。SDK等の別stdout経路、実USB/割込み時間・動的高水位・実パネルはこの受入に含めない。既存Wire protocolや動画のACK/期限をこの単位で変更していない。

## 独立レビューの3件（2026-10-20）

契約追記 `9b67aa0` の後、K-06〜08を実装した。ownerごとのOS乱数CID seedと同ownerの単調採番、active完全送信要求のCID/state照合、空白tokenでの拒否、reply上限超過時の未送信queue取消しと通常dispatch禁止を追加した。

`collector-review-fixes-local-pty.log`：19件成功（2.555秒）。既存16件に、旧ownerのCID1/旧event/正しいON境界、誤ったon flag、SIM COMMIT同等空白6表現、reply行/byte上限→queue済みB取消し→遅着A→tag付きOFFを追加した。最初のsandbox実行はUnix socket作成EPERMのため失敗し、同じテストを新規PTYとUnix IPCだけで実行可能なローカル権限に切り替えて成功した。実USB、実端末、サービス、ネットワーク機器は操作していない。

これにより第2-Aの追加ブロッカー3件はソフトウェア試験上解消した。64-bit CIDは認証ではなく偶発衝突の可能性が残る。因果追跡D-05や実機間欠遅延の解消を意味しない。

再レビューP3：RNG取得をJournal worker/FDとselectorの取得より先へ移した。RNG例外注入時にJournal/selectorが一度も呼ばれず、出力ファイルが作られないことを追加検証した。`collector-rng-failure-regression.log`：新規PTY/Unix IPCを含む20件成功（2.614秒）。既存のK-06の「乱数失敗時は開始失敗」に従う初期化順序の是正である。実機操作なし。
