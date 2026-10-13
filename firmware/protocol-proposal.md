# 汎用メニュー契約の追加提案

現 `service/src/protocol.rs` の v1 は画像と `PluginPosition`、物理ボタン操作を持つが、メニュー項目の配信と決定結果を持たない。ファームはローカルの家/外設定メニューを表示できるが、サーバが後から追加する共通メニューを v1 だけでは扱えない。

次の通信契約検討では、サーバ→Pico の `MenuOpen` に `incarnation`・`session_epoch`・`display_epoch`・`screen_owner_epoch`、`menu_id`、表示用 `title`、有界の `{id,label}` 項目列、選択初期位置を載せる。Pico→サーバの `MenuSelect` は同じ世代と `menu_id`・選択 `id` を返す。サーバ→Pico の `MenuClose` または新画像はメニュー所有を終わらせる。上/下はローカルの選択移動、サブは選択確定、電源は取消しとする。項目数・UTF-8 文字数・全 JSON 長は 4096 バイトの control 制限内に収める。

Pico は `MenuOpen` の全長・重複 `id`・世代・項目数を検証し、古い接続や表示世代のメニューを描画しない。メニューを開いた後の画像も所有世代で扱う。サーバと端末の双方でモードを明示し、現在の `PluginPosition` の有無だけでサブモードを推測しなくて済むようにする。新メッセージを追加する際は v1 の kind 範囲・JSON decoder・端末能力申告と互換性を決めてから実装する。

初回実機の通信試験は現 v1 の `Hello`・`Welcome`・`Settings`・`FrameStart`/`FrameChunk`・ACK・ボタン操作で行えます。この提案はサーバ担当による契約判断用で、`service/` は変更していません。
