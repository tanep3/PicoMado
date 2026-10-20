// Included in application::contract_tests; exercise the shared publication API.
#[test]
fn shared_prefix_is_published_after_later_sqlite_failure_and_before_notification() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (state, token) = commit_prefix_fixture(&path, temp.path());
    let app = crate::shared::Application::new(state);
    let mut notifications = app.read().unwrap().dispatch_notifications();
    app.lock().unwrap().connection.lock().unwrap().execute_batch(
        "CREATE TEMP TABLE persist_count(n INTEGER NOT NULL);
         INSERT INTO persist_count VALUES(0);
         CREATE TEMP TRIGGER fail_second_persist BEFORE UPDATE ON app_state BEGIN
           UPDATE persist_count SET n=n+1;
           SELECT CASE WHEN (SELECT n FROM persist_count)=2 THEN RAISE(ABORT,'second save failed') END;
         END;"
    ).unwrap();
    let result = { app.lock().unwrap().button(&token, ButtonAction::Down) };
    assert!(result.is_err());
    let published = app.read().unwrap();
    let body: String = published
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let durable: serde_json::Value = serde_json::from_str(&body).unwrap();
    let mut live = serde_json::to_value(StoredState::from(&published.runtime)).unwrap();
    // Failed operations reserve IDs without reusing them. The allocator may
    // be ahead of the last saved prefix, while state and outbox must agree.
    for key in ["next_effect_id"] {
        assert!(live[key].as_u64().unwrap() >= durable[key].as_u64().unwrap());
        live[key] = durable[key].clone();
    }
    assert!(
        live["world"]["next_frame_id"].as_u64().unwrap()
            >= durable["world"]["next_frame_id"].as_u64().unwrap()
    );
    live["world"]["next_frame_id"] = durable["world"]["next_frame_id"].clone();
    assert_eq!(live, durable);
    assert_eq!(
        published.view().devices[0].selected.as_deref(),
        Some("commit-b")
    );
    assert!(published.frame_assets.is_empty());
    assert!(notifications.has_changed().unwrap());
    notifications.borrow_and_update();
    assert!(!notifications.has_changed().unwrap());
    let restored = App::open_with_plugin_set(
        &path,
        temp.path(),
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
        .any(|(_, e)| matches!(e, Effect::SendFrame { .. })));
}

#[test]
fn shared_prefix_panic_marks_queries_unavailable_but_retains_commit_notification() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (state, token) = commit_prefix_fixture(&path, temp.path());
    let app = crate::shared::Application::new(state);
    let mut notifications = app.read().unwrap().dispatch_notifications();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut owner = app.lock().unwrap();
        owner.terminal_event(Event::Next { token }).unwrap();
        panic!("later phase panics after durable selection commit");
    }))
    .is_err());
    assert!(app.read().is_err());
    assert!(app.lock().is_err());
    assert!(notifications.has_changed().unwrap());
    notifications.borrow_and_update();
    assert!(!notifications.has_changed().unwrap());
    let restored = App::open_with_plugin_set(
        &path,
        temp.path(),
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
        .any(|(_, e)| matches!(e, Effect::SendFrame { .. })));
}

#[test]
fn transient_fork_cannot_publish_durable_selection_or_settings_without_save() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (state, token) = commit_prefix_fixture(&path, temp.path());
    let before: String = state
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let mut candidate = state.transient_snapshot();
    assert!(candidate.button(&token, ButtonAction::Down).is_err());
    assert_eq!(
        candidate.view().devices[0].selected.as_deref(),
        Some("commit-a")
    );
    let d = candidate.view().devices.remove(0);
    assert!(candidate
        .set_enabled(
            &token.device_id,
            token.incarnation,
            d.enabled_revision,
            "commit-b",
            false
        )
        .is_err());
    let after: String = state
        .connection
        .lock()
        .unwrap()
        .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(candidate.runtime.settings, state.runtime.settings);
}

