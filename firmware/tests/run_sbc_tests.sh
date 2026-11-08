#!/usr/bin/env bash
# Pure/production-fake fixtures only. No devices, sockets, or services.
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-sbc.XXXXXX)
trap 'rm -rf "$work"' EXIT
flags=(-std=gnu11 -Wall -Wextra -Werror -Isrc -DPM_USE_SAMPLE_CONFIG=1
       -include profiles/pico_w_lcd_160x80_4.h -Iprofiles -Idevices/sample-a)
for mode in native sanitized; do
 extra=()
 if [[ $mode == sanitized ]]; then extra=(-fsanitize=address,undefined -fno-omit-frame-pointer); fi
 for name in sbc sbc_runtime sbc_media; do
  cc "${flags[@]}" "${extra[@]}" "tests/bluetooth_${name}_test.c" -o "$work/$name-$mode"
  ASAN_OPTIONS=detect_leaks=0 "$work/$name-$mode" ../service/tests/fixtures/sbc-v1-frames.txt
 done
done
