//! SQLite-backed application boundary around the pure transition engine.
//! This module does not open a listener or execute effects.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
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
    plugin_states: BTreeMap<String, serde_json::Value>,
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
            plugin_states: state.plugin_states.clone(),
        }
    }
}

impl TryFrom<StoredState> for Runtime {
    type Error = AppError;

    fn try_from(stored: StoredState) -> Result<Self, Self::Error> {
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
            plugin_states: stored.plugin_states,
        };
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
            || !valid_order(&state.world.plugin_order)
        {
            return Err(AppError::Conflict("inconsistent stored state"));
        }
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

fn valid_order(order: &[String]) -> bool {
    order.len() == DEFAULT_ORDER.len()
        && order.iter().collect::<BTreeSet<_>>().len() == DEFAULT_ORDER.len()
        && order.iter().all(|id| DEFAULT_ORDER.contains(&id.as_str()))
}

pub struct App {
    connection: Connection,
    engine: Engine,
    runtime: Runtime,
    clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
    /// Completed LCD pixels are transient; only validated handles enter the sender.
    frame_assets: BTreeMap<String, Arc<Vec<u8>>>,
    contents: BTreeMap<String, Box<dyn ContentPlugin>>,
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
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS app_state (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               schema_version INTEGER NOT NULL,
               body TEXT NOT NULL
             );",
        )?;
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
            Runtime::try_from(serde_json::from_str::<StoredState>(&body)?)?
        } else {
            let runtime = Runtime {
                world: World::new(DEFAULT_ORDER.iter().map(|id| (*id).to_owned()).collect()),
                settings: SettingsStore::new(),
                order_revision: 1,
                outbox: BTreeMap::new(),
                next_effect_id: 1,
                plugin_states: BTreeMap::new(),
            };
            Self::persist(&mut connection, &runtime)?;
            runtime
        };
        let mut contents = crate::plugins::contents();
        for (id, value) in &runtime.plugin_states {
            if let Some(plugin) = contents.get_mut(id) {
                plugin
                    .load_state(value.clone())
                    .map_err(|_| AppError::Conflict("invalid plugin state"))?;
            }
        }
        Ok(Self {
            connection,
            engine: crate::plugins::engine(),
            runtime,
            clock,
            frame_assets: BTreeMap::new(),
            contents,
        })
    }

    fn persist(connection: &mut Connection, runtime: &Runtime) -> Result<(), AppError> {
        let body = serde_json::to_string(&StoredState::from(runtime))?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO app_state (id, schema_version, body) VALUES (1, 2, ?1)
             ON CONFLICT(id) DO UPDATE SET body = excluded.body",
            params![body],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn change<T>(
        &mut self,
        action: impl FnOnce(&Engine, &mut Runtime) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut next = self.runtime.clone();
        let result = action(&self.engine, &mut next)?;
        next.prune_stale_effects();
        Self::persist(&mut self.connection, &next)?;
        self.runtime = next;
        self.frame_assets.retain(|handle, _| {
            self.runtime.outbox.values().any(|effect|
            matches!(effect, Effect::SendFrame { asset_handle, .. } if asset_handle == handle))
        });
        Ok(result)
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
        let completed = self.change(|_, state| Ok(state.outbox.remove(&id).is_some()))?;
        if let Some(handle) = handle {
            self.frame_assets.remove(&handle);
        }
        Ok(completed)
    }

    /// Queue one complete RGB565 big-endian LCD image. The pixel buffer is
    /// immutable and never written into SQLite or claimed to be drawn here.
    pub fn queue_lcd_frame(
        &mut self,
        token: &SessionToken,
        plugin: &str,
        bytes: Vec<u8>,
    ) -> Result<FrameToken, AppError> {
        if bytes.len() != crate::protocol::LCD_FRAME_BYTES {
            return Err(AppError::Invalid("invalid LCD frame length"));
        }
        if self.frame_assets.len() >= 128 {
            return Err(AppError::Conflict("transient frame asset limit"));
        }
        let handle = format!(
            "lcd:{}:{}",
            token.device_id, self.runtime.world.next_frame_id
        );
        let frame = self.request_frame(token, plugin, &handle)?;
        self.frame_assets.insert(handle, Arc::new(bytes));
        Ok(frame)
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
            state.plugin_states.insert(plugin.into(), snapshot);
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
            state.plugin_states.insert(plugin.into(), snapshot);
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
                state.plugin_states.extend(states);
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
            state.plugin_states.extend(states);
            Ok(())
        })?;
        self.contents = contents;
        Ok(())
    }

    pub fn complete_plugin_fetch(
        &mut self,
        job: &FetchJob,
        now_unix: i64,
        result: Result<Vec<u8>, String>,
    ) -> Result<bool, AppError> {
        let Some(device) = self.runtime.world.devices.get(&job.device_id) else {
            return Ok(false);
        };
        if device.incarnation != job.incarnation
            || device.settings_revision != job.settings_revision
        {
            return Ok(false);
        }
        let mut content = self
            .contents
            .get(&job.plugin)
            .ok_or(AppError::NotFound)?
            .clone();
        let pixels = match content.complete_fetch(job, now_unix, result) {
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
            state.plugin_states.insert(job.plugin.clone(), snapshot);
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
        let Some(device) = self.runtime.world.devices.get(&token.device_id) else {
            return Ok(());
        };
        // A date observation may already have queued the current display's
        // image. RequestFrame permits only one pending frame per display.
        if device.mode != Mode::Normal || device.pending_frame_id.is_some() {
            return Ok(());
        }
        let Some(id) = device.selected.as_ref() else {
            return Ok(());
        };
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: device.settings_revision,
            mode: device.mode.clone(),
        };
        if let Some(pixels) = self.contents.get(id).and_then(|p| p.current_frame(&ctx)) {
            let id = id.clone();
            self.queue_lcd_frame(token, &id, pixels)?;
        }
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
        let plan = match plugin.plan(&ctx, action) {
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
        if let Some(event) = plan.event {
            if !self.terminal_event(event)? {
                let (selected, display_epoch) =
                    self.selection_for(token).ok_or(AppError::NotFound)?;
                return Ok(InputResult::Selection {
                    selected,
                    display_epoch,
                });
            }
            self.contents.insert(id.clone(), plugin);
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
                state.plugin_states.extend(states);
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

    pub fn frame_is_current(&self, token: &FrameToken) -> bool {
        self.runtime
            .world
            .devices
            .get(&token.device_id)
            .is_some_and(|device| {
                frame_current(device, token) && device.pending_frame_id == Some(token.frame_id)
            })
    }

    pub fn hello(
        &mut self,
        id: &str,
        capabilities: BTreeSet<String>,
        screen_owner_epoch: u64,
    ) -> Result<SessionToken, AppError> {
        if !valid_name(id, 64)
            || capabilities.is_empty()
            || capabilities.len() > 32
            || capabilities.iter().any(|cap| !valid_name(cap, 40))
            || (!capabilities.contains("lcd") && !capabilities.contains("eink"))
        {
            return Err(AppError::Invalid("invalid device hello"));
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
            state.plugin_states.extend(snapshots);
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
        self.change(|engine, state| {
            let device = state.world.devices.get(id).ok_or(AppError::NotFound)?;
            if device.incarnation != incarnation {
                return Err(AppError::Conflict("stale device incarnation"));
            }
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
            Ok(())
        })
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
        let changed = self.change(|engine, state| {
            let decision = engine
                .handle(&state.world, &event)
                .map_err(|_| AppError::Conflict("terminal event rejected"))?;
            state.accept(decision)
        })?;
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
        self.change(|engine, state| {
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
        })
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
        if !valid_order(&order) {
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
        let mut app = App::open_with_clock(&temp.path().join("state.db"), clock.clone()).unwrap();
        app.set_order(
            1,
            ["weather", "rain-radar", "youtube", "caricature"]
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
                    let mut app = App::open(&temp.path().join("state.db")).unwrap();
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
            let mut app = App::open(&path).unwrap();
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
                ["weather", "youtube", "caricature", "rain-radar"]
                    .map(str::to_owned)
                    .to_vec(),
            )
            .unwrap();
        }
        {
            let mut app = App::open(&path).unwrap();
            let view = app.view();
            assert!(!view.devices[0].online);
            assert_eq!(view.devices[0].desired.wifi_timeout_seconds, 25);
            assert!(view.devices[0].pending);
            assert_eq!(view.order[0], "weather");
            app.delete_device("desk", old_incarnation).unwrap();
        }
        {
            let mut app = App::open(&path).unwrap();
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
        let mut app = App::open(&path).unwrap();
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
        let reopened = App::open(&path).unwrap();
        let mut after = serde_json::to_value(reopened.view()).unwrap();
        // A process restart intentionally clears the transient online bit.
        after["devices"][0]["online"] = before["devices"][0]["online"].clone();
        assert_eq!(before, after);
    }

    #[test]
    fn stale_session_heartbeat_does_not_confirm_new_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
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
        assert!(!App::open(&path).unwrap().view().devices[0].pending);
    }

    #[test]
    fn nvm_ack_send_completion_receipt_and_draw_are_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
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
            let mut app = App::open(&path).unwrap();
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
            let mut app = App::open(&path).unwrap();
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
            let mut app = App::open(&path).unwrap();
            let token = app.hello("desk", caps(), 1).unwrap();
            old_incarnation = token.incarnation;
            old_frame = app.request_frame(&token, "youtube", "asset-1").unwrap();
            app.delete_device("desk", token.incarnation).unwrap();
        }
        {
            let mut app = App::open(&path).unwrap();
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
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
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
        for _ in 0..3 {
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
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
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
