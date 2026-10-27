> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# Wi-Fi省電力の起動経路修正と検証（2026-10-18）

現在の方針：省電力設定漏れ修正を維持し、旧版復帰案はユーザー指示で撤回済み。過去の対照UF2は転送しない。以後の計測は [固定RAM計測記録](runtime-stats-verification-2026-10-18.md) を参照。

## 修正前の確定事項

通常起動のmainはCYW43→LCDの初期化順を守るためSTAを先に有効化しwifi_enabled=trueにしていた。一方、start_wifiのCYW43_NONE_PM要求はif(!wifi_enabled)内にあり、初回接続で実行されなかった。時計追加前HEADと現在のソースのホストprobeで、初回join直前の要求0回、STA-offからの対照は1回と再現。SDKのCYW43_DEFAULT_PMはPERFORMANCE（PM2）でありNONEではない。

15:21:49〜15:22:14 JSTのLCD USBと同時刻サーバログでは、frame296760のstart→CRCに3,016,400us、SPIは33,191us。途中も毎秒のtcp_write/output処理は走っていたが通知は届かず、15:21:55.955の約2秒write期限、LCD側peer_closed、5秒再接続の順を確認した。SPI111件の中央値15,598us。受信窓が開いたままの通信停滞であり、単純なRXゼロ窓deadlockとは判断しない。省電力不具合の寄与・無線停止の根本原因・改善はまだ実機未確認。

## 変更

- STA有効化済みでも、Wi-Fiの初回接続・再join・待機復帰でNONE要求を行う。CYW43→LCDの初期化順は維持。
- Wi-Fi接続成功時にSDKロック内でcyw43_wifi_get_pmを1回呼ぶ。サーバだけの再接続・動画ループ・heartbeatでは呼ばない。
- 要求と読戻しを別々に記録する。modeはraw値の下位4bitで、none=1は読戻し成功かつmode=0のときだけ。SDK戻り値だけで実モードを断定しない。ローカルSDKのlow-level getter自体は個々のIOCTL失敗を十分伝搬しないため、読戻し値も通信の健全性の証明にはしない。
- 読戻し失敗ではraw/mode/noneを出さず、エラーコードだけを記録する。失敗や不一致で無限再試行・新しい切断は起こさない。
- 後からUSBを開いた直後と既存15秒snapshotは保存結果を再掲する。追加の無線IOCTLは行わない。SSID/password/token/受信JSONは記録しない。

例：

```text
net wifi_pm requested=none sdk_result=0
net wifi_pm request_sdk_result=0 readback_sdk_result=0 raw=0x00000010 mode=0 none=1
```

接続中の無線が起動経路によらず意図どおりawakeになる変更。消費電流は未測定。共通ソースなので将来この版をビルドすればE-inkにも適用されるが、今回の実機転送対象はLCDのみ。

## 自動検証

作業コピーの `bash firmware/tests/run_host_tests.sh`：31ケースPASS。既存28ケースにpm-set-error、pm-read-error、pm-read-mismatchの3ケースを追加。最初のjoin直前にも必ず要求されること、サーバだけのretryは設定/読戻しを増やさないこと、Wi-Fi再joinは読戻しが更新されることを実mainループで確認。失敗や不一致をログで区別し、遅いUSBの再掲では読戻しが増えないことも検証。LCD/E-ink、USB満杯、trace無効、時計部分更新、CRC/入力/切替の既存回帰も成功。

実リポジトリでも31ケースPASS、git diff --check成功。LCD実configの通常ビルド成功（manifest OFF、pico_w）。UF2 726528B、SHA256 `44565624d3640edad6ad994b06eb3d375c9796671056a6df416e6753db1ac382`、Flash ID `0123456789ABCDE0`。config.hとboard-id.txtはビルド前後のSHA一致。実機には未書込み。16:32 JST時点で通常USB /dev/ttyACM0 を同個体serialで確認し、BOOTSEL列挙なし。本番サーバはPID907828・active・NRestarts0で維持。新たなBOOTSEL連絡と単一ボード一致を待つ。

## 実機比較の手順と未確認事項

