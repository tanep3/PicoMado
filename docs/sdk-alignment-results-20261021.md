# SDK整合化とLCD比較候補（2026-10-21）

[比較契約](sdk-alignment-comparison-contract.md)の設計コミットf4474f6を先行し、/home/tane/dev/PicoW/pico-sdkの本体2.3.1 /079c6f39023649b154152db30f1d781e884879bcを維持してsubmoduleを公式gitlinkへ揃えた。[公式SDKのlib tree](https://github.com/raspberrypi/pico-sdk/tree/2.3.1/lib)を固定commitのGitHub tree APIでも照合・保存した。全repo clean、recursive6件すべて指定に一致。SDKフォルダ変更は以後の他projectのbuildにも使用されるが、既存Pico/サーバbinaryは更新していない。

| submodule | 更新前 | 整合後 |
| --- | --- | --- |
| CYW43 | c1075d4bc440422cf2b2fd12c64a1f53f77660ee | 055d64274b014dd7b1c2fc94d26e8a18face7124 |
| lwIP | 0a0452b2c39bdd91e252aef045c115f88f6ca773 (2.2.0) | 77dcd25a72509eb83f72b033d219b1d40cd8eb95 (2.2.1) |
| btstack | 501e6d2b86e6c92bfb9c390bcf55709938e25ac1 | eb0bb8b5ea6d234ccb940313b47f7a5c3b4e20ec |
| mbedtls | 5a764e5555c64337ed17444410269ff21cb617b1 (2.28.8) | 0bebf8b8c7f07abe3571ded48a11aa907a1ffb20 (3.6.6) |
| tinyusb | 86ad6e56c1700e85f1c5678607a762cfe3aa2f47 | 同じ |
| mbedtls/framework | 旧親版にはなし | dff9da04438d712f7647fd995bc90fadd0c0e2ce（新親gitlink、公式HTTPSで取得） |

## 退避・容量WIPの分離

SDK-align-20261021T032923Z/SDK-before.tarにSDK全体/.git/submodule metadata/ignoredを12563 memberとして退避、SHA256 4e3e39f8bc98f7a646258a39d1b33611fafd2bf6936b43eefcf33748b4ed3f35。before.jsonに各旧HEAD/branch/status、backup-ready.jsonにarchive/旧private LL/diff等のSHA。旧tracked/untracked SDK source変更はなく、復旧は非forceの旧SHA checkoutと別dirへのarchive展開照合を契約に記載した。reset/clean/SSHなし。

容量対策の途中6ファイルはcapacity-WIP.patch / capacity-WIP-files.tar / capacity-WIP.jsonへ退避し、元workにも保持。まだcompile/test前で、今回の候補に含めていない。検証238項目にはWIP全6fileの元hash不変確認も含む。

## 固定sourceと計測patch

比較用sourceは書込み済みf2bdc7b2e17cc620e5d568c16842ee6ea871e7b0からgit archive。個別config.h / board-id.txtは前回LCD buildとSHA一致。両variantのsrc C/H・lwipopts・CMakeは当該固定commitのまま。STA lock/flash疑い、buffer修復、TCP bool/policy、容量serializerは変更しない。

新CYW43原本LL SHA7f4c93755b2ab911d18606652497b2f3cd08085ce63b3b1836171d375f806b10へ診断生成器の厳密pinとCAPS markerを適合した。send_common/send_ethernetの関数本文は旧CYW43と同一で、元11個の置換箇所は全て一意。shared SDKへpatchは適用せず、build内private LLを生成。app C/Hは無変更。差分は生成器、collector期待marker、2つのhost fixture marker、実SDK関数抽出testへ新しいupstreamのhost-wake極性定数blockをそのまま含める計5ファイル。全diffをdiagnostic-port.jsonに保存した。

## 成果物・ARM比較

rootはSDK-align-20261021T032923Z。candidateはartifacts/aligned-observed/PicoMado-lcd1.uf2、参照はartifacts/aligned-official-LL/PicoMado-lcd1.uf2。両方とも別source/build dirで成功した。既存CMakeはsource/firmware/build/PicoMado-lcd1を厳密要求するため、variantごとに同じsourceを配置してこの制約を維持した。

| 項目 | 書込み済み旧組合せ | 整合SDK・既存計測維持 | 整合SDK・公式LL参照 |
| --- | ---: | ---: | ---: |
| UF2 SHA256 | fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3a | 11a11556e6e5c71b2f90fb98aeabca15132aabb5914b27d6ccebd217d7a5c485 | c5e2f3e6a42b76c0dc83dea5707f231f3adaa241604f7188f22ead70a9ae425d |
| static main RAM | 251064B | 251048B | 249760B |
| linker heap可能 | 11080B | 11096B | 12384B |
| core0 stack予約 | 4096B | 4096B | 4096B |

計測維持候補はtext+88B、BSS−16B。heap8192B/stack4096B下限を維持。StackTop−StackBottomが予約stack、StackLimitとの差8192Bはscratch全域であり予約stackではない。__HeapBaseはexportされず、heap余地はHeapLimit−__end__で算定した。初期build scriptの暫定symbol集計をこの正式検証で訂正している。linker余地は実行時free heap/IRQ込みcall-chain stack highwaterを保証しない。

計測維持候補はEVENT_STREAM/STREAM_DIAGNOSTICS/AP2/SIM/TX/CYW43_INNER/AWAY_EEX1等のflagsを旧版どおり保持し、threadsafe_background/stack/pin/bindingも照合した。公式LL参照だけはCYW43_INNER_PROBE=OFFでSDK LLをそのままcompileし、その他flagsは同じ。全アプリ無計測の参照ではない。両UF2のRP2040 family/flash範囲と設定sector非書込みを確認。

## 試験結果と限界

- 新SDK実関数18シナリオをnative/ASan/UBSanでON/OFF比較し、bus bytes/呼出順/戻り値/pbuf意味一致。
- 既存host suite（278 PASS行）、USB/TX suite（11 PASS行＋28 Python tests）、CYW suite（13 Python tests等）、collector27tests、採取境界5tests（40新規PTY＋192事実/ACK/byte組合せ）、USB trial16testsが成功。
- source/flags/private設定/LL出自/UF2/heap/stack/SDK clean/WIP分離の238照合成功。詳細はvalidation.json、build-results.json、resource-comparison.json、各host log。

最初の2回のCMake configureは既存buildパス制約でcompile前に停止し、source/flagsを変えず配置を補正した。sandbox内collectorはIPC生成EPERMで開始不能だったため、実USBを使わない新規PTY限定testを許可環境で実行して成功。境界suiteの1件はfirmwareだけのarchiveに検証文書が欠けたことによるFileNotFoundで、同じ固定commitの文書を補って該当methodを再実行し成功。これらの失敗log/補正記録も保持し、SDK機能不良と混同しない。

実機書込み/USB open・コマンド/service操作/ai-news/E-ink変更は0。実機の間欠遅延・RF・IRQ/動的heap/stack効果は未検証で、SDK整合化を原因修正完了とは呼ばない。

## 次の実機手順（未実行）

対象はLCD PicoMado-lcd1、Flash ID E661640843239D21のみ。親が候補と操作範囲を提示した後、BOOTSEL状態で個体・UF2 SHA・個別設定/bindingを再照合し、既存flash.shと同等のpicotool手順で1回load/verify/rebootする。書込み/verify結果不明なら重複実行せず調べる。続いて同個体USB再認識・通常サーバ再接続/受信ACK/描画ACKを確認する。

次の比較はEV1 OFFから開始し、単一USB ownerと新source marker 7f4c9375対応collectorを使う。USB SIMと物理buttonの入口差を明記し、速い対照と遅い例を同条件で比較する。SDK変更だけではserver更新は不要。実機書込み/採取は今回のfolder更新・build範囲では行わない。

直前に戻すfirmwareは現在書込み済みのfd19…（gap-p2-build/artifacts/PicoMado-lcd1/PicoMado-lcd1.uf2）。f0c6a15a…はさらに前の履歴で、今回候補の直前rollbackと取り違えない。SDK自身はbefore.jsonの旧組合せへ戻す。DB/configの巻き戻しはしない。

[機械可読checkpoint](checkpoints/20261021/sdk-alignment-candidates.json)、root/ready.jsonに候補・復旧先・未実施範囲を固定した。
