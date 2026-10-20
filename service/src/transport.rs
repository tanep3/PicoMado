//! Pico WebSocket adapter with bounded control and LCD frame dispatch.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{State, Extension, ConnectInfo};
use crate::socket_progress::WriteProgress;
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
        WireMessage::StreamStatus { .. } => "stream_status",
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

pub async fn upgrade(State(app): State<SharedApp>, progress: Option<Extension<ConnectInfo<WriteProgress>>>, ws: WebSocketUpgrade) -> Response {
    let progress=progress.map(|Extension(ConnectInfo(p))|p);
    ws.write_buffer_size(0)
        .max_write_buffer_size(crate::lcd_stream::WEBSOCKET_WRITE_BUFFER_BYTES)
        .max_message_size(MAX_CHUNK_BYTES)
        .max_frame_size(MAX_CHUNK_BYTES)
        .on_upgrade(move |socket| serve_socket_with_progress(socket, app, progress))
}

#[cfg(test)]
fn process(
    app: &SharedApp,
    current: &mut Option<SessionToken>,
    message: WireMessage,
    diagnostics: &mut AckDiagnostics,
) -> Result<Option<WireMessage>, &'static str> {
    process_on_connection(app, current, message, diagnostics, None)
}

#[cfg(test)]
fn process_on_connection(app:&SharedApp,current:&mut Option<SessionToken>,message:WireMessage,diagnostics:&mut AckDiagnostics,accept_order:Option<std::num::NonZeroU64>)->Result<Option<WireMessage>,&'static str>{
    process_with_observation(app,current,message,diagnostics,accept_order,None)
}

fn process_with_observation(
    app: &SharedApp,
    current: &mut Option<SessionToken>,
    message: WireMessage,
    diagnostics: &mut AckDiagnostics,
    accept_order: Option<std::num::NonZeroU64>,
    cause: Option<crate::observation::Cause>,
) -> Result<Option<WireMessage>, &'static str> {
    let received = std::time::Instant::now();
    let received_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let kind = wire_label(&message);
    let frame_id = match &message {
        WireMessage::ReceiptAck { token } | WireMessage::RenderedAck { token } => token.frame_id,
        _ => 0,
    };
    let mut lock_wait = Duration::ZERO;
    let mut persistence_us = 0;
    let mut image_preparation_us = 0;
    let ignored_before = diagnostics.ignored;
    let result = process_inner(
        app,
        current,
        message,
        diagnostics,
        accept_order,
        &mut lock_wait,
        &mut persistence_us,
        &mut image_preparation_us,
        cause,
    );
    let elapsed = received.elapsed();
    let sampled = diagnostics.ignored == ignored_before
        || diagnostics.ignored <= 4
        || diagnostics.ignored.is_power_of_two();
    if sampled
        && matches!(
            kind,
            "next" | "power" | "sub" | "up" | "down" | "receipt_ack" | "rendered_ack"
        )
        || elapsed >= Duration::from_millis(50)
    {
        let (id, incarnation, session_epoch) = session_identity(current);
        eprintln!("picomado message_processed id={} incarnation={} session_epoch={} kind={} frame_id={} received_unix_ms={} lock_wait_us={} processing_us={} persistence_us={} image_preparation_us={} elapsed_us={} outcome={}",
            id, incarnation, session_epoch, kind, frame_id, received_unix_ms,
            lock_wait.as_micros(), elapsed.saturating_sub(lock_wait).as_micros(), persistence_us, image_preparation_us, elapsed.as_micros(),
            if result.is_ok() { "ok" } else { "rejected" });
    }
    result
}

fn process_inner(
    app: &SharedApp,
    current: &mut Option<SessionToken>,
    message: WireMessage,
    diagnostics: &mut AckDiagnostics,
    accept_order: Option<std::num::NonZeroU64>,
    lock_wait: &mut Duration,
    persistence_us: &mut u128,
    image_preparation_us: &mut u128,
    cause: Option<crate::observation::Cause>,
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
        let waiting = std::time::Instant::now();
        let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Reader).map_err(|_| "unavailable")?;
        *lock_wait = waiting.elapsed();
        let totals = guard.operation_totals();
        let result = match accept_order {
            Some(order) => guard.hello_from_connection(&device_id, caps, screen_owner_epoch, order),
            None => guard.hello(&device_id, caps, screen_owner_epoch),
        };
        let token = result.map_err(|error| match error {
            crate::application::AppError::Conflict("stale_connection") => "stale_connection",
            _ => "invalid_hello",
        })?;
        let after = guard.operation_totals();
        *persistence_us = after.0.saturating_sub(totals.0);
        *image_preparation_us = after.1.saturating_sub(totals.1);
        *current = Some(token.clone());
        return Ok(Some(WireMessage::Welcome {
            incarnation: token.incarnation,
            session_epoch: token.session_epoch,
        }));
    }
    let token = current.as_ref().ok_or("hello_required")?;
    let waiting = std::time::Instant::now();
    let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Reader).map_err(|_| "unavailable")?;
    *lock_wait = waiting.elapsed();
    if !guard.session_is_current(token) {
        return Err("stale_session");
    }
    let totals = guard.operation_totals();
    guard.observation_source().record_context(crate::observation::Component::Api,crate::observation::Phase::Dispatched,0,0,0,message.kind() as u64,0,cause);
    let result = guard.with_observation_cause(&token.device_id,cause,|guard| match message {
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
                    input_context: Some(crate::protocol::InputContext {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                        display_epoch,
                        mode: guard.session_mode(token).unwrap_or("normal").into(),
                    }),
                    selected,
                    display_epoch,
                },
                InputResult::Position(position) => WireMessage::PluginPosition {
                    input_context: Some(crate::protocol::InputContext {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                        display_epoch: after.as_ref().map_or(0, |v| v.1),
                        mode: guard.session_mode(token).unwrap_or("normal").into(),
                    }),
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
        WireMessage::StreamStatus {
            token: frame,
            generation,
            received_frames,
            rendered_frames,
            dropped_frames,
            elapsed_ms,
            max_gap_us,
        } => {
            if !guard.supports_lcd_push(token)
                || frame.device_id != token.device_id
                || frame.incarnation != token.incarnation
                || frame.session_epoch != token.session_epoch
                || !guard.frame_ack_within_known_generations(&frame)
                || rendered_frames > received_frames
                || dropped_frames > received_frames
                || rendered_frames
                    .checked_add(dropped_frames)
                    .is_none_or(|n| n > received_frames)
            {
                return Err("invalid_stream_status");
            }
            let accepted = guard.lcd_stream_status(&frame, generation);
            let diagnostic_log=crate::diagnostics::mark();
            eprintln!("picomado stream_status id={} frame_id={} generation={} received_frames={} rendered_frames={} dropped_frames={} elapsed_ms={} max_gap_us={} accepted={}",
                token.device_id,frame.frame_id,generation,received_frames,rendered_frames,dropped_frames,elapsed_ms,max_gap_us,accepted);
            crate::diagnostics::duration(crate::diagnostics::Kind::LogIo,diagnostic_log,0,0,0);
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
    });
    let after = guard.operation_totals();
    *persistence_us = after.0.saturating_sub(totals.0);
    *image_preparation_us = after.1.saturating_sub(totals.1);
    result
}

struct QueuedReply {
    message: WireMessage,
    cause: Option<crate::observation::Cause>,
    #[cfg(feature="stream-diagnostics")]
    enqueued_at: u64,
}
enum WriterCommand {
    #[cfg(test)]
    Bind(SessionToken),
    BoundReply(SessionToken, QueuedReply),
    Reply(QueuedReply),
    Stop,
}
impl WriterCommand {
    #[cfg(test)]
    #[inline(always)]
    fn reply(message:WireMessage)->Self {Self::reply_observed(message,None,None)}
    fn reply_observed(message:WireMessage,cause:Option<crate::observation::Cause>,binding:Option<SessionToken>)->Self {
        let reply=QueuedReply{message,cause,#[cfg(feature="stream-diagnostics")] enqueued_at:crate::diagnostics::mark()};
        match binding {Some(token)=>Self::BoundReply(token,reply),None=>Self::Reply(reply)}
    }
}
#[cfg(all(test,feature="boundary-observation"))]
async fn enqueue_observed_reply(tx:&tokio::sync::mpsc::Sender<WriterCommand>,message:WireMessage,session:u64,cause:Option<crate::observation::Cause>)->Result<(),()> {
    enqueue_reply_common(tx,message,session,cause,None).await
}
async fn enqueue_reply(tx:&tokio::sync::mpsc::Sender<WriterCommand>,message:WireMessage,session:u64)->Result<(),()> {
    enqueue_reply_common(tx,message,session,None,None).await
}
async fn enqueue_reply_common(tx:&tokio::sync::mpsc::Sender<WriterCommand>,message:WireMessage,_session:u64,cause:Option<crate::observation::Cause>,binding:Option<SessionToken>)->Result<(),()> {
    #[cfg(not(feature="stream-diagnostics"))]
    {tx.send(WriterCommand::reply_observed(message,cause,binding)).await.map_err(|_|())}
    #[cfg(feature="stream-diagnostics")]
    {
        let start=crate::diagnostics::mark();
        let permit=tx.reserve().await.map_err(|_|())?;
        crate::diagnostics::duration(crate::diagnostics::Kind::ReplyCapacity,start,_session,0,0);
        permit.send(WriterCommand::reply_observed(message,cause,binding));Ok(())
    }
}
const SEND_TIMEOUT: Duration = Duration::from_millis(250);
// Local kernel-write acceptance only; no TCP ACK or device arrival inference.
const FRAGMENT_STALL_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_FRAME_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_PROGRESS_POLL: Duration = Duration::from_millis(25);
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

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u8)]
enum WriteFailure {Deadline=1,Sink=2,Encode=3}
async fn write_wire<S>(sink: &mut S, message: WireMessage) -> Result<(), WriteFailure>
where
    S: Sink<Message> + Unpin,
{
    let bytes = encode(&message).map_err(|_| WriteFailure::Encode)?;
    match tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Binary(bytes.into()))).await {
        Ok(Ok(()))=>Ok(()),
        Ok(Err(_))=>Err(WriteFailure::Sink),
        Err(_)=>Err(WriteFailure::Deadline),
    }
}

