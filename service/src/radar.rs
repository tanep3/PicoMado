//! JMA high-resolution precipitation nowcast. The immutable hourly set,
//! browsing cursor, fetch freshness, and raster composition belong here.
//! The base map is the saved JMA-hosted GSI pale2 zoom-4 tile set; live
//! precipitation and no-data layers are fetched together for each generation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, FetchJob, PluginContext, PluginPlan, PluginPosition,
};
use crate::settings::{FetchPolicy, PluginSettings};
use crate::{Candidate, Capability, Event, Mode, Plugin, Priority, Transition, WorldQuery};

const ID: &str = "rain-radar";
const N1_URL: &str = "https://www.jma.go.jp/bosai/jmatile/data/nowc/targetTimes_N1.json";
const N2_URL: &str = "https://www.jma.go.jp/bosai/jmatile/data/nowc/targetTimes_N2.json";
const ROOT: &str = "https://www.jma.go.jp/bosai/jmatile/data/nowc";
const MAP_PNG: [&[u8]; 4] = [
    include_bytes!("../assets/radar-map/13/5.png"),
    include_bytes!("../assets/radar-map/14/5.png"),
    include_bytes!("../assets/radar-map/13/6.png"),
    include_bytes!("../assets/radar-map/14/6.png"),
];
const GLYPHS: &[u8] = include_bytes!("../assets/radar-glyphs.bin");
const CHARS: &str = include_str!("../assets/radar-chars.txt");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TargetTime {
    basetime: String,
    validtime: String,
    elements: Vec<String>,
}
#[derive(Deserialize, Serialize)]
pub struct RadarSource {
    n1: Vec<TargetTime>,
    n2: Vec<TargetTime>,
    /// Four PNG tiles in 13/5, 14/5, 13/6, 14/6 order.
    rain: BTreeMap<String, [String; 4]>,
    nodata: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct RadarFrame {
    validtime: String,
    observed: bool,
    pixels: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct RadarSet {
    id: String,
    frames: Vec<RadarFrame>,
    latest_observed: usize,
}
#[derive(Clone, Debug)]
struct RadarBrowse {
    latest: RadarSet,
    pinned: Option<RadarSet>,
    index: usize,
}
impl RadarBrowse {
    fn new(set: RadarSet) -> Self {
        let index = set.latest_observed;
        Self {
            latest: set,
            pinned: None,
            index,
        }
    }
    fn current(&self) -> (&RadarSet, &RadarFrame) {
        let set = self.pinned.as_ref().unwrap_or(&self.latest);
        (set, &set.frames[self.index])
    }
    fn publish(&mut self, set: RadarSet) {
        self.latest = set;
        if self.pinned.is_none() {
            self.index = self.latest.latest_observed;
        }
    }
    fn begin(&mut self) {
        self.pinned = Some(self.latest.clone());
        self.index = self.latest.latest_observed;
    }
    fn cancel(&mut self) {
        self.pinned = None;
        self.index = self.latest.latest_observed;
    }
    fn move_hour(&mut self, delta: i64) -> bool {
        let set = self.pinned.as_ref().unwrap_or(&self.latest);
        let current = &set.frames[self.index].validtime;
        let Some(target) = timestamp(current).map(|v| v + delta * 3600) else {
            return false;
        };
        if let Some(next) = set
            .frames
            .iter()
            .position(|f| timestamp(&f.validtime) == Some(target))
        {
            self.index = next;
            true
        } else {
            false
        }
    }
}
#[derive(Clone, Debug)]
struct RadarSlot {
    incarnation: u64,
    settings_revision: u64,
    browse: Option<RadarBrowse>,
    last_success_unix: Option<i64>,
    last_attempt_unix: Option<i64>,
    error: bool,
    generation: u64,
    in_flight: Option<u64>,
}
impl RadarSlot {
    fn new(ctx: &PluginContext) -> Self {
        Self {
            incarnation: ctx.token.incarnation,
            settings_revision: ctx.settings_revision,
            browse: None,
            last_success_unix: None,
            last_attempt_unix: None,
            error: false,
            generation: 0,
            in_flight: None,
        }
    }
    fn stored(&self) -> StoredSlot {
        StoredSlot {
            incarnation: self.incarnation,
            settings_revision: self.settings_revision,
            latest: self.browse.as_ref().map(|b| b.latest.clone()),
            last_success_unix: self.last_success_unix,
            last_attempt_unix: self.last_attempt_unix,
            error: self.error,
            generation: self.generation,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct StoredSlot {
    incarnation: u64,
    settings_revision: u64,
    latest: Option<RadarSet>,
    last_success_unix: Option<i64>,
    last_attempt_unix: Option<i64>,
    error: bool,
    generation: u64,
}
#[derive(Clone, Default)]
pub struct RadarContent {
    devices: BTreeMap<String, RadarSlot>,
}
impl RadarContent {
    fn slot(&self, ctx: &PluginContext) -> Option<&RadarSlot> {
        self.devices
            .get(&ctx.token.device_id)
            .filter(|s| s.incarnation == ctx.token.incarnation)
    }
    fn slot_mut(&mut self, ctx: &PluginContext) -> &mut RadarSlot {
        let slot = self
            .devices
            .entry(ctx.token.device_id.clone())
            .or_insert_with(|| RadarSlot::new(ctx));
        if slot.incarnation != ctx.token.incarnation {
            *slot = RadarSlot::new(ctx);
        }
        if slot.settings_revision != ctx.settings_revision {
            slot.settings_revision = ctx.settings_revision;
            slot.in_flight = None;
        }
        slot
    }
}
impl ContentPlugin for RadarContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        ID
    }
    fn publish(
        &mut self,
        ctx: &PluginContext,
        source: &[u8],
        dataset_id: &str,
        _: &str,
    ) -> Result<(), ContentError> {
        let set = parse_and_render(source, dataset_id).map_err(|_| ContentError::InvalidSource)?;
        let slot = self.slot_mut(ctx);
        match &mut slot.browse {
            Some(b) => b.publish(set),
            None => slot.browse = Some(RadarBrowse::new(set)),
        }
        slot.error = false;
        Ok(())
    }
    fn position(&self, ctx: &PluginContext) -> Option<PluginPosition> {
        let browse = self.slot(ctx)?.browse.as_ref()?;
        let (set, frame) = browse.current();
        Some(PluginPosition {
            plugin: ID.into(),
            dataset_id: set.id.clone(),
            target_date: frame.validtime.clone(),
            index: browse.index,
        })
    }
    fn current_frame(&self, ctx: &PluginContext) -> Option<Vec<u8>> {
        let slot = self.slot(ctx)?;
        if slot.error {
            return Some(error_lcd());
        }
        let (_, frame) = slot.browse.as_ref()?.current();
        lcd_frame(frame).ok()
    }
    fn plan(
        &mut self,
        ctx: &PluginContext,
        action: ButtonAction,
    ) -> Result<PluginPlan, ContentError> {
        let slot = self.slot_mut(ctx);
        let browse = slot.browse.as_mut().ok_or(ContentError::Unavailable)?;
        let event = match (action, &ctx.mode) {
            (ButtonAction::Sub, Mode::Normal) => {
                browse.begin();
                Some(Event::EnterSub {
                    token: ctx.token.clone(),
                })
            }
            (ButtonAction::Up, Mode::Sub) => browse.move_hour(-1).then(|| Event::PluginInput {
                token: ctx.token.clone(),
                name: "up".into(),
            }),
            (ButtonAction::Down, Mode::Sub) => browse.move_hour(1).then(|| Event::PluginInput {
                token: ctx.token.clone(),
                name: "down".into(),
            }),
            (ButtonAction::Power, Mode::Sub) => {
                browse.cancel();
                Some(Event::Power {
                    token: ctx.token.clone(),
                })
            }
            _ => return Err(ContentError::Unsupported),
        };
        let (set, frame) = browse.current();
        let pixels = if event.is_some() {
            if slot.error {
                Some(error_lcd())
            } else {
                Some(lcd_frame(frame).map_err(|_| ContentError::InvalidSource)?)
            }
        } else {
            None
        };
        Ok(PluginPlan {
            event,
            frame: pixels,
            position: PluginPosition {
                plugin: ID.into(),
                dataset_id: set.id.clone(),
                target_date: frame.validtime.clone(),
                index: browse.index,
            },
        })
    }
    fn session_reset(&mut self, id: &str, incarnation: u64) {
        if let Some(slot) = self.devices.get_mut(id) {
            if slot.incarnation == incarnation {
                if let Some(b) = &mut slot.browse {
                    b.cancel();
                }
            } else {
                self.devices.remove(id);
            }
        }
    }
    fn delete_device(&mut self, id: &str) {
        self.devices.remove(id);
    }
    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(
            self.devices
                .iter()
                .map(|(id, s)| (id.clone(), s.stored()))
                .collect::<BTreeMap<_, _>>(),
        )
        .expect("radar state")
    }
    fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
        let stored: BTreeMap<String, StoredSlot> =
            serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        self.devices = stored
            .into_iter()
            .map(|(id, s)| {
                (
                    id,
                    RadarSlot {
                        incarnation: s.incarnation,
                        settings_revision: s.settings_revision,
                        browse: s.latest.map(RadarBrowse::new),
                        last_success_unix: s.last_success_unix,
                        last_attempt_unix: s.last_attempt_unix,
                        error: s.error,
                        generation: s.generation,
                        in_flight: None,
                    },
                )
            })
            .collect();
        Ok(())
    }
    fn fetch_policy(&self, settings: &PluginSettings) -> Option<FetchPolicy> {
        Some(settings.radar_fetch.clone())
    }
    fn device_config(&self, id: &str, incarnation: u64) -> serde_json::Value {
        let s = self
            .devices
            .get(id)
            .filter(|s| s.incarnation == incarnation);
        serde_json::json!({"last_success_unix": s.and_then(|s| s.last_success_unix),
            "error": s.is_some_and(|s| s.error)})
    }
    fn plan_fetch(
        &mut self,
        ctx: &PluginContext,
        now: i64,
        today: &str,
        policy: &FetchPolicy,
    ) -> Option<FetchJob> {
        let slot = self.slot_mut(ctx);
        if slot.in_flight.is_some()
            || slot
                .last_success_unix
                .is_some_and(|at| now.saturating_sub(at) < 3600)
            || (slot.error
                && slot
                    .last_attempt_unix
                    .is_some_and(|at| now.saturating_sub(at) < 60))
        {
            return None;
        }
        slot.generation += 1;
        slot.in_flight = Some(slot.generation);
        slot.last_attempt_unix = Some(now);
        Some(FetchJob {
            plugin: ID.into(),
            device_id: ctx.token.device_id.clone(),
            incarnation: ctx.token.incarnation,
            settings_revision: ctx.settings_revision,
            config_revision: 0,
            generation: slot.generation,
            source_key: "nowc/hrpns".into(),
            dataset_id: format!("nowc:{}:{}", now, slot.generation),
            today_jst: today.into(),
            timeout_seconds: policy.timeout_seconds,
            retries: policy.retries,
        })
    }
    fn complete_fetch(
        &mut self,
        job: &FetchJob,
        now: i64,
        result: Result<Vec<u8>, String>,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        let slot = self
            .devices
            .get_mut(&job.device_id)
            .ok_or(ContentError::Stale)?;
        if slot.incarnation != job.incarnation
            || slot.settings_revision != job.settings_revision
            || slot.in_flight != Some(job.generation)
        {
            return Err(ContentError::Stale);
        }
        slot.in_flight = None;
        let set = result
            .ok()
            .and_then(|source| parse_and_render(&source, &job.dataset_id).ok());
        if let Some(set) = set {
            let pixels = BASE64
                .decode(&set.frames[set.latest_observed].pixels)
                .map_err(|_| ContentError::InvalidSource)?;
            match &mut slot.browse {
                Some(b) => b.publish(set),
                None => slot.browse = Some(RadarBrowse::new(set)),
            }
            slot.last_success_unix = Some(now);
            slot.error = false;
            Ok((!slot.browse.as_ref().is_some_and(|b| b.pinned.is_some())).then_some(pixels))
        } else {
            slot.error = true;
            Ok(Some(error_lcd()))
        }
    }
    fn cancel_fetches(&mut self) {
        for s in self.devices.values_mut() {
            if s.in_flight.take().is_some() {
                s.last_attempt_unix = None;
            }
        }
    }
}

pub struct RadarPlugin;
impl Plugin for RadarPlugin {
    fn id(&self) -> &str {
        ID
    }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool {
        caps.contains("lcd") || caps.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        query: &'a dyn WorldQuery,
        event: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        let candidate = match event {
            Event::EnterSub { token }
                if query.device(&token.device_id).is_some_and(|d| {
                    d.selected.as_deref() == Some(ID) && d.mode == Mode::Normal
                }) =>
            {
                Some(Candidate::new(
                    query,
                    &token.device_id,
                    Priority::User,
                    "radar-browse-start",
                    Transition::EnterMode {
                        token: token.clone(),
                        mode: Mode::Sub,
                    },
                ))
            }
            Event::PluginInput { token, name }
                if (name == "up" || name == "down")
                    && query.device(&token.device_id).is_some_and(|d| {
                        d.selected.as_deref() == Some(ID) && d.mode == Mode::Sub
                    }) =>
            {
                Some(Candidate::new(
                    query,
                    &token.device_id,
                    Priority::User,
                    "radar-browse-move",
                    Transition::RefreshView {
                        token: token.clone(),
                        plugin: ID.into(),
                    },
                ))
            }
            _ => None,
        };
        Box::new(candidate.into_iter())
    }
}

fn timestamp(s: &str) -> Option<i64> {
    if s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year = s[0..4].parse().ok()?;
    let month = Month::try_from(s[4..6].parse::<u8>().ok()?).ok()?;
    let day = s[6..8].parse().ok()?;
    let hour = s[8..10].parse().ok()?;
    let minute = s[10..12].parse().ok()?;
    let second = s[12..14].parse().ok()?;
    Some(
        PrimitiveDateTime::new(
            Date::from_calendar_date(year, month, day).ok()?,
            Time::from_hms(hour, minute, second).ok()?,
        )
        .assume_utc()
        .unix_timestamp(),
    )
}
fn selected_times(
    n1: &[TargetTime],
    n2: &[TargetTime],
) -> Result<(Vec<(TargetTime, bool)>, usize), String> {
    let latest = n1
        .iter()
        .filter(|t| t.basetime == t.validtime && t.elements.iter().any(|e| e == "hrpns"))
        .filter(|t| timestamp(&t.validtime).is_some())
        .max_by_key(|t| &t.validtime)
        .ok_or("no observed radar time")?;
    let base = timestamp(&latest.validtime).ok_or("bad radar time")?;
    let mut times = Vec::new();
    for offset in -3..=0 {
        if let Some(t) = n1.iter().find(|t| {
            t.basetime == t.validtime
                && t.elements.iter().any(|e| e == "hrpns")
                && timestamp(&t.validtime) == Some(base + offset * 3600)
        }) {
            times.push((t.clone(), true));
        }
    }
    let latest_index = times.len() - 1;
    if let Some(t) = n2.iter().find(|t| {
        t.basetime == latest.basetime
            && t.elements.iter().any(|e| e == "hrpns")
            && timestamp(&t.validtime) == Some(base + 3600)
    }) {
        times.push((t.clone(), false));
    }
    Ok((times, latest_index))
}
fn key(t: &TargetTime) -> String {
    format!("{}-{}", t.basetime, t.validtime)
}

fn parse_and_render(source: &[u8], id: &str) -> Result<RadarSet, String> {
    let source: RadarSource = serde_json::from_slice(source).map_err(|e| e.to_string())?;
    let (times, latest_observed) = selected_times(&source.n1, &source.n2)?;
    let mut frames = Vec::with_capacity(times.len());
    for (t, observed) in times {
        let pngs = source.rain.get(&key(&t)).ok_or("missing rain tiles")?;
        let tiles: Vec<_> = pngs
            .iter()
            .map(|s| decode_tile(&BASE64.decode(s).map_err(|e| e.to_string())?))
            .collect::<Result<_, _>>()?;
        let nd = source.nodata.get(&key(&t)).ok_or("missing no-data layer")?;
        let pixels = render_lcd(&t.validtime, observed, &tiles, nd)?;
        frames.push(RadarFrame {
            validtime: t.validtime,
            observed,
            pixels: BASE64.encode(pixels),
        });
    }
    Ok(RadarSet {
        id: id.into(),
        frames,
        latest_observed,
    })
}

fn decode_tile(bytes: &[u8]) -> Result<RgbaImage, String> {
    let im = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    if im.dimensions() != (256, 256) {
        return Err("unexpected tile dimensions".into());
    }
    Ok(im)
}
static MAP: LazyLock<[RgbaImage; 4]> =
    LazyLock::new(|| MAP_PNG.map(|p| decode_tile(p).expect("saved map tile")));

fn merc(lon: f64, lat: f64) -> (f64, f64) {
    let n = 4096.0;
    let radians = lat.clamp(-85.05112878, 85.05112878).to_radians();
    (
        (lon + 180.0) / 360.0 * n,
        (1.0 - (radians.tan().asinh() / std::f64::consts::PI)) / 2.0 * n,
    )
}
fn screen_to_map(x: usize, y: usize, w: usize, mh: usize) -> (f64, f64) {
    let (x0, y0) = merc(125.0, 47.0);
    let (x1, y1) = merc(148.0, 24.0);
    let cw = x1 - x0;
    let ch = y1 - y0;
    let a = 35.0_f64.to_radians();
    let rw = cw * a.cos() + ch * a.sin();
    let target_h = rw * mh as f64 / w as f64;
    let dx = (x as f64 + 0.5) / w as f64 * rw - rw / 2.0;
    let dy = (y as f64 + 0.5) / mh as f64 * target_h - target_h / 2.0;
    (
        x0 + cw / 2.0 + dx * a.cos() + dy * a.sin(),
        y0 + ch / 2.0 - dx * a.sin() + dy * a.cos(),
    )
}
fn sample_tile(tiles: &[RgbaImage], x: f64, y: f64) -> [u8; 4] {
    let tx = (x.floor() as i32).div_euclid(256);
    let ty = (y.floor() as i32).div_euclid(256);
    if !(13..=14).contains(&tx) || !(5..=6).contains(&ty) {
        return [0, 0, 0, 0];
    }
    let i = ((ty - 5) * 2 + (tx - 13)) as usize;
    tiles[i]
        .get_pixel(
            (x.floor() as i32).rem_euclid(256) as u32,
            (y.floor() as i32).rem_euclid(256) as u32,
        )
        .0
}
fn land(p: [u8; 4]) -> bool {
    p[0] > 225 && p[1] > 225 && p[2] > 225
}
fn coverage_rings(geojson: &str) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let data: serde_json::Value = serde_json::from_str(geojson).map_err(|e| e.to_string())?;
    let features = data["features"]
        .as_array()
        .ok_or("missing no-data features")?;
    let mut rings = Vec::new();
    for feature in features {
        if feature["geometry"]["type"] != "Polygon" {
            return Err("unsupported no-data geometry".into());
        }
        let Some(coords) = feature["geometry"]["coordinates"].as_array() else {
            return Err("invalid no-data coordinates".into());
        };
        for hole in coords.iter().skip(1) {
            let points = hole
                .as_array()
                .ok_or("invalid coverage ring")?
                .iter()
                .map(|p| {
                    let lon = p[0].as_f64().ok_or("invalid longitude")?;
                    let lat = p[1].as_f64().ok_or("invalid latitude")?;
                    Ok(merc(lon, lat))
                })
                .collect::<Result<Vec<_>, &str>>()?;
            let mut reduced = Vec::new();
            for point in points {
                if reduced.last().is_none_or(|last: &(f64, f64)| {
                    (last.0 - point.0).abs() + (last.1 - point.1).abs() > 1.5
                }) {
                    reduced.push(point);
                }
            }
            rings.push(reduced);
        }
    }
    Ok(rings)
}

