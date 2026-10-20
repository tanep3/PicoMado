# リファクタリングの最終照合（2026-10-20）

実装固定点は`52d8fddfd301f7f79c092c1ceca10d3c9fbb30f6`。本書は着手時の[乖離一覧](implementation-conformance-20261020.md)と[段階契約](contracts-observation-design.md)を照合した最新の索引であり、過去の試験記録・未実装要件を上書きしない。本番の基準は`ffb62d6`、今回の実装は隔離branchにある。本番・実機・サービス・SSH・pushは操作していない。

## 受入状態

「受入済み」は列記したソフトウェア範囲の独立確認を指す。実機の間欠遅延解消、実USB負荷、実パネルの合格を含めない。

| 追跡 | 実装と固定点 | 現在の状態・証拠 |
| --- | --- | --- |
| D01 重い取得完了 | prepare/apply、session再検査、native画素の事前生成。`6c9fe32` | 独立受入済み。23関連＋2追加の確認。[検証](fetch-completion-isolation-verification.md)。未知拡張の同期defaultには同じ隔離を保証しない。 |
| D02 保存待ちと一時操作 | 保存成功後の公開、独立query/HTTP/ACK/browse/配信/時計、一時view合流、成功prefix、表示identityでの義務引継ぎ。`a6c7cf4` | 独立受入済み。31関連＋LCD/native画素比較。[検証](application-live-state-verification.md)。永続選択は保存を待つ。 |
| D03A 起動・参照 | 登録済みstartup、pluginが解釈する同plugin未完了job。`53cb612` | 独立受入済み、11関連確認。[検証](plugin-lifecycle-boundary-verification.md)。 |
| D03B 期限整理・外部作用 | prepare→純粋計画→印→SQL→明示公開→finish。時刻・保護・32件・再開はplugin所有。`52d8fdd` | 独立43試験で受入済み、追加指摘なし。SQL失敗後再試行・台帳待ちHTTP保護更新の公開再検査・実起動hook復旧も確認。[検証](plugin-maintenance-verification.md)。永続変更はfinish終了まで待ち得る。 |
| D04 E-ink描画中pending | 既存assemblyを再利用、世代/CRC/metadata、同一token再送のdispatch時合流。`bae0575` | 独立受入済み、既知P2解消・追加指摘なし。[検証](eink-pending-frame-verification.md)。自己stack frame192Bは実機高水位ではない。 |
| D05 採取・保存・因果解析 | 単一owner、CID、区間/欠測/永続prefix、任意cause、境界事実、collector成果物の解析。是正`1cbb992` | ソフトウェアの5是正は独立確認済み。[収集](observation-collector-verification.md)・[因果](observation-causality-verification.md)・[追加契約](observation-causality-review-corrections.md)。D05全体の実機受入は未完了。 |

最終server全体は全feature294/294（89.81秒）、通常273/273（87.61秒）、bin1、保存済みJMA4/4、通常lib/bins/tests check成功（24.69秒）。各結果は`d03b-final-policy-all-features.log`、`d03b-final-policy-default-offline.log`、`d03b-final-check.log`。D04のhost/API/EV1/sanitizer/ARMは`HANDOFF-D04-REPLAY.md`にあり、親が独立再確認した。

## 動画・Pico設定の照合

[採用済み通信と設定](../firmware/adopted-protocol-and-configuration.md)を`protocol.rs`、`lcd_stream.rs`、`transport.rs`、`main.c`、`net.c`、`video.c`、`settings.rs`、`CMakeLists.txt`と照合した。

- TCP/WS/applicationの境界、v1 kind、JSON制御、任意観測sidecar、native kind18、token/offset/CRCを維持。LCD160×80 RGB565BE/25600B、E-ink250×122 mono1/3813B。パネルRAM4000Bとwire長を混同しない。
- 新LCD動画は3能力の申告時だけ最新画像のACK非依存配信。送信中1枚は不変、未送信は最新1枚、旧LCD/E-ink/静止画は従来ACK。無進捗3秒、動画全体10秒、制御猶予250ms、静止画はwire長から最大20秒という既存期限を変更していない。kernel-write受付は対向到達ではない。
- LCD限定NODELAY、Wi-Fi PMの要求/readback、正常EOFの有限drainと画素保持、機能時計、frame/応答先着、Hello順序は既存回帰を維持。短い正常区間から間欠遅延の不存在を結論しない。
- 設定初期値はWi-Fi15秒、アプリ10秒、再試行5〜30秒、heartbeat15秒、offline45秒、2ボタン長押し1800ms。これは新規/破損時の初期値であり、保存済み設定を上書きする指示ではない。改訂・incarnation・適用/不揮発保存確認を維持。
- 個体config/board binding、LCD4・E-ink0/2ボタン、通常stack4096B、LCD SPI/色/回転/offset、E-ink SPIとパネル版を変更していない。個体設定と実build flagsが正本。sample、UNBOUND、別診断候補のメモリ値や名前を実機設定として流用しない。
- 時計の基本画像はサーバ側。E-inkの全面更新間隔は`eink-clock-v1`の時だけ有効で、旧ファームは毎分全面更新。[時計要件](requirements/plugins/clock.md)の既存選択・有効化保持も全体回帰に含む。

