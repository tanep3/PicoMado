> 公開用注記: 本文のホームパス・ネットワーク値は匿名化した例示値です。時刻付き観測の原値や現在の設定先を示しません。

# LCD送信直前RAMの全量回収

診断専用 `PM_REPRO_CAPTURE=ON` で初回・2回目・7回目の25,600 Bを保存する。通常版とE-inkには回収処理をリンクしない。

最新の併合候補は `PM_REPRO_HOLD_FRAME_CS=ON` も指定する。同速度の描画ではCSを一回だけlowにし、CASET→RASET→RAMWR→全画素完了まで保持する。frame専用ヘルパーはDCだけを切り替え、CSを操作しない。最終 `spi_write_blocking()` のBSY待ちに加えて、明示的なBSY確認後にCSをhighへ戻す。初期化のcommand/data方式と、異速度通常版の転送は変更しない。診断は初期5MHz、上/下後も制御・画素が同速度なので、この保持経路を使う。

取得点は色補正/180度回転の後、同じポインタ/同じ長さで `transfer_pixels()` に入り、CASET/RASET/RAMWRを出す直前。別の静的配列へ全量コピーするだけで、この時点ではUSB出力・CRC計算を行わない。記録した時刻・copy_us・元RAMアドレス・frame番号・phaseを後で送る。SPIデータを出すまで元バッファを変更する処理はない。

観測への影響は、該当3回のmemcpyと時刻/metadata保存。`copy_us` は実機で測る。7回目の取得後、ホストがUSBへ `D` を送ると保存済み3枚をまとめて送る。この後送信中は描画が止まるため、その間の画面保持/周期を元の500ms反復と同一視しない。取得済み初回/2回目へ後送信の遅延は影響しない。従来のUSBログ自体の影響も残る。

転送形式は `LCDCAP BEGIN`、3組の `FRAME / DATA(offset+256B以下のhex) / END`、`DONE`。Pi側は連続offset、25,600 Bの完了、USB回収列のCRCを確認してから、**CRCだけで終わらず全バイト**を独立期待列と比較する。初回との全バイト差も記録する。欠落/重複/途中切断を拒否する。

独立期待列はRGB成分組から算術でRGB565BEを作り、固定画面仕様の座標を反転する。Cのパッキング/変換関数を再利用しない。字形bitmapの内容のみlcd.cから読む。画像再構成は回収した各2Bをbig endianで読み、反転＋赤青交換の逆変換を適用し、180度回転を戻してRGB8 PNGへ保存する。これは要求した変換の逆で、誤ったLCDレジスタ状態まで再現するものではない。

## ビルド（実機転送は別途root判断）

```sh
umask 077
cmake -S firmware/diagnostics/lcd-repro -B firmware/build/lcd-repro-sdk231-capture-held-cs \
  -DPICO_SDK_PATH=$HOME/dev/PicoW/pico-sdk -DPICO_BOARD=pico_w \
  -DCMAKE_BUILD_TYPE=Release -DPM_REPRO_DEVICE=PicoMado-lcd1 \
  -DPM_REPRO_SAMPLE=OFF -DPM_REPRO_WIFI_HOME=ON \
  -DPM_REPRO_YOUTUBE_CYW43=OFF -DPM_REPRO_SPI_BAUD=5000000 \
  -DPM_REPRO_CAPTURE=ON -DPM_REPRO_HOLD_FRAME_CS=ON
cmake --build firmware/build/lcd-repro-sdk231-capture-held-cs -j4
```

## 承認後の確認順

1. rootが差分/試験/UF2を確認し転送を判断。ユーザーのBOOTSELをこちらから再依頼しない。
2. 下記ホスト回収を待機させ、rootの指示で候補を対象個体へ転送する。回収はUSB serialの個体IDを照合し、1200baudを拒否する。ネット・サーバ経路は追加しない。
3. ボタン無操作で初回・2回目と接続試行後7回目を取得。`READY`後にホストが自動で `D` を一度送る。出力先は毎回新しい空の場所を指定する。
4. `frame-1/2/7-ram.bin`を期待列と全バイト比較し、対応PNGと期待PNGを確認。一致なら**取得時点の変換後RAMまで正常**。LCD受信や可視表示の正常とは扱わない。

```sh
python3 firmware/diagnostics/lcd-repro/capture_usb.py --collect \
  --board-id E661640843239D21 --output /tmp/picomado-capture-real-run1
```

ログからのオフライン再構成は `--from-log PATH --output NEW_DIRECTORY`。出力は権限を制限し、JSONに全バイト差数・最初の差のoffset/行/列を保存する。秘密資格情報を回収しない。対象は固定画素バッファだけ。
