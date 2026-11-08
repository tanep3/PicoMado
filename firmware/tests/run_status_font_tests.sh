#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
work=$(mktemp -d /tmp/picomado-status-font.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
out=${1:-$work};mkdir -p "$out"
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -Iprofiles -Isrc -Itests/stubs)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
cc "${base[@]}" tests/status_font_test.c -o "$work/font"
"$work/font"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -Idevices/sample-a \
    tests/lcd_status_font_test.c src/lcd.c src/core.c -o "$work/lcd-font"
"$work/lcd-font" "$out/lcd-home2.ppm"
cc "${base[@]}" -include "$root/profiles/pico_w_eink_250x122_2.h" -Idevices/E-ink_1 \
    tests/eink_status_font_test.c src/display_eink.c -o "$work/eink-font"
"$work/eink-font" "$out/eink-home2.ppm"
