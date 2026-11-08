#!/usr/bin/env python3
"""Exercise the installed SDK's single-core flash helper without physical flash."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--sdk', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--compile-commands', type=Path)
a = p.parse_args()
sdk, out = a.sdk.resolve(), a.out.resolve()
out.mkdir(parents=True, exist_ok=True)
flash = sdk / 'src/rp2_common/pico_flash/flash.c'
inc = sdk / 'src/rp2_common/pico_flash/include'
header = inc / 'pico/flash.h'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
before = {str(p): sha(p) for p in (flash, header)}
stubs = out / 'stubs'
(stubs / 'hardware').mkdir(parents=True, exist_ok=True)
(stubs / 'pico').mkdir(exist_ok=True)
(stubs / 'pico.h').write_text('''#pragma once
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#define __unused __attribute__((unused))
#define NUM_CORES 2
#define PICO_OK 0
#define PICO_ERROR_NOT_PERMITTED -4
unsigned get_core_num(void);
''')
(stubs / 'hardware/sync.h').write_text('''#pragma once
#include <stdint.h>
uint32_t save_and_disable_interrupts(void);
void restore_interrupts_from_disabled(uint32_t);
''')
(stubs / 'pico/time.h').write_text('/* No timing API is used in this SDK branch. */\n')
source = out / 'fixture.c'
source.write_text('''#include "pico/flash.h"
#include <stdio.h>
#if PICO_FLASH_SAFE_EXECUTE_SUPPORT_MULTICORE_LOCKOUT || PICO_FLASH_SAFE_EXECUTE_SUPPORT_FREERTOS_SMP || PICO_FLASH_ASSUME_CORE0_SAFE || PICO_FLASH_ASSUME_CORE1_SAFE
#error "Fixture must exercise the default single-core branch without safety overrides"
#endif
static uint32_t irq_enabled = 1;
static unsigned enters, exits, effects;
unsigned get_core_num(void) { return 0; }
uint32_t save_and_disable_interrupts(void) {
    uint32_t previous = irq_enabled; irq_enabled = 0; ++enters; return previous;
}
void restore_interrupts_from_disabled(uint32_t previous) {
    assert(!irq_enabled); irq_enabled = previous; ++exits;
}
static void effect(void *arg) { assert(!irq_enabled); assert(arg == &effects); ++effects; }
static void execute(uint32_t timeout) {
    uint32_t previous = irq_enabled;
    unsigned before = effects;
    assert(flash_safe_execute(effect, &effects, timeout) == PICO_OK);
    assert(irq_enabled == previous && effects == before + 1 && enters == exits);
}
int main(void) {
    /* The product has not called core_init before its first save. */
    execute(0); execute(20); execute(UINT32_MAX);
    assert(flash_safe_execute_core_init()); execute(20);
    assert(flash_safe_execute_core_deinit()); execute(20);
    irq_enabled = 0; execute(20); assert(!irq_enabled);
    assert(effects == 6 && enters == 6 && exits == 6);
    puts("PASS actual SDK single-core: first execute without core init, no timeout/lockout, IRQ restoration");
}
''')
results = []
def run(label, command, cwd=None):
    command = list(map(str, command))
    r = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=90,
                       env=dict(os.environ, ASAN_OPTIONS='detect_leaks=0'))
    (out / (label + '.log')).write_text(r.stdout + r.stderr)
    results.append(dict(label=label, command=command, exit=r.returncode))
    if r.returncode:
        raise SystemExit(label + '\n' + r.stdout + r.stderr)
    return r.stdout
for name, extra in [('native', []), ('sanitized', ['-fsanitize=address,undefined', '-fno-omit-frame-pointer'])]:
    binary = out / name
    run('compile-' + name, ['cc', '-std=c11', '-O1', '-g', '-Wall', '-Wextra', '-Werror',
        '-I' + str(stubs), '-I' + str(inc), *extra, source, flash, '-o', binary])
    print(run(name, [binary]).strip())
actual_flags = False
if a.compile_commands:
    rows = json.loads(a.compile_commands.read_text())
    row = next(r for r in rows if Path(r['file']).resolve() == flash)
    args = shlex.split(row['command'])
    i = args.index('-o'); del args[i:i + 2]
    args.remove('-c')
    macros = run('actual-sdk-macros', args + ['-E', '-dM'], row['directory'])
    for macro in ('LIB_PICO_MULTICORE', 'LIB_FREERTOS_KERNEL',
                  'PICO_FLASH_SAFE_EXECUTE_SUPPORT_MULTICORE_LOCKOUT',
                  'PICO_FLASH_SAFE_EXECUTE_SUPPORT_FREERTOS_SMP',
                  'PICO_FLASH_SAFE_EXECUTE_USE_FREERTOS_SMP',
                  'PICO_FLASH_ASSUME_CORE0_SAFE', 'PICO_FLASH_ASSUME_CORE1_SAFE'):
        lines = [line.split(maxsplit=2)[2] for line in macros.splitlines()
                 if line.startswith('#define ' + macro + ' ')]
        assert not lines or lines == ['0'], (macro, lines)
    preprocessed = run('actual-sdk-preprocessed', args + ['-E', '-P'], row['directory'])
    body = preprocessed[preprocessed.index('default_core_init_deinit(') :]
    assert 'multicore_lockout_ready' not in body and 'xTaskCreate' not in body
    assert 'PICO_ERROR_TIMEOUT' not in body and 'make_timeout_time_ms' not in body
    assert 'irq_state[get_core_num()] = save_and_disable_interrupts();' in body
    assert 'restore_interrupts_from_disabled(irq_state[get_core_num()]);' in body
    actual_flags = True
assert all(sha(Path(path)) == digest for path, digest in before.items())
(out / 'summary.json').write_text(json.dumps(dict(passed=True, steps=len(results),
    actual_compile_flags_checked=actual_flags, SDK_hashes=before, results=results,
    scope='Actual SDK flash helper/header, fake local IRQ operations. No hardware flash, multicore or RF timing proof.'), indent=2) + '\n')
print('PASS SDK flash safety', len(results), 'steps; actual compile flags:', actual_flags)
