#!/usr/bin/env python3
"""JMA high-resolution precipitation nowcast + JMA-hosted GSI map tiles."""
import argparse
import io
import json
import math
from datetime import datetime, timedelta, timezone
from pathlib import Path

import requests
from PIL import Image, ImageDraw, ImageFilter, ImageFont

JST=timezone(timedelta(hours=9))
ROOT="https://www.jma.go.jp"
N1=ROOT+"/bosai/jmatile/data/nowc/targetTimes_N1.json"
N2=ROOT+"/bosai/jmatile/data/nowc/targetTimes_N2.json"
XML=ROOT+"/bosai/nowc/table/nowc.properties__7d6f7d8dc6f16a416574.xml"
JS=ROOT+"/bosai/jmatile/js/jmatile.bundle__6888179e2a40840a12d6.js"
FONT="/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"
ZOOM=4  # official hrpns minimum; one 2x2 tile crop covers the main islands
BBOX=(125.0,24.0,148.0,47.0)  # W,S,E,N; nationwide incl. most Nansei Islands


def fetch(url,path,records):
    if path.is_file():return path.read_bytes()
    r=requests.get(url,timeout=25,headers={"User-Agent":"PicoMado-JMA-render-experiment/1.0"})
    r.raise_for_status();path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(r.content)
    records.append({"url":url,"status":r.status_code,"retrieved_utc":datetime.now(timezone.utc).isoformat(),
                    "response_date":r.headers.get("Date"),"last_modified":r.headers.get("Last-Modified"),
                    "content_type":r.headers.get("Content-Type"),"bytes":len(r.content)})
    return r.content


def parse_time(s):return datetime.strptime(s,"%Y%m%d%H%M%S").replace(tzinfo=timezone.utc)


def select(n1,n2):
    obs=sorted([r for r in n1 if "hrpns" in r["elements"] and r["basetime"]==r["validtime"]],key=lambda r:r["validtime"])
    if not obs:raise ValueError("no observed hrpns time")
    latest=obs[-1];base=parse_time(latest["validtime"])
    bytime={parse_time(r["validtime"]):r for r in obs}
    frames=[]
    for hour in (-3,-2,-1,0):
        r=bytime.get(base+timedelta(hours=hour))
        if r:frames.append({"basetime":r["basetime"],"validtime":r["validtime"],"kind":"観測","offset_h":hour})
    futures=[r for r in n2 if r["basetime"]==latest["basetime"] and "hrpns" in r["elements"]]
    for r in futures:
        if parse_time(r["validtime"])==base+timedelta(hours=1):
            frames.append({"basetime":r["basetime"],"validtime":r["validtime"],"kind":"予測","offset_h":1});break
    return latest,frames


def merc(lon,lat,z):
    n=2**z; x=(lon+180)/360*n*256
    lat=max(-85.05112878,min(85.05112878,lat))
    y=(1-math.asinh(math.tan(math.radians(lat)))/math.pi)/2*n*256
    return x,y


