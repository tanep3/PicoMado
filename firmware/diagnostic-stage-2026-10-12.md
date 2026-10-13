# LCD 段階診断版（書込み前）

対象は `PicoMado-lcd1`、Flash ID `E661640843239D21`、現在の個体専用 UF2 SHA256 `fbcac607fb55776e1a9077f8c552c4aa66f71f1962c221af6740930a2b292f29`。診断フラグはGit除外・権限600の端末 `config.h` に `PM_DIAGNOSTIC_STAGE=1` として追加。`0` へ戻すと段階表示・ボタン単体期間・USB診断行を無効化できる。旧 `PM_DIAGNOSTIC_BOOT=0` を維持し、サーバ画像ごとの一瞬の四色帯は出さない。秘密情報をログへ出さない。

## 実機で見える予定の順序

1. Pico SDK の `cyw43_arch_init()` と STA有効化のあと、実働 YouTube と同じ順にLCDを初期化する。白枠＋横方向の赤／緑／青三帯を **6秒**表示。これは実働 YouTube `show_test_pattern()` と GPIO操作・コマンド・SPI送信25,600 Bが同一になるよう、補正なしの画素を180度回転して送る。USB行 `diag stage=boot_pattern duration_ms=6000`。
2. `BUTTON / PRESS BUTTON` を **25秒**表示。この間はWi-Fiサーバへ接続しない。GP12 SUB、GP13 UP、GP14 POWER、GP15 DOWNの押下を35ms安定判定で検出し、LCDに `BUTTON / SUB|UP|POWER|DOWN` を表示、USBには `diag local_button=0|1|2|3`。ボタンはサーバ処理と無関係で、最後の名前は次の押下または25秒終了まで残る。USB行 `diag stage=local_buttons duration_ms=25000`。
3. Wi-Fi接続を開始。USB行 `diag stage=wifi_connect`。リンク成立後、同じ白枠＋三帯を **6秒**再表示し、引き続きボタンをローカル確認できる。USB行 `diag stage=wifi_up_pattern duration_ms=6000`。この6秒はサーバ接続timeoutへ算入しない。
4. サーバへ接続、通常のローカル接続画面→`PICO MADO / WAIT IMAGE`→サーバ画像へ移る。USBは `lcd local: ...`、`diag stage=welcome session=...`、`diag frame_spi_complete=...`、`diag heartbeat_sent`。通常動作中のボタンは `diag button=<0..3> state=<数字>` としてサーバ処理の成否とは独立にUSBへ記録する。ただしその後の画像・ACK・視認表示は別々に判定する。

最初の三帯が黒ならWi-Fi接続やサーバ画像より前のLCD初期化/電気経路、最初は見えて二度目が黒ならWi-Fiリンク前後、両三帯は見えて画像だけ黒なら画像経路/表示状態を優先。USB SPI完了は目視表示の証明ではない。ユーザーの写真・ボタン確認が必要。

## 構造とホスト検証

`display.h`/`display.c` を通信・状態遷移からの唯一の表示入口にし、`lcd.c`/`lcd.h` がLCD/ST7735S専用の物理処理を所有。`display_eink.c` は未実装を明示しており、端末configでE-inkを選ぶとビルド時に拒否する。将来のE-ink実装は独立の初期化・更新・画像保持・sleep/wakeと、サーバ側画素契約が必要。`main.c` はパネルGPIOやSPIコマンドを直接扱わない。

本番個体向けReleaseビルド、ダミーsample-a/bビルド成功。core、LCD変換、TCP受信フロー、WebSocket Acceptのホスト4試験成功。実働YouTubeの`lcd_display.h`と現`lcd.c`をGPIO/SPIモックで直接実行した初期化＋通常画像traceは一致（SHA256 `4196e917e183dfce8fd3cc2e0e309f957133bd7ada92f3be25dc124f70238e48`）。今回の三帯も実働YouTubeの生成式・LCD送信経路とtrace完全一致（SHA256 `c823e132cfe3054f1a21586f26c85bd81df10c051840c7682dd639766a99f44b`）。E-ink選択のホストコンパイルは明示的な未実装エラーで失敗。

最終ELF `.bss=193,256 B`、4,096 Bスタック（scratch Y `0x20041000..0x20042000`）。`-fstack-usage` は `main` 624 B、診断ループ48 B、`pm_net_poll`40 B、`pm_net_send`72 Bで、既存スタック削減を維持。個体 `config.h` とUF2はともに権限600・Git除外。YouTube復旧UF2のSHA256は `65900f5224c64efdde49e9652d035ad34416fb532212f92740396825b70eaa1c`。ユーザーBOOTSEL待機は単一 `2e8a:0003` RP2として再確認済み。書込み前に `flash.sh --check-only` でFlash ID/manifestを再照合する。
