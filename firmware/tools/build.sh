#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 && $# != 3 ]] || [[ ${1-} != --device ]]; then
  echo 'usage: build.sh --device DEVICE [--sample|--unbound|--unbound-panel]' >&2
  exit 2
fi
device=$2
if [[ ! $device =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo 'invalid device name' >&2
  exit 2
fi
sample=OFF
unbound=OFF
panel=OFF
if [[ $# == 3 ]]; then
  case $3 in --sample) sample=ON;; --unbound) unbound=ON;; --unbound-panel) unbound=ON; panel=ON;; *) echo 'unknown option' >&2; exit 2;; esac
fi
root=$(cd "$(dirname "$0")/.." && pwd)
build_dir="$root/build/$device"
if [[ $sample == ON ]]; then build_dir="$root/build/.samples/$device"; fi
if [[ $unbound == ON ]]; then build_dir="$root/build/.unbound/$device"; fi
if [[ $panel == ON ]]; then build_dir="$root/build/.unbound-panel/$device"; fi
python3 "$root/../tools/project_gate.py" verify-check
if [[ -n ${PM_BUILD_DIR:-} ]]; then build_dir=$PM_BUILD_DIR; fi
mode=normal
if [[ $sample == ON ]]; then mode=sample; fi
if [[ $unbound == ON ]]; then mode=unbound; fi
if [[ $panel == ON ]]; then mode=unbound_panel; fi
umask 077
python3 "$root/../tools/project_gate.py" build --kind firmware --device "$device" --mode "$mode" --build-dir "$build_dir"
board_file="$root/devices/$device/board-id.txt"
if [[ $sample == ON ]]; then board_file="$root/devices/$device/board-id.sample.txt"; fi
if [[ $unbound == ON ]]; then board=0000000000000000; sample=UNBOUND;
else board=$(tr -d '\r\n' < "$board_file"); fi
if [[ $panel == ON ]]; then sample=UNBOUND-PANEL; fi
profile=$(tr -d '\r\n' < "$root/devices/$device/profile.txt")
case "$profile" in pico_w_lcd_160x80_4|pico_w_eink_250x122_0|pico_w_eink_250x122_2) ;; *) echo 'unverified hardware profile' >&2; exit 2;; esac
profile_hash=$(sha256sum "$root/profiles/$profile.h" | cut -d' ' -f1)
hash=$(sha256sum "$build_dir/$device.uf2" | cut -d' ' -f1)
printf '%s %s %s %s %s %s\n' "$sample" "$device" "$board" "$hash" "$profile" "$profile_hash" > "$build_dir/manifest.txt"
printf 'UF2: %s\n' "$build_dir/$device.uf2"

if [[ $sample == OFF && $unbound == OFF ]]; then
  python3 "$root/../tools/project_gate.py" record --build-receipt "$build_dir/build-receipt.json" --artifact "$build_dir/$device.uf2" --kind firmware \
    --device "$device" --board-id "$board_file" --config "$root/devices/$device/config.h" --out "$build_dir/candidate.json"
fi
