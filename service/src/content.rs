//! Plugin-owned content state and input planning. The application supplies a
//! read-only device context, applies the resulting kernel event, then commits
//! the cloned plugin state and sends the plugin's completed pixel buffer.

use crate::plugin_jobs::{
    AssetDescriptor, CommandContext, JobOutcome, JobProgress, PlaybackSpec, PluginJob,
    PluginWorkPlan,
};
use crate::settings::{FetchPolicy, PluginSettings};
use crate::{Event, Mode, SessionToken};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    Next,
    Sub,
    Up,
    Down,
    Power,
}

#[derive(Clone, Debug)]
pub struct PluginContext {
    pub token: SessionToken,
    pub settings_revision: u64,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginPosition {
    pub plugin: String,
    pub dataset_id: String,
    pub target_date: String,
    pub index: usize,
}

#[derive(Debug)]
pub enum ContentError {
    Unavailable,
    InvalidSource,
    Unsupported,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchJob {
    pub plugin: String,
    pub device_id: String,
    pub incarnation: u64,
    pub settings_revision: u64,
    pub config_revision: u64,
    pub generation: u64,
    pub source_key: String,
    pub dataset_id: String,
    pub today_jst: String,
    pub timeout_seconds: u32,
    pub retries: u8,
}

/// Plugin-owned result, computed without the shared application lock.
pub type PreparedFetch = Box<dyn std::any::Any + Send>;
pub type FetchPreparation =
    Box<dyn FnOnce(Result<Vec<u8>, String>) -> Result<PreparedFetch, ContentError> + Send>;

pub struct PluginPlan {
    pub event: Option<Event>,
    pub frame: Option<Vec<u8>>,
    pub position: PluginPosition,
}

pub trait ContentPlugin: Send + Sync {
    /// Archive IDs that remain in a plugin's live display or browse state.
    fn retention_references(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }
    /// Remove a non-displayed archive entry from persisted plugin state.
    fn retire_artwork(&mut self, _id: &str) -> Result<(), ContentError> {
        Err(ContentError::Unsupported)
    }
    /// Public shared content/settings/history; never include credentials.
    fn web_state(&self) -> Value {
        Value::Null
    }
    fn web_state_at(&self, _now_mono_ms: u64) -> Value {
        self.web_state()
    }
    /// Plan on a cloned state. No network, file IO, process or clock reads here.
    fn plan_command(
        &mut self,
        _ctx: &CommandContext,
        _command: &Value,
    ) -> Result<PluginWorkPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn plan_schedule(
        &mut self,
        _ctx: &CommandContext,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        Ok(None)
    }
    fn work_completed(
        &mut self,
        _ctx: &CommandContext,
        _job: &PluginJob,
        _result: &JobOutcome,
    ) -> Result<PluginWorkPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn work_progress(
        &mut self,
        _ctx: &CommandContext,
        _job: &PluginJob,
        _progress: &JobProgress,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        Ok(None)
    }
    fn playback(&self, _ctx: &PluginContext) -> Option<PlaybackSpec> {
        None
    }
    fn asset(&self, _asset_id: &str) -> Option<AssetDescriptor> {
        None
    }
    fn clone_box(&self) -> Box<dyn ContentPlugin>;
    fn id(&self) -> &'static str;
    fn publish(
        &mut self,
        ctx: &PluginContext,
        source: &[u8],
        dataset_id: &str,
        today_jst: &str,
    ) -> Result<(), ContentError>;
    fn position(&self, ctx: &PluginContext) -> Option<PluginPosition>;
    /// An uncached selected plugin replaces the previous image while fetching.
    fn loading_frame(&self) -> Vec<u8> {
        crate::lcd_text::status_frame(&self.id().to_ascii_uppercase(), "FETCHING")
    }
    /// Optional native monochrome asset matching the exact queued LCD snapshot.
    fn eink_frame_for(&self, _ctx: &PluginContext, _lcd: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn eink_refresh(&self, _ctx: &PluginContext) -> Option<crate::protocol::EinkRefresh> {
        None
    }
    /// A wall-clock refresh key, only queried for an online selected display.
    /// Returning None opts out of time-driven rendering entirely.
    fn time_key(&self, _ctx: &PluginContext, _now_unix: i64) -> Option<i64> {
        None
    }
    /// Snapshot host time before producing both LCD and native E-ink pixels.
    /// This is transient display state and must not read a clock or perform IO.
    fn observe_time(&mut self, _ctx: &PluginContext, _now_unix: i64) {}
    fn current_frame(&self, _ctx: &PluginContext) -> Option<Vec<u8>> {
        None
    }
    /// Observe a host-supplied calendar date before input or cached delivery.
    /// Plugins with dated content can rebase their normal view without reading
    /// the clock inside a Rule or inventing a missing day's data.
    fn observe_date(&mut self, _ctx: &PluginContext, _today_jst: &str) -> bool {
        false
    }
    fn plan(
        &mut self,
        ctx: &PluginContext,
        action: ButtonAction,
    ) -> Result<PluginPlan, ContentError>;
    /// Optional archive work requested by a button plan. The host commits it
    /// after accepting the button event; implementations keep IO in workers.
    fn button_work(&self, _ctx: &PluginContext, _action: ButtonAction) -> Option<PluginWorkPlan> {
        None
    }
    fn session_reset(&mut self, device_id: &str, incarnation: u64);
    fn delete_device(&mut self, device_id: &str);
    fn save_state(&self) -> Value {
        Value::Null
    }
    fn load_state(&mut self, _state: Value) -> Result<(), ContentError> {
        Ok(())
    }
    fn options(&self) -> Value {
        Value::Null
    }
    fn fetch_policy(&self, _settings: &PluginSettings) -> Option<FetchPolicy> {
        None
    }
    fn device_config(&self, _device_id: &str, _incarnation: u64) -> Value {
        Value::Null
    }
    fn set_device_config(
        &mut self,
        _ctx: &PluginContext,
        _expected_revision: u64,
        _config: Value,
    ) -> Result<u64, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn plan_fetch(
        &mut self,
        _ctx: &PluginContext,
        _now_unix: i64,
        _today_jst: &str,
        _policy: &FetchPolicy,
    ) -> Option<FetchJob> {
        None
    }
    fn fetch_preparation(&self, _job: &FetchJob) -> Result<FetchPreparation, ContentError> {
        Ok(Box::new(|source| Ok(Box::new(source) as PreparedFetch)))
    }
    fn apply_prepared_fetch(
        &mut self,
        job: &FetchJob,
        now_unix: i64,
        prepared: PreparedFetch,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        let source = *prepared
            .downcast::<Result<Vec<u8>, String>>()
            .map_err(|_| ContentError::InvalidSource)?;
        self.complete_fetch(job, now_unix, source)
    }
    fn complete_fetch(
        &mut self,
        _job: &FetchJob,
        _now_unix: i64,
        _result: Result<Vec<u8>, String>,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn cancel_fetches(&mut self) {}
}

impl Clone for Box<dyn ContentPlugin> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
