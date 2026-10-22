# SDK整合版の先行TCP hole追解析

[実機結果](sdk-aligned-live-result-20261021.md)の同一run `18e0701e35d08e10` を、保存済み証拠と固定sourceだけで追解析した。新規USB/実機/サービス/ネットワーク操作0、品質停止条件の変更0。証拠root Rは `/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/SDK-live-20261021T040833Z`。

**確認できたTCP holeはSEQ[8064,8163)の99B。受信message順と固定符号化からheartbeat message13に対応するとの推定が得られた。** 先行データの初回送信TX/CY行は今回の窓に存在せず、その時刻も未記録。最終TXのCY欠測1件とは別である。通知がPiに届いても先行holeで順序待ちになることと、初回packetが失われた場所がunknownであることを区別する。

## メッセージとSEQの対応

host connection431/session2299はmessage13～19を以下の順でdecodeした。Pico connection1、boot `5ae570e360f434a5` は同一で世代変化なし。PicoのTX_POSITIONが通知message15の[8262,8333)を直接固定する。

| message | kind/内容 | JSON / WS本体 / TCP内WS長B | SEQ半開区間 | 対応の根拠 |
| ---: | --- | --- | --- | --- |
| 13 | 5 heartbeat | 87 / 93 / 99 | [8064,8163) | 順序・固定符号化長から逆算 |
| 14 | 5 heartbeat | 87 / 93 / 99 | [8163,8262) | 同上 |
| 15 | 15 UP切替通知 | 59 / 65 / 71 | [8262,8333) | Pico records[7]直接 |
| 16 | 5 heartbeat | 87 / 93 / 99 | [8333,8432) | Pico records[17]直接 |
| 17 | 8 ReceiptAck frame1021500 | 169 / 175 / 183 | [8432,8615) | Pico records[28]直接 |
| 18 | 9 RenderedAck同frame | 170 / 176 / 184 | [8615,8799) | Pico records[41]直接 |
| 19 | 5 heartbeat | 87 / 93 / 99 | [8799,8898) | Pico records[64]直接 |

host.jsonのdecoded records[6,12,18,26,38,44,56]とdecode_beginの長さを照合した。heartbeat13/14/16のJSON fingerprintはすべて4187173491、入力15は2177506005。raw_ready368B = 99+99+71+99 = 8432−8064、次のraw_ready367B = 183+184とも一致する。

固定LCD source `SDK-align-20261021T032923Z/source/firmware/src/net.c:568–584` は1 app messageを1 masked binary WS frameへ格納し、JSON+6Bの独自headerにWS header/mask6B（本体126B以上なら8B）を付加する。main.c:599–605はheartbeatをkind5で送る。ack_probe.c:86–97はmessage番号とCOPY位置を記録する。host側transport.rs:1105–1166はBinaryをdecodeしmessage順序を進め、ack_probe.rs:355–370はkind/JSON長/CRCを保存する。Pong等のcontrolはこのconnectionで受理して継続する経路ではなく、非対応として終了する。

13/14には採取前のPico COPY/初回TX行もraw byte→WS offsetの直接記録もないため、この2件のkind対応は**固定コードとstream順序からの推定**であり、packet payloadの復号・直接照合とは呼ばない。最初のTXはすでにnrtx4だが、これはPCB共有counterで、heartbeat13や通知の正確な初回送信時刻・再送回数を与えない。

## TX・CY・Piの相関時系列

TX時刻はPico共通入力event24を0とするµs、Pi時刻は通知IPID46のkernel timestampを0とするms。**両列の原点・時計は異なり、列間差分を計算しない。** 同一packetは4-tuple/SEQ/end/ACK/IPID/len、TX内CY入退出時刻で対応させた。

| TX index | 内容 / IPID | Pico相対µs | CY CRC entry=prebus / coveredB | linkoutput / bus戻り値 | Pi相対ms |
| ---: | --- | ---: | --- | --- | ---: |
| 採取前 | heartbeat13初回/過去再送 | unknown | 未採取 | unknown | 元packetは未採取 |
| 0 | 通知15 / 46 | 1098 | 3701199909 / 129 | 0 / 0 | 0 |
| 1 | heartbeat16 / 47 | 1337959 | 3286584721 / 157 | 0 / 0 | 1336.660117 |
| 2 | 先行heartbeat13の範囲を再送 / 48 | 1344478 | 3513366763 / 157 | 0 / 0 | 1344.373226 |
| 3 | ReceiptAck17初回 / 61 | 1476515 | 3817693979 / 241 | 0 / 0 | 観測なし |
| 4 | RenderedAck18初回 / 62 | 1497592 | 1910414416 / 242 | 0 / 0 | 1497.882795 |
| 5 | ReceiptAck17再送 / 63 | 2755189 | 4093287630 / 241 | 0 / 0 | 2754.959082 |
| 6 | RenderedAck18再送 / 64 | 2755808 | 1015216354 / 242 | 0 / 0 | 2756.140859 |
| 7 | heartbeat19初回 / 65 | 16341930 | 248841040 / 157 | 0 / 0 | 観測なし |
| 8 | heartbeat19再送 / 66 | 17756104 | **容量超過で未保存** | 0 / unknown | 観測なし |
| Pico窓外 | heartbeat19範囲の後続再送 / 67 | 未採取 | 未採取 | unknown | 20755.386891 |

