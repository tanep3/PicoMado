//! Pico WebSocket adapter with bounded control and LCD frame dispatch.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{Sink, SinkExt, Stream, StreamExt};

use crate::application::InputResult;
use crate::content::ButtonAction;
use crate::protocol::{encode, Decoder, WireMessage, IMAGE_FRAGMENT_BYTES, MAX_CHUNK_BYTES};
use crate::queue::{FairQueue, Packet, PacketKind};
use crate::web::SharedApp;
use crate::{Effect, Event, SessionToken};

/// Provisional per-session dispatch quantum pending multi-device measurement.
const MAX_CONTROL_PER_TICK: usize = 8;

fn session_identity(current: &Option<SessionToken>) -> (&str, u64, u64) {
    current.as_ref().map_or(("unbound", 0, 0), |token| {
        (
            token.device_id.as_str(),
            token.incarnation,
            token.session_epoch,
        )
    })
}

fn wire_label(message: &WireMessage) -> &'static str {
    match message {
        WireMessage::Hello { .. } => "hello",
        WireMessage::Welcome { .. } => "welcome",
        WireMessage::Heartbeat { .. } => "heartbeat",
        WireMessage::Next { .. } => "next",
        WireMessage::Power { .. } => "power",
        WireMessage::EnterSub { .. } => "sub",
        WireMessage::Up { .. } => "up",
        WireMessage::Down { .. } => "down",
        WireMessage::ReceiptAck { .. } => "receipt_ack",
        WireMessage::RenderedAck { .. } => "rendered_ack",
        WireMessage::InputAccepted { .. } => "input_accepted",
        WireMessage::PluginPosition { .. } => "plugin_position",
        WireMessage::SettingsStored { .. } => "settings_stored",
        WireMessage::Settings { .. } => "settings",
        WireMessage::Error { .. } => "error",
        _ => "unexpected",
    }
}

#[derive(Default)]
struct AckDiagnostics {
    ignored: u64,
}
impl AckDiagnostics {
    fn count_ignored(&mut self) -> bool {
        self.ignored = self.ignored.saturating_add(1);
        self.ignored <= 4 || self.ignored.is_power_of_two()
    }
    fn log_ignored(
        &mut self,
        token: &SessionToken,
        frame: &crate::FrameToken,
        kind: &str,
        current_epoch: u64,
    ) {
        if self.count_ignored() {
            eprintln!("picomado ack_ignored id={} incarnation={} session_epoch={} kind={} frame_id={} ack_display_epoch={} current_display_epoch={} reason=stale_or_duplicate ignored_count={}",
                token.device_id,token.incarnation,token.session_epoch,kind,frame.frame_id,frame.display_epoch,current_epoch,self.ignored);
        }
    }
}

pub async fn upgrade(State(app): State<SharedApp>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(MAX_CHUNK_BYTES)
        .max_frame_size(MAX_CHUNK_BYTES)
        .on_upgrade(move |socket| serve_socket(socket, app))
}

fn process(
    app: &SharedApp,
    current: &mut Option<SessionToken>,
    message: WireMessage,
    diagnostics: &mut AckDiagnostics,
) -> Result<Option<WireMessage>, &'static str> {
    if let WireMessage::Hello {
        device_id,
        capabilities,
        screen_owner_epoch,
    } = message
    {
        if current.is_some() {
            return Err("duplicate_hello");
        }
        let caps: BTreeSet<_> = capabilities.into_iter().collect();
        let token = app
            .lock()
            .map_err(|_| "unavailable")?
            .hello(&device_id, caps, screen_owner_epoch)
            .map_err(|_| "invalid_hello")?;
        *current = Some(token.clone());
        return Ok(Some(WireMessage::Welcome {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
        }));
    }
    let token = current.as_ref().ok_or("hello_required")?;
    let mut guard = app.lock().map_err(|_| "unavailable")?;
    if !guard.session_is_current(token) {
        return Err("stale_session");
    }
    match message {
        WireMessage::SettingsStored {
            incarnation,
            revision,
        } => {
            if incarnation != token.incarnation {
                return Err("stale_settings");
            }
            guard
                .settings_stored(token, revision)
                .map_err(|_| "stale_settings")?;
            Ok(None)
        }
        WireMessage::Heartbeat {
            incarnation,
            session_epoch,
            applied_revision,
        } => {
            if incarnation != token.incarnation || session_epoch != token.session_epoch {
                return Err("stale_session");
            }
            guard
                .heartbeat(token, applied_revision)
                .map_err(|_| "stale_settings")?;
            eprintln!("picomado heartbeat_received id={} incarnation={} session_epoch={} applied_revision={}", token.device_id, token.incarnation, token.session_epoch, applied_revision);
            Ok(None)
        }
        WireMessage::Next {
            incarnation,
            session_epoch,
        }
        | WireMessage::Power {
            incarnation,
            session_epoch,
        }
        | WireMessage::EnterSub {
            incarnation,
            session_epoch,
        }
        | WireMessage::Up {
            incarnation,
            session_epoch,
        }
        | WireMessage::Down {
            incarnation,
            session_epoch,
        } => {
            if incarnation != token.incarnation || session_epoch != token.session_epoch {
                return Err("stale_session");
            }
            let action = match message {
                WireMessage::Next { .. } => ButtonAction::Next,
                WireMessage::Power { .. } => ButtonAction::Power,
                WireMessage::EnterSub { .. } => ButtonAction::Sub,
                WireMessage::Up { .. } => ButtonAction::Up,
                WireMessage::Down { .. } => ButtonAction::Down,
                _ => unreachable!(),
            };
            let before = guard.selection_for(token);
            let started = std::time::Instant::now();
            let result = guard.button(token, action).map_err(|_| "invalid_input")?;
            let after = guard.selection_for(token);
            eprintln!("picomado input_processed id={} incarnation={} session_epoch={} action={:?} before_selected={} before_display_epoch={} selected={} display_epoch={} elapsed_ms={}",
                token.device_id, token.incarnation, token.session_epoch, action,
                before.as_ref().and_then(|v|v.0.as_deref()).unwrap_or("none"), before.as_ref().map_or(0,|v|v.1),
                after.as_ref().and_then(|v|v.0.as_deref()).unwrap_or("none"), after.as_ref().map_or(0,|v|v.1), started.elapsed().as_millis());
            Ok(Some(match result {
                InputResult::Selection {
                    selected,
                    display_epoch,
                } => WireMessage::InputAccepted {
                    selected,
                    display_epoch,
                },
                InputResult::Position(position) => WireMessage::PluginPosition {
                    plugin: position.plugin,
                    dataset_id: position.dataset_id,
                    target_date: position.target_date,
                    index: position.index,
                },
            }))
        }
        WireMessage::ReceiptAck { token: frame } => {
            if frame.device_id != token.device_id
                || frame.incarnation != token.incarnation
                || frame.session_epoch != token.session_epoch
            {
                return Err("stale_frame");
            }
            if !guard.frame_ack_within_known_generations(&frame) {
                return Err("invalid_ack_token");
            }
            if !guard
                .terminal_event(Event::ReceiptAck {
                    token: frame.clone(),
                })
                .map_err(|_| "invalid_ack")?
            {
                // Input may invalidate this image before its queued ACK
                // reaches the server. Reject its state change, keep the session.
                diagnostics.log_ignored(
                    token,
                    &frame,
                    "receipt",
                    guard.selection_for(token).map_or(0, |v| v.1),
                );
            }
            Ok(None)
        }
        WireMessage::RenderedAck { token: frame } => {
            if frame.device_id != token.device_id
                || frame.incarnation != token.incarnation
                || frame.session_epoch != token.session_epoch
            {
                return Err("stale_frame");
            }
            if !guard.frame_ack_within_known_generations(&frame) {
                return Err("invalid_ack_token");
            }
            if !guard
                .terminal_event(Event::RenderedAck {
                    token: frame.clone(),
                })
                .map_err(|_| "invalid_ack")?
            {
                diagnostics.log_ignored(
                    token,
                    &frame,
                    "rendered",
                    guard.selection_for(token).map_or(0, |v| v.1),
                );
            }
            Ok(None)
        }
        _ => Err("unexpected_message"),
    }
}

enum WriterCommand {
    Bind(SessionToken),
    Reply(WireMessage),
    Stop,
}
const SEND_TIMEOUT: Duration = Duration::from_millis(250);
const FRAGMENT_STALL_TIMEOUT: Duration = Duration::from_secs(2);
/// The JSON-array wire image is about 100 KiB for a 25.6 KiB LCD buffer.
/// Budget by encoded bytes, with a floor for setup and a bounded upper limit.
const FRAME_MIN_BYTES_PER_SECOND: usize = 12 * 1024;
const MAX_FRAME_DEADLINE: Duration = Duration::from_secs(20);

