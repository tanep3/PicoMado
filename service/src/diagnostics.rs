//! Optional bounded stream diagnostics. No network or application state changes.
//! Disabled builds inline to the original lock and empty hooks.
use std::sync::{LockResult, Mutex, MutexGuard};
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Domain {
    Reader,
    Writer,
    Playback,
    Work,
    Fetch,
    Clock,
}
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Kind {
    Publish,
    Replace,
    Clear,
    FrameStart,
    PacketStart,
    PacketDone,
    SendError,
    FragmentDeadline,
    FrameDeadline,
    ReplyStart,
    ReplyDone,
    ReplyError,
    End,
    LogIo,
    Persist,
    Tick,
    LockWait,
    LockHold,
    Diagnostic,
    FrameDone,
    ReplyQueue,
    DeadlineWake,
    ReplyCapacity,
}
#[cfg(not(feature = "stream-diagnostics"))]
#[inline(always)]
pub fn mark() -> u64 {
    0
}
#[cfg(not(feature = "stream-diagnostics"))]
#[inline(always)]
pub fn event(_: Kind, _: u64, _: u64, _: u64, _: u64, _: u64) {}
#[cfg(not(feature = "stream-diagnostics"))]
#[inline(always)]
pub fn duration(_: Kind, _: u64, _: u64, _: u64, _: u64) {}
#[cfg(not(feature = "stream-diagnostics"))]
#[inline(always)]
pub fn lock<T>(m: &Mutex<T>, _: Domain) -> LockResult<MutexGuard<'_, T>> {
    m.lock()
}
#[cfg(not(feature = "stream-diagnostics"))]
pub fn start_from_env() {}

