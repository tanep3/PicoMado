# 自然heartbeat capture：隔離実装・検証結果

P2の対象混同対策を[契約](periodic-heartbeat-capture-contract.md)へ先行固定した後、独立worktreeで実装した。文書先行 `07f5c062523e3d3396d380082bf158dc8c6bb241`、初期実装 `042044c0a68a3e8d49a5ea02b2b79fe04d203695`、最終実装 **`660583b52f6d42d86289e663fade3472e347d10a`**。branchは `diag/heartbeat-capture`、workは `/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/heartbeat-capture-20261021/work`。

実機USB/書込み/サービス操作/SSH/pushは0。テストはホスト内のfixtureと新規PTYだけで、実USBの代用として偽入力を送っていない。容量WIP6ファイルは元workに元hashで保持し、overlay/serializer等を候補へ含めていない。

## 実装した境界

- Picoは明示的な `AP2 ARM_HB` の後、DTR lowを経て次の自然kind5のbefore_lockでevent25を1回だけ保存し、そのheartbeatのCOPY前から既存20秒を記録する。通常ARM/物理/SIM開始とheartbeat周期・送信bool・app設定は変更しない。
- ARM origin、DTR gate、対象message、異常flagsを固定metadataに保存する。PREPARED中送出、予期しない入力、接続世代変化、ordinal wrapを品質失敗として残す。新Caps/Trigger replyも既存の有界応答形式を使う。
- hostは対象の処理済みordinalをtry_lockで保持し、heartbeat arm時にPico originと同じbaseline・expected=baseline+1を同じlock内で確認する。正しい番号/kind5/session/bindingだけで仮開始し、別番号・別kind・再接続は品質停止する。旧readerが新bindingのwatermarkを更新しない。対象の最終認定はPico COPY範囲/CRC/長さ/番号/世代とhost Decodeを照合し、初期unacked/unsentや曖昧さを受け入れない。
- CYは単純な全量15行。対象tupleのdata初回・再送・異常を既存の各境界で記録する。選択範囲による除外は追加せず、skip/drop/rejectなど全counterとCRC coverageを保存する。16件目はdropとなり、全窓をincompleteとする。
- `firmware/tools/heartbeat_capture.py` は既存単一owner向けのprepare/observe/collect/releaseと専用validatorを提供する。EV1は変更せずOFFとsequence不変を検査。観測中USB通信0、待機上限16秒+観測20秒+0.1秒、ヘッダobserverの開始/終了は注入された既存ownerのcallbackで扱う。CLIは保存ファイルのオフライン検証だけで、USBを開かない。
- 保存したAP2/host/TX/CYのdigestとraw headers.jsonl/recvq.jsonlのSHAを検査してから保存確定する。品質不良でも証拠を保持し、保存前のrelease・自動rearm・品質条件の迂回は行わない。部分準備やtimeoutはidentityとRAMを保持し、既存ownerのエラー処理でfreeze/saveしてからcloseする統合が必要。

## 試験

証拠root Hはworkの親 `heartbeat-capture-20261021`。成功した最終logを以下へ固定した。PASS行は独立試験数ではなく、境界matrixをまとめた行も含む。

| 確認 | 結果 | 証拠 |
| --- | --- | --- |
| host全体、ack-path-probe有効 | Rust 286 passed、失敗0 | H/host-full.log |
| firmware既存host回帰 | 175 PASS行、終了0 | H/firmware-host.log |
| 既存USB/TXと新HB状態/実net経路 | 20 PASS行、既存Python28成功 | H/usb-tx-complete.log |
| 専用validator/準備・保存・解放 | Python22成功。実PTY分割応答・TIOCEXCL・FD終了を含む | H/heartbeat-python-PTY-final.log |
| 既存USB SIM collector互換 | Python16成功 | H/usb-trial-compat.log |
| CY15 core、wire、実TX adapter、実HB net経路 | 各nativeとASan/UBSan、計8variant成功 | H/extra-tests/results.json、各log |
| 既存CY collector | Python13成功 | H/extra-tests/collector.log |
| 整合SDKの実4関数、ON/OFF比較 | 18シナリオ、native/ASan/UBSanでbus bytes/戻り値一致 | H/CY-tests/result.json |

新C統合testは実ack_probe、pm_net_send、COPY、tx_sync、同期linkoutput、CY entry/prebus/finishと後日の再送を接続し、元hookへの転送1回を確認した。ERR_MEM時はCOPY/TXを捏造せず、既存falseを維持した。CY15のticket slot15、16件目drop、FROZEN/partial出力保持、threaded callback/snapshot/reset、144B wire応答も確認。

