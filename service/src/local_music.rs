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
}
impl Track {
    fn valid(&self) -> bool {
        good_key(&self.key) && good_name(&self.name) && (1..=MAX_FRAMES).contains(&self.frames)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Selection {
    incarnation: u64,
    key: String,
    generation: u64,
    position: u64,
    #[serde(skip)]
    playing: bool,
    #[serde(skip)]
    started: u64,
}
impl Selection {
    fn position(&self, now: u64, max: u64) -> u64 {
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
        let limit = current.as_ref().and_then(|s| self.tracks.get(&s.key)).map(|t| t.frames);
        let position = current.as_ref().zip(limit).map(|(s, max)| s.position(self.clock, max));
        let playing = current.as_ref().zip(position.zip(limit))
            .is_some_and(|(s, (position, max))| s.playing && position < max);
        let (operation, name) = match control {
            MusicControl::Toggle => {
                if current.is_none() { return Err(ContentError::Unavailable); }
                (if playing { MusicOperation::Pause } else { MusicOperation::Play }, TOGGLE_PLAYBACK)
            }
            MusicControl::Next => (MusicOperation::Next, NEXT_TRACK),
        };
        let mut next = current.clone().unwrap_or(Selection {
            incarnation: ctx.token.incarnation, key: keys[0].clone(), generation: 0,
            position: 0, playing: false, started: self.clock,
        });
        let changed = match operation {
            MusicOperation::Play if playing => false,
            MusicOperation::Play => {
                next.position = position.filter(|p| self.tracks.get(&next.key).is_some_and(|t| *p < t.frames)).unwrap_or(0);
                next.playing = true;
                true
            }
            MusicOperation::Pause if !playing => false,
            MusicOperation::Pause => {
                next.position = position.unwrap(); next.playing = false; true
            }
            MusicOperation::Next => {
                let index = current.as_ref().and_then(|s| keys.iter().position(|key| key == &s.key));
                let target = index.map(|i| (i + 1) % keys.len()).unwrap_or(0);
                if current.as_ref().is_some_and(|s| s.key == keys[target]) { false }
                else { next.key = keys[target].clone(); next.position = 0; next.playing = playing; true }
            }
        };
        if changed {
            next.generation = self.generation()?;
            next.started = self.clock;
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
            || !s.playing
            || s.position >= t.frames
            || ctx.mode != crate::Mode::Normal
        {
            return None;
        }
        Some(AudioPlaybackSpec {
            asset_id: t.key.clone(),
            generation: s.generation,
            sample_rate: SAMPLE_RATE,
            channels: 2,
            frame_count: Some(t.frames),
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
    fn session_reset(&mut self, id: &str, inc: u64) {
        if let Some(s) = self.devices.get_mut(id) {
            if s.incarnation != inc {
                self.devices.remove(id);
            } else {
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
            if s.is_some_and(|s| s.playing) {
                "MUSIC REQUESTED"
            } else {
                "MUSIC STOPPED"
            },
            text,
        ))
    }
    fn eink_frame_for(&self, ctx: &PluginContext, _: &[u8]) -> Option<Vec<u8>> {
        let selection = self.devices.get(&ctx.token.device_id).filter(|s| s.incarnation == ctx.token.incarnation);
        let name = selection.and_then(|s| self.tracks.get(&s.key)).map(|t| t.name.as_str()).unwrap_or("NO MUSIC");
        Some(crate::music_text::eink(if selection.is_some_and(|s|s.playing) {"MUSIC REQUESTED"} else {"MUSIC STOPPED"}, name))
    }
    fn audio_playback(&self, ctx: &PluginContext) -> Option<AudioPlaybackSpec> {
        self.spec(ctx)
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
        json!({"imports":self.imports,"tracks":self.tracks.values().collect::<Vec<_>>(),"devices":self.devices.iter().map(|(id,s)|{let max=self.tracks.get(&s.key).map(|t|t.frames).unwrap_or(0);(id,json!({"key":s.key,"incarnation":s.incarnation,"playing":s.playing&&s.position(now,max)<max,"position_ms":s.position(now,max)*1000/SAMPLE_RATE as u64}))}).collect::<BTreeMap<_,_>>(),"pending":self.pending.is_some(),"error":self.error})
    }
    fn plan_command(
        &mut self,
        ctx: &CommandContext,
        c: &Value,
    ) -> Result<PluginWorkPlan, ContentError> {
        self.clock = ctx.now_mono_ms;
        let mut p = PluginWorkPlan::default();
        match c["kind"].as_str().ok_or(ContentError::InvalidSource)? {
            "scan" => p.jobs.push(request("scan", Work::Scan)),
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
                                    && !s.playing
                                    && s.position < t.frames
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
                                playing: true,
                                started: ctx.now_mono_ms,
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
                            s.position(ctx.now_mono_ms, self.tracks[&s.key].frames)
                        };
                        s.playing = false;
                        s.generation = gen;
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
                if let Some(ref key) = key {
                    for selection in self.devices.values_mut().filter(|s| &s.key == key) { selection.playing = false; }
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
        if self.scheduled {
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
                if let JobOutcome::Succeeded(v) = outcome {
                    let names: Vec<String> = serde_json::from_value(v.clone())
                        .map_err(|_| ContentError::InvalidSource)?;
                    if names.iter().any(|n| !good_name(n)) {
                        return Err(ContentError::InvalidSource);
                    }
                    self.imports = names;
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
                    self.imports.retain(|n| n != &name);
                    if let Some(key) = key {
                        self.tracks.remove(&key);
                        self.devices.retain(|_, s| s.key != key);
                        p.refresh = RefreshTargets::Selected;
                    }
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
    fn ctx() -> CommandContext {
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
                frames: 480000,
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
        c.work_completed(&x,&job,&JobOutcome::Succeeded(json!(Track {key:"a".repeat(64),name:"x.wav".into(),frames:4800}))).unwrap();
        assert!(c.tracks.is_empty());
    }
    #[test]
    fn missing_track_selection_is_safe_and_invalid_restore_is_rejected() {
        let mut c = LocalMusicContent::default();
        let x = ctx();
        c.devices.insert("a".into(), Selection { incarnation:1, key:"b".repeat(64), generation:1, position:123, playing:true, started:0 });
        c.next_generation=2;
        assert!(c.plan(&x.devices[0].context, ButtonAction::Sub).is_err());
        c.tracks.insert("a".repeat(64), Track {key:"a".repeat(64),name:"valid.wav".into(),frames:48000});
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
