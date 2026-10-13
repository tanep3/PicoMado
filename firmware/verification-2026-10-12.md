# 2026-10-12 PicoMado LCD firmware verification

## 対象

- raspi5 ローカル USB 上の BOOTSEL デバイスは作業時 1 台のみ。`picotool info -d` は RP2040、2 MiB Flash、Flash ID `0xE661640843239D21`。既存 YouTube LCD 端末の診断記録と一致。
- 書込み直前の `picotool info -a` は `pico_w_jpeg_player` / board `pico_w`。既存 `video-rotate180.uf2` に対する `picotool verify` は 100% OK。復旧用 UF2 の SHA256 は `65900f5224c64efdde49e9652d035ad34416fb532212f92740396825b70eaa1c`。
- 配線は既存 `docs/wiring.md`、既存 `pico/lcd_display.h`、PicoMado `kernel.md` の GP17〜21、GP12〜15 と一致。既存ボタン実機診断も参照。

## ビルドとホスト検査

- `PICO_BOARD=pico_w` / Release で `picomado_selftest` をビルド。対象リポジトリの `firmware/` から別の `/tmp/picomado-target-build` に再ビルド成功。UF2 SHA256 は `22ee9ec70a83a5439de3f2fad7cff2999dcddf321c02b3b93090ea56180b0a24`。
- 初期の単一 Config 案ではダミー値だけで `picomado_lcd` をビルドし、実資格情報は扱わなかった。その後ユーザー指示で端末別構成に変更した。
- Pico SDK の `pico_get_unique_board_id_string()` は同じ Flash unique ID を返す実装と照合。現在の接続ファームは選択端末名と `PM_DEVICE_ID` を比較し、端末別 `board-id.txt` の ID を実ボード ID と照合する。本人指定の `PM_DEVICE_ID` は `PicoMado-lcd1`。
- 接続ファームはダミー設定でクロスビルド成功。最終の静的 RAM (`bss`) は 188,816 B。32 KiB の TCP 受信バッファと 32 KiB の WebSocket アプリバッファを使う。TCP 受信窓は 23,360 B。受信済みデータをバッファへ取り込んだ時点で、末尾に 1 受信窓分を確保しつつ枠を返す。
- `cc -std=c11 -Wall -Wextra -Werror` による `tests/core_test.c` と `tests/ws_accept_test.c` が成功。画像長、連続 offset、CRC、世代照合、35 ms ボタン安定判定、WebSocket Accept の RFC 6455 既知値を確認。
- `tests/rx_flow_test.c` を同じ警告設定で実行成功。1 バイト単位を含む分割受信、17 KiB 相当のフレーム 3 通の連続受信、受信窓とバッファ容量の不変条件、容量超過の拒否、接続途中切断時のリセットを確認。現サーバの画像断片は 4096 バイトで、JSON 整数配列化しても 17 KiB 未満。将来の契約変更時は上限を再確認する。
- ユーザー作成の `firmware/config.h` を内容変更なし・同じ inode で `firmware/devices/PicoMado-lcd1/config.h` へ移動。非秘密の `board-id.txt` は対象実機の確認済み ID を格納。両ファイルと `firmware/build/` の Git 除外を確認。旧ルート `firmware/config.h` は存在しない。
- `sample-a → sample-b → sample-a` を端末別ビルドフォルダで成功。A の再ビルド UF2 SHA256 は不変、各 ELF には自身の dummy ID があり他方の ID はない。サンプル構成で秘密値や実機 ID は使わない。
- 本人指定 `PicoMado-lcd1` の実 `config.h` を `tools/build.sh --device PicoMado-lcd1` でビルド成功。最終 UF2 は `build/PicoMado-lcd1/PicoMado-lcd1.uf2`、SHA256 `af2c7204a4e74c0c0cabf4aa6306940edcdb9cc8a8a4455481cd9b6356578401`、権限 600、Git 除外。ELF の自己 ID と期待ボード ID の存在、ダミー ID の不在を検査。`picotool info` は RP2040 UF2。
- `tools/flash.sh --device PicoMado-lcd1 --check-only` が manifest の端末名・ボード ID・UF2 ハッシュと現在の単一 BOOTSEL ボードの一致を返した。`--check-only` は書込みを行わない。ダミー端末の明示選択は拒否を確認。実 UF2 の書込みはサーバ待受と親の段取りまで保留。

## 実機