P2回帰はPREPARED送出、古い番号、期待番号の別kind、未来/重複番号、同じCRCの後続HB、初期unacked/ACK遅れ、COPY失敗、未到着/遅着、接続世代変更、watermarkと旧reader、USB overlap、全drop/generation/digest/partial READ/保存失敗を扱う。全品質条件を保持し、失敗を後続heartbeatへ付け替えない。sourceのtry_lock競合時は既存contention品質またはbaseline不一致で拒否する。実負荷でのobserver effectは未測定。

テスト追加の初回には、CY wire fixtureに不要なTX flagを付けたリンク失敗、net fixtureのinclude順でrand stubが二重定義になったcompile失敗、PTY mockのmodule参照ミスがあった。いずれもfixture側を補正して成功し、初期logをH/extra-tests/initial-CY15-wire-wrong-TX-flag.log、H/initial-heartbeat-net-stub-order.log、H/heartbeat-python-PTY.logに保持した。実機/SDK原本の不具合や失敗操作とは区別する。

前段の進捗ファイルに記した278 PASSは以前候補の集計だった。**今回のfirmware host回帰の実測集計は175 PASS行**で、Rust286や専用22件とは別に数える。

## ARM成果物・出自・資源

SDK本体 `079c6f39023649b154152db30f1d781e884879bc` と公式整合submodule6件を維持し、全repo clean。private LL SHA `d1a5a9a01a95ee24422d29cd741ef6eeb07facf7de31f9d1ab8c868f758295d8` も前候補と同じ。個別config.h/board-id.txt、全build flags、main.c/net.c/CMake/lwipoptsのhash一致を確認した。整合SDKに合わせた診断適合5ファイルは固定実装へ含めて再現性を確保した。

| 項目 | 値 |
| --- | --- |
| UF2 | H/artifacts/PicoMado-lcd1.uf2 |
| UF2 SHA256 | `9d4f66d1b71e07ef5c6e85b802201d0b6712b6290cd1c9133b40bdbf36479687` |
| 静的main RAM | 252072B（前候補251048Bから+1024B） |
| heap linker余地 | 10072B（8192B下限以上） |
| core0 stack予約 | 4096B |
| CY probe object増分 | 1008B（7行×144B） |
| AP2 probe object増分 | 16B（新metadata構造20Bは既存paddingも使う。native総量と区別） |

LCD本体・selftest・backlight-polarity targetのARM buildは成功。全UF2 blockのRP2040 family・index・範囲と設定sector非書込みを検査した。flags/SDK/設定/source/容量WIP保全・全UF2 block・資源の1783照合成功（ホスト試験件数とは別）。H/verify.py、validation.json、ready.json、configure.log、ARM-final-build.logに再現可能な証拠を残した。動的heap/実機stack highwaterをこのlinker値で保証しない。

## 未成立のgateと次の操作範囲

CY15でも20秒の任意な再送/自然frame更新を全て収容する保証は未証明。上限超過は従来どおり品質停止する。実機のobserver effect、IRQ/lock高水位、入力poll/ACK/描画の同条件比較は未実施。実機gateは閉じたままで、今回結果は間欠遅延の根因修正・解消を示さない。

次工程は固定sourceの独立レビューと、既存単一owner/owned-socket observerへのcallback統合・失敗時freeze/save/closeの照合。host sourceは全回帰済みだが、新しいreleaseサーバbinaryは今回buildしていない。実機承認前に必要なserver候補を準備し、設定/flags・退避・復旧先を照合する必要がある。

自然HB triggerには将来LCDファーム更新とhost側の新metadata/arm機能が必要で、サーバだけでは完結しない。対象は引き続きFlash ID E661640843239D21のLCDだけ。新候補を書いた場合の直前復旧先は現在書込み済み整合版 `SDK-align-20261021T032923Z/artifacts/aligned-observed/PicoMado-lcd1.uf2`、SHA `11a11556e6e5c71b2f90fb98aeabca15132aabb5914b27d6ccebd217d7a5c485`。fd19…はそのさらに前の履歴。今回flash/service更新は未実施で、E-ink/ai-newsにも変更なし。

[機械可読checkpoint](checkpoints/20261021/periodic-heartbeat-implementation.json)。
