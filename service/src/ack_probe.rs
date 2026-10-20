//! Opt-in bounded ACK-path capture. Explicit local control; no hot-path IO.
use crate::protocol::WireMessage;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
const CAP: usize = 512;
const WINDOW_US: u64 = 20_000_000;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CONNECTION: AtomicU64 = AtomicU64::new(0);
static CONTENDED: AtomicU64 = AtomicU64::new(0);
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
#[derive(Clone, Copy, Default, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    #[default]
    Bind,
    RawPoll,
    RawPending,
    RawReady,
    RawError,
    StreamPoll,
    StreamPending,
    StreamReady,
    Wake,
    DecodeBegin,
    Decoded,
    ProcessBegin,
    ProcessDone,
    ReplyQueueBegin,
    ReplyQueueDone,
    End,
}
#[derive(Clone, Copy, Default, Serialize)]
pub struct Record {
    pub us: u64,
    pub event: Event,
    pub connection: u64,
    pub message: u64,
    pub session: u64,
    pub frame: u64,
    pub kind: u8,
    pub a: u64,
    pub b: u64,
}
#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Idle,
    Armed,
    Recording,
    Frozen,
}
struct State {
    target: String,
    boot: String,
    start: Instant,
    run: u64,
    last_run: u64,
    phase: Phase,
    reason: &'static str,
    armed: u64,
    trigger: u64,
    freeze: u64,
    records: [Record; CAP],
    head: usize,
    count: usize,
    pre_overwritten: u64,
    overwritten: u64,
    digest: u32,
    last_digest: u32,
    sealed: bool,
    bound: u64,
    session: u64,
    last_accept: u64,
    hooks: u64,
    total_ns: u64,
    max_ns: u64,
    contended_frozen: u64,
}
impl State {
    fn new(target: String) -> Self {
        Self {
            target,
            boot: format!(
                "{:x}-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                std::process::id()
            ),
            start: Instant::now(),
            run: 0,
            last_run: 0,
            phase: Phase::Idle,
            reason: "none",
            armed: 0,
            trigger: 0,
            freeze: 0,
            records: [Record::default(); CAP],
            head: 0,
            count: 0,
            pre_overwritten: 0,
            overwritten: 0,
            digest: 0,
            last_digest: 0,
            sealed: false,
            bound: 0,
            session: 0,
            last_accept: 0,
            hooks: 0,
            total_ns: 0,
            max_ns: 0,
            contended_frozen: 0,
        }
    }
    fn frozen_digest(&self) -> u32 {
        struct Sink(crc32fast::Hasher);
        impl std::io::Write for Sink {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.update(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink(crc32fast::Hasher::new());
        for i in 0..self.count {
            serde_json::to_writer(&mut sink, &self.records[(self.head + i) % CAP])
                .expect("fixed record serialization");
        }
        sink.0.finalize()
    }
    fn freeze(&mut self, now: u64) {
        self.freeze_reason(now, "manual");
    }
    fn freeze_reason(&mut self, now: u64, reason: &'static str) {
        if matches!(self.phase, Phase::Armed | Phase::Recording) {
            self.phase = Phase::Frozen;
            self.reason = reason;
            self.freeze = now;
            self.sealed = false;
            self.contended_frozen = CONTENDED.load(Ordering::Relaxed);
        }
    }
    fn seal(&mut self) {
        if self.phase == Phase::Frozen && !self.sealed {
            self.digest = self.frozen_digest();
            self.sealed = true;
        }
    }
    fn expire(&mut self, now: u64) {
        if self.phase == Phase::Recording && now.saturating_sub(self.trigger) >= WINDOW_US {
            self.freeze_reason(now, "deadline");
        }
    }
    fn push(&mut self, r: Record) -> bool {
        self.expire(r.us);
        if !matches!(self.phase, Phase::Armed | Phase::Recording) {
            return false;
        }
        if self.phase == Phase::Armed
            && r.event == Event::Decoded
            && matches!(r.kind, 6 | 7 | 14 | 15 | 16)
        {
            self.phase = Phase::Recording;
            self.trigger = r.us;
        }
        let at = (self.head + self.count) % CAP;
        if self.count == CAP {
            let victim = self.records[self.head];
            self.head = (self.head + 1) % CAP;
            if self.phase == Phase::Armed || victim.us < self.trigger {
                self.pre_overwritten = self.pre_overwritten.saturating_add(1);
            } else {
                self.overwritten = self.overwritten.saturating_add(1);
            }
        } else {
            self.count += 1;
        }
        self.records[at] = r;
        true
    }
    fn arm(&mut self, run: u64, now: u64) -> Result<(), &'static str> {
        if !matches!(self.phase, Phase::Idle) {
            return if self.run == run {
                Ok(())
            } else {
                Err("retained")
            };
        }
        if run == 0 || run <= self.last_run {
            return Err("old_run");
        }
        self.run = run;
        self.last_run = run;
        self.phase = Phase::Armed;
        self.reason = "none";
        self.armed = now;
        self.trigger = 0;
        self.freeze = 0;
        self.head = 0;
        self.count = 0;
        self.pre_overwritten = 0;
        self.overwritten = 0;
        self.digest = 0;
        self.sealed = false;
        self.hooks = 0;
        self.total_ns = 0;
        self.max_ns = 0;
        Ok(())
    }
    fn release(&mut self, digest: u32) -> Result<(), &'static str> {
        if self.phase != Phase::Frozen || digest != self.digest {
            return Err("digest");
        }
        self.last_digest = digest;
        self.phase = Phase::Idle;
        self.run = 0;
        self.count = 0;
        self.head = 0;
        Ok(())
    }
    fn accept(&mut self, connection: u64, session: u64) -> bool {
        if connection <= self.last_accept {
            return false;
        }
        self.last_accept = connection;
        self.bound = connection;
        self.session = session;
        true
    }
}
pub fn active(connection: u64) -> bool {
    connection != 0
        && ACTIVE.load(Ordering::Relaxed)
        && CONNECTION.load(Ordering::Relaxed) == connection
}
pub fn record(
    event: Event,
    connection: u64,
    message: u64,
    session: u64,
    frame: u64,
    kind: u8,
    a: u64,
    b: u64,
) {
    if !active(connection) {
        return;
    }
    let hook = Instant::now();
    let Some(state) = STATE.get() else {
        return;
    };
    if let Ok(mut s) = state.try_lock() {
        if !matches!(s.phase, Phase::Armed | Phase::Recording) {
            ACTIVE.store(false, Ordering::Relaxed);
            return;
        }
        let us = s.start.elapsed().as_micros() as u64;
        if !s.push(Record {
            us,
            event,
            connection,
            message,
            session,
            frame,
            kind,
            a,
            b,
        }) {
            ACTIVE.store(false, Ordering::Relaxed);
        }
        let ns = hook.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        s.hooks += 1;
        s.total_ns = s.total_ns.saturating_add(ns);
        s.max_ns = s.max_ns.max(ns);
    } else {
        CONTENDED.fetch_add(1, Ordering::Relaxed);
    }
}
// Register accepted target even while IDLE: ARM needs no reconnect/restart.
// try_lock only; failed recognition is retried on the next admitted message.
pub fn bind_accepted(
    app: &crate::web::SharedApp,
    token: &crate::SessionToken,
    connection: u64,
) -> bool {
    let Some(state) = STATE.get() else {
        return true;
    };
    let Ok(mut s) = state.try_lock() else {
        CONTENDED.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    if s.target != token.device_id || connection <= s.last_accept {
        return true;
    }
    let Ok(guard) = app.try_lock() else {
        CONTENDED.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    if !guard.session_is_current(token) {
        return true;
    }
    if s.accept(connection, token.session_epoch) {
        CONNECTION.store(connection, Ordering::Relaxed);
        let us = s.start.elapsed().as_micros() as u64;
        s.push(Record {
            us,
            event: Event::Bind,
            connection,
            session: token.session_epoch,
            ..Record::default()
        });
    }
    true
}
pub fn unbind(connection: u64) {
    // Called after normal reader End. An old close cannot clear a newer binding.
    let _ = CONNECTION.compare_exchange(connection, 0, Ordering::Relaxed, Ordering::Relaxed);
    let Some(state) = STATE.get() else {
        return;
    };
    let Ok(mut s) = state.try_lock() else {
        CONTENDED.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if s.bound == connection {
        s.bound = 0;
        s.session = 0;
        CONNECTION.store(0, Ordering::Relaxed);
    }
}
pub fn identity(message: &WireMessage) -> (u64, u64, u8) {
    let (s, f) = match message {
        WireMessage::ReceiptAck { token } | WireMessage::RenderedAck { token } => {
            (token.session_epoch, token.frame_id)
        }
        WireMessage::Up { session_epoch, .. }
        | WireMessage::Down { session_epoch, .. }
        | WireMessage::Next { session_epoch, .. }
        | WireMessage::Power { session_epoch, .. }
        | WireMessage::EnterSub { session_epoch, .. }
        | WireMessage::Heartbeat { session_epoch, .. } => (*session_epoch, 0),
        _ => (0, 0),
    };
    (s, f, message.kind())
}
pub fn decoded(connection: u64, ordinal: u64, kind: u8, json: &[u8], message: &WireMessage) {
    if !active(connection) {
        return;
    }
    let (session, frame, _) = identity(message);
    record(
        Event::Decoded,
        connection,
        ordinal,
        session,
        frame,
        kind,
        crc32fast::hash(json) as u64,
        json.len() as u64,
    );
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    op: String,
    boot: Option<String>,
    run: Option<String>,
    start: Option<usize>,
    limit: Option<usize>,
    digest: Option<u32>,
}
fn status(s: &State) -> serde_json::Value {
    serde_json::json!({"schema":2,"boot":s.boot,"run":format!("{:x}",s.run),"last_run":format!("{:x}",s.last_run),"target":s.target,"phase":s.phase,"reason":s.reason,"connection":s.bound,"session":s.session,"clock":"host_monotonic_us_since_process_probe_init","armed_us":s.armed,"trigger_us":s.trigger,"freeze_us":s.freeze,"window_us":WINDOW_US,"count":s.count,"capacity":CAP,"pre_overwritten":s.pre_overwritten,"overwritten":s.overwritten,"contended":if s.phase==Phase::Frozen{s.contended_frozen}else{CONTENDED.load(Ordering::Relaxed)},"digest":s.digest,"record_bytes":std::mem::size_of::<Record>(),"hooks":s.hooks,"hook_total_ns":s.total_ns,"hook_max_ns":s.max_ns})
}
fn control(s: &mut State, r: Request, now: u64) -> Result<serde_json::Value, &'static str> {
    s.expire(now);
    s.seal();
    if r.op == "status" {
        return Ok(status(s));
    }
    if r.boot.as_deref() != Some(s.boot.as_str()) {
        return Err("boot");
    }
    let text = r.run.as_deref().ok_or("run")?;
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("run");
    }
    let run = u64::from_str_radix(text, 16).map_err(|_| "run")?;
    if run == 0 {
        return Err("run");
    }
    if r.op == "arm" {
        let fresh = s.phase == Phase::Idle;
        s.arm(run, now)?;
        if fresh {
            CONTENDED.store(0, Ordering::Relaxed);
            if s.bound != 0 {
                s.push(Record {
                    us: now,
                    event: Event::Bind,
                    connection: s.bound,
                    session: s.session,
                    ..Record::default()
                });
            }
        }
        return Ok(status(s));
    }
    if r.op == "release"
        && s.phase == Phase::Idle
        && run == s.last_run
        && r.digest == Some(s.last_digest)
    {
        return Ok(status(s));
    }
    if s.run != run || s.phase == Phase::Idle {
        return Err("run");
    }
    match r.op.as_str() {
        "freeze" => {
            s.freeze(now);
            s.seal();
            Ok(status(s))
        }
        "read" => {
            if s.phase != Phase::Frozen {
                return Err("not_frozen");
            }
            let start = r.start.ok_or("page")?;
            let limit = r.limit.ok_or("page")?;
            if start > s.count || limit == 0 || limit > 16 {
                return Err("page");
            }
            let count = limit.min(s.count - start);
            let rows: Vec<_> = (start..start + count)
                .map(|i| s.records[(s.head + i) % CAP])
                .collect();
            Ok(serde_json::json!({"status":status(s),"start":start,"count":count,"records":rows}))
        }
        "release" => {
            s.release(r.digest.ok_or("digest")?)?;
            Ok(status(s))
        }
        _ => Err("op"),
    }
}
fn configured(target: String) -> std::io::Result<()> {
    if target.is_empty() || target.len() > 64 || !target.is_ascii() {
        return Err(std::io::Error::other("invalid target"));
    }
    STATE
        .set(Mutex::new(State::new(target)))
        .map_err(|_| std::io::Error::other("already configured"))
}
// Local Unix control only. Fixed <=512-byte request and <=16-row response.
// Dedicated control thread: no filesystem/write/format in network hooks.
fn serve_control(mut socket: std::os::unix::net::UnixStream) -> std::io::Result<()> {
    use std::io::{Read, Write};
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    socket.set_write_timeout(Some(Duration::from_millis(250)))?;
    let mut buf = [0u8; 512];
    let mut used = 0;
    let deadline = Instant::now() + Duration::from_secs(1);
    while used < buf.len() && Instant::now() < deadline {
        match socket.read(&mut buf[used..used + 1]) {
            Ok(0) => break,
            Ok(n) => {
                used += n;
                if buf[used - 1] == b'\n' {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
    }
    let reply = if used == 0 || used == buf.len() || buf[used - 1] != b'\n' {
        serde_json::json!({"error":"length_or_deadline"})
    } else {
        match serde_json::from_slice::<Request>(&buf[..used]) {
            Ok(request) => {
                let mut s = STATE
                    .get()
                    .expect("configured")
                    .lock()
                    .map_err(|_| std::io::Error::other("poisoned"))?;
                let now = s.start.elapsed().as_micros() as u64;
                if CONNECTION.load(Ordering::Relaxed) == 0 {
                    s.bound = 0;
                    s.session = 0;
                }
                match control(&mut s, request, now) {
                    Ok(v) => v,
                    Err(e) => serde_json::json!({"error":e}),
                }
            }
            Err(_) => serde_json::json!({"error":"syntax"}),
        }
    };
    if let Some(state) = STATE.get() {
        if let Ok(s) = state.lock() {
            ACTIVE.store(
                matches!(s.phase, Phase::Armed | Phase::Recording),
                Ordering::Relaxed,
            );
        }
    }
    serde_json::to_writer(&mut socket, &reply)?;
    socket.write_all(b"\n")
}
pub fn start_from_env() {
    let (Some(target), Some(path)) = (
        std::env::var_os("PICOMADO_ACK_PROBE_DEVICE"),
        std::env::var_os("PICOMADO_ACK_PROBE_CONTROL"),
    ) else {
        return;
    };
    let result = (|| -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::path::PathBuf::from(path);
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        configured(target.to_string_lossy().into_owned())?;
        std::thread::Builder::new()
            .name("pm-ack-control".into())
            .stack_size(256 * 1024)
            .spawn(move || loop {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let _ = serve_control(socket);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
                if let Some(state) = STATE.get() {
                    if let Ok(mut s) = state.try_lock() {
                        let now = s.start.elapsed().as_micros() as u64;
                        s.expire(now);
                        s.seal();
                        ACTIVE.store(
                            matches!(s.phase, Phase::Armed | Phase::Recording),
                            Ordering::Relaxed,
                        );
                    }
                }
            })?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("picomado ack_control_start_failed error={e}");
    }
}
// Observe poll/read boundaries without allocating proxy wakers or changing wakes.
pub async fn next<S: futures_util::Stream + Unpin>(
    stream: &mut S,
    connection: u64,
) -> Option<S::Item> {
    std::future::poll_fn(|cx| {
        record(Event::StreamPoll, connection, 0, 0, 0, 0, 0, 0);
        let result = std::pin::Pin::new(&mut *stream).poll_next(cx);
        record(
            if result.is_pending() {
                Event::StreamPending
            } else {
                Event::StreamReady
            },
            connection,
            0,
            0,
            0,
            0,
            0,
            0,
        );
        result
    })
    .await
}
#[cfg(test)]
pub(crate) fn test_start(device: &str, _path: &std::path::Path) {
    configured(device.into()).unwrap();
}
#[cfg(test)]
pub(crate) fn test_records() -> Vec<Record> {
    let s = STATE.get().unwrap().lock().unwrap();
    (0..s.count)
        .map(|i| s.records[(s.head + i) % CAP])
        .collect()
}
#[cfg(test)]
pub(crate) fn test_hold_state(
    ready: tokio::sync::oneshot::Sender<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _guard = STATE.get().unwrap().lock().unwrap();
        let _ = ready.send(());
        std::thread::sleep(Duration::from_millis(150));
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(us: u64, kind: u8) -> Record {
        Record {
            us,
            event: Event::Decoded,
            kind,
            connection: 2,
            ..Record::default()
        }
    }
    fn request(op: &str, s: &State, run: u64) -> Request {
        Request {
            op: op.into(),
            boot: Some(s.boot.clone()),
            run: Some(format!("{run:x}")),
            start: None,
            limit: None,
            digest: None,
        }
    }
    #[test]
    fn ack_probe_wait_and_first_input_deadline() {
        let mut s = State::new("lcd".into());
        s.arm(7, 1).unwrap();
        for i in 0..2000 {
            s.push(row(10_000_000_000 + i, 19));
        }
        assert_eq!(s.phase, Phase::Armed);
        assert_eq!(s.count, CAP);
        assert!(s.pre_overwritten > 0);
        s.push(row(20_000_000_000, 15));
        s.push(row(20_019_000_000, 15));
        assert_eq!(s.trigger, 20_000_000_000);
        s.expire(20_020_000_000);
        assert_eq!(s.phase, Phase::Frozen);
        assert_eq!(s.overwritten, 0); // Only prehistory was evicted in this case.
        assert!(std::mem::size_of::<State>() < 34000);
    }
    #[test]
    fn ack_probe_frozen_pages_and_release_are_stable() {
        let mut s = State::new("lcd".into());
        s.arm(1, 1).unwrap();
        s.push(row(10, 15));
        s.freeze(15);
        s.seal();
        let digest = s.digest;
        let r = request("arm", &s, 2);
        assert_eq!(control(&mut s, r, 100).unwrap_err(), "retained");
        let mut r = request("read", &s, 1);
        r.start = Some(0);
        r.limit = Some(16);
        let a = control(&mut s, r, 999_000_000).unwrap();
        let mut r = request("read", &s, 1);
        r.start = Some(0);
        r.limit = Some(16);
        assert_eq!(a, control(&mut s, r, 9_999_000_000).unwrap());
        assert_eq!(s.release(digest ^ 1), Err("digest"));
        s.release(digest).unwrap();
        s.arm(2, 10_000_000_000).unwrap();
        assert_eq!(s.phase, Phase::Armed);
    }
    #[test]
    fn ack_probe_boot_run_and_capacity_validation() {
        let mut s = State::new("lcd".into());
        let mut q = request("arm", &s, 1);
        q.boot = Some("old-boot".into());
        assert_eq!(control(&mut s, q, 1).unwrap_err(), "boot");
        s.arm(1, 1).unwrap();
        for i in 0..(CAP + 500) {
            s.push(row(i as u64 + 1, 15));
        }
        assert_eq!(s.count, CAP);
        assert_eq!(s.phase, Phase::Recording);
        assert_eq!(s.overwritten, 500);
        s.freeze(1000);
        let mut r = request("read", &s, 1);
        r.start = Some(0);
        r.limit = Some(17);
        assert_eq!(control(&mut s, r, 1001).unwrap_err(), "page");
        assert_eq!(s.count, CAP);
    }
    #[test]
    fn ack_probe_control_rejects_zero_and_noncanonical_run() {
        let mut s = State::new("lcd".into());
        for text in ["0", "+1", "10000000000000000", ""] {
            let mut q = request("release", &s, 0);
            q.run = Some(text.into());
            q.digest = Some(0);
            assert_eq!(control(&mut s, q, 1).unwrap_err(), "run");
        }
        assert_eq!(s.phase, Phase::Idle);
    }
    #[test]
    fn ack_probe_release_retry_and_old_arm_are_safe() {
        let mut s = State::new("lcd".into());
        s.arm(1, 1).unwrap();
        s.push(row(2, 15));
        s.freeze(3);
        s.seal();
        let digest = s.digest;
        let mut q = request("release", &s, 1);
        q.digest = Some(digest);
        assert_eq!(control(&mut s, q, 4).unwrap()["phase"], "idle");
        let mut q = request("release", &s, 1);
        q.digest = Some(digest);
        assert_eq!(control(&mut s, q, 5).unwrap()["phase"], "idle");
        assert_eq!(s.arm(1, 6), Err("old_run"));
        assert!(s.arm(2, 7).is_ok());
    }
    #[test]
    fn ack_probe_freeze_ack_contains_final_digest() {
        let mut s = State::new("lcd".into());
        s.arm(1, 1).unwrap();
        s.push(row(2, 15));
        let q = request("freeze", &s, 1);
        let ack = control(&mut s, q, 3).unwrap();
        let mut q = request("read", &s, 1);
        q.start = Some(0);
        q.limit = Some(16);
        let page = control(&mut s, q, 4).unwrap();
        assert_eq!(ack["digest"], page["status"]["digest"]);
        assert!(s.sealed);
    }
    #[test]
    fn ack_probe_connection_generations_survive_freeze() {
        let mut s = State::new("lcd".into());
        assert!(s.accept(10, 7));
        assert!(!s.accept(9, 8));
        assert!(!s.accept(10, 8));
        s.arm(1, 1).unwrap();
        s.push(row(10, 15));
        s.freeze(30);
        s.seal();
        let hash = s.digest;
        assert!(s.accept(11, 9));
        assert_eq!(s.digest, hash);
        assert_eq!(s.records[0].connection, 2);
        assert_eq!(s.bound, 11);
    }
}

#[cfg(test)]
pub(crate) fn test_control(request: serde_json::Value) -> serde_json::Value {
    use std::io::{Read, Write};
    let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    let worker = std::thread::spawn(move || serve_control(server).unwrap());
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    serde_json::to_writer(&mut client, &request).unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let mut buf = String::new();
    client.read_to_string(&mut buf).unwrap();
    worker.join().unwrap();
    serde_json::from_str(&buf).unwrap()
}
