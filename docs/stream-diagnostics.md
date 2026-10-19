# 動画の統合計測（未反映候補）

ACK非依存push、server送信中1枚＋未送信最新1枚、Pico組立中1枚＋完成済み最新1枚という方針は変更しない。画像採用・破棄条件、ローカル描画周期、断片2秒/画像5秒期限、PM修正、通信wire、端末選択/有効化/共通順序も変更しない。追加は任意の診断機能とホスト採取・解析ツール。

## サーバ

Cargo feature `stream-diagnostics` は既定OFF。ON buildでも `PICOMADO_DIAGNOSTICS_PATH` がなければ採取しない。設定された未使用のファイルをcreate_newで開き、専用threadが1秒毎にまとめてJSONLを書き、300秒で自動終了する。ファイル作成/worker開始が失敗しても通常サービスを継続する。フレーム処理の途中で追加printfやファイルI/Oを行わない。

固定容量128 event、全spanのヒストグラム、event理由別累積counterを持つ。producerはtry_lockで待たず、取得できなければ`contended`を増やす。古いeventの上書きは`overwritten`で示す。従ってこれらが非0のとき、server event列や集計は欠落を含み得る。静的collectorは18032B。export側の一時JSON/Vecと専用threadのstackは別で、画像枚数に応じて無制限に増えない。

記録するもの：publish/replace/clearとArc参照数、frame開始、packet送信待ち/完了、送信Err、断片期限、画像全体期限、制御返信とqueue待ち、frame全体時間、既存ログI/O時間、同期保存、共有Appロック待ち/保持、playback/work/fetch/clockの起床間隔。domain名とkind名、monotonic時刻とUTCの対応を各JSONLへ添える。ReplyErrorはwriterでsession付き1件だけ計上し、`a=1`は250ms期限、`a=2`はsink error、`a=3`はencode error。外向きの既存終了理由やタイムアウト値は維持する。

reader/writer/playback/work/fetch/clockの対象ロックを観測guardで包む。ロックの取得/解放順とpoison処理は従来どおり。診断mutexはI/O中に保持しない。計測OFF buildでは元のMutexGuardと空のinline hookになり、診断thread・ファイル・collector領域を持たない。runtime OFFとcompile OFFは区別する。

## Pico LCD

CMake `PM_STREAM_DIAGNOSTICS` は既定OFF。ON時の追加collectorのsizeof合計はホストで2853B。既存時間統計への合計時間追加は384Bで、パディングを含むLCD全体BSS増は3236B。SDK/表示driverは変更しない。

全ての成功した動画SPI完了間隔、動画SPI時間、完成画像の組立時間を固定32binへ集計する。間隔は測定窓のN回描画ならN−1標本で、USBを閉じた同一窓では再接続をまたぐ動画描画の空白も含む。動画以外へ選択変更した場合の空白も混ざるため、比較時は再生条件を固定する。部分画像・CRC不良は完成時間分布へ入れず、理由別counterと既存RX記録で扱う。

最大値と500ms以上/1秒以上の回数は全間隔から集計する。p50/p95/p99はbinの下限以上・上限未満の範囲として求め、個別間隔から計算した厳密なpercentileとは扱わない。最終binは10秒以上で上限なし。counter/合計の飽和はoverflowで示し、解析器はその場合にpercentileを出さない。

既存SDKロック内で約200ms毎にRX/app/credit/window/rcv_nxt、組立量、組立と完成/描画bufferを別々のframe/session/sequenceとvalidityで24枠に保存する。abort/reset/closeで失効させ、TCP生成世代とPCB有効性を独立に記録する。`rcv_nxt`の差分は、同じTCP世代かつ両端で有効な標本の間だけ32bit modularに解釈する。最初の動画開始後の通信異常/closeをtriggerに、約1秒後にring更新を停止し、異常前後の約4.8秒を保持する。全窓のヒストグラム/counterは続ける。従って全切断のPico詳細時系列ではなく、最初の異常の詳細＋全窓集計である。

背景callbackでは小さなエラーlatchだけを更新する。SDKロック下でmainが取り込み、collector集計・出力はmainが所有する。同時期のエラーは最初の理由を保持し、件数を合算する。callbackにparse/printf/待機/動的割当を追加しない。FINはtrigger value1、lwIP errorはuint32表現（負数は2の補数）。

USBのDTR上昇で集計をfreezeし、既存stats/RXの後に20ms刻みで出力する。DTR低下で次のUSB-off窓を初期化する。USB-on中の画像をこのoff窓へ混ぜない。hookの呼出し数/合計/最大時間、export時間、既存のUSB-on/off処理時間合計も観測する。自己計測はclock/observer自身を完全に含むCPU profilerではなく、実機での無影響を保証しない。

## 採取・解析

ホスト側の準備内容だけを表示する：

```
python3 service/tools/collect_stream_diag.py --plan
```

