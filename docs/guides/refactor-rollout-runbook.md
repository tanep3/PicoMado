# 次回の反映・退避・復旧・採取手順

## Current staging gate

Read [AGENTS](../../AGENTS.md) and [development process](../development-process.md) first. Before historical install commands below, run the root gate; stage the exact artifact and obtain independent parent approval. This does not authorize service/device operations.

```bash
python3 tools/project_gate.py check --out .gate
python3 tools/project_gate.py build --kind server --build-dir "$PM_STAGE/build" --out "$PM_STAGE/build-receipt.json"
python3 tools/project_gate.py record --build-receipt "$PM_STAGE/build-receipt.json" --kind server --artifact "$PM_SERVER_CANDIDATE" --out "$PM_STAGE/candidate.json"
python3 tools/project_gate.py preflight --candidate "$PM_STAGE/candidate.json" --review "$PM_STAGE/review.json" --artifact "$PM_SERVER_CANDIDATE"
# Offline only. --check-only still accesses USB.
bash firmware/tools/flash.sh --device "$PM_DEVICE" --verify-only
```

The worker never manufactures a production approval. Existing accepted binaries stay intact. Direct install/copy/picotool remains a bypass unless external permissions/repository protection enforce the route; those controls are not applied here.

**準備文書であり、以下は未実行。** 現時点の本番反映、実機/USB、サービス操作、SSH、pushの禁止を解除しない。D03B`52d8fdd`は独立43試験で受入済み。対象・操作の親からの指示後に、許可された段階だけを実施する。[状態一覧](../history/refactor-final-acceptance-20261020.md)の実機未検証を合格に変えない。

## 影響と反映単位

- サーバ更新は全WebSocketを一度閉じる。停止時間は退避量と起動時間に依存する。端末は再接続して新sessionになるが、選択・端末別有効化・共通順序・設定改訂を上書きしない。schema2を保ち、通常buildでは削除gate OFF。
- D01/D02/D03はサーバだけ。D04採用はE-ink firmware更新が別途必要。EV1の実機採取には対応firmwareと観測用server buildが必要で、通常server更新と自動で一緒に行わない。
- 本番Gitへ自動merge/cherry-pickしない。実行対象は固定したsourceとhash付きbinary。既存unit/configをtemplateで上書きせず、他サービス・SDK・ネットワーク機器を変えない。

## サーバ：buildから通常反映まで

同じbashセッションで段階ごとに実行する。下記変数は操作時に確認して設定する。`PM_RELEASE_STAGE`は新規の私有ディレクトリ、`PM_SOURCE_REF`は**独立受入済みの最終hash**。受入済みruntimeは`52d8fddfd301f7f79c092c1ceca10d3c9fbb30f6`。後続が文書だけなら、このruntime固定点をbuildできる。追加runtime是正があれば再レビューしたhashへ更新する。

```bash
set -euo pipefail
umask 077
PM_SOURCE=/home/tane/Documents/Codex/2026-10-18/task/architecture-refactor-20261020/work
: "${PM_SOURCE_REF:?set the independently accepted full commit hash}"
: "${PM_RELEASE_STAGE:?set a new private absolute staging directory}"
PM_UNIT=picomado.service
PM_BIN=/home/tane/bin/picomado-server
PM_CONFIG_ROOT=/home/tane/.config/PicoMado
mkdir -m 700 -- "$PM_RELEASE_STAGE"
mkdir -m 700 -- "$PM_RELEASE_STAGE/source" "$PM_RELEASE_STAGE/backup"
git -C "$PM_SOURCE" archive "$PM_SOURCE_REF" | tar -xf - -C "$PM_RELEASE_STAGE/source"
CARGO_TARGET_DIR="$PM_RELEASE_STAGE/target" cargo build --offline --locked --release \
  --no-default-features --manifest-path "$PM_RELEASE_STAGE/source/service/Cargo.toml" \
  --bin picomado-server
sha256sum "$PM_RELEASE_STAGE/target/release/picomado-server" > "$PM_RELEASE_STAGE/new-binary.sha256"
git -C "$PM_SOURCE" rev-parse "$PM_SOURCE_REF" > "$PM_RELEASE_STAGE/source.commit"
```

ここまでは停止前のbuild。`cargo install`や新規dependency installを行わない。失敗時は稼働binaryを変えず停止する。通常buildに`caricature-retention-delete`や観測featureを黙って足さない。

