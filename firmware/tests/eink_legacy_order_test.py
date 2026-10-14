"""Independent byte oracle for the two exact legacy conversion stages."""
import subprocess
import sys

for pattern in ["corners", "dense", "stripes", "white"]:
    mono = bytearray([255] * 3813)
    wire = bytearray([255] * 4000)
    for y in range(122):
        for x in range(250):
            black = (
                pattern == "corners" and (x, y) in [(0, 0), (249, 0), (0, 121), (249, 121), (8, 1)]
                or pattern == "dense" and ((x * 17 + y * 31) % 23 < 9 or x == 0 or y == 121)
                or pattern == "stripes" and (y % 13 == 0 or x % 17 == 0)
            )
            if black:
                bit = y * 250 + x
                mono[bit // 8] &= ~(0x80 >> (bit % 8))
                u = 121 - y
                wire[x * 16 + u // 8] &= ~(0x80 >> (u % 8))
    # pico/protocol.py landscape_buffer, then panel_v4.py Panel.display.
    landscape = bytearray(4000)
    for i in range(250):
        for j in range(16):
            landscape[i + (15 - j) * 250] = wire[i * 16 + j]
    legacy = bytes(landscape[i + j * 250] for j in range(15, -1, -1) for i in range(250))
    actual = subprocess.run([sys.argv[1]], input=mono, stdout=subprocess.PIPE, check=True).stdout
    assert len(actual) == 4000 and actual == legacy, pattern
    assert all(actual[15 * 250 + x] & 0x3F == 0x3F for x in range(250))
print("legacy order: four asymmetric patterns, all 4000 SPI bytes identical; plane15 white padding PASS")