- 上記単一 Flash ID を書込み前に再確認し、`picotool load -v -x /tmp/picomado-target-build/picomado_selftest.uf2` 実行。Flash 書込み 100%、verify 100% OK、picotool は再起動実行を報告。
- 再起動後 `lsusb` に `2e8a:000a Raspberry Pi Pico`、`/dev/ttyACM0` を確認。USB CDC の 5 秒読み取りでは出力なし。起動直後の一度きりのログはポート開放前に流れた可能性があり、シリアルログによる動作確認とは扱わない。
- ユーザーは POWER・SUB・UP・DOWN の全ボタンが反応すると確認した。4 ボタン項目は合格。LCD の向き・色の明示的な目視確認は未取得。Wi-Fi とサーバ通信はこの自己診断 UF2 に含まれない。
- ユーザーの再 BOOTSEL 後、単一 `RPI-RP2` の同じ Flash ID を再確認。`picotool info -a` は `picomado_selftest` / board `pico_w` を示し、書込み済み自己診断 UF2 に対する `picotool verify` は再度 100% OK。次の接続ファームは実設定とサーバ待受が揃うまで書き込まない。
- 臨時ホストが `192.168.0.120:16153` で待受・HTTP 200 を返し、プロセス `temporary_lan_host` であることを確認。再度同じ単一 Flash ID を確認し、`tools/flash.sh --device PicoMado-lcd1` で接続ファームを書込んだ。Flash 書込み 100%、verify 100% OK、picotool は再起動を報告。
- 初回接続ファーム再起動後の USB は `2e8a:000a Raspberry Pi Pico`、`/dev/ttyACM0`。サーバ側に `192.168.0.173` からの持続 TCP を確認。`/api/state` は `PicoMado-lcd1` online、設定 stored/confirmed revision 1。ユーザーの DOWN 操作 2 回でサーバ側選択が weather まで進み、ボタン往復が成立した。一方、画像断片の JSON 長が当初の TCP 受信窓 11,680 B より長く、フレーム受信前に枠を返さなかったため転送が停止。サーバ TCP Send-Q と outbox の画像送信から原因を特定した。
- 受信枠を修正後、ユーザーが同じ Pico を BOOTSEL に戻した。`tools/flash.sh --device PicoMado-lcd1 --check-only` は Flash ID `E661640843239D21` と個体専用 manifest の一致を確認。`tools/flash.sh --device PicoMado-lcd1` は Flash 書込み 100%、verify 100% OK と再起動を報告。USB CDC が再列挙され、持続 TCP を確認した。サーバ `/api/state` は `PicoMado-lcd1` online、選択 weather、`last_received_frame=4`、`last_rendered_frame=4` を返した。受信 ACK と描画完了 ACK がサーバへ別々に届いたことを確認。LCD の内容・色・向きは目視未確認。

## 次の縦経路に必要な条件

家の direct 接続・設定同期・ボタン往復・画像の受信 ACK と描画完了 ACK は確認できた。LCD 目視、家/外メニュー・待機・Wi-Fi 断からの再接続・SOCKS5 経路の実機確認が残る。汎用サーバメニューの配信契約は現 `protocol.rs` にない。次の書換えでは同じ Pico W の BOOTSEL と Flash ID を再確認する。

## 黒画面の追加切り分け

- ユーザーの目視報告は「PICO は画面真っ暗」。先の RenderedAck は `pm_lcd_draw()` の SPI ブロッキング送信後に送るが、ST7735S からの読戻しはなく、可視表示の証明にはならない。実表示は未合格へ訂正する。
- 自己診断と接続ファームは同じ `lcd.c` の初期化・描画処理を使う。GP17〜21、RGB565、MADCTL 0x60、回転 180 度、Y オフセット 24 は既存動作コードと一致する。GP22 BLK は両ファームで未駆動。自己診断では 4 ボタン表示をユーザーが目視できた。黒画面の確定原因は未特定。
- 端末固有 `config.h` とダミー `config.sample.h` に非秘密のハード定義を追加し、ドライバ・ボタン・画像長検査・Hello 能力へ結線した。既存実設定のバイト列は追記前後で一致し、権限 600・Git 除外を維持。E-ink 指定はコンパイル時に拒否されることをホストで確認。
- sample-a、sample-b のクロスビルド、CRC・ボタン・TCP 受信枠のホスト試験は成功。個体用 LCD 診断 UF2 はビルド成功、SHA256 `eebf1c2129e0b46272ed4a4bfb14c3c47045f6913b3bdd0b51d38c39a8638441`、静的 RAM 188,816 B。起動時 2.5 秒の四色帯、画像描画直前 0.5 秒の四色帯、非ゼロ画像バイト数とローカル画面遷移の USB CDC 出力で表示段階を切り分ける。
- ユーザーが同じ実機を BOOTSEL に戻した後、`flash.sh --check-only` で単一 Flash ID `E661640843239D21` と個体 manifest の一致を確認。診断 UF2 を書込み、Flash verify 100% OK と再起動を確認。USB CDC 復帰、サーバ online、frame 10 とその後 frame 14 の受信・描画 ACK 両方を確認。USB CDC は frame 11〜13 について「非ゼロバイト 25,600/25,600」と SPI 送信完了を出した。四色帯と天気画像のユーザー目視はまだ未取得。画像が実際に見えたかは ACK からは判断しない。

## 色と表示位置の照合・修正版

- ユーザーは診断版で「天気らしいが壊れた画面、色もおかしい」と報告。既存 YouTube 完成版の `pico/lcd_display.h` と比較すると SPI0 20 MHz、SWRESET→SLPOUT→COLMOD `0x05`→MADCTL `0x60`→INVON→E0/E1 ガンマ→DISPON、窓 X=0..159・Y=24..103、180 度画素回転、RGB565 高位バイト先は一致する。ガンマの正負各16バイトも一致。Y オフセット 24 は既存と同じ RASET 側で変更していない。
- 差は色補正。既存 `server/streamer.py` と `pico/rgb565_transform.h` は送信画素へ反転＋赤青交換を適用するが、PicoMado の Rust 描画器は標準 RGB565BE をそのまま送っていた。端末 `config.h` と両 sample に `PM_LCD_COMPENSATE_INVERT_BGR=1` を追加し、LCD 転送前に一度だけ変換する。診断帯、ローカル画面、サーバ画像に同じ変換を使う。
- 補正式は既存関数と全 65,536 色で一致するホスト比較を通過。`tests/lcd_transform_test.c` は黒・白・三原色、変換の往復、160×80 全画面の行ストライド・180 度回転・ビッグエンディアン保持を検査した。`pm_chunk` は 25,600 B を offset 順に静的バッファへコピーし、CRC 合格後に同期 SPI で全画面送信する。受信と描画は同一メインループで直列に行われ、SPI 送信中に次画像でバッファを書き換えない。描画後に RenderedAck を送る。
- 色補正済み個体 UF2 SHA256 `a6d5b18d958c696331ccb753593b10be95ce1907f230e6eb60aba8ea5bba063f` をビルド。sample-a・sample-b もビルド成功。単一 BOOTSEL Flash ID `E661640843239D21` と個体 manifest を照合後に書込み、Flash verify 100% OK・再起動を確認。USB CDC が復帰し、サーバは online・選択 rain-radar・frame 27 の受信/描画 ACK を返した。色・文字・配置の実画面はユーザー目視待ち。

