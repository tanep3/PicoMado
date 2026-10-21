# ON/OFF原子性・重複beginのP2修正

対象は `58b6d14` に対する独立レビュー2件。設計・検証条件だけの先行コミットは `1b39771caf855e10049613536e60046674f1f1b3`、修正の固定コミットは `4563976f35d894359337bd75da8e600ae7d5240c`。著者側の修正・ローカル検証は完了し、再レビュー待ちである。旧実装を独立レビューで受入済みと扱わない。

[checkpoint JSON](checkpoints/20261021/observation-runtime-gap-p2.json) にreview/source/log/artifactのSHA、構成と資源量を保存した。既存の [初回実装](observation-runtime-gap-implementation.md) と [実測](manual-switch-analysis-20261020.md) は履歴として維持する。

## 修正と実行証拠

- **P2-1**: `usb_control.c` の有限helperがON/OFFのenable変更、境界予約、返信watermarkのsnapshotを同一IRQ禁止区間で確定する。既存の入れ子helperは外側の禁止を維持し、整形・enqueue・USB IOは復帰後に行う。新しいqueue・heap確保・SDK再入はない。実trace/control/serializerの同一core IRQモデルで、begin=1、直後のNetwork=2、ON返信=1、end=3、OFF返信=3を保存し、OFF復帰直後のproducerは新seqを作らないことを確認した。full時の境界dropも維持し、整形/enqueue/USB writeがIRQ禁止区間へ入らないことをassertした。
- **P2-2**: `Window` の既存32件preludeへfactと元のhost sequenceを有界に保持し、ON応答で現在区間を特定した後、beginを一度だけ受理する。応答後のbeginも同じ受理処理を通る。重複は `duplicate_begin`、後続事実の後から来た初回beginは `begin_reordered` とし、準備・最終完全性をfalseにする。最初のbeginとそのdurable prefixの根拠を後の重複で上書きしない。内部preludeの照合を、入力の二重受信として数えない。

応答前/後/前後にまたがるCRC有効な重複beginを単一chunkとbyte分割で検証した。正常begin1件は応答前後とも成功する。新規PTY6ケースと直接Window/Linesの10ケースで、重複時はcausal_ready通知0回、causal_complete=false、品質エラー1件となり、canonical interval_sourceと一致した。準備完了後に遅い重複が到着した場合も、その後のstatusと最終品質をfalseに落とす。既に出した過去の通知を無かったことにはしない。

独立レビューの元のfresh-PTY再現も修正済み固定コードに対して再実行し、1.15秒採取・fsync後も通知0回、不完全、canonicalも不完全と確認した。保存先はtaskの `gap-p2-evidence/duplicate-verified/device.jsonl` とsummary。IRQの生byte列は `gap-p2-evidence/irq.events` に保存した。どちらも物理端末のログではない。

## 検証結果

| 検証 | 結果 |
| --- | --- |
| firmware契約 / ASan・UBSan | 全成功。新しいIRQ境界・full・整形位置、既存COPY相関、両観測有効時のAP2一回転送、drain/idle、飽和、LCD/E-ink因果経路を含む。LSanは既知のptrace制限で無効 |
| firmware既存host | 175 PASS項目、exit 0 |
| collector | 27試験成功、exit 0。上記6新規PTY/10直接ケースとlate/reorderを含む |
| 因果parser / USB・TX | 10試験 / Python28試験を含む全成功、exit 0 |
| ARM | LCD ON/OFF、E-ink_0 ON、E-ink_1 OFFの4構成成功、実行session exit 0 |
| server | 全source SHAが既存固定版と一致。297/all・273/default＋設定1件の旧検証とreleaseを継承し、長時間動画試験とserverビルドは今回は再実行していない |

補助証拠保存の準備時に、出力先の相対パス誤りとUnix socketパス長上限を検出した。前者は絶対出力先へ修正、後者はcommand送信0件を確認後、短い新規 `/tmp` IPCパスと別の出力先で再実行して成功した。最終の未解決失敗・権限待ちは0。

| 構成 | 静的main RAM | 旧固定版からの差 | リンカheap可能範囲 |
| --- | ---: | ---: | ---: |
| LCD ON | 251,064B | 0B | 11,080B |
| LCD OFF | 250,376B | 0B | 11,768B |
| E-ink_0 ON | 181,208B | +8B | 80,936B |
| E-ink_1 OFF | 180,612B | 0B | 81,532B |

全構成のstack予約は4,096B。LCDの関数単位 `.su` はcommand.constprop 608B、response 160B、main 592B。call chainやIRQ併用時の高水位ではない。私有設定・bindingは退避版と一致し、TCP相関・hook・server sourceの未変更SHAもcheckpointへ記録した。新候補は `gap-p2-build` に分離し、旧候補のartifactを上書きしていない。

SSH・USB open/command・実機更新・サービス操作・pushは実施していない。同一core IRQ注入は実機で当該競合が発生した証拠ではなく、実IRQ時間、動的heap/stack、RF、物理遅延改善は未検証。最初の観測以前と厳密なraw→WS対応がunknownである既存契約は維持する。
