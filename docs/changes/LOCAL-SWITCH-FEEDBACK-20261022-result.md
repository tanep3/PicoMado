> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# ローカル切替案内: 開発検証結果

通常書込み入口への接続は、このP2修正の受入れ後に別途進める。最新の実build/candidate/親承認/オフライン確認の結果は、work親の `normal-flash-20261022/handoff.json` を参照する。以下の「未作成」は各検証段階当時の記録である。

最新の修正固定点は `38bd24d09e923f05feb359079bcd8c114a355b08`（下記P2-01節）。初回候補はP2で受入れ保留となった。親承認・実機適用は未実施。

## 固定点と変更

- 仕様先行: `eb6bc60ee8243ba78215c0a1a384ead9395d26ea`。候補画素の寿命を補足した文書先行コミット: `99aea78f3e2d3d3c77845a9ddd1656621ec4be2b`。
- 既存の受入済みE-ink NODELAY差分を継承: `44f08344c673ebcadc482f0b2206caf9d76a89ed`（元 `34b4849` の製品/試験差分）。
- 実装固定: `2a5693e2aa3cde29c652717602f0e624c077bc50`。隔離場所: `$HOME/Documents/Codex/2026-10-18/task/picomado-input-feedback-20261022/work`。
- `switch_feedback.h` は単調時刻、接続内要求/応答、世代下限、保管されているCRC済み候補を所有する純粋モデル。mainは入力通知を先に送信し、既存のフレーム検証と描画経路へ接続する。UP/DOWNとSUBに同じ200 ms条件を使う。ネットワークcallback内の描画・待機sleep・新しいwire/ACKは追加しない。
- LCDは既存local bufferに直前の変換済み画素を保持し、案内だけscanline描画。no-opの復元は二重変換しない。E-inkは既存queued/ram bufferと非同期jobを使い、未開始/準備中の案内を取消し、本画像を優先する。局所案内/復元はremote描画ACKを生成しない。
- 限定点検で「SUB案内→同epoch候補CRC完了→次FrameStartで画素置換→no-op返信」に候補tokenだけ残る不整合を発見した。画素寿命を先に仕様へ反映し、未移譲候補の破棄と同じ呼出経路で一致tokenを失効させた。動画copy/描画adapterへ渡した画素は失効しない。元の反例はLCD/E-ink両個体で再試験し、`waiting=0 / received=0 / restores=1`を確認。

## 検証済み

- `firmware/tests/run_host_tests.sh`: 最終188 PASS。新しい純粋モデル、実main、LCD実SPI adapter、E-ink両個体/trace ON・OFF/未接続panelの案内試験を通常runnerへ統合。既存動画、V4時計更新、世代/CRC、接続復旧の回帰を含む。
- `firmware/tests/run_usb_tx_tests.sh`: 成功。物理ボタンとSIMが通る既存共通入口、単一owner、ACK/heartbeat/保存契約の回帰。
- 既存 `python3 -m unittest discover -s tests -p 'test_*.py'` と `python3 tools/project_gate.py static`: 成功。
- 新規範囲のASan/UBSan: 10 binaries成功（モデル1、実main LCD/E-ink両個体×trace有無6、E-ink実adapter2、LCD実adapter1）。sandboxのptrace下でLeakSanitizerが動かないため `ASAN_OPTIONS=detect_leaks=0`。新しい動的確保はない。
- 199999/200000/200001 µs、32bit超の時刻、期限直前CRC完了、header/部分/不正CRC、遅着、連打、frame先行/古い応答、SUB no-op、旧動画継続、送信/描画失敗、owner置換/再接続を確認。E-inkは準備中取消し、物理BUSY後合流、本画像優先、no-op復元、remote結果非生成を確認。
- 3個体の通常ARM build成功。private設定/board bindingは元リポジトリとSHA一致。LCD診断7 flagsとPR155 private LLは既存候補と一致し、共有SDKはcleanのまま。SDK `079c6f39023649b154152db30f1d781e884879bc`、CYW43 `055d6427…`、lwIP `77dcd25a…`。
- フル画像buffer追加なし。LCD静的RAMは既存候補比160 B増（model 120 Bを含むmetadata）。案内scanline 320 B、compilerの該当関数stack 368 B。main stackは4096 B、linkで予約されたstack領域合計は8192 Bを維持。LCDは各本画像を既存local bufferへ25,600 B memcpyするため、その実機時間/動画CPU影響は未測定。
- bitmapをLCD 160×80（白字/黒背景）、E-ink 250×122（黒字/白背景）へ配置したPNGを目視確認。2行の文言・中央配置・余白を確認。

