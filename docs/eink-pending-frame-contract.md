# D-04 E-ink描画中の最新pending

正本は設計第4節の局所/サーバ世代、描画開始・完了ACK前の再検証、中断不能描画と待機の規則である。描画中Startの拒否を仕様へ昇格しない。LCD・動画・旧/native wireの既存契約を保ち、plugin名や表示内容で分岐しない。

## バッファ所有の確認

`display_eink.c::pm_display_frame`は受信画素を内部`queued[3813]`へコピーし、pollで`ram[4000]`へ変換してからSPI/busy進行を行う。mainのassembly画素を非同期に参照し続けない。したがって、mainの既存assemblyを最新の受信/pending画素に使える。追加のフル画素bufferは不要で、追加保持はtokenと更新metadata/状態に限定する。ドライバのqueued/active metadataはそれぞれの画素と結び、後続受信で現在描画のmetadataを変えない。

## 状態と外部作用

現在描画tokenと最新pendingを分ける。純粋なpending状態モデルは、受理したStart、CRC完了、scope失効、driver完了を入力に、保持・破棄・描画依頼を返す。SPI/ネットワーク/ログはモデルの外で行う。

有効な新Startの受理時は、前pendingを失効させてassemblyを新しい受信へ使う。不正headerは有効なpendingを壊さない。CRCまで完了した最新画像だけを描画候補とし、token、partial/full更新metadata、画素を同じ単位として保持する。受信ACKと描画ACKを区別し、描画開始待ちはAPI_PENDINGとする。部分画像・不正CRCを描画しない。

古い物理描画の完了時はそのtokenを検査し、新pendingを完了扱いにしない。現在scopeに合う最新pendingだけを次に描画し、同じ画像を二度dispatchしない。取消し・切断・待機・新session/local ownerでは旧pendingを破棄する。中断不能な物理描画は既存のdrain-before-standbyと局所画面復帰を維持する。

## 受入と範囲

純粋モデルと実mainのC試験で、描画中の新Start/部分chunk/CRC完了、複数世代の置換、不正header/CRC、旧完了、取消し・切断・再接続・待機、更新metadataの対応とAPI結果を検証する。2/4ボタンE-ink構成、native/旧chunk、既存driverのbase/partial更新試験を維持する。LCD回帰、native sanitizerとARMのRAM/stackを確認する。実パネル・ファーム書込みは行わない。

この契約はファーム側の非同期表示処理の修正対象である。D-01/D-02のサーバ変更のために追加ファーム更新が必要になった意味ではない。実機反映と物理的な残像・ちらつき・遅延の受入は別途とする。