## 実機写真と診断帯の再点検

- ユーザー添付の実機写真を Library から作業領域に取得し、2048×1536 JPEG として可読性と実画素を確認。画面上部に四色帯、その下に雨雲らしい見出しと広い白系領域が写る。雨雲レーダーのサーバ状態は `error=true` で、見出しと白系背景は取得失敗画像とも整合する。写真が色補正版の書込み前後どちらか、上部の帯が数秒後も残るかは未確認。
- LCD ドライバを GPIO/SPI モックで実行。診断帯と続く画像の両方が同一 `CASET 0..159 → RASET 24..103 → RAMWR` と 25,600 バイト転送を使い、初期化の COLMOD/MADCTL/INVON/ガンマも既存 YouTube 機と一致した。SDK の `spi_write_blocking` は TX FIFO へ全バイトを入れた後、シフト完了まで待ってから返る。フレーム組立と SPI はメインループ内で直列で、描画中の受信バッファ上書きは見当たらない。
- 毎画像の診断帯 0.5 秒と本画像の切替途中が写真に写った可能性がある。実個体 `config.h` の `PM_DIAGNOSTIC_BOOT` だけ 1→0 に戻し、他の設定バイトは保持。診断文字列を含まないクリーン版 UF2 SHA256 `b5b5fa78ed83777582cd6306fc334f70eb7a83a3846a02e2254d85de0691772d` をビルドしたが**未書込み**。現在の実機は依然として四色帯ありの診断版。帯の持続性とサーバ再起動後の表示を確認してから次の書込みを判断する。
- サーバ担当の Web UI 反映に伴う再起動を 14:15 UTC ごろ確認。Pico は新セッションで online、選択 weather、設定改訂維持。直後の `/api/state` は現セッションの受信/描画 ACK がともに `None`。USB CDC を 16 秒読んだ間、新たな LCD ローカル画面・画像描画の診断行は 0 件。古い写真の表示内容と現セッションの画面を同一視しない。クリーン版 UF2 の書込みはユーザーの持続表示回答まで保留する。
- その後サーバ担当が writer 終了時の reader 残存と雨雲 gzip 応答を修正・導入。ユーザーが改めて BOOTSEL を連絡したので、現在の `config.h`（`PM_DIAGNOSTIC_BOOT=0`、色補正1、Yオフセット24、180度回転）、ソース、manifest の端末名・Flash ID・SHA256 を再照合。ホストの core、LCD 変換、TCP 受信枠、WebSocket Accept の4試験を再実行して成功。個体ビルドは同じクリーン UF2 SHA256 `b5b5fa78ed83777582cd6306fc334f70eb7a83a3846a02e2254d85de0691772d` を再生成した。
- `flash.sh` は単一 BOOTSEL の Flash ID `E661640843239D21` を再確認してから書込み、Flash verify 100% OK と再起動を報告。USB CDC が新たに列挙され、サーバ状態は online・選択 weather・frame 63 の受信/描画 ACK を返した。これが現在の実機ファーム。診断四色帯なしの天気画面について、色・文字・配置の最終目視は未取得。

## 追加写真・停止状態・スタック調査

- 次の実機写真 `756(1).jpg` を Library から読み取り、2048×1536 JPEG の全体を確認。LCD のほぼ全表示領域が細かな多色ノイズで、文字・診断四色帯は読めない。先の写真の四色帯と雨雲エラー画像の混在とは異なり、瞬間的な撮影だけでは説明できない。ユーザーは「いつまでも砂嵐、ボタンも反応しない」と報告した。現在の可視表示は不合格。
- 既知正常のボタン自己診断 ELF と通信版 ELF の LCD 初期化を比較。どちらも `pm_lcd_init` のサイズは 0x1b4、SPI0 20 MHz、ST7735S 初期化、X 0..159/Y 24..103、25,600 B の同期転送、GP12/13/14/15 ボタン配線が同じ。通信版だけがサーバ RGB565BE を既存完成版の方式へ色補正する。全色ホスト比較は既存送信器と一致。これだけでは砂嵐の原因を断定できない。
- 2026-10-12 20:59 UTC 頃の稼働サーバ `/api/state` は対象 `online=true`、選択 rain-radar、現セッションの受信/描画 ACK はともに null。サーバ TCP の `192.168.0.173:61288` からの `bytes_received=1545` は約9分以上増えておらず、15秒 heartbeat に反する。`online` 表示は持続 TCP による見かけ上の可能性があり、メインループが停止したかはまだ未確定。入力イベントが送信されないのか、ノイズに隠れているだけかも現時点では未確定。
- 実際に書き込まれた通信版 ELF の RP2040 `__StackTop=0x20042000` / `__StackBottom=0x20041800`、予約スタック 2,048 B。逆アセンブルで `main` は約1,080 B、旧 `pm_net_poll` は単独で約2,096 B のスタックを取る。ネストした Wi-Fi/lwIP コールバックが入るためスタック範囲を超える実害があり得る。これは具体的な欠陥で、画面・通信・ボタンの複合停止の有力候補だが、実機のプログラムカウンタやスタック破損を直接観測していないため根本原因確定とはしない。
- `net.c` の WebSocket 応答ヘッダー 2,049 B と送信パケット 1,040 B、および `main.c` の送信 JSON ワークスペースを静的 BSS に移した。いずれも単一メインループ内で使い、送信は `TCP_WRITE_FLAG_COPY`。新 ELF では `pm_net_poll` は約40 B、`pm_net_send` は約72 B のフレーム。静的 RAM 192,420 B、再ビルド成功、core/LCD transform/RX flow/WS accept のホスト試験成功。新 UF2 SHA256 `6784e2d6208aec8e95951647f7bd405351e0e1eae48f85d55e56e8cb7deca839` は**未書込み**。
- 次の切り分けは (1) 現ファームのまま対象ボタンを1回押す前後でサーバの Pico 受信バイト、選択、ACK を同期観測して入力経路を確かめる、(2) 対象を単一 BOOTSEL/Flash ID で再識別し、既知正常の自己診断 UF2 へ戻して四色帯ではなくボタン文字画面が安定して表示されるか確かめる、(3) その後にスタック修正版を同じ対象に書き、起動画面→通信画像→各ボタンと heartbeat/ACK を別々に観測する、という順序。追加書込み前に親へこの計画を共有する。

