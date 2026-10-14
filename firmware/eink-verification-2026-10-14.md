# E-ink firmware verification — 2026-10-14

E-ink_0は最終RAM転置を修正したe8409d06版へ再書込み・verify・再起動済み。mono1対応サーバも反映し、HOME／Hello／ReceiptAck／RenderedAck（frame817）を確認した。風刺画のみONを維持。E-ink_0の実表示成功はユーザー確認済み。初回のローカル表示崩れと修正証拠は以下に残す。

PicoMadoサーバのmono1対応は全回帰・Releaseビルド・clippy完了。初回の本番操作は自動承認レビューが明示許可不足として拒否。親からユーザーの本番反映承認が届き、拒否された同じ操作を一度だけ再試行して成功した。削除feature／既存実行フラグを維持。旧風刺画HTTPサービスと毎時タイマーはactiveを維持。SSH・OS／ネットワーク変更・追加バックアップなし。今回の変更はユーザー指示により検証・本番反映後にcommitし、pushしない。

## 表示崩れの証拠と修正

- 添付JPEG（Library ID `libfile_867a3faaeabc819195bf0671d68d3829`）をこの環境の `/home/tane/Documents/Codex/2026-10-14/task-3/eink-evidence/768.jpg` へ取得し、436614B・読取可能・実ピクセル閲覧を確認。中央に規則的な短い黒線、上下は白。画像だけで配線不良と判断しない。
- 初回USBログ `/tmp/picomado-E-ink_0-usb.txt` ではローカル描画の4000B SPI完了とACTIVE／READYを確認。サーバのCRC／Receipt／Renderedなし、APIもlast_received_frame／last_rendered_frameがnullなので、写真はローカル文字表示の崩れと判断。初回時点の本番サーバはLCD配信固定で、ファームがRGB565フレームを拒否していた。
- 同じ非対称3813B画像から旧 `ai_news.frame.png_to_wire` のW[x*16+u/8]（u=121-y）を構成し、旧 `pico/panel_v4.py` の実 `Panel.display` をmock SPIで実行した。初回C版のSPI出力とは4000B中3519Bが異なった。
- 旧 `pico/protocol.py` はB[x+(15-byte)*250]=W[x*16+byte]に変換し、Panel.displayがbyte面ごとに250列を送る。最終送信は `W[x*16+byte] for byte=0..15 then x=0..249`。初回C版はWの列順のまま送っており、この最終転置を欠いていた。
- 修正 `eink_transform.h` は直接RAM[(u/8)*250+x]へbitを置く。修正後は旧Panel.display実コードの4000Bすべてと一致。4種類の独立パターンの常設回帰も追加した。SPI速度・初期化・座標レジスタ・極性は変更していない。
- wireは250×122の連続MSB-first／白=1／3813B（末尾4bit白）。PNGの32B行幅をそのまま送る形式ではない。最終パネル送信は4000B、16 byte面×250列。最終面の各列で下位6bitを白に保つ。

## 配線・パネル・共通実装

2ボタンは [kernel.md](../docs/requirements/kernel.md) の上GP20（物理26番）／下GP21（物理27番）／共通GND物理28番、内蔵pull-up・押下LOW。上短押し前、下短押し次、上長押しSUB／決定、下長押し待機／取消／復帰。長押し閾値は押下ごとに固定、短押しと連続発火を抑止。0ボタンは同じmainから入力を無効にする。

