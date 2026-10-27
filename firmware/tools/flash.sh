#!/usr/bin/env bash
set -euo pipefail
if [[ ( $# != 2 && $# != 3 ) || ${1-} != --device || ( $# == 3 && ${3-} != --check-only && ${3-} != --verify-only ) ]]; then
  echo 'usage: flash.sh --device DEVICE [--check-only|--verify-only]' >&2
  exit 2
fi
device=$2
[[ $device =~ ^[A-Za-z0-9_-]+$ ]] || { echo 'invalid device name' >&2; exit 2; }
root=$(cd "$(dirname "$0")/.." && pwd)
build_dir=${PM_BUILD_DIR:-"$root/build/$device"}
image="$build_dir/$device.uf2"
[[ -f $image ]] || { echo 'UF2 file is missing' >&2; exit 2; }
printf 'UF2: %s\n' "$image"
printf 'Put your Pico W into BOOTSEL mode and copy the UF2 to the RPI-RP2 drive.\n'
