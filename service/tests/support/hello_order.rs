// Included inside transport::tests::hello_order; all I/O here is in-memory.
fn hello(id: &str) -> Message {
    Message::Binary(
        encode(&WireMessage::Hello {
            device_id: id.into(),
            screen_owner_epoch: 1,
            capabilities: vec![
                "lcd".into(),
                crate::protocol::FRAME_BYTES_CAPABILITY.into(),
                crate::protocol::LCD_PUSH_CAPABILITY.into(),
            ],
        })
        .unwrap()
        .into(),
    )
}
async fn until(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("deterministic in-memory transport must make progress");
}
fn decoded(recorded: &Arc<std::sync::Mutex<Vec<Message>>>) -> Vec<WireMessage> {
    recorded
        .lock()
        .unwrap()
        .iter()
        .flat_map(|message| match message {
            Message::Binary(bytes) => Decoder::default().push(bytes).unwrap(),
            _ => vec![],
        })
        .collect()
}
fn completed_video_sequences(recorded: &Arc<std::sync::Mutex<Vec<Message>>>) -> Vec<u64> {
    let mut assembler = FrameAssembler::default();
    let mut sequence = None;
    let mut completed = Vec::new();
    for message in decoded(recorded) {
        match &message {
            WireMessage::FrameStart { stream, .. } => {
                sequence = stream.as_ref().map(|s| s.sequence);
            }
            WireMessage::FrameChunk { .. } => {}
            _ => continue,
        }
        if let Some((_token, pixels)) = assembler.accept(message).unwrap() {
            assert_eq!(pixels.len(), 25600);
            if let Some(sequence) = sequence {
                completed.push(sequence);
            }
        }
    }
    completed
}
struct FailingReplySink;
impl Sink<Message> for FailingReplySink {
    type Error = ();
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), ()> {
        Err(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn delayed_older_hello_with_failed_reply_does_not_evict_active_video() {
    let temp = tempfile::tempdir().unwrap();
    let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
        &temp.path().join("state.db"),
    )));
    // The progress handles are created at simulated TCP accept. Older HTTP/WS
    // upgrade is delayed, so the newer WS handler and Hello run first.
    let older_accept = WriteProgress::default();
    let newer_accept = WriteProgress::default();
    let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
    let count = Arc::new(AtomicUsize::new(0));
    let (input, incoming) = tokio::sync::mpsc::channel::<Result<Message, ()>>(4);
    let stream = futures_util::stream::unfold(incoming, |mut rx| async {
        rx.recv().await.map(|m| (m, rx))
    });
    let active = tokio::spawn(serve_split_with_progress(
        MockSink {
            count: count.clone(),
            limit: usize::MAX,
            recorded: Some(recorded.clone()),
        },
        Box::pin(stream),
        app.clone(),
        Some(newer_accept),
    ));
    input.send(Ok(hello("ordered-video"))).await.unwrap();
    until(|| count.load(Ordering::SeqCst) >= 2).await;
    let token = decoded(&recorded)
        .into_iter()
        .find_map(|message| match message {
            WireMessage::Welcome {
                incarnation,
                session_epoch,
            } => Some(SessionToken {
                device_id: "ordered-video".into(),
                incarnation,
                session_epoch,
            }),
            _ => None,
        })
        .unwrap();
    app.lock()
        .unwrap()
        .queue_lcd_video(&token, 12, 1, 41, vec![1; 25600])
        .unwrap();
    until(|| completed_video_sequences(&recorded).contains(&1)).await;
    let stale = tokio::spawn(serve_split_with_progress(
        FailingReplySink,
        Box::pin(
            futures_util::stream::once(async { Ok::<_, ()>(hello("ordered-video")) })
                .chain(futures_util::stream::pending()),
        ),
        app.clone(),
        Some(older_accept),
    ));
    let why = tokio::time::timeout(Duration::from_secs(2), stale)
        .await
        .unwrap()
        .unwrap();
    assert!(
        app.lock().unwrap().session_is_current(&token),
        "late older Hello evicted the active newer connection before its failed reply"
    );
    assert_eq!(why, "protocol_rejected");
    assert!(!active.is_finished());
    app.lock()
        .unwrap()
        .queue_lcd_video(&token, 12, 2, 41, vec![2; 25600])
        .unwrap();
    until(|| completed_video_sequences(&recorded).contains(&2)).await;
    drop(input);
    tokio::time::timeout(Duration::from_secs(2), active)
        .await
        .unwrap()
        .unwrap();
}

include!("hello_order_matrix.rs");
