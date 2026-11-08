#!/usr/bin/env python3
"""BT-AUDIO-20261107: serial offline actual owner/cache lifecycle fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
out = args.out.resolve()
out.mkdir(parents=True, exist_ok=True)
firmware = Path(__file__).resolve().parents[1]
results = []
cases = ['bootstrap', 'replacement', 'no-hint', 'lifetime', 'scope', 'eligibility',
         'cache-miss', 'install-failure', 'cleanup-failure', 'deadline-hook',
         'terminal-failure', 'seed-identity', 'cancellation']

def run(label, command, expected_abort=False):
    command = list(map(str, command))
    result = subprocess.run(command, cwd=firmware, capture_output=True, text=True,
                            timeout=90, env=dict(os.environ, ASAN_OPTIONS='detect_leaks=0',
                                                 UBSAN_OPTIONS='halt_on_error=1:print_stacktrace=1'))
    (out / (label + '.log')).write_text(result.stdout + result.stderr)
    accepted = result.returncode == (-6 if expected_abort else 0)
    results.append(dict(label=label, command=command, exit=result.returncode,
                        expected_abort=expected_abort, passed=accepted))
    (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    print(('PASS ' if accepted else 'FAIL ') + label, flush=True)
    return accepted

sources = ['src/net.c', 'src/server_link_owner.inc', 'src/server_link_hint.c',
           'src/server_link_cache.c', 'src/main.c', 'tests/app_timing_test.c', 'tests/server_link_cache_owner_test.c',
           'tests/net_nagle_test.c', 'tests/run_server_link_cache_owner_tests.py']
(out / 'source-hashes.json').write_text(json.dumps({p: hashlib.sha256((firmware / p).read_bytes()).hexdigest()
                                                 for p in sources}, indent=2) + '\n')
for variant, sanitizer in [('native', []), ('sanitized', ['-fsanitize=address,undefined', '-fno-omit-frame-pointer'])]:
    binary = out / ('owner-' + variant)
    command = ['cc', '-std=gnu11', '-O1', '-g', '-Wall', '-Wextra', '-Werror'] + sanitizer + [
        '-DPM_USE_SAMPLE_CONFIG=1', '-DPM_SERVER_LINK_HINT=1', '-DPM_SERVER_LINK_CACHE=1',
        '-DETHARP_SUPPORT_STATIC_ENTRIES=1', '-include', 'profiles/pico_w_eink_250x122_2.h',
        '-Idevices/E-ink_1', '-Iprofiles', '-Isrc', '-Itests/net_stubs', '-Itests/app_stubs', '-I.',
        'tests/server_link_cache_owner_test.c', 'src/ws_accept.c', 'src/server_link_hint.c',
        'src/server_link_cache.c', '-o', binary]
    if not run('compile-' + variant, command):
        continue
    for case in cases:
        run(case + '-' + variant, [binary, case])
    app_binary = out / ('app-deadline-' + variant)
    app_command = ['cc', '-std=gnu11', '-O1', '-g', '-Wall', '-Wextra', '-Werror'] + sanitizer + [
        '-DPM_USE_SAMPLE_CONFIG=1', '-DPM_SERVER_LINK_HINT=1', '-DPM_SERVER_LINK_CACHE=1',
        '-DPM_CACHE_APP_DEADLINE=1', '-DPM_NORMAL_BUILD=1', '-DPM_BLUETOOTH_AUDIO=0',
        '-DPM_EXPECTED_BOARD_ID="0000000000000001"', '-DPM_BUILD_DEVICE="E-ink_1"',
        '-include', 'profiles/pico_w_eink_250x122_2.h', '-Idevices/E-ink_1', '-Iprofiles',
        '-Isrc', '-Itests/app_stubs', 'tests/server_link_cache_owner_test.c',
        'src/core.c', 'src/video.c', 'src/trace.c', 'src/rx_stats.c', 'src/server_link_hint.c',
        'src/server_link_cache.c', '-o', app_binary]
    if run('compile-app-deadline-' + variant, app_command):
        for phase in range(3):
            for seconds in (3, 10, 120):
                run(f'app-deadline-{phase}-{seconds}-{variant}', [app_binary, phase, seconds])
# Negative controls alter temporary copies only. Each must fail a specific
# real-code assertion, not compilation or an unrelated nonzero exit.
mutations = out / 'mutations'
mutations.mkdir(exist_ok=True)
owner_text = (firmware / 'src/server_link_owner.inc').read_text()
net_text = (firmware / 'src/net.c').read_text()
hint_text = (firmware / 'src/server_link_hint.c').read_text()
main_text = (firmware / 'src/main.c').read_text()
negative_cases = [
    ('provisional-stage', 'bootstrap', 'owner',
     'if(server_link.provisional&&!socket_dead&&!terminal_send_failed)', 'if(false)'),
    ('netif-number', 'scope', 'hint',
     '&&a->netif_index==b->netif_index', ''),
    ('wrong-seed', 'seed-identity', 'owner',
     'server_link.provisional=false;\n#endif\n            }else',
     'server_link.provisional=false;memcpy(server_link.seed.mac,hint->mac,6);\n#endif\n            }else'),
    ('missing-failure-hook', '0', 'app',
     '    pm_net_server_link_connection_failed();', '    /* Deliberately removed failure notification. */')]
for label, case, target, old, new in negative_cases:
    directory = mutations / label
    directory.mkdir(exist_ok=True)
    text = {'owner': owner_text, 'hint': hint_text, 'app': main_text}[target]
    assert text.count(old) == 1, (label, 'mutation anchor changed')
    changed = text.replace(old, new, 1)
    # Reuse the native command assembled above without the sanitizer switches.
    base = app_command if target == 'app' else command
    mutant_command = [str(v) for v in base if str(v) not in
                      ('-fsanitize=address,undefined', '-fno-omit-frame-pointer')]
    mutant_binary = directory / 'test'
    mutant_command[-1] = str(mutant_binary)
    if target == 'owner':
        (directory / 'net.c').write_text(net_text)
        (directory / 'server_link_owner.inc').write_text(changed)
        mutant_command.insert(1, '-DPM_CACHE_OWNER_NET_SOURCE="' + str(directory / 'net.c') + '"')
    elif target == 'hint':
        path = directory / 'server_link_hint.c'
        path.write_text(changed)
        mutant_command[mutant_command.index('src/server_link_hint.c')] = str(path)
    else:
        path = directory / 'main.c'
        path.write_text(changed)
        mutant_command.insert(1, '-DPM_CACHE_APP_MAIN_SOURCE="' + str(path) + '"')
    if run('compile-negative-' + label, mutant_command):
        args = [mutant_binary, case, '10'] if target == 'app' else [mutant_binary, case]
        run('reject-negative-' + label, args, expected_abort=True)
passed = len(results) == 2 * (11 + len(cases)) + 2 * len(negative_cases) and all(r['passed'] for r in results)
(out / 'summary.json').write_text(json.dumps(dict(passed=passed, steps=len(results),
    scope='Production net owner/cache aggregate plus actual main-loop deadlines, deterministic TCP/ARP/4KiB RAM IO; four regression mutations rejected. No device, network, flash, or RF proof.'), indent=2) + '\n')
raise SystemExit(0 if passed else 1)