#[test]
fn sqlite_commit_invalidation_does_not_restore_old_browse_or_its_cause() {
    for operation in ["settings", "delete", "reconnect"] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut state = App::open_with_clock(&path, Arc::new(ReviewClock)).unwrap();
        let token = state.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &token, "weather");
        let job = state
            .plan_plugin_fetches(1791763200, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        state
            .complete_plugin_fetch(
                &job,
                1791763201,
                Ok(include_bytes!("../assets/jma-tokyo-20261012.json").to_vec()),
            )
            .unwrap();
        acknowledge_review_frames(&mut state, "desk");
        for (id, _) in state.pending_effects() {
            state.complete_effect(id).unwrap();
        }
        let enabled_revision = state.view().devices[0].enabled_revision;
        let app = Arc::new(crate::shared::Application::new(state));
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, resume) = std::sync::mpsc::sync_channel(1);
        app.lock()
            .unwrap()
            .pause_next_persistence(Box::new(move || {
                entered.send(()).unwrap();
                resume
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|_| AppError::Conflict("test pause expired"))?;
                Ok(())
            }));
        let worker_app = app.clone();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            let mut owner = worker_app.lock().unwrap();
            match operation {
                "settings" => owner
                    .set_enabled(
                        "desk",
                        worker_token.incarnation,
                        enabled_revision,
                        "weather",
                        false,
                    )
                    .map(|_| ()),
                "delete" => owner.delete_device("desk", worker_token.incarnation),
                _ => owner.hello("desk", ["lcd".into()].into(), 1).map(|_| ()),
            }
            .unwrap();
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let message = crate::protocol::WireMessage::EnterSub {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
        };
        let cause = crate::observation::Cause {
            version: 1,
            boot: 901,
            operation: 2,
        };
        {
            let mut live = app.input_lock(&token, &message).unwrap();
            live.with_observation_cause("desk", Some(cause), |view| {
                view.button(&token, ButtonAction::Sub)
            })
            .unwrap();
        }
        assert_eq!(app.read().unwrap().session_mode(&token), Some("sub"));
        let old_frame = app
            .read()
            .unwrap()
            .pending_effects()
            .into_iter()
            .find_map(|(_, e)| match e {
                Effect::SendFrame { token, .. } => Some(token),
                _ => None,
            })
            .unwrap();
        release.send(()).unwrap();
        worker.join().unwrap();
        let published = app.read().unwrap();
        if operation == "delete" {
            assert!(published.view().devices.is_empty());
        } else {
            let d = published.runtime.world.devices.get("desk").unwrap();
            assert_eq!(d.mode, Mode::Normal);
            if operation == "settings" {
                assert_ne!(d.selected.as_deref(), Some("weather"));
                assert!(!d.enabled.contains("weather"));
            } else {
                assert!(d.session_epoch > token.session_epoch);
            }
        }
        for (_, effect) in published.pending_effects() {
            if let Effect::SendFrame { token, .. } = effect {
                assert_ne!(
                    published.frame_observation_cause(&token),
                    Some(cause),
                    "discarded view cannot label a new durable frame"
                );
            }
        }
        drop(published);
        let ack = crate::protocol::WireMessage::RenderedAck {
            token: old_frame.clone(),
        };
        let mut live = app.input_lock(&token, &ack).unwrap();
        assert!(!live
            .terminal_event(Event::RenderedAck { token: old_frame })
            .unwrap());
    }
}

#[test]
fn published_and_transient_snapshots_share_large_plugin_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (mut state, _) = commit_prefix_fixture(&path, temp.path());
    let value = Arc::new(serde_json::json!({"pixels":"A".repeat(1024*1024)}));
    state
        .runtime
        .plugin_states
        .insert("fixture".into(), value.clone());
    let published = state.published_snapshot();
    let live = published.transient_snapshot();
    assert!(Arc::ptr_eq(
        &value,
        &published.runtime.plugin_states["fixture"]
    ));
    assert!(Arc::ptr_eq(&value, &live.runtime.plugin_states["fixture"]));
    let stored = StoredState::from(&state.runtime);
    assert!(
        stored.plugin_states["fixture"] == *value,
        "stored snapshot schema must remain unchanged"
    );
}

#[test]
fn duplicate_delivery_completion_and_old_ack_do_not_wake_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let (state, token) = commit_prefix_fixture(&path, temp.path());
    let app = crate::shared::Application::new(state);
    let notices = app.read().unwrap().dispatch_notifications();
    assert!(!app.complete_delivery_effect(u64::MAX).unwrap().0);
    assert!(!notices.has_changed().unwrap());
    let d = app.read().unwrap().runtime.world.devices["prefix"].clone();
    let frame = crate::FrameToken {
        device_id: token.device_id.clone(),
        incarnation: token.incarnation,
        session_epoch: token.session_epoch,
        display_epoch: d.display_epoch,
        screen_owner_epoch: d.screen_owner_epoch,
        frame_id: 1,
    };
    let message = crate::protocol::WireMessage::RenderedAck {
        token: frame.clone(),
    };
    assert!(!app
        .input_lock(&token, &message)
        .unwrap()
        .terminal_event(Event::RenderedAck { token: frame })
        .unwrap());
    assert!(!notices.has_changed().unwrap());
}

