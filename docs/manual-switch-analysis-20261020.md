# 2026-10-20 LCD切替の実測と観測不足

対象はPicoMado-lcd1、session2282。受入済みruntime `52d8fdd` と同じcodeの `8ef0919` から、既存全診断flagsを保ちEV1を追加したLCD UF2と観測serverを使用した。利用者の申告は23:08:12 UTCの「時計からYoutubeで気持ち遅延」。従来の数秒遅延と同じ事象とは断定しない。

停止・両ログ保存・通常server復帰と3端末の受信／描画ACKは23:09:20 UTCに確認済み。設定差分0、元flags保持、DB巻戻しなし。以後の追加再起動、USB再採取、再フラッシュ、SSHはない。

## 実測

| 切替 | cause op / 初回frame | Pico入力受付→描画 | Pico通知受理→応答API | server入力処理 |
| --- | --- | ---: | ---: | ---: |
| 風刺画→雨データ | 1906 / 914701 | 163.420ms | 97.862ms | 67.606ms |
| 雨データ→天気 | 1921 / 914702 | 470.985ms | 405.373ms | 47.196ms |
| 天気→時計 | 1936 / 914703 | 640.259ms | 573.016ms | 62.485ms |
| 時計→YouTube | 1951 / 914704 | 1,578.827ms | 1,478.404ms | 22.226ms |

時計→YouTubeのPico内の加算できる内訳は、入力→通知受理1.310、通知受理→応答受付1,478.404、応答→FrameStart34.865、CRCまで37.551、描画開始待ち7.384、描画19.313ms。全て同じPico bootの時計。入力はdebounce後であり、送出受理はRFやTCP ACKではない。

サーバはAPI Received後のlock待ち1.501、選択commitまで20.592、初回動画Frame Pendingまで39.925ms。全入力処理22.226ms中、永続化18.411ms。返信sink0.086、初回frame転送0.289ms。これらは重なる区間を含むため加算しない。静止画のFrame PendingはApp登録点、動画はwriterの転送準備点で、どちらもsocket送信完了と同一ではない。

## 確定した境界と条件付きの所見

`main.c::send_simple`から`net.c::send_raw`までの通知処理は1.035ms。TCP_WRITE_FLAG_COPYが成功してoutputを呼んだことは分かるが、output_resultは既存成功判定へ使われない。unsentが残ってもbool=trueとなる経路があり、直ちにnetif／CYW43／RFへ出たとは言えない。この呼出し自体の長いSDK待ちは今回の1.478秒を説明しない。

今回は全4操作で返信送信→初回frame、Picoでも返信API→FrameStartの順。特に動画writerの同一stream順序とserver Source事実から、**decode後のAPI Received→返信sink完了は22.226〜39.925ms、→初回frame sink完了は39.925〜66.022ms**と区間を置ける。exactなenqueue/dequeue／送信終了factは未採取。従ってAPI受付後のサーバApp／writerが1.478秒の大部分を待った説明は除外できる。server socket/read/decode前はこの上限の外。

同一PiのUSB受領 `host_mono_ns` とserverのprocess入口SystemTimeを、journalのREALTIME−MONOTONICの観測幅10us／1,921標本で対応させると、USBが送出終了事実を受けてからserver処理入口まで、時計→YouTubeは1,384.036〜1,385.046ms。**未記録のclock stepがなく、offsetが観測幅内という条件付き**。Pico時計をserver時計へ変換した値ではない。USB受領はevent発生より遅いため、正の値は残時間の下限で、片道RF時間ではない。

他の3操作の同じ下限指標は−59.672〜−58.662、269.296〜270.306、423.050〜424.060ms。負値はserver処理がUSB報告より先だったことを示す。この所見は長い部分をserver API受付前へ絞るが、PicoのTCP／driver滞留、HOMEネットワーク、server kernel／task／read／decode待ちの区別は未確定。

AP2／TX／CYはcompile ONだが今回はARM・exportしていない。そのため当該Upのtcp_output結果・TCP seq/window、CYW43 WAIT／CRCの証拠はない。古いrunから補わない。LCDのNagle無効化はコード上存在するが、その送信時の実pcb flagsは未保存。保存net statusはHOME、join時のcached PM readbackはnone=1であり、全期間のRF正常性を証明しない。

## 885件の観測欠測

期待seq5,988に対し5,103事実を保持。欠番合計とsource drop増分885が一致し、host queue/write欠落は0、保存durable。source counter非飽和の現コードでこれを増やす条件は、`pm_observe`が共有queueの16枠閾値で予約を拒否すること。端末の観測queue満杯と特定できる。

排出はmain loop末尾で最大1record／write、partial保持。先に最大8messageを処理し、動画描画は同期。producer/drainの不均衡に合うが、どのloop／USB／IRQ負荷が閾値到達を作ったかは未測定。OFFは生涯counterやnegotiated済みのcontrol予約policyをリセットしない。

4回の入力→初回描画はseq1906–1920、1921–1935、1936–1950、1951–1969で連続し、欠測増分0。最初の欠測seq1984はその後の動画中。正の区間値は示せるが、global incompleteと正規解析4件のincompleteを変えない。最後の動画標本は839受信／830描画／8dropで、885の観測欠測とは別の定義である。

## 仮説と次の区切り

残る候補はPicoのoutput後unsent／再送／driver待ち、HOME経路、server API前待ち。受信済みreplyをPico mainが長く放置したことだけを主因とする説明は、同一Pi時刻対応の下限所見に合わない。ただし小さい後段待ちは残る。SDK破損、radio、Nagle、server schedulerのいずれも根因確定には至っていない。

[修正前契約](observation-runtime-gap-contract.md)と[検証条件](observation-runtime-gap-verification.md)を先に定義した。runtime修正は未実施。既存wire、bool結果、返信／画像の順序許容、ACK、個別設定と診断flagsを保ち、最小の受信／queue／送出境界と観測排出の検証から始める。

## 証拠の所在

作業証拠は `architecture-refactor-20261020/observation-live-20261020T230517Z/analysis/` の `REPORT.md`、`BOUNDARY-FOLLOWUP.md`、`all-switches.json`、`boundary-followup.json`、`Pi-timebase-mapping.json`、`boundary-code-references.json` と `operation-<op>-{device,server,journal}.json`。個体の秘密設定・生ログはGitへ入れない。

共有可能な数値と証拠SHAは [checkpoint](checkpoints/20261020/manual-switch-observation.json)。serverログSHAは `75f90dd64bab35f5659f05e2c388ced4b5ae8d7d3049559e706aebe3bc0045e2`。時計差分の規則・条件は各JSONにも保存した。
