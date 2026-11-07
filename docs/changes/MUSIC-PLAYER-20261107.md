# MUSIC-PLAYER-20261107 — ローカル音楽プレーヤー

## 目的と比較元
Web UIから音源をアップロードし、選択したPicoMado端末のBluetoothスピーカーで再生する。マイク/HFPの完成は待たない。既存local-video/local-imageの有限アップロード、PluginJobとContentPlugin境界を再利用する。ファイルを母艦の手作業で配置する方式を主導線にしない。

## 契約
- 音源と一覧は母艦で共有、選曲・再生状態はdevice IDとincarnationごとに独立。音源未選択・準備中・停止・再生要求・失敗を区別し、要求だけで実音声確認済みとしない。
- Web UIのローカル音楽ページにアップロード、一覧更新、変換、端末選択、再生/一時停止/停止、削除。Bluetooth探索/ON/OFF/ペアリングはホーム端末カードのBT-AUDIO-20261107担当とする。
- 当初はMP3/WAV/FLAC/OGG/M4A/AAC。拡張子だけを信頼せずffmpegで音声ストリームを検証。入力は256MiB上限、1曲2時間上限、PCM s16le stereo 48kHz。画像/動画のframeとは別のaudio sample frame（全ch同時点）を用いる。
- 音源プラグインは純粋な再生意図AudioPlaybackSpecを返す。取得/デコード/読取りはexecutor、Bluetooth wire配送はBT-AUDIO共通層。ネットラジオは別プラグインとして追加可能な契約を保つ。
- PCMはbounded chunkで読み、ストリーム世代とstart_frameを持つ。古いread結果を配送側が破棄。画像latest置換と混同しない。実音声配送未接続の間は端末出音を完成扱いしない。
- アップロードはストリーミング上限・名前/path検証・非上書き・一時ファイルcleanup。ffmpegはネットワーク入力を禁止しprivate work内コピーを使用、期限/cancel/dropで子プロセスを終了。出力サイズも上限検査。
- 変換済み音源はファイル名+内容hashで再利用。同一内容別名は別キャッシュとし削除の干渉を避ける。削除は確認付きで当該名の原本・変換済み・参照を整理。symlinkは拒否する。
- restart時は自動再生しない。端末incarnation変更・削除では旧再生状態を再利用しない。

## 検証
純粋遷移の端末分離・世代/古いjob・pause/stop・delete、upload空/過大/既存/path/中断、executor不正音源/欠落/取消し/PCM境界、plugin登録、Web UIの再試行/選択/HTML安全、既存cargo tests、オプションproject_gate。実機出音とAVRCP操作・Wi-Fi共存は別担当と協調して別証拠とする。

## 段階
仕様を先にcommit。実装・独立検証・本番適用・効果確認を分離する。push、サービス再起動、実機書込みは未実施。親が独立検証と最終クローズを所有する。

## Implementation record

- 仕様コミット `0d7b6d7` の後に実装。`local_music.rs` は端末別再生意図と有限音源の状態のみを保持、executorがffmpeg・ファイル操作を担当する。WebUIアップロードはbounded streamingと非上書きpublishを使用する。
- `AudioPlaybackSpec` は将来のネットラジオ用に frame_count を Option とし、現段階のローカル曲は必ず有限。Appはsession/display tokenと選択プラグイン付きのofferを返し、配送側が非同期read後に再検証する。
- 入力をprivate作業ディレクトリにコピーし、ffmpegのprotocol/formatを制限。子processのdrop/cancel/期限で終了を保証し、PCMは最大4800 sample framesずつ読み取る。変換上限・サイズ検証・symlink拒否を実装。
- ユーザー操作の再生要求と実際の出音を混同しないUI。再起動時autoplayなし。削除要求時に参照端末の再生を止める。AVRCPなどのボタン時刻用にmonotonic observation hookを追加。
- `cargo test --manifest-path service/Cargo.toml --lib local_music --no-default-features`: 初回6 tests passed（2026-11-07）。端末独立/再起動、名前検証、PCM境界/破損、symlink/取消cleanup、uploadサイズ/非上書き、実ffmpeg WAVデコード/再利用を確認。取消後完了の古いjobを拒否する追加試験も用意。
- WebUI JavaScript: `node --check` 合格。wire identity modelは更新済み。全feature試験367 passed / 2 ignored、最終差分後lib361 passed / 1 ignored。既存gateのflash.sh契約不整合1件は変更前HEADで再現し未解消。実機確認・独立検証は未完了。
- 実装commitはBT-AUDIO-20261107との共通変更を含むため担当間で統合する。push・本番反映・ファーム書込みは実施していない。

