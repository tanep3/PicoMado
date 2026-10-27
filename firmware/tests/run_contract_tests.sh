#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
source tests/profile_env.sh
work=$(mktemp -d /tmp/picomado-contract.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -Iprofiles -Isrc)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" tests/device_contract_test.c -o "$work/device"
"$work/device"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_test.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/observation"
"$work/observation" "$work/events.txt"
python3 tests/event_stream_test.py "$work/events.txt"

cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_USE_SAMPLE_CONFIG=1 -DPM_ACK_PATH_PROBE=1 -DPM_USB_BUTTON_SIM=1 -DPM_TRACE_ENABLE=1 -DPM_EVENT_STREAM=1 -Idevices/sample-a -Itests/app_stubs tests/usb_multiplex_test.c src/ack_probe.c src/usb_control.c src/button_sim.c src/core.c src/trace.c src/rx_stats.c -o "$work/compat"
"$work/compat"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_limits_test.c src/usb_control.c -o "$work/limits"
"$work/limits"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_boundary_irq_test.c -o "$work/boundary"
"$work/boundary" "$work/boundary.events"
python3 tests/observation_boundary_irq_test.py "$work/boundary.events"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_drain_test.c src/usb_control.c -o "$work/drain"
"$work/drain"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" tests/observation_transport_test.c -o "$work/provenance"
"$work/provenance"
cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" -DPM_USE_SAMPLE_CONFIG=1 -DPM_TRACE_ENABLE=1 -DPM_EVENT_STREAM=1 -DPM_ACK_PATH_PROBE=1 -DPM_TX_LINK_PROBE=1 -DPM_NET_TX_THREADS=1 -Idevices/sample-a -Itests/net_stubs -Itests/app_stubs -I. tests/observation_net_test.c src/ws_accept.c src/trace.c src/rx_stats.c src/tx_link_probe.c -pthread -o "$work/net"
"$work/net"

cc "${base[@]}" -include "$root/profiles/pico_w_lcd_160x80_4.h" tests/observation_json_test.c -o "$work/metadata"
"$work/metadata"

for device in E-ink_0 E-ink_1; do
 cc "${base[@]}" -include "$(pm_test_profile_header "$device")" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 \
  '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" \
  -I"devices/$device" -Itests/app_stubs tests/eink_pending_observation_test.c \
  src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/pending-$device"
 "$work/pending-$device" "$work/pending-$device.events"
 python3 tests/eink_pending_observation_test.py "$work/pending-$device.events"
done
