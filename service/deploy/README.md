# ユーザーごとのサーバ設定

[最初の設定手順](../../docs/guides/first-setup.md)に従い、`config.toml` の例示 IP を自分の LAN アドレスへ変更します。`database_path` と `plugins_root` の `~/` は実行ユーザーの HOME へ展開されます。既存の設定、DB、各 plugin の `data/` と `cache/` は自動移動・上書きしません。

`picomado.service` は systemd **user** unit の例です。実行ファイルを `~/.cargo/bin/picomado-server` に置く例なので、別の場所へ入れたら `ExecStart` を自分のパスへ変更します。`%h` は systemd がそのユーザーのホームへ展開します。設定ディレクトリを先に作り、動かす前にファイルを確認してください。このテンプレートの配置やサービス操作は自動では行いません。

DB は `plugins_root/data/picomado.db`、各 plugin の永続素材は `<plugin>/data/`、取り直せる素材は `<plugin>/cache/` に置きます。フォントのライセンスは [NOTO-COPYRIGHT](../assets/NOTO-COPYRIGHT) を参照してください。