統合ホスト検証: [BT-AUDIO-20261107-host.md](../evidence/BT-AUDIO-20261107-host.md)。

実装コミット: `870098f`。独立検証・本番適用・効果確認は未実施。

## 実機曲名表示の修正（2026-11-07）

曲名が文字化けするとの指摘を調査し、天気用限定文字集合に英小文字やピリオドがなく、fallbackの`?`になっていたことを確認。音楽専用Noto Sans CJK JP資産を追加した。ASCII印字文字、JIS X0208、半角カナなど7038文字、14/16px、1bit約560KB。生成スクリプト・出典情報・既存フォントライセンス参照を同梱する。既存天気資産と描画は変更しない。LCD160x80とE-ink250x122をそれぞれnative描画し、切り詰めは文字境界と省略記号で行う。収録外文字はreplacement glyphで明示する。

music関連9テスト成功（専用font2試験を含む）。サーバへの反映はユーザー承認済み。無音の調査はBT-AUDIO-20261107側へ記録。再生時間表示だけを出音成功の証拠とはしない。

## 物理再生ボタンの状態採用修正（2026-11-07、実装前契約）

- **原因と比較:** Web の play/pause は `accept_work_plan` で変更済み content を採用する。一方、物理ボタンの `App::button_at` は content の clone に `plan` を実行し、返された event が aggregate に受理された場合だけ clone を採用する。local-music は再生状態を変更しても `event: None` を返しており、Sub の受付ログがあっても変更と描画が捨てられていた。plugin 単体の直接呼出試験ではこの境界の違いを検出できなかった。これは入力状態採用の欠陥であり、Bluetooth 接続・PCM/SBC 配送・実際の出音とは別の原因である。
- **採用契約:** 既存 weather の session 付き `PluginInput → Plugin.candidates` と同じ入口を使い、local-music の候補は既存 `Transition::PluginDataChanged` とする。plan は現在の SessionToken と `toggle-playback` を持つ `PluginInput` を返し、rules は選択中 local-music / Normal / 対応 name の場合だけ検証済み token と同じ device ID / incarnation を対象にする。既存 aggregate が session を検証・受理した後だけ content と画面を採用する。`Event::PluginDataChanged` の端末 allowlist 外という境界と `RefreshView` の Sub 専用契約は維持し、共通入口・遷移の適用範囲は広げない。
- **Sub の意味:** 選曲済みで停止中なら再生、再生途中なら現在位置で一時停止、停止位置から再開する。曲終端では再生中フラグが残っていても終了扱いにし、1回の Sub で先頭から再生要求する。各採用操作で音声 generation を更新し、古い非同期 read offer の再利用を防ぐ。選曲のない Sub は無変更。
- **既存操作の維持:** Normal の Up/Down/Next は従来どおりチャンネル切替。音楽の曲送りには割り当てない。変更は対象端末だけに作用し、他端末の音声 offer と表示世代を変えない。再接続・端末削除後の自動再生は禁止のまま。
- **無音の検証境界:** `App::button_at` と公開 API を通る integration test で再生/一時停止/再開/終端 replay、端末独立、チャンネル操作、stale session/incarnation/offer 拒否を確認する。temp DB と純粋な音源メタデータのみを使い、音声再生・実機・USB・SSH・サービス変更を行わない。合格しても出音と物理ボタンの実機効果は未確認として分離する。

