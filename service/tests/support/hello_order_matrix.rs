fn order(n: u64) -> std::num::NonZeroU64 {
    std::num::NonZeroU64::new(n).unwrap()
}
fn capabilities() -> std::collections::BTreeSet<String> {
    ["lcd".into()].into()
}
fn is_stale(result: Result<SessionToken, crate::application::AppError>) {
    assert!(matches!(
        result,
        Err(crate::application::AppError::Conflict("stale_connection"))
    ));
}

#[test]
fn newer_reconnect_can_replace_online_then_recover_after_current_disconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_at_fixture_date(&tmp.path().join("state.db"));
    let first = app
        .hello_from_connection("desk", capabilities(), 99, order(10))
        .unwrap();
    // A reboot may reset device owner_epoch. Only the server accept order is
    // compared; valid new connections must not be rejected by that reset.
    let second = app
        .hello_from_connection("desk", capabilities(), 1, order(20))
        .unwrap();
    assert!(second.session_epoch > first.session_epoch);
    assert!(!app.disconnect(first).unwrap());
    assert!(app.session_is_current(&second));
    assert!(app.disconnect(second).unwrap());
    let third = app
        .hello_from_connection("desk", capabilities(), 1, order(30))
        .unwrap();
    assert!(app.session_is_current(&third));
}

#[test]
fn disconnect_does_not_allow_old_pending_or_equal_order_hello_to_resurrect() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_at_fixture_date(&tmp.path().join("state.db"));
    let token = app
        .hello_from_connection("desk", capabilities(), 1, order(20))
        .unwrap();
    assert!(app.disconnect(token).unwrap());
    let before = serde_json::to_value(app.view()).unwrap();
    let effects = format!("{:?}", app.pending_effects());
    for stale in [10, 20] {
        is_stale(app.hello_from_connection("desk", capabilities(), 2, order(stale)));
        assert_eq!(serde_json::to_value(app.view()).unwrap(), before);
        assert_eq!(format!("{:?}", app.pending_effects()), effects);
    }
    assert!(app
        .hello_from_connection("desk", capabilities(), 1, order(21))
        .is_ok());
}

#[test]
fn invalid_hello_does_not_poison_accept_order_for_valid_reconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_at_fixture_date(&tmp.path().join("state.db"));
    app.hello_from_connection("desk", capabilities(), 1, order(10))
        .unwrap();
    assert!(matches!(
        app.hello_from_connection("desk", [].into(), 1, order(100)),
        Err(crate::application::AppError::Invalid(_))
    ));
    assert!(app
        .hello_from_connection("desk", capabilities(), 1, order(20))
        .is_ok());
}

#[test]
fn different_device_ids_have_independent_accept_order() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_at_fixture_date(&tmp.path().join("state.db"));
    let a = app
        .hello_from_connection("A", capabilities(), 1, order(100))
        .unwrap();
    let b = app
        .hello_from_connection("B", capabilities(), 1, order(2))
        .unwrap();
    assert!(app.session_is_current(&a) && app.session_is_current(&b));
    assert!(app
        .hello_from_connection("B", capabilities(), 1, order(3))
        .is_ok());
    assert!(app.session_is_current(&a));
}

#[test]
fn restart_reloads_session_epoch_but_not_process_local_accept_order() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state.db");
    let old = {
        let mut app = app_at_fixture_date(&path);
        app.hello_from_connection("desk", capabilities(), 1, order(1000))
            .unwrap()
    };
    // Models a new process: old sockets are gone, the process counter may
    // start at1, and App restores persistent session epochs as offline.
    let mut app = app_at_fixture_date(&path);
    assert!(!app.view().devices[0].online);
    let fresh = app
        .hello_from_connection("desk", capabilities(), 1, order(1))
        .unwrap();
    assert_eq!(fresh.incarnation, old.incarnation);
    assert!(fresh.session_epoch > old.session_epoch);
    assert!(!app.disconnect(old).unwrap());
    assert!(app.session_is_current(&fresh));
}

#[test]
fn concurrent_valid_hellos_end_with_newest_accept_order() {
    for _ in 0..24 {
        let tmp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &tmp.path().join("state.db"),
        )));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let spawn = |n| {
            let (app, barrier) = (app.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let result =
                    app.lock()
                        .unwrap()
                        .hello_from_connection("desk", capabilities(), 1, order(n));
                result
            })
        };
        let older = spawn(10);
        let newer = spawn(20);
        let old = older.join().unwrap();
        let new = newer.join().unwrap().unwrap();
        let app = app.lock().unwrap();
        assert!(app.session_is_current(&new));
        match old {
            Ok(token) => assert!(!app.session_is_current(&token)),
            Err(crate::application::AppError::Conflict("stale_connection")) => {}
            other => panic!("unexpected result {other:?}"),
        }
    }
}

#[tokio::test]
async fn failed_newest_welcome_does_not_let_an_older_socket_revive() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
        &tmp.path().join("state.db"),
    )));
    let old = WriteProgress::default();
    let new = WriteProgress::default();
    let old_token = app
        .lock()
        .unwrap()
        .hello_from_connection("desk", capabilities(), 1, old.accept_order())
        .unwrap();
    let task = tokio::spawn(serve_split_with_progress(
        FailingReplySink,
        Box::pin(
            futures_util::stream::once(async { Ok::<_, ()>(hello("desk")) })
                .chain(futures_util::stream::pending()),
        ),
        app.clone(),
        Some(new),
    ));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        "writer_ended"
    );
    let mut app = app.lock().unwrap();
    assert!(!app.session_is_current(&old_token));
    assert!(!app.view().devices[0].online);
    is_stale(app.hello_from_connection("desk", capabilities(), 1, old.accept_order()));
    let next = crate::socket_progress::next_accept_order();
    let fresh = app
        .hello_from_connection("desk", capabilities(), 1, next)
        .unwrap();
    assert!(app.session_is_current(&fresh));
}
