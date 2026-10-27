"""Bounded, offline admission of an independently approved historical LCD build."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time
from types import SimpleNamespace

OPERATION = 'LCD-FIXED-UPDATE-20261027'
SOURCE = '17e231ffa9db9b9cc9d93ecd55444eae0b7f26b1'
DEVICE = 'example-lcd'
PROFILE = 'pico_w_lcd_160x80_4'
ARTIFACT = '7e0dce0b72b1676604705a0dc6a01015014895a88ffc5abfc2bea4467d7cde41'


def historical_subject(root, gate):
    # Materialize tracked files only; never copy private device configuration.
    with tempfile.TemporaryDirectory(prefix='picomado-source-') as directory:
        snapshot = Path(directory)
        entries = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', SOURCE], cwd=root).decode().splitlines()
        values = {}
        for name in entries:
            body = subprocess.check_output(['git', 'show', SOURCE+':'+name], cwd=root)
            target = snapshot/name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(body)
            values[name] = hashlib.sha256(body).hexdigest()
        context = json.loads((snapshot/'contracts/development-context.json').read_text())
        change = 'docs/changes/'+context['active_change']+'.json'
        record = json.loads((snapshot/change).read_text())
        intent = gate.workflow_module(snapshot).intent(record)
        values[change] = hashlib.sha256(json.dumps(intent, sort_keys=True).encode()).hexdigest()
        return hashlib.sha256(json.dumps(values, sort_keys=True).encode()).hexdigest()


def verify(root, args, gate, now=None):
    root = Path(root)
    require = gate.require
    operation_path = Path(args.operation)
    operation = json.loads(operation_path.read_text())
    approval = json.loads(Path(args.operation_review).read_text())
    require(operation.get('id') == OPERATION and operation.get('source_commit') == SOURCE,
            'unsupported fixed operation/source')
    require(operation.get('device') == DEVICE and operation.get('profile') == PROFILE
            and operation.get('artifact_SHA256') == ARTIFACT, 'unsupported fixed LCD target')
    require(operation.get('status') == 'active', 'operation revoked/inactive')
    current = time.time() if now is None else now
    require(operation['not_before'] <= current < operation['expires_at'], 'operation approval expired/not yet valid')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    require(not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root).strip(),
            'operation implementation must be committed/clean')
    require(approval.get('owner') == 'parent' and approval.get('accepted') is True
            and approval.get('operation') == OPERATION and approval.get('implementation_commit') == head
            and approval.get('operation_SHA256') == gate.sha(operation_path), 'operation parent acceptance missing/stale')
    review = gate.load_proof(approval['independent_review'])
    require(review.get('accepted') is True and review.get('reviewer')
            and review['reviewer'] != operation['implementer']
            and review.get('implementation_commit') == head and review.get('operation_SHA256') == gate.sha(operation_path)
            and review.get('operation') == OPERATION, 'independent operation review missing/stale')
    for name, path in [('candidate', args.candidate), ('firmware_review', args.review),
                       ('artifact', args.artifact)]:
        require(gate.sha(path) == operation[name+'_SHA256'], name+' differs from fixed operation')
    candidate = json.loads(Path(args.candidate).read_text())
    require(candidate['kind'] == 'firmware' and candidate['device'] == args.device == DEVICE,
            'fixed operation device/kind mismatch')
    require(candidate['artifact_SHA256'] == ARTIFACT, 'fixed operation artifact mismatch')
    require(gate.sha(candidate['build_receipt']) == operation['build_receipt_SHA256'] == candidate['build_receipt_SHA256'],
            'fixed operation receipt mismatch')
    subject = historical_subject(root, gate)
    report = json.loads(Path(candidate['check_report']).read_text())
    require(report['source_commit'] == SOURCE and gate.validate_check_proof(report, subject)
            and gate.sha(candidate['check_report']) == candidate['check_report_SHA256'], 'historical check rejected')
    require(candidate['subject_digest'] == subject, 'historical candidate subject mismatch')
    # Existing lineage verifier still checks actual SDK, compiler, flags, logs and private configuration.
    def historical_check(path, action='stage'):
        value = json.loads(Path(path).read_text())
        require(gate.validate_check_proof(value, subject), 'historical build check rejected')
        return value
    historical_gate = SimpleNamespace(CASE=gate.CASE, input_snapshot=lambda: dict(subject_digest=subject),
                                      verify_check=historical_check)
    build = gate.lineage_module().verify(root, candidate['build_receipt'], args.artifact, historical_gate)
    require(build['source_commit'] == SOURCE and build['build']['mode'] == 'normal'
            and build['build']['device'] == DEVICE and build['build']['profile'] == PROFILE, 'historical build target mismatch')
    gate.artifact(args.artifact, 'firmware')
    require(Path(args.board_id).read_text().strip().upper() == candidate['board_id']
            and gate.sha(args.config) == candidate['config_SHA256'], 'device/config binding changed')
    firmware_review = json.loads(Path(args.review).read_text())
    for key in ('subject_digest', 'artifact_SHA256', 'build_receipt_SHA256'):
        require(firmware_review.get(key) == candidate[key], 'firmware review binding mismatch')
    require(firmware_review.get('candidate_SHA256') == gate.sha(args.candidate)
            and firmware_review.get('owner') == 'parent' and firmware_review.get('accepted') is True,
            'firmware independent parent acceptance missing')
    original = gate.load_proof(firmware_review['independent_review'])
    require(original.get('reviewer') and original.get('source_commit') == SOURCE
            and all(original.get(k) == candidate[k] for k in ('subject_digest', 'artifact_SHA256', 'build_receipt_SHA256')),
            'original firmware review mismatch')
    return dict(verified=True, offline=True, physical_actions=0, kind='firmware', operation=OPERATION)