### ボタン修正のホスト検証

`service/tests/local_music_input.rs` を追加し、2026-11-07 の最終修正ソースで `cargo test --manifest-path service/Cargo.toml --test local_music_input --no-default-features` は **7 passed / 0 failed**。公開 App 入力経路の再生/一時停止/位置を保持した再開、曲終端の1回 replay、2端末の独立、2曲がある場合の Up/Down チャンネル切替維持、再接続と再登録での古い session/incarnation 拒否、古い audio offer の不一致、非対応 name/mode の拒否、候補の同一端末/incarnation/plugin 限定を確認した。共通 application / Engine / terminal allowlist は変更していない。修正前の固定ソースを使った厳密な前後比較は未実施。全体の独立検証・本番反映・実機効果確認は親が別途確認する。

## 端末 Sub を曲入れ替えへ変更する仕様案（2026-11-08、未実装）

ユーザーの新要望は「ローカル音楽表示時、端末のサブメニューボタンは曲の入れ替えにしたい」。これが従来の物理 Sub = 再生/一時停止トグルを置き換える。以下は実装前の案であり、AVRCP との入力分離方針を確定し、仕様コミット後に実装する。

### 現在の入力経路と制約

- 4ボタン機の Sub は押下時に1回の SUB 操作。長押し専用動作はない。2ボタン機は左短押し=Up、左長押し=Sub、右短押し=Down、右長押し=Power（`firmware/src/core.c` の binding）。
- AVRCP PLAY (`0x44`) / PAUSE (`0x46`) は両方 `PM_OP_SUB`、FORWARD (`0x4b`) は Down、BACKWARD (`0x4c`) は Up。押下重複を抑制し、release は次の押下を許可するだけ（`firmware/src/bluetooth_input.h`）。PLAY と PAUSE も個別の再生/停止指示ではなく現在は同じトグルである。
- Normal の local-music は Sub だけをプラグインへ渡す。Up/Down は App 共通のチャンネル切替なので、現在 AVRCP FORWARD/BACKWARD も曲送り/曲戻しにはならない。
- GPIO/SIM/AVRCP は `perform_operation` に合流し、wire の `enter_sub` / `up` / `down` は incarnation と session_epoch のみを持つ。起源・AVRCP opcode は service の入力意味として届かない。任意の observation 情報は診断用であり、制御の意味を復元する代用品にしない。
- **実装 blocker:** local-music の Sub だけを順送りへ置き換えると AVRCP PLAY/PAUSE まで順送りになる。現在の wire では区別不能。端末 Sub と AVRCP の意味を分離する契約を別途確定するまで、その置換を適用しない。プロトコル拡張や AVRCP 割当変更をこの案だけで承認済みとしない。

### 曲入れ替えの状態遷移案

- 対象は Normal で local-music を表示している現在 session の端末のみ。端末の意味上の Sub を1回受けると、変換済み曲一覧の次の曲へ進み、最終曲から先頭へ循環する。順序は Web の `data.tracks` と同じ確定順（現状は track key の BTreeMap 順）を維持し、新しい並び替え設定は追加しない。
- 再生途中の入れ替えは次曲の先頭から再生要求を継続する。一時停止/停止中または曲終端後なら、次曲を先頭の停止状態で選ぶ。曲変更を暗黙の再生開始にしない。
- 未選曲で曲があれば先頭曲を停止状態で選ぶ。0曲なら無変更。1曲で既にその曲を選択済みなら位置・再生状態・generation を維持して無変更とし、意図しない先頭戻しを起こさない。
- 実際に曲が変わる場合だけ新しい audio generation と表示世代を採用し、旧曲の非同期 audio offer を拒否する。他端末の曲・位置・状態・世代は維持する。stale session/incarnation、非対応 mode と入力 name は拒否する。
- Normal の端末 Up/Down のチャンネル切替は維持する。Web の play/pause/stop も維持する。AVRCP の適切な再生・一時停止・曲送り/曲戻し割当は別の入力意味として確定する必要があり、端末 Up/Down の意味を流用して変更しない。
- 無音ホスト試験は App の実入力経路で、循環・再生継続・停止維持・終端・未選曲・0/1曲・端末独立・stale offer/session/incarnation 拒否・チャンネル維持・AVRCP との意味分離を固定する。実機、USB、SSH、サービス変更、音声再生、本番適用は別承認と別証拠のまま。

