# ACK・入力経路診断 v2（ソース候補、既定無効）

反復操作で発生する遅延を、Picoの入力検出・TCP enqueue/進行と、同一サーバ接続のraw read・decode・App処理へ分けて記録する。通常のwire JSON、送信戻り値、再送、期限、端末設定は変更しない。今回は隔離ホスト試験までで、実機用ビルド、導入、再起動、書込み、追加採取は行っていない。

## 待機・観測・保持

`IDLE → ARM(run) → PREPARED(USB接続中) → USB切断 → ARMED → 最初の入力 → RECORDING → 20秒または明示FREEZE → FROZEN → 保存確認付きRELEASE → IDLE`。

PREPARED/ARMEDの待機には起動後期限がない。USB接続中は未開始のPicoをPREPAREDとしてtriggerを遮断する。GPIO処理前に接続状態を反映し、USB切断後の最初の入力だけが観測を始める。準備中入力は通常のアプリ動作を変えずに数え、最終LIVEのstate/trigger/arm時button数と照合する。Picoはこの間に観測ringを使わず、serverは対象の循環prehistoryだけを保持する。最初の実physical buttonでPicoの20秒を、最初にdecodeした入力でserverの20秒を開始する。後続buttonは期限を延ばさず、容量で早期凍結もしない。各端の時計域と開始時刻は別であり、同じ物理20秒窓とは主張しない。hostにUPが届かない場合、hostはARMEDのままで、明示freeze後のprehistoryに限界が残る。

この版での反復操作は最初の押下から20秒以内に限る。遅延が出た時点、または20秒で操作を止める。未再現なら保存・release後に再armでき、再起動や再書込みは不要。20秒後の押下は記録を再開しない。Picoは凍結後のphysical button数をLIVEに残し、読出しtoolがこれを検出した場合、最後に報告したbuttonをこのcaptureで確認したとは扱わない。

FROZENの記録は明示RELEASEまで保持する。USBのopen/close、DTR、STATUS、同一ARM再送、ページの再読は記録を消さない。凍結はRAM保持であり、電源断を跨ぐ永続化ではない。READは先にFREEZEしたデータだけを返す。RELEASEはboot・run・digest一致が必要で、直前RELEASEの応答喪失後も同じ要求を再送できる。異なるARMは未releaseデータがある間拒否し、release後も以前のrunを拒否する。runはboot内で単調増加する64bit値で、toolが両端のlast_runより大きい値を選ぶ。

## 制御と短い手順

実機候補を別途レビュー・承認して反映した後に行う手順。`firmware/tools/ack_probe_v2.py` は `--execute` がなければ計画の表示だけで、serialもsocketも開かない。serial、ローカルsocket、端末ID、保存先は毎回明示し、固定の実機宛先は持たない。

1. `prepare`：対象・boot・run・Pico PREPARED/未trigger/button数不変・server ARMED・同じ稼働sessionを照合する。最終server確認をUSB接続中に済ませ、LIVEを最後のendpoint確認にする。USBをcloseするとPicoの未開始gateがARMEDへ移る。成功時の `READY` と `ready.json` が出るまで押す合図を出さない。合図待ちは失効しない。途中失敗時は同じ保存先でprepareを再実行できるが、READYは確認できるまで出さない。READYは最終LIVE時点での未開始・準備完了とUSB切断gateの証明であり、以後の入力がなかったという無期限の証明ではない。最終確認後から切断までの入力もgate側で区別し、回収時に準備中入力のqualityを確認する。
2. READY後に反復操作し、遅延の再発時か最初の押下から20秒で止める。臨界区間のUSBは閉じる。
3. `collect`：入力を止めてから実行する。serverをfreeze後、collect開始から20.1秒を単調時計で待ってPicoの固定窓終了を保証してからUSBを開く。未開始の部分armは明示FREEZEで終了できる。ページを検証してpico.json/host.jsonをfsyncする。boot/run、全row、digest、欠測・上書き・準備中/凍結後押下・観測中USB接続の有無を保存する。記録中USBが観測されればUSBなしの窓として扱わない。応答喪失や部分保存でもcaptureは保持され、同じ読出しを再実行できる。
4. 保存結果を確認して `release`：保存済みファイルのSHA256一致を確認して両端をreleaseする。再現しなければ新しい保存先でprepareへ戻る。

呼出しの形：

```sh
python3 firmware/tools/ack_probe_v2.py prepare --serial <LCD-by-id> --socket <local-control.sock> --device <device-id> --dir <new-run-dir>
```

確認後に `--execute` を付ける。同じ引数で `collect`、`release` と進める。失敗を時刻経過や見た目で成功に読み替えない。prepareはidentity.jsonを先に保存するので、部分armの終了処理にも同じboot/runを使える。

PicoのUSB制御は `AP2 ID` / `STATUS` / `LIVE` / `WINDOW` / `QUALITY`、`AP2 ARM <boot> <run>`、`FREEZE <boot> <run>`、`READ <boot> <run> <from> <count>`、`RELEASE <boot> <run> <digest>`。IDとLIVEで端末・現在sessionを確認する。数値引数はhex、READは最大8件/ページ。128byte未満の有界command、16byte/main-loopの非待機入力、凍結ページは40行/秒で既存の部分書込みqueueを使う。ページが欠けても再読可能で、自動の全量出力や自動の再armはない。

