#!/usr/bin/env python3
"""Refresh the checked JMA location catalogue from public JMA JSON only.

This is a manual development tool. Review changes and run the Rust tests before
replacing the bundled snapshot; it never runs in the service.
"""
import json
from pathlib import Path
from urllib.request import urlopen

BASE = 'https://www.jma.go.jp/bosai'

def get_json(path):
    with urlopen(BASE + path, timeout=20) as response:
        return json.load(response)

area = get_json('/common/const/area.json')
locations = []
for office, office_info in area['offices'].items():
    try:
        forecast = get_json(f'/forecast/data/forecast/{office}.json')
        short = [entry['area'] for entry in forecast[0]['timeSeries'][0]['areas']]
        short_points = [entry['area'] for entry in forecast[0]['timeSeries'][2]['areas']]
        weekly = [entry['area'] for entry in forecast[1]['timeSeries'][0]['areas']]
        weekly_points = [entry['area'] for entry in forecast[1]['timeSeries'][1]['areas']]
    except (KeyError, IndexError, OSError, ValueError):
        continue  # An office without both required forecast series is not selectable.
    for short_area in short:
        weekly_area = next((w for w in weekly if w['code'] == short_area['code']), None)
        if weekly_area is None and len(weekly) == 1:
            weekly_area = weekly[0]
        if weekly_area is None:
            weekly_area = next((w for w in weekly if w['name'] in short_area['name']
                                or short_area['name'] in w['name']), None)
        if weekly_area is None:
            continue
        point = weekly_points[weekly.index(weekly_area)]
        if not any(p['code'] == point['code'] for p in short_points):
            continue
        is_tokyo = office == '130000' and short_area['code'] == '130010'
        is_ogasawara = office == '130000' and short_area['code'] == '130040'
        locations.append(dict(
            id='tokyo-station' if is_tokyo else 'ogasawara' if is_ogasawara
               else f"{office}:{short_area['code']}",
            label='東京駅' if is_tokyo else office_info['name'] if office_info['name'] == short_area['name']
                  else f"{office_info['name']} {short_area['name']}",
            office_code=office, office_label=office_info['name'],
            area_code=short_area['code'], area_label=short_area['name'],
            weekly_area_code=weekly_area['code'], point_code=point['code'],
            point_label=point['name'],
        ))
locations.sort(key=lambda v: (0 if v['id'] == 'tokyo-station' else
                              1 if v['id'] == 'ogasawara' else 2,
                              v['office_code'], v['area_code']))
Path(__file__).with_name('jma-locations.json').write_text(
    json.dumps(locations, ensure_ascii=False, indent=2) + '\n')
print(f'{len(locations)} mappings from {len({x["office_code"] for x in locations})} offices')
