//! Cached JMA forecast interpreter and the approved LCD composition.
//! No HTTP or clock reads occur here. Callers supply one immutable source set
//! and its JST 'today'; a browser pins that exact set during day navigation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use time::{Date, Month};

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, FetchJob, PluginContext, PluginPlan, PluginPosition,
};
use crate::{Candidate, Capability, Event, Mode, Plugin, Priority, Transition, WorldQuery};

#[cfg(test)]
thread_local! {static FULL_RENDER_CALLS:std::cell::Cell<(usize,usize)>=const{std::cell::Cell::new((0,0))};}
#[cfg(test)]
pub(crate) fn take_full_render_calls() -> (usize, usize) {
    FULL_RENDER_CALLS.with(|v| v.replace((0, 0)))
}

#[derive(Clone, Default)]
pub struct WeatherContent {
    devices: BTreeMap<String, WeatherSlot>,
}
#[derive(Clone)]
struct WeatherSlot {
    incarnation: u64,
    settings_revision: u64,
    config_revision: u64,
    location_id: String,
    today_jst: Option<String>,
    browse: Option<WeatherBrowse>,
    rendered: Vec<RenderedForecast>,
    error_pixels: Arc<WeatherPixels>,
    last_success_unix: Option<i64>,
    last_attempt_unix: Option<i64>,
    error: bool,
    generation: u64,
    in_flight: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct StoredWeatherSlot {
    incarnation: u64,
    settings_revision: u64,
    config_revision: u64,
    location_id: String,
    #[serde(default)]
    today_jst: Option<String>,
    latest: Option<ForecastSet>,
    last_success_unix: Option<i64>,
    last_attempt_unix: Option<i64>,
    error: bool,
    generation: u64,
    #[serde(default)]
    had_in_flight: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocationChoice {
    location_id: String,
}
struct WeatherPixels {
    lcd: Vec<u8>,
    eink: Vec<u8>,
}
#[derive(Clone)]
struct RenderedForecast {
    set: Arc<ForecastSet>,
    frames: Arc<Vec<WeatherPixels>>,
}
impl WeatherPixels {
    fn error(choice: &WeatherLocation) -> Self {
        let lcd = render_error_lcd(choice);
        let eink = crate::eink::from_lcd(&lcd);
        Self { lcd, eink }
    }
}
fn render_set(set: &ForecastSet) -> Result<Vec<WeatherPixels>, ContentError> {
    set.days
        .iter()
        .map(|day| {
            Ok(WeatherPixels {
                lcd: render_lcd(set, day).map_err(|_| ContentError::InvalidSource)?,
                eink: render_eink(set, day).map_err(|_| ContentError::InvalidSource)?,
            })
        })
        .collect()
}
impl WeatherSlot {
    fn new(incarnation: u64, settings_revision: u64) -> Self {
        Self {
            incarnation,
            settings_revision,
            config_revision: 1,
            location_id: "tokyo-station".into(),
            today_jst: None,
            browse: None,
            rendered: Vec::new(),
            error_pixels: Arc::new(WeatherPixels::error(
                location("tokyo-station").expect("default location"),
            )),
            last_success_unix: None,
            last_attempt_unix: None,
            error: false,
            generation: 0,
            in_flight: None,
        }
    }
    fn prune_rendered(&mut self) {
        let latest = self.browse.as_ref().map(|b| Arc::as_ptr(&b.latest));
        let pinned = self
            .browse
            .as_ref()
            .and_then(|b| b.pinned.as_ref())
            .map(Arc::as_ptr);
        self.rendered
            .retain(|r| Some(Arc::as_ptr(&r.set)) == latest || Some(Arc::as_ptr(&r.set)) == pinned);
    }
    fn install_rendered(&mut self, frames: Vec<WeatherPixels>) {
        if let Some(b) = &self.browse {
            self.rendered.push(RenderedForecast {
                set: b.latest.clone(),
                frames: Arc::new(frames),
            });
        }
        self.prune_rendered();
    }
    fn current_pixels(&self) -> Option<&WeatherPixels> {
        let browse = self.browse.as_ref()?;
        let set = browse.pinned.as_deref().unwrap_or(&browse.latest);
        self.rendered
            .iter()
            .find(|r| std::ptr::eq(r.set.as_ref(), set))?
            .frames
            .get(browse.index)
    }
    fn stored(&self) -> StoredWeatherSlot {
        StoredWeatherSlot {
            incarnation: self.incarnation,
            settings_revision: self.settings_revision,
            config_revision: self.config_revision,
            location_id: self.location_id.clone(),
            today_jst: self.today_jst.clone(),
            latest: self.browse.as_ref().map(|b| (*b.latest).clone()),
            last_success_unix: self.last_success_unix,
            last_attempt_unix: self.last_attempt_unix,
            error: self.error,
            generation: self.generation,
            had_in_flight: self.in_flight.is_some(),
        }
    }
}
impl WeatherContent {
    fn slot(&self, ctx: &PluginContext) -> Option<&WeatherSlot> {
        self.devices
            .get(&ctx.token.device_id)
            .filter(|s| s.incarnation == ctx.token.incarnation)
    }
    fn slot_mut(&mut self, ctx: &PluginContext) -> &mut WeatherSlot {
        let slot = self
            .devices
            .entry(ctx.token.device_id.clone())
            .or_insert_with(|| WeatherSlot::new(ctx.token.incarnation, ctx.settings_revision));
        if slot.incarnation != ctx.token.incarnation {
            *slot = WeatherSlot::new(ctx.token.incarnation, ctx.settings_revision);
        }
        if slot.settings_revision != ctx.settings_revision {
            slot.settings_revision = ctx.settings_revision;
            slot.in_flight = None;
            slot.last_attempt_unix = None;
        }
        slot
    }
}
struct PreparedWeather {
    set: Option<ForecastSet>,
    pixels: Vec<u8>,
    frames: Vec<WeatherPixels>,
    error_pixels: Arc<WeatherPixels>,
}
struct WeatherView {
    pinned: Option<Arc<ForecastSet>>,
    index: usize,
    start_index: usize,
    rendered: Vec<RenderedForecast>,
}
impl ContentPlugin for WeatherContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "weather"
    }
    fn transient_input_ready(&self, ctx:&PluginContext, today:&str)->bool {
        self.slot(ctx).is_some_and(|s|s.browse.is_some() && s.today_jst.as_deref()==Some(today))
    }
    fn transient_view(&self,ctx:&PluginContext)->Option<crate::content::TransientView>{
        let slot=self.slot(ctx)?;let b=slot.browse.as_ref()?;
        Some(Box::new(WeatherView{pinned:b.pinned.clone(),index:b.index,start_index:b.start_index,rendered:slot.rendered.clone()}))
    }
    fn merge_transient_view(&mut self,ctx:&PluginContext,view:&dyn std::any::Any)->Result<(),ContentError>{
        let view=view.downcast_ref::<WeatherView>().ok_or(ContentError::Stale)?;
        let slot=self.devices.get_mut(&ctx.token.device_id).ok_or(ContentError::Stale)?;
        let b=slot.browse.as_mut().ok_or(ContentError::Stale)?;
        if let Some(pin)=&view.pinned {
            b.pinned=Some(pin.clone());b.index=view.index;b.start_index=view.start_index;
            if !slot.rendered.iter().any(|r|Arc::ptr_eq(&r.set,pin)) {
                let pixels=view.rendered.iter().find(|r|Arc::ptr_eq(&r.set,pin)).ok_or(ContentError::Stale)?;
                slot.rendered.push(pixels.clone());
            }
        } else { b.cancel(); }
        slot.prune_rendered();Ok(())
    }
    fn publish(
        &mut self,
        ctx: &PluginContext,
        source: &[u8],
        dataset_id: &str,
        today_jst: &str,
    ) -> Result<(), ContentError> {
        let slot = self.slot_mut(ctx);
        slot.today_jst = Some(today_jst.into());
        let choice = location(&slot.location_id).ok_or(ContentError::InvalidSource)?;
        let set = ForecastSet::for_location(source, dataset_id, today_jst, choice)
            .map_err(|_| ContentError::InvalidSource)?;
        let frames = render_set(&set)?;
        match &mut slot.browse {
            Some(b) => b.publish(set),
            None => slot.browse = Some(WeatherBrowse::new(set)),
        }
        if let Some(b) = &mut slot.browse {
            b.rebase_normal(today_jst);
        }
        slot.install_rendered(frames);
        slot.error_pixels = Arc::new(WeatherPixels::error(choice));
        slot.error = false;
        Ok(())
    }
    fn position(&self, ctx: &PluginContext) -> Option<PluginPosition> {
        let slot = self.slot(ctx)?;
        let browse = slot.browse.as_ref()?;
        let (set, day) = browse.current();
        if !browse.is_browsing() && slot.today_jst.as_deref() != Some(day.date_jst.as_str()) {
            return None;
        }
        Some(PluginPosition {
            plugin: self.id().into(),
            dataset_id: set.id.clone(),
            target_date: day.date_jst.clone(),
            index: browse.index(),
        })
    }
    fn eink_frame_for(&self, ctx: &PluginContext, lcd: &[u8]) -> Option<Vec<u8>> {
        let slot = self.slot(ctx)?;
        let pixels = if slot.error {
            slot.error_pixels.as_ref()
        } else {
            slot.current_pixels()?
        };
        (pixels.lcd.as_slice() == lcd).then(|| pixels.eink.clone())
    }
    fn current_frame(&self, ctx: &PluginContext) -> Option<Vec<u8>> {
        let slot = self.slot(ctx)?;
        if slot.error
            || slot.browse.as_ref().is_some_and(|b| {
                !b.is_browsing()
                    && slot.today_jst.as_deref() != Some(b.current().1.date_jst.as_str())
            })
        {
            return Some(slot.error_pixels.lcd.clone());
        }
        Some(slot.current_pixels()?.lcd.clone())
    }
    fn observe_date(&mut self, ctx: &PluginContext, today_jst: &str) -> bool {
        let slot = self.slot_mut(ctx);
        if slot.today_jst.as_deref() == Some(today_jst) {
            return false;
        }
        slot.today_jst = Some(today_jst.into());
        if let Some(b) = &mut slot.browse {
            b.rebase_normal(today_jst);
        }
        true
    }
    fn plan(
        &mut self,
        ctx: &PluginContext,
        action: ButtonAction,
    ) -> Result<PluginPlan, ContentError> {
        let slot = self.slot_mut(ctx);
        let today = slot.today_jst.clone();
        let browse = slot.browse.as_mut().ok_or(ContentError::Unavailable)?;
        let event = match (action, &ctx.mode) {
            (ButtonAction::Sub, Mode::Normal) => {
                if !today
                    .as_ref()
                    .is_some_and(|date| browse.latest.days.iter().any(|d| &d.date_jst == date))
                {
                    return Err(ContentError::Unavailable);
                }
                browse.begin();
                Some(Event::EnterSub {
                    token: ctx.token.clone(),
                })
            }
            (ButtonAction::Up, Mode::Sub) => browse.previous().then(|| Event::PluginInput {
                token: ctx.token.clone(),
                name: "up".into(),
            }),
            (ButtonAction::Down, Mode::Sub) => browse.advance().then(|| Event::PluginInput {
                token: ctx.token.clone(),
                name: "down".into(),
            }),
            (ButtonAction::Power, Mode::Sub) => {
                browse.cancel();
                if let Some(date) = &today {
                    browse.rebase_normal(date);
                }
                Some(Event::Power {
                    token: ctx.token.clone(),
                })
            }
            _ => return Err(ContentError::Unsupported),
        };
        let (set, day) = browse.current();
        let day_available = browse.is_browsing() || today.as_deref() == Some(day.date_jst.as_str());
        let position = PluginPosition {
            plugin: "weather".into(),
            dataset_id: set.id.clone(),
            target_date: if day_available {
                day.date_jst.clone()
            } else {
                today.unwrap_or_else(|| day.date_jst.clone())
            },
            index: browse.index(),
        };
        let frame = if event.is_none() {
            None
        } else if slot.error || !day_available {
            Some(slot.error_pixels.lcd.clone())
        } else {
            Some(
                slot.current_pixels()
                    .ok_or(ContentError::Unavailable)?
                    .lcd
                    .clone(),
            )
        };
        slot.prune_rendered();
        Ok(PluginPlan {
            event,
            frame,
            position,
        })
    }
    fn session_reset(&mut self, device_id: &str, incarnation: u64) {
        if let Some(slot) = self.devices.get_mut(device_id) {
            if slot.incarnation == incarnation {
                if slot.in_flight.take().is_some() {
                    slot.last_attempt_unix = None;
                }
                if let Some(b) = &mut slot.browse {
                    b.cancel();
                }
                slot.prune_rendered();
            } else {
                self.devices.remove(device_id);
            }
        }
    }
    fn delete_device(&mut self, device_id: &str) {
        self.devices.remove(device_id);
    }
    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(
            self.devices
                .iter()
                .map(|(id, slot)| (id.clone(), slot.stored()))
                .collect::<BTreeMap<_, _>>(),
        )
        .expect("weather state")
    }
    fn options(&self) -> serde_json::Value {
        serde_json::json!({"locations": &*LOCATIONS})
    }
    fn fetch_policy(
        &self,
        settings: &crate::settings::PluginSettings,
    ) -> Option<crate::settings::FetchPolicy> {
        Some(settings.weather_fetch.clone())
    }
    fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
        let stored: BTreeMap<String, StoredWeatherSlot> =
            serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        self.devices = stored
            .into_iter()
            .map(|(id, s)| {
                if location(&s.location_id).is_none() {
                    return Err(ContentError::InvalidSource);
                }
                let mut browse = s.latest.map(WeatherBrowse::new);
                if let (Some(b), Some(today)) = (&mut browse, &s.today_jst) {
                    b.rebase_normal(today);
                }
                let rendered = if let Some(b) = &browse {
                    vec![RenderedForecast {
                        set: b.latest.clone(),
                        frames: Arc::new(render_set(&b.latest)?),
                    }]
                } else {
                    Vec::new()
                };
                let error_pixels = Arc::new(WeatherPixels::error(
                    location(&s.location_id).ok_or(ContentError::InvalidSource)?,
                ));
                Ok((
                    id,
                    WeatherSlot {
                        incarnation: s.incarnation,
                        settings_revision: s.settings_revision,
                        config_revision: s.config_revision,
                        location_id: s.location_id,
                        today_jst: s.today_jst,
                        browse,
                        rendered,
                        error_pixels,
                        last_success_unix: s.last_success_unix,
                        last_attempt_unix: if s.had_in_flight {
                            None
                        } else {
                            s.last_attempt_unix
                        },
                        error: s.error,
                        generation: s.generation,
                        in_flight: None,
                    },
                ))
            })
            .collect::<Result<_, _>>()?;
        Ok(())
    }
    fn device_config(&self, device_id: &str, incarnation: u64) -> serde_json::Value {
        let slot = self
            .devices
            .get(device_id)
            .filter(|s| s.incarnation == incarnation);
        serde_json::json!({"revision":slot.map_or(1, |s| s.config_revision),
            "location_id":slot.map_or("tokyo-station", |s| s.location_id.as_str()),
            "last_success_unix":slot.and_then(|s| s.last_success_unix),
            "error":slot.is_some_and(|s| s.error)})
    }
    fn set_device_config(
        &mut self,
        ctx: &PluginContext,
        expected_revision: u64,
        config: serde_json::Value,
    ) -> Result<u64, ContentError> {
        let choice: LocationChoice =
            serde_json::from_value(config).map_err(|_| ContentError::InvalidSource)?;
        if location(&choice.location_id).is_none() {
            return Err(ContentError::InvalidSource);
        }
        let slot = self.slot_mut(ctx);
        if slot.config_revision != expected_revision {
            return Err(ContentError::Stale);
        }
        if slot.location_id != choice.location_id {
            slot.location_id = choice.location_id;
            slot.config_revision += 1;
            slot.settings_revision += 1;
            slot.browse = None;
            slot.rendered.clear();
            slot.error_pixels = Arc::new(WeatherPixels::error(
                location(&slot.location_id).ok_or(ContentError::InvalidSource)?,
            ));
            slot.today_jst = None;
            slot.last_success_unix = None;
            slot.last_attempt_unix = None;
            slot.in_flight = None;
            slot.error = false;
        }
        Ok(slot.config_revision)
    }
    fn plan_fetch(
        &mut self,
        ctx: &PluginContext,
        now_unix: i64,
        today_jst: &str,
        policy: &crate::settings::FetchPolicy,
    ) -> Option<FetchJob> {
        let slot = self.slot_mut(ctx);
        slot.today_jst = Some(today_jst.into());
        if let Some(b) = &mut slot.browse {
            b.rebase_normal(today_jst);
        }
        if slot.in_flight.is_some() {
            return None;
        }
        let day_changed = slot
            .browse
            .as_ref()
            .is_some_and(|b| b.latest.today_jst != today_jst);
        let due = day_changed
            || slot
                .last_success_unix
                .is_none_or(|at| now_unix.saturating_sub(at) >= 3600);
        // A failed attempt never becomes the freshness origin. A short,
        // separate cooldown keeps an unavailable source from being hammered.
        let retry_ready = !slot.error
            || slot
                .last_attempt_unix
                .is_none_or(|at| now_unix.saturating_sub(at) >= 60);
        if !due || !retry_ready {
            return None;
        }
        let choice = location(&slot.location_id)?;
        slot.generation += 1;
        slot.in_flight = Some(slot.generation);
        slot.last_attempt_unix = Some(now_unix);
        Some(FetchJob {
            plugin: "weather".into(),
            device_id: ctx.token.device_id.clone(),
            incarnation: ctx.token.incarnation,
            session_epoch: ctx.token.session_epoch,
            settings_revision: ctx.settings_revision,
            config_revision: slot.config_revision,
            generation: slot.generation,
            source_key: choice.office_code.clone(),
            dataset_id: format!("{}:{}:{}", choice.office_code, now_unix, slot.generation),
            today_jst: today_jst.into(),
            timeout_seconds: policy.timeout_seconds,
            retries: policy.retries,
        })
    }
    fn fetch_preparation(
        &self,
        job: &FetchJob,
    ) -> Result<crate::content::FetchPreparation, ContentError> {
        let slot = self
            .devices
            .get(&job.device_id)
            .ok_or(ContentError::Stale)?;
        if slot.incarnation != job.incarnation
            || slot.settings_revision != job.settings_revision
            || slot.config_revision != job.config_revision
            || slot.in_flight != Some(job.generation)
        {
            return Err(ContentError::Stale);
        }
        let choice = location(&slot.location_id).ok_or(ContentError::InvalidSource)?;
        let job = job.clone();
        Ok(Box::new(move |result| {
            let set = result.ok().and_then(|bytes| {
                ForecastSet::for_location(&bytes, &job.dataset_id, &job.today_jst, choice).ok()
            });
            let frames = match &set {
                Some(set) => render_set(set)?,
                None => Vec::new(),
            };
            let error_pixels = Arc::new(WeatherPixels::error(choice));
            let pixels = frames
                .first()
                .map(|f| f.lcd.clone())
                .unwrap_or_else(|| error_pixels.lcd.clone());
            Ok(Box::new(PreparedWeather {
                set,
                pixels,
                frames,
                error_pixels,
            }) as crate::content::PreparedFetch)
        }))
    }
    fn complete_fetch(
        &mut self,
        job: &FetchJob,
        now: i64,
        result: Result<Vec<u8>, String>,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        let prepared = self.fetch_preparation(job)?(result)?;
        self.apply_prepared_fetch(job, now, prepared)
    }
    fn apply_prepared_fetch(
        &mut self,
        job: &FetchJob,
        now: i64,
        prepared: crate::content::PreparedFetch,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        let prepared = *prepared
            .downcast::<PreparedWeather>()
            .map_err(|_| ContentError::InvalidSource)?;
        let slot = self
            .devices
            .get_mut(&job.device_id)
            .ok_or(ContentError::Stale)?;
        if slot.incarnation != job.incarnation
            || slot.settings_revision != job.settings_revision
            || slot.config_revision != job.config_revision
            || slot.in_flight != Some(job.generation)
        {
            return Err(ContentError::Stale);
        }
        slot.in_flight = None;
        slot.error_pixels = prepared.error_pixels;
        if let Some(set) = prepared.set {
            match &mut slot.browse {
                Some(b) => b.publish(set),
                None => slot.browse = Some(WeatherBrowse::new(set)),
            }
            slot.install_rendered(prepared.frames);
            slot.today_jst = Some(job.today_jst.clone());
            if let Some(b) = &mut slot.browse {
                b.rebase_normal(&job.today_jst);
            }
            slot.last_success_unix = Some(now);
            slot.error = false;
            Ok((!slot.browse.as_ref().is_some_and(|b| b.is_browsing())).then_some(prepared.pixels))
        } else {
            slot.error = true;
            Ok(Some(prepared.pixels))
        }
    }
    fn cancel_fetches(&mut self) {
        for slot in self.devices.values_mut() {
            if slot.in_flight.take().is_some() {
                slot.last_attempt_unix = None;
            }
        }
    }
}

