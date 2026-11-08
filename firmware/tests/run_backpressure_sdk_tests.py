#!/usr/bin/env python3
"""Offline COPY/backpressure: real installed tcp_write + production fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--sdk', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
a.out = a.out.resolve()
a.out.mkdir(parents=True, exist_ok=True)
f = Path(__file__).resolve().parents[1]
lwip = a.sdk.resolve() / 'lib/lwip/src'
files = [lwip / name for name in ['core/tcp_out.c', 'core/def.c', 'core/stats.c', 'include/lwip/tcp.h']]
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
hashes = {str(path): sha(path) for path in files}
results = []
for variant, extra in [('native', []), ('sanitized', ['-fsanitize=address,undefined', '-fno-omit-frame-pointer'])]:
    base = ['cc', '-std=gnu11', '-O1', '-g', '-Wall', '-Wextra', '-Werror'] + extra
    profile = ['-DPM_USE_SAMPLE_CONFIG=1', '-include', 'profiles/pico_w_lcd_160x80_4.h', '-Idevices/sample-a', '-Iprofiles', '-Isrc']
    cases = [
        ('sdk', ['-ffunction-sections', '-fdata-sections', '-Itests/nagle_sdk_stubs', '-Itests/app_stubs', '-I.', '-I'+str(lwip/'include'), 'tests/net_backpressure_sdk_test.c'] + [str(x) for x in files[:3]] + ['-Wl,--gc-sections'], []),
        ('net', profile + ['-Itests/net_stubs', '-Itests/app_stubs', '-I.', 'tests/net_nagle_test.c', 'src/ws_accept.c'], [str(a.out / ('net-'+variant+'.flags'))]),
        ('cleanup', profile + ['-Itests/net_stubs', '-Itests/app_stubs', '-I.', 'tests/net_terminal_cleanup_test.c', 'src/ws_accept.c'], []),
        ('media', profile + ['tests/bluetooth_media_test.c'], []),
    ]
    for name, flags, args in cases:
        label = name+'-'+variant
        binary = a.out / label
        for phase, cmd in [('compile', base+flags+['-o', str(binary)]), ('test', [str(binary)]+args)]:
            r = subprocess.run(cmd, cwd=f, capture_output=True, text=True, timeout=60,
                               env=dict(os.environ, ASAN_OPTIONS='detect_leaks=0'))
            log = a.out/(label+'-'+phase+'.log')
            log.write_text(r.stdout+r.stderr)
            results.append(dict(label=label, phase=phase, exit=r.returncode, log=str(log), command=cmd))
            (a.out/'results.json').write_text(json.dumps(results, indent=2)+'\n')
            if r.returncode:
                raise SystemExit(r.stdout+r.stderr)
            if phase == 'test':
                print(r.stdout.strip())
assert all(sha(Path(path)) == value for path, value in hashes.items())
summary = dict(passed=True, steps=len(results), SDK_source_unchanged=True, SDK_hashes=hashes,
               sanitizers='ASan+UBSan; LeakSanitizer unsupported under ptrace, disabled',
               scope='actual tcp_write/tcp_output with deterministic allocation, ACK release and link fixtures; production adapter and media owner; no RF/USB/network')
(a.out/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
print(json.dumps(summary, indent=2))
