//! Durable job transitions and side-effect contracts shared by plugins.
//! Plugin payloads and public content remain opaque to this module.

use std::collections::BTreeMap;
use std::path::PathBuf;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::content::PluginContext;
use crate::settings::PluginSettings;
use crate::DeviceToken;

#[derive(Clone, Debug)]
pub struct CommandContext {
    pub now_unix: i64,
    pub now_mono_ms: u64,
    pub today_jst: String,
    pub settings: PluginSettings,
    pub devices: Vec<PluginDevice>,
}

#[derive(Clone, Debug)]
pub struct PluginDevice {
    pub context: PluginContext,
    pub selected: Option<String>,
    pub online: bool,
    pub capabilities: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
pub enum RefreshTargets {
    #[default]
    None,
    /// The plugin explicitly chooses all of its currently selected devices.
    Selected,
    Devices(Vec<DeviceToken>),
}

#[derive(Clone, Debug, Default)]
pub struct PluginWorkPlan {
    pub jobs: Vec<JobRequest>,
    pub cancel_slots: Vec<String>,
    pub refresh: RefreshTargets,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRequest {
    pub slot: String,
    pub payload: Value,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JobToken {
    pub plugin: String,
    pub slot: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginJob {
    pub token: JobToken,
    /// Fixed job-start settings/inputs; credentials must never enter this value.
    pub payload: Value,
    pub timeout_seconds: u64,
    pub created_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFailure {
    pub code: String,
    /// Only an explicitly public/sanitized description, never process stderr.
    pub message: String,
}

impl JobFailure {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn unsupported() -> Self {
        Self::new("unsupported", "This operation is not supported")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    Succeeded(Value),
    Failed(JobFailure),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProgress {
    pub percent: Option<u8>,
    pub message: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Clone, Default)]
pub struct JobProgressSink {
    channel: Option<(JobToken, mpsc::Sender<(JobToken, JobProgress)>)>,
}

impl JobProgressSink {
    pub(crate) fn new(token: JobToken, sender: mpsc::Sender<(JobToken, JobProgress)>) -> Self {
        Self {
            channel: Some((token, sender)),
        }
    }
    /// Progress is disposable; a slow observer cannot block an external job.
    pub fn report(&self, percent: Option<u8>, message: &str) -> bool {
        self.report_data(percent, message, Value::Null)
    }
    /// Critical cache metadata must reach the ledger even if cancellation
    /// follows completion. Workers drain queued progress before result apply.
    pub async fn report_data_wait(&self, percent: Option<u8>, message: &str, data: Value) -> bool {
        if percent.is_some_and(|p| p > 100) || message.len() > 512 {
            return false;
        }
        if let Some((token, sender)) = &self.channel {
            sender
                .send((
                    token.clone(),
                    JobProgress {
                        percent,
                        message: message.into(),
                        data,
                    },
                ))
                .await
                .is_ok()
        } else {
            false
        }
    }
    pub fn report_data(&self, percent: Option<u8>, message: &str, data: Value) -> bool {
        if percent.is_some_and(|p| p > 100) || message.len() > 512 {
            return false;
        }
        self.channel.as_ref().is_some_and(|(token, sender)| {
            sender
                .try_send((
                    token.clone(),
                    JobProgress {
                        percent,
                        message: message.into(),
                        data,
                    },
                ))
                .is_ok()
        })
    }
}

#[derive(Clone, Debug)]
pub struct PluginPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
}

pub struct JobExecutionContext {
    pub token: JobToken,
    pub paths: PluginPaths,
    pub cancel: watch::Receiver<bool>,
    pub progress: JobProgressSink,
}

impl JobExecutionContext {
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    pub async fn cancelled(&mut self) {
        while !*self.cancel.borrow() {
            if self.cancel.changed().await.is_err() {
                return;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackSpec {
    pub asset_id: String,
    pub generation: u64,
    pub fps: u32,
    pub frame_count: u64,
    pub started_mono_ms: u64,
    /// Plugin-owned immutable frame source metadata; never sent directly to the Web.
    pub source: Value,
}

impl PlaybackSpec {
    pub fn valid(&self) -> bool {
        !self.asset_id.is_empty()
            && self.asset_id.len() <= 256
            && self.generation > 0
            && self.fps > 0
            && self.fps <= 30
            && self.frame_count > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetArea {
    Data,
    Cache,
}

#[derive(Clone, Debug)]
pub struct AssetDescriptor {
    pub area: AssetArea,
    pub relative_path: PathBuf,
    pub content_type: String,
}

/// External work may await or use spawn_blocking, and is never called with an
/// App mutex held. Cancellation must stop/wait children before returning;
/// dropping a future must not leave a child process running.
pub trait PluginJobExecutor: Send + Sync {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome>;
    fn read_frame(
        &self,
        _spec: PlaybackSpec,
        _index: u64,
        _paths: PluginPaths,
    ) -> BoxFuture<'static, Result<Vec<u8>, JobFailure>> {
        Box::pin(async { Err(JobFailure::unsupported()) })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn running(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
    pub fn finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub job: PluginJob,
    pub status: JobStatus,
    pub progress: Option<JobProgress>,
    pub failure: Option<JobFailure>,
    pub finished_unix: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    pub next_generation: u64,
    pub jobs: BTreeMap<u64, JobRecord>,
    pub slots: BTreeMap<String, u64>,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            next_generation: 1,
            jobs: BTreeMap::new(),
            slots: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum JobEvent {
    Submit {
        plugin: String,
        request: JobRequest,
        now_unix: i64,
    },
    Cancel {
        plugin: String,
        slot: String,
        now_unix: i64,
    },
    Started(JobToken),
    Progress {
        token: JobToken,
        progress: JobProgress,
    },
    Finished {
        token: JobToken,
        outcome: JobOutcome,
        now_unix: i64,
    },
}

#[derive(Clone, Debug)]
pub enum JobEffect {
    Run(PluginJob),
    Cancel(JobToken),
}

pub struct JobDecision {
    pub state: JobState,
    pub effects: Vec<JobEffect>,
    pub completion: Option<(PluginJob, JobOutcome)>,
    pub applied: bool,
}

fn slot_key(plugin: &str, slot: &str) -> String {
    format!("{plugin}/{slot}")
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl JobState {
    pub fn current(&self, token: &JobToken) -> bool {
        self.slots.get(&slot_key(&token.plugin, &token.slot)) == Some(&token.generation)
            && self
                .jobs
                .get(&token.generation)
                .is_some_and(|r| r.job.token == *token)
    }

    pub fn ready(&self) -> Vec<PluginJob> {
        self.jobs
            .values()
            .filter(|record| {
                record.status == JobStatus::Queued
                    && self.current(&record.job.token)
                    && !self.jobs.values().any(|other| {
                        other.status.running()
                            && other.job.token.plugin == record.job.token.plugin
                            && other.job.token.slot == record.job.token.slot
                    })
            })
            .map(|r| r.job.clone())
            .collect()
    }

    pub fn recover(&mut self) -> Result<(), JobFailure> {
        if self.next_generation == 0
            || self.jobs.iter().any(|(id, record)| {
                *id == 0
                    || *id >= self.next_generation
                    || *id != record.job.token.generation
                    || !valid_name(&record.job.token.plugin)
                    || !valid_name(&record.job.token.slot)
            })
        {
            return Err(JobFailure::new("invalid_state", "Invalid stored job state"));
        }
        for record in self.jobs.values_mut() {
            match record.status {
                JobStatus::Running => record.status = JobStatus::Queued,
                JobStatus::Cancelling => record.status = JobStatus::Cancelled,
                _ => {}
            }
        }
        Ok(())
    }

    /// Rule and Transition: calculate a new state and explicit work effects.
    /// Running predecessors stay Cancelling until their executor has exited.
    pub fn handle(&self, event: JobEvent) -> Result<JobDecision, JobFailure> {
        let mut next = self.clone();
        let mut effects = Vec::new();
        let mut completion = None;
        match event {
            JobEvent::Submit {
                plugin,
                request,
                now_unix,
            } => {
                if !valid_name(&plugin)
                    || !valid_name(&request.slot)
                    || request.timeout_seconds == 0
                {
                    return Err(JobFailure::new("invalid_job", "Invalid job request"));
                }
                let key = slot_key(&plugin, &request.slot);
                if let Some(old) = next.slots.get(&key).and_then(|id| next.jobs.get_mut(id)) {
                    if old.status == JobStatus::Running {
                        old.status = JobStatus::Cancelling;
                        effects.push(JobEffect::Cancel(old.job.token.clone()));
                    } else if old.status == JobStatus::Queued {
                        old.status = JobStatus::Cancelled;
                        old.finished_unix = Some(now_unix);
                    }
                }
                let generation = next.next_generation;
                next.next_generation = generation
                    .checked_add(1)
                    .ok_or_else(|| JobFailure::new("exhausted", "Job generation exhausted"))?;
                let job = PluginJob {
                    token: JobToken {
                        plugin,
                        slot: request.slot,
                        generation,
                    },
                    payload: request.payload,
                    timeout_seconds: request.timeout_seconds,
                    created_unix: now_unix,
                };
                next.slots.insert(key, generation);
                next.jobs.insert(
                    generation,
                    JobRecord {
                        job,
                        status: JobStatus::Queued,
                        progress: None,
                        failure: None,
                        finished_unix: None,
                    },
                );
                // Retain current slot results and every unfinished executor.
                let latest: std::collections::BTreeSet<_> = next.slots.values().copied().collect();
                next.jobs
                    .retain(|id, record| latest.contains(id) || !record.status.finished());
            }
            JobEvent::Cancel {
                plugin,
                slot,
                now_unix,
            } => {
                if let Some(record) = next
                    .slots
                    .get(&slot_key(&plugin, &slot))
                    .and_then(|id| next.jobs.get_mut(id))
                {
                    if record.status == JobStatus::Running {
                        record.status = JobStatus::Cancelling;
                        effects.push(JobEffect::Cancel(record.job.token.clone()));
                    } else if record.status == JobStatus::Queued {
                        record.status = JobStatus::Cancelled;
                        record.finished_unix = Some(now_unix);
                        completion = Some((record.job.clone(), JobOutcome::Cancelled));
                    }
                }
            }
            JobEvent::Started(token) => {
                if next.ready().iter().any(|job| job.token == token) {
                    let record = next.jobs.get_mut(&token.generation).expect("ready job");
                    record.status = JobStatus::Running;
                    effects.push(JobEffect::Run(record.job.clone()));
                }
            }
            JobEvent::Progress { token, progress } => {
                if progress.percent.is_none_or(|p| p <= 100) && progress.message.len() <= 512 {
                    if let Some(record) = next.jobs.get_mut(&token.generation) {
                        if record.job.token == token && record.status.running() {
                            record.progress = Some(progress);
                        }
                    }
                }
            }
            JobEvent::Finished {
                token,
                outcome,
                now_unix,
            } => {
                let current = next.current(&token);
                if let Some(record) = next.jobs.get_mut(&token.generation) {
                    if record.job.token == token && record.status.running() {
                        let outcome = if record.status == JobStatus::Cancelling {
                            JobOutcome::Cancelled
                        } else {
                            outcome
                        };
                        record.status = match &outcome {
                            JobOutcome::Succeeded(_) => JobStatus::Succeeded,
                            JobOutcome::Failed(failure) => {
                                record.failure = Some(failure.clone());
                                JobStatus::Failed
                            }
                            JobOutcome::Cancelled => JobStatus::Cancelled,
                        };
                        record.finished_unix = Some(now_unix);
                        if current {
                            completion = Some((record.job.clone(), outcome));
                        }
                    }
                }
            }
        }
        let applied = next != *self;
        Ok(JobDecision {
            state: next,
            effects,
            completion,
            applied,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct JobView {
    pub token: JobToken,
    pub status: JobStatus,
    pub progress: Option<JobProgress>,
    pub failure: Option<JobFailure>,
    pub created_unix: i64,
    pub finished_unix: Option<i64>,
}

impl JobState {
    pub fn view(&self, plugin: &str) -> Vec<JobView> {
        self.jobs
            .values()
            .filter(|r| r.job.token.plugin == plugin && self.current(&r.job.token))
            .map(|r| JobView {
                token: r.job.token.clone(),
                status: r.status,
                progress: r.progress.clone(),
                failure: r.failure.clone(),
                created_unix: r.job.created_unix,
                finished_unix: r.finished_unix,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn submit(state: JobState, plugin: &str, slot: &str) -> JobState {
        state
            .handle(JobEvent::Submit {
                plugin: plugin.into(),
                request: JobRequest {
                    slot: slot.into(),
                    payload: serde_json::json!({"fixed_config":1}),
                    timeout_seconds: 30,
                },
                now_unix: 1,
            })
            .unwrap()
            .state
    }
    #[test]
    fn cancellation_waits_for_cleanup_and_old_result_cannot_publish() {
        let state = submit(JobState::default(), "video", "prepare");
        let old = state.ready()[0].token.clone();
        let state = state.handle(JobEvent::Started(old.clone())).unwrap().state;
        let state = submit(state, "video", "prepare");
        assert!(state.ready().is_empty());
        let finished = state
            .handle(JobEvent::Finished {
                token: old.clone(),
                outcome: JobOutcome::Succeeded(serde_json::json!({"old":true})),
                now_unix: 2,
            })
            .unwrap();
        assert!(finished.completion.is_none());
        assert_eq!(
            finished.state.jobs[&old.generation].status,
            JobStatus::Cancelled
        );
        assert_eq!(finished.state.ready().len(), 1);
        assert!(
            !finished
                .state
                .handle(JobEvent::Finished {
                    token: old,
                    outcome: JobOutcome::Cancelled,
                    now_unix: 3
                })
                .unwrap()
                .applied
        );
    }
    #[test]
    fn independent_slots_and_plugins_are_ready_and_restart_preserves_generation() {
        let state = submit(JobState::default(), "image", "hourly");
        let state = submit(state, "image", "manual");
        let state = submit(state, "video", "prepare");
        assert_eq!(state.ready().len(), 3);
        let token = state.ready()[0].token.clone();
        let state = state
            .handle(JobEvent::Started(token.clone()))
            .unwrap()
            .state;
        let mut restored: JobState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        restored.recover().unwrap();
        assert_eq!(restored.ready().len(), 3);
        assert_eq!(restored.next_generation, state.next_generation);
        assert_eq!(restored.jobs[&token.generation].job.token, token);
    }
}
