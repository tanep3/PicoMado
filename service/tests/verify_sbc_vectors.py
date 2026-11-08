#!/usr/bin/env python3
"""Independent decode sanity for fixed raw SBC vectors; never plays audio."""
import subprocess
from pathlib import Path

vectors = Path(__file__).resolve().parent / "fixtures" / "sbc-v1-frames.txt"
count = 0
for line in vectors.read_text().splitlines():
    if not line or line.startswith("#"):
        continue
    mode, allocation, bitpool, encoded = line.split()
    result = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "sbc", "-i", "pipe:0",
         "-f", "s16le", "pipe:1"],
        input=bytes.fromhex(encoded), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=10, check=False,
    )
    assert result.returncode == 0, (mode, allocation, bitpool, result.stderr)
    assert len(result.stdout) == 128 * 2 * 2, (mode, allocation, bitpool)
    count += 1
assert count == 12
print("PASS: independent FFmpeg decode of 12 SBC tuples to128 stereo sample frames each")
