// Included in transport::tests. Actual SQLite transaction, shared queries,
// playback validation and production writer; only external pixels/sink mocked.
struct PersistRules(&'static str);
impl crate::Plugin for PersistRules {
    fn id(&self) -> &str {
        self.0
    }
    fn supports(&self, c: &BTreeSet<String>) -> bool {
        c.contains("lcd") || c.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        _: &'a dyn crate::WorldQuery,
        _: &'a crate::Event,
    ) -> Box<dyn Iterator<Item = crate::Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}
#[derive(Clone)]
struct PersistContent {
    id: &'static str,
    video: bool,
}
impl crate::content::ContentPlugin for PersistContent {
    fn clone_box(&self) -> Box<dyn crate::content::ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        self.id
    }
    fn publish(
        &mut self,
        _: &crate::content::PluginContext,
        _: &[u8],
        _: &str,
        _: &str,
    ) -> Result<(), crate::content::ContentError> {
        Err(crate::content::ContentError::Unsupported)
    }
    fn position(
        &self,
        _: &crate::content::PluginContext,
    ) -> Option<crate::content::PluginPosition> {
        None
    }
    fn plan(
        &mut self,
        _: &crate::content::PluginContext,
        _: ButtonAction,
    ) -> Result<crate::content::PluginPlan, crate::content::ContentError> {
        Err(crate::content::ContentError::Unsupported)
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
    fn current_frame(&self, _: &crate::content::PluginContext) -> Option<Vec<u8>> {
        (!self.video).then(|| vec![7; 25600])
    }
    fn playback(
        &self,
        _: &crate::content::PluginContext,
    ) -> Option<crate::plugin_jobs::PlaybackSpec> {
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
fn persistence_composition() -> crate::plugins::PluginSet {
    let mut plugins = crate::plugins::PluginSet::default();
    for (id, video) in [
        ("persist-static", false),
        ("persist-video", true),
        ("persist-next", false),
    ] {
        plugins
            .register(
                Box::new(PersistRules(id)),
                Box::new(PersistContent { id, video }),
            )
            .unwrap();
    }
    plugins
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sqlite_wait_preserves_published_state_while_other_video_and_http_progress() {
    use tower::ServiceExt;
    for profile in ["push", "legacy", "eink"] {
        for failure in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let db = temp.path().join("state.db");
            let mut state = App::open_with_plugin_set(
                &db,
                temp.path(),
                Arc::new(crate::fetch::SystemClock),
                persistence_composition(),
            )
            .unwrap();
            let mut caps: BTreeSet<String> = [
                if profile == "eink" { "eink" } else { "lcd" }.to_owned(),
                crate::protocol::FRAME_BYTES_CAPABILITY.into(),
            ]
            .into();
            if profile == "push" {
                caps.insert(crate::protocol::LCD_PUSH_CAPABILITY.into());
            }
            let a = state.hello("a", caps.clone(), 1).unwrap();
            let b = state.hello("b", caps, 1).unwrap();
            state.button(&a, ButtonAction::Down).unwrap();
            state.button(&b, ButtonAction::Down).unwrap();
            for (id, _) in state.pending_effects() {
                state.complete_effect(id).unwrap();
            }
            let app = Arc::new(crate::shared::Application::new(state));
            let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
            let count = Arc::new(AtomicUsize::new(0));
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            let writer = tokio::spawn(writer_loop(
                MockSink {
                    count: count.clone(),
                    limit: usize::MAX,
                    recorded: Some(recorded.clone()),
                },
                rx,
                app.clone(),
            ));
            tx.send(WriterCommand::Bind(b.clone())).await.unwrap();
            enqueue_reply(
                &tx,
                WireMessage::Welcome {
                    incarnation: b.incarnation,
                    session_epoch: b.session_epoch,
                },
                b.session_epoch,
            )
            .await
            .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                while count.load(Ordering::SeqCst) < 1 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let query = rusqlite::Connection::open(&db).unwrap();
            let old_body: String = query
                .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let mut notifications = app.read().unwrap().dispatch_notifications();
            let (entered, waiting) = std::sync::mpsc::sync_channel(1);
            let (release, resume) = std::sync::mpsc::sync_channel(1);
            app.lock()
                .unwrap()
                .pause_next_persistence(Box::new(move || {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                        crate::application::AppError::Conflict("test storage pause expired")
                    })?;
                    if failure {
                        Err(crate::application::AppError::Invalid(
                            "injected storage failure",
                        ))
                    } else {
                        Ok(())
                    }
                }));
            let worker_app = app.clone();
            let worker_token = a.clone();
            let (finished, result) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::spawn(move || {
                let outcome = { worker_app.lock().unwrap().button(&worker_token, ButtonAction::Down) };
                finished.send(outcome).unwrap();
            });
            tokio::task::spawn_blocking(move || {
                waiting.recv_timeout(Duration::from_secs(2)).unwrap()
            })
            .await
            .unwrap();
            assert_eq!(
                query
                    .query_row::<String, _, _>("SELECT body FROM app_state WHERE id=1", [], |row| {
                        row.get(0)
                    })
                    .unwrap(),
                old_body,
                "SQLite update is not committed yet"
            );
            assert_eq!(
                app.read()
                    .unwrap()
                    .view()
                    .devices
                    .iter()
                    .find(|d| d.id == "a")
                    .unwrap()
                    .selected
                    .as_deref(),
                Some("persist-video")
            );
            assert!(
                !notifications.has_changed().unwrap(),
                "notification cannot precede publication"
            );
            let response = tokio::time::timeout(
                Duration::from_millis(500),
                web::router(app.clone()).oneshot(
                    axum::http::Request::builder()
                        .uri("/api/state")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let mut offer = app
                .read()
                .unwrap()
                .playback_offers()
                .into_iter()
                .find(|offer| offer.plugin == "persist-video")
                .unwrap();
            offer.targets.retain(|target| target.token.device_id == "b");
            let mut frame_offset = recorded.lock().unwrap().len();
            let mut current = Some(b.clone());
            for sequence in 1..=3 {
                assert_eq!(
                    app.accept_playback_pixels_at(&offer, vec![3; 25600], sequence)
                        .unwrap(),
                    1
                );
                if profile == "push" {
                    let expected = 1 + sequence as usize * 8;
                    tokio::time::timeout(Duration::from_secs(2), async {
                        while count.load(Ordering::SeqCst) < expected {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                } else {
                    let (frame, end) = wait_persist_frame(
                        &recorded,
                        frame_offset,
                        if profile == "eink" { 3813 } else { 25600 },
                    )
                    .await;
                    frame_offset = end;
                    persist_ack(&app, &mut current, frame);
                }
            }
            assert!(
                result.try_recv().is_err(),
                "the storage owner must still be paused"
            );
            // Legacy/native frame completion and ACK are real published live
            // transitions. Separate them from the pending durable result.
            notifications.borrow_and_update();
            release.send(()).unwrap();
            let applied = tokio::task::spawn_blocking(move || {
                result.recv_timeout(Duration::from_secs(3)).unwrap()
            })
            .await
            .unwrap();
            worker.join().unwrap();
            assert_eq!(applied.is_err(), failure);
            let expected = if failure {
                "persist-video"
            } else {
                "persist-next"
            };
            let view = app.read().unwrap();
            assert_eq!(
                view.view()
                    .devices
                    .iter()
                    .find(|d| d.id == "a")
                    .unwrap()
                    .selected
                    .as_deref(),
                Some(expected)
            );
            if !failure {
                tokio::time::timeout(Duration::from_secs(2), notifications.changed())
                    .await
                    .unwrap()
                    .unwrap();
                if profile != "legacy" {
                    drop(view);
                    let mut guard = app.lock().unwrap();
                    let rx = guard.subscribe_lcd_video(&a).unwrap();
                    let frame = rx.borrow().as_ref().unwrap().clone();
                    assert_eq!(frame.delivery, crate::lcd_stream::DeliveryKind::Ordinary);
                    assert_eq!(frame.pixels.len(), if profile == "eink" { 3813 } else { 25600 });
                    assert!(guard.lcd_video_is_current(&frame));
                } else {
                    let (token, handle) = view
                        .pending_effects()
                        .into_iter()
                        .find_map(|(_, effect)| match effect {
                            Effect::SendFrame {
                                token,
                                asset_handle,
                            } if token.device_id == "a" => Some((token, asset_handle)),
                            _ => None,
                        })
                        .unwrap();
                    assert!(view.frame_is_current(&token));
                    assert!(view.frame_asset(&handle).is_some(), "publication includes prepared pixels");
                }
            } else {
                assert!(!notifications.has_changed().unwrap());
                assert_eq!(
                    query
                        .query_row::<String, _, _>(
                            "SELECT body FROM app_state WHERE id=1",
                            [],
                            |row| { row.get(0) }
                        )
                        .unwrap(),
                    old_body
                );
            }
            tx.send(WriterCommand::Stop).await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), writer)
                    .await
                    .unwrap()
                    .unwrap(),
                "requested_stop"
            );
            let restored = App::open_with_plugin_set(
                &db,
                temp.path(),
                Arc::new(crate::fetch::SystemClock),
                persistence_composition(),
            )
            .unwrap();
            assert_eq!(
                restored
                    .view()
                    .devices
                    .iter()
                    .find(|d| d.id == "a")
                    .unwrap()
                    .selected
                    .as_deref(),
                Some(expected)
            );
        }
    }
}
#[test]
fn owner_panic_does_not_silently_publish_or_lend_mutable_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = App::open_with_plugin_set(
        &temp.path().join("db"),
        temp.path(),
        Arc::new(crate::fetch::SystemClock),
        persistence_composition(),
    )
    .unwrap();
    let app = crate::shared::Application::new(state);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _owner = app.lock().unwrap();
        panic!("injected owner panic");
    }))
    .is_err());
    assert!(app.read().is_err());
    assert!(app.lock().is_err());
}