## 初回開発候補（P2で受入れ保留）

| 端末 | binding | UF2 SHA256 | flash終端（排他） | link時heap余地 |
| --- | --- | --- | --- | --- |
| example-lcd | `0123456789ABCDE0` | `291aa0a30506817eacc813e1e77d61c3d41c1e100e0c5da7a1fa1f2a881aca6a` | 0x1006ad00 | 12496 B |
| E-ink_0 | `0123456789ABCDE1` | `6aba8f87a5d87b4398838d3ce2813043ef56c3988db71111a66cff1e75821613` | 0x1005a500 | 98076 B |
| E-ink_1 | `0123456789ABCDE2` | `3770c07251d76fa6e9932111d0e7939c416e17dbaededca8ae80b39d9b67c076` | 0x1005af00 | 97980 B |

各UF2は `firmware/build/<端末>/<端末>.uf2`。全blockのfamily/index/addressを検証し、設定予約sector `[0x101ff000,0x10200000)` を含まない。heap値はlink時の余地であり、実行時空き量の計測ではない。

証拠はworkの親に `candidate-artifacts.json`、`arm-commands.json`、`arm-results.json`、`*-build-final.log`、`host-tests-final.log`、`usb-tx-tests.log`、`structural-tests.log`、`feedback-sanitized.log`、`config-bindings.json` と `lcd-switch-feedback*.png` / `eink-switch-feedback*.png` を保存した。秘密値は出力・追跡しない。

既存root build adapterはLCDの診断/PR155 flagsを指定できない。今回のARM成果物は明示した同flagsによる**隔離CMake開発ビルド**であり、controlled-build receipt / candidate preflight / 独立受入を取得済みとは扱わない。既存入口を拡張する改修はこの機能へ混ぜていない。既存形式のchange記録は設計と実装固定点を示し、運用完了を宣言しない。

## 実機で残る確認と適用範囲

200 msは押下受理の単調時刻による期限判定で、表示開始時刻を厳密に200 msへ割込み駆動する仕様ではない。実機でのmain-loop遅延、視認性、ちらつき、連打/SUB、動画CPU負荷は未確認。E-inkでcommand 0x20以後のfull refreshは中断できず、本画像が到着すると既存BUSY完了＋sleep待ち（100 ms）の後に描画する。案内開始後は最大一回のfull refresh待ちが加わり得る。新しいpartial/LUTは導入しない。

現サーバの接続内返信順序とsession/owner/display_epochを使う。任意の別サーバや外部更新をaction IDで完全に因果同定するものではない。画像を返さない同epoch返信はno-opとして完了させる。

本機能には端末firmware更新が必要で、サーバの変更/再起動は不要。独立確認後、親と実機担当が以下を行う。

1. 固定実装と候補の出自・flagsを確認し、通常flash入口に必要な実際の検証/受入記録を整合させる。今回の開発ビルドを既存の別候補や旧changeのreceiptで受入扱いしない。
2. 稼働中の最新firmwareと復旧UF2を端末ごとに再照合する。LCDの基準はPR155候補 `77c40a06…90e9`。E-inkのNagle更新状況は別担当が操作中なので、元の `4ce4c6ad…` / `4f3f1d94…` と現在を再確認する。
3. USB ownerを調整してから、承認された対象だけBOOTSELのFlash IDとbindingを照合し、設定sectorを保持して1回書込み、verify、再接続する。サーバ/DB設定を変更しない。
4. 速い切替で案内なし、200 msを超える未到着で案内、到着後の本画像優先、SUB no-opとサブ端復元、連打、旧動画、通信失敗時の既存表示を確認する。E-inkは案内前に本画像が届くケースとrefresh開始後のケースを分ける。異常時は対象の直前UF2へ戻し、設定/データは巻き戻さない。

今回、実機・USB・書込み・サービス操作・SSH・外部pushは行っていない。パケット欠落の根治調査とその実機担当を止める変更はない。

## P2-01 修正・再確認依頼