pub struct WeatherPlugin;
impl Plugin for WeatherPlugin {
    fn id(&self) -> &str {
        "weather"
    }
    fn supports(&self, caps: &std::collections::BTreeSet<Capability>) -> bool {
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
                    d.selected.as_deref() == Some("weather") && d.mode == Mode::Normal
                }) =>
            {
                Some(Candidate::new(
                    query,
                    &token.device_id,
                    Priority::User,
                    "weather-browse-start",
                    Transition::EnterMode {
                        token: token.clone(),
                        mode: Mode::Sub,
                    },
                ))
            }
            Event::PluginInput { token, name }
                if (name == "up" || name == "down")
                    && query.device(&token.device_id).is_some_and(|d| {
                        d.selected.as_deref() == Some("weather") && d.mode == Mode::Sub
                    }) =>
            {
                Some(Candidate::new(
                    query,
                    &token.device_id,
                    Priority::User,
                    "weather-browse-move",
                    Transition::RefreshView {
                        token: token.clone(),
                        plugin: "weather".into(),
                    },
                ))
            }
            _ => None,
        };
        Box::new(candidate.into_iter())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastDay {
    pub date_jst: String,
    pub weather_code: String,
    pub weather_source: String,
    pub min_c: Option<i16>,
    pub max_c: Option<i16>,
    pub humidity: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForecastSet {
    pub id: String,
    pub today_jst: String,
    pub area_code: String,
    pub area_label: String,
    pub point_code: String,
    pub point_label: String,
    pub days: Vec<ForecastDay>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeatherLocation {
    pub id: String,
    pub label: String,
    pub office_code: String,
    pub office_label: String,
    pub area_code: String,
    pub area_label: String,
    pub weekly_area_code: String,
    pub point_code: String,
    pub point_label: String,
}

/// Verified mappings derived from JMA's public area catalogue and matching
/// short/weekly forecast and representative-temperature series.
pub static LOCATIONS: LazyLock<Vec<WeatherLocation>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/jma-locations.json"))
        .expect("bundled JMA location catalogue")
});
pub fn location(id: &str) -> Option<&'static WeatherLocation> {
    LOCATIONS.iter().find(|l| l.id == id)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WeatherError {
    InvalidJson,
    MissingSeries,
    InvalidDate,
    IncompleteForecast,
    InvalidTemperature,
}

