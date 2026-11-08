"""Serial, fake-IO host tests. No Cargo, USB, SDK mutation, socket or service."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
base = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-DPM_USE_SAMPLE_CONFIG=1',
        '-include', str(root/'profiles/pico_w_lcd_160x80_4.h'),
        '-Idevices/sample-a', '-Iprofiles', '-Isrc']
def run(args):
    subprocess.run(args, cwd=root, check=True)

with tempfile.TemporaryDirectory(prefix='picomado-bt-startup.') as tmp:
    work = Path(tmp)
    for mode in (0, 1, 2):
        source = 'tests/bluetooth_startup_test.c' if mode == 2 else 'tests/bluetooth_control_test.c'
        binary = work/f'control-{mode}'
        run([*base, f'-DPM_BT_STARTUP_DIAGNOSTIC={mode}', source, '-o', str(binary)])
        run([str(binary)])
    off = work/'off.c'
    off.write_text('#include "bluetooth_startup.h"\n#include "bluetooth_control.inc"\n'
                   'int main(void){poll_bluetooth();return 0;}\n')
    for mode in (0, 1, 2):
        binary = work/f'off-{mode}'
        run([*base, '-DPM_BLUETOOTH_AUDIO=0', f'-DPM_BT_STARTUP_DIAGNOSTIC={mode}', str(off), '-o', str(binary)])
        run([str(binary)])
    # Production main loop with fake hardware/network, BT compiled out.
    app = work/'app-off'
    run([*base, '-DPM_BLUETOOTH_AUDIO=0', '-DPM_BT_STARTUP_DIAGNOSTIC=2',
         '-DPM_NORMAL_BUILD=1', '-DPM_TRACE_ENABLE=1',
         '-DPM_EXPECTED_BOARD_ID="0000000000000001"', '-DPM_BUILD_DEVICE="sample-a"',
         '-Itests/app_stubs', 'tests/app_timing_test.c', 'src/core.c', 'src/video.c',
         'src/trace.c', 'src/rx_stats.c', '-o', str(app)])
    run([str(app)])
    module = root/'cmake/bluetooth_startup.cmake'
    for value in (None, '0', '1', '2', '-1', '3', '01', 'ON', '', '1.5'):
        args = ['cmake', *([] if value is None else [f'-DPM_BT_STARTUP_DIAGNOSTIC={value}']), '-P', str(module)]
        result = subprocess.run(args, cwd=work, capture_output=True)
        assert (result.returncode == 0) == (value in (None, '0', '1', '2')), (value, result.stderr)
    for mode in (-1, 3):
        result = subprocess.run([*base, f'-DPM_BT_STARTUP_DIAGNOSTIC={mode}', '-DPM_BLUETOOTH_AUDIO=0', str(off), '-o', str(work/'invalid')], cwd=root, capture_output=True)
        assert result.returncode != 0
print('Startup modes0/1/2, BT-off/main-loop regression, C/CMake option boundaries PASS')
