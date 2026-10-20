#!/usr/bin/env python3
"""EV1 decoding and experimental one-owner serial collection.

The CLI is a prototype: end-to-end source sealing and serial lifecycle tests
are not accepted yet. Existing controllers do not use it. No synthetic input
is sent on startup.

Run only with an explicit serial path/output/socket. Existing control clients
use the Unix socket instead of opening a second serial reader. Capture data is
disposable: storage pressure is reported, never propagated to the device.
"""
import argparse
import concurrent.futures
import fcntl
import json
import os
from pathlib import Path
import queue
import selectors
import socket
import struct
import termios
import threading
import time
import zlib

BODY = struct.Struct('>BHB8Q2I')
FIELDS = ('component', 'event', 'phase', 'boot', 'sequence', 'us', 'operation',
          'parent', 'session', 'frame', 'dropped', 'a', 'b')
MAX_LINE = 512


def decode(line):
    if not line.startswith(b'ev1 ') or line.startswith((b'ev1 status ', b'ev1 error=')):
        return None
    if len(line) != 166 or line[-1:] != b'\n' or line[156:157] != b' ':
        raise ValueError('EV1 length/framing')
    try:
        body = bytes.fromhex(line[4:156].decode('ascii'))
        crc = int(line[157:165], 16)
    except (ValueError, UnicodeError) as error:
        raise ValueError('EV1 encoding') from error
    if len(body) != BODY.size or zlib.crc32(body) != crc:
        raise ValueError('EV1 checksum')
    return dict(zip(FIELDS, BODY.unpack(body)))


class Lines:
    """Bounded resynchronization for fragmented, mixed or malformed lines."""
    def __init__(self):
        self.pending = bytearray()
        self.discarding = False
        self.errors = 0

    def feed(self, chunk):
        for byte in chunk:
            if byte == 10:
                if not self.discarding:
                    yield bytes(self.pending) + b'\n'
                self.pending.clear()
                self.discarding = False
            elif not self.discarding:
                if len(self.pending) >= MAX_LINE - 1:
                    self.pending.clear()
                    self.discarding = True
                    self.errors += 1
                else:
                    self.pending.append(byte)


class Journal:
    """Single writer, bounded queue/file; reader submits without disk IO."""
    def __init__(self, path, capacity=256, max_bytes=64*1024*1024):
        self.fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        self.queue = queue.Queue(capacity)
        self.max_bytes = max_bytes
        self.bytes = self.attempted = self.lost = self.durable = 0
        self.error = None
        self.thread = threading.Thread(target=self._write, daemon=True)
        self.thread.start()

    def submit(self, value):
        self.attempted += 1
        value = dict(value, host_sequence=self.attempted, host_mono_ns=time.monotonic_ns())
        try:
            self.queue.put_nowait(value)
            return True
        except queue.Full:
            self.lost += 1
            return False

    def _write(self):
        try:
            while True:
                value = self.queue.get()
                if value is None:
                    break
                data = (json.dumps(value, separators=(',', ':'))+'\n').encode()
                if self.bytes + len(data) > self.max_bytes:
                    self.error = 'journal byte limit'
                    self.lost += 1
                    continue
                if self.error:
                    self.lost += 1
                    continue
                try:
                    view = memoryview(data)
                    while view:
                        n = os.write(self.fd, view)
                        if n <= 0:
                            raise OSError('short journal write')
                        view = view[n:]
                    self.bytes += len(data)
                except OSError as error:
                    self.error = str(error)
                    self.lost += 1
            os.fsync(self.fd)
            # This is a durable host watermark, not device/application completion.
            self.durable = self.attempted if not self.error and not self.lost else 0
        except OSError as error:
            self.error = str(error)
        finally:
            os.close(self.fd)

    def close(self, timeout=3):
        deadline = time.monotonic()+timeout
        while True:
            try:
                self.queue.put(None, timeout=max(0, deadline-time.monotonic()))
                break
            except queue.Full:
                self.error = self.error or 'journal writer did not drain'
                return False
        self.thread.join(max(0, deadline-time.monotonic()))
        return not self.thread.is_alive() and not self.error and not self.lost


