# LCD ACK非依存動画 — 2026-10-15

基準commitは `5224c30b8769fc4bd4489dd7751425ff66b73de1`。この追加変更は2026-10-15の統合コミット対象。LCD動画だけに `lcd-push-v1` を追加し、`lcd` と `frame-bytes-v1` も申告する端末に限定する。静止画、旧LCD、E-inkは従来のフレームとACK契約を維持する。

サーバーは既存の共有monotonic再生位置から過ぎた位置を飛ばし、各端末に最新画像1枚をwatchで保持する。writerが取り出した画像はimmutableなまま全断片を送り切り、送信途中を置き換えない。未送信画像は次の最新版で置き換える。writer/readerは端末ごとに独立し、制御応答は完全なWS/applicationメッセージの境界で送る。ネット待機中はApp mutexを保持しない。動画画像と1秒ごとのStreamStatusはSQLite/outboxに追加せず、描画ACKを送信creditに使わない。静止画のACK監視は維持する。

画素はサーバーで送信中1枚＋未送信最新1枚（各25600B）。WS write bufferは32KiB、各packetは約4.2KiB以下、Linux SO_SNDBUFは32KiB要求（Linux上64KiB、metadataを含む）に固定する。設定に失敗した接続はshutdownして受け付けない。カーネルへ既に入ったデータは破棄できない。2秒の断片stall/5秒の全画像送信期限では当該接続全体を閉じて復旧し、途中画像の残りを別画像にすり替えない。TCPの再送とhead-of-line遅延そのものは残る。TCP_NODELAYの変更は含めない。

LCD mainは組立中25600Bと描画待ち/描画用25600Bを持つ。IRQはRX transport bytesのみ書き、mainの描画中pixelsを上書きしない。世代/owner/session、順序とCRCを通った最新の完成画像だけを、ローカルmonotonic時計に従って描く。期限は遅れが1周期未満なら前回位相に追従し、1周期以上なら現在時刻から再設定するため、遅延直後に1周期より短い描画間隔が生じ得る。入力処理は毎main loopに残し、ローカル選択/待機要求後の未描画画像を抑止する。新静止画へ戻ると通常のReceipt/Rendered ACKを再開する。

通常ビルドはtrace clockを有効にする既存CMake構成を使用する。JSON workspaceを18000B、application RXを18006Bに縮小し、連結したWS bodyをapplication message単位で順に取り出す。旧4096画素byte配列JSONにも足りる。TCP RXは32768Bのまま。

6/24/30fpsの模擬試験はスケジュール・上書き防止・latest-only破棄と定常メモリを確認する。raw pixelsだけの必要量は6fps=153600B/s、24fps=614400B/s、30fps=768000B/s。実際のwireはヘッダ分増える。30fpsの無線・実描画性能は未検証で、実測成功とは扱わない。ユーザーの6fps設定を変更しない。

比較基準は実際にeth0経路を確認した09:25:37〜09:28:37の6fps：949 Rendered ACK、5.2807fps、描画間隔median164.1ms/p95 209.9ms/max2.659s、再接続なし。最大停滞はPico SPI約16msではなく、TCPへの投入からサーバー着信までにも存在した。前日のWi-Fi値とは直接比較しない。

最小の復旧資産は基準LCD UF2 `build/.rollback/5224c30/PicoMado-lcd1.uf2`（SHA256 c5c9fc741c94cc77fbdde036548fc17e905a9637d0313d4d29692f5db2ab1d56、720384B）と、本番反映直前に確保する旧server/config。秘密値を含むconfig/UF2/ELF/manifestはGit除外・0600。既存資格情報の新送信先追加、OS/ネットワーク/デスクトップ設定変更、SSH、E-ink書換えは行わない。

実機ビルド：LCD UF2 724480B、text368284B/BSS223976B、main RAM残32928B。SHA256 `4d7def9e0ab6da3c90a6e82404787bcd24140a08c71ab10cc75e9c4c2ee69e31`。書き込み前check-onlyで単一Flash ID `E661640843239D21` とmanifest/hash一致を確認。書込・本番反映・実測結果は完了後に追記する。

