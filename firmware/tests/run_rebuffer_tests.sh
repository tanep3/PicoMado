#!/usr/bin/env bash
# Host fixtures only; does not operate a device, service or network.
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-rebuffer.XXXXXX)
trap 'rm -rf "$work"' EXIT
flags=(-std=gnu11 -Wall -Wextra -Werror -Isrc -DPM_USE_SAMPLE_CONFIG=1
       -include profiles/pico_w_lcd_160x80_4.h -Iprofiles -Idevices/sample-a)
for mode in native sanitized; do
 extra=()
 if [[ $mode == sanitized ]]; then extra=(-fsanitize=address,undefined -fno-omit-frame-pointer); fi
 for name in rebuffer rebuffer_runtime refresh media control audio packet_policy; do
  cc "${flags[@]}" "${extra[@]}" "tests/bluetooth_${name}_test.c" -o "$work/$name-$mode"
  ASAN_OPTIONS=detect_leaks=0 "$work/$name-$mode"
 done
done
python3 tests/bluetooth_pipeline_test.py