fn date(s: &str) -> Result<Date, WeatherError> {
    let y = s
        .get(0..4)
        .and_then(|v| v.parse().ok())
        .ok_or(WeatherError::InvalidDate)?;
    let m: u8 = s
        .get(5..7)
        .and_then(|v| v.parse().ok())
        .ok_or(WeatherError::InvalidDate)?;
    let d = s
        .get(8..10)
        .and_then(|v| v.parse().ok())
        .ok_or(WeatherError::InvalidDate)?;
    if s.as_bytes().get(4) != Some(&b'-') || s.as_bytes().get(7) != Some(&b'-') {
        return Err(WeatherError::InvalidDate);
    }
    Date::from_calendar_date(
        y,
        Month::try_from(m).map_err(|_| WeatherError::InvalidDate)?,
        d,
    )
    .map_err(|_| WeatherError::InvalidDate)
}
fn date_string(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}
fn field<'a>(
    raw: &'a Value,
    block: usize,
    series: usize,
    code: &str,
    key: &str,
) -> Result<(&'a [Value], &'a [Value], &'a str), WeatherError> {
    let s = raw
        .get(block)
        .and_then(|b| b.get("timeSeries"))
        .and_then(|a| a.get(series))
        .ok_or(WeatherError::MissingSeries)?;
    let times = s
        .get("timeDefines")
        .and_then(Value::as_array)
        .ok_or(WeatherError::MissingSeries)?;
    let area = s
        .get("areas")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|v| v["area"]["code"].as_str() == Some(code)))
        .ok_or(WeatherError::MissingSeries)?;
    let values = area
        .get(key)
        .and_then(Value::as_array)
        .ok_or(WeatherError::MissingSeries)?;
    let label = area["area"]["name"]
        .as_str()
        .ok_or(WeatherError::MissingSeries)?;
    if times.len() != values.len() {
        return Err(WeatherError::MissingSeries);
    }
    Ok((times, values, label))
}
fn dated_strings(
    times: &[Value],
    values: &[Value],
) -> Result<BTreeMap<String, String>, WeatherError> {
    let mut result = BTreeMap::new();
    for (t, v) in times.iter().zip(values) {
        let timestamp = t.as_str().ok_or(WeatherError::InvalidDate)?;
        let day = timestamp.get(..10).ok_or(WeatherError::InvalidDate)?;
        date(day)?;
        let value = v.as_str().ok_or(WeatherError::MissingSeries)?;
        if !value.is_empty() {
            result.insert(day.to_owned(), value.to_owned());
        }
    }
    Ok(result)
}
fn temperature(v: Option<&String>) -> Result<Option<i16>, WeatherError> {
    match v {
        Some(s) if !s.is_empty() => {
            let n: i16 = s.parse().map_err(|_| WeatherError::InvalidTemperature)?;
            if !(-100..=100).contains(&n) {
                return Err(WeatherError::InvalidTemperature);
            }
            Ok(Some(n))
        }
        _ => Ok(None),
    }
}

