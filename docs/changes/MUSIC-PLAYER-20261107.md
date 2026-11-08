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

### 置換仕様: 従来 AVRCP 操作と長押しキャンセル（2026-11-08、未実装）

ユーザーの追加要件「基本AVRCP今のまま、PLAY/PAUSE長押しをキャンセル、音量ボタンそのまま」により、直前の独立 PLAY/PAUSE/NEXT/PREVIOUS 案の操作意味を置き換える。直前案の実装は未commit・未反映の候補であり、本節の仕様コミット後に変更する。物理 Sub の曲循環、旧BT端末だけ旧Subトグルを保護する条件、専用 kind `27` / `media_input` / `media-input-v1` / optional Welcome `media_input_version: 1` の交渉は維持する。

- wire `operation` は `toggle_playback`, `channel_next`, `channel_previous`, `cancel`, `volume_up`, `volume_down` の6種類とする。未反映の旧4種類を互換用に残さず、未知operationとして拒否する。
- AVRCP PLAY/PAUSE の短押しは両方 `toggle_playback`。local-music の再生/一時停止トグルを行い、終了した曲は先頭から再生する。他channelにはtoggleによる変更を加えない。PLAYとPAUSEを別々の冪等操作としては扱わない。
- AVRCP NEXT/PREVは `channel_next` / `channel_previous`。serviceの既存 Down/Up 入力へ委譲し、Normalではチャンネル切替、Subでは既存のプラグイン内操作を維持する。曲送り/曲戻しにはしない。
- PLAY/PAUSE 長押しはどちらも `cancel`。serviceの既存 Power入力へ委譲し、Sub中はキャンセル、Normal時は従来Powerの意味を維持する。PLAYとPAUSEを別のcancel/powerへ割り当てない。
- gestureの短長判定はfirmware側でrelease時に一度だけ確定する。長押しに短押しが先行しない。PLAY/PAUSE押下の重複、古い接続世代、release欠落を任意の追加操作へ変換しない。閾値・押下保持期限はBT仕様の確定値へ揃える。
- AVRCP VOLUME_UP/DOWN のイベントを受信した場合だけ `volume_up` / `volume_down` を送る。serviceの既存Bluetooth aggregateコマンドへ現在revisionで委譲し、対象端末だけ0..100範囲で増減する。`muted` は一切変更せず、音量変更をunmute・BT再接続・再生再開にしない。speaker側だけで処理され通知が来ない音量操作の結果は推測しない。1操作の増減幅は親の確定値を追記する。
- 共通Appはcurrent session/incarnationとmedia capabilityを検証し、チャンネル・cancel・音量操作を既存aggregate入口へ委譲する。plugin ID分岐は追加しない。toggleだけを選択中のNormal pluginへtyped操作として渡す。
- 新host試験は短toggle・長cancelの排他性、旧4op拒否、channel/sub操作維持、端末独立の音量clamp・永続化・mute不変、stale session拒否を追加する。実機での操作/音量/長押しの効果は未確認とし、再生中productionに触れない。

確定値: PLAY/PAUSEはreleaseまでの保持時間が1800ms未満ならtoggle、1800ms以上ならcancelのみ。押下状態は10秒で失効し、期限後のreleaseは無操作。音量は受信した押下1回につき5ポイント（0..100へclamp）とし、押下重複・holdの自動反復を行わない。clamp境界で値が変わらない操作は永続revisionを増やさない。これらの値で親承認済み。

### 最新6操作の実装・ホスト検証（2026-11-08）

仕様コミット `13a03f5` に従い実装した。独立PLAY/PAUSE/NEXT/PREVの旧4操作候補はcompile確認のみで未反映のまま置換し、現在のwire enumは最新6種類だけを受理する。Appは能力・sessionを検証して既存Down/Up/Power/Bluetooth aggregateへ委譲し、plugin IDで分岐しない。local-musicは物理Subの循環と再生トグルを純粋に計画し、aggregate採用後に選曲・位置・世代を保存する。欠落track参照は検証して未選曲扱いにし、indexによるpanicを防いだ。

- **最終全Cargo:** `PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px cargo test --manifest-path service/Cargo.toml --all-features -- --test-threads=1` は **400 passed / 0 failed / 2 ignored**。内訳はlibrary381、server2、音楽integration13、official JMA4。libraryの1件とlive HTTPSの1件は既存のignore。既存shutdown・SQLite待機統合もPASSで、timeoutやassertionを緩めていない。
- **音楽・media経路:** `service/tests/local_music_input.rs` の13件は、旧BTトグル、新BT/非BTのSub循環、0/1曲・未選曲・終端・端末分離・古いsession/offer、短toggle、チャンネル切替、天気Sub内の移動/キャンセル、音量±5と上下clamp、境界のrevision据置、Web側の古いmute revision拒否、最新muteの保持、永続化を検証。`service/src/media_input_contract_tests.rs` の3件はwire decode→App経路、6operation roundtrip、未知/旧play拒否、Welcome交渉・旧Welcome、旧BT互換とsession保護を検証した。別のplugin unitで欠落trackと不整合restoreも確認した。
- **環境の前提:** pic2gray4pxはPATHで探索されず、上記環境変数の指定が必要。指定を欠いた初回全libraryは372 pass / 9 fail / 1 ignored。radar/fetchとそのSQLite待機統合が失敗したログを保持し、同じcompiled binaryの環境指定付きradar/SQLite単独PASSと、環境指定付き全suite PASSで分離した。コード故障やshutdownのflakyとして扱っていない。
- **その他:** `python3 tools/generate_observation.py --check` と `git diff --check` はPASS。firmware側の6operation文字列、kind27、Welcome v1条件とserviceを直接照合した。firmware host/buildの証拠はBT-AUDIO側を参照する。
- **未解決の既存検査:** 変更していないUIのNode全試験は22 pass / 6 failで、6件は `ui_video_fps.test.cjs` のfixtureに `renderEinkVideoControls` がない問題。fresh-out project_gateは49 structural中1件、既存 `test_actual_firmware_build_sdk_flags_and_offline_flash` の承認削除後return0問題で拒否された。いずれも今回の音楽/media変更に合わせて検査を緩めていない。