#[derive(Clone, Copy)]
struct FramePlan {
    frame_id: u64,
    payload_bytes: usize,
    wire_bytes: usize,
}

struct FrameProgress {
    plan: FramePlan,
    started: std::time::Instant,
    sent_bytes: usize,
}

fn frame_deadline(wire_bytes: usize) -> Duration {
    Duration::from_secs(2 + wire_bytes.div_ceil(FRAME_MIN_BYTES_PER_SECOND) as u64)
        .min(MAX_FRAME_DEADLINE)
}

fn log_frame_end(device_id: &str, effect_id: u64, frame: FrameProgress, outcome: &str) {
    eprintln!(
        "picomado frame_send id={} effect_id={} frame_id={} outcome={} payload_bytes={} wire_bytes={} sent_bytes={} elapsed_ms={}",
        device_id,
        effect_id,
        frame.plan.frame_id,
        outcome,
        frame.plan.payload_bytes,
        frame.plan.wire_bytes,
        frame.sent_bytes,
        frame.started.elapsed().as_millis()
    );
}

async fn write_wire<S>(sink: &mut S, message: WireMessage) -> Result<(), ()>
where
    S: Sink<Message> + Unpin,
{
    let bytes = encode(&message).map_err(|_| ())?;
    tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(bytes.into())))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

fn enqueue_outbox(
    app: &SharedApp,
    token: &SessionToken,
    queue: &mut FairQueue,
    next_packet_id: &mut u64,
    plans: &mut std::collections::BTreeMap<u64, FramePlan>,
) -> Result<bool, ()> {
    let effects = app.lock().map_err(|_| ())?.pending_effects();
    let pending: std::collections::BTreeSet<_> = effects.iter().map(|(id, _)| *id).collect();
    queue.retain_effects(&pending);
    plans.retain(|id, _| pending.contains(id));
    let mut controls = 0;
    let mut frame_selected = queue.bytes(&token.device_id) > 0
        && effects
            .iter()
            .any(|(id, e)| matches!(e, Effect::SendFrame { .. }) && queue.contains_effect(*id));
    for (id, effect) in effects {
        match effect {
            Effect::SyncSettings {
                token: target,
                snapshot,
            } if target == *token => {
                if controls >= MAX_CONTROL_PER_TICK || queue.contains_effect(id) {
                    continue;
                }
                let bytes = encode(&WireMessage::Settings { snapshot }).map_err(|_| ())?;
                queue
                    .push(Packet {
                        device_id: token.device_id.clone(),
                        packet_id: *next_packet_id,
                        outbox_id: id,
                        finish_effect: true,
                        incarnation: token.incarnation,
                        display_epoch: 0,
                        kind: PacketKind::Control,
                        bytes,
                    })
                    .map_err(|_| ())?;
                *next_packet_id += 1;
                controls += 1;
            }
            Effect::CloseSession(target) if target == *token => {
                app.lock()
                    .map_err(|_| ())?
                    .complete_effect(id)
                    .map_err(|_| ())?;
                return Ok(true);
            }
            Effect::DropQueuedFrames {
                incarnation,
                through_display_epoch,
                ..
            } if incarnation == token.incarnation => {
                queue.drop_video_through(&token.device_id, incarnation, through_display_epoch);
                app.lock()
                    .map_err(|_| ())?
                    .complete_effect(id)
                    .map_err(|_| ())?;
            }
            Effect::SendFrame {
                token: frame,
                asset_handle,
            } if !frame_selected
                && !queue.contains_effect(id)
                && frame.device_id == token.device_id
                && frame.incarnation == token.incarnation
                && frame.session_epoch == token.session_epoch =>
            {
                let asset = {
                    let guard = app.lock().map_err(|_| ())?;
                    if guard.frame_is_current(&frame) {
                        guard.frame_asset(&asset_handle)
                    } else {
                        None
                    }
                };
                let Some(asset) = asset else {
                    continue;
                };
                let start = WireMessage::FrameStart {
                    token: frame.clone(),
                    width: 160,
                    height: 80,
                    format: "rgb565be".into(),
                    byte_len: asset.len() as u32,
                    crc32: crc32fast::hash(&asset),
                };
                let encoded_start = encode(&start).map_err(|_| ())?;
                let mut wire_bytes = encoded_start.len();
                queue
                    .push(Packet {
                        device_id: token.device_id.clone(),
                        packet_id: *next_packet_id,
                        outbox_id: id,
                        finish_effect: false,
                        incarnation: frame.incarnation,
                        display_epoch: frame.display_epoch,
                        kind: PacketKind::Control,
                        bytes: encoded_start,
                    })
                    .map_err(|_| ())?;
                *next_packet_id += 1;
                let count = asset.chunks(IMAGE_FRAGMENT_BYTES).len();
                for (index, chunk) in asset.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
                    let message = WireMessage::FrameChunk {
                        token: frame.clone(),
                        offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                        bytes: chunk.to_vec(),
                    };
                    let encoded_chunk = encode(&message).map_err(|_| ())?;
                    wire_bytes += encoded_chunk.len();
                    queue
                        .push(Packet {
                            device_id: token.device_id.clone(),
                            packet_id: *next_packet_id,
                            outbox_id: id,
                            finish_effect: index + 1 == count,
                            incarnation: frame.incarnation,
                            display_epoch: frame.display_epoch,
                            kind: PacketKind::Video,
                            bytes: encoded_chunk,
                        })
                        .map_err(|_| ())?;
                    *next_packet_id += 1;
                }
                plans.insert(
                    id,
                    FramePlan {
                        frame_id: frame.frame_id,
                        payload_bytes: asset.len(),
                        wire_bytes,
                    },
                );
                frame_selected = true;
            }
            _ => {}
        }
    }
    Ok(false)
}

