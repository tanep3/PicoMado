//! Finite local videos. Pure state transitions live here; file and process IO
//! belongs to LocalVideoExecutor and never runs with the application lock.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use crate::plugin_jobs::{
    CommandContext, JobExecutionContext, JobFailure, JobOutcome, JobRequest, PlaybackSpec,
    PluginJob, PluginJobExecutor, PluginPaths, PluginWorkPlan, RefreshTargets,
};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};

pub(crate) const MAX_SOURCE: u64 = 512 * 1024 * 1024;
const MAX_JPEG: u64 = 65_536;

pub struct LocalVideoPlugin;
impl Plugin for LocalVideoPlugin {
    fn id(&self) -> &str {
        "local-video"
    }
    fn supports(&self, caps: &BTreeSet<Capability>) -> bool {
        caps.contains("lcd")
    }
    fn candidates<'a>(
        &'a self,
        _: &'a dyn WorldQuery,
        _: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Video {
    pub key: String,
    pub name: String,
    pub fps: u32,
    pub frames: u64,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalVideoContent {
    videos: BTreeMap<String, Video>,
    active: Option<PlaybackSpec>,
    next_generation: u64,
    pending: Option<u64>,
    error: Option<String>,
    #[serde(skip)]
    validated: bool,
    #[serde(skip)]
    imports: Vec<String>,
    #[serde(skip)]
    bootstrap_scheduled: bool,
}
impl Default for LocalVideoContent {
    fn default() -> Self {
        Self {
            videos: BTreeMap::new(),
            active: None,
            next_generation: 1,
            pending: None,
            error: None,
            validated: false,
            imports: Vec::new(),
            bootstrap_scheduled: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Work {
    Scan,
    Prepare {
        name: String,
        fps: u32,
        generation: u64,
    },
    Validate {
        video: Video,
        generation: u64,
    },
    Delete {
        key: String,
    },
    DeleteImport {
        name: String,
    },
    Bootstrap {
        active: Option<PlaybackSpec>,
    },
}
fn request(slot: &str, work: Work, timeout_seconds: u64) -> JobRequest {
    JobRequest {
        slot: slot.into(),
        payload: serde_json::to_value(work).expect("serializable work"),
        timeout_seconds,
    }
}
pub(crate) fn good_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(|c| c.is_control())
}
fn good_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}
fn fps_valid(fps: u32) -> bool {
    (1..=30).contains(&fps)
}
fn source_spec(video: &Video, generation: u64, now: u64) -> PlaybackSpec {
    PlaybackSpec {
        asset_id: video.key.clone(),
        generation,
        fps: video.fps,
        frame_count: video.frames,
        started_mono_ms: now,
        source: serde_json::to_value(video).expect("serializable video"),
    }
}
impl ContentPlugin for LocalVideoContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "local-video"
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
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> {
        if self.validated && self.active.is_some() {
            return None;
        }
        Some(crate::lcd_text::status_frame(
            "LOCAL VIDEO",
            if self.pending.is_some() {
                "PREPARING"
            } else if self.active.is_some() {
                "VALIDATING"
            } else {
                "NO VIDEO"
            },
        ))
    }
    fn save_state(&self) -> Value {
        serde_json::to_value(self).expect("serializable local video")
    }
    fn load_state(&mut self, value: Value) -> Result<(), ContentError> {
        if value.is_null() {
            return Ok(());
        }
        *self = serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)?;
        self.validated = false;
        self.bootstrap_scheduled = false;
        Ok(())
    }
    fn web_state_at(&self, now: u64) -> Value {
        json!({"imports":self.imports,"videos":self.videos.values().collect::<Vec<_>>(),
            "active":self.active,"position_ms":self.active.as_ref().map(|s|now.saturating_sub(s.started_mono_ms)).unwrap_or(0),
            "pending":self.pending.is_some(),"error":self.error})
    }
    fn plan_command(
        &mut self,
        ctx: &CommandContext,
        command: &Value,
    ) -> Result<PluginWorkPlan, ContentError> {
        let kind = command["kind"]
            .as_str()
            .ok_or(ContentError::InvalidSource)?;
        let mut plan = PluginWorkPlan::default();
        match kind {
            "scan" => plan.jobs.push(request("scan", Work::Scan, 60)),
            "prepare" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let name = command["name"]
                    .as_str()
                    .ok_or(ContentError::InvalidSource)?;
                let fps = command["fps"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or(ContentError::InvalidSource)?;
                if !good_name(name) || !fps_valid(fps) || !self.imports.iter().any(|n| n == name) {
                    return Err(ContentError::InvalidSource);
                }
                let generation = self.next_generation;
                self.next_generation = self
                    .next_generation
                    .checked_add(1)
                    .ok_or(ContentError::Unavailable)?;
                self.pending = Some(generation);
                self.error = None;
                plan.jobs.push(request(
                    "prepare",
                    Work::Prepare {
                        name: name.into(),
                        fps,
                        generation,
                    },
                    7500,
                ));
            }
            "play" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let key = command["key"].as_str().ok_or(ContentError::InvalidSource)?;
                let video = self.videos.get(key).ok_or(ContentError::InvalidSource)?;
                let generation = self.next_generation;
                self.next_generation = self
                    .next_generation
                    .checked_add(1)
                    .ok_or(ContentError::Unavailable)?;
                self.pending = Some(generation);
                self.error = None;
                plan.jobs.push(request(
                    "prepare",
                    Work::Validate {
                        video: video.clone(),
                        generation,
                    },
                    180,
                ));
            }
            "stop" => {
                self.active = None;
                self.validated = false;
                plan.refresh = RefreshTargets::Selected
            }
            "cancel" => {
                if self.pending.is_some() {
                    plan.cancel_slots.push("prepare".into())
                }
            }
            "delete" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let key = command["key"].as_str().ok_or(ContentError::InvalidSource)?;
                if !self.videos.contains_key(key) || command["confirmation"].as_str() != Some(key) {
                    return Err(ContentError::InvalidSource);
                }
                if self
                    .active
                    .as_ref()
                    .is_some_and(|spec| spec.asset_id == key)
                {
                    self.active = None;
                    self.validated = false;
                    plan.refresh = RefreshTargets::Selected;
                }
                plan.jobs.push(request(
                    &format!("delete_{key}"),
                    Work::Delete { key: key.into() },
                    180,
                ));
            }
            "delete_import" => {
                if self.pending.is_some() {
                    return Err(ContentError::Unavailable);
                }
                let name = command["name"]
                    .as_str()
                    .ok_or(ContentError::InvalidSource)?;
                if !good_name(name)
                    || command["confirmation"].as_str() != Some(name)
                    || !self.imports.iter().any(|item| item == name)
                {
                    return Err(ContentError::InvalidSource);
                }
                plan.jobs.push(request(
                    "delete_import",
                    Work::DeleteImport { name: name.into() },
                    180,
                ));
            }
            _ => return Err(ContentError::Unsupported),
        }
        let _ = ctx;
        Ok(plan)
    }
    fn plan_schedule(
        &mut self,
        _: &CommandContext,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        if self.bootstrap_scheduled {
            return Ok(None);
        }
        self.bootstrap_scheduled = true;
        Ok(Some(PluginWorkPlan {
            jobs: vec![
                request(
                    "bootstrap",
                    Work::Bootstrap {
                        active: self.active.clone(),
                    },
                    180,
                ),
                request("scan", Work::Scan, 60),
            ],
            ..Default::default()
        }))
    }
    fn work_completed(
        &mut self,
        ctx: &CommandContext,
        job: &PluginJob,
        outcome: &JobOutcome,
    ) -> Result<PluginWorkPlan, ContentError> {
        let work: Work =
            serde_json::from_value(job.payload.clone()).map_err(|_| ContentError::InvalidSource)?;
        let mut plan = PluginWorkPlan::default();
        match work {
            Work::Scan => {
                if let JobOutcome::Succeeded(value) = outcome {
                    self.imports = serde_json::from_value(value.clone())
                        .map_err(|_| ContentError::InvalidSource)?
                }
            }
            Work::Prepare { generation, .. } | Work::Validate { generation, .. } => {
                if self.pending != Some(generation) {
                    return Err(ContentError::Stale);
                }
                self.pending = None;
                match outcome {
                    JobOutcome::Succeeded(value) => {
                        let video: Video = serde_json::from_value(value.clone())
                            .map_err(|_| ContentError::InvalidSource)?;
                        if !good_key(&video.key)
                            || !good_name(&video.name)
                            || !fps_valid(video.fps)
                            || video.frames == 0
                        {
                            return Err(ContentError::InvalidSource);
                        }
                        self.videos.insert(video.key.clone(), video.clone());
                        self.active = Some(source_spec(&video, generation, ctx.now_mono_ms));
                        self.validated = true;
                        self.error = None;
                        plan.refresh = RefreshTargets::Selected;
                    }
                    JobOutcome::Failed(f) => self.error = Some(f.message.clone()),
                    JobOutcome::Cancelled => self.error = Some("処理を取り消しました".into()),
                }
            }
            Work::Delete { key } => {
                if matches!(outcome, JobOutcome::Succeeded(_)) {
                    self.videos.remove(&key);
                    if self.active.as_ref().is_some_and(|s| s.asset_id == key) {
                        self.active = None;
                        self.validated = false;
                        plan.refresh = RefreshTargets::Selected
                    }
                }
            }
            Work::DeleteImport { name } => {
                if matches!(outcome, JobOutcome::Succeeded(_)) {
                    self.imports.retain(|item| item != &name);
                }
            }
            Work::Bootstrap { active } => {
                if self.active.as_ref().map(|s| (&s.asset_id, s.generation))
                    != active.as_ref().map(|s| (&s.asset_id, s.generation))
                {
                    return Err(ContentError::Stale);
                }
                if let JobOutcome::Succeeded(value) = outcome {
                    if value.as_bool() == Some(true) {
                        if let Some(spec) = &mut self.active {
                            spec.started_mono_ms = ctx.now_mono_ms;
                            self.validated = true
                        }
                    } else {
                        self.active = None;
                        self.validated = false
                    }
                } else {
                    self.active = None;
                    self.validated = false
                }
                plan.refresh = RefreshTargets::Selected;
            }
        }
        Ok(plan)
    }
    fn playback(&self, ctx: &PluginContext) -> Option<PlaybackSpec> {
        (self.validated && ctx.mode == crate::Mode::Normal)
            .then(|| self.active.clone())
            .flatten()
    }
}

