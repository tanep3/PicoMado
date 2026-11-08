//! Local music state owns intent; filesystem/ffmpeg are isolated in executor.
mod executor;
use crate::content::*;
use crate::plugin_jobs::*;
use crate::{Candidate, Capability, Event, Mode, Plugin, Priority, Transition, WorldQuery};
pub use executor::LocalMusicExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) const MAX_SOURCE: u64 = 256 * 1024 * 1024;
pub const SAMPLE_RATE: u32 = 48_000;
pub const MAX_FRAMES: u64 = 48_000 * 7200;
const TOGGLE_PLAYBACK: &str = "toggle-playback";
const NEXT_TRACK: &str = "next-track";
#[derive(Clone, Copy)]
enum MusicControl { Next, Toggle }
#[derive(Clone, Copy)]
enum MusicOperation { Play, Pause, Next }
pub(crate) fn good_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.starts_with('.')
        && !s.contains(['/', '\\'])
        && !s.chars().any(char::is_control)
        && s.rsplit('.').next().is_some_and(|e| {
            ["mp3", "wav", "flac", "ogg", "m4a", "aac"].contains(&e.to_ascii_lowercase().as_str())
        })
}
fn good_key(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub key: String,
    pub name: String,
    pub frames: u64,
    #[serde(default)]
    pub streaming: bool,
}
impl Track {
    fn limit(&self) -> u64 { if self.streaming { MAX_FRAMES } else { self.frames } }
    fn valid(&self) -> bool {
        good_key(&self.key) && good_name(&self.name) && (1..=MAX_FRAMES).contains(&self.frames)
    }
}
/// Saved reason why playback is not active. Legacy selections are ambiguous:
/// preserve a possible explicit pause rather than silently start them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RestingState { Stopped, #[default] Paused, Failed }
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Selection {
    incarnation: u64,
    key: String,
    generation: u64,
    position: u64,
    #[serde(default)]
    resting: RestingState,
    #[serde(skip)]
    playing: bool,
    #[serde(skip)]
    started: u64,
    #[serde(skip)]
    failure: Option<AudioPlaybackFailure>,
    #[serde(skip)]
    consumed: Option<u64>,
    #[serde(default)]
    ended: bool,
}
impl Selection {
    fn position(&self, now: u64, max: u64) -> u64 {
        if let Some(consumed) = self.consumed { return consumed.min(MAX_FRAMES); }
        self.position
            .saturating_add(if self.playing {
                now.saturating_sub(self.started)
                    .saturating_mul(SAMPLE_RATE as u64)
                    / 1000
            } else {
                0
            })
            .min(max)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalMusicContent {
    tracks: BTreeMap<String, Track>,
    devices: BTreeMap<String, Selection>,
    next_generation: u64,
    #[serde(skip)]
    imports: Vec<String>,
    #[serde(skip)]
    pending: Option<u64>,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    scheduled: bool,
    #[serde(skip)]
    scan_blocked: bool,
    #[serde(skip)]
    clock: u64,
}
impl Default for LocalMusicContent {
    fn default() -> Self {
        Self {
            tracks: BTreeMap::new(),
            devices: BTreeMap::new(),
            next_generation: 1,
            imports: vec![],
            pending: None,
            error: None,
            scheduled: false,
            scan_blocked: false,
            clock: 0,
        }
    }
}
pub struct LocalMusicPlugin;
impl Plugin for LocalMusicPlugin {
    fn id(&self) -> &str {
        "local-music"
    }
    fn supports(&self, c: &BTreeSet<Capability>) -> bool {
        c.contains("lcd") || c.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        query: &'a dyn WorldQuery,
        event: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        // Engine validates the PluginInput session before consulting these rules.
        // RefreshView is for Sub mode; music keeps normal channel navigation.
        let candidate = match event {
            Event::PluginInput { token, name }
                if [TOGGLE_PLAYBACK, NEXT_TRACK].contains(&name.as_str())
                    && query.device(&token.device_id).is_some_and(|d| {
                        d.selected.as_deref() == Some(self.id()) && d.mode == Mode::Normal
                    }) =>
            {
                Some(Candidate::new(
                    query,
                    &token.device_id,
                    Priority::User,
                    "music-control",
                    Transition::PluginDataChanged {
                        device_id: token.device_id.clone(),
                        incarnation: token.incarnation,
                        plugin: self.id().into(),
                    },
                ))
            }
            _ => None,
        };
        Box::new(candidate.into_iter())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Work {
    Scan,
    Prepare {
        name: String,
        generation: u64,
    },
    Delete {
        name: String,
        key: Option<String>,
        keep_cache: bool,
    },
}
fn request(slot: &str, w: Work) -> JobRequest {
    JobRequest {
        slot: slot.into(),
        payload: serde_json::to_value(w).unwrap(),
        timeout_seconds: 900,
    }
}
impl LocalMusicContent {
    fn generation(&mut self) -> Result<u64, ContentError> {
        let n = self.next_generation;
        self.next_generation = n.checked_add(1).ok_or(ContentError::Unavailable)?;
        Ok(n)
    }
    fn plan_control(&mut self, ctx: &PluginContext, control: MusicControl) -> Result<PluginPlan, ContentError> {
        if ctx.mode != Mode::Normal { return Err(ContentError::Unsupported); }
        let keys: Vec<_> = self.tracks.keys().cloned().collect();
        if keys.is_empty() { return Err(ContentError::Unavailable); }
        let current = self.devices.get(&ctx.token.device_id)
            .filter(|s| s.incarnation == ctx.token.incarnation && self.tracks.contains_key(&s.key)).cloned();
        let limit = current.as_ref().and_then(|s| self.tracks.get(&s.key)).map(|t| t.limit());
        let position = current.as_ref().zip(limit).map(|(s, max)| s.position(self.clock, max));
        let playing = current.as_ref().zip(position.zip(limit))
            .is_some_and(|(s, (position, max))| s.playing && !s.ended && position < max);
        let (operation, name) = match control {
            MusicControl::Toggle => {
                if current.is_none() { return Err(ContentError::Unavailable); }
                (if playing { MusicOperation::Pause } else { MusicOperation::Play }, TOGGLE_PLAYBACK)
            }
            MusicControl::Next => (MusicOperation::Next, NEXT_TRACK),
        };
        let mut next = current.clone().unwrap_or(Selection {
            incarnation: ctx.token.incarnation, key: keys[0].clone(), generation: 0,
            position: 0, resting: RestingState::Stopped, playing: false, started: self.clock, failure: None, consumed: None, ended: false,
        });
        let changed = match operation {
            MusicOperation::Play if playing => false,
            MusicOperation::Play => {
                next.position = position.filter(|_| !next.ended).filter(|p| self.tracks.get(&next.key).is_some_and(|t| *p < t.limit())).unwrap_or(0);
                next.playing = true;
                next.resting = RestingState::Stopped;
                true
            }
            MusicOperation::Pause if !playing => false,
            MusicOperation::Pause => {
                next.position = position.unwrap(); next.playing = false; next.resting = RestingState::Paused; true
            }
            MusicOperation::Next => {
                let index = current.as_ref().and_then(|s| keys.iter().position(|key| key == &s.key));
                let target = index.map(|i| (i + 1) % keys.len()).unwrap_or(0);
                if current.as_ref().is_some_and(|s| s.key == keys[target]) { false }
                else { next.key = keys[target].clone(); next.position = 0; next.playing = playing;
                    if next.resting == RestingState::Failed { next.resting = RestingState::Stopped; }
                    true }
            }
        };
        if changed {
            next.failure = None;
            next.ended = false;
            next.generation = self.generation()?;
            next.started = self.clock;
            next.consumed = self.tracks[&next.key].streaming.then_some(next.position);
            self.devices.insert(ctx.token.device_id.clone(), next);
        }
        Ok(PluginPlan {
            event: changed.then(|| Event::PluginInput { token: ctx.token.clone(), name: name.into() }),
            frame: self.current_frame(ctx),
            position: PluginPosition { plugin: self.id().into(), dataset_id: "music".into(), target_date: String::new(), index: 0 },
        })
    }
    fn spec(&self, ctx: &PluginContext) -> Option<AudioPlaybackSpec> {
        let s = self.devices.get(&ctx.token.device_id)?;
        let t = self.tracks.get(&s.key)?;
        if s.incarnation != ctx.token.incarnation
            || !s.playing || s.ended
            || s.position >= t.limit()
            || ctx.mode != crate::Mode::Normal
        {
            return None;
        }
        Some(AudioPlaybackSpec {
            asset_id: t.key.clone(),
            generation: s.generation,
            sample_rate: SAMPLE_RATE,
            channels: 2,
            frame_count: (!t.streaming).then_some(t.frames),
            start_frame: s.position,
            started_mono_ms: s.started,
            source: json!(t),
        })
    }
}
impl ContentPlugin for LocalMusicContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "local-music"
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
    fn plan(&mut self, ctx: &PluginContext, action: ButtonAction) -> Result<PluginPlan, ContentError> {
        let control = match action {
            ButtonAction::Sub => MusicControl::Next,
            ButtonAction::Media(MediaOperation::TogglePlayback) => MusicControl::Toggle,
            _ => return Err(ContentError::Unsupported),
        };
        self.plan_control(ctx, control)
    }
    fn plan_legacy_sub(&mut self, ctx: &PluginContext) -> Result<PluginPlan, ContentError> {
        self.plan_control(ctx, MusicControl::Toggle)
    }
    fn button_work(&self, _: &PluginContext, _: ButtonAction) -> Option<PluginWorkPlan> {
        // Persist selection/position and fence stale Web commands after accepted input.
        Some(PluginWorkPlan::default())
    }
    fn navigation_changed(&mut self, ctx: &PluginContext, active: bool) -> Result<bool, ContentError> {
        let Some(current) = self.devices.get(&ctx.token.device_id)
            .filter(|s| s.incarnation == ctx.token.incarnation) else { return Ok(false); };
        let Some(track) = self.tracks.get(&current.key) else { return Ok(false); };
        let position = current.position(self.clock, track.limit());
        let mut next = current.clone();
        if active {
            if ctx.mode != Mode::Normal || next.resting != RestingState::Stopped
                || (next.playing && !next.ended && position < track.limit()) { return Ok(false); }
            next.position = if next.ended || position >= track.limit() { 0 } else { position };
            next.playing = true;
            next.ended = false;
            next.started = self.clock;
            next.consumed = track.streaming.then_some(next.position);
        } else {
            if !next.playing { return Ok(false); }
            next.position = if next.ended || position >= track.limit() { 0 } else { position };
            next.playing = false;
            next.ended = false;
            next.consumed = None;
        }
        next.generation = self.generation()?;
        self.devices.insert(ctx.token.device_id.clone(), next);
        Ok(true)
    }
    fn session_reset(&mut self, id: &str, inc: u64) {
        if let Some(s) = self.devices.get_mut(id) {
            if s.incarnation != inc {
                self.devices.remove(id);
            } else {
                if let Some(consumed)=s.consumed { s.position=consumed; }
                s.playing = false;
            }
        }
    }
    fn delete_device(&mut self, id: &str) {
        self.devices.remove(id);
    }
    fn observe_monotonic_time(&mut self, now: u64) { self.clock = now; }
    fn observe_time(&mut self, _: &PluginContext, now: i64) {
        let _ = now;
    }
    fn current_frame(&self, ctx: &PluginContext) -> Option<Vec<u8>> {
        let s = self
            .devices
            .get(&ctx.token.device_id)
            .filter(|s| s.incarnation == ctx.token.incarnation);
        let text = s
            .and_then(|s| self.tracks.get(&s.key))
            .map(|t| t.name.as_str())
            .unwrap_or("NO MUSIC");
        Some(crate::music_text::lcd(
            if s.is_some_and(|s| s.resting == RestingState::Failed) {
                "MUSIC FAILED"
            } else if s.is_some_and(|s| s.playing && !s.ended) {
                "MUSIC REQUESTED"
            } else if s.is_some_and(|s| s.resting == RestingState::Paused) {
                "MUSIC PAUSED"
            } else {
                "MUSIC STOPPED"
            },
            text,
        ))
    }
    fn eink_frame_for(&self, ctx: &PluginContext, _: &[u8]) -> Option<Vec<u8>> {
        let selection = self.devices.get(&ctx.token.device_id).filter(|s| s.incarnation == ctx.token.incarnation);
        let name = selection.and_then(|s| self.tracks.get(&s.key)).map(|t| t.name.as_str()).unwrap_or("NO MUSIC");
        Some(crate::music_text::eink(if selection.is_some_and(|s|s.resting == RestingState::Failed) {"MUSIC FAILED"} else if selection.is_some_and(|s|s.playing&&!s.ended) {"MUSIC REQUESTED"} else if selection.is_some_and(|s|s.resting == RestingState::Paused) {"MUSIC PAUSED"} else {"MUSIC STOPPED"}, name))
    }
    fn audio_playback(&self, ctx: &PluginContext) -> Option<AudioPlaybackSpec> {
        self.spec(ctx)
    }
    fn audio_progress(&mut self, ctx: &PluginContext, spec: &AudioPlaybackSpec, consumed: u64, ended: bool) {
        if self.spec(ctx).as_ref() != Some(spec) { return; }
        if let Some(s) = self.devices.get_mut(&ctx.token.device_id) {
            if s.consumed.is_some_and(|old| consumed >= old) { s.consumed = Some(consumed.min(MAX_FRAMES)); s.ended = ended; }
        }
    }
    fn audio_failed(&mut self, ctx: &PluginContext, spec: &AudioPlaybackSpec, consumed_frame: u64,
        reason: AudioPlaybackFailure) -> Result<Option<PluginWorkPlan>, ContentError> {
        if self.spec(ctx).as_ref() != Some(spec) || consumed_frame < spec.start_frame { return Ok(None); }
        let generation = self.generation()?;
        let selection = self.devices.get_mut(&ctx.token.device_id).ok_or(ContentError::Stale)?;
        selection.consumed = None;
        selection.position = consumed_frame.min(self.tracks[&selection.key].limit());
        selection.playing = false;
        selection.resting = RestingState::Failed;
        selection.generation = generation;
        selection.failure = Some(reason);
        Ok(Some(PluginWorkPlan {
            refresh: RefreshTargets::Devices(vec![crate::DeviceToken {
                device_id: ctx.token.device_id.clone(), incarnation: ctx.token.incarnation,
            }]),
            ..Default::default()
        }))
    }
    fn save_state(&self) -> Value {
        json!(self)
    }
    fn load_state(&mut self, v: Value) -> Result<(), ContentError> {
        if v.is_null() {
            return Ok(());
        }
        let next: Self = serde_json::from_value(v).map_err(|_| ContentError::InvalidSource)?;
        if next.next_generation == 0
            || next.tracks.iter().any(|(k, t)| k != &t.key || !t.valid())
            || next
                .devices
                .values()
                .any(|s| !next.tracks.contains_key(&s.key) || s.generation >= next.next_generation)
        {
            return Err(ContentError::InvalidSource);
        }
        *self = next;
        Ok(())
    }
    fn web_state_at(&self, now: u64) -> Value {
        json!({"imports":self.imports,"tracks":self.tracks.values().collect::<Vec<_>>(),"devices":self.devices.iter().map(|(id,s)|{let max=self.tracks.get(&s.key).map(|t|t.limit()).unwrap_or(0);(id,json!({"key":s.key,"incarnation":s.incarnation,"resting":s.resting,"playing":s.playing&&!s.ended&&(s.consumed.is_some()||s.position(now,max)<max),"position_ms":s.position(now,max)*1000/SAMPLE_RATE as u64,"error":s.failure.map(|reason|json!({"code":reason,"message":failure_message(reason)}))}))}).collect::<BTreeMap<_,_>>(),"pending":self.pending.is_some(),"error":self.error})
    }
    fn plan_command(
        &mut self,
        ctx: &CommandContext,
        c: &Value,
    ) -> Result<PluginWorkPlan, ContentError> {
        self.clock = ctx.now_mono_ms;
        let mut p = PluginWorkPlan::default();
        match c["kind"].as_str().ok_or(ContentError::InvalidSource)? {
            "scan" => { self.scan_blocked=false;self.scheduled=true;p.jobs.push(request("scan", Work::Scan)); },
            "prepare" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let name = c["name"]
                    .as_str()
                    .filter(|n| good_name(n) && self.imports.iter().any(|i| i == n))
                    .ok_or(ContentError::InvalidSource)?;
                let generation = self.generation()?;
                self.pending = Some(generation);
                self.error = None;
                p.jobs.push(request(
                    "prepare",
                    Work::Prepare {
                        name: name.into(),
                        generation,
                    },
                ));
            }
            "cancel" => {
                p.cancel_slots.push("prepare".into());
                self.pending = None;
            }
            "play" | "pause" | "stop" => {
                let id = c["device_id"].as_str().ok_or(ContentError::InvalidSource)?;
                let d = ctx
                    .devices
                    .iter()
                    .find(|d| d.context.token.device_id == id)
                    .ok_or(ContentError::InvalidSource)?;
                let gen = self.generation()?;
                match c["kind"].as_str().unwrap() {
                    "play" => {
                        let key = c["key"].as_str().ok_or(ContentError::InvalidSource)?;
                        let t = self.tracks.get(key).ok_or(ContentError::InvalidSource)?;
                        let pos = self
                            .devices
                            .get(id)
                            .filter(|s| {
                                s.incarnation == d.context.token.incarnation
                                    && s.key == key
                                    && !s.playing && !s.ended
                                    && s.position < t.limit()
                            })
                            .map(|s| s.position)
                            .unwrap_or(0);
                        self.devices.insert(
                            id.into(),
                            Selection {
                                incarnation: d.context.token.incarnation,
                                key: key.into(),
                                generation: gen,
                                position: pos,
                                resting: RestingState::Stopped,
                                playing: true,
                                started: ctx.now_mono_ms,
                                failure: None, consumed: t.streaming.then_some(pos), ended: false,
                            },
                        );
                    }
                    kind => {
                        let s = self
                            .devices
                            .get_mut(id)
                            .filter(|s| s.incarnation == d.context.token.incarnation)
                            .ok_or(ContentError::Unavailable)?;
                        s.position = if kind == "stop" {
                            0
                        } else {
                            s.position(ctx.now_mono_ms, self.tracks[&s.key].limit())
                        };
                        s.playing = false;
                        s.resting = if kind == "stop" { RestingState::Stopped } else { RestingState::Paused };
                        s.consumed = None;
                        s.generation = gen;
                        s.failure = None;
                    }
                }
                p.refresh = RefreshTargets::Selected;
            }
            "delete" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let name = c["name"]
                    .as_str()
                    .filter(|n| good_name(n) && c["confirmation"].as_str() == Some(n))
                    .ok_or(ContentError::InvalidSource)?;
                let key = self
                    .tracks
                    .values()
                    .find(|t| t.name == name)
                    .map(|t| t.key.clone());
                if !self.imports.iter().any(|n| n == name) && key.is_none() {
                    return Err(ContentError::InvalidSource);
                }
                self.scan_blocked=true;
                p.cancel_slots.push("scan".into());
                if let Some(ref key) = key {
                    for selection in self.devices.values_mut().filter(|s| &s.key == key) { selection.playing = false; selection.resting = RestingState::Paused; }
                    p.refresh = RefreshTargets::Selected;
                }
                p.jobs.push(request(
                    "delete",
                    Work::Delete {
                        name: name.into(),
                        key,
                        keep_cache: false,
                    },
                ));
            }
            _ => return Err(ContentError::Unsupported),
        }
        Ok(p)
    }
    fn plan_schedule(
        &mut self,
        ctx: &CommandContext,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        self.clock = ctx.now_mono_ms;
        if self.scheduled || self.scan_blocked {
            return Ok(None);
        }
        self.scheduled = true;
        Ok(Some(PluginWorkPlan {
            jobs: vec![request("scan", Work::Scan)],
            ..Default::default()
        }))
    }
    fn work_completed(
        &mut self,
        _: &CommandContext,
        job: &PluginJob,
        outcome: &JobOutcome,
    ) -> Result<PluginWorkPlan, ContentError> {
        let w: Work =
            serde_json::from_value(job.payload.clone()).map_err(|_| ContentError::InvalidSource)?;
        let mut p = PluginWorkPlan::default();
        match w {
            Work::Scan => {
                if self.scan_blocked { return Ok(p); }
                if let JobOutcome::Succeeded(v) = outcome {
                    let names: Vec<String> = serde_json::from_value(if v.is_array() { v.clone() } else { v["imports"].clone() })
                        .map_err(|_| ContentError::InvalidSource)?;
                    if names.iter().any(|n| !good_name(n)) {
                        return Err(ContentError::InvalidSource);
                    }
                    self.imports = names;
                    if let Some(values) = v["tracks"].as_array() {
                        for value in values {
                            let t: Track = serde_json::from_value(value.clone()).map_err(|_| ContentError::InvalidSource)?;
                            if !t.valid() { return Err(ContentError::InvalidSource); }
                            let active = self.devices.values().any(|s| s.key == t.key && s.playing);
                            if !active {
                                if let Some(old) = self.tracks.get(&t.key).filter(|old| !old.streaming && t.streaming) {
                                    for selection in self.devices.values_mut().filter(|s| s.key == t.key && !s.playing && s.position >= old.frames) {
                                        selection.position=0;selection.consumed=None;selection.ended=false;
                                    }
                                }
                                self.tracks.insert(t.key.clone(), t);
                            }
                        }
                    }
                    self.error = v["errors"].as_array().filter(|e| !e.is_empty()).map(|e| e.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"));
                }
            }
            Work::Prepare { generation, .. } => {
                if self.pending != Some(generation) {
                    return Ok(p);
                }
                self.pending = None;
                match outcome {
                    JobOutcome::Succeeded(v) => {
                        let t: Track = serde_json::from_value(v.clone())
                            .map_err(|_| ContentError::InvalidSource)?;
                        if !t.valid() {
                            return Err(ContentError::InvalidSource);
                        }
                        self.tracks.insert(t.key.clone(), t);
                        self.error = None;
                    }
                    JobOutcome::Failed(e) => self.error = Some(e.message.clone()),
                    JobOutcome::Cancelled => {}
                }
            }
            Work::Delete { name, key, .. } => match outcome {
                JobOutcome::Succeeded(_) => {
                    self.scan_blocked=true;
                    p.cancel_slots.push("scan".into());
                    p.cancel_slots.push("prepare".into());
                    self.pending=None;
                    self.imports.retain(|n| n != &name);
                    let mut removed: BTreeSet<String> = self.tracks.values().filter(|t| t.name == name).map(|t| t.key.clone()).collect();
                    removed.extend(key);
                    self.tracks.retain(|key, _| !removed.contains(key));
                    self.devices.retain(|_, s| !removed.contains(&s.key));
                    if !removed.is_empty() { p.refresh = RefreshTargets::Selected; }
                }
                JobOutcome::Failed(e) => self.error = Some(e.message.clone()),
                _ => {}
            },
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn ctx() -> CommandContext {
        CommandContext {
            now_unix: 0,
            now_mono_ms: 100,
            today_jst: "2026-11-07".into(),
            settings: Default::default(),
            devices: ["a", "b"]
                .into_iter()
                .map(|id| PluginDevice {
                    context: PluginContext {
                        token: crate::SessionToken {
                            device_id: id.into(),
                            incarnation: 1,
                            session_epoch: 1,
                        },
                        settings_revision: 1,
                        mode: crate::Mode::Normal,
                    },
                    selected: Some("local-music".into()),
                    online: true,
                    capabilities: BTreeSet::from(["lcd".into()]),
                })
                .collect(),
        }
    }
    #[test]
    fn independent_devices_and_no_restart_autoplay() {
        let mut c = LocalMusicContent::default();
        let x = ctx();
        let key = "a".repeat(64);
        c.tracks.insert(
            key.clone(),
            Track {
                key: key.clone(),
                name: "one.wav".into(),
                frames: 480000, streaming: false,
            },
        );
        for id in ["a", "b"] {
            c.plan_command(&x, &json!({"kind":"play","device_id":id,"key":key}))
                .unwrap();
        }
        c.plan_command(&x, &json!({"kind":"pause","device_id":"a"}))
            .unwrap();
        assert!(c.spec(&x.devices[0].context).is_none());
        assert!(c.spec(&x.devices[1].context).is_some());
        let mut restored = LocalMusicContent::default();
        restored.load_state(c.save_state()).unwrap();
        assert!(restored.spec(&x.devices[1].context).is_none());
        c.session_reset("b", 2);
        assert!(c.devices.get("b").is_none());
    }
    #[test]
    fn cancelled_prepare_cannot_reappear() {
        let mut c = LocalMusicContent::default(); let x = ctx(); c.imports.push("x.wav".into());
        let p = c.plan_command(&x, &json!({"kind":"prepare","name":"x.wav"})).unwrap();
        c.plan_command(&x, &json!({"kind":"cancel"})).unwrap();
        let job = PluginJob { token: JobToken {plugin:"local-music".into(),slot:"prepare".into(),generation:1},payload:p.jobs[0].payload.clone(),timeout_seconds:900,created_unix:0 };
        c.work_completed(&x,&job,&JobOutcome::Succeeded(json!(Track {key:"a".repeat(64),name:"x.wav".into(),frames:4800,streaming:false}))).unwrap();
        assert!(c.tracks.is_empty());
    }
    #[test]
    fn missing_track_selection_is_safe_and_invalid_restore_is_rejected() {
        let mut c = LocalMusicContent::default();
        let x = ctx();
        c.devices.insert("a".into(), Selection { incarnation:1, key:"b".repeat(64), generation:1, position:123, resting:RestingState::Stopped, playing:true, started:0, failure:None, consumed:None, ended:false });
        c.next_generation=2;
        assert!(c.plan(&x.devices[0].context, ButtonAction::Sub).is_err());
        c.tracks.insert("a".repeat(64), Track {key:"a".repeat(64),name:"valid.wav".into(),frames:48000,streaming:false});
        let invalid=c.save_state();
        assert!(matches!(LocalMusicContent::default().load_state(invalid),Err(ContentError::InvalidSource)));
        c.plan(&x.devices[0].context, ButtonAction::Sub).unwrap();
        assert_eq!(c.devices["a"].key,"a".repeat(64));
        assert!(!c.devices["a"].playing);
        assert_eq!(c.devices["a"].position,0);
    }
    #[test]
    fn invalid_source_and_unknown_device_rejected() {
        for n in ["../a.wav", ".x.mp3", "a.m3u", "a/b.wav", "a\\b.wav"] {
            assert!(!good_name(n));
        }
        let mut c = LocalMusicContent::default();
        assert!(c
            .plan_command(
                &ctx(),
                &json!({"kind":"play","device_id":"unknown","key":"a".repeat(64)})
            )
            .is_err());
    }
}

fn failure_message(reason: AudioPlaybackFailure) -> &'static str {
    match reason {
        AudioPlaybackFailure::DeviceFailed => "端末が音声再生の失敗を報告しました。",
        AudioPlaybackFailure::InvalidCredit => "端末の音声消費情報が不正なため停止しました。",
        AudioPlaybackFailure::CreditTimeout => "端末から音声消費の進捗が届かず停止しました。",
        AudioPlaybackFailure::SourceReadFailed => "音源データを読み込めず停止しました。",
        AudioPlaybackFailure::SbcReadyTimeout => "端末の音声形式の確認が時間内に完了しませんでした。",
        AudioPlaybackFailure::UnsupportedFormat => "対応していない音声形式のため停止しました。",
        AudioPlaybackFailure::CodecMismatch => "端末の音声形式が一致しないため停止しました。",
        AudioPlaybackFailure::EncodeFailed => "音声の圧縮処理に失敗したため停止しました。",
        AudioPlaybackFailure::SourceReadTimeout => "音源データの読み込みが時間内に完了しませんでした。",
    }
}

#[cfg(test)]
mod streaming_state_tests {
    use super::*;
    #[test]
    fn automatic_scan_registers_tracks_preserves_active_identity_and_pcm_only_legacy() {
        let mut c=LocalMusicContent::default();let x=super::tests::ctx();let key="a".repeat(64);let legacy="b".repeat(64);
        c.tracks.insert(legacy.clone(),Track{key:legacy.clone(),name:"legacy.wav".into(),frames:4800,streaming:false});
        let job=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"scan".into(),generation:1},payload:json!({"kind":"scan"}),timeout_seconds:900,created_unix:0};
        let result=json!({"imports":["tone.wav"],"tracks":[{"key":key,"name":"tone.wav","frames":4800,"streaming":true}],"errors":[]});
        c.work_completed(&x,&job,&JobOutcome::Succeeded(result.clone())).unwrap();
        assert!(c.tracks[&key].streaming);assert!(!c.tracks[&legacy].streaming);assert!(c.pending.is_none());
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx=&x.devices[0].context;let before=c.spec(ctx).unwrap();let mut changed=result;
        changed["tracks"][0]["frames"]=json!(9600);
        c.work_completed(&x,&job,&JobOutcome::Succeeded(changed)).unwrap();
        assert_eq!(c.spec(ctx),Some(before));assert!(c.tracks.contains_key(&legacy));
        let mut restored=LocalMusicContent::default();restored.load_state(c.save_state()).unwrap();assert!(restored.spec(ctx).is_none());
    }
    #[test]
    fn deletion_fences_late_scan_and_only_explicit_rescan_reopens_registration() {
        let mut c=LocalMusicContent::default();let x=super::tests::ctx();let key="a".repeat(64);
        let result=json!({"imports":["tone.wav"],"tracks":[{"key":key,"name":"tone.wav","frames":4800,"streaming":true}],"errors":[]});
        let job=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"scan".into(),generation:1},payload:json!({"kind":"scan"}),timeout_seconds:900,created_unix:0};
        c.work_completed(&x,&job,&JobOutcome::Succeeded(result.clone())).unwrap();
        let deletion=c.plan_command(&x,&json!({"kind":"delete","name":"tone.wav","confirmation":"tone.wav"})).unwrap();
        assert!(deletion.cancel_slots.iter().any(|s|s=="scan"));
        assert!(c.plan_schedule(&x).unwrap().is_none());
        let deleted=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"delete".into(),generation:1},payload:deletion.jobs[0].payload.clone(),timeout_seconds:900,created_unix:0};
        c.plan_command(&x,&json!({"kind":"scan"})).unwrap();
        let completion=c.work_completed(&x,&deleted,&JobOutcome::Succeeded(Value::Null)).unwrap();
        assert!(completion.cancel_slots.iter().any(|s|s=="scan"));
        c.work_completed(&x,&job,&JobOutcome::Succeeded(result.clone())).unwrap();
        assert!(c.tracks.is_empty());assert!(c.imports.is_empty());
        c.plan_command(&x,&json!({"kind":"scan"})).unwrap();
        // Aggregate job generation fencing occurs before this content callback.
        let mut fresh=job.clone();fresh.token.generation=2;
        c.work_completed(&x,&fresh,&JobOutcome::Succeeded(result)).unwrap();
        assert!(c.tracks.contains_key(&key));
    }
    #[test]
    fn delete_completion_removes_name_registered_while_deleting_and_retains_other_tracks() {
        for had_key in [false,true] {
            let mut c=LocalMusicContent::default();let x=super::tests::ctx();let old="a".repeat(64);let new="b".repeat(64);let other="c".repeat(64);
            c.imports.push("tone.wav".into());
            if had_key { c.tracks.insert(old.clone(),Track{key:old.clone(),name:"tone.wav".into(),frames:4800,streaming:true}); }
            c.tracks.insert(other.clone(),Track{key:other.clone(),name:"other.wav".into(),frames:4800,streaming:true});
            let plan=c.plan_command(&x,&json!({"kind":"delete","name":"tone.wav","confirmation":"tone.wav"})).unwrap();
            c.plan_command(&x,&json!({"kind":"scan"})).unwrap();
            let scan=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"scan".into(),generation:1},payload:json!({"kind":"scan"}),timeout_seconds:900,created_unix:0};
            let result=json!({"imports":["tone.wav"],"tracks":[{"key":new,"name":"tone.wav","frames":4800,"streaming":true}],"errors":[]});
            c.work_completed(&x,&scan,&JobOutcome::Succeeded(result)).unwrap();
            c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":new})).unwrap();
            let deleted=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"delete".into(),generation:1},payload:plan.jobs[0].payload.clone(),timeout_seconds:900,created_unix:0};
            c.work_completed(&x,&deleted,&JobOutcome::Succeeded(Value::Null)).unwrap();
            assert!(!c.tracks.contains_key(&old));assert!(!c.tracks.contains_key(&new));assert!(c.devices.is_empty());assert!(c.tracks.contains_key(&other));
        }
    }
    #[test]
    fn compatibility_prepare_cannot_resurrect_a_deleted_name() {
        let mut c=LocalMusicContent::default();let x=super::tests::ctx();let key="a".repeat(64);c.imports.push("tone.wav".into());
        let deletion=c.plan_command(&x,&json!({"kind":"delete","name":"tone.wav","confirmation":"tone.wav"})).unwrap();
        let preparation=c.plan_command(&x,&json!({"kind":"prepare","name":"tone.wav"})).unwrap();
        let deleted=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"delete".into(),generation:1},payload:deletion.jobs[0].payload.clone(),timeout_seconds:900,created_unix:0};
        let plan=c.work_completed(&x,&deleted,&JobOutcome::Succeeded(Value::Null)).unwrap();
        assert!(plan.cancel_slots.iter().any(|s|s=="prepare"));assert!(c.pending.is_none());
        let prepared=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"prepare".into(),generation:1},payload:preparation.jobs[0].payload.clone(),timeout_seconds:900,created_unix:0};
        c.work_completed(&x,&prepared,&JobOutcome::Succeeded(json!({"key":key,"name":"tone.wav","frames":4800,"streaming":true}))).unwrap();
        assert!(c.tracks.is_empty());assert!(c.imports.is_empty());
    }
    #[test]
    fn legacy_eof_migration_replays_from_zero_after_reload_without_rewinding_partial_pause() {
        for position in [512,4800] {
            let mut c=LocalMusicContent::default();let x=super::tests::ctx();let key="a".repeat(64);
            c.load_state(json!({"tracks":{key.clone():{"key":key,"name":"tone.wav","frames":4800}},"devices":{"a":{"incarnation":1,"key":key,"generation":1,"position":position}},"next_generation":2})).unwrap();
            let scan=PluginJob{token:JobToken{plugin:"local-music".into(),slot:"scan".into(),generation:1},payload:json!({"kind":"scan"}),timeout_seconds:900,created_unix:0};
            c.work_completed(&x,&scan,&JobOutcome::Succeeded(json!({"imports":["tone.wav"],"tracks":[{"key":key,"name":"tone.wav","frames":5000,"streaming":true}],"errors":[]}))).unwrap();
            assert!(c.spec(&x.devices[0].context).is_none());
            let mut reloaded=LocalMusicContent::default();reloaded.load_state(c.save_state()).unwrap();
            reloaded.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
            assert_eq!(reloaded.spec(&x.devices[0].context).unwrap().start_frame,if position==4800 {0} else {512});
        }
    }
    #[test]
    fn streaming_pause_resume_and_actual_eof_exceed_metadata_without_rewind() {
        let mut c = LocalMusicContent::default();
        let x = super::tests::ctx();
        let key = "a".repeat(64);
        c.tracks.insert(key.clone(), Track {
            key: key.clone(), name: "tone.wav".into(), frames: 4800, streaming: true,
        });
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx = &x.devices[0].context;
        let spec = c.spec(ctx).unwrap();
        c.audio_progress(ctx, &spec, 6000, false);
        for _ in 0..2 {
            c.plan_command(&x, &json!({"kind":"pause","device_id":"a"})).unwrap();
            assert_eq!(c.devices["a"].position, 6000);
            assert_eq!(c.web_state_at(x.now_mono_ms)["devices"]["a"]["position_ms"], 125);
        }
        let mut reloaded = LocalMusicContent::default();
        reloaded.load_state(c.save_state()).unwrap();
        assert_eq!(reloaded.web_state_at(x.now_mono_ms)["devices"]["a"]["position_ms"], 125);
        reloaded.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let resumed = reloaded.spec(ctx).unwrap();
        assert_eq!(resumed.start_frame, 6000);
        reloaded.audio_progress(ctx, &resumed, 6500, true);
        assert!(reloaded.spec(ctx).is_none());
        for _ in 0..2 {
            reloaded.plan_command(&x, &json!({"kind":"pause","device_id":"a"})).unwrap();
            assert_eq!(reloaded.devices["a"].position, 6500);
            assert_eq!(reloaded.web_state_at(x.now_mono_ms)["devices"]["a"]["position_ms"], 135);
        }
        reloaded.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        assert_eq!(reloaded.spec(ctx).unwrap().start_frame, 0);
    }
    #[test]
    fn legacy_pause_and_web_position_keep_exact_pcm_limit() {
        let mut c = LocalMusicContent::default();
        let mut x = super::tests::ctx();
        let key = "a".repeat(64);
        c.tracks.insert(key.clone(), Track {
            key: key.clone(), name: "legacy.wav".into(), frames: 4800, streaming: false,
        });
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        x.now_mono_ms += 125;
        assert_eq!(c.web_state_at(x.now_mono_ms)["devices"]["a"]["position_ms"], 100);
        for _ in 0..2 {
            c.plan_command(&x, &json!({"kind":"pause","device_id":"a"})).unwrap();
            assert_eq!(c.devices["a"].position, 4800);
        }
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let spec = c.spec(&x.devices[0].context).unwrap();
        assert_eq!(spec.start_frame, 0);
        assert_eq!(spec.frame_count, Some(4800));
    }
    #[test]
    fn startup_pause_consumed_position_stale_progress_and_eof() {
        let mut c=LocalMusicContent::default();let mut x=super::tests::ctx();let key="a".repeat(64);
        c.tracks.insert(key.clone(),Track{key:key.clone(),name:"tone.wav".into(),frames:4800,streaming:true});
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx=&x.devices[0].context;let original=c.spec(ctx).unwrap();assert_eq!(original.frame_count,None);
        x.now_mono_ms+=9000;
        assert_eq!(c.web_state_at(x.now_mono_ms)["devices"]["a"]["position_ms"],0);
        c.plan_command(&x,&json!({"kind":"pause","device_id":"a"})).unwrap();assert_eq!(c.devices["a"].position,0);
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx=&x.devices[0].context;let current=c.spec(ctx).unwrap();assert_eq!(current.start_frame,0);
        c.audio_progress(ctx,&original,2400,false);assert_eq!(c.devices["a"].consumed,Some(0));
        c.audio_progress(ctx,&current,512,false);assert_eq!(c.spec(ctx),Some(current.clone()));
        c.audio_progress(ctx,&current,0,false);assert_eq!(c.devices["a"].consumed,Some(512));
        c.session_reset("a",1);assert_eq!(c.devices["a"].position,512);
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx=&x.devices[0].context;let current=c.spec(ctx).unwrap();assert_eq!(current.start_frame,512);
        c.plan_command(&x,&json!({"kind":"pause","device_id":"a"})).unwrap();assert_eq!(c.devices["a"].position,512);
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let ctx=&x.devices[0].context;let current=c.spec(ctx).unwrap();assert_eq!(current.start_frame,512);
        c.audio_progress(ctx,&current,4500,true);assert!(c.spec(ctx).is_none());
        assert_eq!(c.web_state_at(x.now_mono_ms)["devices"]["a"]["playing"],false);
        c.plan_control(ctx,MusicControl::Toggle).unwrap();assert_eq!(c.spec(ctx).unwrap().start_frame,0);
        let current=c.spec(ctx).unwrap();c.audio_failed(ctx,&current,6000,AudioPlaybackFailure::DeviceFailed).unwrap();
        assert_eq!(c.devices["a"].position,6000,"metadata duration does not rewind consumed position");
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();assert_eq!(c.spec(ctx).unwrap().start_frame,6000);
        let current=c.spec(ctx).unwrap();c.audio_progress(ctx,&current,6500,true);
        c.plan_command(&x,&json!({"kind":"pause","device_id":"a"})).unwrap();
        c.plan_command(&x,&json!({"kind":"play","device_id":"a","key":key})).unwrap();assert_eq!(c.spec(ctx).unwrap().start_frame,0);
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    fn fixture() -> (LocalMusicContent, CommandContext, String) {
        let mut c = LocalMusicContent::default();
        let key = "a".repeat(64);
        c.tracks.insert(key.clone(), Track { key: key.clone(), name: "one.wav".into(), frames: 4800, streaming: true });
        (c, super::tests::ctx(), key)
    }
    #[test]
    fn navigation_stopped_pause_failure_and_duplicate_entry_are_distinct() {
        let (mut c, x, key) = fixture(); let ctx = &x.devices[0].context;
        assert!(!c.navigation_changed(ctx, true).unwrap());
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let first = c.spec(ctx).unwrap();
        assert!(!c.navigation_changed(ctx, true).unwrap());
        assert_eq!(c.spec(ctx), Some(first.clone()));
        c.audio_progress(ctx, &first, 6400, false);
        assert!(c.navigation_changed(ctx, false).unwrap());
        assert_eq!(c.devices["a"].position, 6400, "do not clamp to estimated metadata duration");
        assert!(!c.navigation_changed(ctx, false).unwrap());
        assert!(c.navigation_changed(ctx, true).unwrap());
        let resumed = c.spec(ctx).unwrap();
        assert_eq!(resumed.start_frame, 6400);
        c.audio_progress(ctx, &first, 7000, false);
        assert_eq!(c.devices["a"].consumed, Some(6400), "old transfer is fenced");
        c.plan_control(ctx, MusicControl::Toggle).unwrap();
        assert_eq!(c.devices["a"].resting, RestingState::Paused);
        assert!(!c.navigation_changed(ctx, false).unwrap());
        assert!(!c.navigation_changed(ctx, true).unwrap());
        c.plan_control(ctx, MusicControl::Toggle).unwrap();
        let resumed = c.spec(ctx).unwrap();
        assert_eq!(resumed.start_frame, 6400);
        c.audio_failed(ctx, &resumed, 6500, AudioPlaybackFailure::DeviceFailed).unwrap();
        assert!(!c.navigation_changed(ctx, true).unwrap());
        let mut loaded = LocalMusicContent::default(); loaded.load_state(c.save_state()).unwrap();
        assert_eq!(loaded.devices["a"].resting, RestingState::Failed);
        assert!(!loaded.navigation_changed(ctx, true).unwrap());
        loaded.plan_command(&x, &json!({"kind":"stop","device_id":"a"})).unwrap();
        assert!(loaded.navigation_changed(ctx, true).unwrap());
        assert_eq!(loaded.spec(ctx).unwrap().start_frame, 0);
    }
    #[test]
    fn navigation_eof_restarts_only_on_entry_and_explicit_pause_still_wins() {
        let (mut c, x, key) = fixture(); let ctx = &x.devices[0].context;
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let spec = c.spec(ctx).unwrap(); c.audio_progress(ctx, &spec, 6000, true);
        assert!(c.spec(ctx).is_none(), "EOF does not loop");
        assert!(c.navigation_changed(ctx, false).unwrap());
        assert_eq!(c.devices["a"].position, 0);
        assert!(c.navigation_changed(ctx, true).unwrap());
        assert_eq!(c.spec(ctx).unwrap().start_frame, 0);
        let spec = c.spec(ctx).unwrap(); c.audio_progress(ctx, &spec, 6000, true);
        c.plan_command(&x, &json!({"kind":"pause","device_id":"a"})).unwrap();
        assert!(!c.navigation_changed(ctx, false).unwrap());
        assert!(!c.navigation_changed(ctx, true).unwrap());
        let mut reloaded = LocalMusicContent::default(); reloaded.load_state(c.save_state()).unwrap();
        assert!(!reloaded.navigation_changed(ctx, true).unwrap());
        reloaded.plan_control(ctx, MusicControl::Toggle).unwrap();
        assert_eq!(reloaded.spec(ctx).unwrap().start_frame, 0);
    }
    #[test]
    fn navigation_during_delete_does_not_reopen_the_source() {
        let (mut c, x, key) = fixture(); let ctx = &x.devices[0].context;
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        c.plan_command(&x, &json!({"kind":"delete","name":"one.wav","confirmation":"one.wav"})).unwrap();
        assert!(!c.navigation_changed(ctx, false).unwrap());
        assert!(!c.navigation_changed(ctx, true).unwrap());
        assert!(c.spec(ctx).is_none());
        assert!(c.tracks.contains_key(&key), "only a successful delete result removes metadata");
    }
    #[test]
    fn navigation_restore_migration_incarnation_and_generation_failure() {
        let (mut c, x, key) = fixture(); let ctx = &x.devices[0].context;
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        let mut saved = c.save_state();
        saved["devices"]["a"].as_object_mut().unwrap().remove("resting");
        let mut loaded = LocalMusicContent::default(); loaded.load_state(saved).unwrap();
        assert_eq!(loaded.devices["a"].resting, RestingState::Paused);
        assert!(loaded.spec(ctx).is_none());
        assert!(!loaded.navigation_changed(ctx, true).unwrap());
        loaded.plan_command(&x, &json!({"kind":"stop","device_id":"a"})).unwrap();
        let mut restored = LocalMusicContent::default(); restored.load_state(loaded.save_state()).unwrap();
        restored.session_reset("a", 1);
        assert!(restored.spec(ctx).is_none(), "load/Hello alone never plays");
        let mut stale = ctx.clone(); stale.token.incarnation += 1;
        assert!(!restored.navigation_changed(&stale, true).unwrap());
        let before = restored.save_state(); restored.next_generation = u64::MAX;
        assert!(restored.navigation_changed(ctx, true).is_err());
        assert_eq!(restored.save_state()["devices"], before["devices"]);
    }
    #[test]
    fn navigation_explicit_pause_survives_next_track_and_save_load() {
        let (mut c, x, key) = fixture(); let ctx = &x.devices[0].context;
        let b = "b".repeat(64); c.tracks.insert(b.clone(), Track { key: b.clone(), name: "two.wav".into(), frames: 4800, streaming: true });
        c.plan_command(&x, &json!({"kind":"play","device_id":"a","key":key})).unwrap();
        c.plan_command(&x, &json!({"kind":"pause","device_id":"a"})).unwrap();
        c.plan_control(ctx, MusicControl::Next).unwrap();
        assert_eq!(c.devices["a"].key, b);
        let mut loaded = LocalMusicContent::default(); loaded.load_state(c.save_state()).unwrap();
        assert_eq!(loaded.devices["a"].resting, RestingState::Paused);
        assert!(!loaded.navigation_changed(ctx, true).unwrap());
        loaded.plan_control(ctx, MusicControl::Toggle).unwrap();
        assert_eq!(loaded.spec(ctx).unwrap().asset_id, b);
    }
}
