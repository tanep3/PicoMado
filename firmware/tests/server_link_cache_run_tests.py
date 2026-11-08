#!/usr/bin/env python3
"""Serial offline tests of production server-link cache codec, owner and SDK adapter."""
import argparse
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
stubs = out / 'sdk-stubs'
stubs.mkdir(exist_ok=True)
(stubs / 'test_sdk.h').write_text('''#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define PICO_FLASH_SIZE_BYTES (2u*1024u*1024u)
#define FLASH_SECTOR_SIZE 4096u
#define FLASH_PAGE_SIZE 256u
#define PICO_OK 0
#define __not_in_flash_func(name) name
extern uint8_t test_flash_bytes[PICO_FLASH_SIZE_BYTES];
#define XIP_BASE ((uintptr_t)test_flash_bytes)
int flash_safe_execute(void (*fn)(void *), void *arg, uint32_t timeout);
void flash_range_erase(uint32_t offset, size_t n);
void flash_range_program(uint32_t offset, const uint8_t *page, size_t n);
''')
for header in ['hardware/flash.h', 'hardware/regs/addressmap.h', 'pico/flash.h', 'pico/stdlib.h']:
    dest = stubs / header
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text('#include "test_sdk.h"\n')
results = []
def run(label, command, fail=False):
    command = list(map(str, command))
    result = subprocess.run(command, cwd=firmware, capture_output=True, text=True,
                            timeout=180, env=dict(os.environ, ASAN_OPTIONS='detect_leaks=0'))
    (out / (label + '.log')).write_text(result.stdout + result.stderr)
    results.append(dict(label=label, command=command, exit=result.returncode, expected_failure=fail))
    (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    if (result.returncode != 0) != fail:
        raise SystemExit(label + '\n' + result.stdout + result.stderr)
    if not label.startswith('compile'):
        print(label + ': ' + result.stdout.strip(), flush=True)

for variant, sanitizer in [('native', []), ('sanitized', ['-fsanitize=address,undefined', '-fno-omit-frame-pointer'])]:
    cc = ['cc', '-std=c11', '-O1', '-g', '-Wall', '-Wextra', '-Werror', '-Isrc'] + sanitizer
    for label, files, includes in [
        ('storage', ['tests/server_link_cache_test.c', 'src/server_link_cache.c'], []),
        ('sdk-adapter', ['tests/server_link_cache_flash_test.c', 'src/server_link_cache.c', 'src/server_link_cache_flash.c'], ['-I' + str(stubs)]),
    ]:
        name = label + '-' + variant
        binary = out / name
        run('compile-' + name, cc + includes + files + ['-o', binary])
        run(name, [binary])
for macro in ['LIB_PICO_MULTICORE', 'LIB_FREERTOS_KERNEL']:
    run('compile-reject-' + macro, ['cc', '-std=c11', '-Isrc', '-I' + str(stubs), '-D' + macro + '=1',
                                   '-fsyntax-only', 'src/server_link_cache_flash.c'], True)
for macros in [['-DPM_SERVER_LINK_CACHE=2'], ['-DPM_SERVER_LINK_CACHE=1', '-DPM_SERVER_LINK_HINT=0']]:
    run('compile-reject-option-' + str(len(macros)), ['cc', '-std=c11', '-Isrc'] + macros +
        ['-fsyntax-only', 'src/server_link_cache.c'], True)
(out / 'summary.json').write_text(json.dumps(dict(passed=True, steps=len(results),
    scope='Actual storage aggregate/codec and SDK adapter with RAM-only flash; not hardware timing, durability or RF acceptance'), indent=2) + '\n')
print('PASS server link cache', len(results), 'steps')
