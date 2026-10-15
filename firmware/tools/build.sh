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
umask 077
cmake -S "$root" -B "$build_dir" -DPM_DEVICE="$device" \
  -DPM_USE_SAMPLE="$sample" -DPM_PREPARE_UNBOUND="$unbound" -DPM_PREPARE_UNBOUND_PANEL="$panel" -DPICO_BOARD=pico_w -DCMAKE_BUILD_TYPE=Release
cmake --build "$build_dir" -j4
board_file="$root/devices/$device/board-id.txt"
if [[ $sample == ON ]]; then board_file="$root/devices/$device/board-id.sample.txt"; fi
if [[ $unbound == ON ]]; then board=0000000000000000; sample=UNBOUND;
else board=$(tr -d '\r\n' < "$board_file"); fi
if [[ $panel == ON ]]; then sample=UNBOUND-PANEL; fi
hash=$(sha256sum "$build_dir/$device.uf2" | cut -d' ' -f1)
printf '%s %s %s %s\n' "$sample" "$device" "$board" "$hash" > "$build_dir/manifest.txt"
printf 'UF2: %s\n' "$build_dir/$device.uf2"
