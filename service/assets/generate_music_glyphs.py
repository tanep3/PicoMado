#!/usr/bin/env python3
"""Local music only: Noto Sans CJK JP, ASCII + JIS X0208 + halfwidth kana.
Record: codepoint LE32, then two (14px,16px) metrics: width,height,x:i8,y:i8,
advance,pad,offset:LE32. Bitmap is packed MSB-first, contiguous per glyph.
License: adjacent NOTO-COPYRIGHT. Does not touch weather/radar assets.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import struct, math, hashlib, json
root=Path(__file__).parent
path=Path('/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc')
chars=set(map(chr,range(32,127)))|set(map(chr,range(0xff61,0xffa0)))|set('…�')
for a in range(0xa1,0xff):
 for b in range(0xa1,0xff):
  try: chars.add(bytes([a,b]).decode('euc_jp'))
  except UnicodeDecodeError: pass
chars=sorted(chars)
fonts=[ImageFont.truetype(str(path),size,index=0) for size in (14,16)]
assert all(f.getname()[0]=='Noto Sans CJK JP' for f in fonts)
baselines=[-min(f.getbbox(c,anchor='ls')[1] for c in chars) for f in fonts]
index=bytearray(); pixels=bytearray()
for ch in chars:
 index+=struct.pack('<I',ord(ch))
 for font,baseline in zip(fonts,baselines):
  l,t,r,b=font.getbbox(ch,anchor='ls');w,h=r-l,b-t
  index+=struct.pack('<BBbbBBI',w,h,l,baseline+t,max(1,math.ceil(font.getlength(ch))),0,len(pixels))
  im=Image.new('L',(max(w,1),max(h,1)))
  ImageDraw.Draw(im).text((-l,-t),ch,font=font,fill=255,anchor='ls')
  bits=[v>96 for v in im.getdata()] if w*h else []
  for start in range(0,len(bits),8): pixels.append(sum(int(v)<<(7-i) for i,v in enumerate(bits[start:start+8])))
(root/'music-glyph-index.bin').write_bytes(index)
(root/'music-glyphs.bin').write_bytes(pixels)
(root/'music-font.json').write_text(json.dumps(dict(family=fonts[0].getname(),sizes=[14,16],source_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),characters=len(chars),baselines=baselines,license='NOTO-COPYRIGHT'),indent=2)+'\n')
print(len(chars),len(index),len(pixels),baselines)
