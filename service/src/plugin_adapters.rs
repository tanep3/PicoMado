//! Built-in composition adapters for the dedicated typed media module.
//! Application, transport and workers only use the common content contracts.

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use crate::plugin_jobs::*;
use crate::youtube as yt;

#[derive(Clone, Default)]
pub struct YoutubeManaged {
    inner: yt::YoutubeContent,
    usage: Option<yt::Usage>,
    bootstrap_scheduled: bool,
    validated: bool,
}

#[derive(Serialize, Deserialize)]
struct YoutubeSnapshot {
    stored: yt::StoredYoutube,
    job: Option<yt::JobState>,
    deleting: BTreeSet<String>,
    usage: Option<yt::Usage>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum YoutubeWork {
    Prepare {
        #[serde(default)]
        cached_only: bool,
        generation: u64,
        url: yt::VideoUrl,
        fps: yt::Fps,
        start_ms: Option<u64>,
        end_ms: Option<u64>,
        previous: Option<Box<yt::VideoRecord>>,
    },
    Delete {
        video_id: String,
    },
    ValidateSelected {
        conversion: Option<yt::ConversionCache>,
    },
    Usage,
}

fn request(slot: &str, work: YoutubeWork) -> Result<JobRequest, ContentError> {
    Ok(JobRequest {
        slot: slot.into(),
        payload: serde_json::to_value(work).map_err(|_| ContentError::InvalidSource)?,
        timeout_seconds: 86_400,
    })
}
fn prepare(job: yt::PrepareJob, cached_only: bool) -> Result<JobRequest, ContentError> {
    request(
        "prepare",
        YoutubeWork::Prepare {
            cached_only,
            generation: job.generation,
            url: job.url,
            fps: job.fps,
            start_ms: job.start_ms,
            end_ms: job.end_ms,
            previous: job.previous.map(Box::new),
        },
    )
}
fn input_error(error: yt::YoutubeError) -> ContentError {
    match error {
        yt::YoutubeError::Busy => ContentError::Unavailable,
        yt::YoutubeError::Stale => ContentError::Stale,
        _ => ContentError::InvalidSource,
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ContentError> {
    serde_json::from_value(value).map_err(|_| ContentError::InvalidSource)
}

impl ContentPlugin for YoutubeManaged {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "youtube"
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
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> {
        if self.validated && self.inner.playback.is_some() {
            return None; // The streaming worker reads pixels outside the App lock.
        }
        let status = match self.inner.job.as_ref().map(|job| job.status) {
            Some(yt::JobStatus::Working) => "PREPARING",
            Some(yt::JobStatus::Cancelling) => "CANCELLING",
            Some(yt::JobStatus::Failed) => "FAILED",
            _ if self.inner.playback.is_some() => "VALIDATING",
            _ => "NO VIDEO",
        };
        Some(crate::lcd_text::status_frame("YOUTUBE", status))
    }
    fn session_reset(&mut self, id: &str, _: u64) {
        self.inner.forget_viewer(id);
    }
    fn delete_device(&mut self, id: &str) {
        self.inner.forget_viewer(id);
    }
    fn save_state(&self) -> Value {
        serde_json::to_value(YoutubeSnapshot {
            stored: self.inner.save_state(),
            job: self.inner.job.clone(),
            deleting: self.inner.deleting.clone(),
            usage: self.usage,
        })
        .expect("serializable YouTube state")
    }
    fn load_state(&mut self, value: Value) -> Result<(), ContentError> {
        if value.is_null() {
            return Ok(());
        } // Legacy placeholder had no state.

        let state: YoutubeSnapshot = decode(value)?;
        self.inner = yt::YoutubeContent::restore(state.stored, 0);
        self.inner.job = state.job;
        self.inner.deleting = state.deleting;
        self.usage = state.usage;
        self.bootstrap_scheduled = false;
        self.validated = false;
        Ok(())
    }
    fn web_state(&self) -> Value {
        self.web_state_at(0)
    }
    fn web_state_at(&self, now: u64) -> Value {
        let videos: Vec<_> = self.inner.history().into_iter().cloned().collect();
        let mut source = 0u64;
        let mut converted = 0u64;
        let mut thumbnail = 0u64;
        for video in &videos {
            source = source.saturating_add(video.sources.values().map(|v| v.bytes).sum::<u64>());
            converted =
                converted.saturating_add(video.conversions.values().map(|v| v.bytes).sum::<u64>());
            thumbnail = thumbnail.saturating_add(video.metadata.thumbnail_bytes);
        }
        json!({"videos":videos,"active":self.inner.playback,"job":self.inner.job,"deleting":self.inner.deleting,
            "position_ms":self.inner.position_ms(now),"cache_usage":self.usage,
            "known_bytes":{"source":source,"converted":converted,"thumbnail":thumbnail,"total":source.saturating_add(converted).saturating_add(thumbnail)}})
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
        let now = ctx.now_unix.saturating_mul(1000);
        match kind {
            "prepare" => {
                self.inner
                    .set_default_fps(ctx.settings.youtube_default_fps)
                    .map_err(input_error)?;
                let url = command["url"]
                    .as_str()
                    .ok_or(ContentError::InvalidSource)?
                    .into();
                let fps = command
                    .get("fps")
                    .filter(|v| !v.is_null())
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| *n <= u8::MAX as u64)
                            .map(|n| n as u8)
                            .ok_or(ContentError::InvalidSource)
                    })
                    .transpose()?;
                let start = command
                    .get("start")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let end = command
                    .get("end")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                plan.jobs.push(prepare(
                    self.inner
                        .request_prepare(
                            yt::PrepareRequest {
                                url,
                                fps,
                                start,
                                end,
                            },
                            now,
                        )
                        .map_err(input_error)?,
                    false,
                )?);
            }
            "play" => plan.jobs.push(prepare(
                self.inner
                    .request_play(
                        command["key"].as_str().ok_or(ContentError::InvalidSource)?,
                        now,
                    )
                    .map_err(input_error)?,
                true,
            )?),
            "set_fps" => {
                let fps = command["fps"]
                    .as_u64()
                    .filter(|n| *n <= u8::MAX as u64)
                    .ok_or(ContentError::InvalidSource)? as u8;
                plan.jobs.push(prepare(
                    self.inner.request_fps(fps, now).map_err(input_error)?,
                    true,
                )?);
            }
            "cancel" => {
                if let Some(job) = &mut self.inner.job {
                    if job.status == yt::JobStatus::Working {
                        // Plan only the state change. The common worker sends
                        // cancellation after this cloned state is persisted.
                        job.status = yt::JobStatus::Cancelling;
                        job.message = "処理を取り消しています".into();
                        plan.cancel_slots.push("prepare".into());
                    }
                }
            }
            "stop" => {
                self.inner.stop();
                plan.refresh = RefreshTargets::Selected;
            }
            "delete" => {
                let id = command["video_id"]
                    .as_str()
                    .ok_or(ContentError::InvalidSource)?;
                let confirmation = command["confirmation"]
                    .as_str()
                    .ok_or(ContentError::InvalidSource)?;
                let job = self
                    .inner
                    .request_delete(id, confirmation)
                    .map_err(input_error)?;
                plan.jobs.push(request(
                    &format!("delete_{}", job.video_id),
                    YoutubeWork::Delete {
                        video_id: job.video_id,
                    },
                )?);
            }
            "scan_usage" => plan.jobs.push(request("usage", YoutubeWork::Usage)?),
            _ => return Err(ContentError::Unsupported),
        }
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
            jobs: vec![request(
                "bootstrap",
                YoutubeWork::ValidateSelected {
                    conversion: self.inner.playback.as_ref().map(|p| p.conversion.clone()),
                },
            )?],
            ..Default::default()
        }))
    }
    fn work_progress(
        &mut self,
        _: &CommandContext,
        _: &PluginJob,
        progress: &JobProgress,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        if progress.data.is_null() {
            return Ok(None);
        }
        let value = &progress.data;
        let update = yt::Progress {
            generation: value["generation"]
                .as_u64()
                .ok_or(ContentError::InvalidSource)?,
            stage: decode(value["stage"].clone())?,
            percent: value["percent"].as_u64().unwrap_or(0).min(100) as u8,
            message: value["message"].as_str().unwrap_or_default().into(),
            metadata: decode(value["metadata"].clone())?,
            source: decode(value["source"].clone())?,
            conversion: decode(value["conversion"].clone())?,
        };
        Ok(self.inner.progress(update).then(PluginWorkPlan::default))
    }
    fn work_completed(
        &mut self,
        ctx: &CommandContext,
        job: &PluginJob,
        outcome: &JobOutcome,
    ) -> Result<PluginWorkPlan, ContentError> {
        let work: YoutubeWork = decode(job.payload.clone())?;
        let mut plan = PluginWorkPlan::default();
        match work {
            YoutubeWork::Prepare { generation, .. } => {
                let result = match outcome {
                    JobOutcome::Succeeded(value) => Ok(yt::PrepareResult {
                        metadata: decode(value["metadata"].clone())?,
                        source: decode(value["source"].clone())?,
                        conversion: decode(value["conversion"].clone())?,
                        reused: value["reused"].as_bool().unwrap_or(false),
                    }),
                    JobOutcome::Cancelled => Err(yt::YoutubeError::Cancelled),
                    JobOutcome::Failed(failure) => {
                        Err(yt::YoutubeError::Tool(failure.message.clone()))
                    }
                };
                let was_cancelled = self
                    .inner
                    .job
                    .as_ref()
                    .is_some_and(|j| j.status == yt::JobStatus::Cancelling);
                if !self.inner.complete_prepare(
                    generation,
                    result,
                    ctx.now_mono_ms,
                    ctx.now_unix.saturating_mul(1000),
                ) {
                    return Err(ContentError::Stale);
                }
                if !was_cancelled && matches!(outcome, JobOutcome::Succeeded(_)) {
                    self.validated = true;
                    plan.refresh = RefreshTargets::Selected;
                }
            }
            YoutubeWork::Delete { video_id } => {
                let result = match outcome {
                    JobOutcome::Succeeded(_) => Ok(()),
                    JobOutcome::Cancelled => Err(yt::YoutubeError::Cancelled),
                    JobOutcome::Failed(f) => Err(yt::YoutubeError::Storage(f.message.clone())),
                };
                self.inner
                    .complete_delete(yt::DeleteJob { video_id }, result)
                    .map_err(input_error)?;
            }
            YoutubeWork::ValidateSelected { .. } => {
                self.validated = true;
                if let JobOutcome::Succeeded(value) = outcome {
                    if !value["valid"].as_bool().unwrap_or(false) {
                        self.inner.stop();
                    } else if let Some(active) = &mut self.inner.playback {
                        active.started_mono_ms = ctx.now_mono_ms;
                    }
                } else {
                    self.inner.stop();
                }
                plan.refresh = RefreshTargets::Selected;
            }
            YoutubeWork::Usage => {
                if let JobOutcome::Succeeded(value) = outcome {
                    self.usage = Some(decode(value.clone())?);
                }
            }
        }
        Ok(plan)
    }
    fn playback(&self, ctx: &PluginContext) -> Option<PlaybackSpec> {
        if !self.validated || ctx.mode != crate::Mode::Normal {
            return None;
        }
        self.inner.playback_spec()
    }
    fn asset(&self, id: &str) -> Option<AssetDescriptor> {
        let video = id.strip_prefix("thumbnail-")?;
        self.inner
            .videos
            .contains_key(video)
            .then(|| AssetDescriptor {
                area: AssetArea::Cache,
                relative_path: format!("videos/{video}/thumbnail.jpg").into(),
                content_type: "image/jpeg".into(),
            })
    }
}