## 実施結果

サーバー全targetは182成功（lib177・設定1・integration4）、失敗0・filtered0、外部HTTPS opt-in1件のみignored。ACKなし実WebSocket、旧LCD/E-inkのcredit維持、動画/統計の非永続化、世代変更時の破棄、遅いwriterの開始済み画像CRC保持・latestへの移行・別端末とcontrol replyの独立を含む。初回の外部HTTPSテストバイナリ起動にOS EACCES、途中の既存子プロセス終了試験に1回失敗があったが、最終全試験はその両方を含め通常実行して成功。既存試験の除外や変更はしていない。clippy全target -D warnings、fmt --check、Release feature付きは成功。

ファームは27実行PASS。6/12/18/24/30fpsを模擬したローカル時計、描画領域の上書き拒否、100枚の受信から最新1枚、世代・CRC・静止画ACKへの復帰・入力後の描画抑止・連結した26KiB WS bodyの18KiB workspace内処理を確認。旧mainのLCD/E-ink_0/E-ink_1も3構成PASS。E-inkファームは今回転送していない。

本番反映は 2026-10-15T10:23:14.773544+09:00、PID236178。server SHA256 `e0b425f6dbba94f2cf6f38d367c8232b9cb4b439647e1ba4433ba7a5639c5b67`、15406112B。configとunitは不変、caricature-retention-delete featureと実env=1を維持し、API200。旧server/config各1ファイルを `service/target/rollback-5224c30/` に0600保存した。DB/OS/network/desktopは変更しない。

LCDは新BOOTSEL連絡の後、10:23 JSTに単一Flash ID E661640843239D21へ上記4d7def9e UF2をload、Flash verify100% OK、起動。実Helloでlcd-push-v1を確認。HOMEと実DHCPアドレスへのeth0経路、6fps設定を確認し、設定操作を行っていない。E-ink_0は旧能力buttons0/eink・風刺画だけONで再接続し、frame70038のReceipt/Renderedと継続heartbeatを確認。旧ai-news-http.service PID2081とai-news-generate.timerも継続。

採取は 2026-10-15T10:23:41.589116+09:00〜2026-10-15T10:27:11.662212+09:00、210秒。Pico統計の比較区間208.587秒で受信1246枚・描画1133枚・latest置換等の破棄113枚、**5.432fps**。全StreamStatusがaccepted、画像ごとのReceipt/Rendered ACKは0、再接続・write stall・USB受信/送信エラー0。USBトレースのdropped counterは[12, 12]で増えておらず、接続前の12件は測定中の欠落と混同しない。

最後の約180秒の実SPI完了972枚／5.398fps、描画間隔median 166.742ms・p95 336.074ms・max 946.969ms。全期間のPico集計maxは946.971ms。従来の5.281fps／p95 209.9ms／max2659msに対し、今回の平均fpsと最大停止は改善したが、p95は悪化し、一定の6fpsという受入には未達。描画ACK遅延が送信を止めないことは実機で確認した。TCPの揺らぎと端末のlatest破棄/描画位相は残る。

サーバーCPUは1コア換算median 2.91%／p95 3.97%／max 4.89%。RSS median 34.43MiB／max 36.18MiB。実ソケットSO_SNDBUFは全サンプル65536B。基準はCPUmedian26.9%、RSS204.7MiBだったが、RSSは今回が再起動直後なので全差分を方式変更に帰属させない。

24/30fpsの実機測定、長時間連続運転、物理ボタン操作の実機受入は未確認。ユーザー離席中のためFPS変更やボタン操作依頼はしていない。最新変更は未commit/push。収集プロセスは210秒で終了しUSBを閉じた。証跡は `task-3/eink-evidence/wired-20261015/lcd-push-v1/`（summary.json、service-tests/host-tests/clippy/release、deployment.json、lcd-flash.txt、USB/journal/TCP/load、compat-live.txt）。