- 原レビュー: `/tmp/picomado-feedback-review-kcjcccs2/REVIEW.md` / `review-outcome.json`（受入れ保留）。manifest対象42ファイルをSHA照合して、workの親の `review-fix-20261022/original-review/` へ保存した。`retained-files.json` は保存先との対応を示し、原文書の絶対パスを改変していない。以前のUF2/ELF/CMakeCache/compile_commandsは同結果領域の `previous-candidates/` に保持する。元の再現Cは当時の/tmpをincludeするが、恒久回帰はrepo内の相対includeで再現できる。
- 原因・不変条件・試験表を先行コミット: `9624c17cedc6442c4c23f98bb23bc5d0afa47efd`。モデルの要求済み状態と物理表示済みを混同し、取消しの反映を欠いた。旧描画中の要求→取消し→候補喪失/再入力という合成遷移が旧回帰に欠けていた。
- 限定実装修正: `38bd24d09e923f05feb359079bcd8c114a355b08`。`pm_switch_cancel_hint_for_image()` と `pm_switch_request_hint()` が要求の取消し/発行を所有する。実mainの受信完了・返信・pollは `prioritize_received_image()` という同じ呼出経路でモデルの取消し→adapter REPLACEを実行する。表示済みの同文再描画抑止は既存adapterが所有する。新state/wire/timer、描画方式、入力送信挙動、RAM bufferを追加/変更しない。
- 恒久回帰 `switch_feedback_cancel_test.c`: 両E-ink×trace ON/OFF×候補置換/次切替/次SUBの12ケース。修正前は全件exit 2、修正後は全件exit 0。元の8ケースに加え、次SUB、再入力の199 msでは非表示・200 msで再要求、再要求一回のみ、対応画像の最終描画完了まで確認。`before/` / `after/` の各ログとresults.jsonを保存した。
- 実adapterの回帰にも、旧remote物理描画中の取消し→再要求、RAM準備中の取消し→再要求、表示済み案内のREPLACE→再SHOWで更新増加なしを追加した。最初の追加試験では既存helperが「remote resultをアプリが消費する前にlocal hintまで完了する」と仮定して失敗した。driverの既存契約に合わせremote resultを消費してから進行を確認するfixtureへ修正した。製品adapterの挙動は変更せず、初回ログも `host-fixture-initial.log` に保持する。
- 通常host runner成功。旧188 PASS行に、新12ケースのrunner報告12行と同ケース内部報告12行が加わり212 PASS行（同じ12ケースを二重の試験数とは数えない）。新旧のモデル/adapter/main/復旧/動画回帰を含む。ASan/UBSanは14 binaries / 22 invocations成功。LeakSanitizerのptrace制限は初回と同じ。
- 3個体のARM build成功、LCD flags/private PR155 LL・個体設定・board bindingは保持。SDK clean、UF2全blockの設定sector非包含を再確認。静的RAM/heap余地は初回と同じ、main stack 4096 B（link予約領域合計8192 B）。

| 端末 | 修正候補 UF2 SHA256 | flash終端（排他） | link時heap余地 |
| --- | --- | --- | --- |
| example-lcd | `92fc0ee9dc0c50161b7fa158252407f507b29292ffd2a6f8ccb1ac3eeaf4fc79` | 0x1006ad00 | 12496 B |
| E-ink_0 | `4160d025ed2584b6eb4bd332bb5262dc90c3b74c12d81cff2ea2a331e7e83cfc` | 0x1005a500 | 98076 B |
| E-ink_1 | `4405a4d03d2773de1f392bacf9e0aaa5d062bfc623811b544d2a22e2367fee34` | 0x1005af00 | 97980 B |

新候補の実体は同じ `firmware/build/<端末>/<端末>.uf2`。最新の `candidate-artifacts.json` はwork親と `review-fix-20261022/` に保存。修正固定点の出自、before/after、全テスト/ビルドログ、原レビュー、再確認用handoffは同結果領域で参照できる。既存root CLI checkの実行結果とsource bindingは、その結果領域の `root-check/check.json` を正とする。

今回も隔離CMake開発ビルドでありcontrolled-build receipt、candidate preflight、親承認の代作はしない。通常flash入口に関する前節の未完了範囲、実機の視認性/ちらつき/CPU負荷、0x20以後のE-ink中断不能性は変わらない。実機・USB・SSH・サービス操作は実施していない。