impl ForecastSet {
    /// Interpret one cached JMA response for a forecast area and representative
    /// temperature point. The two labels remain distinct on every frame.
    pub fn from_jma(
        bytes: &[u8],
        id: &str,
        today_jst: &str,
        area_code: &str,
        point_code: &str,
    ) -> Result<Self, WeatherError> {
        Self::from_jma_regions(bytes, id, today_jst, area_code, area_code, point_code)
    }
    fn from_jma_regions(
        bytes: &[u8],
        id: &str,
        today_jst: &str,
        area_code: &str,
        weekly_area_code: &str,
        point_code: &str,
    ) -> Result<Self, WeatherError> {
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| WeatherError::InvalidJson)?;
        let today = date(today_jst)?;
        let (swt, swa, area_label) = field(&raw, 0, 0, area_code, "weatherCodes")?;
        let (wwt, wwa, _) = field(&raw, 1, 0, weekly_area_code, "weatherCodes")?;
        let (stt, sta, point_label) = field(&raw, 0, 2, point_code, "temps")?;
        let (wtt, wta_min, _) = field(&raw, 1, 1, point_code, "tempsMin")?;
        let (_, wta_max, _) = field(&raw, 1, 1, point_code, "tempsMax")?;
        let sw = dated_strings(swt, swa)?;
        let ww = dated_strings(wwt, wwa)?;
        let wmin = dated_strings(wtt, wta_min)?;
        let wmax = dated_strings(wtt, wta_max)?;
        let issued = raw[0]["reportDatetime"]
            .as_str()
            .ok_or(WeatherError::InvalidDate)?;
        let issue_day = issued.get(..10).ok_or(WeatherError::InvalidDate)?;
        date(issue_day)?;
        let mut smin = BTreeMap::new();
        let mut smax = BTreeMap::new();
        for (timestamp, value) in stt.iter().zip(sta) {
            let t = timestamp.as_str().ok_or(WeatherError::InvalidDate)?;
            let day = t.get(..10).ok_or(WeatherError::InvalidDate)?;
            date(day)?;
            let hour = t.get(11..13).ok_or(WeatherError::InvalidDate)?;
            let v = value.as_str().ok_or(WeatherError::MissingSeries)?;
            if v.is_empty() {
                continue;
            }
            if hour == "00" && t >= issued {
                smin.insert(day.to_owned(), v.to_owned());
            }
            if hour == "09" && day >= issue_day {
                smax.insert(day.to_owned(), v.to_owned());
            }
        }
        let mut days = Vec::with_capacity(7);
        let mut current = today;
        for _ in 0..7 {
            let key = date_string(current);
            let (code, source) = if let Some(code) = sw.get(&key) {
                (code, "短期")
            } else if let Some(code) = ww.get(&key) {
                (code, "週間")
            } else {
                return Err(WeatherError::IncompleteForecast);
            };
            if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(WeatherError::IncompleteForecast);
            }
            days.push(ForecastDay {
                date_jst: key.clone(),
                weather_code: code.clone(),
                weather_source: source.into(),
                min_c: temperature(smin.get(&key).or_else(|| wmin.get(&key)))?,
                max_c: temperature(smax.get(&key).or_else(|| wmax.get(&key)))?,
                humidity: None,
            });
            current = current.next_day().ok_or(WeatherError::InvalidDate)?;
        }
        Ok(Self {
            id: id.into(),
            today_jst: today_jst.into(),
            area_code: area_code.into(),
            area_label: area_label.into(),
            point_code: point_code.into(),
            point_label: point_label.into(),
            days,
        })
    }
    pub fn tokyo_from_cache(bytes: &[u8], id: &str, today_jst: &str) -> Result<Self, WeatherError> {
        Self::from_jma(bytes, id, today_jst, "130010", "44132")
    }
    pub fn for_location(
        bytes: &[u8],
        id: &str,
        today_jst: &str,
        choice: &WeatherLocation,
    ) -> Result<Self, WeatherError> {
        let mut set = Self::from_jma_regions(
            bytes,
            id,
            today_jst,
            &choice.area_code,
            &choice.weekly_area_code,
            &choice.point_code,
        )?;
        if choice.office_code != "130000" && choice.office_label != choice.area_label {
            set.area_label = format!("{}{}", choice.office_label, choice.area_label);
        }
        Ok(set)
    }
}