比較上の留保：基準5.2807fps/最大2.659sはサーバーへのRendered ACK着信、新値5.432fps/最大0.947sはPico側実描画集計なので、測定位置は異なる。保存済み基準USBを同じSPI完了で再集計すると952枚/179.987秒=5.284fps、median164.973ms/p95 205.397ms/max2592.154msだった。ただし基準trace dropped counterは651969→652379（+410）で欠測の可能性があり、厳密な完全同条件比較とは扱わない。今回traceの増分は0。参考値でもp95の悪化が残り、滑らかな一定6fpsの達成とは報告しない。

## 機能時計とトレース時計の分離候補 — 未転送

トレースOFFではpm_trace_clock()が0を返すため、最初の動画フレーム後に描画期限が進まないバグを修正。mainの動画開始・描画開始/完了・統計の4か所だけを、トレースとは独立したSDK time_us_64()の機能時計へ切り替えた。video.cのペーシング/置換ロジック、FPS、通信/表示ドライバ、CMakeと全serverソースは修正前ハッシュと一致する。本番・端末への転送は行っていない。

実mainを含む同じ回帰で、修正前ONは成功、OFFはfirst_us==SDK時刻のassertで失敗を再現。修正後はON/OFFとも32bitを超える時刻・2枚目の描画期限前/後・1秒後のelapsed_ms・完了時刻/最大間隔・静止画ACK/入力キャンセルを確認し、全28ホスト実行PASS。通常SDKビルド成功。OFFも既存SDK/core libraryを再利用し、全7applicationファイルをPM_TRACE_ENABLE=0でクロスコンパイル/リンクして検証UF2を生成した。CMakeの通常ビルドは従来のtrace=1のまま。OFF候補は標準flash.sh対象外の検証variantである。

現在、通常生成先 `build/PicoMado-lcd1/PicoMado-lcd1.uf2` は未転送の時計修正ON候補へ更新済み：724480B、SHA256 `9bc20fa7ca823551d813c3afa0ac082fafadafc9e1bfda94da801942af214e8c`、text368284B/BSS223976B。OFF検証候補 `build/PicoMado-lcd1/clock-trace-off/PicoMado-lcd1-clock-trace-off.uf2` は721920B、SHA256 `4e7162f9d4ad6f21f9b3ce5e3aadffbfec82acccb08eede7b9551e44d6909964`、text367084B/BSS218632B。両方0600・Git除外。実機で動いているのは引き続き4d7def9eの10:23書込版。新たなBOOTSEL通知まで再書込みしない。

6fps未達・p95の揺らぎと表示ペーシング方針はユーザーの見た目/ボタン評価待ちで変更しない。30fps実機と実空きheap/stack最高水位も未確認。今回の時計修正による滑らかさ改善を主張しない。追加commit/pushなし。証跡は `task-3/eink-evidence/wired-20261015/clock-fix/verification.json` とON/OFF build/host/beforeログ。

## 後続測定と統合時点

上記の未commit表記は各チェックポイント当時の状態。ユーザーの明示依頼により、現在のサーバ・共通ファーム候補・関連試験/要件を統合コミットする。時計修正候補9bc20fa7は引き続き未書込で、ソースのコミットを実機更新と混同しない。

ユーザー操作で24fpsと18fpsも採取した。異なる動画・区間のため設定だけの比較には使わない。24fps設定は209.475秒で受信17.58205fps・描画17.54863fps・置換7枚、18fps設定は209.043秒で受信17.58968fps・描画17.33615fps・置換53枚。どちらも切断0。USBのSPI完了観測は描画数に届かず、SPI p95を確定していない。

その後の全編スリラー12fpsは14:12:16～14:15:16に180秒採取し、LCD統計179.809秒で受信12.001624fps、描画11.996062fps、置換0・切断0。実変換キーと9863フレームを照合済み。USB未接続のためSPI p95は未測定。サービス方式変更、レスポンシブWeb、試験と本番一致は[サーバ検証記録](../service/youtube-media-verification-2026-10-15.md)を参照する。
