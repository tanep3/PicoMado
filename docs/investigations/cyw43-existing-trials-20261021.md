# 既存CYW43 / AP2とTCPヘッダの照合（2026-10-21）

既存の書込み済みファームでCY1/AP2/TX1を利用できた。EV1 OFF、単一USB ownerの有限SIMを5回実施し、Pico入力→通知→サーバdecode→対応frame受信→描画完了を保存した。画面切替は140.427–170.673msで、今回の5回では秒単位の画面停止は再現していない。一方、天気→雨データの約9秒後のheartbeatに、初回を含め4送信がPiで観測されない事例を得た。画面停止の解消や全事例の同一原因とはしない。

## 実施範囲と品質

書込み済みLCD fd19ace94a80c0baf9ad97ed9e97255aca563045ee428efd1ac7aad52a2d8e3a、通常server e7b52c29d776bf9b9fbfea9125216c7b5c05613f47fadc9660618dedb76c727c、PID1780574を維持した。今回firmware書込み・server切替/再起動・E-ink/ai-news/OS/network変更・SSHは行っていない。

AP2 ARM直後の接続状態はPREPAREDで、単なる接続中PRESSでは内部窓を開始できない。既存SIM BEGIN/STEP/COMMITを使い、同じTIOCEXCLのFDを開いたままDTRだけを下げ、有限1step（hold150ms）と最長job15s＋観測窓20s＋guardを含む35.6s待った。owner FDのclose/reopenや別readerは挟んでいない。待機中はUSB read/writeを行わず、対象LCDのexact TCP tupleに対するIP/TCP固定ヘッダのみ受動採取した。最大40s、実採取約35.6s、queue sampling gap≤200ms、kernel drop0、全packet scope valid、payload保存なしを確認した。

これはSIMの入口であり、物理button IRQ/debounceの検証ではない。AP2 synthetic job/stepと通知のsession/message/kind/fingerprint、frame IDで因果を確認し、SIM acceptedだけを描画成功にしていない。USB観測重複0、物理counter不変、P/H上書き0、TX/CY欠測・世代変更0、25 TX全件がCYへ一意joinした。P/H/TX/CYのdigestを再計算し、証拠SHAも保存した。87項目の証拠照合が成功した。EV1の過去のloss counterは累積保持されており、今回のEV1 OFF窓の欠測値として転用しない。

| 試行（証拠root A/B） | 切替 | Pico入力→描画完了 | CY行数 |
| --- | --- | ---: | ---: |
| A/20261021T023416Z/inner-trial-01 | 時計 → 天気 | 140.942 ms | 5 |
| A/20261021T023416Z/inner-trial-02 | 天気 → 雨データ | 163.670 ms | 7 |
| A/20261021T023416Z/inner-trial-03 | 雨データ → 風刺画 | 140.427 ms | 4 |
| B/20261021T025006Z/inner-trial-01 | 風刺画 → 雨データ | 162.221 ms | 5 |
| B/20261021T025006Z/inner-trial-02 | 雨データ → 風刺画 | 170.673 ms | 4 |

A: CY-probe-feasibility-20261021T023416Z、B: CY-inner-followup-20261021T025006Z。各rootは本task architecture-refactor-20261020直下。入力と描画の差は同じPico uptime clockだけで計算した。ホストとPicoの絶対時刻の単純差分は使わない。初期5試行のCRC計測は1run283–464µs、1CRC最大49–50µsで、これ自体はobserver effectやRF中立性の証明ではない。

## 再送の根拠と識別限界

A/inner-trial-02、run18e06b3f2461a56a、SIMjob14（decimal）、heartbeat message236/kind5。AP2 COPY範囲[TCP SEQ57920,58019)、TX/CYは同じSEQ/end/ACK/IPID/長さとPico時刻包含で結び付く。元keyとbus直前keyは一致、初期化済みBDC＋Ethernet157B全域のCRCは入口とbus直前で一致した。CRC一致は衝突のない数学的同一性証明ではない。