実測を承認された後だけ、PIDと新しい保存先を明示して実行する。/procは1秒、ssは2秒、collector自身のCPU/RSS/I/Oも採る。journalは既存記録を終了後に一度読み、PIDのstarttimeが変われば停止する。ss/journalのTimeoutExpiredは捕捉し、部分データとfinallyからのresult.jsonを残し`complete=false`にする。サービス操作、設定書込み、SSH、tcpdump、USB接続は行わない。

```
python3 service/tools/collect_stream_diag.py --pid PID --seconds 180 --out NEW_DIRECTORY
python3 firmware/tools/parse_stream_diag.py USB_JSONL --output RESULT_JSON
```

USB JSONLはtext断片を結合して解析する。空入力、trigger/end・bins・counter・sampleの欠落/重複、end.rowsと実行数、bin合計と標本数の不整合を拒否する。24個のUSB on/off処理時間総量を解析し、一部だけ欠ける出力は拒否する。全24個がない旧/単体fixtureは`metric_totals.available=false`と欠測一覧を明示する。schema1の画像IDは`legacy_ambiguous`とし、schema2と同じ意味に再解釈しない。7秒程度の期限付きUSB snapshotを前後に採り、窓中は閉じる想定。既存captureのstats complete判定に加え、この解析器のdiag end/全row確認も必要。

## 検証と留保

サーバONはretention featureを保った全213ホスト試験が成功。ON/OFFで実writerの部分進捗・2秒期限・5秒期限・sink error・制御待ちを確認した。Pico host runnerは48実行case（内部のPASS行を含めると60行）、全て成功。分布の全標本集計、閾値境界、飽和、再接続空白、USB freeze、最初の故障保持、callback latch→mainへの引渡しと解析器の不完全出力拒否を含む。

LCD SDK buildは既存のSDK/driver objectを再利用し、変更したapp/core objectだけを再コンパイルしてリンクした。OFFは現行と同じtext392764/BSS230872、ONはtext398516/BSS234108、linker上の余白22796B。OFFのmain/net/trace/videoは、生成テーブルの番号を正規化した逆アセンブルで従来と一致。ELF全体/UF2のhash一致は主張しない。

実行時heap/stack高水位、SDK内部のpending処理、ACKの実wire到達、Pico→スマホ区間のradio品質はこの候補でも直接測れない。静的linker余白と個別stack usageは実行時余裕の保証ではない。serverのssは中継側のTCPであり、PicoのWi-Fi区間と同一視しない。主に単一LCDで同一動画を固定して測り、frame/sessionと時刻幅でserver/MCUを突き合わせる。

今回、端末への書込み・本番server導入・サービス再起動・ネット設定変更・commit/pushは行っていない。FWとserverの検証済み候補をまとめ、実稼働採取を始める前に反映範囲をレビューする。

## 独立レビュー修正の再現

- 組立frame11/4096Bと完成/描画frame10を同時に保存する。rejectイベントで組立IDを変更しない。reset/close/abort時の失効、TCP世代更新、USB窓切替をC実collectorと出力解析の両方で確認した。
- deadline_kindは2秒/5秒のminを選ぶ時点で固定する。2秒を選んだ待ちの処理が5.2秒へ遅れた再現でもFragmentDeadline=1/FrameDeadline=0で、約3.2秒のDeadlineWake遅延を別に計上する。timeout値と外向き理由は変更しない。
- ReplyCapacityはchannel予約の空き待ち、ReplyQueueは予約後enqueueからwriter dequeueまで、ReplyDoneはその後の送信時間。約404msの滞留をReplyQueue約403msで検出した。ON commandは192B、OFFは184Bで、32枠のpayload差は最大256B（他のstack/allocator領域は別）。
- private writeエラーをencode/deadline/sinkに型分けし、失敗はwriterでsession付き1件だけ記録する。
- ss/journalそれぞれと両方のtimeoutをmockで再現し、host/journal/resultの保存と欠測表示を検証した。
- 空入力・trigger欠落・end.rows誤り・metric欠落/重複を負例として検証した。

## この一回のON候補で確定できないこと

一回のON書込みで5観測軸のデータを集める構成であり、5仮説の因果をすべて分離できる構成ではない。サーバ→中継のssとMCU側の進行だけでは、スマホ中継のbufferと下流Wi-Fi/無線の停止は混同されたままになる。中継側の追加証拠や条件を固定した対照が必要である。

hook/export/procの記録は診断負荷の一部を示すが、ONだけでは「診断がなければどうだったか」は分からない。runtime OFFとcompile OFFはコード/RAMコストが異なり、USB-on exportも別の負荷である。共有Appロックやログの全保持箇所を覆ってはおらず、同時刻に発生したことも因果の証明ではない。追加比較が必要でも、今回の許可なく書込み・反映・ネット変更は実施しない。