#[derive(Clone)]
pub struct WeatherBrowse {
    latest: Arc<ForecastSet>,
    pinned: Option<Arc<ForecastSet>>,
    index: usize,
    start_index: usize,
}
impl WeatherBrowse {
    pub fn new(set: ForecastSet) -> Self {
        Self {
            latest: Arc::new(set),
            pinned: None,
            index: 0,
            start_index: 0,
        }
    }
    pub fn publish(&mut self, set: ForecastSet) {
        self.latest = Arc::new(set);
        if self.pinned.is_none() {
            self.index = 0;
            self.start_index = 0;
        }
    }
    pub fn rebase_normal(&mut self, today_jst: &str) {
        if self.pinned.is_none() {
            if let Some(index) = self
                .latest
                .days
                .iter()
                .position(|d| d.date_jst == today_jst)
            {
                self.index = index;
                self.start_index = index;
            }
        }
    }
    pub fn begin(&mut self) {
        self.pinned = Some(self.latest.clone());
        self.start_index = self.index;
    }
    pub fn advance(&mut self) -> bool {
        if self.pinned.is_some() {
            let next = (self.index + 1).min(6);
            let changed = next != self.index;
            self.index = next;
            return changed;
        }
        false
    }
    pub fn previous(&mut self) -> bool {
        if self.pinned.is_some() {
            let next = self.index.saturating_sub(1).max(self.start_index);
            let changed = next != self.index;
            self.index = next;
            return changed;
        }
        false
    }
    pub fn cancel(&mut self) {
        self.pinned = None;
        self.index = self.start_index;
    }
    pub fn current(&self) -> (&ForecastSet, &ForecastDay) {
        let set = self.pinned.as_deref().unwrap_or(&self.latest);
        (set, &set.days[self.index])
    }
    pub fn is_browsing(&self) -> bool {
        self.pinned.is_some()
    }
    pub fn index(&self) -> usize {
        self.index - self.start_index
    }
}

