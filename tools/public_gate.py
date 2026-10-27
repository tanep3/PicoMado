#!/usr/bin/env python3
"""Owner-operated, offline build and preflight for a history-free source distribution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import build_lineage as lineage


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1048576), b''):
            h.update(chunk)
    return h.hexdigest()


def write(path, obj):
    path = Path(path).resolve()
    require(not path.exists(), 'output already exists')
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as f:
        json.dump(obj, f, sort_keys=True, indent=2)
        f.write('\n')


def source_digest(root=None):
    root = Path(root if root is not None else ROOT).resolve()
    files = {}
    samples = {'sample-a', 'sample-b', 'E-ink_0', 'E-ink_1', 'custom-wiring'}
    for p in root.rglob('*'):
        rel = p.relative_to(root)
        parts = rel.parts
        if parts[0] in {'.git', '.gate', '.venv', 'target'} or (len(parts) >= 2 and parts[:2] == ('service', 'target')) or '__pycache__' in parts:
            continue
        if len(parts) >= 2 and parts[:2] == ('firmware', 'build'):
            continue
        if len(parts) >= 3 and parts[:2] == ('firmware', 'devices') and parts[2] not in samples:
            continue
        if len(parts) >= 4 and parts[:2] == ('firmware', 'devices') and parts[-1] in {'config.h', 'board-id.txt'}:
            continue
        if p.is_symlink():
            raise ValueError('source symlink is not supported: ' + str(rel))
        if p.is_file():
            files[rel.as_posix()] = sha(p)
    require(files, 'empty source')
    return hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()


def run_logged(command, log, env=None):
    with Path(log).open('x') as f:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=f, stderr=subprocess.STDOUT)
    require(result.returncode == 0, 'check/build failed; see ' + str(log))
    return {'command': command, 'log': str(Path(log).resolve()), 'sha256': sha(log), 'exit': 0}


def verify_source():
    manifest = json.loads((ROOT/'PUBLIC-SOURCE-MANIFEST.json').read_text())
    require(manifest.get('schema') == 1 and isinstance(manifest.get('files'), dict) and manifest['files'], 'invalid distribution manifest')
    for name, digest in manifest['files'].items():
        rel = Path(name)
        require(not rel.is_absolute() and '..' not in rel.parts and bool(rel.parts), 'invalid manifest path')
        path = ROOT/rel
        require(all(not (ROOT/Path(*rel.parts[:i])).is_symlink() for i in range(1, len(rel.parts)+1)), 'distribution symlink is not supported')
        require(path.is_file() and sha(path) == digest, 'distribution file missing or changed: '+name)
    allowed = set(manifest['files']) | {'PUBLIC-SOURCE-MANIFEST.json'}
    for path in ROOT.rglob('*'):
        rel = path.relative_to(ROOT)
        if '__pycache__' in rel.parts: continue
        require(not path.is_symlink(), 'distribution symlink is not supported')
        if path.is_file(): require(rel.as_posix() in allowed, 'unexpected file in distribution: '+rel.as_posix())
    print('Distribution files match the included manifest. Verify the archive hash through a trusted channel.')


def check(args):
    out = Path(args.out).resolve()
    require(not out.exists(), 'use a fresh check directory')
    require(not out.is_relative_to(ROOT), 'check directory must be outside source')
    out.mkdir(parents=True, mode=0o700)
    before = source_digest()
    target = Path(args.cargo_target).resolve() if args.cargo_target else out/'cargo-target'
    require(not target.is_relative_to(ROOT), 'Cargo target must be outside source')
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1', CARGO_TARGET_DIR=str(target))
    checks = []
    checks.append(run_logged([sys.executable, 'tools/generate_observation.py', '--check'], out/'generated.log', env))
    checks.append(run_logged([sys.executable, 'development/standard/v1/check.py', '--root', str(ROOT), '--config', str(ROOT/'contracts/development-boundaries.json')], out/'boundaries.log', env))
    checks.append(run_logged([sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_hardware_profile_validation.py'], out/'hardware-profile.log', env))
    checks.append(run_logged([sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_public_gate.py'], out/'public-gate.log', env))
    checks.append(run_logged(['bash', 'firmware/tests/run_host_tests.sh'], out/'firmware-host.log', env))
    checks.append(run_logged(['bash', 'firmware/tests/run_usb_tx_tests.sh'], out/'firmware-usb.log', env))
    checks.append(run_logged(['cargo', 'test', '--offline', '--locked', '--all-features', '--manifest-path', 'service/Cargo.toml'], out/'rust.log', env))
    require(source_digest() == before, 'source changed during check')
    report = {'schema': 1, 'type': 'picomado-public-check', 'source': before, 'checks': checks}
    write(out/'check.json', report)
    print('Public check passed:', out/'check.json')


def verify_check(path):
    report = json.loads(Path(path).read_text())
    require(report.get('schema') == 1 and report.get('type') == 'picomado-public-check', 'invalid check report')
    require(report['source'] == source_digest(), 'source changed since check')
    checks = report['checks']
    out = Path(path).resolve().parent
    expected = [[sys.executable, 'tools/generate_observation.py', '--check'], [sys.executable, 'development/standard/v1/check.py', '--root', str(ROOT), '--config', str(ROOT/'contracts/development-boundaries.json')], [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_hardware_profile_validation.py'], [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_public_gate.py'], ['bash', 'firmware/tests/run_host_tests.sh'], ['bash', 'firmware/tests/run_usb_tx_tests.sh'], ['cargo', 'test', '--offline', '--locked', '--all-features', '--manifest-path', 'service/Cargo.toml']]
    logs = ['generated.log', 'boundaries.log', 'hardware-profile.log', 'public-gate.log', 'firmware-host.log', 'firmware-usb.log', 'rust.log']
    require(len(checks) == 7 and all(x.get('exit') == 0 and x['command'] == cmd and x['log'] == str(out/name) and sha(x['log']) == x['sha256'] for x, cmd, name in zip(checks, expected, logs)), 'check logs or commands missing or changed')
    return report


def artifact(path, kind):
    data = Path(path).read_bytes()
    require(bool(data), 'empty artifact')
    if kind == 'server':
        require(data[:4] == b'\x7fELF', 'server artifact is not ELF')
        return
    require(len(data) % 512 == 0, 'invalid UF2 length')
    for i in range(len(data)//512):
        b = data[i*512:(i+1)*512]
        a, magic, flags, address, n, index, total, family = struct.unpack_from('<8I', b)
        require(a == 0x0a324655 and magic == 0x9e5d5157 and struct.unpack_from('<I', b, 508)[0] == 0x0ab16f30 and flags & 0x2000 and family == 0xe48bff56 and index == i and total == len(data)//512 and 0 < n <= 476 and 0x10000000 <= address < address+n <= 0x101ff000, 'invalid UF2 block or range')


def firmware_target(device):
    _, _, paths = lineage.hardware_profile(ROOT, device)
    profile = paths[1].read_text()
    eink = re.search(r'^\s*#define\s+PM_DISPLAY_TYPE\s+PM_DISPLAY_EINK\b', profile, re.M)
    return 'picomado_eink' if eink else 'picomado_lcd'


def build(args):
    report = verify_check(args.report)
    out = Path(args.build_dir).resolve()
    require(not out.exists(), 'use a fresh build directory')
    features = tuple(sorted(set(x for x in args.features.split(',') if x)))
    configure = None
    require(args.kind == 'server' or not features, 'Cargo features are only supported for server builds')
    if args.kind == 'firmware':
        require(args.device is not None, 'device required')
        profile, board, profile_files = lineage.hardware_profile(ROOT, args.device)
        inputs = list(profile_files) + [ROOT/'firmware/devices'/args.device/'config.h', ROOT/'firmware/devices'/args.device/'board-id.txt']
        require(all(p.is_file() and not p.is_symlink() for p in inputs), 'device and profile inputs must be regular files')
        require(out == ROOT/'firmware/build'/args.device, 'firmware build directory must be firmware/build/DEVICE')
        command = [shutil.which('cmake'), '-S', str(ROOT/'firmware'), '-B', str(out), '-DPM_DEVICE='+args.device, '-DPM_USE_SAMPLE=OFF', '-DPM_PREPARE_UNBOUND=OFF', '-DPM_PREPARE_UNBOUND_PANEL=OFF', '-DPICO_BOARD='+board, '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON', '-DPICO_SDK_FETCH_FROM_GIT=OFF', '-DPICO_SDK_SUBMODULE_AUTO_UPDATE=OFF']
        command += ['-D'+flag+'=OFF' for flag in lineage.FIRMWARE_FLAGS]
        out.parent.mkdir(parents=True, exist_ok=True)
        configure = run_logged(command, out.parent/(out.name+'-configure.log'))
        build_command = [shutil.which('cmake'), '--build', str(out), '--target', firmware_target(args.device), '-j4', '--verbose']
        image = out/(args.device+'.uf2')
    else:
        require(not out.is_relative_to(ROOT), 'server build directory must be outside source')
        out.parent.mkdir(parents=True, exist_ok=True)
        build_command = [shutil.which('cargo'), 'build', '--offline', '--locked', '--release', '--manifest-path', str(ROOT/'service/Cargo.toml'), '--target-dir', str(out), '--bin', 'picomado-server', '--verbose']
        if features: build_command += ['--features', ','.join(features)]
        image = out/'release/picomado-server'
    before = lineage.snapshot(ROOT, args.kind, out, args.device, 'normal', features, False)
    if args.kind == 'firmware':
        require(all(before['flags'].get(flag) == 'OFF' for flag in lineage.FIRMWARE_FLAGS), 'diagnostic flags must be off')
    log = out.parent/(out.name+'-build.log')
    result = run_logged(build_command, log)
    after = lineage.snapshot(ROOT, args.kind, out, args.device, 'normal', features, False)
    require(before == after, 'build inputs or toolchain changed')
    require(source_digest() == report['source'], 'source changed during build')
    artifact(image, args.kind)
    if args.kind == 'server':
        lineage.fingerprint(ROOT, out, features, False)
    receipt = {'schema': 1, 'type': 'picomado-public-build', 'kind': args.kind, 'source': report['source'], 'check': str(Path(args.report).resolve()), 'check_sha256': sha(args.report), 'build': after, 'configure': configure, 'command': result, 'artifact': str(image), 'artifact_sha256': sha(image)}
    write(out/'public-build.json', receipt)
    print('Built:', image, 'receipt:', out/'public-build.json')


def preflight(receipt_path):
    receipt = json.loads(Path(receipt_path).read_text())
    require(receipt.get('schema') == 1 and receipt.get('type') == 'picomado-public-build', 'invalid build receipt')
    report = verify_check(receipt['check'])
    require(sha(receipt['check']) == receipt['check_sha256'] and receipt['source'] == report['source'], 'check changed')
    b = receipt['build']; kind = receipt['kind']
    require(kind in ('server', 'firmware'), 'unknown artifact kind')
    build_dir = Path(b['build_dir'])
    if kind == 'firmware':
        require(b.get('mode') == 'normal' and build_dir == ROOT/'firmware/build'/b['device'], 'firmware build mode or directory mismatch')
        require(all(b['flags'].get(flag) == 'OFF' for flag in lineage.FIRMWARE_FLAGS), 'diagnostic firmware is not a public build')
        require(all(Path(path).is_file() and not Path(path).is_symlink() for path in b['configuration']), 'device or profile input missing or symlinked')
        configure = receipt.get('configure')
        expected_configure = [b['builder']['path'], '-S', str(ROOT/'firmware'), '-B', str(build_dir), '-DPM_DEVICE='+b['device'], '-DPM_USE_SAMPLE=OFF', '-DPM_PREPARE_UNBOUND=OFF', '-DPM_PREPARE_UNBOUND_PANEL=OFF', '-DPICO_BOARD='+b['board'], '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON', '-DPICO_SDK_FETCH_FROM_GIT=OFF', '-DPICO_SDK_SUBMODULE_AUTO_UPDATE=OFF'] + ['-D'+flag+'=OFF' for flag in lineage.FIRMWARE_FLAGS]
        require(isinstance(configure, dict) and configure.get('command') == expected_configure and configure.get('exit') == 0 and configure.get('log') == str(build_dir.parent/(build_dir.name+'-configure.log')) and sha(configure['log']) == configure['sha256'], 'configure command or log changed')
    else:
        require(receipt.get('configure') is None, 'unexpected configure command')
    expected_artifact = build_dir/(b['device']+'.uf2') if kind == 'firmware' else build_dir/'release/picomado-server'
    require(receipt['artifact'] == str(expected_artifact), 'unexpected artifact path')
    expected_cmd = [b['builder']['path'], '--build', str(build_dir), '--target', firmware_target(b['device']), '-j4', '--verbose'] if kind == 'firmware' else [b['builder']['path'], 'build', '--offline', '--locked', '--release', '--manifest-path', str(ROOT/'service/Cargo.toml'), '--target-dir', str(build_dir), '--bin', 'picomado-server', '--verbose']
    if kind == 'server' and b['features']: expected_cmd += ['--features', ','.join(b['features'])]
    require(receipt['command']['command'] == expected_cmd, 'unexpected builder command')
    now = lineage.snapshot(ROOT, kind, b['build_dir'], b.get('device'), b.get('mode', 'normal'), tuple(b.get('features', [])), b.get('no_default', False))
    require(now == b, 'configuration, SDK, or build tool changed')
    require(receipt['command']['exit'] == 0 and receipt['command']['log'] == str(build_dir.parent/(build_dir.name+'-build.log')) and sha(receipt['command']['log']) == receipt['command']['sha256'], 'build log changed')
    artifact(receipt['artifact'], kind)
    require(sha(receipt['artifact']) == receipt['artifact_sha256'], 'artifact changed')
    if kind == 'server': lineage.fingerprint(ROOT, Path(b['build_dir']), tuple(b['features']), b['no_default'])
    return receipt


def flash(args):
    receipt = preflight(args.receipt)
    require(receipt['kind'] == 'firmware' and receipt['build']['device'] == args.device, 'selected device mismatch')
    board_file = ROOT/'firmware/devices'/args.device/'board-id.txt'
    expected = board_file.read_text().strip().upper()
    require(re.fullmatch('[0-9A-F]{16}', expected), 'invalid board ID')
    image = receipt['artifact']
    info = subprocess.check_output(['picotool', 'info', '-a', image], text=True)
    require(re.search(r'^\s*pico_board:\s*pico_w\s*$', info, re.M), 'UF2 board does not match Pico W')
    devices = subprocess.check_output(['picotool', 'info', '-d'], text=True)
    require(len(re.findall(r'^Device Information$', devices, re.M)) == 1, 'connect exactly one BOOTSEL device')
    ids = re.findall(r'^\s*flash id:\s*0x([0-9a-fA-F]{16})', devices, re.M)
    require(len(ids) == 1 and ids[0].upper() == expected, 'connected board ID mismatch')
    preflight(args.receipt)
    print('Connected board and artifact match.')
    if not args.check_only:
        subprocess.run(['picotool', 'load', '-v', '-x', image], check=True)


def main():
    os.umask(0o077)
    p = argparse.ArgumentParser(); sub = p.add_subparsers(dest='op', required=True)
    sub.add_parser('verify-source')
    c = sub.add_parser('check'); c.add_argument('--out', required=True); c.add_argument('--cargo-target')
    c = sub.add_parser('build'); c.add_argument('--kind', choices=['server','firmware'], required=True); c.add_argument('--report', required=True); c.add_argument('--build-dir', required=True); c.add_argument('--device'); c.add_argument('--features', default='')
    c = sub.add_parser('preflight'); c.add_argument('--receipt', required=True)
    c = sub.add_parser('flash'); c.add_argument('--receipt', required=True); c.add_argument('--device', required=True); c.add_argument('--check-only', action='store_true')
    args = p.parse_args()
    try:
        require(not (ROOT/'.git').exists() and (ROOT/'PUBLIC-SOURCE-MANIFEST.json').is_file(), 'public route requires the history-free distribution; use the internal gate in the development repository')
        if args.op == 'verify-source': verify_source()
        elif args.op == 'check': check(args)
        elif args.op == 'build': build(args)
        elif args.op == 'preflight': preflight(args.receipt); print('Offline preflight passed; no device operation performed.')
        else: flash(args)
    except (Exception, subprocess.CalledProcessError) as e:
        print('PUBLIC GATE REJECTED:', e, file=sys.stderr); raise SystemExit(1)


if __name__ == '__main__': main()