## 汎用ログ：採取から解析まで

1. Picoの`PM_EVENT_STREAM`とサーバの`boundary-observation`＋新規`PICOMADO_OBSERVATION_PATH`が必要。通常構成では自動採取しない。サーバ保存開始失敗でもアプリは動くため、プロセスの生存だけで採取準備完了としない。
2. `event_stream.py`の1 ownerだけがserialを開き、IPC `status`の`ready=true`と正しいON境界を確認してから操作する。有限時間上限を必ず指定する。既存AP2/SIMを自動armしない。別readerを追加しない。
3. 再現したら同じownerへIPC `stop`を送り、tag付きOFF、watermark、fsync、summaryを待つ。stop要求の受理は保存完了ではない。Ctrl+C/killや抜線を正常な区間確定の代用にしない。再接続は明示した有限回数だけ、通常commandは再送しない。
4. 正規入力はdevice JSONL＋そのsummary、server JSONL＋SHA-256付きsummary。serverの確定は現在のhost終了処理に結び付く。deviceを止めただけでserver summaryまで確定したとはしない。host終了が必要な採取計画はサービス操作の別承認を要する。
5. `event_causality.py`がCID/boot/link epoch/sequence/欠測/durable prefix/catalog/coverageを検査する。source coverageはPico`0x9f`・server`0xeb`。raw EV1はfixture互換で、collector保存確認の代用ではない。複数segmentは現在の解析入口では曖昧として不完全になる。
6. 同sourceの入力→対応frame描画時刻だけを減算する。`rendered/slow`、明示`explicit_failure`（REJECTED/ERROR）と`cancelled`、完備区間の`missing_after_deadline`、`unknown/incomplete`を分ける。閾値は解析policyであり、アプリのtimeoutを変更しない。no-opを無条件に画像欠落としない。

コマンド・終了条件と保存物の詳細は[反映・採取手順](refactor-rollout-runbook.md)。CLIの`--help`はローカルで照合した。最終固定点でも解析9/9（0.940秒、`final-audit-causality.log`）、collector21/21（2.802秒、`final-audit-collector.log`）を再確認した。collectorは新規PTY/Unix IPCだけで、実serialは開いていない。

## 残る設計差・別範囲の未実装

| 区別 | 残る事項 |
| --- | --- |
| 保存構造の設計差 | schema2は単一JSON行の同期serialize/SQLite保存を維持し、一部snapshot（風刺画latest等）に画素配列が残る。設計の「小さい索引・不変素材参照」への全面移行は未完了。[保存設計](caricature-storage-retention.md)が別改訂として明記する範囲。D02の一時経路分離やD03B完了で消えたとはしない。 |
| 観測の未実装/未受入 | server socket内部境界、native FrameBytes入口固有のAPI operation fact、SDK等の全stdout所有統合は未完了。未登録bitを立てず、不明を失敗へ変換しない。実USB/IRQ/stack高水位/RF/物理画素は別受入。 |
| 元から未実装の機能 | サーバ汎用メニューは[提案](../firmware/protocol-proposal.md)のまま。ローカルHOME/AWAYやSUBを代替の完成証拠にしない。Bluetooth音声等のWantも本改修の完了範囲に加えない。 |
| 実機受入 | このcandidateの長時間/複数実機/HOME・AWAY/動画/切替/再接続、E-ink波形・残像・電力、採取ON/OFF負荷、間欠遅延の再発記録と原因は未検証。過去の採取や過去の利用者報告を新しい再現証拠に数えない。 |

残る設計差を消すためのschema変更・新API・専用freeze検出器は追加しない。今回の独立レビューで新しい不具合が出た場合は、その是正を優先する。

文書の最終検証：変更13文書のローカル参照に欠落なし、手順のbash9ブロックを構文確認、埋込みPython2本を構文確認し、SQLite退避例は合成temp DBで整合性と重要設定保持を確認した（`final-audit-docs-validation.json`）。手順内の本番配置・サービス・実USBコマンドは実行していない。runtime差分は`52d8fdd`から0。
