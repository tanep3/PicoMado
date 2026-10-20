use super::*;
use crate::content::{ContentError, PluginPlan};

struct Rules(&'static str);
impl crate::Plugin for Rules {
    fn id(&self) -> &str {
        self.0
    }
    fn supports(&self, caps: &BTreeSet<String>) -> bool {
        caps.contains("lcd")
    }
    fn candidates<'a>(
        &'a self,
        _q: &'a dyn crate::WorldQuery,
        _e: &'a Event,
    ) -> Box<dyn Iterator<Item = crate::Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}
#[derive(Clone)]
struct Content {
    id: &'static str,
    video: bool,
}
impl ContentPlugin for Content {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        self.id
    }
    fn publish(
        &mut self,
        _ctx: &PluginContext,
        _source: &[u8],
        _id: &str,
        _day: &str,
    ) -> Result<(), ContentError> {
        Ok(())
    }
    fn position(&self, _ctx: &PluginContext) -> Option<PluginPosition> {
        None
    }
    fn plan(
        &mut self,
        _ctx: &PluginContext,
        _action: ButtonAction,
    ) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn session_reset(&mut self, _id: &str, _inc: u64) {}
    fn delete_device(&mut self, _id: &str) {}
    fn current_frame(&self, _ctx: &PluginContext) -> Option<Vec<u8>> {
        (!self.video).then(|| vec![7; 25600])
    }
    fn playback(&self, _ctx: &PluginContext) -> Option<crate::plugin_jobs::PlaybackSpec> {
        self.video.then(|| crate::plugin_jobs::PlaybackSpec {
            asset_id: "fixture".into(),
            generation: 1,
            fps: 6,
            frame_count: 20,
            started_mono_ms: 0,
            source: serde_json::Value::Null,
        })
    }
}
fn composition() -> crate::plugins::PluginSet {
    let mut set = crate::plugins::PluginSet::default();
    for (id, video) in [
        ("fixture-static", false),
        ("fixture-video", true),
        ("extension", false),
    ] {
        set.register(Box::new(Rules(id)), Box::new(Content { id, video }))
            .unwrap();
    }
    set
}
#[test]
fn new_static_and_video_plugins_use_unchanged_application_and_transport_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let mut app = App::open_with_plugin_set(
        &db,
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        composition(),
    )
    .unwrap();
    let caps = [
        "lcd",
        crate::protocol::FRAME_BYTES_CAPABILITY,
        crate::protocol::LCD_PUSH_CAPABILITY,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let token = app.hello("fixture-device", caps, 1).unwrap();
    assert_eq!(
        app.view().devices[0].selected.as_deref(),
        Some("fixture-static")
    );
    app.queue_selected_cached_frame(&token).unwrap();
    assert!(app
        .pending_effects()
        .iter()
        .any(|(_, e)| matches!(e, Effect::SendFrame { .. })));
    app.button(&token, ButtonAction::Down).unwrap();
    assert_eq!(app.playback_offers()[0].plugin, "fixture-video");
    let rx = app.subscribe_lcd_video(&token).unwrap();
    app.queue_lcd_video(&token, 6, 0, 1, vec![3; 25600])
        .unwrap();
    let old = rx.borrow().as_ref().unwrap().clone();
    assert!(app.lcd_video_is_current(&old));
    app.button(&token, ButtonAction::Down).unwrap();
    assert_eq!(app.view().devices[0].selected.as_deref(), Some("extension"));
    assert!(!app.lcd_video_is_current(&old));
    drop(app);
    let app = App::open_with_plugin_set(
        &db,
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        composition(),
    )
    .unwrap();
    assert_eq!(app.view().devices[0].selected.as_deref(), Some("extension"));
}
#[cfg(feature = "boundary-observation")]
#[test]
fn full_observer_preserves_selection_and_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::open(&temp.path().join("state.db")).unwrap();
    let token = app
        .hello("fixture-device", ["lcd".into()].into(), 1)
        .unwrap();
    let source = app.observation_source();
    source.enable(true);
    for _ in 0..100 {
        app.button(&token, ButtonAction::Down).unwrap();
    }
    assert!(source.status().dropped > 0);
    let expected = app.view().devices[0].selected.clone();
    drop(app);
    let restored = App::open(&temp.path().join("state.db")).unwrap();
    assert_eq!(restored.view().devices[0].selected, expected);
}

