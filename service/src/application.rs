//! SQLite-backed application boundary around the pure transition engine.
//! This module does not open a listener or execute effects.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, FetchJob, PluginContext, PluginPosition,
};
use crate::settings::{DeviceRecord, DeviceSettings, PluginSettings, SettingsError, SettingsStore};
use crate::{
    display_current, frame_current, session_current, Dataset, Decision, Device, Effect, Engine,
    Event, FetchKey, FrameToken, Mode, Owner, SessionToken, World,
};

/// Provisional safety cap; replace after measured workload and durable runner testing.
pub const MAX_PENDING_EFFECTS: usize = 1024;

pub use crate::plugins::DEFAULT_ORDER;

#[derive(Debug)]
pub enum AppError {
    Invalid(&'static str),
    NotFound,
    Conflict(&'static str),
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Internal(Box::new(value))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Internal(Box::new(value))
    }
}

fn settings_error(error: SettingsError) -> AppError {
    match error {
        SettingsError::Invalid(_) => AppError::Invalid("invalid settings"),
        SettingsError::UnknownDevice(_) => AppError::NotFound,
        _ => AppError::Conflict("stale settings revision or device incarnation"),
    }
}

#[derive(Clone)]
struct Runtime {
    world: World,
    settings: SettingsStore,
    order_revision: u64,
    outbox: BTreeMap<u64, Effect>,
    next_effect_id: u64,
    plugin_states: BTreeMap<String, Arc<serde_json::Value>>,
    jobs: crate::plugin_jobs::JobState,
    managed_revisions: BTreeMap<String, u64>,
}

#[derive(Serialize, Deserialize)]
struct StoredState {
    world: StoredWorld,
    settings: SettingsStore,
    order_revision: u64,
    outbox: BTreeMap<u64, Effect>,
    next_effect_id: u64,
    #[serde(default)]
    plugin_states: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    jobs: crate::plugin_jobs::JobState,
    #[serde(default)]
    managed_revisions: BTreeMap<String, u64>,
}

#[derive(Serialize, Deserialize)]
struct StoredWorld {
    revision: u64,
    next_incarnation: u64,
    next_frame_id: u64,
    plugin_order: Vec<String>,
    devices: BTreeMap<String, StoredDevice>,
}

#[derive(Serialize, Deserialize)]
struct StoredDevice {
    incarnation: u64,
    capabilities: BTreeSet<String>,
    enabled: BTreeSet<String>,
    selected: Option<String>,
    session_epoch: u64,
    display_epoch: u64,
    settings_revision: u64,
    requests: Vec<(FetchKey, u64)>,
    datasets: Vec<(FetchKey, Dataset)>,
}

impl From<&Runtime> for StoredState {
    fn from(state: &Runtime) -> Self {
        let world = &state.world;
        Self {
            world: StoredWorld {
                revision: world.revision,
                next_incarnation: world.next_incarnation,
                next_frame_id: world.next_frame_id,
                plugin_order: world.plugin_order.clone(),
                devices: world
                    .devices
                    .iter()
                    .map(|(id, device)| {
                        (
                            id.clone(),
                            StoredDevice {
                                incarnation: device.incarnation,
                                capabilities: device.capabilities.clone(),
                                enabled: device.enabled.clone(),
                                selected: device.selected.clone(),
                                session_epoch: device.session_epoch,
                                display_epoch: device.display_epoch,
                                settings_revision: device.settings_revision,
                                requests: device
                                    .requests
                                    .iter()
                                    .map(|(key, id)| (key.clone(), *id))
                                    .collect(),
                                datasets: device
                                    .datasets
                                    .iter()
                                    .map(|(key, data)| (key.clone(), data.clone()))
                                    .collect(),
                            },
                        )
                    })
                    .collect(),
            },
            settings: state.settings.clone(),
            order_revision: state.order_revision,
            outbox: state.outbox.clone(),
            next_effect_id: state.next_effect_id,
            plugin_states: state.plugin_states.iter().map(|(k,v)|(k.clone(),v.as_ref().clone())).collect(),
            jobs: state.jobs.clone(),
            managed_revisions: state.managed_revisions.clone(),
        }
    }
}

#[cfg(test)]
impl TryFrom<StoredState> for Runtime {
    type Error=AppError;
    fn try_from(stored:StoredState)->Result<Self,Self::Error>{
        Self::from_stored(stored,&DEFAULT_ORDER.iter().map(|id|(*id).to_owned()).collect::<Vec<_>>())
    }
}

impl Runtime {
    fn from_stored(stored: StoredState, catalog: &[String]) -> Result<Self, AppError> {
        let devices = stored
            .world
            .devices
            .into_iter()
            .map(|(id, device)| {
                (
                    id,
                    Device {
                        incarnation: device.incarnation,
                        capabilities: device.capabilities,
                        enabled: device.enabled,
                        selected: device.selected,
                        mode: Mode::Normal,
                        owner: Owner::Local,
                        screen_owner_epoch: 0,
                        online: false,
                        session_epoch: device.session_epoch,
                        display_epoch: device.display_epoch,
                        settings_revision: device.settings_revision,
                        requests: device.requests.into_iter().collect(),
                        datasets: device.datasets.into_iter().collect(),
                        pending_frame_id: None,
                        last_sent_frame: None,
                        last_received_frame: None,
                        last_rendered_frame: None,
                    },
                )
            })
            .collect();
        let mut state = Self {
            world: World {
                revision: stored.world.revision,
                next_incarnation: stored.world.next_incarnation,
                next_frame_id: stored.world.next_frame_id,
                plugin_order: stored.world.plugin_order,
                devices,
            },
            settings: stored.settings,
            order_revision: stored.order_revision,
            outbox: stored.outbox,
            next_effect_id: stored.next_effect_id,
            plugin_states: stored.plugin_states.into_iter().map(|(k,v)|(k,Arc::new(v))).collect(),
            jobs: stored.jobs,
            managed_revisions: stored.managed_revisions,
        };
        // Schema-2 installations before the clock plugin have exactly these
        // four IDs, in any saved order. Append only; preserve all selections,
        // enablement sets and per-device revisions. Reject malformed orders.
        let legacy: BTreeSet<_> = ["youtube", "caricature", "weather", "rain-radar"].into();
        if catalog.iter().any(|id| id == "clock")
            && state.world.plugin_order.len() == legacy.len()
            && state
                .world
                .plugin_order
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                == legacy
        {
            state.world.plugin_order.push("clock".into());
            state.order_revision = state
                .order_revision
                .checked_add(1)
                .ok_or(AppError::Conflict("order revision exhausted"))?;
        }
        for record in state.settings.devices.values_mut() {
            record.online = false;
        }
        if state.world.next_incarnation == 0
            || state.next_effect_id == 0
            || state.outbox.len() > MAX_PENDING_EFFECTS
            || state
                .outbox
                .keys()
                .next_back()
                .is_some_and(|id| *id >= state.next_effect_id)
            || state.settings.next_incarnation != state.world.next_incarnation
            || state.world.devices.len() != state.settings.devices.len()
            || state.settings.plugins.value.validate().is_err()
            || state.world.devices.iter().any(|(id, device)| {
                state.settings.devices.get(id).is_none_or(|record| {
                    record.desired.incarnation != device.incarnation
                        || record.confirmed.incarnation != device.incarnation
                        || record.desired.value.validate().is_err()
                        || record.confirmed.value.validate().is_err()
                        || device.incarnation >= state.world.next_incarnation
                })
            })
            || !valid_order(&state.world.plugin_order, catalog)
        {
            return Err(AppError::Conflict("inconsistent stored state"));
        }
        state
            .jobs
            .recover()
            .map_err(|_| AppError::Conflict("invalid stored plugin jobs"))?;
        state.prune_stale_effects();
        Ok(state)
    }
}

impl Runtime {
    fn enqueue(&mut self, effects: Vec<Effect>) -> Result<(), AppError> {
        self.prune_stale_effects();
        if self.outbox.len().saturating_add(effects.len()) > MAX_PENDING_EFFECTS {
            return Err(AppError::Conflict("effect outbox full"));
        }
        for effect in effects {
            let id = self.next_effect_id;
            self.next_effect_id = id
                .checked_add(1)
                .ok_or(AppError::Conflict("effect id exhausted"))?;
            self.outbox.insert(id, effect);
        }
        Ok(())
    }

    fn accept(&mut self, decision: Decision) -> Result<bool, AppError> {
        let changed = decision.applied.is_some();
        self.world = decision.state;
        self.enqueue(decision.effects)?;
        Ok(changed)
    }

    fn prune_stale_effects(&mut self) {
        self.outbox
            .retain(|_, effect| effect_current(&self.world, &self.settings, effect));
    }
}

fn same_durable_projection(a:&Runtime,b:&Runtime)->bool {
    a.world.next_incarnation==b.world.next_incarnation && a.world.plugin_order==b.world.plugin_order
        && a.order_revision==b.order_revision && a.settings==b.settings
        && a.plugin_states.len()==b.plugin_states.len() && a.plugin_states.iter().all(|(k,v)|b.plugin_states.get(k).is_some_and(|w|Arc::ptr_eq(v,w)||v.as_ref()==w.as_ref())) && a.jobs==b.jobs && a.managed_revisions==b.managed_revisions
        && a.world.devices.len()==b.world.devices.len()
        && a.world.devices.iter().all(|(id,x)|b.world.devices.get(id).is_some_and(|y|
            x.incarnation==y.incarnation && x.capabilities==y.capabilities && x.enabled==y.enabled
            && x.selected==y.selected && x.session_epoch==y.session_epoch && x.settings_revision==y.settings_revision
            && x.requests==y.requests && x.datasets==y.datasets))
}

struct InputClock {unix:i64,today:String}
impl crate::fetch::Clock for InputClock {
    fn unix_seconds(&self)->i64{self.unix}
    fn today_jst(&self)->String{self.today.clone()}
}

fn effect_current(world: &World, settings: &SettingsStore, effect: &Effect) -> bool {
    match effect {
        Effect::CloseSession(_) | Effect::DropQueuedFrames { .. } => true,
        Effect::ShowSelected { token, plugin } | Effect::ResetPluginView { token, plugin } => {
            world.devices.get(&token.device_id).is_some_and(|device| {
                display_current(device, token) && device.selected.as_deref() == Some(plugin)
            })
        }
        Effect::ShowLogo { token } => world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| display_current(device, token) && device.selected.is_none()),
        Effect::EnterStandby { token } => world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| session_current(device, token) && device.mode == Mode::Standby),
        Effect::SyncSettings { token, snapshot } => {
            world.devices.get(&token.device_id).is_some_and(|device| {
                session_current(device, token)
                    && settings
                        .devices
                        .get(&token.device_id)
                        .is_some_and(|record| record.desired == *snapshot)
            })
        }
        Effect::Fetch(token) => world.devices.get(&token.device_id).is_some_and(|device| {
            device.incarnation == token.incarnation
                && device.settings_revision == token.settings_revision
                && device.requests.get(&token.key) == Some(&token.request_id)
                && device.datasets.get(&token.key) == Some(&Dataset::Loading)
        }),
        Effect::SendFrame { token, .. } => {
            world.devices.get(&token.device_id).is_some_and(|device| {
                frame_current(device, token) && device.pending_frame_id == Some(token.frame_id)
            })
        }
    }
}

fn valid_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_order(order: &[String], catalog: &[String]) -> bool {
    order.len()==catalog.len()
        && order.iter().collect::<BTreeSet<_>>().len()==catalog.len()
        && order.iter().all(|id| catalog.contains(id))
}

#[cfg(test)]
type PersistenceHook=Box<dyn FnOnce()->Result<(),AppError>+Send>;

pub struct App {
    connection: Arc<std::sync::Mutex<Connection>>,
    engine: Arc<Engine>,
    frame_counter: Arc<std::sync::Mutex<u64>>,
    effect_counter: Arc<std::sync::Mutex<u64>>,
    transient_only: bool,
    transient_changed: bool,
    defer_latest_publication: bool,
    input_fences: BTreeMap<String,u64>,
    #[cfg(test)] persistence_hook:Arc<std::sync::Mutex<Option<PersistenceHook>>>,
    plugin_catalog: Vec<String>,
    observations: crate::observation::Source,
    observation_parent: u64,
    observation_cause: Option<crate::observation::Cause>,
    #[cfg(feature="boundary-observation")]
    frame_causes: std::collections::VecDeque<(u64,crate::observation::Cause)>,
    #[cfg(feature="boundary-observation")]
    view_causes: std::collections::VecDeque<((u64,u64,u64,u64),crate::observation::Cause)>,
    runtime: Runtime,
    clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
    /// Completed LCD pixels are transient; only validated handles enter the sender.
    frame_assets: BTreeMap<String, Arc<Vec<u8>>>,
    video_slots: BTreeMap<String, Arc<std::sync::Mutex<crate::lcd_stream::VideoSlot>>>,
    pending_latest: BTreeMap<String, crate::lcd_stream::VideoFrame>,
    contents: BTreeMap<String, Box<dyn ContentPlugin>>,
    timed_frames: BTreeMap<String, (crate::DisplayToken, i64)>,
    // Latest content must still be shown after the preceding physical draw.
    // This obligation is runtime-only and scoped to the current display.
    content_refresh_debt: BTreeMap<String, crate::DisplayToken>,
    dispatch: crate::shared::Signal,
    work_signal: crate::shared::Signal,
    plugin_root: PathBuf,
    mono_origin: std::time::Instant,
    persistence_us: u128,
    image_preparation_us: u128,
    // High-water mark for valid transport Hello attempts. Keep after disconnect
    // or failed Welcome; old pending sockets must not resurrect a past attempt.
    // Runtime only: a restarted process/App starts with a fresh acceptance order.
    hello_accept_order: BTreeMap<String, std::num::NonZeroU64>,
    maintenance_plugin: Option<String>,
}

#[derive(Debug)]
pub enum InputResult {
    Selection {
        selected: Option<String>,
        display_epoch: u64,
    },
    Position(PluginPosition),
}

