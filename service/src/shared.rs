//! One mutable application owner, with independent immutable published queries.
//! The handoff mutex never covers application work or durable storage.
use crate::application::{App, AppError};
use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, RwLock, RwLockWriteGuard,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct Signal(Arc<SignalInner>);
struct SignalInner {
    sender: tokio::sync::watch::Sender<()>,
    deferred: AtomicBool,
    pending: AtomicBool,
}
impl Signal {
    pub fn new() -> Self {
        Self(Arc::new(SignalInner {
            sender: tokio::sync::watch::channel(()).0,
            deferred: AtomicBool::new(false),
            pending: AtomicBool::new(false),
        }))
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.0.sender.subscribe()
    }
    pub fn notify_published(&self) {
        self.0.sender.send_replace(());
    }
    pub fn send_replace(&self, _: ()) {
        if self.0.deferred.load(Ordering::SeqCst) {
            self.0.pending.store(true, Ordering::SeqCst);
        } else {
            self.0.sender.send_replace(());
        }
    }
    pub fn defer(&self) {
        self.0.deferred.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        self.0.deferred.store(false, Ordering::SeqCst);
        if self.0.pending.swap(false, Ordering::SeqCst) {
            self.0.sender.send_replace(());
        }
    }
}
pub(crate) fn blocking<T>(action: impl FnOnce() -> T) -> T {
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(action)
    } else {
        action()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    Busy,
    Poisoned,
}
impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "application access: {self:?}")
    }
}
impl std::error::Error for AccessError {}
struct Slot {
    app: Option<App>,
    waiters: usize,
}
pub struct Application {
    slot: Mutex<Slot>,
    ready: Condvar,
    published: RwLock<Arc<App>>,
    poisoned: AtomicBool,
}
impl Application {
    pub fn new(app: App) -> Self {
        let published = Arc::new(app.published_snapshot());
        Self {
            slot: Mutex::new(Slot {
                app: Some(app),
                waiters: 0,
            }),
            ready: Condvar::new(),
            published: RwLock::new(published),
            poisoned: AtomicBool::new(false),
        }
    }
    fn acquire(&self, wait: bool) -> Result<Lease<'_>, AccessError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(AccessError::Poisoned);
        }
        let mut slot = self.slot.lock().map_err(|_| AccessError::Poisoned)?;
        if slot.app.is_none() {
            if !wait || slot.waiters >= 64 {
                return Err(AccessError::Busy);
            }
            slot.waiters += 1;
            while slot.app.is_none() && !self.poisoned.load(Ordering::Acquire) {
                slot = self.ready.wait(slot).map_err(|_| AccessError::Poisoned)?;
            }
            slot.waiters -= 1;
        }
        if self.poisoned.load(Ordering::Acquire) {
            return Err(AccessError::Poisoned);
        }
        let mut app = slot.app.take().ok_or(AccessError::Busy)?;
        drop(slot);
        let base = app.published_snapshot();
        let current = match self.read() {
            Ok(view) => view,
            Err(error) => {
                self.poisoned.store(true, Ordering::Release);
                self.ready.notify_all();
                return Err(error);
            }
        };
        if app.merge_transient_since(&base, &current).is_err() {
            self.poisoned.store(true, Ordering::Release);
            self.ready.notify_all();
            return Err(AccessError::Poisoned);
        }
        let base = Arc::new(app.published_snapshot());
        app.defer_publication_signals();
        Ok(Lease {
            owner: self,
            app: Some(app),
            modified: false,
            base,
        })
    }
    pub fn try_lock(&self) -> Result<Lease<'_>, AccessError> {
        self.acquire(false)
    }
    pub fn lock(&self) -> Result<Lease<'_>, AccessError> {
        match self.acquire(false) {
            Err(AccessError::Busy) => blocking(|| self.acquire(true)),
            result => result,
        }
    }
    #[cfg(test)]
    pub(crate) fn poison_publication_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.published.write().unwrap();
            panic!("fixture publication unavailable");
        }));
    }
    pub fn read(&self) -> Result<Arc<App>, AccessError> {
        if self.poisoned.load(Ordering::Acquire) || self.slot.is_poisoned() {
            return Err(AccessError::Poisoned);
        }
        self.published
            .read()
            .map(|view| view.clone())
            .map_err(|_| AccessError::Poisoned)
    }
    pub(crate) fn input_lock(
        &self,
        token: &crate::SessionToken,
        message: &crate::protocol::WireMessage,
    ) -> Result<InputGuard<'_>, AccessError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(AccessError::Poisoned);
        }
        let published = self.published.write().map_err(|_| AccessError::Poisoned)?;
        if let Some(app) = published.transient_button_snapshot(token, message) {
            return Ok(InputGuard::Transient(TransientLease {
                owner: self,
                published,
                app: Some(app),
                modified: false,
            }));
        }
        drop(published);
        self.lock().map(InputGuard::Durable)
    }
    pub(crate) fn complete_delivery_effect(&self, id: u64) -> Result<(bool, Duration), AppError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(AppError::Conflict("application unavailable"));
        }
        let waiting = Instant::now();
        let published = self
            .published
            .write()
            .map_err(|_| AppError::Conflict("application unavailable"))?;
        let wait = waiting.elapsed();
        let app = published.transient_snapshot();
        let mut guard = TransientLease {
            owner: self,
            published,
            app: Some(app),
            modified: false,
        };
        guard
            .complete_transient_effect(id)
            .map(|changed| (changed, wait))
    }
    pub(crate) fn refresh_timed_frames(&self) -> Result<usize, AppError> {
        if !self
            .read()
            .map_err(|_| AppError::Conflict("application unavailable"))?
            .timed_refresh_due()
        {
            return Ok(0);
        }
        let published = self
            .published
            .write()
            .map_err(|_| AppError::Conflict("application unavailable"))?;
        let app = published.transient_snapshot();
        let mut guard = TransientLease {
            owner: self,
            published,
            app: Some(app),
            modified: false,
        };
        guard.refresh_timed_frames()
    }
    pub(crate) fn accept_playback_pixels_at(
        &self,
        offer: &crate::application::PlaybackOffer,
        pixels: Vec<u8>,
        sequence: u64,
    ) -> Result<usize, AppError> {
        if pixels.len() != crate::protocol::LCD_FRAME_BYTES {
            return Err(AppError::Invalid("invalid playback frame"));
        }
        let mut delivered = 0;
        let mut legacy = offer.clone();
        legacy.targets.clear();
        {
            let view = self
                .published
                .read()
                .map_err(|_| AppError::Conflict("application unavailable"))?;
            if self.poisoned.load(Ordering::Acquire) {
                return Err(AppError::Conflict("application unavailable"));
            }
            for target in &offer.targets {
                let Some(ctx) = view.playback_target_context(offer, target) else {
                    continue;
                };
                if view.supports_lcd_push(&ctx.token)
                    && offer.spec.fps <= crate::lcd_stream::MAX_FPS
                {
                    view.queue_published_lcd_video(
                        &ctx.token,
                        offer.spec.fps,
                        sequence,
                        offer.spec.generation,
                        pixels.clone(),
                    )?;
                    delivered += 1;
                } else {
                    legacy.targets.push(target.clone());
                }
            }
        }
        if !legacy.targets.is_empty() {
            if self.poisoned.load(Ordering::Acquire) {
                return Err(AppError::Conflict("application unavailable"));
            }
            let published = self
                .published
                .write()
                .map_err(|_| AppError::Conflict("application unavailable"))?;
            let app = published.transient_snapshot();
            let mut guard = TransientLease {
                owner: self,
                published,
                app: Some(app),
                modified: false,
            };
            delivered += guard.accept_playback_pixels_at(&legacy, pixels, sequence)?;
        }
        Ok(delivered)
    }
}
pub struct Lease<'a> {
    owner: &'a Application,
    app: Option<App>,
    modified: bool,
    base: Arc<App>,
}
impl Lease<'_> {
    pub(crate) fn reserve_maintenance(&mut self, plugin: &str) -> Result<(), AppError> {
        let mut published = self
            .owner
            .published
            .write()
            .map_err(|_| AppError::Conflict("application publication unavailable"))?;
        let app = self.app.as_mut().expect("application lease");
        app.merge_transient_since(&self.base, &published)?;
        app.begin_maintenance_scope(plugin)?;
        let snapshot = Arc::new(app.published_snapshot());
        *published = snapshot.clone();
        self.base = snapshot;
        self.modified = true;
        Ok(())
    }
    /// Publish a successful commit prefix before effects, retaining the owner
    /// to keep reference-changing durable operations ordered through finish.
    pub(crate) fn publish_now(&mut self) -> Result<(), AppError> {
        let result = (|| {
            let mut published = self
                .owner
                .published
                .write()
                .map_err(|_| AppError::Conflict("application publication unavailable"))?;
            let app = self.app.as_mut().expect("application lease");
            app.merge_transient_since(&self.base, &published)?;
            app.reconcile_video_slots();
            let snapshot = Arc::new(app.published_snapshot());
            *published = snapshot.clone();
            self.base = snapshot;
            self.modified = false;
            Ok(())
        })();
        if result.is_err() {
            self.owner.poisoned.store(true, Ordering::Release);
        } else {
            let app = self.app.as_ref().expect("application lease");
            app.resume_publication_signals();
            app.defer_publication_signals();
        }
        result
    }
    pub(crate) fn release_maintenance(&mut self) {
        self.app
            .as_mut()
            .expect("application lease")
            .end_maintenance_scope();
        self.modified = true;
    }
}
impl Deref for Lease<'_> {
    type Target = App;
    fn deref(&self) -> &App {
        self.app.as_ref().expect("application lease")
    }
}
impl DerefMut for Lease<'_> {
    fn deref_mut(&mut self) -> &mut App {
        self.modified = true;
        self.app.as_mut().expect("application lease")
    }
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let Some(mut app) = self.app.take() else {
            return;
        };
        if std::thread::panicking() {
            self.owner.poisoned.store(true, Ordering::Release);
        }
        if !self.owner.poisoned.load(Ordering::Acquire) {
            blocking(|| {
                if let Ok(mut published) = self.owner.published.write() {
                    if app.merge_transient_since(&self.base, &published).is_ok() {
                        app.reconcile_video_slots();
                        if self.modified {
                            *published = Arc::new(app.published_snapshot());
                        }
                    } else {
                        self.owner.poisoned.store(true, Ordering::Release);
                    }
                } else {
                    self.owner.poisoned.store(true, Ordering::Release);
                }
            });
        }
        // A later panic cannot erase the notification of a successful commit
        // prefix. Readers observe either its coherent publication or unavailable.
        app.resume_publication_signals();
        if let Ok(mut slot) = self.owner.slot.lock() {
            slot.app = Some(app);
            self.owner.ready.notify_all();
        }
    }
}

