# 因果追跡のソフトウェア検証（第2-B）

時点の区別：本文の未修正・未受入は各保存点の記録。D01〜D05の最新状態、残る設計差と実機未検証は[最終照合](refactor-final-acceptance-20261020.md)を正本とする。

設計正本は `4517d4f` の `observation-causality-contract.md`。実装より前に、任意cause、境界、保存、判定、T-01〜05を定義した。追加のsource coverageはPico0x9f・server0xebであり、server socket送出の未採取境界まで有効としない。

## 実装範囲

- 任意JSON sidecar `observation:{version,boot,operation}`。なしのbytesは従来どおり。domain decodeはmetadataの不正で拒否しない。旧受信器・native kind18・画素・CRC・ACKは維持。
- Picoの受理入力、通知送出結果、API dispatch/result、FrameStart受理、CRC、static/video drawとE-ink pending/完了を固定値で採取。描画時に実際のtokenをsnapshotし、現在受信中の別tokenを使わない。業務用plugin ID分岐は追加していない。
- サーバはscope causeとframe/viewの対応を各256件の一時mapで保持。純粋Engineへobserverを入れない。遅い準備結果は正確なview keyだけで相関し、後続入力のcauseを旧frameへ付けない。
- `boundary-observation`と明示 `PICOMADO_OBSERVATION_PATH` の両方がある場合だけ保存workerを作る。source producerは固定値とtry-lockのみ。cold consumer一つ、256件buffer、排他的0600、64MiB、begin/end watermark、fsync、SHA-256 sidecar、有限停止。満杯/保存障害/終了期限超過は完了扱いにしない。
- 解析 `firmware/tools/event_causality.py` は同sourceの入力とdraw時刻だけを減算する。catalog/coverage/seq/drop/保存確認が欠ければunknown/incomplete。frame要求が確認できない操作を自動的に欠落不具合としない。後続入力による失効があり得る未完了もunknownとする。

**共有ロック外へ移った保存は観測ログだけである。** `App::persist` のアプリ状態全体JSON serializeとSQLite transactionは変更しておらず、D-02に残る。D-01のfetch完了renderも別契約 `8e4c875` の後続実装対象。

## 再現と証拠

全ログとsample成果物は隔離作業の親ディレクトリに保存し、実設定や秘密値は含めない。

- Rust全体：`causal-service-final-local-loopback.log` で単体238、bin1、オフライン結合4成功。public HTTPSテストは元からignore。追加storage faultの最終結果は後記。
- T-01：metadataなしbytes同一、旧型へのdecode、3byte断片、無効metadataでも正常Up処理。旧token/セッション・画像受信の既存回帰も維持。
- T-02：Rust実decoder→transport→App→Engine→packet schedulerと、C実main/API/assembly/drawを同一試験で通す。`causal_contract_tests.rs` と `causal_fixture.c`。新規stdin/stdout subprocessだけを使い、USB/socket実機は開かない。LCDとボタン付きE-ink1でFrameStartをInputAcceptedより先に送る。プラグインは通常契約でLCD画素を返し、E-ink adapterが3813Bへ変換する。
- T-03：重なる入力でframe/view causeを固定し、immutable VideoTransferも旧causeを保持。E-ink0は物理入力なしの機種なので入力横断の対象にせず、既存API描画試験の3profileに含める。E-ink busy時の新FrameStart破棄D-04は未修正。
- T-04：`event_causality_test.py` 5件成功。完全な未完了と有限期限前終了、CRC/seq/drop/旧catalog/metadata不足/host footer不足、重なる入力、明示draw失敗を検証。実Cのoffset/token/CRC/IO障害は `run_api_result_tests.sh` のLCD/E-ink0/E-ink1で検証。
- T-05：source保存の排他consumer、0600、既存path保持、byte上限、4producer4000試行の順序と欠測、write/fsync障害、終了2秒のtimeout中のworker所有継続を検証。通常C全回帰は52種EOFまで成功、追加C契約/実APIはASan/UBSan成功。sandbox ptraceでLeakSanitizerが使えない実行は `ASAN_OPTIONS=detect_leaks=0` と明示したログへ分ける。
- ARM：pico_w/sample-a、EV1のみON、sampleのnever-flash設定。`causal-arm-build.log`、text392444/BSS228672。実機へ書いていない。

`causal-evidence/` のLCD/E-ink1 source filesと解析JSONは実関数＋IO doubleの証拠である。表示まで15msという数値もfake clock・描画doubleの結果で、物理panelや実機遅延の測定値ではない。

## 残る受入

server socket境界の内訳、実USB/IRQ負荷、RF上の送出、実画素、E-ink波形・残像、未登録の将来境界は未確認。D-01〜04とD-05の実機負荷受入は閉じない。T-04の人工fault解析と実関数正常横断を、あらゆるfaultの全経路再現と報告しない。既存sourceとSDKと本番サービスは変更していない。

最終結果：追加faultを含むunit239件のうち、raspi5切断前に238件の成功を `causal-service-accepted-local-loopback.log` に確認した。再接続後にprocess不在とcompiled testの239件一覧を照合し、未完了だった `youtube::tests::complete_source_local_absolute_cuts_and_fps_reuse` だけを再実行して成功（41.80秒）。`causal-regression-resume.json` と `causal-regression-resumed-missing.log` がその証拠である。一度のrunが正常終了したという集計には置き換えない。オフライン結合4件も `causal-integration-resumed.log` で成功。feature無効の `cargo check --no-default-features --lib` 成功。

fixture配置の是正：既存のinclude用 `tests/transport_recovery.rs` がCargoから独立integration testとしても自動発見されるため、`tests/support/transport_recovery.rs` に移した。実writerへのincludeとassert内容は維持し、テストを外したことにはしない。初回の独立compile失敗ログも残す。

## 是正後の現在の範囲

`1cbb992`の5是正は親の独立確認で解消済み。最新server回帰294/273件に関連Rust試験を含み、最終照合ではcollector21件と解析9件も再確認した。D01/D02/D03/D04の後続状態は[最終照合](refactor-final-acceptance-20261020.md)を参照する。D05のソフトウェア範囲の成功と、実USB/SDK stdout全経路/実機負荷・物理表示の未受入は別のままである。
