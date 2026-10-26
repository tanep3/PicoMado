#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/picomado-usb-tx.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
base=(-std=gnu11 -Wall -Wextra -Werror -DPM_USE_SAMPLE_CONFIG=1 -Iprofiles -Isrc)
if [[ ${SANITIZE:-0} == 1 ]]; then base+=(-g -fsanitize=address,undefined -fno-omit-frame-pointer -fno-pie -no-pie); fi
buildrun(){ local name=$1;shift;cc "${base[@]}" "$@" -o "$work/$name";"$work/$name"; }
buildrun button-sim -Idevices/sample-a tests/button_sim_test.c src/core.c src/button_sim.c
buildrun button-batch -Idevices/sample-a tests/button_sim_batch_test.c src/core.c src/button_sim.c
buildrun usb-run-binding -Idevices/sample-a -DPM_USB_BUTTON_SIM=1 -DPM_ACK_PATH_PROBE=1 -Itests/app_stubs tests/usb_run_binding_test.c src/core.c src/button_sim.c src/usb_control.c src/ack_probe.c
buildrun input-video-cadence -DPM_NORMAL_BUILD=1 -DPM_TEST_REAL_USB_CONTROL=1 \
 '-DPM_EXPECTED_BOARD_ID="0000000000000001"' '-DPM_BUILD_DEVICE="sample-a"' \
 -Idevices/sample-a -Itests/app_stubs tests/input_video_cadence_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/usb_control.c
buildrun tx-link -DPM_TX_LINK_PROBE=1 tests/tx_link_probe_test.c src/tx_link_probe.c
buildrun usb-control -Idevices/sample-a -DPM_USB_BUTTON_SIM=1 -DPM_ACK_PATH_PROBE=1 -Itests/app_stubs tests/usb_control_test.c src/core.c src/button_sim.c src/usb_control.c src/ack_probe.c
for device in sample-a E-ink_0 E-ink_1; do
 buildrun "usb-main-$device" -DPM_USE_SAMPLE_CONFIG=1 -DPM_NORMAL_BUILD=1 -DPM_TRACE_ENABLE=1 -DPM_ACK_PATH_PROBE=1 -DPM_USB_BUTTON_SIM=1 \
 '-DPM_EXPECTED_BOARD_ID="0000000000000001"' "-DPM_BUILD_DEVICE=\"$device\"" -I"devices/$device" -Itests/app_stubs \
 tests/usb_main_input_test.c src/core.c src/video.c src/trace.c src/rx_stats.c src/ack_probe.c src/usb_control.c src/button_sim.c
done
buildrun tx-adapter -DPM_USE_SAMPLE_CONFIG=1 -DPM_TRACE_ENABLE=1 -DPM_ACK_PATH_PROBE=1 -DPM_TX_LINK_PROBE=1 -DPM_NET_TX_THREADS=1 -Idevices/sample-a -Itests/net_stubs -Itests/app_stubs -I. tests/tx_adapter_test.c src/ws_accept.c src/trace.c src/rx_stats.c src/tx_link_probe.c -pthread
buildrun usb-multiplex -DPM_ACK_PATH_PROBE=1 -DPM_USB_BUTTON_SIM=1 -DPM_TRACE_ENABLE=1 -Idevices/sample-a -Itests/app_stubs tests/usb_multiplex_test.c src/ack_probe.c src/usb_control.c src/button_sim.c src/core.c src/trace.c src/rx_stats.c
buildrun heartbeat-trigger -DPM_ACK_PATH_PROBE=1 -DPM_TRACE_ENABLE=1 -Idevices/sample-a -Itests/app_stubs tests/ack_probe_heartbeat_test.c src/core.c
buildrun heartbeat-net -DPM_ACK_PATH_PROBE=1 -DPM_TX_LINK_PROBE=1 -DPM_CYW43_INNER_PROBE=1 -DPM_TRACE_ENABLE=1 -DPM_NET_TEST_OUTPUT_HOOK=1 -Idevices/sample-a -Itests/net_stubs -Itests/app_stubs -I. tests/heartbeat_net_test.c src/ack_probe.c src/cyw43_inner_probe.c src/tx_link_probe.c src/core.c src/ws_accept.c src/trace.c src/rx_stats.c
# Existing AP2 export and strict host validation must survive shared-parser integration.
cc "${base[@]}" -DPM_ACK_PATH_PROBE=1 -DPM_TRACE_ENABLE=1 -Idevices/sample-a -Itests/app_stubs tests/ack_probe_test.c src/core.c src/ack_probe.c src/trace.c src/rx_stats.c -o "$work/ack-probe"
export PM_ACK_EXPORT_FIXTURE="$work/export.txt"
"$work/ack-probe"
python3 tests/ack_probe_v2_test.py

python3 tests/heartbeat_capture_test.py
python3 tests/heartbeat_evidence_contract_test.py
python3 tests/heartbeat_owner_runner_test.py

# Read-only radio query through the normal owner/main path.
bash tests/run_radio_query_tests.sh