fn in_ring(x: f64, y: f64, ring: &[(f64, f64)]) -> bool {
    let mut inside = false;
    for (a, b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}
fn paint_text(
    pixels: &mut [u16],
    dims: (usize, usize),
    x0: usize,
    y0: usize,
    text: &str,
    size: usize,
    fg: u16,
) {
    let (w, h) = dims;
    let mut x = x0;
    for ch in text.chars() {
        if let Some(i) = CHARS.chars().position(|c| c == ch) {
            for yy in 0..size {
                for xx in 0..size {
                    if GLYPHS[i * 256 + (yy * 16 / size) * 16 + xx * 16 / size] > 96
                        && x + xx < w
                        && y0 + yy < h
                    {
                        pixels[(y0 + yy) * w + x + xx] = fg;
                    }
                }
            }
        }
        x += if ch == ' ' {
            size / 3
        } else if ch.is_ascii_digit() {
            size * 3 / 5
        } else if ch == '/' || ch == ':' {
            size / 2
        } else {
            size
        };
        if x >= w {
            break;
        }
    }
}
fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}
fn label(validtime: &str, observed: bool) -> Result<String, String> {
    let ts = timestamp(validtime).ok_or("bad frame time")?;
    let jst = OffsetDateTime::from_unix_timestamp(ts + 9 * 3600).map_err(|e| e.to_string())?;
    Ok(format!(
        "{:02}/{:02} {:02}:{:02} {}",
        u8::from(jst.month()),
        jst.day(),
        jst.hour(),
        jst.minute(),
        if observed { "観測" } else { "予測" }
    ))
}
fn render_lcd(
    validtime: &str,
    observed: bool,
    tiles: &[RgbaImage],
    geojson: &str,
) -> Result<Vec<u8>, String> {
    if tiles.len() != 4 {
        return Err("four rain tiles required".into());
    }
    let rings = coverage_rings(geojson)?;
    let w = 160;
    let h = 80;
    let header = 19;
    let mut pixels = vec![rgb565(18, 60, 88); w * h];
    for y in 0..h - header {
        for x in 0..w {
            let (mx, my) = screen_to_map(x, y, w, h - header);
            let base = sample_tile(&MAP[..], mx, my);
            let wet = sample_tile(tiles, mx, my);
            let covered = rings.iter().any(|ring| in_ring(mx, my, ring));
            let coast = land(base) != land(sample_tile(&MAP[..], mx + 2.0, my))
                || land(base) != land(sample_tile(&MAP[..], mx - 2.0, my))
                || land(base) != land(sample_tile(&MAP[..], mx, my + 2.0))
                || land(base) != land(sample_tile(&MAP[..], mx, my - 2.0));
            let c = if coast {
                rgb565(24, 40, 55)
            } else if !covered {
                if (x + y) % 7 == 0 {
                    rgb565(96, 111, 125)
                } else {
                    rgb565(222, 225, 225)
                }
            } else if wet[3] > 45 {
                rgb565(wet[0], wet[1], wet[2])
            } else if land(base) {
                rgb565(248, 246, 234)
            } else {
                rgb565(189, 218, 243)
            };
            pixels[(y + header) * w + x] = c;
        }
    }
    paint_lcd_labels(&mut pixels, validtime, observed)?;
    Ok(pixels.into_iter().flat_map(u16::to_be_bytes).collect())
}

