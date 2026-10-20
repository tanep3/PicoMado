//! Disposable boundary facts. Pure Rules never call this module. Producers do
//! no IO and never wait for the consumer; capture loss has its own sequence.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cause {
    pub version: u8,
    pub boot: u64,
    pub operation: u64,
}
impl Cause {
    pub fn valid(&self) -> bool {
        self.version == 1 && self.boot != 0 && self.operation != 0
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum Component {
    Input = 1,
    Api = 2,
    Network = 3,
    Frame = 4,
    Display = 5,
    State = 6,
    Effect = 7,
    Observation = 8,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Received = 1,
    Dispatched = 2,
    Pending = 3,
    Completed = 4,
    Rejected = 5,
    Error = 6,
    Cancelled = 7,
    Committed = 8,
}
#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub sequence: u64,
    pub us: u64,
    pub component: Component,
    pub phase: Phase,
    pub operation: u64,
    pub parent: u64,
    pub revision: u64,
    pub argument: u64,
    pub boot: u64,
    pub frame: u64,
    pub cause: Option<Cause>,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Status {
    pub boot: u64,
    pub sequence: u64,
    pub dropped: u64,
    pub enabled: bool,
    pub saturated: bool,
}

#[cfg(feature = "boundary-observation")]
mod enabled {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex,
        },
        time::Instant,
    };
    pub const CAPACITY: usize = 256;
    struct Inner {
        boot: u64,
        start: Instant,
        enabled: AtomicBool,
        sequence: AtomicU64,
        dropped: AtomicU64,
        active: AtomicU64,
        saturated: AtomicBool,
        consumer: AtomicBool,
        queue: Mutex<VecDeque<Fact>>,
    }
    fn source_boot() -> u64 {
        use std::io::Read;
        let mut bytes = [0u8; 8];
        if std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .is_err()
        {
            return 0;
        }
        u64::from_ne_bytes(bytes).max(1)
    }
    struct Producer<'a>(&'a AtomicU64);
    impl Drop for Producer<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    #[derive(Clone)]
    pub struct Source(Arc<Inner>);
    impl Default for Source {
        fn default() -> Self {
            Self(Arc::new(Inner {
                boot: source_boot(),
                start: Instant::now(),
                enabled: AtomicBool::new(false),
                sequence: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
                active: AtomicU64::new(0),
                saturated: AtomicBool::new(false),
                consumer: AtomicBool::new(false),
                queue: Mutex::new(VecDeque::with_capacity(CAPACITY)),
            }))
        }
    }
    impl Source {
        pub fn enable(&self, on: bool) {
            self.0
                .enabled
                .store(on && self.0.boot != 0, Ordering::SeqCst);
        }
        pub fn enabled(&self) -> bool {
            self.0.enabled.load(Ordering::SeqCst)
        }
        pub fn record(
            &self,
            component: Component,
            phase: Phase,
            operation: u64,
            parent: u64,
            revision: u64,
            argument: u64,
        ) -> u64 {
            self.record_context(
                component, phase, operation, parent, revision, argument, 0, None,
            )
        }
        pub fn record_context(
            &self,
            component: Component,
            phase: Phase,
            operation: u64,
            parent: u64,
            revision: u64,
            argument: u64,
            frame: u64,
            cause: Option<Cause>,
        ) -> u64 {
            self.0.active.fetch_add(1, Ordering::SeqCst);
            let _producer = Producer(&self.0.active);
            if !self.enabled() {
                return 0;
            }
            let mut queue = self.0.queue.try_lock().ok();
            let Ok(old) = self
                .0
                .sequence
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            else {
                self.0.saturated.store(true, Ordering::Relaxed);
                self.enable(false);
                return 0;
            };
            let sequence = old + 1;
            if let Some(queue) = queue.as_mut() {
                if queue.len() < CAPACITY {
                    queue.push_back(Fact {
                        sequence,
                        us: self.0.start.elapsed().as_micros().min(u64::MAX as u128) as u64,
                        component,
                        phase,
                        operation: if operation == 0 { sequence } else { operation },
                        parent,
                        revision,
                        argument,
                        boot: self.0.boot,
                        frame,
                        cause,
                    });
                    return sequence;
                }
            }
            let _ = self
                .0
                .dropped
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_add(1))
                });
            sequence
        }
        pub fn stop_producers(&self, timeout: std::time::Duration) -> bool {
            self.enable(false);
            let began = Instant::now();
            while self.0.active.load(Ordering::SeqCst) != 0 {
                if began.elapsed() >= timeout {
                    return false;
                }
                std::thread::yield_now();
            }
            true
        }
        /// Cold consumer only. Call outside the shared App lock.
        pub fn drain(&self) -> Vec<Fact> {
            if self
                .0
                .consumer
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return Vec::new();
            }
            let rows = self.drain_owned();
            self.0.consumer.store(false, Ordering::SeqCst);
            rows
        }
        fn drain_owned(&self) -> Vec<Fact> {
            self.0
                .queue
                .lock()
                .expect("observation consumer")
                .drain(..)
                .collect()
        }
        fn now_us(&self) -> u64 {
            self.0.start.elapsed().as_micros().min(u64::MAX as u128) as u64
        }
        pub fn status(&self) -> Status {
            Status {
                boot: self.0.boot,
                sequence: self.0.sequence.load(Ordering::Relaxed),
                dropped: self.0.dropped.load(Ordering::Relaxed),
                enabled: self.enabled(),
                saturated: self.0.saturated.load(Ordering::Relaxed),
            }
        }
    }
    /// Owns one cold consumer. Its worker owns the file even if a finite wait expires.
    pub struct Capture {
        stop: Arc<AtomicBool>,
        done: std::sync::mpsc::Receiver<Result<(), String>>,
        worker: std::thread::Thread,
    }
    impl Capture {
        pub fn start(source: Source, path: &std::path::Path) -> Result<Self, String> {
            Self::start_bounded(source, path, 64 * 1024 * 1024)
        }
        fn start_bounded(
            source: Source,
            path: &std::path::Path,
            max_bytes: usize,
        ) -> Result<Self, String> {
            use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
            if source
                .0
                .consumer
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return Err("observation consumer already owned".into());
            }
            let setup = (|| {
                if source.enabled() || source.0.boot == 0 {
                    return Err("source must be disabled and have a boot identity".into());
                }
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                // Establish baseline before enabling, not when the worker is eventually scheduled.
                source.drain_owned();
                Ok((file, source.status(), source.now_us()))
            })();
            let (mut file, begin, begin_us) = match setup {
                Ok(v) => v,
                Err(e) => {
                    source.0.consumer.store(false, Ordering::SeqCst);
                    return Err(e);
                }
            };
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let (send, done) = std::sync::mpsc::sync_channel(1);
            let worker_source = source.clone();
            // Producer work can begin as soon as this call returns.
            source.enable(true);
            let mut manifest_name = path.as_os_str().to_os_string();
            manifest_name.push(".summary.json");
            let manifest_path = std::path::PathBuf::from(manifest_name);
            let spawn=std::thread::Builder::new().name("boundary-observation".into()).spawn(move||{
                let result=write_capture(&worker_source,&mut file,begin,begin_us,max_bytes,&stopping);
                let manifest=(||->Result<(),String>{
                    use std::io::{Read,Seek,SeekFrom,Write};use sha2::{Sha256,Digest};
                    file.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;
                    let mut hash=Sha256::new();let mut buffer=[0u8;8192];
                    loop {let n=file.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}hash.update(&buffer[..n]);}
                    let mut summary=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(manifest_path).map_err(|e|e.to_string())?;
                    let row=serde_json::json!({"complete":result.is_ok(),"storage_durable":result.is_ok(),"source_sha256":format!("{:x}",hash.finalize()),"error":result.as_ref().err()});
                    serde_json::to_writer(&mut summary,&row).map_err(|e|e.to_string())?;summary.write_all(b"\n").map_err(|e|e.to_string())?;summary.sync_all().map_err(|e|e.to_string())?;Ok(())
                })();
                let result=result.and(manifest);
                worker_source.enable(false);worker_source.0.consumer.store(false,Ordering::SeqCst);
                let _=send.send(result);
            });
            match spawn {
                Ok(worker) => Ok(Self {
                    stop,
                    done,
                    worker: worker.thread().clone(),
                }),
                Err(e) => {
                    source.enable(false);
                    source.0.consumer.store(false, Ordering::SeqCst);
                    Err(e.to_string())
                }
            }
        }
        fn stop_and_wait(&self) -> Result<(), String> {
            self.stop.store(true, Ordering::SeqCst);
            self.worker.unpark();
            self.done
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|e| format!("observation finish: {e}"))?
        }
        pub fn finish(self) -> Result<(), String> {
            self.stop_and_wait()
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            if !self.stop.swap(true, Ordering::SeqCst) {
                self.worker.unpark();
                match self.done.recv_timeout(std::time::Duration::from_secs(2)) {
                    Ok(Ok(())) => {}
                    result => eprintln!("picomado observation_finish_incomplete {result:?}"),
                }
            }
        }
    }
    trait Storage: std::io::Write {
        fn sync_data(&mut self) -> std::io::Result<()>;
        fn sync_all(&mut self) -> std::io::Result<()>;
    }
    impl Storage for std::fs::File {
        fn sync_data(&mut self) -> std::io::Result<()> {
            std::fs::File::sync_data(self)
        }
        fn sync_all(&mut self) -> std::io::Result<()> {
            std::fs::File::sync_all(self)
        }
    }
    fn write_capture<W: Storage>(
        source: &Source,
        file: &mut W,
        begin: Status,
        begin_us: u64,
        max_bytes: usize,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        const FOOTER: usize = 4096;
        let mut bytes = 0usize;
        let mut last = begin.sequence;
        let mut lost = 0u64;
        let mut error = None;
        fn write_row<W: Storage>(
            file: &mut W,
            row: &serde_json::Value,
            bytes: &mut usize,
            limit: usize,
        ) -> Result<(), String> {
            let mut data = serde_json::to_vec(row).map_err(|e| e.to_string())?;
            data.push(b'\n');
            if data.len() > limit.saturating_sub(*bytes) {
                return Err("observation byte limit".into());
            }
            file.write_all(&data).map_err(|e| e.to_string())?;
            *bytes += data.len();
            Ok(())
        }
        let header = serde_json::json!({"kind":"begin","status":begin,"us":begin_us,"catalog":2,"coverage":235});
        if let Err(e) = write_row(
            file,
            &header,
            &mut bytes,
            max_bytes.saturating_sub(FOOTER),
        ) {
            error = Some(e);
        }
        let mut synced = Instant::now();
        loop {
            let ending = stop.load(Ordering::SeqCst);
            let quiet = if ending {
                source.stop_producers(std::time::Duration::from_millis(250))
            } else {
                true
            };
            for fact in source.drain_owned() {
                if fact.sequence != last.saturating_add(1) {
                    lost = lost.saturating_add(1);
                }
                last = fact.sequence;
                if error.is_none() {
                    if let Err(e) = write_row(
                        file,
                        &serde_json::json!({"kind":"event","event":fact}),
                        &mut bytes,
                        max_bytes.saturating_sub(FOOTER),
                    ) {
                        error = Some(e);
                        lost = lost.saturating_add(1);
                    }
                } else {
                    lost = lost.saturating_add(1);
                }
            }
            if ending {
                let end = source.status();
                let complete = quiet
                    && error.is_none()
                    && lost == 0
                    && end.dropped == begin.dropped
                    && !end.saturated
                    && last == end.sequence;
                let footer = serde_json::json!({"kind":"end","status":end,"us":source.now_us(),"catalog":2,"coverage":235,"complete":complete,"storage_lost":lost,"error":error});
                write_row(file, &footer, &mut bytes, max_bytes)?;
                file.sync_all().map_err(|e| e.to_string())?;
                return if complete {
                    Ok(())
                } else {
                    Err("observation capture incomplete".into())
                };
            }
            if synced.elapsed() >= std::time::Duration::from_millis(250) {
                if let Err(e) = file.sync_data() {
                    error = Some(e.to_string());
                }
                synced = Instant::now();
            }
            std::thread::park_timeout(std::time::Duration::from_millis(10));
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn exclusive_capture_is_durable_bounded_and_independent_of_app_lock() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("events");
            let source = Source::default();
            let capture = Capture::start(source.clone(), &path).unwrap();
            assert!(Capture::start(source.clone(), &temp.path().join("other")).is_err());
            source.record_context(
                Component::Api,
                Phase::Received,
                0,
                0,
                4,
                16,
                7,
                Some(Cause {
                    version: 1,
                    boot: 5,
                    operation: 9,
                }),
            );
            assert!(
                source.drain().is_empty(),
                "second consumer must not steal rows"
            );
            // No App or application mutex is needed by the cold consumer.
            capture.finish().unwrap();
            let rows: Vec<serde_json::Value> = std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            assert_eq!(rows[0]["kind"], "begin");
            assert_eq!(rows[1]["event"]["sequence"], 1);
            assert_eq!(rows[1]["event"]["cause"]["operation"], 9);
            assert_eq!(rows.last().unwrap()["complete"], true);
            assert_eq!(rows.last().unwrap()["status"]["sequence"], 1);
            assert!(!source.enabled());
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(Capture::start(source.clone(), &path).is_err());
            let small = temp.path().join("bounded");
            let cap = Capture::start_bounded(source.clone(), &small, 4608).unwrap();
            for _ in 0..CAPACITY + 10 {
                source.record(Component::Api, Phase::Received, 0, 0, 0, 0);
            }
            assert!(cap.finish().is_err());
            assert!(std::fs::metadata(&small).unwrap().len() <= 4608);
            let text = std::fs::read_to_string(&small).unwrap();
            let end: serde_json::Value =
                serde_json::from_str(text.lines().last().unwrap()).unwrap();
            assert_eq!(end["complete"], false);
            assert!(end["storage_lost"].as_u64().unwrap() > 0);
        }
        struct FaultStorage {
            fail_write: bool,
            fail_sync: bool,
            release: Option<std::sync::mpsc::Receiver<()>>,
            dropped: Arc<AtomicBool>,
        }
        impl std::io::Write for FaultStorage {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                if self.fail_write {
                    Err(std::io::Error::other("injected write error"))
                } else {
                    Ok(data.len())
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl Storage for FaultStorage {
            fn sync_data(&mut self) -> std::io::Result<()> {
                Ok(())
            }
            fn sync_all(&mut self) -> std::io::Result<()> {
                if let Some(release) = self.release.take() {
                    release.recv().unwrap();
                }
                if self.fail_sync {
                    Err(std::io::Error::other("injected fsync error"))
                } else {
                    Ok(())
                }
            }
        }
        impl Drop for FaultStorage {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::SeqCst);
            }
        }
        #[test]
        fn writer_faults_never_become_complete_and_finite_finish_keeps_worker_ownership() {
            for fail_write in [true, false] {
                let source = Source::default();
                let begin = source.status();
                source.enable(true);
                let mut sink = FaultStorage {
                    fail_write,
                    fail_sync: !fail_write,
                    release: None,
                    dropped: Arc::new(AtomicBool::new(false)),
                };
                assert!(
                    write_capture(&source, &mut sink, begin, 0, 65536, &AtomicBool::new(true))
                        .is_err()
                );
                assert!(!source.enabled());
            }
            let source = Source::default();
            let begin = source.status();
            source.enable(true);
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let (release, wait) = std::sync::mpsc::sync_channel(1);
            let (send, done) = std::sync::mpsc::sync_channel(1);
            let dropped = Arc::new(AtomicBool::new(false));
            let worker_dropped = dropped.clone();
            let worker = std::thread::spawn(move || {
                let mut sink = FaultStorage {
                    fail_write: false,
                    fail_sync: false,
                    release: Some(wait),
                    dropped: worker_dropped,
                };
                let result = write_capture(&source, &mut sink, begin, 0, 65536, &stopping);
                let _ = send.send(result);
            });
            let capture = Capture {
                stop,
                done,
                worker: worker.thread().clone(),
            };
            let started = Instant::now();
            assert!(capture.finish().unwrap_err().contains("observation finish"));
            assert!(started.elapsed() < std::time::Duration::from_secs(3));
            assert!(!dropped.load(Ordering::SeqCst));
            release.send(()).unwrap();
            worker.join().unwrap();
            assert!(dropped.load(Ordering::SeqCst));
        }
        #[test]
        fn concurrent_producers_preserve_accepted_sequence_order_and_explicit_loss() {
            let source = Source::default();
            source.enable(true);
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    let s = source.clone();
                    std::thread::spawn(move || {
                        for _ in 0..1000 {
                            s.record(Component::Api, Phase::Received, 0, 0, 0, 0);
                        }
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
            assert!(source.stop_producers(std::time::Duration::from_millis(250)));
            let rows = source.drain();
            assert!(rows
                .windows(2)
                .all(|w| w[0].sequence < w[1].sequence && w[0].us <= w[1].us));
            assert_eq!(source.status().sequence, 4000);
            assert_eq!(rows.len() as u64 + source.status().dropped, 4000);
        }
        #[test]
        fn saturation_and_contention_do_not_block_or_hide_gaps() {
            let s = Source::default();
            assert_eq!(s.record(Component::Api, Phase::Received, 0, 0, 0, 0), 0);
            s.enable(true);
            for _ in 0..CAPACITY + 1 {
                s.record(Component::Api, Phase::Received, 0, 0, 0, 0);
            }
            assert_eq!(s.status().dropped, 1);
            assert_eq!(s.drain().len(), CAPACITY);
            let lock = s.0.queue.lock().unwrap();
            s.record(Component::Api, Phase::Received, 0, 0, 0, 0);
            drop(lock);
            assert_eq!(s.status().dropped, 2);
            let last = s.record(Component::Api, Phase::Completed, 7, 3, 9, 4);
            let row = s.drain().pop().unwrap();
            assert_eq!(row.sequence, last);
            assert_eq!(row.parent, 3);
            s.enable(false);
            assert_eq!(s.status().sequence, last);
        }
    }
}
#[cfg(feature = "boundary-observation")]
pub use enabled::{Capture, Source};
#[cfg(not(feature = "boundary-observation"))]
#[derive(Clone, Default)]
pub struct Source;
#[cfg(not(feature = "boundary-observation"))]
impl Source {
    pub fn enable(&self, _on: bool) {}
    pub fn enabled(&self) -> bool {
        false
    }
    pub fn record(&self, _c: Component, _p: Phase, _o: u64, _r: u64, _v: u64, _a: u64) -> u64 {
        0
    }
    pub fn record_context(
        &self,
        _c: Component,
        _p: Phase,
        _o: u64,
        _r: u64,
        _v: u64,
        _a: u64,
        _f: u64,
        _cause: Option<Cause>,
    ) -> u64 {
        0
    }
    pub fn stop_producers(&self, _timeout: std::time::Duration) -> bool {
        true
    }
    pub fn drain(&self) -> Vec<Fact> {
        Vec::new()
    }
    pub fn status(&self) -> Status {
        Status::default()
    }
}
