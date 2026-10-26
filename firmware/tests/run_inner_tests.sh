#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-inner.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_CYW43_INNER_PROBE=1 -Iprofiles -Isrc)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
cc "${base[@]}" tests/cyw43_frame_validity_test.c -o "$work/validity"
"$work/validity"
cc "${base[@]}" -Itests/app_stubs tests/cyw43_inner_core_test.c -o "$work/core"
"$work/core"
cc "${base[@]}" -DPM_USB_BUTTON_SIM=1 -DPM_ACK_PATH_PROBE=1 -DPM_USE_SAMPLE_CONFIG=1 -Idevices/sample-a -Itests/app_stubs tests/cyw43_inner_wire_test.c src/core.c src/button_sim.c src/usb_control.c src/ack_probe.c -o "$work/wire"
export PM_CYW_EXPORT_FIXTURE="$work/export.txt"
"$work/wire"
python3 tests/cyw43_inner_collect_test.py
cc "${base[@]}" -DPM_USE_SAMPLE_CONFIG=1 -DPM_TRACE_ENABLE=1 -DPM_ACK_PATH_PROBE=1 -DPM_TX_LINK_PROBE=1 -DPM_NET_TX_THREADS=1 -Idevices/sample-a -Itests/net_stubs -Itests/app_stubs -I. tests/tx_adapter_test.c src/ws_accept.c src/trace.c src/rx_stats.c src/tx_link_probe.c src/cyw43_inner_probe.c -pthread -o "$work/adapter"
"$work/adapter"
