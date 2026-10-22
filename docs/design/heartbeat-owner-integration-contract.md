# ARM_HBの単一owner統合契約

実装本体は `660583b52f6d42d86289e663fade3472e347d10a` に固定し、firmware/host/heartbeat_capture.pyは変更しない。この契約を先行コミットしてからrunnerとPTY試験を追加する。対象は既存 `event_stream.SerialPort` / `Lines` / `Commands` の単一owner、自然HBのみ、EV1 OFF・対象tupleヘッダのみ。USB/実機/サービス/SSH/pushの実行は今回しない。

## 所有・開始・対応

1回のownerが1個のTIOCEXCL FDを保持する。既存Commandsを使うAdapterを `heartbeat_capture.prepare/collect/release` へ渡す。DTR high/lowも同じFD上で行い、観測中はUSB read/write/pumpを行わず、別reader/SIMプロセスを作らない。計測対象入力も偽の開始入力も0回。

準備は `idle→Pico ARM_HB→host baseline/expected arm→ready`。identityはARM前に保存する。readyがなければ観測に進まず、部分armを破棄せず正規freeze・保存する。origin/host ordinalのrace、準備中送出、DTR境界での送出は660583bのflags/番号/COPY検証に従い、後続HBへ付け替えない。ready後にowned-socket targetのbindingを照合し、observerを準備・開始してからDTRをlowにする。

## 状態と失敗時

| 状態/結果 | 次の操作 | 保存・解放 |
| --- | --- | --- |
| 未ARM/拒否 | 観測しない | ownerを閉じ、拒否理由を保存 |
| Picoまたはhostの部分ARM | 現在boot/run/phaseを読み、所有するrunだけfreeze | AP2/host/TX/CYを可能な範囲で保存。完全な保存が検証できた端だけdigest release。無関係runへ触れない |
| ready、DTR low成功 | observerだけpumpし、最大36.1秒の有限窓を待つ | USB read/write0、同じFD保持 |
| DTR low/high失敗 | 同じ切替を盲目的に再実行しない。ioctlの実状態をread-onlyで照合 | 状態不明ならUSBによる追加commandを送らず、host/observerをfreeze/保存し、Pico release未確定として報告 |
| command timeout/partial write | Commandsのuncertain/dirtyを尊重。再送・別reader・再openなし | 相関できないPico commandは追加しない。host/observer証拠を保存し、Pico RAMを保持したまま未完了を報告 |
| peer reconnect/boot/run変更 | quality stop | 新個体/新runへfreeze/releaseを転用しない |
| completeまたはoverflow等の品質不良 | 正規freeze→全証拠保存→SHA/digest検査 | 品質を保持したままreleaseし、両端IDLEを確認。次runへ自動rearmしない |
| READ/保存失敗 | releaseしない | 保持場所・残phaseを示してowner終了。データを捨てない |

DTR制御はTIOCMGETを前後に読み、FDのdev/inodeとfd番号が不変であることも確認する。失敗でも読み取ったbitが要求値ならその事実を保存し、error自体は成功へ読み替えない。状態が元の値なら観測開始せずcleanupに進む。状態不明のとき、close以外に新しいUSB制御を送らない。closeは新しいopenや端末resetではなく、保持FDの後始末として1回行う。

observerは既存guard/workerを使い、prepare≤20秒・capture≤40秒、対象tuple/owned socketのbindingを固定する。正常/異常とも正規stopとguardの終了を確認し、payloadを保存しない。raw headers/RecvQとmetadataを保存し、sample gap/drop/overflow/scopeを品質に反映する。開始/終了やDTR failureを隠さず、capture前のobserver準備失敗を計測成功にしない。

## 試験条件（先行固定）

- 新規PTYだけに接続し、既存SerialPort/Commands/Adapterを通してpartial response/byte splitを検証。open回数1、観測中USB read/write0、同じFD、終了後closeを確認する。
- 正常HBのorigin一致と正規freeze/save/release/両IDLE、PREPARED中HBによるhost先行、ARMとDTR直前番号変化、古い未確認HB、COPY失敗、未到着・遅着、reconnectを検証する。
- DTR low失敗/high失敗（bit不変・要求bitへ到達・GET失敗）、controller deadline、USB断、command timeout、partial command poisoningを検証し、盲目的retry/reopenをしない。
- AP2/TX/CY/host/header overflow・欠測・CRC/digest異常は品質不良を保存し、完全性を宣言しない。保存できないときreleaseなし、保存できた不良runは安全にIDLEへ戻し、再試行しない。
- 660583bのfirmware/host/toolsのhash不変、容量WIP6hash不変、SDK/個別設定不変を照合する。runner差分と文書を別commitで固定する。

## releaseサーバ候補

同じ固定host sourceからoffline releaseをbuildする。前の観測候補と同じcompile features（stream-diagnostics、boundary-observation、ack-path-probe、caricature-retention-delete、default）をfingerprintで照合し、runtime設定やflagsは変更しない。候補binary/hash/architecture/動的依存と、設定/listener開始前の不正usage終了を検証する。install/起動/切替はしない。LCDの直前復旧UF2は現在のSDK整合版11a11556…であり、新候補9d4f66…は未書込み。
