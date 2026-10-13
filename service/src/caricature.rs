//! Caricature rules and content. Generation IO is confined to `external`.
//! A host accepts a request under its short state lock, runs `external::generate`
//! without that lock, then applies the typed result and publishes the artifact.

mod evidence;
pub mod executor;
pub mod external;
mod hash;
pub mod store;
pub use executor::{CaricatureExecutor, CaricatureSuccess, CaricatureWork};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use time::{OffsetDateTime, UtcOffset};

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};

pub const ID: &str = "caricature";
pub const WIDTH_LCD: usize = 160;
pub const HEIGHT_LCD: usize = 80;
pub const WIDTH_EINK: u32 = 250;
pub const HEIGHT_EINK: u32 = 122;
pub const DEFAULT_MODEL: &str = "gpt-6-luna";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub id: String,
    pub label: String,
    pub prompt: String,
}

impl Selection {
    pub fn validate(&self) -> Result<(), CaricatureError> {
        let id = self.id.as_bytes();
        if id.is_empty()
            || id.len() > 40
            || !id[0].is_ascii_lowercase()
            || !id
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
            || self.label.trim().is_empty()
            || self.label.chars().count() > 80
            || self.prompt.trim().is_empty()
            || self.prompt.chars().count() > 1200
        {
            return Err(CaricatureError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub retry_count: u32,
    pub interval_seconds: u32,
    /// Zero disables the total elapsed-time deadline.
    pub deadline_seconds: u32,
    pub attempt_timeout_seconds: u32,
}

impl RetryPolicy {
    pub fn validate(&self) -> Result<(), CaricatureError> {
        if self.attempt_timeout_seconds == 0 || self.retry_count > 20 {
            return Err(CaricatureError::InvalidRequest);
        }
        Ok(())
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            retry_count: 2,
            interval_seconds: 0,
            deadline_seconds: 0,
            attempt_timeout_seconds: 180,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationSettings {
    pub revision: u64,
    pub topic: Selection,
    pub style: Selection,
    pub news_model: String,
    pub image_model: String,
    #[serde(default = "automatic_generation_default")]
    pub automatic_generation: bool,
    pub dpid_lambda: f64,
    pub threshold: u8,
    pub news_retry: RetryPolicy,
    pub image_retry: RetryPolicy,
}

fn automatic_generation_default() -> bool {
    true
}

impl GenerationSettings {
    pub fn validate(&self) -> Result<(), CaricatureError> {
        self.topic.validate()?;
        self.validate_without_topic()
    }
    /// Custom text does not use the selected news topic.
    pub fn validate_without_topic(&self) -> Result<(), CaricatureError> {
        self.style.validate()?;
        for model in [&self.news_model, &self.image_model] {
            let bytes = model.as_bytes();
            if bytes.is_empty()
                || bytes.len() > 128
                || !bytes[0].is_ascii_alphanumeric()
                || !bytes.iter().all(|c| (33..=126).contains(c))
            {
                return Err(CaricatureError::InvalidRequest);
            }
        }
        if !self.dpid_lambda.is_finite()
            || !(0.0..=1.0).contains(&self.dpid_lambda)
            || !(1..=254).contains(&self.threshold)
        {
            return Err(CaricatureError::InvalidRequest);
        }
        self.news_retry.validate()?;
        self.image_retry.validate()?;
        Ok(())
    }
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            revision: 0,
            topic: Selection { id: "ai_news".into(), label: "最新AIニュース".into(), prompt: "24時間以内に発表された、注目に値する面白いAIニュースを1件選び、何が起きたかを要約してください。".into() },
            style: Selection { id: "newspaper_cartoon".into(), label: "新聞風刺画".into(), prompt: "新聞の一コマ風刺画。太く抑揚のある黒い輪郭、白地に少数の象徴物、大きな主役と短い視覚的オチを置く。細かな網点を避け、背景は簡略化する。".into() },
            news_model: DEFAULT_MODEL.into(), image_model: DEFAULT_MODEL.into(),
            automatic_generation: true,
            dpid_lambda: 0.75, threshold: 128,
            news_retry: RetryPolicy::default(),
            image_retry: RetryPolicy { attempt_timeout_seconds: 300, ..RetryPolicy::default() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationInput {
    News,
    CustomText(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    /// Unique durable ID allocated by the host before work starts.
    pub job_id: String,
    /// UTC hour number for hourly jobs; negative values distinguish manual jobs.
    pub slot: i64,
    pub requested_at_unix: i64,
    pub input: GenerationInput,
    /// Immutable snapshot taken at acceptance, including both independent selections.
    pub settings: GenerationSettings,
}

impl GenerationRequest {
    pub fn validate(&self) -> Result<(), CaricatureError> {
        match &self.input {
            GenerationInput::News => self.settings.validate()?,
            GenerationInput::CustomText(_) => self.settings.validate_without_topic()?,
        }
        if self.job_id.is_empty()
            || self.job_id.len() > 100
            || !self
                .job_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || (self.slot >= 0 && self.slot != self.requested_at_unix.div_euclid(3600))
        {
            return Err(CaricatureError::InvalidRequest);
        }
        if let GenerationInput::CustomText(s) = &self.input {
            if self.slot >= 0 || s.trim().is_empty() || s.chars().count() > 4000 || s.contains('\0')
            {
                return Err(CaricatureError::InvalidRequest);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct News {
    pub source_url: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationStage {
    News,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationState {
    Accepted,
    Researching,
    NewsReady,
    Drawing,
    Validated,
    Published {
        artwork_id: String,
    },
    Failed {
        stage: GenerationStage,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryState {
    NotQueued,
    Queued,
    Sending,
    Rendered,
    Failed { reason: String },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationJob {
    pub request: GenerationRequest,
    pub state: GenerationState,
    pub news: Option<News>,
    pub news_attempts: u32,
    pub image_attempts: u32,
    pub delivery: DeliveryState,
}

impl GenerationJob {
    pub fn accept(request: GenerationRequest) -> Result<Self, CaricatureError> {
        request.validate()?;
        Ok(Self {
            request,
            state: GenerationState::Accepted,
            news: None,
            news_attempts: 0,
            image_attempts: 0,
            delivery: DeliveryState::NotQueued,
        })
    }
    pub fn recoverable(&self) -> bool {
        !matches!(
            self.state,
            GenerationState::Published { .. } | GenerationState::Failed { .. }
        )
    }
    pub fn publish(&mut self, artwork_id: String) -> Result<(), CaricatureError> {
        if self.state != GenerationState::Validated {
            return Err(CaricatureError::InvalidTransition);
        }
        self.state = GenerationState::Published { artwork_id };
        self.delivery = DeliveryState::NotQueued;
        Ok(())
    }
    pub fn set_delivery(&mut self, state: DeliveryState) -> Result<(), CaricatureError> {
        if !matches!(self.state, GenerationState::Published { .. }) {
            return Err(CaricatureError::InvalidTransition);
        }
        self.delivery = state;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaricatureError {
    InvalidRequest,
    InvalidTransition,
    InvalidSource,
    DuplicateSource,
    Stale,
    Io,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaricatureState {
    pub jobs: BTreeMap<String, GenerationJob>,
    pub latest_artwork_id: Option<String>,
    pub last_success_unix: Option<i64>,
    pub generation_error: Option<String>,
}

impl CaricatureState {
    /// Acceptance is a short state transition; persist it before dispatch.
    pub fn accept(&mut self, request: GenerationRequest) -> Result<GenerationJob, CaricatureError> {
        if self.jobs.contains_key(&request.job_id)
            || self
                .jobs
                .values()
                .any(|j| j.request.slot == request.slot || j.recoverable())
        {
            return Err(CaricatureError::InvalidTransition);
        }
        let job = GenerationJob::accept(request)?;
        self.jobs.insert(job.request.job_id.clone(), job.clone());
        Ok(job)
    }
    pub fn apply_validated(
        &mut self,
        mut job: GenerationJob,
        artwork_id: &str,
        published_at_unix: i64,
    ) -> Result<(), CaricatureError> {
        let current = self
            .jobs
            .get(&job.request.job_id)
            .ok_or(CaricatureError::Stale)?;
        if current.request != job.request
            || !current.recoverable()
            || !matches!(job.state, GenerationState::Validated)
        {
            return Err(CaricatureError::Stale);
        }
        job.publish(artwork_id.into())?;
        self.latest_artwork_id = Some(artwork_id.into());
        self.last_success_unix = Some(published_at_unix);
        self.generation_error = None;
        self.jobs.insert(job.request.job_id.clone(), job);
        Ok(())
    }
    pub fn apply_failed(&mut self, job: GenerationJob) -> Result<(), CaricatureError> {
        let current = self
            .jobs
            .get(&job.request.job_id)
            .ok_or(CaricatureError::Stale)?;
        if current.request != job.request || !current.recoverable() {
            return Err(CaricatureError::Stale);
        }
        let GenerationState::Failed { reason, .. } = &job.state else {
            return Err(CaricatureError::InvalidTransition);
        };
        if self
            .last_success_unix
            .is_none_or(|at| at <= job.request.requested_at_unix)
        {
            self.generation_error = Some(reason.clone());
        }
        self.jobs.insert(job.request.job_id.clone(), job);
        Ok(())
    }
    pub fn hourly_due(&self, now_unix: i64) -> bool {
        hourly_due(self.last_success_unix, now_unix)
    }
    pub fn cancel(&mut self, job_id: &str) {
        if self
            .jobs
            .get(job_id)
            .is_some_and(GenerationJob::recoverable)
        {
            self.jobs.remove(job_id);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtworkMetadata {
    pub job_id: String,
    pub slot: i64,
    pub created_at_unix: i64,
    pub input: GenerationInput,
    pub source_urls: Vec<String>,
    pub news_summary: Option<String>,
    pub settings: GenerationSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artwork {
    pub id: String,
    pub metadata: ArtworkMetadata,
    /// Canonical display asset. Validated 250x122 monochrome PNG.
    pub eink_png: Vec<u8>,
    /// RGB565 big-endian 160x80, accepted by the shared LCD queue.
    pub lcd_rgb565be: Vec<u8>,
}

impl Artwork {
    pub fn validate(&self) -> Result<(), CaricatureError> {
        if self.id.is_empty()
            || self.id.len() > 100
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || self.lcd_rgb565be.len() != WIDTH_LCD * HEIGHT_LCD * 2
            || self.eink_png.len() > 20_000_000
            || self.eink_png.len() < 33
            || &self.eink_png[0..8] != b"\x89PNG\r\n\x1a\n"
            || &self.eink_png[12..16] != b"IHDR"
            || u32::from_be_bytes(self.eink_png[16..20].try_into().unwrap()) != WIDTH_EINK
            || u32::from_be_bytes(self.eink_png[20..24].try_into().unwrap()) != HEIGHT_EINK
            || self.eink_png[24..29] != [1, 0, 0, 0, 0]
            || self.metadata.source_urls.len() > 10
        {
            return Err(CaricatureError::InvalidSource);
        }
        image::load_from_memory(&self.eink_png).map_err(|_| CaricatureError::InvalidSource)?;
        Ok(())
    }
    pub fn jst_day(&self) -> Result<String, CaricatureError> {
        let dt = OffsetDateTime::from_unix_timestamp(self.metadata.created_at_unix)
            .map_err(|_| CaricatureError::InvalidSource)?
            .to_offset(UtcOffset::from_hms(9, 0, 0).expect("JST"));
        Ok(format!(
            "{:04}-{:02}-{:02}",
            dt.year(),
            dt.month() as u8,
            dt.day()
        ))
    }
}

/// Pure projection for the shared content route. The host persists artwork,
/// then calls `set_latest` and queues returned pixel buffers for selected LCDs.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaricatureContent {
    latest: Option<Artwork>,
    generation_failed: bool,
    state: CaricatureState,
    settings: GenerationSettings,
    topics: Vec<Selection>,
    styles: Vec<Selection>,
    history: Vec<HistoryEntry>,
    #[serde(skip)]
    temporary: BTreeMap<String, (u64, Artwork)>,
    next_manual_slot: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub artwork_id: String,
    pub metadata: ArtworkMetadata,
    pub publish_seq: u64,
    pub published_at_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaricatureCommand {
    GenerateNews,
    GenerateCustom {
        text: String,
    },
    SetTopic {
        id: String,
    },
    SetStyle {
        id: String,
    },
    SetNewsModel {
        model: String,
    },
    SetImageModel {
        model: String,
    },
    SetAutomaticGeneration {
        enabled: bool,
    },
    SetNormalizer {
        dpid_lambda: f64,
        threshold: u8,
    },
    SetNewsRetry {
        policy: RetryPolicy,
    },
    SetImageRetry {
        policy: RetryPolicy,
    },
    ShowHistory {
        artwork_id: String,
        device_id: String,
    },
}

impl CaricatureContent {
    fn accept_manual(
        &mut self,
        now_unix: i64,
        input: GenerationInput,
    ) -> Result<crate::plugin_jobs::JobRequest, ContentError> {
        if !self.style_valid() || (matches!(&input, GenerationInput::News) && !self.topic_valid()) {
            return Err(ContentError::InvalidSource);
        }
        let slot = self
            .next_manual_slot
            .checked_sub(1)
            .ok_or(ContentError::InvalidSource)?;
        let request = GenerationRequest {
            job_id: format!("manual_{}", slot.unsigned_abs()),
            slot,
            requested_at_unix: now_unix,
            input,
            settings: self.settings.clone(),
        };
        let job = self
            .state
            .accept(request)
            .map_err(|_| ContentError::InvalidSource)?;
        self.next_manual_slot = slot;
        generation_job_request("manual", job)
    }
    pub fn set_catalog(
        &mut self,
        topics: Vec<Selection>,
        styles: Vec<Selection>,
    ) -> Result<(), CaricatureError> {
        if topics.is_empty() || styles.is_empty() {
            return Err(CaricatureError::InvalidRequest);
        }
        let mut ids = BTreeSet::new();
        for topic in &topics {
            topic.validate()?;
            if !ids.insert(&topic.id) {
                return Err(CaricatureError::InvalidRequest);
            }
        }
        ids.clear();
        for style in &styles {
            style.validate()?;
            if !ids.insert(&style.id) {
                return Err(CaricatureError::InvalidRequest);
            }
        }
        let mut selected_changed = false;
        if let Some(topic) = topics.iter().find(|item| item.id == self.settings.topic.id) {
            if self.settings.topic != *topic {
                self.settings.topic = topic.clone();
                selected_changed = true;
            }
        }
        if let Some(style) = styles.iter().find(|item| item.id == self.settings.style.id) {
            if self.settings.style != *style {
                self.settings.style = style.clone();
                selected_changed = true;
            }
        }
        if selected_changed {
            self.settings.revision = self.settings.revision.saturating_add(1);
        }
        self.topics = topics;
        self.styles = styles;
        Ok(())
    }
    pub fn settings(&self) -> &GenerationSettings {
        &self.settings
    }
    pub fn state(&self) -> &CaricatureState {
        &self.state
    }
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }
    fn topic_valid(&self) -> bool {
        self.topics.is_empty()
            || self
                .topics
                .iter()
                .any(|item| item.id == self.settings.topic.id)
    }
    fn style_valid(&self) -> bool {
        self.styles.is_empty()
            || self
                .styles
                .iter()
                .any(|item| item.id == self.settings.style.id)
    }
    pub fn web_state(&self) -> serde_json::Value {
        let last_custom_text = self
            .state
            .jobs
            .values()
            .filter_map(|job| match &job.request.input {
                GenerationInput::CustomText(text) => {
                    Some((job.request.requested_at_unix, text.as_str()))
                }
                GenerationInput::News => None,
            })
            .max_by_key(|(at, _)| *at)
            .map(|(_, text)| text)
            .unwrap_or("");
        serde_json::json!({
            "settings": self.settings,
            "topics": if self.topics.is_empty() { vec![self.settings.topic.clone()] } else { self.topics.clone() },
            "styles": if self.styles.is_empty() { vec![self.settings.style.clone()] } else { self.styles.clone() },
            "history": self.history,
            "topic_selection_valid": self.topic_valid(),
            "style_selection_valid": self.style_valid(),
            "last_custom_text": last_custom_text,
            "generation_error": self.state.generation_error,
            "latest_artwork_id": self.state.latest_artwork_id,
        })
    }
    pub fn plan_command(
        &mut self,
        ctx: &crate::plugin_jobs::CommandContext,
        value: &serde_json::Value,
    ) -> Result<crate::plugin_jobs::PluginWorkPlan, ContentError> {
        use crate::plugin_jobs::{PluginWorkPlan, RefreshTargets};
        let command: CaricatureCommand =
            serde_json::from_value(value.clone()).map_err(|_| ContentError::InvalidSource)?;
        let mut plan = PluginWorkPlan::default();
        match command {
            CaricatureCommand::GenerateNews => plan
                .jobs
                .push(self.accept_manual(ctx.now_unix, GenerationInput::News)?),
            CaricatureCommand::GenerateCustom { text } => plan
                .jobs
                .push(self.accept_manual(ctx.now_unix, GenerationInput::CustomText(text))?),
            CaricatureCommand::SetTopic { id } => {
                let selected = self
                    .topics
                    .iter()
                    .find(|x| x.id == id)
                    .cloned()
                    .or_else(|| {
                        (self.topics.is_empty() && self.settings.topic.id == id)
                            .then(|| self.settings.topic.clone())
                    })
                    .ok_or(ContentError::InvalidSource)?;
                self.settings.topic = selected;
                self.settings.revision += 1;
            }
            CaricatureCommand::SetStyle { id } => {
                let selected = self
                    .styles
                    .iter()
                    .find(|x| x.id == id)
                    .cloned()
                    .or_else(|| {
                        (self.styles.is_empty() && self.settings.style.id == id)
                            .then(|| self.settings.style.clone())
                    })
                    .ok_or(ContentError::InvalidSource)?;
                self.settings.style = selected;
                self.settings.revision += 1;
            }
            CaricatureCommand::SetNewsModel { model } => {
                let mut next = self.settings.clone();
                next.news_model = model;
                next.validate_without_topic()
                    .map_err(|_| ContentError::InvalidSource)?;
                next.revision += 1;
                self.settings = next;
            }
            CaricatureCommand::SetImageModel { model } => {
                let mut next = self.settings.clone();
                next.image_model = model;
                next.validate_without_topic()
                    .map_err(|_| ContentError::InvalidSource)?;
                next.revision += 1;
                self.settings = next;
            }
            CaricatureCommand::SetAutomaticGeneration { enabled } => {
                self.settings.automatic_generation = enabled;
                self.settings.revision += 1;
                if !enabled {
                    plan.cancel_slots.push("hourly".into());
                }
            }
            CaricatureCommand::SetNormalizer {
                dpid_lambda,
                threshold,
            } => {
                let mut next = self.settings.clone();
                next.dpid_lambda = dpid_lambda;
                next.threshold = threshold;
                next.validate_without_topic()
                    .map_err(|_| ContentError::InvalidSource)?;
                next.revision += 1;
                self.settings = next;
            }
            CaricatureCommand::SetNewsRetry { policy } => {
                policy.validate().map_err(|_| ContentError::InvalidSource)?;
                self.settings.news_retry = policy;
                self.settings.revision += 1;
            }
            CaricatureCommand::SetImageRetry { policy } => {
                policy.validate().map_err(|_| ContentError::InvalidSource)?;
                self.settings.image_retry = policy;
                self.settings.revision += 1;
            }
            CaricatureCommand::ShowHistory {
                artwork_id,
                device_id,
            } => {
                if !self
                    .history
                    .iter()
                    .any(|entry| entry.artwork_id == artwork_id)
                {
                    return Err(ContentError::InvalidSource);
                }
                let device = ctx
                    .devices
                    .iter()
                    .find(|d| {
                        d.context.token.device_id == device_id && d.selected.as_deref() == Some(ID)
                    })
                    .ok_or(ContentError::Unavailable)?;
                let incarnation = device.context.token.incarnation;
                let payload = CaricatureWork::ShowHistory {
                    artwork_id,
                    device_id,
                    incarnation,
                };
                plan.jobs.push(crate::plugin_jobs::JobRequest {
                    slot: "history".into(),
                    payload: serde_json::to_value(payload)
                        .map_err(|_| ContentError::InvalidSource)?,
                    timeout_seconds: 30,
                });
            }
        }
        plan.refresh = RefreshTargets::None;
        Ok(plan)
    }
    pub fn plan_schedule(
        &mut self,
        ctx: &crate::plugin_jobs::CommandContext,
    ) -> Result<Option<crate::plugin_jobs::PluginWorkPlan>, ContentError> {
        if !self.settings.automatic_generation {
            return Ok(None);
        }
        if !self.topic_valid() || !self.style_valid() {
            return Err(ContentError::InvalidSource);
        }
        if !self.state.hourly_due(ctx.now_unix) {
            return Ok(None);
        }
        let slot = hourly_slot(ctx.now_unix);
        let request = GenerationRequest {
            job_id: format!("hour_{slot}"),
            slot,
            requested_at_unix: ctx.now_unix,
            input: GenerationInput::News,
            settings: self.settings.clone(),
        };
        let job = match self.state.accept(request) {
            Ok(v) => v,
            Err(CaricatureError::InvalidTransition) => return Ok(None),
            Err(_) => return Err(ContentError::InvalidSource),
        };
        Ok(Some(crate::plugin_jobs::PluginWorkPlan {
            jobs: vec![generation_job_request("hourly", job)?],
            ..Default::default()
        }))
    }
    pub fn work_completed(
        &mut self,
        _ctx: &crate::plugin_jobs::CommandContext,
        job: &crate::plugin_jobs::PluginJob,
        result: &crate::plugin_jobs::JobOutcome,
    ) -> Result<crate::plugin_jobs::PluginWorkPlan, ContentError> {
        use crate::plugin_jobs::{PluginWorkPlan, RefreshTargets};
        let work: CaricatureWork =
            serde_json::from_value(job.payload.clone()).map_err(|_| ContentError::InvalidSource)?;
        let mut plan = PluginWorkPlan::default();
        match (work, result) {
            (CaricatureWork::Generate { .. }, crate::plugin_jobs::JobOutcome::Succeeded(value)) => {
                let success: CaricatureSuccess = serde_json::from_value(value.clone())
                    .map_err(|_| ContentError::InvalidSource)?;
                let CaricatureSuccess::Published { job, publication } = success else {
                    return Err(ContentError::InvalidSource);
                };
                self.state
                    .apply_validated(*job, &publication.artwork.id, publication.published_at_unix)
                    .map_err(|_| ContentError::Stale)?;
                self.set_latest(publication.artwork.clone())
                    .map_err(|_| ContentError::InvalidSource)?;
                self.temporary.clear();
                self.history.push(HistoryEntry {
                    artwork_id: publication.artwork.id,
                    metadata: publication.artwork.metadata,
                    publish_seq: publication.publish_seq,
                    published_at_unix: publication.published_at_unix,
                });
                plan.refresh = RefreshTargets::Selected;
            }
            (
                CaricatureWork::Generate { mut job },
                crate::plugin_jobs::JobOutcome::Failed(reason),
            ) => {
                let stage = if reason.code == "news_failed" {
                    GenerationStage::News
                } else {
                    GenerationStage::Image
                };
                job.state = GenerationState::Failed {
                    stage,
                    reason: reason.message.clone(),
                };
                self.state
                    .apply_failed(*job)
                    .map_err(|_| ContentError::Stale)?;
                self.generation_failed();
                plan.refresh = RefreshTargets::Selected;
            }
            (CaricatureWork::Generate { job }, crate::plugin_jobs::JobOutcome::Cancelled) => {
                self.state.cancel(&job.request.job_id);
            }
            (
                CaricatureWork::ShowHistory { .. },
                crate::plugin_jobs::JobOutcome::Succeeded(value),
            ) => {
                let success: CaricatureSuccess = serde_json::from_value(value.clone())
                    .map_err(|_| ContentError::InvalidSource)?;
                let CaricatureSuccess::HistoryFrame {
                    artwork,
                    device_id,
                    incarnation,
                } = success
                else {
                    return Err(ContentError::InvalidSource);
                };
                artwork
                    .validate()
                    .map_err(|_| ContentError::InvalidSource)?;
                self.temporary
                    .insert(device_id.clone(), (incarnation, *artwork));
                plan.refresh = RefreshTargets::Devices(vec![crate::DeviceToken {
                    device_id,
                    incarnation,
                }]);
            }
            (CaricatureWork::ShowHistory { .. }, _) => {}
        }
        Ok(plan)
    }
    pub fn set_latest(&mut self, artwork: Artwork) -> Result<(), CaricatureError> {
        artwork.validate()?;
        self.latest = Some(artwork);
        self.generation_failed = false;
        self.temporary.clear();
        Ok(())
    }
    /// Queue these pixels to one device without changing latest or history.
    pub fn temporary_frame(&self, artwork: &Artwork) -> Result<Vec<u8>, CaricatureError> {
        artwork.validate()?;
        Ok(artwork.lcd_rgb565be.clone())
    }
    pub fn generation_failed(&mut self) {
        self.generation_failed = true;
    }
    pub fn latest(&self) -> Option<&Artwork> {
        self.latest.as_ref()
    }
}

impl ContentPlugin for CaricatureContent {
    fn web_state(&self) -> serde_json::Value {
        CaricatureContent::web_state(self)
    }
    fn plan_command(
        &mut self,
        ctx: &crate::plugin_jobs::CommandContext,
        command: &serde_json::Value,
    ) -> Result<crate::plugin_jobs::PluginWorkPlan, ContentError> {
        CaricatureContent::plan_command(self, ctx, command)
    }
    fn plan_schedule(
        &mut self,
        ctx: &crate::plugin_jobs::CommandContext,
    ) -> Result<Option<crate::plugin_jobs::PluginWorkPlan>, ContentError> {
        CaricatureContent::plan_schedule(self, ctx)
    }
    fn work_completed(
        &mut self,
        ctx: &crate::plugin_jobs::CommandContext,
        job: &crate::plugin_jobs::PluginJob,
        result: &crate::plugin_jobs::JobOutcome,
    ) -> Result<crate::plugin_jobs::PluginWorkPlan, ContentError> {
        CaricatureContent::work_completed(self, ctx, job, result)
    }
    fn asset(&self, asset_id: &str) -> Option<crate::plugin_jobs::AssetDescriptor> {
        if !self
            .history
            .iter()
            .any(|entry| entry.artwork_id == asset_id)
        {
            return None;
        }
        Some(crate::plugin_jobs::AssetDescriptor {
            area: crate::plugin_jobs::AssetArea::Data,
            relative_path: std::path::PathBuf::from("artwork/archive")
                .join(format!("{asset_id}.png")),
            content_type: "image/png".into(),
        })
    }
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        ID
    }
    fn publish(
        &mut self,
        _ctx: &PluginContext,
        source: &[u8],
        _dataset_id: &str,
        _today_jst: &str,
    ) -> Result<(), ContentError> {
        let art: Artwork =
            serde_json::from_slice(source).map_err(|_| ContentError::InvalidSource)?;
        self.set_latest(art)
            .map_err(|_| ContentError::InvalidSource)
    }
    fn position(&self, _ctx: &PluginContext) -> Option<PluginPosition> {
        let art = self
            .temporary
            .get(&_ctx.token.device_id)
            .filter(|(incarnation, _)| *incarnation == _ctx.token.incarnation)
            .map(|(_, artwork)| artwork)
            .or(self.latest.as_ref())?;
        Some(PluginPosition {
            plugin: ID.into(),
            dataset_id: art.id.clone(),
            target_date: art.jst_day().ok()?,
            index: 0,
        })
    }
    fn current_frame(&self, ctx: &PluginContext) -> Option<Vec<u8>> {
        if let Some((_, artwork)) = self
            .temporary
            .get(&ctx.token.device_id)
            .filter(|(incarnation, _)| *incarnation == ctx.token.incarnation)
        {
            return Some(artwork.lcd_rgb565be.clone());
        }
        if self.generation_failed {
            Some(error_lcd())
        } else {
            self.latest
                .as_ref()
                .map(|artwork| artwork.lcd_rgb565be.clone())
                .or_else(|| {
                    Some(crate::lcd_text::status_frame(
                        "CARICATURE",
                        if self.state.jobs.values().any(GenerationJob::recoverable) {
                            "GENERATING"
                        } else {
                            "NO IMAGE"
                        },
                    ))
                })
        }
    }
    fn plan(
        &mut self,
        _ctx: &PluginContext,
        _action: ButtonAction,
    ) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn session_reset(&mut self, device_id: &str, _incarnation: u64) {
        self.temporary.remove(device_id);
    }
    fn delete_device(&mut self, device_id: &str) {
        self.temporary.remove(device_id);
    }
    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("caricature state")
    }
    fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
        if value.is_null() {
            return Ok(());
        } // Legacy placeholder had no state.

        let next: Self = serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        next.settings
            .validate_without_topic()
            .map_err(|_| ContentError::InvalidSource)?;
        if let Some(art) = &next.latest {
            art.validate().map_err(|_| ContentError::InvalidSource)?;
        }
        *self = next;
        Ok(())
    }
}

fn generation_job_request(
    slot: &str,
    job: GenerationJob,
) -> Result<crate::plugin_jobs::JobRequest, ContentError> {
    fn budget(policy: &RetryPolicy) -> u64 {
        if policy.deadline_seconds > 0 {
            return policy.deadline_seconds as u64;
        }
        let attempts = policy.retry_count as u64 + 1;
        attempts
            .saturating_mul(policy.attempt_timeout_seconds as u64)
            .saturating_add((attempts - 1).saturating_mul(policy.interval_seconds as u64))
    }
    let timeout_seconds = 90u64
        .saturating_add(if matches!(job.request.input, GenerationInput::News) {
            budget(&job.request.settings.news_retry)
        } else {
            0
        })
        .saturating_add(budget(&job.request.settings.image_retry));
    let payload = serde_json::to_value(CaricatureWork::Generate { job: Box::new(job) })
        .map_err(|_| ContentError::InvalidSource)?;
    Ok(crate::plugin_jobs::JobRequest {
        slot: slot.into(),
        payload,
        timeout_seconds,
    })
}

/// Distinct terminal error image; publication and immutable history stay intact.
pub fn error_lcd() -> Vec<u8> {
    let mut out = vec![0u8; WIDTH_LCD * HEIGHT_LCD * 2];
    for y in 0..HEIGHT_LCD {
        for x in 0..WIDTH_LCD {
            let red =
                x > 7 && x < 152 && y > 7 && y < 72 && ((x + y) % 11 < 3 || y == 8 || y == 71);
            let color: u16 = if red { 0xf800 } else { 0xffff };
            let i = (y * WIDTH_LCD + x) * 2;
            out[i..i + 2].copy_from_slice(&color.to_be_bytes());
        }
    }
    out
}

pub struct CaricaturePlugin;
impl Plugin for CaricaturePlugin {
    fn id(&self) -> &str {
        ID
    }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool {
        caps.contains("lcd") || caps.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        _query: &'a dyn WorldQuery,
        _event: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

/// Hourly slots are UTC to avoid DST and duplicate local hours.
pub fn hourly_slot(now_unix: i64) -> i64 {
    now_unix.div_euclid(3600)
}

/// A failed attempt never moves this freshness anchor.
pub fn hourly_due(last_success_unix: Option<i64>, now_unix: i64) -> bool {
    last_success_unix.is_none_or(|at| now_unix.saturating_sub(at) >= 3600)
}

/// URL comparison used by the existing implementation: lowercase authority,
/// preserve path/query, strip fragment, and add '/' for an empty path.
pub fn source_url_key(value: &str) -> Result<String, CaricatureError> {
    let (scheme, rest) = value
        .split_once("://")
        .ok_or(CaricatureError::InvalidSource)?;
    if scheme != "http" && scheme != "https" {
        return Err(CaricatureError::InvalidSource);
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..split];
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('\\')
        || value.chars().any(char::is_control)
        || value.contains(' ')
    {
        return Err(CaricatureError::InvalidSource);
    }
    let path = &rest[split..];
    let path = if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_string()
    };
    Ok(format!(
        "{scheme}://{}{path}",
        authority.to_ascii_lowercase()
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedSource {
    pub source_url: String,
    pub published_at_unix: i64,
}

pub fn recent_source_keys<'a>(
    published: impl IntoIterator<Item = &'a PublishedSource>,
    now_unix: i64,
) -> BTreeSet<String> {
    published
        .into_iter()
        .filter(|s| s.published_at_unix > now_unix - 86_400 && s.published_at_unix <= now_unix)
        .filter_map(|s| source_url_key(&s.source_url).ok())
        .collect()
}

pub fn ensure_unused_source(
    source_url: &str,
    recent: &BTreeSet<String>,
) -> Result<(), CaricatureError> {
    let key = source_url_key(source_url)?;
    if recent.contains(&key) {
        Err(CaricatureError::DuplicateSource)
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_canonical_png() -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(kind);
        hasher.update(data);
        out.extend_from_slice(&hasher.finalize().to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&WIDTH_EINK.to_be_bytes());
    header.extend_from_slice(&HEIGHT_EINK.to_be_bytes());
    header.extend_from_slice(&[1, 0, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    for _ in 0..HEIGHT_EINK {
        encoder.write_all(&[0]).unwrap();
        encoder.write_all(&[0xff; 32]).unwrap();
    }
    chunk(&mut out, b"IDAT", &encoder.finish().unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_and_snapshot_are_fixed() {
        let mut settings = GenerationSettings::default();
        let request = GenerationRequest {
            job_id: "job_1".into(),
            slot: 1,
            requested_at_unix: 3600,
            input: GenerationInput::News,
            settings: settings.clone(),
        };
        let job = GenerationJob::accept(request).unwrap();
        settings.style.prompt = "changed".into();
        assert_ne!(job.request.settings.style.prompt, settings.style.prompt);
        assert_eq!(job.request.settings.dpid_lambda, 0.75);
        assert_eq!(job.request.settings.threshold, 128);
    }
    #[test]
    fn rolling_source_window_and_custom_request() {
        let published = [PublishedSource {
            source_url: "https://Example.com/a#fragment".into(),
            published_at_unix: 100_000,
        }];
        let keys = recent_source_keys(&published, 100_000 + 86_399);
        assert_eq!(
            ensure_unused_source("https://example.com/a", &keys),
            Err(CaricatureError::DuplicateSource)
        );
        assert!(recent_source_keys(&published, 100_000 + 86_400).is_empty());
        let request = GenerationRequest {
            job_id: "manual_1".into(),
            slot: -1,
            requested_at_unix: 0,
            input: GenerationInput::CustomText("絵を描く".into()),
            settings: GenerationSettings::default(),
        };
        assert!(GenerationJob::accept(request).is_ok());
    }
    #[test]
    fn generation_and_delivery_are_independent() {
        let request = GenerationRequest {
            job_id: "job1".into(),
            slot: 0,
            requested_at_unix: 0,
            input: GenerationInput::News,
            settings: GenerationSettings::default(),
        };
        let mut job = GenerationJob::accept(request).unwrap();
        assert_eq!(
            job.publish("a".into()),
            Err(CaricatureError::InvalidTransition)
        );
        job.state = GenerationState::Validated;
        job.publish("a".into()).unwrap();
        job.set_delivery(DeliveryState::Failed {
            reason: "timeout".into(),
        })
        .unwrap();
        assert_eq!(
            job.state,
            GenerationState::Published {
                artwork_id: "a".into()
            }
        );
    }
    #[test]
    fn state_applies_only_accepted_result_and_success_drives_hourly_freshness() {
        let request = GenerationRequest {
            job_id: "job_state".into(),
            slot: 0,
            requested_at_unix: 0,
            input: GenerationInput::News,
            settings: GenerationSettings::default(),
        };
        let mut state = CaricatureState::default();
        let mut job = state.accept(request.clone()).unwrap();
        assert!(state.hourly_due(3600));
        assert_eq!(
            state.accept(request),
            Err(CaricatureError::InvalidTransition)
        );
        job.state = GenerationState::Validated;
        state.apply_validated(job.clone(), "art", 10).unwrap();
        assert_eq!(state.latest_artwork_id.as_deref(), Some("art"));
        assert!(!state.hourly_due(3609));
        assert_eq!(
            state.apply_validated(job, "art", 11),
            Err(CaricatureError::Stale)
        );
    }
    #[test]
    fn error_does_not_erase_artwork() {
        let art = Artwork {
            id: "a".into(),
            metadata: ArtworkMetadata {
                job_id: "j".into(),
                slot: 0,
                created_at_unix: 0,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            eink_png: test_canonical_png(),
            lcd_rgb565be: vec![0; WIDTH_LCD * HEIGHT_LCD * 2],
        };
        let mut content = CaricatureContent::default();
        content.set_latest(art).unwrap();
        content.generation_failed();
        assert_eq!(content.latest().unwrap().id, "a");
    }
    #[test]
    fn content_commands_snapshot_settings_and_plan_hourly_work() {
        let mut content = CaricatureContent::default();
        let mut bold = content.settings.style.clone();
        bold.id = "bold".into();
        bold.prompt = "太い線".into();
        content
            .set_catalog(
                vec![content.settings.topic.clone()],
                vec![content.settings.style.clone(), bold.clone()],
            )
            .unwrap();
        let ctx = crate::plugin_jobs::CommandContext {
            now_unix: 7200,
            now_mono_ms: 0,
            today_jst: "1970-01-01".into(),
            settings: crate::settings::PluginSettings::default(),
            devices: vec![],
        };
        content
            .plan_command(&ctx, &serde_json::json!({"kind":"set_style","id":"bold"}))
            .unwrap();
        assert_eq!(content.settings.topic.id, "ai_news");
        assert_eq!(content.settings.style.id, "bold");
        let planned = content.plan_schedule(&ctx).unwrap().unwrap();
        assert_eq!(planned.jobs.len(), 1);
        let CaricatureWork::Generate { job } =
            serde_json::from_value(planned.jobs[0].payload.clone()).unwrap()
        else {
            panic!("wrong work");
        };
        assert_eq!(job.request.settings.style.prompt, "太い線");
        assert_eq!(job.request.slot, 2);
        assert_eq!(planned.jobs[0].slot, "hourly");
        assert!(content.plan_schedule(&ctx).unwrap().is_none());
    }
    #[test]
    fn catalog_prompt_edit_refreshes_selected_snapshot() {
        let mut content = CaricatureContent::default();
        let old_revision = content.settings.revision;
        let mut topic = content.settings.topic.clone();
        topic.prompt = "新しい題材指示".into();
        let mut style = content.settings.style.clone();
        style.prompt = "新しい画風指示".into();
        content
            .set_catalog(vec![topic.clone()], vec![style.clone()])
            .unwrap();
        assert_eq!(content.settings.revision, old_revision + 1);
        assert_eq!(content.settings.topic, topic);
        assert_eq!(content.settings.style, style);
        let ctx = crate::plugin_jobs::CommandContext {
            now_unix: 3600,
            now_mono_ms: 0,
            today_jst: "1970-01-01".into(),
            settings: crate::settings::PluginSettings::default(),
            devices: vec![],
        };
        let planned = content.plan_schedule(&ctx).unwrap().unwrap();
        let CaricatureWork::Generate { job } =
            serde_json::from_value(planned.jobs[0].payload.clone()).unwrap()
        else {
            panic!("wrong work")
        };
        assert_eq!(job.request.settings.topic.prompt, "新しい題材指示");
        assert_eq!(job.request.settings.style.prompt, "新しい画風指示");
    }
    #[test]
    fn hourly_generation_can_pause_durably_without_blocking_one_manual_job() {
        let ctx = crate::plugin_jobs::CommandContext {
            now_unix: 3600,
            now_mono_ms: 0,
            today_jst: "1970-01-01".into(),
            settings: crate::settings::PluginSettings::default(),
            devices: vec![],
        };
        let mut content = CaricatureContent::default();
        let paused = content
            .plan_command(
                &ctx,
                &serde_json::json!({"kind":"set_automatic_generation","enabled":false}),
            )
            .unwrap();
        assert_eq!(paused.cancel_slots, ["hourly"]);
        let mut restored = CaricatureContent::default();
        restored.load_state(content.save_state()).unwrap();
        assert!(restored.plan_schedule(&ctx).unwrap().is_none());
        let mut manual = restored.clone();
        assert_eq!(
            manual
                .plan_command(
                    &ctx,
                    &serde_json::json!({"kind":"generate_custom","text":"A tiny local test idea"})
                )
                .unwrap()
                .jobs
                .len(),
            1
        );
        restored
            .plan_command(
                &ctx,
                &serde_json::json!({"kind":"set_automatic_generation","enabled":true}),
            )
            .unwrap();
        assert_eq!(restored.plan_schedule(&ctx).unwrap().unwrap().jobs.len(), 1);
        let mut old = content.save_state();
        old["settings"]
            .as_object_mut()
            .unwrap()
            .remove("automatic_generation");
        let mut compatible = CaricatureContent::default();
        compatible.load_state(old).unwrap();
        assert!(compatible.settings.automatic_generation);
    }
    #[test]
    fn custom_generation_ignores_missing_topic_and_history_send_keeps_latest() {
        use crate::plugin_jobs::{JobOutcome, JobToken, PluginDevice, PluginJob, RefreshTargets};
        let mut content = CaricatureContent::default();
        let mut other_topic = content.settings.topic.clone();
        other_topic.id = "other".into();
        content
            .set_catalog(vec![other_topic], vec![content.settings.style.clone()])
            .unwrap();
        let device = PluginContext {
            token: crate::SessionToken {
                device_id: "pico".into(),
                incarnation: 1,
                session_epoch: 1,
            },
            settings_revision: 1,
            mode: crate::Mode::Normal,
        };
        let ctx = crate::plugin_jobs::CommandContext {
            now_unix: 0,
            now_mono_ms: 0,
            today_jst: "1970-01-01".into(),
            settings: crate::settings::PluginSettings::default(),
            devices: vec![PluginDevice {
                context: device.clone(),
                selected: Some(ID.into()),
                online: true,
                capabilities: BTreeSet::from(["lcd".into()]),
            }],
        };
        assert!(content
            .plan_command(&ctx, &serde_json::json!({"kind":"generate_news"}))
            .is_err());
        let request = content
            .plan_command(
                &ctx,
                &serde_json::json!({"kind":"generate_custom","text":"月面の喫茶店"}),
            )
            .unwrap()
            .jobs
            .remove(0);
        let CaricatureWork::Generate { mut job } =
            serde_json::from_value(request.payload.clone()).unwrap()
        else {
            panic!("wrong work");
        };
        assert_eq!(job.news_attempts, 0);
        job.state = GenerationState::Validated;
        let art = Artwork {
            id: "saved".into(),
            metadata: ArtworkMetadata {
                job_id: job.request.job_id.clone(),
                slot: job.request.slot,
                created_at_unix: 0,
                input: job.request.input.clone(),
                source_urls: vec![],
                news_summary: None,
                settings: job.request.settings.clone(),
            },
            eink_png: test_canonical_png(),
            lcd_rgb565be: vec![1; WIDTH_LCD * HEIGHT_LCD * 2],
        };
        let pubn = store::Publication {
            artwork: art.clone(),
            publish_seq: 1,
            published_at_unix: 1,
        };
        let common_job = PluginJob {
            token: JobToken {
                plugin: ID.into(),
                slot: "manual".into(),
                generation: 1,
            },
            payload: request.payload,
            timeout_seconds: request.timeout_seconds,
            created_unix: 0,
        };
        let outcome = JobOutcome::Succeeded(
            serde_json::to_value(CaricatureSuccess::Published {
                job,
                publication: Box::new(pubn),
            })
            .unwrap(),
        );
        let plan = content.work_completed(&ctx, &common_job, &outcome).unwrap();
        assert!(matches!(plan.refresh, RefreshTargets::Selected));
        assert_eq!(content.current_frame(&device).unwrap(), art.lcd_rgb565be);
        let send = content
            .plan_command(
                &ctx,
                &serde_json::json!({"kind":"show_history","artwork_id":"saved","device_id":"pico"}),
            )
            .unwrap();
        let history_job = PluginJob {
            token: JobToken {
                plugin: ID.into(),
                slot: "history".into(),
                generation: 2,
            },
            payload: send.jobs[0].payload.clone(),
            timeout_seconds: 30,
            created_unix: 0,
        };
        let mut other = art.clone();
        other.id = "past".into();
        other.lcd_rgb565be[0] = 2;
        let shown = JobOutcome::Succeeded(
            serde_json::to_value(CaricatureSuccess::HistoryFrame {
                artwork: Box::new(other.clone()),
                device_id: "pico".into(),
                incarnation: 1,
            })
            .unwrap(),
        );
        let plan = content.work_completed(&ctx, &history_job, &shown).unwrap();
        assert!(matches!(plan.refresh, RefreshTargets::Devices(_)));
        assert_eq!(content.current_frame(&device).unwrap(), other.lcd_rgb565be);
        assert_eq!(content.latest().unwrap().id, "saved");
        content.generation_failed();
        assert_eq!(content.current_frame(&device).unwrap(), other.lcd_rgb565be);
        content.session_reset("pico", 1);
        assert_eq!(content.current_frame(&device).unwrap(), error_lcd());
    }
}
