//! Application integration for opaque plugin commands and work results.

use super::*;
use crate::plugin_jobs::{
    AssetDescriptor, CommandContext, JobEffect, JobEvent, JobOutcome, JobProgress, JobStatus,
    JobToken, JobView, PlaybackSpec, PluginDevice, PluginJob, PluginPaths, PluginWorkPlan,
    RefreshTargets,
};

fn content_error(error: ContentError) -> AppError {
    match error {
        ContentError::Stale => AppError::Conflict("stale plugin state"),
        ContentError::Unavailable => AppError::Conflict("plugin unavailable"),
        ContentError::Unsupported => AppError::Invalid("unsupported plugin command"),
        ContentError::InvalidSource => AppError::Invalid("invalid plugin command"),
    }
}

#[derive(Serialize)]
pub struct PluginWebView {
    pub revision: u64,
    pub data: serde_json::Value,
    pub jobs: Vec<JobView>,
}

#[derive(Clone, Debug)]
pub struct PlaybackTarget {
    pub token: crate::DisplayToken,
}

#[derive(Clone, Debug)]
pub struct PlaybackOffer {
    pub plugin: String,
    pub spec: PlaybackSpec,
    pub targets: Vec<PlaybackTarget>,
}

impl App {
    pub fn monotonic_millis(&self) -> u64 {
        self.mono_origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub(super) fn command_context(&self) -> CommandContext {
        CommandContext {
            now_unix: self.clock.unix_seconds(),
            now_mono_ms: self.monotonic_millis(),
            today_jst: self.clock.today_jst(),
            settings: self.runtime.settings.plugins.value.clone(),
            devices: self
                .runtime
                .world
                .devices
                .iter()
                .map(|(id, d)| PluginDevice {
                    context: PluginContext {
                        token: crate::session_token(id, d),
                        settings_revision: d.settings_revision,
                        mode: d.mode.clone(),
                    },
                    selected: d.selected.clone(),
                    online: d.online,
                    capabilities: d.capabilities.clone(),
                })
                .collect(),
        }
    }

    pub fn plugin_web_view(&self, plugin: &str) -> Option<PluginWebView> {
        let content = self.contents.get(plugin)?;
        Some(PluginWebView {
            revision: self
                .runtime
                .managed_revisions
                .get(plugin)
                .copied()
                .unwrap_or(1),
            data: content.web_state_at(self.monotonic_millis()),
            jobs: self.runtime.jobs.view(plugin),
        })
    }

    pub fn plugin_paths(&self, plugin: &str) -> Option<PluginPaths> {
        self.contents.contains_key(plugin).then(|| PluginPaths {
            data: self.plugin_root.join(plugin).join("data"),
            cache: self.plugin_root.join(plugin).join("cache"),
        })
    }

    pub fn plugin_asset(&self, plugin: &str, id: &str) -> Option<(PluginPaths, AssetDescriptor)> {
        Some((
            self.plugin_paths(plugin)?,
            self.contents.get(plugin)?.asset(id)?,
        ))
    }

    pub fn work_notifications(&self) -> tokio::sync::watch::Receiver<()> {
        self.work_signal.subscribe()
    }

    pub(super) fn accept_work_plan(
        &mut self,
        plugin: &str,
        mut content: Box<dyn ContentPlugin>,
        plan: PluginWorkPlan,
        ctx: &CommandContext,
        jobs: Option<crate::plugin_jobs::JobState>,
    ) -> Result<(), AppError> {
        let targets = match plan.refresh {
            RefreshTargets::None => Vec::new(),
            RefreshTargets::Selected => self
                .runtime
                .world
                .devices
                .iter()
                .filter(|(_, d)| d.selected.as_deref() == Some(plugin))
                .map(|(id, d)| crate::DeviceToken {
                    device_id: id.clone(),
                    incarnation: d.incarnation,
                })
                .collect(),
            RefreshTargets::Devices(targets) => targets,
        };
        let mut next_jobs = jobs.unwrap_or_else(|| self.runtime.jobs.clone());
        let mut requests = plan.jobs;
        for slot in plan.cancel_slots {
            let decision = next_jobs
                .handle(JobEvent::Cancel {
                    plugin: plugin.into(),
                    slot,
                    now_unix: ctx.now_unix,
                })
                .map_err(|_| AppError::Invalid("invalid job cancellation"))?;
            next_jobs = decision.state;
            if let Some((job, outcome)) = decision.completion {
                match content.work_completed(ctx, &job, &outcome) {
                    Ok(extra) => requests.extend(extra.jobs),
                    Err(ContentError::Stale) => {}
                    Err(error) => return Err(content_error(error)),
                }
            }
        }
        for request in requests {
            next_jobs = next_jobs
                .handle(JobEvent::Submit {
                    plugin: plugin.into(),
                    request,
                    now_unix: ctx.now_unix,
                })
                .map_err(|_| AppError::Invalid("invalid plugin job"))?
                .state;
        }
        let snapshot = content.save_state();
        self.change(|engine, state| {
            state.jobs = next_jobs;
            state.plugin_states.insert(plugin.into(), snapshot);
            let revision = state.managed_revisions.entry(plugin.into()).or_insert(1);
            *revision = revision
                .checked_add(1)
                .ok_or(AppError::Conflict("plugin revision exhausted"))?;
            for target in &targets {
                let decision = engine
                    .handle(
                        &state.world,
                        &Event::PluginDataChanged {
                            device_id: target.device_id.clone(),
                            incarnation: target.incarnation,
                            plugin: plugin.into(),
                        },
                    )
                    .map_err(|_| AppError::Conflict("plugin delivery rejected"))?;
                state.accept(decision)?;
            }
            Ok(())
        })?;
        self.contents.insert(plugin.into(), content);
        self.work_signal.send_replace(());
        for target in targets {
            let Some(device) = self.runtime.world.devices.get(&target.device_id) else {
                continue;
            };
            if device.incarnation == target.incarnation
                && device.online
                && device.selected.as_deref() == Some(plugin)
            {
                let session = crate::session_token(&target.device_id, device);
                self.queue_selected_cached_frame(&session)?;
            }
        }
        Ok(())
    }

    pub fn plugin_command(
        &mut self,
        plugin: &str,
        expected_revision: u64,
        command: serde_json::Value,
    ) -> Result<PluginWebView, AppError> {
        let mut content = self.contents.get(plugin).ok_or(AppError::NotFound)?.clone();
        if self
            .runtime
            .managed_revisions
            .get(plugin)
            .copied()
            .unwrap_or(1)
            != expected_revision
        {
            return Err(AppError::Conflict("stale plugin revision"));
        }
        let ctx = self.command_context();
        let plan = content
            .plan_command(&ctx, &command)
            .map_err(content_error)?;
        self.accept_work_plan(plugin, content, plan, &ctx, None)?;
        self.plugin_web_view(plugin).ok_or(AppError::NotFound)
    }

    pub fn plan_scheduled_work(&mut self) -> Result<(), AppError> {
        let ctx = self.command_context();
        let ids: Vec<_> = self.contents.keys().cloned().collect();
        for plugin in ids {
            let mut content = self.contents[&plugin].clone();
            if let Some(plan) = content.plan_schedule(&ctx).map_err(content_error)? {
                self.accept_work_plan(&plugin, content, plan, &ctx, None)?;
            }
        }
        Ok(())
    }

    /// Claim under the state lock, persist Started, then run these effects
    /// after releasing it. Each slot waits for cancelled predecessors to exit.
    pub fn claim_plugin_work(&mut self) -> Result<Vec<PluginJob>, AppError> {
        let ready = self.runtime.jobs.ready();
        if ready.is_empty() {
            return Ok(Vec::new());
        }
        self.change(|_, state| {
            let mut jobs = Vec::new();
            for job in ready {
                let decision = state
                    .jobs
                    .handle(JobEvent::Started(job.token))
                    .map_err(|_| AppError::Conflict("job claim rejected"))?;
                for effect in decision.effects {
                    if let JobEffect::Run(job) = effect {
                        jobs.push(job);
                    }
                }
                state.jobs = decision.state;
            }
            Ok(jobs)
        })
    }

    pub fn plugin_work_cancelled(&self, token: &JobToken) -> bool {
        !self.runtime.jobs.current(token)
            || self
                .runtime
                .jobs
                .jobs
                .get(&token.generation)
                .is_none_or(|r| r.status == JobStatus::Cancelling || r.status.finished())
    }

    pub fn plugin_work_progress(
        &mut self,
        token: JobToken,
        progress: JobProgress,
    ) -> Result<bool, AppError> {
        let decision = self
            .runtime
            .jobs
            .handle(JobEvent::Progress {
                token: token.clone(),
                progress: progress.clone(),
            })
            .map_err(|_| AppError::Invalid("invalid job progress"))?;
        if !decision.applied {
            return Ok(false);
        }
        if let Some(record) = decision.state.jobs.get(&token.generation) {
            if decision.state.current(&token) {
                let ctx = self.command_context();
                let mut content = self
                    .contents
                    .get(&token.plugin)
                    .ok_or(AppError::NotFound)?
                    .clone();
                if let Some(plan) = content
                    .work_progress(&ctx, &record.job, &progress)
                    .map_err(content_error)?
                {
                    self.accept_work_plan(
                        &token.plugin,
                        content,
                        plan,
                        &ctx,
                        Some(decision.state),
                    )?;
                    return Ok(true);
                }
            }
        }
        self.change(|_, state| {
            state.jobs = decision.state;
            Ok(true)
        })
    }

    pub fn complete_plugin_work(
        &mut self,
        token: JobToken,
        outcome: JobOutcome,
    ) -> Result<bool, AppError> {
        let ctx = self.command_context();
        let decision = self
            .runtime
            .jobs
            .handle(JobEvent::Finished {
                token,
                outcome,
                now_unix: ctx.now_unix,
            })
            .map_err(|_| AppError::Conflict("job result rejected"))?;
        if !decision.applied {
            return Ok(false);
        }
        if let Some((job, result)) = decision.completion {
            let mut content = self
                .contents
                .get(&job.token.plugin)
                .ok_or(AppError::NotFound)?
                .clone();
            let plan = content
                .work_completed(&ctx, &job, &result)
                .map_err(content_error)?;
            self.accept_work_plan(&job.token.plugin, content, plan, &ctx, Some(decision.state))?;
            Ok(true)
        } else {
            self.change(|_, state| {
                state.jobs = decision.state;
                Ok(())
            })?;
            self.work_signal.send_replace(());
            Ok(false)
        }
    }

    pub fn cancel_all_plugin_work(&mut self) -> Result<(), AppError> {
        let now_unix = self.clock.unix_seconds();
        let tokens: Vec<_> = self
            .runtime
            .jobs
            .jobs
            .values()
            .filter(|r| r.status == JobStatus::Queued || r.status == JobStatus::Running)
            .map(|r| r.job.token.clone())
            .collect();
        if tokens.is_empty() {
            return Ok(());
        }
        self.change(|_, state| {
            for token in tokens {
                let decision = state
                    .jobs
                    .handle(JobEvent::Cancel {
                        plugin: token.plugin,
                        slot: token.slot,
                        now_unix,
                    })
                    .map_err(|_| AppError::Conflict("job cancellation rejected"))?;
                state.jobs = decision.state;
            }
            Ok(())
        })?;
        self.work_signal.send_replace(());
        Ok(())
    }

    /// Plugin-chosen playback sources for online selected LCD viewers. Pixel
    /// reads happen outside the lock; returned display tokens gate completion.
    pub fn playback_offers(&self) -> Vec<PlaybackOffer> {
        let mut offers: Vec<PlaybackOffer> = Vec::new();
        for (id, device) in &self.runtime.world.devices {
            if !device.online
                || device.owner != Owner::Remote
                || !matches!(device.mode, Mode::Normal | Mode::Sub)
                || (!device.capabilities.contains("lcd") && !device.capabilities.contains("eink"))
                || !self.streaming_ready(device)
            {
                continue;
            }
            let Some(plugin) = &device.selected else {
                continue;
            };
            let ctx = PluginContext {
                token: crate::session_token(id, device),
                settings_revision: device.settings_revision,
                mode: device.mode.clone(),
            };
            let Some(spec) = self
                .contents
                .get(plugin)
                .and_then(|p| p.playback(&ctx))
                .filter(|s| s.valid())
            else {
                continue;
            };
            let target = PlaybackTarget {
                token: crate::display_token(id, device),
            };
            if let Some(offer) = offers
                .iter_mut()
                .find(|o| o.plugin == *plugin && o.spec == spec)
            {
                offer.targets.push(target);
            } else {
                offers.push(PlaybackOffer {
                    plugin: plugin.clone(),
                    spec,
                    targets: vec![target],
                });
            }
        }
        offers
    }

    fn streaming_ready(&self, device: &Device) -> bool {
        device.pending_frame_id.is_none_or(|id| {
            if device.capabilities.contains("eink") && !device.capabilities.contains("lcd") {
                device.last_rendered_frame == Some(id)
            } else {
                device.last_sent_frame == Some(id)
            }
        })
    }
    fn queue_streaming_frame(
        &mut self,
        session: &SessionToken,
        plugin: &str,
        pixels: Vec<u8>,
    ) -> Result<FrameToken, AppError> {
        let handle = format!(
            "lcd:{}:{}",
            session.device_id, self.runtime.world.next_frame_id
        );
        let frame = self.change(|engine, state| {
            let device = state
                .world
                .devices
                .get(&session.device_id)
                .ok_or(AppError::NotFound)?;
            if !session_current(device, session) {
                return Err(AppError::Conflict("stale streaming session"));
            }
            let decision = engine
                .handle(
                    &state.world,
                    &Event::RequestStreamingFrame {
                        token: crate::display_token(&session.device_id, device),
                        plugin: plugin.into(),
                        asset_handle: handle.clone(),
                    },
                )
                .map_err(|_| AppError::Conflict("streaming frame rejected"))?;
            let frame = decision
                .effects
                .iter()
                .find_map(|e| {
                    if let Effect::SendFrame { token, .. } = e {
                        Some(token.clone())
                    } else {
                        None
                    }
                })
                .ok_or(AppError::Conflict("streaming frame not ready"))?;
            state.accept(decision)?;
            Ok(frame)
        })?;
        let pixels = self.compose_lcd_frame(&session.device_id, pixels);
        self.frame_assets.insert(handle, Arc::new(pixels));
        Ok(frame)
    }
    pub fn accept_playback_pixels(
        &mut self,
        offer: &PlaybackOffer,
        pixels: Vec<u8>,
    ) -> Result<usize, AppError> {
        if pixels.len() != crate::protocol::LCD_FRAME_BYTES {
            return Err(AppError::Invalid("invalid playback frame"));
        }
        let mut delivered = 0;
        for target in &offer.targets {
            let Some(device) = self.runtime.world.devices.get(&target.token.device_id) else {
                continue;
            };
            if !crate::frame_display_current(device, &target.token)
                || !self.streaming_ready(device)
                || device.selected.as_deref() != Some(&offer.plugin)
            {
                continue;
            }
            let ctx = PluginContext {
                token: crate::session_token(&target.token.device_id, device),
                settings_revision: device.settings_revision,
                mode: device.mode.clone(),
            };
            if self
                .contents
                .get(&offer.plugin)
                .and_then(|p| p.playback(&ctx))
                .as_ref()
                != Some(&offer.spec)
            {
                continue;
            }
            self.queue_streaming_frame(&ctx.token, &offer.plugin, pixels.clone())?;
            delivered += 1;
        }
        Ok(delivered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Default)]
    struct FakeContent {
        value: u64,
    }
    impl ContentPlugin for FakeContent {
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
            Ok(())
        }
        fn position(&self, _: &PluginContext) -> Option<PluginPosition> {
            None
        }
        fn plan(
            &mut self,
            _: &PluginContext,
            _: ButtonAction,
        ) -> Result<crate::content::PluginPlan, ContentError> {
            Err(ContentError::Unsupported)
        }
        fn session_reset(&mut self, _: &str, _: u64) {}
        fn delete_device(&mut self, _: &str) {}
        fn save_state(&self) -> serde_json::Value {
            serde_json::json!({"value": self.value})
        }
        fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
            self.value = value["value"].as_u64().ok_or(ContentError::InvalidSource)?;
            Ok(())
        }
        fn web_state(&self) -> serde_json::Value {
            self.save_state()
        }
        fn plan_command(
            &mut self,
            _: &CommandContext,
            _: &serde_json::Value,
        ) -> Result<PluginWorkPlan, ContentError> {
            Ok(PluginWorkPlan {
                jobs: vec![crate::plugin_jobs::JobRequest {
                    slot: "prepare".into(),
                    payload: serde_json::json!({"value": self.value+1}),
                    timeout_seconds: 30,
                }],
                ..Default::default()
            })
        }
        fn playback(&self, _: &PluginContext) -> Option<PlaybackSpec> {
            Some(PlaybackSpec {
                asset_id: "fixture".into(),
                generation: self.value + 1,
                fps: 6,
                frame_count: 60,
                started_mono_ms: 0,
                source: serde_json::Value::Null,
            })
        }
        fn work_completed(
            &mut self,
            _: &CommandContext,
            _: &PluginJob,
            result: &JobOutcome,
        ) -> Result<PluginWorkPlan, ContentError> {
            if let JobOutcome::Succeeded(value) = result {
                self.value = value["value"].as_u64().ok_or(ContentError::InvalidSource)?;
            }
            Ok(PluginWorkPlan::default())
        }
    }
    #[test]
    fn durable_generation_rejects_replaced_results_and_preserves_revision() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
        app.contents
            .insert("youtube".into(), Box::new(FakeContent::default()));
        app.plugin_command("youtube", 1, serde_json::json!({}))
            .unwrap();
        let old = app.claim_plugin_work().unwrap().remove(0);
        app.plugin_command("youtube", 2, serde_json::json!({}))
            .unwrap();
        assert!(app.claim_plugin_work().unwrap().is_empty());
        assert!(!app
            .complete_plugin_work(
                old.token,
                JobOutcome::Succeeded(serde_json::json!({"value":99}))
            )
            .unwrap());
        assert_eq!(app.plugin_web_view("youtube").unwrap().data["value"], 0);
        let fresh = app.claim_plugin_work().unwrap().remove(0);
        assert!(app
            .complete_plugin_work(
                fresh.token.clone(),
                JobOutcome::Succeeded(serde_json::json!({"value":7}))
            )
            .unwrap());
        assert!(!app
            .complete_plugin_work(
                fresh.token,
                JobOutcome::Succeeded(serde_json::json!({"value":9}))
            )
            .unwrap());
        assert_eq!(app.plugin_web_view("youtube").unwrap().data["value"], 7);
        assert!(app
            .plugin_command("youtube", 1, serde_json::json!({}))
            .is_err());
        drop(app);
        let connection = Connection::open(&path).unwrap();
        let body: String = connection
            .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let restored =
            Runtime::try_from(serde_json::from_str::<StoredState>(&body).unwrap()).unwrap();
        assert_eq!(restored.plugin_states["youtube"]["value"], 7);
        assert_eq!(restored.managed_revisions["youtube"], 4);
    }
    #[test]
    fn streaming_waits_for_complete_send_and_rejects_old_acks_and_read_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        app.contents
            .insert("youtube".into(), Box::new(FakeContent::default()));
        let session = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let offer = app.playback_offers().remove(0);
        assert_eq!(
            app.accept_playback_pixels(&offer, vec![1; crate::protocol::LCD_FRAME_BYTES])
                .unwrap(),
            1
        );
        let (effect, old) = app
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| {
                if let Effect::SendFrame { token, .. } = e {
                    Some((id, token))
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            app.playback_offers().is_empty(),
            "must not replace fragments still being written"
        );
        app.complete_effect(effect).unwrap();
        let offer = app.playback_offers().remove(0);
        assert_eq!(
            app.accept_playback_pixels(&offer, vec![2; crate::protocol::LCD_FRAME_BYTES])
                .unwrap(),
            1
        );
        assert!(!app
            .terminal_event(Event::RenderedAck { token: old })
            .unwrap());
        let (effect, new) = app
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| {
                if let Effect::SendFrame { token, .. } = e {
                    Some((id, token))
                } else {
                    None
                }
            })
            .unwrap();
        app.complete_effect(effect).unwrap();
        assert!(app
            .terminal_event(Event::RenderedAck { token: new })
            .unwrap());
        let old_read = app.playback_offers().remove(0);
        app.contents
            .insert("youtube".into(), Box::new(FakeContent { value: 1 }));
        assert_eq!(
            app.accept_playback_pixels(&old_read, vec![3; crate::protocol::LCD_FRAME_BYTES])
                .unwrap(),
            0
        );
        let old_read = app.playback_offers().remove(0);
        app.button(&session, ButtonAction::Next).unwrap();
        assert_eq!(
            app.accept_playback_pixels(&old_read, vec![3; crate::protocol::LCD_FRAME_BYTES])
                .unwrap(),
            0
        );
    }

    #[test]
    fn streaming_badge_is_per_device_and_disappears_after_sub_mode() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        // Supply a streaming test fixture under a plugin whose kernel contract
        // supports Sub. This exercises the shared streaming delivery path.
        app.contents
            .insert("caricature".into(), Box::new(FakeContent::default()));
        let sub = app.hello("sub", ["lcd".into()].into(), 1).unwrap();
        let normal = app.hello("normal", ["lcd".into()].into(), 1).unwrap();
        app.button(&sub, ButtonAction::Next).unwrap();
        app.button(&normal, ButtonAction::Next).unwrap();
        app.terminal_event(Event::EnterSub { token: sub.clone() })
            .unwrap();
        let original = vec![0x42; crate::protocol::LCD_FRAME_BYTES];
        let offer = app.playback_offers().remove(0);
        assert_eq!(
            app.accept_playback_pixels(&offer, original.clone())
                .unwrap(),
            2
        );
        let frames: BTreeMap<_, _> = app
            .pending_effects()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } => Some((token.device_id, app.frame_asset(&asset_handle).unwrap())),
                _ => None,
            })
            .collect();
        assert_eq!(frames["normal"].as_ref(), &original);
        assert_eq!(&frames["sub"][0..2], &[0x42, 0x42]);
        assert_eq!(
            &frames["sub"][(69 * 160 + 3) * 2..(69 * 160 + 3) * 2 + 2],
            &[255, 255]
        );

        app.terminal_event(Event::Power { token: sub.clone() })
            .unwrap();
        let offer = app.playback_offers().remove(0);
        assert_eq!(
            app.accept_playback_pixels(&offer, original.clone())
                .unwrap(),
            1
        );
        let normal_again = app
            .pending_effects()
            .into_iter()
            .find_map(|(_, effect)| match effect {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } if token.device_id == sub.device_id && app.frame_is_current(&token) => {
                    app.frame_asset(&asset_handle)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(normal_again.as_ref(), &original);
        assert_eq!(
            app.selection_for(&normal).unwrap().0.as_deref(),
            Some("caricature")
        );
    }
    #[tokio::test]
    async fn long_job_does_not_hold_state_or_stop_jma_heartbeat_and_other_viewer() {
        use crate::plugin_jobs::{JobExecutionContext, PluginJobExecutor, PluginPaths};
        use futures_util::future::BoxFuture;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;
        use tokio::sync::{watch, Notify};
        struct FixedClock;
        impl crate::fetch::Clock for FixedClock {
            fn unix_seconds(&self) -> i64 {
                1_760_256_000
            }
            fn today_jst(&self) -> String {
                "2026-10-12".into()
            }
        }
        struct Executor {
            entered: Arc<AtomicBool>,
            release: Arc<Notify>,
        }
        impl PluginJobExecutor for Executor {
            fn execute(
                &self,
                _: serde_json::Value,
                mut context: JobExecutionContext,
            ) -> BoxFuture<'static, JobOutcome> {
                let entered = self.entered.clone();
                let release = self.release.clone();
                Box::pin(async move {
                    entered.store(true, Ordering::SeqCst);
                    tokio::select! {_=release.notified()=>JobOutcome::Succeeded(serde_json::json!({"value":3})),_=context.cancelled()=>JobOutcome::Cancelled}
                })
            }
            fn read_frame(
                &self,
                _: PlaybackSpec,
                index: u64,
                _: PluginPaths,
            ) -> BoxFuture<'static, Result<Vec<u8>, crate::plugin_jobs::JobFailure>> {
                Box::pin(async move { Ok(vec![index as u8; crate::protocol::LCD_FRAME_BYTES]) })
            }
        }
        struct Fetcher;
        impl crate::fetch::SourceFetcher for Fetcher {
            fn fetch(&mut self, job: &FetchJob) -> Result<Vec<u8>, String> {
                match job.plugin.as_str() {
                    "weather" => Ok(include_bytes!("../assets/jma-tokyo-20261012.json").to_vec()),
                    "rain-radar" => {
                        Ok(include_bytes!("../assets/jma-radar-20261012.json").to_vec())
                    }
                    _ => Err("unexpected source".into()),
                }
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let mut state =
            App::open_with_clock(&temp.path().join("state.db"), Arc::new(FixedClock)).unwrap();
        state
            .contents
            .insert("youtube".into(), Box::new(FakeContent::default()));
        state.contents.remove("caricature");
        let fast = state.hello("fast", ["lcd".into()].into(), 1).unwrap();
        let _slow = state.hello("slow", ["lcd".into()].into(), 1).unwrap();
        state
            .plugin_command("youtube", 1, serde_json::json!({}))
            .unwrap();
        let app = Arc::new(std::sync::Mutex::new(state));
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(Notify::new());
        let executors = std::collections::BTreeMap::from([(
            "youtube".into(),
            Arc::new(Executor {
                entered: entered.clone(),
                release: release.clone(),
            }) as Arc<dyn PluginJobExecutor>,
        )]);
        let (stop, receiver) = watch::channel(false);
        let worker = tokio::spawn(crate::workers::run_work(
            app.clone(),
            executors.clone(),
            receiver.clone(),
        ));
        let playback = tokio::spawn(crate::playback::run_playback(
            app.clone(),
            executors,
            receiver,
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut frames = Vec::new();
        for _ in 0..4 {
            let frame = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let next = {
                        let mut guard = app.lock().unwrap();
                        guard.heartbeat(&fast, 1).unwrap();
                        let next = guard.pending_effects().into_iter().find_map(|(id, e)| {
                            if let Effect::SendFrame { token, .. } = e {
                                (token.device_id == "fast").then_some((id, token))
                            } else {
                                None
                            }
                        });
                        if let Some((effect, token)) = next {
                            guard.complete_effect(effect).unwrap();
                            Some(token.frame_id)
                        } else {
                            None
                        }
                    };
                    if let Some(frame) = next {
                        break frame;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            frames.push(frame);
        }
        assert!(frames.windows(2).all(|ids| ids[0] < ids[1]));
        let slow_count = app
            .lock()
            .unwrap()
            .pending_effects()
            .into_iter()
            .filter(|(_, e)| matches!(e,Effect::SendFrame{token,..}if token.device_id=="slow"))
            .count();
        assert_eq!(
            slow_count, 1,
            "a non-sending terminal must retain one bounded pending image"
        );
        let report = {
            let mut guard = app.lock().unwrap();
            crate::fetch::run_cycle(&mut guard, &FixedClock, &mut Fetcher).unwrap()
        };
        assert!(report.applied >= 2);
        assert_eq!(
            app.lock().unwrap().plugin_web_view("youtube").unwrap().data["value"],
            0,
            "long job remains unfinished"
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            while app.lock().unwrap().plugin_web_view("youtube").unwrap().data["value"] != 3 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        worker.await.unwrap().unwrap();
        playback.await.unwrap().unwrap();
    }
}