#[derive(Clone, Default)]
pub struct YoutubeExecutor;

async fn forward(progress: yt::Progress, sink: &JobProgressSink) {
    let data = json!({"generation":progress.generation,"stage":progress.stage,"percent":progress.percent,
        "message":progress.message,"metadata":progress.metadata,"source":progress.source,"conversion":progress.conversion});
    if !data["metadata"].is_null() || !data["source"].is_null() || !data["conversion"].is_null() {
        sink.report_data_wait(
            Some(progress.percent),
            data["message"].as_str().unwrap_or_default(),
            data.clone(),
        )
        .await;
    } else {
        sink.report_data(
            Some(progress.percent),
            data["message"].as_str().unwrap_or_default(),
            data.clone(),
        );
    }
}

fn fail(error: yt::YoutubeError) -> JobOutcome {
    if error == yt::YoutubeError::Cancelled {
        JobOutcome::Cancelled
    } else {
        JobOutcome::Failed(JobFailure::new(
            "youtube_failed",
            &match error {
                yt::YoutubeError::Storage(_) => "動画の保存処理に失敗しました".into(),
                other => other.to_string(),
            },
        ))
    }
}

impl PluginJobExecutor for YoutubeExecutor {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome> {
        Box::pin(async move {
            let work: YoutubeWork = match serde_json::from_value(payload) {
                Ok(v) => v,
                Err(_) => {
                    return JobOutcome::Failed(JobFailure::new("invalid_job", "Invalid media job"))
                }
            };
            if context.is_cancelled() {
                return JobOutcome::Cancelled;
            }
            match work {
                YoutubeWork::Prepare {
                    cached_only,
                    generation,
                    url,
                    fps,
                    start_ms,
                    end_ms,
                    previous,
                } => {
                    let cancellation = yt::Cancellation::default();
                    let job = yt::PrepareJob {
                        generation,
                        url,
                        fps,
                        start_ms,
                        end_ms,
                        previous: previous.map(|p| *p),
                        cancel: cancellation.clone(),
                    };
                    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
                    let future = async move {
                        let tools = yt::MediaTools::new(context.paths.cache);
                        if cached_only {
                            yt::run_cached_prepare(tools, job, sender, unix_ms()).await
                        } else {
                            yt::run_prepare(tools, job, sender, unix_ms()).await
                        }
                    };
                    tokio::pin!(future);
                    let mut cancel = context.cancel.clone();
                    let result = loop {
                        tokio::select! {
                            result=&mut future=>break result,
                            Some(progress)=receiver.recv()=>forward(progress,&context.progress).await,
                            changed=cancel.changed()=>if changed.is_err() || *cancel.borrow(){
                                cancellation.cancel();break future.await;
                            },
                        }
                    };
                    while let Ok(progress) = receiver.try_recv() {
                        forward(progress, &context.progress).await;
                    }
                    match result {
                        Ok(value) => JobOutcome::Succeeded(
                            json!({"metadata":value.metadata,"source":value.source,"conversion":value.conversion,"reused":value.reused}),
                        ),
                        Err(error) => fail(error),
                    }
                }
                YoutubeWork::Delete { video_id } => {
                    match yt::run_delete(context.paths.cache, yt::DeleteJob { video_id }).await {
                        Ok(()) => JobOutcome::Succeeded(Value::Null),
                        Err(e) => fail(e),
                    }
                }
                YoutubeWork::Usage => match yt::scan_usage(context.paths.cache).await {
                    Ok(value) => match serde_json::to_value(value) {
                        Ok(v) => JobOutcome::Succeeded(v),
                        Err(_) => JobOutcome::Failed(JobFailure::new(
                            "invalid_usage",
                            "Invalid usage snapshot",
                        )),
                    },
                    Err(e) => fail(e),
                },
                YoutubeWork::ValidateSelected { conversion } => {
                    let cancellation = yt::Cancellation::default();
                    let token = cancellation.clone();
                    let root = context.paths.cache;
                    let future = tokio::task::spawn_blocking(move || match conversion {
                        Some(c) => yt::validate_conversion_cancellable(&root, &c, &token),
                        None => Ok(true),
                    });
                    tokio::pin!(future);
                    let mut cancel = context.cancel;
                    let result = tokio::select! {
                        result=&mut future=>result,
                        changed=cancel.changed()=>{if changed.is_err()||*cancel.borrow(){cancellation.cancel();}future.await},
                    };
                    match result {
                        Ok(Ok(valid)) => JobOutcome::Succeeded(json!({"valid":valid})),
                        Ok(Err(e)) => fail(e),
                        Err(_) => JobOutcome::Failed(JobFailure::new(
                            "validation_failed",
                            "Cache validation interrupted",
                        )),
                    }
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
            let conversion = yt::YoutubeContent::conversion_from_spec(&spec)
                .map_err(|_| JobFailure::new("invalid_frame", "Invalid video frame source"))?;
            yt::read_frame(paths.cache, conversion, index, yt::ColorTransform::None)
                .await
                .map_err(|_| JobFailure::new("read_failed", "Video frame read failed"))
        })
    }
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata() -> yt::VideoMetadata {
        yt::VideoMetadata {
            id: "abcdefghijk".into(),
            url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            title: "local fixture".into(),
            channel: "fixture".into(),
            uploader: "fixture".into(),
            duration_ms: 1000,
            thumbnail: None,
            thumbnail_bytes: 0,
            updated_unix_ms: 1,
            played_unix_ms: None,
        }
    }
    fn conversion() -> yt::ConversionCache {
        let clip = yt::Clip {
            start_ms: 0,
            end_ms: 1000,
        };
        let fps = yt::Fps::default();
        yt::ConversionCache {
            key: yt::conversion_key("abcdefghijk", clip, fps),
            video_id: "abcdefghijk".into(),
            clip,
            fps,
            frames: 6,
            bytes: 600,
            created_unix_ms: 1,
        }
    }
    #[test]
    fn cancellation_preserves_completed_cache_without_switching_playback() {
        let mut content = YoutubeManaged::default();
        let ctx = CommandContext {
            now_unix: 1,
            now_mono_ms: 10,
            today_jst: "2026-10-12".into(),
            settings: Default::default(),
            devices: vec![],
        };
        let request = content
            .plan_command(&ctx, &json!({"kind":"prepare","url":metadata().url}))
            .unwrap()
            .jobs
            .remove(0);
        let generation = request.payload["generation"].as_u64().unwrap();
        content
            .plan_command(&ctx, &json!({"kind":"cancel"}))
            .unwrap();
        let job = PluginJob {
            token: JobToken {
                plugin: "youtube".into(),
                slot: "prepare".into(),
                generation: 1,
            },
            payload: request.payload,
            timeout_seconds: 30,
            created_unix: 1,
        };
        content.work_progress(&ctx,&job,&JobProgress{percent:Some(100),message:"cached".into(),data:json!({
            "generation":generation,"stage":"Ready","percent":100,"message":"cached", "metadata":metadata(),
            "source":null,"conversion":conversion(),
        })}).unwrap();
        content
            .work_completed(&ctx, &job, &JobOutcome::Cancelled)
            .unwrap();
        let saved = content.save_state();
        let mut restored = YoutubeManaged::default();
        restored.load_state(saved).unwrap();
        assert_eq!(restored.inner.videos.len(), 1);
        assert_eq!(restored.inner.videos["abcdefghijk"].conversions.len(), 1);
        assert!(restored.inner.playback.is_none());
        assert_eq!(
            restored.inner.job.as_ref().unwrap().status,
            yt::JobStatus::Cancelled
        );
    }
    #[tokio::test]
    async fn managed_api_preserves_revision_and_server_deletion_guard() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let temp = tempfile::tempdir().unwrap();
        let app = std::sync::Arc::new(std::sync::Mutex::new(
            crate::application::App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let router = crate::web::router(app.clone());
        let send = |body: Value| {
            Request::builder()
                .method("POST")
                .uri("/api/plugins/youtube/commands")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let response = router
            .clone()
            .oneshot(send(
                json!({"expected_revision":1,"command":{"kind":"prepare","url":metadata().url}}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(send(
                json!({"expected_revision":1,"command":{"kind":"stop"}}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let job = app.lock().unwrap().claim_plugin_work().unwrap().remove(0);
        app.lock()
            .unwrap()
            .complete_plugin_work(
                job.token,
                JobOutcome::Succeeded(json!({
                    "metadata":metadata(),"source":null,"conversion":conversion(),"reused":false,
                })),
            )
            .unwrap();
        let view = app.lock().unwrap().plugin_web_view("youtube").unwrap();
        let response=router.clone().oneshot(send(json!({"expected_revision":view.revision,"command":{"kind":"delete","video_id":"abcdefghijk","confirmation":"different"}}))).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/plugins/youtube")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["data"]["videos"].as_array().unwrap().len(), 1);
        assert!(value["jobs"][0].get("payload").is_none());
    }
}