1. LCD用の通常UF2を `tools/build.sh --device example-lcd` で実configから作成。manifest OFF・端末名・Flash ID 0123456789ABCDE0・SHA256・pico_wを照合する。sampleやE-inkのUF2は転送しない。
2. 新しいBOOTSEL連絡と単一の対象ボードを確認してからflash.shでload/verifyする。現段階では未書込み。通常USBは個体serialで照合する。
3. 同じ動画・6fps設定・端末選択のまま、最低3分のUSBと同時刻journalを比較する。初回要求と読戻し、stream_write_stall件数/接続断、frame_start→CRC、SPI、firmware elapsedの描画数を確認。設定や選択を勝手に変えない。
4. 操作を伴う比較はユーザーの押下時刻とUSB button→server input→replyを対応づける。自動ログだけを実機目視やボタンの合格としない。

比較用のUSB+既存journal採取スクリプトをtask領域に用意する。Python自身の有限期限・finallyでUSBを閉じ、journalは終了後に期限付きの単発取得とする。tcpdumpやjournal follow等の常駐子プロセスは起動しない。

## 観測プロセスの後始末

ユーザーがsudo kill -INT 914036を実行し、914035/914036のpsがヘッダーのみであることを確認した。こちらでも両PID不在を確認。終了問題は解決。新しいパケット観測は開始していない。


## 書込み後3分比較（悪化、未解決）

# LCD省電力修正後の実機比較：悪化・未解決

指定UF2（44565624d3640edad6ad994b06eb3d375c9796671056a6df416e6753db1ac382）をLCD個体 0123456789ABCDE0 と照合し、load/Flash verify100% OK、通常USB起動を確認した。USBでは要求SDK結果0、読戻しSDK結果0、raw=0x00000010、mode=0、none=1を確認。しかし通信停止は解消せず、ユーザーも「悪化してるがな。」と報告した。改善したとは扱わない。

## 最初の180秒の結果

| 指標 | 書換え前180秒 | 書換え後180秒 |
| --- | ---: | ---: |
| stream_write_stall | 2 | 6 |
| control_write_failed | 0 | 2 |
| Hello再接続記録 | 1 | 8 |
| 端末stream_status件数 | 120 | 22 |
| 端末報告から確認できた描画増分 | 591 | 70 |
| ホストsocket送信完了記録（到達保証ではない） | 655 | 144 |

前窓は16:32:21.609〜16:35:21.609 JST。後窓は16:36:43.718〜16:39:43.723 JST、180.005秒。後窓中のUSBは58,765Bで取得終了・FD close、journal単発取得もexit0で終了。常駐観測子プロセスなし。

同じ動画dFf4AgBNR1E、変換key9a8a64a8baa0b7e745d7dd41、6fps、epoch33を後窓の前後snapshotで確認。前窓の送信も全657 startが6fps。LCDのyoutube選択、enabled_revision72、サーバPID907828/NRestarts0を維持し、こちらから変更していない。

比較の条件差：前180秒はUSB readerが動いていない。後はUSB CDCを開いて診断を読み取り、ファームを再起動した直後。前の実PM値・当時の電波状態は未測定。したがって「書換え後の悪化」は示すが、PM方針、起動、読戻し診断、USB採取のどれが寄与したかはこの１組で分離できない。過去15:21の25秒USBありの旧版記録でも111回のSPIを確認していたが、時刻・電波条件が異なるので厳密な対照とはしない。

## 追加処理と既存処理の切り分け

