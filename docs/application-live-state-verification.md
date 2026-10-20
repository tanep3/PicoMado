# D-02 一時操作と保存待ちの分離・検証

時点の区別：本文の未修正・未受入は各保存点の記録。D01〜D05の最新状態、残る設計差と実機未検証は[最終照合](refactor-final-acceptance-20261020.md)を正本とする。

保存境界の設計を `1082a8a`、内容更新と入力失効の区別を `5aeb57f`、観測上のcommitの区別を `3edf715` として実装より先に記録した。その後、同じ受入内の不変snapshot共有、旧形式配信、所有者待機、時計分更新を明確化した。既存の前回選択・設定の保存保証は維持する。

## 実装範囲

`shared.rs` がmutable Appの所有権を受け渡し、保存中は最後の公開済み不変snapshotをquery/HTTP/送信側へ提供する。保存済み選択と未確定選択は区別する。フレームACK、準備済みweather/radarのSUB・時点移動、配信完了、旧形式/E-ink動画、時計の分更新はSQLiteへ入らない一時経路を使う。plugin固有の一時viewは共通traitで受け渡し、共通層へ具体的なplugin ID分岐を追加しない。

一時経路は永続投影の変更を拒否する。frame/effect IDは共通の短い採番区間で予約し、IO中にそのlockを保持しない。失敗で予約済みIDを再使用しない。保存形式に大きな画素文字列を含むplugin snapshotもruntime上はArcで共有し、既存schemaへの展開は保存側で行う。radarの最新/pinセットも不変Arcを使う。

保存完了時には最新の一時状態を合流する。構造的な選択・設定・接続の失効と、単なる取得セット更新の再描画を区別する。現在のbrowse/pin、ACK、完了済みeffectを戻さず、保存側の新latestや別deviceの新effectも消さない。無効化した旧viewのcauseを、偶然同じdisplay epochになった新frameへ継承しない。

後半の保存が失敗しても成功したcommit prefixを公開してから通知する。所有者panicではquery/後続変更を明示的にunavailableとし、成功prefixの通知を消さない。重複ACKや存在しないeffect完了だけではdispatchを起こさない。

## ソフトウェアの受入証拠

`d02-final-complete-all-feature.log`：全featureのRust unit **272件成功、失敗0、83.13秒**。既存の因果追跡、D-01、Hello順序、旧/native wire、HTTP、static/video/E-inkの回帰を含む。

通常構成は `d02-final-default-feature.log` で **251件成功、失敗0、74.19秒**。`d02-final-offline-integration.log` のbin試験1件と保存済みJMA integration 4件も成功（integration 2.17秒）。`d02-final-default-check.log` の通常lib/bins型チェックも成功（12.28秒）。公開HTTPSを使うlive試験は実行していない。

| 対象 | 検証内容 |
| --- | --- |
| 実SQLite停止とHTTP/動画 | 実transactionを停止し、未保存選択が見えないこと、HTTP queryと実writerへの3画像送出を確認。LCD push、旧LCD、E-inkの3経路 × 保存成功/失敗。 |
| 実SQLite停止とbrowse/ACK/static | LCD/E-ink × weather/radar × 保存成功/失敗の8条件。停止中にSUB、時点移動、完全な画像のCRC組立、受信/描画ACKが進み、DB本文が変化しないことを確認。合流後に旧pinを維持し、cancelで正しいlatestへ戻る。native画素長も照合。 |
| 更新中の通常ACK | LCD/E-inkの通常画面で旧frameのACKが進んでも、保存完了後に新latestのnative画像を送る。 |
| 競合と後着 | 設定変更/削除/再接続の3条件を停止中のSUBと競合させ、旧view・旧ACK・旧causeを新状態へ戻さない。 |
| 保存途中成功 | 実buttonの選択保存成功後に、2回目の保存を実SQLite triggerで失敗させる。DB・公開状態・outbox・通知・再起動を照合。後続panicはunavailableと保存成功prefixの通知/復帰を確認。 |
| 保存保証と共有 | 一時Appで選択/設定を変更しても保存を迂回できない。1MiBのplugin payloadの参照共有と既存保存schemaを確認。 |
| 時計 | LCD/E-inkのJST 2026/12/31 23:59→2027/01/01 00:00を別deviceの実SQLite停止と競合させる。画素・E-ink分metadata・時刻合流・同分重複なし・他pluginの選択不変・DB書込みなしを確認。 |
| 因果追跡 | 一時domain commitも同じcauseへ結び、State/Committedのargument=2と、実writerのFrameStart metadataを照合。argument=1は永続経路、0は従来ログの未指定。 |

## 限界と反映

永続選択・設定などの変更は一つの保存所有者へ順序付ける。実SQLite writerの停止と、全保存状態のserializeはその操作の待ちに残る。schemaは変更せず、永続変更の差分保存までは実装していない。同期互換App APIと登録・再接続・disconnect等のlifecycle処理は既存の所有者経路を保つ。未知pluginの入力は明示的に一時入力へ対応するまで既存経路を使う。productionのmultithread Tokioではblocking区間を分離するが、current-thread runtimeで同期APIを直接呼ぶと呼出threadを占有し得る。

従って、保存成功前に新選択を公開する変更や、選択遅延の完全な解消を意味しない。ログには意図的な保存停止時間が含まれ、実ディスクの性能測定ではない。観測JSONLの保存とは別の経路であり、実機のRF・USB・物理描画や間欠遅延の解消は未検証。

