> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# 既存 YouTube・風刺画の機能抽出と互換チェック

- 調査日：2026-10-12 UTC
- 方法：`$HOME/dev/PicoW/PicoW_Youtube_Player` と `$HOME/dev/PicoW/PicoW_E-ink_Display` のソース・資料をローカルで読み取り。サービス、既存データ、端末は変更していない。
- 状態：既存コードの挙動を抽出したもの。PicoMado の Rust 実装・LCD 実表示・E-ink 実表示・複数端末・動画 fps の達成を示さない。既存のデータや稼働構成を移行する許可でもない。

## YouTube

| 既存コードで確認した機能・例外 | 根拠 | PicoMado の互換チェック |
| --- | --- | --- |
| 単一 YouTube 動画 URL を検証し、全編または開始・終了の範囲を扱う。FPS 候補は 6/12/18/24/30、Web の既定は **6 fps**・全編。 | [library.py](../../../PicoW_Youtube_Player/server/library.py) `16–84`、[web.py](../../../PicoW_Youtube_Player/server/web.py) `32–49` | 対応 URL、時刻入力、全編／区間、FPS 選択と既定値を試験。新環境で 30 fps を保証する意味ではない。 |
| 160×80 の画面へ動画全体を収め、縦横比を保って黒余白を付ける。変換条件をハッシュに含め、完成した JPEG 連番を検証後に公開する。送信時の RGB565 色補正がある。 | [library.py](../../../PicoW_Youtube_Player/server/library.py) `18–25,75–84`、[media.py](../../../PicoW_Youtube_Player/server/media.py) `241–313` | LCD の向き・色・余白、条件違いのキャッシュ分離、途中成果物を配信しないことを確認。実機色補正は元ファームと比較する。 |
| `yt-dlp` と `ffmpeg`／`ffprobe` を使う単一の取消可能な取得・変換ジョブ。区間の前後を少し広く取得し、HTTP 403 時は HLS へ１回切り替える。失敗・変換中は現在の再生を維持し、完成後に切り替える。 | [media.py](../../../PicoW_Youtube_Player/server/media.py) `43–61,178–356` | 新ジョブ・取消し・失敗・再試行・完成公開と現再生維持を別々に試験。外部ツールの版と成功率は新環境で測る。 |
| 共有再生は単調時計を基準にループし、動画切替では先頭へ、再接続では進行中の位置へ入る。読み込みと送信の待ちを分ける。 | [playback.py](../../../PicoW_Youtube_Player/server/playback.py) `30–57,131–192` | PicoMado の複数視聴端末で共有位置と独立選択を保ち、遅い端末が共有時計を止めないことを確認。 |
| SQLite に動画・元素材・変換・選択を保存し、タイトル・作者・サムネイル・履歴・容量を Web に示す。削除時は対象 ID の再確認、再生停止、処理取消完了を要求する。再起動時の選択復元と旧 ZIP/RAW キャッシュ読取りもある。 | [library.py](../../../PicoW_Youtube_Player/server/library.py) `87–114`、[web.py](../../../PicoW_Youtube_Player/server/web.py) `14–82`、[frame_cache.py](../../../PicoW_Youtube_Player/server/frame_cache.py) `26–85` | 履歴・容量・削除ガード・再起動復元を試験。旧 ZIP/RAW は既存機内の読取り互換であり、PicoMado に既存キャッシュを移す判断はここではしない。 |

## 風刺画

調査対象はローカルの `PicoW_E-ink_Display/ai_news`。既存 E-ink 製品の実装・運用をそのまま PicoMado へ移すことは決めていない。

| 既存コードで確認した機能・例外 | 根拠 | PicoMado の互換チェック |
| --- | --- | --- |
| 毎時枠と手動生成枠を分け、ジョブ開始時に題材・画風・モデル等の設定を固定する。公開済みニュース出典 URL の直近24時間重複を避け、試行を制限する。任意文章からの手動作画もある。 | [generator.py](../../../PicoW_E-ink_Display/ai_news/generator.py) `280–451`、[manual.py](../../../PicoW_E-ink_Display/ai_news/manual.py) `68` | 毎時／手動ジョブ、設定 revision、出典の重複除外、カスタム文章、失敗時の再試行を試験。生成バックエンドと所要時間は別途設計・検証する。 |
| 250×122 1bit PNG を検証・正規化し、RAW 4000 バイトを生成する。作品履歴は不変、latest の公開は原子的で、生成失敗時には旧 latest を保持する。 | [frame.py](../../../PicoW_E-ink_Display/ai_news/frame.py) `11–15,31–70,108–124`、[archive.py](../../../PicoW_E-ink_Display/ai_news/archive.py) `109–194,353–390` | 履歴・完成判定・公開の整合を試験。初版 LCD は 160×80 用の描画を別に作る。PicoMado は再試行後の最終失敗を**エラー画面**とする確定要件を優先し、旧 latest を無条件に表示しない。 |
| 完成作品の公開と端末への PUSH 成否を分け、新送信は旧待機送信に優先する。端末は検証前に描画せず、物理表示後に ACK し、旧番号・重複を判定する。 | [push.py](../../../PicoW_E-ink_Display/ai_news/push.py) `91–163,255–319`、[push_receiver.py](../../../PicoW_E-ink_Display/pico/push_receiver.py) `144–190` | PicoMado の共通 WebSocket/ACK 契約で同じ性質を確認。既存 wire protocol をコピーする要件ではない。 |
| Web では題材・画風、revision 付き設定更新、手動生成、日別履歴からの任意作品送信を扱う。 | [server.py](../../../PicoW_E-ink_Display/ai_news/server.py) `124–281`、[GALLERY.md](../../../PicoW_E-ink_Display/docs/GALLERY.md) `9–28` | プラグイン Web 領域の操作と影響する端末、履歴表示・一時送信を確認。 |

旧 Pico の定時 GET 方式は現行仕様ではない。[PUSH 設計資料](../../../PicoW_E-ink_Display/docs/PUSH_DESIGN_2026-10-04.md) `5–18` を参照。PicoMado 初版 LCD の受入に E-ink 実機結果を含めない。

## 次の実装・検証順

1. 既存コードの入力・失敗・保存境界を新プラグインの契約試験へ写し、PicoMado 固有の共有範囲と受入条件を併記する。
2. YouTube の取得・変換・完成公開を隔離ジョブで試験し、LCD 実機の向き・色・fps を測る。既存本番の DB やキャッシュは読取りのままにする。
3. 風刺画は生成バックエンド、ニュース取得と作画の実行経路、LCD への表現を実装前に具体化する。保存履歴と表示エラーを別状態で管理する。
4. 既存 E-ink 端末の改修・データ移行、複数実機受入は別の承認・実機確認へ分ける。
