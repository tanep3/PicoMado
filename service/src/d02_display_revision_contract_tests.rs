// Independent P2 reproduction plus actual ACK/writer progress during the preserving revision save.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn d02_review_unrelated_enable_revision_preserves_latest_delivery() {
    for screen in ["lcd", "eink"] {
        for render_during_save in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let db = temp.path().join("db");
            let mut state = app_at_fixture_date(&db);
            let token = state
                .hello(
                    "normal",
                    [
                        screen.into(),
                        crate::protocol::FRAME_BYTES_CAPABILITY.into(),
                    ]
                    .into(),
                    1,
                )
                .unwrap();
            for _ in 0..10 {
                if state.selection_for(&token).unwrap().0.as_deref() == Some("weather") {
                    break;
                }
                state.button(&token, ButtonAction::Down).unwrap();
            }
            let source = include_bytes!("../assets/jma-tokyo-20261012.json");
            let first = state
                .plan_plugin_fetches(1791763200, "2026-10-12")
                .unwrap()
                .into_iter()
                .find(|j| j.plugin == "weather")
                .unwrap();
            state
                .complete_plugin_fetch(&first, 1791763201, Ok(source.to_vec()))
                .unwrap();
            for (id, effect) in state.pending_effects() {
                if !matches!(effect, crate::Effect::SendFrame { .. }) {
                    state.complete_effect(id).unwrap();
                }
            }
            let fresh = state
                .plan_plugin_fetches(1791766802, "2026-10-12")
                .unwrap()
                .into_iter()
                .find(|j| j.plugin == "weather")
                .unwrap();
            let prepared =
                state.prepare_plugin_fetch(&fresh).unwrap().unwrap()(Ok(source.to_vec())).unwrap();
            let app = Arc::new(crate::shared::Application::new(state));
            let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
            let count = Arc::new(AtomicUsize::new(0));
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            let writer = tokio::spawn(writer_loop(
                MockSink {
                    count,
                    limit: usize::MAX,
                    recorded: Some(recorded.clone()),
                },
                rx,
                app.clone(),
            ));
            enqueue_reply_common(
                &tx,
                WireMessage::Welcome {
                    incarnation: token.incarnation,
                    session_epoch: token.session_epoch,
                },
                token.session_epoch,
                None,
                Some(token.clone()),
            )
            .await
            .unwrap();
            let bytes = if screen == "eink" { 3813 } else { 25600 };
            let (old_frame, offset) = wait_persist_frame(&recorded, 0, bytes).await;
            let (entered, waiting) = std::sync::mpsc::sync_channel(1);
            let (release, resume) = std::sync::mpsc::sync_channel(1);
            app.lock()
                .unwrap()
                .pause_next_persistence(Box::new(move || {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                        crate::application::AppError::Conflict("test pause expired")
                    })?;
                    Ok(())
                }));
            let worker_app = app.clone();
            let worker = std::thread::spawn(move || {
                worker_app
                    .lock()
                    .unwrap()
                    .apply_prepared_plugin_fetch(&fresh, 1791766803, prepared)
                    .unwrap()
            });
            tokio::task::spawn_blocking(move || {
                waiting.recv_timeout(Duration::from_secs(2)).unwrap()
            })
            .await
            .unwrap();
            let mut current = Some(token.clone());
            process(
                &app,
                &mut current,
                WireMessage::ReceiptAck {
                    token: old_frame.clone(),
                },
                &mut AckDiagnostics::default(),
            )
            .unwrap();
            assert_eq!(
                app.read().unwrap().view().devices[0].last_rendered_frame,
                None
            );
            release.send(()).unwrap();
            tokio::task::spawn_blocking(move || worker.join().unwrap())
                .await
                .unwrap();
            let enabled_revision = app.read().unwrap().view().devices[0].enabled_revision;
            let (new_frame, new_end) = if render_during_save {
                let (entered, waiting) = std::sync::mpsc::sync_channel(1);
                let (release, resume) = std::sync::mpsc::sync_channel(1);
                app.lock()
                    .unwrap()
                    .pause_next_persistence(Box::new(move || {
                        entered.send(()).unwrap();
                        resume.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(())
                    }));
                let owner = app.clone();
                let bound = token.clone();
                let worker = std::thread::spawn(move || {
                    owner
                        .lock()
                        .unwrap()
                        .set_enabled(
                            "normal",
                            bound.incarnation,
                            enabled_revision,
                            "youtube",
                            false,
                        )
                        .unwrap()
                });
                tokio::task::spawn_blocking(move || {
                    waiting.recv_timeout(Duration::from_secs(2)).unwrap()
                })
                .await
                .unwrap();
                process(
                    &app,
                    &mut current,
                    WireMessage::RenderedAck {
                        token: old_frame.clone(),
                    },
                    &mut AckDiagnostics::default(),
                )
                .unwrap();
                let delivered = wait_persist_frame(&recorded, offset, bytes).await;
                // The actual writer must finish its nonpersisting completion too.
                tokio::time::timeout(Duration::from_secs(2), async { loop {
                if !app.read().unwrap().pending_effects().iter().any(|(_, e)| matches!(e, crate::Effect::SendFrame { token, .. } if token == &delivered.0)) { break; }
                tokio::task::yield_now().await;
            }}).await.unwrap();
                release.send(()).unwrap();
                tokio::task::spawn_blocking(move || worker.join().unwrap())
                    .await
                    .unwrap();
                delivered
            } else {
                app.lock()
                    .unwrap()
                    .set_enabled(
                        "normal",
                        token.incarnation,
                        enabled_revision,
                        "youtube",
                        false,
                    )
                    .unwrap();
                process(
                    &app,
                    &mut current,
                    WireMessage::RenderedAck {
                        token: old_frame.clone(),
                    },
                    &mut AckDiagnostics::default(),
                )
                .unwrap();
                wait_persist_frame(&recorded, offset, bytes).await
            };
            assert_eq!(
                app.read()
                    .unwrap()
                    .selection_for(&token)
                    .unwrap()
                    .0
                    .as_deref(),
                Some("weather")
            );
            assert!(app
                .read()
                .unwrap()
                .frame_waiting_for_render(&token, new_frame.frame_id));
            assert!(!app
                .read()
                .unwrap()
                .frame_waiting_for_render(&token, old_frame.frame_id));
            assert!(new_frame.frame_id > old_frame.frame_id);
            assert_eq!(app.read().unwrap().session_mode(&token), Some("normal"));
            assert_ne!(
                app.read()
                    .unwrap()
                    .plugin_position(&token)
                    .unwrap()
                    .dataset_id,
                first.dataset_id
            );
            persist_ack(&app, &mut current, new_frame);
            let mut notices = app.read().unwrap().dispatch_notifications();
            notices.borrow_and_update();
            process(
                &app,
                &mut current,
                WireMessage::RenderedAck { token: old_frame },
                &mut AckDiagnostics::default(),
            )
            .unwrap();
            assert!(!notices.has_changed().unwrap());
            assert_eq!(recorded.lock().unwrap().len(), new_end);
            tx.send(WriterCommand::Stop).await.unwrap();
            assert_eq!(writer.await.unwrap(), "requested_stop");
        }
    }
}
