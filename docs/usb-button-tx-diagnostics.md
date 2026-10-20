# 有限USB入力と送信リンク境界の診断

これは低頻度の入力遅延を採取する任意の診断機能であり、遅延の修正ではない。共通 `pm_button_input_step` → `button` を使い、プラグイン名・組合せによる分岐を持たない。選択、有効化、並び順、設定保存、PM none、TCP_NODELAY、EEX1、画面保持の意味は変更しない。

## ビルドと対象

`PM_USB_BUTTON_SIM` と `PM_TX_LINK_PROBE` は初期値OFF。TXは `PM_ACK_PATH_PROBE=ON` を必要とする。LCDとE-inkの全アプリは同じ実装を使い、入力能力は既存の0/2/4個から取得する。0個は `unsupported`、CAPSには `keys=0` が返る。診断OFFではUSB制御・TX記録のBSSと関数はリンクされない。

変更は既存ACK診断候補を土台にした隔離ソースで行う。実機書込み前に差分、ホスト試験、実際のARMビルドとUF2 SHAを独立レビューする。E-inkはビルド／隔離入力試験の対象で、実機書込み対象ではない。

## USBプロトコル

USBの受信者はmainの既存getcharループ1つ。固定95文字の行をAP2/SIM/TX1へ振り分ける。出力は既存の原子的な144バイト未満の行キューを共用する。AP2ページの行と他名前空間の行を交互に出せるが、行のバイトは混ぜない。1回の受信予算16文字と既存の非同期部分書込みを保つ。

**SIM/TX1の数値引数は全て16進、符号・0xは付けない。** 返答の `phase,n,key,state,keys,logical,a/d/r/c(真偽),why,rc` は10進。ID、時刻、マスク、長さ、SEQ、カウンタ等は16進。AP2は既存形式のまま。ホストは名前空間と返答種別ごとに解釈し、ASCII空白を正規化した同一コマンドだけを同内容と扱う。

```
SIM CAPS
SIM STATUS <boot>
SIM BIND <boot>
SIM GET <boot> <job> <slot>
SIM ACK <boot> <command_id>
SIM BEGIN <boot> <command_id> <job> <count> <expected_session> <expected_state>
SIM STEP <boot> <command_id> <job> <slot> <delay_ms> <physical_key> <hold_ms>
SIM COMMIT <boot> <command_id> <job>
SIM PRESS <boot> <command_id> <job> <physical_key> <lease_ms>
SIM RELEASE <boot> <command_id> <job>
SIM CANCEL <boot> <command_id> <job>
SIM STOP <boot> <command_id>
TX1 STATUS
TX1 READ <boot> <run> <index>
AP2 SYNTH
```

BEGIN/PRESS受付時にboot、AP2 run、元sessionを固定し、`SIM BIND` で回収する。USB制御の各tick・SIMコマンド・AP2変更コマンド直後に現在boot/runを照合し、変更時はrun取消として仮想FSMを無発火で停止する。run0の単独操作も、新たなAP2 armで取消される。旧COMMITの保存ACKを読み直しても予約の再arm・新runへの付け替えは起きない。

コマンドIDはboot内で単調増加する64ビット、jobも単調増加する別ID。解放・取消のコマンドIDと対象jobは別引数。同じコマンドID＋同内容は固定8件の台帳から元の返答を返す。別内容はconflict、保持外の過去IDはold_commandとして実行しない。再送で再実行やlease延長は起きない。古い結果が台帳から消えても新しい操作にはならない。新jobを開始すると直前jobの詳細結果は置換されるので、開始前に回収する。

上限は1job／4手順、全体15秒、holdは80～5000ms、接続中のstaging／COMMIT待ちは10秒。STEPは個別に領域へ格納し、全件の範囲と非重複をCOMMITで検証して初めてaccepted。delayはCDC close検出＋500msからの相対値、各手順の間隔は直前hold＋70ms以上。50ms超の開始／解放遅延は追いかけ実行せず取消す。

`BEGIN/STEP` のstagingは実行予約の準備にすぎない。`COMMIT` のaccepted後もCDC接続中には実行しない。ホストがACKを受けてDTRを下げcloseすると、mainは先にAP2 gateを開き、次にSIM開始時刻を定める。USBケーブルを抜いた、USB IRQがゼロになった、という意味ではない。

通常PRESSはCDC切断で取消す。明示COMMITした有限jobだけは切断で開始し、再接続は残りを取消す。GPIO rawと既存stableを先に確認し、実入力があれば仮想状態を破棄する。仮想FSMは実buttons配列と独立。CANCEL、STOP、切断、期限、BUSY、session/state変更はFSMを無発火で初期化し、2ボタン短押しを発生させない。正常RELEASEと予定hold終了だけが正常解放経路を通る。長押し閾値は既存FSMの安定押下時snapshot、STANDBY復帰条件も同じ。E-ink drainの早期continueより前にも取消・期限を処理する。

返信が混雑していてもSTOP/CANCEL/RELEASEを処理し、ACKは台帳に保持する。2件の返信枠を超えた応答はdropとしてSTATUSに出す。通常の変更コマンドはbusyを返し、未受理の変更を黙って実行しない。緊急停止は電源操作、Wi-Fi変更、AP2消去、再armを行わない。