1. 追加したPM設定はWi-Fi join開始前に1回。読戻しはjoin成功後に1回。Wi-Fi/IPを保持したサーバ再接続では再実行しない。3分USBには追加のrequested=noneイベントがなく、mode=0 none=1は12回の既存15秒snapshotが保存値を再掲したもの。ソースとホスト試験で無線IOCTLの繰り返しを除外しているが、実機の専用実行カウンタ・初回IOCTL所要時間は計測していない。設定されたPM方針自体は接続中ずっと持続する。
2. ホストは非ブロッキングUSB FDをselectで待ち、端末へデータ送信やbaud変更をしない。ファームのtrace_usb.cは既存の固定リング・空き量までのCDC書き込みで、追加差分には含まれない。PMが増やすログは1回の要求、1回の読戻し、15秒に1行の保存値だけ。USB有無のCPU/IRQ負荷が完全に同じとは未確認であり、満杯回帰PASSだけで実USBの影響を否定しない。
3. USBの診断欠落累計は159のまま。slow_loop18件は最大65,191us。動画SPI132件は中央値15,609.5us、最大32,048us。frame_start→CRC143件は中央値25,176us、最大3,002,992us。秒単位の滞留はSPIより受信完成前にあり、既存tcp_write/output戻り値は観測範囲で全て0。それでも上り通知は遅れ、tcp_peer_closed4回、TCP reset（SDK -14）3回。
4. 新版では画像転送に入る前のSESSION/Welcome周辺でも失敗している。サーバ2秒send期限と端末10秒SESSION期限、Wi-Fi/IPを保った5秒/10秒retryが重なり、接続待ちを長くしている。根拠なくdeadlineを延ばす追加修正はしない。

## 復帰候補の確認

保存UF2は以下にあった。

`$HOME/dev/PicoW/PicoMado/firmware/build/.rollback/5224c30/example-lcd.uf2`

720,384B、SHA256 `c5c9fc741c94cc77fbdde036548fc17e905a9637d0313d4d29692f5db2ab1d56`。picotool info -aでpicomado_lcd/pico_wを確認し、UF2内部の端末名・個体IDも一致した。ただしこれは過去の保存版で、frame-bytes-v1はあるがlcd-push-v1がない。書換え直前に実機が広告していたlcd-push-v1と異なるため、これを「直前版そのもの」としてそのまま戻すのは適切でない。書き込みは行っていない。

直前のmain.cは `task/picomado-clock/firmware/src/main.c` に保存され、SHA256 `56b4ed8bbd45d6753666182adcb00b53f4077e86b3bc967f3bdb557e80f2e3e1` が今回PM修正直前の記録と一致している。安全な復帰候補は、この保存ソースと変更していない実configから通常LCD UF2を再生成し、lcd-push-v1を保ったまま今回のPM/読戻し追加だけを戻すもの。まだその転送用UF2は生成していない。

提案：新しい修正を積む前に、まず直前ソースからの復帰候補を作り、対象と新しいBOOTSELを確認して戻し、USBあり/なしの比較を分ける。ユーザー操作で悪化を確認した状況なので、さらに別の設定を推測で変えるより復帰を優先する。ただし、古い保存UF2への無断転送、現在ファームへの追加修正、SSH、サーバ再起動、他端末への転送は行っていない。

## 証拠

- pm-fix/lcd-pm-flash.log、lcd-pm-flash-result.json
- pm-fix/post-flash-180s/{usb.jsonl,journal.jsonl,metadata.json}
- pm-fix/pre-pm-180s-journal.json
- pm-fix/lcd-pm-comparison-summary.json
- pm-fix/rollback-candidate.json、local-uf2-inventory.json


## USBなし後窓と復帰候補の準備

# LCD PM変更後のUSBなし比較と復帰候補

## 判定

USBを閉じた後の180秒では、大幅な描画低下は続いていない。端末報告で確認できた描画増分は575で、変更前591に近い。USBありの変更直後は70だった。一方、USBなしでもstream_write_stall4件、control_write_failed1件が残り、変更前のstream_write_stall2件より接続断は多い。改善・解決とはしない。USB採取が原因だと断定することもできない。

| 同じ180秒の記録 | 変更前・USBなし | 変更後・USBあり | 変更後・USBなし |
| --- | ---: | ---: | ---: |
| stream_write_stall | 2 | 6 | 4 |
| control_write_failed | 0 | 2 | 1 |
| Hello記録 | 1 | 8 | 7 |
| 端末stream_status | 120 | 22 | 128 |
| 端末報告の描画増分 | 591 | 70 | 575 |
| ホストsocket送信完了記録 | 655 | 144 | 674 |

描画増分は範囲内で受け取れた端末カウンタから算出した下限であり、実際の全表示枚数そのものではない。socket完了も相手到達保証ではない。USBなし窓には旧sessionのsession_not_current終了が別に1件あり、独立した通信故障として上の表へ加算していない。

