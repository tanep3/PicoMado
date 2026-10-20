#!/usr/bin/env python3
"""Bounded EV1 collection. One serial owner; no automatic application commands.

Transport completeness is separate from application/draw completion. CLI use
requires an explicit port, exclusive output, IPC path and finite duration.
"""
import argparse
import collections
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
MAX_U64 = (1 << 64)-1


def decode(line):
    if not line.startswith(b'ev1 ') or line.startswith((b'ev1 status ', b'ev1 s ', b'ev1 error=')):
        return None
    if len(line) != 166 or line[-1:] != b'\n' or line[156:157] != b' ':
        raise ValueError('EV1 framing')
    try:
        body = bytes.fromhex(line[4:156].decode('ascii'))
        crc = int(line[157:165], 16)
    except (ValueError, UnicodeError) as error:
        raise ValueError('EV1 encoding') from error
    if len(body) != BODY.size or zlib.crc32(body) != crc:
        raise ValueError('EV1 checksum')
    return dict(zip(FIELDS, BODY.unpack(body)))


def parse_status(line):
    if not line.startswith(b'ev1 s '):
        return None
    words = line.split()
    if len(words) != 10:
        raise ValueError('EV1 status framing')
    try:
        values = [int(word, 16) for word in words[2:8]]
        flags = [int(word, 10) for word in words[8:]]
    except ValueError as error:
        raise ValueError('EV1 status encoding') from error
    if any(v < 0 or v > MAX_U64 for v in values) or any(v not in (0, 1) for v in flags):
        raise ValueError('EV1 status range')
    return dict(zip(('cid', 'boot', 'sequence', 'dropped', 'first_lost', 'last_lost', 'on', 'saturated'), values+flags))


class Lines:
    def __init__(self):
        self.pending = bytearray()
        self.discarding = False
        self.errors = 0

    def feed(self, chunk):
        for byte in chunk:
            if byte == 10:
                if not self.discarding:
                    yield bytes(self.pending)+b'\n'
                self.pending.clear()
                self.discarding = False
            elif not self.discarding:
                if len(self.pending) >= MAX_LINE-1:
                    self.pending.clear()
                    self.discarding = True
                    self.errors += 1
                else:
                    self.pending.append(byte)

    def partial(self):
        return bool(self.pending or self.discarding)


class Window:
    """Online sequence validation; no unbounded history or inferred causality."""
    def __init__(self, epoch):
        self.epoch = epoch
        self.start = None
        self.last = 0
        self.last_us = 0
        self.reasons = collections.Counter()
        self.prelude = collections.deque(maxlen=32)
        self.received = 0

    def fail(self, reason):
        self.reasons[reason] += 1

    def begin(self, status):
        self.start = status
        self.last = status['sequence']
        if not status['on'] or status['saturated'] or not status['boot']:
            self.fail('invalid_start')
        for fact in self.prelude:
            if fact['boot'] != status['boot'] or fact['sequence'] > status['sequence']:
                self.accept(fact)
        self.prelude.clear()

    def accept(self, fact):
        if self.start is None:
            if len(self.prelude) == self.prelude.maxlen:
                self.fail('prelude_overflow')
            self.prelude.append(fact)
            return
        if fact['boot'] != self.start['boot']:
            self.fail('boot_changed')
            return
        if fact['sequence'] != self.last+1:
            self.fail('sequence_gap_or_reorder')
        if fact['sequence'] <= self.last:
            return
        if fact['us'] < self.last_us:
            self.fail('source_time_reversed')
        self.last = fact['sequence']
        self.last_us = fact['us']
        self.received += 1
        if fact['dropped'] != self.start['dropped']:
            self.fail('source_drop')

    def finish(self, end, partial=False):
        if partial:
            self.fail('partial_line')
        if self.start is None or end is None:
            self.fail('missing_boundary')
        else:
            if end['boot'] != self.start['boot']:
                self.fail('end_boot_changed')
            if end['on'] or end['saturated']:
                self.fail('invalid_end')
            if end['sequence'] != self.last:
                self.fail('end_watermark_gap')
            if end['dropped'] != self.start['dropped']:
                self.fail('source_drop')
        return dict(link_epoch=self.epoch, start=self.start, end=end,
                    last_received_sequence=self.last, received=self.received,
                    transport_complete=not self.reasons, reasons=dict(self.reasons))