def tiles():
    x0,y0=merc(BBOX[0],BBOX[3],ZOOM);x1,y1=merc(BBOX[2],BBOX[1],ZOOM)
    return [(x,y) for x in range(int(x0//256),int(math.ceil(x1/256)))
            for y in range(int(y0//256),int(math.ceil(y1/256)))]


def tile_url(frame,x,y):
    b=frame["basetime"];v=frame["validtime"]
    # Official nowc XML dataRootUrl + jmatile JS default tile pattern.
    return f"{ROOT}/bosai/jmatile/data/nowc/{b}/none/{v}/surf/hrpns/{ZOOM}/{x}/{y}.png"


def nd_url(frame):
    b=frame["basetime"];v=frame["validtime"]
    return f"{ROOT}/bosai/jmatile/data/nowc/{b}/none/{v}/surf/hrpns_nd/data.geojson"


def stitch(gen,frame,records,offline):
    xy=tiles();x_min=min(x for x,y in xy);y_min=min(y for x,y in xy)
    countx=max(x for x,y in xy)-x_min+1;county=max(y for x,y in xy)-y_min+1
    bg=Image.new("RGBA",(countx*256,county*256),(255,255,255,255))
    rain=Image.new("RGBA",bg.size,(0,0,0,0))
    for x,y in xy:
        mp=gen/"cache"/"map"/str(ZOOM)/str(x)/f"{y}.png"
        rp=gen/"cache"/"rain"/frame["basetime"]/frame["validtime"]/str(ZOOM)/str(x)/f"{y}.png"
        mu=f"{ROOT}/tile/gsi/pale2/{ZOOM}/{x}/{y}.png"
        for url,path,dst in [(mu,mp,bg),(tile_url(frame,x,y),rp,rain)]:
            if offline and not path.is_file():raise FileNotFoundError(path)
            b=fetch(url,path,records)
            im=Image.open(io.BytesIO(b)).convert("RGBA")
            if im.size!=(256,256):raise ValueError(f"bad tile size: {url}")
            dst.alpha_composite(im,((x-x_min)*256,(y-y_min)*256))
    left,top=merc(BBOX[0],BBOX[3],ZOOM);right,bottom=merc(BBOX[2],BBOX[1],ZOOM)
    crop=(round(left-x_min*256),round(top-y_min*256),round(right-x_min*256),round(bottom-y_min*256))
    return bg.crop(crop),rain.crop(crop)


def nd_mask(gen,frame,size,records,offline):
    path=gen/"cache"/"nodata"/f"{frame['basetime']}-{frame['validtime']}.geojson"
    if offline and not path.is_file():raise FileNotFoundError(path)
    obj=json.loads(fetch(nd_url(frame),path,records))
    if any(f["geometry"]["type"]!="Polygon" for f in obj["features"]):raise ValueError("unsupported nodata geometry")
    # The published geometry is the world polygon with a coverage hole.
    # Start unavailable everywhere, then clear the hole. Never infer coverage from transparent rain PNG.
    mask=Image.new("1",size,1);d=ImageDraw.Draw(mask)
    w,h=size;x0,y0=merc(BBOX[0],BBOX[3],ZOOM);x1,y1=merc(BBOX[2],BBOX[1],ZOOM)
    def pts(ring):
        return [((merc(lon,lat,ZOOM)[0]-x0)/(x1-x0)*w,(merc(lon,lat,ZOOM)[1]-y0)/(y1-y0)*h) for lon,lat in ring]
    for f in obj["features"]:
        rings=f["geometry"]["coordinates"]
        for hole in rings[1:]:d.polygon(pts(hole),fill=0)
    return mask


def render(bg,rain,nd,frame,size,color):
    w,h=size;header=19 if color else 25
    mh=h-header
    # Japan runs southwest-to-northeast. Rotate the same geographic raster for all
    # three layers so the islands use the narrow screen without anisotropic stretch.
    def screen(im,fill,resample):
        rot=im.rotate(-35,Image.Resampling.BICUBIC,expand=True,fillcolor=fill)
        rw,rh=rot.size; target=w/mh
        if rw/rh > target:
            cw=round(rh*target);box=((rw-cw)//2,0,(rw-cw)//2+cw,rh)
        else:
            ch=round(rw/target);box=(0,(rh-ch)//2,rw,(rh-ch)//2+ch)
        return rot.crop(box).resize((w,mh),resample)
    base=screen(bg.convert("RGB"),(230,241,250),Image.Resampling.LANCZOS)
    wet=screen(rain,(0,0,0,0),Image.Resampling.LANCZOS)
    no=screen(nd,1,Image.Resampling.NEAREST)
    dt=parse_time(frame["validtime"]).astimezone(JST)
    label=f"{dt:%m/%d %H:%M} {frame['kind']}"
    if color:
        out=Image.new("RGB",size,"#123c58");out.paste(base,(0,header))
        out.paste(wet.convert("RGB"),(0,header),wet.getchannel("A"))
        d=ImageDraw.Draw(out)
        # diagonal cross hatch for official no-data polygon
        for x in range(w):
            for y in range(mh):
                if no.getpixel((x,y)) and (x+y)%7==0:d.point((x,y+header),fill="#65737c")
        d.text((3,0),label,font=ImageFont.truetype(FONT,12),fill="white")
        d.text((w-78,5),"気象庁/国土地理院",font=ImageFont.truetype(FONT,7),fill="#c9e1f2")
    else:
        out=Image.new("1",size,1);d=ImageDraw.Draw(out)
        d.text((4,1),label,font=ImageFont.truetype(FONT,16),fill=0)
        d.text((w-88,header-11),"気象庁/国土地理院",font=ImageFont.truetype(FONT,8),fill=0)
        d.line((0,header-1,w,header-1),fill=0)
        # Thin base contours and roads; dense black rain; sparse no-data hatch.
        gray=base.convert("L");alpha=wet.getchannel("A")
        wet_binary=alpha.point(lambda v:255 if v>45 else 0)
        wet_halo=wet_binary.filter(ImageFilter.MaxFilter(3))
        pix=out.load();g=gray.load();a=wet_binary.load();halo=wet_halo.load();n=no.load()
        for y in range(mh):
            for x in range(w):
                if n[x,y]:pix[x,y+header]=0 if (x+y)%8==0 else 1
                elif a[x,y]:pix[x,y+header]=0
                elif halo[x,y]:pix[x,y+header]=1
                elif g[x,y]<125:pix[x,y+header]=0
        d.rectangle((106,h-14,w-1,h-1),fill=1)
        d.text((108,h-13),"黒=降水 斜線=欠測",font=ImageFont.truetype(FONT,9),fill=0)
    return out


def main():
    ap=argparse.ArgumentParser();ap.add_argument("--generation",type=Path,help="rerender cached generation offline")
    args=ap.parse_args();offline=bool(args.generation)
    root=Path(__file__).resolve().parent
    gen=args.generation.resolve() if offline else root/"data"/datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    gen.mkdir(parents=True,exist_ok=offline)
    records=[]
    n1=json.loads(fetch(N1,gen/"targetTimes_N1.json",records) if not offline else (gen/"targetTimes_N1.json").read_bytes())
    n2=json.loads(fetch(N2,gen/"targetTimes_N2.json",records) if not offline else (gen/"targetTimes_N2.json").read_bytes())
    if not offline:
        fetch(XML,gen/"nowc.properties.xml",records)
        # A short excerpt in the manifest below records the JS URL rule; full JS is not copied.
    latest,frames=select(n1,n2)
    if not frames:raise ValueError("no hourly frames")
    if offline:
        manifest=json.loads((gen/"manifest.json").read_text());manifest["reruns"]+=1
    else:
        manifest={"generation":gen.name,"source":"JMA high-resolution precipitation nowcast (hrpns intensity)",
                  "element":"hrpns","nodata_element":"hrpns_nd","zoom":ZOOM,"bbox_wsen":BBOX,
                  "official_properties_xml":XML,"official_site_js":JS,"latest_observed":latest,
                  "N1_bounds_utc":[min(r['validtime'] for r in n1),max(r['validtime'] for r in n1)],
                  "N2_bounds_utc":[min(r['validtime'] for r in n2),max(r['validtime'] for r in n2)],
                  "hourly_frames":frames,"requests":records,"reruns":0,
                  "map_attribution":"国土地理院 地理院タイル 淡色地図 (気象庁サイト配信 /tile/gsi/pale2); JMA precipitation data", 
                  "tile_formula":"{dataRootUrl}/{basetime}/none/{validtime}/surf/hrpns/{z}/{x}/{y}.png (JMA jmatile JS)"}
    output=[]
    for i,fr in enumerate(frames):
        bg,rain=stitch(gen,fr,records,offline)
        nd=nd_mask(gen,fr,bg.size,records,offline)
        entry={"frame":fr,"tile_count":len(tiles()),"nd_pixels":sum(1 for v in nd.getdata() if v),
               "rain_alpha_pixels":sum(1 for v in rain.getchannel("A").getdata() if v>0)}
        for kind,size,color in [("lcd",(160,80),True),("eink",(250,122),False)]:
            im=render(bg,rain,nd,fr,size,color)
            path=gen/f"{kind}-{i:02d}-{fr['validtime']}.png";im.save(path)
            check=Image.open(path);assert check.size==size and check.mode==("RGB" if color else "1")
            entry[kind]=path.name
        output.append(entry)
    manifest["requests"]=records if not offline else manifest["requests"]
    manifest["frames_checked"]=output
    (gen/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n")
    sheet=Image.new("RGB",(len(frames)*260,240),"white")
    for i,fr in enumerate(frames):
        for kind,y in [("lcd",0),("eink",105)]:
            im=Image.open(gen/f"{kind}-{i:02d}-{fr['validtime']}.png").convert("RGB")
            sheet.paste(im,(i*260+5,y+5))
    sheet.save(gen/"contact-sheet.png")
    print(gen)
    print("N1",len(n1),"N2",len(n2),"hourly",[(f['offset_h'],f['kind']) for f in frames],"HTTP",len(records),"reruns",manifest["reruns"])

if __name__=="__main__":main()
