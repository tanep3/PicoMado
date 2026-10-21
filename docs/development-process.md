# Development process — OBS-20261021-01

The entry is [AGENTS](../AGENTS.md); generic principles are [standard v1](../development/standard/v1/standard.md). The existing [contracts and observation design](contracts-observation-design.md#obs-20261021-01-observation-domain) owns the project domain. The sole change state is [OBS-20261021-01](changes/OBS-20261021-01.json). This is development-process prevention, separate from the latency cause investigation.

## Existing mechanisms and gap

At base `86ca475`, tracked root has no AGENTS, unified root check or CI workflow. Existing mechanisms are contract/verification pairs, `docs/checkpoints/*`, fixed commits, independent reviews, firmware `run_host_tests.sh`/`run_usb_tx_tests.sh`, Cargo tests and per-trial verification scripts. `firmware/tools/build.sh` creates a UF2 hash manifest. `flash.sh --check-only` currently calls picotool against USB, so it is not an offline preflight. Server deployment is a manual runbook / `cargo install`, not a protected entry. Searches of the project/ancestor AGENTS locations found none; this entry is newly introduced, not a modification of an imaginary existing policy.

## Change and state gate

A change ID records cause, introduction and escape causes, responsibility, deliverables, dependencies, stage evidence and explicit unapplied stages. Allowed stages are not_started → in_progress → verified, independently for design/implementation/independent_verification/operational_application/effectiveness_confirmation. `registered` never means implemented. Parent ownership is mandatory and the worker cannot self-assert independent acceptance or final closure.

1. Register/adjust domain contract and state tables; commit the specification before implementation.
2. Generate only declared values/state representations. Reject drift and forbidden dependency/write/construction paths before tests.
3. Implement within aggregate APIs; adapters retain IO ownership and bounded behavior.
4. Run root CLI and the same CI check locally. Compile-fail C/Rust and Python AST negatives must reject forbidden substitutions/state writes/unverified completion; historical faulty logic and partial records must fail the acceptance path. These are safeguards, not the primary model.
5. Bind gate receipts to current protected input hashes. Stage artifacts with SHA/provenance and independent parent approval. Offline flash preflight runs before any picotool command. No automatic firmware writing/service install is added.
6. Apply the entry/route only through a bounded integration preserving dirty user files; record active path and proof of rejection there. External branch protection / OS enforcement remain unapplied until separately authorized.

## Bypasses and completion

Direct `cmake`, `cargo`, `picotool load`, shell file copy, edits to the gate or arbitrary casts/reflection can bypass software conventions. Gate validation is fail-closed on the supported path, not an OS security boundary. C struct types and Rust private fields prohibit ordinary substitution; Python needs static adapter restrictions plus runtime validation of untrusted data. The specification can still be wrong. Physical timing, RF and actual device behavior remain runtime evidence.

Future work starts at the recorded active work root and uses this entry for both parent/worker. Existing accepted artifacts are retained; new enforcement does not retrospectively label them faulty. Independent acceptance and parent closure remain pending until their real evidence is added.

## PDCA: YouTubeのACK非依存化という当初仮説 <a id="pdca-youtube-ack"></a>

これは既存の問題記録に調査の比較軸を戻す是正であり、新しい管理システムや開始条件ではない。ユーザーは「最初に提示した仮説を未検証のまま無視し、約2日間調査した」と指摘した。期間全体の会話経緯を推測せず、以下を保存コード・記録と対応付ける。

以下の５項目は実装前の判断記録。適用後の結果は末尾に追記する。

- **元仮説と根拠:** 既にYouTubeで導入済みのACK非依存配信を通常LCDにも適用すれば、ACK待ちが後続画像の送出許可を止める経路を除けるのではないか。既存実装 `8e41fd6` の動画pushと、通常画像の `RequestFrame` / `pending_frame_id` / `RenderedAck` 待ちが比較軸である。仮説を無条件に正解とはせず、その効果を検証する。
- **前提の脱落と見逃し:** [既存調査結果](changes/OBS-20261021-02-local-result.md)の「Repair direction, conditional on location」では欠落場所の特定を優先し、ACK非依存化を未検証の大きな緩和策として後順位に置いた。パケット欠落の原因調査と、既存動画との差分による表示進行の改善を別々に検証せず、後者を未完了の仮説として調査・委任へ引き継がなかった。観測の進展だけでは、この仮説を検証したことにならない。
- **今回確認された差分:** 動画pushはRenderedAckによる画像ごとの送信許可を待たず、送信中1枚と未送信最新1枚に制限する。通常LCDは旧契約でpending frameのRenderedAckを待つ。採取済み2frameは入力から描画完了が161.183 / 143.538 msでも報告到着が遅れていたため、描画完了とサーバの送信許可を分ける根拠がある。一方、後続ボタン通知自体が欠落TCP範囲の後ろにあれば順序待ちは残る。ACK非依存化はパケット欠落そのものの修復とは区別する。
- **是正:** [比較から定めたサーバ契約](changes/LCD-ACKLESS-20261021.md)は仕様コミット `19c6681` で固定済み。対応能力を持つ通常LCDにも既存の有界latest配信を適用し、ACKを送信許可から観測情報へ分ける実装は別担当が進行中である。本記録を調査と委任の共通参照とし、方向変更時にも元仮説の支持・反証・未検証、その根拠と残る比較を残す。誤判断は原因とこの引継ぎ方法の是正に結び付け、次の実作業で確認する。
- **確認済み／未完了:** 動画と通常LCDの契約差分、仕様固定、共通参照への記録は確認済み。**是正の実機効果は未確認。** 次のサーバ検証では、同じファーム・設定・操作条件でACK非依存化の前後を比較し、ACK未達・遅延時の後続画像送出、入力から実描画、描画から報告到着を分けて確認する。結果を当初仮説の支持・反証・未検証へ戻す。ローカル試験や本記録の更新だけで効果確認済みとはしない。

### 本番適用後の検証（2026-10-21）

通常 LCD と E-ink の有界 latest 配信を本番へ適用し、RenderedAck を次画像の送信許可から除いた。最初の適用では E-ink 時計の同分連続発行と LCD 画像公開遅延を検出して旧バイナリへ一時復帰した。混入原因は、画像 slot と App snapshot の公開順が分離していたこと、および durable snapshot との合流時に時刻キーを device state 無変更として捨てたこと。slot を snapshot 公開後に渡し、同一表示 token の時刻キーを合流する構造へ修正した（`e18177b`, `b2785e4`）。競合順序の再現試験と通常・境界試験を通し、修正版を反映した。E-ink 時計は本番ログの 14:56–15:00 UTC の各分で１枚ずつだった。

修正前の有限 LCD 切替で画像送信開始まで 13.881 s 待った。修正版の USB SIM ８操作では、Pi が入力を受けてから画像送出まで 31–54 ms に収まった。一方、２／８操作では SIM RELEASE 後も Pi 入力到着まで 1.20／1.32 s かかり、ACK 非依存化後も遅延が残る。これは今回の８操作で TCP 欠落を直接証明しないが、ACK 待ちだけで全遅延を説明する仮説は支持しない。先行する別試行の TCP hole と同一イベントとして扱わず、残る Pico 送信呼出後→Pi ingress の区間を調べる。次の是正は、正常な 99 B 初回送信と欠落した 99 B 初回送信の実条件を比較し、SDK 修正候補の適用を原因確定と混同しないこと。
