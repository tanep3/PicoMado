// Independent review reproductions, extended to send-only/receipt-only and duplicate SettingsStored.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn d02_review_partial_completion_retains_latest_until_rendered() {
    for screen in ["lcd", "eink"] {
        for receipt in [false, true] {
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
            // Let the actual writer finish SendFrame while SQLite is paused.
            tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !app.read().unwrap().pending_effects().iter().any(|(_, e)| matches!(e, crate::Effect::SendFrame { token, .. } if token == &old_frame)) { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
            let mut current = Some(token.clone());
            if receipt {
                process(
                    &app,
                    &mut current,
                    WireMessage::ReceiptAck {
                        token: old_frame.clone(),
                    },
                    &mut AckDiagnostics::default(),
                )
                .unwrap();
            }
            assert_eq!(
                app.read().unwrap().view().devices[0].last_rendered_frame,
                None
            );
            release.send(()).unwrap();
            tokio::task::spawn_blocking(move || worker.join().unwrap())
                .await
                .unwrap();
            assert_eq!(app
                .read()
                .unwrap()
                .frame_waiting_for_render(&token, old_frame.frame_id), screen == "lcd");
            assert!(app
                .read()
                .unwrap()
                .pending_effects()
                .iter()
                .all(|(_, e)| !matches!(e, crate::Effect::SendFrame { .. })));
            // The legacy LCD waits for physical completion; E-ink may accept
            // a new complete image into its bounded firmware pending slot.
            if screen == "lcd" { assert_eq!(recorded.lock().unwrap().len(), offset); }
            let early = if screen == "eink" {
                Some(wait_persist_frame(&recorded, offset, bytes).await)
            } else { None };
            process(
                &app,
                &mut current,
                WireMessage::RenderedAck {
                    token: old_frame.clone(),
                },
                &mut AckDiagnostics::default(),
            )
            .unwrap();
            let (new_frame, _) = match early {
                Some(delivered) => delivered,
                None => wait_persist_frame(&recorded, offset, bytes).await,
            };
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
            tx.send(WriterCommand::Stop).await.unwrap();
            assert_eq!(writer.await.unwrap(), "requested_stop");
        }
    }
}