## スタックの定量検証（追加書込み前）

- 実書込み版のソースから、修正箇所だけを元に戻した `main_old.c` / `net_old.c` を `/tmp/picomado-stack-audit` へ再現。個体ビルドと同一の ARM GCC 定義・include・`-O3` フラグに `-fstack-usage` だけ追加してコンパイルした。実書込み版 ELF の逆アセンブルも `pm_net_poll` の 2,096 B フレーム、`main` の 1,080 B フレームと整合する。
- 旧版の関数フレーム: `main` 1,080 B、`pm_net_poll` 2,096 B、`pm_net_send` 1,104 B、`upgrade` 424 B、`pm_ws_accept` 200 B、SHA-1 `block` 392 B。旧 `__StackTop=0x20042000`、`__StackBottom=0x20041800`、予約は2,048 B。通常の `main→pm_net_poll` だけで3,176 Bとなり予約を1,128 B超える。`main→pm_net_poll→cyw43_arch_poll→connected_cb→upgrade→pm_ws_accept→block` は SDK 内部を除いて約4,200 Bになり、scratch Y 4,096 B の物理容量にも迫る（正確な旧版最高水位は未測定）。同じ関数が LCD 自己診断版にないことは、通信版固有の疑点。
- 最終修正版の関数フレーム: `main` 632 B、`pm_net_poll` 40 B、`pm_net_send` 72 B、`upgrade` 104 B、`pm_net_take` 208 B、`pm_ws_accept` 200 B、`block` 392 B。ヘッダー2,049 B、WebSocket送信1,040 B、HTTP要求320 B、SOCKS認証513 B、送信JSON512 Bを静的BSSへ移動。残る自動配列はSHA-1 `w[80]` (320 B)、受信pong131 B、SHA input128 B、64 Bキー名などで、同様の1 KiB級配列はない。`pm_net_poll` 内のSOCKS分岐は最終ビルドで同関数へインライン化済み。
- 通信版の `PICO_STACK_SIZE=0x1000` はリンクmapで `__StackBottom=0x20041000`、`__StackTop=0x20042000` と確認。4,096 B はRP2040 scratch Y全域。最悪の明示的呼出し連鎖 `main→pm_net_poll→cyw43_arch_poll→connected_cb→upgrade→pm_ws_accept→block` の自前フレームは1,392 B（SDK側は未集計）で、SDK処理と割込みに2,704 B残る。これは静的な下限であり、SDKの全呼出し・割込み深さを含む最高水位の証明ではない。実機ではスタック最低水位またはガードを追加観測することが望ましい。
- 最終 ELF の `.bss=193,256 B`、`__bss_end__=0x20030a80`、主RAMのheap上限 `0x20040000` まで62,848 B。stackは別のscratch Y bankで、静的バッファ移動による主RAM不足はリンク上ない。
- `pico_cyw43_arch_lwip_poll` は単一メインループで `cyw43_arch_poll()` によりlwIP callbackを処理する。`ws_header`、`outbound_json`、`tx_packet` はそれぞれ同期的な1用途で、callbackはこれらに書かない。`tcp_write(...,TCP_WRITE_FLAG_COPY)` により `tx_packet`、`upgrade_request`、`socks_auth_request` の送信寿命は `send_raw` の戻りまで。SOCKSの認証用コピーはその直後に消去する。受信callbackは別の `rx` とフロー状態のみを触る。割込みから `pm_net_send` 等を呼ぶ登録はファームソースにない。SDK内部の再入が絶対にないことまでは動的検証していない。
- 修正版は個体向けReleaseビルドと core/LCD変換/RX flow/WS Accept の4ホスト試験に成功。最終 UF2 SHA256 `9671e0dfcfe7014a78ceb8d0d600bc77cec631cc7cb44f2ed976aa24a1b49e4e`。**未書込み**。サーバ再起動も行っていない。
- 書込み回数を抑える場合は、現状態のままボタン1回前後の TCP受信量とサーバ選択を確認し、対象個体の再BOOTSEL/Flash IDを照合して修正版を1回だけ書く。再起動直後のローカル接続画面、15秒heartbeat、画像受信/描画ACK、通常画面、ボタン4個のサーバイベントを順に確認する。ローカル画面から既にノイズならLCD初期化/ハード側、ローカル画面が正常でサーバ画像からノイズなら画像経路側を再調査する。表示の最終判定にはユーザーの目視写真が必要。

## スタック修正版の単一実機書込みと初期通信確認

