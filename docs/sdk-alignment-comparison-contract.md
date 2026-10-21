# SDK 2.3.1 submodule整合化と比較契約（2026-10-21）

ユーザーの「SDKフォルダ内を適切な組合せへupdateして試す」指示に基づく。SDK本体は2.3.1 /079c6f39023649b154152db30f1d781e884879bcのまま、公式gitlinkへ全submoduleを揃える。CYW43 055d64274b014dd7b1c2fc94d26e8a18face7124、lwIP77dcd25a72509eb83f72b033d219b1d40cd8eb95、btstack eb0bb8b5ea6d234ccb940313b47f7a5c3b4e20ec、mbedtls0bebf8b8c7f07abe3571ded48a11aa907a1ffb20、tinyusb86ad6e56c1700e85f1c5678607a762cfe3aa2f47（既に一致）。mbedtlsの新しいframework submoduleもgitlinkを確認し、HTTPSのみで取得する。SSHは使わない。

## 変更前の保持

SDK全体（.gitとsubmodule metadata/未追跡/ignoredを含む）をSDK-align-20261021T032923Z/SDK-before.tarへ退避する。各repo HEAD・branch・status、archive SHA256、private診断生成source/diffと元source、書込み済みUF2/ELF・個別configのhashを保存し、退避照合後だけcheckoutする。reset/clean/force checkoutを使わない。想定外のlocal変更/競合があれば保存して止める。

容量対策の途中6ファイルは同rootのcapacity-WIP.patch / capacity-WIP-files.tar / capacity-WIP.jsonへ退避し、元workにも残す。未コンパイル・未検証で、SDK比較には混ぜない。比較sourceは書込み済みf2bdc7b2e17cc620e5d568c16842ee6ea871e7b0から別dirへgit archiveし、直前LCD buildの個別config / board bindingをhash照合して使用する。

## 比較条件

基準は書込み済みLCD fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3a（ELF827f556716e32d4afbf78a78406ae3963d85a5ef07eb1c83c76983626af2edd8）。候補はapp固定コード、config、pin/解像度、Release/pico_w、stack4096、EVENT_STREAM/STREAM_DIAGNOSTICS/ACK_PATH_PROBE/USB_BUTTON_SIM/TX_LINK_PROBE/CYW43_INNER_PROBE/AWAY_EEX1/port48116等の同じflagsを維持し、SDKの組合せだけ変える。新しいbuild dirと私有patch生成dirを用い、共有SDK原本へ診断patchを適用しない。

旧CYW43原本SHAに固定された診断生成器を新原本へ機械的に通さず、関数・置換箇所・引数/回数/戻り値を比較してから最小適合する。出自marker/CAPS source pinも新原本へ更新し、そのmetadata変更と生成器/対応collectorの差を明示する。appのACK/入力/描画/TCP bool、STA lock/flash疑いを混ぜて修正しない。

計測付き候補と公式LL無計測の参照buildを区別する。後者はLL probeだけOFFとし、他のflagsは維持するため「全アプリ無計測」と呼ばない。比較候補の動作改善やRF効果は実機前には未検証。既存旧成果物・新候補・参照buildのsource/SDK/flags/hashをそれぞれ記録する。

## 検証と実機へのgate

公式repositoryのgitlinkとローカルHEADを全件照合し、submodule tracked/untracked変更を確認する。新SDK実関数のON/OFF 18シナリオ（native/ASan/UBSan）でbus bytes/呼出順/error/pbuf意味を照合し、変更したpin metadataとcollectorのhost回帰、既存firmware host回帰を実施する。ARM link/map/stack/heap余裕を旧版と比較し、heap8192B/stack4096B下限を変えない。不足や失敗を隠してflagsを落とさない。

容量対策は未実装の別比較であり、SDK候補へoverlay/serializer差分を含めない。SDK更新による他projectの次回buildへの影響を明記する。既に稼働中のPico/サーバはSDKフォルダ変更だけでは更新されない。

候補完成後に対象LCD E661640843239D21、UF2 SHA、verify/BOOTSEL/再接続の方法、直前復旧UF2 f0c6a15a…と現在fd19…の位置を先に報告し、実機書込みは行わない。E-ink書込み、ai-news/OS/network/service変更、SSH、pushは対象外。

## 復旧

HEAD/ローカル変更を再点検し、更新後の追加変更があれば先に別退避。保存before.jsonの各旧SHAへ非forceのdetached checkoutで戻す（SDK本体は変更しない）。新規mbedtls/frameworkは削除せず別保持し、旧親commitでは非使用として記録する。metadata/全fileの復旧が必要な場合はSDK-before.tarを別ディレクトリへ展開・SHA照合し、現在フォルダへ盲目的に上書きしない。稼働機器/DBの巻き戻しは含まない。
