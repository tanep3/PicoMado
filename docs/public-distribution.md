# 公開候補の作成と確認

この文書は保守者向けです。通常の利用者は配布物に含まれる日英の初回設定ガイドを使います。

元の `main` の履歴には、過去のローカルパスや個体・ネットワーク観測が残ります。現在の履歴を書き換えず、元リポジトリやその Git 履歴を公開対象にしないでください。非公開の原本と Git 履歴のバックアップは公開候補・添付物に含めません。

## 候補の生成

クリーンな `main` で `python3 tools/export_public.py --out /非公開の新しいパス` を実行します。Git が追跡する許可リストのソース・ガイド・素材・ライセンスから、履歴なしのディレクトリと各ファイルの SHA256 manifest を作ります。個体設定、内部変更記録と観測記録は選びません。既知の試験用個体識別子は置換し、内部 JSON 中の識別子とローカルホームパスへの一致を拒否します。

`--allow-dirty` は未コミットの実装を試すためだけのオプションです。その出力は配布しません。これらの検査は秘密情報の不在を証明しないため、候補のテキスト・画像・メタデータ・第三者権利表示を別途確認します。候補は独立レビューまで非公開に保ちます。

## 試験とアーカイブ

候補側で `python3 tools/public_gate.py verify-source` を実行し、日英ガイドに従い空の Cargo キャッシュで依存取得、個体設定、検査、サーバ・ファームのビルド、preflight、プラグインを試します。テストの設定、ログ、receipt と成果物は個体識別子やパスを含み得るため私的な場所に保持します。実機を使わない結果を実機確認と扱いません。

検査を終えたら、同じクリーンな `main` から**新しい空の staging ディレクトリ**を作り直します。試験用設定、build、Cargo キャッシュ、ログ、receipt やバックアップをコピーしません。試験候補と新しい staging の `PUBLIC-SOURCE-MANIFEST.json` が一致すること、新しい staging に Git メタデータや個体設定がないことを確認します。新しい staging 全体を履歴なしアーカイブにし、アーカイブを再展開して manifest を検証し、アーカイブの SHA256 を控えます。manifest の一致だけを公開承認とせず、ソース・素材・ライセンスの独立レビューと実機の残件を明記してください。アーカイブ生成は公開や push の許可ではありません。

次は GNU tar を使う例です。`PM_TEST_SOURCE` を検査・ビルドした候補ディレクトリに設定し、検査・preflight と独立レビューを先に完了します。アーカイブの出力先は Git 管理外です。

```sh
(
set -euo pipefail
PM_TEST_SOURCE=/検査済み候補の絶対パス
PM_RELEASE=$(mktemp -d /tmp/picomado-release.XXXXXX)
python3 tools/export_public.py --out "$PM_RELEASE/source"
cmp "$PM_TEST_SOURCE/PUBLIC-SOURCE-MANIFEST.json" "$PM_RELEASE/source/PUBLIC-SOURCE-MANIFEST.json"
PM_TEST_DIGEST=$(cd "$PM_TEST_SOURCE" && PYTHONDONTWRITEBYTECODE=1 python3 -c 'import sys; sys.path.insert(0,"tools"); import public_gate; print(public_gate.source_digest())')
PM_RELEASE_DIGEST=$(cd "$PM_RELEASE/source" && PYTHONDONTWRITEBYTECODE=1 python3 -c 'import sys; sys.path.insert(0,"tools"); import public_gate; print(public_gate.source_digest())')
test "$PM_TEST_DIGEST" = "$PM_RELEASE_DIGEST"
(cd "$PM_RELEASE/source" && PYTHONDONTWRITEBYTECODE=1 python3 tools/public_gate.py verify-source)
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --exclude='__pycache__' -C "$PM_RELEASE/source" -cf - . | gzip -n > "$PM_RELEASE/picomado-public.tar.gz"
sha256sum "$PM_RELEASE/picomado-public.tar.gz"
mkdir "$PM_RELEASE/unpacked"
tar -xzf "$PM_RELEASE/picomado-public.tar.gz" -C "$PM_RELEASE/unpacked"
(cd "$PM_RELEASE/unpacked" && PYTHONDONTWRITEBYTECODE=1 python3 tools/public_gate.py verify-source)
printf "Local archive: %s\n" "$PM_RELEASE/picomado-public.tar.gz"
)
```

この手順は候補をローカルに作るだけです。ハッシュの配布先の確認、レビュー、公開操作は別途扱います。