| IPID | PCB nrtx | 初回TX呼出しからの差（Pico時計） | linkoutput所要 | credit待ちµs / poll回数 | Piの同一packet |
| --- | ---: | ---: | ---: | ---: | ---: |
| 493 | 0 | 0.000000 s | 528 µs | 0 / 0 | 0 |
| 494 | 1 | 1.134452 s | 472 µs | 0 / 0 | 0 |
| 495 | 2 | 4.134387 s | 456 µs | 0 / 0 | 0 |
| 496 | 3 | 10.134548 s | 485 µs | 0 / 0 | 0 |

4件ともCY flags25（FULL/BUS/DONE）、wait/poll/RX読出し0、元bus API戻り値0、既存linkoutput戻り値0。今回の4送信はcredit待ち中pollによるbuffer変化の枝を通っていない。common入口→元bus書込み直前のkey/CRC変化は観測されない。この送信に限れば、既知の「credit待ち中共有buffer変化」を支持しない。

Piの約35.6sのヘッダ区間は欠落/範囲逸脱なし。IPID493–496は記録にない。その後のSEQ58019..58118/IPID497は先に到着し、PiはACK57920のまま返す。SEQ57920..58019が初めて現れるのはIPID498で、Piの初回切替通知packet観測から31.304443s後、そこから74.963µs後に累積ACK58118が出た。この区間はPiのkernel realtime同士で比較した。Pico窓は20sで終了するため、IPID498のCY内訳・ACK受信処理は未観測であり、同じ性質だったとは推測しない。

したがって、このheartbeatの欠落をPiアプリが到着済みdataを長時間読まないことだけでは説明できない。bus APIの成功は無線送信完了・AP受領・TCP ACK到達を保証しない。元bus書込み処理の内部/低位driver、CYW43 firmware、RF/AP、Pi NICに届く前の経路は分離できない。既存観測には各試行のchip TX status、802.11 ACK、AP側の同一frame観測がない。これが最小の観測不足で、現行診断から根因を断定しない。追加firmwareやネットワーク設定変更には進んでいない。

前のEV1 OFF試験の雨→風刺画1.416–1.437sは[別報告](tcp-header-minimal-trial-20261021.md)のまま保持する。今回の速い5回とheartbeat再送を、その画面停止への直接の因果証明として結合しない。

## 期限切れ試行と終了確認

Aの4回目はBEGINからCOMMIT要求まで14.629sかかり、既存10sのstaging leaseを超えてCOMMITがinvalidとなった。DTR低下も入力dispatchもしていない。P/Hをfreeze/read/digest照合で保存し、fourth-recovery/saved-before-release.jsonのSHA確認後に正規RELEASE、両probe IDLEを確認した。同じCOMMITは再送していない。

追加root Bでは、ARP REACHABLE確認とobserver準備を先に済ませ、BEGIN/STEP/COMMITを最後に置いてleaseを消費しない順序に直した。2回とも正常終了した。全5回のP/H/TX/CYは保存後にRELEASE、最後にPico/host IDLE、EV1 OFF、同じ通常server PID/binary、USB owner終了を検証した。現在稼働serverを復旧先へ切り替える操作は今回不要だった。

## 証拠と次の設計判断

完全な再計算結果はA/analysis/results.json、validation.json、evidence-sha256.json、再実行可能なA/validate_inner_analysis.py。各trialのidentity/pico/host/extra/cyw43/headers/observer-result/saved-before-release/releasedが原証拠。B/inner-cleanup.jsonが終了状態。checkpoint [cyw43-existing-trials.json](../checkpoints/20261021/cyw43-existing-trials.json)にも集約した。

容量対策は[資源再配分比較](../design/observation-resource-reallocation.md)へ分離し、heap8KiB下限・必須境界・loss/unknownを維持する。AP2とEV1記録の排他overlayは計算上heap12600Bだが、同時採取禁止というtradeoffの独立レビュー前で、実装・ビルド・flashは保留した。
