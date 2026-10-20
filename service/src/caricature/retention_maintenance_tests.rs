// Included in retention::tests. All physical files are synthetic temp fixtures.
fn maintenance_input(root: &Path, hour: u8) -> MaintenanceInput {
    MaintenanceInput {
        paths: crate::plugin_jobs::PluginPaths {
            data: root.join("data"),
            cache: root.join("cache"),
        },
        now_unix: unix_jst(2026, Month::April, 30) + i64::from(hour) * 3600,
        revision: 1,
        snapshot: std::sync::Arc::new(serde_json::json!({"history":[]})),
    }
}
fn retention_content(ids: &[&str]) -> super::super::CaricatureContent {
    let mut content = super::super::CaricatureContent::default();
    for (index, id) in ids.iter().enumerate() {
        let artwork = art(id, unix_jst(2026, Month::January, 31));
        content.history.push(super::super::HistoryEntry {
            artwork_id: (*id).into(),
            metadata: artwork.metadata,
            publish_seq: index as u64 + 1,
            published_at_unix: 1,
        });
    }
    content
}
fn retention_proposal(ids: &[&str], pending: &[&str], protected: &[&str]) -> serde_json::Value {
    serde_json::to_value(RetentionProposal {
        today: "2026-04-30".into(),
        daily: true,
        expired: ids.iter().map(|s| (*s).into()).collect(),
        pending: pending.iter().map(|s| (*s).into()).collect(),
        protected: protected.iter().map(|s| (*s).into()).collect(),
    })
    .unwrap()
}
#[test]
fn d03b_retention_gate_jst_window_success_cache_and_root_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let disabled = RetentionMaintenance::default();
    assert_eq!(disabled.enabled(), deletion_enabled());
    if !deletion_enabled() {
        assert!(disabled
            .prepare(maintenance_input(tmp.path(), 3))
            .unwrap()
            .is_none());
        assert!(!tmp.path().join("data").exists());
    }
    let executor = RetentionMaintenance::for_test();
    assert!(executor
        .prepare(maintenance_input(tmp.path(), 2))
        .unwrap()
        .is_none());
    assert!(executor
        .prepare(maintenance_input(tmp.path(), 4))
        .unwrap()
        .is_none());
    let mut prepared = executor
        .prepare(maintenance_input(tmp.path(), 3))
        .unwrap()
        .unwrap();
    let mut content = retention_content(&[]);
    let plan = plan_retention(&mut content, &prepared.proposal, &[]).unwrap();
    assert!(!plan.changed);
    executor.stage(&mut prepared, &plan.outcome).unwrap();
    executor
        .finish(
            prepared,
            &MaintenanceCommit {
                revision: 1,
                snapshot: std::sync::Arc::new(content.save_state()),
                references: BTreeSet::new(),
                outcome: plan.outcome,
            },
        )
        .unwrap();
    assert!(executor
        .prepare(maintenance_input(tmp.path(), 3))
        .unwrap()
        .is_none());
    let other = tempfile::tempdir().unwrap();
    assert!(executor
        .prepare(maintenance_input(other.path(), 3))
        .unwrap()
        .is_some());
    let mut tomorrow = maintenance_input(tmp.path(), 3);
    tomorrow.now_unix += 86400;
    assert!(executor.prepare(tomorrow).unwrap().is_some());
}
#[test]
fn d03b_retention_live_latest_temporary_browse_and_cancelling_refs_hold_files() {
    let ids = [
        "latest",
        "temporary",
        "start",
        "shown",
        "cursor",
        "pending",
        "job",
        "free",
    ];
    let mut content = retention_content(&ids);
    content.latest = Some(std::sync::Arc::new(art("latest", 1)));
    content.temporary.insert(
        "temp-device".into(),
        (1, std::sync::Arc::new(art("temporary", 1))),
    );
    content.browsing.insert(
        "browse-device".into(),
        super::super::HistoryBrowse {
            token: crate::SessionToken {
                device_id: "browse-device".into(),
                incarnation: 1,
                session_epoch: 1,
            },
            start: std::sync::Arc::new(art("start", 1)),
            shown: std::sync::Arc::new(art("shown", 1)),
            cursor_id: "cursor".into(),
            pending_id: Some("pending".into()),
        },
    );
    let job = crate::plugin_jobs::PluginJob {
        token: crate::plugin_jobs::JobToken {
            plugin: super::super::ID.into(),
            slot: "history".into(),
            generation: 1,
        },
        payload: serde_json::json!({"artwork_id":"job"}),
        timeout_seconds: 30,
        created_unix: 0,
    };
    let plan = plan_retention(&mut content, &retention_proposal(&ids, &[], &[]), &[&job]).unwrap();
    let outcome: RetentionOutcome = serde_json::from_value(plan.outcome).unwrap();
    assert_eq!(outcome.removed, ["free"]);
    assert_eq!(content.history.len(), ids.len() - 1);
    let mut many = retention_content(&[]);
    for n in 0..40 {
        many.history.push(super::super::HistoryEntry {
            artwork_id: format!("item-{n:02}"),
            metadata: art("metadata", 0).metadata,
            publish_seq: n + 1,
            published_at_unix: 0,
        });
    }
    let all: Vec<_> = many.history.iter().map(|h| h.artwork_id.clone()).collect();
    let values: Vec<_> = all.iter().map(String::as_str).collect();
    let proposal = retention_proposal(&values, &[], &[]);
    let plan = plan_retention(&mut many, &proposal, &[]).unwrap();
    let outcome: RetentionOutcome = serde_json::from_value(plan.outcome).unwrap();
    assert_eq!(outcome.removed.len(), 32);
    assert!(outcome.more);
    let plan = plan_retention(&mut many, &proposal, &[]).unwrap();
    let outcome: RetentionOutcome = serde_json::from_value(plan.outcome).unwrap();
    assert_eq!(outcome.removed.len(), 8);
    assert!(!outcome.more);
    assert!(many.history.is_empty());
}
#[test]
fn d03b_retention_partial_unlink_failure_leaves_intent_and_retries_outside_window() {
    let tmp = tempfile::tempdir().unwrap();
    let input = maintenance_input(tmp.path(), 3);
    let root = input.paths.data.join("artwork");
    let cache = input.paths.cache.join("artwork_variants");
    let store = ArtworkStore::new(root.clone());
    store
        .publish(art("old", unix_jst(2026, Month::January, 31)), 1)
        .unwrap();
    fs::create_dir_all(cache.join("old-block.bin")).unwrap();
    let executor = RetentionMaintenance::for_test();
    let mut content = retention_content(&["old"]);
    let mut prepared = executor.prepare(input.clone()).unwrap().unwrap();
    let plan = plan_retention(&mut content, &prepared.proposal, &[]).unwrap();
    assert!(plan.changed);
    executor.stage(&mut prepared, &plan.outcome).unwrap();
    let snapshot = std::sync::Arc::new(content.save_state());
    assert!(executor
        .finish(
            prepared,
            &MaintenanceCommit {
                revision: 2,
                snapshot: snapshot.clone(),
                references: BTreeSet::new(),
                outcome: plan.outcome
            }
        )
        .is_err());
    assert!(pending_path(&root, "old").exists());
    assert!(executor.last_days.lock().unwrap().is_empty());
    assert!(!root.join("archive/old.json").exists()); // cleanup had already made partial progress.
    fs::remove_dir(cache.join("old-block.bin")).unwrap();
    let mut recovery_input = input;
    recovery_input.now_unix += 5 * 3600;
    recovery_input.snapshot = snapshot;
    let mut prepared = executor.prepare(recovery_input).unwrap().unwrap();
    let plan = plan_retention(&mut content, &prepared.proposal, &[]).unwrap();
    assert!(!plan.changed);
    executor.stage(&mut prepared, &plan.outcome).unwrap();
    executor
        .finish(
            prepared,
            &MaintenanceCommit {
                revision: 2,
                snapshot: std::sync::Arc::new(content.save_state()),
                references: BTreeSet::new(),
                outcome: plan.outcome,
            },
        )
        .unwrap();
    assert!(!pending_path(&root, "old").exists());
    assert!(store.history().unwrap().is_empty());
    // Recovery success does not mark the separate daily expiry batch completed.
    assert!(executor.last_days.lock().unwrap().is_empty());
}
#[test]
fn d03b_retention_recovery_respects_protection_jobs_and_unknown_snapshot() {
    use crate::plugin_jobs::{JobRecord, JobState, JobStatus, JobToken, PluginJob};
    use rusqlite::Connection;
    for hold in ["protected", "job", "unknown", "committed"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("artwork");
        let store = ArtworkStore::new(root.clone());
        let ledger = ProtectionLedger::new(root.clone());
        store
            .publish(art("old", unix_jst(2026, Month::January, 31)), 1)
            .unwrap();
        if hold == "protected" {
            ledger.update(&store, 0, &["old".into()], true).unwrap();
        }
        begin_pending_delete(&root, "old").unwrap();
        let mut jobs = JobState::default();
        if hold == "job" {
            jobs.jobs.insert(
                1,
                JobRecord {
                    job: PluginJob {
                        token: JobToken {
                            plugin: super::super::ID.into(),
                            slot: "history".into(),
                            generation: 1,
                        },
                        payload: serde_json::json!({"artwork_id":"old"}),
                        timeout_seconds: 30,
                        created_unix: 0,
                    },
                    status: JobStatus::Cancelling,
                    progress: None,
                    failure: None,
                    finished_unix: None,
                },
            );
        }
        let plugin = if hold == "unknown" {
            serde_json::json!({})
        } else {
            serde_json::json!({"history":[]})
        };
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE app_state(id INTEGER PRIMARY KEY,body TEXT NOT NULL)")
            .unwrap();
        connection
            .execute(
                "INSERT INTO app_state VALUES(1,?1)",
                [
                    serde_json::json!({"plugin_states":{"caricature":plugin},"jobs":jobs})
                        .to_string(),
                ],
            )
            .unwrap();
        let result = recover_pending_deletes(&ledger, &tmp.path().join("cache"), &connection);
        if hold == "unknown" {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        assert_eq!(store.artwork("old").unwrap().is_some(), hold != "committed");
        assert_eq!(
            pending_path(&root, "old").exists(),
            matches!(hold, "protected" | "unknown")
        );
        if hold == "protected" {
            ledger.update(&store, 1, &["old".into()], false).unwrap();
            recover_pending_deletes(&ledger, &tmp.path().join("cache"), &connection).unwrap();
            assert!(
                store.artwork("old").unwrap().is_none() && !pending_path(&root, "old").exists()
            );
        }
    }
}
#[test]
fn d03b_protection_pending_uses_fresh_logical_refs_and_keeps_legacy_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("artwork");
    let store = ArtworkStore::new(root.clone());
    let ledger = ProtectionLedger::new(root.clone());
    store.publish(art("legacy", 0), 1).unwrap();
    store.publish(art("old", 0), 2).unwrap();
    begin_pending_delete(&root, "old").unwrap();
    assert_eq!(
        ledger.update_guarded(&store, 0, &["old".into()], true, || Ok((
            std::sync::Arc::new(serde_json::json!({"history":[]})),
            BTreeSet::new()
        ))),
        Err(CaricatureError::InvalidRequest)
    );
    let protected = ledger
        .update_guarded(&store, 0, &["old".into()], true, || {
            Ok((
                std::sync::Arc::new(serde_json::json!({"history":[{"artwork_id":"old"}]})),
                BTreeSet::new(),
            ))
        })
        .unwrap();
    assert!(protected.protected_ids.contains("old"));
    ledger.update(&store, 1, &["legacy".into()], true).unwrap();
    let executor = RetentionMaintenance::for_test();
    let input = MaintenanceInput {
        paths: crate::plugin_jobs::PluginPaths {
            data: tmp.path().into(),
            cache: tmp.path().join("cache"),
        },
        now_unix: unix_jst(2026, Month::April, 30) + 8 * 3600,
        revision: 1,
        snapshot: std::sync::Arc::new(serde_json::json!({"history":[]})),
    };
    assert!(executor.prepare(input.clone()).unwrap().is_none()); // protected intents outside the daily window are idle.
    assert!(pending_path(&root, "old").exists() && store.artwork("old").unwrap().is_some());
    // A protected committed intent must not starve unrelated fresh daily expiry.
    store.publish(art("free", 0), 3).unwrap();
    let mut daily = input;
    daily.now_unix = unix_jst(2026, Month::April, 30) + 3 * 3600;
    let mut prepared = executor.prepare(daily.clone()).unwrap().unwrap();
    let mut content = retention_content(&["free"]);
    let plan = plan_retention(&mut content, &prepared.proposal, &[]).unwrap();
    assert!(plan.changed);
    let outcome: RetentionOutcome = serde_json::from_value(plan.outcome.clone()).unwrap();
    assert_eq!(outcome.removed, ["free"]);
    assert!(outcome.daily);
    executor.stage(&mut prepared, &plan.outcome).unwrap();
    executor
        .finish(
            prepared,
            &MaintenanceCommit {
                revision: 2,
                snapshot: std::sync::Arc::new(content.save_state()),
                references: BTreeSet::new(),
                outcome: plan.outcome,
            },
        )
        .unwrap();
    assert!(store.artwork("free").unwrap().is_none());
    assert!(pending_path(&root, "old").exists() && store.artwork("old").unwrap().is_some());
    assert!(
        executor.prepare(daily).unwrap().is_none(),
        "protected intent must not repeat publication after successful daily expiry"
    );
}
