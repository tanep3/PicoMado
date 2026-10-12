#!/usr/bin/env python3
"""Short-lived Pi5-side A/B/C reachability probe; Python standard library only."""

import argparse
import ipaddress
import socket
import socketserver
import threading

PORT_A = 18180
PORT_B = 18181
PORT_C = 18182
MAX_BYTES = 2048


def line(sock):
    data = bytearray()
    while len(data) < MAX_BYTES:
        part = sock.recv(1)
        if not part:
            break
        data.extend(part)
        if part == b"\n":
            break
    return data.decode("ascii", "replace").strip()


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, address, handler, test_id):
        self.test_id = test_id
        super().__init__(address, handler)


class AHandler(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(8)
        data = bytearray()
        while b"\r\n\r\n" not in data and len(data) < MAX_BYTES:
            part = self.request.recv(512)
            if not part:
                return
            data.extend(part)
        if b"\r\n\r\n" not in data:
            return
        head, body = data.split(b"\r\n\r\n", 1)
        headers = head.decode("ascii", "replace").split("\r\n")
        if headers[0] != "POST /probe HTTP/1.1":
            return
        lengths = [x.split(":", 1)[1].strip() for x in headers[1:]
                   if x.lower().startswith("content-length:")]
        if len(lengths) != 1 or not lengths[0].isdigit():
            return
        length = int(lengths[0])
        if length > 128:
            return
        while len(body) < length:
            part = self.request.recv(length - len(body))
            if not part:
                return
            body += part
        expected = f"TEST A {self.server.test_id}\n".encode("ascii")
        if body[:length] != expected:
            print(f"A unexpected test ID/body from {self.client_address[0]}", flush=True)
            return
        print(f"A POST received: id={self.server.test_id} from={self.client_address[0]} to={self.request.getsockname()[0]}", flush=True)
        ack = f"ACK A {self.server.test_id}\n".encode("ascii")
        response = (b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n"
                    + f"Content-Length: {len(ack)}\r\nConnection: close\r\n\r\n".encode()
                    + ack)
        self.request.sendall(response)
        print(f"A ACK sent: id={self.server.test_id}", flush=True)


class CHandler(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(10)
        expected = f"HELLO C {self.server.test_id}"
        hello = line(self.request)
        if hello != expected:
            print(f"C unexpected hello from={self.client_address[0]}: {hello!r}", flush=True)
            return
        print(f"C persistent connection accepted: id={self.server.test_id} from={self.client_address[0]} to={self.request.getsockname()[0]}", flush=True)
        self.request.sendall(f"PUSH C {self.server.test_id}\n".encode("ascii"))
        ack = line(self.request)
        print(f"C ACK {'OK' if ack == f'ACK C {self.server.test_id}' else 'FAILED'}: {ack!r}", flush=True)


def serve(args):
    servers = [Server((args.bind, PORT_A), AHandler, args.id),
               Server((args.bind, PORT_C), CHandler, args.id)]
    threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in servers]
    for t in threads:
        t.start()
    print(f"A listening {args.bind}:{PORT_A}; C listening {args.bind}:{PORT_C}; id={args.id}", flush=True)
    try:
        for t in threads:
            t.join()
    except KeyboardInterrupt:
        print("Stopping test listeners", flush=True)
    finally:
        for s in servers:
            s.shutdown()
            s.server_close()


def push_b(args):
    target = ipaddress.IPv4Address(args.pico)
    with socket.create_connection((str(target), PORT_B), timeout=5) as sock:
        sock.settimeout(5)
        sock.sendall(f"PUSH B {args.id}\n".encode("ascii"))
        ack = line(sock)
    expected = f"ACK B {args.id}"
    print(f"B ACK {'OK' if ack == expected else 'FAILED'}: {ack!r}")
    if ack != expected:
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    s = sub.add_parser("serve", help="Receive A POST and C connection on Pi5")
    s.add_argument("--bind", required=True, help="Pi5 Tailscale IPv4 (or explicitly chosen local IPv4)")
    s.add_argument("--id", default="trial-001", help="Must match Pico TEST_ID")
    s.set_defaults(func=serve)
    b = sub.add_parser("push-b", help="Initiate B TCP connection to Pico tethering DHCP IPv4")
    b.add_argument("--pico", required=True, help="Pico tethering DHCP IPv4, not Pi5 Tailscale IPv4")
    b.add_argument("--id", default="trial-001")
    b.set_defaults(func=push_b)
    args = parser.parse_args()
    if not args.id or len(args.id) > 50 or not all(c.isascii() and (c.isalnum() or c in "-_") for c in args.id):
        parser.error("--id must contain 1-50 ASCII letters, digits, '-' or '_'")
    if args.mode == "serve":
        ipaddress.IPv4Address(args.bind)
    args.func(args)


if __name__ == "__main__":
    main()