const W: usize = 160;
const H: usize = 80;
fn color(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}
struct Canvas([u16; W * H]);
impl Canvas {
    fn new(bg: u16) -> Self {
        Self([bg; W * H])
    }
    fn pixel(&mut self, x: i32, y: i32, c: u16) {
        if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
            self.0[y as usize * W + x as usize] = c;
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u16) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.pixel(x, y, c);
            }
        }
    }
    fn circle(&mut self, cx: i32, cy: i32, r: i32, c: u16) {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y <= r * r {
                    self.pixel(cx + x, cy + y, c);
                }
            }
        }
    }
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u16) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for i in 0..=steps {
            self.pixel(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps, c);
        }
    }
    fn text(&mut self, x: i32, y: i32, s: &str, size: usize, c: u16) {
        crate::lcd_text::draw(&mut self.0, (W, H), (x, y), s, size, c);
    }
    fn finish(self) -> Vec<u8> {
        self.0.iter().flat_map(|v| v.to_be_bytes()).collect()
    }
}

fn location_labels(area: &str, point: &str) -> (String, String) {
    let area = format!("予報:{area}");
    let point = format!("気温:{point}");
    let point_width = crate::lcd_text::width(&point, 12).min(76);
    (
        crate::lcd_text::fit(&area, 12, 152 - point_width),
        crate::lcd_text::fit(&point, 12, point_width),
    )
}

/// Draw the 160×80 RGB565 composition with native-size Japanese glyphs.
/// Long location labels are explicitly abbreviated without shrinking them.
pub fn render_lcd(set: &ForecastSet, day: &ForecastDay) -> Result<Vec<u8>, WeatherError> {
    #[cfg(test)]
    FULL_RENDER_CALLS.with(|v| {
        let (a, b) = v.get();
        v.set((a + 1, b));
    });
    render_forecast(set, day, false)
}

pub fn render_eink(set: &ForecastSet, day: &ForecastDay) -> Result<Vec<u8>, WeatherError> {
    #[cfg(test)]
    FULL_RENDER_CALLS.with(|v| {
        let (a, b) = v.get();
        v.set((a, b + 1));
    });
    Ok(crate::eink::from_lcd(&render_forecast(set, day, true)?))
}