serverはcompile feature `ack-path-probe`（default OFF）に加えて `PICOMADO_ACK_PROBE_DEVICE` と `PICOMADO_ACK_PROBE_CONTROL` がある場合だけローカルUnix制御を作る。socketは0600で、既存socketは上書きしない。専用threadのstack上限は256KiB。JSONの `status/arm/freeze/read/release` を受け、要求は512byte、応答は最大16record/ページ。旧版のPATH/SECONDSによる起動時一回captureは使わない。通常APIには診断操作を追加しない。

## 記録と資源

Picoは96×56byteの固定ringとcontrol用固定bufferを合わせ6KiB以下。RECORDING中の容量超過は最古rowを置換してoverを増やし、20秒より早く凍結しない。serverは512×64byteの固定ringと有界metadata。待機履歴の置換と観測開始後のrow欠落を別に数える。凍結後のページ以外に連続ファイル出力はない。toolはUSB読取り全体512KiB、各保存ファイル1MiB、各RPC/serial要求に有限時間を設ける。

新規hot pathにはheap allocationやファイル出力を入れない。serverの元wakerをそのまま使い、proxy wakerのArc割当は廃止した。wakeup時刻自体は観測しない。hookはtry_lockで争合を数え、待たない。STATUS等のcold処理との争合も欠測として見える。読み取り・digest・保存は凍結後に行う。Picoの既存stats/diagのDTR snapshot負荷自体は残るが、新captureのresetとは独立している。未開始gateだけはUSBに従いPREPARED/ARMEDを切り替えるが、RECORDING/FROZENの記録や期限は再開始しない。

Picoは各接続世代と全送信メッセージ通番を保持し、JSON CRC・長さ、既存session/frameと照合する。新event23 `TX_POSITION` はtcp_write(COPY)成功後、output前の `a=snd_lbb-n, b=snd_lbb, c=snd_nxt, d=lastack`。a/bはこのWS送信のTCP byte範囲[start,end)で、32bit wrapを考慮する。event17はoutput結果/nxt/lastack/sndbuf、1Hz event20はnxt/lastack/cwnd/snd_wnd。flagsはbit0=PCB有効、bit1=unsent、bit2=unacked、bit3=NODELAY。既存SDK lock内で定数個の値を読むだけで、segment走査や新たなpoll/outputをしない。

serverは受理済み現行sessionだけをaccept順で認定し、IDLE中も対象接続を登録するのでarmに再接続は不要。旧closeは新しいbindingを消せず、未認定・拒否Helloは対象を奪わない。認定争合は次の正常メッセージで再試行する。Decode/ProcessのordinalとCRCはPicoと対応し、raw-poll/readは同じconnectionに付く。途中arm後の最初のmessageでもDecoderの通番を接続の現位置に合わせる。別deviceのpayload CRCは計算しない。

## 分離できる境界と残る不足

- Pico内のbutton→send/COPY/outputと、送信byte範囲に対するnxt/lastackの進行を同じMCU時計で比較できる。TCP serial-number arithmeticで `TCP_SEQ_LT(snd_nxt,end)` が続けば、その送信の全byteがTCP outputへ進んでいない。output ERR_OKとNODELAYだけではunsent解消やpeer到達を保証しない。unsigned大小では比較しない。例えばstart=0xfffffff0/end=0x20/nxt=0xfffffff8は未到達で、`(int32_t)(nxt-end)<0`として扱う（区間は2^31byte未満）。lastackがendに達する意味はそのTCP peerによる受理であり、HOMEならserver TCP、AWAY/SOCKSならproxy TCPで、後者をserver kernel到達とは扱わない。
- host内のraw read、WS/decode、process_begin→doneを同じhost時計で追える。ProcessDoneは呼出し全体なので、既存message_processedで計測終了後に行うlogの待ちも区間に含む。
- `raw_ready` はsocket readが返った時刻で、kernelへのpacket到着時刻ではない。raw readは複数のWS/messageを含み得るので、最寄りのraw_readyを特定JSONのpacket着信と断定しない。
- 両端のboot/run、接続世代、ordinal、kind、JSON CRC/長さ、既存tokenで同じmessageを結ぶ。時計は同期しない。MCUとhostの絶対値は引き算しない。
- TCPにenqueue済みでも無線/driver/TCPのどこで待ったか、host kernelに既着でreaderが未pollなのかは、この候補だけで必ず分けられるわけではない。その区別が必要なら、同じhost connectionのRecv-Q/受信byte進行をhostの単調時計で別採取する必要がある。今回はkernel観測を過剰追加していない。packet captureやSSHも実行していない。

低メモリ検証は固定容量の満杯・置換・有界readと、SDKのERR_MEM/queued bytes保持を対象にする。実機の動的heap/stack高水位やMCU CPU負荷はホスト試験から保証しない。`tcp_write(COPY)`成功後のoutput errorを単純なアプリ再送へ変更すると重複し得るため、送信方針の修正は含めない。上書き/争合/不完全読出しや凍結後入力がある場合、欠測を「ACK未生成」の証拠にしない。

保存は同じdirectory内の専用一時ファイルへ全量write→file fsync→hard linkで最終名へatomic no-overwrite publish→directory fsyncする。途中のstagingだけが残っても再実行できる。既存の異なる最終ファイルは上書きせず拒否する。旧方式や外部操作で最終名自体が破損していた場合も勝手に置換しない。