pub(crate) enum InputGuard<'a> {
    Durable(Lease<'a>),
    Transient(TransientLease<'a>),
}
impl Deref for InputGuard<'_> {
    type Target = App;
    fn deref(&self) -> &App {
        match self {
            Self::Durable(g) => g,
            Self::Transient(g) => g,
        }
    }
}
impl DerefMut for InputGuard<'_> {
    fn deref_mut(&mut self) -> &mut App {
        match self {
            Self::Durable(g) => g,
            Self::Transient(g) => g,
        }
    }
}
pub(crate) struct TransientLease<'a> {
    owner: &'a Application,
    published: RwLockWriteGuard<'a, Arc<App>>,
    app: Option<App>,
    modified: bool,
}
impl Deref for TransientLease<'_> {
    type Target = App;
    fn deref(&self) -> &App {
        self.app.as_ref().expect("transient application")
    }
}
impl DerefMut for TransientLease<'_> {
    fn deref_mut(&mut self) -> &mut App {
        self.modified = true;
        self.app.as_mut().expect("transient application")
    }
}
impl Drop for TransientLease<'_> {
    fn drop(&mut self) {
        let Some(mut app) = self.app.take() else {
            return;
        };
        if std::thread::panicking() {
            self.owner.poisoned.store(true, Ordering::Release);
            return;
        }
        if self.modified && app.transient_was_changed() {
            app.finish_transient_snapshot(&self.published);
            *self.published = Arc::new(app);
            self.published.notify_published();
        }
    }
}