#[derive(Clone, Default)]
pub struct LocalVideoExecutor;
fn fail(code: &str, message: &str) -> JobOutcome {
    JobOutcome::Failed(JobFailure::new(code, message))
}
impl PluginJobExecutor for LocalVideoExecutor {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome> {
        Box::pin(async move {
            let work: Work = match serde_json::from_value(payload) {
                Ok(v) => v,
                Err(_) => return fail("invalid_work", "動画の処理要求が不正です"),
            };
            let paths = context.paths;
            let mut cancel = context.cancel;
            let token = crate::youtube::Cancellation::default();
            if *cancel.borrow() {
                token.cancel();
            }
            let worker_token = token.clone();
            let mut worker =
                tokio::task::spawn_blocking(move || run_work(work, paths, worker_token));
            let result = tokio::select! {
                result=&mut worker=>result,
                _=async{while !*cancel.borrow(){if cancel.changed().await.is_err(){break}}}=>{
                    token.cancel();
                    worker.await
                }
            };
            match result {
                Ok(Ok(v)) => JobOutcome::Succeeded(v),
                Ok(Err(WorkError::Cancelled)) => JobOutcome::Cancelled,
                Ok(Err(WorkError::Invalid)) => {
                    fail("invalid_video", "動画ファイルまたは変換結果が不正です")
                }
                Ok(Err(WorkError::Missing)) => {
                    fail("missing_video", "動画ファイルが見つかりません")
                }
                Ok(Err(WorkError::Storage)) => {
                    fail("storage_failed", "動画の保存処理に失敗しました")
                }
                Ok(Err(WorkError::Tool)) | Err(_) => {
                    fail("conversion_failed", "動画の変換に失敗しました")
                }
            }
        })
    }
    fn read_frame(
        &self,
        spec: PlaybackSpec,
        index: u64,
        paths: PluginPaths,
    ) -> BoxFuture<'static, Result<Vec<u8>, JobFailure>> {
        Box::pin(async move {
            let video: Video = serde_json::from_value(spec.source)
                .map_err(|_| JobFailure::new("invalid_frame", "フレーム情報が不正です"))?;
            tokio::task::spawn_blocking(move || read_frame(&paths.cache, &video, index))
                .await
                .map_err(|_| JobFailure::new("read_failed", "フレーム読取りに失敗しました"))?
                .map_err(|_| JobFailure::new("read_failed", "フレーム読取りに失敗しました"))
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
enum WorkError {
    Cancelled,
    Invalid,
    Missing,
    Storage,
    Tool,
}
fn checked_file(path: &Path) -> Result<File, WorkError> {
    let meta = fs::symlink_metadata(path).map_err(|_| WorkError::Missing)?;
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > MAX_SOURCE {
        return Err(WorkError::Invalid);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| WorkError::Invalid)?;
    let opened = file.metadata().map_err(|_| WorkError::Invalid)?;
    if !opened.is_file() || opened.len() != meta.len() {
        return Err(WorkError::Invalid);
    }
    Ok(file)
}
fn cache_dir(root: &Path, key: &str) -> Result<PathBuf, WorkError> {
    if !good_key(key) {
        return Err(WorkError::Invalid);
    }
    let parent = root.join("videos");
    if parent
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(WorkError::Invalid);
    }
    let dir = parent.join(key);
    if dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(WorkError::Invalid);
    }
    Ok(dir)
}
fn manifest(root: &Path, video: &Video) -> Result<bool, WorkError> {
    if !good_key(&video.key)
        || video.frames == 0
        || video.frames > 99_999_999
        || !fps_valid(video.fps)
    {
        return Ok(false);
    }
    let dir = cache_dir(root, &video.key)?;
    if !dir.symlink_metadata().is_ok_and(|m| m.file_type().is_dir()) {
        return Ok(false);
    }
    let mut count = 0u64;
    let mut bytes = 0u64;
    for entry in fs::read_dir(dir).map_err(|_| WorkError::Storage)? {
        let entry = entry.map_err(|_| WorkError::Storage)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 17 || !name.starts_with("frame") || !name.ends_with(".jpg") {
            return Ok(false);
        }
        let number = name[5..13].parse::<u64>().ok();
        if number.is_none_or(|n| n == 0 || n > video.frames) {
            return Ok(false);
        }
        let meta = entry
            .path()
            .symlink_metadata()
            .map_err(|_| WorkError::Storage)?;
        if !meta.file_type().is_file() || !(4..=MAX_JPEG).contains(&meta.len()) {
            return Ok(false);
        }
        count += 1;
        bytes += meta.len();
    }
    Ok(count == video.frames && bytes == video.bytes)
}
fn read_frame(root: &Path, video: &Video, index: u64) -> Result<Vec<u8>, WorkError> {
    if index >= video.frames || !good_key(&video.key) {
        return Err(WorkError::Invalid);
    }
    let path = cache_dir(root, &video.key)?.join(format!("frame{:08}.jpg", index + 1));
    let meta = path.symlink_metadata().map_err(|_| WorkError::Missing)?;
    if !meta.file_type().is_file() || !(4..=MAX_JPEG).contains(&meta.len()) {
        return Err(WorkError::Invalid);
    }
    let jpeg = fs::read(path).map_err(|_| WorkError::Storage)?;
    crate::youtube::rgb565_from_jpeg(&jpeg, crate::youtube::ColorTransform::None)
        .map_err(|_| WorkError::Invalid)
}
fn run_work(
    work: Work,
    paths: PluginPaths,
    cancel: crate::youtube::Cancellation,
) -> Result<Value, WorkError> {
    match work {
        Work::Scan => {
            let dir = paths.data.join("imports");
            fs::create_dir_all(&dir).map_err(|_| WorkError::Storage)?;
            if dir
                .symlink_metadata()
                .map_err(|_| WorkError::Storage)?
                .file_type()
                .is_symlink()
            {
                return Err(WorkError::Invalid);
            }
            let mut names = Vec::new();
            for entry in fs::read_dir(dir).map_err(|_| WorkError::Storage)? {
                let entry = entry.map_err(|_| WorkError::Storage)?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if good_name(&name)
                    && entry.path().symlink_metadata().is_ok_and(|m| {
                        m.file_type().is_file() && m.len() > 0 && m.len() <= MAX_SOURCE
                    })
                {
                    names.push(name)
                }
            }
            names.sort();
            Ok(json!(names))
        }
        Work::Prepare {
            name,
            fps,
            generation,
        } => {
            if !good_name(&name) || !fps_valid(fps) {
                return Err(WorkError::Invalid);
            }
            let dir = paths.data.join("imports");
            if dir
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
            {
                return Err(WorkError::Invalid);
            }
            let source = dir.join(&name);
            let mut input = checked_file(&source)?;
            fs::create_dir_all(&paths.cache).map_err(|_| WorkError::Storage)?;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| WorkError::Storage)?
                .as_nanos();
            let work = paths
                .cache
                .join(format!("work-{}-{generation}-{nonce}", std::process::id()));
            fs::create_dir(&work).map_err(|_| WorkError::Storage)?;
            let result = (|| {
                let copy = work.join("source");
                let mut output = File::create(&copy).map_err(|_| WorkError::Storage)?;
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 65536];
                let mut total = 0u64;
                loop {
                    if cancel.is_cancelled() {
                        return Err(WorkError::Cancelled);
                    }
                    let n = input.read(&mut buffer).map_err(|_| WorkError::Storage)?;
                    if n == 0 {
                        break;
                    }
                    total += n as u64;
                    if total > MAX_SOURCE {
                        return Err(WorkError::Invalid);
                    }
                    hash.update(&buffer[..n]);
                    output
                        .write_all(&buffer[..n])
                        .map_err(|_| WorkError::Storage)?;
                }
                output.sync_all().map_err(|_| WorkError::Storage)?;
                let content_hash = hash.finalize();
                let mut key_hash = Sha256::new();
                key_hash.update(crate::youtube::CONVERSION_VERSION.as_bytes());
                key_hash.update(fps.to_be_bytes());
                key_hash.update(content_hash);
                let key = format!("{:x}", key_hash.finalize());
                let (frames, bytes) = crate::youtube::run_local_conversion(
                    &paths.cache,
                    &copy,
                    &key,
                    fps as u8,
                    &cancel,
                )
                .map_err(|error| match error {
                    crate::youtube::YoutubeError::Cancelled => WorkError::Cancelled,
                    crate::youtube::YoutubeError::Storage(_) => WorkError::Storage,
                    _ => WorkError::Tool,
                })?;
                let video = Video {
                    key,
                    name: name.clone(),
                    fps,
                    frames,
                    bytes,
                };
                Ok(json!(video))
            })();
            let _ = fs::remove_dir_all(work);
            result
        }
        Work::Validate { video, .. } => {
            if !manifest(&paths.cache, &video)? {
                return Err(WorkError::Invalid);
            }
            let _ = read_frame(&paths.cache, &video, 0)?;
            Ok(json!(video))
        }
        Work::Bootstrap { active } => {
            let Some(spec) = active else {
                return Ok(json!(true));
            };
            let video: Video =
                serde_json::from_value(spec.source).map_err(|_| WorkError::Invalid)?;
            Ok(json!(manifest(&paths.cache, &video)?))
        }
        Work::Delete { key } => {
            let dir = cache_dir(&paths.cache, &key)?;
            if dir.exists() {
                fs::remove_dir_all(dir).map_err(|_| WorkError::Storage)?
            }
            Ok(Value::Null)
        }
        Work::DeleteImport { name } => {
            if !good_name(&name) {
                return Err(WorkError::Invalid);
            }
            let dir = paths.data.join("imports");
            if dir
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                return Err(WorkError::Invalid);
            }
            let file = dir.join(name);
            let meta = file.symlink_metadata().map_err(|_| WorkError::Missing)?;
            if !meta.file_type().is_file() {
                return Err(WorkError::Invalid);
            }
            fs::remove_file(file).map_err(|_| WorkError::Storage)?;
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_keys_are_bounded() {
        for name in ["../x", "/tmp/a", ".hidden", "a\\b", ""] {
            assert!(!good_name(name))
        }
        assert!(good_name("sentinel2_japan_2fps_wide_long_clean.mp4"));
        assert!(!good_key("../bad"));
    }
    #[test]
    fn conversion_and_restore_use_the_common_frame_format() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: temp.path().join("data"),
            cache: temp.path().join("cache"),
        };
        let imports = paths.data.join("imports");
        fs::create_dir_all(&imports).unwrap();
        let source = imports.join("clip.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=2",
                "-t",
                "2",
                "-c:v",
                "mpeg4",
                "-y",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        let cancel = crate::youtube::Cancellation::default();
        let scanned = run_work(Work::Scan, paths.clone(), cancel.clone()).unwrap();
        assert_eq!(scanned, json!(["clip.mp4"]));
        let value = run_work(
            Work::Prepare {
                name: "clip.mp4".into(),
                fps: 2,
                generation: 1,
            },
            paths.clone(),
            cancel.clone(),
        )
        .unwrap();
        let video: Video = serde_json::from_value(value).unwrap();
        assert_eq!(video.fps, 2);
        assert_eq!(video.frames, 4);
        assert!(manifest(&paths.cache, &video).unwrap());
        assert_eq!(
            read_frame(&paths.cache, &video, 0).unwrap().len(),
            crate::protocol::LCD_FRAME_BYTES
        );
        assert_eq!(
            read_frame(&paths.cache, &video, video.frames - 1)
                .unwrap()
                .len(),
            crate::protocol::LCD_FRAME_BYTES
        );
        let mut content = LocalVideoContent::default();
        content.videos.insert(video.key.clone(), video.clone());
        content.active = Some(source_spec(&video, 1, 100));
        let saved = content.save_state();
        let mut restored = LocalVideoContent::default();
        restored.load_state(saved).unwrap();
        assert!(!restored.validated);
        let restored_spec = restored.active.clone().unwrap();
        assert!(run_work(
            Work::Bootstrap {
                active: Some(restored_spec)
            },
            paths.clone(),
            cancel.clone()
        )
        .unwrap()
        .as_bool()
        .unwrap());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&source, imports.join("link.mp4")).unwrap();
            assert!(matches!(
                run_work(
                    Work::Prepare {
                        name: "link.mp4".into(),
                        fps: 2,
                        generation: 2
                    },
                    paths.clone(),
                    cancel.clone()
                ),
                Err(WorkError::Invalid)
            ));
        }
        run_work(
            Work::DeleteImport {
                name: "clip.mp4".into(),
            },
            paths.clone(),
            cancel,
        )
        .unwrap();
        assert!(!source.exists());
        assert!(manifest(&paths.cache, &video).unwrap());
        assert_eq!(
            read_frame(&paths.cache, &video, 0).unwrap().len(),
            crate::protocol::LCD_FRAME_BYTES
        );
    }

    #[test]
    #[ignore = "uses the Library reference MP4 when PICOMADO_TEST_VIDEO is set"]
    fn reference_two_fps_mp4() {
        let source = std::path::PathBuf::from(
            std::env::var("PICOMADO_TEST_VIDEO").expect("reference video path"),
        );
        let temp = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: temp.path().join("data"),
            cache: temp.path().join("cache"),
        };
        let imports = paths.data.join("imports");
        fs::create_dir_all(&imports).unwrap();
        fs::copy(source, imports.join("reference.mp4")).unwrap();
        let cancel = crate::youtube::Cancellation::default();
        let value = run_work(
            Work::Prepare {
                name: "reference.mp4".into(),
                fps: 2,
                generation: 1,
            },
            paths.clone(),
            cancel,
        )
        .unwrap();
        let video: Video = serde_json::from_value(value).unwrap();
        assert_eq!(video.fps, 2);
        assert_eq!(video.frames, 460);
        assert_eq!(
            read_frame(&paths.cache, &video, 0).unwrap().len(),
            crate::protocol::LCD_FRAME_BYTES
        );
        assert_eq!(
            read_frame(&paths.cache, &video, 459).unwrap().len(),
            crate::protocol::LCD_FRAME_BYTES
        );
    }
}