最終Cargoログは `/tmp/picomado-media-v2-configured-all-features.log`。無音host検証であり、再生中production・SSH・USB・サービス反映・実音声確認・pushは行っていない。独立検証・運用適用・実機効果確認と変更クローズは親が別途判断する。

## 音声失敗を再生状態へ返す契約（2026-11-08、実装前）

AWAY session 2516 の stream 1/2 は端末 `failed=true` で配送停止した一方、
local-music の playing と時計由来の位置が進み続けた。既存配送は失敗を socket
owner 内に保持するだけで、plugin の再生意図へ返す契約が欠けていた。
既存 `accept_work_plan` の clone → 永続化 → 公開 → 対象描画更新を再利用する。

- 共通 ContentPlugin に音声失敗結果の既定 no-op hook を追加する。具体 plugin ID
  分岐は追加しない。App が current session/incarnation/display、選択 plugin と
  AudioPlaybackSpec 全体（asset/generation/start/開始時刻を含む）を再照合して
  から hook を呼ぶ。spec の開始時刻は固定値であり現在時計ではない。音量/mute は
  spec/display を変更しないため有効な失敗を捨てない。
- socket owner は同じ stream identity の credit を従来 AudioSender 規則で検証
  してから terminal failure を保持する。failed status の有効な consumed も採用し、
  不正/逆行値は最後の有効 consumed を保持する。sent/accepted/壁時計を停止位置に
  しない。終端paddingは plugin が曲長へ clamp する。端末消費は出音の証明ではない。
- 端末 failure、credit 不正、既存3秒credit期限、source read error/既存100ms期限を
  固定理由 enum で返す。1失敗につき一度採用し、古い/重複/別端末/別曲/再接続結果は
  現在意図を変更しない。IO stop の成否に依存せず失敗を先に公開する。
- local-music は一致する端末の playing を false にし、消費位置を固定し、世代を
  更新して端末別失敗理由を公開する。その端末だけ描画更新し、Webに「再生失敗」・
  理由・停止位置を表示する。LCD/E-inkは MUSIC FAILED を表示する。既存変換エラー
  とは分離する。明示play/stop/曲変更はその端末の失敗を解除する。
- source原本/PCM変更、再接続、autoplay、再試行、期限延長を追加しない。利用者の
  次の明示playだけが新しい世代で停止位置から再生を要求できる。
- 無音host試験でApp公開経路の位置固定、端末独立、世代/session/display fence、
  mute/volume不変、重複拒否、明示再開、terminal credit検証と実service_audio経路を
  検証する。実機出音/本番適用/独立受入は別段階のまま親が所有する。


## プレーヤーを音楽一覧より先に表示する契約（2026-11-08、実装前）

ユーザーの要望「音楽リストよりも上に端末ごとの再生を持ってきて」に対応する。
既存 `renderLocalMusic` は音源管理カードと全音源行の後に端末別カードを追加するため、
音源が増えるほど選曲・再生操作までスクロールが必要だった。

- ローカル音楽ページは「端末ごとの再生」を先頭、音源アップロード・一覧・変換・削除をその下に表示する。CSSだけの順序変更ではなくDOM生成順を揃える。
- 端末ごとの選曲、再生・一時停止・停止、失敗理由、位置、ボタン無効条件は変更しない。ページ表示・再表示だけで再生などのコマンドを発行しない。
- 曲の有無、複数端末、失敗／再生中／未選択の表示、再描画、対象端末と選択曲のコマンドを無音DOM試験で確認する。全体JavaScript構文と既存UI試験も実行し、既存失敗は分離する。
- 本変更の範囲はWeb UI配置と回帰試験。音源・永続状態・通信／音声処理・ファームは変更しない。サーバ埋込みUIの反映は他担当と調整し、再生や出音を起こさず、配信HTMLと正常応答で確認する。独立確認と変更全体のクローズは親が所有する。


### プレーヤー先頭配置の実装・無音UI検証（2026-11-08）

仕様 `b710e6e` 後、`renderLocalMusic` の端末別カード生成2行を関数先頭へ移動した。
フォーム・音源一覧・コマンド内容と表示文字列はそのまま保持した。
`service/tests/local_music_ui_test.js` は実際のrendererとsection関数を実行し、
多曲時の順序、端末別失敗／再生中／位置、選択曲と操作対象、曲なし、未選択、
繰り返し再描画の6試験がPASS。旧ソース `b710e6e` では順序3試験だけがFAILする。
全UI試験は28 PASS / 6 FAILで、既存 `ui_video_fps.test.cjs` の
`renderEinkVideoControls is not defined` 6件は対象外のfixture不足として維持した。
JavaScript全体の `node --check`、`git diff --check`、project gateのstaticはPASS。
ログは `/tmp/picomado-ui-order-{all,negative}.log`。全Cargo・本番反映は別途追記する。
