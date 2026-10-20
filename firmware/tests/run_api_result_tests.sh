#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-api-result.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -Isrc)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
for device in sample-a E-ink_0 E-ink_1; do
 cc "${base[@]}" -DPM_TRACE_ENABLE=1 '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" -I"devices/$device" -Itests/app_stubs tests/api_result_test.c src/core.c src/video.c src/trace.c src/rx_stats.c -o "$work/$device"
 "$work/$device"
done
