"""Structural regression for the real runtime callback boundary.
Does not simulate hardware timing or prove realtime throughput.
"""
from pathlib import Path
s=(Path(__file__).resolve().parents[1]/'src/bluetooth_runtime.c').read_text()
prepare=s.split('static void prepare_audio(void){',1)[1].split('static void send_audio(void){',1)[0]
send=s.split('static void send_audio(void){',1)[1].split('uint32_t pm_bt_runtime_rate',1)[0]
tick=s.split('static void tick(btstack_timer_source_t *t){',1)[1].split('static void prepare_audio(void){',1)[0]
stop=s.split('static void media_stop(void){',1)[1].split('static unsigned ready_packet',1)[0]
begin=s.split('bool pm_bt_runtime_audio_begin(',1)[1].split('bool pm_bt_runtime_audio_finish',1)[0]
assert 'encode_signed_16' in prepare and 'encode_signed_16' not in send
assert 'consumed_frames+=' not in prepare
assert send.index('if(status)') < send.index('consumed_frames+=')
assert send.index('stream_send_media_payload_rtp') < send.index('consumed_frames+=')
assert 'schedule_audio(1)' in send and 'prepare_audio()' not in send
assert tick.index('prepare_audio()') < tick.index('request_can_send_now')
assert tick.index('pending=true') < tick.index('request_can_send_now')
assert '!prepared_count' in send
assert 'prepared_count=prepared_bytes=0' in stop
assert 'prepared_count=prepared_bytes=0' in begin
assert 'if(delay < -250)' in send
print('Bluetooth pipeline structure: pre-encode, synchronous-grant guard, send credit, cancellation PASS')
