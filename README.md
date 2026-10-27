# PicoMado

PicoMado は Raspberry Pi Pico W と小型画面で時計、天気、雨雲レーダー、動画、風刺画などを表示する工作プロジェクトです。Linux 上のサーバが表示内容を作り、Wi-Fi 経由で端末へ送ります。

## できること

- 時計、天気、雨雲レーダーを表示する
- サーバで管理する YouTube 動画や風刺画を端末へ送る（外部サービスや追加の実行環境が必要な機能があります）
- ボタンで表示内容を切り替える。LCD 版は 4 ボタン、E-ink 版は 0 または 2 ボタンの構成です
- ブラウザからサーバの設定やプラグインを操作する

サーバ、端末ファームウェア、素材、設定例をこのリポジトリに収めています。完成端末には Pico W、対応画面、配線、Linux サーバ、Pico SDK と個別の Wi-Fi 設定が必要です。

## 対応状況

[対応表](docs/guides/hardware-support.md)に画面とボタンの組合せ、および実機確認の範囲をまとめています。LCD 4 ボタンと E-ink 0 ボタンには実機での確認記録があります。E-ink 2 ボタンの完成端末は実機確認が未完了です。Pico 2 W や任意の画面・配線には対応していません。

## はじめ方

1. [対応表](docs/guides/hardware-support.md)で手元の部品を確認する。
2. [初回設定ガイド](docs/guides/first-setup.md)に沿ってサーバと端末の設定を用意する。
3. 配線と画面の詳細は [ファームウェアの説明](firmware/README.md)、サーバの機能と運用上の制約は [サービスの説明](service/README.md)を読む。

既存端末を更新する場合は、設定や保存データを先に確認し、[復旧手順](docs/guides/release-and-recovery.md)も参照してください。開発のルールと検証記録は [開発入口](AGENTS.md) と [文書案内](docs/README.md)にあります。

コードは [MIT License](LICENSE)、著作権者は Tane Channel Technology です。フォントなどの第三者素材には[個別の権利表示](service/assets/NOTO-COPYRIGHT)が適用されます。
