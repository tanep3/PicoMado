# Weather fixture and glyph atlas

`jma-tokyo-20261012.json` is the saved public JMA forecast response from
`experiments/jma-weather-render/data/20261012T032152Z/`. Tests use this
immutable source without another HTTP request.

`weather-glyphs.bin`, `weather-chars.txt`, and `weather-glyph-metrics.json`
are generated together by `generate_glyphs.py`. The LCD renderer uses native
12, 14, 16, 20, and 22 pixel glyphs with measured advances and ink offsets;
it never shrinks a larger tile. Both weather and radar LCD text use this atlas.
The generator explicitly selects collection face 0, **Noto Sans CJK JP Bold**,
from the already installed `/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc`.
The metadata records that face and the exact charset/offset order. The service
reads only the embedded assets, so it needs no runtime font or fallback.
The previous weather renderer used a 47-character index against a
232-character atlas; larger sizes consequently displayed unrelated glyphs.
Asset consistency and native text bounds now have regression tests.
The source font is Noto Sans CJK, copyright Google Corporation,
licensed under SIL Open Font License 1.1. The installed package's attribution
and license text are in `NOTO-COPYRIGHT`.

## JMA location catalogue

`jma-locations.json` and `jma-official-national-20261012.json` are 2026-10-12 UTC snapshots derived from the public
[JMA area catalogue](https://www.jma.go.jp/bosai/common/const/area.json) and
[office forecast JSON](https://www.jma.go.jp/bosai/forecast/data/forecast/130000.json).
It contains 139 short-range forecast areas from 56 offices, each paired with
a weekly forecast area and a representative temperature point present in both
short and weekly series. The available location label is a forecast area, not
a point forecast for the named town. Two listed offices are excluded: `014030`
(十勝地方, 帯広測候所) and `460040` (奄美地方, 名瀬測候所). On 2026-10-12 their
forecast URLs returned HTML rather than the short and weekly forecast JSON
needed to validate a selectable mapping. This is a snapshot limitation; it
does not establish that these areas are permanently unavailable.
`generate_locations.py` refreshes
the mapping manually; it is never run by the service. Regenerate glyphs with
`generate_glyphs.py` after reviewing any catalogue change.

The national snapshot is an offline test fixture from official HTTPS responses; it is not used as live service data. Each of the 139 selectable mappings is tested for seven-day parsing and LCD rendering against it.

## Radar fixture and static base map

`jma-radar-20261012.json` is derived from the saved public JMA high-resolution
precipitation nowcast generation in
`experiments/jma-radar-render/data/20261012T032404Z/`. It retains the original
N1/N2 time lists and the actual `hrpns` PNG bytes for five hourly frames.
Its `hrpns_nd` coverage rings were simplified to roughly one map pixel of
detail for deterministic 160×80 tests; the live renderer performs the same
screen-scale simplification after fetching the current JMA GeoJSON.
`radar-map/` contains the four saved zoom-4 JMA-hosted GSI `pale2` tiles used
as a common nationwide base map. Precipitation and no-data layers are fetched
from JMA for each live generation, so observations are never simulated from
the static map. See the [GSI tile terms](https://maps.gsi.go.jp/development/ichiran.html)
and [JMA data notice](https://www.jma.go.jp/jma/kishou/info/coment.html).

`radar-glyphs.bin` and `radar-chars.txt` retain the previous regular-font atlas
for the E-ink preview only. Its 12 pixel glyphs occupy 16×16 tiles; the LCD
renderer no longer reduces those tiles to 12/8 pixels. The radar LCD uses the
native JP Bold atlas above and its source label credits JMA and GSI.
`radar-lcd-preview.png` and `radar-eink-preview.png` are direct outputs of the
Rust renderer for the saved latest observation. They are review previews,
not images fetched or displayed by the live service.

## Word tutorial default card

`word-card.png` is the 160×80 default for the optional `word-tutorial` plugin.
Its lettering matches Noto Sans CJK JP Bold at 24 px, generated with
`service/examples/make-word-card.py`. The Noto source and SIL Open Font License
1.1 attribution are included in [`NOTO-COPYRIGHT`](NOTO-COPYRIGHT). Browser-edited
cards use the fonts available to that browser.
