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
    source, target, lam, threshold = sys.argv[1:5]
    canvas_width, canvas_height = map(int, sys.argv[5:7]) if len(sys.argv) == 7 else (250, 122)
    if not 1 <= canvas_width <= 2048 or not 1 <= canvas_height <= 2048:
        raise ValueError("invalid canvas size")
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
    if width * canvas_height > height * canvas_width:
        size = canvas_width, max(1, round(height * canvas_width / width))
    elif width * canvas_height < height * canvas_width:
        size = max(1, round(width * canvas_height / height)), canvas_height
    else:
        size = canvas_width, canvas_height
    luminance = np.asarray(gray, dtype=np.float32) / np.float32(255.0)
    pixels = np.ascontiguousarray(np.repeat(luminance[:, :, None], 3, axis=2))
    reduced = dpid_resize(pixels, size[1], size[0], lam)
    if (reduced.shape != (size[1], size[0], 3) or not np.isfinite(reduced).all()
            or not np.array_equal(reduced[:, :, 0], reduced[:, :, 1])
            or not np.array_equal(reduced[:, :, 0], reduced[:, :, 2])):
        raise ValueError("invalid DPID output")
    output = np.rint(np.clip(reduced[:, :, 0], 0, 1) * 255).astype(np.uint8)
    fitted = Image.fromarray(output)
    canvas = Image.new("L", (canvas_width, canvas_height), 255)
    canvas.paste(fitted, ((canvas_width - size[0]) // 2, (canvas_height - size[1]) // 2))
    canonical = canvas.point(lambda n: 255 if n >= threshold else 0, mode="1")
    canonical.save(target, format="PNG", optimize=False)


if __name__ == "__main__":
    main()
