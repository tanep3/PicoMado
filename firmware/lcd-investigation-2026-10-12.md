# LCD 黒画面の比較調査（2026-10-12、追加書込み前）

## 実機の現在値

ユーザーはスタック修正版で「画面まっくら。ボタン押しても無反応」と報告。LCD の合格判定はしていない。修正版は対象 Flash ID `E661640843239D21` へ verify 100% で書き込まれ、21:41 UTC の新セッションで frame 65、21:49 UTC の frame 66 についてサーバが受信 ACK と描画 ACK を記録した。ACK は SPI API 戻り後のソフトウェア通知であり、LCD が光った証明ではない。21:53 UTC の frame 67 送信は 99,758 B のうち 95,471 B で約2秒止まり、サーバはセッションを切断。21:54 UTC の `/api/state` は offline、TCP 接続なし。ボタン入力と切断の因果関係はログから未確定。さらにユーザーは21:56 UTCにBOOTSELへ移したので、その時点以降のheartbeat停止・offlineは診断根拠にしない。

## 実働 YouTube 基準の端から端の比較

| 項目 | 完成確認済み YouTube | PicoMado 通信版 | 判断 |
| --- | --- | --- | --- |
| 実 UF2 | `video-rotate180.uf2` SHA256 `65900f...eaa1c`。2026-10-10 に対象 Flash ID と全画面表示を目視確認 | `PicoMado-lcd1.uf2` SHA256 `9671e0...1b49e4e`。今回の画面は黒 | YouTube を可視の正本とする |
| 本番ターゲット | `pico_w_jpeg_player` に `pico_player.c`、JPEG decoder、`lgfx_tjpgd.c` をリンク。LCD は `lcd_display.h` の Pico SDK `hardware_spi` 直送。LovyanGFX 本体は LCD ドライバとしてリンクせず、採用した E0/E1 ガンマ表のみ由来 | `picomado_lcd` に `main.c`、`lcd.c`、`net.c`、`ws_accept.c` をリンクし `hardware_spi` 直送 | 「LovyanGFXをそのままリンク」は実働経路と異なる |
| 初期化順 | `cyw43_arch_init()`→STA enable→Wi-Fi PM設定→`lcd_init()`→3秒のカラーテスト→Wi-Fi join | `stdio_init_all()`→`pm_lcd_init()`→config照合/ボタンGPIO→`cyw43_arch_init()`→STA enable→Wi-Fi join。通常ビルドは起動テスト帯なし | **差あり**。CYW43初期化後にLCDをリセットする順は YouTube と一致させる価値がある。黒画面原因かは未証明 |
| LCD ピン | SPI0 GP18 SCK、GP19 MOSI、GP17 CS、GP20 DC、GP21 RESET | 同一。GP12/13/14/15 はボタン | ピン衝突なし。Pico W CYW43 PIOバスはGP23/24/25/29 |
| SPI/初期化 | 20 MHz、8-bit、mode0、MSB、RESET 20/120ms、SWRESET150ms→SLPOUT120ms→COLMOD05→MADCTL60→INVON→E0/E1→DISPON20ms | 同じ値と同じ順序 | 低レベル値の変更は根拠なし |
| CS/DC/転送 | コマンドごとDC low、CS low→`spi_write_blocking`→CS high。データはDC high。同じ関数で 160×80×2=25,600 Bを送る | 同一 | SPI API完了はパネル受理の確認ではない |
| 表示窓・回転 | CASET 0..159、RASET 24..103、RAMWR。送信バッファのRGB565BEを180度反転 | 同一窓。`lcd_transform.h` で180度反転 | 設定値を変えない |
| 色補正 | 送信側で反転＋R/B交換したバイトを受信し、LCD側は回転だけ | サーバが通常RGB565BEを送信し、LCD側で同じ補正式を一度適用して回転 | 補正式は全65,536色ホスト比較済み。黒画面の原因として未証明 |
| バッファ・寿命 | 受信二重バッファとJPEG画素用バッファ、readyスロットをLCDループが所有しSPI完了まで保持 | `pm_assembly.pixels`にCRC合格のフレームを組立、メインループで同期SPI送信。`local_pixels`は独立静的バッファ | SPI中に同バッファを書き換える経路は現コードにない |
| BLK・スリープ | BLK未接続、GPIO制御なし。LCDへ SLPIN を送らず、常時画面更新 | `PM_LCD_BLK_CONNECTED=0` でGPIO制御なし。待機時は黒画素送信＋Wi-Fi停止、SLPINは送らない | BLKピンは有効化されていない。待機中の黒画面は仕様だが、現在待機かは未判定 |
| ネットワーク方式 | `pico_cyw43_arch_lwip_threadsafe_background`。LCD初期化はWi-Fi初期化後 | `pico_cyw43_arch_lwip_poll`。LCD初期化はWi-Fi初期化前 | GPIOは競合しないが順序・ライフサイクル差が残る |
| USB stdio | 本番は無効 | 有効 | USB GPIOは使わない。自己診断版も有効でLCD表示をユーザー確認済み |

