# 採用済み通信とPico設定（2026-10-20）

初期設計後に採用した通信・設定を記録する。基準は本番 `ffb62d6`、診断差分は別候補 `c1304d2`。過去の「現在」「未転送」は各記録時点であり、最新実機状態の台帳ではない。今回、新しい実機採取・書込みは行っていない。

## 通信の現在の契約

| 境界 | 採用動作と適用範囲 | 根拠 |
| --- | --- | --- |
| 搬送・framing | Pico開始TCP、必要時SOCKS5、HTTP Upgrade後binary WS。アプリ1件はbig-endian length:u32、version:u8、kind:u8、body。WSとアプリ境界は一致不要。 | `service/src/protocol.rs::encode/Decoder`、`src/net.c` |
| 制御・画素 | 制御はJSON。`frame-bytes-v1`だけkind18の生画素。bodyはID長:u8、ID、incarnation/session/display/owner/frame:u64×5、offset:u32、画素。fragment4096B以下。旧JSON画像と制御の混在を扱う。 | `encode_frame_bytes`、`main.c::process_frame_bytes`、`control_reply_test.c` |
| 静止画・旧LCD・E-ink | FrameStart寸法・形式・長さ・CRC・tokenを検証。全量組立後ReceiptAck、物理処理完了後RenderedAck。送信、TCP受理、画像受信、描画を同一視しない。 | `core.c`、`main.c`、protocol/queue試験 |
| 新LCD動画 | `lcd`＋`frame-bytes-v1`＋`lcd-push-v1`が必要。FrameStartにfps/sequence/generation。描画ACKをcreditにせず、共有再生時計から最新を配送。静止画へ強制しない。 | `lcd_stream.rs`、`playback.rs`、`transport.rs`、`video.c` |
| 有界待ち | 送信中1枚は不変、未送信は最新1枚。Picoの組立・描画待ちを分離。WS buffer32KiB、SO_SNDBUF32KiB要求。現在はkernel-write進行がない3秒、動画画像全体10秒で接続終了し復旧。制御到着時は書込中の1メッセージを250msの猶予内で完了するか接続終了。静止画全体期限はwire長に応じた式で最大20秒。途中の残りを別画像へ差替えない。Linuxのbuffer読戻しは要求値と異なり得る。 | `lcd_stream.rs`、transport試験、[測定](lcd-push-verification-2026-10-15.md) |
| 取消し・世代 | 切替・standby・所有権・新sessionで旧画像/ACKを失効。送信済みの先頭がある画像は完送し、未送信の旧画像は破棄する（完送できなければ接続終了）。FrameStartが応答より先でも新世代を認め、同世代応答でchunks組立を壊さない。旧input_contextを捨てる。 | `control_reply_test.c`、transport試験 |
| 接続 | Helloをsocket受理順で検証。旧Hello、失敗Welcome、旧closeで新sessionを置換しない。 | `hello_order.rs`、`hello_order_matrix.rs` |
| 正常EOF | Close/FIN後、受信済みを有限drain。未完成画像を描かず、失効tokenの既描画画素は通常復旧中に保持。hard error・期限は別経路。 | `net.c`、`main.c`、EOF回帰 |
| 動画時計・統計 | `time_us_64()`を使いtrace OFFの0に期限を依存させない。1秒の任意StreamStatusは観測でありcreditではない。画像・統計を毎回DBへ書かない。 | `video_now_us/poll_video`、実main ON/OFF試験 |

LCDは160×80 RGB565BE/25600B、E-inkは250×122 mono1/4000B。SPI転置はドライバが担当し、内容はサーバのプラグインが所有する。サーバ汎用メニューは依然 [未実装提案](protocol-proposal.md)。

## 通常値・個体設定・実験設定の区別