- ユーザーが BOOTSEL を連絡。`flash.sh --check-only` と `picotool info -d` により単一 RP2040 2 MiB、Flash ID `E661640843239D21` を確認。端末 manifest は `OFF PicoMado-lcd1 E661640843239D21 9671e0dfcfe7014a78ceb8d0d600bc77cec631cc7cb44f2ed976aa24a1b49e4e` と実 UF2 SHA256 に一致。`PM_DIAGNOSTIC_BOOT=0`、Yオフセット24、MADCTL0x60、180度回転、反転＋赤青補正、GP12〜15ボタンを非秘密のマクロで確認。既存の完成済み YouTube 版 UF2 は復旧用として保持。
- `flash.sh --device PicoMado-lcd1` を対象1台へ実行。Flash load 100%、verify 100% OK、picotool はアプリ再起動を報告。USB は `2e8a:000a Raspberry Pi Pico` として再列挙。サーバサービス再起動やOS/network変更は行っていない。
- サーバ PID 964178 の新しいTCP `192.168.0.120:16153 ↔ 192.168.0.173:60310` が ESTAB。サーバ `/api/state` は `PicoMado-lcd1` online、rain-radar、`last_received_frame=65` と `last_rendered_frame=65`、設定 stored/confirmed revision1、pending=false を返した。受信ACKと描画ACKは両方成立。ただしLCDの視認表示証明ではない。
- 同じTCPの Pico→サーバ `bytes_received` は 797→1185→1379 と増加。その後5秒刻みの測定で21:43:49 UTCに1670、21:44:04に1767、21:44:20に1864と毎回97 B増えた。サーバの設定値15秒と一致する周期で同じTCPにPico発信データが継続し、`lastrcv` も更新された。TCPは切断されず、ACK状態も維持。実機可視画面と4ボタン動作は親経由のユーザー確認待ち。

## YouTube基準の段階診断版（実機、画面目視待ち）

- ユーザーの21:56 UTC BOOTSEL待機を受け、単一RP2040 `2e8a:0003` の Flash ID `E661640843239D21` と個体 manifest/UF2 SHA256 `fbcac607fb55776e1a9077f8c552c4aa66f71f1962c221af6740930a2b292f29` を `flash.sh --check-only` と `picotool info -d` で一致確認。診断四色帯の毎フレーム挿入はOFF、Yオフセット24・MADCTL0x60・回転180・通常画像色補正は維持。
- YouTube本番と同じCYW43→LCD初期化順へ変更。`display.h`/`display.c`を共通表示入口、`lcd.c`をLCD専用バックエンドとし、未対応E-inkは明示的にビルド拒否。起動白枠＋RGB横三帯6秒、ローカルボタン診断25秒、Wi-Fi成立後同じ三帯6秒を `PM_DIAGNOSTIC_STAGE=1` で実装。通常サーバ画像ごとに診断帯を出さない。端末configのフラグ0で段階診断を無効化可能。
- 実働YouTube `lcd_display.h` と現在LCDドライバのGPIO/SPIホストtraceは初期化・全画素送信まで完全一致。さらに新起動三帯とYouTube `show_test_pattern()` のGPIO/SPI traceは同一SHA256 `c823e132cfe3054f1a21586f26c85bd81df10c051840c7682dd639766a99f44b`。本番個体・ダミーsample-a/bのクロスビルド、core/LCD変換/RX flow/WS Acceptホスト4試験、E-ink選択の明示拒否試験が成功。最終ELF `.bss=193,256 B`、stack reserve4,096 B、`main`フレーム624 B。
- `flash.sh --device PicoMado-lcd1` で単一対象へ一回だけ書込み、load100%、Flash verify100% OK、picotoolがアプリ再起動を報告。USBは`2e8a:000a Raspberry Pi Pico`で再列挙。USB CDCを事前監視して、22:04:58 `boot_pattern`、22:05:04 `local_buttons`、22:05:29 `wifi_connect`、22:05:32 `wifi_up_pattern`、22:05:38 `SERVER CONNECTING`、22:05:40 `welcome session=23` / `frame_spi_complete=69`を確認。22:05:55、22:06:10、22:06:25の`heartbeat_sent`が15秒間隔。サーバAPIはrain-radar、online、frame69の受信/描画ACK両方、設定改訂1 confirmed。
- ローカル25秒間にはボタン押下USBログが無かった。実際に押されたか未確認なのでボタン機能の合否は判定しない。USBのSPI完了・ACKはLCD画面の視認表示を証明しない。白枠/三帯、ボタン名、サーバ画像の実機目視は親経由のユーザー確認待ち。サーバ、OS/network、他サービス、YouTube側は変更していない。

## 22:53 UTC 電源再投入後の黒画面・入力報告

