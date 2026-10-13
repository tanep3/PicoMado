from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

font_file = '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
font = ImageFont.truetype(font_file, 18)
lines = [('wifi', 'Wifi接続中'), ('server', 'サーバ接続中')]
out = ['// Generated from the installed Noto Sans CJK JP font for two local status labels.', '#pragma once', '#include <stdint.h>', '#define PM_STATUS_W 160', '#define PM_STATUS_H 24']
for name, label in lines:
    img = Image.new('1', (160, 24), 0)
    draw = ImageDraw.Draw(img)
    box = draw.textbbox((0, 0), label, font=font)
    x = (160 - (box[2]-box[0])) // 2 - box[0]
    y = (24 - (box[3]-box[1])) // 2 - box[1]
    draw.text((x, y), label, font=font, fill=1)
    packed = []
    for row in range(24):
        for col in range(0, 160, 8):
            byte = 0
            for bit in range(8):
                byte |= bool(img.getpixel((col+bit, row))) << (7-bit)
            packed.append(byte)
    out.append(f'static const uint8_t pm_status_{name}[480] = {{')
    for start in range(0, len(packed), 16):
        out.append('  ' + ','.join(f'0x{n:02x}' for n in packed[start:start+16]) + ',')
    out.append('};')
Path(__file__).resolve().parent.parent.joinpath('src/status_bitmaps.h').write_text('\n'.join(out)+'\n')