#[derive(Clone,Copy,Debug)]
enum FrameWriteFailure { Idle(tokio::time::Instant), Total(tokio::time::Instant), Control, Stop, Session, Sink }
impl FrameWriteFailure {
    fn reason(self,stream:bool)->&'static str {
        match self {
            Self::Control=>"control_preempt_timeout",Self::Stop=>"requested_stop",Self::Session=>"session_not_current",
            Self::Idle(_) if stream=>"stream_write_stall",Self::Total(_) if stream=>"stream_write_stall",Self::Sink if stream=>"stream_write_stall",
            Self::Idle(_)=>"write_stall",Self::Total(_)=>"frame_deadline",Self::Sink=>"sink_error",
        }
    }
}
async fn write_observed_wire<S>(sink:&mut S,message:WireMessage,cause:Option<crate::observation::Cause>)->Result<(),WriteFailure> where S:Sink<Message>+Unpin {
    if cause.is_none(){return write_wire(sink,message).await;}
    let bytes=crate::protocol::encode_observed(&message,cause).map_err(|_|WriteFailure::Encode)?;
    tokio::time::timeout(SEND_TIMEOUT,sink.send(Message::Binary(bytes.into()))).await.map_err(|_|WriteFailure::Deadline)?.map_err(|_|WriteFailure::Sink)
}

fn log_write_failure(failure:FrameWriteFailure,session:u64,frame:u64,sequence:u64,started:u64){
    let kind=match failure {FrameWriteFailure::Idle(_)=>Some(crate::diagnostics::Kind::FragmentDeadline),FrameWriteFailure::Total(_)=>Some(crate::diagnostics::Kind::FrameDeadline),FrameWriteFailure::Sink=>Some(crate::diagnostics::Kind::SendError),_=>None};
    let _late=match failure {FrameWriteFailure::Idle(at)|FrameWriteFailure::Total(at)=>tokio::time::Instant::now().saturating_duration_since(at).as_micros() as u64,_=>0};
    if let Some(kind)=kind {crate::diagnostics::event(kind,session,frame,crate::diagnostics::mark().saturating_sub(started),sequence,_late);}
    #[cfg(feature="stream-diagnostics")]
    if _late>0 {let now=crate::diagnostics::mark();crate::diagnostics::duration(crate::diagnostics::Kind::DeadlineWake,now.saturating_sub(_late),session,frame,0);}
    eprintln!("picomado frame_write_end session_epoch={} frame_id={} cause={:?} progress=kernel_write_acceptance",session,frame,failure);
}
// Only one WS/application message is in flight. On control arrival it must
// finish within250ms or the entire connection ends; never splice a reply into
// a partly written WS message. One pending command plus existing32-slot channel.
async fn send_frame_packet<S>(
    sink:&mut S,bytes:Vec<u8>,rx:&mut tokio::sync::mpsc::Receiver<WriterCommand>,
    pending:&mut Option<WriterCommand>,progress:Option<&WriteProgress>,total:tokio::time::Instant,
    session:Option<(&SharedApp,&SessionToken)>,
)->Result<(),FrameWriteFailure> where S:Sink<Message>+Unpin {
    let mut observed=progress.map_or(0,WriteProgress::bytes);
    let mut idle=tokio::time::Instant::now()+FRAGMENT_STALL_TIMEOUT;
    let mut control=None;
    let send=sink.send(Message::Binary(bytes.into()));tokio::pin!(send);
    let mut tick=tokio::time::interval_at(tokio::time::Instant::now()+WRITE_PROGRESS_POLL,WRITE_PROGRESS_POLL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let current=progress.map_or(0,WriteProgress::bytes);
        if current!=observed {observed=current;idle=tokio::time::Instant::now()+FRAGMENT_STALL_TIMEOUT;}
        let deadline=idle.min(total).min(control.unwrap_or(total));
        tokio::select! {biased;
            _=tokio::time::sleep_until(deadline)=>{
                let now=tokio::time::Instant::now();
                // Classify the deadline selected before sleep, not whichever
                // other budget a late executor wake has also crossed.
                if control.is_some_and(|at|at<=idle && at<=total){return Err(FrameWriteFailure::Control);}
                if total<=idle{return Err(FrameWriteFailure::Total(total));}
                if now<total && control.is_none_or(|at|now<at) && progress.is_some_and(|p|p.bytes()!=observed){continue;}
                return Err(FrameWriteFailure::Idle(idle));
            },
            result=&mut send=>return result.map_err(|_|FrameWriteFailure::Sink),
            command=rx.recv(),if pending.is_none()=>match command {
                Some(WriterCommand::Stop)|None=>return Err(FrameWriteFailure::Stop),
                Some(command)=>{*pending=Some(command);control=Some(tokio::time::Instant::now()+SEND_TIMEOUT);},
            },
            _=tick.tick()=>{
                if session.is_some_and(|(app,t)|!crate::diagnostics::lock(app,crate::diagnostics::Domain::Writer).is_ok_and(|a|a.session_is_current(t))){return Err(FrameWriteFailure::Session);}
            },
        }
    }
}

fn enqueue_outbox(
    app: &SharedApp,
    token: &SessionToken,
    queue: &mut FairQueue,
    next_packet_id: &mut u64,
    plans: &mut std::collections::BTreeMap<u64, FramePlan>,
) -> Result<bool, ()> {
    let effects = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).map_err(|_| ())?.pending_effects();
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
                crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                    let guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).map_err(|_| ())?;
                    if guard.frame_is_current(&frame) {
                        guard
                            .frame_asset(&asset_handle)
                            .zip(guard.frame_mode(&frame))
                            .map(|(asset, mode)| {
                                (
                                    asset,
                                    mode,
                                    guard.binary_frames(token),
                                    guard.frame_eink_refresh(&frame),
                                    guard.frame_observation_cause(&frame),
                                )
                            })
                    } else {
                        None
                    }
                };
                let Some((asset, mode, binary, eink_refresh, cause)) = asset else {
                    continue;
                };
                let (width, height, format) = if asset.len() == crate::eink::FRAME_BYTES {
                    (250, 122, "mono1")
                } else if asset.len() == crate::protocol::LCD_FRAME_BYTES {
                    (160, 80, "rgb565be")
                } else {
                    return Err(());
                };
                let start = WireMessage::FrameStart {
                    eink_refresh,
                    stream: None,
                    mode: Some(mode.into()),
                    token: frame.clone(),
                    width,
                    height,
                    format: format.into(),
                    byte_len: asset.len() as u32,
                    crc32: crc32fast::hash(&asset),
                };
                let encoded_start = crate::protocol::encode_observed(&start,cause).map_err(|_| ())?;
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
                    let encoded_chunk = if binary {
                        crate::protocol::encode_frame_bytes(&message)
                    } else {
                        encode(&message)
                    }
                    .map_err(|_| ())?;
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

