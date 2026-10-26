# PUBLIC-PROFILE-GUARD-20261026 — 条件付き個体設定の封止

## 再レビューの指摘と原因

先行の profile validator は compiler に本番 target の `PM_NORMAL_BUILD=1` 等を渡さずに個体 config を前処理した。個体 config が profile の後に `#if PM_NORMAL_BUILD` で別ヘッダーを読み込み、その中で GPIO を再定義すると、検査時だけ枝が無効になり通過する。本番の定義と検査の定義がずれるため、先行の root gate と拒否試験では見逃す。

## 補正契約

1. 個体 config は条件分岐（`#if`、`#ifdef`、`#ifndef`、`#elif`、`#else`、`#endif`）と追加ヘッダーを持たない平坦な定義ファイルとする。新形式は選択 profile の include をちょうど一度だけ許し、旧形式は include を一切許さない。profile header も `#pragma once` と平坦な定義だけを持ち、条件分岐・別ヘッダー・undef を持たない。これにより build target の任意の `-D` 条件で別枝のハード定義が現れる余地をなくす。
2. 既存の全個体秘密設定は上記の平坦形式に適合することを秘密値を表示せず確認する。追加ヘッダーを許さないため推移依存をビルド証拠へ新たに収集する必要はない。将来ヘッダーが必要なら契約・追跡依存・ビルド証拠を先に拡張する。
3. 有効ハードマクロの profile 照合と重複拒否を維持する。`PM_NORMAL_BUILD` と別の build macro を条件にした間接 include、旧形式の条件付き定義、profile 自身の条件付き定義を回帰試験で拒否する。許可した個体値は変わらない。
4. この検査器と契約は追跡ソースなので変更後は旧 receipt を使わず、root gate と対象別試験の証拠を最終 subject に結び直す。

通信 ACK、世代、フィードバックの振舞いは変更しない。独立受入・実機適用・効果確認は親が所有する。