STATUS/GETのaccepted、dispatched、released、cancelledは別項目。取消時のreleasedは仮想押下の除去であり、cancelledとの組で判定する。dispatchedは共通button関数を呼んだことだけを示し、サーバ受付・TCP ACK・描画完了は別の採取結果で確認する。

## AP2とTXの採取

AP2物理counterは物理入力だけ。合成入力はイベント24でjob low/high、slot、論理key＋longを記録し、最初の合成入力も20秒窓を開始できる。`AP2 SYNTH` は合成count/job/slotを返す。既存の物理入力品質フラグを合成入力の成功へ流用しない。

`netif_default->linkoutput` の元関数を保存し、同じnetifとpbufを正確に1回渡し、同じerr_tを返す。netifの再初期化・切替時はSDK lock内で復元／付け替え、wrapperを重ねない。pbufを保持・複製・解放・書換せず、追加tcp_output、tcp_recved、ACKを呼ばない。

観測はEthernet/VLAN最大2段、IPv4基本20バイト、IHLから求めたTCP基本20バイトだけを有界peekする。pbuf chain、IP/TCP options、短いframe、fragment、total長を検査する。payloadやoptionsはコピー・走査・CRCしない。対象4tupleのデータ付きTCPだけについて元hook時刻、SEQ/end/ACK、payload長、IPID、hook入口から元関数復帰後までの時間（header観測時間を含む）、戻り値、nrtx、pbuf参照数、TCP flagsを記録する。

固定32件・追記のみの専用TXバッファを使い、満杯時はdropを増やして元処理を継続する。IRQ/callbackからAP2のmain専用リングには書かない。既存SDK lockがcallbackとmainのsnapshot/reset/freezeを直列化する。USB、printf、malloc、filesystem、追加spin待ちはhook内にない。行のtは採取元時刻で、USB回収時刻ではない。run/boot/connection世代を固定し、同じrunの世代変更時は不完全として停止。freeze後の遅着レコードは採取時刻で除外し、明示新runまで保持する。

TX1 READは非activeかつboot/run一致の場合だけ。STATUSのover、changed、badを必ず確認する。行digestは定義した各フィールドのlittle-endianバイトをFNV-1a化し、構造体paddingは使わない。linkoutput成功はドライバへの受渡しであり、RF送信・対向到着の証明にはならない。

## 自動試行

`firmware/tools/usb_button_trial.py` は初期値plan-only。明示のserial、host socket/PID、既知MAC、interface、local IP、observerソース、出力先、keyを必要とする。`--execute` で既存の承認済みheader_observerを60秒／固定容量で一度だけ起動する。独自のcaptureフィルタや常駐プロセスは作らない。

1. IDLE確認。ユーザーBOOTSEL後に旧Pico RAMが失われhostだけFROZENの場合は、明示 `--release-saved` の保存SHA／旧boot/run/digestを照合し、旧ファイルを保全してhost RAMだけ解放する。不明な記録は捨てない。
2. 同一boot/run/sessionをP/Hでarmし、USB接続中に有限1操作をstagingする。
3. REACHABLEな既知MAC、route、選択PIDが所有するcontrol socketと唯一のTCP socket、inode/cookie、前後のH sessionを確認する。設定変更、ping、無限再確認はしない。
4. 既存observerのPREPARED→START→started後にSIM COMMIT、ACK受信後直ちにDTR closeする。ユーザーの手動操作や会話待ちを挟まない。
5. 60秒の有限観測後にguardの終了／reap証拠と出力SHAを保存する。H freeze、固定20.1秒settle後にcold USBでAP2/SIM/TXを読み、ページ・digest・合成由来・各品質フラグを検証して保存する。自動release/rearmは行わない。

例外時はUSBが開いていればDTR close前にSTOPを試み、observerへ停止とstdin closeを送り、3秒の終了確認を行う。ACK消失時の結果はuncertainと記録し、同じ操作を自動再送しない。firmware自体にも有限job/hold期限があり、controller消失時も無限入力は続かない。既存guardは制御EOFと固定上限を持ち、終了不明時は正確なPIDを保持して報告する。書込み・再起動・再試行への拡大はしない。

## 検証と限界

`bash firmware/tests/run_usb_tx_tests.sh` が仮想FSM、共有USB parser、実mainの0/2/4入力、元送信wrapper／SDK lock競合、AP2の実exportと既存Python28件を検証する。`ASAN_OPTIONS=detect_leaks=0 SANITIZE=1 bash firmware/tests/run_usb_tx_tests.sh` はASan/UBSan。LeakSanitizerはこの実行環境のptrace下では動作しないため無効化する。診断実装は動的確保を行わない。`python3 firmware/tests/usb_button_trial_test.py` は無実機の有限controller14件の試験（各serial段階の例外、COMMIT ACK消失、STOP/reap、旧保存SHA／boot照合、TX/SIM回収・digest破損・run誤対応拒否・永続化）を行う。独立レビューで発見した run A COMMIT→FREEZE/RELEASE→run B ARM の再現ケースもC回帰試験へ追加した。既存 `run_host_tests.sh` と実SDKのNagle試験も維持する。

ARMのBSS／stack使用量、UF2 SHA、差分と試験ログは候補ディレクトリのレビュー成果物を正とする。静的stackサイズはlibcやSDK callbackを含む実測ピークではない。hook時間／loop gap／実USBとRF／実パネルの影響は、独立レビュー後のLCD1台への書込みと有限採取で確認する。ビルド成功やホスト試験だけで実機合格とはしない。