#[cfg(test)]
async fn writer_loop<S>(
    sink: S,
    rx: tokio::sync::mpsc::Receiver<WriterCommand>,
    app: SharedApp,
) -> &'static str
where
    S: Sink<Message> + Unpin,
{
    writer_loop_with_ack_timeout(sink, rx, app, None).await
}
#[cfg(test)]
async fn writer_loop_with_ack_timeout<S>(
    sink: S,
    rx: tokio::sync::mpsc::Receiver<WriterCommand>,
    app: SharedApp,
    ack_timeout_override: Option<Duration>,
) -> &'static str
where
    S: Sink<Message> + Unpin,
{
    writer_loop_with_progress(sink,rx,app,ack_timeout_override,None).await
}
async fn writer_loop_with_progress<S>(
    mut sink:S, mut rx:tokio::sync::mpsc::Receiver<WriterCommand>, app:SharedApp,
    ack_timeout_override:Option<Duration>, write_progress:Option<WriteProgress>,
)->&'static str where S:Sink<Message>+Unpin {
    let mut pending_command=None;
    let mut current: Option<SessionToken> = None;
    let mut queue = FairQueue::default();
    let mut next_packet_id = 1;
    let mut plans = std::collections::BTreeMap::<u64, FramePlan>::new();
    let mut progress = std::collections::BTreeMap::<u64, FrameProgress>::new();
    let mut changes = match crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer) {
        Ok(guard) => guard.dispatch_notifications(),
        Err(_) => return "application_unavailable",
    };
    let mut awaiting_render: Option<(u64, tokio::time::Instant)> = None;
    let mut video_rx: Option<
        tokio::sync::watch::Receiver<Option<std::sync::Arc<crate::lcd_stream::VideoFrame>>>,
    > = None;
    let mut video_active: Option<crate::lcd_stream::VideoTransfer> = None;
    let mut video_pending = false;
    let mut refresh = false;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let end_reason = loop {
        tokio::select! {
            biased;
            command=async {if pending_command.is_some(){pending_command.take()}else{rx.recv().await}}=>match command {
                #[cfg(test)]
                Some(WriterCommand::Bind(token))=>{
                    video_rx=crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).ok().and_then(|mut a|a.subscribe_lcd_video(&token));
                    current=Some(token);refresh=true;video_pending=true;
                },
                Some(WriterCommand::BoundReply(token,reply))=>{
                    video_rx=crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).ok().and_then(|mut a|a.subscribe_lcd_video(&token));
                    current=Some(token);refresh=true;video_pending=true;
                    // This internal reply is ready before every refresh branch,
                    // independent of when the producer next gets scheduled.
                    pending_command=Some(WriterCommand::Reply(reply));
                },
                Some(WriterCommand::Reply(reply))=>{
                    #[cfg(feature="stream-diagnostics")]
                    crate::diagnostics::duration(crate::diagnostics::Kind::ReplyQueue,reply.enqueued_at,current.as_ref().map_or(0,|s|s.session_epoch),0,0);
                    let message=reply.message;
                    let kind=wire_label(&message);
                    let reply_context=match &message {
                        WireMessage::InputAccepted{input_context,..}|WireMessage::PluginPosition{input_context,..}=>input_context.clone(),
                        _=>None,
                    };
                    let reply_started=std::time::Instant::now();
                    let diagnostic_started=crate::diagnostics::mark();
                    let (_,_,session)=session_identity(&current);
                    crate::diagnostics::event(crate::diagnostics::Kind::ReplyStart,session,0,0,0,0);
                    if let Err(failure)=write_observed_wire(&mut sink,message,reply.cause).await {
                        crate::diagnostics::event(crate::diagnostics::Kind::ReplyError,session,0,failure as u64,0,0);
                        let (id,incarnation,session_epoch)=session_identity(&current);
                        eprintln!("picomado control_write_failed id={} incarnation={} session_epoch={} kind={}",id,incarnation,session_epoch,kind);
                        break "control_write_failed";
                    }
                    crate::diagnostics::duration(crate::diagnostics::Kind::ReplyDone,diagnostic_started,session,0,0);
                    if let Some(ctx)=reply_context {
                        let (id,_,_)=session_identity(&current);
                        eprintln!("picomado input_reply_sent id={} incarnation={} session_epoch={} display_epoch={} mode={} kind={} elapsed_us={}",id,ctx.incarnation,ctx.session_epoch,ctx.display_epoch,ctx.mode,kind,reply_started.elapsed().as_micros());
                    }
                },
                Some(WriterCommand::Stop)=>break "requested_stop",
                None=>break "command_channel_closed",
            },
            result=async {match &mut video_rx {Some(r)=>r.changed().await, None=>std::future::pending().await}},if video_rx.is_some()=>{
                if result.is_ok(){video_pending=true;}else{video_rx=None;}
            },
            result=changes.changed()=>{
                if result.is_err(){break "application_unavailable";}
                refresh=true;
            },
            _=tick.tick()=>{
                refresh=true;
                if let (Some(token),Some((frame_id,deadline)))=(&current,awaiting_render) {
                    if !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).is_ok_and(|guard|guard.frame_waiting_for_render(token,frame_id)) { awaiting_render=None; }
                    else if tokio::time::Instant::now()>=deadline {
                        eprintln!("picomado render_ack_timeout id={} incarnation={} session_epoch={} frame_id={}",token.device_id,token.incarnation,token.session_epoch,frame_id);
                        break "render_ack_timeout";
                    }
                }
            },
            _=std::future::ready(()),if refresh && current.is_some()=>{
                refresh=false;
                let token=current.as_ref().expect("bound");
                if !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).is_ok_and(|guard|guard.session_is_current(token)) {break "session_not_current";}
                match enqueue_outbox(&app,token,&mut queue,&mut next_packet_id,&mut plans) {
                    Ok(true)=>break "close_session_effect",
                    Err(())=>break "enqueue_failed",
                    Ok(false)=>{},
                }
                let stale: Vec<_> = progress.keys().copied().filter(|id|
                    !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).is_ok_and(|guard|guard.effect_pending(*id))).collect();
                for id in stale {
                    if let Some(frame)=progress.remove(&id) {
                        log_frame_end(&token.device_id,id,frame,"effect_cancelled");
                    }
                }
            }
            _=std::future::ready(()),if video_active.is_none() && video_pending && current.as_ref().is_some_and(|t|queue.bytes(&t.device_id)==0)=>{
                video_pending=false;
                let latest=video_rx.as_mut().and_then(|r|r.borrow_and_update().clone());
                if let Some(frame)=latest {
                    if crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).is_ok_and(|a|a.lcd_video_is_current(&frame)) {
                        // A new video supersedes the initial static placeholder;
                        // no application ACK may hold streaming or its connection.
                        awaiting_render=None;
                        let diagnostic_log=crate::diagnostics::mark();
                        eprintln!("picomado stream_frame_start id={} frame_id={} sequence={} fps={}",frame.token.device_id,frame.token.frame_id,frame.sequence,frame.fps);
                        crate::diagnostics::duration(crate::diagnostics::Kind::LogIo,diagnostic_log,0,0,0);
                        crate::diagnostics::event(crate::diagnostics::Kind::FrameStart,frame.token.session_epoch,frame.token.frame_id,frame.sequence,frame.generation,0);
                        #[cfg(feature="boundary-observation")]
                        let cause=crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).ok().and_then(|a|{
                            let cause=a.frame_observation_cause(&frame.token);
                            a.observation_source().record_context(crate::observation::Component::Frame,crate::observation::Phase::Pending,0,0,0,0,frame.token.frame_id,cause);
                            cause
                        });
                        #[cfg(not(feature="boundary-observation"))]
                        let cause=None;
                        video_active=Some(crate::lcd_stream::VideoTransfer::new_observed(frame,cause));
                    }
                }
            },
            _=tokio::task::yield_now(),if video_active.is_some()=>{
                let token=current.as_ref().expect("bound stream");
                if !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).is_ok_and(|a|a.session_is_current(token)) {break "session_not_current";}
                let active=video_active.as_mut().expect("active stream");
                // Once started, finish all chunks. Selection controls can be
                // interleaved only at complete application/WS message boundaries.
                let packet=match active.next_packet(){Ok(Some(p))=>p,Ok(None)=>{
                    let diagnostic_log=crate::diagnostics::mark();
                    eprintln!("picomado stream_frame_sent id={} frame_id={} elapsed_us={}",active.frame.token.device_id,active.frame.token.frame_id,active.started.elapsed().as_micros());
                    crate::diagnostics::duration(crate::diagnostics::Kind::LogIo,diagnostic_log,0,0,0);
                    #[cfg(feature="stream-diagnostics")]
                    {let end=crate::diagnostics::mark();let elapsed=active.started.elapsed().as_micros() as u64;
                        crate::diagnostics::duration(crate::diagnostics::Kind::FrameDone,end.saturating_sub(elapsed),token.session_epoch,active.frame.token.frame_id,0);}
                    video_active=None;continue;
                },Err(_)=>break "stream_encode_failed"};
                let frame_deadline=tokio::time::Instant::from_std(active.started)+STREAM_FRAME_TIMEOUT;
                let diagnostic_started=crate::diagnostics::mark();
                crate::diagnostics::event(crate::diagnostics::Kind::PacketStart,token.session_epoch,active.frame.token.frame_id,packet.len() as u64,active.frame.sequence,0);
                match send_frame_packet(&mut sink,packet,&mut rx,&mut pending_command,write_progress.as_ref(),frame_deadline,Some((&app,token))).await {
                    Ok(())=>crate::diagnostics::duration(crate::diagnostics::Kind::PacketDone,diagnostic_started,token.session_epoch,active.frame.token.frame_id,0),
                    Err(failure)=>{
                        log_write_failure(failure,token.session_epoch,active.frame.token.frame_id,active.frame.sequence,diagnostic_started);
                        break failure.reason(true);
                    }
                }
            },
            _=tokio::task::yield_now(),if video_active.is_none() && current.as_ref().is_some_and(|t|queue.bytes(&t.device_id)>0)=>{
                let token=current.as_ref().expect("bound");
                let Some(packet)=queue.pop_for(&token.device_id) else {continue;};
                let valid={let Ok(guard)=crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer) else {break "application_unavailable";};
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
                let packet_len=packet.bytes.len();
                let result=if let Some(limit)=frame_limit {
                    send_frame_packet(&mut sink,packet.bytes,&mut rx,&mut pending_command,write_progress.as_ref(),limit,Some((&app,token))).await
                }else {
                    match tokio::time::timeout_at(now+SEND_TIMEOUT,sink.send(Message::Binary(packet.bytes.into()))).await {
                        Ok(Ok(()))=>Ok(()),Ok(Err(_))=>Err(FrameWriteFailure::Sink),Err(_)=>Err(FrameWriteFailure::Idle(now+SEND_TIMEOUT)),
                    }
                };
                if let Err(failure)=result {
                    let reason=failure.reason(false);
                    if let Some(frame)=progress.remove(&packet.outbox_id){log_frame_end(&token.device_id,packet.outbox_id,frame,reason);}
                    break reason;
                }
                if let Some(frame)=progress.get_mut(&packet.outbox_id) {
                    frame.sent_bytes+=packet_len;
                }
                if packet.finish_effect {
                    let io_complete = std::time::Instant::now();
                    let waiting = std::time::Instant::now();
                    let mut lock_wait = Duration::ZERO;
                    let mut completion_time = Duration::ZERO;
                    let completed=crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).ok().and_then(|mut guard| {
                        lock_wait = waiting.elapsed();
                        let started = std::time::Instant::now();
                        let result = guard.complete_effect(packet.outbox_id).ok();
                        completion_time = started.elapsed();
                        result
                    });
                    if let Some(frame) = progress.get(&packet.outbox_id) {
                        eprintln!("picomado frame_socket_sent id={} effect_id={} frame_id={} socket_elapsed_us={} completion_lock_wait_us={} completion_us={}",
                            token.device_id, packet.outbox_id, frame.plan.frame_id,
                            io_complete.duration_since(frame.started).as_micros(), lock_wait.as_micros(), completion_time.as_micros());
                    }
                    if let Some(frame)=progress.remove(&packet.outbox_id) {
                        // Bound a lost/failed draw ACK without keeping a peer
                        // stuck forever. This wait holds neither sink nor App lock.
                        let budget=ack_timeout_override.unwrap_or(Duration::from_secs(if frame.plan.payload_bytes==crate::eink::FRAME_BYTES {15}else{10}));
                        awaiting_render=Some((frame.plan.frame_id,tokio::time::Instant::now()+budget));
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
    crate::diagnostics::event(crate::diagnostics::Kind::End,session_epoch,0,0,0,0);
    let _ = tokio::time::timeout(SEND_TIMEOUT, sink.send(Message::Close(None))).await;
    end_reason
}

pub async fn serve_socket(socket: WebSocket, app: SharedApp) {
    serve_socket_with_progress(socket,app,None).await;
}
async fn serve_socket_with_progress(socket:WebSocket,app:SharedApp,progress:Option<WriteProgress>){
    let (sink,stream)=socket.split();serve_split_with_progress(sink,stream,app,progress).await;
}

#[cfg(test)]
async fn serve_split<S, St, E>(sink: S, stream: St, app: SharedApp) -> &'static str
where
    S: Sink<Message> + Unpin + Send + 'static,
    St: Stream<Item = Result<Message, E>> + Unpin,
{
    serve_split_with_progress(sink,stream,app,None).await
}
async fn serve_split_with_progress<S,St,E>(sink:S,mut stream:St,app:SharedApp,write_progress:Option<WriteProgress>)->&'static str
where S:Sink<Message>+Unpin+Send+'static, St:Stream<Item=Result<Message,E>>+Unpin {
    let (accept_order, order_source) = match &write_progress {
        Some(progress) => (progress.accept_order(), "tcp_accept"),
        // Legacy/custom in-process hosts without TrackedListener have no TCP
        // accept metadata. Preserve them with explicit WS-start ordering.
        None => (crate::socket_progress::next_accept_order(), "websocket_start"),
    };
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let mut writer = tokio::spawn(writer_loop_with_progress(sink, rx, app.clone(),None,write_progress));
    let mut decoder = Decoder::default();
    let mut current: Option<SessionToken> = None;
    #[cfg(feature="ack-path-probe")]
    let mut probe_bind_pending=false;
    let mut writer_finished = false;
    static NEXT_CONNECTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let connection_id = NEXT_CONNECTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut last_rx = started;
    let (mut rx_frames, mut rx_messages, mut rx_bytes) = (0u64, 0u64, 0u64);
    let mut end_reason = "read_eof";
    let mut end_detail = "none";
    let mut diagnostics = AckDiagnostics::default();
    #[cfg(feature="boundary-observation")]
    let observations=app.lock().ok().map(|app|app.observation_source());
    #[cfg(not(feature="boundary-observation"))]
    let observations:Option<crate::observation::Source>=None;
    eprintln!("picomado websocket_open connection_id={} accept_order={} order_source={}", connection_id, accept_order, order_source);
    loop {
        let incoming = tokio::select! {
            result = &mut writer => {
                end_reason="writer_ended";
                end_detail=match result {Ok(reason)=>reason,Err(error)=>if error.is_panic(){"writer_panic"}else{"writer_task_failed"}};
                writer_finished = true;
                break;
            }
            incoming = async {
                #[cfg(feature="ack-path-probe")]
                {crate::ack_probe::next(&mut stream,accept_order.get()).await}
                #[cfg(not(feature="ack-path-probe"))]
                {stream.next().await}
            } => incoming,
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
        #[cfg(feature="ack-path-probe")]
        crate::ack_probe::record(crate::ack_probe::Event::DecodeBegin,accept_order.get(),rx_messages+1,0,0,0,bytes.len()as u64,0);
        #[cfg(feature="ack-path-probe")]
        if crate::ack_probe::active(accept_order.get()){decoder.probe_bind(accept_order.get(),rx_messages);}
        #[cfg(feature="boundary-observation")]
        let decoded=decoder.push_observed(&bytes);
        #[cfg(not(feature="boundary-observation"))]
        let decoded=decoder.push(&bytes);
        let messages = match decoded {
            Ok(messages) => messages,
            Err(_) => {
                end_reason = "decode_invalid_frame";
                let _ = enqueue_reply(&tx,WireMessage::Error {
                        code: "invalid_frame".into(),
                    },current.as_ref().map_or(0,|s|s.session_epoch)).await;
                break;
            }
        };
        let mut failed = false;
        for received in messages {
            #[cfg(feature="boundary-observation")]
            let (message,cause,invalid_cause)=(received.message,received.cause,received.invalid_cause);
            #[cfg(not(feature="boundary-observation"))]
            let (message,cause,invalid_cause)=(received,None,false);
            let cause=if matches!(message,WireMessage::Next{..}|WireMessage::Up{..}|WireMessage::Down{..}|WireMessage::Power{..}|WireMessage::EnterSub{..}){cause}else{None};
            if let Some(source)=&observations {
                source.record_context(crate::observation::Component::Api,crate::observation::Phase::Received,0,0,0,message.kind() as u64,0,cause);
                if invalid_cause{source.record(crate::observation::Component::Observation,crate::observation::Phase::Rejected,0,0,0,1);}
            }
            rx_messages += 1;
            let kind = wire_label(&message);
            let was_unbound = current.is_none();
            #[cfg(feature="ack-path-probe")]
            let probe_identity=crate::ack_probe::identity(&message);
            #[cfg(feature="ack-path-probe")]
            crate::ack_probe::record(crate::ack_probe::Event::ProcessBegin,accept_order.get(),rx_messages,probe_identity.0,probe_identity.1,probe_identity.2,0,0);
            #[cfg(feature="ack-path-probe")]
            let probe_process_started=crate::ack_probe::active(accept_order.get()).then(std::time::Instant::now);
            match process_with_observation(&app, &mut current, message, &mut diagnostics, Some(accept_order),cause) {
                Ok(reply) => {
                    #[cfg(feature="ack-path-probe")]
                    {if was_unbound {probe_bind_pending=true;}if probe_bind_pending {if let Some(token)=&current {probe_bind_pending=!crate::ack_probe::bind_accepted(&app,token,accept_order.get());}}}
                    #[cfg(feature="ack-path-probe")]
                    crate::ack_probe::record(crate::ack_probe::Event::ProcessDone,accept_order.get(),rx_messages,probe_identity.0,probe_identity.1,probe_identity.2,probe_process_started.as_ref().map_or(0,|started|started.elapsed().as_micros()as u64),1);
                    let binding=if was_unbound {current.clone()}else{None};
                    if let Some(reply) = reply {
                        let queued = std::time::Instant::now();
                        #[cfg(feature="ack-path-probe")]
                        crate::ack_probe::record(crate::ack_probe::Event::ReplyQueueBegin,accept_order.get(),rx_messages,probe_identity.0,probe_identity.1,probe_identity.2,tx.capacity()as u64,0);
                        if enqueue_reply_common(&tx,reply,current.as_ref().map_or(0,|s|s.session_epoch),cause,binding).await.is_err() {
                            end_reason = "writer_command_closed";
                            failed = true;
                            break;
                        }
                        #[cfg(feature="ack-path-probe")]
                        crate::ack_probe::record(crate::ack_probe::Event::ReplyQueueDone,accept_order.get(),rx_messages,probe_identity.0,probe_identity.1,probe_identity.2,queued.elapsed().as_micros()as u64,0);
                        if queued.elapsed() > Duration::from_millis(100) {
                            eprintln!("picomado reply_queue_wait connection_id={} request={} elapsed_ms={}",connection_id,kind,queued.elapsed().as_millis());
                        }
                        if was_unbound {
                            if let Some(token)=&current {
                                eprintln!("picomado websocket_bound connection_id={} id={} incarnation={} session_epoch={}",connection_id,token.device_id,token.incarnation,token.session_epoch);
                            }
                        }
                    }
                }
                Err(code) => {
                    #[cfg(feature="ack-path-probe")]
                    crate::ack_probe::record(crate::ack_probe::Event::ProcessDone,accept_order.get(),rx_messages,probe_identity.0,probe_identity.1,probe_identity.2,probe_process_started.as_ref().map_or(0,|started|started.elapsed().as_micros()as u64),0);
                    end_reason = "protocol_rejected";
                    end_detail = code;
                    let (id, incarnation, session_epoch) = session_identity(&current);
                    eprintln!("picomado protocol_rejected connection_id={} id={} incarnation={} session_epoch={} kind={} code={}",connection_id,id,incarnation,session_epoch,kind,code);
                    let _ = enqueue_reply(&tx,WireMessage::Error {
                            code: code.into(),
                        },current.as_ref().map_or(0,|s|s.session_epoch)).await;
                    failed = true;
                    break;
                }
            }
        }
        #[cfg(feature="ack-path-probe")]
        if crate::ack_probe::active(accept_order.get()){decoder.probe_bind(accept_order.get(),rx_messages);}
        if failed {
            break;
        }
    }
    #[cfg(feature="ack-path-probe")]
    crate::ack_probe::record(crate::ack_probe::Event::End,accept_order.get(),rx_messages,0,0,0,0,0);
    #[cfg(feature="ack-path-probe")]
    crate::ack_probe::unbind(accept_order.get());
    if !writer_finished {
        let _ = tx.send(WriterCommand::Stop).await;
        let _ = writer.await;
    }
    let (id, incarnation, session_epoch) = session_identity(&current);
    eprintln!("picomado session_end connection_id={} id={} incarnation={} session_epoch={} trigger={} detail={} elapsed_ms={} idle_ms={} rx_frames={} rx_messages={} rx_bytes={} ignored_acks={}",connection_id,id,incarnation,session_epoch,end_reason,end_detail,started.elapsed().as_millis(),last_rx.elapsed().as_millis(),rx_frames,rx_messages,rx_bytes,diagnostics.ignored);
    if let Some(token) = current {
        if let Ok(mut guard) = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer) {
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

    #[tokio::test]
    async fn real_websocket_lcd_video_continues_without_any_frame_ack() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_app = app.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, web::router(server_app))
                .await
                .unwrap()
        });
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/device"))
            .await
            .unwrap();
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Hello {
                device_id: "push".into(),
                screen_owner_epoch: 1,
                capabilities: vec![
                    "lcd".into(),
                    crate::protocol::FRAME_BYTES_CAPABILITY.into(),
                    crate::protocol::LCD_PUSH_CAPABILITY.into(),
                ],
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let session = match read_wire(&mut ws).await {
            WireMessage::Welcome {
                incarnation,
                session_epoch,
            } => SessionToken {
                device_id: "push".into(),
                incarnation,
                session_epoch,
            },
            m => panic!("{m:?}"),
        };
        assert!(matches!(
            read_wire(&mut ws).await,
            WireMessage::Settings { .. }
        ));
        let before = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().pending_effects().len();
        let mut last = 0;
        for sequence in 1..=6 {
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .queue_lcd_video(&session, 6, sequence, 1, vec![sequence as u8; 25600])
                .unwrap();
            let mut assembler = FrameAssembler::default();
            let (token, pixels) = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let message = read_wire(&mut ws).await;
                    if matches!(
                        message,
                        WireMessage::FrameStart {
                            stream: Some(_),
                            ..
                        } | WireMessage::FrameChunk { .. }
                    ) {
                        if let Some(frame) = assembler.accept(message).unwrap() {
                            break frame;
                        }
                    }
                }
            })
            .await
            .unwrap();
            assert!(token.frame_id > last);
            last = token.frame_id;
            // The composition adds configured overlays; transport must preserve
            // the complete immutable frame, whose CRC FrameAssembler checked.
            assert_eq!(pixels.len(), 25600);
        }
        assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().pending_effects().len() <= before);
        ws.send(ClientMessage::Binary(
            encode(&WireMessage::Next {
                incarnation: session.incarnation,
                session_epoch: session.session_epoch,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        loop {
            if matches!(read_wire(&mut ws).await, WireMessage::InputAccepted { .. }) {
                break;
            }
        }
        ws.close(None).await.unwrap();
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn lcd_and_eink_dispatch_use_independent_dimensions_format_crc_and_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
        ));
        for (id, cap, size, dimensions, format) in [
            ("lcd", "lcd", 25600, (160, 80), "rgb565be"),
            ("E-ink_0", "eink", 3813, (250, 122), "mono1"),
        ] {
            let token = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .hello(id, [cap.into()].into(), 1)
                .unwrap();
            let frame = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .queue_lcd_frame(&token, "youtube", vec![255; 25600])
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
            let mut assembler = FrameAssembler::default();
            let mut complete = None;
            while let Some(packet) = queue.pop_for(id) {
                let message = Decoder::default().push(&packet.bytes).unwrap().remove(0);
                if let WireMessage::FrameStart {
                    width,
                    height,
                    format: actual,
                    byte_len,
                    ..
                } = &message
                {
                    assert_eq!((*width, *height), dimensions);
                    assert_eq!(actual, format);
                    assert_eq!(*byte_len as usize, size);
                }
                if matches!(
                    message,
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                ) {
                    if let Some(result) = assembler.accept(message).unwrap() {
                        complete = Some(result);
                    }
                }
            }
            let (received, pixels) = complete.unwrap();
            assert_eq!(received, frame);
            assert_eq!(pixels, vec![255; size]);
        }
    }

    #[tokio::test]
    async fn http_settings_push_weather_and_logo_to_lcd_and_eink_without_reconnect() {
        for cap in ["lcd", "eink"] {
            let temp = tempfile::tempdir().unwrap();
            let app = Arc::new(std::sync::Mutex::new(
                App::open_with_clock(&temp.path().join("state.db"), Arc::new(FixtureClock))
                    .unwrap(),
            ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let web_app = app.clone();
            let server =
                tokio::spawn(
                    async move { axum::serve(listener, web::router(web_app)).await.unwrap() },
                );
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/device"))
                .await
                .unwrap();
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec![cap.into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            let session = match read_wire(&mut ws).await {
                WireMessage::Welcome {
                    incarnation,
                    session_epoch,
                } => SessionToken {
                    device_id: "desk".into(),
                    incarnation,
                    session_epoch,
                },
                other => panic!("{other:?}"),
            };
            let mut assembler = FrameAssembler::default();
            let first = loop {
                let message = read_wire(&mut ws).await;
                if matches!(
                    message,
                    WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                ) {
                    if let Some((token, _)) = assembler.accept(message).unwrap() {
                        break token;
                    }
                }
            };
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .publish_plugin_source(
                    "weather",
                    &session,
                    1,
                    include_bytes!("../assets/jma-tokyo-20261012.json"),
                    "cached",
                    "2026-10-12",
                )
                .unwrap();
            let client = reqwest::Client::new();
            for plugin in ["caricature", "rain-radar", "clock", "youtube", "weather"] {
                let d = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices.remove(0);
                let response=client.put(format!("http://{address}/api/devices/desk/plugins/{plugin}"))
                    .header("Content-Type","application/json").body(serde_json::to_vec(&serde_json::json!({"incarnation":d.incarnation,"expected_revision":d.enabled_revision,"enabled":false})).unwrap()).send().await.unwrap();
                assert!(response.status().is_success());
                let view: serde_json::Value =
                    serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
                if plugin == "caricature" || plugin == "rain-radar" || plugin == "clock" {
                    assert_eq!(view["devices"][0]["selected"], "youtube");
                    continue;
                }
                let (frame, pixels) = loop {
                    let message = read_wire(&mut ws).await;
                    if let WireMessage::FrameStart { mode, .. } = &message {
                        assert_eq!(mode.as_deref(), Some("normal"));
                    }
                    if matches!(
                        message,
                        WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                    ) {
                        if let Some(result) = assembler.accept(message).unwrap() {
                            break result;
                        }
                    }
                };
                assert!(frame.display_epoch > first.display_epoch);
                let expected = if plugin == "youtube" {
                    let set = ForecastSet::tokyo_from_cache(
                        include_bytes!("../assets/jma-tokyo-20261012.json"),
                        "cached",
                        "2026-10-12",
                    )
                    .unwrap();
                    if cap == "lcd" {
                        crate::weather::render_lcd(&set, &set.days[0]).unwrap()
                    } else {
                        crate::weather::render_eink(&set, &set.days[0]).unwrap()
                    }
                } else {
                    let logo = crate::lcd_text::logo_frame();
                    if cap == "lcd" {
                        logo
                    } else {
                        crate::eink::from_lcd(&logo)
                    }
                };
                assert_eq!(pixels, expected);
                for message in [
                    WireMessage::ReceiptAck {
                        token: frame.clone(),
                    },
                    WireMessage::RenderedAck {
                        token: frame.clone(),
                    },
                ] {
                    ws.send(ClientMessage::Binary(encode(&message).unwrap().into()))
                        .await
                        .unwrap();
                }
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].last_rendered_frame
                            == Some(frame.frame_id)
                        {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().session_is_current(&session));
            }
            assert_eq!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].selected, None);
            ws.close(None).await.unwrap();
            server.abort();
        }
    }

    #[test]
    fn input_context_tracks_current_mode_generation_and_keeps_legacy_wire_compatible() {
        for cap in ["lcd", "eink"] {
            let temp = tempfile::tempdir().unwrap();
            let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
                &temp.path().join("state.db"),
            )));
            let mut current = None;
            let mut diagnostics = AckDiagnostics::default();
            process(
                &app,
                &mut current,
                WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec![cap.into()],
                    screen_owner_epoch: 1,
                },
                &mut diagnostics,
            )
            .unwrap();
            let token = current.clone().unwrap();
            {
                let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
                guard
                    .publish_plugin_source(
                        "weather",
                        &token,
                        1,
                        include_bytes!("../assets/jma-tokyo-20261012.json"),
                        "cached",
                        "2026-10-12",
                    )
                    .unwrap();
                for plugin in ["caricature", "rain-radar", "youtube"] {
                    let d = guard.view().devices.remove(0);
                    guard
                        .set_enabled("desk", d.incarnation, d.enabled_revision, plugin, false)
                        .unwrap();
                }
            }
            for (message, expected_mode) in [
                (
                    WireMessage::EnterSub {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    "sub",
                ),
                (
                    WireMessage::Down {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    "sub",
                ),
                (
                    WireMessage::Power {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    "normal",
                ),
                (
                    WireMessage::Down {
                        incarnation: token.incarnation,
                        session_epoch: token.session_epoch,
                    },
                    "normal",
                ),
            ] {
                let reply = process(&app, &mut current, message, &mut diagnostics)
                    .unwrap()
                    .unwrap();
                let context = match &reply {
                    WireMessage::InputAccepted {
                        input_context: Some(context),
                        display_epoch,
                        ..
                    } => {
                        assert_eq!(*display_epoch, context.display_epoch);
                        context
                    }
                    WireMessage::PluginPosition {
                        input_context: Some(context),
                        ..
                    } => context,
                    other => panic!("missing input generation: {other:?}"),
                };
                assert_eq!(context.incarnation, token.incarnation);
                assert_eq!(context.session_epoch, token.session_epoch);
                assert_eq!(
                    context.display_epoch,
                    crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().selection_for(&token).unwrap().1
                );
                assert_eq!(context.mode, expected_mode);
                assert_eq!(
                    Decoder::default().push(&encode(&reply).unwrap()).unwrap(),
                    vec![reply]
                );
            }
        }
        // Old server replies omit the additive context and still decode.
        for reply in [
            WireMessage::InputAccepted {
                input_context: None,
                selected: Some("weather".into()),
                display_epoch: 5,
            },
            WireMessage::PluginPosition {
                input_context: None,
                plugin: "weather".into(),
                dataset_id: "fixture".into(),
                target_date: "2026-10-14".into(),
                index: 0,
            },
        ] {
            let encoded = encode(&reply).unwrap();
            assert!(!String::from_utf8_lossy(&encoded).contains("input_context"));
            assert_eq!(Decoder::default().push(&encoded).unwrap(), vec![reply]);
        }
    }

    #[tokio::test]
    async fn binary_negotiation_delivers_lcd_and_eink_pixels_and_both_acks() {
        for cap in ["lcd", "eink"] {
            let temp = tempfile::tempdir().unwrap();
            let app = Arc::new(std::sync::Mutex::new(
                App::open_with_clock(&temp.path().join("state.db"), Arc::new(FixtureClock))
                    .unwrap(),
            ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server_app = app.clone();
            let server = tokio::spawn(async move {
                axum::serve(listener, web::router(server_app))
                    .await
                    .unwrap()
            });
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/device"))
                .await
                .unwrap();
            ws.send(ClientMessage::Binary(
                encode(&WireMessage::Hello {
                    device_id: "desk".into(),
                    capabilities: vec![cap.into(), crate::protocol::FRAME_BYTES_CAPABILITY.into()],
                    screen_owner_epoch: 1,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
            let mut decoder = Decoder::default();
            let mut assembler = FrameAssembler::default();
            let mut native = 0;
            let (frame, pixels) = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let ClientMessage::Binary(bytes) = ws.next().await.unwrap().unwrap() else {
                        continue;
                    };
                    if bytes[5] == crate::protocol::FRAME_BYTES_KIND {
                        native += 1;
                    }
                    for message in decoder.push(&bytes).unwrap() {
                        if matches!(
                            message,
                            WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                        ) {
                            if let Some(frame) = assembler.accept(message).unwrap() {
                                return frame;
                            }
                        }
                    }
                }
            })
            .await
            .unwrap();
            assert!(native > 0);
            assert_eq!(pixels.len(), if cap == "lcd" { 25600 } else { 3813 });
            for message in [
                WireMessage::ReceiptAck {
                    token: frame.clone(),
                },
                WireMessage::RenderedAck {
                    token: frame.clone(),
                },
            ] {
                ws.send(ClientMessage::Binary(encode(&message).unwrap().into()))
                    .await
                    .unwrap();
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].last_rendered_frame
                    != Some(frame.frame_id)
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            ws.close(None).await.unwrap();
            server.abort();
        }
    }

    #[tokio::test]
    async fn lost_render_ack_is_bounded_while_control_and_heartbeat_still_run() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = app_at_fixture_date(&temp.path().join("state.db"));
        let session = state.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let frame = state
            .queue_lcd_frame(&session, "youtube", vec![0; 25600])
            .unwrap();
        let app = Arc::new(std::sync::Mutex::new(state));
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let worker = tokio::spawn(writer_loop_with_ack_timeout(
            MockSink {
                count: count.clone(),
                limit: usize::MAX,
                recorded: Some(recorded.clone()),
            },
            rx,
            app.clone(),
            Some(Duration::from_millis(80)),
        ));
        tx.send(WriterCommand::Bind(session.clone())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while count.load(Ordering::SeqCst) < 8 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        {
            let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
            guard.heartbeat(&session, 1).unwrap();
            assert!(guard
                .terminal_event(Event::ReceiptAck {
                    token: frame.clone()
                })
                .unwrap());
            assert!(guard.frame_waiting_for_render(&session, frame.frame_id));
        }
        tx.send(WriterCommand::reply(WireMessage::InputAccepted {
            input_context: None,
            selected: Some("youtube".into()),
            display_epoch: frame.display_epoch,
        }))
        .await
        .unwrap();
        let reason = tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reason, "render_ack_timeout");
        assert!(recorded.lock().unwrap().iter().any(|m|matches!(m,Message::Binary(bytes) if matches!(Decoder::default().push(bytes).ok().and_then(|v|v.into_iter().next()),Some(WireMessage::InputAccepted{..})))));
        let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
        guard.button(&session, ButtonAction::Next).unwrap();
        assert!(!guard.frame_waiting_for_render(&session, frame.frame_id));
        assert!(!guard
            .terminal_event(Event::RenderedAck { token: frame })
            .unwrap());
    }

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
        crate::test_support::manual_frames_app_with_clock(path, Arc::new(FixtureClock)).unwrap()
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bound_welcome_precedes_outbox_without_a_second_producer_command() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = crate::test_support::manual_frames_app(&temp.path().join("db")).unwrap();
        let token = state.hello("welcome", ["lcd".into()].into(), 1).unwrap();
        let app = Arc::new(std::sync::Mutex::new(state));
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let writer = tokio::spawn(writer_loop(
            MockSink {
                count: count.clone(),
                limit: usize::MAX,
                recorded: Some(recorded.clone()),
            },
            rx,
            app,
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
        // No second producer command can supply a missing Welcome. The
        // writer must send it, then discover Settings in the bound outbox.
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let messages: Vec<_> = recorded
            .lock()
            .unwrap()
            .iter()
            .flat_map(|message| match message {
                Message::Binary(bytes) => Decoder::default().push(bytes).unwrap(),
                _ => Vec::new(),
            })
            .collect();
        assert!(matches!(messages.first(), Some(WireMessage::Welcome { incarnation, session_epoch })
            if *incarnation == token.incarnation && *session_epoch == token.session_epoch));
        assert!(matches!(messages.get(1), Some(WireMessage::Settings { .. })));
        tx.send(WriterCommand::Stop).await.unwrap();
        assert_eq!(writer.await.unwrap(), "requested_stop");
    }

    #[tokio::test]
    async fn non_reading_writer_times_out_without_delaying_fast_peer_input() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
        ));
        let (slow, fast) = {
            let mut a = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
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
            .send(WriterCommand::reply(WireMessage::Welcome {
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
            .send(WriterCommand::reply(WireMessage::Welcome {
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
        let result = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            .send(WriterCommand::reply(WireMessage::InputAccepted {
                input_context: None,
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
                crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
            ));
            let token = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .hello("desk", ["lcd".into()].into(), 1)
                .unwrap();
            if scenario == "cancel" {
                assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                    .unwrap()
                    .terminal_event(Event::Next {
                        token: token.clone()
                    })
                    .unwrap());
                assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                    .unwrap()
                    .terminal_event(Event::Next {
                        token: token.clone()
                    })
                    .unwrap());
                assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            let frame = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                    assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                        .unwrap()
                        .terminal_event(Event::Next {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.display_epoch += 1;
                }
                "cancel" => {
                    assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                        .unwrap()
                        .terminal_event(Event::Power {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.display_epoch += 1;
                }
                "standby" => {
                    assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                        .unwrap()
                        .terminal_event(Event::Power {
                            token: token.clone()
                        })
                        .unwrap());
                    terminal.screen.take_local(crate::LocalScreen::Standby);
                }
                "reconnect" => {
                    let newer = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .terminal_event(Event::ReceiptAck {
                    token: frame.clone()
                })
                .unwrap());
            assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .terminal_event(Event::RenderedAck { token: frame })
                .unwrap());
        }
    }

    #[tokio::test]
    async fn blocked_terminal_still_processes_input_while_other_terminal_gets_two_frames() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
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
                    let guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
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
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .queue_lcd_frame(&slow, "youtube", vec![1; crate::protocol::LCD_FRAME_BYTES])
            .unwrap();
        let first_fast = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                if crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .terminal_event(Event::RenderedAck { token: first_fast })
            .unwrap());
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .set_order(
                1,
                ["weather", "rain-radar", "youtube", "caricature", "clock"]
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
                let device = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices.into_iter().next();
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
            let mut guard = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap();
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
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].online);
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
                let view = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view();
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
        let issued = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].last_rendered_frame,
            None
        );
        assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().frame_is_current(&issued));

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
        let mut future = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].online);
        assert_eq!(
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].selected.as_deref(),
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
    async fn video_blocked_writer_finishes_started_frame_then_latest_and_does_not_block_peer() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let caps = std::collections::BTreeSet::from([
            "lcd".into(),
            crate::protocol::FRAME_BYTES_CAPABILITY.into(),
            crate::protocol::LCD_PUSH_CAPABILITY.into(),
        ]);
        let slow = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .hello("slow-push", caps.clone(), 1)
            .unwrap();
        let fast = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().hello("fast-push", caps, 1).unwrap();
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
                at: 2,
                open: open.clone(),
                wake: wake.clone(),
            },
            rx,
            app.clone(),
        ));
        tx.send(WriterCommand::Bind(slow.clone())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while count.load(Ordering::SeqCst) < 1 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .queue_lcd_video(&slow, 30, 1, 1, vec![1; 25600])
            .unwrap();
        // Settings plus FrameStart have been sent. First chunk is blocked.
        tokio::time::timeout(Duration::from_secs(3), async {
            while count.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        for n in 2..=100 {
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .queue_lcd_video(&slow, 30, n, 1, vec![n as u8; 25600])
                .unwrap();
        }
        let fast_recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let fast_count = Arc::new(AtomicUsize::new(0));
        let (fast_tx, fast_rx) = tokio::sync::mpsc::channel(32);
        let fast_task = tokio::spawn(writer_loop(
            MockSink {
                count: fast_count.clone(),
                limit: usize::MAX,
                recorded: Some(fast_recorded.clone()),
            },
            fast_rx,
            app.clone(),
        ));
        fast_tx
            .send(WriterCommand::Bind(fast.clone()))
            .await
            .unwrap();
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .queue_lcd_video(&fast, 6, 1, 1, vec![7; 25600])
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while fast_count.load(Ordering::SeqCst) < 9 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        tx.send(WriterCommand::reply(WireMessage::InputAccepted {
            input_context: None,
            selected: None,
            display_epoch: 1,
        }))
        .await
        .unwrap();
        open.store(true, Ordering::SeqCst);
        wake.wake();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 18 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        tx.send(WriterCommand::Stop).await.unwrap();
        fast_tx.send(WriterCommand::Stop).await.unwrap();
        assert_eq!(task.await.unwrap(), "requested_stop");
        assert_eq!(fast_task.await.unwrap(), "requested_stop");
        let mut assembler = FrameAssembler::default();
        let mut sequences = Vec::new();
        let mut completed = 0;
        let mut replies = 0;
        for packet in recorded.lock().unwrap().iter() {
            if let Message::Binary(bytes) = packet {
                for message in Decoder::default().push(bytes).unwrap() {
                    if let WireMessage::FrameStart {
                        stream: Some(ref metadata),
                        ..
                    } = message
                    {
                        sequences.push(metadata.sequence);
                    }
                    if matches!(message, WireMessage::InputAccepted { .. }) {
                        replies += 1;
                    }
                    if matches!(
                        message,
                        WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
                    ) && assembler.accept(message).unwrap().is_some()
                    {
                        completed += 1;
                    }
                }
            }
        }
        assert_eq!(sequences, vec![1, 100]);
        assert_eq!(completed, 2);
        assert_eq!(replies, 1);
    }

    #[tokio::test]
    async fn burst_switch_inputs_keep_ordered_responses_while_old_image_is_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(std::sync::Mutex::new(app_at_fixture_date(
            &temp.path().join("state.db"),
        )));
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let open = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(futures_util::task::AtomicWaker::new());
        let (input, mut receiver) = tokio::sync::mpsc::channel(16);
        let task = tokio::spawn(serve_split(
            GateSink {
                inner: MockSink {
                    count: count.clone(),
                    limit: usize::MAX,
                    recorded: Some(recorded.clone()),
                },
                at: 3,
                open: open.clone(),
                wake: wake.clone(),
            },
            futures_util::stream::poll_fn(move |cx| receiver.poll_recv(cx)),
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
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let session = SessionToken {
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 1,
        };
        let old = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .queue_lcd_frame(
                &session,
                "youtube",
                vec![1; crate::protocol::LCD_FRAME_BYTES],
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while count.load(Ordering::SeqCst) < 3 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        let epoch = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().selection_for(&session).unwrap().1;
        let mut bytes = Vec::new();
        for _ in 0..12 {
            bytes.extend(
                encode(&WireMessage::Next {
                    incarnation: 1,
                    session_epoch: 1,
                })
                .unwrap(),
            );
        }
        input.send(Ok(Message::Binary(bytes.into()))).await.unwrap();
        tokio::time::timeout(Duration::from_millis(500), async {
            while crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().selection_for(&session).unwrap().1 != epoch + 12 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().frame_is_current(&old));
        // Input transitions finish before the blocked writer can send replies.
        assert_eq!(count.load(Ordering::SeqCst), 3);
        open.store(true, Ordering::SeqCst);
        wake.wake();
        let responses = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let responses: Vec<_> = recorded
                    .lock()
                    .unwrap()
                    .iter()
                    .filter_map(|message| {
                        let Message::Binary(bytes) = message else {
                            return None;
                        };
                        match Decoder::default().push(bytes).unwrap().remove(0) {
                            WireMessage::InputAccepted {
                                selected,
                                display_epoch,
                                ..
                            } => Some((selected, display_epoch)),
                            WireMessage::Error { code } => panic!("unexpected error: {code}"),
                            _ => None,
                        }
                    })
                    .collect();
                if responses.len() == 12 {
                    break responses;
                }
                assert!(!task.is_finished());
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        for (index, (selected, display_epoch)) in responses.iter().enumerate() {
            assert_eq!(
                selected.as_deref(),
                Some(["caricature", "weather", "rain-radar", "clock", "youtube"][index % 5])
            );
            assert_eq!(*display_epoch, epoch + index as u64 + 1);
        }
        let mut assembler = FrameAssembler::default();
        for message in recorded.lock().unwrap().iter() {
            let Message::Binary(bytes) = message else {
                continue;
            };
            let message = Decoder::default().push(bytes).unwrap().remove(0);
            if matches!(
                message,
                WireMessage::FrameStart { .. } | WireMessage::FrameChunk { .. }
            ) {
                assert!(
                    assembler.accept(message).unwrap().is_none(),
                    "old image completed after switch"
                );
            }
        }
        let mut bytes = encode(&WireMessage::ReceiptAck { token: old.clone() }).unwrap();
        bytes.extend(encode(&WireMessage::RenderedAck { token: old }).unwrap());
        bytes.extend(
            encode(&WireMessage::Next {
                incarnation: 1,
                session_epoch: 1,
            })
            .unwrap(),
        );
        input.send(Ok(Message::Binary(bytes.into()))).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().selection_for(&session).unwrap().1 != epoch + 13 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().session_is_current(&session));
        assert_eq!(
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].last_rendered_frame,
            None
        );
        drop(input);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
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
        tx.send(WriterCommand::reply(WireMessage::Welcome {
            incarnation: 1,
            session_epoch: 1,
        }))
        .await
        .unwrap();
        let old = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        } = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .button(&session, ButtonAction::Next)
            .unwrap()
        else {
            panic!("selection expected");
        };
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().frame_is_current(&old));
        tx.send(WriterCommand::reply(WireMessage::InputAccepted {
            input_context: None,
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
        let new = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        let old = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                let view = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view();
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
        let new = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            while crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].last_rendered_frame != Some(new.frame_id) {
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
                let view = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view();
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
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
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
                if crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].stored_revision == snapshot.revision {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].pending);
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
                if !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].pending {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].pending);
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
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .terminal_event(Event::Next { token: old })
            .unwrap());
        assert_eq!(
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().view().devices[0].selected.as_deref(),
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
        let slow_frame = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .queue_lcd_frame(
                &tokens[0],
                "weather",
                crate::weather::render_lcd(&set, &set.days[0]).unwrap(),
            )
            .unwrap();
        let fast_frame = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                let device = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                let device = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
                .unwrap()
                .view()
                .devices
                .iter()
                .find(|d| d.id == "slow")
                .unwrap()
                .last_rendered_frame,
            None
        );
        let stale = crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
                if !crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer).unwrap().session_is_current(&tokens[1]) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        let mut expected = crate::weather::render_lcd(&set, &set.days[0]).unwrap();
        crate::display_badge::draw_sub_rgb565be(&mut expected);
        assert_eq!(first.unwrap().1, expected);
        let mut updated = set.clone();
        updated.id = "new".into();
        updated.days[0].max_c = Some(28);
        let mut revised_source: serde_json::Value = serde_json::from_slice(source).unwrap();
        revised_source[0]["timeSeries"][2]["areas"][0]["temps"][0] = serde_json::json!("28");
        let revised_bytes = serde_json::to_vec(&revised_source).unwrap();
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
            crate::test_support::manual_frames_app(&temp.path().join("state.db")).unwrap(),
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
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
            .unwrap()
            .publish_plugin_source("rain-radar", &token, 1, source, "set-two", "2026-10-12")
            .unwrap();
        assert_eq!(
            crate::diagnostics::lock(&app, crate::diagnostics::Domain::Writer)
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
        let mut normal_pixels = frame.unwrap();
        assert_ne!(normal_pixels, latest, "SUB badge must disappear on exit");
        crate::display_badge::draw_sub_rgb565be(&mut normal_pixels);
        assert_eq!(normal_pixels, latest);
        drop(ws);
        server.abort();
        let _ = server.await;
    }
    // Inserted only into an isolated source snapshot inside the existing test
    // module, so these tests call the real private writer and existing fixtures.
    static DIAGNOSTIC_FIXTURE: std::sync::Mutex<()>=std::sync::Mutex::new(());
    struct AuditProgressSink {
        recorded: Arc<std::sync::Mutex<Vec<Message>>>,
        count: Arc<AtomicUsize>,
        progress: Arc<AtomicUsize>,
        wait: Option<Pin<Box<tokio::time::Sleep>>>,
        began: Option<tokio::time::Instant>,
        hold: Duration,
        per_packet: bool,
    }
    impl Sink<Message> for AuditProgressSink {
        type Error=();
        fn poll_ready(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
        fn start_send(mut self:Pin<&mut Self>,m:Message)->Result<(),()>{
            self.count.fetch_add(1,Ordering::SeqCst);
            self.recorded.lock().unwrap().push(m);
            if self.per_packet {self.began=None;self.wait=None;}
            Ok(())
        }
        fn poll_flush(mut self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<Result<(),()>>{
            use std::future::Future;
            let gate=if self.per_packet {2}else{3};
            if self.count.load(Ordering::SeqCst)<gate{return Poll::Ready(Ok(()));}
            let start=*self.began.get_or_insert_with(tokio::time::Instant::now);
            if start.elapsed()>=self.hold{return Poll::Ready(Ok(()));}
            if self.wait.is_none(){self.wait=Some(Box::pin(tokio::time::sleep(Duration::from_millis(100))));}
            if self.wait.as_mut().unwrap().as_mut().poll(cx).is_ready(){
                self.progress.fetch_add(512,Ordering::SeqCst);
                self.wait=Some(Box::pin(tokio::time::sleep(Duration::from_millis(100))));
                let _=self.wait.as_mut().unwrap().as_mut().poll(cx);
            }
            Poll::Pending
        }
        fn poll_close(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
    }
    async fn audit_writer(hold:Duration,per_packet:bool,with_reply:bool)->(&'static str,Duration,usize,Option<Duration>){
        crate::diagnostics::start_from_env();
        let tmp=tempfile::tempdir().unwrap();
        let app=Arc::new(std::sync::Mutex::new(app_at_fixture_date(&tmp.path().join("audit.db"))));
        let caps=["lcd".into(),crate::protocol::FRAME_BYTES_CAPABILITY.into(),crate::protocol::LCD_PUSH_CAPABILITY.into()].into();
        let token=app.lock().unwrap().hello("cache-audit",caps,1).unwrap();
        let recorded=Arc::new(std::sync::Mutex::new(Vec::new()));
        let count=Arc::new(AtomicUsize::new(0));let progress=Arc::new(AtomicUsize::new(0));
        let (tx,rx)=tokio::sync::mpsc::channel(32);
        let task=tokio::spawn(writer_loop(AuditProgressSink{recorded:recorded.clone(),count:count.clone(),progress:progress.clone(),wait:None,began:None,hold,per_packet},rx,app.clone()));
        tx.send(WriterCommand::Bind(token.clone())).await.unwrap();
        while count.load(Ordering::SeqCst)<1 {tokio::time::sleep(Duration::from_millis(1)).await;}
        let started=std::time::Instant::now();
        app.lock().unwrap().queue_lcd_video(&token,12,1,1,vec![0x37;25600]).unwrap();
        let mut reply_delay=None;
        if with_reply {
            while count.load(Ordering::SeqCst)<3 {tokio::time::sleep(Duration::from_millis(1)).await;}
            let queued=std::time::Instant::now();
            enqueue_reply(&tx,WireMessage::InputAccepted{input_context:None,selected:None,display_epoch:1},token.session_epoch).await.unwrap();
            loop {
                let found=recorded.lock().unwrap().iter().any(|m|match m{
                    Message::Binary(b)=>Decoder::default().push(b).unwrap().iter().any(|w|matches!(w,WireMessage::InputAccepted{..})),_=>false});
                if found{reply_delay=Some(queued.elapsed());break;}
                assert!(queued.elapsed()<Duration::from_secs(2));
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            tx.send(WriterCommand::Stop).await.unwrap();
        }
        let reason=tokio::time::timeout(Duration::from_secs(13),task).await.unwrap().unwrap();
        let elapsed=started.elapsed();
        if std::env::var_os("PICOMADO_DIAGNOSTICS_PATH").is_some(){tokio::time::sleep(Duration::from_millis(1100)).await;}
        (reason,elapsed,progress.load(Ordering::SeqCst),reply_delay)
    }
    #[tokio::test]
    async fn cache_audit_untracked_sink_activity_does_not_reset_idle_deadline(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();
        let (why,elapsed,bytes,_)=audit_writer(Duration::from_secs(4),false,false).await;
        assert_eq!(why,"stream_write_stall");assert!(bytes>=4096);
        #[cfg(feature="stream-diagnostics")]
        if std::env::var_os("PICOMADO_DIAGNOSTICS_PATH").is_some(){assert!(crate::diagnostics::test_count(crate::diagnostics::Kind::FragmentDeadline)>0);}
        assert!(elapsed>=Duration::from_millis(2900)&&elapsed<Duration::from_millis(3500));
        println!("AUDIT fragment: elapsed_ms={} simulated_partial_bytes={} reason={why}",elapsed.as_millis(),bytes);
    }
    #[tokio::test]
    async fn cache_audit_control_reply_waits_for_inflight_fragment(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();
        #[cfg(feature="stream-diagnostics")] crate::diagnostics::test_enable();
        let (why,_,bytes,delay)=audit_writer(Duration::from_millis(150),false,true).await;
        #[cfg(feature="stream-diagnostics")]
        {let (n,max)=crate::diagnostics::test_hist(crate::diagnostics::Kind::ReplyQueue);assert!(n>=1&&max>=100000);println!("R3 queue residence max_us={max} actual_reply_ms={}",delay.unwrap().as_millis());}
        assert_eq!(why,"requested_stop");assert!(bytes>0);
        let delay=delay.unwrap();assert!(delay>=Duration::from_millis(100));
        println!("AUDIT reply: queued_ms={} successful_after_fragment=true",delay.as_millis());
    }
    #[tokio::test]
    async fn cache_audit_total_frame_deadline_even_when_fragments_finish_under_idle_limit(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();
        let (why,elapsed,bytes,_)=audit_writer(Duration::from_millis(1400),true,false).await;
        assert_eq!(why,"stream_write_stall");assert!(bytes>0);
        #[cfg(feature="stream-diagnostics")]
        if std::env::var_os("PICOMADO_DIAGNOSTICS_PATH").is_some(){assert!(crate::diagnostics::test_count(crate::diagnostics::Kind::FrameDeadline)>0);}
        assert!(elapsed>=Duration::from_millis(9800)&&elapsed<Duration::from_secs(11));
        println!("AUDIT frame: elapsed_ms={} per_packet_delay_ms=1400 simulated_partial_bytes={bytes}",elapsed.as_millis());
    }

    struct AuditErrorSink {count:usize}
    impl Sink<Message> for AuditErrorSink {
        type Error=();
        fn poll_ready(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
        fn start_send(mut self:Pin<&mut Self>,_:Message)->Result<(),()>{self.count+=1;if self.count==3{Err(())}else{Ok(())}}
        fn poll_flush(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
        fn poll_close(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
    }
    #[tokio::test]
    async fn cache_audit_sink_error_distinct_from_deadline(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();
        crate::diagnostics::start_from_env();
        let tmp=tempfile::tempdir().unwrap();
        let app=Arc::new(std::sync::Mutex::new(app_at_fixture_date(&tmp.path().join("err.db"))));
        let caps=["lcd".into(),crate::protocol::FRAME_BYTES_CAPABILITY.into(),crate::protocol::LCD_PUSH_CAPABILITY.into()].into();
        let token=app.lock().unwrap().hello("cache-error",caps,1).unwrap();
        let (tx,rx)=tokio::sync::mpsc::channel(32);
        app.lock().unwrap().queue_lcd_video(&token,12,1,1,vec![0x37;25600]).unwrap();
        tx.send(WriterCommand::Bind(token)).await.unwrap();
        let why=writer_loop(AuditErrorSink{count:0},rx,app).await;
        assert_eq!(why,"stream_write_stall");
        #[cfg(feature="stream-diagnostics")]
        if std::env::var_os("PICOMADO_DIAGNOSTICS_PATH").is_some(){assert!(crate::diagnostics::test_count(crate::diagnostics::Kind::SendError)>0);}
    }

    #[cfg(feature="stream-diagnostics")]
    struct ReviewLatePollSink { count: usize, blocked: bool }
    #[cfg(feature="stream-diagnostics")]
    impl Sink<Message> for ReviewLatePollSink {
        type Error=();
        fn poll_ready(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
        fn start_send(mut self:Pin<&mut Self>,_:Message)->Result<(),()>{self.count+=1;Ok(())}
        fn poll_flush(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{
            if self.count<3{return Poll::Ready(Ok(()));}
            if !self.blocked {self.blocked=true;std::thread::sleep(Duration::from_millis(5200));}
            Poll::Pending
        }
        fn poll_close(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
    }
    #[cfg(feature="stream-diagnostics")]
    #[tokio::test]
    async fn review_regression_late_poll_deadline_classification(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();crate::diagnostics::test_enable();
        crate::diagnostics::start_from_env();
        let tmp=tempfile::tempdir().unwrap();
        let app=Arc::new(std::sync::Mutex::new(app_at_fixture_date(&tmp.path().join("late.db"))));
        let caps=["lcd".into(),crate::protocol::FRAME_BYTES_CAPABILITY.into(),crate::protocol::LCD_PUSH_CAPABILITY.into()].into();
        let token=app.lock().unwrap().hello("late-poll",caps,1).unwrap();
        app.lock().unwrap().queue_lcd_video(&token,12,1,1,vec![0;25600]).unwrap();
        let (tx,rx)=tokio::sync::mpsc::channel(32);tx.send(WriterCommand::Bind(token)).await.unwrap();
        let before_fragment=crate::diagnostics::test_count(crate::diagnostics::Kind::FragmentDeadline);
        let before_frame=crate::diagnostics::test_count(crate::diagnostics::Kind::FrameDeadline);
        let why=writer_loop(ReviewLatePollSink{count:0,blocked:false},rx,app).await;
        let fragment=crate::diagnostics::test_count(crate::diagnostics::Kind::FragmentDeadline)-before_fragment;
        let frame=crate::diagnostics::test_count(crate::diagnostics::Kind::FrameDeadline)-before_frame;
        assert_eq!(why,"stream_write_stall");
        println!("REVIEW actual chosen idle budget=3s first video chunk, observed late at 5.2s: fragment_count={fragment} frame_count={frame}");
        assert_eq!((fragment,frame),(1,0));
        let (_,late)=crate::diagnostics::test_hist(crate::diagnostics::Kind::DeadlineWake);assert!(late>=2000000);println!("R2 deadline wake late_us={late}");
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }
    #[cfg(feature="stream-diagnostics")]
    #[tokio::test]
    async fn review_regression_reply_error_count(){
        let _serial=DIAGNOSTIC_FIXTURE.lock().unwrap();crate::diagnostics::test_enable();
        crate::diagnostics::start_from_env();
        let tmp=tempfile::tempdir().unwrap();
        let app=Arc::new(std::sync::Mutex::new(app_at_fixture_date(&tmp.path().join("reply.db"))));
        let token=app.lock().unwrap().hello("reply-error",["lcd".into()].into(),1).unwrap();
        let (tx,rx)=tokio::sync::mpsc::channel(32);
        tx.send(WriterCommand::Bind(token)).await.unwrap();
        tx.send(WriterCommand::reply(WireMessage::InputAccepted{input_context:None,selected:None,display_epoch:1})).await.unwrap();
        let before=crate::diagnostics::test_count(crate::diagnostics::Kind::ReplyError);
        let why=writer_loop(AuditErrorSink{count:2},rx,app).await;
        let errors=crate::diagnostics::test_count(crate::diagnostics::Kind::ReplyError)-before;
        assert_eq!(why,"control_write_failed");assert_eq!(errors,1);
        println!("REVIEW one reply sink error increments ReplyError {errors} times");
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }


    #[test]
    fn review_regression_reply_command_layout(){
        println!("R3 layout command_bytes={} message_bytes={} timestamp_field={}",std::mem::size_of::<WriterCommand>(),std::mem::size_of::<WireMessage>(),cfg!(feature="stream-diagnostics"));
    }

    include!("../tests/support/transport_recovery.rs");
    mod hello_order { use super::*; include!("../tests/support/hello_order.rs"); }
    include!("../tests/support/ack_probe_reader.rs");

    #[cfg(feature="boundary-observation")]
    include!("causal_contract_tests.rs");

}
