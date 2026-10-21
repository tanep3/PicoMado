import errno
import importlib.util
import json
import os
from pathlib import Path
import select
import socket
import sys
import tempfile
import threading
import time
import unittest
import zlib

spec = importlib.util.spec_from_file_location('events', Path(__file__).resolve().parents[1]/'tools/event_stream.py')
ev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ev)


def packet(seq, boot=1, dropped=0):
    body = ev.BODY.pack(2, 12, 4, boot, seq, seq*1000, seq, 0, 1, 2, dropped, 0, 0)
    return b'ev1 '+body.hex().encode()+b' '+('%08x' % zlib.crc32(body)).encode()+b'\n'


class Device:
    """Only a freshly allocated PTY. No USB path is accepted by this fixture."""
    def __init__(self, events=None, boot=1, dropped=0, disconnect=False, hook=None, initial_sequence=0):
        self.master, self.slave = os.openpty()
        self.path = os.ttyname(self.slave)
        assert self.path.startswith('/dev/pts/')
        os.set_blocking(self.master, False)
        self.events = [packet(1), packet(2)] if events is None else events
        self.boot, self.dropped = boot, dropped
        self.seq = initial_sequence
        self.on = False
        self.suppress_on_reply = 0
        self.disconnect, self.hook = disconnect, hook
        self.commands = []
        self.stop = threading.Event()
        self.errors = []
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def send(self, data):
        for at in range(0, len(data), 7):
            chunk = data[at:at+7]
            while chunk and not self.stop.is_set():
                try:
                    n = os.write(self.master, chunk)
                    chunk = chunk[n:]
                except BlockingIOError:
                    time.sleep(0.001)

    def run(self):
        pending = bytearray()
        try:
            while not self.stop.is_set():
                if self.master is None or not select.select([self.master], [], [], 0.02)[0]:
                    continue
                data = os.read(self.master, 1024)
                for b in data:
                    if b != 10:
                        pending.append(b)
                        continue
                    command = pending.decode().strip()
                    pending.clear()
                    self.commands.append(command)
                    words = command.split()
                    if words[0] == 'EV1':
                        if words[1] in ('ON','OFF'):
                            self.on = words[1]=='ON'
                        on = self.on
                        if words[1]=='ON' and self.suppress_on_reply:
                            self.suppress_on_reply-=1
                        else:
                            self.send(('ev1 s %s %x %x %x 0 0 %u 0\n' %
                                       (words[2], self.boot, self.seq, self.dropped, on)).encode())
                        if words[1] == 'ON':
                            for event in self.events:
                                self.send(event)
                                try:
                                    fact = ev.decode(event)
                                    if fact:
                                        self.seq = max(self.seq, fact['sequence'])
                                except ValueError:
                                    self.seq += 1
                            if self.hook:
                                self.hook(self)
                            if self.disconnect:
                                time.sleep(.03)
                                os.close(self.master)
                                self.master = None
                                return
                    elif command.startswith('SIM STATUS'):
                        self.send(b'sim status phase=0\n')
                    else:
                        self.send(b'usb error=syntax\n')
        except OSError as error:
            if not self.stop.is_set():
                self.errors.append(error)

    def open(self, ignored):
        return ev.SerialPort(self.path, dtr=False)

    def close(self):
        self.stop.set()
        self.thread.join(1)
        if self.master is not None:
            os.close(self.master)
            self.master = None
        os.close(self.slave)
        assert not self.thread.is_alive()


def rpc(path, value):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(2)
        client.connect(str(path))
        client.sendall((json.dumps(value)+'\n').encode())
        reply = bytearray()
        while not reply.endswith(b'\n'):
            reply.extend(client.recv(4096))
        return json.loads(reply)