このD-02変更はサーバRustだけで、追加のファーム更新は不要。既存DBのschema 2とwire/APIを維持する。本番の設定・DB・サービス・共有SDK・実機は変更していない。採用後はレビューしたソースでサーバをビルドし、設定と整合したDBバックアップを確保してから、承認された配置・サービス再起動を行う。今回その操作は実施しない。EV1観測のファーム反映やD-04は別項目である。

## 独立レビューによる受入の再開

7ff1480の既存テスト通過後、部分ACK時の再描画義務消失、実readerで重複Heartbeatが後続ACKを阻害、実CaricatureContent画像の深いclone、StreamStatusの公開欠落を独立レビューで再現した。D-02は追加修正後の独立再レビュー待ちである。修正前に[追加契約](application-live-state-review-corrections.md)を保存した。過去の通過数はこれら4件の解決を示さない。

## 4件の追加修正後の検証

先行契約`2b825e3`に従って4件を修正した。共通層の表示scopeに束縛した再描画義務を保持し、実readerの検証済み重複確認を保存なしで処理する。実CaricatureContentの全Artwork保持箇所をArcで共有し、StreamStatusの実変更を公開する。保存schema、wire/API、通常選択の保存後公開は維持した。

| 最終チェック | 結果 | 証拠 |
| --- | --- | --- |
| 追加再現・境界試験 | 6成功、0失敗、0.84秒 | `d02-review-final-targeted.log` |
| 全feature unit | 278成功、0失敗、78.61秒 | `d02-review-complete-all-features.log` |
| 通常unit | 257成功、0失敗、76.92秒 | `d02-review-complete-default.log` |
| bin / 保存済みJMA integration | 1成功 / 4成功、0失敗（JMA 2.18秒） | `d02-review-offline-integration.log` |

追加試験では実SQLite transaction停止中に実writerの送信を完了させ、LCD/E-ink×送信完了のみ/ReceiptAckありの4条件で旧画像の描画待ちを保つ。保存合流後、後着RenderedAckで最新画像を送る。actual serve_split readerへ重複Heartbeat+SettingsStored+両ACKを一つのpayloadで渡し、SQL解放前のACK公開を確認した。未来revision・旧sessionの拒否と新確認の永続経路も確認した。

約1MiBのPNGを持つ実風刺画contentをAppへ導入して公開/一時snapshotで同じArtworkが共有されることと既存保存形式での復元を確認した。StreamStatusは初回のprogress変更だけ通知し、重複/異なるgenerationで追加通知しない。選択・plugin有効化設定・再接続・待機・削除の5条件では旧再描画義務を消し、後着ACKが新しい画像effectを追加しない。

初期の全体試行2回は各277成功/1失敗だった。失敗は新しいguard試験のfixtureで、無関係な設定変更での有効な描画ACKを拒否すると期待した点と、送信完了前のeffectを残していた点に限られた。既存挙動に合わせてfixtureを修正し、上記の最終結果を得た。4件の修正実装を変更して通したものではない。

追加修正もサーバRustのみでファーム更新は不要。実機、サービス、本番DB・設定、SSH、共有SDKへの書込みは行っていない。本番ソースは`ffb62d6`のまま差分なし。SDKの既存submodule差分を保持し、cyw43_ll.cのSHA-256は基準の`a4e6baa503b9332a0a445c8b22deb4bee62793c441dc74a200b0fe8065c397c7`と一致した。実機の間欠遅延解消は引き続き未検証であり、独立再レビューと本番反映は別途行う。D-03/D-04の未完了範囲も維持する。


## 表示を保つ改訂の追加P2

551445aの前4件は独立再レビューで解消確認された。追加で、未選択pluginの無効化がsettings revisionを進めるだけで、表示中のlatest配信義務も消していた。上記の「有効化設定変更で義務を消す」試験基準はこの点を捉えていなかった。[追加契約](application-live-state-review-corrections.md)を2c8eaf7で先に記録し、表示の同一性で引継ぎと失効を区別する修正を行う。D03Aの保存点53cb612は保持し、D04の進行中差分は0dbcd816へ安全に退避した。

表示同一性の修正では、取得更新後に未選択pluginを無効化してから旧RenderedAckを送る場合と、その設定の実SQLite保存を停止した間にACK・実writer配信を進める場合を、LCD/E-inkの4条件で確認した。後者も新しい画像のpending/effect完了を古い保存snapshotへ戻さず、後着重複ACKで二度発行しない。選択・再接続・待機・削除の旧義務失効と、表示を保つ改訂の義務引継ぎを区別した。共通実装に具体plugin名の分岐は追加していない。

| 追加P2修正後 | 結果 | 証拠 |
| --- | --- | --- |
| 再現・境界 | 7成功、0失敗、1.03秒 | `d02-display-revision-targeted.log` |
| 全feature unit（D03Aも含む） | 284成功、0失敗、81.24秒 | `d02-display-revision-all-features.log` |
| 通常unit | 263成功、0失敗、87.65秒 | `d02-display-revision-default.log` |
| bin / 保存済みJMA integration | 1成功 / 4成功、0失敗（JMA 2.38秒） | `d02-display-revision-offline.log` |

通常unitの最初の実行はセッションが失効し、ログにコンパイル開始しか残らなかった。cargo/rustc/test/linkerが残っていないことを確認し、部分ログを`d02-display-revision-default-interrupted.log`へ保持して再実行した。上表はその完了結果であり、中断した試行を成功に数えていない。

`d02-display-revision-check.log`の通常lib/bins型チェックも成功（5.55秒）。この追加修正もサーバ側だけで、ファーム更新や本番操作は行っていない。独立再レビュー用の固定点として保存し、退避済みD04の作業はその後に再開する。
