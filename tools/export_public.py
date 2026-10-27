#!/usr/bin/env python3
"""Create a private staging tree for independent publication review, without Git history."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SAMPLE_DIRS = {'sample-a', 'sample-b', 'E-ink_0', 'E-ink_1', 'custom-wiring'}
GUIDES = {'README.md', 'first-setup.md', 'first-setup.en.md', 'hardware-support.md', 'hardware-support.en.md', 'customization.md', 'customization.en.md', 'plugin-author.md', 'plugin-author.en.md', 'release-and-recovery.md', 'release-and-recovery.en.md', 'caricature-prompts.md'}
EXACT = {'.gitignore', 'README.md', 'README.ja.md', 'LICENSE', 'firmware/CMakeLists.txt', 'firmware/diagnostics/backlight-polarity/main.c', 'firmware/lwipopts.h', 'firmware/README.md', 'service/README.md', 'service/Cargo.toml', 'service/Cargo.lock', 'contracts/observation-domain.json', 'contracts/development-boundaries.json', 'development/standard/v1/check.py', 'development/standard/v1/generate.py', 'tools/generate_observation.py', 'tools/build_lineage.py', 'tests/test_hardware_profile_validation.py', 'docs/design/eex1-endpoint-discovery.md'}
PREFIXES = ('firmware/src/', 'firmware/profiles/', 'firmware/tests/', 'firmware/tools/', 'service/src/', 'service/tests/', 'service/assets/', 'service/deploy/', 'service/examples/', 'service/tools/', 'docs/media/')


def selected(name):
    if name in EXACT: return True
    if name.startswith('docs/guides/'): return name.removeprefix('docs/guides/') in GUIDES
    if name.startswith('firmware/devices/'):
        parts = name.split('/')
        return len(parts) == 4 and parts[2] in SAMPLE_DIRS and parts[3] in {'config.sample.h', 'profile.txt'}
    return name.startswith(PREFIXES)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(); parser.add_argument('--out', required=True); args = parser.parse_args()
    out = Path(args.out).resolve()
    if out.exists() or out.is_relative_to(ROOT): raise SystemExit('choose a fresh output outside the source tree')
    data = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT)
    files = sorted(set(x.decode() for x in data.split(b'\0') if x and selected(x.decode())))
    if not files or not EXACT.issubset(files): raise SystemExit('required tracked publication inputs are missing')
    fixture = (ROOT/'firmware/tests/heartbeat_owner_runner_test.py').read_text()
    m = re.search(r"expected_flash_id=['\"]([A-F0-9]{16})['\"]", fixture)
    n = re.search(r"device=['\"]([A-Za-z0-9_-]+)['\"],expected_flash_id=", fixture)
    if not m or not n: raise SystemExit('private fixture reference could not be identified')
    replacements = {m.group(1).encode(): b'0123456789ABCDEF', n.group(1).encode(): b'example-lcd'}
    # Compare selected files against typed identifiers found in private JSON records.
    # Never print or copy the values. This supplements the allowlist, not a manual review.
    sensitive = set()
    def collect(value):
        if isinstance(value, dict):
            for key, child in value.items():
                k = key.lower()
                if (k in {'ip', 'device', 'target', 'board', 'host', 'interface', 'serial_port'} or any(t in k for t in ('flash_id', 'board_id', 'device_id', 'device_name', 'mac_address', 'serial', 'ssid', 'local_ip', 'peer_ip', 'remote_ip'))) and isinstance(child, str) and len(child) >= 7:
                    sensitive.add(child.encode())
                collect(child)
        elif isinstance(value, list):
            for child in value: collect(child)
    for entry in data.split(b'\0'):
        if not entry or not entry.endswith(b'.json') or entry.startswith((b'service/', b'contracts/')): continue
        path = ROOT/entry.decode()
        if path.is_symlink() or not path.is_file(): raise ValueError('private record missing or symlinked: '+entry.decode())
        try: collect(json.loads(path.read_text()))
        except (ValueError, UnicodeError) as exc: raise ValueError('private record is not valid JSON: '+entry.decode()) from exc
    out.mkdir(parents=True, mode=0o700)
    manifest = {}
    try:
        for name in files:
            src = ROOT/name
            if src.is_symlink() or not src.is_file(): raise ValueError('invalid source file: '+name)
            body = src.read_bytes()
            for old, new in replacements.items(): body = body.replace(old, new)
            if b'/home/' in body: raise ValueError('home path found in selected input: '+name)
            if any(value in body for value in sensitive): raise ValueError('selected input matches a private record: '+name)
            dest = out/name; dest.parent.mkdir(parents=True, exist_ok=True); dest.write_bytes(body)
            if os.access(src, os.X_OK): dest.chmod(0o700)
            manifest[name] = hashlib.sha256(body).hexdigest()
        index = '# Documentation\n\n- [日本語ガイド](guides/README.md)\n- [First setup](guides/first-setup.en.md)\n- [Plugin guide](guides/plugin-author.en.md)\n'
        (out/'docs/README.md').write_text(index)
        manifest['docs/README.md'] = hashlib.sha256(index.encode()).hexdigest()
        (out/'PUBLIC-SOURCE-MANIFEST.json').write_text(json.dumps({'schema': 1, 'files': manifest}, indent=2, sort_keys=True)+'\n')
    except BaseException:
        shutil.rmtree(out)
        raise
    print('Private staging tree created:', out)
    print('Review privacy, links and tests before distribution; this output is not an approval.')


if __name__ == '__main__': main()
