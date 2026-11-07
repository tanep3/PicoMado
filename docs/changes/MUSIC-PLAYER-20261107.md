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
- 変換済み音源は内容hashで再利用。削除は確認付きで原本・変換済み・参照を整理。同一内容別名の参照を保護。symlinkは拒否する。
- restart時は自動再生しない。端末incarnation変更・削除では旧再生状態を再利用しない。

## 検証
純粋遷移の端末分離・世代/古いjob・pause/stop・delete、upload空/過大/既存/path/中断、executor不正音源/欠落/取消し/PCM境界、plugin登録、Web UIの再試行/選択/HTML安全、既存cargo tests、オプションproject_gate。実機出音とAVRCP操作・Wi-Fi共存は別担当と協調して別証拠とする。

## 段階
仕様を先にcommit。実装・独立検証・本番適用・効果確認を分離する。push、サービス再起動、実機書込みは未実施。親が独立検証と最終クローズを所有する。