- ユーザーは「電源刺した。画面まっくら。１〜４ボタン押下すべて無反応。配線切れたかな？」と報告。追加書込みは行っていない。USBを開く前に確認したところ、Picoはアプリ USB `2e8a:000a` で再列挙し `/dev/ttyACM0` が存在。サーバは新session 24、online、weather、frame71・72の受信/描画ACKを記録。これはCPU・USB・Wi-Fi・画像受信・SPI API呼出しが走った証拠だが、LCD発光・配線導通の証明ではない。rain-radar→weather選択変更が見られたが、Web操作の可能性を排除できず、ボタン成功とは断定しない。
- Pico SDK `stdio_usb_connected()` はDTRでログ出力可否を判定する。BOOTSELリセットのCDC条件は1200 baudの設定変更で、DTR単独ではない。22:54 UTC以降、baud変更・装置への送信なしでCDCを開いて読み取った。`diag heartbeat_sent` は22:54:40、:55、22:55:10、:25、:40、:55、22:56:10、:25と継続。主ループはその間動作。監視期間中に`diag button=`や`diag local_button=`は0件。ただしユーザーの4ボタン操作は監視開始前であり、起動ログと押下ログはCDCから遡れない。監視中に押下した証拠がないため、ボタン配線の合否は未判定。DTRによるログ出力状態の変化はあり得る。
- 以前ユーザーが4ボタンすべての表示を確認した**当時の実ファイル**は `/tmp/picomado-target-build/picomado_selftest.uf2`。SHA256 `22ee9ec70a83a5439de3f2fad7cff2999dcddf321c02b3b93090ea56180b0a24`、`picotool info` は `picomado_selftest` / board `pico_w` / SDK2.3.1 / Release、2026-10-12 build。実際に対象 Flash ID `E661640843239D21` へ書込み・100% verifyし、ユーザーの全ボタン確認を受けたことは本記録上部に記載。現ソースから再ビルドされた `firmware/build/PicoMado-lcd1/PicoMado-lcd1-selftest.uf2` は SHA256 `2df4bcfae0b490874cefdac287ec142ca5c249b8d0767a36590ae0d9cdf9177b` で別物。旧実績品を作業領域 `/home/tane/Documents/Codex/2026-10-12/task-5/rollback/picomado-selftest-user-verified.uf2` に同一ハッシュ・権限600で保全。今回の書込み指示はなく未書込み。

## 当時の自己診断UF2へ書戻し

- ユーザーの新たな BOOTSEL 指示を受け、単一 USB `2e8a:0003`、RP2040 2 MiB、Flash ID `E661640843239D21` を確認。保全済み・当時ユーザーが4ボタン表示を確認した `rollback/picomado-selftest-user-verified.uf2` のSHA256は `22ee9ec70a83a5439de3f2fad7cff2999dcddf321c02b3b93090ea56180b0a24`、`picotool info` は `picomado_selftest` / `pico_w` / SDK2.3.1 / Release。再ビルドは行わなかった。
- `picotool load -v -x` でそのファイルを対象1台へ一度だけ書戻し、Flash load100%、verify100% OK、picotoolがアプリ再起動を報告。USBは`2e8a:000a Raspberry Pi Pico` / `/dev/ttyACM0`で再列挙。ファームの実可視画面はユーザー目視待ち。自己診断はWi-Fi/サーバ通信を持たないため、サーバofflineは異常判定に使わない。
- 期待画面は起動時 `PICO MADO` / `PRESS BUTTON`。GP12 SUB、GP13 UP、GP14 POWER、GP15 DOWN の安定した押下で `BUTTON` / `SUB|UP|POWER|DOWN` に変わり、次の押下まで保持する。これは当時の自己診断ソースとELFの文字列、先行ユーザー確認に基づく仕様であり、今回の物理表示結果を代弁しない。

## 自己診断UF2の履歴と画面確認の訂正

- 復旧に使ったUF2は当時の `/tmp/picomado-target-build/picomado_selftest.uf2` とバイト単位で一致する（SHA256 `22ee9ec70a83a5439de3f2fad7cff2999dcddf321c02b3b93090ea56180b0a24`）。元のビルド時刻は 2026-10-12 21:35:43 JST。復旧コピーの 2026-10-13 07:55:43 JST はコピー時刻である。後日再ビルドした個体向け selftest UF2（SHA256 `2df4bcfae0b490874cefdac287ec142ca5c249b8d0767a36590ae0d9cdf9177b`）は使っていない。
- 最初の書込み・verify・ユーザーの4ボタン反応・再BOOTSEL後のverifyは本記録の上部に記載。ただし最初の書込みと再verifyの独立した生ログは見つからず、当時の記録に依拠する。今回の書戻しの生ログ `/tmp/picomado-selftest-restore.log` は load/verify 100% OKを示す。
- 当時確認できたのは4ボタンの反応である。LCDの色・向き・画面全体の安定表示は確認されていなかった。「当時正常だった自己診断UF2」「既知正常の自己診断」という表現を全画面表示の意味に解釈しない。旧ELFには後から追加した反転・赤青交換の色補正がなく、色差は予想できるが、砂嵐や文字残片の原因は確定しない。
- 復旧後のユーザー写真757/758では `SUB` の文字が一部読める一方、砂嵐と崩れた文字・残像が写る。入力と一部表示の動作はうかがえるが、画面全体は不合格。過去から同じ崩れ方だったか新たに生じたかは判定できない。
- 現在のアプリ動作状態から `picotool save -f` でFlashを読み出す場合、ツールの仕様上リセットとBOOTSELへの遷移が起こる。現状態を変えない再読出し手段は確認できていないため、追加の実機照合は行っていない。

## YouTube 実働版との比較範囲の訂正

