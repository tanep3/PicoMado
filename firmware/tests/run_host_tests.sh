#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
work=$(mktemp -d /tmp/picomado-host.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Isrc)
build() { local name=$1; shift; cc "${base[@]}" "$@" -o "$work/$name"; }
run() { local name=$1; shift; "$work/$name" "$@"; printf 'PASS %s %s\n' "$name" "$*"; }
build core tests/core_test.c src/core.c
build wake tests/wake_test.c src/core.c
build transform tests/lcd_transform_test.c
build rx-flow tests/rx_flow_test.c
build ws-accept tests/ws_accept_test.c src/ws_accept.c
for name in core wake transform rx-flow ws-accept; do run "$name"; done
build trace -DPM_TRACE_ENABLE=1 -Itests/trace_stubs -Itests/app_stubs -Itests/stubs \
    tests/trace_test.c src/trace.c src/trace_usb.c src/core.c
run trace
build app -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/app_timing_test.c src/core.c src/trace.c
run app
for mode in drop-during-retry usb-snapshot sdk-error error-log; do run app "$mode"; done
build net -DPM_TRACE_ENABLE=1 -Itests/net_stubs -Itests/app_stubs -I. \
    tests/net_socks_test.c src/ws_accept.c src/trace.c
run net
build lcd-bus -DPM_TRACE_ENABLE=1 -DPM_NORMAL_BUILD=1 -DPM_LCD_FRAME_CS_HOLD=1 \
    -DPM_LCD_CONTROL_SPI_BAUD=20000000 -DPM_TEST_NORMAL_SPI_BAUD=20000000 \
    -Itests/stubs tests/lcd_bus_test.c src/lcd.c src/core.c src/trace.c
run lcd-bus
build backlight -DPM_NORMAL_BUILD=1 -DPM_LCD_FRAME_CS_HOLD=1 \
    -DPM_LCD_CONTROL_SPI_BAUD=20000000 -Itests/stubs \
    tests/backlight_test.c src/display.c src/lcd.c src/core.c
run backlight
# Use only dummy config values for the connected/polarity variants.
for polarity in 1 0; do
    mkdir -p "$work/config-$polarity"
    sed -e 's/^#define PM_LCD_BLK_CONNECTED 0/#define PM_LCD_BLK_CONNECTED 1/' \
        -e "s/^#define PM_LCD_BLK_ACTIVE_HIGH 1/#define PM_LCD_BLK_ACTIVE_HIGH $polarity/" \
        devices/sample-a/config.sample.h > "$work/config-$polarity/config.sample.h"
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 \
        -I"$work/config-$polarity" -Isrc -Itests/stubs -DPM_NORMAL_BUILD=1 \
        -DPM_LCD_FRAME_CS_HOLD=1 -DPM_LCD_CONTROL_SPI_BAUD=20000000 \
        tests/backlight_test.c src/display.c src/lcd.c src/core.c -o "$work/backlight-$polarity"
    run "backlight-$polarity"
done

build input tests/input_test.c src/core.c
run input
for device in E-ink_0 E-ink_1; do
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 \
        -I"devices/$device" -Isrc -Itests/stubs tests/eink_test.c src/display_eink.c \
        -o "$work/eink-$device"
    run "eink-$device"
done
mkdir -p "$work/absent-panel"
sed 's/^#define PM_EINK_PANEL_CONNECTED 1/#define PM_EINK_PANEL_CONNECTED 0/' \
    devices/E-ink_1/config.sample.h > "$work/absent-panel/config.sample.h"
cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 \
    -I"$work/absent-panel" -Isrc -Itests/stubs tests/eink_test.c src/display_eink.c \
    -o "$work/eink-absent"
run eink-absent

for device in E-ink_0 E-ink_1; do
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 \
        '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
        -I"devices/$device" -Isrc -Itests/app_stubs tests/eink_app_test.c src/core.c \
        -o "$work/eink-app-$device"
    run "eink-app-$device"
done

build eink-order tests/eink_transform_dump.c
python3 tests/eink_legacy_order_test.py "$work/eink-order"
printf 'PASS eink-legacy-order\n'

for device in sample-a E-ink_0 E-ink_1; do
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 \
        '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
        -I"devices/$device" -Isrc -Itests/app_stubs tests/control_reply_test.c src/core.c src/trace.c \
        -o "$work/control-$device"
    run "control-$device"
done
