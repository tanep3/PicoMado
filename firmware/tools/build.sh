#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 && $# != 3 ]] || [[ ${1-} != --device ]]; then
  echo 'usage: build.sh --device DEVICE [--sample]' >&2
  exit 2
fi
device=$2
if [[ ! $device =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo 'invalid device name' >&2
  exit 2
fi
sample=OFF
if [[ $# == 3 ]]; then
  if [[ $3 != --sample ]]; then echo 'unknown option' >&2; exit 2; fi
  sample=ON
fi
root=$(cd "$(dirname "$0")/.." && pwd)
build_dir="$root/build/$device"
if [[ $sample == ON ]]; then build_dir="$root/build/.samples/$device"; fi
umask 077
cmake -S "$root" -B "$build_dir" -DPM_DEVICE="$device" \
  -DPM_USE_SAMPLE="$sample" -DPICO_BOARD=pico_w -DCMAKE_BUILD_TYPE=Release
cmake --build "$build_dir" -j4
board_file="$root/devices/$device/board-id.txt"
if [[ $sample == ON ]]; then board_file="$root/devices/$device/board-id.sample.txt"; fi
board=$(tr -d '\r\n' < "$board_file")
hash=$(sha256sum "$build_dir/$device.uf2" | cut -d' ' -f1)
printf '%s %s %s %s\n' "$sample" "$device" "$board" "$hash" > "$build_dir/manifest.txt"
printf 'UF2: %s\n' "$build_dir/$device.uf2"
