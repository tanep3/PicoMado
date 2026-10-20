#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-contract.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -Isrc)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
cc "${base[@]}" tests/device_contract_test.c -o "$work/device"
"$work/device"
cc "${base[@]}" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_test.c src/trace.c src/rx_stats.c src/usb_control.c -o "$work/observation"
"$work/observation" "$work/events.txt"
python3 tests/event_stream_test.py "$work/events.txt"

cc "${base[@]}" -DPM_USE_SAMPLE_CONFIG=1 -DPM_ACK_PATH_PROBE=1 -DPM_USB_BUTTON_SIM=1 -DPM_TRACE_ENABLE=1 -DPM_EVENT_STREAM=1 -Idevices/sample-a -Itests/app_stubs tests/usb_multiplex_test.c src/ack_probe.c src/usb_control.c src/button_sim.c src/core.c src/trace.c src/rx_stats.c -o "$work/compat"
"$work/compat"
cc "${base[@]}" -DPM_EVENT_STREAM=1 -DPM_TRACE_ENABLE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/observation_limits_test.c src/usb_control.c -o "$work/limits"
"$work/limits"

cc "${base[@]}" tests/observation_json_test.c -o "$work/metadata"
"$work/metadata"