/// Reapply current text to cached pixels too; a renderer update must not wait
/// for another weather-data fetch to fix an already saved radar caption.
fn lcd_frame(frame: &RadarFrame) -> Result<Vec<u8>, String> {
    let bytes = BASE64.decode(&frame.pixels).map_err(|e| e.to_string())?;
    if bytes.len() != 160 * 80 * 2 {
        return Err("invalid cached radar frame length".into());
    }
    let mut pixels: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_be_bytes(*b))
        .collect();
    paint_lcd_labels(&mut pixels, &frame.validtime, frame.observed)?;
    Ok(pixels.into_iter().flat_map(u16::to_be_bytes).collect())
}

fn paint_lcd_labels(pixels: &mut [u16], validtime: &str, observed: bool) -> Result<(), String> {
    let w = 160;
    let h = 80;
    pixels[..w * 19].fill(if observed {
        rgb565(18, 60, 88)
    } else {
        rgb565(109, 61, 26)
    });
    crate::lcd_text::draw(
        pixels,
        (w, h),
        (3, 0),
        &label(validtime, observed)?,
        16,
        rgb565(255, 255, 255),
    );
    let credit = "気象庁/国土地理院";
    let credit_x = w - crate::lcd_text::width(credit, 12) - 3;
    for y in 64..80 {
        for x in credit_x - 2..160 {
            pixels[y * w + x] = rgb565(241, 247, 250);
        }
    }
    crate::lcd_text::draw(
        pixels,
        (w, h),
        (credit_x as i32, 65),
        credit,
        12,
        rgb565(20, 45, 65),
    );
    Ok(())
}
fn error_lcd() -> Vec<u8> {
    let mut pixels = vec![rgb565(237, 243, 247); 160 * 80];
    for p in pixels.iter_mut().take(160 * 19) {
        *p = rgb565(132, 39, 39);
    }
    crate::lcd_text::draw(
        &mut pixels,
        (160, 80),
        (5, 0),
        "雨雲",
        16,
        rgb565(255, 255, 255),
    );
    crate::lcd_text::draw(
        &mut pixels,
        (160, 80),
        (40, 36),
        "取得失敗",
        20,
        rgb565(132, 39, 39),
    );
    pixels.into_iter().flat_map(u16::to_be_bytes).collect()
}

