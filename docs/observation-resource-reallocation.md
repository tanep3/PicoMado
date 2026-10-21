# 観測queueの資源再配分比較（設計のみ、2026-10-21・レビュー補正）

[容量契約](observation-video-pressure-contract.md)のdisjoint配置はheap7384Bで下限8192Bへ808B不足する。下限・burst B50/R600・source必須境界・64bit値・control予約16を黙って変更しない。実装は保留し、次の配置を比較する。

| 案 | RAMの概算 | 判断 |
| --- | ---: | --- |
| O84/C16/T4を独立配置 | 増分3696B、heap7384B | 808B不足で不合格 |
| O72へ縮小 | 元のheap余裕へ近づく | B50＋600×50ms＝80のbacklogに足りず不合格 |
| control16の予約を減らす | 1枠148Bずつ回収 | 既存control契約を弱めるため採らない |
| AP2/TX/CY compile OFF | 複数KiB回収可能 | 今回必要な内部診断を失うため採らない |
| timestamp/seq等を固定幅で切詰める | 4–8B/record回収 | 完全性/元値を失うので不可 |
| lossless差分/dictionary＋full escape | 未評価 | 64bit復元・wrap・長期欠測・escape最悪容量の証明が必要。複雑性が高く第1候補にしない |
| **AP2記録ringとEV1 O記録を排他的に重ねる** | 概算1520B減、heap12600B | 境界を削らない候補。ただし診断同時利用制約を明示しレビュー必須 |

## 排他overlay候補の収支

現在のtrace recordは160B×32＝5120B。既存AP2 recordは56B×96＝5376B（ack_probe.hの全fieldを保持）。旧対象合計10496B。

候補はunion max(O84×68＝5712B, AP2 96×56＝5376B)＝5712B、C16×148＝2368B、T4×160＝640B、新管理/counter上限256Bの計8976B。旧比1520B減、既存heap可能11080Bから計算上12600B。計算は他のbuffer/flagを保持した仮定で、実ARM sizeof/map/stack/動的heapの検証前に合格扱いしない。

CY8/TX32の記録、AP2 summary/boot/run/last_run/digest/control buffer、EV1 summary/loss/metrics、単一serializerのpartialはunion外へ保持し、失ったrecordを隠さない。APP framebuffer/SDK packet bufferは流用しない。数値payloadを圧縮/切捨てせず、通常のsource事実を減らさない。

## 所有権と保持

メモリ所有者をNONE/AP2/EV1の排他状態とする。AP2のARM/PREPARED/ARMED/RECORDING/FROZENはAP2所有。FROZEN記録はdigest照合済みRELEASEまで所有を離さず、EV1 ONで上書きしない。

EV1所有を取得できるのはAP2 IDLEかつretained/read中でない場合だけ。EV1 OFF後もqueued O record/partial/end watermarkを全部排出するまでEV1所有を保持する。AP2 ARMはEV1 active/pending中にbusyで拒否し、逆も明示busyで拒否。片方を自動freeze/release/resetしない。default診断OFF・通常アプリ動作は維持する。

FROZEN/READのheader＋16行＋end（18行）生成中、C queue内、serializer partial内のいずれかにそのrunの未完了出力がある間は所有を渡さない。busy返信用C枠を先に予約し、元nonce/runに対応する確定busyを返す。無応答をbusy成功としない。別runのAP2とEV1を同時観測した因果証拠として合成しない。

所有権移譲は既存lock順を維持したcold処理だけで、IRQ producer/SDK callbackが旧bufferへ書かないことを証明してから短い原子区間で確定する。IRQ内のformat、malloc、待機、追加IOは不可。boot/run/digest、上書き/欠測、pending exportの情報を別所有者へ付け替えない。

**tradeoffはAP2とEV1の同時採取を明示的に禁止すること**。今回の内部診断はEV1 OFFなので両方の機能を順番に保持できるが、既存のあらゆる併用形態をそのまま維持する案ではない。将来同時採取を必須とするならこの案を棄却し、lossless差分/既存重複bufferの別再配分を再検討する。診断制約を非表示にしたまま資源が足りたとしない。

## 実装前の試験条件

- AP2 PREPARED/RECORDING/FROZEN/READ中のEV1 ON拒否、EV1有効/partial/end排出中のAP2 ARM拒否、IDLEへの正規戻り、同run再送と未知run拒否。
- IRQ producerを移譲前後へ挿入し、旧ringへの後書込み/二重所有/retained data破壊がないこと。source seq/全境界/64bit値/CRC/digestを元実装と比較。
- 規定の到着envelopeを超えるburstでは既定loss/unknownを維持。O84/C16/T4の全service/partial/再同期/返信待ち条件を弱めない。AP2 A≤1＋40t（25ms/行）、他control U≤9＋40t、C合算10＋80t、backlog O80/C14/T3、保守的合算65,500B/sへ補正。元80KB/s・50ms service条件と実RAMは未実証gateのまま。
- AP2/TX/CYをcompile ONのまま全既存host回帰・ARM map/stack・動的heap/stackを検証。独立レビューでこの排他tradeoffを採用するまでruntime実装/flashを行わない。