- 比較元の YouTube UF2 は `PicoW_Youtube_Player/diagnostics/artifacts/buttons-rotation-20261010/video-rotate180.uf2`。当時のビルド `/tmp/picow_video_rotate180_20261010/pico_w_jpeg_player.uf2` と SHA256 `65900f5224c64efdde49e9652d035ad34416fb532212f92740396825b70eaa1c` が一致。当時の flash-result 記録は対象 Flash ID `E661640843239D21`、Flash verify 100%、ユーザーによる画面の向き・色・収まりの確認を記す。
- 当時の CMakeCache/link.txt は YouTube 本番ターゲット `pico_w_jpeg_player`、Release/pico_w、`DISPLAY_PROFILE=0`、`PLAYER_LCD_GAMMA_LGFX=ON`、`PLAYER_ROTATE_180=ON`、`PLAYER_CYW43_DIAG=ON`、`pico/pico_player.c`・JPEG decoder・診断CYW43ソースのリンクを示す。現 YouTube の LCD ヘッダ、プレイヤーソース、CMakeLists は完成時 commit `9ffb634` と同一。YouTube 実個体の非秘密 LCD 定義は 20 MHz、MADCTL 0x60、X0/Y24、INVON。
- ホストtraceの比較元はその `pico/lcd_display.h` だが、UF2バイナリそのものを実行した試験ではない。`/tmp/picomado-lcd-mock/compare.c` と `pattern_compare.c` は、同ヘッダ/PicoMado LCD実装を共通stubへリンクし、GPIO/SPI APIの引数・呼出順・送出予定バイト列を記録した。両組のtraceは各25,927 Bでバイト一致し、SPI書込18回、画面窓 X0..159/Y24..103、25,600 B画素転送を含む。汎用画像の YouTube 側は実サーバを使わず、色補正を試験コードが事前適用した。
- 以前の「GPIO/SPI全trace完全一致」は、この限定したホスト入力とAPI呼出しtraceの意味でのみ成立する。stubはSDKの実SPI送信、GPIO波形、割込み・電源・初期化前処理・LCD可視表示を検証しない。YouTube実績ビルドのSDKは `/home/tane/pico/pico-sdk`、PicoMadoは `/home/tane/dev/PicoW/pico-sdk` という別リビジョンで、前者は `pico_cyw43_arch_lwip_threadsafe_background`、後者は `pico_cyw43_arch_lwip_poll` をリンクする。両ファームの実機動作が一致すると結論できない。

## LCD低速制御候補と固定画面の最小再現（未書込み）

- 過去自己診断の来歴調査を終了し、`lcd.c` の制御/初期化を1MHz要求、全画素転送だけ個体configの20MHz要求へ分けた。SPI rate変更は転送完了後のCS highで行う。窓・回転・色補正・GPIO順は維持。低速化が崩れを直すかは実機未確認。
- `diagnostics/lcd-repro` は同じ全25,600 Bの固定画面（白枠/RGB帯/文字）を毎回再生成し、Wi-Fi前6回、既存home設定で接続試行後500ms毎に再描画する。phaseで画面内容を変えず、UP/DOWNで制御と画像の速度だけ同時に1/5/20MHzへ変える。USBにclk_peri、SPI CR0/CR1/CPSR、SDK返却実baud、SPI戻りバイト数、payload CRCを記録。LCDの読戻しではない。サーバへ接続しない。
- 最小再現SDK2.1.1版は実働YouTubeの既存SDK/診断CYW43 driver/background方式を使用。SDK2.3.1版は既存PicoMado SDK/stock CYW43/background方式。後者との比較にはSDKとdriver両方の差がある。速度一因子比較は一つのUF2内で行う。background方式に合わせた再現用lwIP allocatorだけ変更し、本体poll用lwipoptsは変更しない。
- 個体チェック付き通常ファーム、SDK2.1.1最小再現、SDK2.3.1最小再現はいずれも最終クロスビルド成功。実機書込みなし。通常ファームのbssは193,264 B、最小再現はSDK2.1.1 90,412 B/SDK2.3.1 88,696 B。生成UF2はすべて権限600、Git除外。
- `tests/lcd_bus_test.c` を実lcd.c/core.cへリンクし、`cc -std=c11 -Wall -Wextra -Werror` で成功。制御1MHz/画素20MHz要求、CS idle時のrate切替、窓、全画素、戻り長を検査。固定画面を繰返し/1・5・20MHzで描いて全25,600 B一致、CRC `20ac31e9`。`lcd_transform_test.c` も成功。stubで電気波形/実表示は検証しない。
- 最終UF2 SHA256: `build/PicoMado-lcd1/PicoMado-lcd1.uf2`=`fa869b9d3a89d58512817860fc0961fa6402a30866bd3bf6b3cd4ae145f9de3f`、`build/lcd-repro-sdk211/picomado_lcd_repro.uf2`=`c735ef1270f7f792fab532164a643ec5d6be513d9401e3a861a7d6127f8068cb`、`build/lcd-repro-sdk231/picomado_lcd_repro.uf2`=`0a12c19a1dfa82b488569fd926aca74ea67e53ddf43e5c242051f6e88b550f2e`。
- 作業接続の断続によりSDK2.3.1初回のプロセス作成が失敗したことを確認してから再開。既存ビルドsessionは継続確認した。最終ビルドと検査は実コマンドで成功し、ホスト全体が切断していたとは判断しない。書込みは親の証拠レビューと新たなBOOTSEL指示まで保留。

## 通常プラグイン版20MHz要求の転送（2026-10-13）

- はんだ不良修理後、ユーザーは診断最高値20MHz要求で表示安定を確認。通常版は同じ制御/画像20MHz要求と窓指定〜画素完了までCS保持を採用し、診断画像/段階待ち/バッファ回収を除外。個体秘密configは前後バイト不変。40MHzと周辺クロック変更は導入していない。
- 親のレビューとユーザー02:34:26「はい書き込んで」を受領。`PicoMado-lcd1-plugin-20MHz.uf2` SHA256 `d1613b61561d825fd030654d1be326d5988ef3498adbef9ac22b411c33888a52`、単一BOOTSELのFlash ID `E661640843239D21`を再確認して一度だけload/verify/restart。load100%、verify100% OK、USBアプリ再列挙。生ログ `/tmp/picomado-normal-plugin-flash.log`。
- 02:36:51 UTC、既存サーバのdevice_hello incarnation3/session25。frame75の25,600B送信と受信ACK/描画ACKを別々に確認、frame96までACK一致。対象接続の受動観測でもversion1/kind5のheartbeat（同incarnation/session、applied revision1）を確認。サーバ設定/再起動なし。
- ユーザー02:40:30の写真2枚付き報告で天気/雨雲の通常画面表示成功。字形/拡大要望はサーバ担当へ引継ぎ。ファーム追加編集なし。起動USB速度行はttyACM0の権限エラーで取り逃し、今回実速度未取得（15.625MHzは計算上の見込み）。4ボタン全操作成功は今回ログだけから断定しない。
- 詳細と観測終了の制約は `/home/tane/Documents/Codex/2026-10-12/task-5/normal-plugin-20MHz-flash-verification.md`。USB読取りは正常終了。作業専用受動capture PID1031102はSIGINTがカーネルに拒否され残存、親へ終了を引継ぎ。


