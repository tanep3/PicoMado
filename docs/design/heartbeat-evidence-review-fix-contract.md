# heartbeat証拠品質：追加P2の修正契約

独立レビュー `/tmp/picomado-heartbeat-impl-review-6_n04u_d/review-outcome.json` のP2 3件をこの文書に先行固定する。660583bに対する指示はこの3件だけ解除し、firmware/CY15/SDK/設定は変更しない。runner統合差分は別commitへ分ける。新しい実機/USB/サービス/SSH/push操作は行わない。

## 1. 実型のsession/frameを保持

StreamStatusはtoken.session_epoch/frame_idをidentityへ抽出する。正しいHB開始後の同sessionのStreamStatusやACKは継続記録する。sessionを持たないmessageの0を「実sessionが0へ変化した」と扱わず、sessionを持つと分かるkindの不一致を品質停止する。対象heartbeat自身は引き続き厳密なsession/番号/binding一致が必要。

試験は実WireMessageのStreamStatusをencode→Decoder→identity→Stateへ通し、同session継続と別session停止を確認する。未知/sessionless型も区別する。通常の受理/プロトコル動作・target ordinal基準は変えない。

## 2. Pi単調時計によるobserver包含

Pico時刻やhost probeの相対時刻をPi時刻へ減算・変換しない。controllerとheader observerの両方が同じPiのCLOCK_MONOTONICを使用し、以下のbracketを整数nsで保存する。

`start_request_before <= worker_started.clock.mono_before <= mono_after <= start_confirm_after <= DTR_low_before <= DTR_low_after <= required_end <= stop_request_before <= finished_after`

required_endはDTR_low_beforeから指定待機予算（最大16秒）+20秒+0.1秒で定まり、最低20.1秒以上。START確認前のDTR lowは拒否する。既存workerのstop_command正常終了を要し、その実終了がstop_request_before～finished_afterに入ることをend bracketとして保存する。time_limit/異常終了/未確認は包含成功としない。capture_secondsもこのbracketから得られる保守的な下限・上限と整合させ、1秒の正常summaryを受け入れない。start/end bracketがない旧ファイルはunknownとして不合格。

この方法はworker本体/filter/ネットワーク設定を変えず、同じpipeで受けたstarted/stop_command/終了だけから観測区間を囲む。端点の1msのduration量子化許容と、厳密なnsのbracket順序を区別する。summaryのdrop/sample gap/socket close/scope/countも検査する。fixtureの仮想時計を実機の識別能力には転用しない。

試験は正常36.1秒、1秒summary、早いtime_limit、START未確認、bracket欠落・順序逆転・違う時計名・DTRより遅い開始・required_endより早いstop・duration不整合を含める。runnerのDTR失敗・timeoutにも同じbracket/不合格規則を適用する。

## 3. 共通のディレクトリ証拠検証

CLI・収集完了・releaseは同じdirectory validatorを必ず通す。in-memoryのfact検査は保存成功の代用にしない。完全runはready/identity/pico/host/extra/headers/quality JSON、saved.json、headers.jsonl、recvq.jsonlを必須にし、saved identity、必須SHA set、サイズ上限、全SHA、frozen digest、observer包含と対象originを再計算する。部分保存・原本欠落・改変・異なるready・SHA set不足はevidence_confirmed=falseかつcomplete=falseとする。CLIは不完全で非0終了する。

overflowなど事実を失わず記録できた品質不良はcomplete=falseのまま保存し、evidence_confirmed=trueなら正規releaseできる。digest/identity/必須原本不備ではreleaseしない。準備失敗で開始していないrunは明示partial schemaで保存し、存在する所有RAMのdigestと未ARM端の状態、原本を検査する。このschemaは決してcomplete=trueにならず、無関係boot/runを解放しない。共通validatorがrelease対象digestを返し、runnerもその結果だけを使用する。

試験は5JSONのみ、savedなし、raw2種欠落/改変、required hash欠落、ready/manifest不一致、サイズ超過、quality改変、完全保存、保存済みoverflow、partial準備とrelease拒否を実CLI・API・PTYで確認する。shared validatorの拒否を例外で握りつぶさない。

## 成果物

firmware source/hash/UF2 `9d4f66d1…36479687` とRAM証拠を継承する。hostだけ修正後に全feature回帰とreleaseサーバbuildを更新し、修正前に完成したbinaryは旧候補として区別する。runnerにはSTART/STOP/controller bracketsと共通validatorを組み込み、契約・テスト・固定commitを提示する。
