"""Operation admission fixtures only; no USB and no production approvals."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
import fixed_operation as op

class FixedOperationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        import hashlib
        self.sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
        def put(name, value):
            p = self.root/name; p.write_text(json.dumps(value)); return p
        self.put = put
        self.config = self.root/'config'; self.config.write_text('fixture')
        self.board = self.root/'board'; self.board.write_text('0000000000000001')
        self.image = self.root/'image'; self.image.write_bytes(b'fixture')
        # Patch only the fixed binary hash, historical subject and underlying lineage IO.
        self.artifact_patch = patch.object(op, 'ARTIFACT', self.sha(self.image)); self.artifact_patch.start(); self.addCleanup(self.artifact_patch.stop)
        self.report = put('check', dict(source_commit=op.SOURCE, subject_digest='subject'))
        self.receipt = put('receipt', dict(source_commit=op.SOURCE, build=dict(mode='normal', device=op.DEVICE, profile=op.PROFILE)))
        self.candidate = put('candidate', dict(kind='firmware', device=op.DEVICE, board_id=self.board.read_text(), config_SHA256=self.sha(self.config),
            artifact_SHA256=op.ARTIFACT, build_receipt=str(self.receipt), build_receipt_SHA256=self.sha(self.receipt),
            check_report=str(self.report), check_report_SHA256=self.sha(self.report), subject_digest='subject'))
        self.original = put('original', dict(reviewer='firmware reviewer', source_commit=op.SOURCE, subject_digest='subject', artifact_SHA256=op.ARTIFACT, build_receipt_SHA256=self.sha(self.receipt)))
        ref = lambda p: dict(path=str(p), SHA256=self.sha(p))
        self.fw_review = put('firmware-review', dict(owner='parent', accepted=True, independent_review=ref(self.original),
            candidate_SHA256=self.sha(self.candidate), subject_digest='subject', artifact_SHA256=op.ARTIFACT, build_receipt_SHA256=self.sha(self.receipt)))
        self.operation = put('operation', dict(id=op.OPERATION, source_commit=op.SOURCE, device=op.DEVICE, profile=op.PROFILE,
            artifact_SHA256=op.ARTIFACT, status='active', not_before=10, expires_at=100, implementer='worker',
            candidate_SHA256=self.sha(self.candidate), firmware_review_SHA256=self.sha(self.fw_review), build_receipt_SHA256=self.sha(self.receipt)))
        self.independent = put('independent', dict(operation=op.OPERATION, accepted=True, reviewer='other', implementation_commit='head', operation_SHA256=self.sha(self.operation)))
        self.approval = put('approval', dict(owner='parent', accepted=True, operation=op.OPERATION, implementation_commit='head',
            operation_SHA256=self.sha(self.operation), independent_review=ref(self.independent)))
        self.args = SimpleNamespace(operation=self.operation, operation_review=self.approval, candidate=self.candidate,
            review=self.fw_review, artifact=self.image, config=self.config, board_id=self.board, device=op.DEVICE)
        def require(ok, message):
            if not ok: raise ValueError(message)
        def load(ref):
            require(self.sha(ref['path']) == ref['SHA256'], 'proof hash'); return json.loads(Path(ref['path']).read_text())
        self.gate = SimpleNamespace(require=require, sha=self.sha, CASE='PUBLIC-DOCS-20261027', load_proof=load,
            validate_check_proof=lambda r,s:r['subject_digest']==s, artifact=lambda *a:None,
            lineage_module=lambda:SimpleNamespace(verify=lambda *a:json.loads(self.receipt.read_text())))
    def verify(self):
        def git(args, **kw): return b'' if 'status' in args else 'head\n'
        with patch.object(op.subprocess, 'check_output', side_effect=git), patch.object(op, 'historical_subject', return_value='subject'):
            return op.verify(self.root, self.args, self.gate, now=50)
    def mutate(self, path, key, value):
        data=json.loads(path.read_text()); data[key]=value; path.write_text(json.dumps(data))
    def test_correct_candidate(self): self.assertTrue(self.verify()['verified'])
    def test_wrong_artifact(self):
        self.image.write_bytes(b'wrong')
        with self.assertRaisesRegex(ValueError, 'artifact'): self.verify()
    def test_wrong_device(self):
        self.args.device='other'
        with self.assertRaisesRegex(ValueError, 'device'): self.verify()
    def test_wrong_board(self):
        self.board.write_text('0000000000000002')
        with self.assertRaisesRegex(ValueError, 'binding'): self.verify()
    def test_wrong_config(self):
        self.config.write_text('changed')
        with self.assertRaisesRegex(ValueError, 'binding'): self.verify()
    def test_receipt_changed(self):
        self.receipt.write_text('{}')
        with self.assertRaisesRegex(ValueError, 'receipt'): self.verify()
    def test_parent_unaccepted(self):
        self.mutate(self.approval,'accepted',False)
        with self.assertRaisesRegex(ValueError, 'parent acceptance'): self.verify()
    def test_stale_implementation(self):
        self.mutate(self.approval,'implementation_commit','old')
        with self.assertRaisesRegex(ValueError, 'stale'): self.verify()
    def test_independent_unaccepted(self):
        self.mutate(self.independent,'accepted',False)
        with self.assertRaises(ValueError): self.verify()
    def test_expired(self):
        self.mutate(self.operation,'expires_at',49)
        with self.assertRaisesRegex(ValueError, 'expired'): self.verify()
    def test_revoked(self):
        self.mutate(self.operation,'status','revoked')
        with self.assertRaisesRegex(ValueError, 'revoked'): self.verify()
    def test_firmware_unaccepted(self):
        self.mutate(self.fw_review,'accepted',False)
        with self.assertRaises(ValueError): self.verify()
    def test_wrong_profile(self):
        self.mutate(self.operation,'profile','other')
        with self.assertRaisesRegex(ValueError, 'target'): self.verify()
    def test_wrong_approval_hash(self):
        self.mutate(self.approval,'operation_SHA256','wrong')
        with self.assertRaisesRegex(ValueError, 'stale'): self.verify()
    def test_self_review(self):
        self.mutate(self.independent,'reviewer','worker')
        with self.assertRaises(ValueError): self.verify()
    def test_candidate_changed(self):
        self.mutate(self.candidate,'device','other')
        with self.assertRaisesRegex(ValueError, 'candidate'): self.verify()

if __name__ == '__main__': unittest.main()
