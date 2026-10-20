//! Japan-time clock. Calendar/hand geometry and pixels are plugin-owned;
//! the host supplies time, visibility, delivery tokens and minute scheduling.
use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub struct ClockPlugin;
impl Plugin for ClockPlugin {
    fn id(&self) -> &str {
        "clock"
    }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool {
        caps.contains("lcd") || caps.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        _: &'a dyn WorldQuery,
        _: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub date: String,
    pub digital: String,
    pub hour: u8,
    pub minute: u8,
}
pub fn reading(now_unix: i64) -> Option<Reading> {
    let time = time::OffsetDateTime::from_unix_timestamp(now_unix)
        .ok()?
        .to_offset(time::UtcOffset::from_hms(9, 0, 0).ok()?);
    let weekday = ["月", "火", "水", "木", "金", "土", "日"]
        [time.weekday().number_days_from_monday() as usize];
    Some(Reading {
        date: format!(
            "{:04}/{:02}/{:02}（{}）",
            time.year(),
            u8::from(time.month()),
            time.day(),
            weekday
        ),
        digital: format!("{:02}:{:02}", time.hour(), time.minute()),
        hour: time.hour(),
        minute: time.minute(),
    })
}
fn hand_tip(center: (i32, i32), length: f64, turns: f64) -> (i32, i32) {
    let angle = turns * std::f64::consts::TAU;
    (
        center.0 + (angle.sin() * length).round() as i32,
        center.1 - (angle.cos() * length).round() as i32,
    )
}
fn line(
    pixels: &mut [u16],
    dims: (usize, usize),
    from: (i32, i32),
    to: (i32, i32),
    color: u16,
    bold: bool,
) {
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let dy = -(to.1 - y).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let sy = if y < to.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        for ox in 0..=i32::from(bold) {
            for oy in 0..=i32::from(bold) {
                if x + ox >= 0 && y + oy >= 0 && x + ox < dims.0 as i32 && y + oy < dims.1 as i32 {
                    pixels[(y + oy) as usize * dims.0 + (x + ox) as usize] = color;
                }
            }
        }
        if (x, y) == to {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}
fn pixels(now_unix: i64, eink: bool) -> Vec<u16> {
    let (w, h, header, size, center, radius) = if eink {
        (250, 122, 28, 20, (58, 77), 40)
    } else {
        (160, 80, 20, 14, (36, 51), 26)
    };
    let bg = if eink { 0xffff } else { 0x0841 };
    let fg = if eink { 0 } else { 0xffff };
    let accent = if eink { 0 } else { 0x07ff };
    let mut p = vec![bg; w * h];
    let Some(r) = reading(now_unix) else { return p };
    let date_width = crate::lcd_text::width(&r.date, size);
    crate::lcd_text::draw(
        &mut p,
        (w, h),
        (((w - date_width) / 2) as i32, 0),
        &r.date,
        size,
        fg,
    );
    line(
        &mut p,
        (w, h),
        (4, header),
        (w as i32 - 5, header),
        accent,
        false,
    );
    for step in 0..60 {
        let a = hand_tip(center, f64::from(radius), f64::from(step) / 60.);
        let b = hand_tip(
            center,
            f64::from(radius - if step % 5 == 0 { 4 } else { 1 }),
            f64::from(step) / 60.,
        );
        line(&mut p, (w, h), a, b, fg, false);
    }
    let minute_tip = hand_tip(center, f64::from(radius - 6), f64::from(r.minute) / 60.);
    let hour_tip = hand_tip(
        center,
        f64::from(radius) * 0.54,
        (f64::from(r.hour % 12) + f64::from(r.minute) / 60.) / 12.,
    );
    line(&mut p, (w, h), center, hour_tip, fg, true);
    line(&mut p, (w, h), center, minute_tip, accent, false);
    let digit_size = 22;
    let scale = if eink { 2 } else { 1 };
    let digital_width = crate::lcd_text::width(&r.digital, digit_size);
    let left = if eink { 110 } else { 70 };
    let mut digits = vec![bg; digital_width * digit_size];
    crate::lcd_text::draw(
        &mut digits,
        (digital_width, digit_size),
        (0, 0),
        &r.digital,
        digit_size,
        fg,
    );
    let x = left + (w - left - digital_width * scale) / 2;
    let y = center.1 as usize - digit_size * scale / 2;
    for row in 0..digit_size * scale {
        for col in 0..digital_width * scale {
            p[(y + row) * w + x + col] = digits[(row / scale) * digital_width + col / scale];
        }
    }
    p
}
pub fn render_lcd(now_unix: i64) -> Vec<u8> {
    pixels(now_unix, false)
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect()
}
pub fn render_eink(now_unix: i64) -> Vec<u8> {
    let p = pixels(now_unix, true);
    let mut bytes = vec![0xff; crate::eink::FRAME_BYTES];
    for (i, c) in p.iter().enumerate() {
        if *c == 0 {
            bytes[i / 8] &= !(0x80 >> (i % 8));
        }
    }
    bytes
}

fn default_interval() -> u16 {
    10
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    incarnation: u64,
    revision: u64,
    #[serde(default = "default_interval")]
    full_refresh_minutes: u16,
}
#[derive(Clone, Default)]
pub struct ClockContent {
    configs: BTreeMap<String, Config>,
    times: BTreeMap<String, (u64, i64)>,
}
impl ContentPlugin for ClockContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "clock"
    }
    fn transient_view(&self,ctx:&PluginContext)->Option<crate::content::TransientView>{Some(Box::new(self.time(ctx)?))}
    fn merge_transient_view(&mut self,ctx:&PluginContext,view:&dyn std::any::Any)->Result<(),ContentError>{
        let now=view.downcast_ref::<i64>().ok_or(ContentError::Stale)?;
        self.times.insert(ctx.token.device_id.clone(),(ctx.token.incarnation,*now));Ok(())
    }
    fn publish(
        &mut self,
        _: &PluginContext,
        _: &[u8],
        _: &str,
        _: &str,
    ) -> Result<(), ContentError> {
        Err(ContentError::Unsupported)
    }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> {
        None
    }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn eink_refresh(&self, ctx: &PluginContext) -> Option<crate::protocol::EinkRefresh> {
        Some(crate::protocol::EinkRefresh {
            full_refresh_minutes: self.device_config(&ctx.token.device_id, ctx.token.incarnation)
                ["full_refresh_minutes"]
                .as_u64()? as u16,
            minute: u64::try_from(self.time(ctx)?.div_euclid(60)).ok()?,
        })
    }
    fn time_key(&self, _: &PluginContext, now: i64) -> Option<i64> {
        reading(now).map(|_| now.div_euclid(60))
    }
    fn observe_time(&mut self, ctx: &PluginContext, now: i64) {
        self.times
            .insert(ctx.token.device_id.clone(), (ctx.token.incarnation, now));
    }
    fn current_frame(&self, ctx: &PluginContext) -> Option<Vec<u8>> {
        self.time(ctx).map(render_lcd)
    }
    fn eink_frame_for(&self, ctx: &PluginContext, _: &[u8]) -> Option<Vec<u8>> {
        self.time(ctx).map(render_eink)
    }
    fn session_reset(&mut self, id: &str, _: u64) {
        self.times.remove(id);
    }
    fn delete_device(&mut self, id: &str) {
        self.times.remove(id);
        self.configs.remove(id);
    }
    fn save_state(&self) -> Value {
        serde_json::to_value(&self.configs).expect("clock config")
    }
    fn load_state(&mut self, value: Value) -> Result<(), ContentError> {
        self.configs = serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        if self.configs.values().any(|c| {
            !(1..=60).contains(&c.full_refresh_minutes) || c.revision == 0 || c.incarnation == 0
        }) {
            return Err(ContentError::InvalidSource);
        }
        Ok(())
    }
    fn device_config(&self, id: &str, incarnation: u64) -> Value {
        let c = self
            .configs
            .get(id)
            .filter(|c| c.incarnation == incarnation);
        json!({"revision":c.map_or(1,|c|c.revision),"full_refresh_minutes":c.map_or(10,|c|c.full_refresh_minutes)})
    }
    fn set_device_config(
        &mut self,
        ctx: &PluginContext,
        expected: u64,
        value: Value,
    ) -> Result<u64, ContentError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            full_refresh_minutes: u16,
        }
        let input: Input =
            serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        if !(1..=60).contains(&input.full_refresh_minutes) {
            return Err(ContentError::InvalidSource);
        }
        let current = self.device_config(&ctx.token.device_id, ctx.token.incarnation);
        if current["revision"] != expected {
            return Err(ContentError::Stale);
        }
        if current["full_refresh_minutes"] == input.full_refresh_minutes {
            return Ok(expected);
        }
        let revision = expected.checked_add(1).ok_or(ContentError::InvalidSource)?;
        self.configs.insert(
            ctx.token.device_id.clone(),
            Config {
                incarnation: ctx.token.incarnation,
                revision,
                full_refresh_minutes: input.full_refresh_minutes,
            },
        );
        Ok(revision)
    }
    fn web_state(&self) -> Value {
        json!({"timezone":"Asia/Tokyo","format":"HH:MM","update_seconds":60,"eink_partial_ready":true})
    }
}
impl ClockContent {
    fn time(&self, ctx: &PluginContext) -> Option<i64> {
        self.times
            .get(&ctx.token.device_id)
            .filter(|(incarnation, _)| *incarnation == ctx.token.incarnation)
            .map(|(_, now)| *now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unix(y: i32, m: u8, d: u8, h: u8, min: u8) -> i64 {
        time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d)
            .unwrap()
            .with_hms(h, min, 0)
            .unwrap()
            .assume_offset(time::UtcOffset::from_hms(9, 0, 0).unwrap())
            .unix_timestamp()
    }
    #[test]
    fn jst_calendar_weekdays_midnight_leap_day_and_24_hour_digits() {
        let before = unix(2026, 12, 31, 23, 59);
        assert_eq!(reading(before).unwrap().date, "2026/12/31（木）");
        assert_eq!(reading(before).unwrap().digital, "23:59");
        assert_eq!(reading(before + 60).unwrap().date, "2027/01/01（金）");
        assert_eq!(reading(before + 60).unwrap().digital, "00:00");
        for (day, label) in [
            (19, "月"),
            (20, "火"),
            (21, "水"),
            (22, "木"),
            (23, "金"),
            (24, "土"),
            (25, "日"),
        ] {
            assert_eq!(
                reading(unix(2026, 10, day, 12, 0)).unwrap().date,
                format!("2026/10/{day}（{label}）")
            );
        }
        assert_eq!(
            reading(unix(2028, 2, 28, 23, 59) + 60).unwrap().date,
            "2028/02/29（火）"
        );
        for hour in 0..24 {
            assert_eq!(
                reading(unix(2026, 10, 18, hour, 5)).unwrap().digital,
                format!("{hour:02}:05")
            );
        }
    }
    #[test]
    fn hands_include_minutes_and_no_seconds() {
        assert_eq!(hand_tip((0, 0), 100., 0.), (0, -100));
        assert_eq!(hand_tip((0, 0), 100., 0.25), (100, 0));
        let half_past_three = hand_tip((0, 0), 100., 3.5 / 12.);
        assert!(half_past_three.0 > 90 && half_past_three.1 > 20);
        let now = unix(2026, 10, 18, 3, 30);
        assert_eq!(render_lcd(now), render_lcd(now + 59));
        assert_eq!(render_eink(now), render_eink(now + 59));
        assert_ne!(render_lcd(now), render_lcd(now + 60));
    }
    #[test]
    fn native_dimensions_palette_padding_and_text_bounds() {
        let now = unix(2026, 10, 18, 23, 59);
        assert_eq!(render_lcd(now).len(), 25600);
        assert_eq!(render_eink(now).len(), 3813);
        assert_eq!(render_eink(now).last().unwrap() & 15, 15);
        assert!(render_lcd(now).as_chunks::<2>().0.contains(&[0x07, 0xff]));
        let date = reading(now).unwrap().date;
        for (width, height, size) in [(160, 80, 14), (250, 122, 20)] {
            crate::lcd_text::assert_text_fits(
                &date,
                size,
                ((width - crate::lcd_text::width(&date, size)) / 2, 0),
                (width, height),
            );
        }
    }
}
