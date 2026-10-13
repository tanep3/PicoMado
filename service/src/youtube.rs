//! YouTube's shared state, rules, and effect contracts.
//!
//! The application applies these transitions while holding its state lock. It
//! runs `run_prepare`, `run_delete`, `read_frame`, and `scan_usage` outside it.
//! A worker result is accepted only for the current job generation.

mod media;
pub use media::{
    read_frame, rgb565_from_jpeg, run_cached_prepare, run_delete, run_prepare, scan_usage,
    scan_usage_or_last, validate_conversion, validate_conversion_cancellable, ColorTransform,
    MediaTools, Usage,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::plugin_jobs::PlaybackSpec;
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};

pub const WIDTH: u32 = 160;
pub const HEIGHT: u32 = 80;
pub const FRAME_BYTES: usize = (WIDTH * HEIGHT * 2) as usize;
pub const FPS_CHOICES: [u8; 5] = [6, 12, 18, 24, 30];
pub const CONVERSION_VERSION: &str = "fit-black-jpeg-q5-rgb565be-v1";
pub const MAX_JPEG_BYTES: u64 = 65_536;

/// Kernel registration; YouTube has no local button action at present.
pub struct YoutubePlugin;
impl Plugin for YoutubePlugin {
    fn id(&self) -> &str {
        "youtube"
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Fps(u8);
impl<'de> Deserialize<'de> for Fps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Fps::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl Default for Fps {
    fn default() -> Self {
        Self(6)
    }
}
impl Fps {
    pub fn new(value: u8) -> Result<Self, YoutubeError> {
        if FPS_CHOICES.contains(&value) {
            Ok(Self(value))
        } else {
            Err(YoutubeError::InvalidFps)
        }
    }
    pub fn value(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoUrl {
    pub id: String,
    pub canonical: String,
}
impl VideoUrl {
    pub fn parse(input: &str) -> Result<Self, YoutubeError> {
        if input.len() > 2048 {
            return Err(YoutubeError::InvalidUrl);
        }
        // URL parsers normalize an explicit default port away. Keep the old
        // validation rule by checking the raw authority as well.
        let authority = input
            .trim()
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or("")
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("");
        if authority.contains(':') {
            return Err(YoutubeError::InvalidUrl);
        }
        let url = reqwest::Url::parse(input.trim()).map_err(|_| YoutubeError::InvalidUrl)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err(YoutubeError::InvalidUrl);
        }
        let id = match (
            url.host_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            url.path(),
        ) {
            ("youtu.be", path)
                if path.trim_matches('/').len() == 11 && path.matches('/').count() <= 2 =>
            {
                path.trim_matches('/').to_owned()
            }
            ("youtube.com" | "www.youtube.com" | "m.youtube.com", "/watch") => {
                let mut values = url.query_pairs().filter(|(k, _)| k == "v");
                let value = values
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                if values.next().is_some() {
                    return Err(YoutubeError::InvalidUrl);
                }
                value
            }
            ("youtube.com" | "www.youtube.com" | "m.youtube.com", path) => {
                let parts: Vec<_> = path.trim_matches('/').split('/').collect();
                if parts.len() == 2 && matches!(parts[0], "shorts" | "embed") {
                    parts[1].to_owned()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        if !valid_id(&id) {
            return Err(YoutubeError::InvalidUrl);
        }
        Ok(Self {
            canonical: format!("https://www.youtube.com/watch?v={id}"),
            id,
        })
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn valid_key(key: &str) -> bool {
    key.len() == 24 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Milliseconds preserve the old three-decimal-second input precision.
pub fn parse_time(value: Option<&str>) -> Result<Option<u64>, YoutubeError> {
    let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let fields: Vec<&str> = value.split(':').collect();
    if fields.len() > 3 || fields.is_empty() || fields.iter().any(|s| s.is_empty()) {
        return Err(YoutubeError::InvalidTime);
    }
    if fields.len() > 1
        && fields[1..]
            .iter()
            .any(|s| s.parse::<f64>().map_or(true, |n| n >= 60.0))
    {
        return Err(YoutubeError::InvalidTime);
    }
    let mut seconds = 0.0;
    for field in fields {
        if !field.bytes().all(|b| b.is_ascii_digit() || b == b'.') || field.matches('.').count() > 1
        {
            return Err(YoutubeError::InvalidTime);
        }
        let number: f64 = field.parse().map_err(|_| YoutubeError::InvalidTime)?;
        seconds = seconds * 60.0 + number;
    }
    if !seconds.is_finite() || seconds < 0.0 || seconds > (u64::MAX as f64 / 1000.0) {
        return Err(YoutubeError::InvalidTime);
    }
    Ok(Some((seconds * 1000.0).round() as u64))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub start_ms: u64,
    pub end_ms: u64,
}
impl Clip {
    pub fn resolve(
        start: Option<u64>,
        end: Option<u64>,
        duration_ms: u64,
    ) -> Result<Self, YoutubeError> {
        let clip = Self {
            start_ms: start.unwrap_or(0),
            end_ms: end.unwrap_or(duration_ms),
        };
        if clip.start_ms >= clip.end_ms || clip.end_ms > duration_ms {
            Err(YoutubeError::InvalidClip)
        } else {
            Ok(clip)
        }
    }
    pub fn is_full(self, duration_ms: u64) -> bool {
        self.start_ms == 0 && self.end_ms == duration_ms
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub id: String,
    pub url: String,
    pub title: String,
    pub channel: String,
    pub uploader: String,
    pub duration_ms: u64,
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub thumbnail_bytes: u64,
    pub updated_unix_ms: i64,
    pub played_unix_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceCache {
    pub key: String,
    pub video_id: String,
    pub coverage: Clip,
    pub full: bool,
    pub extension: String,
    pub bytes: u64,
    pub origin_ms: u64,
    pub duration_ms: u64,
}
impl SourceCache {
    fn covers(&self, clip: Clip, metadata_duration: u64) -> bool {
        self.coverage.start_ms <= clip.start_ms
            && (self.full
                || self.coverage.end_ms >= clip.end_ms
                || (clip.end_ms == metadata_duration
                    && self.coverage.end_ms.saturating_add(1000) >= clip.end_ms
                    && self.key
                        == source_key(
                            &self.video_id,
                            self.coverage.start_ms,
                            metadata_duration,
                            false,
                        )))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionCache {
    pub key: String,
    pub video_id: String,
    pub clip: Clip,
    pub fps: Fps,
    pub frames: u64,
    pub bytes: u64,
    pub created_unix_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoRecord {
    pub metadata: VideoMetadata,
    pub sources: BTreeMap<String, SourceCache>,
    pub conversions: BTreeMap<String, ConversionCache>,
}
impl VideoRecord {
    pub fn bytes(&self) -> u64 {
        self.sources.values().map(|s| s.bytes).sum::<u64>()
            + self.conversions.values().map(|c| c.bytes).sum::<u64>()
            + self.metadata.thumbnail_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStage {
    Metadata,
    Download,
    Convert,
    Validate,
    Ready,
    Cancelled,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Working,
    Cancelling,
    Ready,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobState {
    pub generation: u64,
    pub video_id: String,
    pub status: JobStatus,
    pub stage: JobStage,
    pub progress: u8,
    pub message: String,
    pub reused: bool,
    pub started_unix_ms: i64,
    pub finished_unix_ms: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct Cancellation(Arc<AtomicBool>);
impl Default for Cancellation {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<(), YoutubeError> {
        if self.is_cancelled() {
            Err(YoutubeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrepareRequest {
    pub url: String,
    pub fps: Option<u8>,
    pub start: Option<String>,
    pub end: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PrepareJob {
    pub generation: u64,
    pub url: VideoUrl,
    pub fps: Fps,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub previous: Option<VideoRecord>,
    pub cancel: Cancellation,
}
#[derive(Clone, Debug)]
pub struct PrepareResult {
    pub metadata: VideoMetadata,
    pub source: Option<SourceCache>,
    pub conversion: ConversionCache,
    pub reused: bool,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub generation: u64,
    pub stage: JobStage,
    pub percent: u8,
    pub message: String,
    pub metadata: Option<VideoMetadata>,
    pub source: Option<SourceCache>,
    /// Sent only after the validated directory has been atomically published.
    pub conversion: Option<ConversionCache>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivePlayback {
    pub conversion: ConversionCache,
    pub started_mono_ms: u64,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameDue {
    pub key: String,
    pub video_id: String,
    pub index: u64,
    pub sequence: u64,
    pub epoch: u64,
}

/// The kernel supplies its current per-device selection and display ability.
/// An E-ink viewer can advertise a low `max_fps`; zero means unsupported.
#[derive(Clone, Debug)]
pub struct Viewer<'a> {
    pub device_id: &'a str,
    pub selected_youtube: bool,
    pub online: bool,
    pub max_fps: u8,
}
#[derive(Clone, Debug)]
pub struct Delivery {
    pub device_id: String,
    pub frame: FrameDue,
}
#[derive(Clone, Copy, Debug)]
struct ViewerCursor {
    epoch: u64,
    sequence: u64,
    queued_mono_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredYoutube {
    pub default_fps: Fps,
    pub videos: BTreeMap<String, VideoRecord>,
    pub selected_key: Option<String>,
    pub next_generation: u64,
    pub playback_epoch: u64,
}

#[derive(Clone, Debug, Default)]
pub struct YoutubeContent {
    pub default_fps: Fps,
    pub videos: BTreeMap<String, VideoRecord>,
    pub playback: Option<ActivePlayback>,
    pub job: Option<JobState>,
    pub deleting: BTreeSet<String>,
    next_generation: u64,
    playback_epoch: u64,
    cancellation: Option<Cancellation>,
    viewers: BTreeMap<String, ViewerCursor>,
}
impl YoutubeContent {
    pub fn save_state(&self) -> StoredYoutube {
        StoredYoutube {
            default_fps: self.default_fps,
            videos: self.videos.clone(),
            selected_key: self.playback.as_ref().map(|p| p.conversion.key.clone()),
            next_generation: self.next_generation,
            playback_epoch: self.playback_epoch,
        }
    }
    /// Resume the selected completed cache at its beginning on service restart.
    /// `validate_conversion` must confirm the selected file before delivery.
    pub fn restore(state: StoredYoutube, now_mono_ms: u64) -> Self {
        let mut content = Self {
            default_fps: Fps::new(state.default_fps.value()).unwrap_or_default(),
            videos: state.videos,
            next_generation: state.next_generation,
            playback_epoch: state.playback_epoch,
            ..Self::default()
        };
        if let Some(key) = state.selected_key {
            if let Some(conversion) = content
                .videos
                .values()
                .find_map(|v| v.conversions.get(&key))
            {
                content.playback_epoch = content.playback_epoch.saturating_add(1);
                content.playback = Some(ActivePlayback {
                    conversion: conversion.clone(),
                    started_mono_ms: now_mono_ms,
                    epoch: content.playback_epoch,
                });
            }
        }
        content
    }
    /// Validate the persisted selection on a blocking worker before putting
    /// it back on screen. A missing or incomplete cache leaves history intact.
    pub async fn restore_checked(
        state: StoredYoutube,
        root: std::path::PathBuf,
        now_mono_ms: u64,
    ) -> Result<Self, YoutubeError> {
        let mut content = Self::restore(state, now_mono_ms);
        if let Some(playback) = &content.playback {
            let item = playback.conversion.clone();
            let valid = tokio::task::spawn_blocking(move || validate_conversion(&root, &item))
                .await
                .map_err(|e| YoutubeError::Storage(e.to_string()))??;
            if !valid {
                content.stop();
            }
        }
        Ok(content)
    }
    pub fn set_default_fps(&mut self, fps: u8) -> Result<(), YoutubeError> {
        self.default_fps = Fps::new(fps)?;
        Ok(())
    }
    pub fn request_prepare(
        &mut self,
        input: PrepareRequest,
        now_unix_ms: i64,
    ) -> Result<PrepareJob, YoutubeError> {
        if self
            .job
            .as_ref()
            .is_some_and(|j| matches!(j.status, JobStatus::Working | JobStatus::Cancelling))
        {
            return Err(YoutubeError::Busy);
        }
        let url = VideoUrl::parse(&input.url)?;
        if self.deleting.contains(&url.id) {
            return Err(YoutubeError::Busy);
        }
        let fps = Fps::new(input.fps.unwrap_or(self.default_fps.value()))?;
        let start_ms = parse_time(input.start.as_deref())?;
        let end_ms = parse_time(input.end.as_deref())?;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(YoutubeError::Overflow)?;
        let cancel = Cancellation::default();
        self.cancellation = Some(cancel.clone());
        self.job = Some(JobState {
            generation: self.next_generation,
            video_id: url.id.clone(),
            status: JobStatus::Working,
            stage: JobStage::Metadata,
            progress: 0,
            message: "動画情報を確認しています".into(),
            reused: false,
            started_unix_ms: now_unix_ms,
            finished_unix_ms: None,
        });
        Ok(PrepareJob {
            generation: self.next_generation,
            previous: self.videos.get(&url.id).cloned(),
            url,
            fps,
            start_ms,
            end_ms,
            cancel,
        })
    }
    pub fn request_play(
        &mut self,
        key: &str,
        now_unix_ms: i64,
    ) -> Result<PrepareJob, YoutubeError> {
        let record = self
            .videos
            .values()
            .find(|v| v.conversions.contains_key(key))
            .ok_or(YoutubeError::MissingCache)?;
        let conversion = record
            .conversions
            .get(key)
            .ok_or(YoutubeError::MissingCache)?;
        let input = PrepareRequest {
            url: record.metadata.url.clone(),
            fps: Some(conversion.fps.value()),
            start: Some(format_ms(conversion.clip.start_ms)),
            end: Some(format_ms(conversion.clip.end_ms)),
        };
        self.request_prepare(input, now_unix_ms)
    }
    pub fn request_fps(&mut self, fps: u8, now_unix_ms: i64) -> Result<PrepareJob, YoutubeError> {
        let current = self.playback.as_ref().ok_or(YoutubeError::NoPlayback)?;
        let record = self
            .videos
            .get(&current.conversion.video_id)
            .ok_or(YoutubeError::MissingCache)?;
        let input = PrepareRequest {
            url: record.metadata.url.clone(),
            fps: Some(fps),
            start: Some(format_ms(current.conversion.clip.start_ms)),
            end: Some(format_ms(current.conversion.clip.end_ms)),
        };
        self.request_prepare(input, now_unix_ms)
    }
    pub fn progress(&mut self, update: Progress) -> bool {
        let Some(job) = &mut self.job else {
            return false;
        };
        if job.generation != update.generation
            || !matches!(job.status, JobStatus::Working | JobStatus::Cancelling)
        {
            return false;
        }
        if update
            .metadata
            .as_ref()
            .is_some_and(|m| m.id != job.video_id)
            || update
                .source
                .as_ref()
                .is_some_and(|s| s.video_id != job.video_id)
            || update
                .conversion
                .as_ref()
                .is_some_and(|c| c.video_id != job.video_id)
        {
            return false;
        }
        if let Some(metadata) = update.metadata {
            let record = self
                .videos
                .entry(metadata.id.clone())
                .or_insert_with(|| VideoRecord {
                    metadata: metadata.clone(),
                    sources: BTreeMap::new(),
                    conversions: BTreeMap::new(),
                });
            record.metadata = metadata;
        }
        if let Some(source) = update.source {
            if let Some(record) = self.videos.get_mut(&source.video_id) {
                record.sources.insert(source.key.clone(), source);
            }
        }
        if let Some(conversion) = update.conversion {
            if let Some(record) = self.videos.get_mut(&conversion.video_id) {
                record
                    .conversions
                    .insert(conversion.key.clone(), conversion);
            }
        }
        if job.status == JobStatus::Working {
            job.stage = update.stage;
            job.progress = update.percent.min(99);
            job.message = update.message;
        }
        true
    }
    pub fn cancel(&mut self) -> bool {
        if let Some(job) = &mut self.job {
            if job.status == JobStatus::Working {
                job.status = JobStatus::Cancelling;
                job.message = "処理を取り消しています".into();
                if let Some(token) = &self.cancellation {
                    token.cancel();
                }
                return true;
            }
        }
        false
    }
    pub fn complete_prepare(
        &mut self,
        generation: u64,
        result: Result<PrepareResult, YoutubeError>,
        now_mono_ms: u64,
        now_unix_ms: i64,
    ) -> bool {
        let Some(job) = &mut self.job else {
            return false;
        };
        if job.generation != generation
            || !matches!(job.status, JobStatus::Working | JobStatus::Cancelling)
        {
            return false;
        }
        let cancelled = job.status == JobStatus::Cancelling
            || self
                .cancellation
                .as_ref()
                .is_some_and(Cancellation::is_cancelled);
        if cancelled {
            // Publication is independent of playback. A completed result may
            // arrive after Cancel was requested; retain it for later reuse.
            if let Ok(ready) = result {
                let record = self
                    .videos
                    .entry(ready.metadata.id.clone())
                    .or_insert_with(|| VideoRecord {
                        metadata: ready.metadata.clone(),
                        sources: BTreeMap::new(),
                        conversions: BTreeMap::new(),
                    });
                record.metadata = ready.metadata;
                if let Some(source) = ready.source {
                    record.sources.insert(source.key.clone(), source);
                }
                record
                    .conversions
                    .insert(ready.conversion.key.clone(), ready.conversion);
            }
            job.status = JobStatus::Cancelled;
            job.stage = JobStage::Cancelled;
            job.message = "取り消しました。現在の再生は継続します".into();
        } else {
            match result {
                Ok(mut ready) => {
                    ready.metadata.played_unix_ms = Some(now_unix_ms);
                    let record =
                        self.videos
                            .entry(ready.metadata.id.clone())
                            .or_insert_with(|| VideoRecord {
                                metadata: ready.metadata.clone(),
                                sources: BTreeMap::new(),
                                conversions: BTreeMap::new(),
                            });
                    record.metadata = ready.metadata;
                    if let Some(source) = ready.source {
                        record.sources.insert(source.key.clone(), source);
                    }
                    record
                        .conversions
                        .insert(ready.conversion.key.clone(), ready.conversion.clone());
                    self.playback_epoch = self.playback_epoch.saturating_add(1);
                    self.playback = Some(ActivePlayback {
                        conversion: ready.conversion,
                        started_mono_ms: now_mono_ms,
                        epoch: self.playback_epoch,
                    });
                    job.status = JobStatus::Ready;
                    job.stage = JobStage::Ready;
                    job.progress = 100;
                    job.reused = ready.reused;
                    job.message = if ready.reused {
                        "キャッシュから再生しました"
                    } else {
                        "変換完了・先頭から再生します"
                    }
                    .into();
                }
                Err(YoutubeError::Cancelled) => {
                    job.status = JobStatus::Cancelled;
                    job.stage = JobStage::Cancelled;
                    job.message = "取り消しました。現在の再生は継続します".into();
                }
                Err(error) => {
                    job.status = JobStatus::Failed;
                    job.stage = JobStage::Failed;
                    job.message = error.to_string().chars().take(800).collect();
                }
            }
        }
        job.finished_unix_ms = Some(now_unix_ms);
        self.cancellation = None;
        true
    }
    pub fn stop(&mut self) {
        self.playback_epoch = self.playback_epoch.saturating_add(1);
        self.playback = None;
    }
    pub fn frame_due(&self, now_mono_ms: u64) -> Option<FrameDue> {
        let p = self.playback.as_ref()?;
        if p.conversion.frames == 0 {
            return None;
        }
        let sequence = now_mono_ms
            .saturating_sub(p.started_mono_ms)
            .saturating_mul(p.conversion.fps.value() as u64)
            / 1000;
        Some(FrameDue {
            key: p.conversion.key.clone(),
            video_id: p.conversion.video_id.clone(),
            index: sequence % p.conversion.frames,
            sequence,
            epoch: p.epoch,
        })
    }
    /// Export the plugin's one shared clock to the common transport. The
    /// common dispatcher must use this start, never a per-device start time.
    pub fn playback_spec(&self) -> Option<PlaybackSpec> {
        let active = self.playback.as_ref()?;
        Some(PlaybackSpec {
            asset_id: format!("conversion-{}", active.conversion.key),
            generation: active.epoch,
            fps: active.conversion.fps.value() as u32,
            frame_count: active.conversion.frames,
            started_mono_ms: active.started_mono_ms,
            source: serde_json::to_value(&active.conversion).ok()?,
        })
    }
    /// Check the immutable descriptor before an executor reads one frame.
    pub fn conversion_from_spec(spec: &PlaybackSpec) -> Result<ConversionCache, YoutubeError> {
        if !spec.valid() {
            return Err(YoutubeError::InvalidFrame);
        }
        let item: ConversionCache =
            serde_json::from_value(spec.source.clone()).map_err(|_| YoutubeError::InvalidFrame)?;
        if spec.asset_id != format!("conversion-{}", item.key)
            || spec.fps != item.fps.value() as u32
            || spec.frame_count != item.frames
            || item.frames == 0
            || !valid_id(&item.video_id)
            || !valid_key(&item.key)
        {
            return Err(YoutubeError::InvalidFrame);
        }
        Ok(item)
    }
    /// Plan each recipient from the same clock. Queue capacity and actual
    /// network sends remain per-device responsibilities of the dispatcher.
    pub fn plan_delivery(&self, viewer: Viewer<'_>, now_mono_ms: u64) -> Option<Delivery> {
        if !viewer.selected_youtube || !viewer.online || viewer.max_fps == 0 {
            return None;
        }
        let frame = self.frame_due(now_mono_ms)?;
        if let Some(cursor) = self.viewers.get(viewer.device_id) {
            if cursor.epoch == frame.epoch {
                if cursor.sequence == frame.sequence {
                    return None;
                }
                let interval = (1000u64 / viewer.max_fps as u64).max(1);
                if now_mono_ms.saturating_sub(cursor.queued_mono_ms) < interval {
                    return None;
                }
            }
        }
        Some(Delivery {
            device_id: viewer.device_id.to_owned(),
            frame,
        })
    }
    /// Call only after the current frame entered that device's bounded queue.
    pub fn mark_enqueued(&mut self, delivery: &Delivery, now_mono_ms: u64) -> bool {
        if self.playback.as_ref().is_none_or(|p| {
            p.epoch != delivery.frame.epoch || p.conversion.key != delivery.frame.key
        }) {
            return false;
        }
        self.viewers.insert(
            delivery.device_id.clone(),
            ViewerCursor {
                epoch: delivery.frame.epoch,
                sequence: delivery.frame.sequence,
                queued_mono_ms: now_mono_ms,
            },
        );
        true
    }
    pub fn forget_viewer(&mut self, device_id: &str) {
        self.viewers.remove(device_id);
    }
    pub fn position_ms(&self, now_mono_ms: u64) -> Option<u64> {
        let p = self.playback.as_ref()?;
        let frame = self.frame_due(now_mono_ms)?;
        Some(frame.index.saturating_mul(1000) / p.conversion.fps.value() as u64)
    }
    pub fn history(&self) -> Vec<&VideoRecord> {
        let mut entries: Vec<_> = self.videos.values().collect();
        entries.sort_by_key(|v| {
            std::cmp::Reverse(
                v.metadata
                    .played_unix_ms
                    .unwrap_or(v.metadata.updated_unix_ms),
            )
        });
        entries
    }
    pub fn request_delete(
        &mut self,
        video_id: &str,
        confirmation: &str,
    ) -> Result<DeleteJob, YoutubeError> {
        if !valid_id(video_id) || confirmation != video_id {
            return Err(YoutubeError::ConfirmationRequired);
        }
        if !self.videos.contains_key(video_id) {
            return Err(YoutubeError::MissingVideo);
        }
        if self.deleting.contains(video_id)
            || self
                .playback
                .as_ref()
                .is_some_and(|p| p.conversion.video_id == video_id)
            || self.job.as_ref().is_some_and(|j| {
                j.video_id == video_id
                    && matches!(j.status, JobStatus::Working | JobStatus::Cancelling)
            })
        {
            return Err(YoutubeError::Busy);
        }
        self.deleting.insert(video_id.to_owned());
        Ok(DeleteJob {
            video_id: video_id.to_owned(),
        })
    }
    pub fn complete_delete(
        &mut self,
        job: DeleteJob,
        result: Result<(), YoutubeError>,
    ) -> Result<(), YoutubeError> {
        if !self.deleting.remove(&job.video_id) {
            return Err(YoutubeError::Stale);
        }
        result?;
        self.videos.remove(&job.video_id);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DeleteJob {
    pub video_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YoutubeError {
    InvalidUrl,
    InvalidFps,
    InvalidTime,
    InvalidClip,
    InvalidMetadata,
    MissingCache,
    MissingVideo,
    NoPlayback,
    ConfirmationRequired,
    Busy,
    Cancelled,
    Stale,
    Overflow,
    Tool(String),
    Storage(String),
    InvalidFrame,
}
impl std::fmt::Display for YoutubeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl => write!(f, "単一のYouTube動画URLを指定してください"),
            Self::InvalidFps => write!(f, "FPSは6 / 12 / 18 / 24 / 30から選択してください"),
            Self::InvalidTime => write!(f, "時刻は秒または HH:MM:SS で指定してください"),
            Self::InvalidClip => write!(f, "切出範囲は 0 ≤ 開始 < 終了 ≤ 動画長にしてください"),
            Self::InvalidMetadata => write!(f, "長さが確定した単一動画だけを再生できます"),
            Self::MissingCache => write!(f, "完成キャッシュが見つかりません"),
            Self::MissingVideo => write!(f, "履歴が見つかりません"),
            Self::NoPlayback => write!(f, "再生中の動画がありません"),
            Self::ConfirmationRequired => write!(f, "削除する動画の確認が必要です"),
            Self::Busy => write!(f, "処理中です。停止または取消完了後に操作してください"),
            Self::Cancelled => write!(f, "取り消しました"),
            Self::Stale => write!(f, "古い処理結果です"),
            Self::Overflow => write!(f, "処理番号の上限に達しました"),
            Self::Tool(msg) | Self::Storage(msg) => f.write_str(msg),
            Self::InvalidFrame => write!(f, "映像フレームが不正です"),
        }
    }
}
impl std::error::Error for YoutubeError {}

fn format_ms(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}
fn hash_key(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())[..24].to_owned()
}
pub fn conversion_key(id: &str, clip: Clip, fps: Fps) -> String {
    hash_key(&[
        id,
        &format_ms(clip.start_ms),
        &format_ms(clip.end_ms),
        &fps.value().to_string(),
        CONVERSION_VERSION,
    ])
}
pub fn source_key(id: &str, start_ms: u64, end_ms: u64, full: bool) -> String {
    hash_key(&[
        id,
        &format_ms(start_ms),
        &format_ms(end_ms),
        if full { "full" } else { "clip" },
        "source-v1",
    ])
}

pub(crate) fn check_cache_ids(id: &str, key: &str) -> Result<(), YoutubeError> {
    if valid_id(id) && valid_key(key) {
        Ok(())
    } else {
        Err(YoutubeError::InvalidFrame)
    }
}

#[cfg(test)]
mod tests;
