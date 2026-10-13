#!/usr/bin/env python3
"""Build native-size LCD glyphs and metrics from the existing Japanese font."""
from pathlib import Path
from math import ceil
from PIL import Image, ImageDraw, ImageFont
import json

ROOT = Path(__file__).parent
FONT = '/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc'
FACE_INDEX = 0
SIZES = (12, 14, 16, 20, 22)
base = '0123456789/ 月火水木金土日短期週間予報東京地方気温低高ー°:小笠原諸島父取得失敗-?…'
locations = json.loads((ROOT / 'jma-locations.json').read_text())
chars = ''.join(dict.fromkeys(base + ''.join(
    str(item[key]) for item in locations
    for key in ('label', 'office_label', 'area_label', 'point_label')
) + (ROOT / 'radar-chars.txt').read_text()))
pixels = bytearray()
glyphs = []
for size in SIZES:
    font = ImageFont.truetype(FONT, size, index=FACE_INDEX)
    family, style = font.getname()
    assert family == 'Noto Sans CJK JP', (family, style)
    baseline = -min(font.getbbox(ch, anchor='ls')[1] for ch in chars)
    for ch in chars:
        left, top, right, bottom = font.getbbox(ch, anchor='ls')
        width, height = right - left, bottom - top
        glyphs.append({
            'offset': len(pixels), 'width': width, 'height': height,
            'x': left, 'y': baseline + top,
            'advance': max(1, ceil(font.getlength(ch))),
        })
        if width and height:
            bitmap = Image.new('L', (width, height), 0)
            ImageDraw.Draw(bitmap).text((-left, -top), ch, font=font, fill=255, anchor='ls')
            pixels.extend(bitmap.tobytes())

(ROOT / 'weather-chars.txt').write_text(chars)
(ROOT / 'weather-glyphs.bin').write_bytes(pixels)
(ROOT / 'weather-glyph-metrics.json').write_text(json.dumps({
    'font': {'path': FONT, 'family': family, 'style': style, 'collection_index': FACE_INDEX},
    'sizes': SIZES, 'chars': chars, 'glyphs': glyphs,
}, ensure_ascii=False, separators=(',', ':')) + '\n')
print(f'{family} {style}, face {FACE_INDEX}, {len(chars)} characters, {len(pixels)} alpha bytes')