## 2026-10-13 チェックポイント：通常版の表示と接続時刻修正

- ユーザーは通常20MHz要求のプラグイン画像の実表示と、サーバ側で修正・拡大した日本語文字の読みやすさを確認した。20MHz要求、表示窓から25,600バイト完了までのCS保持、同速度の再設定なしを採用し、40MHz要求へ変更していない。
- 接続先の決定／取消しや待機復帰で処理時刻が進むため、ボタン処理後に単調時計を再取得してから接続期限を比較する。受信処理後も同様に再取得し、Welcome後の即heartbeatを防ぐ。古い時計から新しい開始時刻を引くunsigned差分が、AWAY決定直後にtimeoutとして扱われる再現を修正した。
- 実mainループを使う `tests/app_timing_test.c` は旧コードで期待失敗、新コードで成功。Wi-Fi／サーバtimeout境界、再試行、メニュー取消し、HOME選択、Welcome、待機復帰を検証。既存core、ws_accept、rx_flow、lcd_transform、normal_lcd20の5ホスト試験も通過した。
- 接続診断はHOME/AWAYの選択、SDK戻り値、Wi-Fi/TCPIP状態、IP取得の有無だけを記録する。SSID、password、認証情報、実IP値を出さない。個体の秘密Configは変更していない。
- 最終 `PicoMado-lcd1-switch-timing.uf2` のSHA256は `3e03b366d292295e58569ce20314b623499a578cc2a8e886e4179f135738afab`。03:28:05 UTCにFlash verify 100%、03:28:10 UTCにHelloを確認し、後続のframe158まで受信／描画ACKを確認した。UF2、実Config、board-id、ビルド出力と生ログはコミット対象外。
- Rustサーバの文字修正は、天気の47文字索引と232文字atlasの不一致を解消し、既存Noto Sans CJK JP Boldのface 0から原寸字形とmetricsを同時生成する。日付16px、地域12px、気温22px、雨雲出典12px。保存済み雨雲画像は配送時に文字だけ更新する。全国139地点、文字領域、欠損／負の気温、キャッシュ再描画、模擬端末を含む89テスト、fmt、clippyが通過した。

### 未解決・未検証（03:28 UTCチェックポイント時点）

- **HOME操作中のWebSocket不安定は未解決。** 修正版でもsessions29〜33の再接続が観測され、切断理由は未取得。表示成功と通信安定性を同一視しない。
- **AWAYは未接続。** 修正版の観察であり、USBの40秒読み取りは0バイトだったため、Wi-Fi認証／IP取得／SOCKS5／WebSocketのどの段階で失敗したか未区別。時刻差の修正だけでAWAY成功を確認したとは扱わない。
- E-ink実機、複数実機、BLK実機極性、外出経路の継続性、動画fpsと長期安定の受入は未完了。E-inkの2ボタン配線は要件として確定したが、実装・実機受入の完了を意味しない。

## 2026-10-13 続報：ACK修正とAWAYのLAN宛先接続

- ACK修正版Rustサーバを05:05:57 UTCに導入・再起動。95試験、fmt、clippyとreleaseビルドが通過。待受は既存の `192.168.0.120:16153` のまま。
- ファーム側はユーザー承認でAWAYのサーバIPだけを `192.168.0.120` へ変更し、SOCKS認証返信の段階別version判定、サーバ再試行時のWi-Fi維持、非秘密CDC診断を含む版を転送した。資格情報・LCD条件を変更していない。
- AWAYのMETHOD/AUTH/CONNECT成功とACTIVE/READYをCDCで確認し、05:16:30のHello/session35・画像217の両ACK、05:16:56のHello/session36・画像218、05:17:01の219、05:17:07の220の両ACKと照合した。実機LCDの全表示品質はこの通信ACKだけから判定しない。
- 本端末・本環境では既存subnet routingによりTailSocksからLAN宛先へ到達でき、追加listenerは不要だった。Tailnet参加だけで任意のLANへ到達できるとは扱わない。別環境は実際のTailSocksノードで経路利用・アクセス許可・対象待受を確認する。
- 05:13:09と05:13:21の旧receipt ACKは `ack_ignored` となり、session34で後続入力が処理された。これは遅延ACK修正の実機証拠。05:16:46のsession35終了はPower入力直後のread_errorで、ACK拒否／送信stallは記録なし。端末操作との因果はCDCを含めて評価する。
- HOME全操作中の再接続、長時間・回線切替・省電力後の復帰、動画帯域、複数実機、E-ink・BLKの受入は未完了。先の「AWAY未接続」は03:28チェックポイント時点の記録であり、後続成功で更新する。
- ユーザーはBLK接続後も電源OFF時にバックライトらしき発光が残ると報告した。ファーム担当は接続済みフラグが0でバックライト関数が即returnすることを確認した。修正はこの疎通checkpoint後に行い、バックライトOFFの実機受入は未完了。
- 時刻付き証拠と最新セッションの一回確認は[今回の接続検証](../experiments/tether-tailnet/results-2026-10-13-picomado-away-lan.md)に記録する。OS・ネットワーク・他サービス設定変更、追加listener、接続試験中のSSH、commit/pushは行っていない。
