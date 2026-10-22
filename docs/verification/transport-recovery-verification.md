# 統合診断・期限/FIN回復の反映と実測（2026-10-19）

統合診断と送信期限・正常EOF回復を独立レビュー後に本番へ反映した。設計・事前試験は[stream-diagnostics.md](../investigations/stream-diagnostics.md)、[transport-recovery.md](../design/transport-recovery.md)、[main-loop-recovery.md](../design/main-loop-recovery.md)を参照。これらの「未反映」は事前レビュー時点の記録であり、以下が反映後の結果。

## 反映した内容

- サーバは同一接続の正のAsyncWrite受付を進捗とし、無進捗3秒・動画全体10秒・制御猶予250msを分ける。進捗はカーネル受付でありTCP ACK/端末到達ではない。
- LCDは正常FIN/Close後のコピー済みデータを最大1.5秒まで有限に処理する。期限後の新しい描画を禁止し、正常終了時は有効な物理画像を保持してownerを無効化する。hard/protocol error・ユーザー操作・表示失敗の解除方針を維持する。
- 固定RAM/キューの統合診断と安全な前後USB snapshot、読み取り専用ホストcollectorを追加。通常ターゲットでは診断を既定OFFにする。

反映時のサーバSHA256は `75a0fa9d25b0a04944b97427e465ca21922dfcc71e66e85f57deb57fcd7139bb`。LCD UF2は `8d651b9bf0c6d5319cbea05c90164a18b5c8d41c9d00bd07291d60eaff196066`。既存guardで指定LCDを1回load/verifyした。E-inkのflash、端末選択・有効化・共通順序の変更は行っていない。秘密設定、生成バイナリ、実機privateログはリポジトリへ追加しない。

## 検証

- ファーム52ケースが診断ON/OFF/ASan+UBSanで各52/52成功。各構成284件の実main遅延・操作・エラー試験、既存全host/E-ink/PM/EEX試験も成功。LSanはexecutor制約で無効。
- TR1〜TR6の独立再現はすべて解消し、最終独立レビューに阻害事項なし。LCD診断ONはtext401260B、BSS234120B、linker gap22784B。実機stack/heap最大使用量は未測定。
- サーバON全222件成功。OFF一括は217/218と単独retry成功の記録を維持する。外部process-group試験は独立比較で旧新版とも199/200、同じassert失敗という既存不安定性があり、全件一括成功とは扱わない。

## 同じ定義による改善比較

HOME、Thriller `sOnqjkJTMaA`、12fps設定、PM none readback、同じcache key、E-ink_0=caricature/E-ink_1=clockを維持した。測定の開始・中間・終了で条件一致。

| 指標 | 以前 | 反映後 |
| --- | ---: | ---: |
| MCU動画draw数/実計測秒 | 990/182.542196 | 1376/181.044173 |
| 再接続込み実fps | 5.4234 | 7.6004 |
| host約180秒の切断回数 | 7 | 1 |
| offline推計秒 | 50.6125 | 5.1204 |
| 描画gap最大秒 | 14.333822 | 7.103195 |
| 描画gap >=1秒 | 23 | 23 |

host窓は以前180.062694秒、反映後180.046971秒。MCU/host/server集計窓は異なる。単一のbefore/after比較であり、改善量を各変更へ厳密に分解できない。サーバ再起動でplayback generationは37→39へ進んだ。

残った切断はstream_write_stallが1回。個々の動画SPIは最大38.558ms、組立は最大3.028706秒で>=1秒が23回。送信背圧・再送とMCU受信進捗の途切れが残る。AP/RF/CYW43/IRQ/lwIP/ACK戻り/source schedulingのどこかは未確定で、12fps回復や根因解決を主張しない。

物理画面保持の実機映像や保持flag自体は採っていない。全LCD drawと動画drawの差は21→3でstatic/local描画減少と整合するが、保持そのものの証明ではない。描画gapは消画時間とは区別する。

前後USB snapshotは完全・DTR lower/close成功。host/journal収集は正常終了し、収集process・対象USB descriptor・300秒上限server診断threadの残存なし。稼働PID/hashを確認した。定型画像比較は別の準備作業であり、この反映・実測・コミットには含めない。SSH・新しいpacket capture・測定中の追加flash/restartは行っていない。