class Tests(unittest.TestCase):
    def test_causal_readiness_requires_crc_begin_catalog_and_durable_prefix(self):
        class Storage:
            def status(self):return dict(durable_prefix=self.prefix,queue_lost=0,write_lost=0,error=None)
        owner=object.__new__(ev.Collector);owner.window=ev.Window(1);owner.link_epoch=1
        owner.stop_requested=False;owner.journal=Storage();owner.journal.prefix=0
        start=dict(boot=1,sequence=4,dropped=99,on=1,saturated=0)
        owner.window.begin(start)
        self.assertTrue(owner.status()['ready']);self.assertFalse(owner.status()['causal_ready'])
        body=ev.BODY.pack(8,1,1,1,4,1000,4,0,0,0,99,3,0x9f)
        raw=b'ev1 '+body.hex().encode()+b' '+('%08x'%zlib.crc32(body)).encode()+b'\n'
        owner.window.accept(ev.decode(raw));owner.window.begin_host_sequence=10
        self.assertFalse(owner.status()['causal_ready'])
        owner.journal.prefix=10;self.assertTrue(owner.status()['causal_ready'])
        owner.window.begin_fact['a']=1;self.assertFalse(owner.status()['causal_ready'])
        damaged=bytearray(raw);damaged[10]=ord('f') if damaged[10]!=ord('f') else ord('e')
        with self.assertRaises(ValueError):ev.decode(bytes(damaged))

    def test_slow_port_factory_and_dtr_prelude_do_not_age_command_deadline(self):
        from unittest.mock import patch
        device=Device();real=time.monotonic;offset=[0]
        def opened(path):
            port=device.open(path);offset[0]+=5
            device.send(b'timing snapshot prelude\n'*64)
            return port
        try:
            with tempfile.TemporaryDirectory() as temp,patch.object(ev.time,'monotonic',side_effect=lambda:real()+offset[0]):
                root=Path(temp)
                result=ev.collect('new-PTY-only',root/'events',root/'owner',.12,port_factory=opened)
                self.assertTrue(result['transport_complete'],result)
                self.assertEqual([c.split()[1] for c in device.commands],['STATUS','ON','OFF'])
                rows=[json.loads(line) for line in (root/'events').read_text().splitlines()]
                self.assertEqual(sum(r['kind']=='prelude_line' and r['line']=='timing snapshot prelude' for r in rows),64)
                self.assertFalse(any(r['kind']=='command_uncertain' for r in rows))
        finally:device.close()

    def test_uncertain_on_is_reconciled_by_status_off_then_new_nonce(self):
        from unittest.mock import patch
        real=time.monotonic;offset=[0]
        def hook(device):
            if device.events:
                offset[0]+=5;device.events=[]
        device=Device(hook=hook);device.suppress_on_reply=1
        try:
            with tempfile.TemporaryDirectory() as temp,patch.object(ev.time,'monotonic',side_effect=lambda:real()+offset[0]):
                root=Path(temp)
                result=ev.collect('new-PTY-only',root/'events',root/'owner',.12,port_factory=device.open)
                self.assertFalse(result['transport_complete'])
                self.assertTrue(result['segments'][-1]['transport_complete'],result)
                self.assertEqual([c.split()[1] for c in device.commands],['STATUS','ON','STATUS','OFF','ON','OFF'])
                self.assertEqual(len(set(c.split()[2] for c in device.commands)),6)
                self.assertEqual(result['command_uncertain'],1)
        finally:device.close()

    def capture(self, device, **options):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            result = ev.collect('fixture-only', path/'events', path/'owner', .12,
                                port_factory=device.open, **options)
            rows = [json.loads(s) for s in (path/'events').read_text().splitlines()]
            self.assertEqual(json.loads((path/'events.summary.json').read_text()), result)
            self.assertFalse((path/'owner').exists())
            return result, rows

    def test_real_pty_owner_fragmentation_ipc_exclusivity_and_seal(self):
        device = Device()
        answers, threads = [], []
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def ready(status):
                self.assertTrue(status['ready'])
                with self.assertRaises(OSError):
                    os.open(device.path, os.O_RDWR | os.O_NOCTTY)
                def client():
                    answers.append(rpc(root/'owner', {'command': 'SIM STATUS 1'}))
                    answers.append(rpc(root/'owner', {'op': 'stop'}))
                t = threading.Thread(target=client)
                threads.append(t)
                t.start()
            try:
                result = ev.collect('fixture-only', root/'events', root/'owner', 1,
                                    port_factory=device.open, ready=ready)
                threads[0].join(2)
                self.assertFalse(threads[0].is_alive())
                self.assertTrue(result['transport_complete'], result)
                self.assertFalse(result['causal_complete']) # fixture lacks source begin/end
                self.assertTrue(result['storage_durable'])
                self.assertIsNone(result['application_complete'])
                self.assertEqual(answers[0]['replies'], ['sim status phase=0'])
                self.assertEqual(answers[1]['state'], 'stopping')
                self.assertEqual([c.split()[1] for c in device.commands if c.startswith('EV1')], ['STATUS', 'ON', 'OFF'])
                self.assertEqual(len([c for c in device.commands if c.startswith('SIM')]), 1)
            finally:
                device.close()

    def test_missing_duplicate_crc_overlong_and_drop_are_incomplete(self):
        bad = bytearray(packet(2)); bad[10] = ord('f')
        variants = [[packet(1), packet(3)], [packet(1), packet(1), packet(2)],
                    [packet(1), bytes(bad)], [b'x'*10000+b'\n'], [packet(1, dropped=1)]]
        for events in variants:
            with self.subTest(events=[len(x) for x in events]):
                d = Device(events=events)
                try:
                    result, _ = self.capture(d)
                    self.assertFalse(result['transport_complete'])
                    self.assertTrue(result['segments'][0]['reasons'])
                finally:
                    d.close()

    def test_reconnect_is_a_new_segment_and_never_replays_commands(self):
        for new_boot in [1, 2]:
            first = Device(events=[packet(1)], disconnect=True)
            second = Device(events=[packet(2 if new_boot==1 else 1, boot=new_boot)], boot=new_boot, initial_sequence=1 if new_boot==1 else 0)
            devices = iter([first, second])
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                try:
                    result = ev.collect('same-requested-path', root/'events', root/'owner', .12,
                                        reconnects=1, port_factory=lambda path: next(devices).open(path))
                    self.assertFalse(result['transport_complete'])
                    self.assertTrue(result['interrupted'])
                    self.assertEqual(len(result['segments']), 2)
                    self.assertTrue(result['segments'][1]['transport_complete'], result)
                    self.assertEqual(result['reconnect_attempts'], 1)
                    self.assertTrue(all(c.startswith('EV1') for d in (first, second) for c in d.commands))
                    self.assertNotEqual(first.commands[0].split()[2], second.commands[0].split()[2])
                finally:
                    first.close(); second.close()

    def test_same_boot_sequence_reset_is_not_a_complete_segment(self):
        first = Device(events=[packet(1)], disconnect=True)
        second = Device(events=[packet(1)])
        devices = iter([first, second])
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            try:
                result = ev.collect('same-requested-path', root/'events', root/'owner', .15,
                                    reconnects=1, port_factory=lambda path: next(devices).open(path))
                self.assertFalse(result['segments'][1]['transport_complete'])
                self.assertIn('source_sequence_reset', result['segments'][1]['reasons'])
            finally:
                first.close(); second.close()

    def test_partial_eof_and_new_boot_without_reconnect_are_incomplete(self):
        for events in [[packet(1)[:20]], [packet(1, boot=2)]]:
            d = Device(events=events, disconnect=True)
            try:
                result, _ = self.capture(d)
                self.assertFalse(result['complete'])
            finally:
                d.close()

    def test_slow_disk_queue_limit_and_join_timeout_never_block_submit(self):
        with tempfile.TemporaryDirectory() as temp:
            entered, release = threading.Event(), threading.Event()
            def slow(fd, data):
                entered.set(); release.wait(2)
                return os.write(fd, data)
            journal = ev.Journal(Path(temp)/'events', capacity=2, write=slow)
            journal.submit({'kind': 'first'})
            self.assertTrue(entered.wait(1))
            start = time.monotonic()
            for i in range(100):
                journal.submit({'kind': 'flood', 'i': i})
            self.assertLess(time.monotonic()-start, .2)
            self.assertGreater(journal.queue_lost, 0)
            self.assertFalse(journal.close(.01))
            self.assertTrue(journal.status()['worker_alive'])
            release.set(); journal.thread.join(1)
            self.assertFalse(journal.thread.is_alive())

    def test_write_and_fsync_failures_never_report_durable(self):
        for name in ['write', 'fsync']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                def fail(*args):
                    raise OSError(errno.ENOSPC, 'injected disk full')
                journal = ev.Journal(Path(temp)/'events', **{name: fail})
                journal.submit({'kind': 'test'})
                self.assertFalse(journal.close())
                self.assertIn('injected disk full', journal.error)

    def test_unknown_or_disconnected_client_does_not_stop_capture(self):
        d = Device()
        clients, threads = [], []
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def ready(status):
                def client():
                    clients.append(rpc(root/'owner', []))
                    clients.append(rpc(root/'owner', {'command': 'SIM COMMIT 1 2 3'}))
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
                        s.connect(str(root/'owner')); s.sendall(b'{')
                t = threading.Thread(target=client); threads.append(t); t.start()
            try:
                result = ev.collect('fixture-only', root/'events', root/'owner', .15,
                                    port_factory=d.open, ready=ready)
                threads[0].join(1)
                self.assertTrue(result['complete'], result)
                self.assertIn('error', clients[0]); self.assertIn('error', clients[1])
                self.assertFalse(any(c.startswith('SIM COMMIT') for c in d.commands))
            finally:
                d.close()

    def test_tagged_stop_survives_late_uncorrelated_legacy_reply(self):
        mux = ev.Commands(start_id=0); first = mux.submit('SIM STATUS 1')
        sent = bytearray()
        def write(data):
            sent.extend(data); return len(data)
        mux.tick(write, 0); mux.tick(write, 4)
        with self.assertRaises(TimeoutError):
            first.result()
        stop = mux.submit('OFF', internal=True)
        mux.tick(write, 5)
        mux.receive(b'sim status stale=1\n')
        self.assertFalse(stop.done())
        mux.receive(b'ev1 s 1 1 0 0 0 0 0 0\n')
        self.assertFalse(stop.done())
        mux.receive(b'ev1 s 2 1 0 0 0 0 0 0\n')
        self.assertFalse(stop.result()['status']['on'])

    def test_reply_overflow_cancels_queued_legacy_and_only_tagged_off_can_follow(self):
        for mode in ['lines', 'bytes']:
            with self.subTest(mode=mode):
                audit, writes = [], []
                mux = ev.Commands(start_id=0, audit=lambda **v: audit.append(v))
                first = mux.submit('SIM STATUS 1', 'sim ', 'sim end ')
                second = mux.submit('SIM PRESS 1 2 3 1 100', 'sim ', 'sim end ')
                def write(data):
                    writes.append(data); return len(data)
                mux.tick(write, 0)
                for _ in range(1025 if mode=='lines' else 257):
                    mux.receive(b'sim row '+(b'x'*510 if mode=='bytes' else b'x')+b'\n')
                self.assertTrue(mux.uncertain)
                for future in [first, second]:
                    with self.assertRaises(RuntimeError):
                        future.result(timeout=0)
                mux.tick(write, .1); mux.receive(b'sim end stale=1\n')
                self.assertEqual(writes, [b'SIM STATUS 1\n'])
                self.assertTrue(any(row['kind']=='command_cancelled' and row['command_id']==2 for row in audit))
                with self.assertRaises(RuntimeError):
                    mux.submit('SIM STATUS 1')
                stop = mux.submit('OFF', internal=True); mux.tick(write, .2)
                mux.receive(b'sim end stale=1\n'); self.assertFalse(stop.done())
                mux.receive(b'ev1 s 3 1 0 0 0 0 0 0\n')
                self.assertFalse(stop.result()['status']['on'])

    def test_real_collector_jsonl_and_summary_feed_causal_analysis(self):
        sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
        import event_causality as causal
        def record(seq,component,phase,parent=0,frame=0,a=0,b=0):
            body=ev.BODY.pack(component,1,phase,1,seq,seq*1000,seq,parent,1,frame,0,a,b)
            return b'ev1 '+body.hex().encode()+b' '+('%08x'%zlib.crc32(body)).encode()+b'\n'
        class CatalogDevice(Device):
            def run(self):
                pending=bytearray()
                try:
                    while not self.stop.is_set():
                        if not select.select([self.master],[],[],.02)[0]:continue
                        for byte in os.read(self.master,1024):
                            if byte!=10:pending.append(byte);continue
                            command=pending.decode().strip();pending.clear();self.commands.append(command);words=command.split()
                            if words[1]=='STATUS':self.send(('ev1 s %s 1 0 0 0 0 0 0\n'%words[2]).encode())
                            if words[1]=='ON':
                                self.seq=1;self.send(record(1,8,1,a=2,b=0x9f))
                                self.send(('ev1 s %s 1 1 0 0 0 1 0\n'%words[2]).encode())
                                for event in [record(2,1,1),record(3,3,3,parent=2),record(4,4,1,parent=2,frame=8),record(5,4,4,frame=8),record(6,5,4,frame=8)]:self.send(event)
                                self.seq=6
                            elif words[1]=='OFF':
                                self.seq=7;self.send(record(7,8,4,a=2,b=0x9f));self.send(('ev1 s %s 1 7 0 0 0 0 0\n'%words[2]).encode())
                except OSError as error:
                    if not self.stop.is_set():self.errors.append(error)
        device=CatalogDevice(events=[])
        try:
            with tempfile.TemporaryDirectory() as temp:
                root=Path(temp);path=root/'events'
                announced=[]
                result=ev.collect('new-PTY-only',path,root/'owner',.12,port_factory=device.open,
                                  journal_factory=lambda p:ev.Journal(p,sync_seconds=.02),causal_ready=announced.append)
                self.assertTrue(result['transport_complete'],result)
                self.assertTrue(result['causal_complete'],result)
                self.assertEqual(len(announced),1)
                self.assertTrue(announced[0]['causal_ready'])
                self.assertGreaterEqual(announced[0]['storage']['durable_prefix'],result['segments'][0]['begin_host_sequence'])
                self.assertGreater(result['segments'][0]['causal_ready_durable_prefix'],0)
                parsed=causal.device_source(path);self.assertTrue(parsed[1],parsed)
                cause=dict(version=1,boot=1,operation=2)
                server=([dict(component=c,phase=p,frame=f,cause=cause) for c,p,f in [('Api','Dispatched',0),('State','Committed',0),('Frame','Pending',8)]],True,[])
                self.assertEqual(causal.analyze(parsed,server)['results'][0]['status'],'rendered')
                summary=Path(str(path)+'.summary.json');saved=summary.read_bytes();summary.unlink()
                self.assertFalse(causal.device_source(path)[1]);summary.write_bytes(saved)
                rows=[json.loads(line) for line in path.read_text().splitlines()];rows[0]['host_sequence']=0
                path.write_text(''.join(json.dumps(row,separators=(',',':'))+'\n' for row in rows))
                parsed=causal.device_source(path);self.assertFalse(parsed[1]);self.assertIn('collector_host_sequence_or_time',parsed[2])
        finally:device.close()

    def test_rng_failure_acquires_no_writer_fd_or_selector(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            with patch.object(ev.secrets,'randbelow',side_effect=OSError('injected RNG failure')), \
                 patch.object(ev,'Journal') as journal, \
                 patch.object(ev.selectors,'DefaultSelector') as selector:
                with self.assertRaisesRegex(OSError,'injected RNG failure'):
                    ev.Collector('never-open',root/'events',root/'owner',1)
                journal.assert_not_called();selector.assert_not_called()
                self.assertFalse((root/'events').exists())

    def test_previous_owner_boundary_is_ignored_before_current_snapshot(self):
        class StaleDevice(Device):
            def run(self):
                pending = bytearray()
                try:
                    while not self.stop.is_set():
                        if not select.select([self.master], [], [], .02)[0]:
                            continue
                        for b in os.read(self.master, 1024):
                            if b != 10:
                                pending.append(b); continue
                            command = pending.decode().strip(); pending.clear()
                            self.commands.append(command); words = command.split()
                            if words[1]=='STATUS':self.send(('ev1 s %s 1 2 0 0 0 0 0\n'%words[2]).encode())
                            if words[1] == 'ON':
                                self.send(b'ev1 s 1 1 0 0 0 0 1 0\n')
                                self.send(packet(1)); self.send(packet(2))
                                # Correct CID with the wrong state is not an ON receipt.
                                self.send(('ev1 s %s 1 0 0 0 0 0 0\n' % words[2]).encode())
                                self.send(('ev1 s %s 1 2 0 0 0 1 0\n' % words[2]).encode())
                            elif words[1] == 'OFF':
                                self.send(('ev1 s %s 1 2 0 0 0 0 0\n' % words[2]).encode())
                except OSError as error:
                    if not self.stop.is_set():
                        self.errors.append(error)
        from unittest.mock import patch
        with patch.object(ev.secrets, 'randbelow', side_effect=[100, 1000]):
            for seed in [101, 1001]:
                d = StaleDevice(events=[], initial_sequence=2)
                try:
                    result, rows = self.capture(d)
                    self.assertTrue(result['transport_complete'], result)
                    self.assertEqual(result['segments'][0]['start']['sequence'], 2)
                    self.assertEqual(result['segments'][0]['received'], 0)
                    self.assertEqual(int(d.commands[0].split()[2], 16), seed+1)
                    reasons = [r['reason'] for r in rows if r['kind']=='boundary_reply_ignored']
                    self.assertEqual(reasons, ['cid_mismatch', 'control_state_mismatch'])
                finally:
                    d.close()

    def test_equivalent_detached_commit_and_capture_commands_never_reach_wire(self):
        d = Device(events=[]); answers, threads = [], []
        commands = ['SIM COMMIT 1 2 3', 'SIM  COMMIT 1 2 3', '  SIM   COMMIT 1 2 3  ',
                    'sim CoMmIt 1 2 3', 'SIM\tCOMMIT 1 2 3', ' EV1  OFF']
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def ready(status):
                def client():
                    for command in commands:
                        answers.append(rpc(root/'owner', {'command':command}))
                    answers.append(rpc(root/'owner', {'op':'stop'}))
                thread = threading.Thread(target=client); threads.append(thread); thread.start()
            try:
                result = ev.collect('new-PTY-only', root/'events', root/'owner', 2, port_factory=d.open, ready=ready)
                threads[0].join(2); self.assertFalse(threads[0].is_alive())
                self.assertTrue(result['transport_complete'], result)
                self.assertEqual(len(answers), len(commands)+1)
                self.assertTrue(all('cannot be forwarded' in a.get('error', '') for a in answers[:-1]))
                self.assertEqual([c.split()[1] for c in d.commands], ['STATUS', 'ON', 'OFF'])
            finally:
                d.close()

    def test_partial_command_never_appends_a_stop_or_delimiter(self):
        mux = ev.Commands(); future = mux.submit('SIM STATUS 1')
        writes = []
        def partial(data):
            if not writes:
                writes.append(data[:3]); return 3
            return 0
        mux.tick(partial, 0); mux.tick(partial, 4)
        self.assertTrue(mux.dirty)
        with self.assertRaises(TimeoutError):
            future.result()
        with self.assertRaises(RuntimeError):
            mux.submit('OFF', internal=True)
        self.assertEqual(b''.join(writes), b'SIM')

    def test_no_written_bytes_is_cancelled_without_poisoning_legacy_control(self):
        facts = []
        mux = ev.Commands(audit=lambda **v: facts.append(v))
        future = mux.submit('SIM STATUS 1'); mux.tick(lambda data: 0, 0); mux.tick(lambda data: 0, 4)
        self.assertFalse(mux.dirty); self.assertFalse(mux.uncertain)
        self.assertEqual(facts[-1]['kind'], 'command_cancelled')
        with self.assertRaises(TimeoutError):
            future.result()
        mux.submit('SIM STATUS 1')

    def test_prelude_above_start_is_validated_and_duplicate_is_not_hidden(self):
        for duplicate in [False, True]:
            window = ev.Window(1)
            row = ev.decode(packet(2)); window.accept(row)
            if duplicate:
                window.accept(row)
            start = dict(cid=1, boot=1, sequence=1, dropped=0, first_lost=0, last_lost=0, on=1, saturated=0)
            window.begin(start)
            end = dict(start, sequence=2, on=0)
            self.assertEqual(window.finish(end)['transport_complete'], not duplicate)

    def test_invalid_source_start_never_announces_ready(self):
        d = Device(events=[], boot=0)
        try:
            result, _ = self.capture(d, ready=lambda status: self.fail('invalid source cannot be ready'))
            self.assertFalse(result['complete'])
            self.assertIn('invalid_start', result['segments'][0]['reasons'])
        finally:
            d.close()

    def test_corrupt_line_is_preserved_as_bounded_evidence(self):
        bad = bytearray(packet(1)); bad[10] = ord('f')
        d = Device(events=[bytes(bad)])
        try:
            result, rows = self.capture(d)
            self.assertFalse(result['transport_complete'])
            row = next(r for r in rows if r['kind']=='decode_error')
            self.assertEqual(row['line'].encode(), bytes(bad).rstrip())
        finally:
            d.close()

    def test_storage_failure_keeps_source_result_separate(self):
        d = Device()
        try:
            result, _ = self.capture(d, journal_factory=lambda path: ev.Journal(path, max_bytes=1))
            self.assertFalse(result['transport_complete'])
            self.assertFalse(result['storage_durable'])
            self.assertTrue(result['segments'][0]['transport_complete'])
            self.assertGreater(result['storage']['write_lost'], 0)
        finally:
            d.close()

    def test_existing_socket_and_output_are_preserved_without_opening_serial(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); (root/'owner').write_text('keep')
            def forbidden(path):
                self.fail('must not open a serial path when IPC bind fails')
            result = ev.collect('forbidden', root/'events', root/'owner', .01, port_factory=forbidden)
            self.assertFalse(result['complete'])
            self.assertEqual((root/'owner').read_text(), 'keep')
            with self.assertRaises(FileExistsError):
                ev.Collector('forbidden', root/'events', root/'another', .01, port_factory=forbidden)


if __name__ == '__main__':
    unittest.main()
