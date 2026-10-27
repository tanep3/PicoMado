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
if [[ -n ${PM_BUILD_DIR:-} ]]; then build_dir=$PM_BUILD_DIR; fi
profile_file="$root/devices/$device/profile.txt"
[[ -f $profile_file ]] || { echo 'hardware profile is missing' >&2; exit 2; }
profile=$(tr -d '\r\n' < "$profile_file")
if [[ ! $profile =~ ^[A-Za-z0-9_-]+$ || ! -f $root/profiles/$profile.h || ! -f $root/profiles/$profile.board ]]; then
  echo 'invalid or missing hardware profile' >&2; exit 2
fi
board=$(tr -d '\r\n' < "$root/profiles/$profile.board")
if [[ $board != pico_w ]]; then echo 'MCU board implementation unavailable: only pico_w is implemented' >&2; exit 2; fi
target=picomado_lcd
if grep -Eq '^[[:space:]]*#define[[:space:]]+PM_DISPLAY_TYPE[[:space:]]+PM_DISPLAY_EINK\b' "$root/profiles/$profile.h"; then target=picomado_eink; fi
options=()
for flag in PM_ACK_PATH_PROBE PM_USB_BUTTON_SIM PM_TX_LINK_PROBE PM_CYW43_INNER_PROBE PM_CYW43_SPI_REAL_SIZE PM_EVENT_STREAM PM_STREAM_DIAGNOSTICS; do
  if [[ -v $flag ]]; then
    value=${!flag}
    [[ $value == ON || $value == OFF ]] || { echo "$flag must be ON or OFF" >&2; exit 2; }
    options+=("-D$flag=$value")
  fi
done
cmake -S "$root" -B "$build_dir" -DPM_DEVICE="$device" -DPICO_BOARD="$board" \
  -DPM_USE_SAMPLE="$sample" -DPM_PREPARE_UNBOUND="$unbound" -DPM_PREPARE_UNBOUND_PANEL="$panel" \
  -DCMAKE_BUILD_TYPE=Release "${options[@]}"
cmake --build "$build_dir" --target "$target" -j4
printf 'UF2: %s\n' "$build_dir/$device.uf2"