impl App {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::open_with_clock(path, Arc::new(crate::fetch::SystemClock))
    }

    pub(crate) fn open_with_clock(
        path: &Path,
        clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
    ) -> Result<Self, AppError> {
        let root = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins");
        Self::open_with_plugin_root_and_clock(path, &root, clock)
    }

    pub fn open_with_plugin_root(path: &Path, root: &Path) -> Result<Self, AppError> {
        Self::open_with_plugin_root_and_clock(path, root, Arc::new(crate::fetch::SystemClock))
    }

    fn open_with_plugin_root_and_clock(
        path: &Path,
        root: &Path,
        clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
    ) -> Result<Self, AppError> {
        Self::open_with_plugin_set(path, root, clock, crate::plugins::builtins())
    }

    /// Inject a plugin composition without editing application or transport.
    pub fn open_with_plugin_set(
        path: &Path, root: &Path,
        clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
        plugins: crate::plugins::PluginSet,
    ) -> Result<Self, AppError> {
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS app_state (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               schema_version INTEGER NOT NULL,
               body TEXT NOT NULL
             );",
        )?;
        for id in &plugins.order {
            if let Some(recovery) = plugins.startup.get(id) {
                let paths = crate::plugin_jobs::PluginPaths {
                    data: root.join(id).join("data"),
                    cache: root.join(id).join("cache"),
                };
                recovery.recover(&paths, &connection)
                    .map_err(|error| AppError::Internal(Box::new(error)))?;
            }
        }
        let existing: Option<(i64, String)> = connection
            .query_row(
                "SELECT schema_version, body FROM app_state WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let runtime = if let Some((schema_version, body)) = existing {
            if schema_version != 2 {
                return Err(AppError::Conflict("unsupported database schema"));
            }
            let stored = serde_json::from_str::<StoredState>(&body)?;
            let previous_order_revision = stored.order_revision;
            let runtime = Runtime::from_stored(stored, &plugins.order)?;
            if runtime.order_revision != previous_order_revision {
                Self::persist(&mut connection, &runtime)?;
            }
            runtime
        } else {
            let runtime = Runtime {
                world: World::new(plugins.order.clone()),
                settings: SettingsStore::new(),
                order_revision: 1,
                outbox: BTreeMap::new(),
                next_effect_id: 1,
                plugin_states: BTreeMap::new(),
                jobs: crate::plugin_jobs::JobState::default(),
                managed_revisions: BTreeMap::new(),
            };
            Self::persist(&mut connection, &runtime)?;
            runtime
        };
        let mut contents = plugins.contents;
        for (id, value) in &runtime.plugin_states {
            if let Some(plugin) = contents.get_mut(id) {
                plugin
                    .load_state(value.as_ref().clone())
                    .map_err(|_| AppError::Conflict("invalid plugin state"))?;
            }
        }
        let frame_counter=Arc::new(std::sync::Mutex::new(runtime.world.next_frame_id));
        let effect_counter=Arc::new(std::sync::Mutex::new(runtime.next_effect_id));
        Ok(Self {
            connection:Arc::new(std::sync::Mutex::new(connection)),
            engine: Arc::new(plugins.engine),
            frame_counter,
            effect_counter,
            transient_only:false,
            transient_changed:false,
            defer_latest_publication:false,
            input_fences:BTreeMap::new(),
            #[cfg(test)] persistence_hook:Arc::new(std::sync::Mutex::new(None)),
            plugin_catalog: plugins.order,
            observations: crate::observation::Source::default(),
            observation_parent: 0,
            observation_cause: None,
            #[cfg(feature="boundary-observation")]
            frame_causes: std::collections::VecDeque::with_capacity(256),
            #[cfg(feature="boundary-observation")]
            view_causes: std::collections::VecDeque::with_capacity(256),
            runtime,
            clock,
            frame_assets: BTreeMap::new(),
            video_slots: BTreeMap::new(),
            pending_latest: BTreeMap::new(),
            contents,
            timed_frames: BTreeMap::new(),
            content_refresh_debt: BTreeMap::new(),
            dispatch: crate::shared::Signal::new(),
            work_signal: crate::shared::Signal::new(),
            plugin_root: root.to_owned(),
            mono_origin: std::time::Instant::now(),
            persistence_us: 0,
            image_preparation_us: 0,
            hello_accept_order: BTreeMap::new(),
            maintenance_plugin: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn pause_next_persistence(&self,hook:PersistenceHook){*self.persistence_hook.lock().unwrap()=Some(hook);}
    pub(crate) fn published_snapshot(&self)->Self {
        Self {
            connection:self.connection.clone(),
            engine:self.engine.clone(),
            frame_counter:self.frame_counter.clone(),
            effect_counter:self.effect_counter.clone(),
            transient_only:self.transient_only,
            transient_changed:false,
            defer_latest_publication:false,
            input_fences:self.input_fences.clone(),
            #[cfg(test)] persistence_hook:self.persistence_hook.clone(),
            plugin_catalog:self.plugin_catalog.clone(),
            observations:self.observations.clone(),
            observation_parent:self.observation_parent.clone(),
            observation_cause:self.observation_cause.clone(),
            runtime:self.runtime.clone(),
            clock:self.clock.clone(),
            frame_assets:self.frame_assets.clone(),
            video_slots:self.video_slots.clone(),
            pending_latest:BTreeMap::new(),
            contents:self.contents.clone(),
            timed_frames:self.timed_frames.clone(),
            content_refresh_debt:self.content_refresh_debt.clone(),
            dispatch:self.dispatch.clone(),
            work_signal:self.work_signal.clone(),
            plugin_root:self.plugin_root.clone(),
            mono_origin:self.mono_origin.clone(),
            persistence_us:self.persistence_us.clone(),
            image_preparation_us:self.image_preparation_us.clone(),
            hello_accept_order:self.hello_accept_order.clone(),
            #[cfg(feature="boundary-observation")] frame_causes:self.frame_causes.clone(),
            #[cfg(feature="boundary-observation")] view_causes:self.view_causes.clone(),
            maintenance_plugin:self.maintenance_plugin.clone(),
        }
    }
    pub(crate) fn defer_publication_signals(&mut self){self.defer_latest_publication=true;self.dispatch.defer();self.work_signal.defer();}
    pub(crate) fn resume_publication_signals(&self){self.dispatch.resume();self.work_signal.resume();}
    pub(crate) fn take_latest_publications(&mut self)->Vec<(Arc<std::sync::Mutex<crate::lcd_stream::VideoSlot>>,crate::lcd_stream::VideoFrame)>{
        std::mem::take(&mut self.pending_latest).into_iter().filter_map(|(id,frame)|{
            let device=self.runtime.world.devices.get(&id)?;
            if !crate::frame_current(device,&frame.token){return None;}
            self.video_slots.get(&id).map(|slot|(slot.clone(),frame))
        }).collect()
    }
    pub(crate) fn reconcile_video_slots(&self){
        for (id,slot) in &self.video_slots {
            if let Ok(mut slot)=slot.lock(){
                let keep=self.runtime.world.devices.get(id).is_some_and(|d|session_current(d,&slot.session) && d.owner==Owner::Remote && matches!(d.mode,Mode::Normal|Mode::Sub));
                let stale=!keep || slot.latest.borrow().as_ref().is_some_and(|f|!frame_current(&self.runtime.world.devices[id],&f.token));
                if stale{slot.clear();}
            }
        }
    }
    fn persist(connection:&mut Connection,runtime:&Runtime)->Result<(),AppError>{Self::persist_with_hook(connection,runtime,||Ok(()))}
    fn persist_with_hook(connection:&mut Connection,runtime:&Runtime,before_commit:impl FnOnce()->Result<(),AppError>)->Result<(),AppError>{
        let diagnostic_started=crate::diagnostics::mark();
        let started = std::time::Instant::now();
        let body = serde_json::to_string(&StoredState::from(runtime))?;
        let serialized = started.elapsed();
        let body_bytes = body.len();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO app_state (id, schema_version, body) VALUES (1, 2, ?1)
             ON CONFLICT(id) DO UPDATE SET body = excluded.body",
            params![body],
        )?;
        before_commit()?;
        transaction.commit()?;
        let elapsed = started.elapsed();
        crate::diagnostics::duration(crate::diagnostics::Kind::Persist,diagnostic_started,0,0,0);
        if elapsed >= std::time::Duration::from_millis(50) {
            eprintln!("picomado state_persist_slow body_bytes={} serialize_us={} sqlite_us={} elapsed_us={}",
                body_bytes, serialized.as_micros(), elapsed.saturating_sub(serialized).as_micros(), elapsed.as_micros());
        }
        Ok(())
    }

    fn change<T>(
        &mut self,
        action: impl FnOnce(&Engine, &mut Runtime) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        use crate::observation::{Component, Phase};
        let before = self.runtime.world.revision;
        let operation = self.record_observation(Component::State, Phase::Received, 0, self.observation_parent, before, 0);
        let first_effect = self.runtime.next_effect_id;
        let mut next = self.runtime.clone();
        let mut frame_counter=self.frame_counter.lock().map_err(|_|AppError::Conflict("frame allocator unavailable"))?;
        let mut effect_counter=self.effect_counter.lock().map_err(|_|AppError::Conflict("effect allocator unavailable"))?;
        next.world.next_frame_id=next.world.next_frame_id.max(*frame_counter);
        next.next_effect_id=next.next_effect_id.max(*effect_counter);
        let result = match action(&self.engine, &mut next) {
            Ok(value) => value,
            Err(error) => {
                self.record_observation(Component::State, Phase::Rejected, operation, 0, before, 0);
                return Err(error);
            }
        };
        *frame_counter=(*frame_counter).max(next.world.next_frame_id);drop(frame_counter);
        *effect_counter=(*effect_counter).max(next.next_effect_id);drop(effect_counter);
        next.prune_stale_effects();
        self.record_observation(Component::State, Phase::Dispatched, operation, 0, next.world.revision, next.outbox.len() as u64);
        let next_fences=if !self.transient_only {
            let mut fences=self.input_fences.clone();
            for (id,after) in &next.world.devices {
                if self.runtime.world.devices.get(id).is_none_or(|before|
                    before.incarnation!=after.incarnation || before.session_epoch!=after.session_epoch
                    || before.settings_revision!=after.settings_revision || before.selected!=after.selected
                    || before.enabled!=after.enabled || before.capabilities!=after.capabilities
                    || before.mode!=after.mode || before.owner!=after.owner || before.online!=after.online
                    || before.screen_owner_epoch!=after.screen_owner_epoch) {
                    let fence=fences.entry(id.clone()).or_default();*fence=fence.checked_add(1).ok_or(AppError::Conflict("input fence exhausted"))?;
                }
            }
            Some(fences)
        }else{None};
        let saving = std::time::Instant::now();
        if self.transient_only && !same_durable_projection(&self.runtime,&next) {
            return Err(AppError::Conflict("transient operation requires persistence"));
        }
        let persisted=if self.transient_only {Ok(())}else{crate::shared::blocking(||{
            let mut connection=self.connection.lock().map_err(|_|AppError::Conflict("storage unavailable"))?;
            Self::persist_with_hook(&mut connection,&next,||{
                #[cfg(test)] if let Some(hook)=self.persistence_hook.lock().map_err(|_|AppError::Conflict("storage hook unavailable"))?.take(){hook()?;}
                Ok(())
            })
        })};
        if let Err(error) = persisted {
            self.record_observation(Component::State, Phase::Error, operation, 0, before, 0);
            return Err(error);
        }
        if !self.transient_only {self.persistence_us = self.persistence_us.saturating_add(saving.elapsed().as_micros());}
        let revision = next.world.revision;
        if let Some(fences)=next_fences{self.input_fences=fences;}
        self.content_refresh_debt.retain(|id, token| {
            let Some(before) = self.runtime.world.devices.get(id) else { return false; };
            let Some(after) = next.world.devices.get(id) else { return false; };
            if !crate::display_current(before, token) || !same_display_identity(before, after) {
                return false;
            }
            token.settings_revision = after.settings_revision;
            true
        });
        self.runtime = next;
        if self.transient_only{self.transient_changed=true;}
        self.record_observation(Component::State, Phase::Committed, operation, 0, revision, if self.transient_only {2}else{1});
        #[cfg(feature="boundary-observation")]
        for (_, effect) in self.runtime.outbox.range(first_effect..) {
            if let Effect::SendFrame{token,..}=effect {
                let key=(token.incarnation,token.session_epoch,token.display_epoch,token.screen_owner_epoch);
                let cause=self.observation_cause.or_else(||self.view_causes.iter().rev().find(|(view,_)|*view==key).map(|(_,cause)|*cause));
                if let Some(cause)=cause {
                    if self.frame_causes.len()==256 {self.frame_causes.pop_front();}
                    self.frame_causes.push_back((token.frame_id,cause));
                }
                self.observations.record_context(Component::Frame,Phase::Pending,0,operation,revision,0,token.frame_id,cause);
            }
        }
        if self.observations.enabled() {
            for (id, _) in self.runtime.outbox.range(first_effect..) {
                self.record_observation(Component::Effect, Phase::Pending, 0, operation, revision, *id);
            }
        }
        self.timed_frames
            .retain(|id, _| self.runtime.world.devices.contains_key(id));
        self.content_refresh_debt.retain(|id, token| {
            self.runtime.world.devices.get(id).is_some_and(|d| {
                crate::display_current(d, token) && d.mode == Mode::Normal && d.owner == Owner::Remote
            })
        });
        self.reconcile_video_slots();
        self.frame_assets.retain(|handle, _| {
            self.runtime.outbox.values().any(|effect|
            matches!(effect, Effect::SendFrame { asset_handle, .. } if asset_handle == handle))
        });
        if !self.transient_only {self.dispatch.send_replace(());}
        Ok(result)
    }

    fn plugin_retention_references(&self, plugin: &str) -> BTreeSet<String> {
        let jobs: Vec<_> = self.runtime.jobs.jobs.values()
            .filter(|record| record.job.token.plugin == plugin && !record.status.finished())
            .map(|record| &record.job)
            .collect();
        self.contents.get(plugin)
            .map(|content| content.retention_references_for_jobs(&jobs))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn replace_test_content(&mut self, id: &str, content: Box<dyn ContentPlugin>) {
        self.contents.insert(id.into(), content);
    }

    /// Diagnostic counters are transient and never enter plugin/kernel state.
    pub(crate) fn operation_totals(&self) -> (u128, u128) {
        (self.persistence_us, self.image_preparation_us)
    }

    /// Subscribe before inspecting the outbox so changes cannot be missed.
    fn record_observation(&self,component:crate::observation::Component,phase:crate::observation::Phase,operation:u64,parent:u64,revision:u64,argument:u64)->u64 {
        self.observations.record_context(component,phase,operation,parent,revision,argument,0,self.observation_cause)
    }
    pub fn with_observation_cause<T>(&mut self,device_id:&str,cause:Option<crate::observation::Cause>,action:impl FnOnce(&mut Self)->T)->T {
        #[cfg(feature="boundary-observation")]
        let before=self.runtime.world.devices.get(device_id).map(|d|(d.incarnation,d.session_epoch,d.display_epoch,d.screen_owner_epoch));
        let previous=self.observation_cause;self.observation_cause=cause.filter(|c|c.valid());
        let result=action(self);
        #[cfg(feature="boundary-observation")]
        if let Some(cause)=self.observation_cause {
            let after=self.runtime.world.devices.get(device_id).map(|d|(d.incarnation,d.session_epoch,d.display_epoch,d.screen_owner_epoch));
            if after!=before {
                if let Some(key)=after {
                    if self.view_causes.len()==256{self.view_causes.pop_front();}
                    self.view_causes.push_back((key,cause));
                }
            }
        }
        #[cfg(not(feature="boundary-observation"))]
        let _=device_id;
        self.observation_cause=previous;result
    }
    pub fn frame_observation_cause(&self,frame:&FrameToken)->Option<crate::observation::Cause>{
        #[cfg(feature="boundary-observation")]
        {
            if let Some((_,cause))=self.frame_causes.iter().rev().find(|(id,_)|*id==frame.frame_id){return Some(*cause);}
            let key=(frame.incarnation,frame.session_epoch,frame.display_epoch,frame.screen_owner_epoch);
            self.view_causes.iter().rev().find(|(view,_)|*view==key).map(|(_,cause)|*cause)
        }
        #[cfg(not(feature="boundary-observation"))]
        {let _=frame;None}
    }
    pub fn observation_source(&self) -> crate::observation::Source { self.observations.clone() }

    pub(crate) fn observe_effect(&self, phase: crate::observation::Phase, id: u64) {
        self.record_observation(crate::observation::Component::Effect, phase, 0, 0, self.runtime.world.revision, id);
    }

    pub fn has_plugin(&self, id: &str) -> bool { self.contents.contains_key(id) }

    pub fn dispatch_notifications(&self) -> tokio::sync::watch::Receiver<()> {
        self.dispatch.subscribe()
    }

    /// Pending effects survive restart. The caller must complete an effect only
    /// after its external operation is known to have completed.
    pub fn pending_effects(&self) -> Vec<(u64, Effect)> {
        self.runtime
            .outbox
            .iter()
            .map(|(id, effect)| (*id, effect.clone()))
            .collect()
    }

    pub fn effect_pending(&self, id: u64) -> bool {
        self.runtime.outbox.contains_key(&id)
    }

    pub fn session_is_current(&self, token: &SessionToken) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| session_current(device, token))
    }

    pub fn selection_for(&self, token: &SessionToken) -> Option<(Option<String>, u64)> {
        let device = self.runtime.world.devices.get(&token.device_id)?;
        session_current(device, token).then(|| (device.selected.clone(), device.display_epoch))
    }

    pub fn complete_effect(&mut self, id: u64) -> Result<bool, AppError> {
        let handle = match self.runtime.outbox.get(&id) {
            Some(Effect::SendFrame { asset_handle, .. }) => Some(asset_handle.clone()),
            _ => None,
        };
        let sent_frame = match self.runtime.outbox.get(&id) {
            Some(Effect::SendFrame { token, .. }) => Some(token.clone()),
            _ => None,
        };
        let completed = self.change(|engine, state| {
            if let Some(token) = sent_frame {
                let decision = engine
                    .handle(&state.world, &Event::FrameSent { token })
                    .map_err(|_| AppError::Conflict("frame send result rejected"))?;
                state.accept(decision)?;
            }
            Ok(state.outbox.remove(&id).is_some())
        })?;
        if let Some(handle) = handle {
            self.frame_assets.remove(&handle);
        }
        Ok(completed)
    }

    /// Queue one complete RGB565 big-endian LCD image. The source pixels are
    /// never written into SQLite or claimed to be drawn here.
    pub fn queue_lcd_frame(
        &mut self,
        token: &SessionToken,
        plugin: &str,
        bytes: Vec<u8>,
    ) -> Result<FrameToken, AppError> {
        if bytes.len() != crate::protocol::LCD_FRAME_BYTES {
            return Err(AppError::Invalid("invalid LCD frame length"));
        }
        if self.supports_latest_delivery(token) {
            return self.queue_lcd_latest(token, Some(plugin), bytes);
        }
        if self.frame_assets.len() >= 128 {
            return Err(AppError::Conflict("transient frame asset limit"));
        }
        let handle = format!(
            "lcd:{}:{}",
            token.device_id, self.runtime.world.next_frame_id
        );
        let frame = self.request_frame(token, plugin, &handle)?;
        let bytes = self.compose_lcd_frame(&token.device_id, bytes);
        self.frame_assets.insert(handle, Arc::new(bytes));
        Ok(frame)
    }

    /// Ordinary images use the existing bounded writer slot. Pico still
    /// sends both legacy ACKs, but neither ACK grants another send.
    fn queue_lcd_latest(
        &mut self,
        token: &SessionToken,
        plugin: Option<&str>,
        pixels: Vec<u8>,
    ) -> Result<FrameToken, AppError> {
        let d = self.runtime.world.devices.get(&token.device_id).ok_or(AppError::NotFound)?;
        if !session_current(d, token) || d.owner != Owner::Remote
            || !matches!(d.mode, Mode::Normal | Mode::Sub)
            || d.selected.as_deref() != plugin
            || (plugin.is_none() && d.mode != Mode::Normal)
            || pixels.len() != crate::protocol::LCD_FRAME_BYTES
        {
            return Err(AppError::Conflict("LCD image no longer selected"));
        }
        self.subscribe_lcd_video(token).ok_or(AppError::Conflict("LCD latest unavailable"))?;
        let frame_id = {
            let mut counter = self.frame_counter.lock().map_err(|_| AppError::Conflict("frame allocator unavailable"))?;
            let id = *counter;
            *counter = counter.checked_add(1).ok_or(AppError::Conflict("frame counter exhausted"))?;
            id
        };
        let d = &self.runtime.world.devices[&token.device_id];
        let frame = FrameToken {
            device_id: token.device_id.clone(), incarnation: d.incarnation,
            session_epoch: d.session_epoch, display_epoch: d.display_epoch,
            screen_owner_epoch: d.screen_owner_epoch, frame_id,
        };
        let eink_refresh = self.frame_eink_refresh(&frame);
        let pixels = Arc::new(self.compose_lcd_frame(&token.device_id, pixels));
        let slot = self.video_slots.get(&token.device_id).ok_or(AppError::Conflict("LCD latest unavailable"))?;
        let mut slot = slot.lock().map_err(|_| AppError::Conflict("LCD latest unavailable"))?;
        if slot.session != *token { return Err(AppError::Conflict("stale LCD latest slot")); }
        let outgoing = crate::lcd_stream::VideoFrame {
            token: frame.clone(), delivery: crate::lcd_stream::DeliveryKind::Ordinary,
            fps: 0, sequence: frame_id, generation: 0,
            mode: if d.mode == Mode::Sub { "sub" } else { "normal" }, eink_refresh, pixels,
        };
        if self.transient_only || self.defer_latest_publication {
            // A reader can otherwise see the slot before the display epoch
            // and timed key that validate it. Publish at the App boundary.
            drop(slot);
            self.pending_latest.insert(token.device_id.clone(), outgoing);
        } else {
            slot.publish(outgoing);
        }
        // The slot is shared with readers, but timed_frames and the display
        // projection live in App. Publish their snapshot together after this
        // operation, including when no durable state changed.
        self.transient_changed |= self.transient_only;
        self.runtime.world.next_frame_id = self.runtime.world.next_frame_id.max(frame_id + 1);
        Ok(frame)
    }

    /// Compose a per-device mode indicator only in the outbound frame copy.
    /// In particular, plugin history and shared artwork remain untouched.
    fn compose_lcd_frame(&self, device_id: &str, mut bytes: Vec<u8>) -> Vec<u8> {
        if let Some(device) = self
            .runtime
            .world
            .devices
            .get(device_id)
            .filter(|d| d.capabilities.contains("eink") && !d.capabilities.contains("lcd"))
        {
            let ctx = PluginContext {
                token: crate::session_token(device_id, device),
                settings_revision: device.settings_revision,
                mode: device.mode.clone(),
            };
            let mut mono = device
                .selected
                .as_ref()
                .and_then(|id| self.contents.get(id))
                .and_then(|plugin| plugin.eink_frame_for(&ctx, &bytes))
                .unwrap_or_else(|| crate::eink::from_lcd(&bytes));
            if device.mode == Mode::Sub {
                crate::eink::draw_sub(&mut mono);
            }
            return mono;
        }
        if self
            .runtime
            .world
            .devices
            .get(device_id)
            .is_some_and(|device| device.mode == Mode::Sub)
        {
            crate::display_badge::draw_sub_rgb565be(&mut bytes);
        }
        bytes
    }

    /// Deliver a completed source to a plugin by ID. Source parsing and
    /// snapshot ownership stay inside the selected plugin implementation.
    pub fn publish_plugin_source(
        &mut self,
        plugin: &str,
        token: &SessionToken,
        settings_revision: u64,
        source: &[u8],
        dataset_id: &str,
        today_jst: &str,
    ) -> Result<(), AppError> {
        let device = self
            .runtime
            .world
            .devices
            .get(&token.device_id)
            .ok_or(AppError::NotFound)?;
        if device.incarnation != token.incarnation || device.settings_revision != settings_revision
        {
            return Err(AppError::Conflict("stale plugin source"));
        }
        let ctx = PluginContext {
            token: crate::session_token(&token.device_id, device),
            settings_revision,
            mode: device.mode.clone(),
        };
        let mut content = self.contents.get(plugin).ok_or(AppError::NotFound)?.clone();
        content
            .publish(&ctx, source, dataset_id, today_jst)
            .map_err(|_| AppError::Invalid("invalid plugin source"))?;
        let snapshot = content.save_state();
        self.change(|_, state| {
            state.plugin_states.insert(plugin.into(), Arc::new(snapshot));
            Ok(())
        })?;
        self.contents.insert(plugin.into(), content);
        Ok(())
    }

    pub fn plugin_options(&self, plugin: &str) -> Option<serde_json::Value> {
        self.contents.get(plugin).map(|p| p.options())
    }

    pub fn plugin_device_config(&self, id: &str, plugin: &str) -> Option<serde_json::Value> {
        let device = self.runtime.world.devices.get(id)?;
        Some(
            self.contents
                .get(plugin)?
                .device_config(id, device.incarnation),
        )
    }

    pub fn set_plugin_device_config(
        &mut self,
        id: &str,
        incarnation: u64,
        plugin: &str,
        expected_revision: u64,
        config: serde_json::Value,
    ) -> Result<u64, AppError> {
        let device = self
            .runtime
            .world
            .devices
            .get(id)
            .ok_or(AppError::NotFound)?;
        if device.incarnation != incarnation {
            return Err(AppError::Conflict("stale incarnation"));
        }
        let ctx = PluginContext {
            token: crate::session_token(id, device),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        let mut content = self.contents.get(plugin).ok_or(AppError::NotFound)?.clone();
        let revision = content
            .set_device_config(&ctx, expected_revision, config)
            .map_err(|error| match error {
                ContentError::Stale => AppError::Conflict("stale plugin config"),
                _ => AppError::Invalid("invalid plugin config"),
            })?;
        if revision == expected_revision {
            return Ok(revision);
        }
        let snapshot = content.save_state();
        self.change(|engine, state| {
            let decision = engine
                .handle(
                    &state.world,
                    &Event::PluginConfigChanged {
                        device_id: id.into(),
                        incarnation,
                        plugin: plugin.into(),
                        expected_settings_revision: ctx.settings_revision,
                    },
                )
                .map_err(|_| AppError::Conflict("plugin config rejected"))?;
            if !state.accept(decision)? {
                return Err(AppError::Conflict("stale plugin config"));
            }
            state.plugin_states.insert(plugin.into(), Arc::new(snapshot));
            Ok(())
        })?;
        self.contents.insert(plugin.into(), content);
        Ok(revision)
    }

    pub fn plan_plugin_fetches(
        &mut self,
        now_unix: i64,
        today_jst: &str,
    ) -> Result<Vec<FetchJob>, AppError> {
        let contexts: Vec<_> = self
            .runtime
            .world
            .devices
            .iter()
            .map(|(id, device)| PluginContext {
                token: crate::session_token(id, device),
                settings_revision: device.settings_revision,
                mode: device.mode.clone(),
            })
            .collect();
        let mut contents = self.contents.clone();
        let mut jobs = Vec::new();
        let mut states = BTreeMap::new();
        for (id, plugin) in &mut contents {
            let Some(policy) = plugin.fetch_policy(&self.runtime.settings.plugins.value) else {
                continue;
            };
            for ctx in &contexts {
                if let Some(job) = plugin.plan_fetch(ctx, now_unix, today_jst, &policy) {
                    jobs.push(job);
                }
            }
            states.insert(id.clone(), plugin.save_state());
        }
        if !jobs.is_empty() {
            self.change(|_, state| {
                state.plugin_states.extend(states.into_iter().map(|(k,v)|(k,Arc::new(v))));
                Ok(())
            })?;
            self.contents = contents;
        }
        Ok(jobs)
    }

    pub fn cancel_plugin_fetches(&mut self) -> Result<(), AppError> {
        let mut contents = self.contents.clone();
        let states: BTreeMap<_, _> = contents
            .iter_mut()
            .map(|(id, plugin)| {
                plugin.cancel_fetches();
                (id.clone(), plugin.save_state())
            })
            .collect();
        self.change(|_, state| {
            state.plugin_states.extend(states.into_iter().map(|(k,v)|(k,Arc::new(v))));
            Ok(())
        })?;
        self.contents = contents;
        Ok(())
    }

    pub fn prepare_plugin_fetch(&self,job:&FetchJob)->Result<Option<crate::content::FetchPreparation>,AppError>{
        let Some(device)=self.runtime.world.devices.get(&job.device_id) else{return Ok(None);};
        if device.incarnation!=job.incarnation || device.session_epoch!=job.session_epoch || device.settings_revision!=job.settings_revision{return Ok(None);}
        match self.contents.get(&job.plugin).ok_or(AppError::NotFound)?.fetch_preparation(job){
            Ok(prepare)=>Ok(Some(prepare)),Err(ContentError::Stale)=>Ok(None),Err(_)=>Err(AppError::Invalid("plugin fetch preparation invalid")),
        }
    }
    pub fn complete_plugin_fetch(&mut self,job:&FetchJob,now:i64,result:Result<Vec<u8>,String>)->Result<bool,AppError>{
        let Some(prepare)=self.prepare_plugin_fetch(job)? else{return Ok(false);};
        let prepared=prepare(result).map_err(|_|AppError::Invalid("plugin fetch preparation invalid"))?;
        self.apply_prepared_plugin_fetch(job,now,prepared)
    }
    pub fn apply_prepared_plugin_fetch(
        &mut self,
        job: &FetchJob,
        now_unix: i64,
        prepared: crate::content::PreparedFetch,
    ) -> Result<bool, AppError> {
        let Some(device) = self.runtime.world.devices.get(&job.device_id) else {
            return Ok(false);
        };
        if device.incarnation != job.incarnation
            || device.session_epoch != job.session_epoch
            || device.settings_revision != job.settings_revision
        {
            return Ok(false);
        }
        let mut content = self
            .contents
            .get(&job.plugin)
            .ok_or(AppError::NotFound)?
            .clone();
        let pixels = match content.apply_prepared_fetch(job, now_unix, prepared) {
            Ok(pixels) => pixels,
            Err(ContentError::Stale) => return Ok(false),
            Err(_) => return Err(AppError::Invalid("plugin fetch result invalid")),
        };
        let snapshot = content.save_state();
        self.change(|engine, state| {
            let decision = engine
                .handle(
                    &state.world,
                    &Event::PluginDataChanged {
                        device_id: job.device_id.clone(),
                        incarnation: job.incarnation,
                        plugin: job.plugin.clone(),
                    },
                )
                .map_err(|_| AppError::Conflict("plugin data rejected"))?;
            state.accept(decision)?;
            state.plugin_states.insert(job.plugin.clone(), Arc::new(snapshot));
            Ok(())
        })?;
        self.contents.insert(job.plugin.clone(), content);
        let pixels = if self.runtime.world.devices[&job.device_id].mode == Mode::Sub {
            let device = &self.runtime.world.devices[&job.device_id];
            let ctx = PluginContext {
                token: crate::session_token(&job.device_id, device),
                settings_revision: device.settings_revision,
                mode: device.mode.clone(),
            };
            self.contents
                .get(&job.plugin)
                .and_then(|p| p.current_frame(&ctx))
        } else {
            pixels
        };
        if let Some(pixels) = pixels {
            let device = &self.runtime.world.devices[&job.device_id];
            if device.online
                && matches!(device.mode, Mode::Normal | Mode::Sub)
                && device.selected.as_deref() == Some(&job.plugin)
            {
                let token = crate::session_token(&job.device_id, device);
                self.queue_lcd_frame(&token, &job.plugin, pixels)?;
            }
        }
        Ok(true)
    }

    fn queue_selected_cached_frame(&mut self, token: &SessionToken) -> Result<(), AppError> {
        self.queue_selected_frame(token, false)
    }

    fn queue_selected_frame(
        &mut self,
        token: &SessionToken,
        include_loading: bool,
    ) -> Result<(), AppError> {
        let Some(device) = self.runtime.world.devices.get(&token.device_id) else {
            return Ok(());
        };
        // A date observation may already have queued the current display's
        // image. RequestFrame permits only one pending frame per display.
        if !session_current(device, token)
            || device.owner != Owner::Remote
            || !matches!(device.mode, Mode::Normal | Mode::Sub)
            || (device.pending_frame_id.is_some() && !self.supports_latest_delivery(token))
        {
            return Ok(());
        }
        let Some(id) = device.selected.as_ref() else {
            return self.queue_logo_frame(token);
        };
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        let preparing = std::time::Instant::now();
        let now = self.clock.unix_seconds();
        let mut display = crate::display_token(&token.device_id, device);
        display.settings_revision = 0; // Only a changed display, not unrelated settings, needs pixels.
        let id = id.clone();
        let time_key = self.contents.get(&id).and_then(|p| p.time_key(&ctx, now));
        if let Some(plugin) = self.contents.get_mut(&id) {
            plugin.observe_time(&ctx, now);
        }
        let pixels = self.contents.get(&id).and_then(|p| {
            p.current_frame(&ctx)
                .or_else(|| include_loading.then(|| p.loading_frame()))
        });
        self.image_preparation_us = self
            .image_preparation_us
            .saturating_add(preparing.elapsed().as_micros());
        if let Some(pixels) = pixels {
            self.queue_lcd_frame(token, &id, pixels)?;
            if let Some(key) = time_key {
                self.timed_frames
                    .insert(token.device_id.clone(), (display, key));
            }
        }
        Ok(())
    }

    /// Queue the latest minute only after the preceding frame finishes. Late
    /// wakes never replay missed minutes and never invalidate an in-flight send.
    pub(crate) fn timed_refresh_due(&self)->bool{!self.timed_frame_targets().is_empty()}
    fn timed_frame_targets(&self)->Vec<SessionToken>{
        let now=self.clock.unix_seconds();
        self
            .runtime
            .world
            .devices
            .iter()
            .filter_map(|(id, d)| {
                if !d.online
                    || d.owner != Owner::Remote
                    || d.mode != Mode::Normal
                    || (d.pending_frame_id.is_some() && !self.supports_latest_delivery(&crate::session_token(id, d)))
                {
                    return None;
                }
                if self.maintenance_blocks(d.selected.as_deref()) { return None; }
                let plugin = self.contents.get(d.selected.as_ref()?)?;
                let ctx = PluginContext {
                    token: crate::session_token(id, d),
                    settings_revision: d.settings_revision,
                    mode: d.mode.clone(),
                };
                let key = plugin.time_key(&ctx, now)?;
                let mut display = crate::display_token(id, d);
                display.settings_revision = 0;
                (self.timed_frames.get(id) != Some(&(display, key))).then_some(ctx.token)
            })
            .collect()
    }
    pub fn refresh_timed_frames(&mut self) -> Result<usize, AppError> {
        let targets=self.timed_frame_targets();
        for token in &targets {self.queue_selected_cached_frame(token)?;}
        Ok(targets.len())
    }

    fn queue_logo_frame(&mut self, token: &SessionToken) -> Result<(), AppError> {
        if self.supports_latest_delivery(token) {
            self.queue_lcd_latest(token, None, crate::lcd_text::logo_frame())?;
            return Ok(());
        }
        if self.frame_assets.len() >= 128 {
            return Err(AppError::Conflict("transient frame asset limit"));
        }
        let handle = format!(
            "logo:{}:{}",
            token.device_id, self.runtime.world.next_frame_id
        );
        let pixels = self.compose_lcd_frame(&token.device_id, crate::lcd_text::logo_frame());
        self.change(|engine, state| {
            let device = state
                .world
                .devices
                .get(&token.device_id)
                .ok_or(AppError::NotFound)?;
            let decision = engine
                .handle(
                    &state.world,
                    &Event::RequestLogoFrame {
                        token: crate::display_token(&token.device_id, device),
                        asset_handle: handle.clone(),
                    },
                )
                .map_err(|_| AppError::Conflict("logo frame rejected"))?;
            if decision.applied.is_none() {
                return Err(AppError::Conflict("logo frame not ready"));
            }
            state.accept(decision)?;
            Ok(())
        })?;
        self.frame_assets.insert(handle, Arc::new(pixels));
        Ok(())
    }

    pub fn plugin_position(&self, token: &SessionToken) -> Option<PluginPosition> {
        let device = self.runtime.world.devices.get(&token.device_id)?;
        if !session_current(device, token) {
            return None;
        }
        let selected = device.selected.as_ref()?;
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        self.contents.get(selected)?.position(&ctx)
    }

    /// Interpret common input and dispatch plugin-specific controls through
    /// the content contract. No plugin ID is branched on here.
    pub fn button(
        &mut self,
        token: &SessionToken,
        action: ButtonAction,
    ) -> Result<InputResult, AppError> {
        let today = self.clock.today_jst();
        self.button_at(token, action, &today)
    }

    /// The clock is supplied at this boundary so date changes can be tested
    /// without changing plugin Rules or relying on wall-clock test timing.
    pub fn button_at(
        &mut self,
        token: &SessionToken,
        action: ButtonAction,
        today_jst: &str,
    ) -> Result<InputResult, AppError> {
        let device = self
            .runtime
            .world
            .devices
            .get(&token.device_id)
            .ok_or(AppError::NotFound)?;
        if !session_current(device, token) {
            return Err(AppError::Conflict("stale session"));
        }
        self.observe_plugin_date(token, today_jst)?;
        let device = &self.runtime.world.devices[&token.device_id];
        let selected = device.selected.clone();
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        let event = match (&ctx.mode, action) {
            (Mode::Normal, ButtonAction::Next | ButtonAction::Down) => Some(Event::Next {
                token: token.clone(),
            }),
            (Mode::Normal, ButtonAction::Up) => Some(Event::Previous {
                token: token.clone(),
            }),
            (_, ButtonAction::Power)
                if ctx.mode != Mode::Sub
                    || selected
                        .as_ref()
                        .and_then(|id| self.contents.get(id))
                        .and_then(|p| p.position(&ctx))
                        .is_none() =>
            {
                Some(Event::Power {
                    token: token.clone(),
                })
            }
            _ => None,
        };
        if let Some(event) = event {
            self.terminal_event(event)?;
            self.queue_selected_cached_frame(token)?;
            let (selected, display_epoch) = self.selection_for(token).ok_or(AppError::NotFound)?;
            return Ok(InputResult::Selection {
                selected,
                display_epoch,
            });
        }
        let Some(id) = selected else {
            let (selected, display_epoch) = self.selection_for(token).ok_or(AppError::NotFound)?;
            return Ok(InputResult::Selection {
                selected,
                display_epoch,
            });
        };
        if !self.contents.contains_key(&id) || matches!(ctx.mode, Mode::Standby | Mode::Menu) {
            let (selected, display_epoch) = self.selection_for(token).ok_or(AppError::NotFound)?;
            return Ok(InputResult::Selection {
                selected,
                display_epoch,
            });
        }
        let mut plugin = self
            .contents
            .get(&id)
            .ok_or(AppError::Conflict("plugin input unavailable"))?
            .clone();
        let preparing = std::time::Instant::now();
        let plan = plugin.plan(&ctx, action);
        self.image_preparation_us = self
            .image_preparation_us
            .saturating_add(preparing.elapsed().as_micros());
        let plan = match plan {
            Ok(plan) => plan,
            Err(ContentError::Unavailable | ContentError::Unsupported | ContentError::Stale) => {
                let (selected, display_epoch) =
                    self.selection_for(token).ok_or(AppError::NotFound)?;
                return Ok(InputResult::Selection {
                    selected,
                    display_epoch,
                });
            }
            Err(ContentError::InvalidSource) => {
                return Err(AppError::Invalid("invalid plugin source"))
            }
        };
        let work = plan
            .event
            .as_ref()
            .and_then(|_| plugin.button_work(&ctx, action));
        if let Some(event) = plan.event {
            if !self.terminal_event(event)? {
                let (selected, display_epoch) =
                    self.selection_for(token).ok_or(AppError::NotFound)?;
                return Ok(InputResult::Selection {
                    selected,
                    display_epoch,
                });
            }
            if let Some(work) = work {
                let work_ctx = self.command_context();
                self.accept_work_plan(&id, plugin, work, &work_ctx, None)?;
            } else {
                self.contents.insert(id.clone(), plugin);
            }
            if let Some(frame) = plan.frame {
                self.queue_lcd_frame(token, &id, frame)?;
            }
        }
        Ok(InputResult::Position(plan.position))
    }

    fn observe_plugin_date(
        &mut self,
        token: &SessionToken,
        today_jst: &str,
    ) -> Result<(), AppError> {
        let device = self
            .runtime
            .world
            .devices
            .get(&token.device_id)
            .ok_or(AppError::NotFound)?;
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        let mut contents = self.contents.clone();
        let mut states = BTreeMap::new();
        for (id, plugin) in &mut contents {
            if plugin.observe_date(&ctx, today_jst) {
                states.insert(id.clone(), plugin.save_state());
            }
        }
        if !states.is_empty() {
            let changed_selected = ctx.mode == Mode::Normal
                && device
                    .selected
                    .as_ref()
                    .is_some_and(|id| states.contains_key(id));
            let selected = device.selected.clone();
            self.change(|engine, state| {
                state.plugin_states.extend(states.into_iter().map(|(k,v)|(k,Arc::new(v))));
                if changed_selected {
                    let id = selected.as_ref().expect("selected plugin");
                    let decision = engine
                        .handle(
                            &state.world,
                            &Event::PluginDataChanged {
                                device_id: token.device_id.clone(),
                                incarnation: token.incarnation,
                                plugin: id.clone(),
                            },
                        )
                        .map_err(|_| AppError::Conflict("calendar view rejected"))?;
                    state.accept(decision)?;
                }
                Ok(())
            })?;
            self.contents = contents;
            if changed_selected {
                self.queue_selected_cached_frame(token)?;
            }
        }
        Ok(())
    }

    pub fn frame_asset(&self, handle: &str) -> Option<Arc<Vec<u8>>> {
        self.frame_assets.get(handle).cloned()
    }

    pub fn supports_lcd_push(&self, token: &SessionToken) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|d| {
                session_current(d, token)
                    && d.capabilities.contains("lcd")
                    && d.capabilities
                        .contains(crate::protocol::FRAME_BYTES_CAPABILITY)
                    && d.capabilities
                        .contains(crate::protocol::LCD_PUSH_CAPABILITY)
            })
    }

    pub fn supports_latest_delivery(&self, token: &SessionToken) -> bool {
        self.supports_lcd_push(token) || self.runtime.world.devices.get(&token.device_id).is_some_and(|d|
            session_current(d, token)
                && d.capabilities.contains("eink")
                && !d.capabilities.contains("lcd")
                && d.capabilities.contains(crate::protocol::FRAME_BYTES_CAPABILITY))
    }

    pub fn subscribe_lcd_video(
        &mut self,
        token: &SessionToken,
    ) -> Option<tokio::sync::watch::Receiver<Option<Arc<crate::lcd_stream::VideoFrame>>>> {
        if !self.supports_latest_delivery(token) {
            return None;
        }
        let slot = self
            .video_slots
            .entry(token.device_id.clone())
            .or_insert_with(|| Arc::new(std::sync::Mutex::new(crate::lcd_stream::VideoSlot::new(token.clone()))));
        let mut slot=slot.lock().ok()?;
        if slot.session != *token {
            *slot = crate::lcd_stream::VideoSlot::new(token.clone());
        }
        Some(slot.latest.subscribe())
    }

    pub fn lcd_video_is_current(&self, frame: &crate::lcd_stream::VideoFrame) -> bool {
        self.runtime
            .world
            .devices
            .get(&frame.token.device_id)
            .is_some_and(|d| {
                frame_current(d, &frame.token)
                    && self.supports_latest_delivery(&crate::session_token(&frame.token.device_id, d))
                    && (frame.delivery == crate::lcd_stream::DeliveryKind::Ordinary || self.supports_lcd_push(&crate::session_token(&frame.token.device_id, d)))
            })
    }

    pub fn start_lcd_latest_send(&self, frame: &crate::lcd_stream::VideoFrame) -> bool {
        if frame.delivery != crate::lcd_stream::DeliveryKind::Ordinary || !self.lcd_video_is_current(frame) { return false; }
        self.video_slots.get(&frame.token.device_id)
            .and_then(|slot| slot.lock().ok().map(|mut slot| slot.start_ordinary(frame)))
            .unwrap_or(false)
    }

    pub(super) fn queue_lcd_video(&mut self,session:&SessionToken,fps:u32,sequence:u64,generation:u64,pixels:Vec<u8>)->Result<(),AppError>{
        self.subscribe_lcd_video(session).ok_or(AppError::Invalid("unsupported LCD stream"))?;
        {let mut counter=self.frame_counter.lock().map_err(|_|AppError::Conflict("frame allocator unavailable"))?;*counter=(*counter).max(self.runtime.world.next_frame_id);}
        self.queue_published_lcd_video(session,fps,sequence,generation,pixels)?;
        self.runtime.world.next_frame_id=self.runtime.world.next_frame_id.max(*self.frame_counter.lock().map_err(|_|AppError::Conflict("frame allocator unavailable"))?);Ok(())
    }
    pub(crate) fn queue_published_lcd_video(&self,session:&SessionToken,fps:u32,sequence:u64,generation:u64,pixels:Vec<u8>)->Result<(),AppError>{
        if !self.supports_lcd_push(session)||fps==0||fps>crate::lcd_stream::MAX_FPS{return Err(AppError::Invalid("unsupported LCD stream"));}
        let d=self.runtime.world.devices.get(&session.device_id).ok_or(AppError::NotFound)?;
        let mode=match d.mode{Mode::Normal=>"normal",Mode::Sub=>"sub",_=>return Err(AppError::Conflict("not remote"))};
        let frame_id={let mut counter=self.frame_counter.lock().map_err(|_|AppError::Conflict("frame allocator unavailable"))?;let id=*counter;*counter=counter.checked_add(1).ok_or(AppError::Conflict("frame counter exhausted"))?;id};
        let token=FrameToken{device_id:session.device_id.clone(),incarnation:d.incarnation,session_epoch:d.session_epoch,display_epoch:d.display_epoch,screen_owner_epoch:d.screen_owner_epoch,frame_id};
        let pixels=Arc::new(self.compose_lcd_frame(&session.device_id,pixels));
        let slot=self.video_slots.get(&session.device_id).ok_or(AppError::Conflict("video slot unavailable"))?;
        let mut slot=slot.lock().map_err(|_|AppError::Conflict("video slot unavailable"))?;
        if slot.session!=*session{return Err(AppError::Conflict("stale video slot"));}
        slot.publish(crate::lcd_stream::VideoFrame{token,delivery:crate::lcd_stream::DeliveryKind::Video,fps,sequence,generation,mode,eink_refresh:None,pixels});Ok(())
    }

    pub fn lcd_stream_status(&mut self, token: &FrameToken, generation: u64) -> bool {
        let Some(d) = self.runtime.world.devices.get(&token.device_id) else {
            return false;
        };
        if !frame_current(d, token) {
            return false;
        }
        let Some(slot) = self.video_slots.get(&token.device_id) else {return false;};
        let Ok(mut slot)=slot.lock()else{return false;};
        if slot.session.incarnation != token.incarnation
            || slot.session.session_epoch != token.session_epoch
            || slot.generation != generation
            || !slot.latest.borrow().as_ref().is_some_and(|f| f.delivery == crate::lcd_stream::DeliveryKind::Video)
            || token.frame_id < slot.first_issued
            || token.frame_id > slot.last_issued
            || token.frame_id < slot.last_reported
        {
            return false;
        }
        slot.last_reported = token.frame_id;
        let d = self
            .runtime
            .world
            .devices
            .get_mut(&token.device_id)
            .expect("checked");
        let changed = d.last_received_frame != Some(token.frame_id)
            || d.last_rendered_frame != Some(token.frame_id);
        d.last_received_frame = Some(token.frame_id);
        d.last_rendered_frame = Some(token.frame_id);
        self.transient_changed |= self.transient_only && changed;
        // Observational counters are transient; never enter SQLite or grant sending credit.
        true
    }

    /// An ordinary LCD or E-ink still reports ACKs. Keep the
    /// observation monotonic without turning it into delivery credit.
    pub fn lcd_latest_ack(&mut self, token: &FrameToken, rendered: bool) -> bool {
        let Some(d) = self.runtime.world.devices.get(&token.device_id) else { return false; };
        if !frame_current(d, token) || !self.supports_latest_delivery(&crate::session_token(&token.device_id, d)) {
            return false;
        }
        let Some(slot) = self.video_slots.get(&token.device_id) else { return false; };
        let Ok(slot) = slot.lock() else { return false; };
        if slot.session.incarnation != token.incarnation || slot.session.session_epoch != token.session_epoch
            || slot.generation != 0
            || !slot.latest.borrow().as_ref().is_some_and(|f| f.delivery == crate::lcd_stream::DeliveryKind::Ordinary)
            || !slot.ordinary_started.contains(&token.frame_id)
        { return false; }
        let d = self.runtime.world.devices.get_mut(&token.device_id).expect("checked");
        let previous = if rendered { d.last_rendered_frame } else { d.last_received_frame };
        if previous.is_some_and(|id| id >= token.frame_id) { return false; }
        d.last_received_frame = Some(d.last_received_frame.map_or(token.frame_id, |id| id.max(token.frame_id)));
        if rendered { d.last_rendered_frame = Some(token.frame_id); }
        self.transient_changed |= self.transient_only;
        true
    }

    pub fn frame_waiting_for_render(&self, token: &SessionToken, frame_id: u64) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| {
                session_current(device, token)
                    && device.pending_frame_id == Some(frame_id)
                    && device.last_rendered_frame != Some(frame_id)
            })
    }

    pub fn binary_frames(&self, token: &SessionToken) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| {
                session_current(device, token)
                    && device
                        .capabilities
                        .contains(crate::protocol::FRAME_BYTES_CAPABILITY)
            })
    }

    pub fn session_mode(&self, token: &SessionToken) -> Option<&'static str> {
        let device = self.runtime.world.devices.get(&token.device_id)?;
        if !session_current(device, token) {
            return None;
        }
        Some(match device.mode {
            Mode::Normal => "normal",
            Mode::Sub => "sub",
            Mode::Menu => "menu",
            Mode::Standby => "standby",
        })
    }

    pub fn frame_eink_refresh(&self, token: &FrameToken) -> Option<crate::protocol::EinkRefresh> {
        let device = self.runtime.world.devices.get(&token.device_id)?;
        if !frame_current(device, token)
            || device.mode != Mode::Normal
            || !device.capabilities.contains("eink")
            || device.capabilities.contains("lcd")
            || !device
                .capabilities
                .contains(crate::protocol::EINK_CLOCK_CAPABILITY)
        {
            return None;
        }
        let ctx = PluginContext {
            token: crate::session_token(&token.device_id, device),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        self.contents
            .get(device.selected.as_ref()?)?
            .eink_refresh(&ctx)
    }

    pub fn frame_mode(&self, token: &FrameToken) -> Option<&'static str> {
        let device = self.runtime.world.devices.get(&token.device_id)?;
        if !frame_current(device, token) {
            return None;
        }
        match device.mode {
            Mode::Normal => Some("normal"),
            Mode::Sub => Some("sub"),
            _ => None,
        }
    }

    pub fn frame_is_current(&self, token: &FrameToken) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| {
                frame_current(device, token) && device.pending_frame_id == Some(token.frame_id)
            })
    }

    /// Past/current-generation ACKs may be delayed or repeated. Values beyond
    /// any allocated frame/display/owner generation are protocol violations.
    pub fn frame_ack_within_known_generations(&self, token: &FrameToken) -> bool {
        token.frame_id > 0
            && token.frame_id < self.frame_counter.lock().map(|n|*n).unwrap_or(self.runtime.world.next_frame_id)
            && self
                .runtime
                .world
                .devices
                .get(&token.device_id)
                .is_some_and(|device| {
                    token.incarnation == device.incarnation
                        && token.session_epoch == device.session_epoch
                        && token.display_epoch > 0
                        && token.display_epoch <= device.display_epoch
                        && token.screen_owner_epoch <= device.screen_owner_epoch
                })
    }

    pub fn hello(
        &mut self,
        id: &str,
        capabilities: BTreeSet<String>,
        screen_owner_epoch: u64,
    ) -> Result<SessionToken, AppError> {
        self.hello_impl(id, capabilities, screen_owner_epoch, None)
    }

    pub(crate) fn hello_from_connection(
        &mut self,
        id: &str,
        capabilities: BTreeSet<String>,
        screen_owner_epoch: u64,
        accept_order: std::num::NonZeroU64,
    ) -> Result<SessionToken, AppError> {
        self.hello_impl(id, capabilities, screen_owner_epoch, Some(accept_order))
    }

    fn hello_impl(
        &mut self,
        id: &str,
        capabilities: BTreeSet<String>,
        screen_owner_epoch: u64,
        accept_order: Option<std::num::NonZeroU64>,
    ) -> Result<SessionToken, AppError> {
        if !valid_name(id, 64)
            || capabilities.is_empty()
            || capabilities.len() > 32
            || capabilities.iter().any(|cap| !valid_name(cap, 40))
            || (!capabilities.contains("lcd") && !capabilities.contains("eink"))
        {
            return Err(AppError::Invalid("invalid device hello"));
        }
        if let Some(order) = accept_order {
            if self.hello_accept_order.get(id).is_some_and(|previous| order <= *previous) {
                return Err(AppError::Conflict("stale_connection"));
            }
            // Reserve before any fallible application/storage work. The caller
            // already holds the App lock, so comparison+reservation+Hello are
            // atomic with respect to all competing connection handlers.
            self.hello_accept_order.insert(id.to_owned(), order);
        }
        let token = self.change(|engine, state| {
            let decision = engine
                .handle(
                    &state.world,
                    &Event::Hello {
                        device_id: id.to_owned(),
                        capabilities,
                        screen_owner_epoch,
                    },
                )
                .map_err(|_| AppError::Conflict("device hello rejected"))?;
            state.accept(decision)?;
            let device = state.world.devices.get(id).ok_or(AppError::NotFound)?;
            if state
                .settings
                .devices
                .get(id)
                .is_none_or(|record| record.desired.incarnation != device.incarnation)
            {
                state
                    .settings
                    .devices
                    .insert(id.to_owned(), DeviceRecord::new(device.incarnation));
            }
            state.settings.next_incarnation = state.world.next_incarnation;
            state.settings.devices.get_mut(id).expect("inserted").online = true;
            let token = crate::session_token(id, device);
            let snapshot = state.settings.devices[id].desired.clone();
            state.enqueue(vec![Effect::SyncSettings {
                token: token.clone(),
                snapshot,
            }])?;
            Ok(token)
        })?;
        for plugin in self.contents.values_mut() {
            plugin.session_reset(id, token.incarnation);
        }
        let today = self.clock.today_jst();
        self.observe_plugin_date(&token, &today)?;
        if self.supports_latest_delivery(&token){self.subscribe_lcd_video(&token);}
        self.queue_selected_cached_frame(&token)?;
        eprintln!(
            "picomado device_hello id={} incarnation={} session_epoch={}",
            token.device_id, token.incarnation, token.session_epoch
        );
        Ok(token)
    }

    pub fn disconnect(&mut self, token: SessionToken) -> Result<bool, AppError> {
        let changed = self.change(|engine, state| {
            let decision = engine
                .handle(
                    &state.world,
                    &Event::Disconnect {
                        token: token.clone(),
                    },
                )
                .map_err(|_| AppError::Conflict("disconnect rejected"))?;
            let changed = state.accept(decision)?;
            if changed {
                if let Some(record) = state.settings.devices.get_mut(&token.device_id) {
                    record.online = false;
                }
            }
            Ok(changed)
        })?;
        if changed {
            for plugin in self.contents.values_mut() {
                plugin.session_reset(&token.device_id, token.incarnation);
            }
            eprintln!(
                "picomado device_disconnect id={} incarnation={} session_epoch={}",
                token.device_id, token.incarnation, token.session_epoch
            );
        }
        Ok(changed)
    }

    pub fn delete_device(&mut self, id: &str, incarnation: u64) -> Result<(), AppError> {
        self.change(|engine, state| {
            let device = state.world.devices.get(id).ok_or(AppError::NotFound)?;
            if device.incarnation != incarnation {
                return Err(AppError::Conflict("stale device incarnation"));
            }
            let decision = engine
                .handle(
                    &state.world,
                    &Event::Delete {
                        device_id: id.to_owned(),
                    },
                )
                .map_err(|_| AppError::Conflict("device deletion rejected"))?;
            state.accept(decision)?;
            state.settings.devices.remove(id);
            Ok(())
        })?;
        for plugin in self.contents.values_mut() {
            plugin.delete_device(id);
        }
        let snapshots: BTreeMap<_, _> = self
            .contents
            .iter()
            .map(|(name, plugin)| (name.clone(), plugin.save_state()))
            .collect();
        self.change(|_, state| {
            state.plugin_states.extend(snapshots.into_iter().map(|(k,v)|(k,Arc::new(v))));
            Ok(())
        })?;
        Ok(())
    }

    pub fn set_enabled(
        &mut self,
        id: &str,
        incarnation: u64,
        expected_revision: u64,
        plugin: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut contents = self.contents.clone();
        let (reselected, token) = self.change(|engine, state| {
            let device = state.world.devices.get(id).ok_or(AppError::NotFound)?;
            if device.incarnation != incarnation {
                return Err(AppError::Conflict("stale device incarnation"));
            }
            let previous_epoch = device.display_epoch;
            let decision = engine
                .handle(
                    &state.world,
                    &Event::SetEnabled {
                        device_id: id.to_owned(),
                        plugin: plugin.to_owned(),
                        enabled,
                        expected_settings_revision: expected_revision,
                    },
                )
                .map_err(|_| AppError::Conflict("plugin update rejected"))?;
            state.accept(decision)?;
            let device = &state.world.devices[id];
            let reselected = device.display_epoch != previous_epoch;
            let token = crate::session_token(id, device);
            if reselected {
                for content in contents.values_mut() {
                    content.session_reset(id, incarnation);
                }
                state
                    .plugin_states
                    .extend(contents.iter().map(|(id, p)| (id.clone(), Arc::new(p.save_state()))));
            }
            Ok((reselected, token))
        })?;
        self.contents = contents;
        if reselected {
            let today = self.clock.today_jst();
            self.observe_plugin_date(&token, &today)?;
            self.queue_selected_frame(&token, true)?;
        }
        Ok(())
    }

    pub fn set_device_settings(
        &mut self,
        id: &str,
        incarnation: u64,
        expected_revision: u64,
        desired: DeviceSettings,
    ) -> Result<u64, AppError> {
        self.change(|_, state| {
            let revision = state
                .settings
                .update_device(id, incarnation, expected_revision, desired)
                .map_err(settings_error)?;
            state.world.revision += 1;
            if let Some(device) = state.world.devices.get(id) {
                if device.online {
                    let token = crate::session_token(id, device);
                    let snapshot = state.settings.devices[id].desired.clone();
                    state.enqueue(vec![Effect::SyncSettings { token, snapshot }])?;
                }
            }
            Ok(revision)
        })
    }

    pub fn heartbeat(
        &mut self,
        token: &SessionToken,
        applied_revision: u64,
    ) -> Result<(), AppError> {
        if self.settings_confirmation(token, applied_revision, false)?.is_none() {
            return Ok(());
        }
        self.change(|_, state| {
            let device = state
                .world
                .devices
                .get(&token.device_id)
                .ok_or(AppError::NotFound)?;
            if !session_current(device, token) {
                return Err(AppError::Conflict("stale session"));
            }
            state
                .settings
                .devices
                .get_mut(&token.device_id)
                .ok_or(AppError::NotFound)?
                .heartbeat(token.incarnation, applied_revision)
                .map_err(settings_error)?;
            Ok(())
        })
    }

    /// Pico has written settings to NVM; application of the revision is still
    /// confirmed separately by a heartbeat from the current session.
    pub fn settings_stored(&mut self, token: &SessionToken, revision: u64) -> Result<(), AppError> {
        if self.settings_confirmation(token, revision, true)?.is_none() {
            return Ok(());
        }
        self.change(|_, state| {
            let device = state
                .world
                .devices
                .get(&token.device_id)
                .ok_or(AppError::NotFound)?;
            if !session_current(device, token) {
                return Err(AppError::Conflict("stale session"));
            }
            state
                .settings
                .devices
                .get_mut(&token.device_id)
                .ok_or(AppError::NotFound)?
                .mark_stored(token.incarnation, revision)
                .map_err(settings_error)?;
            Ok(())
        })
    }

    pub fn terminal_event(&mut self, event: Event) -> Result<bool, AppError> {
        if !matches!(
            event,
            Event::Next { .. }
                | Event::Previous { .. }
                | Event::Power { .. }
                | Event::EnterSub { .. }
                | Event::PluginInput { .. }
                | Event::ReceiptAck { .. }
                | Event::RenderedAck { .. }
        ) {
            return Err(AppError::Invalid("unsupported terminal event"));
        }
        use crate::observation::{Component, Phase};
        let operation = self.record_observation(Component::Input, Phase::Received, 0, 0, self.runtime.world.revision, 0);
        let decision = match self.engine.handle(&self.runtime.world, &event) {
            Ok(decision) => decision,
            Err(_) => {
                self.record_observation(Component::Input, Phase::Rejected, operation, 0, self.runtime.world.revision, 0);
                return Err(AppError::Conflict("terminal event rejected"));
            }
        };
        self.record_observation(Component::Input, Phase::Dispatched, operation, 0, decision.state.revision, decision.effects.len() as u64);
        // A delayed/duplicate ACK is a Rule no-op, so it must not create a
        // SQLite write on every retry. State-changing events still commit.
        if decision.applied.is_none() {
            self.record_observation(Component::Input, Phase::Completed, operation, 0, self.runtime.world.revision, 0);
            return Ok(false);
        }
        self.observation_parent = operation;
        let result = self.change(|_, state| state.accept(decision));
        self.observation_parent = 0;
        self.record_observation(Component::Input, if result.is_ok() { Phase::Completed } else { Phase::Error }, operation, 0, self.runtime.world.revision, 1);
        let changed = result?;
        if changed && matches!(event, Event::RenderedAck { .. }) {
            self.queue_due_content_refreshes()?;
        }
        if changed {
            match &event {
                Event::ReceiptAck { token } => eprintln!(
                    "picomado frame_received id={} frame_id={}",
                    token.device_id, token.frame_id
                ),
                Event::RenderedAck { token } => eprintln!(
                    "picomado frame_rendered id={} frame_id={}",
                    token.device_id, token.frame_id
                ),
                _ => {}
            }
        }
        Ok(changed)
    }

    pub fn begin_fetch(
        &mut self,
        device_id: &str,
        incarnation: u64,
        key: FetchKey,
    ) -> Result<u64, AppError> {
        if key.key.is_empty() || key.key.len() > 256 {
            return Err(AppError::Invalid("invalid fetch key"));
        }
        self.change(|engine, state| {
            let device = state
                .world
                .devices
                .get(device_id)
                .ok_or(AppError::NotFound)?;
            if device.incarnation != incarnation {
                return Err(AppError::Conflict("stale device incarnation"));
            }
            let decision = engine
                .handle(
                    &state.world,
                    &Event::BeginFetch {
                        token: crate::device_token(device_id, device),
                        key,
                    },
                )
                .map_err(|_| AppError::Conflict("fetch rejected"))?;
            if decision.applied.is_none() {
                return Err(AppError::Conflict("fetch not applicable"));
            }
            let id = state.next_effect_id;
            state.accept(decision)?;
            Ok(id)
        })
    }

    /// A renderer has selected a completed asset handle. This creates a
    /// SendFrame effect; it does not claim any bytes were delivered or drawn.
    pub fn request_frame(
        &mut self,
        token: &SessionToken,
        plugin: &str,
        asset_handle: &str,
    ) -> Result<FrameToken, AppError> {
        if asset_handle.is_empty() || asset_handle.len() > 256 || asset_handle.contains('\0') {
            return Err(AppError::Invalid("invalid asset handle"));
        }
        let frame=self.change(|engine, state| {
            let device = state
                .world
                .devices
                .get(&token.device_id)
                .ok_or(AppError::NotFound)?;
            if !session_current(device, token) {
                return Err(AppError::Conflict("stale session"));
            }
            let decision = engine
                .handle(
                    &state.world,
                    &Event::RequestFrame {
                        token: crate::display_token(&token.device_id, device),
                        plugin: plugin.to_owned(),
                        asset_handle: asset_handle.to_owned(),
                    },
                )
                .map_err(|_| AppError::Conflict("frame rejected"))?;
            let frame = decision
                .effects
                .iter()
                .find_map(|effect| {
                    if let Effect::SendFrame { token, .. } = effect {
                        Some(token.clone())
                    } else {
                        None
                    }
                })
                .ok_or(AppError::Conflict("frame not applicable"))?;
            state.accept(decision)?;
            Ok(frame)
        })?;
        Ok(frame)
    }

    /// A future plugin executor reports fetch completion by outbox ID. The
    /// result and acknowledgement of that effect commit atomically.
    pub fn resolve_fetch(
        &mut self,
        effect_id: u64,
        result: Result<String, String>,
    ) -> Result<bool, AppError> {
        self.change(|engine, state| {
            let Some(Effect::Fetch(token)) = state.outbox.get(&effect_id).cloned() else {
                return Err(AppError::NotFound);
            };
            state.outbox.remove(&effect_id);
            if !effect_current(&state.world, &state.settings, &Effect::Fetch(token.clone())) {
                return Ok(false);
            }
            let event = match result {
                Ok(snapshot_id) => Event::FetchCompleted { token, snapshot_id },
                Err(error_kind) => Event::FetchFailed { token, error_kind },
            };
            let decision = engine
                .handle(&state.world, &event)
                .map_err(|_| AppError::Conflict("fetch result rejected"))?;
            state.accept(decision)
        })
    }

    pub fn set_plugin_settings(
        &mut self,
        expected_revision: u64,
        desired: PluginSettings,
    ) -> Result<u64, AppError> {
        self.change(|_, state| {
            let revision = state
                .settings
                .update_plugins(expected_revision, desired)
                .map_err(settings_error)?;
            state.world.revision += 1;
            Ok(revision)
        })
    }

    pub fn set_order(
        &mut self,
        expected_revision: u64,
        order: Vec<String>,
    ) -> Result<u64, AppError> {
        if !valid_order(&order, &self.plugin_catalog) {
            return Err(AppError::Invalid("invalid plugin order"));
        }
        self.change(|_, state| {
            if state.order_revision != expected_revision {
                return Err(AppError::Conflict("stale order revision"));
            }
            if state.world.plugin_order != order {
                state.world.plugin_order = order;
                state.world.revision += 1;
                state.order_revision += 1;
            }
            Ok(state.order_revision)
        })
    }

    pub fn view(&self) -> AppView {
        AppView {
            order_revision: self.runtime.order_revision,
            order: self.runtime.world.plugin_order.clone(),
            plugins_revision: self.runtime.settings.plugins.revision,
            plugins: self.runtime.settings.plugins.value.clone(),
            devices: self
                .runtime
                .world
                .devices
                .iter()
                .filter_map(|(id, device)| {
                    let record = self.runtime.settings.devices.get(id)?;
                    Some(DeviceView {
                        id: id.clone(),
                        incarnation: device.incarnation,
                        online: device.online,
                        selected: device.selected.clone(),
                        capabilities: device.capabilities.clone(),
                        enabled: device.enabled.clone(),
                        enabled_revision: device.settings_revision,
                        desired_revision: record.desired.revision,
                        confirmed_revision: record.confirmed.revision,
                        stored_revision: record.stored_revision,
                        desired: record.desired.value.clone(),
                        pending: record.pending(),
                        last_received_frame: device.last_received_frame,
                        last_rendered_frame: device.last_rendered_frame,
                        plugin_configs: self
                            .contents
                            .iter()
                            .filter_map(|(plugin, content)| {
                                let config = content.device_config(id, device.incarnation);
                                (config != serde_json::Value::Null)
                                    .then(|| (plugin.clone(), config))
                            })
                            .collect(),
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceView {
    pub id: String,
    pub incarnation: u64,
    pub online: bool,
    pub selected: Option<String>,
    pub capabilities: BTreeSet<String>,
    pub enabled: BTreeSet<String>,
    pub enabled_revision: u64,
    pub desired_revision: u64,
    pub confirmed_revision: u64,
    pub stored_revision: u64,
    pub desired: DeviceSettings,
    pub pending: bool,
    pub last_received_frame: Option<u64>,
    pub last_rendered_frame: Option<u64>,
    pub plugin_configs: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppView {
    pub order_revision: u64,
    pub order: Vec<String>,
    pub plugins_revision: u64,
    pub plugins: PluginSettings,
    pub devices: Vec<DeviceView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_delivery(app: &App, id: &str) -> (FrameToken, Arc<Vec<u8>>) {
        app.pending_effects()
            .into_iter()
            .find_map(|(_, effect)| match effect {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } if token.device_id == id && app.frame_is_current(&token) => {
                    Some((token, app.frame_asset(&asset_handle).unwrap()))
                }
                _ => None,
            })
            .expect("current selection must queue a frame")
    }
    fn web_enabled(app: &mut App, id: &str, plugin: &str, enabled: bool) {
        let d = &app.runtime.world.devices[id];
        let inc = d.incarnation;
        let rev = d.settings_revision;
        app.set_enabled(id, inc, rev, plugin, enabled).unwrap();
    }
    #[test]
    fn web_selection_delivers_immediately_rejects_old_video_and_fetch_and_keeps_valid_selection() {
        use crate::application::{PlaybackOffer, PlaybackTarget};
        use crate::plugin_jobs::PlaybackSpec;
        for cap in ["lcd", "eink"] {
            let temp = tempfile::tempdir().unwrap();
            let mut app = App::open_with_clock(
                &temp.path().join("state.db"),
                Arc::new(SwitchDayClock(std::sync::atomic::AtomicU8::new(0))),
            )
            .unwrap();
            let token = app.hello("desk", [cap.into()].into(), 1).unwrap();
            let old = current_delivery(&app, "desk").0;
            let target = crate::display_token("desk", &app.runtime.world.devices["desk"]);
            let jobs = app.plan_plugin_fetches(0, "2026-10-12").unwrap();
            app.publish_plugin_source(
                "weather",
                &token,
                1,
                include_bytes!("../assets/jma-tokyo-20261012.json"),
                "cached",
                "2026-10-12",
            )
            .unwrap();
            web_enabled(&mut app, "desk", "caricature", false);
            web_enabled(&mut app, "desk", "rain-radar", false);
            assert_eq!(
                app.runtime.world.devices["desk"].display_epoch,
                old.display_epoch
            );
            assert!(app.frame_is_current(&old));
            web_enabled(&mut app, "desk", "youtube", false);
            let (fresh, pixels) = current_delivery(&app, "desk");
            assert_eq!(
                app.runtime.world.devices["desk"].selected.as_deref(),
                Some("weather")
            );
            assert_eq!(app.runtime.world.devices["desk"].mode, Mode::Normal);
            assert!(fresh.display_epoch > old.display_epoch && !app.frame_is_current(&old));
            assert_eq!(pixels.len(), if cap == "lcd" { 25600 } else { 3813 });
            let offer = PlaybackOffer {
                plugin: "youtube".into(),
                spec: PlaybackSpec {
                    asset_id: "old".into(),
                    generation: 1,
                    fps: 10,
                    frame_count: 10,
                    started_mono_ms: 0,
                    source: serde_json::Value::Null,
                },
                targets: vec![PlaybackTarget { token: target }],
            };
            assert_eq!(
                app.accept_playback_pixels(&offer, vec![0; 25600]).unwrap(),
                0
            );
            for job in jobs {
                assert!(!app
                    .complete_plugin_fetch(
                        &job,
                        1,
                        Ok(include_bytes!("../assets/jma-tokyo-20261012.json").to_vec())
                    )
                    .unwrap());
            }
            assert!(!app
                .terminal_event(Event::RenderedAck { token: old })
                .unwrap());
            assert_eq!(current_delivery(&app, "desk").0, fresh);
        }
    }
    #[test]
    fn web_off_resets_browse_and_all_off_logo_survives_offline_reconnect() {
        for cap in ["lcd", "eink"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("state.db");
            let clock = Arc::new(SwitchDayClock(std::sync::atomic::AtomicU8::new(0)));
            let mut app = App::open_with_clock(&path, clock.clone()).unwrap();
            let token = app.hello("desk", [cap.into()].into(), 1).unwrap();
            app.publish_plugin_source(
                "weather",
                &token,
                1,
                include_bytes!("../assets/jma-tokyo-20261012.json"),
                "cached",
                "2026-10-12",
            )
            .unwrap();
            for id in ["youtube", "caricature", "rain-radar", "clock"] {
                web_enabled(&mut app, "desk", id, false);
            }
            app.button(&token, ButtonAction::Sub).unwrap();
            app.button(&token, ButtonAction::Down).unwrap();
            assert_eq!(app.plugin_position(&token).unwrap().index, 1);
            let epoch = app.runtime.world.devices["desk"].display_epoch;
            web_enabled(&mut app, "desk", "caricature", true);
            assert_eq!(app.runtime.world.devices["desk"].display_epoch, epoch);
            assert_eq!(app.plugin_position(&token).unwrap().index, 1);
            web_enabled(&mut app, "desk", "weather", false);
            assert_eq!(app.runtime.world.devices["desk"].mode, Mode::Normal);
            web_enabled(&mut app, "desk", "weather", true);
            web_enabled(&mut app, "desk", "caricature", false);
            assert_eq!(app.plugin_position(&token).unwrap().index, 0);
            assert_eq!(
                app.plugin_position(&token).unwrap().target_date,
                "2026-10-12"
            );
            web_enabled(&mut app, "desk", "weather", false);
            let (logo, pixels) = current_delivery(&app, "desk");
            assert_eq!(app.runtime.world.devices["desk"].selected, None);
            let expected = crate::lcd_text::logo_frame();
            assert_eq!(
                pixels.as_slice(),
                if cap == "lcd" {
                    expected
                } else {
                    crate::eink::from_lcd(&expected)
                }
            );
            assert_eq!(app.frame_mode(&logo), Some("normal"));
            app.disconnect(token.clone()).unwrap();
            web_enabled(&mut app, "desk", "weather", true);
            assert!(!app
                .pending_effects()
                .iter()
                .any(|(_, e)| matches!(e, Effect::SendFrame { .. })));
            drop(app);
            let mut app = App::open_with_clock(&path, clock).unwrap();
            let next = app.hello("desk", [cap.into()].into(), 2).unwrap();
            assert_eq!(
                app.selection_for(&next).unwrap().0.as_deref(),
                Some("weather")
            );
            assert_eq!(
                current_delivery(&app, "desk").0.session_epoch,
                next.session_epoch
            );
        }
    }

    #[test]
    fn fresh_plugins_deliver_distinct_idle_images_and_cancel_old_generation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fresh-status.db");
        let mut legacy = App::open(&path).unwrap();
        legacy
            .change(|_, state| {
                state
                    .plugin_states
                    .insert("youtube".into(), Arc::new(serde_json::Value::Null));
                state
                    .plugin_states
                    .insert("caricature".into(), Arc::new(serde_json::Value::Null));
                Ok(())
            })
            .unwrap();
        drop(legacy);
        let mut app = App::open(&path).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let frame = |app: &App| {
            app.pending_effects()
                .into_iter()
                .find_map(|(_, effect)| {
                    if let Effect::SendFrame {
                        token,
                        asset_handle,
                    } = effect
                    {
                        Some((token, app.frame_asset(&asset_handle).unwrap()))
                    } else {
                        None
                    }
                })
                .expect("selected idle plugin must provide pixels")
        };
        let (youtube, youtube_pixels) = frame(&app);
        assert_eq!(youtube_pixels.len(), crate::protocol::LCD_FRAME_BYTES);
        app.button(&token, ButtonAction::Next).unwrap();
        let (caricature, caricature_pixels) = frame(&app);
        assert_eq!(
            app.selection_for(&token).unwrap().0.as_deref(),
            Some("caricature")
        );
        assert_eq!(caricature_pixels.len(), crate::protocol::LCD_FRAME_BYTES);
        assert_ne!(youtube_pixels, caricature_pixels);
        assert!(!app.frame_is_current(&youtube));
        assert!(!app
            .terminal_event(Event::RenderedAck { token: youtube })
            .unwrap());
        assert!(app
            .terminal_event(Event::RenderedAck {
                token: caricature.clone()
            })
            .unwrap());
        assert_eq!(
            app.view().devices[0].last_rendered_frame,
            Some(caricature.frame_id)
        );
    }

    #[test]
    fn ignored_frame_acks_do_not_write_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let mut app =
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap();
        let session = app.hello("desk", caps(), 1).unwrap();
        let frame = app
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![0; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        assert!(app
            .terminal_event(Event::RenderedAck {
                token: frame.clone()
            })
            .unwrap());
        let changes = |app: &App| {
            app.connection.lock().unwrap()
                .query_row("SELECT total_changes()", [], |row| row.get::<_, u64>(0))
                .unwrap()
        };
        let before = changes(&app);
        for _ in 0..100 {
            assert!(!app
                .terminal_event(Event::RenderedAck {
                    token: frame.clone()
                })
                .unwrap());
        }
        assert_eq!(changes(&app), before);
        app.button(&session, ButtonAction::Next).unwrap();
        let before = changes(&app);
        assert!(!app
            .terminal_event(Event::ReceiptAck { token: frame })
            .unwrap());
        assert_eq!(changes(&app), before);
        assert_eq!(
            app.view().devices[0].selected.as_deref(),
            Some("caricature")
        );
    }

    struct SwitchDayClock(std::sync::atomic::AtomicU8);

    impl crate::fetch::Clock for SwitchDayClock {
        fn unix_seconds(&self) -> i64 {
            0
        }

        fn today_jst(&self) -> String {
            match self.0.load(std::sync::atomic::Ordering::SeqCst) {
                0 => "2026-10-12",
                1 => "2026-10-13",
                _ => "2026-10-19",
            }
            .into()
        }
    }

    #[test]
    fn reconnect_after_jst_day_change_queues_one_current_or_error_frame() {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(SwitchDayClock(std::sync::atomic::AtomicU8::new(0)));
        let mut app = crate::test_support::manual_frames_app_with_clock(
            &temp.path().join("state.db"),
            clock.clone(),
        )
        .unwrap();
        app.set_order(
            1,
            ["weather", "rain-radar", "youtube", "caricature", "clock"]
                .map(str::to_owned)
                .to_vec(),
        )
        .unwrap();
        let first = app.hello("desk", caps(), 1).unwrap();
        let source = include_bytes!("../assets/jma-tokyo-20261012.json");
        app.publish_plugin_source("weather", &first, 1, source, "cached", "2026-10-12")
            .unwrap();
        app.disconnect(first).unwrap();

        clock.0.store(1, std::sync::atomic::Ordering::SeqCst);
        let second = app.hello("desk", caps(), 2).unwrap();
        let set =
            crate::weather::ForecastSet::tokyo_from_cache(source, "cached", "2026-10-12").unwrap();
        let frames: Vec<_> = app
            .pending_effects()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } => Some((token, app.frame_asset(&asset_handle).unwrap())),
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 1);
        assert!(app.frame_is_current(&frames[0].0));
        assert_eq!(
            frames[0].1.as_ref(),
            &crate::weather::render_lcd(&set, &set.days[1]).unwrap()
        );
        assert_eq!(
            app.plugin_position(&second).unwrap().target_date,
            "2026-10-13"
        );
        let day_two_frame = frames[0].0.clone();
        app.disconnect(second).unwrap();

        clock.0.store(2, std::sync::atomic::Ordering::SeqCst);
        let third = app.hello("desk", caps(), 3).unwrap();
        assert!(app.plugin_position(&third).is_none());
        assert!(!app.frame_is_current(&day_two_frame));
        let frames: Vec<_> = app
            .pending_effects()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                Effect::SendFrame { asset_handle, .. } => app.frame_asset(&asset_handle),
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 1);
        let location = crate::weather::LOCATIONS
            .iter()
            .find(|location| location.id == "tokyo-station")
            .unwrap();
        assert_eq!(
            frames[0].as_ref(),
            &crate::weather::render_error_lcd(location)
        );
    }

    fn caps() -> BTreeSet<String> {
        ["lcd", "buttons4"].iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn initial_plugins_accept_inputs_in_every_mode() {
        let actions = [
            ButtonAction::Next,
            ButtonAction::Up,
            ButtonAction::Down,
            ButtonAction::Sub,
            ButtonAction::Power,
        ];
        let modes = [Mode::Normal, Mode::Sub, Mode::Menu, Mode::Standby];
        for (plugin_index, plugin) in DEFAULT_ORDER.iter().enumerate() {
            for mode in &modes {
                for action in actions {
                    let temp = tempfile::tempdir().unwrap();
                    let mut app =
                        crate::test_support::manual_frames_app(&temp.path().join("state.db"))
                            .unwrap();
                    let token = app.hello("desk", caps(), 1).unwrap();
                    for _ in 0..plugin_index {
                        app.button(&token, ButtonAction::Next).unwrap();
                    }
                    assert_eq!(app.view().devices[0].selected.as_deref(), Some(*plugin));
                    if *plugin == "weather" {
                        app.publish_plugin_source(
                            "weather",
                            &token,
                            1,
                            include_bytes!("../assets/jma-tokyo-20261012.json"),
                            "matrix",
                            "2026-10-12",
                        )
                        .unwrap();
                    }
                    app.runtime.world.devices.get_mut("desk").unwrap().mode = mode.clone();
                    let result = app.button(&token, action);
                    assert!(result.is_ok(), "{plugin} {mode:?} {action:?}: {result:?}");
                    if *mode == Mode::Normal && action == ButtonAction::Up {
                        assert_eq!(
                            app.view().devices[0].selected.as_deref(),
                            Some(
                                DEFAULT_ORDER[(plugin_index + DEFAULT_ORDER.len() - 1)
                                    % DEFAULT_ORDER.len()]
                            )
                        );
                    }
                    if *mode == Mode::Normal && action == ButtonAction::Down {
                        assert_eq!(
                            app.view().devices[0].selected.as_deref(),
                            Some(DEFAULT_ORDER[(plugin_index + 1) % DEFAULT_ORDER.len()])
                        );
                    }
                    if *mode == Mode::Normal && action == ButtonAction::Sub && *plugin != "weather"
                    {
                        assert_eq!(app.view().devices[0].selected.as_deref(), Some(*plugin));
                        assert_eq!(app.runtime.world.devices["desk"].mode, Mode::Normal);
                    }
                }
            }
        }
    }

    #[test]
    fn restores_offline_settings_order_and_new_incarnation_after_delete() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let old_incarnation;
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            let token = app.hello("desk", caps(), 1).unwrap();
            old_incarnation = token.incarnation;
            let desired = DeviceSettings {
                wifi_timeout_seconds: 25,
                ..DeviceSettings::default()
            };
            assert_eq!(
                app.set_device_settings("desk", old_incarnation, 1, desired)
                    .unwrap(),
                2
            );
            app.set_order(
                1,
                ["weather", "youtube", "caricature", "rain-radar", "clock"]
                    .map(str::to_owned)
                    .to_vec(),
            )
            .unwrap();
        }
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            let view = app.view();
            assert!(!view.devices[0].online);
            assert_eq!(view.devices[0].desired.wifi_timeout_seconds, 25);
            assert!(view.devices[0].pending);
            assert_eq!(view.order[0], "weather");
            app.delete_device("desk", old_incarnation).unwrap();
        }
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            assert!(app.view().devices.is_empty());
            let new = app.hello("desk", caps(), 1).unwrap();
            assert!(new.incarnation > old_incarnation);
            assert_eq!(app.view().devices[0].desired_revision, 1);
            assert_eq!(app.view().devices[0].desired.wifi_timeout_seconds, 15);
            assert_eq!(app.view().devices[0].selected.as_deref(), Some("weather"));
            assert!(matches!(
                app.set_device_settings("desk", old_incarnation, 2, DeviceSettings::default()),
                Err(AppError::Conflict(_))
            ));
        }
    }

    #[test]
    fn invalid_or_stale_changes_leave_persisted_state_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = crate::test_support::manual_frames_app(&path).unwrap();
        let token = app.hello("desk", caps(), 1).unwrap();
        let before = serde_json::to_value(app.view()).unwrap();
        let bad = DeviceSettings {
            offline_seconds: 10,
            ..DeviceSettings::default()
        };
        assert!(matches!(
            app.set_device_settings("desk", token.incarnation, 1, bad),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            app.set_order(1, vec!["youtube".into()]),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            app.set_enabled("desk", token.incarnation, 9, "weather", false),
            Err(AppError::Conflict(_))
        ));
        drop(app);
        let reopened = crate::test_support::manual_frames_app(&path).unwrap();
        let mut after = serde_json::to_value(reopened.view()).unwrap();
        // A process restart intentionally clears the transient online bit.
        after["devices"][0]["online"] = before["devices"][0]["online"].clone();
        assert_eq!(before, after);
    }

    #[test]
    fn stale_session_heartbeat_does_not_confirm_new_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = crate::test_support::manual_frames_app(&path).unwrap();
        let old = app.hello("desk", caps(), 1).unwrap();
        let new = app.hello("desk", caps(), 2).unwrap();
        assert!(new.session_epoch > old.session_epoch);
        let desired = DeviceSettings {
            wifi_timeout_seconds: 25,
            ..DeviceSettings::default()
        };
        app.set_device_settings("desk", new.incarnation, 1, desired)
            .unwrap();
        assert!(matches!(app.heartbeat(&old, 2), Err(AppError::Conflict(_))));
        assert!(app.view().devices[0].pending);
        app.heartbeat(&new, 2).unwrap();
        assert!(!app.view().devices[0].pending);
        drop(app);
        assert!(
            !crate::test_support::manual_frames_app(&path)
                .unwrap()
                .view()
                .devices[0]
                .pending
        );
    }

    #[test]
    fn nvm_ack_send_completion_receipt_and_draw_are_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let mut app =
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", caps(), 1).unwrap();
        let desired = DeviceSettings {
            wifi_timeout_seconds: 25,
            ..DeviceSettings::default()
        };
        app.set_device_settings("desk", token.incarnation, 1, desired)
            .unwrap();
        app.settings_stored(&token, 2).unwrap();
        assert_eq!(app.view().devices[0].stored_revision, 2);
        assert!(app.view().devices[0].pending);
        app.heartbeat(&token, 2).unwrap();
        assert!(!app.view().devices[0].pending);

        let frame = app.request_frame(&token, "youtube", "asset-1").unwrap();
        let sent_id = app
            .pending_effects()
            .into_iter()
            .find_map(|(id, effect)| matches!(effect, Effect::SendFrame { .. }).then_some(id))
            .unwrap();
        assert!(app.complete_effect(sent_id).unwrap());
        assert_eq!(app.view().devices[0].last_received_frame, None);
        assert_eq!(app.view().devices[0].last_rendered_frame, None);
        assert!(app
            .terminal_event(Event::ReceiptAck {
                token: frame.clone()
            })
            .unwrap());
        assert_eq!(
            app.view().devices[0].last_received_frame,
            Some(frame.frame_id)
        );
        assert_eq!(app.view().devices[0].last_rendered_frame, None);
        assert!(app
            .terminal_event(Event::RenderedAck {
                token: frame.clone()
            })
            .unwrap());
        assert_eq!(
            app.view().devices[0].last_rendered_frame,
            Some(frame.frame_id)
        );
        assert!(!app
            .terminal_event(Event::RenderedAck { token: frame })
            .unwrap());
    }

    #[test]
    fn fetch_effect_and_result_survive_restart_and_old_result_is_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let effect_id;
        let incarnation;
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            incarnation = app.hello("desk", caps(), 1).unwrap().incarnation;
            effect_id = app
                .begin_fetch(
                    "desk",
                    incarnation,
                    FetchKey {
                        plugin: "weather".into(),
                        key: "tokyo-today".into(),
                    },
                )
                .unwrap();
            assert!(app
                .pending_effects()
                .iter()
                .any(|(id, effect)| *id == effect_id && matches!(effect, Effect::Fetch(_))));
        }
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            assert!(app
                .pending_effects()
                .iter()
                .any(|(id, effect)| *id == effect_id && matches!(effect, Effect::Fetch(_))));
            assert!(app
                .resolve_fetch(effect_id, Ok("snapshot-1".into()))
                .unwrap());
            assert!(!app.pending_effects().iter().any(|(id, _)| *id == effect_id));
            let key = FetchKey {
                plugin: "weather".into(),
                key: "tokyo-today".into(),
            };
            assert_eq!(
                app.runtime.world.devices["desk"].datasets[&key],
                Dataset::Ready {
                    snapshot_id: "snapshot-1".into()
                }
            );
            assert!(matches!(
                app.resolve_fetch(effect_id, Ok("late".into())),
                Err(AppError::NotFound)
            ));
            app.delete_device("desk", incarnation).unwrap();
            let fresh = app.hello("desk", caps(), 2).unwrap();
            assert!(fresh.incarnation > incarnation);
            assert!(!app.runtime.world.devices["desk"]
                .datasets
                .contains_key(&key));
        }
    }

    #[test]
    fn delete_effect_survives_restart_and_old_frame_acks_cannot_touch_new_device() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let old_frame;
        let old_incarnation;
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            let token = app.hello("desk", caps(), 1).unwrap();
            old_incarnation = token.incarnation;
            old_frame = app.request_frame(&token, "youtube", "asset-1").unwrap();
            app.delete_device("desk", token.incarnation).unwrap();
        }
        {
            let mut app = crate::test_support::manual_frames_app(&path).unwrap();
            assert!(app
                .pending_effects()
                .iter()
                .any(|(_, effect)| matches!(effect, Effect::CloseSession(_))));
            let fresh = app.hello("desk", caps(), 2).unwrap();
            assert!(fresh.incarnation > old_incarnation);
            assert!(!app
                .terminal_event(Event::ReceiptAck {
                    token: old_frame.clone()
                })
                .unwrap());
            assert!(!app
                .terminal_event(Event::RenderedAck { token: old_frame })
                .unwrap());
            assert_eq!(app.view().devices[0].last_received_frame, None);
            assert_eq!(app.view().devices[0].last_rendered_frame, None);
        }
    }

    #[test]
    fn weather_frame_is_discarded_on_display_change_and_reconnect() {
        let temp = tempfile::tempdir().unwrap();
        let mut app =
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", caps(), 1).unwrap();
        for _ in 0..2 {
            assert!(app
                .terminal_event(Event::Next {
                    token: token.clone()
                })
                .unwrap());
        }
        let set = crate::weather::ForecastSet::tokyo_from_cache(
            include_bytes!("../assets/jma-tokyo-20261012.json"),
            "cached",
            "2026-10-12",
        )
        .unwrap();
        let old = app
            .queue_lcd_frame(
                &token,
                "weather",
                crate::weather::render_lcd(&set, &set.days[0]).unwrap(),
            )
            .unwrap();
        let handle = app
            .pending_effects()
            .into_iter()
            .find_map(|(_, effect)| {
                if let Effect::SendFrame {
                    token,
                    asset_handle,
                } = effect
                {
                    (token == old).then_some(asset_handle)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(app.frame_asset(&handle).is_some());
        assert!(app
            .terminal_event(Event::Next {
                token: token.clone()
            })
            .unwrap());
        assert!(app.frame_asset(&handle).is_none());
        assert!(!app
            .pending_effects()
            .iter()
            .any(|(_, e)| matches!(e, Effect::SendFrame { token, .. } if *token == old)));
        assert!(!app
            .terminal_event(Event::RenderedAck { token: old })
            .unwrap());
        for _ in 0..DEFAULT_ORDER.len() - 1 {
            assert!(app
                .terminal_event(Event::Next {
                    token: token.clone()
                })
                .unwrap());
        }
        let queued = app
            .queue_lcd_frame(
                &token,
                "weather",
                crate::weather::render_lcd(&set, &set.days[0]).unwrap(),
            )
            .unwrap();
        let newer = app.hello("desk", caps(), 2).unwrap();
        assert!(newer.session_epoch > token.session_epoch);
        assert!(!app
            .pending_effects()
            .iter()
            .any(|(_, e)| matches!(e, Effect::SendFrame { token, .. } if *token == queued)));
        assert!(!app
            .terminal_event(Event::ReceiptAck { token: queued })
            .unwrap());
    }

    #[test]
    fn cached_weather_publish_rejects_old_incarnation_and_revision() {
        let temp = tempfile::tempdir().unwrap();
        let mut app =
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", caps(), 1).unwrap();
        assert!(app
            .publish_plugin_source(
                "weather",
                &SessionToken {
                    incarnation: token.incarnation + 1,
                    ..token.clone()
                },
                1,
                include_bytes!("../assets/jma-tokyo-20261012.json"),
                "cached",
                "2026-10-12"
            )
            .is_err());
        assert!(app
            .publish_plugin_source(
                "weather",
                &token,
                2,
                include_bytes!("../assets/jma-tokyo-20261012.json"),
                "cached",
                "2026-10-12"
            )
            .is_err());
        app.publish_plugin_source(
            "weather",
            &token,
            1,
            include_bytes!("../assets/jma-tokyo-20261012.json"),
            "cached",
            "2026-10-12",
        )
        .unwrap();
        app.delete_device("desk", token.incarnation).unwrap();
        let fresh = app.hello("desk", caps(), 2).unwrap();
        for _ in 0..2 {
            app.terminal_event(Event::Next {
                token: fresh.clone(),
            })
            .unwrap();
        }
        assert!(app.plugin_position(&fresh).is_none());
    }
}

#[path = "application_jobs.rs"]
mod managed;
pub use managed::{PlaybackOffer, PlaybackTarget, PluginWebView};

#[cfg(test)]
mod clock_tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    struct Clock(AtomicI64);
    impl crate::fetch::Clock for Clock {
        fn unix_seconds(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
        fn today_jst(&self) -> String {
            crate::clock::reading(self.unix_seconds()).unwrap().date[..10].replace('/', "-")
        }
    }
    fn delivery(app: &App, id: &str) -> (FrameToken, Arc<Vec<u8>>) {
        app.pending_effects()
            .into_iter()
            .find_map(|(_, effect)| match effect {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } if token.device_id == id && app.frame_is_current(&token) => {
                    Some((token, app.frame_asset(&asset_handle).unwrap()))
                }
                _ => None,
            })
            .expect("frame")
    }
    fn finish(app: &mut App, id: &str) {
        let (token, _) = delivery(app, id);
        let effect = app
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| matches!(e,Effect::SendFrame{token:t,..} if t==token).then_some(id))
            .unwrap();
        app.complete_effect(effect).unwrap();
        app.terminal_event(Event::ReceiptAck {
            token: token.clone(),
        })
        .unwrap();
        app.terminal_event(Event::RenderedAck { token }).unwrap();
    }
    fn select_clock(app: &mut App, token: &SessionToken) {
        while app.runtime.world.devices[&token.device_id]
            .selected
            .as_deref()
            != Some("clock")
        {
            app.button(token, ButtonAction::Next).unwrap();
        }
    }
    #[test]
    fn immediate_switch_minute_boundary_duplicates_late_wake_and_reconnect_native_frames() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let mut app = App::open_with_clock(&tmp.path().join("db"), now.clone()).unwrap();
        for (id, cap) in [("lcd", "lcd"), ("paper", "eink")] {
            let token = app.hello(id, [cap.into()].into(), 1).unwrap();
            select_clock(&mut app, &token);
            let expected = if cap == "lcd" {
                crate::clock::render_lcd(now.0.load(Ordering::SeqCst))
            } else {
                crate::clock::render_eink(now.0.load(Ordering::SeqCst))
            };
            assert_eq!(delivery(&app, id).1.as_ref(), &expected);
            finish(&mut app, id);
        }
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        now.0.fetch_add(1, Ordering::SeqCst);
        assert_eq!(app.refresh_timed_frames().unwrap(), 2);
        let first = delivery(&app, "paper").0;
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        now.0.fetch_add(180, Ordering::SeqCst);
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        finish(&mut app, "paper");
        assert_eq!(app.refresh_timed_frames().unwrap(), 1);
        assert_eq!(
            delivery(&app, "paper").1.as_ref(),
            &crate::clock::render_eink(now.0.load(Ordering::SeqCst))
        );
        assert!(delivery(&app, "paper").0.frame_id > first.frame_id);
        let old = crate::session_token("paper", &app.runtime.world.devices["paper"]);
        app.disconnect(old).unwrap();
        now.0.fetch_add(60, Ordering::SeqCst);
        let token = app.hello("paper", ["eink".into()].into(), 2).unwrap();
        assert!(token.session_epoch > first.session_epoch);
        assert_eq!(
            delivery(&app, "paper").1.as_ref(),
            &crate::clock::render_eink(now.0.load(Ordering::SeqCst))
        );
    }
    #[test]
    fn other_plugins_standby_menu_local_owner_and_offline_do_not_refresh_or_persist() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let mut app = App::open_with_clock(&tmp.path().join("db"), now.clone()).unwrap();
        for id in ["other", "sleep", "menu", "local", "offline"] {
            let token = app.hello(id, ["lcd".into()].into(), 1).unwrap();
            if id == "other" {
                continue;
            }
            select_clock(&mut app, &token);
            finish(&mut app, id);
            match id {
                "sleep" => {
                    app.terminal_event(Event::Power { token }).unwrap();
                }
                "menu" => {
                    app.runtime
                        .world
                        .devices
                        .get_mut(&token.device_id)
                        .unwrap()
                        .mode = Mode::Menu;
                }
                "local" => {
                    app.runtime
                        .world
                        .devices
                        .get_mut(&token.device_id)
                        .unwrap()
                        .owner = Owner::Local;
                }
                "offline" => {
                    app.disconnect(token).unwrap();
                }
                _ => unreachable!(),
            }
        }
        let rev = app.runtime.world.revision;
        let effects = app.pending_effects();
        let assets = app.frame_assets.len();
        now.0.fetch_add(300, Ordering::SeqCst);
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        assert_eq!(app.runtime.world.revision, rev);
        assert_eq!(app.pending_effects(), effects);
        assert_eq!(app.frame_assets.len(), assets);
    }
    #[test]
    fn legacy_order_migration_preserves_disabled_sets_and_selection_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let mut app = App::open(&path).unwrap();
        let t = app.hello("old", ["lcd".into()].into(), 1).unwrap();
        app.set_enabled("old", t.incarnation, 1, "clock", false)
            .unwrap();
        let selection = app.runtime.world.devices["old"].selected.clone();
        let enabled = app.runtime.world.devices["old"].enabled.clone();
        let settings_rev = app.runtime.world.devices["old"].settings_revision;
        app.runtime.world.plugin_order = vec![
            "rain-radar".into(),
            "weather".into(),
            "youtube".into(),
            "caricature".into(),
        ];
        App::persist(&mut app.connection.lock().unwrap(), &app.runtime).unwrap();
        drop(app);
        let mut app = App::open(&path).unwrap();
        assert_eq!(
            app.view().order,
            ["rain-radar", "weather", "youtube", "caricature", "clock"]
        );
        assert_eq!(app.runtime.world.devices["old"].selected, selection);
        assert_eq!(app.runtime.world.devices["old"].enabled, enabled);
        assert_eq!(
            app.runtime.world.devices["old"].settings_revision,
            settings_rev
        );
        let rev = app.view().order_revision;
        app.hello("old", ["lcd".into()].into(), 2).unwrap();
        drop(app);
        let app = App::open(&path).unwrap();
        assert_eq!(app.view().order_revision, rev);
        assert!(!app.runtime.world.devices["old"].enabled.contains("clock"));
    }
    #[test]
    fn current_eink_clock_latest_preserves_refresh_metadata_on_wire() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let mut app = App::open_with_clock(&tmp.path().join("db"), now).unwrap();
        let token = app.hello("paper", ["eink", crate::protocol::FRAME_BYTES_CAPABILITY,
            crate::protocol::EINK_CLOCK_CAPABILITY].into_iter().map(str::to_owned).collect(), 1).unwrap();
        select_clock(&mut app, &token);
        let rx = app.subscribe_lcd_video(&token).unwrap();
        let frame = rx.borrow().as_ref().unwrap().clone();
        assert_eq!(frame.delivery, crate::lcd_stream::DeliveryKind::Ordinary);
        assert_eq!(frame.pixels.len(), crate::eink::FRAME_BYTES);
        assert_eq!(frame.eink_refresh.as_ref().unwrap().full_refresh_minutes, 10);
        let mut transfer = crate::lcd_stream::VideoTransfer::new(frame);
        let start = transfer.next_packet().unwrap().unwrap();
        let decoded = crate::protocol::Decoder::default().push(&start).unwrap();
        assert!(matches!(&decoded[0], crate::protocol::WireMessage::FrameStart{
            width:250,height:122,format,eink_refresh:Some(_),stream:None,..} if format == "mono1"));
    }
    #[test]
    fn shared_eink_clock_latest_publishes_one_frame_per_minute_even_after_ack() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let shared = crate::shared::Application::new(
            App::open_with_clock(&tmp.path().join("db"), now.clone()).unwrap(),
        );
        let token = {
            let mut app = shared.lock().unwrap();
            let token = app.hello("paper", ["eink", crate::protocol::FRAME_BYTES_CAPABILITY,
                crate::protocol::EINK_CLOCK_CAPABILITY].into_iter().map(str::to_owned).collect(), 1).unwrap();
            select_clock(&mut app, &token);
            token
        };
        let first = {
            let app = shared.read().unwrap();
            let rx = app.published_video_receiver(&token).unwrap();
            let frame = rx.borrow().as_ref().unwrap().clone();
            frame
        };
        assert_eq!(shared.refresh_timed_frames().unwrap(), 0);
        assert!(shared.read().unwrap().start_lcd_latest_send(&first));
        let ack = crate::protocol::WireMessage::RenderedAck { token: first.token.clone() };
        assert!(shared.input_lock(&token, &ack).unwrap().lcd_latest_ack(&first.token, true));
        assert_eq!(shared.refresh_timed_frames().unwrap(), 0);
        now.0.fetch_add(1, Ordering::SeqCst);
        assert_eq!(shared.refresh_timed_frames().unwrap(), 1);
        let second = {
            let app = shared.read().unwrap();
            let rx = app.published_video_receiver(&token).unwrap();
            let frame = rx.borrow().as_ref().unwrap().clone();
            frame
        };
        assert!(second.token.frame_id > first.token.frame_id);
        assert_eq!(shared.refresh_timed_frames().unwrap(), 0);
    }
    #[test]
    fn concurrent_durable_publication_keeps_eink_clock_minute_key() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let shared = crate::shared::Application::new(
            App::open_with_clock(&tmp.path().join("db"), now.clone()).unwrap(),
        );
        let token = {
            let mut app = shared.lock().unwrap();
            let token = app.hello("paper", ["eink", crate::protocol::FRAME_BYTES_CAPABILITY,
                crate::protocol::EINK_CLOCK_CAPABILITY].into_iter().map(str::to_owned).collect(), 1).unwrap();
            select_clock(&mut app, &token);
            token
        };
        let mut durable = shared.lock().unwrap();
        now.0.fetch_add(1, Ordering::SeqCst);
        assert_eq!(shared.refresh_timed_frames().unwrap(), 1);
        let next = {
            let app = shared.read().unwrap();
            let rx = app.published_video_receiver(&token).unwrap();
            let frame = rx.borrow().as_ref().unwrap().clone();
            frame
        };
        // Force a successful durable publication of an otherwise unchanged
        // Device while the transient minute update is already visible.
        durable.heartbeat(&token, 1).unwrap();
        drop(durable);
        assert_eq!(shared.refresh_timed_frames().unwrap(), 0);
        assert!(shared.read().unwrap().start_lcd_latest_send(&next));
        let ack = crate::protocol::WireMessage::ReceiptAck { token: next.token.clone() };
        assert!(shared.input_lock(&token, &ack).unwrap().lcd_latest_ack(&next.token, false));
        assert_eq!(shared.refresh_timed_frames().unwrap(), 0);
    }
    #[test]
    fn clock_refresh_metadata_is_negotiated_and_isolated_from_lcd_and_other_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Clock(AtomicI64::new(1_792_336_799)));
        let mut app = App::open_with_clock(&tmp.path().join("db"), now).unwrap();
        for (id, caps, supported) in [
            ("old", vec!["eink"], false),
            ("new", vec!["eink", "eink-clock-v1"], true),
            ("lcd", vec!["lcd", "eink-clock-v1"], false),
        ] {
            let t = app
                .hello(id, caps.into_iter().map(str::to_owned).collect(), 1)
                .unwrap();
            assert!(app.frame_eink_refresh(&delivery(&app, id).0).is_none());
            select_clock(&mut app, &t);
            let frame = delivery(&app, id).0;
            let policy = app.frame_eink_refresh(&frame);
            assert_eq!(policy.is_some(), supported);
            if let Some(policy) = policy {
                assert_eq!(policy.full_refresh_minutes, 10);
                assert_eq!(policy.minute, 1_792_336_799 / 60);
            }
            if supported {
                finish(&mut app, id);
                // Non-selected plugin settings do not duplicate a minute's frame.
                let rev = app.runtime.world.devices[id].settings_revision;
                app.set_enabled(id, t.incarnation, rev, "weather", false)
                    .unwrap();
                assert_eq!(app.refresh_timed_frames().unwrap(), 0);
                app.set_plugin_device_config(
                    id,
                    t.incarnation,
                    "clock",
                    1,
                    serde_json::json!({"full_refresh_minutes":20}),
                )
                .unwrap();
                assert_eq!(app.refresh_timed_frames().unwrap(), 1);
                assert_eq!(
                    app.frame_eink_refresh(&delivery(&app, id).0)
                        .unwrap()
                        .full_refresh_minutes,
                    20
                );
            }
            app.button(&t, ButtonAction::Next).unwrap();
            assert!(app.frame_eink_refresh(&delivery(&app, id).0).is_none());
        }
    }

    #[test]
    fn refresh_config_validation_revision_persistence_and_device_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db");
        let mut app = App::open(&path).unwrap();
        let token = app.hello("paper", ["eink".into()].into(), 1).unwrap();
        assert_eq!(
            app.plugin_device_config("paper", "clock").unwrap()["full_refresh_minutes"],
            10
        );
        for value in [0, 61, -1] {
            assert!(app
                .set_plugin_device_config(
                    "paper",
                    token.incarnation,
                    "clock",
                    1,
                    serde_json::json!({"full_refresh_minutes":value})
                )
                .is_err());
        }
        app.set_plugin_device_config(
            "paper",
            token.incarnation,
            "clock",
            1,
            serde_json::json!({"full_refresh_minutes":15}),
        )
        .unwrap();
        assert!(app
            .set_plugin_device_config(
                "paper",
                token.incarnation,
                "clock",
                1,
                serde_json::json!({"full_refresh_minutes":20})
            )
            .is_err());
        drop(app);
        let mut app = App::open(&path).unwrap();
        assert_eq!(
            app.plugin_device_config("paper", "clock").unwrap()["full_refresh_minutes"],
            15
        );
        app.delete_device("paper", token.incarnation).unwrap();
        let t = app.hello("paper", ["eink".into()].into(), 1).unwrap();
        assert!(t.incarnation > token.incarnation);
        assert_eq!(
            app.plugin_device_config("paper", "clock").unwrap()["full_refresh_minutes"],
            10
        );
    }
}

#[cfg(test)]
#[path="application_contract_tests.rs"]
mod contract_tests;

include!("application_live.rs");
include!("application_maintenance.rs");
