// In-memory fixtures only. Fixture arrival knowledge is never an observation field.
#[tokio::test]
async fn network_ws_decode_api_parent_survives_split_message() {
    use crate::observation::{Component,NetworkBoundary as N,Phase};
    let temp=tempfile::tempdir().unwrap();
    let state=crate::test_support::manual_frames_app(&temp.path().join("db")).unwrap();
    let source=state.observation_source();source.enable(true);
    let app=Arc::new(crate::shared::Application::new(state));
    let hello=crate::protocol::encode(&WireMessage::Hello{device_id:"gap-fixture".into(),capabilities:vec!["lcd".into()],screen_owner_epoch:1}).unwrap();
    // A single application envelope crosses two WS binary messages. The trace
    // identifies the completing decode call, not a guessed TCP packet parent.
    let split=hello.len()/2;
    let incoming=futures_util::stream::iter(vec![Ok::<_,()>(Message::Binary(hello[..split].to_vec().into())),Ok(Message::Binary(hello[split..].to_vec().into()))]);
    serve_split(MockSink{count:Arc::new(AtomicUsize::new(0)),limit:usize::MAX,recorded:None},incoming,app).await;
    let facts=source.drain();
    let ws:Vec<_>=facts.iter().filter(|f|f.detail==N::WsComplete as u16).collect();assert_eq!(ws.len(),2);
    assert!(ws.iter().all(|f|f.parent==0&&f.cause.is_none()));
    let decode:Vec<_>=facts.iter().filter(|f|f.detail==N::Decode as u16&&f.phase==Phase::Dispatched).collect();assert_eq!(decode.len(),2);
    assert_eq!((decode[1].first,decode[1].end),(0,hello.len() as u64));assert_eq!(decode[1].parent,ws[1].operation);
    let api=facts.iter().find(|f|f.component==Component::Api&&f.phase==Phase::Received).unwrap();
    assert_eq!(api.parent,decode[1].operation);
    let dispatch=facts.iter().find(|f|f.component==Component::Api&&f.phase==Phase::Dispatched).unwrap();
    assert_eq!(dispatch.parent,api.operation);
    let queue=facts.iter().find(|f|f.detail==N::ReplyQueue as u16&&f.phase==Phase::Pending).unwrap();
    assert_eq!(queue.parent,api.operation);
}

#[tokio::test]
async fn network_reply_capacity_and_sink_wait_are_distinct_boundaries() {
    use crate::observation::{Source,NetworkBoundary as N,Phase};
    let source=Source::default();source.enable(true);
    let (tx,mut rx)=tokio::sync::mpsc::channel(1);
    tx.send(WriterCommand::Stop).await.unwrap();
    let producer=tokio::spawn({let tx=tx.clone();let source=source.clone();async move {
        enqueue_reply_traced(&tx,WireMessage::Error{code:"fixture".into()},0,None,None,
            Some(ReplyObservation{source,connection:123,parent:900,operation:0})).await.unwrap();
    }});
    tokio::time::timeout(Duration::from_secs(2),async {while source.status().sequence==0 {tokio::task::yield_now().await;}}).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let before=source.drain();assert_eq!(before.len(),1);
    assert_eq!((before[0].detail,before[0].phase),(N::ReplyQueue as u16,Phase::Pending));
    assert!(!producer.is_finished());assert!(matches!(rx.recv().await,Some(WriterCommand::Stop)));
    producer.await.unwrap();let enqueued=source.drain();assert_eq!(enqueued.len(),1);
    assert_eq!((enqueued[0].detail,enqueued[0].phase),(N::ReplyQueue as u16,Phase::Completed));
    assert!(enqueued[0].us-before[0].us>=15_000);
    let temp=tempfile::tempdir().unwrap();
    let state=crate::test_support::manual_frames_app(&temp.path().join("db")).unwrap();
    let app=Arc::new(crate::shared::Application::new(state));
    let writer=tokio::spawn(writer_loop(MockSink{count:Arc::new(AtomicUsize::new(0)),limit:0,recorded:None},rx,app));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let waiting=source.drain();assert_eq!(waiting.len(),2);
    assert_eq!((waiting[0].detail,waiting[0].phase),(N::WriterDequeue as u16,Phase::Received));
    assert_eq!((waiting[1].detail,waiting[1].phase),(N::ReplySink as u16,Phase::Dispatched));
    assert!(!writer.is_finished());assert_eq!(writer.await.unwrap(),"control_write_failed");
    let end=source.drain();assert_eq!(end.len(),1);
    assert_eq!((end[0].detail,end[0].phase),(N::ReplySink as u16,Phase::Error));
    assert!(end[0].us-waiting[1].us>=200_000);
    for fact in before.iter().chain(&enqueued).chain(&waiting).chain(&end) {
        assert_eq!((fact.connection,fact.parent),(123,900));assert_eq!(fact.operation,before[0].operation);
    }
}