#[cfg(feature = "stream-diagnostics")]
mod enabled {
    use super::*;
    use serde::Serialize;
    use std::io::{BufWriter, Write};
    use std::ops::{Deref, DerefMut};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock, PoisonError,
    };
    use std::time::{Duration, Instant, SystemTime};
    const CAP: usize = 128;
    const METRICS: usize = 23;
    const EDGES: [u64; 32] = [
        1000,
        2000,
        4000,
        8000,
        16000,
        24000,
        32000,
        40000,
        50000,
        60000,
        70000,
        80000,
        90000,
        100000,
        120000,
        150000,
        180000,
        220000,
        260000,
        320000,
        400000,
        500000,
        650000,
        800000,
        1000000,
        1300000,
        1600000,
        2000000,
        3000000,
        5000000,
        10000000,
        u64::MAX,
    ];
    #[derive(Clone, Copy, Default, Serialize)]
    struct Record {
        us: u64,
        kind: u64,
        session: u64,
        frame: u64,
        a: u64,
        b: u64,
        c: u64,
    }
    #[derive(Clone, Copy, Default, Serialize)]
    struct Histogram {
        bins: [u64; 32],
        count: u64,
        total_us: u64,
        max_us: u64,
        ge_500ms: u64,
        ge_1s: u64,
    }
    impl Histogram {
        fn add(&mut self, n: u64) {
            self.count = self.count.saturating_add(1);
            self.total_us = self.total_us.saturating_add(n);
            self.max_us = self.max_us.max(n);
            let i = EDGES.iter().position(|&e| n < e).unwrap_or(31);
            self.bins[i] = self.bins[i].saturating_add(1);
            self.ge_500ms = self.ge_500ms.saturating_add(u64::from(n >= 500000));
            self.ge_1s = self.ge_1s.saturating_add(u64::from(n >= 1000000));
        }
    }
    struct State {
        records: [Record; CAP],
        head: usize,
        len: usize,
        overwritten: u64,
        events: [u64; METRICS],
        hooks: Histogram,
        hist: [Histogram; METRICS],
        waits: [Histogram; 6],
        holds: [Histogram; 6],
    }
    impl State {
        const fn new() -> Self {
            const H: Histogram = Histogram {
                bins: [0; 32],
                count: 0,
                total_us: 0,
                max_us: 0,
                ge_500ms: 0,
                ge_1s: 0,
            };
            Self {
                records: [Record {
                    us: 0,
                    kind: 0,
                    session: 0,
                    frame: 0,
                    a: 0,
                    b: 0,
                    c: 0,
                }; CAP],
                head: 0,
                len: 0,
                overwritten: 0,
                events: [0; METRICS],
                hooks: H,
                hist: [H; METRICS],
                waits: [H; 6],
                holds: [H; 6],
            }
        }
        fn push(&mut self, r: Record) {
            if self.len == CAP {
                self.head = (self.head + 1) % CAP;
                self.len -= 1;
                self.overwritten = self.overwritten.saturating_add(1);
            }
            self.records[(self.head + self.len) % CAP] = r;
            self.len += 1;
        }
    }
    static STATE: Mutex<State> = Mutex::new(State::new());
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static STARTED: AtomicBool = AtomicBool::new(false);
    static CONTENDED: AtomicU64 = AtomicU64::new(0);
    #[cfg(test)]
    thread_local! {
        // Current-thread Tokio fixtures inspect their own hook calls, not
        // unrelated tests using the process-wide production exporter.
        static TEST_STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
    }
    pub fn mark() -> u64 {
        if !ACTIVE.load(Ordering::Relaxed) {
            return 0;
        }
        EPOCH.get().map_or(0, |s| {
            s.elapsed().as_micros().min(u64::MAX as u128) as u64 + 1
        })
    }
    pub fn event(kind: Kind, session: u64, frame: u64, a: u64, b: u64, c: u64) {
        let us = mark();
        if us == 0 {
            return;
        }
        #[cfg(test)]
        TEST_STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.events[kind as usize] = state.events[kind as usize].saturating_add(1);
            }
        });
        let hook = Instant::now();
        if let Ok(mut s) = STATE.try_lock() {
            s.events[kind as usize] = s.events[kind as usize].saturating_add(1);
            s.push(Record {
                us,
                kind: kind as u64,
                session,
                frame,
                a,
                b,
                c,
            });
            s.hooks.add(hook.elapsed().as_micros() as u64);
        } else {
            CONTENDED.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn duration(kind: Kind, start: u64, session: u64, frame: u64, domain: u64) {
        if start == 0 {
            return;
        }
        let now = mark();
        let elapsed = now.saturating_sub(start);
        #[cfg(test)]
        TEST_STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.hist[kind as usize].add(elapsed);
            }
        });
        let hook = Instant::now();
        if let Ok(mut s) = STATE.try_lock() {
            s.hist[kind as usize].add(elapsed);
            if domain < 6 {
                if matches!(kind, Kind::LockWait) {
                    s.waits[domain as usize].add(elapsed);
                }
                if matches!(kind, Kind::LockHold) {
                    s.holds[domain as usize].add(elapsed);
                }
            }
            if elapsed >= 50000 {
                s.push(Record {
                    us: now,
                    kind: kind as u64,
                    session,
                    frame,
                    a: elapsed,
                    b: domain,
                    c: 0,
                });
            }
            s.hooks.add(hook.elapsed().as_micros() as u64);
        } else {
            CONTENDED.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub struct Guard<'a, T> {
        guard: MutexGuard<'a, T>,
        domain: Domain,
        start: u64,
    }
    impl<T> Deref for Guard<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.guard
        }
    }
    impl<T> DerefMut for Guard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            &mut self.guard
        }
    }
    impl<T> Drop for Guard<'_, T> {
        fn drop(&mut self) {
            duration(Kind::LockHold, self.start, 0, 0, self.domain as u64);
        }
    }
    pub fn lock<T>(m: &Mutex<T>, domain: Domain) -> LockResult<Guard<'_, T>> {
        let before = mark();
        let result = m.lock();
        duration(Kind::LockWait, before, 0, 0, domain as u64);
        match result {
            Ok(guard) => Ok(Guard {
                guard,
                domain,
                start: mark(),
            }),
            Err(e) => Err(PoisonError::new(Guard {
                guard: e.into_inner(),
                domain,
                start: mark(),
            })),
        }
    }
    pub fn start_from_env() {
        let Some(path) = std::env::var_os("PICOMADO_DIAGNOSTICS_PATH") else {
            return;
        };
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let file = match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!("picomado diagnostics_unavailable error={e}");
                return;
            }
        };
        let _ = EPOCH.set(Instant::now());
        ACTIVE.store(true, Ordering::Release);
        let spawn_result=std::thread::Builder::new().name("pm-diagnostics".into()).spawn(move||{
            let mut out=BufWriter::with_capacity(32768,file);
            let mut export_us=0u64;let mut export_bytes=0u64;
            let deadline=Instant::now()+Duration::from_secs(300);
            loop {
                std::thread::sleep(Duration::from_secs(1));let started=Instant::now();
                // Copy under the diagnostic mutex, then do serialization and IO
                // outside it. This mutex is never waited on by producer hooks.
                let (records,hist,waits,holds,overwritten,events,hooks)= {
                    let Ok(mut s)=STATE.lock() else{break;};
                    let mut records=Vec::with_capacity(s.len);
                    for i in 0..s.len {records.push(s.records[(s.head+i)%CAP]);}
                    s.head=0;s.len=0;
                    (records,s.hist,s.waits,s.holds,s.overwritten,s.events,s.hooks)
                };
                let data=serde_json::json!({"schema":2,"mono_us":mark(),
                    "unix_us":SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_micros(),
                    "record_capacity":CAP,"static_state_bytes":std::mem::size_of::<State>(),
                    "contended":CONTENDED.load(Ordering::Relaxed),"overwritten":overwritten,
                    "previous_export_us":export_us,"previous_export_bytes":export_bytes,
                    "histogram_upper_exclusive_us":EDGES,"histogram":hist,"event_counts":events,"hook_cost_us":hooks,
                    "kind_names":["publish","replace","clear","frame_start","packet_start","packet_done","send_error","fragment_deadline","frame_deadline","reply_start","reply_done","reply_error","end","log_io","persist","tick","lock_wait","lock_hold","diagnostic","frame_done","reply_queue","deadline_wake","reply_capacity"],
                    "domain_names":["reader","writer","playback","work","fetch","clock"],
                    "lock_wait_by_domain":waits,"lock_hold_by_domain":holds,"events":records});
                let bytes=match serde_json::to_vec(&data){Ok(b)=>b,Err(_)=>break};
                if out.write_all(&bytes).and_then(|_|out.write_all(b"\n")).and_then(|_|out.flush()).is_err(){break;}
                export_bytes=bytes.len() as u64+1;export_us=started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                if Instant::now()>=deadline {break;}
            }
            ACTIVE.store(false,Ordering::Release);
        });
        if let Err(e) = spawn_result {
            ACTIVE.store(false, Ordering::Release);
            eprintln!("picomado diagnostics_worker_unavailable error={e}");
        }
    }
    #[cfg(test)]
    pub fn test_enable() {
        let _ = EPOCH.get_or_init(Instant::now);
        TEST_STATE.with(|state| *state.borrow_mut() = Some(State::new()));
        ACTIVE.store(true, Ordering::Release);
    }
    #[cfg(test)]
    pub fn test_hist(kind: Kind) -> (u64, u64) {
        if let Some(result) = TEST_STATE.with(|state| {
            state.borrow().as_ref().map(|state| {
                let h = state.hist[kind as usize];
                (h.count, h.max_us)
            })
        }) {
            return result;
        }
        let s = STATE.lock().unwrap();
        let h = s.hist[kind as usize];
        (h.count, h.max_us)
    }
    #[cfg(test)]
    pub fn test_count(kind: Kind) -> u64 {
        if let Some(count) = TEST_STATE.with(|state| {
            state.borrow().as_ref().map(|state| state.events[kind as usize])
        }) {
            return count;
        }
        STATE.lock().unwrap().events[kind as usize]
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn fixture_counters_and_histograms_exclude_other_threads() {
            test_enable();
            event(Kind::FragmentDeadline, 1, 1, 0, 0, 0);
            duration(Kind::DeadlineWake, mark(), 1, 1, 0);
            std::thread::spawn(|| {
                event(Kind::FragmentDeadline, 2, 2, 0, 0, 0);
                duration(Kind::DeadlineWake, mark(), 2, 2, 0);
            })
            .join()
            .unwrap();
            assert_eq!(test_count(Kind::FragmentDeadline), 1);
            assert_eq!(test_hist(Kind::DeadlineWake).0, 1);
        }
        #[test]
        fn bounded_ring_and_all_sample_histogram() {
            let mut s = State::new();
            for i in 0..10000 {
                s.push(Record {
                    frame: i,
                    ..Record::default()
                });
                s.hist[0].add(i * 1000);
            }
            assert_eq!(s.len, CAP);
            assert_eq!(s.overwritten, 10000 - CAP as u64);
            assert_eq!(s.records[s.head].frame, 10000 - CAP as u64);
            let h = s.hist[0];
            assert_eq!(h.count, 10000);
            assert_eq!(h.bins.iter().sum::<u64>(), h.count);
            assert_eq!(h.ge_500ms, 9500);
            assert_eq!(h.ge_1s, 9000);
        }
        #[test]
        fn histogram_boundaries_and_saturation() {
            let mut h = Histogram::default();
            for n in [0, 499999, 500000, 999999, 1000000, u64::MAX] {
                h.add(n);
            }
            assert_eq!(h.count, 6);
            assert_eq!(h.bins.iter().sum::<u64>(), 6);
            assert_eq!(h.ge_500ms, 4);
            assert_eq!(h.ge_1s, 2);
            assert_eq!(h.max_us, u64::MAX);
            assert_eq!(h.total_us, u64::MAX);
        }
    }
}
#[cfg(feature = "stream-diagnostics")]
pub use enabled::{duration, event, lock, mark, start_from_env};

#[cfg(all(test, feature = "stream-diagnostics"))]
pub use enabled::{test_count, test_enable, test_hist};

#[inline(always)]
pub fn tick(domain: Domain, previous: &mut u64) {
    let now = mark();
    if *previous != 0 {
        duration(Kind::Tick, *previous, 0, 0, domain as u64);
    }
    *previous = now;
}
