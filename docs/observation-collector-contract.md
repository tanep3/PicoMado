# 継続観測の収集・区間確定（既存設計第8節の具体化）

第1段階 `6a2a361` のホストCLIは未受入である。本書は既存の非待機観測とC-04/C-05を具体化し、プラグインや端末の機能規則は変更しない。入力→描画の因果対応は別の契約単位で整備し、収集器の区間確定と混同しない。

## 接続とコマンドの所有

1. 一つのreactorだけがserialをopen/read/writeする。TIOCEXCLを取得し、115200 baud・DTR ONで開く。アプリの操作、SIMやAP2のarmは自動実行しない。試験は注入した擬似端末だけを使い、実USBを開かない。
2. Unix IPCは最大4 client、各request/replyは有限長、command待ちは8件、wire上は1件ずつ。部分送信を保持し、reader内でdiskやclientの送出完了を待たない。clientの切断で採取全体を終了しない。受理済み操作を黙って再送しない。
3. 旧コマンドにwire CIDがないため、返信期限超過後の旧コマンドはuncertainとして後続旧コマンドを拒否する。遅着を次の返信に割り当てない。収集器自身の区間制御は独立のtag付きEV1を使い、旧コマンドの不確定状態でもOFFの結果を確認できる。
4. 追加形式は `EV1 ON|OFF|STATUS <cid-hex>`。応答は `ev1 s <cid> <boot> <sequence> <dropped> <first_lost> <last_lost> <on> <saturated>`（ID/counterはhex、真偽は0/1）。最大値でも既存144B未満の返信枠に収める。従来の引数2個のEV1とAP2/SIM/TX/CYの形式・意味は維持する。cookieは採取制御の相関でありアプリ操作IDではない。
5. ON/OFFはownerだけが使う。IPCは通常のstatus/exportと明示された接続中操作を中継するが、DTR closeで始まる旧SIM COMMITは継続接続と両立しないためこのownerでは拒否する。既存の独立した有限trial手順は変更しない。

## 収集区間と欠測

ON応答のboot・sequence・dropを開始境界にし、OFF応答を終了境界にする。同じbootで開始より後・終了以下の全sequenceが順に届き、drop増分0、飽和0、decode/行あふれ/host欠測0、最後の保存fsync成功の場合だけ `transport_complete=true` とする。実機処理や対応描画の完了を意味しない。ON snapshot直後のIRQ記録がON返信より先に並ぶ可能性があるため、境界前の記録を最大32件保持して照合する。超過時は不完全とする。受信記録は境界前も捨てずjournalへ保存する。

CRC破損、過長行、seq重複/逆転/欠落、boot変更、不一致CID、採取境界の応答期限超過、切断、途中行、保存障害を別の理由として保持する。通常コマンドの返信不明はcommand_uncertainとして別記し、EV1区間の無欠測と操作成功を混同しない。未送信のqueue取消しはcommand_cancelled、部分/全量送信後の結果不明はcommand_uncertainと区別する。旧返信prefixは配送条件だけで、boot/run/cidと意味の検証は既存クライアントの契約を維持する。欠測を「処理されなかった」証拠にしない。clockの異なるPicoとPiの時刻を直接減算しない。

切断時は現在のsegmentを不完全として閉じる。再接続は明示した有限回数だけ、同じ指定pathへ行い、link epochを進め、EV1のONと新しい境界だけを確認する。通常の制御操作は再送しない。新bootは別source。後続segmentは完全性を独立に確認できるが、切断を含む採取全体を無欠測としない。再接続待ち・返信待ち・区間末尾・client待ちには有限期限を置く。

## 保存と終了

readerは固定長の値を最大256件の保存queueへtry-putする。専用workerがJSON化/write/定期fsyncする。ファイルは排他的create、0600、既定64MiB上限。queue満杯、byte上限、disk errorは欠測counterへ記録し、serialの読取りを続ける。producer側dropとworker側dropを別所有のcounterとし、相互の上書きを避ける。

終了は有限のOFF応答待ち→serial解放→workerの有限join→summaryの排他的保存。join期限内にfsyncが終わらなければ `storage_durable=false`、worker生存を明示し、mainがworker所有fdを閉じない。summaryと保存データを既存名へ上書きしない。`last_durable_host_sequence`と、欠測なしで連続保存されたprefixを区別する。受信、durable、transport complete、実機処理完了は別項目にする。

## 受入と回帰

- K-01：C実装のtag付き応答を最大64-bit値でも144B未満にencodeし、旧EV1/AP2/SIM/TX/CYのexport/digest回帰を維持する。
- K-02：擬似端末の実reactorでON→混在/断片化event→IPC返信→OFF→fsync→区間確定を通す。独立した第二readerを拒否し、clientの途中切断・低速読出しでも採取が進む。
- K-03：seq欠落/逆転、drop、CRC破損、過長行、ON前記録、応答不明、EOF、途中行、同boot/新boot再接続を注入し、全体不完全とsegment別結果を検証する。SIM等の変更コマンドを再送しない。
- K-04：保存queue満杯・低速disk・byte上限・write/fsync error・join超過を注入して有限メモリと失敗理由を確認する。完了条件を緩めない。
- K-05：旧コマンドtimeout後の遅着を誤対応させず、tag付きOFFと有限終了を維持する。既存controllerは変更しない。実USB、SDK由来stdoutの所有統合と実負荷は別の受入である。

この単位でD-05全体を閉じない。採取点、source/context、入力とframeの因果、ホスト解析、実ARM/機能回帰、実機負荷のうち未完了の項目は引き続き [追跡表](contracts-observation-design.md) に残す。
