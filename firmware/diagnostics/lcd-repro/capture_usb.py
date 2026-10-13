#!/usr/bin/env python3
"""Recover diagnostic RAM snapshots; compare every byte and undo LCD transforms."""
import argparse
from dataclasses import dataclass
import glob
import json
import os
from pathlib import Path
import re
import select
import termios
import time
import zlib
from PIL import Image

WIDTH, HEIGHT, FRAME_BYTES = 160, 80, 25600
FRAME_IDS = [1, 2, 7]


@dataclass
class Snapshot:
    metadata: dict
    pixels: bytes


class CaptureParser:
    def __init__(self):
        self.frames = []
        self.current = None
        self.buffer = bytearray()
        self.started = False
        self.complete = False

    def feed(self, line):
        if not line.startswith("LCDCAP ") or line.startswith("LCDCAP READY "):
            return
        pieces = line.split()
        kind = pieces[1]
        fields = dict(part.split("=", 1) for part in pieces[2:])
        if kind == "BEGIN":
            if self.started or int(fields["count"]) != 3:
                raise ValueError("invalid capture BEGIN")
            self.started = True
        elif not self.started or self.complete:
            raise ValueError("capture record outside BEGIN/DONE")
        elif kind == "FRAME":
            if self.current is not None or len(self.frames) >= 3:
                raise ValueError("overlapping/extra snapshot")
            frame = int(fields["frame"])
            if frame != FRAME_IDS[len(self.frames)] or int(fields["bytes"]) != FRAME_BYTES:
                raise ValueError("unexpected snapshot ID/length")
            self.current = fields
            self.buffer = bytearray()
        elif kind == "DATA":
            if self.current is None or fields["frame"] != self.current["frame"]:
                raise ValueError("DATA frame mismatch")
            if int(fields["offset"]) != len(self.buffer):
                raise ValueError("non-contiguous snapshot offset")
            chunk = bytes.fromhex(fields["hex"])
            if not 1 <= len(chunk) <= 256 or len(self.buffer) + len(chunk) > FRAME_BYTES:
                raise ValueError("invalid DATA length")
            self.buffer.extend(chunk)
        elif kind == "END":
            if self.current is None or fields["frame"] != self.current["frame"]:
                raise ValueError("END frame mismatch")
            if len(self.buffer) != FRAME_BYTES:
                raise ValueError("incomplete snapshot bytes")
            if zlib.crc32(self.buffer) != int(self.current["crc"], 16):
                raise ValueError("USB snapshot CRC mismatch")
            self.frames.append(Snapshot(self.current, bytes(self.buffer)))
            self.current = None
        elif kind == "DONE":
            if int(fields["count"]) != 3 or len(self.frames) != 3 or self.current is not None:
                raise ValueError("incomplete capture batch")
            self.complete = True
        else:
            raise ValueError("unknown capture record")


def independent_expected(glyph_source):
    """Scene specification + RGB tuples; do not reuse the C packing/transform."""
    source = Path(glyph_source).read_text()
    font = source.split("static const uint8_t glyphs[][5] = {", 1)[1].split("};", 1)[0]
    glyphs = [[int(v, 16) for v in re.findall(r"0x[0-9a-fA-F]+", row)]
              for row in re.findall(r"\{([^}]+)\}", font)]
    if len(glyphs) != 26 or any(len(g) != 5 for g in glyphs):
        raise ValueError("unexpected fixed-scene font data")
    black, white = (0, 0, 0), (31, 63, 31)
    canvas = [[black for _ in range(WIDTH)] for _ in range(HEIGHT)]
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if x in (0, 159) or y in (0, 79):
                canvas[y][x] = white
            elif 1 <= y <= 9:
                canvas[y][x] = (31, 0, 0) if x <= 53 else (0, 63, 0) if x <= 106 else (0, 0, 31)
    # Read only font bitmap content; placement and channel arithmetic are independent.
    for text, sx, sy, color in [("PICO MADO", 26, 20, white),
                                ("FIXED FRAME", 14, 56, (0, 63, 31))]:
        for k, ch in enumerate(text):
            if ch == " ":
                continue
            for column, bits in enumerate(glyphs[ord(ch) - ord("A")]):
                for row in range(7):
                    if bits // (2 ** row) % 2:
                        for dy in range(2):
                            for dx in range(2):
                                canvas[sy + 2 * row + dy][sx + 12 * k + 2 * column + dx] = color
    result = bytearray()
    for y in range(79, -1, -1):
        for x in range(159, -1, -1):
            r, g, b = canvas[y][x]
            word = (31 - b) * 2048 + (63 - g) * 32 + (31 - r)
            result.extend(word.to_bytes(2, "big"))
    return bytes(result)