class Commands:
    """One outstanding legacy command: replies without wire CID stay unambiguous.

    A client specifies reply/end prefixes for multi-line exports. The host CID
    correlates only this command response, never invents an application action ID.
    """
    def __init__(self, capacity=8):
        self.queue = queue.Queue(capacity)
        self.active = None
        self.next_id = 0
        self.uncertain = False

    def submit(self, line, reply_prefix=None, end_prefix=None):
        if self.uncertain:
            raise RuntimeError('uncorrelated late response possible; control transport must be re-established')
        raw = line.encode('ascii')
        if not raw or len(raw) >= 96 or any(c < 32 or c > 126 for c in raw):
            raise ValueError('command must be one bounded ASCII line')
        self.next_id += 1
        prefix = (reply_prefix or line.split()[0].lower()+' ').encode('ascii')
        end = (end_prefix or prefix.decode('ascii')).encode('ascii')
        result = concurrent.futures.Future()
        request = dict(id=self.next_id, raw=raw+b'\n', offset=0, prefix=prefix,
                       end=end, result=result, replies=[], deadline=0)
        self.queue.put_nowait(request)
        return result

    def tick(self, write, now):
        if self.active is None:
            try:
                self.active = self.queue.get_nowait()
                self.active['deadline'] = now+4
            except queue.Empty:
                return
        q = self.active
        if now >= q['deadline']:
            q['result'].set_exception(TimeoutError('command response uncertain; not retried'))
            self.active = None
            self.uncertain = True
            while not self.queue.empty():
                self.queue.get_nowait()['result'].set_exception(RuntimeError('prior response uncertain'))
        elif q['offset'] < len(q['raw']):
            try:
                q['offset'] += write(q['raw'][q['offset']:])
            except BlockingIOError:
                pass

    def receive(self, line):
        q = self.active
        if q is None or q['offset'] != len(q['raw']) or not line.startswith(q['prefix']):
            return
        if len(q['replies']) >= 1024:
            q['result'].set_exception(ValueError('reply line limit'))
            self.active = None
            return
        q['replies'].append(line.decode('ascii', errors='replace').rstrip())
        if line.startswith(q['end']):
            q['result'].set_result(dict(id=q['id'], replies=q['replies']))
            self.active = None