/// Packed MSB-first, white=1 monochrome preview for the future 250×122
/// E-ink path. Coast and precipitation are independent black strokes.
pub fn render_eink_preview(source: &[u8], validtime: &str) -> Result<Vec<u8>, String> {
    let source: RadarSource = serde_json::from_slice(source).map_err(|e| e.to_string())?;
    let (times, _) = selected_times(&source.n1, &source.n2)?;
    let (time, observed) = times
        .iter()
        .find(|(t, _)| t.validtime == validtime)
        .ok_or("time absent")?;
    let pngs = source.rain.get(&key(time)).ok_or("missing rain tiles")?;
    let tiles: Vec<_> = pngs
        .iter()
        .map(|s| decode_tile(&BASE64.decode(s).map_err(|e| e.to_string())?))
        .collect::<Result<_, _>>()?;
    let rings = coverage_rings(
        source
            .nodata
            .get(&key(time))
            .ok_or("missing no-data layer")?,
    )?;
    let w = 250;
    let h = 122;
    let header = 25;
    let mut pixels = vec![u16::MAX; w * h];
    for x in 0..w {
        pixels[(header - 1) * w + x] = 0;
    }
    for y in 0..h - header {
        for x in 0..w {
            let (mx, my) = screen_to_map(x, y, w, h - header);
            let base = sample_tile(&MAP[..], mx, my);
            let wet = sample_tile(&tiles, mx, my)[3] > 45;
            let near_wet = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
                .iter()
                .any(|(dx, dy)| sample_tile(&tiles, mx + dx, my + dy)[3] > 45);
            let coast = land(base) != land(sample_tile(&MAP[..], mx + 1.5, my))
                || land(base) != land(sample_tile(&MAP[..], mx - 1.5, my))
                || land(base) != land(sample_tile(&MAP[..], mx, my + 1.5))
                || land(base) != land(sample_tile(&MAP[..], mx, my - 1.5));
            let covered = rings.iter().any(|ring| in_ring(mx, my, ring));
            if coast || wet || (!covered && (x + y) % 8 == 0 && !near_wet) {
                pixels[(y + header) * w + x] = 0;
            }
        }
    }
    paint_text(
        &mut pixels,
        (w, h),
        4,
        2,
        &label(validtime, *observed)?,
        16,
        0,
    );
    paint_text(&mut pixels, (w, h), 130, 112, "気象庁/国土地理院", 8, 0);
    let mut packed = vec![0u8; (w * h).div_ceil(8)];
    for (i, pixel) in pixels.iter().enumerate() {
        if *pixel != 0 {
            packed[i / 8] |= 1 << (7 - i % 8);
        }
    }
    Ok(packed)
}

