# PicoMado 切替遅延・上りパケット欠落の調査記録（2026-10-22）

対象は LCD のボタン通知・heartbeat・描画報告に共通する Pico→Pi 通信と、LCD/E-ink の画像配信。現行の固定ソースは 72603910a97ce9012c2b87ec24c92507ebf6034a、サーバは ACK 非依存版 SHA256 a48052af6f99a42ee277b7aa27926cea4c1a6a1a8b9784389de18a406f963604。この文書は調査と実機適用の記録であり、パケット欠落の根因を修正済みとは認定しない。

## 現時点の判断

- 正の証拠として、同一 LCD 接続の上り TCP 区間 [12073,12172)（99 B）が欠落し、後続の同長 heartbeat は Pi に到着しても累積 ACK 12073 から進まず、約45秒後に欠落区間が届くと ACK 12469 と4件の heartbeat のアプリ到着が続いた。Pico の TX/CYW43 bus 記録と Pi pcap を対応させた事実。CYW43 の bus 戻り値0は無線送信成功を保証せず、欠落場所は chip、AP、経路、Pi受理のどこか未確定。これは mode 4（GET raw 0x84）の session 2355 であり、後の mode 3 試行と同一イベントではない。[同一接続解析](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/LCD-ONE-CONNECTION-CAPTURE-20261022/analysis.md)
- 先行する別の baseline では約1.497秒の切替遅延と TCP 欠落 [8064,8163) 99 B の約1.344秒後の再送を確認した。欠落区間は heartbeat と推定されたが、初回送信は観測窓外で、その喪失位置は特定していない。[当時の引渡し](/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/heartbeat-capture-20261021/baseline-proof-20261021T063055Z/ON-HOLD-HANDOFF.json)
- ACK 非依存サーバは YouTube で先に成功した有界 latest 配信を通常 LCD と E-ink へ広げ、描画報告待ちが次画像の送信許可を止める経路を除いた。送信中1枚と未送信最新1枚を維持し、receipt/rendered 報告は観測情報として受ける。実機でも Pi 入力受信→画像送信は速くなったが、ボタン通知そのものの上り停滞は残った。[契約と適用結果](../changes/LCD-ACKLESS-20261021.md)、[PDCA](../development-process.md#pdca-youtube-ack)
- 最新統合 LCD の HOME 試行では、USB正式操作の dispatched 返信→Pi入力処理が10.127秒、20.718秒。ユーザーは HOME で最大16秒を体感し、同じファームでテザリングに切り替えると快適と報告した。これは環境条件差を示すが、AWAYは Wi-Fi だけでなく hotspot、SOCKS proxy、経路も変える。以前のテザリングでも遅延があったというユーザーの観察があり、L13単体、Wi-Fi単独、端末実装のいずれかを排他的な根因とはしない。[最新 LCD 結果](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/demo-400ms-flash-stage/LCD-INTEGRATED-RESULT.json)、[テザ時サーバ結果](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/demo-400ms-flash-stage/EINK1-AND-TETHER-RESULT.json)

## 変更と検証範囲

| 項目 | 実施した内容 | 効果・限界 |
| --- | --- | --- |
| サーバ送信許可 | YouTube と同じ ACK 非依存・有界 latest を通常 LCD/E-ink に適用。公開順と E-ink 時計分キーの競合を修正。 | 画像ごとの rendered ACK 待ちは解消。最初の版で LCD 公開遅延13.88秒と E-ink 時計連続送信を検出し一時 rollback、修正後に再適用。上り TCP 欠落は残る。 |
| 送信経路 | LCD は既存の Wi-Fi PM NONE、TCP_NODELAY（Nagle OFF）を維持。公式 CYW43 PR155 の SPI command 長だけ私有 overlay へ適用し、SDK原本は固定。 | PR155 は 99 B TCP 相当の SDPCM 171 B に対する命令長172→171 B を直すが、同条件で未達が再現し、根因と認定しない。[SPI 長の比較](../changes/CYW43-SPI-LENGTH-20261021.md) |
| AWAY 接続 | 製品の Endpoint Exchange を必須経路へ戻し、毎回の Wi-Fi 世代で現在の proxy endpoint を取得する契約に復旧。 | 固定された古い proxy IP への接続失敗を是正。テザリング時の低遅延を EEX 単独の効果とはしない。[EEX設計](../changes/EEX1-RESTORE-20261022.md) |
| 無線比較 | GET raw 0x84 を mode 4 + AUTO_ACTIVE状態bit と復号し、GET観測値とSET許容値の型を分離。比較用mode 3/roaming OFFを導入し、現行版ではSTA有効化後の初回 link UPで一度ずつSETと別GET。 | 3台の起動ログと別USB GETでraw 3/1の候補一致を確認。SDKはCDC status/応答長を公開せず、RF設定の完全成功は未証明。mode 3・roaming OFF でも LCD 遅延は再発。[読戻し境界](../changes/INTERFERENCE-READBACK-20261022.md)、[起動既定](../changes/DEMO-RADIO-DEFAULTS-20261022.md) |
| 物理入力 | 35 ms 超のサンプリング空白で全ボタンを取消す回帰を発見。on_press の直接操作は元の押下FSMへ戻し、長押しでSUB/POWERへ変換する入力のみ連続性判定を保持。 | 旧モデル20件失敗→新モデル20件成功、通常32件と偽SUB/POWER12件も独立確認。実事象の個々のGPIO空白は測れておらず、この回帰だけが全無反応の唯一原因とは断定しない。[入力修正](../changes/INPUT-CADENCE-20261022.md) |
| 表示案内 | 通常/SUB共通の「切替中」案内を200→400 ms、最新操作のPico単調時刻起点に変更。 | 399999/400000/400001 µs境界のhost試験済み。視認頻度と光学的な表示時刻は未測定。[400 ms仕様](../changes/SWITCH-FEEDBACK-400MS-20261022.md) |

送信APIの成功と到達は別の事実である。正式USB操作の status=dispatched は同一handler内で tcp_write(COPY) が ERR_OK となり tcp_output を呼んだ後に返る。ただし出力結果はその bool に反映せず、CYW43受理・RF送信・TCP ACK・Pi ingressを証明しない。最新 HOME の10.127/20.718秒は、この返信からPiアプリ入力処理までの区間。AP2/CYはこの2操作でARMされず、個別SEQ・lastack・RTOは保存されていない。Pi入力処理→画像送信は3.723/4.016 ms。画像送信→rendered報告到着4.458/4.477秒も、実描画時刻とは同一視しない。[実装境界](../../firmware/src/net.c)、[保存結果](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/demo-400ms-flash-stage/LCD-INTEGRATED-RESULT.json)

## 実機の固定版とデモ条件

最終統合ソースは 72603910a97ce9012c2b87ec24c92507ebf6034a。各候補は個体ID/設定ファイルSHA、UF2全範囲と設定予約sector 0x101ff000 非重複、独立レビューを照合し、正規入口から各1回だけ書込み・Flash verify 100% OK。起動後に自動mode 3/roaming OFFのSET戻り0・別GET raw 3/1を確認した。CDC statusは未検証。Piサーバは上記ACK非依存SHAのまま。

| 端末 / Flash ID | 適用UF2 SHA256 | 起動後の確認 |
| --- | --- | --- |
| LCD / E661640843239D21 | 9c97fdb7390cf1f998a0bf0ad0a74fab627d7782bb69c8e7d108865c3465ccd6 | boot 4395c33ada81770a、元画像の受信・rendered報告、USB有限切替後も復帰 |
| E-ink_1 / E6614864D34F8E22 | 662c77419d08dfcd389bbcbaa62e621ef51622aab4a4b13f96ddb5b9209806fe | boot 8dc818c156d9b7b6、session 113、weather frame 1214068 の受信・rendered報告 |
| E-ink_0 / E6614864D3813227 | f041f895332457f9b327ef03b91c99f148069c278d6a64eb15898baec2e78a9c | boot 3733e37bcf5ff09、session 86、caricature frame 1218937 の受信・rendered報告 |

[3台の実機handoff](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/demo-400ms-flash-stage/FINAL-THREE-DEVICE-HANDOFF.json)に個体別の書込み・起動証拠SHAと07:18 UTC時点の3台onlineを保存した。rendered ACK は端末報告がPiへ届いた事実であり、目視確認ではない。現在のデモはこの固定版とユーザーが快適と確認したテザリングを用い、別の対策を同時投入しない。

テザリング側の新しいLCD session 2360ではサーバに72件の入力が到着した。通常画像63件のサーバ送信→rendered報告到着は中央値242.7 ms、最大616.5 ms。押下→Pi入力の時刻は採れていないため、この数値を体感切替時間と呼ばない。サーバjournalだけではAWAY設定値やEEX取得の実行も証明できない。

## 調査手順の失敗とPDCA

YouTubeの動画は、既に描画ACKを送信許可から外して滑らかになっていた。通常画像にも同じ待ち構造がある差を初期の比較軸に置かず、欠落場所の観測を先行させたため、ユーザーの既存解決策を未検証のまま引き継いだ。後にACK非依存化の効果を確認しつつ、上り欠落という別の未解決区間を残した。[元のPDCA](../development-process.md#pdca-youtube-ack)。今後は正常な類似経路との差、送信許可と実配送、症状の発見箇所と根因候補を別々の責務・事実として比較する。

同一接続のhost pcapは対象LCDソケットに限定したが、承認90秒に対し実際390.050秒記録した。sudo timeout -s INT -k 5 90s tcpdump ... -Z tane の timeout は137で戻った一方、権限降格後のtcpdump PID 9781は残存。先行の無害な timeout sleep 試験はそのUID遷移を再現しなかった。停止権限がなく、ユーザーの sudo kill -INT 9781 で終了。終了後はPID/FD残留なしを読み取り確認したが、最終kernel drop統計は未保存。この起動方式を再利用しない。pcapでの正の到着/累積ACK事実は有効だが、drop不明を「0」と扱わない。[捕捉解析と停止経緯](/home/tane/Documents/Codex/2026-10-21/task/latency-eex1-restore-20261022/LCD-ONE-CONNECTION-CAPTURE-20261022/analysis.md)。今後もし採取を再開するなら、同じ権限・同じprocess群での停止保証と時間・容量上限を先に確認し、承認範囲を守る。

根因の次の識別点は、同じsessionの正常送信と未達送信で、PicoのTCP COPY/output、CYW43 chip受理、Pi packet ingress、Pi readerを対応させる境界。現保存値だけでSDK idle bus、RF、AP、L13、受信側taskのどれかへ昇格しない。デモ後にHOMEとテザリングを同じファーム・操作・負荷で比較し、hotspot/proxy/経路差を明記する。Webトップでの端末別案内ms調整、RFのCDC status取得、目視表示時刻と物理押下時刻の検証も後日の別件とする。
