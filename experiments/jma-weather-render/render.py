#!/usr/bin/env python3
"""Reproducible Tokyo JMA forecast render experiment. No service integration."""
import argparse
import io
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

import requests
from PIL import Image, ImageDraw, ImageFont

JST = timezone(timedelta(hours=9))
URL = "https://www.jma.go.jp/bosai/forecast/data/forecast/130000.json"
FONT = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"
AREA = ("130010", "東京地方")
POINT = ("44132", "東京")


def fetch(path):
    start = datetime.now(timezone.utc).isoformat()
    r = requests.get(URL, timeout=20, headers={"User-Agent": "PicoMado-JMA-render-experiment/1.0"})
    r.raise_for_status()
    data = r.json()
    path.write_bytes(r.content)
    return data, {"url": URL, "status": r.status_code, "retrieved_utc": start,
                  "response_date": r.headers.get("Date"), "last_modified": r.headers.get("Last-Modified"),
                  "bytes": len(r.content)}


def area_series(block, ti, area_code):
    series = block["timeSeries"][ti]
    area = next(a for a in series["areas"] if a["area"]["code"] == area_code)
    return series["timeDefines"], area


def dated_values(times, values):
    return {datetime.fromisoformat(t).astimezone(JST).date().isoformat(): v for t, v in zip(times, values)}


def rows_from_jma(raw, today):
    short, weekly = raw
    swt, swa = area_series(short, 0, AREA[0]); wwt, wwa = area_series(weekly, 0, AREA[0])
    stt, sta = area_series(short, 2, POINT[0]); wtt, wta = area_series(weekly, 1, POINT[0])
    short_weather = dated_values(swt, swa["weatherCodes"])
    week_weather = dated_values(wwt, wwa["weatherCodes"])
    week_min = dated_values(wtt, wta["tempsMin"])
    week_max = dated_values(wtt, wta["tempsMax"])
    issued = datetime.fromisoformat(short["reportDatetime"])
    # Short-term temperature timestamps at 00 and 09 JST denote daily min/max.
    # A 00-JST slot before the issuance is not a future forecast for today's low.
    short_temps = {}
    for t, v in zip(stt, sta["temps"]):
        dt = datetime.fromisoformat(t).astimezone(JST)
        key = "min" if dt.hour == 0 else "max" if dt.hour == 9 else None
        if key and v != "" and (dt >= issued if key == "min" else dt.date() >= issued.date()):
            short_temps.setdefault(dt.date().isoformat(), {})[key] = v
    out = []
    for i in range(7):
        date = (today + timedelta(days=i)).isoformat()
        code = short_weather.get(date) or week_weather.get(date)
        source = "短期" if date in short_weather else "週間" if date in week_weather else None
        # Short-term temperatures take precedence only when their individual slot exists.
        # Weekly blanks remain missing, never zero or observed values.
        temp = short_temps.get(date, {})
        tmin = temp.get("min") or week_min.get(date) or None
        tmax = temp.get("max") or week_max.get(date) or None
        if not code:
            raise ValueError(f"weather missing for {date}")
        out.append({"date_jst": date, "weather_code": code, "weather_source": source,
                    "min_c": tmin, "max_c": tmax, "humidity": None,
                    "temp_min_source": "短期" if temp.get("min") else "週間" if week_min.get(date) else None,
                    "temp_max_source": "短期" if temp.get("max") else "週間" if week_max.get(date) else None})
    return out


def mark(draw, x, y, scale, code, ink):
    # JMA weather code first digit is the primary sky/precipitation class.
    category = code[0]
    r = 10 * scale
    if category == "1":
        draw.ellipse((x-r, y-r, x+r, y+r), fill="#ffd34f" if ink else 0, outline="#c67500" if ink else 0, width=2)
        for dx, dy in [(0,-17),(0,17),(-17,0),(17,0),(-12,-12),(12,12),(-12,12),(12,-12)]:
            draw.line((x+dx*scale*.68,y+dy*scale*.68,x+dx*scale,y+dy*scale), fill="#df9a00" if ink else 0, width=2)
    else:
        c = "#d3e3ec" if ink else 255
        o = "#4a6379" if ink else 0
        draw.ellipse((x-18*scale,y-5*scale,x-2*scale,y+11*scale),fill=c,outline=o,width=2)
        draw.ellipse((x-5*scale,y-13*scale,x+15*scale,y+11*scale),fill=c,outline=o,width=2)
        draw.rectangle((x-18*scale,y,x+15*scale,y+11*scale),fill=c)
        draw.line((x-17*scale,y+11*scale,x+15*scale,y+11*scale),fill=o,width=2)
        if category in ("3","4"):
            for dx in (-8,1,10):
                draw.line((x+dx*scale,y+16*scale,x+(dx-3)*scale,y+22*scale),fill="#2384cb" if ink else 0,width=2)


