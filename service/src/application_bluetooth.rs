// BT-AUDIO-20261107: desired-only persistence; transient commands never survive restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BluetoothSaved {
    incarnation: u64,
    revision: u64,
    desired: crate::bluetooth::DesiredBluetooth,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct BluetoothLive {
    incarnation: u64,
    session: Option<u64>,
    supported: bool,
    model: crate::bluetooth::BluetoothDevice,
    queued: Option<crate::bluetooth::Effect>,
}
#[derive(Clone, Debug, Serialize)]
pub struct BluetoothView {
    pub revision: u64,
    pub desired: crate::bluetooth::DesiredBluetooth,
    pub observed: crate::bluetooth::Observed,
    pub candidates: Vec<crate::bluetooth::Candidate>,
    pub pending: Option<crate::bluetooth::CommandToken>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BluetoothAction {
    SetEnabled {
        enabled: bool,
    },
    Discover {},
    Pair {
        discovery: crate::bluetooth::CommandToken,
        target: crate::bluetooth::BluetoothAddress,
    },
}
fn bluetooth_error(error: crate::bluetooth::Error) -> AppError {
    use crate::bluetooth::Error::*;
    match error {
        InvalidInput => AppError::Invalid("invalid Bluetooth input"),
        Offline => AppError::Conflict("device offline"),
        Unsupported => AppError::Conflict("Bluetooth firmware update required"),
        Disabled => AppError::Conflict("Bluetooth disabled"),
        Stale => AppError::Conflict("stale Bluetooth operation"),
        Expired => AppError::Conflict("Bluetooth operation expired"),
        Capacity => AppError::Conflict("Bluetooth candidate limit"),
        Exhausted => AppError::Conflict("Bluetooth counter exhausted"),
    }
}
impl Runtime {
    fn bluetooth_saved(&self, id: &str, incarnation: u64) -> BluetoothSaved {
        self.bluetooth_desired
            .get(id)
            .filter(|r| r.incarnation == incarnation)
            .cloned()
            .unwrap_or(BluetoothSaved {
                incarnation,
                revision: 1,
                desired: Default::default(),
            })
    }
    fn sync_bluetooth(&mut self, id: &str) -> Result<&mut BluetoothLive, AppError> {
        let d = self.world.devices.get(id).ok_or(AppError::NotFound)?;
        let incarnation = d.incarnation;
        let session = d.online.then_some(d.session_epoch);
        let supported = d.capabilities.contains("bluetooth-audio-v1");
        let saved = self.bluetooth_saved(id, incarnation);
        let live = self
            .bluetooth_live
            .entry(id.to_owned())
            .or_insert_with(|| BluetoothLive {
                incarnation,
                session: None,
                supported: false,
                model: crate::bluetooth::BluetoothDevice::new(saved.desired.clone()),
                queued: None,
            });
        if live.incarnation != incarnation {
            *live = BluetoothLive {
                incarnation,
                session: None,
                supported: false,
                model: crate::bluetooth::BluetoothDevice::new(saved.desired),
                queued: None,
            };
        }
        if live.session != session || live.supported != supported {
            live.model
                .bind(session, supported)
                .map_err(bluetooth_error)?;
            live.session = session;
            live.supported = supported;
            live.queued = None;
        }
        Ok(live)
    }
}
impl App {
    fn bluetooth_now(&self) -> u64 {
        self.mono_origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub fn bluetooth_supported(&self, session: &SessionToken) -> bool {
        self.runtime.world.devices.get(&session.device_id).is_some_and(|d| session_current(d,session)&&d.capabilities.contains("bluetooth-audio-v1"))
    }
    pub fn bluetooth_view(&self, id: &str) -> BluetoothView {
        let Some(d) = self.runtime.world.devices.get(id) else {
            return BluetoothView {
                revision: 1,
                desired: Default::default(),
                observed: crate::bluetooth::Observed::Offline,
                candidates: vec![],
                pending: None,
            };
        };
        let saved = self.runtime.bluetooth_saved(id, d.incarnation);
        let supported = d.capabilities.contains("bluetooth-audio-v1");
        let session = d.online.then_some(d.session_epoch);
        let mut model = self
            .runtime
            .bluetooth_live
            .get(id)
            .filter(|live| {
                live.incarnation == d.incarnation
                    && live.session == session
                    && live.supported == supported
            })
            .map(|live| live.model.clone())
            .unwrap_or_else(|| {
                let mut model = crate::bluetooth::BluetoothDevice::new(saved.desired.clone());
                // A freshly constructed model has generation zero, so binding cannot overflow.
                let _ = model.bind(session, supported);
                model
            });
        model.expire(self.bluetooth_now());
        BluetoothView {
            revision: saved.revision,
            desired: saved.desired,
            observed: model.observed(),
            candidates: model.candidates().to_vec(),
            pending: model.pending(),
        }
    }

    pub fn bluetooth_command(
        &mut self,
        id: &str,
        incarnation: u64,
        expected_revision: u64,
        action: BluetoothAction,
    ) -> Result<BluetoothView, AppError> {
        let now = self.bluetooth_now();
        self.change(|_, state| {
            let d = state.world.devices.get(id).ok_or(AppError::NotFound)?;
            if d.incarnation != incarnation {
                return Err(AppError::Conflict("stale device incarnation"));
            }
            let mut saved = state.bluetooth_saved(id, incarnation);
            if saved.revision != expected_revision {
                return Err(AppError::Conflict("stale Bluetooth revision"));
            }
            let live = state.sync_bluetooth(id)?;
            live.model.expire(now);
            let effect = match action {
                BluetoothAction::SetEnabled { enabled } => live.model.set_enabled(enabled, now),
                BluetoothAction::Discover {} => live.model.discover(now).map(Some),
                BluetoothAction::Pair { discovery, target } => {
                    live.model.pair(discovery, now, &target).map(Some)
                }
            }
            .map_err(bluetooth_error)?;
            live.queued = effect;
            saved.desired = live.model.desired().clone();
            saved.revision = saved
                .revision
                .checked_add(1)
                .ok_or(AppError::Conflict("Bluetooth revision exhausted"))?;
            state.bluetooth_desired.insert(id.to_owned(), saved);
            Ok(())
        })?;
        Ok(self.bluetooth_view(id))
    }
    pub fn take_bluetooth_command(
        &mut self,
        session: &SessionToken,
    ) -> Option<crate::bluetooth::Effect> {
        if !self.session_is_current(session) {
            return None;
        }
        let now = self.bluetooth_now();
        let live = self.runtime.sync_bluetooth(&session.device_id).ok()?;
        if !live.supported {
            return None;
        }
        if live.model.expire(now) {
            live.queued = None;
        }
        // Only a new bind/explicit request should initiate a connection. Do not retry failure endlessly.
        if live.queued.is_none() && live.model.observed() == crate::bluetooth::Observed::Idle {
            live.queued = live.model.reconcile(now).ok().flatten();
        }
        let effect = live.queued.take();
        self.transient_changed = true;
        effect
    }
    pub fn apply_bluetooth_report(
        &mut self,
        session: &SessionToken,
        token: crate::bluetooth::CommandToken,
        report: crate::bluetooth::Report,
    ) -> Result<(), AppError> {
        if !self.session_is_current(session) {
            return Err(AppError::Conflict("stale session"));
        }
        let now = self.bluetooth_now();
        self.change(|_, state| {
            let live = state.sync_bluetooth(&session.device_id)?;
            if !live.supported {
                return Err(AppError::Conflict("Bluetooth firmware update required"));
            }
            match report {
                crate::bluetooth::Report::Candidate { candidate } => {
                    live.model.candidate(token, now, candidate)
                }
                crate::bluetooth::Report::Complete { success } => {
                    live.model.complete(token, now, success)
                }
                crate::bluetooth::Report::Disconnected => live.model.disconnected(token),
            }
            .map_err(bluetooth_error)
        })
    }
}

#[cfg(test)]
mod bluetooth_integration_tests {
    use super::*;
    fn caps() -> BTreeSet<String> {
        ["lcd".into(), "bluetooth-audio-v1".into()].into()
    }
    #[test]
    fn desired_survives_restart_but_commands_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bt.sqlite");
        let mut app = App::open(&path).unwrap();
        let t = app.hello("pico", caps(), 1).unwrap();
        app.bluetooth_command(
            "pico",
            t.incarnation,
            1,
            BluetoothAction::SetEnabled { enabled: true },
        )
        .unwrap();
        app.bluetooth_command("pico", t.incarnation, 2, BluetoothAction::Discover {})
            .unwrap();
        let old = app.bluetooth_view("pico").pending.unwrap();
        drop(app);
        let mut app = App::open(&path).unwrap();
        assert!(app.bluetooth_view("pico").desired.enabled);
        assert!(app.bluetooth_view("pico").pending.is_none());
        let t = app.hello("pico", caps(), 1).unwrap();
        assert!(app.take_bluetooth_command(&t).is_none());
        assert!(app
            .apply_bluetooth_report(
                &t,
                old,
                crate::bluetooth::Report::Complete { success: true }
            )
            .is_err());
    }
    #[test]
    fn revision_capability_and_device_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::open(&dir.path().join("bt.sqlite")).unwrap();
        let a = app.hello("a", caps(), 1).unwrap();
        let b = app.hello("b", ["lcd".into()].into(), 1).unwrap();
        app.bluetooth_command(
            "a",
            a.incarnation,
            1,
            BluetoothAction::SetEnabled { enabled: true },
        )
        .unwrap();
        assert!(app
            .bluetooth_command("a", a.incarnation, 1, BluetoothAction::Discover {})
            .is_err());
        app.bluetooth_command("a", a.incarnation, 2, BluetoothAction::Discover {})
            .unwrap();
        assert!(app.take_bluetooth_command(&a).is_some());
        assert!(app.take_bluetooth_command(&a).is_none());
        assert!(app.take_bluetooth_command(&b).is_none());
        assert_eq!(
            app.bluetooth_view("b").observed,
            crate::bluetooth::Observed::Unsupported
        );
        assert!(!app.bluetooth_view("b").desired.enabled);
    }
    #[test]
    fn failed_persistence_does_not_publish_or_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::open(&dir.path().join("bt.sqlite")).unwrap();
        let t = app.hello("a", caps(), 1).unwrap();
        app.pause_next_persistence(Box::new(|| {
            Err(AppError::Conflict("injected storage failure"))
        }));
        assert!(app
            .bluetooth_command(
                "a",
                t.incarnation,
                1,
                BluetoothAction::SetEnabled { enabled: true }
            )
            .is_err());
        assert!(!app.bluetooth_view("a").desired.enabled);
        assert_eq!(app.bluetooth_view("a").revision, 1);
        assert!(app.take_bluetooth_command(&t).is_none());
    }
    #[test]
    fn concurrent_consumption_is_not_republished() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::open(&dir.path().join("bt.sqlite")).unwrap();
        let t = app.hello("a", caps(), 1).unwrap();
        app.bluetooth_command(
            "a",
            t.incarnation,
            1,
            BluetoothAction::SetEnabled { enabled: true },
        )
        .unwrap();
        app.bluetooth_command("a", t.incarnation, 2, BluetoothAction::Discover {})
            .unwrap();
        let base = app.published_snapshot();
        let mut next = base.published_snapshot();
        assert!(app.take_bluetooth_command(&t).is_some());
        next.merge_transient_since(&base, &app).unwrap();
        assert!(next.take_bluetooth_command(&t).is_none());
    }
}