実際のunitのFragmentPath/ExecStart/WorkingDirectory/停止期限を、次のreadでtemplateと照合する。追加drop-in、別binary/path、未確認のworkerがあれば反映を止めて報告する。認証情報やEnvironment全体を出力しない。

```bash
systemctl --user show "$PM_UNIT" -p FragmentPath -p DropInPaths -p ExecStart \
  -p WorkingDirectory -p MainPID -p TimeoutStopUSec > "$PM_RELEASE_STAGE/unit-before.txt"
df -h "$PM_RELEASE_STAGE" "$PM_CONFIG_ROOT" > "$PM_RELEASE_STAGE/disk-before.txt"
du -sh "$PM_CONFIG_ROOT" > "$PM_RELEASE_STAGE/content-size.txt"
test -f "$PM_BIN"
test ! -L "$PM_BIN"
test ! -e "$PM_RELEASE_STAGE/backup/picomado-server.old"
cp -a -- "$PM_BIN" "$PM_RELEASE_STAGE/backup/picomado-server.old"
sha256sum "$PM_RELEASE_STAGE/backup/picomado-server.old" > "$PM_RELEASE_STAGE/old-binary.sha256"
```

容量・unit・復旧binaryを確認してから、承認された停止時間帯に進む。以後はサービス操作の許可が必要。強制killやUSB操作で代替しない。

```bash
systemctl --user stop "$PM_UNIT"
test "$(systemctl --user show "$PM_UNIT" -p ActiveState --value)" = inactive
test "$(systemctl --user show "$PM_UNIT" -p MainPID --value)" = 0
test ! -e "$PM_RELEASE_STAGE/backup/PicoMado.tar"
tar --acls --xattrs -cpf "$PM_RELEASE_STAGE/backup/PicoMado.tar" \
  -C /home/tane/.config PicoMado
sha256sum "$PM_RELEASE_STAGE/backup/PicoMado.tar" > "$PM_RELEASE_STAGE/content.sha256"
```

停止が完了せず子仕事も残る場合は退避・配置へ進まない。tarは設定・DB/WAL・全plugin素材/cacheを一緒に保存し、内容をGitや画面へ出さない。稼働中のDB本体だけをcopyしない。別途、停止中DBの整合snapshotと重要設定の比較用ファイルを作る。

```bash
python3 - "$PM_CONFIG_ROOT/data/picomado.db" "$PM_RELEASE_STAGE/backup" <<'PY'
import json, pathlib, sqlite3, sys
source = sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri() + '?mode=ro', uri=True)
out = pathlib.Path(sys.argv[2])
target_path = out / 'picomado-consistent.db'
target_path.touch(mode=0o600, exist_ok=False)
target = sqlite3.connect(target_path)
source.backup(target)
assert target.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
schema, body = target.execute('SELECT schema_version,body FROM app_state WHERE id=1').fetchone()
assert schema == 2
state = json.loads(body)
critical = {'settings': state['settings'], 'order_revision': state['order_revision'],
            'plugin_order': state['world']['plugin_order'],
            'devices': {key: {k: value.get(k) for k in ('incarnation','selected','enabled')}
                        for key, value in state['world']['devices'].items()}}
with (out / 'critical-state.json').open('x') as saved:
    saved.write(json.dumps(critical, ensure_ascii=False, indent=2)+'\n')
target.close(); source.close()
PY
cmp -- "$PM_BIN" "$PM_RELEASE_STAGE/backup/picomado-server.old"
test ! -e "$PM_BIN.new"
install -m 755 -- "$PM_RELEASE_STAGE/target/release/picomado-server" "$PM_BIN.new"
mv -T -- "$PM_BIN.new" "$PM_BIN"
systemctl --user start "$PM_UNIT"
systemctl --user is-active "$PM_UNIT"
curl --connect-timeout 3 --max-time 10 --fail --silent --show-error http://192.168.0.120:16153/api/state \
  > "$PM_RELEASE_STAGE/state-after.json"
journalctl --user -u "$PM_UNIT" -n 100 --no-pager > "$PM_RELEASE_STAGE/journal-after.txt"
```

APIの起動・再接続に有限の待ち時間を決め、未接続を勝手な設定変更で直さない。退避した選択/有効化/共通順序/設定改訂とAPIを照合し、静止画・動画・時計の通常動作を確認する。再接続で変わるonline/sessionやframe IDを永続設定の変更と誤認しない。既存端末のclockを自動でONにしない。API/JMA/動画の本番確認やボタン操作は、許可された確認範囲だけで行う。