#[test]
fn d02_review_stream_status_publishes_only_changed_progress() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = App::open_with_plugin_set(
        &temp.path().join("db"),
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        persistence_composition(),
    )
    .unwrap();
    let token = state
        .hello(
            "stream",
            [
                "lcd".into(),
                crate::protocol::FRAME_BYTES_CAPABILITY.into(),
                crate::protocol::LCD_PUSH_CAPABILITY.into(),
            ]
            .into(),
            1,
        )
        .unwrap();
    state.button(&token, ButtonAction::Down).unwrap();
    let rx = state.subscribe_lcd_video(&token).unwrap();
    state
        .queue_lcd_video(&token, 6, 1, 1, vec![3; 25600])
        .unwrap();
    let frame = rx.borrow().clone().unwrap();
    let app = Arc::new(crate::shared::Application::new(state));
    let mut current = Some(token);
    let mut notices = app.read().unwrap().dispatch_notifications();
    let before = app.read().unwrap().view().devices[0].last_rendered_frame;
    process(
        &app,
        &mut current,
        WireMessage::StreamStatus {
            token: frame.token.clone(),
            generation: 1,
            received_frames: 1,
            rendered_frames: 1,
            dropped_frames: 0,
            elapsed_ms: 1,
            max_gap_us: 0,
        },
        &mut AckDiagnostics::default(),
    )
    .unwrap();
    let after = app.read().unwrap().view().devices[0].last_rendered_frame;
    println!(
        "REVIEW StreamStatus before={before:?} after={after:?} accepted frame={}",
        frame.token.frame_id
    );
    assert_eq!(after, Some(frame.token.frame_id));
    assert!(notices.has_changed().unwrap());
    notices.borrow_and_update();
    for generation in [1, 2] {
        process(
            &app,
            &mut current,
            WireMessage::StreamStatus {
                token: frame.token.clone(),
                generation,
                received_frames: 2,
                rendered_frames: 2,
                dropped_frames: 0,
                elapsed_ms: 2,
                max_gap_us: 0,
            },
            &mut AckDiagnostics::default(),
        )
        .unwrap();
        assert!(!notices.has_changed().unwrap());
    }
    assert_eq!(
        app.read().unwrap().view().devices[0].last_received_frame,
        Some(frame.token.frame_id)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn d02_review_real_reader_duplicate_confirmation_does_not_block_following_ack() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = app_at_fixture_date(&temp.path().join("db"));
    let order: Vec<String> = ["weather", "rain-radar", "youtube", "caricature", "clock"]
        .into_iter().chain(crate::plugins::DEFAULT_ORDER.iter().copied().filter(|id| *id == "my-words"))
        .map(str::to_owned).collect();
    state.set_order(1, order.clone()).unwrap();
    let _token = state
        .hello(
            "wire",
            ["lcd".into(), crate::protocol::FRAME_BYTES_CAPABILITY.into()].into(),
            1,
        )
        .unwrap();
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
    persist_finish_state_frames(&mut state, "wire");
    let app = Arc::new(crate::shared::Application::new(state));
    let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (input, mut incoming) = tokio::sync::mpsc::channel(16);
    let server = tokio::spawn(serve_split(
        MockSink {
            count: Arc::new(AtomicUsize::new(0)),
            limit: usize::MAX,
            recorded: Some(recorded.clone()),
        },
        futures_util::stream::poll_fn(move |cx| incoming.poll_recv(cx)),
        app.clone(),
    ));
    input
        .send(Ok::<_, ()>(Message::Binary(
            encode(&WireMessage::Hello {
                device_id: "wire".into(),
                capabilities: vec!["lcd".into(), crate::protocol::FRAME_BYTES_CAPABILITY.into()],
                screen_owner_epoch: 1,
            })
            .unwrap()
            .into(),
        )))
        .await
        .unwrap();
    let (frame, _) = wait_persist_frame(&recorded, 0, 25600).await;
    let bound = SessionToken {
        device_id: frame.device_id.clone(),
        incarnation: frame.incarnation,
        session_epoch: frame.session_epoch,
    };
    app.lock().unwrap().heartbeat(&bound, 1).unwrap();
    app.lock().unwrap().settings_stored(&bound, 1).unwrap(); // Both revisions already confirmed.
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
    let worker = std::thread::spawn(move || owner.lock().unwrap().set_order(2, order).unwrap());
    tokio::task::spawn_blocking(move || waiting.recv_timeout(Duration::from_secs(2)).unwrap())
        .await
        .unwrap();
    let mut bytes = encode(&WireMessage::Heartbeat {
        incarnation: bound.incarnation,
        session_epoch: bound.session_epoch,
        applied_revision: 1,
    })
    .unwrap();
    bytes.extend(
        encode(&WireMessage::SettingsStored {
            incarnation: bound.incarnation,
            revision: 1,
        })
        .unwrap(),
    );
    bytes.extend(
        encode(&WireMessage::ReceiptAck {
            token: frame.clone(),
        })
        .unwrap(),
    );
    bytes.extend(
        encode(&WireMessage::RenderedAck {
            token: frame.clone(),
        })
        .unwrap(),
    );
    input.send(Ok(Message::Binary(bytes.into()))).await.unwrap();
    let completed_during_pause = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if app.read().unwrap().view().devices[0].last_rendered_frame == Some(frame.frame_id) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();
    let during = app.read().unwrap().view().devices[0].last_rendered_frame;
    release.send(()).unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if app.read().unwrap().view().devices[0].last_rendered_frame == Some(frame.frame_id) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        completed_during_pause,
        "actual reader ACK must progress before SQLite release"
    );
    drop(input);
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        during,
        Some(frame.frame_id),
        "duplicate confirmed Heartbeat blocks following ACKs on real reader"
    );
}