struct LiveMinuteClock(std::sync::atomic::AtomicI64);
impl crate::fetch::Clock for LiveMinuteClock {
    fn unix_seconds(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn today_jst(&self) -> String {
        let d = time::OffsetDateTime::from_unix_timestamp(self.unix_seconds())
            .unwrap()
            .to_offset(time::UtcOffset::from_hms(9, 0, 0).unwrap())
            .date();
        format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
    }
}
#[test]
fn sqlite_wait_keeps_visible_clock_minute_delivery_and_time_after_other_settings_commit() {
    for screen in ["lcd", "eink"] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let start = time::Date::from_calendar_date(2026, time::Month::December, 31)
            .unwrap()
            .with_hms(23, 59, 0)
            .unwrap()
            .assume_offset(time::UtcOffset::from_hms(9, 0, 0).unwrap())
            .unix_timestamp();
        let clock = Arc::new(LiveMinuteClock(std::sync::atomic::AtomicI64::new(start)));
        let mut state = App::open_with_clock(&path, clock.clone()).unwrap();
        let mut caps: BTreeSet<String> = [screen.into()].into();
        if screen == "eink" {
            caps.insert(crate::protocol::EINK_CLOCK_CAPABILITY.into());
        }
        let token = state.hello("clock", caps, 1).unwrap();
        let other = state.hello("other", ["lcd".into()].into(), 1).unwrap();
        select_review_plugin(&mut state, &token, "clock");
        acknowledge_review_frames(&mut state, "clock");
        for (id, _) in state.pending_effects() {
            state.complete_effect(id).unwrap();
        }
        let d = state
            .view()
            .devices
            .into_iter()
            .find(|d| d.id == "other")
            .unwrap();
        let mut settings = d.desired;
        settings.heartbeat_seconds += 1;
        let app = Arc::new(crate::shared::Application::new(state));
        let query = Connection::open(&path).unwrap();
        let old_body: String = query
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, resume) = std::sync::mpsc::sync_channel(1);
        app.lock()
            .unwrap()
            .pause_next_persistence(Box::new(move || {
                entered.send(()).unwrap();
                resume
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|_| AppError::Conflict("test pause expired"))?;
                Ok(())
            }));
        let worker_app = app.clone();
        let worker = std::thread::spawn(move || {
            worker_app
                .lock()
                .unwrap()
                .set_device_settings("other", other.incarnation, d.desired_revision, settings)
                .unwrap()
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        clock
            .0
            .store(start + 60, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            crate::clock::reading(start + 60).unwrap().date,
            "2027/01/01（金）"
        );
        assert_eq!(app.refresh_timed_frames().unwrap(), 1);
        let (frame, handle) = app
            .read()
            .unwrap()
            .pending_effects()
            .into_iter()
            .find_map(|(_, e)| match e {
                Effect::SendFrame {
                    token,
                    asset_handle,
                } if token.device_id == "clock" => Some((token, asset_handle)),
                _ => None,
            })
            .unwrap();
        let expected = if screen == "eink" {
            crate::clock::render_eink(start + 60)
        } else {
            crate::clock::render_lcd(start + 60)
        };
        assert!(
            app.read().unwrap().frame_asset(&handle).unwrap().as_slice() == expected.as_slice(),
            "minute pixels must reflect current year/date and HH:MM"
        );
        for receipt in [true, false] {
            let message = if receipt {
                crate::protocol::WireMessage::ReceiptAck {
                    token: frame.clone(),
                }
            } else {
                crate::protocol::WireMessage::RenderedAck {
                    token: frame.clone(),
                }
            };
            let event = if receipt {
                Event::ReceiptAck {
                    token: frame.clone(),
                }
            } else {
                Event::RenderedAck {
                    token: frame.clone(),
                }
            };
            assert!(app
                .input_lock(&token, &message)
                .unwrap()
                .terminal_event(event)
                .unwrap());
        }
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        assert_eq!(
            query
                .query_row::<String, _, _>("SELECT body FROM app_state WHERE id=1", [], |r| r
                    .get(0))
                .unwrap(),
            old_body,
            "clock and ACK must not write SQLite"
        );
        release.send(()).unwrap();
        worker.join().unwrap();
        let published = app.read().unwrap();
        let d = &published.runtime.world.devices["clock"];
        let ctx = PluginContext {
            token: token.clone(),
            settings_revision: d.settings_revision,
            mode: d.mode.clone(),
        };
        let lcd = published.contents["clock"].current_frame(&ctx).unwrap();
        assert!(
            lcd == crate::clock::render_lcd(start + 60),
            "owner completion must not restore an old minute"
        );
        if screen == "eink" {
            assert_eq!(
                published.frame_eink_refresh(&frame).unwrap().minute,
                (start + 60) as u64 / 60
            );
        }
        assert_eq!(app.refresh_timed_frames().unwrap(), 0);
        assert_eq!(
            published
                .view()
                .devices
                .iter()
                .find(|d| d.id == "other")
                .unwrap()
                .selected
                .as_deref(),
            Some("youtube")
        );
    }
}

