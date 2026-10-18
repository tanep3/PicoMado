# 時計とV4 E-ink更新（2026-10-18）

## 調査と設計根拠

変更前の `src/display_eink.c` はV4通常全面更新のみ。毎画像リセット、RAM `0x24`、更新制御 `0xf7`、BUSY完了、deep sleepを行う。サーバだけでは毎分表示と10分全面更新を両立できない。

次の両実装を確認した。

- [Waveshare公式 EPD_2in13_V4.c](https://raw.githubusercontent.com/waveshareteam/e-Paper/master/RaspberryPi_JetsonNano/c/lib/e-Paper/EPD_2in13_V4.c)：`Init`、`Display_Base`、`Display_Partial`、`Sleep`を含む全体。baseは同じ画像を `0x24` と `0x26` の両RAMへ書き `0xf7`、partialは短いハードリセット後にborder `0x80`、RAM `0x24`、更新制御 `0xff`。部分更新にSWRESETやV3 LUTは使わない。
- `/home/tane/plutopg/Raspberry_Pi_PICO/picoW/e-paper/e-paper_monitor/pico/e_paper.py` の `EPD_2in13_V4_Landscape`：既存の横向きRAM順序・data entry `0x07`・byte-CSと同じV4 base／partialの組合せ。既存通常描画の実績はあるが、部分更新の実機成功記録とは扱わない。

公式Web製品／wikiの取得は403／robots制限で完了せず、部分更新時間・連続更新回数の数値を保証する資料は今回取得できなかった。動画のfpsやパネル寿命について推測の数値は採用しない。V4 sourceで確認できる更新手順とBUSY契約だけを使う。

## 実装

新能力 `eink-clock-v1` に対してのみ、時計画像のFrameStartへ `eink_refresh` を付加する。標準10分、端末別1〜60分。間隔と画像のUnix分を同じフレーム世代で渡す。既存の共通端末設定/NVM形式は変更せず、設定の正本はサーバSQLite、次回の時計画像から反映する。

Picoは検証済み画像・CRC・現世代の受付後だけ時計経路へ入る。初回・別表示世代はbaseを作る。時計連続表示中はdeep sleepを行わず、V4コントローラの基準RAMを維持する。指定分間隔に達すると再初期化し、両RAMへ現在画像を書いて全面更新する。エラー・待機・復帰・ローカル画面・通常プラグインはbaseを失効させる。部分更新のSPIも1 pollあたり128byteの上限を保ち、BUSY待ち中はボタン／通信をブロックしない。

非時計の通常フレームは従来の全面更新とsleep。LCDの受信・描画・速度・ACK経路は変更しない。旧サーバの新ファーム向け画像は通常全面更新、新サーバの旧ファーム向け画像は追加フィールドなしで毎分全面更新になる。Webには旧ファームの未対応を明記する。

## YouTubeへ適用する場合

現行再生workerは16ms周期で共有再生位置を観測するが、E-inkの次画像はRenderedAck後にだけ受け付ける。描画待ちの動画画像を蓄積せず、ACK後の最新位置へ飛ぶ。Pico側はBUSY完了後だけRenderedAckを返す。

このclock-only部分更新経路を動画へ一般化することは可能。ただしYouTubeが明示的に選択する能力・端末別設定、全面更新間隔（時間に加えて更新回数上限の要否）、高コントラスト／動きの大きい素材での残像・温度・BUSY実測が必要。今回は動画に部分更新を許可せず、動画仕様・fps設定・ACKを変更していない。BUSY無視や未検証波形は導入しない。

## 検証結果と本番反映

検証結果とPNGの位置は [時計検証記録](../service/clock-verification-2026-10-18.md) に記す。サンプル個体のUF2はコンパイル確認専用で書込み禁止。本番UF2は対象端末・その設定とFlash IDを照合し、BOOTSEL確認後に明示指示を受けてビルド・転送する。

初期検証時点では本番ファイル置換・サービス再起動・ファーム書込み・コミット・pushを行っていなかった。後段の実施状況を以下に追記する。時計画像そのものはサーバ更新だけで利用可能。フラッシュ抑制にはE-inkの新ファームが必要。実機での残像、部分／全面更新の実時間、10分以上の連続運転、接続復帰と画面切替の受入は未実施。

## V4 sequence license

Portions of the V4 initialization/base/partial command sequence are derived from the Waveshare team reference linked above, under its following license:

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## その後の本番対応

ユーザーの対象指定とBOOTSEL完了を受け、E-ink_0だけを実config／binding付き本番UF2で更新した。唯一の接続Flash ID `E6614864D3813227` を照合し、load/verify/起動、同IDのUSB再認識、ACTIVE/READY、サーバへの `eink-clock-v1` 申告とRenderedAckを確認した。続いてユーザーの「E-ink_1 Bootselしました」を受け、E-ink_1も実config／唯一の接続Flash ID `E6614864D34F8E22` を照合しload/verify/起動した。同IDのUSB再認識、新能力申告、ACTIVE/READYと描画ACKを確認した。LCDには書き込んでいない。

本番サーバも保持削除機能と実環境値を維持して `14:43:27 JST` に更新・再起動した。更新直前の選択／ON/OFFは保持し、時計への強制切替は行っていない。部分更新の残像・フラッシュ間隔と物理表示はユーザー目視待ち。バイナリSHA、PID、最小DB退避と実機配送の留保は[本番検証記録](../service/clock-verification-2026-10-18.md)を参照する。

E-ink_1とLCDはその後のユーザー操作で時計を表示している。E-ink_0は14:46:24に描画ACK待ち切れ1回、その後自動再接続と14:46:41の描画完了・heartbeat継続を確認した。原因は未確定で、全期間を異常なしとは扱わない。時計の10分全面更新・残像の目視受入は引き続きユーザー確認事項。
