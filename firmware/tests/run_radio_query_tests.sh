#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-radio.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -Iprofiles -Isrc -Itests/app_stubs)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
for device in sample-a E-ink_0 E-ink_1; do
 for trace in 0 1; do
  cc "${base[@]}" "-DPM_TRACE_ENABLE=$trace" '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
   -I"devices/$device" tests/radio_query_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/$device-$trace"
  "$work/$device-$trace"
  cc "${base[@]}" "-DPM_TRACE_ENABLE=$trace" '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
   -I"devices/$device" tests/radio_compare_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/compare-$device-$trace"
  "$work/compare-$device-$trace"
  cc "${base[@]}" "-DPM_TRACE_ENABLE=$trace" '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
   -I"devices/$device" tests/radio_readback_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/readback-$device-$trace"
  "$work/readback-$device-$trace"
  cc "${base[@]}" "-DPM_TRACE_ENABLE=$trace" '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
   -I"devices/$device" tests/roaming_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/roaming-$device-$trace"
  "$work/roaming-$device-$trace"
  cc "${base[@]}" "-DPM_TRACE_ENABLE=$trace" '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
   -I"devices/$device" tests/radio_defaults_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/defaults-$device-$trace"
  "$work/defaults-$device-$trace"
 done
done