async fn writer_loop<S>(
    mut sink: S,
    mut rx: tokio::sync::mpsc::Receiver<WriterCommand>,
    app: SharedApp,
) -> &'static str
where
    S: Sink<Message> + Unpin,
{
    let mut current: Option<SessionToken> = None;
    let mut queue = FairQueue::default();
    let mut next_packet_id = 1;
    let mut plans = std::collections::BTreeMap::<u64, FramePlan>::new();
    let mut progress = std::collections::BTreeMap::<u64, FrameProgress>::new();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let end_reason = loop {
        tokio::select! {
            biased;
            command=rx.recv()=>match command {
                Some(WriterCommand::Bind(token))=>current=Some(token),
                Some(WriterCommand::Reply(message))=>{
                    let kind=wire_label(&message);
                    if write_wire(&mut sink,message).await.is_err() {
                        let (id,incarnation,session_epoch)=session_identity(&current);
                        eprintln!("picomado control_write_failed id={} incarnation={} session_epoch={} kind={}",id,incarnation,session_epoch,kind);
                        break "control_write_failed";
                    }
                },
                Some(WriterCommand::Stop)=>break "requested_stop",
                None=>break "command_channel_closed",
            },
            _=tick.tick(),if current.is_some()=>{
                let token=current.as_ref().expect("bound");
                if !app.lock().is_ok_and(|guard|guard.session_is_current(token)) {break "session_not_current";}
                match enqueue_outbox(&app,token,&mut queue,&mut next_packet_id,&mut plans) {
                    Ok(true)=>break "close_session_effect",
                    Err(())=>break "enqueue_failed",
                    Ok(false)=>{},
                }
                let stale: Vec<_> = progress.keys().copied().filter(|id|
                    !app.lock().is_ok_and(|guard|guard.effect_pending(*id))).collect();
                for id in stale {
                    if let Some(frame)=progress.remove(&id) {
                        log_frame_end(&token.device_id,id,frame,"effect_cancelled");
                    }
                }
            }
            _=tokio::task::yield_now(),if current.as_ref().is_some_and(|t|queue.bytes(&t.device_id)>0)=>{
                let token=current.as_ref().expect("bound");
                let Some(packet)=queue.pop_for(&token.device_id) else {continue;};
                let valid={let Ok(guard)=app.lock() else {break "application_unavailable";};
                    if !guard.session_is_current(token) {false} else {
                        guard.pending_effects().into_iter().find(|(id,_)|*id==packet.outbox_id)
                            .is_some_and(|(_,effect)|match effect {
                                Effect::SendFrame {token,..}=>guard.frame_is_current(&token),
                                _=>true,
                            })
                    }};
                if !valid {
                    if let Some(frame)=progress.remove(&packet.outbox_id) {
                        log_frame_end(&token.device_id,packet.outbox_id,frame,"generation_changed");
                    }
                    continue;
                }
                if packet.kind == PacketKind::Control && !packet.finish_effect {
                    if let Some(plan)=plans.get(&packet.outbox_id).copied() {
                        eprintln!(
                            "picomado frame_send_start id={} effect_id={} frame_id={} payload_bytes={} wire_bytes={} deadline_ms={}",
                            token.device_id,packet.outbox_id,plan.frame_id,plan.payload_bytes,
                            plan.wire_bytes,frame_deadline(plan.wire_bytes).as_millis()
                        );
                        progress.insert(packet.outbox_id,FrameProgress{
                            plan,started:std::time::Instant::now(),sent_bytes:0
                        });
                    }
                }
                let now=tokio::time::Instant::now();
                let frame_limit=progress.get(&packet.outbox_id).map(|frame|
                    tokio::time::Instant::from_std(frame.started)+frame_deadline(frame.plan.wire_bytes));
                let stall=if frame_limit.is_some(){FRAGMENT_STALL_TIMEOUT}else{SEND_TIMEOUT};
                let deadline=frame_limit.map_or(now+stall,|limit|(now+stall).min(limit));
                let packet_len=packet.bytes.len();
                match tokio::time::timeout_at(deadline,sink.send(Message::Binary(packet.bytes.into()))).await {
                    Ok(Ok(()))=>{},
                    result=>{
                        let reason=match result {
                            Err(_) if frame_limit.is_some_and(|limit|tokio::time::Instant::now()>=limit)=>"frame_deadline",
                            Err(_)=>"write_stall",
                            Ok(Err(_))=>"sink_error",
                            Ok(Ok(()))=>unreachable!(),
                        };
                        if let Some(frame)=progress.remove(&packet.outbox_id) {
                            log_frame_end(&token.device_id,packet.outbox_id,frame,reason);
                        } else {
                            eprintln!("picomado writer_exit id={} reason={}",token.device_id,reason);
                        }
                        break reason;
                    }
                }
                if let Some(frame)=progress.get_mut(&packet.outbox_id) {
                    frame.sent_bytes+=packet_len;
                }
                if packet.finish_effect {
                    let completed=app.lock().ok().and_then(|mut guard|guard.complete_effect(packet.outbox_id).ok());
                    if let Some(frame)=progress.remove(&packet.outbox_id) {
                        log_frame_end(&token.device_id,packet.outbox_id,frame,
                            match completed {Some(true)=>"sent",Some(false)=>"effect_already_resolved",None=>"effect_completion_failed"});
                    }
                    // Input cancellation or a fast ACK can prune the effect
                    // during the final write. That is not a transport failure.
                    if completed.is_none() {break "effect_completion_failed";}
                }
            }
        }
    };
    if let Some(token) = &current {
        for (id, frame) in progress {
            log_frame_end(&token.device_id, id, frame, "session_ended");
        }
    }
    let (id, incarnation, session_epoch) = session_identity(&current);
    eprintln!(
        "picomado writer_exit id={} incarnation={} session_epoch={} reason={}",
        id, incarnation, session_epoch, end_reason
    );
    let _ = tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Close(None))).await;
    end_reason
}

pub async fn serve_socket(socket: WebSocket, app: SharedApp) {
    let (sink, stream) = socket.split();
    serve_split(sink, stream, app).await;
}

