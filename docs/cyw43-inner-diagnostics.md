# CYW43内部の有限RAM診断

今回の目的は、条件付きモデルで確認したTX/RX共用bufferの変化と実機の欠落を、同一SEQ/IPIDで対応付けること。機能修正ではない。前回は操作から描画完了179.446ms、描画19.879msであり、秒単位の画面停止を再現したとは扱わない。描画完了通知の初回IPID102がホスト観測に現れず、1.034240秒後の同SEQ/IPID103が到着したことが新診断の出発点。

## ビルドと隔離

`PM_CYW43_INNER_PROBE` はdefault OFFで、ONには既存 `PM_TX_LINK_PROBE` と `PM_ACK_PATH_PROBE` が必要。OFFは共有SDKの元 `cyw43_ll.c` をそのままコンパイルする。ONは `prepare_cyw43_inner.py` が、原本SHA `a4e6baa503b9332a0a445c8b22deb4bee62793c441dc74a200b0fe8065c397c7` を検証し、ビルドディレクトリだけに私有コピーを生成する。CMakeの `cyw43_driver` INTERFACE_SOURCESのその1ファイルだけを置換する。共有SDKやサブモジュールは編集・更新しない。各コピーの原本／生成SHAとdiffを保存する。

実際のdriver TUのcompile command・PM_CYW43_INNER_PROBE/CYW43_USE_SPI定義、link map、hook呼出しrelocation、`pm_cyw43_inner_translation_unit` を確認する。CAPSにも `cyw43-inner-v1:a4e6baa5` が現れる。main.cのフラグだけでリンク成功を判断しない。

## 採取位置と意味

LLのpbufコピーの戻り値だけを追加で観測する。同じコピー関数を同じ引数で1回呼び、短いコピーを修復しない。コピー長が不足／不明なら未初期化のtailをCRCしないで、reject/incompleteを記録する。LL copy直後からcommon入口までは同じSDK lock内でcallbackがないため、直後のDATA openがそのコピー長を消費する。nested Ethernet sendも各コピーを入口で消費し、親にコピー情報を付け替えない。

対象DATA送信のcommon入口で、上書き前の元SEQ/end/ACK/IPID・TCP長・interfaceを値として保存する。boot/run/connection世代/tupleはTX1と共通。1送信のticketは、その後のpollやbus書込み後に再導出しない。4段の固定context stackと各呼出しのローカルticketで、nested CONTROL/Ethernet sendを分離する。親identityは保持し、深さ超過・不整合・serial枯渇は明示する。SDK本来のnested動作を止めたり変えたりしない。

記録はcommon入口、credit待ち開始、各pollの集約、最初の実RX読出し後、元bus書込み直前、元bus戻り値／終了。SPI/SDIOの既存RX read直後で要求bytesと戻り値を集約する。返答種別がDATA以外、ヘッダ不正、部分読出しエラーでもbufferが変わり得る。最初の読出しが観測されたpoll後にCRCを1回だけ取り、最終bus直前とも比較する。各ループでCRCしない。negative pollのres_len/res_bufは一切読まず、positiveの場合だけ元SDKが初期化したres_lenを使う。DATAのinterfaceタグbit31は、保存するrx_lengthのbit15へ移し、下位15bitをpayload長とする。read APIは実コピーbytes数を返さないため、`req` は要求量であり、エラー時の実書込み量とは呼ばない。

bus/pollの回数・順序・引数・pbufの所有権・credit更新・callback・timeout/poke方針・戻り値は元処理を維持する。bufferの復元、再送、RX配送の変更、余計なbus/network操作は加えない。

## CRCと固定上限

比較領域は `spid_buf +14` からの **初期化済みBDC4＋Ethernet**。先頭SDPCM12とpadding2、末尾bus alignment paddingを除外する。通常のSDPCM size/sequence/header構築による変化は誤検出しない。interfaceは元BDCのflags2から復元でき、元／bus直前のBDC4bytesも保存する。

元の領域長は `domain`、比較長は `covered=min(domain,256)`。256超はprefixフラグ。1記録のCRCは入口・最初の実RX read後・bus直前の最大3回、1run最大8記録、CRC走査最大6144byte。4bit lookup CRC32を使い、CRC処理の経過時間・回数・bytesも集約する。CRC一致は数学的同一性の証明でも、prefix外の不変性・RF送信の証明でもない。