def collect(serial_path, output, socket_path, seconds):
    """Finite explicit invocation. IPC command handling and serial reads share a reactor."""
    journal = Journal(output)
    port = None
    listener = None
    socket_bound = False
    clients = {}
    sel = selectors.DefaultSelector()
    commands = Commands()
    framing = Lines()
    errors = device_drops = 0
    end_status = None
    started = time.monotonic()
    summary = dict(complete=False, reason='not sealed')
    try:
        port = os.open(serial_path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        fcntl.ioctl(port, termios.TIOCEXCL)
        attrs = termios.tcgetattr(port)
        attrs[0] = attrs[1] = attrs[3] = 0
        attrs[2] = termios.CLOCAL | termios.CREAD | termios.CS8
        attrs[4] = attrs[5] = termios.B115200
        attrs[6][termios.VMIN] = attrs[6][termios.VTIME] = 0
        termios.tcsetattr(port, termios.TCSANOW, attrs)
        fcntl.ioctl(port, termios.TIOCMBIS, struct.pack('I', termios.TIOCM_DTR))
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(socket_path))
        socket_bound = True
        os.chmod(socket_path, 0o600)
        listener.listen(4)
        listener.setblocking(False)
        sel.register(port, selectors.EVENT_READ, 'serial')
        sel.register(listener, selectors.EVENT_READ, 'listener')
        commands.submit('EV1 ON', 'ev1 status ', 'ev1 status ')
        closing = None
        while time.monotonic()-started < seconds+5:
            now = time.monotonic()
            if now-started >= seconds and closing is None:
                closing = commands.submit('EV1 OFF', 'ev1 status ', 'ev1 status ')
            commands.tick(lambda data: os.write(port, data), now)
            if closing is not None and closing.done():
                end_status = closing.result()
                break
            for key, _ in sel.select(0.02):
                if key.data == 'listener':
                    client, _ = listener.accept()
                    client.setblocking(False)
                    if len(clients) >= 4:
                        client.close()
                    else:
                        clients[client] = dict(buffer=bytearray(), future=None, reply=b'')
                        sel.register(client, selectors.EVENT_READ, 'client')
                elif key.data == 'serial':
                    data = os.read(port, 4096)
                    if not data:
                        raise EOFError('serial disconnected; capture incomplete')
                    for line in framing.feed(data):
                        try:
                            fact = decode(line)
                            if fact:
                                device_drops = max(device_drops, fact['dropped'])
                            journal.submit(dict(kind='event' if fact else 'line', event=fact,
                                                line=None if fact else line.decode('ascii', errors='replace').rstrip()))
                        except ValueError as error:
                            errors += 1
                            journal.submit(dict(kind='decode_error', error=str(error)))
                        commands.receive(line)
                else:
                    client = key.fileobj
                    entry = clients[client]
                    data = client.recv(1024)
                    if not data:
                        sel.unregister(client)
                        client.close()
                        del clients[client]
                        continue
                    entry['buffer'].extend(data)
                    if len(entry['buffer']) > 1024 or entry['future'] is not None:
                        sel.unregister(client)
                        client.close()
                        del clients[client]
                        continue
                    if b'\n' in entry['buffer']:
                        try:
                            request = json.loads(entry['buffer'])
                            if request['command'].strip().upper() in ('EV1 ON', 'EV1 OFF'):
                                raise ValueError('capture lifecycle belongs to the serial owner')
                            entry['future'] = commands.submit(request['command'], request.get('reply_prefix'), request.get('end_prefix'))
                        except (ValueError, KeyError, queue.Full, RuntimeError) as error:
                            entry['reply'] = (json.dumps(dict(error=str(error)))+'\n').encode()
            for client, entry in list(clients.items()):
                future = entry['future']
                if future is not None and future.done() and not entry['reply']:
                    try:
                        value = future.result()
                    except Exception as error:
                        value = dict(error=str(error))
                    entry['reply'] = (json.dumps(value)+'\n').encode()
                if entry['reply']:
                    try:
                        n = client.send(entry['reply'])
                        entry['reply'] = entry['reply'][n:]
                        if not entry['reply']:
                            sel.unregister(client)
                            client.close()
                            del clients[client]
                    except BlockingIOError:
                        pass
        summary.update(reason='end response received; source completeness not validated' if end_status else 'stop response missing', end_status=end_status)
    finally:
        for client in clients:
            client.close()
        sel.close()
        if listener is not None:
            listener.close()
            if socket_bound:
                Path(socket_path).unlink()
        if port is not None:
            os.close(port)
        durable = journal.close()
        summary.update(storage_durable=durable, host_lost=journal.lost, storage_error=journal.error,
                       decode_errors=errors+framing.errors, observed_device_drops=device_drops,
                       host_durable_sequence=journal.durable)
        # Until end STATUS and the complete device sequence interval are checked,
        # persistence alone must not be labelled an observation-complete capture.
        summary['complete'] = False
        with Path(str(output)+'.summary.json').open('x') as result:
            result.write(json.dumps(summary, indent=2)+'\n')
    return summary


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--serial', required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--socket', type=Path, required=True)
    p.add_argument('--seconds', type=float, required=True)
    args = p.parse_args()
    if not 0 < args.seconds <= 86400:
        p.error('seconds must be finite and <=86400')
    print(json.dumps(collect(args.serial, args.output, args.socket, args.seconds), indent=2))