class Journal:
    def __init__(self, path, capacity=256, max_bytes=64*1024*1024,
                 write=None, fsync=None, sync_seconds=1):
        self.fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        self.path = Path(path)
        self.queue = queue.Queue(capacity)
        self.max_bytes = max_bytes
        self.write = write or os.write
        self.fsync = fsync or os.fsync
        self.sync_seconds = sync_seconds
        self.attempted = self.queue_lost = 0  # reactor owned
        self.bytes = self.write_lost = self.last_written = 0  # worker owned
        self.durable = self.durable_prefix = self.written_prefix = 0
        self.error = self.close_error = None
        self.closing = False
        self.thread = threading.Thread(target=self._write, daemon=True)
        try:
            self.thread.start()
        except BaseException:
            os.close(self.fd)
            raise

    @property
    def lost(self):
        return self.queue_lost+self.write_lost

    def submit(self, value):
        if self.closing:
            raise RuntimeError('journal is closing')
        self.attempted += 1
        value = dict(value, host_sequence=self.attempted, host_mono_ns=time.monotonic_ns())
        try:
            self.queue.put_nowait(value)
            return True
        except queue.Full:
            self.queue_lost += 1
            return False

    def _sync(self):
        if self.error:
            return
        try:
            self.fsync(self.fd)
            self.durable = self.last_written
            self.durable_prefix = self.written_prefix
        except OSError as error:
            self.error = str(error)

    def _write(self):
        next_sync = time.monotonic()+self.sync_seconds
        try:
            while not self.closing or not self.queue.empty():
                try:
                    value = self.queue.get(timeout=0.02)
                except queue.Empty:
                    value = None
                if value is not None:
                    data = (json.dumps(value, separators=(',', ':'))+'\n').encode()
                    if self.bytes+len(data) > self.max_bytes:
                        self.error = self.error or 'journal byte limit'
                    if self.error:
                        self.write_lost += 1
                    else:
                        try:
                            view = memoryview(data)
                            while view:
                                n = self.write(self.fd, view)
                                if n <= 0:
                                    raise OSError('short journal write')
                                view = view[n:]
                            self.bytes += len(data)
                            seq = value['host_sequence']
                            if seq == self.written_prefix+1:
                                self.written_prefix = seq
                            self.last_written = seq
                        except OSError as error:
                            self.error = str(error)
                            self.write_lost += 1
                if time.monotonic() >= next_sync:
                    self._sync()
                    next_sync = time.monotonic()+self.sync_seconds
            self._sync()
            if not self.error:
                directory = os.open(self.path.parent, os.O_RDONLY | os.O_DIRECTORY)
                try:
                    self.fsync(directory)
                finally:
                    os.close(directory)
        except OSError as error:
            self.error = str(error)
        finally:
            os.close(self.fd)

    def status(self):
        return dict(attempted=self.attempted, queue_lost=self.queue_lost,
                    write_lost=self.write_lost, error=self.error or self.close_error,
                    bytes=self.bytes, last_durable_host_sequence=self.durable,
                    durable_prefix=self.durable_prefix, worker_alive=self.thread.is_alive())

    def close(self, timeout=3):
        self.closing = True
        self.thread.join(timeout)
        if self.thread.is_alive():
            self.close_error = 'journal writer did not finish'
        return not self.thread.is_alive() and not self.error and not self.close_error and not self.lost and self.durable_prefix == self.attempted