#[test]
fn d02_review_confirmation_fast_path_keeps_session_revision_and_noop_guards() {
    use crate::protocol::WireMessage as W;
    let temp = tempfile::tempdir().unwrap();
    let (mut state, token) = commit_prefix_fixture(&temp.path().join("db"), temp.path());
    state.heartbeat(&token, 1).unwrap();
    state.settings_stored(&token, 1).unwrap();
    let before = state.runtime.settings.clone();
    let notices = state.dispatch_notifications();
    state.pause_next_persistence(Box::new(|| {
        panic!("duplicate/rejected confirmation entered SQLite")
    }));
    for stored in [false, true] {
        for revision in [0, 1, 2] {
            for stale_session in [false, true] {
                let mut bound = token.clone();
                if stale_session {
                    bound.session_epoch += 1;
                }
                let message = if stored {
                    W::SettingsStored {
                        incarnation: bound.incarnation,
                        revision,
                    }
                } else {
                    W::Heartbeat {
                        incarnation: bound.incarnation,
                        session_epoch: bound.session_epoch,
                        applied_revision: revision,
                    }
                };
                let mut live = state
                    .transient_button_snapshot(&bound, &message)
                    .expect("validated no-op or rejection cannot wait for durable owner");
                let result = if stored {
                    live.settings_stored(&bound, revision)
                } else {
                    live.heartbeat(&bound, revision)
                };
                assert_eq!(result.is_ok(), !stale_session && revision <= 1);
                assert!(!live.transient_was_changed());
                assert_eq!(live.runtime.settings, before);
            }
        }
    }
    assert!(!notices.has_changed().unwrap());
    let mut unconfirmed = state.published_snapshot();
    let record = unconfirmed
        .runtime
        .settings
        .devices
        .get_mut(&token.device_id)
        .unwrap();
    record.confirmed.revision = 0;
    record.stored_revision = 0;
    for message in [
        W::Heartbeat {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
            applied_revision: 1,
        },
        W::SettingsStored {
            incarnation: token.incarnation,
            revision: 1,
        },
    ] {
        assert!(
            unconfirmed
                .transient_button_snapshot(&token, &message)
                .is_none(),
            "new durable confirmation must still save before publication"
        );
    }
}

#[test]
fn d02_review_refresh_obligation_follows_display_identity() {
    for invalidation in ["selection", "settings", "reconnect", "standby", "delete"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut state, token) = commit_prefix_fixture(&temp.path().join("db"), temp.path());
        state.queue_selected_cached_frame(&token).unwrap();
        let (sent_effect, old) = state
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| match e {
                Effect::SendFrame { token, .. } => Some((id, token)),
                _ => None,
            })
            .unwrap();
        state.complete_effect(sent_effect).unwrap();
        let d = &state.runtime.world.devices[&token.device_id];
        state.content_refresh_debt.insert(
            token.device_id.clone(),
            crate::display_token(&token.device_id, d),
        );
        match invalidation {
            "selection" => {
                state.button(&token, ButtonAction::Down).unwrap();
            }
            "settings" => {
                let revision = state.runtime.world.devices[&token.device_id].settings_revision;
                state
                    .set_enabled(
                        &token.device_id,
                        token.incarnation,
                        revision,
                        "commit-b",
                        false,
                    )
                    .unwrap();
            }
            "reconnect" => {
                state
                    .hello(&token.device_id, ["lcd".into()].into(), 2)
                    .unwrap();
            }
            "standby" => {
                state.button(&token, ButtonAction::Power).unwrap();
            }
            "delete" => {
                state
                    .delete_device(&token.device_id, token.incarnation)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        if invalidation == "settings" {
            let current = &state.runtime.world.devices[&token.device_id];
            assert_eq!(
                state.content_refresh_debt[&token.device_id].settings_revision,
                current.settings_revision
            );
            assert!(state
                .terminal_event(Event::RenderedAck { token: old.clone() })
                .unwrap());
            assert!(state.content_refresh_debt.is_empty());
            let frames: Vec<_> = state
                .pending_effects()
                .into_iter()
                .filter_map(|(_, e)| match e {
                    Effect::SendFrame { token, .. } => Some(token),
                    _ => None,
                })
                .collect();
            assert_eq!(frames.len(), 1);
            assert!(frames[0].frame_id > old.frame_id);
        } else {
            assert!(state.content_refresh_debt.is_empty(), "{invalidation}");
            let effects = state.pending_effects();
            assert!(!state
                .terminal_event(Event::RenderedAck { token: old })
                .unwrap());
            assert_eq!(
                state.pending_effects(),
                effects,
                "a replaced display cannot revive its old obligation"
            );
        }
    }
}
