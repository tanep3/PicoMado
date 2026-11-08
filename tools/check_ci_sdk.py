#!/usr/bin/env python3
"""Read-only validation of the SDK used by CI's existing firmware host tests."""
import argparse
from pathlib import Path
import re
import subprocess
import sys


REQUIRED_FILES = (
    'lib/lwip/src/core/ipv4/etharp.c',
    'lib/lwip/src/core/ipv4/ip4_addr.c',
    'lib/lwip/src/core/def.c',
    'lib/lwip/src/core/stats.c',
    'lib/lwip/src/include/lwip/etharp.h',
    'lib/lwip/src/include/lwip/opt.h',
    'lib/cyw43-driver/src/cyw43_ll.c',
    'src/rp2_common/pico_flash/flash.c',
    'src/rp2_common/pico_flash/include/pico/flash.h',
)


def git_output(sdk, *args):
    try:
        result = subprocess.run(
            ['git', '-C', str(sdk), *args], check=True, text=True,
            capture_output=True, timeout=30,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError) as error:
        raise ValueError(f'Cannot inspect Pico SDK with git: {error}') from error
    return result.stdout


def validate(sdk, expected_revision):
    sdk = Path(sdk).resolve()
    if not re.fullmatch(r'[0-9a-f]{40}', expected_revision):
        raise ValueError('Expected SDK revision must be a full 40-character commit SHA')
    if not sdk.is_dir():
        raise ValueError(f'Pico SDK directory is missing: {sdk}; check PICO_SDK_PATH and SDK checkout')
    revision = git_output(sdk, 'rev-parse', 'HEAD').strip()
    if revision != expected_revision:
        raise ValueError(f'Pico SDK revision mismatch: expected {expected_revision}, got {revision}')
    modules = git_output(sdk, 'submodule', 'status', '--recursive').splitlines()
    if not modules or any(not line.startswith(' ') for line in modules):
        raise ValueError('Pico SDK submodules are missing or differ from their pinned revisions; use recursive checkout')
    if git_output(sdk, 'status', '--porcelain', '--untracked-files=no').strip():
        raise ValueError('Pico SDK or its submodules have tracked changes; use an unchanged SDK checkout')
    missing = [name for name in REQUIRED_FILES if not (sdk / name).is_file()]
    if missing:
        raise ValueError('Pico SDK test dependencies are missing: ' + ', '.join(missing))
    return revision


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sdk', required=True, type=Path)
    parser.add_argument('--revision', required=True)
    args = parser.parse_args()
    try:
        revision = validate(args.sdk, args.revision)
    except ValueError as error:
        print(f'CI SDK preflight failed: {error}', file=sys.stderr)
        return 1
    print(f'CI SDK preflight passed: {revision}; pinned submodules and host-test sources present')
    return 0


if __name__ == '__main__':
    sys.exit(main())