各保存CY rowはflags25（FULL/BUS/DONE）、covered=domain、entry/prebus key一致、CRC一致、credit待ち0/poll0/RX0、bus戻り値0。bus戻り値は生成patchが `cyw43_write_bytes` の直前に記録し、その返り値をfinishへ渡したもの（候補tools/prepare_cyw43_inner.py:35–37）。CRC対象はBDC+Ethernet/IP/TCP全域でありSDPCM header/paddingを含まない。異なる再送packet間のCRCはIPID等が違うため同じである必要はなく、同じ1回の呼出し内entry→prebusを比較した。

CRC一致は指紋照合であって数学的なbyte同一性の証明ではない。成功戻りはチップのRF送信完了/802.11 ACK/AP受信を保証しない。保存された通知・再送のdriver内待ちや観測対象領域の書換えは検出されなかったが、未採取の初回先行packetへこの結果を転用しない。

## Pi側out-of-order・累積ACK・SACK

headers.jsonlは40件、kernel drop0、同一tupleのみ。IPID46到着（1行目）の42.074µs後、PiはACK8064を返した（2行目）。IPID47が+1336.660117msに届いてもACK8064（3–4行目）。通知と後続heartbeatが先行holeを越えて到着していた。

IPID48が[8064,8163)を+1344.373226msに埋め、49.241µs後に累積ACK8432（5–6行目）。この到着で[8163,8432)も連続化できたことが確認できる。[8163,8262)がPiのTCP受信側に揃った正確な時刻は採取前/未観測。先行heartbeat13の範囲が埋まる前の通知到着から1,344,373.226µsが、今回の主要なTCP順序待ちに対応する。

後続のReceiptAck17初回IPID61はPi観測にない一方、RenderedAck18のIPID62は到着している。+1497.941258msでもACK8432。IPID63が穴を埋めると+2755.030544msにACK8799へ進んだ（31–36行目）。したがってACK17と18がともに初回欠落したとはしない。描画完了はPico側で入力+1496540µsに確定しており、hostでRenderedAckをdecodeした時刻を実際の画面完了と取り違えない。

observerはIHL+20Bだけをkernelからcopyし、TCP optionを保存しない。ただし全40件の固定header中data offsetが20Bだったため、**今回保存したpacketにはoption自体がなくSACKも0**と判定できる。SACK blockを復元したとはしない。SYNは未採取で交渉の直接記録なし。`LWIP_TCP_SACK_OUT`既定0（整合SDK opt.h:1325–1329、appでoverrideなし）とも整合するが、今回の結論はheader長と累積ACKに基づく。

## 欠測1件・除外できる範囲

CY容量8に対するover1はTX8/IPID66/heartbeat19の再送[8799,8898)、入力+17.756104秒であり、描画完了後の別件。先行heartbeat13の初回は**トリガ前で採取対象外**、診断drop1で失ったものではない。先行再送IPID48にはCY row2がある。全20秒CY区間は引き続きincompleteで、保存8分類をcleanへ変更しない。別runとの同時因果合成なし。

今回の保存事実で以下を切り分けられる。

- 新しい切替通知がPico appのSDK lock/COPY/linkoutputで1.344秒止まった説明は合わない。入力→send return1812µs、通知IPID46はPiへ到達済み。`tcp_output`成功を配送成功とした判断ではない。
- 1.344秒を「Pi appに順番どおり読み出せる通知を放置した時間」とする説明は合わない。累積ACKは先行holeを示す。通知decode→処理完了は別のhost時計で62119µs、描画そのものはPico時計で19248µs。
- 今回の画面遅延全体をplugin生成/描画のみに帰すことはできない。旧SDK組合せだけが必要条件という説明も、新整合SDKでの再発と合わない。
- **除外できない**のは採取前の先行packetについてのapp/SDK/driver待ち、buffer問題、bus以後のチップ/RF/AP/経路の欠落、ACK経路の問題、計測影響。保存された後続再送の正常なCRC/戻り値だけで初回や全区間を正常とはしない。packet filter外/観測点以前もunknownで、drop0を全ネットワークの無欠測保証へ拡張しない。

## 必要最小の次観測案（未実行）

必要な追加事実は、**最初に未確認となった先行heartbeatのCOPY→初回linkoutput→CY entry/prebus/finishと、同じkeyのPi到着/ACK**。現在の「入力後だけ採取」では初回が過去となり、8行CY全体品質を失って次試行も止まる。この2点を解決せず同じ試験を反復しても初回の欠落場所は分離できない。

まずfirmwareを変えない案は、対象tupleのヘッダのみを入力前から有限時間採取し、通常15秒heartbeatの少なくとも2周期を含めて先行packetとACK停滞開始を捉えること。単なる35秒程度の待機は全状況の初回包含を保証しないので、開始時点の未確認範囲と保持上限を記録し、当該初回が窓外ならunknownのままにする。既存40秒observer上限に前後を黙って詰め込まず、必要な有限保持時間・byte上限・終了条件を先に契約化する。この案でも初回のCY境界は取れず、RF/driverの最終分離には不足する。

初回driver境界まで分離する最小追加は、全量ログではなく、対象tupleの先行未確認segmentの初回key・timestamp・CRC/covered・credit待ち・bus戻り値を**入力前から固定RAMで保持**し、入力時にその履歴と後続を同一runでfreezeできること。これを必要観測仕様として検討し、保持数・容量溢れ・世代変化・partial/FROZEN所有・observer effectを設計レビューする。既存20秒全区間品質を短縮/緩和したり、未検証容量WIPを混入したりする案ではない。実装・書込み・再採取は今回実行していない。

R/analysis/sequence_followup.pyが原証拠から相関表と35照合を生成する。結果はR/analysis/sequence-followup/results.json、validation.json。35成功は解析の内部整合性であり、欠測を含む元CY全区間の完全性成功を意味しない。初回の未観測点はこの解析の限界として確定する。