## サーバの復旧

通常は**binaryだけを戻す**。新旧ともschema2なので、DBを安易に巻き戻して反映後の利用者変更を消さない。以下も復旧操作の許可範囲で実施する。

```bash
systemctl --user stop "$PM_UNIT"
test "$(systemctl --user show "$PM_UNIT" -p ActiveState --value)" = inactive
test "$(systemctl --user show "$PM_UNIT" -p MainPID --value)" = 0
test ! -e "$PM_BIN.new"
cp -a -- "$PM_RELEASE_STAGE/backup/picomado-server.old" "$PM_BIN.new"
mv -T -- "$PM_BIN.new" "$PM_BIN"
systemctl --user start "$PM_UNIT"
```

DB/素材の復元が必要と判明した場合は、まず停止後の現状も別名で退避し、反映後の変更と失う範囲を親へ報告する。その承認後、現`PicoMado`ディレクトリを退避名へ移し、元の`PicoMado.tar`を元の親ディレクトリへ権限付きで展開する。既存木への上書き展開や、古いDBに新WALを残す復旧は行わない。DBだけでなく索引と素材を同じ退避点へ合わせ、`integrity_check`後に起動する。自動削除gateの有効化は本手順に含まない。

## Firmware：別の操作範囲

対象名、物理board ID、現在の復旧UF2/hash、個体config、パネル有効状態、現行build flagsを確認する。退避済み「ALL-OFF」という名前からflagを推定しない。今回のsample/UNBOUND成果物はflashしない。

実個体設定を内容・0600権限のまま別の隔離sourceへ複写し、`firmware/build/<対象名>`で通常bindingを持つbuildを作る。`PM_USE_SAMPLE=OFF`、`PM_PREPARE_UNBOUND=OFF`、`PM_PREPARE_UNBOUND_PANEL=OFF`、`PICO_BOARD=pico_w`を明示し、EV1/STREAM/AP2/SIM/TX/CY/EEX1の各値は承認されたmanifestに合わせる。共有SDKのauto update/fetchはOFF、元SHAと私有診断の出自を確認し、共有SDKを編集しない。新CMakeCache/compile flags/ELF/map/.su/UF2/hashを保存する。

実個体用のconfigure例（変数の値は承認されたmanifestで設定し、未確認なら実行しない）：

```bash
: "${PM_FIRMWARE_SOURCE:?set the isolated source with private device config}"
: "${PM_DEVICE:?set the explicitly approved device name}"
cmake -S "$PM_FIRMWARE_SOURCE/firmware" -B "$PM_FIRMWARE_SOURCE/firmware/build/$PM_DEVICE" \
  -DPICO_BOARD=pico_w -DCMAKE_BUILD_TYPE=Release -DPM_DEVICE="$PM_DEVICE" \
  -DPM_USE_SAMPLE=OFF -DPM_PREPARE_UNBOUND=OFF -DPM_PREPARE_UNBOUND_PANEL=OFF \
  -DPICO_SDK_SUBMODULE_AUTO_UPDATE=OFF -DPICO_SDK_FETCH_FROM_GIT=OFF \
  -DPM_EVENT_STREAM="${PM_FW_EV1:?}" -DPM_STREAM_DIAGNOSTICS="${PM_FW_STREAM:?}" \
  -DPM_ACK_PATH_PROBE="${PM_FW_AP2:?}" -DPM_USB_BUTTON_SIM="${PM_FW_SIM:?}" \
  -DPM_TX_LINK_PROBE="${PM_FW_TX:?}" -DPM_CYW43_INNER_PROBE="${PM_FW_CY:?}" \
  -DPM_AWAY_EEX1="${PM_FW_EEX1:?}" -DPM_EEX1_PORT="${PM_FW_EEX1_PORT:?}"
bash "$PM_FIRMWARE_SOURCE/firmware/tools/build.sh" --device "$PM_DEVICE"
```

個体別出力とmanifestは既存`firmware/tools/build.sh --device <対象名>`の契約に合わせる。実USB操作の承認と対象BOOTSELの確認後に限り、`firmware/tools/flash.sh --device <対象名> --check-only`で照合し、続いて同scriptでverify付き書込みを行う。`--check-only`も実デバイス照会を行うため、現在は禁止。書込み後に対象端末の接続・画面・ボタン・通常動画/時計を確認する。

