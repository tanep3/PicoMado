import importlib.util
from pathlib import Path
import tempfile
import sys
import unittest
spec = importlib.util.spec_from_file_location('event_stream', Path(__file__).resolve().parents[1]/'tools/event_stream.py')
ev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ev)
fixture = Path(sys.argv[1]).read_bytes() if len(sys.argv)>1 else None
sys.argv = [sys.argv[0]]


class Tests(unittest.TestCase):
    def test_c_encoder_fragmentation_loss_and_stop_resume(self):
        self.assertIsNotNone(fixture)
        lines = ev.Lines()
        rows = []
        for at in range(0, len(fixture), 3):
            for line in lines.feed(fixture[at:at+3]):
                value = ev.decode(line)
                if value:
                    rows.append(value)
        self.assertEqual([r['sequence'] for r in rows], list(range(2, 18))+[1002, 1003, 1004, 1005])
        self.assertEqual(rows[-1]['dropped'], 984)
        self.assertEqual(rows[0]['us'], 1<<40)
        self.assertEqual(rows[-1]['operation'], 1)

    def test_c_tagged_status_and_maximum_width(self):
        line = next(x+b'\n' for x in fixture.splitlines() if x.startswith(b'ev1 s '))
        status = ev.parse_status(line)
        self.assertEqual(status['cid'], (1<<64)-1)
        self.assertEqual(status['sequence'], 1005)
        maximum = ('ev1 s '+' '.join(['ffffffffffffffff']*6)+' 1 1\n').encode()
        self.assertLess(len(maximum), 144)
        self.assertEqual(ev.parse_status(maximum)['dropped'], (1<<64)-1)

    def test_corruption_and_line_resync(self):
        good = next(line+b'\n' for line in fixture.splitlines() if len(line)==165)
        bad = bytearray(good)
        bad[50] = ord('f') if bad[50]!=ord('f') else ord('0')
        with self.assertRaises(ValueError):
            ev.decode(bytes(bad))
        lines = ev.Lines()
        self.assertEqual(list(lines.feed(b'x'*10000+b'\n'+good)), [good])
        self.assertEqual(lines.errors, 1)

    def test_command_partial_output_and_no_automatic_retry(self):
        mux = ev.Commands()
        first = mux.submit('SIM STATUS 1234')
        second = mux.submit('EV1 STATUS', 'ev1 status ')
        sent = bytearray()
        def write(data):
            sent.extend(data[:2])
            return min(2, len(data))
        for _ in range(20):
            mux.tick(write, 0)
        self.assertEqual(sent, b'SIM STATUS 1234\n')
        mux.receive(b'ev1 status on=1\n')
        self.assertFalse(first.done())
        mux.receive(b'sim status phase=0\n')
        self.assertTrue(first.done())
        mux.tick(write, 1)
        mux.tick(write, 5)
        with self.assertRaises(TimeoutError):
            second.result()
        with self.assertRaises(RuntimeError):
            mux.submit("SIM STATUS 1234")

    def test_storage_limit_is_explicit_and_files_are_exclusive(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'events'
            journal = ev.Journal(path, max_bytes=1)
            self.assertTrue(journal.submit(dict(kind='test')))
            self.assertFalse(journal.close())
            self.assertEqual(journal.error, 'journal byte limit')
            self.assertEqual(journal.durable, 0)
            with self.assertRaises(FileExistsError):
                ev.Journal(path)

    def test_storage_durable_watermark_after_fsync(self):
        with tempfile.TemporaryDirectory() as temp:
            journal = ev.Journal(Path(temp)/'events')
            for i in range(10):
                self.assertTrue(journal.submit(dict(kind='test', value=i)))
            self.assertTrue(journal.close())
            self.assertEqual(journal.durable, 10)


unittest.main()
