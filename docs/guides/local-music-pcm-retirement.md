# MUSIC-PLAYER-20261107: streaming移行後のPCM整理手順

これは実行前の手順。現時点で削除・移動は行わない。原本は保持する。親担当が新serverの反映・実再生確認後に棚卸し、対象一覧と影響を提示する。通常復旧できる保管と承認条件を満たすまでPCMを保持する。

## 候補判定（すべて必要）

1. local-musicの正規cache/audio直下の64hex key名の通常PCMファイル。symlink、形式不明、別プラグイン、設定、原本は対象外。
2. 現在のmetadataのkey/nameとの対応が一意で、data/importsに同名の通常原本がある。原本SHA256とサイズを記録し、key生成規則（local-music-pcm48-v1＋名前＋原本bytes）との一致を確認する。名前の類似だけで結び付けない。
3. 同じ原本が新readerで正常EOFまで復号できる。初回出音やffprobe成功だけでは末尾破損を否定できない。検証は有界readで行い、全曲PCMを再保存せずsample countと終了statusを記録する。音出しはしない。
4. 稼働serverのsource/artifact identityと、metadataのstreaming=true移行を確認する。旧PCM依存の再生specや旧readerが残っていないことを確認する。再生中legacy、移行保留は除外する。
5. 元PCMのサイズ（4byte frame整合）・SHA256、復旧可能な退避先、保持条件、復元方法を確定する。保管の完了とhash確認前に唯一のPCMを消さない。

**原本のないlegacy PCMは唯一の音源なので削除禁止。** metadata不在、hash不一致、decoder失敗、未移行、対応不明も保持する。「孤立ファイルらしい」だけで削除しない。

## 事前一覧

原本名/key、原本size/SHA、PCM size/SHA、metadata移行状態、全曲復号結果、稼働参照、退避先を曲ごとにまとめる。対象件数/容量、保護件数と理由、再生成のCPU時間と失敗時の影響を報告する。秘密設定は含めない。

## 実行と復旧

- 原本は終始保持。確認済みのTrash相当または保管場所へ、同一権限を維持して退避し、hashを照合する。同一disk内の退避だけでは容量を解放しないことを明示する。
- 親が対象と保管条件を確定した後、そのbatchのみを扱う。失敗・対象変化があれば止める。cache全体のglob削除、自動cleanupは追加しない。
- 復元は保管bytesを同じkey.pcmへ戻し、台帳のsize/SHAと照合する。同名fileが既にあれば上書きせず調べる。設定・原本・他曲は変更しない。
- 保管copyの完全消去は通常復旧できなくなるため、必要な別承認が揃うまで行わない。原本からの再生成可能性と、元PCMを通常操作で復元できることは別である。

server起動やScanの副作用に掃除を入れない。本番反映・再生確認・棚卸し・対象承認・復旧可能な整理を分離する。

## 配布前のrollback保全

初回Scanは原本付きmetadataをstreaming方式と推定durationへ更新する。旧serverはPCMの実際のframe数を必要とするため、旧binaryだけへの差し戻しで完全互換とはみなさない。移行前DB/設定も保全し、旧binaryと移行前stateを対応付けて復旧する。原本とPCMが保持されていても、metadata移行後のstateを旧実装へそのまま渡すことは別の検証が必要。

metadataの永続先はSQLiteのapp_state（id=1）のbody JSON内、plugin_states["local-music"]である。tracks/devices/next_generationを含む。移行で独立した永続JSONファイルは作成・上書きしない。cache/music-work-*/probe.jsonは専用一時領域の結果であり、終了・取消で整理する。稼働DBはWAL方式なので、生のDBファイルだけを不整合にcopyせず、SQLiteの通常backup手順で保全する。