旧V4の実表示成功は旧プロジェクトのpico/panel_v4.py、docs/PICO_TRANSFER.md、docs/OPERATIONS.mdで確認。[Waveshare公式GPIO](https://github.com/waveshareteam/Pico_ePaper_Code/blob/main/c/lib/Config/DEV_Config.c)と同じSPI1／4MHz、CS9・SCK10・MOSI11・DC8・RESET12・BUSY13を継承する。SCK／MOSIの実物導通を今回測定した事実とは区別する。V4全更新のみ、V3実機・fast／部分更新は未確認。

通信・状態・メニューは共通main/net/core、LCD／E-ink表示実装はCMakeで分離。表示は1 poll最大128B、BUSYをsleepループで待たず全更新に10秒期限。sleep後100msも非同期。待機では未開始の再描画を破棄し、物理更新終了後に画像を保持。旧世代・未描画・失敗フレームへRenderedAckを返さない。

HOME／AWAY等の10通信マクロはLCD個体と値を表示せず一致比較済み。両config.hは0600・Git除外。DHCPと別々の端末IDを維持。E-ink_1はモジュール未到着なのでCONNECTED=0で通信／ボタンを先行確認する構成。到着時のパネル世代を確認し、V4ならCONNECTED=1で再ビルドする。

## 完了した試験

- firmware/tests/run_host_tests.sh：修正後23実行PASS。LCD回帰、2／0入力、長押し、実mainのCRC→Receipt→非同期Rendered、失敗／世代抑止、待機の描画終了待ち、BUSY固着期限、パネル不在、byte-CS、旧SPI順序との4000B一致。
- cargo test --offline --all-targets：169成功（lib164、binary1、integration4）、実外部取得opt-inの1件はignored。
- cargo fmt、全target clippy -D warnings、git diff --check成功。
- cargo build --release --offline --bin picomado-server成功。標準featureでの検証用バイナリSHA256 `1b074f434963bf8af89e3293765f6d1111eccb9801783ceec275a67cba968c11`。既存稼働バイナリのSHA256 `3e681d9ee8a00c26acd64526f0ad01f1187a2bbdd034479b346fa92d3ce1a106`とは異なる。

## 現在の実機と修正成果物

E-ink_0のFlash IDはE6614864D3813227。本人確認後の単一BOOTSELで取得しGit除外board-id.txtに保存。BOOTSEL USB serial E0C9125B0D9Bとは別物。通常起動のUSB CDCは `/dev/serial/by-id/usb-Raspberry_Pi_Pico_E6614864D3813227-if00`。追加の個体識別機構は作っていない。

初回書込みのUF2 SHA256は `3de693dc5c6aee71cfbfee3d835c2e9fc4f34fcf28acf962ab3934d80c258c15`。load／verify100% OKとアプリ再起動を確認した。その後、再BOOTSELの本人確認を受け、以下の修正版を同じFlash IDに一度だけ再転送した。ログ `/tmp/picomado-E-ink_0-fixed-flash.txt` はload／verify100% OK、再起動、終了コード0。

| 端末 | 修正成果物 | UF2 bytes | ELF text | ELF bss | 転送 |
| --- | --- | ---: | ---: | ---: | --- |
| E-ink_0 | build/E-ink_0/E-ink_0.uf2 | 718336 | 365180 | 159136 | load／verify済み |
| E-ink_1 | build/.unbound/E-ink_1/E-ink_1.uf2 | 720896 | 366468 | 155228 | 未結合・禁止 |
| PicoMado-lcd1 | build/PicoMado-lcd1/PicoMado-lcd1.uf2 | 718848 | 365348 | 198680 | load／verify済み |

ELF dataは両方0。静的RAM量でありスタック等を含む実測最大値ではない。E-ink_1はCONNECTED=0で表示処理が最適化除去されるため、転置修正によるUF2差分はない。モジュール到着後のCONNECTED=1再ビルドは修正された共通変換を使用する。

```text
E-ink_0 e8409d06a6786467b11b7d1554334d8bbbc8aa8cd399436fb04e16e0e230131a
E-ink_1 6e03c4ecee2c0560df164f750ec19ab1079e45602add607134eb413401a87b8c
PicoMado-lcd1 ea8d2a3a9a98dc635117ac6819a2901281549bf874213a40896cee7bcde71a1b
```

## 本番反映後の証拠

- cargo install記録の既存feature `caricature-retention-delete` を確認し、同featureでRelease再ビルド。運用設定／サービスunitは変更せず、既存 `PICOMADO_CARICATURE_DELETE=1` を再起動後の実プロセスで確認した。
- 前段mono1対応の稼働サーバSHA256は `16d30233d2130837f7ae2724e474526f243a4e0d239733206741064bd794faf0`、ビルド元とインストール先 `/home/tane/bin/picomado-server` が一致。picomado.serviceはPID1462842／active／running。
- 17:25:48 UTC：E-ink_0 incarnation4／session3のHelloとWebSocket結合、frame817を3813B mono1として送信し、ReceiptAckをサーバ観測。
- 17:25:54 UTC：同frame817のRenderedAckをサーバ観測。APIもlast_received_frame=817／last_rendered_frame=817。USBログ `/tmp/picomado-E-ink_0-deployed-usb.txt` にはCRC3813B、Receipt投入成功、4000B SPI、Rendered投入成功、ACTIVE／READY。USB未接続時の診断欠落8件はカウントされているが、サーバ側の両ACK到着を確認した。
- LCDもオンラインへ再接続しframe818のReceipt／Renderedを確認。enabled4機能・selected=caricatureを維持。
- E-ink_0はenabled=[caricature]／selected=caricatureを維持。旧ai-news-http.serviceはactive running、ai-news-generate.timerはactive waiting。

## 残件

1. LCDの物理SUB押下からWeb変更で解除される操作全体の目視受入。局所Sub→Normal同期は実mainのホスト試験済み、更新LCDのWeb再配信は両ACK確認済み。E-ink_0は今回再書込み不要。
2. E-ink_1の実機Flash IDと到着モジュール世代を確認。現在は未結合・CONNECTED=0で未転送。
3. 2ボタン実機、AWAY実機、天気／雨雲の最終可読性・電流・長期運転は未確認。本番反映の承認ブロッカーは解消済み。

## 天気・Web保存の追加修正と復旧後の本番試験

天気の既存LCDパレット・形・位置を保持し、E-ink用では太陽・雲・降水線だけ黒へ置換する。晴れ／曇り／降水の4コードの画素・形・コントラストを試験し、比較PNGの実ピクセルも閲覧した。比較画像はtask-3/eink-evidence/weather-icon-comparison.png。窓の字形のみフォントへ追加し、既存1345字形の寸法・alphaは不変を確認した。

Webで選択中の機能をOFFにすると、共通順序の先頭へ再選択し、端末別閲覧位置を解放、Normalへ戻し、その接続へキャッシュ（なければ取得中）を即時送る。現在選択が有効なら表示世代・Sub・閲覧位置を維持。全OFFは共通フレーム経路の中央「PICO窓」。古い動画／取得／ACKは表示世代で拒否。オフライン保存とDB reopen後の再接続も試験した。

FrameStartへ後方互換の任意mode=normal/subを追加。共通Pico入力実装は現在世代・画像契約を検証したFrameStartでだけ局所sub_modeを同期する。LCDは更新が必要。E-ink_0は0ボタンなので前段ファームを維持。E-ink_1の未結合UF2には同じ同期修正を含む。

- サービス試験174成功（lib169、binary1、integration4）、実外部取得opt-in1件ignored。実HTTP→WebSocketのLCD／E-ink即時配信、CRC、mode、両ACK、同じセッション維持を含む。
- ファームの23実行PASS。実mainのLCD／E-ink FrameStartで局所SubがNormalへ戻る回帰を追加。ロゴ試験のclippy指摘は等価なas_chunksへ修正し、該当試験も再実行PASS。
- fmt・全target clippy -D warnings成功。Releaseはcaricature-retention-deleteを明示して成功。
- OS再起動後18:40:45 JSTに前段サーバが自動起動していた。LCDは通常USB／HOME／ACTIVE／READY、BOOTSEL不在。/tmpの中断時ログは消失し、新旧ビルド識別が無いので再BOOTSELが必要と報告した。OS設定には追加作業していない。
- 18:53:43 JST：今回の修正版を/home/tane/bin/picomado-serverへ原子的導入し、picomado.serviceを再起動。PID14966／active／running。Release元と稼働先のSHA256は3bb276ff2cc15647fbb73e20c66c7581d6ee536035e581dd19e7a3e19f1b06ee、15048288B。
- 再起動前後の実プロセスでPICOMADO_CARICATURE_DELETE=1を確認。unit／設定は変更せず、旧ai-news-http.serviceはactive running、ai-news-generate.timerはactive waiting。
- 現在のユーザー設定は両端末ともrain-radarのみON。実HTTP保存でweatherをON（選択維持）、rain-radarをOFF（天気へ即時）、weatherをOFF（ロゴへ即時）、rain-radarをON（元へ復元）を試験した。CASで並行変更を保護。
- E-ink_0：incarnation4／session10を維持し、天気frame889、全OFFロゴ890、復元雨雲891でReceiptAck／RenderedAck。各画像3813B、実更新約2.6秒。LCD：incarnation3／session128を維持し、天気892、ロゴ893、復元雨雲894で両ACK、各25600B。
- 試験後は両端末ともrain-radarのみON／選択rain-radar／online。公開メタデータ証跡はtask-3/eink-evidence/live-settings-acks.json。秘密config、UF2、private board-idはGit除外を維持。

## LCDモード同期版の実機書込み完了

ユーザーの再BOOTSEL連絡後、既存flash.shで単一対象Flash ID E661640843239D21を照合。準備済みPicoMado-lcd1 UF2（718848B、SHA256 ea8d2a3a9a98dc635117ac6819a2901281549bf874213a40896cee7bcde71a1b）をloadし、Flash verify100% OK、アプリ再起動、終了コード0を確認した。書込みログ最終時刻は2026-10-14 19:05:43 JST。/tmp/picomado-lcd-mode-flash.txtへ記録。

通常USBは同じE661640843239D21へ復帰。HOME／ACTIVE／READY／IP取得を確認。incarnation3／新session129へ再接続し、rain-radarのframe904でサーバ側ReceiptAck／RenderedAck、USB側CRC25600B／Receipt投入／Rendered投入を確認した。未接続中のUSB診断欠落4件はカウントされているが、サーバ側の両ACK到着を確認した。

更新後のLCDだけでWeb保存を再試験。weather ONで現在rain-radarを維持、rain-radar OFFで天気frame905、weather OFFで中央ロゴ906、rain-radar ONで復元frame907へ即時配信し、すべてReceiptAck／RenderedAckを確認した。session129を維持、最後はrain-radarのみON／選択rain-radar／online、enabled_revision40。証跡はtask-3/eink-evidence/lcd-postflash-settings-acks.json。物理SUBの押下とその後のWeb解除は今回は操作しておらず、実mainホスト試験の同期確認と区別する。

サーバはPID14966・18:53:43 JST起動の3bb276ff版のまま、追加ビルド／再起動なし。E-inkへの書込みや設定変更なし。実装コミット37c0e80に続き、この実機検証記録だけを関連コミットし、pushしない。
