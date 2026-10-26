# PUBLIC-PROFILE-RESOLUTION-20261026 — 実コンパイルのprofile解決を固定

## 再レビューの指摘

先行の新形式 config は `#include "profile.h"` を許し、プリプロセッサの引用符探索が個体ディレクトリの同名ファイルを優先する。検査器は追跡済みprofileを別に読むため、個体ディレクトリの未追跡shadow headerにある条件付きGPIO上書きが検査外となり得る。平坦なconfig制限だけではincludeの実解決先を固定できない。

## 解決契約

1. 新形式の個体 config はハードマクロも include も持たない。CMakeが `profile.txt` から許可済みの追跡profileを選び、その絶対パスをコンパイルへ `-include` で渡す。検査器も同じ絶対profileを強制includeして有効マクロを比較する。個体ディレクトリに同名ファイルがあっても参照しない。
2. 旧形式のconfigは従来の平坦なハード定義を持つ。追加headerも条件分岐も禁止し、追跡profileと定義集合・値が完全一致する場合だけ受け入れる。旧形式に別の強制includeを足さない。既存の秘密ファイルは変更しない。
3. CMakeは検査器が判定した形式だけを使う。新形式の全firmware翻訳単位へ同じ絶対profileを渡し、compile_commands/receiptはその実引数とprofile SHAを記録する。試験用コンパイルも対象ごとの追跡profileを絶対パスで強制includeする。インクルード探索順に依存したハード構成を禁止する。
4. 同名shadow header、条件付き上書き、旧形式の追加include、サンプルと実設定、A→B→Aの試験を確認する。変更後のroot gateと対象別ログを最終subjectで再採取する。

通信・入力・描画のruntimeは変更しない。運用適用と独立受入は親が所有する。