async fn serve_split<S, St, E>(sink: S, mut stream: St, app: SharedApp) -> &'static str
where
    S: Sink<Message> + Unpin + Send + 'static,
    St: Stream<Item = Result<Message, E>> + Unpin,
{
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let mut writer = tokio::spawn(writer_loop(sink, rx, app.clone()));
    let mut decoder = Decoder::default();
    let mut current: Option<SessionToken> = None;
    let mut writer_finished = false;
    static NEXT_CONNECTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let connection_id = NEXT_CONNECTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut last_rx = started;
    let (mut rx_frames, mut rx_messages, mut rx_bytes) = (0u64, 0u64, 0u64);
    let mut end_reason = "read_eof";
    let mut end_detail = "none";
    let mut diagnostics = AckDiagnostics::default();
    eprintln!("picomado websocket_open connection_id={}", connection_id);
    loop {
        let incoming = tokio::select! {
            result = &mut writer => {
                end_reason="writer_ended";
                end_detail=match result {Ok(reason)=>reason,Err(error)=>if error.is_panic(){"writer_panic"}else{"writer_task_failed"}};
                writer_finished = true;
                break;
            }
            incoming = stream.next() => incoming,
        };
        let message = match incoming {
            Some(Ok(message)) => message,
            Some(Err(_)) => {
                end_reason = "read_error";
                break;
            }
            None => break,
        };
        last_rx = std::time::Instant::now();
        rx_frames += 1;
        let bytes = match message {
            Message::Binary(bytes) => bytes,
            Message::Close(frame) => {
                eprintln!(
                    "picomado peer_close connection_id={} code={}",
                    connection_id,
                    frame.map_or(0, |f| f.code)
                );
                end_reason = "peer_close";
                break;
            }
            Message::Text(_) => {
                end_reason = "unsupported_text";
                break;
            }
            Message::Ping(_) => {
                end_reason = "unsupported_ping";
                break;
            }
            Message::Pong(_) => {
                end_reason = "unsupported_pong";
                break;
            }
        };
        rx_bytes += bytes.len() as u64;
        let messages = match decoder.push(&bytes) {
            Ok(messages) => messages,
            Err(_) => {
                end_reason = "decode_invalid_frame";
                let _ = tx
                    .send(WriterCommand::Reply(WireMessage::Error {
                        code: "invalid_frame".into(),
                    }))
                    .await;
                break;
            }
        };
        let mut failed = false;
        for message in messages {
            rx_messages += 1;
            let kind = wire_label(&message);
            let was_unbound = current.is_none();
            match process(&app, &mut current, message, &mut diagnostics) {
                Ok(reply) => {
                    if was_unbound {
                        if let Some(token) = &current {
                            if tx.send(WriterCommand::Bind(token.clone())).await.is_err() {
                                end_reason = "writer_command_closed";
                                failed = true;
                                break;
                            }
                            eprintln!("picomado websocket_bound connection_id={} id={} incarnation={} session_epoch={}",connection_id,token.device_id,token.incarnation,token.session_epoch);
                        }
                    }
                    if let Some(reply) = reply {
                        let queued = std::time::Instant::now();
                        if tx.send(WriterCommand::Reply(reply)).await.is_err() {
                            end_reason = "writer_command_closed";
                            failed = true;
                            break;
                        }
                        if queued.elapsed() > Duration::from_millis(100) {
                            eprintln!("picomado reply_queue_wait connection_id={} request={} elapsed_ms={}",connection_id,kind,queued.elapsed().as_millis());
                        }
                    }
                }
                Err(code) => {
                    end_reason = "protocol_rejected";
                    end_detail = code;
                    let (id, incarnation, session_epoch) = session_identity(&current);
                    eprintln!("picomado protocol_rejected connection_id={} id={} incarnation={} session_epoch={} kind={} code={}",connection_id,id,incarnation,session_epoch,kind,code);
                    let _ = tx
                        .send(WriterCommand::Reply(WireMessage::Error {
                            code: code.into(),
                        }))
                        .await;
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            break;
        }
    }
    if !writer_finished {
        let _ = tx.send(WriterCommand::Stop).await;
        let _ = writer.await;
    }
    let (id, incarnation, session_epoch) = session_identity(&current);
    eprintln!("picomado session_end connection_id={} id={} incarnation={} session_epoch={} trigger={} detail={} elapsed_ms={} idle_ms={} rx_frames={} rx_messages={} rx_bytes={} ignored_acks={}",connection_id,id,incarnation,session_epoch,end_reason,end_detail,started.elapsed().as_millis(),last_rx.elapsed().as_millis(),rx_frames,rx_messages,rx_bytes,diagnostics.ignored);
    if let Some(token) = current {
        if let Ok(mut guard) = app.lock() {
            let _ = guard.disconnect(token);
        }
    }
    end_reason
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::App;
    use crate::fetch::Clock;
    use crate::protocol::WireMessage;
    use crate::protocol::{FrameAssembler, GatedFrameReceiver};
    use crate::weather::ForecastSet;
    use crate::web;
    use crate::{PicoScreen, ScreenKind};
    use futures_util::{SinkExt, StreamExt};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    struct FixtureClock;
    impl Clock for FixtureClock {
        fn unix_seconds(&self) -> i64 {
            0
        }
        fn today_jst(&self) -> String {
            "2026-10-12".into()
        }
    }

    fn app_at_fixture_date(path: &std::path::Path) -> App {
        App::open_with_clock(path, Arc::new(FixtureClock)).unwrap()
    }

    /// After `limit` writes this sink never becomes ready, like a terminal
    /// whose receive window has stayed full under sustained image output.
    struct MockSink {
        count: Arc<AtomicUsize>,
        limit: usize,
        recorded: Option<Arc<std::sync::Mutex<Vec<Message>>>>,
    }
    impl Sink<Message> for MockSink {
        type Error = ();
        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            if self.count.load(Ordering::SeqCst) >= self.limit {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }
        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            self.count.fetch_add(1, Ordering::SeqCst);
            if let Some(messages) = &self.recorded {
                messages.lock().unwrap().push(item);
            }
            Ok(())
        }
        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn non_reading_writer_times_out_without_delaying_fast_peer_input() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let (slow, fast) = {
            let mut a = app.lock().unwrap();
            let slow = a.hello("slow", ["lcd".into()].into(), 1).unwrap();
            let fast = a.hello("fast", ["lcd".into()].into(), 1).unwrap();
            a.queue_lcd_frame(
                &slow,
                "youtube",
                vec![0x5a; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
            a.queue_lcd_frame(
                &fast,
                "youtube",
                vec![0x5b; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
            (slow, fast)
        };
        let slow_count = Arc::new(AtomicUsize::new(0));
        let fast_count = Arc::new(AtomicUsize::new(0));
        let (slow_tx, slow_rx) = tokio::sync::mpsc::channel(32);
        let (fast_tx, fast_rx) = tokio::sync::mpsc::channel(32);
        let slow_writer = tokio::spawn(writer_loop(
            MockSink {
                count: slow_count.clone(),
                limit: 2,
                recorded: None,
            },
            slow_rx,
            app.clone(),
        ));
        let fast_writer = tokio::spawn(writer_loop(
            MockSink {
                count: fast_count.clone(),
                limit: usize::MAX,
                recorded: None,
            },
            fast_rx,
            app.clone(),
        ));
        slow_tx.send(WriterCommand::Bind(slow)).await.unwrap();
        slow_tx
            .send(WriterCommand::Reply(WireMessage::Welcome {
                incarnation: 1,
                session_epoch: 1,
            }))
            .await
            .unwrap();
        fast_tx
            .send(WriterCommand::Bind(fast.clone()))
            .await
            .unwrap();
        fast_tx
            .send(WriterCommand::Reply(WireMessage::Welcome {
                incarnation: fast.incarnation,
                session_epoch: fast.session_epoch,
            }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if slow_count.load(Ordering::SeqCst) >= 2 && fast_count.load(Ordering::SeqCst) >= 3
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let started = std::time::Instant::now();
        let result = app
            .lock()
            .unwrap()
            .button(&fast, ButtonAction::Next)
            .unwrap();
        let InputResult::Selection {
            selected,
            display_epoch,
        } = result
        else {
            panic!("selection expected")
        };
        let prior = fast_count.load(Ordering::SeqCst);
        fast_tx
            .send(WriterCommand::Reply(WireMessage::InputAccepted {
                selected,
                display_epoch,
            }))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_millis(200), async {
            loop {
                if fast_count.load(Ordering::SeqCst) > prior {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_millis(200));
        tokio::time::timeout(Duration::from_secs(4), slow_writer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(slow_count.load(Ordering::SeqCst), 2);
        fast_tx.send(WriterCommand::Stop).await.unwrap();
        fast_writer.await.unwrap();
    }

    #[test]
    fn frame_deadline_accounts_for_encoded_wire_size() {
        assert_eq!(frame_deadline(100_670), Duration::from_secs(11));
        assert_eq!(frame_deadline(25_600), Duration::from_secs(5));
        assert_eq!(frame_deadline(256 * 1024), MAX_FRAME_DEADLINE);
    }

    #[test]
    fn dispatch_fragments_obey_receiver_generation_during_owner_transitions() {
        for scenario in ["switch", "cancel", "standby", "reconnect"] {
            let temp = tempfile::tempdir().unwrap();
            let app = Arc::new(std::sync::Mutex::new(
                App::open(&temp.path().join("state.db")).unwrap(),
            ));
            let token = app
                .lock()
                .unwrap()
                .hello("desk", ["lcd".into()].into(), 1)
                .unwrap();
            if scenario == "cancel" {
                assert!(app
                    .lock()
                    .unwrap()
                    .terminal_event(Event::Next {
                        token: token.clone()
                    })
                    .unwrap());
                assert!(app
                    .lock()
                    .unwrap()
                    .terminal_event(Event::Next {
                        token: token.clone()
                    })
                    .unwrap());
                assert!(app
                    .lock()
                    .unwrap()
                    .terminal_event(Event::EnterSub {
                        token: token.clone()
                    })
                    .unwrap());
            }
            let plugin = if scenario == "cancel" {
                "weather"
            } else {
                "youtube"
            };
            let frame = app
                .lock()
                .unwrap()
                .queue_lcd_frame(&token, plugin, vec![0x5a; crate::protocol::LCD_FRAME_BYTES])
                .unwrap();
            let mut queue = FairQueue::default();
            enqueue_outbox(
                &app,
                &token,
                &mut queue,
                &mut 1,
                &mut std::collections::BTreeMap::new(),
            )
            .unwrap();
            let mut messages = Vec::new();
            while let Some(packet) = queue.pop_for("desk") {
                let message = Decoder::default().push(&packet.bytes).unwrap().remove(0);
                if matches!(
                    message,
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                ) {
                    messages.push(message);
                }
            }
            assert_eq!(messages.len(), 8);
            let mut terminal = GatedFrameReceiver::new(PicoScreen {
                kind: ScreenKind::Lcd,
                device_id: "desk".into(),
                incarnation: frame.incarnation,
                session_epoch: frame.session_epoch,
                display_epoch: frame.display_epoch,
                screen_owner_epoch: frame.screen_owner_epoch,
                local: None,
                drawing: None,
                pending_standby: false,
            });
            let mut raw = FrameAssembler::default();
            for message in messages.iter().take(2) {
                assert!(terminal.accept(message.clone()).unwrap().is_empty());
                assert!(raw.accept(message.clone()).unwrap().is_none());
            }
            match scenario {
                "switch" => {
                    assert!(app
                        .lock()
                        .unwrap()
                        .terminal_event(Event::Next {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.display_epoch += 1;
                }
                "cancel" => {
                    assert!(app
                        .lock()
                        .unwrap()
                        .terminal_event(Event::Power {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.display_epoch += 1;
                }
                "standby" => {
                    assert!(app
                        .lock()
                        .unwrap()
                        .terminal_event(Event::Power {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.take_local(crate::LocalScreen::Standby);
                }
                "reconnect" => {
                    let newer = app
                        .lock()
                        .unwrap()
                        .hello("desk", ["lcd".into()].into(), 2)
                        .unwrap();
                    assert!(newer.session_epoch > token.session_epoch);
                    terminal.screen.session_epoch = newer.session_epoch;
                }
                _ => unreachable!(),
            }
            let mut assembled = None;
            for message in messages.into_iter().skip(2) {
                assert!(
                    terminal.accept(message.clone()).unwrap().is_empty(),
                    "{scenario}"
                );
                assembled = raw.accept(message).unwrap().or(assembled);
            }
            assert_eq!(assembled.unwrap().0, frame, "{scenario}");
            assert!(
                terminal.complete_draw(frame.clone()).is_none(),
                "{scenario}"
            );
            assert!(!app
                .lock()
                .unwrap()
                .terminal_event(Event::ReceiptAck {
                    token: frame.clone()
                })
                .unwrap());
            assert!(!app
                .lock()
                .unwrap()
                .terminal_event(Event::RenderedAck { token: frame })
                .unwrap());
        }
    }

    #[tokio::test]
    async fn blocked_terminal_still_processes_input_while_other_terminal_gets_two_frames() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let (slow_tx, mut slow_rx) = tokio::sync::mpsc::channel(16);
        let (fast_tx, mut fast_rx) = tokio::sync::mpsc::channel(16);
        let slow_count = Arc::new(AtomicUsize::new(0));
        let fast_count = Arc::new(AtomicUsize::new(0));
        let fast_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let slow_task = tokio::spawn(serve_split(
            MockSink {
                count: slow_count.clone(),
                limit: 3,
                recorded: None,
            },
            futures_util::stream::poll_fn(move |cx| slow_rx.poll_recv(cx)),
            app.clone(),
        ));
        let fast_task = tokio::spawn(serve_split(
            MockSink {
                count: fast_count.clone(),
                limit: usize::MAX,
                recorded: Some(fast_messages.clone()),
            },
            futures_util::stream::poll_fn(move |cx| fast_rx.poll_recv(cx)),
            app.clone(),
        ));
        for (tx, id) in [(&slow_tx, "slow"), (&fast_tx, "fast")] {
            tx.send(Ok::<_, ()>(Message::Binary(
                encode(&WireMessage::Hello {
                    device_id: id.into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        }
        let (slow, fast) = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let tokens: Vec<_> = {
                    let guard = app.lock().unwrap();
                    ["slow", "fast"]
                        .iter()
                        .filter_map(|id| {
                            guard.view().devices.iter().find(|d| &d.id == id).map(|d| {
                                SessionToken {
                                    device_id: (*id).into(),
                                    incarnation: d.incarnation,
                                    session_epoch: 1,
                                }
                            })
                        })
                        .collect()
                };
                if tokens.len() == 2
                    && slow_count.load(Ordering::SeqCst) >= 2
                    && fast_count.load(Ordering::SeqCst) >= 2
                {
                    break (tokens[0].clone(), tokens[1].clone());
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        app.lock()
            .unwrap()
            .queue_lcd_frame(&slow, "youtube", vec![1; crate::protocol::LCD_FRAME_BYTES])
            .unwrap();
        let first_fast = app
            .lock()
            .unwrap()
            .queue_lcd_frame(&fast, "youtube", vec![2; crate::protocol::LCD_FRAME_BYTES])
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if slow_count.load(Ordering::SeqCst) >= 3 && fast_count.load(Ordering::SeqCst) >= 10
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // The slow writer is stuck on a fragment. Its reader still accepts a
        // selection change, which invalidates the in-flight image generation.
        slow_tx
            .send(Ok(Message::Binary(
                encode(&WireMessage::Next {
                    incarnation: slow.incarnation,
                    session_epoch: slow.session_epoch,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_millis(200), async {
            loop {
                if app
                    .lock()
                    .unwrap()
                    .view()
                    .devices
                    .iter()
                    .any(|d| d.id == "slow" && d.selected.as_deref() == Some("caricature"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert!(app
            .lock()
            .unwrap()
            .terminal_event(Event::RenderedAck { token: first_fast })
            .unwrap());
        app.lock()
            .unwrap()
            .queue_lcd_frame(&fast, "youtube", vec![3; crate::protocol::LCD_FRAME_BYTES])
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if fast_count.load(Ordering::SeqCst) >= 18 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(slow_count.load(Ordering::SeqCst) <= 3);
        let mut assembler = FrameAssembler::default();
        let completed: Vec<_> = fast_messages
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let Message::Binary(bytes) = message else {
                    return None;
                };
                let wire = Decoder::default().push(bytes).unwrap().remove(0);
                match wire {
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. } => {
                        assembler.accept(wire).unwrap()
                    }
                    _ => None,
                }
            })
            .collect();
        assert_eq!(completed.len(), 2);
        assert_eq!(completed[0].1, vec![2; crate::protocol::LCD_FRAME_BYTES]);
        assert_eq!(completed[1].1, vec![3; crate::protocol::LCD_FRAME_BYTES]);
        drop(slow_tx);
        drop(fast_tx);
        slow_task.await.unwrap();
        fast_task.await.unwrap();
    }

    #[tokio::test]
    async fn failed_writer_ends_reader_session_and_reconnect_resends_cached_image() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        app.lock()
            .unwrap()
            .set_order(
                1,
                ["weather", "rain-radar", "youtube", "caricature"]
                    .map(str::to_owned)
                    .to_vec(),
            )
            .unwrap();
        let (slow_tx, mut slow_rx) = tokio::sync::mpsc::channel(16);
        let slow_count = Arc::new(AtomicUsize::new(0));
        let slow_task = tokio::spawn(serve_split(
            MockSink {
                count: slow_count.clone(),
                limit: 4,
                recorded: None,
            },
            futures_util::stream::poll_fn(move |cx| slow_rx.poll_recv(cx)),
            app.clone(),
        ));
        slow_tx
            .send(Ok::<_, ()>(Message::Binary(
                encode(&WireMessage::Hello {
                    device_id: "slow".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        let slow = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let device = app.lock().unwrap().view().devices.into_iter().next();
                if let Some(device) = device.filter(|d| d.online) {
                    if slow_count.load(Ordering::SeqCst) >= 2 {
                        break SessionToken {
                            device_id: device.id,
                            incarnation: device.incarnation,
                            session_epoch: 1,
                        };
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let source = include_bytes!("../assets/jma-tokyo-20261012.json");
        let set = ForecastSet::tokyo_from_cache(source, "cached", "2026-10-12").unwrap();
        let pixels = crate::weather::render_lcd(&set, &set.days[0]).unwrap();
        let first = {
            let mut guard = app.lock().unwrap();
            guard
                .publish_plugin_source("weather", &slow, 1, source, "cached", "2026-10-12")
                .unwrap();
            guard
                .queue_lcd_frame(&slow, "weather", pixels.clone())
                .unwrap()
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            while slow_count.load(Ordering::SeqCst) < 4 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // Input can arrive while a fragment is stalled. The writer's later
        // failure must still close this reader without waiting for stream EOF.
        slow_tx
            .send(Ok(Message::Binary(
                encode(&WireMessage::Heartbeat {
                    incarnation: slow.incarnation,
                    session_epoch: slow.session_epoch,
                    applied_revision: 1,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), slow_task)
            .await
            .unwrap()
            .unwrap();
        assert!(!app.lock().unwrap().view().devices[0].online);
        assert!(slow_tx.is_closed());

        let (fast_tx, mut fast_rx) = tokio::sync::mpsc::channel(16);
        let messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let fast_task = tokio::spawn(serve_split(
            MockSink {
                count: count.clone(),
                limit: usize::MAX,
                recorded: Some(messages.clone()),
            },
            futures_util::stream::poll_fn(move |cx| fast_rx.poll_recv(cx)),
            app.clone(),
        ));
        fast_tx
            .send(Ok::<_, ()>(Message::Binary(
                encode(&WireMessage::Hello {
                    device_id: "slow".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 2,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            while count.load(Ordering::SeqCst) < 10 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let mut assembler = FrameAssembler::default();
        let complete = messages
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let Message::Binary(bytes) = message else {
                    return None;
                };
                let wire = Decoder::default().push(bytes).unwrap().remove(0);
                match wire {
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. } => {
                        assembler.accept(wire).unwrap()
                    }
                    _ => None,
                }
            })
            .last()
            .unwrap();
        assert!(complete.0.frame_id > first.frame_id);
        assert_eq!(complete.1, pixels);
        for ack in [
            WireMessage::ReceiptAck {
                token: complete.0.clone(),
            },
            WireMessage::RenderedAck {
                token: complete.0.clone(),
            },
        ] {
            fast_tx
                .send(Ok(Message::Binary(encode(&ack).unwrap().into())))
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let view = app.lock().unwrap().view();
                let device = &view.devices[0];
                if device.last_received_frame == Some(complete.0.frame_id)
                    && device.last_rendered_frame == Some(complete.0.frame_id)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        drop(fast_tx);
        fast_task.await.unwrap();
    }

    async fn read_wire<S>(stream: &mut S) -> WireMessage
    where
        S: StreamExt<Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        let next = tokio::time::timeout(Duration::from_secs(3), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let ClientMessage::Binary(bytes) = next else {
            panic!("expected binary")
        };
        Decoder::default().push(&bytes).unwrap().remove(0)
    }

    #[tokio::test]
    async fn receiver_exit_reasons_distinguish_peer_close_decode_error_and_eof() {
        let cases: Vec<(Vec<Result<Message, ()>>, &str)> = vec![
            (vec![Ok(Message::Close(None))], "peer_close"),
            (
                vec![Ok(Message::Binary(vec![0, 0, 0, 2, 99, 1].into()))],
                "decode_invalid_frame",
            ),
            (vec![], "read_eof"),
            (vec![Err(())], "read_error"),
        ];
        for (messages, expected) in cases {
            let temp = tempfile::tempdir().unwrap();
            let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
                &temp.path().join("state.db"),
            )));
            let reason = serve_split(
                MockSink {
                    count: Arc::new(AtomicUsize::new(0)),
                    limit: usize::MAX,
                    recorded: None,
                },
                futures_util::stream::iter(messages),
                app,
            )
            .await;
            assert_eq!(reason, expected);
        }
    }

    #[tokio::test]
    async fn future_ack_is_a_protocol_error_not_a_delayed_ack() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let mut bound = None;
        let mut diagnostics = AckDiagnostics::default();
        process(
            &app,
            &mut bound,
            WireMessage::Hello {
                device_id: "desk".into(),
                capabilities: vec!["lcd".into()],
                screen_owner_epoch: 1,
            },
            &mut diagnostics,
        )
        .unwrap();
        let session = bound.clone().unwrap();
        let issued = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![1; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        for field in ["id", "display", "owner", "zero"] {
            let mut future = issued.clone();
            match field {
                "id" => future.frame_id += 1,
                "display" => future.display_epoch += 1,
                "owner" => future.screen_owner_epoch += 1,
                "zero" => future.frame_id = 0,
                _ => unreachable!(),
            }
            assert!(matches!(
                process(
                    &app,
                    &mut bound,
                    WireMessage::RenderedAck { token: future },
                    &mut diagnostics
                ),
                Err("invalid_ack_token")
            ));
        }
        assert_eq!(
            app.lock().unwrap().view().devices[0].last_rendered_frame,
            None
        );
        assert!(app.lock().unwrap().frame_is_current(&issued));

        // Use a fresh real reader/writer pair to verify the fatal response and
        // session shutdown, rather than merely checking the classifier.
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let (input, mut incoming) = tokio::sync::mpsc::channel(4);
        let count = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let task = tokio::spawn(serve_split(
            MockSink {
                count: count.clone(),
                limit: usize::MAX,
                recorded: Some(recorded.clone()),
            },
            futures_util::stream::poll_fn(move |cx| incoming.poll_recv(cx)),
            app.clone(),
        ));
        input
            .send(Ok::<_, ()>(Message::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let session = SessionToken {
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 1,
        };
        let mut future = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![1; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        future.frame_id += 1;
        input
            .send(Ok(Message::Binary(
                encode(&WireMessage::RenderedAck { token: future })
                    .unwrap()
                    .into(),
            )))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            "protocol_rejected"
        );
        assert!(!app.lock().unwrap().view().devices[0].online);
        assert_eq!(
            app.lock().unwrap().view().devices[0].selected.as_deref(),
            Some("youtube")
        );
        assert!(recorded.lock().unwrap().iter().any(|message| {
            let Message::Binary(bytes)=message else{return false;};
            matches!(Decoder::default().push(bytes).unwrap().remove(0),WireMessage::Error{code} if code=="invalid_ack_token")
        }));
    }

    #[test]
    fn ignored_ack_logging_is_bounded_per_connection() {
        let mut diagnostics = AckDiagnostics::default();
        let logged = (0..10_000).filter(|_| diagnostics.count_ignored()).count();
        assert_eq!(diagnostics.ignored, 10_000);
        assert!(logged <= 20);
        assert!(AckDiagnostics::default().count_ignored());
    }

    struct GateSink {
        inner: MockSink,
        at: usize,
        open: Arc<AtomicBool>,
        wake: Arc<futures_util::task::AtomicWaker>,
    }
    impl Sink<Message> for GateSink {
        type Error = ();
        fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
            if self.inner.count.load(Ordering::SeqCst) >= self.at
                && !self.open.load(Ordering::SeqCst)
            {
                self.wake.register(cx.waker());
                if !self.open.load(Ordering::SeqCst) {
                    return Poll::Pending;
                }
            }
            Poll::Ready(Ok(()))
        }
        fn start_send(self: Pin<&mut Self>, message: Message) -> Result<(), ()> {
            Pin::new(&mut self.get_mut().inner).start_send(message)
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn cancelled_last_fragment_does_not_close_writer() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = app_at_fixture_date(&temp.path().join("state.db"));
        let session = state.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let app = Arc::new(std::sync::Mutex::new(state));
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let open = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(futures_util::task::AtomicWaker::new());
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let task = tokio::spawn(writer_loop(
            GateSink {
                inner: MockSink {
                    count: count.clone(),
                    limit: usize::MAX,
                    recorded: Some(recorded.clone()),
                },
                at: 9,
                open: open.clone(),
                wake: wake.clone(),
            },
            rx,
            app.clone(),
        ));
        tx.send(WriterCommand::Bind(session.clone())).await.unwrap();
        tx.send(WriterCommand::Reply(WireMessage::Welcome {
            incarnation: 1,
            session_epoch: 1,
        }))
        .await
        .unwrap();
        let old = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![1; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 9 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let InputResult::Selection {
            selected,
            display_epoch,
        } = app
            .lock()
            .unwrap()
            .button(&session, ButtonAction::Next)
            .unwrap()
        else {
            panic!("selection expected");
        };
        assert!(!app.lock().unwrap().frame_is_current(&old));
        tx.send(WriterCommand::Reply(WireMessage::InputAccepted {
            selected,
            display_epoch,
        }))
        .await
        .unwrap();
        open.store(true, Ordering::SeqCst);
        wake.wake();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let has_reply = recorded.lock().unwrap().iter().any(|m| {
                    let Message::Binary(b) = m else {
                        return false;
                    };
                    matches!(
                        Decoder::default().push(b).unwrap().remove(0),
                        WireMessage::InputAccepted { .. }
                    )
                });
                if has_reply {
                    break;
                }
                assert!(
                    !task.is_finished(),
                    "writer closed after cancelled final fragment"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let new = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "caricature",
                vec![2; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 19 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(!task.is_finished());
        let mut assembler = FrameAssembler::default();
        let complete: Vec<_> = recorded
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| {
                let Message::Binary(b) = m else {
                    return None;
                };
                match Decoder::default().push(b).unwrap().remove(0) {
                    message @ (WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }) => {
                        assembler.accept(message).unwrap()
                    }
                    _ => None,
                }
            })
            .collect();
        assert_eq!(complete.len(), 2);
        assert_eq!(complete[1].0, new);
        tx.send(WriterCommand::Stop).await.unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn delayed_and_duplicate_acks_keep_the_current_session_alive() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let (input, mut incoming) = tokio::sync::mpsc::channel(16);
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(serve_split(
            MockSink {
                count: count.clone(),
                limit: usize::MAX,
                recorded: Some(recorded.clone()),
            },
            futures_util::stream::poll_fn(move |cx| incoming.poll_recv(cx)),
            app.clone(),
        ));
        input
            .send(Ok::<_, ()>(Message::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let session = SessionToken {
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 1,
        };
        let old = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![1; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 10 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();

        // The terminal can have already drawn/queued the old ACK when its
        // input invalidates the server generation. These arrive in one read.
        let mut batch = Vec::new();
        for message in [
            WireMessage::Next {
                incarnation: 1,
                session_epoch: 1,
            },
            WireMessage::ReceiptAck { token: old.clone() },
            WireMessage::RenderedAck { token: old },
            WireMessage::Next {
                incarnation: 1,
                session_epoch: 1,
            },
        ] {
            batch.extend(encode(&message).unwrap());
        }
        input.send(Ok(Message::Binary(batch.into()))).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let view = app.lock().unwrap().view();
                if view.devices[0].online && view.devices[0].selected.as_deref() == Some("weather")
                {
                    break;
                }
                assert!(!task.is_finished(), "session ended after delayed ACK");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let new = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &session,
                "weather",
                vec![2; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 20 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let mut assembler = FrameAssembler::default();
        let complete: Vec<_> = recorded
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let Message::Binary(bytes) = message else {
                    return None;
                };
                match Decoder::default().push(bytes).unwrap().remove(0) {
                    message @ (WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }) => {
                        assembler.accept(message).unwrap()
                    }
                    WireMessage::Error { code } => panic!("unexpected protocol error {code}"),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(complete.len(), 2);
        assert_eq!(complete[1].0, new);
        assert_eq!(complete[1].1, vec![2; crate::protocol::LCD_FRAME_BYTES]);
        for message in [
            WireMessage::ReceiptAck { token: new.clone() },
            WireMessage::RenderedAck { token: new.clone() },
        ] {
            input
                .send(Ok(Message::Binary(encode(&message).unwrap().into())))
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while app.lock().unwrap().view().devices[0].last_rendered_frame != Some(new.frame_id) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let mut batch = Vec::new();
        for message in [
            WireMessage::ReceiptAck { token: new.clone() },
            WireMessage::RenderedAck { token: new },
            WireMessage::Next {
                incarnation: 1,
                session_epoch: 1,
            },
        ] {
            batch.extend(encode(&message).unwrap());
        }
        input.send(Ok(Message::Binary(batch.into()))).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let view = app.lock().unwrap().view();
                if view.devices[0].online
                    && view.devices[0].selected.as_deref() == Some("rain-radar")
                {
                    break;
                }
                assert!(!task.is_finished(), "session ended after duplicate ACK");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        drop(input);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn mock_terminal_hello_settings_input_reconnect_and_old_session_guard() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = web::router(app.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut first, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/device"))
                .await
                .unwrap();
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let (incarnation, first_epoch) = match read_wire(&mut first).await {
            WireMessage::Welcome {
                incarnation,
                session_epoch,
            } => (incarnation, session_epoch),
            other => panic!("{other:?}"),
        };
        let settings = read_wire(&mut first).await;
        let WireMessage::Settings { snapshot } = settings else {
            panic!("settings expected")
        };
        assert_eq!(snapshot.incarnation, incarnation);
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::SettingsStored {
                    incarnation,
                    revision: snapshot.revision,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if app.lock().unwrap().view().devices[0].stored_revision == snapshot.revision {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(app.lock().unwrap().view().devices[0].pending);
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::Heartbeat {
                    incarnation,
                    session_epoch: first_epoch,
                    applied_revision: snapshot.revision,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if !app.lock().unwrap().view().devices[0].pending {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!app.lock().unwrap().view().devices[0].pending);
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::Next {
                    incarnation,
                    session_epoch: first_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        assert!(
            matches!(read_wire(&mut first).await, WireMessage::InputAccepted { selected:Some(ref name), .. } if name=="caricature")
        );
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::EnterSub {
                    incarnation,
                    session_epoch: first_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        assert!(
            matches!(read_wire(&mut first).await, WireMessage::InputAccepted {
            selected: Some(name), ..
        } if name == "caricature")
        );
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::Up {
                    incarnation,
                    session_epoch: first_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        assert!(
            matches!(read_wire(&mut first).await, WireMessage::InputAccepted {
            selected: Some(name), ..
        } if name == "youtube")
        );
        first
            .send(ClientMessage::Binary(
                encode(&WireMessage::Down {
                    incarnation,
                    session_epoch: first_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        assert!(
            matches!(read_wire(&mut first).await, WireMessage::InputAccepted {
            selected: Some(name), ..
        } if name == "caricature")
        );
        let (mut second, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/device"))
                .await
                .unwrap();
        second
            .send(ClientMessage::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 2,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let new_epoch = match read_wire(&mut second).await {
            WireMessage::Welcome { session_epoch, .. } => session_epoch,
            other => panic!("{other:?}"),
        };
        assert!(new_epoch > first_epoch);
        assert!(matches!(
            read_wire(&mut second).await,
            WireMessage::Settings { .. }
        ));
        // The old socket may close before it can send another input. The pure
        // session guard is checked independently and cannot move the selection.
        let old = SessionToken {
            device_id: "desk".into(),
            incarnation,
            session_epoch: first_epoch,
        };
        assert!(!app
            .lock()
            .unwrap()
            .terminal_event(Event::Next { token: old })
            .unwrap());
        assert_eq!(
            app.lock().unwrap().view().devices[0].selected.as_deref(),
            Some("caricature")
        );
        second.close(None).await.unwrap();
        drop(first);
        drop(second);
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn cached_weather_frame_reaches_fast_mock_with_both_acks_despite_slow_peer() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn({
            let app = app.clone();
            async move { axum::serve(listener, web::router(app)).await.unwrap() }
        });
        let mut clients = Vec::new();
        let mut tokens = Vec::new();
        for id in ["slow", "fast"] {
            let (mut ws, _) =
                tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/device"))
                    .await
                    .unwrap();
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Hello {
                    device_id: id.into(),
                    capabilities: vec!["lcd".into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            let (incarnation, session_epoch) = match read_wire(&mut ws).await {
                WireMessage::Welcome {
                    incarnation,
                    session_epoch,
                } => (incarnation, session_epoch),
                other => panic!("{other:?}"),
            };
            assert!(matches!(
                read_wire(&mut ws).await,
                WireMessage::Settings { .. }
            ));
            for expected in ["caricature", "weather"] {
                ws.send(ClientMessage::Binary(
                    encode(&WireMessage::Next {
                        incarnation,
                        session_epoch,
                    })
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
                assert!(
                    matches!(read_wire(&mut ws).await, WireMessage::InputAccepted { selected: Some(name), .. } if name == expected)
                );
            }
            tokens.push(SessionToken {
                device_id: id.into(),
                incarnation,
                session_epoch,
            });
            clients.push(ws);
        }
        let cache = include_bytes!("../assets/jma-tokyo-20261012.json");
        let set = ForecastSet::tokyo_from_cache(cache, "cached-generation", "2026-10-12").unwrap();
        let slow_frame = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &tokens[0],
                "weather",
                crate::weather::render_lcd(&set, &set.days[0]).unwrap(),
            )
            .unwrap();
        let fast_frame = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &tokens[1],
                "weather",
                crate::weather::render_lcd(&set, &set.days[0]).unwrap(),
            )
            .unwrap();
        let mut fast = clients.pop().unwrap();
        let slow = clients.pop().unwrap();
        let mut assembler = FrameAssembler::default();
        let mut gated: Option<GatedFrameReceiver> = None;
        let mut receipt = None;
        let mut received = None;
        for _ in 0..8 {
            let message = read_wire(&mut fast).await;
            if let WireMessage::FrameStart { token, .. } = &message {
                gated = Some(GatedFrameReceiver::new(PicoScreen {
                    kind: ScreenKind::Lcd,
                    device_id: token.device_id.clone(),
                    incarnation: token.incarnation,
                    session_epoch: token.session_epoch,
                    display_epoch: token.display_epoch,
                    screen_owner_epoch: token.screen_owner_epoch,
                    local: None,
                    drawing: None,
                    pending_standby: false,
                }));
            }
            receipt = gated
                .as_mut()
                .unwrap()
                .accept(message.clone())
                .unwrap()
                .pop()
                .or(receipt);
            received = assembler.accept(message).unwrap().or(received);
        }
        let (token, pixels) = received.unwrap();
        assert_eq!(token, fast_frame);
        assert_eq!(pixels.len(), crate::protocol::LCD_FRAME_BYTES);
        assert_ne!(fast_frame.device_id, slow_frame.device_id);
        fast.send(ClientMessage::Binary(
            encode(&receipt.expect("gated receipt")).unwrap().into(),
        ))
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let device = app
                    .lock()
                    .unwrap()
                    .view()
                    .devices
                    .into_iter()
                    .find(|d| d.id == "fast")
                    .unwrap();
                if device.last_received_frame == Some(fast_frame.frame_id) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            app.lock()
                .unwrap()
                .view()
                .devices
                .iter()
                .find(|d| d.id == "fast")
                .unwrap()
                .last_rendered_frame,
            None
        );
        fast.send(ClientMessage::Binary(
            encode(
                &gated
                    .as_mut()
                    .unwrap()
                    .complete_draw(fast_frame.clone())
                    .expect("gated draw"),
            )
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let device = app
                    .lock()
                    .unwrap()
                    .view()
                    .devices
                    .into_iter()
                    .find(|d| d.id == "fast")
                    .unwrap();
                if device.last_rendered_frame == Some(fast_frame.frame_id) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            app.lock()
                .unwrap()
                .view()
                .devices
                .iter()
                .find(|d| d.id == "slow")
                .unwrap()
                .last_rendered_frame,
            None
        );
        let stale = app
            .lock()
            .unwrap()
            .queue_lcd_frame(
                &tokens[1],
                "weather",
                crate::weather::render_lcd(&set, &set.days[1]).unwrap(),
            )
            .unwrap();
        drop(fast);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !app.lock().unwrap().session_is_current(&tokens[1]) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!app
            .lock()
            .unwrap()
            .terminal_event(Event::RenderedAck { token: stale })
            .unwrap());
        drop(slow);
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn weather_buttons_pin_browse_stop_at_edges_and_cancel_to_latest_today() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn({
            let app = app.clone();
            async move { axum::serve(listener, web::router(app)).await.unwrap() }
        });
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/device"))
            .await
            .unwrap();
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Hello {
                device_id: "desk".into(),
                capabilities: vec!["lcd".into()],
                screen_owner_epoch: 1,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let (incarnation, session_epoch) = match read_wire(&mut ws).await {
            WireMessage::Welcome {
                incarnation,
                session_epoch,
            } => (incarnation, session_epoch),
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            read_wire(&mut ws).await,
            WireMessage::Settings { .. }
        ));
        for _ in 0..2 {
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Next {
                    incarnation,
                    session_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            assert!(matches!(
                read_wire(&mut ws).await,
                WireMessage::InputAccepted { .. }
            ));
        }
        let source = include_bytes!("../assets/jma-tokyo-20261012.json");
        let set = ForecastSet::tokyo_from_cache(source, "old", "2026-10-12").unwrap();
        let token = SessionToken {
            device_id: "desk".into(),
            incarnation,
            session_epoch,
        };
        app.lock()
            .unwrap()
            .publish_plugin_source("weather", &token, 1, source, "old", "2026-10-12")
            .unwrap();
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::EnterSub {
                incarnation,
                session_epoch,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        assert!(
            matches!(read_wire(&mut ws).await, WireMessage::PluginPosition { dataset_id, target_date, index:0, .. }
            if dataset_id=="old" && target_date=="2026-10-12")
        );
        let mut receiver = FrameAssembler::default();
        let mut first = None;
        for _ in 0..8 {
            first = receiver.accept(read_wire(&mut ws).await).unwrap().or(first);
        }
        assert_eq!(
            first.unwrap().1,
            crate::weather::render_lcd(&set, &set.days[0]).unwrap()
        );
        let mut updated = set.clone();
        updated.id = "new".into();
        updated.days[0].max_c = Some(28);
        let mut revised_source: serde_json::Value = serde_json::from_slice(source).unwrap();
        revised_source[0]["timeSeries"][2]["areas"][0]["temps"][0] = serde_json::json!("28");
        let revised_bytes = serde_json::to_vec(&revised_source).unwrap();
        app.lock()
            .unwrap()
            .publish_plugin_source("weather", &token, 1, &revised_bytes, "new", "2026-10-12")
            .unwrap();
        async fn position<S>(ws: &mut S) -> (String, String, usize)
        where
            S: StreamExt<Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>>
                + Unpin,
        {
            loop {
                match read_wire(ws).await {
                    WireMessage::PluginPosition {
                        dataset_id,
                        target_date,
                        index,
                        ..
                    } => return (dataset_id, target_date, index),
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. } => {}
                    other => panic!("{other:?}"),
                }
            }
        }
        for expected in 1..=6 {
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Down {
                    incarnation,
                    session_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            let (id, date, index) = position(&mut ws).await;
            assert_eq!(id, "old");
            assert_eq!(index, expected);
            assert_eq!(date, set.days[expected].date_jst);
        }
        for _ in 0..3 {
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Down {
                    incarnation,
                    session_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            assert_eq!(
                position(&mut ws).await,
                ("old".into(), "2026-10-18".into(), 6)
            );
        }
        for expected in (0..6).rev() {
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Up {
                    incarnation,
                    session_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            assert_eq!(
                position(&mut ws).await,
                ("old".into(), set.days[expected].date_jst.clone(), expected)
            );
        }
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Up {
                incarnation,
                session_epoch,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        assert_eq!(
            position(&mut ws).await,
            ("old".into(), "2026-10-12".into(), 0)
        );
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Power {
                incarnation,
                session_epoch,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        assert_eq!(
            position(&mut ws).await,
            ("new".into(), "2026-10-12".into(), 0)
        );
        let mut receiver = FrameAssembler::default();
        let latest = loop {
            let msg = read_wire(&mut ws).await;
            if let Ok(Some(result)) = receiver.accept(msg) {
                if result.1 == crate::weather::render_lcd(&updated, &updated.days[0]).unwrap() {
                    break result;
                }
            }
        };
        assert_eq!(latest.1.len(), crate::protocol::LCD_FRAME_BYTES);
        drop(ws);
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn radar_mock_terminal_browses_real_hourly_frames_and_cancels_to_latest_observation() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn({
            let app = app.clone();
            async move { axum::serve(listener, web::router(app)).await.unwrap() }
        });
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/device"))
            .await
            .unwrap();
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Hello {
                device_id: "radar-desk".into(),
                capabilities: vec!["lcd".into()],
                screen_owner_epoch: 1,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let (incarnation, session_epoch) = match read_wire(&mut ws).await {
            WireMessage::Welcome {
                incarnation,
                session_epoch,
            } => (incarnation, session_epoch),
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            read_wire(&mut ws).await,
            WireMessage::Settings { .. }
        ));
        for _ in 0..3 {
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Next {
                    incarnation,
                    session_epoch,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            assert!(
                matches!(read_wire(&mut ws).await,WireMessage::InputAccepted{selected:Some(id),..} if id=="rain-radar" || id=="caricature" || id=="weather")
            );
        }
        let token = SessionToken {
            device_id: "radar-desk".into(),
            incarnation,
            session_epoch,
        };
        let source = include_bytes!("../assets/jma-radar-20261012.json");
        app.lock()
            .unwrap()
            .publish_plugin_source("rain-radar", &token, 1, source, "set-one", "2026-10-12")
            .unwrap();
        async fn input<S>(ws: &mut S, message: WireMessage)
        where
            S: SinkExt<ClientMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
        {
            ws.send(ClientMessage::Binary(encode(&message).unwrap().into()))
                .await
                .unwrap();
        }
        async fn state<S>(
            ws: &mut S,
            frame_expected: bool,
        ) -> (String, String, usize, Option<Vec<u8>>)
        where
            S: StreamExt<Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>>
                + Unpin,
        {
            let mut position = None;
            let mut pixels = None;
            let mut assembler = FrameAssembler::default();
            loop {
                let message = read_wire(ws).await;
                match &message {
                    WireMessage::PluginPosition {
                        dataset_id,
                        target_date,
                        index,
                        ..
                    } => {
                        position = Some((dataset_id.clone(), target_date.clone(), *index));
                    }
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. } => {
                        if let Some((_, bytes)) = assembler.accept(message).unwrap() {
                            pixels = Some(bytes);
                        }
                    }
                    other => panic!("unexpected radar message: {other:?}"),
                }
                if !frame_expected || pixels.is_some() {
                    if let Some((id, date, index)) = position.take() {
                        return (id, date, index, pixels);
                    }
                }
            }
        }
        input(
            &mut ws,
            WireMessage::EnterSub {
                incarnation,
                session_epoch,
            },
        )
        .await;
        let (id, date, index, latest) = state(&mut ws, true).await;
        assert_eq!(
            (id.as_str(), date.as_str(), index),
            ("set-one", "20261012032000", 3)
        );
        let latest = latest.unwrap();
        assert_eq!(latest.len(), crate::protocol::LCD_FRAME_BYTES);
        let mut previous = latest.clone();
        for (index, date) in [
            (2, "20261012022000"),
            (1, "20261012012000"),
            (0, "20261012002000"),
        ] {
            input(
                &mut ws,
                WireMessage::Up {
                    incarnation,
                    session_epoch,
                },
            )
            .await;
            let (id, actual, found, frame) = state(&mut ws, true).await;
            assert_eq!(
                (id.as_str(), actual.as_str(), found),
                ("set-one", date, index)
            );
            assert_ne!(frame.as_ref().unwrap(), &previous);
            previous = frame.unwrap();
        }
        input(
            &mut ws,
            WireMessage::Up {
                incarnation,
                session_epoch,
            },
        )
        .await;
        let (_, date, index, frame) = state(&mut ws, false).await;
        assert_eq!((date.as_str(), index), ("20261012002000", 0));
        assert!(frame.is_none());
        for (index, date) in [
            (1, "20261012012000"),
            (2, "20261012022000"),
            (3, "20261012032000"),
            (4, "20261012042000"),
        ] {
            input(
                &mut ws,
                WireMessage::Down {
                    incarnation,
                    session_epoch,
                },
            )
            .await;
            let (id, actual, found, frame) = state(&mut ws, true).await;
            assert_eq!(
                (id.as_str(), actual.as_str(), found),
                ("set-one", date, index)
            );
            assert_eq!(frame.unwrap().len(), crate::protocol::LCD_FRAME_BYTES);
        }
        input(
            &mut ws,
            WireMessage::Down {
                incarnation,
                session_epoch,
            },
        )
        .await;
        let (_, date, index, frame) = state(&mut ws, false).await;
        assert_eq!((date.as_str(), index), ("20261012042000", 4));
        assert!(frame.is_none());
        app.lock()
            .unwrap()
            .publish_plugin_source("rain-radar", &token, 1, source, "set-two", "2026-10-12")
            .unwrap();
        assert_eq!(
            app.lock()
                .unwrap()
                .plugin_position(&token)
                .unwrap()
                .dataset_id,
            "set-one"
        );
        input(
            &mut ws,
            WireMessage::Power {
                incarnation,
                session_epoch,
            },
        )
        .await;
        let (id, date, index, frame) = state(&mut ws, true).await;
        assert_eq!(
            (id.as_str(), date.as_str(), index),
            ("set-two", "20261012032000", 3)
        );
        assert_eq!(frame.unwrap(), latest);
        drop(ws);
        server.abort();
        let _ = server.await;
    }
}