参照: `/home/tane/dev/PicoW/PicoW_Youtube_Player/CMakeLists.txt`、`pico/lcd_display.h`、`pico/pico_player.c`、`docs/diagnostics/BUTTONS_ROTATION.md`、`docs/wiring.md`。既存 YouTube ソースとサービスは読み取りのみ。

ホストのGPIO/SPIモックで YouTube の実 `lcd_display.h` と PicoMado の実 `lcd.c` をそれぞれ実行し、初期化、GPIO操作、待ち時間、SPI0設定、全コマンド/データ、合成RGB565フレーム25,600 Bの転送までバイナリtraceを比較した。YouTube経路には実送信器と同じ反転＋R/B補正を事前に適用。両traceは完全一致（SHA256 `4196e917e183dfce8fd3cc2e0e309f957133bd7ada92f3be25dc124f70238e48`）。よって低レベルLCDルーチンを単純コピーするだけでは黒画面の振る舞いは変わらない。主な未検証差分は初期化の時点、起動テストパターンの有無、ネットワークの実行方式、動作中の状態遷移である。

## 状態遷移と切り分け

`pm_lcd_init()` 後、端末固有ファームが触るLCDピンは `lcd.c` のCS/DC/RESET/SCK/MOSIだけ。ボタン初期化はGP12..15、CYW43はGP23/24/25/29。設定適用は最後のFlash sectorに保存し、SPIレジスタやGPIOを変更しない。Wi-Fi待ち／再接続／本体設定では `pm_lcd_local()`、サーバ画像では `pm_lcd_draw()` を使う。待機は `pm_lcd_black()` を送ってWi-Fiを止める。待機からの押下は `start_wifi()` でローカル描画を再開するはずだが、実機の状態遷移はUSBログが無いので判定できない。現写真がローカル画面から黒いのか、サーバ画像から黒いのかが最大の未解決点。

まず追加書込みなしで電源再投入後の最初の数秒を動画に記録し、サーバ接続・フレーム時刻と照合する。この版にはYouTube式の3秒テスト帯がないため、ローカルの `WIFI CONNECTING` / `SERVER CONNECTING` / `PICO MADO WAIT IMAGE` が見えるかを確認する。起動直後から黒ならローカル描画、画像受信直後から黒ならサーバ画像経路を優先する。ただし短い表示を動画で捉えられなければ結論を出さない。

次の1回の診断UF2案は、YouTube `lcd_display.h` のコマンド/データ/初期化/表示窓ルーチンをLCDバックエンドの基準として共用可能な形に整理し、CYW43初期化→LCD初期化の順も一致させる。現低レベル転送とtraceが同一なので、コピー自体は修理の根拠とせず、初期化順と可視の段階表示で差を測る。起動時はYouTubeと同じ白枠＋RGB三帯のテストパターンを3秒表示し、Wi-Fi接続後にも同じローカルパターンを短時間表示してからサーバ画像へ移る。USB CDCには秘密情報を含めず、段階名・ボタン番号・フレームID・SPI完了だけを出す。これで一回の書込みでも初期化直後、Wi-Fi後、画像後の可視差とボタン処理を分離できる。今はビルド・書込みを行わず、先にこの診断案を親へ報告する。既知正常の自己診断UF2への復帰は、診断UF2でも最初のパターンが見えない場合の第二段階とする。

## 表示バックエンドの責務境界

`display.h` を通信・状態遷移側の唯一の窓口とし、`init`、`show_status`、`draw_frame(bytes,len,format)`、`standby`、`wake`、能力・寸法を提供する。LCD実装 `display_lcd.c` はYouTube由来のSPI0/ST7735S低レベル描画、RGB565色補正、ローカル画面を所有する。E-inkは別 `display_eink.c` に初期化、全画面/部分更新、表示保持、sleep/wakeと固有のボタン数・画素形式を閉じ込める。`display_select.c` は端末configの物理種別で一方のみリンクし、存在しない実装・ピン構成・サーバ形式はビルド時に拒否する。`main.c` は画像世代・CRC・ボタン・接続・待機遷移を持ち、LCDコマンドやE-ink更新を含めない。現サービスのv1画像契約はLCD 160×80 RGB565BEのみなので、E-inkの実装済み・実機検証済みとは扱わず、後続の形式契約と実機確認が必要。
