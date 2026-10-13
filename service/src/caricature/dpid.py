"""Exact legacy DPID canonicalization helper, invoked only by the Rust worker.

Requires Pillow, NumPy and pepedpid. A missing dependency fails before Codex
generation starts; there is no resizing or model fallback.
"""
import sys
from io import BytesIO

import numpy as np
from pepedpid import dpid_resize
from PIL import Image


def main():
    source, target, lam, threshold = sys.argv[1:]
    lam = float(lam)
    threshold = int(threshold)
    if not np.isfinite(lam) or not 0 <= lam <= 1 or not 1 <= threshold <= 254:
        raise ValueError("invalid normalization settings")
    data = open(source, "rb").read()
    if not 0 < len(data) <= 20_000_000:
        raise ValueError("invalid source size")
    with Image.open(BytesIO(data)) as image:
        image.load()
        gray = image.convert("L")
    width, height = gray.size
    if width * 122 > height * 250:
        size = 250, max(1, round(height * 250 / width))
    elif width * 122 < height * 250:
        size = max(1, round(width * 122 / height)), 122
    else:
        size = 250, 122
    luminance = np.asarray(gray, dtype=np.float32) / np.float32(255.0)
    pixels = np.ascontiguousarray(np.repeat(luminance[:, :, None], 3, axis=2))
    reduced = dpid_resize(pixels, size[1], size[0], lam)
    if (reduced.shape != (size[1], size[0], 3) or not np.isfinite(reduced).all()
            or not np.array_equal(reduced[:, :, 0], reduced[:, :, 1])
            or not np.array_equal(reduced[:, :, 0], reduced[:, :, 2])):
        raise ValueError("invalid DPID output")
    output = np.rint(np.clip(reduced[:, :, 0], 0, 1) * 255).astype(np.uint8)
    fitted = Image.fromarray(output)
    canvas = Image.new("L", (250, 122), 255)
    canvas.paste(fitted, ((250 - size[0]) // 2, (122 - size[1]) // 2))
    canonical = canvas.point(lambda n: 255 if n >= threshold else 0, mode="1")
    canonical.save(target, format="PNG", optimize=False)


if __name__ == "__main__":
    main()
