# LCD 固定画面の最小再現

同一の全画面（白枠、RGB帯、`PICO MADO` / `FIXED FRAME`）を Wi-Fi 接続前6回、接続後500msごとに描く。画面の位相表示やカウンタは変えない。窓 X0..159/Y24..103、180度回転、色補正は個体configの値を維持。LCD専用でE-inkはビルド拒否する。

上/下ボタンは SPI の要求速度を 1/5/20MHz で変更し、制御と画像に同じ値を設定する。電源ボタンは同条件でLCDをリセット、サブは同じ画面を再描画する。USBログは要求/実速度、clk_peri、CR0/CR1/CPSR、SPI API戻りバイト数、送信バッファCRC、位相、ボタン番号を出す。資格情報を出さない。API戻り値はLCD読戻しではなく、目視結果は別途必要。

個体IDとFlash unique IDを既存端末ファイルで照合し、違うボードまたはsampleビルドではLCD/Wi-Fiを初期化しない。UF2に既存home資格情報が入るため、ビルドは `umask 077` で行い、Git除外の `firmware/build/` へ保存する。新しいネット設定は作らない。`PM_REPRO_WIFI_HOME=ON` のときだけ既存home設定で15秒以内に接続を試す。サーバへ接続しない。

```sh
umask 077
cmake -S firmware/diagnostics/lcd-repro -B firmware/build/lcd-repro-sdk211 \
  -DPICO_SDK_PATH=/home/tane/pico/pico-sdk -DPICO_BOARD=pico_w \
  -DCMAKE_BUILD_TYPE=Release -DPM_REPRO_DEVICE=PicoMado-lcd1 \
  -DPM_REPRO_SAMPLE=OFF -DPM_REPRO_WIFI_HOME=ON \
  -DPM_REPRO_YOUTUBE_CYW43=ON -DPM_REPRO_SPI_BAUD=5000000
cmake --build firmware/build/lcd-repro-sdk211 -j4
```

この版は実働YouTubeのSDK2.1.1と診断CYW43ドライバを読み取り参照し、同じbackground方式を使う。USBログ、固定画面、個体チェック、DHCPは最小再現の差。background方式と互換のない `MEM_LIBC_MALLOC=1` は再現用ヘッダで0へ切り替える。本体のpoll用lwIP設定は変更しない。

SDK2.3.1版は別buildフォルダに `-DPICO_SDK_PATH=/home/tane/dev/PicoW/pico-sdk -DPM_REPRO_YOUTUBE_CYW43=OFF` で作る。この場合SDKとCYW43ドライバ両方の差があるため、SDKだけの原因判定には使わない。速度だけの比較は一つのUF2内の上/下操作で行う。

本体LCDの低速制御候補は既定で制御1MHz・画素は個体configの20MHz要求。実速度はRP2040の分周で決まる。最小再現はこの既定を上書きし、制御/画素を同一速度にする。低速化の実効は実機確認待ち。

2026-10-12ホスト試験では、実 `lcd.c` の制御/画像速度、CS、窓、全25,600バイトを検査し、固定画面の繰返しと1/5/20MHzでpayloadが同一（CRC `20ac31e9`）を確認した。stubは電気波形と可視表示を検証しない。
