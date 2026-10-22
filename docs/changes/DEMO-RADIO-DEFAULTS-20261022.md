# デモ既定: interference3 / roaming OFF

ユーザーが「ローミングOFF・干渉抑制3は今の時点のデフォルトとして、電源OFFでも有効にしてよい」と承認した。97360ecの入力修正と71b1e64の400 ms案内を保持し、LCD/E-ink_0/E-ink_1共通の起動方針としてこの2設定だけを再適用する。設定credential sector/NVRAMを書かず、既存SDK/PR155/PM NONE/Nagle/EEX必須/server/diagnostic flagsを維持する。

## 一つのSTA稼働区間で一度

STA有効化後の最初のWIFI→SERVER遷移で、既存PM設定の読戻しに続けて、型付き固定値interference3とroam_off1を各1回SETし、それぞれ別GETを記録する。関連付け後なので初期化で上書きされる前の値を成功扱いしない。LCD/E-inkの画像処理より前に共通経路を通す。

mainのSTA区間フラグが未試行の時だけ実行する。TCP/WebSocket/EEX再接続だけでは再SETしない。Wi-Fi再関連付けだけでSTAを無効化していなければ同じ区間。既存wifi_offでフラグを未試行へ戻し、実際のdisable/enable後の初回接続で再試行する。RESET/電源投入でもBSS未試行から始まる。試行は成功/失敗に関わらず区間内で一度で、無限retryや待機ループを作らない。

既存RADIO比較のbaselineやUSB要求IDを流用して上書きしない。固定setter部分だけを型付きSDK関数として共有し、USBのquiet操作、GET/比較/RESTORE契約は維持する。任意IOCTL/数値SET/API公開は追加しない。設定変更は初期接続中に行い、稼働中の入力/描画callbackから実行しない。

## 返却事実と失敗

interference/roamingそれぞれSET rc・別GET rc/raw・候補一致を保持し、既存USB snapshotログへ再掲できる。CDC status/応答長が未公開というSDK制約を明示し、w=0/候補一致をRF設定成功と呼ばない。SET失敗なら別GETが一致しても失敗を隠さない。未知値・不一致・読戻し失敗を保存する。失敗時も既存接続を進め、黙って成功認定したり、無限に設定し直したり、通信を待ち続けたりしない。SDKの同期IOCTL response期限は各500 msで、bus/lockを含む総時間のhard boundではない。既存の制約を悪化させる独自待機は加えない。

## 検証

実main/SDK fixtureでSTA接続前は無SET、最初のlink UPで固定2SETと2GET、同区間で再呼出しても無I/O、TCPだけ再接続で無SET、wifi_off→再有効化→link UPで再適用、SET/GETエラー・未知/不一致でも1区間1回・結果可視、通常接続を進めることを確認する。個別設定と既存flagsを保つ3候補をroot入口でbuildし、設定sector非包含・Nagle・EEX・SDK/private LLと入力修正の維持を照合する。実機/USB/flash/SSHはここでは行わない。独立レビュー・実読戻し・電源再投入確認は別担当へ渡す。
