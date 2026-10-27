> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# 契約整理・第1実装単位の検証（2026-10-20）

時点の区別：本文の未修正・未受入は各保存点の記録。D01〜D05の最新状態、残る設計差と実機未検証は[最終照合](refactor-final-acceptance-20261020.md)を正本とする。

これは全リファクタ完了・本番反映の報告ではない。[既存との対応](implementation-conformance-20261020.md)、[既存設計の契約明確化](../contracts-observation-design.md)、[採用済み通信と設定](../../firmware/adopted-protocol-and-configuration.md) に対する最初の実装単位である。正しいQuery/Rule/純粋Transition/Effectの設計を維持し、エンジン自体へIOを追加していない。

## 変更した境界

- Picoの既存kind/typeを `device_api.h` のAPI表からwelcome・settings・入力結果・frame等の能力ハンドラへ振り分ける。受信・dispatch・pending/completed/rejected/errorを共通イベントで記録する。受信packetのTCP/WS形式、token、ACK、動画/静止画の選択条件は変更しない。
- ローカル入力判断を読み取り専用viewから `input_rules.h` の決定へ切り出し、端末操作を別関数で実行する。0/2/4ボタン、全8状態、sub/long/keyの480通りを旧規則と照合した。プラグイン選択判断をPicoに持ち込まない。
- `PluginSet` に対応するrules/contentの登録をまとめ、AppとWebの対象確認を登録契約へ結ぶ。静止画・動画・第三の追加fixtureを同じApp、frame/video配送、保存・再起動で検証する。個別IDの分岐は追加していない。固有描画を共通層へ移していない。
- Appの入力・判断・状態commitとeffect runnerの境界を、任意の固定容量Sourceへ記録する。純粋Ruleにはmutable observerを渡さず、sourceが満杯・競合でも待たない。feature既定OFF。保存の原子性やoutboxの完了意味を変更しない。
- EV1は既存traceキューを共用し、producerは固定値コピーのみ。明示ON後の通常記録を16件までとして返信枠を残し、sequenceとdrop範囲を保持する。USB writeは既存の非待機・部分送信。既存文字列の整形をIRQ mask外へ出した。EV1単独でAP2/SIMを必要としない。
- ホスト側はdecoder、有界line再同期、有限journal worker、単一コマンド多重化を実装した。応答期限超過後は遅着応答の誤対応を防ぐため制御をuncertainとし、自動再送しない。実serial ownerのCLIは**未受入prototype**で、実USB/擬似端末ライフサイクル、source区間の完全性判定、ホストとサーバsourceの統合は未完了。`complete=false`を成功へ書き換えていない。

## オフライン検証

ログはGit外の `$HOME/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/` に保存した。

| 検証 | 結果・証拠 |
| --- | --- |
| サーバlibrary、boundary-observation ON | 232件成功・失敗0。`service-regression-local-loopback.log`。模擬TCPはローカルloopbackだけ。 |
| 新規plugin登録・静止画/動画/保存の共通経路、feature OFF | 1件成功。`service-feature-off.log`。ON版でも同じ契約を確認。 |
| 固定source満杯・mutex競合 | 全体232件に含む。drop明示、入力・永続化の継続、再起動後の選択保持。 |
| 既存ファーム全体 | `run_host_tests.sh` 成功。LCD/E-ink、trace ON/OFF、動画、設定、応答先着/混在、EEX1、正常EOFの52モード等。`firmware-final-regression.log`。 |
| USB/TX/AP2既存回帰 | `run_usb_tx_tests.sh` 成功、既存Python28件を含む。`usb-tx-regression.log`。 |
| CYW43既存回帰 | `run_inner_tests.sh` 成功、Python13件とSDK-lock/driver adapterを含む。`inner-regression.log`。共有SDK不変。 |
| 有限controller | 16件成功。`controller-tests.log`。実機trialは実行していない。 |
| 実lwIPのNODELAY比較 | 42工程成功。`nagle-sdk-regression/summary.json`。変更前後・LCD/E-ink・ASan/UBSan、SDK SHA一致。無線やUSB実測ではない。 |
| 新規契約/EV1 | 480入力ケース、984dropの明示、64-bit時刻、USB容量0・7-byte write、制御混在、OFF/ONでseq保持、CRC破損・再同期、保存上限・fsync・応答不確定のPython5件が成功。`contracts-tests.log`。 |
| EV1と旧制御の共存 | EV1有効の共有キューでAP2 page/SIM/STOPを保持し、旧COMMITをrun変更後とDTR下降で再発火させない。上記契約試験に含む。 |
| 新規契約のsanitizer | ASan/UBSan成功。`contracts-sanitize.log`。ptrace下のLeakSanitizerは既存方針通り無効。 |
| 実ARM build | ダミーsample-a、`PICO_BOARD=pico_w`、EV1 ON、AP2/SIM/TX/CY/STREAM/EEX1 OFFで成功。text390116B、BSS228664B、data0。`arm-event-on`にELF/map/flags/SHAとstack値を保存。絶対にflashしないサンプル。 |

ARMの静的frame値はmain496B、pm_observe64B、pm_logf168B。予約main stack4096B、`__HeapLimit-__bss_end__`は28240B。ただしlibc/SDK callback/割込みの合成ピークや実heap高水位ではない。診断ONの既存実機候補へこの余裕を当てはめない。

最初のsandbox試験ではloopback bindのEPERMで13件が失敗し、旧negative比較用cfgのfixtureが実probeをwriterへ渡さないため2件が期限assertで失敗した。これらを成功扱いせずログを保持した。ローカルloopback実行を許可範囲で行い、fixtureを採用済みWriteProgressへ接続し、3秒/10秒/250msのassertを緩めず232件成功を確認した。初回のARM設定はboard指定不足、続いてEV1単独構成にACK関数依存があったため失敗し、それぞれを直した上で最終ビルドが成功した。

## 未完了・反映範囲

D-01重い取得完了、D-02同期全状態保存、D-03風刺画固有処理、D-04E-ink非同期受信、D-05入力から対応描画までの完全な因果追跡は未修正/未受入。EV1のenumが存在するだけで全境界に採取点があるとはしない。SDK由来を含む全stdoutの所有統合と、host collectorの実用的な保存区間確認も残る。transport単体成功を切替原因の診断能力完成と報告しない。

本番リポジトリは既存作業checkpoint `ffb62d6`以外変更せず、隔離した別リポジトリで作業した。元候補・採取証拠・個体config・共有SDKを保持。SSH、push、install、サービス再起動、ファーム書込み、実USB接続、E-ink操作は行っていない。実機の滑らかさ、間欠遅延、USB時間・IRQ時間・動的高水位、物理パネルは未検証であり、無回帰の絶対保証も遅延解消も主張しない。

## 第1段階の独立レビュー修正

E-ink通信画像3813BとパネルRAM変換後4000Bを区別した。`receive_chunk`から実際のAPI結果を返し、`api_frame`で受理失敗をCOMPLETEDへ変換しない。無効offset、u32超過、旧token、CRC不正をREJECTED、部分受理・動画待ち・E-ink描画待ちをPENDING、同期描画完了をCOMPLETED、ACK送信/描画失敗をERRORとして検証した。`run_api_result_tests.sh` は実main/handler/assemblyと既存IO doublesを使い、LCD・E-ink0/2ボタンの3構成で通常/ASan/UBSan成功。ログは `stage1-review-api-tests.log` と `stage1-review-api-sanitize.log`。実機の確認ではない。収集器の次段階作業とは別の修正として保存する。
