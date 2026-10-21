#!/usr/bin/env python3
"""Compare the pinned SDK send functions with only upstream PR155's SPI length change."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import sys

firmware = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(firmware / 'tools'))
import prepare_cyw43_inner as overlay


def function(source, signature, last=False):
    start = source.rindex(signature) if last else source.index(signature)
    return source[start:source.index('\n}\n', start) + 3]


def fields(line):
    return dict(part.split('=', 1) for part in line.split()[1:] if '=' in part)


def build_and_run(source, tag, out, real_size):
    directory = out / tag
    directory.mkdir(parents=True, exist_ok=True)
    start = source.index('// Configure the active level of the host interrupt pin.')
    end = source.index('\n#if CYW43_USE_STATS', start)
    chunks = [
        '#define CYW43_PIN_WL_HOST_WAKE 1', source[start:end],
        function(source, 'static int sdpcm_process_rx_packet('),
        function(source, 'static int cyw43_ll_sdpcm_poll_device(', True),
        function(source, 'static int cyw43_sdpcm_send_common('),
        function(source, 'int cyw43_ll_send_ethernet('),
    ]
    (directory / 'generated_sdk_functions.inc').write_text('\n\n'.join(chunks))
    cmd = ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror',
           '-DPM_CYW43_INNER_PROBE=1',
           '-DPM_SPI_LENGTH_EXPECT_REAL=' + ('1' if real_size else '0'),
           '-I' + str(directory), '-I' + str(firmware / 'src'),
           '-I' + str(firmware / 'tests/app_stubs'),
           str(firmware / 'tests/cyw43_driver_path_test.c'),
           '-o', str(directory / 'sdk-test')]
    compile_result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    (directory / 'compile.log').write_text(compile_result.stdout + compile_result.stderr)
    assert compile_result.returncode == 0, compile_result.stderr[-3000:]
    result = subprocess.run([str(directory / 'sdk-test')], capture_output=True,
                            text=True, timeout=20, env=os.environ.copy())
    (directory / 'run.log').write_text(result.stdout + result.stderr)
    assert result.returncode == 0, (result.stdout + result.stderr)[-3000:]
    return result.stdout.splitlines()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--sdk', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    source = (args.sdk / 'lib/cyw43-driver/src/cyw43_ll.c').read_text()
    bus = (args.sdk / 'src/rp2_common/pico_cyw43_driver/cyw43_bus_pio_spi.c').read_text()
    assert 'make_cmd(true, true, fn, addr, len)' in bus
    assert 'cyw43_spi_transfer(self, (uint8_t *)&self->spi_header[(CYW43_BACKPLANE_READ_PAD_LEN_BYTES / 4)], aligned_len + 4, NULL, 0)' in bus
    baseline = overlay.generate(source)
    candidate = overlay.generate(source, spi_real_size=True)
    old = 'int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, CYW43_WRITE_BYTES_PAD(size), buf);'
    new = 'int pm_result = cyw43_write_bytes(self, WLAN_FUNCTION, 0, size, buf);'
    assert baseline.count(old) == 1 and candidate == baseline.replace(old, new)
    left = build_and_run(baseline, 'baseline', args.out, False)
    right = build_and_run(candidate, 'candidate', args.out, True)
    assert [x for x in left if x.startswith('diagnostic ')] == [x for x in right if x.startswith('diagnostic ')]
    for a, b in zip([x for x in left if x.startswith('wire ')],
                    [x for x in right if x.startswith('wire ')], strict=True):
        aa, bb = fields(a), fields(b)
        for key in ('scenario', 'result', 'reads', 'writes', 'callback', 'credit', 'sequence'):
            assert aa[key] == bb[key], (key, aa, bb)
    old_spi = [fields(x) for x in left if x.startswith('spi ')]
    new_spi = [fields(x) for x in right if x.startswith('spi ')]
    assert len(old_spi) == len(new_spi) >= 18
    for a, b in zip(old_spi, new_spi, strict=True):
        assert {k: v for k, v in a.items() if k != 'command'} == {k: v for k, v in b.items() if k != 'command'}
        assert int(a['command']) == int(a['transfer'])
        assert int(b['command']) == int(b['real'])
    cases = {row['case']: row for row in new_spi if 'case' in row}
    assert cases['data153']['real'] == '171' and cases['data153']['transfer'] == '172'
    assert cases['control15']['real'] == '27' and cases['control15']['transfer'] == '28'
    report = {'passed': True, 'sdk_source_sha256': hashlib.sha256(source.encode()).hexdigest(),
              'private_baseline_sha256': hashlib.sha256(baseline.encode()).hexdigest(),
              'private_candidate_sha256': hashlib.sha256(candidate.encode()).hexdigest(),
              'source_change': [old, new], 'scenarios': len([x for x in left if x.startswith('wire ')]),
              'spi_writes_compared': len(old_spi), 'cases': {
                  key: {'real': value['real'], 'baseline_command': next(x['command'] for x in old_spi if x.get('case') == key),
                        'candidate_command': value['command'], 'physical_transfer': value['transfer']}
                  for key, value in cases.items()}, 'shared_sdk_modified': False, 'rf_proven': False}
    (args.out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print('PASS real SDK send functions: only F2 command length changes; aligned transfer and nominal data match')


if __name__ == '__main__':
    main()