class Commands:
    def __init__(self, capacity=8, start_id=0, audit=None):
        self.queue = queue.Queue(capacity)
        self.internal = None
        self.active = None
        self.next_id = start_id
        self.uncertain = False
        self.dirty = False
        self.audit = audit or (lambda **value: None)

    def submit(self, line, reply_prefix=None, end_prefix=None, internal=False):
        if self.dirty:
            raise RuntimeError('partial wire command requires a new link')
        if self.uncertain and not internal:
            raise RuntimeError('uncorrelated late response possible; legacy control is uncertain')
        if internal:
            if line not in ('ON', 'OFF', 'STATUS') or self.internal is not None:
                raise ValueError('invalid capture control')
        if not isinstance(line, str) or any(v is not None and (not isinstance(v, str) or len(v)>96) for v in (reply_prefix, end_prefix)):
            raise ValueError('invalid command or reply prefix')
        raw = line.encode('ascii')
        if not raw or len(raw) >= 96 or any(c < 32 or c > 126 for c in raw):
            raise ValueError('command must be one bounded ASCII line')
        if self.next_id == MAX_U64:
            raise RuntimeError('command ID exhausted')
        self.next_id += 1
        result = concurrent.futures.Future()
        prefix = (reply_prefix or line.split()[0].lower()+' ').encode('ascii')
        end = (end_prefix or prefix.decode('ascii')).encode('ascii')
        wire = ('EV1 %s %x' % (line, self.next_id)).encode() if internal else raw
        request = dict(id=self.next_id, raw=wire+b'\n', offset=0, prefix=prefix,
                       end=end, result=result, replies=[], size=0, deadline=0, internal=internal)
        if internal:
            self.internal = request
        else:
            self.queue.put_nowait(request)
        self.audit(kind='command_queued', command_id=request['id'], command=wire.decode())
        return result

    def _fail(self, request, reason):
        if not request['result'].done():
            request['result'].set_exception(RuntimeError(reason))
        self.audit(kind='command_uncertain' if request['offset'] else 'command_cancelled', command_id=request['id'], reason=reason)

    def tick(self, write, now):
        if self.active is None:
            if self.internal is not None:
                self.active, self.internal = self.internal, None
            else:
                try:
                    self.active = self.queue.get_nowait()
                except queue.Empty:
                    return
            self.active['deadline'] = now+4
        q = self.active
        if now >= q['deadline']:
            if not q['result'].done():
                q['result'].set_exception(TimeoutError('command response uncertain; not retried'))
            self.audit(kind='command_uncertain' if q['offset'] else 'command_cancelled', command_id=q['id'], reason='deadline')
            self.dirty = 0 < q['offset'] < len(q['raw'])
            self.active = None
            if not q['internal'] and q['offset']:
                self.uncertain = True
                while not self.queue.empty():
                    self._fail(self.queue.get_nowait(), 'prior response uncertain')
        elif q['offset'] < len(q['raw']):
            try:
                n = write(q['raw'][q['offset']:q['offset']+64])
                if n < 0 or n > len(q['raw'])-q['offset']:
                    raise OSError('invalid serial write length')
                q['offset'] += n
                if q['offset'] == len(q['raw']):
                    self.audit(kind='command_written', command_id=q['id'])
            except BlockingIOError:
                pass

    def receive(self, line):
        q = self.active
        if q is None or q['offset'] != len(q['raw']):
            return
        if q['internal']:
            status = parse_status(line)
            if status is None or status['cid'] != q['id']:
                return
            q['result'].set_result(dict(id=q['id'], status=status, replies=[line.decode().rstrip()]))
            self.active = None
            return
        if not line.startswith(q['prefix']):
            return
        q['size'] += len(line)
        if q['size'] > 128*1024 or len(q['replies']) >= 1024:
            self._fail(q, 'reply limit')
            self.active = None
            self.uncertain = True
            return
        q['replies'].append(line.decode('ascii', errors='replace').rstrip())
        if line.startswith(q['end']):
            if not q['result'].done():
                q['result'].set_result(dict(id=q['id'], replies=q['replies']))
            self.active = None

    def disconnect(self):
        if self.active:
            self._fail(self.active, 'link closed; outcome uncertain')
            self.active = None
        if self.internal:
            self._fail(self.internal, 'link closed')
            self.internal = None
        while not self.queue.empty():
            self._fail(self.queue.get_nowait(), 'link closed before dispatch')
        self.uncertain = True


