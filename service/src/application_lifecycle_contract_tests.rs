// Included in application::contract_tests: real bootstrap DB, opaque plugin refs.
#[derive(Clone)]
struct LifecycleContent {
    id: &'static str,
    events: Arc<std::sync::Mutex<Vec<String>>>,
}
impl ContentPlugin for LifecycleContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        self.id
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
    fn load_state(&mut self, value: serde_json::Value) -> Result<(), ContentError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("load:{}", self.id));
        if value["recovered"] != true {
            return Err(ContentError::InvalidSource);
        }
        Ok(())
    }
    fn retention_references(&self) -> BTreeSet<String> {
        [format!("display-{}", self.id)].into()
    }
    fn retention_references_for_jobs(
        &self,
        jobs: &[&crate::plugin_jobs::PluginJob],
    ) -> BTreeSet<String> {
        let mut refs = self.retention_references();
        for job in jobs {
            if let Some(value) = job.payload.get("opaque_asset").and_then(|v| v.as_str()) {
                refs.insert(value.into());
            }
        }
        refs
    }
}
struct LifecycleRecovery {
    id: &'static str,
    root: PathBuf,
    events: Arc<std::sync::Mutex<Vec<String>>>,
    fail: bool,
}
impl crate::plugin_jobs::PluginStartupExecutor for LifecycleRecovery {
    fn recover(
        &self,
        paths: &crate::plugin_jobs::PluginPaths,
        database: &Connection,
    ) -> Result<(), std::io::Error> {
        self.events
            .lock()
            .unwrap()
            .push(format!("recover:{}", self.id));
        assert_eq!(paths.data, self.root.join(self.id).join("data"));
        assert_eq!(paths.cache, self.root.join(self.id).join("cache"));
        if self.fail {
            return Err(std::io::Error::other("fixture recovery refused"));
        }
        let body: String = database
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .map_err(std::io::Error::other)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&body).map_err(std::io::Error::other)?;
        value["plugin_states"][self.id] = serde_json::json!({"recovered":true});
        database
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [value.to_string()],
            )
            .map_err(std::io::Error::other)?;
        Ok(())
    }
}
fn lifecycle_composition(
    ids: &[&'static str],
    events: Arc<std::sync::Mutex<Vec<String>>>,
) -> crate::plugins::PluginSet {
    let mut plugins = crate::plugins::PluginSet::default();
    for id in ids {
        plugins
            .register(
                Box::new(Rules(id)),
                Box::new(LifecycleContent {
                    id,
                    events: events.clone(),
                }),
            )
            .unwrap();
    }
    plugins
}
fn lifecycle_existing_db(db: &Path, root: &Path, ids: &[&'static str]) {
    let plugins = lifecycle_composition(ids, Arc::new(std::sync::Mutex::new(Vec::new())));
    let state =
        App::open_with_plugin_set(db, root, Arc::new(crate::fetch::SystemClock), plugins).unwrap();
    let mut value = serde_json::to_value(StoredState::from(&state.runtime)).unwrap();
    for id in ids {
        value["plugin_states"][id] = serde_json::json!({"recovered":false});
    }
    state
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE app_state SET body=?1 WHERE id=1",
            [value.to_string()],
        )
        .unwrap();
}
#[test]
fn d03a_bootstrap_uses_registration_order_paths_and_precedes_rehydration() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("db");
    let root = temp.path().join("plugins");
    let ids = ["boot-b", "boot-a"];
    lifecycle_existing_db(&db, &root, &ids);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut plugins = lifecycle_composition(&ids, events.clone());
    for id in ids {
        plugins
            .register_startup(
                id,
                Arc::new(LifecycleRecovery {
                    id,
                    root: root.clone(),
                    events: events.clone(),
                    fail: false,
                }),
            )
            .unwrap();
    }
    let state = App::open_with_plugin_set(&db, &root, Arc::new(crate::fetch::SystemClock), plugins)
        .unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [
            "recover:boot-b",
            "recover:boot-a",
            "load:boot-a",
            "load:boot-b"
        ]
    );
    assert_eq!(state.runtime.world.plugin_order, ids);
    for id in ids {
        assert_eq!(state.runtime.plugin_states[id]["recovered"], true);
    }
}
#[test]
fn d03a_failed_bootstrap_does_not_load_or_publish_application() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("db");
    let root = temp.path().join("plugins");
    lifecycle_existing_db(&db, &root, &["boot-a"]);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut plugins = lifecycle_composition(&["boot-a"], events.clone());
    plugins
        .register_startup(
            "boot-a",
            Arc::new(LifecycleRecovery {
                id: "boot-a",
                root,
                events: events.clone(),
                fail: true,
            }),
        )
        .unwrap();
    let result = App::open_with_plugin_set(
        &db,
        &temp.path().join("plugins"),
        Arc::new(crate::fetch::SystemClock),
        plugins,
    );
    match result {
        Err(error) => assert!(format!("{error:?}").contains("fixture recovery refused")),
        Ok(_) => panic!("failed recovery must not produce an App"),
    }
    assert_eq!(*events.lock().unwrap(), ["recover:boot-a"]);
}
#[test]
fn d03a_startup_registration_rejects_unknown_and_duplicate_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut plugins = lifecycle_composition(&["boot-a"], events.clone());
    let hook = Arc::new(LifecycleRecovery {
        id: "boot-a",
        root: temp.path().into(),
        events,
        fail: false,
    });
    assert!(
        matches!(plugins.register_startup("missing", hook.clone()), Err(crate::Error::UnknownPlugin(id)) if id=="missing")
    );
    plugins.register_startup("boot-a", hook.clone()).unwrap();
    assert!(
        matches!(plugins.register_startup("boot-a", hook), Err(crate::Error::DuplicatePlugin(id)) if id=="boot-a")
    );
}
fn lifecycle_job(
    plugin: &str,
    generation: u64,
    status: crate::plugin_jobs::JobStatus,
    payload: serde_json::Value,
) -> crate::plugin_jobs::JobRecord {
    crate::plugin_jobs::JobRecord {
        job: crate::plugin_jobs::PluginJob {
            token: crate::plugin_jobs::JobToken {
                plugin: plugin.into(),
                slot: format!("slot-{generation}"),
                generation,
            },
            payload,
            timeout_seconds: 30,
            created_unix: 0,
        },
        status,
        progress: None,
        failure: None,
        finished_unix: None,
    }
}
#[test]
fn d03a_extension_owns_payload_reference_meaning_and_receives_only_unfinished_own_jobs() {
    use crate::plugin_jobs::JobStatus;
    let temp = tempfile::tempdir().unwrap();
    let plugins = lifecycle_composition(
        &["extension", "other"],
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    let mut state = App::open_with_plugin_set(
        &temp.path().join("db"),
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        plugins,
    )
    .unwrap();
    for (p, plugin) in ["extension", "other"].into_iter().enumerate() {
        for (i, status) in [
            JobStatus::Queued,
            JobStatus::Running,
            JobStatus::Cancelling,
            JobStatus::Succeeded,
            JobStatus::Failed,
            JobStatus::Cancelled,
        ]
        .into_iter()
        .enumerate()
        {
            let generation = (p * 10 + i + 1) as u64;
            state.runtime.jobs.jobs.insert(generation, lifecycle_job(plugin, generation, status, serde_json::json!({"opaque_asset":format!("asset-{plugin}-{i}"),"artwork_id":"wrong-common-interpretation"})));
        }
    }
    assert_eq!(
        state.plugin_retention_references("extension"),
        [
            "display-extension",
            "asset-extension-0",
            "asset-extension-1",
            "asset-extension-2"
        ]
        .map(str::to_owned)
        .into()
    );
    assert!(state.plugin_retention_references("missing").is_empty());
}
#[test]
fn d03a_real_caricature_retains_cancelling_history_job_and_excludes_finished_job() {
    use crate::plugin_jobs::JobStatus;
    let temp = tempfile::tempdir().unwrap();
    let mut state = App::open(&temp.path().join("db")).unwrap();
    state.runtime.jobs.jobs.insert(
        1,
        lifecycle_job(
            "caricature",
            1,
            JobStatus::Cancelling,
            serde_json::json!({"artwork_id":"held"}),
        ),
    );
    state.runtime.jobs.jobs.insert(
        2,
        lifecycle_job(
            "caricature",
            2,
            JobStatus::Succeeded,
            serde_json::json!({"artwork_id":"finished"}),
        ),
    );
    state.runtime.jobs.jobs.insert(
        3,
        lifecycle_job(
            "other",
            3,
            JobStatus::Running,
            serde_json::json!({"artwork_id":"unrelated"}),
        ),
    );
    assert_eq!(
        state.plugin_retention_references("caricature"),
        ["held".to_owned()].into()
    );
}