#[cfg(feature = "boundary-observation")]
#[test]
fn causes_stay_with_exact_frame_and_view_across_overlapping_inputs() {
    use crate::observation::Cause;
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::open_with_plugin_set(
        &temp.path().join("db"),
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        composition(),
    )
    .unwrap();
    let token = app.hello("fixture", ["lcd".into()].into(), 42).unwrap();
    let initial = app
        .pending_effects()
        .into_iter()
        .find_map(|(_, e)| {
            if let Effect::SendFrame { token, .. } = e {
                Some(token)
            } else {
                None
            }
        })
        .unwrap();
    let a = Cause {
        version: 1,
        boot: 101,
        operation: 201,
    };
    let b = Cause {
        version: 1,
        boot: 101,
        operation: 202,
    };
    app.with_observation_cause("fixture", Some(a), |app| {
        app.button(&token, ButtonAction::Down)
    })
    .unwrap();
    // Video starts asynchronously after the accepted view change.
    app.queue_lcd_frame(&token, "fixture-video", vec![3; 25600])
        .unwrap();
    let frame_a = app
        .pending_effects()
        .into_iter()
        .find_map(|(_, e)| {
            if let Effect::SendFrame { token, .. } = e {
                Some(token)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(app.frame_observation_cause(&frame_a), Some(a));
    let mut transfer = crate::lcd_stream::VideoTransfer::new_observed(
        Arc::new(crate::lcd_stream::VideoFrame {
            token: frame_a.clone(),
            fps: 6,
            sequence: 1,
            generation: 1,
            mode: "normal",
            pixels: Arc::new(vec![3; 25600]),
        }),
        Some(a),
    );
    app.with_observation_cause("fixture", Some(b), |app| {
        app.button(&token, ButtonAction::Down)
    })
    .unwrap();
    let metadata = crate::protocol::Decoder::default()
        .push_observed(&transfer.next_packet().unwrap().unwrap())
        .unwrap();
    assert_eq!(
        metadata[0].cause,
        Some(a),
        "in-flight video retains its immutable cause"
    );
    let frame_b = app
        .pending_effects()
        .into_iter()
        .find_map(|(_, e)| {
            if let Effect::SendFrame { token, .. } = e {
                Some(token)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(app.frame_observation_cause(&frame_b), Some(b));
    assert_eq!(app.frame_observation_cause(&frame_a), Some(a));
    assert_eq!(app.frame_observation_cause(&initial), None);
    let unknown = FrameToken {
        frame_id: u64::MAX,
        display_epoch: u64::MAX,
        ..frame_b.clone()
    };
    assert_eq!(app.frame_observation_cause(&unknown), None);
}

#[derive(Clone)]
struct BlockingFetchContent {
    started: std::sync::mpsc::SyncSender<()>,
    release: Arc<std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
    generation: u64,
    applied: bool,
}
impl ContentPlugin for BlockingFetchContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "fixture-block"
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
    fn save_state(&self) -> serde_json::Value {
        serde_json::json!({"generation":self.generation,"applied":self.applied})
    }
    fn fetch_preparation(
        &self,
        _: &FetchJob,
    ) -> Result<crate::content::FetchPreparation, ContentError> {
        let started = self.started.clone();
        let release = self.release.clone();
        Ok(Box::new(move |source| {
            assert!(source.is_ok());
            started.send(()).unwrap();
            let wait = release.lock().unwrap().take().unwrap();
            wait.recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| ContentError::Unavailable)?;
            Ok(Box::new(7u64))
        }))
    }
    fn apply_prepared_fetch(
        &mut self,
        job: &FetchJob,
        _: i64,
        prepared: crate::content::PreparedFetch,
    ) -> Result<Option<Vec<u8>>, ContentError> {
        if job.generation != self.generation {
            return Err(ContentError::Stale);
        }
        assert_eq!(*prepared.downcast::<u64>().unwrap(), 7);
        self.applied = true;
        Ok(None)
    }
    fn cancel_fetches(&mut self) {
        self.generation += 1;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paused_fetch_preparation_allows_other_input_video_and_revalidates_latest_state() {
    for mutation in ["none", "settings", "delete", "cancel"] {
        let temp = tempfile::tempdir().unwrap();
        let (started, wait) = std::sync::mpsc::sync_channel(1);
        let (release, permit) = std::sync::mpsc::sync_channel(1);
        let permit = Arc::new(std::sync::Mutex::new(Some(permit)));
        let mut plugins = crate::plugins::PluginSet::default();
        plugins
            .register(
                Box::new(Rules("fixture-block")),
                Box::new(BlockingFetchContent {
                    started,
                    release: permit,
                    generation: 1,
                    applied: false,
                }),
            )
            .unwrap();
        plugins
            .register(
                Box::new(Rules("fixture-video")),
                Box::new(Content {
                    id: "fixture-video",
                    video: true,
                }),
            )
            .unwrap();
        let mut state = App::open_with_plugin_set(
            &temp.path().join("db"),
            temp.path(),
            Arc::new(crate::fetch::SystemClock),
            plugins,
        )
        .unwrap();
        let caps = [
            "lcd",
            crate::protocol::FRAME_BYTES_CAPABILITY,
            crate::protocol::LCD_PUSH_CAPABILITY,
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let a = state.hello("a", caps, 1).unwrap();
        let b = state
            .hello(
                "b",
                [
                    "lcd",
                    crate::protocol::FRAME_BYTES_CAPABILITY,
                    crate::protocol::LCD_PUSH_CAPABILITY,
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                1,
            )
            .unwrap();
        let job = FetchJob {
            plugin: "fixture-block".into(),
            device_id: "a".into(),
            incarnation: a.incarnation,
            session_epoch: a.session_epoch,
            settings_revision: state.runtime.world.devices["a"].settings_revision,
            config_revision: 1,
            generation: 1,
            source_key: "offline".into(),
            dataset_id: "fixture".into(),
            today_jst: "2026-10-20".into(),
            timeout_seconds: 3,
            retries: 0,
        };
        let app = Arc::new(crate::shared::Application::new(state));
        let worker_app = app.clone();
        let task = tokio::spawn(async move {
            crate::fetch::complete_fetched_source(&worker_app, &job, 100, Ok(vec![])).await
        });
        tokio::task::spawn_blocking(move || {
            wait.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        })
        .await
        .unwrap();
        {
            let mut guard = app
                .try_lock()
                .expect("prepare must hold no shared App lock");
            guard.button(&b, ButtonAction::Down).unwrap();
            let video = guard.subscribe_lcd_video(&b).unwrap();
            guard.queue_lcd_video(&b, 6, 1, 1, vec![3; 25600]).unwrap();
            let frame = video.borrow().clone().unwrap();
            let mut transfer = crate::lcd_stream::VideoTransfer::new(frame);
            assert!(transfer.next_packet().unwrap().unwrap().len() > 6);
            match mutation {
                "settings" => {
                    let rev = guard
                        .view()
                        .devices
                        .iter()
                        .find(|d| d.id == "a")
                        .unwrap()
                        .enabled_revision;
                    guard
                        .set_enabled("a", a.incarnation, rev, "fixture-block", false)
                        .unwrap();
                }
                "delete" => guard.delete_device("a", a.incarnation).unwrap(),
                "cancel" => guard.cancel_plugin_fetches().unwrap(),
                _ => {}
            }
        }
        release.send(()).unwrap();
        let applied = tokio::time::timeout(std::time::Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(applied, mutation == "none", "mutation={mutation}");
        assert_eq!(
            app.lock()
                .unwrap()
                .view()
                .devices
                .iter()
                .find(|d| d.id == "b")
                .unwrap()
                .selected
                .as_deref(),
            Some("fixture-video")
        );
    }
}

struct ReviewClock;
impl crate::fetch::Clock for ReviewClock {
    fn unix_seconds(&self) -> i64 {
        1791763200
    }
    fn today_jst(&self) -> String {
        "2026-10-12".into()
    }
}
fn select_review_plugin(app: &mut App, token: &SessionToken, plugin: &str) {
    for _ in 0..10 {
        if app
            .view()
            .devices
            .iter()
            .find(|d| d.id == token.device_id)
            .unwrap()
            .selected
            .as_deref()
            == Some(plugin)
        {
            return;
        }
        app.button(token, ButtonAction::Down).unwrap();
    }
    panic!("plugin not selectable");
}
fn acknowledge_review_frames(app: &mut App, device: &str) {
    for _ in 0..10 {
        let frame = app
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| match e {
                Effect::SendFrame { token, .. } if token.device_id == device => Some((id, token)),
                _ => None,
            });
        let Some((id, token)) = frame else {
            return;
        };
        app.complete_effect(id).unwrap();
        app.terminal_event(Event::ReceiptAck {
            token: token.clone(),
        })
        .unwrap();
        app.terminal_event(Event::RenderedAck { token }).unwrap();
    }
    panic!("unexpected continuing frame production");
}
#[test]
fn prepared_builtin_fetch_is_rejected_after_reconnect_and_fresh_job_can_start() {
    for (plugin, bytes) in [
        (
            "weather",
            include_bytes!("../assets/jma-tokyo-20261012.json").as_slice(),
        ),
        (
            "rain-radar",
            include_bytes!("../assets/jma-radar-20261012.json").as_slice(),
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open_with_clock(&temp.path().join("db"), Arc::new(ReviewClock)).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut app, &token, plugin);
        let job = app
            .plan_plugin_fetches(1791763200, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == plugin)
            .unwrap();
        let prepared =
            app.prepare_plugin_fetch(&job).unwrap().unwrap()(Ok(bytes.to_vec())).unwrap();
        let next = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        assert!(next.session_epoch > token.session_epoch);
        assert!(!app
            .apply_prepared_plugin_fetch(&job, 1791763201, prepared)
            .unwrap());
        assert!(app.prepare_plugin_fetch(&job).unwrap().is_none());
        let fresh = app
            .plan_plugin_fetches(1791763201, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == plugin)
            .unwrap();
        assert_eq!(fresh.session_epoch, next.session_epoch);
        assert!(fresh.generation > job.generation);
    }
}
#[test]
fn native_weather_completion_selects_prepared_pixels_without_full_render_under_app_lock() {
    for sub in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut state =
            App::open_with_clock(&temp.path().join("db"), Arc::new(ReviewClock)).unwrap();
        let token = state.hello("paper", ["eink".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &token, "weather");
        acknowledge_review_frames(&mut state, "paper");
        let source = include_bytes!("../assets/jma-tokyo-20261012.json");
        let first = state
            .plan_plugin_fetches(1791763200, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        crate::weather::take_full_render_calls();
        let prepared =
            state.prepare_plugin_fetch(&first).unwrap().unwrap()(Ok(source.to_vec())).unwrap();
        let counts = crate::weather::take_full_render_calls();
        assert!(counts.0 >= 7 && counts.1 >= 7);
        let app = Arc::new(std::sync::Mutex::new(state));
        {
            let mut guard = app.lock().unwrap();
            assert!(guard
                .apply_prepared_plugin_fetch(&first, 1791763201, prepared)
                .unwrap());
            assert_eq!(crate::weather::take_full_render_calls(), (0, 0));
            let (_, handle) = guard
                .pending_effects()
                .into_iter()
                .find_map(|(_, e)| match e {
                    Effect::SendFrame {
                        token,
                        asset_handle,
                    } if token.device_id == "paper" => Some((token, asset_handle)),
                    _ => None,
                })
                .unwrap();
            let pixels = guard.frame_asset(&handle).unwrap();
            assert_eq!(pixels.len(), 3813);
            let set = crate::weather::ForecastSet::tokyo_from_cache(
                source,
                &first.dataset_id,
                "2026-10-12",
            )
            .unwrap();
            assert!(pixels.as_slice()==crate::weather::render_eink(&set,&set.days[0]).unwrap().as_slice(),"cached native pixels must match renderer");
            acknowledge_review_frames(&mut guard, "paper");
            if !sub {
                continue;
            }
            guard.button(&token, ButtonAction::Sub).unwrap();
            let before = guard
                .pending_effects()
                .into_iter()
                .find_map(|(_, e)| match e {
                    Effect::SendFrame {
                        token,
                        asset_handle,
                    } if token.device_id == "paper" => guard.frame_asset(&asset_handle),
                    _ => None,
                })
                .unwrap();
            acknowledge_review_frames(&mut guard, "paper");
            let next = guard
                .plan_plugin_fetches(1791766802, "2026-10-12")
                .unwrap()
                .into_iter()
                .find(|j| j.plugin == "weather")
                .unwrap();
            drop(guard);
            let prepare = {
                app.lock()
                    .unwrap()
                    .prepare_plugin_fetch(&next)
                    .unwrap()
                    .unwrap()
            };
            let prepared = prepare(Ok(source.to_vec())).unwrap();
            crate::weather::take_full_render_calls();
            let mut guard = app.lock().unwrap();
            assert!(guard
                .apply_prepared_plugin_fetch(&next, 1791766803, prepared)
                .unwrap());
            assert_eq!(crate::weather::take_full_render_calls(), (0, 0));
            let after = guard
                .pending_effects()
                .into_iter()
                .find_map(|(_, e)| match e {
                    Effect::SendFrame {
                        token,
                        asset_handle,
                    } if token.device_id == "paper" => guard.frame_asset(&asset_handle),
                    _ => None,
                })
                .unwrap();
            assert!(before==after,"fetch completion changed pinned native pixels");
            let d = &guard.runtime.world.devices["paper"];
            let ctx = PluginContext {
                token: token.clone(),
                settings_revision: d.settings_revision,
                mode: d.mode.clone(),
            };
            assert_eq!(
                guard.contents["weather"].position(&ctx).unwrap().dataset_id,
                first.dataset_id,
                "new latest must not replace the pinned snapshot"
            );
        }
    }
}

fn commit_prefix_composition() -> crate::plugins::PluginSet {
    let mut plugins = crate::plugins::PluginSet::default();
    for id in ["commit-a", "commit-b"] {
        plugins
            .register(
                Box::new(Rules(id)),
                Box::new(Content { id, video: false }),
            )
            .unwrap();
    }
    plugins
}

fn commit_prefix_fixture(path: &Path, root: &Path) -> (App, SessionToken) {
    let mut app = App::open_with_plugin_set(
        path,
        root,
        Arc::new(crate::fetch::SystemClock),
        commit_prefix_composition(),
    )
    .unwrap();
    let token = app.hello("prefix", ["lcd".into()].into(), 1).unwrap();
    acknowledge_review_frames(&mut app, "prefix");
    for (id, _) in app.pending_effects() {
        app.complete_effect(id).unwrap();
    }
    (app, token)
}

fn assert_commit_prefix(app: &App, path: &Path, root: &Path, revision: u64) {
    let body: String = app
        .connection.lock().unwrap()
        .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let durable: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        durable,
        serde_json::to_value(StoredState::from(&app.runtime)).unwrap(),
        "live state must match the last successful commit, including outbox"
    );
    assert_eq!(app.runtime.world.revision, revision);
    assert_eq!(app.view().devices[0].selected.as_deref(), Some("commit-b"));
    assert_eq!(app.runtime.world.devices["prefix"].pending_frame_id, None);
    assert!(!app
        .pending_effects()
        .iter()
        .any(|(_, effect)| matches!(effect, Effect::SendFrame { .. })));
    assert!(app.frame_assets.is_empty());
    let restored = App::open_with_plugin_set(
        path,
        root,
        Arc::new(crate::fetch::SystemClock),
        commit_prefix_composition(),
    )
    .unwrap();
    assert_eq!(
        restored.view().devices[0].selected.as_deref(),
        Some("commit-b")
    );
    assert!(!restored
        .pending_effects()
        .iter()
        .any(|(_, effect)| matches!(effect, Effect::SendFrame { .. })));
}

#[test]
fn multiple_commits_keep_successful_prefix_when_later_sqlite_save_fails() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let (mut app, token) = commit_prefix_fixture(&path, temp.path());
    let mut notifications = app.dispatch_notifications();
    let before = app.runtime.world.revision;
    app.connection.lock().unwrap()
        .execute_batch(
            "CREATE TEMP TABLE persist_count(n INTEGER NOT NULL);
             INSERT INTO persist_count VALUES(0);
             CREATE TEMP TRIGGER fail_second_persist BEFORE UPDATE ON app_state
             BEGIN
               UPDATE persist_count SET n=n+1;
               SELECT CASE WHEN (SELECT n FROM persist_count)=2
                 THEN RAISE(ABORT, 'injected second persist failure') END;
             END;",
        )
        .unwrap();
    // Actual button has two saves: select commit-b, then request its frame.
    let error = app.button(&token, ButtonAction::Down).unwrap_err();
    assert!(matches!(error, AppError::Internal(_)));
    let writes: i64 = app
        .connection.lock().unwrap()
        .query_row("SELECT n FROM persist_count", [], |r| r.get(0))
        .unwrap();
    assert_eq!(writes, 1, "the second transaction must roll back");
    assert_commit_prefix(&app, &path, temp.path(), before + 1);
    assert!(notifications.has_changed().unwrap());
    notifications.borrow_and_update();
    assert!(!notifications.has_changed().unwrap());
}

#[test]
fn multiple_commits_keep_successful_prefix_and_notification_after_later_panic() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let (state, token) = commit_prefix_fixture(&path, temp.path());
    let before = state.runtime.world.revision;
    let mut notifications = state.dispatch_notifications();
    let app = Arc::new(std::sync::Mutex::new(state));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut owner = app.lock().unwrap();
        owner.terminal_event(Event::Next { token }).unwrap();
        owner
            .change::<()>(|_, candidate| {
                // This second candidate must never leak into current or DB state.
                candidate.world.devices.get_mut("prefix").unwrap().selected =
                    Some("uncommitted".into());
                panic!("injected panic after first durable commit");
            })
            .unwrap();
    }));
    assert!(outcome.is_err());
    assert!(app.is_poisoned(), "do not silently reuse the failed owner");
    // Inspect the poisoned baseline explicitly. This is not a claim that its
    // ordinary shared query API remains available after an owner panic.
    let owner = match app.lock() {
        Err(error) => error.into_inner(),
        Ok(_) => panic!("expected poisoned application owner"),
    };
    assert_commit_prefix(&owner, &path, temp.path(), before + 1);
    assert!(notifications.has_changed().unwrap());
    notifications.borrow_and_update();
    assert!(!notifications.has_changed().unwrap());
}

include!("application_shared_contract_tests.rs");

include!("application_lifecycle_contract_tests.rs");

include!("application_maintenance_contract_tests.rs");