class SerialPort:
    def __init__(self, path, dtr=True):
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        self.dtr = dtr
        try:
            fcntl.ioctl(self.fd, termios.TIOCEXCL)
            attrs = termios.tcgetattr(self.fd)
            self.previous = attrs[:]
            self.previous[6] = attrs[6][:]
            attrs[0] = attrs[1] = attrs[3] = 0
            attrs[2] = termios.CLOCAL | termios.CREAD | termios.CS8 | termios.HUPCL
            attrs[4] = attrs[5] = termios.B115200
            attrs[6][termios.VMIN] = attrs[6][termios.VTIME] = 0
            termios.tcsetattr(self.fd, termios.TCSANOW, attrs)
            if dtr:
                fcntl.ioctl(self.fd, termios.TIOCMBIS, struct.pack('I', termios.TIOCM_DTR))
        except BaseException:
            os.close(self.fd)
            raise

    def fileno(self):
        return self.fd

    def read(self):
        return os.read(self.fd, 4096)

    def write(self, data):
        return os.write(self.fd, data)

    def close(self):
        try:
            if self.dtr:
                fcntl.ioctl(self.fd, termios.TIOCMBIC, struct.pack('I', termios.TIOCM_DTR))
            termios.tcsetattr(self.fd, termios.TCSANOW, self.previous)
            fcntl.ioctl(self.fd, termios.TIOCNXCL)
        except (OSError, termios.error):
            pass
        finally:
            os.close(self.fd)


