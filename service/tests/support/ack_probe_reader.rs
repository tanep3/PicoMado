async fn switch_probe_static_socket_for(device_id:&str) -> (
    SharedApp,
    SessionToken,
    crate::FrameToken,
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
) {
    let temp = tempfile::tempdir().unwrap();
    let app = Arc::new(crate::shared::Application::new(app_at_fixture_date(
        &temp.path().join("state.db"),
    )));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_app = app.clone();
    let task = tokio::spawn(async move {
        axum::serve(
            crate::socket_progress::TrackedListener(listener),
            web::router(server_app).into_make_service_with_connect_info::<WriteProgress>(),
        )
        .await
        .unwrap();
    });
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/device"))
        .await
        .unwrap();
    ws.send(ClientMessage::Binary(
        encode(&WireMessage::Hello {
            device_id: device_id.into(),
            screen_owner_epoch: 1,
            capabilities: vec!["lcd".into(), crate::protocol::FRAME_BYTES_CAPABILITY.into()],
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let token = match read_wire(&mut ws).await {
        WireMessage::Welcome {
            incarnation,
            session_epoch,
        } => SessionToken {
            device_id: device_id.into(),
            incarnation,
            session_epoch,
        },
        m => panic!("{m:?}"),
    };
    assert!(matches!(
        read_wire(&mut ws).await,
        WireMessage::Settings { .. }
    ));
    let frame = app
        .lock()
        .unwrap()
        .queue_lcd_frame(&token, "youtube", vec![7; 25600])
        .unwrap();
    let mut assembler = FrameAssembler::default();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let m = read_wire(&mut ws).await;
            if matches!(
                m,
                WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
            ) && assembler.accept(m).unwrap().is_some()
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    ws.send(ClientMessage::Binary(
        encode(&WireMessage::ReceiptAck {
            token: frame.clone(),
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while app.lock().unwrap().view().devices[0].last_received_frame != Some(frame.frame_id) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(app
        .lock()
        .unwrap()
        .frame_waiting_for_render(&token, frame.frame_id));
    (app, token, frame, ws, task, temp)
}
async fn switch_probe_static_socket()->(SharedApp,SessionToken,crate::FrameToken,tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,tokio::task::JoinHandle<()>,tempfile::TempDir){switch_probe_static_socket_for("ack-read-probe").await}
#[tokio::test]
async fn switch_probe_read_loop_accepts_up_before_static_render_ack() {
    let (app, token, frame, mut ws, server, _temp) = switch_probe_static_socket().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(app
        .lock()
        .unwrap()
        .frame_waiting_for_render(&token, frame.frame_id));
    let epoch = app.lock().unwrap().selection_for(&token).unwrap().1;
    let start = std::time::Instant::now();
    ws.send(ClientMessage::Binary(
        encode(&WireMessage::Up {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if matches!(read_wire(&mut ws).await, WireMessage::InputAccepted { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(app.lock().unwrap().selection_for(&token).unwrap().1 > epoch);
    assert!(!app
        .lock()
        .unwrap()
        .frame_waiting_for_render(&token, frame.frame_id));
    assert!(app.lock().unwrap().session_is_current(&token));
    println!("static RenderedAck intentionally absent; real socket UP reply_ms={} before10sdeadline=true",start.elapsed().as_millis());
    drop(ws);
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn switch_probe_read_loop_handles_heartbeat_while_static_timeout_counts() {
    let (app, token, frame, mut ws, server, _temp) = switch_probe_static_socket().await;
    let start = std::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(app
        .lock()
        .unwrap()
        .frame_waiting_for_render(&token, frame.frame_id));
    ws.send(ClientMessage::Binary(
        encode(&WireMessage::SettingsStored {
            incarnation: token.incarnation,
            revision: 1,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    ws.send(ClientMessage::Binary(
        encode(&WireMessage::Heartbeat {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
            applied_revision: 1,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(500), async {
        while app.lock().unwrap().view().devices[0].stored_revision != 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(app
        .lock()
        .unwrap()
        .frame_waiting_for_render(&token, frame.frame_id));
    println!(
        "SettingsStored processed while RenderedAck absent; elapsed_ms={}",
        start.elapsed().as_millis()
    );
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            match ws.next().await {
                Some(Ok(ClientMessage::Close(_))) | None => break,
                Some(Err(e)) => panic!("unexpected socket error {e}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_secs(9) && elapsed < Duration::from_secs(12));
    println!(
        "unacknowledged static frame closed at_ms={} (10s budget plus 1s tick granularity)",
        elapsed.as_millis()
    );
    server.abort();
    let _ = server.await;
}

#[cfg(feature="ack-path-probe")]
#[tokio::test(flavor="multi_thread",worker_threads=3)]
async fn ack_probe_trace_raw_read_decode_and_blocked_application() {
    let evidence=tempfile::tempdir().unwrap();
    crate::ack_probe::test_start("ack-path-trace-regression",&evidence.path().join("probe.json"));
    let (app,token,_frame,mut ws,server,_temp)=switch_probe_static_socket_for("ack-path-trace-regression").await;
    // Arm the already accepted connection: no service/WS restart is needed.
    let idle=crate::ack_probe::test_control(serde_json::json!({"op":"status"}));
    assert_eq!(idle["phase"],"idle");assert!(idle["connection"].as_u64().unwrap()>0);
    let armed=crate::ack_probe::test_control(serde_json::json!({"op":"arm","boot":idle["boot"],"run":"1"}));
    assert_eq!(armed["phase"],"armed");assert_eq!(armed["connection"],idle["connection"]);
    let (held_tx,held_rx)=tokio::sync::oneshot::channel();
    let blocker_app=app.clone();
    let blocker=std::thread::spawn(move||{let _guard=blocker_app.lock().unwrap();held_tx.send(()).unwrap();std::thread::sleep(Duration::from_millis(250));});
    held_rx.await.unwrap();
    let up=WireMessage::Up{incarnation:token.incarnation,session_epoch:token.session_epoch};
    let bytes=encode(&up).unwrap();let crc=crc32fast::hash(&bytes[6..]);
    ws.send(ClientMessage::Binary(bytes.into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2),async{loop{if matches!(read_wire(&mut ws).await,WireMessage::InputAccepted{..}){break;}}}).await.unwrap();
    blocker.join().unwrap();
    let rows=crate::ack_probe::test_records();
    use crate::ack_probe::Event;
    let decoded=rows.iter().find(|r|r.event==Event::Decoded&&r.kind==15).unwrap();
    assert_eq!(decoded.a,crc as u64);
    let begin=rows.iter().find(|r|r.event==Event::ProcessBegin&&r.message==decoded.message).unwrap();
    let done=rows.iter().find(|r|r.event==Event::ProcessDone&&r.message==decoded.message).unwrap();
    assert!(done.a>=150000,"positive control did not block application long enough");
    assert!(rows.iter().any(|r|r.event==Event::RawReady&&r.a>0&&r.us<=decoded.us));
    assert!(decoded.us<=begin.us&&begin.us<=done.us);
    assert!(rows.iter().any(|r|r.event==Event::ReplyQueueBegin&&r.message==decoded.message));
    println!("ACK_PROBE positive control: ordinal={} json_crc32={} raw-read/decode before process; blocked_process_us={} records={} record_bytes={}",decoded.message,decoded.a,done.a,rows.len(),std::mem::size_of::<crate::ack_probe::Record>());
    if let Some(path)=std::env::var_os("PM_ACK_PROBE_TEST_EVIDENCE"){std::fs::write(path,serde_json::to_vec_pretty(&rows).unwrap()).unwrap();}
    ack_probe_admission_regressions(&app,&ws).await;
    let frozen=crate::ack_probe::test_control(serde_json::json!({"op":"freeze","boot":idle["boot"],"run":"1"}));
    assert_eq!(frozen["phase"],"frozen");
    let page=crate::ack_probe::test_control(serde_json::json!({"op":"read","boot":idle["boot"],"run":"1","start":0,"limit":16}));
    assert!(page["count"].as_u64().unwrap()>0);assert_eq!(page["status"]["digest"],frozen["digest"]);
    if let Some(path)=std::env::var_os("PM_ACK_PROBE_TEST_EXPORT") {
        let mut pages=Vec::new();
        for start in (0..frozen["count"].as_u64().unwrap() as usize).step_by(16) {
            let page=crate::ack_probe::test_control(serde_json::json!({"op":"read","boot":idle["boot"],"run":"1","start":start,"limit":16}));
            assert_eq!(page["status"]["digest"],frozen["digest"]);pages.push(page);
        }
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"status":frozen,"pages":pages})).unwrap()).unwrap();
    }
    let rejected=crate::ack_probe::test_control(serde_json::json!({"op":"release","boot":idle["boot"],"run":"1","digest":frozen["digest"].as_u64().unwrap()^1}));
    assert_eq!(rejected["error"],"digest");
    let released=crate::ack_probe::test_control(serde_json::json!({"op":"release","boot":idle["boot"],"run":"1","digest":frozen["digest"]}));
    assert_eq!(released["phase"],"idle");
    let armed2=crate::ack_probe::test_control(serde_json::json!({"op":"arm","boot":idle["boot"],"run":"2"}));assert_eq!(armed2["phase"],"armed");
    let _=crate::ack_probe::test_control(serde_json::json!({"op":"freeze","boot":idle["boot"],"run":"2"}));

    drop(ws);server.abort();let _=server.await;
}

#[cfg(feature="ack-path-probe")]
async fn ack_probe_admission_regressions(app:&SharedApp,existing:&tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>){
 use crate::ack_probe::Event;
 let addr=match existing.get_ref(){tokio_tungstenite::MaybeTlsStream::Plain(socket)=>socket.peer_addr().unwrap(),_=>panic!("localhost plain socket expected")};
 let url=format!("ws://{addr}/device");
 let hello=WireMessage::Hello{device_id:"ack-path-trace-regression".into(),screen_owner_epoch:1,capabilities:vec!["lcd".into(),crate::protocol::FRAME_BYTES_CAPABILITY.into()]};
 let previous_connection=crate::ack_probe::test_records().iter().rev().find(|r|r.event==Event::Bind).unwrap().connection;
 let (mut late,_)=tokio_tungstenite::connect_async(&url).await.unwrap();
 let (mut current,_)=tokio_tungstenite::connect_async(&url).await.unwrap();
 current.send(ClientMessage::Binary(encode(&hello).unwrap().into())).await.unwrap();
 let (current_inc,current_session)=match read_wire(&mut current).await{WireMessage::Welcome{incarnation,session_epoch}=>(incarnation,session_epoch),other=>panic!("{other:?}")};assert!(matches!(read_wire(&mut current).await,WireMessage::Settings{..}));
 // Accepted Hello registration is nonblocking and may require its next message.
 let old_connection=previous_connection;
 let mut accepted_connection=old_connection;
 for _ in 0..50 {
  current.send(ClientMessage::Binary(encode(&WireMessage::Heartbeat{incarnation:current_inc,session_epoch:current_session,applied_revision:1}).unwrap().into())).await.unwrap();
  tokio::time::sleep(Duration::from_millis(2)).await;
  let candidate=crate::ack_probe::test_records().iter().rev().find(|r|r.event==Event::Bind).unwrap().connection;
  if candidate>old_connection {accepted_connection=candidate;break;}
 }
 assert!(accepted_connection>old_connection,"accepted Hello bind was not retried");
 let connection=accepted_connection;
 late.send(ClientMessage::Binary(encode(&hello).unwrap().into())).await.unwrap();assert!(matches!(read_wire(&mut late).await,WireMessage::Error{..}));assert!(crate::ack_probe::active(connection));
 let (mut invalid,_)=tokio_tungstenite::connect_async(&url).await.unwrap();
 invalid.send(ClientMessage::Binary(encode(&WireMessage::Hello{device_id:"ack-path-trace-regression".into(),screen_owner_epoch:1,capabilities:vec![]}).unwrap().into())).await.unwrap();
 assert!(matches!(read_wire(&mut invalid).await,WireMessage::Error{..}));assert!(crate::ack_probe::active(connection));
 let (mut newest,_)=tokio_tungstenite::connect_async(&url).await.unwrap();
 let (held_tx,held_rx)=tokio::sync::oneshot::channel();let holder=crate::ack_probe::test_hold_state(held_tx);held_rx.await.unwrap();
 newest.send(ClientMessage::Binary(encode(&hello).unwrap().into())).await.unwrap();
 let token=match read_wire(&mut newest).await{WireMessage::Welcome{incarnation,session_epoch}=>SessionToken{device_id:"ack-path-trace-regression".into(),incarnation,session_epoch},other=>panic!("{other:?}")};
 assert!(matches!(read_wire(&mut newest).await,WireMessage::Settings{..}));holder.join().unwrap();
 newest.send(ClientMessage::Binary(encode(&WireMessage::Heartbeat{incarnation:token.incarnation,session_epoch:token.session_epoch,applied_revision:1}).unwrap().into())).await.unwrap();
 let rebound=tokio::time::timeout(Duration::from_secs(1),async{loop{let conn=crate::ack_probe::test_records().iter().rev().find(|r|r.event==Event::Bind).unwrap().connection;if conn>connection{break conn;}tokio::time::sleep(Duration::from_millis(1)).await;}}).await.unwrap();
 assert!(crate::ack_probe::active(rebound)&&app.lock().unwrap().session_is_current(&token));
 newest.send(ClientMessage::Binary(encode(&WireMessage::Up{incarnation:token.incarnation,session_epoch:token.session_epoch}).unwrap().into())).await.unwrap();
 tokio::time::timeout(Duration::from_secs(2),async{loop{if matches!(read_wire(&mut newest).await,WireMessage::InputAccepted{..}){break;}}}).await.unwrap();
 assert!(crate::ack_probe::test_records().iter().any(|r|r.event==Event::Decoded&&r.session==token.session_epoch&&r.kind==15));
 println!("ACK_PROBE admission PASS: accepted newer; stale and newer invalid Hello cannot steal; contended bind retries after admission");
}