1記録144byte以内×8、状態全体1536byte以内をstatic assertする。固定追記・上書きなし。reserve中はDONEなし、元関数が終了したときだけcommitする。公開読出しは非active・DONE・boot/run一致を要求し、partialはincompleteとして可視化。満杯、depth不足、counter飽和、reject、世代変更を隠さない。frozenの記録は更新しない。新runのresetは既存SDK lock内で、古いAP2の明示release後だけに起きる。

## lock・時刻・負荷

writerは既存CYW43 API／SDK callbackのlock内。mainの同期・snapshot・reset・freezeも既存 `cyw43_arch_lwip_begin/end` を使う。hook内で再lock、USB、printf、malloc、filesystem、sleep、追加callbackを行わない。AP2のmain専用リングへIRQから書かない。保存するのは値だけで、pbuf/spid_buf pointerを保持しない。64bit時刻は同じPico uptime clock、wait intervalは32bitのwrap-safe差分。TCP SEQの32bit wrap、SDPCM credit/sequenceの8bit wrapと混同しない。ホストrealtimeとの差を直接取らない。

CRCや集約の追加時間はゼロではない。credit/RX条件を作る／抑える可能性があり、枝の観測から元の頻度を推定しない。前回のlinkoutput226–282μsは元driverも含み、診断増分ではない。実機のCRC時間、hook区間、loop/drop、静的BSS／ARM stackを分けて報告する。動的heap／stack high-waterや実IRQ・RF中立性はホスト試験だけでは証明しない。

## CY1回収

同じCDC parserから、各数値引数は16進で送る。

```
CY1 CAPS
CY1 STATUS
CY1 QUALITY
CY1 READ <boot> <run> <index> <part0..5>
```

返答も基本は16進、`rxrc/brc` は符号付き10進、active/changedは0/1。6partの順序は0時刻/ticket/flags/depth、1元key、2bus直前key、3CRC/domain/BDC、4wait/RX集約、5IO要求量/credit/sequence/元bus戻り値。digestは公開フィールド順のlittle-endian FNV-1aでpaddingを含めない。全行144byte未満を最大幅の試験で確認する。正常 `cy1` 行とエラーを実Serial parserから回収する回帰を含む。

`usb_button_trial.py --require-inner` がschema/source/capacityを旧H RAMのreleaseより前に確認する。AP2 ARM後、CY1 boot/run/gen・empty windowをBEGIN/COMMIT前に確認する。CDC close後だけ有限jobを実行し、観測終了後にP/H/TX/CY1を回収する。各SDK partは読出し直後に別ファイルへ保存し、途中切断時も保持。最終status/qualityとdigestの一致を確認し、同じ元SEQ/end/ACK/IPID/長さとPico時刻包含でTX1へ結び付ける。欠けたjoinやoverflowをcleanと扱わない。失敗時はSTOPに加えて、開いているUSBとHで該当窓をfreezeし、不明な記録をrelease/rearmしない。

## 試験と解釈

実SDKの送信2関数、SPI poll、SDPCM parserを原本／計測版から抽出し、valid framed DATA/CONTROL/ASYNCEVENT/credit-only、invalid header、部分read error、timeout、bus error、nested send、prefix、短いpbuf copy、別interface、credit255→0／TCP wrapを試験する。IO／clock／async callbackはmockと明記し、ON/OFFのbus bytes/call trace/return/pbufを比較する。既存の条件付き破損は両版に残る。sourceモデルの到達性はRF再現とは別。

採取後は、no stall、stallで変化未観測、stall＋RX＋比較領域変化、不完全／overflow、bus errorを各TXごとに分類する。bus直前で比較領域が変わったことはその送信のdriver内部変化を支持するが、RF送信を証明しない。変化なしでホストに現れない場合は、その後のdriver/firmware/RF/AP/NIC経路も残る。no stallは、その送信の当該credit-poll分岐を除外するだけ。イベント欠測や1回の成功を、不具合不存在・画面停止の修正完了とは呼ばない。