class Collector:
    def __init__(self, serial_path, output, socket_path, seconds, reconnects=0,
                 port_factory=SerialPort, journal_factory=Journal, ready=None):
        if not 0 < seconds <= 86400 or not 0 <= reconnects <= 2:
            raise ValueError('finite duration and 0..2 reconnect attempts required')
        self.serial_path, self.output, self.socket_path = serial_path, Path(output), Path(socket_path)
        self.seconds, self.reconnects = seconds, reconnects
        self.port_factory, self.ready = port_factory, ready
        self.journal = journal_factory(self.output)
        self.sel = selectors.DefaultSelector()
        self.listener = self.port = None
        self.socket_inode = None
        self.clients = {}
        self.mux = Commands(audit=self.audit)
        self.framing = Lines()
        self.window = None
        self.segments = []
        self.prior_source_sequences = {}
        self.link_epoch = self.reopened = 0
        self.beginning = self.closing = None
        self.started = time.monotonic()
        self.data_deadline = None
        self.hard_deadline = self.started+seconds+14
        self.retry_at = 0
        self.stop_requested = self.interrupted = self.done = False
        self.errors = []
        self.command_uncertain = 0

    def audit(self, **value):
        if value.get('kind') == 'command_uncertain':
            self.command_uncertain += 1
        self.journal.submit(dict(value, link_epoch=self.link_epoch))

    def status(self):
        return dict(ready=self.window is not None and self.window.start is not None and not self.window.reasons.get('invalid_start') and not self.stop_requested,
                    source=self.window.start if self.window else None,
                    link_epoch=self.link_epoch, stopping=self.stop_requested,
                    storage=self.journal.status())

    def _open(self):
        self.port = self.port_factory(self.serial_path)
        try:
            self.sel.register(self.port, selectors.EVENT_READ, 'serial')
        except BaseException:
            self.port.close()
            self.port = None
            raise
        self.link_epoch += 1
        self.window = Window(self.link_epoch)
        self.framing = Lines()
        self.mux = Commands(start_id=self.mux.next_id, audit=self.audit)
        self.beginning = self.mux.submit('ON', internal=True)
        self.closing = None
        self.audit(kind='link_open')

    def _close_link(self, end=None, reason=None):
        if self.port is None:
            return
        if reason:
            self.window.fail(reason)
        seen = list(self.window.prelude)
        if self.window.start is not None:
            seen.append(dict(boot=self.window.start['boot'], sequence=self.window.last))
        for fact in seen:
            boot = fact['boot']
            self.prior_source_sequences[boot] = max(self.prior_source_sequences.get(boot, 0), fact['sequence'])
        if self.framing.partial():
            self.audit(kind='partial_line', line=bytes(self.framing.pending).decode('ascii', errors='replace'), overflow=self.framing.discarding)
        self.segments.append(self.window.finish(end, self.framing.partial()))
        self.mux.disconnect()
        port, self.port = self.port, None
        self.sel.unregister(port)
        try:
            port.close()
        except OSError as error:
            self.errors.append('close: '+str(error))
        self.audit(kind='link_closed', reason=reason)

    def _lost(self, reason):
        self._close_link(reason=reason)
        self.interrupted = True
        if self.reopened >= self.reconnects or self.stop_requested:
            self.done = True
        else:
            self.reopened += 1
            self.retry_at = time.monotonic()+0.05

    def _line(self, line):
        try:
            fact = decode(line)
            self.audit(kind='event' if fact else 'line', event=fact,
                       line=None if fact else line.decode('ascii', errors='replace').rstrip())
            if fact:
                self.window.accept(fact)
            self.mux.receive(line)
        except ValueError as error:
            self.window.fail('decode_error')
            self.audit(kind='decode_error', reason=str(error), line=line.decode('ascii', errors='replace').rstrip())

    def _drop_client(self, client):
        self.sel.unregister(client)
        client.close()
        del self.clients[client]

    def _reply(self, client, value):
        entry = self.clients[client]
        entry['reply'] = (json.dumps(value)+'\n').encode()
        entry['reply_deadline'] = time.monotonic()+2
        self.sel.modify(client, selectors.EVENT_WRITE, 'client')

    def _client(self, client, mask):
        entry = self.clients[client]
        try:
            if mask & selectors.EVENT_WRITE:
                n = client.send(entry['reply'][:4096])
                if n == 0:
                    self._drop_client(client)
                else:
                    entry['reply'] = entry['reply'][n:]
                    if not entry['reply']:
                        self._drop_client(client)
                return
            data = client.recv(1024)
            if not data:
                self._drop_client(client)
                return
            entry['buffer'].extend(data)
            if len(entry['buffer']) > 1024 or entry['future'] is not None:
                self._drop_client(client)
                return
            if b'\n' not in entry['buffer']:
                return
            try:
                request = json.loads(entry['buffer'])
                if not isinstance(request, dict):
                    raise ValueError('object request required')
                if request.get('op') == 'stop':
                    self.stop_requested = True
                    self._reply(client, dict(state='stopping'))
                elif request.get('op') == 'status':
                    self._reply(client, self.status())
                else:
                    command = request['command']
                    if not isinstance(command, str) or not command.split():
                        raise ValueError('command required')
                    if command.split()[0].upper() == 'EV1' or command.upper().startswith('SIM COMMIT'):
                        raise ValueError('capture lifecycle or detached COMMIT cannot be forwarded')
                    if self.port is None or self.stop_requested:
                        raise RuntimeError('capture link unavailable or stopping')
                    entry['future'] = self.mux.submit(command, request.get('reply_prefix'), request.get('end_prefix'))
            except (ValueError, KeyError, IndexError, TypeError, queue.Full, RuntimeError) as error:
                self._reply(client, dict(error=str(error)))
        except (BrokenPipeError, ConnectionResetError):
            self._drop_client(client)
        except BlockingIOError:
            pass

    def _clients(self):
        now = time.monotonic()
        for client, entry in list(self.clients.items()):
            future = entry['future']
            if future is not None and future.done() and not entry['reply']:
                try:
                    value = future.result()
                except Exception as error:
                    value = dict(error=str(error))
                self._reply(client, value)
            elif entry['reply'] and now >= entry['reply_deadline']:
                self._drop_client(client)
            elif future is None and not entry['reply'] and now-entry['accepted'] >= 2:
                self._drop_client(client)

    def run(self):
        try:
            self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.listener.bind(str(self.socket_path))
            self.socket_inode = self.socket_path.stat().st_ino
            os.chmod(self.socket_path, 0o600)
            self.listener.listen(4)
            self.listener.setblocking(False)
            self.sel.register(self.listener, selectors.EVENT_READ, 'listener')
            while not self.done and time.monotonic() < self.hard_deadline:
                now = time.monotonic()
                if self.port is None and now >= self.retry_at:
                    try:
                        self._open()
                    except (OSError, termios.error) as error:
                        self.errors.append('open: '+str(error))
                        self.port = None
                        if self.reopened >= self.reconnects:
                            break
                        self.reopened += 1
                        self.interrupted = True
                        self.retry_at = now+0.05
                if self.port is not None:
                    if self.beginning is not None and self.beginning.done():
                        try:
                            status = self.beginning.result()['status']
                        except Exception:
                            self._lost('start_response_missing')
                            continue
                        self.beginning = None
                        self.window.begin(status)
                        if status['sequence'] < self.prior_source_sequences.get(status['boot'], 0):
                            self.window.fail('source_sequence_reset')
                        if self.window.reasons.get('invalid_start'):
                            self._lost('invalid_start')
                            continue
                        if self.data_deadline is None:
                            self.data_deadline = now+self.seconds
                        if self.ready:
                            self.ready(self.status())
                    if self.data_deadline is not None and now >= self.data_deadline:
                        self.stop_requested = True
                    if self.stop_requested and self.closing is None:
                        self.closing = self.mux.submit('OFF', internal=True)
                    try:
                        self.mux.tick(self.port.write, now)
                    except OSError:
                        self._lost('write_error')
                        continue
                    if self.mux.dirty:
                        self._lost('partial_command')
                        continue
                    if self.closing is not None and self.closing.done():
                        try:
                            end = self.closing.result()['status']
                        except Exception:
                            self._lost('stop_response_missing')
                        else:
                            self._close_link(end=end)
                            self.done = True
                        continue
                for key, mask in self.sel.select(0.02):
                    if key.data == 'listener':
                        client, _ = self.listener.accept()
                        client.setblocking(False)
                        if len(self.clients) >= 4:
                            client.close()
                        else:
                            self.clients[client] = dict(buffer=bytearray(), future=None, reply=b'',
                                                        accepted=time.monotonic(), reply_deadline=0)
                            self.sel.register(client, selectors.EVENT_READ, 'client')
                    elif key.data == 'serial':
                        try:
                            data = self.port.read()
                            if not data:
                                self._lost('disconnected')
                                continue
                            prior_errors = self.framing.errors
                            for line in self.framing.feed(data):
                                self._line(line)
                            if self.framing.errors != prior_errors:
                                self.window.fail('line_overflow')
                        except BlockingIOError:
                            pass
                        except OSError:
                            self._lost('read_error')
                    elif key.fileobj in self.clients:
                        self._client(key.fileobj, mask)
                self._clients()
            if self.port is not None:
                self._close_link(reason='collector_deadline')
        except (OSError, ValueError, RuntimeError, KeyboardInterrupt) as error:
            self.errors.append(str(error))
            if self.port is not None:
                self._close_link(reason='collector_error')
        finally:
            if self.port is not None:
                self._close_link(reason='collector_interrupted')
            for client in list(self.clients):
                self._drop_client(client)
            self.sel.close()
            if self.listener is not None:
                self.listener.close()
            if self.socket_inode is not None and self.socket_path.exists() and self.socket_path.stat().st_ino == self.socket_inode:
                self.socket_path.unlink()
            durable = self.journal.close()
        complete = bool(self.segments) and all(s['transport_complete'] for s in self.segments) and not self.interrupted and not self.errors and durable
        summary = dict(transport_complete=complete, complete=complete, application_complete=None,
                       storage_durable=durable, storage=self.journal.status(),
                       interrupted=self.interrupted, reconnect_attempts=self.reopened, command_uncertain=self.command_uncertain,
                       segments=self.segments, errors=self.errors)
        with Path(str(self.output)+'.summary.json').open('x') as result:
            result.write(json.dumps(summary, indent=2)+'\n')
        return summary


def collect(serial_path, output, socket_path, seconds, **options):
    return Collector(serial_path, output, socket_path, seconds, **options).run()


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--serial', required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--socket', type=Path, required=True)
    p.add_argument('--seconds', type=float, required=True)
    p.add_argument('--reconnects', type=int, default=0)
    args = p.parse_args()
    print(json.dumps(collect(args.serial, args.output, args.socket, args.seconds, reconnects=args.reconnects), indent=2))
