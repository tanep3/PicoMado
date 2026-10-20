// Included in application::contract_tests; uses real SQLite and independent queries.
const SHELF: &str = "opaque-maintenance";
#[derive(Clone)]
struct MaintenanceContent {
    shelf: BTreeSet<String>,
    trace: Arc<std::sync::Mutex<Vec<String>>>,
}
impl ContentPlugin for MaintenanceContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        SHELF
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
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
    fn transient_input_ready(&self, _: &PluginContext, _: &str) -> bool {
        true
    }
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> {
        Some(vec![7; 25600])
    }
    fn time_key(&self, _: &PluginContext, now: i64) -> Option<i64> {
        Some(now / 60)
    }
    fn web_state(&self) -> serde_json::Value {
        self.save_state()
    }
    fn save_state(&self) -> serde_json::Value {
        serde_json::json!({"shelf":self.shelf})
    }
    fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
        self.shelf = serde_json::from_value(value["shelf"].clone())
            .map_err(|_| ContentError::InvalidSource)?;
        Ok(())
    }
    fn retention_references_for_jobs(
        &self,
        jobs: &[&crate::plugin_jobs::PluginJob],
    ) -> BTreeSet<String> {
        let mut refs = BTreeSet::from(["pin".into()]);
        for job in jobs {
            assert_eq!(job.token.plugin, SHELF);
            if let Some(id) = job.payload["opaque_asset"].as_str() {
                refs.insert(id.into());
            }
        }
        refs
    }
    fn plan_maintenance(
        &mut self,
        _: &crate::plugin_jobs::CommandContext,
        proposal: &serde_json::Value,
        jobs: &[&crate::plugin_jobs::PluginJob],
    ) -> Result<crate::maintenance::MaintenancePlan, ContentError> {
        self.trace.lock().unwrap().push("plan".into());
        let refs = self.retention_references_for_jobs(jobs);
        let id = proposal["opaque_retire"]
            .as_str()
            .ok_or(ContentError::InvalidSource)?;
        let changed = !refs.contains(id) && self.shelf.remove(id);
        Ok(crate::maintenance::MaintenancePlan {
            changed,
            outcome: serde_json::json!({"unlinked":changed,"name":id}),
        })
    }
}
fn maintenance_composition(trace: Arc<std::sync::Mutex<Vec<String>>>) -> crate::plugins::PluginSet {
    let mut set = crate::plugins::builtins();
    set.register(
        Box::new(Rules(SHELF)),
        Box::new(MaintenanceContent {
            shelf: ["drop".into(), "pin".into()].into(),
            trace,
        }),
    )
    .unwrap();
    set
}
fn maintenance_fixture(db: &Path, root: &Path, trace: Arc<std::sync::Mutex<Vec<String>>>) -> App {
    let mut state = App::open_with_plugin_set(
        db,
        root,
        Arc::new(ReviewClock),
        maintenance_composition(trace),
    )
    .unwrap();
    let snapshot = Arc::new(state.contents[SHELF].save_state());
    state
        .change(|_, r| {
            r.plugin_states.insert(SHELF.into(), snapshot);
            Ok(())
        })
        .unwrap();
    state
}
struct MaintenanceExecutorFixture {
    app: crate::web::SharedApp,
    db: PathBuf,
    marker: PathBuf,
    artifact: PathBuf,
    trace: Arc<std::sync::Mutex<Vec<String>>>,
    prepare_hook: Option<Box<dyn Fn() + Send + Sync>>,
    pause: Option<(
        std::sync::mpsc::SyncSender<()>,
        std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    )>,
    fail_stage: bool,
    poison_publication: bool,
    fail_finish: bool,
    panic_finish: bool,
}
impl crate::maintenance::PluginMaintenanceExecutor for MaintenanceExecutorFixture {
    fn prepare(
        &self,
        input: crate::maintenance::MaintenanceInput,
    ) -> Result<Option<crate::maintenance::PreparedMaintenance>, crate::plugin_jobs::JobFailure>
    {
        assert!(input.snapshot["shelf"].is_array());
        self.trace.lock().unwrap().push("prepare".into());
        if let Some(hook) = &self.prepare_hook {
            hook();
        }
        Ok(Some(crate::maintenance::PreparedMaintenance {
            proposal: serde_json::json!({"opaque_retire":"drop"}),
            permit: Box::new(()),
        }))
    }
    fn stage(
        &self,
        _: &mut crate::maintenance::PreparedMaintenance,
        _: &serde_json::Value,
    ) -> Result<(), crate::plugin_jobs::JobFailure> {
        self.trace.lock().unwrap().push("stage".into());
        std::fs::write(&self.marker, b"intent").unwrap();
        if self.poison_publication {
            self.app.poison_publication_for_test();
        }
        if self.fail_stage {
            return Err(crate::plugin_jobs::JobFailure::new(
                "fixture_stage",
                "Fixture stage failed",
            ));
        }
        Ok(())
    }
    fn finish(
        &self,
        _: crate::maintenance::PreparedMaintenance,
        committed: &crate::maintenance::MaintenanceCommit,
    ) -> Result<(), crate::plugin_jobs::JobFailure> {
        self.trace.lock().unwrap().push("finish".into());
        assert_eq!(committed.outcome["unlinked"], true);
        assert_eq!(committed.references, BTreeSet::from(["pin".into()]));
        assert!(
            !self.app.read().unwrap().runtime.plugin_states[SHELF]["shelf"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("drop"))
        );
        let body: String = Connection::open(&self.db)
            .unwrap()
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert!(
            !serde_json::from_str::<serde_json::Value>(&body).unwrap()["plugin_states"][SHELF]
                ["shelf"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("drop"))
        );
        assert!(self.artifact.exists() && self.marker.exists());
        if let Some((entered, resume)) = &self.pause {
            entered.send(()).unwrap();
            resume
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(8))
                .unwrap();
        }
        if self.panic_finish {
            panic!("fixture external finish panic");
        }
        if self.fail_finish {
            return Err(crate::plugin_jobs::JobFailure::new(
                "fixture_finish",
                "Fixture finish failed",
            ));
        }
        std::fs::remove_file(&self.artifact).unwrap();
        std::fs::remove_file(&self.marker).unwrap();
        Ok(())
    }
}
fn maintenance_executor(
    app: crate::web::SharedApp,
    db: &Path,
    root: &Path,
    trace: Arc<std::sync::Mutex<Vec<String>>>,
) -> MaintenanceExecutorFixture {
    let artifact = root.join("fixture-artifact");
    std::fs::write(&artifact, b"fixture").unwrap();
    MaintenanceExecutorFixture {
        app,
        db: db.into(),
        marker: root.join("fixture-intent"),
        artifact,
        trace,
        prepare_hook: None,
        pause: None,
        fail_stage: false,
        poison_publication: false,
        fail_finish: false,
        panic_finish: false,
    }
}
#[test]
fn d03b_opaque_plugin_publishes_and_notifies_before_finish_unlinks() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("db");
    let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = Arc::new(crate::shared::Application::new(maintenance_fixture(
        &db,
        temp.path(),
        trace.clone(),
    )));
    let notifications = app.read().unwrap().dispatch_notifications();
    let (entered, waiting) = std::sync::mpsc::sync_channel(1);
    let (release, resume) = std::sync::mpsc::sync_channel(1);
    let mut executor = maintenance_executor(app.clone(), &db, temp.path(), trace.clone());
    executor.pause = Some((entered, std::sync::Mutex::new(resume)));
    let worker_app = app.clone();
    let worker =
        std::thread::spawn(move || crate::maintenance::run_once(&worker_app, SHELF, &executor));
    waiting
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        ["prepare", "plan", "stage", "finish"]
    );
    assert!(notifications.has_changed().unwrap());
    assert!(matches!(
        app.try_lock(),
        Err(crate::shared::AccessError::Busy)
    ));
    assert!(temp.path().join("fixture-artifact").exists());
    assert!(app.read().unwrap().maintenance_blocks(Some(SHELF)));
    release.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap());
    assert!(!app.read().unwrap().maintenance_blocks(Some(SHELF)));
    assert!(!temp.path().join("fixture-artifact").exists());
    let reopened = App::open_with_plugin_set(
        &db,
        temp.path(),
        Arc::new(ReviewClock),
        maintenance_composition(trace),
    )
    .unwrap();
    assert_eq!(
        reopened.runtime.plugin_states[SHELF]["shelf"],
        serde_json::json!(["pin"])
    );
}
#[test]
fn d03b_stale_or_busy_preparation_never_stages_or_finishes() {
    for stale in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("db");
        let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = Arc::new(crate::shared::Application::new(maintenance_fixture(
            &db,
            temp.path(),
            trace.clone(),
        )));
        let mut executor = maintenance_executor(app.clone(), &db, temp.path(), trace.clone());
        if stale {
            let a = app.clone();
            executor.prepare_hook = Some(Box::new(move || {
                a.lock()
                    .unwrap()
                    .change(|_, r| {
                        r.managed_revisions.insert(SHELF.into(), 2);
                        Ok(())
                    })
                    .unwrap();
            }));
        }
        let owner = if !stale {
            Some(app.lock().unwrap())
        } else {
            None
        };
        let result = crate::maintenance::run_once(&app, SHELF, &executor);
        if stale {
            assert!(result.is_err());
        } else {
            assert!(!result.unwrap());
        }
        drop(owner);
        assert_eq!(*trace.lock().unwrap(), ["prepare"]);
        assert!(executor.artifact.exists() && !executor.marker.exists());
    }
}
#[test]
fn d03b_stage_save_finish_failure_and_panic_preserve_required_prefix() {
    for phase in ["stage", "save", "publish", "finish", "panic"] {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("db");
        let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = Arc::new(crate::shared::Application::new(maintenance_fixture(
            &db,
            temp.path(),
            trace.clone(),
        )));
        let executor = MaintenanceExecutorFixture {
            fail_stage: phase == "stage",
            poison_publication: phase == "publish",
            fail_finish: phase == "finish",
            panic_finish: phase == "panic",
            ..maintenance_executor(app.clone(), &db, temp.path(), trace)
        };
        if phase == "save" {
            app.lock().unwrap().pause_next_persistence(Box::new(|| {
                Err(AppError::Conflict("fixture SQL failure"))
            }));
        }
        let notifications = app.read().unwrap().dispatch_notifications();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::maintenance::run_once(&app, SHELF, &executor)
        }));
        if phase == "panic" {
            assert!(result.is_err());
            assert!(app.read().is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        assert!(executor.artifact.exists() && executor.marker.exists());
        let body: String = Connection::open(&db)
            .unwrap()
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let values = serde_json::from_str::<serde_json::Value>(&body).unwrap()["plugin_states"]
            [SHELF]["shelf"]
            .clone();
        let retired = matches!(phase, "publish" | "finish" | "panic");
        assert_eq!(
            !values
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("drop")),
            retired
        );
        if !matches!(phase, "publish" | "panic") {
            assert_eq!(
                app.read().unwrap().runtime.plugin_states[SHELF]["shelf"],
                values
            );
        }
        if phase == "publish" {
            assert!(app.read().is_err());
        }
        if retired {
            assert!(notifications.has_changed().unwrap());
        }
    }
}
#[test]
fn d03b_sql_and_finish_pause_allow_other_plugin_inputs_ack_and_clock() {
    for phase in ["save", "finish"] {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("db");
        let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut state = maintenance_fixture(&db, temp.path(), trace.clone());
        let target = state.hello("target", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &target, SHELF);
        acknowledge_review_frames(&mut state, "target");
        let other = state.hello("weather", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &other, "weather");
        let job = state
            .plan_plugin_fetches(1791763200, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather" && j.device_id == "weather")
            .unwrap();
        state
            .complete_plugin_fetch(
                &job,
                1791763201,
                Ok(include_bytes!("../assets/jma-tokyo-20261012.json").to_vec()),
            )
            .unwrap();
        acknowledge_review_frames(&mut state, "weather");
        let clock = state.hello("clock", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &clock, "clock");
        acknowledge_review_frames(&mut state, "clock");
        state.timed_frames.remove("clock");
        state.timed_frames.remove("target");
        let sub = crate::protocol::WireMessage::EnterSub {
            incarnation: target.incarnation,
            session_epoch: target.session_epoch,
        };
        assert!(state.transient_button_snapshot(&target, &sub).is_some());
        let app = Arc::new(crate::shared::Application::new(state));
        let mut executor = maintenance_executor(app.clone(), &db, temp.path(), trace);
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, resume) = std::sync::mpsc::sync_channel(1);
        if phase == "save" {
            app.lock()
                .unwrap()
                .pause_next_persistence(Box::new(move || {
                    entered.send(()).unwrap();
                    resume
                        .recv_timeout(std::time::Duration::from_secs(8))
                        .unwrap();
                    Ok(())
                }));
        } else {
            executor.pause = Some((entered, std::sync::Mutex::new(resume)));
        }
        let a = app.clone();
        let worker = std::thread::spawn(move || crate::maintenance::run_once(&a, SHELF, &executor));
        waiting
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(app
            .read()
            .unwrap()
            .transient_button_snapshot(&target, &sub)
            .is_none());
        assert!(app.read().unwrap().maintenance_blocks(Some(SHELF)));
        use tower::ServiceExt;
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                crate::web::router(app.clone())
                    .oneshot(
                        axum::http::Request::builder()
                            .uri("/api/plugins/opaque-maintenance")
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            });
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let message = crate::protocol::WireMessage::EnterSub {
            incarnation: other.incarnation,
            session_epoch: other.session_epoch,
        };
        assert!(
            app.read()
                .unwrap()
                .transient_button_snapshot(&other, &message)
                .is_some(),
            "weather fixture must be prepared for its own device"
        );
        app.input_lock(&other, &message)
            .unwrap()
            .button(&other, ButtonAction::Sub)
            .unwrap();
        assert_eq!(
            app.read().unwrap().runtime.world.devices["weather"].mode,
            Mode::Sub
        );
        let frame = app
            .read()
            .unwrap()
            .pending_effects()
            .into_iter()
            .find_map(|(_, e)| match e {
                Effect::SendFrame { token, .. } if token.device_id == "weather" => Some(token),
                _ => None,
            })
            .unwrap();
        let ack = crate::protocol::WireMessage::RenderedAck {
            token: frame.clone(),
        };
        app.input_lock(&other, &ack)
            .unwrap()
            .terminal_event(Event::RenderedAck { token: frame })
            .unwrap();
        assert_eq!(app.refresh_timed_frames().unwrap(), 1);
        assert!(app
            .read()
            .unwrap()
            .pending_effects()
            .iter()
            .any(|(_, e)| matches!(e,Effect::SendFrame{token,..}if token.device_id=="clock")));
        release.send(()).unwrap();
        assert!(worker.join().unwrap().unwrap());
        assert_eq!(
            app.refresh_timed_frames().unwrap(),
            1,
            "target time refresh resumes once after release"
        );
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        assert_eq!(
            app.read().unwrap().runtime.world.devices["weather"].mode,
            Mode::Sub
        );
    }
}

#[test]
fn d03b_plan_filters_current_unfinished_opaque_jobs_including_cancelling() {
    use crate::plugin_jobs::JobStatus;
    for (plugin, status, kept) in [
        (SHELF, JobStatus::Queued, true),
        (SHELF, JobStatus::Running, true),
        (SHELF, JobStatus::Cancelling, true),
        (SHELF, JobStatus::Succeeded, false),
        ("other", JobStatus::Running, false),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut state = maintenance_fixture(&temp.path().join("db"), temp.path(), trace);
        state.runtime.jobs.jobs.insert(
            1,
            lifecycle_job(
                plugin,
                1,
                status,
                serde_json::json!({"opaque_asset":"drop","artwork_id":"ignored"}),
            ),
        );
        let plan = state
            .plan_maintenance(SHELF, 1, &serde_json::json!({"opaque_retire":"drop"}))
            .unwrap();
        assert_eq!(plan.outcome["unlinked"], !kept);
        assert_eq!(
            state.runtime.plugin_states[SHELF]["shelf"],
            serde_json::json!(["drop", "pin"]),
            "pure plan must leave actual state unchanged"
        );
    }
}