| 種類 | 保存点の値・動作 | 適用範囲・留保 |
| --- | --- | --- |
| ネットワーク | `pico_cyw43_arch_lwip_threadsafe_background`。callbackのRXをSDK lock内でcopy/consumeし、描画前に解放。RX32768B、アプリworkspace18006B。 | ARM mapと動的高水位を別確認。 |
| Wi-Fi PM | join開始時`CYW43_NONE_PM`要求。join後に戻り値/readbackを記録。 | `start_wifi/check_wifi_pm`。要求を実設定成功とみなさない。 |
| LCD TCP | 新規PCBのconnect前にNagle無効。LCDのHOME/AWAY/再接続のみ、E-inkは維持。 | `net.c`、[NODELAY検証](nagle-candidate-verification-2026-10-19.md)。旧正常版でもNagle有効。これだけで間欠遅延の原因・解消としない。 |
| LCD SPI | 通常20MHz要求、制御と画素同速、窓指定から25600B完了までCS保持。125MHzでは実15.625MHzになり得るのでSDK読戻しを使う。40MHzは未採用。 | GPIO、offset24、回転・色・gammaを変更しない。 |
| 入力・個体 | LCD4、E-ink0/2ボタン。config.h/board-idのbindingと秘密はGit外。通常main stack4096B。 | LCDピン・35ms安定判定、E-ink SPI1/4MHz等はREADME/プリセットが正本。長押し閾値は端末設定。 |
| 通常ログ | trace ON、LCD stats ON/verbose OFF。stdio stdout/deadlock timeoutは両方0。固定RAM、1回1record・部分write。 | SDK既定500ms待ちをこのビルドへ当てはめない。処理時間0の保証でもない。 |
| 診断候補 | AP2/SIM/TX/CYW43_INNER/STREAMは既定OFF。私有LL生成時に元SDK SHAを検査し共有SDKを編集しない。 | 候補のLCD BSS245136B/静的heap余裕11768Bはそのvariantの値。保存済みPicoMado-lcd1-ALL-OFFもSTREAM_DIAGNOSTICSとAWAY_EEX1はONで、AP2/SIM/TX/CYのみOFF。名前で全診断OFFと判断せず実CMakeCache/compile commandとhashを照合する。今後の変更へ値を流用しない。 |
| DTR | 立上りで統計snapshot。既存AP2未開始gate、detached SIM COMMITは下降で開始、再接続で残り取消し。 | closeは抜線・USB IRQ消失ではない。継続観測とは別の実験契約。 |

資格情報、SSID、秘匿URL、board IDの内容は転記しない。端末選択・有効化・共通順序・個体設定を文書整理で変えない。

## 性能と検証の限界

10-15の方式変更は平均fpsと最大停止が改善した一方p95悪化も記録した。別区間の12fpsは約12fps・置換0・切断0だが、動画・経路・測定点が異なる区間を同条件比較しない。10-19 NODELAYは実lwIPで小送信の待ち条件を確認した修正で、全遅延の説明ではない。正常な短い採取も間欠遅延の不存在を示さない。

実USB負荷・IRQ時間、実heap/stack高水位、長時間安定、RF・物理パネルは未検証範囲。protocol、動画、応答順、pending ACK、旧Hello、EOF、NODELAY、仮想入力のDTR/run/digest、SDK出自の試験対応は [回帰表](../docs/contracts-observation-design.md#回帰と証拠の対応) に記す。

### 後続の保存済み動画測定

`nagle-fix-candidate/latest-live-result.json` の `derived180_windows.regulated_device_180` を再確認した。同一session/generationの端末時計180秒区間では描画数の標本境界から11.861111〜11.994444fpsの上下界、1秒以上の間隔0、最大間隔は既存累積値による上界461.778ms。厳密な180秒枚数や区間内最大値は復元できない。全232.089973秒区間は2447描画・10.543325fpsで、再接続に伴う8.210821秒の間隔1回を含む。定常区間は再接続を除くため元の採取窓と同じ比較ではなく、厳密なA/BでもNODELAY単独の因果証明でもない。動画の改善と、原因未特定の間欠切替遅延を分けて扱う。

現在の期限は `transport.rs` の `FRAGMENT_STALL_TIMEOUT=3s`、`STREAM_FRAME_TIMEOUT=10s`、`SEND_TIMEOUT=250ms` を照合した。10-15記録の2s/5sは当時値として保持する。Wi-Fiのrequest/readbackは `app_timing_test.c`、EEX1の世代内cacheとWi-Fi世代失効は `endpoint_client_test.c` と `app_eex_test.c`、旧frame/旧応答・ACK混在は `control_reply_test.c` が回帰の根拠となる。
