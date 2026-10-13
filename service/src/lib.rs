//! Pure PicoMado kernel contract. No IO is performed here.

pub mod application;
pub mod content;
pub mod fetch;
pub mod host;
mod lcd_text;
pub mod plugins;
pub mod protocol;
pub mod queue;
pub mod radar;
pub mod runner;
pub mod settings;
pub mod transport;
pub mod weather;
pub mod web;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type DeviceId = String;
pub type PluginId = String;
pub type Capability = String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Sub,
    Menu,
    Standby,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Remote,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FetchKey {
    pub plugin: PluginId,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dataset {
    Loading,
    Ready { snapshot_id: String },
    Failed { error_kind: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub incarnation: u64,
    pub capabilities: BTreeSet<Capability>,
    pub enabled: BTreeSet<PluginId>,
    pub selected: Option<PluginId>,
    pub mode: Mode,
    pub owner: Owner,
    pub screen_owner_epoch: u64,
    pub online: bool,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub settings_revision: u64,
    pub requests: BTreeMap<FetchKey, u64>,
    pub datasets: BTreeMap<FetchKey, Dataset>,
    pub pending_frame_id: Option<u64>,
    pub last_received_frame: Option<u64>,
    pub last_rendered_frame: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    pub revision: u64,
    /// This counter is persisted by the future repository and never reset by deletion.
    pub next_incarnation: u64,
    pub next_frame_id: u64,
    pub plugin_order: Vec<PluginId>,
    pub devices: BTreeMap<DeviceId, Device>,
}

impl World {
    pub fn new(plugin_order: Vec<PluginId>) -> Self {
        Self {
            revision: 0,
            next_incarnation: 1,
            next_frame_id: 1,
            plugin_order,
            devices: BTreeMap::new(),
        }
    }
}

pub trait WorldQuery {
    fn revision(&self) -> u64;
    fn device(&self, id: &str) -> Option<&Device>;
    fn plugin_order(&self) -> &[PluginId];
}

pub struct View<'a>(pub &'a World);

impl WorldQuery for View<'_> {
    fn revision(&self) -> u64 {
        self.0.revision
    }
    fn device(&self, id: &str) -> Option<&Device> {
        self.0.devices.get(id)
    }
    fn plugin_order(&self) -> &[PluginId] {
        &self.0.plugin_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionToken {
    pub device_id: DeviceId,
    pub incarnation: u64,
    pub session_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceToken {
    pub device_id: DeviceId,
    pub incarnation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayToken {
    pub device_id: DeviceId,
    pub incarnation: u64,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub screen_owner_epoch: u64,
    pub settings_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchToken {
    pub device_id: DeviceId,
    pub incarnation: u64,
    pub settings_revision: u64,
    pub request_id: u64,
    pub key: FetchKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameToken {
    pub device_id: DeviceId,
    pub incarnation: u64,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub screen_owner_epoch: u64,
    pub frame_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Hello {
        device_id: DeviceId,
        capabilities: BTreeSet<Capability>,
        screen_owner_epoch: u64,
    },
    Disconnect {
        token: SessionToken,
    },
    Delete {
        device_id: DeviceId,
    },
    SetEnabled {
        device_id: DeviceId,
        plugin: PluginId,
        enabled: bool,
        expected_settings_revision: u64,
    },
    PluginConfigChanged {
        device_id: DeviceId,
        incarnation: u64,
        plugin: PluginId,
        expected_settings_revision: u64,
    },
    PluginDataChanged {
        device_id: DeviceId,
        incarnation: u64,
        plugin: PluginId,
    },
    Next {
        token: SessionToken,
    },
    Previous {
        token: SessionToken,
    },
    Power {
        token: SessionToken,
    },
    EnterSub {
        token: SessionToken,
    },
    EnterMenu {
        token: SessionToken,
    },
    LocalOwner {
        token: SessionToken,
        epoch: u64,
        owner: Owner,
    },
    BeginFetch {
        token: DeviceToken,
        key: FetchKey,
    },
    FetchCompleted {
        token: FetchToken,
        snapshot_id: String,
    },
    FetchFailed {
        token: FetchToken,
        error_kind: String,
    },
    RequestFrame {
        token: DisplayToken,
        plugin: PluginId,
        asset_handle: String,
    },
    RenderedAck {
        token: FrameToken,
    },
    ReceiptAck {
        token: FrameToken,
    },
    PluginInput {
        token: SessionToken,
        name: String,
    },
    PluginTick {
        token: DeviceToken,
        plugin: PluginId,
    },
}

impl Event {
    pub fn device_id(&self) -> &str {
        match self {
            Self::Hello { device_id, .. }
            | Self::Delete { device_id }
            | Self::SetEnabled { device_id, .. }
            | Self::PluginConfigChanged { device_id, .. }
            | Self::PluginDataChanged { device_id, .. } => device_id,
            Self::Disconnect { token }
            | Self::Next { token }
            | Self::Previous { token }
            | Self::Power { token }
            | Self::EnterSub { token }
            | Self::EnterMenu { token }
            | Self::LocalOwner { token, .. }
            | Self::PluginInput { token, .. } => &token.device_id,
            Self::BeginFetch { token, .. } | Self::PluginTick { token, .. } => &token.device_id,
            Self::RequestFrame { token, .. } => &token.device_id,
            Self::FetchCompleted { token, .. } | Self::FetchFailed { token, .. } => {
                &token.device_id
            }
            Self::RenderedAck { token } | Self::ReceiptAck { token } => &token.device_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    CloseSession(SessionToken),
    DropQueuedFrames {
        device_id: DeviceId,
        incarnation: u64,
        through_display_epoch: u64,
    },
    ShowSelected {
        token: DisplayToken,
        plugin: PluginId,
    },
    ShowLogo {
        token: DisplayToken,
    },
    EnterStandby {
        token: SessionToken,
    },
    SyncSettings {
        token: SessionToken,
        snapshot: settings::DeviceSnapshot,
    },
    /// The selected plugin discards a pinned browsing snapshot and renders its
    /// newest default view. It reports any later render via RequestFrame.
    ResetPluginView {
        token: DisplayToken,
        plugin: PluginId,
    },
    Fetch(FetchToken),
    SendFrame {
        token: FrameToken,
        asset_handle: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Periodic,
    User,
    Protect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transition {
    Hello {
        device_id: DeviceId,
        capabilities: BTreeSet<Capability>,
        screen_owner_epoch: u64,
    },
    Disconnect(SessionToken),
    Delete(DeviceId),
    SetEnabled {
        device_id: DeviceId,
        plugin: PluginId,
        enabled: bool,
        expected_settings_revision: u64,
    },
    PluginConfigChanged {
        device_id: DeviceId,
        incarnation: u64,
        plugin: PluginId,
        expected_settings_revision: u64,
    },
    PluginDataChanged {
        device_id: DeviceId,
        incarnation: u64,
        plugin: PluginId,
    },
    Next(SessionToken),
    Previous(SessionToken),
    Power(SessionToken),
    EnterMode {
        token: SessionToken,
        mode: Mode,
    },
    RefreshView {
        token: SessionToken,
        plugin: PluginId,
    },
    LocalOwner {
        token: SessionToken,
        epoch: u64,
        owner: Owner,
    },
    BeginFetch {
        token: DeviceToken,
        key: FetchKey,
    },
    FetchCompleted {
        token: FetchToken,
        result: Result<String, String>,
    },
    RequestFrame {
        token: DisplayToken,
        plugin: PluginId,
        asset_handle: String,
    },
    RenderedAck(FrameToken),
    ReceiptAck(FrameToken),
}

impl Transition {
    pub fn device_id(&self) -> &str {
        match self {
            Self::Hello { device_id, .. }
            | Self::SetEnabled { device_id, .. }
            | Self::PluginConfigChanged { device_id, .. }
            | Self::PluginDataChanged { device_id, .. } => device_id,
            Self::Delete(id) => id,
            Self::Disconnect(token)
            | Self::Next(token)
            | Self::Previous(token)
            | Self::Power(token)
            | Self::EnterMode { token, .. }
            | Self::RefreshView { token, .. }
            | Self::LocalOwner { token, .. } => &token.device_id,
            Self::BeginFetch { token, .. } => &token.device_id,
            Self::RequestFrame { token, .. } => &token.device_id,
            Self::FetchCompleted { token, .. } => &token.device_id,
            Self::RenderedAck(token) | Self::ReceiptAck(token) => &token.device_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub scope: DeviceId,
    pub expected_revision: u64,
    pub priority: Priority,
    pub reason: &'static str,
    pub transition: Transition,
}

impl Candidate {
    pub fn new(
        query: &dyn WorldQuery,
        device_id: &str,
        priority: Priority,
        reason: &'static str,
        transition: Transition,
    ) -> Self {
        Self {
            scope: device_id.to_owned(),
            expected_revision: query.revision(),
            priority,
            reason,
            transition,
        }
    }
}

/// A plugin owns its own input and timer rules. The kernel only routes by ID.
pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn supports(&self, capabilities: &BTreeSet<Capability>) -> bool;
    fn candidates<'a>(
        &'a self,
        query: &'a dyn WorldQuery,
        event: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a>;
}

#[derive(Default)]
pub struct Registry {
    plugins: BTreeMap<PluginId, Box<dyn Plugin>>,
}

impl Registry {
    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<(), Error> {
        let id = plugin.id().to_owned();
        if self.plugins.contains_key(&id) {
            return Err(Error::DuplicatePlugin(id));
        }
        self.plugins.insert(id, plugin);
        Ok(())
    }
    pub fn contains(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }
    pub fn supports(&self, id: &str, caps: &BTreeSet<Capability>) -> bool {
        self.plugins.get(id).is_some_and(|p| p.supports(caps))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    DuplicatePlugin(PluginId),
    Ambiguous {
        scope: DeviceId,
        reasons: Vec<&'static str>,
    },
    InvalidScope {
        event_device: DeviceId,
        candidate_scope: DeviceId,
        transition_device: DeviceId,
    },
    StaleCandidate,
    UnsupportedPlugin(PluginId),
    UnknownPlugin(PluginId),
    StaleSettingsRevision {
        expected: u64,
        actual: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub state: World,
    pub effects: Vec<Effect>,
    pub applied: Option<&'static str>,
}

pub struct Engine {
    pub registry: Registry,
}

impl Engine {
    pub fn new(registry: Registry) -> Self {
        Self { registry }
    }

    pub fn handle(&self, state: &World, event: &Event) -> Result<Decision, Error> {
        let view = View(state);
        // Terminal input and timer callbacks are rejected before any plugin rule
        // can turn them into a transition for a newer session or registration.
        let current_source = match event {
            Event::Next { token }
            | Event::Previous { token }
            | Event::Power { token }
            | Event::EnterSub { token }
            | Event::EnterMenu { token }
            | Event::LocalOwner { token, .. }
            | Event::PluginInput { token, .. } => view
                .device(&token.device_id)
                .is_some_and(|d| session_current(d, token)),
            Event::PluginTick { token, .. } | Event::BeginFetch { token, .. } => view
                .device(&token.device_id)
                .is_some_and(|d| d.incarnation == token.incarnation),
            _ => true,
        };
        if !current_source {
            return Ok(Decision {
                state: state.clone(),
                effects: vec![],
                applied: None,
            });
        }
        // Rules produce candidates only when consumed. A plugin is consulted only
        // for its selected device or its own timer event.
        let mut candidates: Vec<_> = core_candidates(&view, event).collect();
        let routed_plugin = match event {
            Event::PluginInput { token, .. }
            | Event::EnterSub { token }
            | Event::EnterMenu { token } => view
                .device(&token.device_id)
                .and_then(|d| d.selected.as_deref()),
            Event::PluginTick { plugin, .. } => Some(plugin.as_str()),
            _ => None,
        };
        if let Some(id) = routed_plugin {
            if let Some(plugin) = self.registry.plugins.get(id) {
                candidates.extend(plugin.candidates(&view, event));
            }
        }
        if candidates.is_empty() {
            return Ok(Decision {
                state: state.clone(),
                effects: vec![],
                applied: None,
            });
        }
        if let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.scope != event.device_id()
                || candidate.transition.device_id() != event.device_id()
        }) {
            return Err(Error::InvalidScope {
                event_device: event.device_id().to_owned(),
                candidate_scope: candidate.scope.clone(),
                transition_device: candidate.transition.device_id().to_owned(),
            });
        }
        let scope = candidates[0].scope.clone();
        // One event operates on one aggregate. Cross-aggregate work is another event.
        if candidates.iter().any(|c| c.scope != scope) {
            return Err(Error::Ambiguous {
                scope,
                reasons: candidates.iter().map(|c| c.reason).collect(),
            });
        }
        candidates.retain(|c| c.expected_revision == state.revision);
        let Some(priority) = candidates.iter().map(|c| c.priority).max() else {
            return Err(Error::StaleCandidate);
        };
        let winners: Vec<_> = candidates
            .into_iter()
            .filter(|c| c.priority == priority)
            .collect();
        if winners.len() != 1 {
            return Err(Error::Ambiguous {
                scope,
                reasons: winners.iter().map(|c| c.reason).collect(),
            });
        }
        let winner = winners.into_iter().next().expect("one winner");
        let mut next = state.clone();
        let mut effects = Vec::new();
        if apply(&mut next, &self.registry, winner.transition, &mut effects)? {
            next.revision = next.revision.checked_add(1).expect("revision exhausted");
            Ok(Decision {
                state: next,
                effects,
                applied: Some(winner.reason),
            })
        } else {
            Ok(Decision {
                state: state.clone(),
                effects: vec![],
                applied: None,
            })
        }
    }
}

fn core_candidates<'a>(
    q: &'a dyn WorldQuery,
    event: &'a Event,
) -> Box<dyn Iterator<Item = Candidate> + 'a> {
    let (priority, reason, transition) = match event {
        Event::Hello {
            device_id,
            capabilities,
            screen_owner_epoch,
        } => (
            Priority::Protect,
            "hello",
            Transition::Hello {
                device_id: device_id.clone(),
                capabilities: capabilities.clone(),
                screen_owner_epoch: *screen_owner_epoch,
            },
        ),
        Event::Disconnect { token } => (
            Priority::Protect,
            "disconnect",
            Transition::Disconnect(token.clone()),
        ),
        Event::Delete { device_id } => (
            Priority::Protect,
            "delete",
            Transition::Delete(device_id.clone()),
        ),
        Event::SetEnabled {
            device_id,
            plugin,
            enabled,
            expected_settings_revision,
        } => (
            Priority::User,
            "set-enabled",
            Transition::SetEnabled {
                device_id: device_id.clone(),
                plugin: plugin.clone(),
                enabled: *enabled,
                expected_settings_revision: *expected_settings_revision,
            },
        ),
        Event::PluginDataChanged {
            device_id,
            incarnation,
            plugin,
        } => (
            Priority::Periodic,
            "plugin-data-changed",
            Transition::PluginDataChanged {
                device_id: device_id.clone(),
                incarnation: *incarnation,
                plugin: plugin.clone(),
            },
        ),
        Event::PluginConfigChanged {
            device_id,
            incarnation,
            plugin,
            expected_settings_revision,
        } => (
            Priority::User,
            "plugin-config-changed",
            Transition::PluginConfigChanged {
                device_id: device_id.clone(),
                incarnation: *incarnation,
                plugin: plugin.clone(),
                expected_settings_revision: *expected_settings_revision,
            },
        ),
        Event::Next { token } => (Priority::User, "next", Transition::Next(token.clone())),
        Event::Previous { token } => (
            Priority::User,
            "previous",
            Transition::Previous(token.clone()),
        ),
        Event::Power { token } => (Priority::Protect, "power", Transition::Power(token.clone())),
        Event::LocalOwner {
            token,
            epoch,
            owner,
        } => (
            Priority::Protect,
            "local-owner",
            Transition::LocalOwner {
                token: token.clone(),
                epoch: *epoch,
                owner: owner.clone(),
            },
        ),
        Event::BeginFetch { token, key } => (
            Priority::Periodic,
            "begin-fetch",
            Transition::BeginFetch {
                token: token.clone(),
                key: key.clone(),
            },
        ),
        Event::FetchCompleted { token, snapshot_id } => (
            Priority::Periodic,
            "fetch-completed",
            Transition::FetchCompleted {
                token: token.clone(),
                result: Ok(snapshot_id.clone()),
            },
        ),
        Event::FetchFailed { token, error_kind } => (
            Priority::Periodic,
            "fetch-failed",
            Transition::FetchCompleted {
                token: token.clone(),
                result: Err(error_kind.clone()),
            },
        ),
        Event::RequestFrame {
            token,
            plugin,
            asset_handle,
        } => (
            Priority::Periodic,
            "frame-request",
            Transition::RequestFrame {
                token: token.clone(),
                plugin: plugin.clone(),
                asset_handle: asset_handle.clone(),
            },
        ),
        Event::RenderedAck { token } => (
            Priority::Periodic,
            "rendered-ack",
            Transition::RenderedAck(token.clone()),
        ),
        Event::ReceiptAck { token } => (
            Priority::Periodic,
            "receipt-ack",
            Transition::ReceiptAck(token.clone()),
        ),
        Event::PluginInput { .. }
        | Event::PluginTick { .. }
        | Event::EnterSub { .. }
        | Event::EnterMenu { .. } => return Box::new(std::iter::empty()),
    };
    Box::new(std::iter::once(Candidate::new(
        q,
        event.device_id(),
        priority,
        reason,
        transition,
    )))
}

fn first_enabled(order: &[PluginId], device: &Device) -> Option<PluginId> {
    order
        .iter()
        .find(|id| device.enabled.contains(*id))
        .cloned()
}

fn show_selected(id: &str, device: &Device, out: &mut Vec<Effect>) {
    if !device.online || device.owner != Owner::Remote || device.mode != Mode::Normal {
        return;
    }
    let token = display_token(id, device);
    match &device.selected {
        Some(plugin) => out.push(Effect::ShowSelected {
            token,
            plugin: plugin.clone(),
        }),
        None => out.push(Effect::ShowLogo { token }),
    }
}

pub fn session_token(id: &str, device: &Device) -> SessionToken {
    SessionToken {
        device_id: id.to_owned(),
        incarnation: device.incarnation,
        session_epoch: device.session_epoch,
    }
}

pub fn device_token(id: &str, device: &Device) -> DeviceToken {
    DeviceToken {
        device_id: id.to_owned(),
        incarnation: device.incarnation,
    }
}

pub fn display_token(id: &str, device: &Device) -> DisplayToken {
    DisplayToken {
        device_id: id.to_owned(),
        incarnation: device.incarnation,
        session_epoch: device.session_epoch,
        display_epoch: device.display_epoch,
        screen_owner_epoch: device.screen_owner_epoch,
        settings_revision: device.settings_revision,
    }
}

pub fn session_current(device: &Device, token: &SessionToken) -> bool {
    device.online
        && device.incarnation == token.incarnation
        && device.session_epoch == token.session_epoch
}

pub fn display_current(device: &Device, token: &DisplayToken) -> bool {
    device.mode == Mode::Normal && frame_display_current(device, token)
}

fn frame_display_current(device: &Device, token: &DisplayToken) -> bool {
    device.online
        && device.owner == Owner::Remote
        && matches!(device.mode, Mode::Normal | Mode::Sub)
        && device.incarnation == token.incarnation
        && device.session_epoch == token.session_epoch
        && device.display_epoch == token.display_epoch
        && device.screen_owner_epoch == token.screen_owner_epoch
        && device.settings_revision == token.settings_revision
}

fn invalidate_display(id: &str, device: &mut Device, out: &mut Vec<Effect>) {
    device.display_epoch += 1;
    device.pending_frame_id = None;
    device.last_received_frame = None;
    device.last_rendered_frame = None;
    out.push(Effect::DropQueuedFrames {
        device_id: id.to_owned(),
        incarnation: device.incarnation,
        through_display_epoch: device.display_epoch - 1,
    });
}

fn apply(
    state: &mut World,
    registry: &Registry,
    transition: Transition,
    out: &mut Vec<Effect>,
) -> Result<bool, Error> {
    match transition {
        Transition::Hello {
            device_id,
            capabilities,
            screen_owner_epoch,
        } => {
            if let Some(d) = state.devices.get_mut(&device_id) {
                if d.online {
                    out.push(Effect::CloseSession(SessionToken {
                        device_id: device_id.clone(),
                        incarnation: d.incarnation,
                        session_epoch: d.session_epoch,
                    }));
                }
                d.session_epoch += 1;
                d.online = true;
                d.owner = Owner::Remote;
                d.screen_owner_epoch = screen_owner_epoch;
                d.mode = Mode::Normal;
                if d.capabilities != capabilities {
                    d.capabilities = capabilities;
                    d.enabled
                        .retain(|id| registry.supports(id, &d.capabilities));
                    d.settings_revision += 1;
                }
                if d.selected.as_ref().is_none_or(|id| !d.enabled.contains(id)) {
                    d.selected = first_enabled(&state.plugin_order, d);
                }
                invalidate_display(&device_id, d, out);
                show_selected(&device_id, d, out);
            } else {
                let incarnation = state.next_incarnation;
                state.next_incarnation += 1;
                let enabled: BTreeSet<_> = state
                    .plugin_order
                    .iter()
                    .filter(|id| registry.supports(id, &capabilities))
                    .cloned()
                    .collect();
                let selected = state
                    .plugin_order
                    .iter()
                    .find(|id| enabled.contains(*id))
                    .cloned();
                let d = Device {
                    incarnation,
                    capabilities,
                    enabled,
                    selected,
                    mode: Mode::Normal,
                    owner: Owner::Remote,
                    screen_owner_epoch,
                    online: true,
                    session_epoch: 1,
                    display_epoch: 1,
                    settings_revision: 1,
                    requests: BTreeMap::new(),
                    datasets: BTreeMap::new(),
                    pending_frame_id: None,
                    last_received_frame: None,
                    last_rendered_frame: None,
                };
                show_selected(&device_id, &d, out);
                state.devices.insert(device_id, d);
            }
            Ok(true)
        }
        Transition::Disconnect(token) => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if d.incarnation != token.incarnation
                || d.session_epoch != token.session_epoch
                || !d.online
            {
                return Ok(false);
            }
            d.online = false;
            d.owner = Owner::Local;
            d.mode = Mode::Normal;
            invalidate_display(&token.device_id, d, out);
            Ok(true)
        }
        Transition::Delete(id) => {
            let Some(d) = state.devices.remove(&id) else {
                return Ok(false);
            };
            if d.online {
                out.push(Effect::CloseSession(SessionToken {
                    device_id: id.clone(),
                    incarnation: d.incarnation,
                    session_epoch: d.session_epoch,
                }));
            }
            out.push(Effect::DropQueuedFrames {
                device_id: id,
                incarnation: d.incarnation,
                through_display_epoch: d.display_epoch,
            });
            Ok(true)
        }
        Transition::SetEnabled {
            device_id,
            plugin,
            enabled,
            expected_settings_revision,
        } => {
            if !registry.contains(&plugin) {
                return Err(Error::UnknownPlugin(plugin));
            }
            let Some(d) = state.devices.get_mut(&device_id) else {
                return Ok(false);
            };
            if d.settings_revision != expected_settings_revision {
                return Err(Error::StaleSettingsRevision {
                    expected: expected_settings_revision,
                    actual: d.settings_revision,
                });
            }
            if enabled && !registry.supports(&plugin, &d.capabilities) {
                return Err(Error::UnsupportedPlugin(plugin));
            }
            let changed = if enabled {
                d.enabled.insert(plugin.clone())
            } else {
                d.enabled.remove(&plugin)
            };
            if !changed {
                return Ok(false);
            }
            d.settings_revision += 1;
            if d.selected.as_ref().is_none_or(|id| !d.enabled.contains(id)) {
                d.selected = first_enabled(&state.plugin_order, d);
                d.mode = Mode::Normal;
                invalidate_display(&device_id, d, out);
                show_selected(&device_id, d, out);
            }
            Ok(true)
        }
        Transition::PluginDataChanged {
            device_id,
            incarnation,
            plugin,
        } => {
            if !registry.contains(&plugin) {
                return Err(Error::UnknownPlugin(plugin));
            }
            let Some(d) = state.devices.get_mut(&device_id) else {
                return Ok(false);
            };
            if d.incarnation != incarnation {
                return Ok(false);
            }
            if d.online
                && d.selected.as_deref() == Some(&plugin)
                && matches!(d.mode, Mode::Normal | Mode::Sub)
            {
                invalidate_display(&device_id, d, out);
                show_selected(&device_id, d, out);
            }
            Ok(true)
        }
        Transition::PluginConfigChanged {
            device_id,
            incarnation,
            plugin,
            expected_settings_revision,
        } => {
            if !registry.contains(&plugin) {
                return Err(Error::UnknownPlugin(plugin));
            }
            let Some(d) = state.devices.get_mut(&device_id) else {
                return Ok(false);
            };
            if d.incarnation != incarnation || d.settings_revision != expected_settings_revision {
                return Ok(false);
            }
            d.settings_revision += 1;
            if d.selected.as_deref() == Some(&plugin) {
                d.mode = Mode::Normal;
                invalidate_display(&device_id, d, out);
                show_selected(&device_id, d, out);
            }
            Ok(true)
        }
        Transition::Next(token) => {
            let id = &token.device_id;
            let Some(d) = state.devices.get_mut(id) else {
                return Ok(false);
            };
            if !session_current(d, &token) || d.mode != Mode::Normal {
                return Ok(false);
            }
            let available: Vec<_> = state
                .plugin_order
                .iter()
                .filter(|p| d.enabled.contains(*p))
                .collect();
            if available.len() < 2 {
                return Ok(false);
            }
            let pos = available
                .iter()
                .position(|p| Some(*p) == d.selected.as_ref());
            let selected = available[(pos.map(|p| p + 1).unwrap_or(0)) % available.len()].clone();
            d.selected = Some(selected);
            invalidate_display(id, d, out);
            show_selected(id, d, out);
            Ok(true)
        }
        Transition::Previous(token) => {
            let id = &token.device_id;
            let Some(d) = state.devices.get_mut(id) else {
                return Ok(false);
            };
            if !session_current(d, &token) || d.mode != Mode::Normal {
                return Ok(false);
            }
            let available: Vec<_> = state
                .plugin_order
                .iter()
                .filter(|p| d.enabled.contains(*p))
                .collect();
            if available.len() < 2 {
                return Ok(false);
            }
            let pos = available
                .iter()
                .position(|p| Some(*p) == d.selected.as_ref());
            let selected = available[pos
                .map(|p| (p + available.len() - 1) % available.len())
                .unwrap_or(0)]
            .clone();
            d.selected = Some(selected);
            invalidate_display(id, d, out);
            show_selected(id, d, out);
            Ok(true)
        }
        Transition::Power(token) => {
            let id = &token.device_id;
            let Some(d) = state.devices.get_mut(id) else {
                return Ok(false);
            };
            if !session_current(d, &token) {
                return Ok(false);
            }
            match d.mode {
                Mode::Standby => Ok(false),
                Mode::Sub | Mode::Menu => {
                    let was_sub = d.mode == Mode::Sub;
                    d.mode = Mode::Normal;
                    invalidate_display(id, d, out);
                    if was_sub {
                        if let Some(plugin) = &d.selected {
                            out.push(Effect::ResetPluginView {
                                token: display_token(id, d),
                                plugin: plugin.clone(),
                            });
                        }
                    }
                    show_selected(id, d, out);
                    Ok(true)
                }
                Mode::Normal => {
                    d.mode = Mode::Standby;
                    d.owner = Owner::Local;
                    d.screen_owner_epoch += 1;
                    invalidate_display(id, d, out);
                    out.push(Effect::EnterStandby { token });
                    Ok(true)
                }
            }
        }
        Transition::EnterMode { token, mode } => {
            let id = &token.device_id;
            let Some(d) = state.devices.get_mut(id) else {
                return Ok(false);
            };
            if d.mode != Mode::Normal || !session_current(d, &token) {
                return Ok(false);
            }
            d.mode = mode;
            invalidate_display(id, d, out);
            Ok(true)
        }
        Transition::RefreshView { token, plugin } => {
            let id = &token.device_id;
            let Some(d) = state.devices.get_mut(id) else {
                return Ok(false);
            };
            if !session_current(d, &token)
                || d.mode != Mode::Sub
                || d.selected.as_deref() != Some(plugin.as_str())
            {
                return Ok(false);
            }
            invalidate_display(id, d, out);
            Ok(true)
        }
        Transition::LocalOwner {
            token,
            epoch,
            owner,
        } => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if !session_current(d, &token) || epoch <= d.screen_owner_epoch {
                return Ok(false);
            }
            d.screen_owner_epoch = epoch;
            d.owner = owner;
            invalidate_display(&token.device_id, d, out);
            show_selected(&token.device_id, d, out);
            Ok(true)
        }
        Transition::BeginFetch { token, key } => {
            if !registry.contains(&key.plugin) {
                return Err(Error::UnknownPlugin(key.plugin));
            }
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if d.incarnation != token.incarnation || !d.enabled.contains(&key.plugin) {
                return Ok(false);
            }
            let request_id = d.requests.entry(key.clone()).or_insert(0);
            *request_id += 1;
            d.datasets.insert(key.clone(), Dataset::Loading);
            out.push(Effect::Fetch(FetchToken {
                device_id: token.device_id,
                incarnation: d.incarnation,
                settings_revision: d.settings_revision,
                request_id: *request_id,
                key,
            }));
            Ok(true)
        }
        Transition::FetchCompleted { token, result } => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if d.incarnation != token.incarnation
                || d.settings_revision != token.settings_revision
                || d.requests.get(&token.key) != Some(&token.request_id)
                || d.datasets.get(&token.key) != Some(&Dataset::Loading)
            {
                return Ok(false);
            }
            d.datasets.insert(
                token.key,
                match result {
                    Ok(snapshot_id) => Dataset::Ready { snapshot_id },
                    Err(error_kind) => Dataset::Failed { error_kind },
                },
            );
            Ok(true)
        }
        Transition::RequestFrame {
            token,
            plugin,
            asset_handle,
        } => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if !frame_display_current(d, &token)
                || d.selected.as_deref() != Some(plugin.as_str())
                || d.pending_frame_id.is_some()
            {
                return Ok(false);
            }
            let frame_id = state.next_frame_id;
            let frame = FrameToken {
                device_id: token.device_id,
                incarnation: token.incarnation,
                session_epoch: token.session_epoch,
                display_epoch: token.display_epoch,
                screen_owner_epoch: token.screen_owner_epoch,
                frame_id,
            };
            d.pending_frame_id = Some(frame_id);
            d.last_received_frame = None;
            state.next_frame_id += 1;
            out.push(Effect::SendFrame {
                token: frame,
                asset_handle,
            });
            Ok(true)
        }
        Transition::ReceiptAck(token) => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if !frame_current(d, &token) || d.pending_frame_id != Some(token.frame_id) {
                return Ok(false);
            }
            if d.last_received_frame == Some(token.frame_id) {
                return Ok(false);
            }
            d.last_received_frame = Some(token.frame_id);
            Ok(true)
        }
        Transition::RenderedAck(token) => {
            let Some(d) = state.devices.get_mut(&token.device_id) else {
                return Ok(false);
            };
            if !frame_current(d, &token) || d.pending_frame_id != Some(token.frame_id) {
                return Ok(false);
            }
            d.pending_frame_id = None;
            d.last_received_frame = Some(token.frame_id);
            d.last_rendered_frame = Some(token.frame_id);
            Ok(true)
        }
    }
}

pub fn frame_current(d: &Device, token: &FrameToken) -> bool {
    d.online
        && d.owner == Owner::Remote
        && matches!(d.mode, Mode::Normal | Mode::Sub)
        && d.incarnation == token.incarnation
        && d.session_epoch == token.session_epoch
        && d.display_epoch == token.display_epoch
        && d.screen_owner_epoch == token.screen_owner_epoch
}

/// Independent Pico-side gate. The physical device must check before starting a draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenKind {
    Lcd,
    Eink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalScreen {
    Connection,
    Menu,
    Standby,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawFinish {
    RenderedAck(FrameToken),
    RepaintLocal,
    HoldImage,
    Discard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicoScreen {
    pub kind: ScreenKind,
    pub device_id: DeviceId,
    pub incarnation: u64,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub screen_owner_epoch: u64,
    pub local: Option<LocalScreen>,
    pub drawing: Option<FrameToken>,
    pub pending_standby: bool,
}

impl PicoScreen {
    pub fn can_begin(&self, token: &FrameToken) -> bool {
        self.local.is_none()
            && self.drawing.is_none()
            && !self.pending_standby
            && token.device_id == self.device_id
            && token.incarnation == self.incarnation
            && token.session_epoch == self.session_epoch
            && token.display_epoch == self.display_epoch
            && token.screen_owner_epoch == self.screen_owner_epoch
    }
    pub fn begin(&mut self, token: FrameToken) -> bool {
        if !self.can_begin(&token) {
            return false;
        }
        self.drawing = Some(token);
        true
    }
    pub fn take_local(&mut self, screen: LocalScreen) {
        self.screen_owner_epoch += 1;
        self.pending_standby = screen == LocalScreen::Standby
            && self.kind == ScreenKind::Eink
            && self.drawing.is_some();
        self.local = Some(screen);
        // A transport queue must discard frames with the old owner epoch here.
    }
    pub fn finish(&mut self, token: FrameToken) -> DrawFinish {
        if self.drawing.as_ref() != Some(&token) {
            return DrawFinish::Discard;
        }
        self.drawing = None;
        if self.pending_standby {
            self.pending_standby = false;
            return DrawFinish::HoldImage;
        }
        if self.local.is_some() {
            return DrawFinish::RepaintLocal;
        }
        if self.can_begin(&token) {
            DrawFinish::RenderedAck(token)
        } else {
            DrawFinish::Discard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        id: &'static str,
        required: &'static str,
        sub: bool,
        menu: bool,
        duplicate: bool,
    }
    impl Plugin for TestPlugin {
        fn id(&self) -> &str {
            self.id
        }
        fn supports(&self, caps: &BTreeSet<Capability>) -> bool {
            caps.contains(self.required)
        }
        fn candidates<'a>(
            &'a self,
            q: &'a dyn WorldQuery,
            event: &'a Event,
        ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
            let mut result = Vec::new();
            if let Event::EnterSub { token } = event {
                if self.sub {
                    result.push(Candidate::new(
                        q,
                        &token.device_id,
                        Priority::User,
                        "plugin-sub",
                        Transition::EnterMode {
                            token: token.clone(),
                            mode: Mode::Sub,
                        },
                    ));
                    if self.duplicate {
                        result.push(Candidate::new(
                            q,
                            &token.device_id,
                            Priority::User,
                            "duplicate-sub",
                            Transition::EnterMode {
                                token: token.clone(),
                                mode: Mode::Sub,
                            },
                        ));
                    }
                }
            }
            if let Event::EnterMenu { token } = event {
                if self.menu {
                    result.push(Candidate::new(
                        q,
                        &token.device_id,
                        Priority::User,
                        "plugin-menu",
                        Transition::EnterMode {
                            token: token.clone(),
                            mode: Mode::Menu,
                        },
                    ));
                }
            }
            Box::new(result.into_iter())
        }
    }

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }
    fn engine() -> Engine {
        let mut r = Registry::default();
        r.register(Box::new(TestPlugin {
            id: "a",
            required: "lcd",
            sub: true,
            menu: true,
            duplicate: false,
        }))
        .unwrap();
        r.register(Box::new(TestPlugin {
            id: "b",
            required: "lcd",
            sub: false,
            menu: false,
            duplicate: false,
        }))
        .unwrap();
        r.register(Box::new(TestPlugin {
            id: "eink",
            required: "eink",
            sub: false,
            menu: false,
            duplicate: false,
        }))
        .unwrap();
        Engine::new(r)
    }
    fn initial() -> (Engine, World) {
        (
            engine(),
            World::new(vec!["a".into(), "b".into(), "eink".into()]),
        )
    }
    fn step(e: &Engine, w: &mut World, event: Event) -> Decision {
        let decision = e.handle(w, &event).unwrap();
        *w = decision.state.clone();
        decision
    }
    fn hello(e: &Engine, w: &mut World, id: &str) {
        step(
            e,
            w,
            Event::Hello {
                device_id: id.into(),
                capabilities: set(&["lcd"]),
                screen_owner_epoch: 1,
            },
        );
    }
    fn device<'a>(w: &'a World, id: &str) -> &'a Device {
        w.devices.get(id).unwrap()
    }
    enum InputKind {
        Next,
        Power,
        Sub,
        Menu,
    }
    fn input_step(e: &Engine, w: &mut World, id: &str, kind: InputKind) -> Decision {
        let token = session_token(id, device(w, id));
        let event = match kind {
            InputKind::Next => Event::Next { token },
            InputKind::Power => Event::Power { token },
            InputKind::Sub => Event::EnterSub { token },
            InputKind::Menu => Event::EnterMenu { token },
        };
        step(e, w, event)
    }
    fn owner_step(e: &Engine, w: &mut World, id: &str, epoch: u64, owner: Owner) -> Decision {
        let token = session_token(id, device(w, id));
        step(
            e,
            w,
            Event::LocalOwner {
                token,
                epoch,
                owner,
            },
        )
    }
    fn enabled_step(e: &Engine, w: &mut World, id: &str, plugin: &str, enabled: bool) -> Decision {
        let expected_settings_revision = device(w, id).settings_revision;
        step(
            e,
            w,
            Event::SetEnabled {
                device_id: id.into(),
                plugin: plugin.into(),
                enabled,
                expected_settings_revision,
            },
        )
    }
    fn fetch_key() -> FetchKey {
        FetchKey {
            plugin: "a".into(),
            key: "region-x".into(),
        }
    }
    fn fetch(e: &Engine, w: &mut World, id: &str) -> FetchToken {
        let token = device_token(id, device(w, id));
        let d = step(
            e,
            w,
            Event::BeginFetch {
                token,
                key: fetch_key(),
            },
        );
        match &d.effects[0] {
            Effect::Fetch(token) => token.clone(),
            _ => panic!("expected fetch"),
        }
    }
    fn frame(e: &Engine, w: &mut World, id: &str, plugin: &str) -> FrameToken {
        let token = display_token(id, device(w, id));
        let d = step(
            e,
            w,
            Event::RequestFrame {
                token,
                plugin: plugin.into(),
                asset_handle: "asset-ref".into(),
            },
        );
        match &d.effects[0] {
            Effect::SendFrame { token, .. } => token.clone(),
            _ => panic!("expected frame"),
        }
    }

    #[test]
    fn hello_registers_supported_plugins_and_reconnects_without_restoring_submode() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let d = device(&w, "one");
        assert_eq!(d.enabled, set(&["a", "b"]));
        assert_eq!(d.selected.as_deref(), Some("a"));
        let first = d.incarnation;
        input_step(&e, &mut w, "one", InputKind::Sub);
        assert_eq!(device(&w, "one").mode, Mode::Sub);
        hello(&e, &mut w, "one");
        assert_eq!(device(&w, "one").mode, Mode::Normal);
        assert_eq!(device(&w, "one").incarnation, first);
        assert_eq!(device(&w, "one").session_epoch, 2);
    }

    #[test]
    fn delete_then_reregister_rejects_old_fetch_even_with_same_revision() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old = fetch(&e, &mut w, "one");
        step(
            &e,
            &mut w,
            Event::Delete {
                device_id: "one".into(),
            },
        );
        hello(&e, &mut w, "one");
        let new = fetch(&e, &mut w, "one");
        assert_ne!(old.incarnation, new.incarnation);
        assert_eq!(old.settings_revision, new.settings_revision);
        let before = w.clone();
        step(
            &e,
            &mut w,
            Event::FetchCompleted {
                token: old,
                snapshot_id: "obsolete".into(),
            },
        );
        assert_eq!(w, before);
        step(
            &e,
            &mut w,
            Event::FetchCompleted {
                token: new,
                snapshot_id: "current".into(),
            },
        );
        assert_eq!(
            device(&w, "one").datasets.get(&fetch_key()),
            Some(&Dataset::Ready {
                snapshot_id: "current".into()
            })
        );
    }

    #[test]
    fn newer_request_wins_at_same_settings_revision_and_duplicate_completion_is_ignored() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old = fetch(&e, &mut w, "one");
        let new = fetch(&e, &mut w, "one");
        assert_eq!(old.settings_revision, new.settings_revision);
        assert!(old.request_id < new.request_id);
        step(
            &e,
            &mut w,
            Event::FetchCompleted {
                token: new.clone(),
                snapshot_id: "new".into(),
            },
        );
        let before = w.clone();
        step(
            &e,
            &mut w,
            Event::FetchCompleted {
                token: old,
                snapshot_id: "old".into(),
            },
        );
        step(
            &e,
            &mut w,
            Event::FetchFailed {
                token: new,
                error_kind: "late-failure".into(),
            },
        );
        assert_eq!(w, before);
    }

    #[test]
    fn stale_disconnect_and_rendered_ack_cannot_affect_new_session() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let token = frame(&e, &mut w, "one", "a");
        let old_session = SessionToken {
            device_id: "one".into(),
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
        };
        hello(&e, &mut w, "one");
        let before = w.clone();
        step(&e, &mut w, Event::Disconnect { token: old_session });
        step(&e, &mut w, Event::RenderedAck { token });
        assert_eq!(w, before);
        assert!(device(&w, "one").online);
    }

    #[test]
    fn old_session_input_cannot_change_new_session_or_reach_plugin_rule() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old = session_token("one", device(&w, "one"));
        hello(&e, &mut w, "one");
        let before = w.clone();
        for event in [
            Event::Next { token: old.clone() },
            Event::Power { token: old.clone() },
            Event::EnterSub { token: old.clone() },
            Event::PluginInput {
                token: old.clone(),
                name: "old".into(),
            },
            Event::LocalOwner {
                token: old,
                epoch: 999,
                owner: Owner::Local,
            },
        ] {
            step(&e, &mut w, event);
        }
        assert_eq!(w, before);
    }

    #[test]
    fn old_display_result_cannot_render_after_switch_back_or_reregistration() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old = display_token("one", device(&w, "one"));
        input_step(&e, &mut w, "one", InputKind::Next);
        input_step(&e, &mut w, "one", InputKind::Next);
        assert_eq!(device(&w, "one").selected.as_deref(), Some("a"));
        let before = w.clone();
        step(
            &e,
            &mut w,
            Event::RequestFrame {
                token: old.clone(),
                plugin: "a".into(),
                asset_handle: "stale".into(),
            },
        );
        assert_eq!(w, before);
        step(
            &e,
            &mut w,
            Event::Delete {
                device_id: "one".into(),
            },
        );
        hello(&e, &mut w, "one");
        let before = w.clone();
        step(
            &e,
            &mut w,
            Event::RequestFrame {
                token: old,
                plugin: "a".into(),
                asset_handle: "stale".into(),
            },
        );
        assert_eq!(w, before);
        let new = display_token("one", device(&w, "one"));
        assert!(matches!(
            step(
                &e,
                &mut w,
                Event::RequestFrame {
                    token: new,
                    plugin: "a".into(),
                    asset_handle: "fresh".into()
                }
            )
            .effects
            .as_slice(),
            [Effect::SendFrame { .. }]
        ));
    }

    #[test]
    fn old_timer_cannot_begin_fetch_for_new_incarnation() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old = device_token("one", device(&w, "one"));
        step(
            &e,
            &mut w,
            Event::Delete {
                device_id: "one".into(),
            },
        );
        hello(&e, &mut w, "one");
        let before = w.clone();
        step(
            &e,
            &mut w,
            Event::BeginFetch {
                token: old,
                key: fetch_key(),
            },
        );
        assert_eq!(w, before);
    }

    #[test]
    fn stale_web_settings_revision_is_rejected() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let old_revision = device(&w, "one").settings_revision;
        enabled_step(&e, &mut w, "one", "b", false);
        let before = w.clone();
        let error = e
            .handle(
                &w,
                &Event::SetEnabled {
                    device_id: "one".into(),
                    plugin: "a".into(),
                    enabled: false,
                    expected_settings_revision: old_revision,
                },
            )
            .unwrap_err();
        assert_eq!(
            error,
            Error::StaleSettingsRevision {
                expected: old_revision,
                actual: device(&w, "one").settings_revision
            }
        );
        assert_eq!(w, before);
    }

    #[test]
    fn web_off_selected_clears_sub_and_falls_back_then_logo_even_offline() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        input_step(&e, &mut w, "one", InputKind::Sub);
        let previous_epoch = device(&w, "one").display_epoch;
        let result = enabled_step(&e, &mut w, "one", "a", false);
        assert_eq!(device(&w, "one").selected.as_deref(), Some("b"));
        assert_eq!(device(&w, "one").mode, Mode::Normal);
        assert!(device(&w, "one").display_epoch > previous_epoch);
        assert!(result
            .effects
            .iter()
            .any(|x| matches!(x, Effect::ShowSelected { plugin, .. } if plugin == "b")));
        let d = device(&w, "one");
        let session = SessionToken {
            device_id: "one".into(),
            incarnation: d.incarnation,
            session_epoch: d.session_epoch,
        };
        step(&e, &mut w, Event::Disconnect { token: session });
        enabled_step(&e, &mut w, "one", "b", false);
        assert_eq!(device(&w, "one").selected, None);
        hello(&e, &mut w, "one");
        assert!(input_step(&e, &mut w, "one", InputKind::Power)
            .effects
            .iter()
            .any(|x| matches!(x, Effect::EnterStandby { .. })));
        hello(&e, &mut w, "one");
        assert!(owner_step(&e, &mut w, "one", 99, Owner::Remote)
            .effects
            .iter()
            .any(|x| matches!(x, Effect::ShowLogo { .. })));
    }

    #[test]
    fn power_is_scoped_to_current_mode() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        input_step(&e, &mut w, "one", InputKind::Sub);
        let cancel = input_step(&e, &mut w, "one", InputKind::Power);
        assert_eq!(device(&w, "one").mode, Mode::Normal);
        assert!(cancel.effects.iter().any(|effect| matches!(effect,
            Effect::ResetPluginView { plugin, .. } if plugin == "a")));
        input_step(&e, &mut w, "one", InputKind::Menu);
        assert_eq!(device(&w, "one").mode, Mode::Menu);
        let cancel_menu = input_step(&e, &mut w, "one", InputKind::Power);
        assert_eq!(device(&w, "one").mode, Mode::Normal);
        assert!(!cancel_menu
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::EnterStandby { .. })));
        input_step(&e, &mut w, "one", InputKind::Power);
        assert_eq!(device(&w, "one").mode, Mode::Standby);
        assert_eq!(device(&w, "one").owner, Owner::Local);
    }

    #[test]
    fn web_cannot_enable_plugin_missing_firmware_capability() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let err = e
            .handle(
                &w,
                &Event::SetEnabled {
                    device_id: "one".into(),
                    plugin: "eink".into(),
                    enabled: true,
                    expected_settings_revision: device(&w, "one").settings_revision,
                },
            )
            .unwrap_err();
        assert_eq!(err, Error::UnsupportedPlugin("eink".into()));
        assert!(!device(&w, "one").enabled.contains("eink"));
    }

    #[test]
    fn plugin_rules_route_only_to_selected_plugin_and_ambiguity_is_error() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        input_step(&e, &mut w, "one", InputKind::Next);
        assert_eq!(device(&w, "one").selected.as_deref(), Some("b"));
        input_step(&e, &mut w, "one", InputKind::Sub);
        assert_eq!(device(&w, "one").mode, Mode::Normal);
        let mut registry = Registry::default();
        registry
            .register(Box::new(TestPlugin {
                id: "a",
                required: "lcd",
                sub: true,
                menu: false,
                duplicate: true,
            }))
            .unwrap();
        let conflicting = Engine::new(registry);
        let mut one = World::new(vec!["a".into()]);
        hello(&conflicting, &mut one, "one");
        let result = conflicting.handle(
            &one,
            &Event::EnterSub {
                token: session_token("one", device(&one, "one")),
            },
        );
        assert!(matches!(result, Err(Error::Ambiguous { reasons, .. }) if reasons.len() == 2));
    }

    #[test]
    fn plugin_candidate_cannot_change_another_device() {
        struct CrossDevicePlugin;
        impl Plugin for CrossDevicePlugin {
            fn id(&self) -> &str {
                "cross"
            }
            fn supports(&self, _caps: &BTreeSet<Capability>) -> bool {
                true
            }
            fn candidates<'a>(
                &'a self,
                q: &'a dyn WorldQuery,
                event: &'a Event,
            ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
                if let Event::PluginInput { token, .. } = event {
                    Box::new(std::iter::once(Candidate::new(
                        q,
                        &token.device_id,
                        Priority::User,
                        "invalid-other-device",
                        Transition::Power(SessionToken {
                            device_id: "two".into(),
                            incarnation: token.incarnation,
                            session_epoch: token.session_epoch,
                        }),
                    )))
                } else {
                    Box::new(std::iter::empty())
                }
            }
        }
        let mut r = Registry::default();
        r.register(Box::new(CrossDevicePlugin)).unwrap();
        let e = Engine::new(r);
        let mut w = World::new(vec!["cross".into()]);
        hello(&e, &mut w, "one");
        hello(&e, &mut w, "two");
        let before = w.clone();
        let result = e.handle(
            &w,
            &Event::PluginInput {
                token: session_token("one", device(&w, "one")),
                name: "go".into(),
            },
        );
        assert!(matches!(result, Err(Error::InvalidScope { .. })));
        assert_eq!(w, before);
    }

    #[test]
    fn frame_ack_must_be_issued_and_current_and_is_idempotent() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let frame = frame(&e, &mut w, "one", "a");
        let mut fake = frame.clone();
        fake.frame_id += 100;
        let before = w.clone();
        step(&e, &mut w, Event::RenderedAck { token: fake });
        assert_eq!(w, before);
        // Only one unacknowledged frame is admitted by this draft.
        let display = display_token("one", device(&w, "one"));
        assert!(step(
            &e,
            &mut w,
            Event::RequestFrame {
                token: display,
                plugin: "a".into(),
                asset_handle: "new".into()
            }
        )
        .effects
        .is_empty());
        step(
            &e,
            &mut w,
            Event::RenderedAck {
                token: frame.clone(),
            },
        );
        assert_eq!(device(&w, "one").last_rendered_frame, Some(frame.frame_id));
        let before = w.clone();
        step(&e, &mut w, Event::RenderedAck { token: frame });
        assert_eq!(w, before);
    }

    #[test]
    fn local_owner_takes_screen_from_queued_server_frame() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        let frame = frame(&e, &mut w, "one", "a");
        owner_step(&e, &mut w, "one", 2, Owner::Local);
        assert!(!frame_current(device(&w, "one"), &frame));
        let before = w.clone();
        step(&e, &mut w, Event::RenderedAck { token: frame });
        assert_eq!(w, before);
    }

    #[test]
    fn pico_draw_gate_checks_epoch_before_start_and_recovers_after_physical_draw() {
        let frame = FrameToken {
            device_id: "one".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            frame_id: 7,
        };
        let mut lcd = PicoScreen {
            kind: ScreenKind::Lcd,
            device_id: "one".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            local: None,
            drawing: None,
            pending_standby: false,
        };
        assert!(lcd.begin(frame.clone()));
        lcd.take_local(LocalScreen::Connection);
        assert!(!lcd.can_begin(&frame));
        assert_eq!(lcd.finish(frame.clone()), DrawFinish::RepaintLocal);
        let mut eink = PicoScreen {
            kind: ScreenKind::Eink,
            device_id: "one".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            local: None,
            drawing: None,
            pending_standby: false,
        };
        assert!(eink.begin(frame.clone()));
        eink.take_local(LocalScreen::Standby);
        assert!(eink.pending_standby);
        assert_eq!(eink.finish(frame), DrawFinish::HoldImage);
        assert!(!eink.pending_standby);
    }

    #[test]
    fn pico_rejects_old_incarnation_even_if_other_epochs_repeat() {
        let mut pico = PicoScreen {
            kind: ScreenKind::Lcd,
            device_id: "one".into(),
            incarnation: 2,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            local: None,
            drawing: None,
            pending_standby: false,
        };
        let old = FrameToken {
            device_id: "one".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            frame_id: 1,
        };
        assert!(!pico.begin(old));
    }

    #[test]
    fn devices_keep_independent_selection_and_fetch_state() {
        let (e, mut w) = initial();
        hello(&e, &mut w, "one");
        hello(&e, &mut w, "two");
        input_step(&e, &mut w, "one", InputKind::Next);
        let two_fetch = fetch(&e, &mut w, "two");
        assert_eq!(device(&w, "one").selected.as_deref(), Some("b"));
        assert_eq!(device(&w, "two").selected.as_deref(), Some("a"));
        assert!(device(&w, "one").datasets.is_empty());
        step(
            &e,
            &mut w,
            Event::FetchFailed {
                token: two_fetch,
                error_kind: "source-unavailable".into(),
            },
        );
        assert!(matches!(
            device(&w, "two").datasets.get(&fetch_key()),
            Some(Dataset::Failed { .. })
        ));
        assert!(device(&w, "one").datasets.is_empty());
    }
}