def render(row, size, color):
    w,h=size
    im=Image.new("RGB" if color else "1",size,"#ecf5fa" if color else 1)
    d=ImageDraw.Draw(im)
    def f(n): return ImageFont.truetype(FONT,n)
    fg="#102e47" if color else 0
    date=datetime.fromisoformat(row["date_jst"])
    if color:
        d.rectangle((0,0,w-1,18),fill="#123c58")
        d.text((5,1),f"{date:%m/%d}  {('月火水木金土日')[date.weekday()]}  {row['weather_source']}",font=f(12),fill="white")
        d.text((4,19),"予報:東京地方  気温:東京",font=f(9),fill=fg)
        mark(d,34,52,1.1,row["weather_code"],True)
        d.text((66,33),f"低 {row['min_c'] or 'ーー'}°",font=f(13),fill=fg)
        d.text((66,54),f"高 {row['max_c'] or 'ーー'}°",font=f(13),fill=fg)
    else:
        d.rectangle((0,0,w-1,h-1),outline=0)
        d.text((7,3),f"{date:%m/%d}  {('月火水木金土日')[date.weekday()]}  {row['weather_source']}予報",font=f(16),fill=0)
        d.line((5,28,w-6,28),fill=0)
        d.text((7,30),"予報: 東京地方  気温: 東京",font=f(12),fill=0)
        mark(d,44,82,1.45,row["weather_code"],False)
        d.text((91,54),f"最低 {row['min_c'] or 'ーー'}°C",font=f(17),fill=0)
        d.text((91,80),f"最高 {row['max_c'] or 'ーー'}°C",font=f(17),fill=0)
        d.text((7,107),"気象庁 / 東京駅の代表予報",font=f(10),fill=0)
    return im


def main():
    ap=argparse.ArgumentParser();ap.add_argument("--generation",type=Path,help="reuse saved source JSON; no HTTP")
    args=ap.parse_args()
    root=Path(__file__).resolve().parent
    if args.generation:
        gen=args.generation.resolve(); raw=json.loads((gen/"forecast-130000.json").read_text()); manifest=json.loads((gen/"manifest.json").read_text());manifest["reruns"] = manifest.get("reruns",0)+1
    else:
        gen=root/"data"/datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        gen.mkdir(parents=True,exist_ok=False)
        raw,request=fetch(gen/"forecast-130000.json")
        manifest={"generation":gen.name,"request":request,"issuing_office":raw[0]["publishingOffice"],
                  "short_issued_jst":raw[0]["reportDatetime"],"weekly_issued_jst":raw[1]["reportDatetime"],
                  "forecast_area":{"code":AREA[0],"name":AREA[1]},"temperature_point":{"code":POINT[0],"name":POINT[1]},
                  "mapping":"東京駅 -> 東京地方 (予報) + 東京 (気温代表地点)","reruns":0}
    today=datetime.fromisoformat(manifest["request"]["retrieved_utc"]).astimezone(JST).date()
    manifest["today_jst"]=today.isoformat()
    rows=rows_from_jma(raw,today)
    (gen/"derived-days.json").write_text(json.dumps(rows,ensure_ascii=False,indent=2)+"\n")
    frames=[]
    for i,row in enumerate(rows):
        for kind,size,color in [("lcd",(160,80),True),("eink",(250,122),False)]:
            im=render(row,size,color)
            path=gen/f"{kind}-{i:02d}-{row['date_jst']}.png";im.save(path)
            assert Image.open(path).size==size and Image.open(path).mode==("RGB" if color else "1")
            frames.append((kind,i,im))
    sheet=Image.new("RGB",(7*260,300),"#ffffff");d=ImageDraw.Draw(sheet)
    for kind,i,im in frames:
        x=i*260+5;y=5 if kind=="lcd" else 150
        if kind=="lcd":im=im.resize((480,240),Image.Resampling.NEAREST).resize((240,120),Image.Resampling.LANCZOS)
        sheet.paste(im.convert("RGB"),(x,y))
    sheet.save(gen/"contact-sheet.png")
    manifest["days"]=[r["date_jst"] for r in rows]
    manifest["missing_temperatures"]=[{"date":r["date_jst"],"field":k} for r in rows for k in ("min_c","max_c") if r[k] is None]
    manifest["humidity"]="not present in forecast JSON; omitted"
    (gen/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n")
    print(gen)
    print("days",len(rows),"missing",manifest["missing_temperatures"],"reruns",manifest["reruns"])

if __name__=="__main__":main()