### 確定: 端末 Sub と AVRCP の独立契約（2026-11-08）

親承認により入力分離を実装する。上記の blocker は専用 media input と capability 交渉で解決する設計に確定した。本節は従来 Sub トグル契約と上記未確定部分に優先する。実装開始は仕様コミットと親の開始指示後とする。

- **新 wire:** kind `27`、type `media_input`、data は `incarnation: u64`, `session_epoch: u64`, `operation: "play" | "pause" | "next" | "previous"`。操作は独立した enum としてdecodeし、不明値を既存ボタンへ変換しない。session identity の生成モデルへ型を分類する。
- **交渉:** 新firmwareは Hello capabilities に `media-input-v1` を広告する。新serverはその広告がある端末だけ Welcome.data に `media_input_version: 1` を返す。Rust側は欠落時 `None` の optional unsigned integer とする。旧serverの欠落、未知version、未Welcome、非ACTIVE/古いsessionでは新firmwareはAVRCP入力を送らず、旧Subへのfallbackもしない。新Welcomeごとに交渉状態をresetする。
- **旧firmware互換:** `bluetooth-audio-v1` があり `media-input-v1` がない旧BT端末の generic Sub だけは、物理/AVRCPを識別できないため従来の再生/一時停止トグルを維持する。`media-input-v1` のある新端末とBT非対応端末の generic Sub は曲循環にする。端末IDを推測せず実際の能力情報だけを用いる。接続、他のchannelのSub、Normal Up/Downのチャンネル切替は従来どおり。
- **AVRCP意味:** local-music表示中のNormal/current sessionのみで PLAY=再生、PAUSE=一時停止、NEXT=次曲、PREVIOUS=前曲。PLAY中のPLAY、停止中のPAUSEは無変更。PLAYは同曲の一時停止位置から再開し、終端後は先頭から開始する。未選曲なら先頭曲から再生、0曲なら無変更。NEXT/PREVIOUSは一覧順に双方向循環し、物理Subと同じ再生継続/停止維持/先頭位置/0・1曲ルールを適用する。選択した他channelやSub/Menu/Standbyには作用しない。PLAY/PAUSEをトグルと解釈しない。
- **状態所有と境界:** firmwareはAVRCPを既存GPIO operationへ畳み込まず専用wireを送る。serviceはsession・capabilityを検証し、typed media operationを選択pluginへ渡す。local-musicの純粋遷移は引き続き `PluginInput → LocalMusicPlugin.candidates → PluginDataChanged` のaggregate採用後に確定する。既存pluginのlegacy Subは既定の通常Sub経路を維持し、local-musicだけが旧BTトグルを提供する。アプリ共通層にlocal-music等のplugin ID分岐を追加しない。
- **追加検証:** 専用wire4操作のroundtrip、未知operation拒否、旧Welcomeのdecode、cap有無でWelcome交渉、stale session/incarnation拒否、他channel無変更、旧BTのトグル維持、新BT/非BTのSub曲循環、AVRCP PLAY/PAUSEの冪等性を無音host試験で固定する。全体compileは他担当のmute試験終了後に行う。再生中のproduction・USB・SSH・サービス・実音声は操作しない。
