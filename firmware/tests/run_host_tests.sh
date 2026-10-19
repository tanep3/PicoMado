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
build endpoint-policy tests/endpoint_policy_test.c src/endpoint_policy.c
build eex-wire tests/eex_wire_test.c src/eex_wire.c
build eex-exchange tests/eex_exchange_test.c src/eex_wire.c src/eex_exchange.c src/endpoint_policy.c
build endpoint-client tests/endpoint_client_test.c src/eex_wire.c src/eex_exchange.c src/endpoint_policy.c src/endpoint_client.c
for name in core wake transform rx-flow ws-accept endpoint-policy eex-wire eex-exchange endpoint-client; do run "$name"; done
build trace -DPM_TRACE_ENABLE=1 -Itests/trace_stubs -Itests/app_stubs -Itests/stubs \
    tests/trace_test.c src/trace.c src/rx_stats.c src/trace_usb.c src/core.c
run trace
build stats -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/trace_stubs -Itests/app_stubs -Itests/stubs \
    tests/stats_test.c src/trace.c src/rx_stats.c src/trace_usb.c
run stats
build rx-stats -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/app_stubs -Itests/stubs tests/rx_stats_test.c src/rx_stats.c
run rx-stats
build rx-width -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/app_stubs -Itests/stubs tests/rx_stats_width_test.c
run rx-width
build rx-dump -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/trace_stubs -Itests/app_stubs -Itests/stubs \
    tests/rx_dump_test.c src/trace.c src/rx_stats.c src/trace_usb.c
run rx-dump "$work/rx-dump.txt"
python3 tests/rx_dump_parser_test.py "$work/rx-dump.txt"
printf 'PASS rx-parser\n'
build app -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/app_timing_test.c src/core.c src/video.c src/trace.c src/rx_stats.c
run app
for mode in drop-during-retry usb-snapshot sdk-error error-log pm-set-error pm-read-error pm-read-mismatch; do run app "$mode"; done
build app-stats -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/app_timing_test.c src/core.c src/video.c src/trace.c src/rx_stats.c
run app-stats
build net -DPM_TRACE_ENABLE=1 -Itests/net_stubs -Itests/app_stubs -I. \
    tests/net_socks_test.c src/ws_accept.c src/trace.c src/rx_stats.c
run net
build net-stats -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/net_stubs -Itests/app_stubs -I. tests/net_socks_test.c src/ws_accept.c src/trace.c src/rx_stats.c
run net-stats
for device in sample-a E-ink_0 E-ink_1; do
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 \
        -I"devices/$device" -Isrc -Itests/net_stubs -Itests/app_stubs -I. \
        tests/net_nagle_test.c src/ws_accept.c -o "$work/net-nagle-$device"
    run "net-nagle-$device" "$work/net-nagle-$device.flags"
done
build lcd-bus -DPM_TRACE_ENABLE=1 -DPM_NORMAL_BUILD=1 -DPM_LCD_FRAME_CS_HOLD=1 \
    -DPM_LCD_CONTROL_SPI_BAUD=20000000 -DPM_TEST_NORMAL_SPI_BAUD=20000000 \
    -Itests/stubs tests/lcd_bus_test.c src/lcd.c src/core.c src/video.c src/trace.c src/rx_stats.c
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
        -I"devices/$device" -Isrc -Itests/app_stubs tests/eink_app_test.c src/core.c src/video.c \
        -o "$work/eink-app-$device"
    run "eink-app-$device"
done

build eink-order tests/eink_transform_dump.c
python3 tests/eink_legacy_order_test.py "$work/eink-order"
printf 'PASS eink-legacy-order\n'

for device in sample-a E-ink_0 E-ink_1; do
    cc -std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 \
        '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
        -I"devices/$device" -Isrc -Itests/app_stubs tests/control_reply_test.c src/core.c src/video.c src/trace.c src/rx_stats.c \
        -o "$work/control-$device"
    run "control-$device"
done

# Exercise the same production LCD parser and functional deadlines without diagnostics.
build control-trace-off -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=0 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/control_reply_test.c src/core.c src/video.c src/trace.c src/rx_stats.c
run control-trace-off
build control-stats-quiet -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/control_reply_test.c src/core.c src/video.c src/trace.c src/rx_stats.c
run control-stats-quiet

build video tests/video_test.c src/video.c src/core.c
run video

# Exercise production main's EEX1 glue with socket/radio operations mocked.
build app-eex -DPM_AWAY_EEX1=1 -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/app_endpoint_test.c src/core.c src/video.c src/trace.c \
    src/eex_wire.c src/eex_exchange.c src/endpoint_policy.c src/endpoint_client.c
run app-eex

# Unified diagnostics are exercised separately; ordinary targets remain OFF.
build stream-diag -DPM_STREAM_DIAGNOSTICS=1 -DPM_TRACE_ENABLE=1 -Itests/app_stubs tests/stream_diag_test.c
run stream-diag "$work/stream-diag.txt"
python3 tests/stream_diag_parser_test.py "$work/stream-diag.txt"
printf 'PASS stream-diag-parser\n'
build net-diag -DPM_STREAM_DIAGNOSTICS=1 -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    -Itests/net_stubs -Itests/app_stubs -I. tests/net_socks_test.c src/ws_accept.c src/trace.c src/rx_stats.c src/stream_diag.c
run net-diag
build control-diag -DPM_STREAM_DIAGNOSTICS=1 -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/app_stubs tests/control_reply_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/stream_diag.c
run control-diag

build stream-diag-identity -DPM_STREAM_DIAGNOSTICS=1 -DPM_TRACE_ENABLE=1 -Itests/app_stubs tests/stream_diag_identity_test.c
run stream-diag-identity "$work/identity.txt"
python3 tools/parse_stream_diag.py "$work/identity.txt" --output "$work/identity.json"

# Production main+net together: bounded EOF salvage, pacing, hard errors and generations.
build net-eof-recovery -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 -DPM_TRACE_STATS=1 -DPM_TRACE_VERBOSE=0 -DPM_STREAM_DIAGNOSTICS=1 \
    '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
    -Itests/net_stubs -Itests/app_stubs tests/fin_recovery_test.c src/core.c src/video.c src/ws_accept.c src/trace.c src/rx_stats.c src/stream_diag.c
for mode in {1..52}; do run net-eof-recovery "$mode"; done