変更前16:32:21.609〜16:35:21.609 JST、変更後USBあり16:36:43.718〜16:39:43.723 JST、変更後USBなし16:39:43.723〜16:42:43.723 JST。いずれも6fps。USBなし後窓の678回の送信start全て6fps、status全128件がgeneration33。後からGETした選択はyoutube・enabled_revision72、動画dFf4AgBNR1E・変換key9a8a64a8baa0b7e745d7dd41・epoch33のまま。USBは再度開かず、fuserにも利用者なし。観測窓は既に終了してから復帰ビルドを始めており、その55秒のビルド負荷は表へ混ぜていない。

同じ動画でも電波・AP/TCP経路・起動からの経過時間は完全に統制されていない。時間経過による回復とUSB有無の寄与が交絡するため、PM修正だけ、あるいはUSBだけが原因とはまだ言えない。前後の端末選択、サーバ設定、他端末は変更していない。

## 既存証拠からの原因仮説

- 確認済みの現象は、フレーム受信完成前やSESSION/Welcome周辺でTCPの進行が断続的に止まること。動画のSPIは数十msで、受信完成には約3秒の空白がある。画像描画だけを原因とする説明では、画像転送前の失敗を説明できない。
- 省電力無効は読戻しで確認済み。設定とgetterはjoin前後に各1回、15秒ログは保存値のみで、サーバ再接続のたびにIOCTLしているわけではない。継続するPM方針の影響は別問題であり、処理頻度が少ないだけで無関係とはしない。
- USB CDC・TinyUSBとCYW43背景処理/lwIPの相互作用は候補。ただし今回USBありからなしへ移る間に時刻・起動後の経過時間も変わっている。ホストのUSB readは非ブロッキング、ファームの既存loggerは固定リングと空き分だけのwriteで、実機のIRQ負荷は未測定。USBが無関係とも原因とも確定できない。
- サーバの2秒送信期限が通信停滞を切断へ変え、その後5秒/10秒の接続待ちが画面のSERVER CONNECTINGを長くする経路は実ログと一致する。期限延長や別の無線設定を推測で追加していない。

## 復帰UF2を別treeで生成

古い .rollback/5224c30 のUF2は使用していない。保存してあった変更直前ソースをSHA照合し、現在の実configを保って、task領域の別source/buildで通常LCD版を新しく生成した。現在リポジトリの全tracked firmwareファイルと個体config/board-idは生成前後でSHA一致。

UF2：`$HOME/Documents/Codex/2026-10-18/task/pm-fix/rollback-source/firmware/build/example-lcd/example-lcd.uf2`

- SHA256：`7cab7fe026a963b6865178a09ca162e9bebf7dac0611629d6cdf19ba6e32578a`
- サイズ：724,480B
- manifest：OFF / example-lcd / 0123456789ABCDE0
- picotool info：picomado_lcd / pico_w
- lcd-push-v1、frame-bytes-v1、端末名、個体IDの埋込みを確認
- 変更前main.c SHA256：56b4ed8bbd45d6753666182adcb00b53f4077e86b3bc967f3bdb557e80f2e3e1
- 実機用runtime sourceと現在sourceの差はfirmware/src/main.cのみ。今回追加したPM設定位置修正とreadback/保存値表示を取り除く差分をrollback-main.diffへ保存
- 変更前側のホスト回帰28ケースPASS。build成功。実config/board-id不変
- 未書込み。実機は引き続き44565624版

この対照版は省電力要求スキップを再導入するため、ユーザー指示で復帰案を撤回した。上記は生成当時の履歴であり、転送手順ではない。現在の実機は省電力修正を維持した92fd51e4計測版へ更新済み。追加結果は固定RAM計測記録を参照する。

## 証拠

pm-fix/no-usb-state-check.json、post-pm-no-usb-180s-journal.json、no-usb-comparison-summary.json、rollback-host-tests.log、rollback-real-build.log、rollback-ready-result.json、rollback-main.diff。SSH、追加tcpdump、サーバ再起動、設定変更、再書込みは行っていない。