復旧は保存済みのその端末用UF2/hashと個体bindingを照合し、同じ1台を手動BOOTSELにして既存のverify付き経路で戻す。別端末や古い別アプリのUF2を名前だけで選ばない。サーバDBの選択/有効化をfirmware復旧に合わせて書き換えない。

## EV1：有限採取、再現時停止、保存、解析

これは追加の実機・サービス操作を含むため別承認が必要。対応firmwareのEV1と、`boundary-observation`付きserver＋新規`PICOMADO_OBSERVATION_PATH`が前提。serverのcaptureは現在host lifecycleに結び付き、開始にはstartup、確定summaryには正常終了が必要。通常稼働のまま任意にON/OFFできるserver APIは実装していない。

出力は新規・私有directoryへ、serialは対象の安定したby-id path、時間は確認済みの有限上限を使う。以下は採取操作の承認後の例。他のserial readerやAP2/SIMのcontrollerを同時openしない。

```bash
: "${PM_SERIAL:?set the verified stable by-id serial path}"
: "${PM_CAPTURE:?set a new private absolute capture directory}"
: "${PM_SECONDS:?set the approved finite maximum duration}"
umask 077
mkdir -m 700 -- "$PM_CAPTURE"
python3 "$PM_SOURCE/firmware/tools/event_stream.py" --serial "$PM_SERIAL" \
  --output "$PM_CAPTURE/device.jsonl" --socket "$PM_CAPTURE/owner.sock" \
  --seconds "$PM_SECONDS" --reconnects 0 \
  > "$PM_CAPTURE/owner-result.json" 2> "$PM_CAPTURE/owner-stderr.txt" &
PM_CAPTURE_PID=$!
```

IPCはUnix socketへASCII JSON＋LFを1要求送り、LFまでの有限replyを読む。`{"op":"status"}`で`ready=true`と今回のboot/CID境界を確認してから、利用者がボタンを押す。再現時刻と操作方向は別記する。症状別の自動freezeや合成入力を追加しない。

status/stopの送信例。準備確認時は`PM_CAPTURE_OP=status`、再現後は`PM_CAPTURE_OP=stop`として、同じsocketへ実行する。これはserialの第二readerではない。

```bash
python3 - "$PM_CAPTURE/owner.sock" "${PM_CAPTURE_OP:?set status or stop}" <<'PY'
import json, socket, sys
operation = sys.argv[2]
assert operation in ('status', 'stop')
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
    client.settimeout(2)
    client.connect(sys.argv[1])
    client.sendall(json.dumps({'op': operation}).encode('ascii') + b'\n')
    reply = b''
    while b'\n' not in reply:
        part = client.recv(min(1024, 8192-len(reply)))
        if not part or len(reply)+len(part) >= 8192:
            raise RuntimeError('missing or oversized owner reply')
        reply += part
result = json.loads(reply.split(b'\n', 1)[0])
if operation == 'status':
    assert result.get('ready') is True, 'capture is not ready'
else:
    assert result.get('state') == 'stopping', 'stop was not accepted'
print(json.dumps(result, ensure_ascii=False))
PY
```

再現したら`{"op":"stop"}`を同じownerへ送る。`state=stopping`は停止要求の受理だけ。ownerのOFF応答・保存worker終了・`device.jsonl.summary.json`作成まで待つ。例外・kill・切断で終えた場合も元ファイルを残し、不完全として解析する。CLIの終了コードや標準出力だけをcomplete判定に使わない。

`wait "$PM_CAPTURE_PID"`で有限時間のowner終了を確認し、summaryの`transport_complete`、`storage_durable`、segment、欠測理由を読む。サーバ側の正常終了と`server.jsonl.summary.json`も確認してから、保存済みファイルだけを解析する。

```bash
python3 "$PM_SOURCE/firmware/tools/event_causality.py" \
  --device "$PM_CAPTURE/device.jsonl" --server "$PM_CAPTURE/server.jsonl" \
  --slow-us "$PM_SLOW_US" --deadline-us "$PM_DEADLINE_US" \
  > "$PM_CAPTURE/analysis.json"
```

閾値は事前に定めた解析policy。全操作に描画を要求すると確認した試験だけ`--require-frame`を付ける。JSONLと両summaryを一緒に保存し、SHA/sequence/durable prefix/単一segmentを検査する。別source時計を直接引かず、cause/frameの一致で照合する。欠測・metadata不正・catalog不足はunknown/incomplete。正常な短い採取、採取コマンドの起動、過去の報告だけで「遅延なし」「記録できた」「原因解消」と報告しない。
