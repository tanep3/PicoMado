# CI-SDK-20261108: ローカル検証結果

2026-11-08、正本 main、設計 commit `16288aee60a0a9b9bfba81ff9ef1333ff0944bf6` 上の今回差分を検証した。製品の firmware/service ソースは変更していない。

## 結果

- SDK preflight: Pico SDK 2.3.1 / `079c6f39023649b154152db30f1d781e884879bc`、submodule と必要ソースを確認して成功。host 試験後も SDK に tracked change なし。
- SDK 不在の CLI negative: exit 1 と具体的な不足エラーを確認。
- SDK 回帰試験: 8 件成功。完全な SDK、各必要ファイルの欠落、SDK 不在、revision 不一致、可変 ref、未初期化／不一致／競合 submodule、tracked changes、git の失敗／timeout を確認。
- 既存 Python: hardware profile 11 件、public export 3 件成功。新規ファイルの staging 後にも public export と SDK 回帰試験を再実行して成功。
- `bash firmware/tests/run_host_tests.sh`: exit 0。今回 CI が停止した static ARP、SDK flash helper、CYW43 credit と後続全試験を含む。
- `bash firmware/tests/run_usb_tx_tests.sh`: exit 0。モックと既存の fresh PTY fixture のみで実機操作なし。
- `cargo test --locked --all-features --manifest-path service/Cargo.toml`: exit 0、全 suite 合計 474 成功、0 失敗、3 ignored。ignored を成功件数に含めていない。
- Workflow YAML を解析し、push/PR trigger の維持、固定 SHA の再帰 checkout、PICO_SDK_PATH と checkout path の一致、host 試験前の preflight、既存試験コマンドの維持を確認。
- `git diff --check` と Python syntax check 成功。独立レビューでは修正必須の指摘なし。レビュー担当も新規 unit 8 件を独立実行して成功。親の最終受入と hosted CI 成功は別段階。

## 検証環境と限界

raspi5 / Linux ARM64、Python 3.12.3、GCC 13.3.0、Cargo 1.98.1、CMake 3.28.3、ffmpeg 9.0.2。既存 pic2gray4px を `PICOMADO_PIC2GRAY4PX` で指定した。並行開発への負荷を抑えるため `CARGO_BUILD_JOBS=1`、`RUST_TEST_THREADS=2` と低優先度で実行した。共有 SDK は読み取りのみ。デバイス、稼働サービス、Bluetooth 実装、GitHub 設定は変更していない。

GitHub Ubuntu runner での checkout、パッケージ取得、実行の成功は未確認。ローカルで新しく SDK を clone せず、同じ SHA の clean な既存 SDK を利用した。修正の push はユーザーが行う。新コミットの hosted CI 完了を確認するまで operational/effectiveness を完了扱いにしない。CI は最終的に有効のまま維持し、過去の失敗履歴は削除しない。

## 検証したファイルの SHA-256

- `.github/workflows/observation-gate.yml`: `bac1be9af4de964a2d6dafa54eee6434977abe14afd3cdc88eb55a091f8538dc`
- `tools/check_ci_sdk.py`: `46bc0eaee028ddf15112e38b90bb3f3f5e83d9f862da0120af4cb14ef5cb915f`
- `tests/test_ci_sdk_validation.py`: `a84f75cf5f799a6c473859ec55262333c7a72e808683eb071d8b85b0593cf4a6`

## ローカルログ

- `/tmp/picomado-ci-sdk-20261108/host.log`: SHA-256 `bfab78bd42d26542247f6b6c5b944fbb2f17fcc05164fa168be3bb7201c856f6`
- `/tmp/picomado-ci-sdk-20261108/preflight-missing.log`: SHA-256 `015c8e8235afb2297b28ebdbf762b64d7bc7bd7a68d30f74ddf705e32114bea0`
- `/tmp/picomado-ci-sdk-20261108/preflight-sdk.log`: SHA-256 `ca22593d321e2540487684718c46504e9cb3e32d3dda0ec0b18a3b606cc68a4d`
- `/tmp/picomado-ci-sdk-20261108/server.log`: SHA-256 `f157042bf647f0e28e0799c9971a89056a875a86970afe8674ada7f15d9f7249`
- `/tmp/picomado-ci-sdk-20261108/test_ci_sdk_validation.py.log`: SHA-256 `38d3f57e973717f5c9d1b77fab6e19f6f7c56bbbb6ae5c79b6910f4c382180d3`
- `/tmp/picomado-ci-sdk-20261108/test_hardware_profile_validation.py.log`: SHA-256 `6a296fb663f96795e46b99781fcf03db952210249cb761106ed78a0823fb78fd`
- `/tmp/picomado-ci-sdk-20261108/test_public_export.py.log`: SHA-256 `03fbb066fcf7707f9c3f426a2e7f64f5ec3f7b461d02715ef601704e80dabdb1`
- `/tmp/picomado-ci-sdk-20261108/usb.log`: SHA-256 `5883c795ca8168a3f08dcb1f6077b112633462a23aaf181a70b2b4672eabeec0`
- `/tmp/picomado-ci-sdk-20261108/workflow-structure.log`: SHA-256 `1fe485f15e2238702abd10a54e5d779409dc0efd2517fb3e043a04182a34c5e8`
