# BT-AUDIO-20261107 / MUSIC-PLAYER-20261107 ホスト検証

実装者による検証記録。独立検証・本番適用・実機効果確認ではない。

## 成功した試験

- 既存を含む `bash firmware/tests/run_host_tests.sh`: PASS行258件。追加4対象はAVRCP input、PCM ring、Bluetooth control fake、audio media fake。
- `rustc --edition 2021 --test service/src/audio_transport.rs`: 3 tests PASS。重複/古いcredit、世代置換、bounded ring、binary wire長、overflow。
- `bluetooth_control_test.c` / `bluetooth_media_test.c`: `-Wall -Wextra -Werror`、ASan/UBSanでPASS。実行環境上LeakSanitizerだけ無効化。
- WebUI JavaScript `node --check`: PASS。
- `PICOMADO_PIC2GRAY4PX=/home/tane/bin/pic2gray4px cargo test --locked --all-features --manifest-path service/Cargo.toml -j1 -- --test-threads=2`: 367 passed / 0 failed / 2 ignored（lib 361、bin 2、integration 4）。
- 最終の古いBluetooth応答処理・非永続観測経路の差分後、同じ条件の `--lib` 再試験: 361 passed / 0 failed / 1 ignored（197.85秒）。
- Bluetooth/legacy writerがdurable lease保持中でもStopを処理できる追加回帰試験も成功。
- 全featureの `cargo check --all-features --tests`: PASS。
- Bluetooth ONのLCDとE-ink ARMリンク: PASS。
  - LCD: text 543864 bytes / BSS 226540 bytes。
  - E-ink: text 530264 bytes / BSS 157028 bytes。
  - stack/heap予約を縮める方法は使っていない。

## 試験中に修正した点

- 既存設定sectorとSDK Bluetooth鍵領域の重なりを回避。
- 旧JSON数値配列の大きなwork bufferをBT有効版だけ縮小。現行binary画像配送を維持。
- 探索/接続の上書きは旧処理の非同期停止を待って実行。端末側25秒期限と世代拒否。
- 終端PCMの128frame境界補完と正常EOFの区別。
- Web APIの未知フィールド拒否、端末削除時のBluetooth設定除去。
- Bluetooth観測は永続DBを更新しない。停止/OFFと表示制御を音声読取りより優先し、durable lease待ちでwriterを停止させない。
- 音楽追加時の段階的な旧catalog移行と全プラグイン列挙を更新。以前の設定/選択は保持。

## 既存のgate不整合

`project_gate.py check` のstructural試験49件中、既存 `test_actual_firmware_build_sdk_flags_and_offline_flash` の1件が失敗。

変更前HEADでも同じprivate fake fixtureで再現。`flash.sh` は2026-10-27の `ec172264` でUF2確認と手動コピー案内に変更されたが、テストは旧approval拒否を期待したまま。関連ファイルは今回変更していない。実機や本物picotoolは起動していない。この不整合を無視してgate合格とはしない。

## ビルド環境

Pico SDK commit `079c6f39`、検証時SDK作業ツリーはclean。既存SDKへのパッチは加えていない。UF2の書込みは未実施。

## 未検証

実機の出音、AVRCP各ボタン、Wi-Fiとの同時連続再生、Bluetooth鍵の保持、相手機器差、長時間のメモリ/音声安定性、急速なpause/resume。サーバ・ファームの配備/再起動/書込みは行っていない。