async fn wait_persist_frame(
    recorded: &Arc<std::sync::Mutex<Vec<Message>>>,
    app: &SharedApp,
    from: usize,
    expected_bytes: usize,
) -> (crate::FrameToken, usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let rows = recorded.lock().unwrap().clone();
            let mut assembler = FrameAssembler::default();
            let mut collecting = false;
            for (index, row) in rows.iter().enumerate().skip(from) {
                if let Message::Binary(bytes) = row {
                    for message in Decoder::default().push(bytes).unwrap() {
                        match &message {
                            WireMessage::FrameStart { token, .. } => {
                                collecting = app.read().unwrap().frame_is_current(token);
                                assembler.reset();
                            }
                            WireMessage::FrameChunk { .. } => {}
                            _ => continue,
                        }
                        if collecting {
                            if let Some((token, pixels)) = assembler.accept(message).unwrap() {
                                assert_eq!(pixels.len(), expected_bytes);
                                return (token, index + 1);
                            }
                        }
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("static frame must complete while SQLite is paused")
}
fn persist_ack(app: &SharedApp, current: &mut Option<SessionToken>, frame: crate::FrameToken) {
    let mut diagnostics = AckDiagnostics::default();
    process(
        app,
        current,
        WireMessage::ReceiptAck {
            token: frame.clone(),
        },
        &mut diagnostics,
    )
    .unwrap();
    process(
        app,
        current,
        WireMessage::RenderedAck { token: frame },
        &mut diagnostics,
    )
    .unwrap();
}
fn persist_finish_state_frames(state: &mut App, id: &str) {
    for _ in 0..12 {
        let next = state
            .pending_effects()
            .into_iter()
            .find_map(|(id, e)| match e {
                crate::Effect::SendFrame { token, .. } => Some((id, token)),
                _ => None,
            });
        let Some((effect, frame)) = next else {
            return;
        };
        state.complete_effect(effect).unwrap();
        state
            .terminal_event(crate::Event::ReceiptAck {
                token: frame.clone(),
            })
            .unwrap();
        state
            .terminal_event(crate::Event::RenderedAck { token: frame })
            .unwrap();
    }
    panic!("unexpected frames for {id}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn sqlite_wait_allows_native_static_browse_ack_and_merges_fresh_latest_with_old_pin() {
    for screen in ["lcd", "eink"] {
        for plugin in ["weather", "rain-radar"] {
            for failure in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let db = temp.path().join("db");
                let mut state = app_at_fixture_date(&db);
                let token = state
                    .hello(
                        "browse",
                        [
                            screen.into(),
                            crate::protocol::FRAME_BYTES_CAPABILITY.into(),
                        ]
                        .into(),
                        1,
                    )
                    .unwrap();
                for _ in 0..10 {
                    if state.selection_for(&token).unwrap().0.as_deref() == Some(plugin) {
                        break;
                    }
                    state.button(&token, ButtonAction::Down).unwrap();
                }
                let source = if plugin == "weather" {
                    include_bytes!("../assets/jma-tokyo-20261012.json").as_slice()
                } else {
                    include_bytes!("../assets/jma-radar-20261012.json").as_slice()
                };
                let first = state
                    .plan_plugin_fetches(1791763200, "2026-10-12")
                    .unwrap()
                    .into_iter()
                    .find(|j| j.plugin == plugin)
                    .unwrap();
                state
                    .complete_plugin_fetch(&first, 1791763201, Ok(source.to_vec()))
                    .unwrap();
                persist_finish_state_frames(&mut state, "browse");
                for (id, _) in state.pending_effects() {
                    state.complete_effect(id).unwrap();
                }
                let fresh = state
                    .plan_plugin_fetches(1791766802, "2026-10-12")
                    .unwrap()
                    .into_iter()
                    .find(|j| j.plugin == plugin)
                    .unwrap();
                let prepared =
                    state.prepare_plugin_fetch(&fresh).unwrap().unwrap()(Ok(source.to_vec()))
                        .unwrap();
                let app = Arc::new(crate::shared::Application::new(state));
                let count = Arc::new(AtomicUsize::new(0));
                let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
                let (tx, rx) = tokio::sync::mpsc::channel(32);
                let writer = tokio::spawn(writer_loop(
                    MockSink {
                        count: count.clone(),
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
                tokio::time::timeout(Duration::from_secs(2), async {
                    while count.load(Ordering::SeqCst) < 1 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                let query = rusqlite::Connection::open(&db).unwrap();
                let old_body: String = query
                    .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
                    .unwrap();
                let (entered, waiting) = std::sync::mpsc::sync_channel(1);
                let (release, resume) = std::sync::mpsc::sync_channel(1);
                app.lock()
                    .unwrap()
                    .pause_next_persistence(Box::new(move || {
                        entered.send(()).unwrap();
                        resume.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                            crate::application::AppError::Conflict("test pause expired")
                        })?;
                        if failure {
                            Err(crate::application::AppError::Invalid(
                                "injected save failure",
                            ))
                        } else {
                            Ok(())
                        }
                    }));
                let worker_app = app.clone();
                let job = fresh.clone();
                let (done, result) = std::sync::mpsc::sync_channel(1);
                let worker = std::thread::spawn(move || {
                    let value = worker_app
                        .lock()
                        .unwrap()
                        .apply_prepared_plugin_fetch(&job, 1791766803, prepared);
                    done.send(value).unwrap();
                });
                tokio::task::spawn_blocking(move || {
                    waiting.recv_timeout(Duration::from_secs(2)).unwrap()
                })
                .await
                .unwrap();
                let mut current = Some(token.clone());
                let mut diagnostics = AckDiagnostics::default();
                let mut offset = recorded.lock().unwrap().len();
                let observations = app.read().unwrap().observation_source();
                observations.enable(true);
                let cause =
                    cfg!(feature = "boundary-observation").then_some(crate::observation::Cause {
                        version: 1,
                        boot: 900,
                        operation: 1,
                    });
                let started = std::time::Instant::now();
                let sub = process_with_observation(
                    &app,
                    &mut current,
                    WireMessage::EnterSub {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    &mut diagnostics,
                    None,
                    cause,
                )
                .unwrap();
                assert!(started.elapsed() < Duration::from_millis(500));
                assert!(
                    matches!(sub,Some(WireMessage::PluginPosition{ref dataset_id,..})if dataset_id==&first.dataset_id)
                );
                let (frame, end) = wait_persist_frame(
                    &recorded,
                    &app,
                    offset,
                    if screen == "eink" { 3813 } else { 25600 },
                )
                .await;
                offset = end;
                persist_ack(&app, &mut current, frame.clone());
                #[cfg(feature = "boundary-observation")]
                {
                    let facts = observations.drain();
                    assert!(facts.iter().any(|f| f.cause == cause
                        && f.component == crate::observation::Component::State
                        && f.phase == crate::observation::Phase::Committed
                        && f.argument == 2));
                    assert!(recorded.lock().unwrap().iter().flat_map(|row|match row{Message::Binary(bytes)=>Decoder::default().push_observed(bytes).unwrap(),_=>Vec::new()})
                .any(|row|row.cause==cause && matches!(row.message,WireMessage::FrameStart{token,..}if token.frame_id==frame.frame_id)));
                }
                let move_button = if plugin == "weather" {
                    WireMessage::Down {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    }
                } else {
                    WireMessage::Up {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    }
                };
                let before = app.read().unwrap().plugin_position(&token).unwrap();
                process(&app, &mut current, move_button, &mut diagnostics).unwrap();
                let expected = app.read().unwrap().plugin_position(&token).unwrap();
                assert_ne!(
                    expected.target_date, before.target_date,
                    "fixture needs an available next browse time"
                );
                let (frame, end) = wait_persist_frame(
                    &recorded,
                    &app,
                    offset,
                    if screen == "eink" { 3813 } else { 25600 },
                )
                .await;
                offset = end;
                persist_ack(&app, &mut current, frame.clone());
                assert_eq!(
                    app.read().unwrap().view().devices[0].last_rendered_frame,
                    Some(frame.frame_id)
                );
                assert_eq!(
                    query
                        .query_row::<String, _, _>(
                            "SELECT body FROM app_state WHERE id=1",
                            [],
                            |r| r.get(0)
                        )
                        .unwrap(),
                    old_body,
                    "ACK/browse/delivery cannot write SQLite"
                );
                assert!(
                    result.try_recv().is_err(),
                    "durable worker must remain paused"
                );
                release.send(()).unwrap();
                let outcome = tokio::task::spawn_blocking(move || {
                    result.recv_timeout(Duration::from_secs(3)).unwrap()
                })
                .await
                .unwrap();
                worker.join().unwrap();
                assert_eq!(outcome.is_err(), failure);
                let after = app.read().unwrap().plugin_position(&token).unwrap();
                assert_eq!(after.dataset_id, expected.dataset_id);
                assert_eq!(after.target_date, expected.target_date);
                assert_eq!(app.read().unwrap().session_mode(&token), Some("sub"));
                // An obsolete normal frame prepared by the durable owner must not
                // overwrite the pin; cancel then selects its new latest on success.
                process(
                    &app,
                    &mut current,
                    WireMessage::Power {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    &mut diagnostics,
                )
                .unwrap();
                let normal = app.read().unwrap().plugin_position(&token).unwrap();
                assert_eq!(
                    normal.dataset_id,
                    if failure {
                        first.dataset_id.clone()
                    } else {
                        fresh.dataset_id.clone()
                    }
                );
                let (frame, _) = wait_persist_frame(
                    &recorded,
                    &app,
                    offset,
                    if screen == "eink" { 3813 } else { 25600 },
                )
                .await;
                persist_ack(&app, &mut current, frame);
                tx.send(WriterCommand::Stop).await.unwrap();
                assert_eq!(writer.await.unwrap(), "requested_stop");
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn sqlite_wait_normal_ack_still_delivers_fresh_native_weather_after_commit() {
    for screen in ["lcd", "eink"] {
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
                resume
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| crate::application::AppError::Conflict("test pause expired"))?;
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
        tokio::task::spawn_blocking(move || waiting.recv_timeout(Duration::from_secs(2)).unwrap())
            .await
            .unwrap();
        let mut current = Some(token.clone());
        persist_ack(&app, &mut current, old_frame.clone());
        assert_eq!(
            app.read().unwrap().view().devices[0].last_rendered_frame,
            Some(old_frame.frame_id)
        );
        release.send(()).unwrap();
        tokio::task::spawn_blocking(move || worker.join().unwrap())
            .await
            .unwrap();
        let (new_frame, _) = wait_persist_frame(&recorded, offset, bytes).await;
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