fn render_forecast(
    set: &ForecastSet,
    day: &ForecastDay,
    monochrome_icon: bool,
) -> Result<Vec<u8>, WeatherError> {
    if !LOCATIONS
        .iter()
        .any(|l| l.area_code == set.area_code && l.point_code == set.point_code)
    {
        return Err(WeatherError::MissingSeries);
    }
    let d = date(&day.date_jst)?;
    let weekday =
        ["月", "火", "水", "木", "金", "土", "日"][d.weekday().number_days_from_monday() as usize];
    let mut im = Canvas::new(color(0xec, 0xf5, 0xfa));
    let fg = color(0x10, 0x2e, 0x47);
    im.rect(0, 0, 159, 21, color(0x12, 0x3c, 0x58));
    im.text(
        4,
        0,
        &format!("{:02}/{:02} {}", u8::from(d.month()), d.day(), weekday),
        16,
        color(255, 255, 255),
    );
    im.text(
        156 - crate::lcd_text::width(&day.weather_source, 14) as i32,
        1,
        &day.weather_source,
        14,
        color(255, 255, 255),
    );
    let (area, point) = location_labels(&set.area_label, &set.point_label);
    im.text(3, 22, &area, 12, fg);
    im.text(
        157 - crate::lcd_text::width(&point, 12) as i32,
        22,
        &point,
        12,
        fg,
    );
    let kind = day
        .weather_code
        .as_bytes()
        .first()
        .copied()
        .ok_or(WeatherError::IncompleteForecast)?;
    let ink = |r, g, b| if monochrome_icon { 0 } else { color(r, g, b) };
    if kind == b'1' {
        im.circle(34, 54, 10, ink(0xff, 0xd3, 0x4f));
        for (x, y) in [
            (0, -17),
            (0, 17),
            (-17, 0),
            (17, 0),
            (-12, -12),
            (12, 12),
            (-12, 12),
            (12, -12),
        ] {
            im.line(
                34 + x * 2 / 3,
                54 + y * 2 / 3,
                34 + x,
                54 + y,
                ink(0xdf, 0x9a, 0),
            );
        }
    } else {
        let cloud = ink(0xd3, 0xe3, 0xec);
        im.circle(24, 57, 11, cloud);
        im.circle(38, 50, 13, cloud);
        im.rect(17, 54, 49, 66, cloud);
        im.line(17, 66, 49, 66, ink(0x4a, 0x63, 0x79));
        if kind == b'3' || kind == b'4' {
            for x in [23, 34, 45] {
                im.line(x, 70, x - 3, 77, ink(0x23, 0x84, 0xcb));
            }
        }
    }
    let low = day
        .min_c
        .map_or_else(|| "ーー".to_owned(), |v| v.to_string());
    let high = day
        .max_c
        .map_or_else(|| "ーー".to_owned(), |v| v.to_string());
    im.text(64, 39, "低", 16, fg);
    im.text(86, 36, &format!("{}°", low), 22, fg);
    im.text(64, 61, "高", 16, fg);
    im.text(86, 58, &format!("{}°", high), 22, fg);
    Ok(im.finish())
}

pub fn render_error_lcd(choice: &WeatherLocation) -> Vec<u8> {
    let mut im = Canvas::new(color(0xec, 0xf5, 0xfa));
    im.rect(0, 0, 159, 21, color(0x85, 0x28, 0x28));
    im.text(5, 0, "予報", 16, color(255, 255, 255));
    im.text(
        4,
        26,
        &crate::lcd_text::fit(&choice.label, 12, 152),
        12,
        color(0x10, 0x2e, 0x47),
    );
    im.text(40, 47, "取得失敗", 20, color(0x85, 0x28, 0x28));
    im.finish()
}

/// Explicit JMA source adapter. Only office codes in the supported catalogue
/// can reach the public endpoint; no device location or private URL is sent.
pub struct JmaHttpFetcher;
impl JmaHttpFetcher {
    fn source_url(job: &FetchJob) -> Result<String, String> {
        if job.plugin != "weather" || !LOCATIONS.iter().any(|l| l.office_code == job.source_key) {
            return Err("unsupported source".into());
        }
        Ok(format!(
            "https://www.jma.go.jp/bosai/forecast/data/forecast/{}.json",
            job.source_key
        ))
    }

