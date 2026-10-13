#!/usr/bin/env bash
set -euo pipefail
if [[ ( $# != 2 && $# != 3 ) || ${1-} != --device || ( $# == 3 && ${3-} != --check-only ) ]]; then
  echo 'usage: flash.sh --device DEVICE [--check-only]' >&2
  exit 2
fi
device=$2
if [[ ! $device =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo 'invalid device name' >&2
  exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
expected_file="$root/devices/$device/board-id.txt"
image="$root/build/$device/$device.uf2"
config="$root/devices/$device/config.h"
manifest="$root/build/$device/manifest.txt"
if [[ ! -f $expected_file || ! -f $config || ! -f $image || ! -f $manifest ]]; then
  echo 'device config, board binding, UF2, or manifest missing' >&2
  exit 2
fi
expected=$(tr -d '\r\n' < "$expected_file")
if [[ ! $expected =~ ^[[:xdigit:]]{16}$ ]]; then
  echo 'invalid board ID binding' >&2
  exit 2
fi
read -r mode built_device built_board built_hash < "$manifest"
actual_hash=$(sha256sum "$image" | cut -d' ' -f1)
if [[ $mode != OFF || $built_device != "$device" ||
      ${built_board^^} != ${expected^^} || $built_hash != "$actual_hash" ]]; then
  echo 'UF2 manifest does not match the selected device and image' >&2
  exit 2
fi
image_info=$(picotool info -a "$image")
if ! grep -Eq '^[[:space:]]*pico_board:[[:space:]]*pico_w[[:space:]]*$' <<< "$image_info"; then
  echo 'selected UF2 is not a Pico W build' >&2
  exit 2
fi
info=$(picotool info -d)
count=$(grep -c '^Device Information$' <<< "$info" || true)
if [[ $count != 1 ]]; then
  echo 'expected exactly one BOOTSEL board' >&2
  exit 2
fi
actual=$(sed -nE 's/^[[:space:]]*flash id:[[:space:]]*0x([[:xdigit:]]{16}).*/\1/p' <<< "$info")
if [[ ${actual^^} != ${expected^^} ]]; then
  echo 'BOOTSEL board ID does not match selected device' >&2
  exit 2
fi
printf 'Target %s verified against BOOTSEL board %s\n' "$device" "$actual"
if [[ ${3-} == --check-only ]]; then exit 0; fi
picotool load -v -x "$image"