def undo_lcd_transform(payload):
    """RGB565BE -> inverse invert/RB swap -> inverse 180 degree rotation -> RGB8."""
    if len(payload) != FRAME_BYTES:
        raise ValueError("image length must be exactly 25600 bytes")
    rgb = bytearray(WIDTH * HEIGHT * 3)
    for pixel in range(WIDTH * HEIGHT):
        word = int.from_bytes(payload[2 * pixel:2 * pixel + 2], "big")
        wire_r, wire_g, wire_b = word // 2048, word // 32 % 64, word % 32
        r, g, b = 31 - wire_b, 63 - wire_g, 31 - wire_r
        destination = 3 * (WIDTH * HEIGHT - 1 - pixel)
        rgb[destination:destination + 3] = bytes((r * 255 // 31, g * 255 // 63, b * 255 // 31))
    return bytes(rgb)


def difference(actual, expected):
    if len(actual) != FRAME_BYTES or len(expected) != FRAME_BYTES:
        raise ValueError("full-frame comparison requires 25600 bytes")
    offsets = [i for i, (a, b) in enumerate(zip(actual, expected)) if a != b]
    first = offsets[0] if offsets else None
    return {"different_bytes": len(offsets), "first_offset": first,
            "first_wire_row": None if first is None else first // 320,
            "first_wire_column": None if first is None else first % 320 // 2,
            "byte_in_pixel": None if first is None else first % 2}


def save_report(frames, expected, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    (output / "expected-ram.bin").write_bytes(expected)
    Image.frombytes("RGB", (WIDTH, HEIGHT), undo_lcd_transform(expected)).save(output / "expected.png")
    report = {"stage": "transformed_RAM_before_SPI_not_LCD_readback", "frames": []}
    for snapshot in frames:
        frame = int(snapshot.metadata["frame"])
        (output / f"frame-{frame}-ram.bin").write_bytes(snapshot.pixels)
        Image.frombytes("RGB", (WIDTH, HEIGHT), undo_lcd_transform(snapshot.pixels)).save(output / f"frame-{frame}.png")
        report["frames"].append({"metadata": snapshot.metadata,
                                 "expected_comparison": difference(snapshot.pixels, expected),
                                 "first_frame_comparison": difference(snapshot.pixels, frames[0].pixels)})
    report["all_bytes_match_expected"] = all(f["expected_comparison"]["different_bytes"] == 0
                                             for f in report["frames"])
    (output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def collect(board_id, timeout):
    parser = CaptureParser()
    deadline, fd, buffer, requested = time.monotonic() + timeout, None, "", False
    transcript = []
    try:
        while time.monotonic() < deadline:
            if fd is None:
                chosen = None
                for port in glob.glob("/dev/ttyACM*"):
                    for parent in (Path("/sys/class/tty") / Path(port).name / "device").resolve().parents:
                        if not (parent / "idVendor").exists():
                            continue
                        if ((parent / "idVendor").read_text().strip() == "2e8a"
                                and (parent / "idProduct").read_text().strip() == "000a"
                                and (parent / "serial").read_text().strip().upper() == board_id.upper()):
                            chosen = port
                        break
                if chosen is None:
                    time.sleep(0.1)
                    continue
                fd = os.open(chosen, os.O_RDWR | os.O_NONBLOCK | os.O_NOCTTY)
                attrs = termios.tcgetattr(fd)
                if attrs[4] == termios.B1200 or attrs[5] == termios.B1200:
                    raise ValueError("refuse 1200 baud: preserve application mode")
                attrs[0] = attrs[1] = attrs[3] = 0
                attrs[6][termios.VMIN] = attrs[6][termios.VTIME] = 0
                termios.tcsetattr(fd, termios.TCSANOW, attrs)
            ready, _, _ = select.select([fd], [], [], 0.5)
            if not ready:
                continue
            try:
                raw = os.read(fd, 8192)
            except OSError:
                raw = b""
            if not raw:
                os.close(fd)
                fd = None
                buffer = ""
                if parser.started:
                    raise ValueError("USB detached during snapshot batch; partial data rejected")
                requested = False
                continue
            buffer += raw.decode("ascii", "replace")
            while "\n" in buffer:
                line, buffer = buffer.split("\n", 1)
                line = line.strip()
                if line.startswith("LCDCAP READY ") and not requested:
                    os.write(fd, b"D")
                    requested = True
                if line.startswith("LCDCAP "):
                    transcript.append(line)
                    parser.feed(line)
                if parser.complete:
                    return parser.frames, transcript
        raise ValueError("capture timeout: no complete diagnostic snapshot batch")
    finally:
        if fd is not None:
            os.close(fd)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--collect", action="store_true")
    mode.add_argument("--from-log", type=Path)
    ap.add_argument("--board-id")
    ap.add_argument("--timeout", type=float, default=120)
    ap.add_argument("--output", required=True, type=Path)
    ap.add_argument("--glyph-source", type=Path, default=Path(__file__).resolve().parents[2] / "src/lcd.c")
    args = ap.parse_args()
    os.umask(0o077)
    if args.collect:
        if not args.board_id or not re.fullmatch(r"[0-9A-Fa-f]{16}", args.board_id):
            ap.error("--collect requires the verified --board-id")
        frames, transcript = collect(args.board_id, args.timeout)
    else:
        transcript = args.from_log.read_text().splitlines()
        parser = CaptureParser()
        for line in transcript:
            parser.feed(line.strip())
        if not parser.complete:
            raise ValueError("incomplete snapshot transcript")
        frames = parser.frames
    report = save_report(frames, independent_expected(args.glyph_source), args.output)
    (args.output / "usb-capture.log").write_text("\n".join(transcript) + "\n")
    print(json.dumps({"snapshots": len(frames), "all_bytes_match_expected": report["all_bytes_match_expected"],
                      "stage": report["stage"], "output": str(args.output)}))


if __name__ == "__main__":
    main()