    pub async fn fetch_async(job: &FetchJob) -> Result<Vec<u8>, String> {
        let url = Self::source_url(job)?;
        reqwest::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(job.timeout_seconds.into()))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .bytes()
            .await
            .map(|body| body.to_vec())
            .map_err(|error| error.to_string())
    }
}
impl crate::fetch::SourceFetcher for JmaHttpFetcher {
    fn fetch(&mut self, job: &FetchJob) -> Result<Vec<u8>, String> {
        let url = Self::source_url(job)?;
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(job.timeout_seconds.into()))
            .build()
            .map_err(|_| "client error".to_owned())?
            .get(url)
            .send()
            .map_err(|_| "request error".to_owned())?
            .error_for_status()
            .map_err(|_| "response error".to_owned())?
            .bytes()
            .map(|body| body.to_vec())
            .map_err(|_| "body error".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eink_icons_keep_geometry_black_contrast_and_lcd_palette() {
        let set = ForecastSet::tokyo_from_cache(CACHE, "icons", "2026-10-12").unwrap();
        for code in ["100", "200", "300", "400"] {
            let mut day = set.days[0].clone();
            day.weather_code = code.into();
            let lcd = render_lcd(&set, &day).unwrap();
            let ink_lcd = render_forecast(&set, &day, true).unwrap();
            let mono = render_eink(&set, &day).unwrap();
            assert_eq!(mono.len(), crate::eink::FRAME_BYTES);
            let pixel = |pixels: &[u8], x: usize, y: usize| {
                u16::from_be_bytes([pixels[(y * 160 + x) * 2], pixels[(y * 160 + x) * 2 + 1]])
            };
            if code == "100" {
                assert_eq!(pixel(&lcd, 34, 54), color(255, 211, 79));
                assert_eq!(pixel(&ink_lcd, 34, 54), 0);
                assert_eq!(pixel(&ink_lcd, 34, 37), 0);
            } else {
                assert_eq!(pixel(&lcd, 38, 50), color(211, 227, 236));
                assert_eq!(pixel(&ink_lcd, 38, 50), 0);
                if code == "300" || code == "400" {
                    assert_eq!(pixel(&ink_lcd, 23, 70), 0);
                }
            }
            // Recolour only existing icon strokes; keep every other LCD pixel.
            for y in 0..80 {
                for x in 0..160 {
                    if pixel(&lcd, x, y) != pixel(&ink_lcd, x, y) {
                        assert!((10..=51).contains(&x) && (37..=77).contains(&y));
                        assert_eq!(pixel(&ink_lcd, x, y), 0);
                    }
                }
            }
            let black = |pixels: &[u8]| {
                (55..118)
                    .flat_map(|y| (15..82).map(move |x| (x, y)))
                    .filter(|(x, y)| {
                        let b = y * 250 + x;
                        pixels[b / 8] & (0x80 >> (b % 8)) == 0
                    })
                    .count()
            };
            assert!(black(&mono) > black(&crate::eink::from_lcd(&lcd)) + 200);
            if let Ok(directory) = std::env::var("PM_EINK_EVIDENCE_DIR") {
                let path = std::path::Path::new(&directory);
                std::fs::create_dir_all(path).unwrap();
                for (suffix, bytes) in [
                    ("lcd.rgb565be", lcd),
                    (
                        "before.mono1",
                        crate::eink::from_lcd(&render_lcd(&set, &day).unwrap()),
                    ),
                    ("after.mono1", mono),
                ] {
                    std::fs::write(path.join(format!("weather-{code}-{suffix}")), bytes).unwrap();
                }
            }
        }
    }

    #[test]
    fn large_lcd_text_fits_national_labels_and_temperature_extremes() {
        for location in LOCATIONS.iter() {
            let area_label = format!("{}{}", location.office_label, location.area_label);
            let (area, point) = location_labels(&area_label, &location.point_label);
            assert!(area.starts_with("予報:") && point.starts_with("気温:"));
            let point_x = 157 - crate::lcd_text::width(&point, 12);
            assert!(3 + crate::lcd_text::width(&area, 12) + 2 <= point_x);
            crate::lcd_text::assert_text_fits(&area, 12, (3, 22), (point_x - 2, 37));
            crate::lcd_text::assert_text_fits(&point, 12, (point_x, 22), (157, 37));
        }
        crate::lcd_text::assert_text_fits("10/31 木", 16, (4, 0), (120, 22));
        for source in ["短期", "週間"] {
            let x = 156 - crate::lcd_text::width(source, 14);
            crate::lcd_text::assert_text_fits(source, 14, (x, 1), (160, 22));
        }
        crate::lcd_text::assert_text_fits("低", 16, (64, 39), (160, 80));
        crate::lcd_text::assert_text_fits("高", 16, (64, 61), (160, 80));
        for value in ["-60°", "60°", "ーー°"] {
            crate::lcd_text::assert_text_fits(value, 22, (86, 58), (160, 80));
        }
    }
    const CACHE: &[u8] = include_bytes!("../assets/jma-tokyo-20261012.json");
    #[test]
    fn cached_jma_seven_days_missing_not_zero_and_render() {
        let set = ForecastSet::tokyo_from_cache(CACHE, "20261012T032152Z", "2026-10-12").unwrap();
        assert_eq!(set.days.len(), 7);
        assert_eq!(set.days[0].date_jst, "2026-10-12");
        assert_eq!(set.days[0].min_c, None);
        assert_eq!(set.days[0].max_c, Some(27));
        assert_eq!(set.days[2].min_c, Some(18));
        assert_eq!(set.days[3].weather_source, "週間");
        assert_eq!(set.days[6].date_jst, "2026-10-18");
        assert!(set.days.iter().all(|d| d.humidity.is_none()));
        let frame = render_lcd(&set, &set.days[0]).unwrap();
        assert_eq!(frame.len(), crate::protocol::LCD_FRAME_BYTES);
        let mut zero_low = set.days[0].clone();
        zero_low.min_c = Some(0);
        assert_ne!(frame, render_lcd(&set, &zero_low).unwrap());
        assert!(frame[..W * 22 * 2].as_chunks::<2>().0.contains(&[255, 255]));
    }
    #[test]
    fn browse_pins_generation_stops_at_edges_and_cancel_uses_latest_today() {
        let old = ForecastSet::tokyo_from_cache(CACHE, "old", "2026-10-12").unwrap();
        let mut browser = WeatherBrowse::new(old);
        browser.begin();
        for _ in 0..10 {
            browser.advance();
        }
        assert_eq!(browser.current().1.date_jst, "2026-10-18");
        let mut updated = ForecastSet::tokyo_from_cache(CACHE, "new", "2026-10-12").unwrap();
        updated.days[0].max_c = Some(28);
        browser.publish(updated);
        assert_eq!(browser.current().0.id, "old");
        for _ in 0..10 {
            browser.previous();
        }
        assert_eq!(browser.current().1.date_jst, "2026-10-12");
        assert_eq!(browser.current().1.max_c, Some(27));
        browser.cancel();
        assert_eq!(browser.current().0.id, "new");
        assert_eq!(browser.current().1.max_c, Some(28));
        assert!(!browser.is_browsing());
    }
    #[test]
    fn rendered_cache_preserves_same_id_pinned_snapshot_and_stays_bounded() {
        let ctx = PluginContext {
            token: crate::SessionToken {
                device_id: "cache".into(),
                incarnation: 1,
                session_epoch: 1,
            },
            settings_revision: 1,
            mode: Mode::Normal,
        };
        let mut content = WeatherContent::default();
        content
            .publish(&ctx, CACHE, "same-id", "2026-10-12")
            .unwrap();
        let original = content.current_frame(&ctx).unwrap();
        content.plan(&ctx, ButtonAction::Sub).unwrap();
        let sub = PluginContext {
            mode: Mode::Sub,
            ..ctx.clone()
        };
        let mut value: serde_json::Value = serde_json::from_slice(CACHE).unwrap();
        fn change(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(map) => {
                    for (k, v) in map {
                        if k == "weatherCodes" {
                            if let Some(days) = v.as_array_mut() {
                                for day in days {
                                    *day = serde_json::Value::String("300".into());
                                }
                            }
                        } else {
                            change(v);
                        }
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        change(item)
                    }
                }
                _ => {}
            }
        }
        change(&mut value);
        let changed = serde_json::to_vec(&value).unwrap();
        for _ in 0..6 {
            content
                .publish(&sub, &changed, "same-id", "2026-10-12")
                .unwrap();
            assert!(content.current_frame(&sub).unwrap()==original,"pinned pixels changed for reused dataset ID");
            assert_eq!(content.devices["cache"].rendered.len(), 2);
        }
        content.plan(&sub, ButtonAction::Power).unwrap();
        assert_eq!(content.devices["cache"].rendered.len(), 1);
        assert!(content.current_frame(&ctx).unwrap()!=original,"cancel must select the new rain pixels");
    }
}
