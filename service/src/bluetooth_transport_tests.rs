//! BT-AUDIO-20261107: writer control remains live while durable application work waits.
//! Pure in-process sink and temporary SQLite fixture; no network/device IO.
use super::*;

async fn stop_while_durable_lease_is_held(bluetooth: bool) {
    let temp = tempfile::tempdir().unwrap();
    let mut state = crate::application::App::open(&temp.path().join("writer.sqlite")).unwrap();
    let mut capabilities = std::collections::BTreeSet::from(["lcd".to_owned()]);
    if bluetooth {
        capabilities.insert(crate::protocol::BLUETOOTH_AUDIO_CAPABILITY.to_owned());
    }
    let session = state.hello("writer-fixture", capabilities, 1).unwrap();
    let app = std::sync::Arc::new(crate::shared::Application::new(state));

    // A real OS thread owns the lease so a regression that blocks a Tokio worker
    // can still be released after the watchdog. Never leave a hanging test task.
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let owner = app.clone();
    let holder = std::thread::spawn(move || {
        let lease = owner.lock().unwrap();
        held_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(std::time::Duration::from_secs(5));
        drop(lease);
    });
    held_rx.await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let mut task = tokio::spawn(writer_loop(futures_util::sink::drain(), rx, app));
    tx.send(WriterCommand::Bind(session)).await.unwrap();
    // Don't queue Stop beside Bind: that would bypass the refresh branch and
    // fail to reproduce a blocking app.lock() in Bluetooth command dispatch.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        tx.send(WriterCommand::Stop).await.unwrap();
        (&mut task).await
    })
    .await;
    // Release only after observing the result: success cannot depend on storage
    // work handing back its lease. Release on failure too, for deterministic cleanup.
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    match result {
        Ok(joined) => assert_eq!(joined.unwrap(), "requested_stop"),
        Err(_) => {
            task.abort();
            let _ = task.await;
            panic!("writer Stop exceeded 500 ms with Bluetooth capability = {bluetooth}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bluetooth_writer_stop_does_not_wait_for_durable_lease() {
    stop_while_durable_lease_is_held(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_writer_stop_does_not_wait_for_durable_lease() {
    stop_while_durable_lease_is_held(false).await;
}

#[path = "audio_barrier_sink_tests.rs"]
mod audio_barrier_sink_tests;

#[path = "sbc_sender_sink_tests.rs"]
mod sbc_sender_sink_tests;