pub struct JmaRadarFetcher;
const MAX_COMPRESSED_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_EXPANDED_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn decode_http_body(bytes: &[u8], encoding: Option<&str>) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_COMPRESSED_RESPONSE_BYTES {
        return Err("radar response too large".into());
    }
    match encoding.map(str::trim) {
        None | Some("") | Some("identity") => Ok(bytes.to_vec()),
        Some("gzip") => {
            use std::io::Read;
            let mut expanded = Vec::new();
            flate2::read::GzDecoder::new(bytes)
                .take((MAX_EXPANDED_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut expanded)
                .map_err(|_| "invalid radar gzip response")?;
            if expanded.len() > MAX_EXPANDED_RESPONSE_BYTES {
                return Err("expanded radar response too large".into());
            }
            Ok(expanded)
        }
        Some(_) => Err("unsupported radar content encoding".into()),
    }
}

impl JmaRadarFetcher {
    pub async fn fetch_async(job: &FetchJob) -> Result<Vec<u8>, String> {
        if job.plugin != ID || job.source_key != "nowc/hrpns" {
            return Err("unsupported source".into());
        }
        let duration = std::time::Duration::from_secs(job.timeout_seconds.into());
        tokio::time::timeout(duration, Self::fetch_set())
            .await
            .map_err(|_| "radar set timeout".to_owned())?
    }
    async fn get(client: &reqwest::Client, url: String) -> Result<Vec<u8>, String> {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let encoding = response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| "invalid radar content encoding")?;
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        decode_http_body(&bytes, encoding.as_deref())
    }
    async fn fetch_set() -> Result<Vec<u8>, String> {
        let client = reqwest::Client::new();
        let (n1, n2) = tokio::try_join!(
            Self::get(&client, N1_URL.into()),
            Self::get(&client, N2_URL.into())
        )?;
        let n1: Vec<TargetTime> = serde_json::from_slice(&n1).map_err(|e| e.to_string())?;
        let n2: Vec<TargetTime> = serde_json::from_slice(&n2).map_err(|e| e.to_string())?;
        let (times, _) = selected_times(&n1, &n2)?;
        let mut requests = Vec::new();
        for (t, _) in &times {
            let k = key(t);
            for (x, y) in [(13, 5), (14, 5), (13, 6), (14, 6)] {
                requests.push((
                    k.clone(),
                    Some(((y - 5) * 2 + x - 13) as usize),
                    format!(
                        "{ROOT}/{}/none/{}/surf/hrpns/4/{x}/{y}.png",
                        t.basetime, t.validtime
                    ),
                ));
            }
            requests.push((
                k,
                None,
                format!(
                    "{ROOT}/{}/none/{}/surf/hrpns_nd/data.geojson",
                    t.basetime, t.validtime
                ),
            ));
        }
        let replies = futures_util::future::join_all(
            requests
                .iter()
                .map(|(_, _, url)| Self::get(&client, url.clone())),
        )
        .await;
        let mut rain: BTreeMap<String, [String; 4]> = BTreeMap::new();
        let mut nodata = BTreeMap::new();
        for ((k, index, _), reply) in requests.into_iter().zip(replies) {
            let bytes = reply?;
            if let Some(i) = index {
                decode_tile(&bytes)?;
                rain.entry(k).or_default()[i] = BASE64.encode(bytes);
            } else {
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                coverage_rings(&text)?;
                nodata.insert(k, text);
            }
        }
        serde_json::to_vec(&RadarSource {
            n1,
            n2,
            rain,
            nodata,
        })
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAVED: &[u8] = include_bytes!("../assets/jma-radar-20261012.json");
    #[test]
    fn gzip_http_body_expands_with_a_size_limit() {
        use std::io::Write;
        let source = br#"{"features":[]}"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(source).unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(decode_http_body(&compressed, Some("gzip")).unwrap(), source);
        assert!(decode_http_body(&compressed, None).is_ok());
        assert!(decode_http_body(b"not gzip", Some("gzip")).is_err());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder
            .write_all(&vec![b'x'; MAX_EXPANDED_RESPONSE_BYTES + 1])
            .unwrap();
        assert_eq!(
            decode_http_body(&encoder.finish().unwrap(), Some("gzip")),
            Err("expanded radar response too large".into())
        );
    }

    #[tokio::test]
    async fn jma_get_expands_gzip_http_response() {
        use std::io::Write;
        let source = br#"{"features":[]}"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(source).unwrap();
        let compressed = encoder.finish().unwrap();
        let router = axum::Router::new().route(
            "/",
            axum::routing::get(move || {
                let body = compressed.clone();
                async move { ([(axum::http::header::CONTENT_ENCODING, "gzip")], body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let result = JmaRadarFetcher::get(&reqwest::Client::new(), format!("http://{address}/"))
            .await
            .unwrap();
        assert_eq!(result, source);
        server.abort();
        let _ = server.await;
    }
    #[test]
    fn native_lcd_header_and_credit_fit_without_clipping() {
        for observed in [false, true] {
            crate::lcd_text::assert_text_fits(
                &label("20261012214500", observed).unwrap(),
                16,
                (3, 0),
                (160, 19),
            );
        }
        let credit = "気象庁/国土地理院";
        let x = 160 - crate::lcd_text::width(credit, 12) - 3;
        crate::lcd_text::assert_text_fits(credit, 12, (x, 65), (160, 80));
    }

    #[test]
    fn saved_radar_captions_use_current_glyphs_without_fetching_again() {
        let ctx = PluginContext {
            token: crate::SessionToken {
                device_id: "cached".into(),
                incarnation: 1,
                session_epoch: 1,
            },
            settings_revision: 1,
            mode: Mode::Normal,
        };
        let mut content = RadarContent::default();
        content.publish(&ctx, SAVED, "saved", "2026-10-12").unwrap();
        let expected = content.current_frame(&ctx).unwrap();
        let mut state = content.save_state();
        for frame in state["cached"]["latest"]["frames"].as_array_mut().unwrap() {
            let mut bytes = BASE64.decode(frame["pixels"].as_str().unwrap()).unwrap();
            bytes[..160 * 19 * 2].fill(0);
            for y in 70..80 {
                for x in 58..160 {
                    bytes[(y * 160 + x) * 2..(y * 160 + x) * 2 + 2].fill(0);
                }
            }
            frame["pixels"] = serde_json::json!(BASE64.encode(bytes));
        }
        let mut restored = RadarContent::default();
        restored.load_state(state).unwrap();
        assert_eq!(restored.current_frame(&ctx).unwrap(), expected);
        assert_eq!(
            restored
                .plan(&ctx, ButtonAction::Sub)
                .unwrap()
                .frame
                .unwrap(),
            expected
        );
    }

    #[test]
    fn saved_jma_set_renders_actual_hourly_times_and_distinct_lcd_frames() {
        let set = parse_and_render(SAVED, "saved").unwrap();
        assert_eq!(set.latest_observed, 3);
        assert_eq!(
            set.frames
                .iter()
                .map(|f| f.validtime.as_str())
                .collect::<Vec<_>>(),
            [
                "20261012002000",
                "20261012012000",
                "20261012022000",
                "20261012032000",
                "20261012042000"
            ]
        );
        assert_eq!(
            set.frames.iter().map(|f| f.observed).collect::<Vec<_>>(),
            [true, true, true, true, false]
        );
        for frame in &set.frames {
            assert_eq!(BASE64.decode(&frame.pixels).unwrap().len(), 160 * 80 * 2);
        }
        assert_ne!(set.frames[3].pixels, set.frames[4].pixels);
        assert!(set.frames[3].pixels != set.frames[0].pixels);
        let eink = render_eink_preview(SAVED, "20261012032000").unwrap();
        assert_eq!(eink.len(), (250usize * 122).div_ceil(8));
        assert!(eink.iter().any(|v| *v != 0xff));
        assert_ne!(eink, render_eink_preview(SAVED, "20261012042000").unwrap());
        if std::env::var_os("RADAR_PREVIEW").is_some() {
            let pixels = BASE64.decode(&set.frames[3].pixels).unwrap();
            let mut image = image::RgbImage::new(160, 80);
            for (i, chunk) in pixels.as_chunks::<2>().0.iter().enumerate() {
                let c = u16::from_be_bytes([chunk[0], chunk[1]]);
                image.put_pixel(
                    (i % 160) as u32,
                    (i / 160) as u32,
                    image::Rgb([
                        ((c >> 11) * 255 / 31) as u8,
                        (((c >> 5) & 63) * 255 / 63) as u8,
                        ((c & 31) * 255 / 31) as u8,
                    ]),
                );
            }
            image.save("/tmp/picomado-radar-preview.png").unwrap();
            let mut monochrome = image::GrayImage::new(250, 122);
            for i in 0..250 * 122 {
                let white = (eink[i / 8] >> (7 - i % 8)) & 1 != 0;
                monochrome.put_pixel(
                    (i % 250) as u32,
                    (i / 250) as u32,
                    image::Luma([if white { 255 } else { 0 }]),
                );
            }
            monochrome
                .save("/tmp/picomado-radar-eink-preview.png")
                .unwrap();
        }
    }

    #[test]
    fn absent_hour_is_not_fabricated_and_generation_cancel_rejects_old_result() {
        let mut source: RadarSource = serde_json::from_slice(SAVED).unwrap();
        source.n2.clear();
        let bytes = serde_json::to_vec(&source).unwrap();
        let set = parse_and_render(&bytes, "observed-only").unwrap();
        assert_eq!(set.frames.len(), 4);
        let mut browse = RadarBrowse::new(set);
        browse.begin();
        assert!(!browse.move_hour(1));
        assert_eq!(browse.index, 3);
        let temp = tempfile::tempdir().unwrap();
        let mut app = crate::application::App::open(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        for _ in 0..3 {
            app.button(&token, ButtonAction::Down).unwrap();
        }
        app.publish_plugin_source(ID, &token, 1, SAVED, "set-one", "2026-10-12")
            .unwrap();
        app.button(&token, ButtonAction::Sub).unwrap();
        app.button(&token, ButtonAction::Up).unwrap();
        let pinned = app.plugin_position(&token).unwrap();
        let old = app
            .plan_plugin_fetches(100, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == ID)
            .unwrap();
        app.cancel_plugin_fetches().unwrap();
        assert!(!app
            .complete_plugin_fetch(&old, 101, Ok(SAVED.to_vec()))
            .unwrap());
        let fresh = app
            .plan_plugin_fetches(101, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == ID)
            .unwrap();
        assert!(fresh.generation > old.generation);
        app.complete_plugin_fetch(&fresh, 102, Ok(SAVED.to_vec()))
            .unwrap();
        assert_eq!(app.plugin_position(&token).unwrap(), pinned);
        app.button(&token, ButtonAction::Power).unwrap();
        let normal = app.plugin_position(&token).unwrap();
        assert_eq!(normal.dataset_id, fresh.dataset_id);
        assert_eq!(
            (normal.target_date.as_str(), normal.index),
            ("20261012032000", 3)
        );
    }
}
