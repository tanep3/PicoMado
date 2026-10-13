//! Pure settings and Pico synchronization contract. Durations have explicit units.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::DeviceId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
    Invalid(&'static str),
    UnknownDevice(DeviceId),
    StaleRevision { expected: u64, actual: u64 },
    RevisionConflict(u64),
    FutureRevision(u64),
    StaleIncarnation { received: u64, current: u64 },
    StaleSnapshot { incarnation: u64, revision: u64 },
    NoMatchingWrite { incarnation: u64, revision: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub revision: u64,
    pub value: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    pub incarnation: u64,
    pub revision: u64,
    pub value: DeviceSettings,
}

impl DeviceSnapshot {
    fn key(&self) -> (u64, u64) {
        (self.incarnation, self.revision)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSettings {
    pub wifi_timeout_seconds: u32,
    pub app_timeout_seconds: u32,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub heartbeat_seconds: u32,
    pub offline_seconds: u32,
    pub two_button_long_press_millis: u32,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            wifi_timeout_seconds: 15,
            app_timeout_seconds: 10,
            retry_initial_seconds: 5,
            retry_max_seconds: 30,
            heartbeat_seconds: 15,
            offline_seconds: 45,
            two_button_long_press_millis: 1_800,
        }
    }
}

impl DeviceSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(5..=120).contains(&self.wifi_timeout_seconds) {
            return Err(SettingsError::Invalid("wifi_timeout_seconds"));
        }
        if !(3..=120).contains(&self.app_timeout_seconds) {
            return Err(SettingsError::Invalid("app_timeout_seconds"));
        }
        if !(1..=60).contains(&self.retry_initial_seconds) {
            return Err(SettingsError::Invalid("retry_initial_seconds"));
        }
        if !(1..=300).contains(&self.retry_max_seconds)
            || self.retry_max_seconds < self.retry_initial_seconds
        {
            return Err(SettingsError::Invalid("retry_max_seconds"));
        }
        if !(5..=120).contains(&self.heartbeat_seconds) {
            return Err(SettingsError::Invalid("heartbeat_seconds"));
        }
        if self.offline_seconds > 600
            || self.offline_seconds < self.heartbeat_seconds.saturating_mul(2)
        {
            return Err(SettingsError::Invalid("offline_seconds"));
        }
        if !(1_500..=2_000).contains(&self.two_button_long_press_millis) {
            return Err(SettingsError::Invalid("two_button_long_press_millis"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchPolicy {
    pub timeout_seconds: u32,
    /// Additional tries after the first request.
    pub retries: u8,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            timeout_seconds: 20,
            retries: 2,
        }
    }
}

impl FetchPolicy {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !(5..=120).contains(&self.timeout_seconds) {
            return Err(SettingsError::Invalid("jma_timeout_seconds"));
        }
        if self.retries > 5 {
            return Err(SettingsError::Invalid("jma_retries"));
        }
        Ok(())
    }
}

pub const YOUTUBE_FPS_CHOICES: [u8; 5] = [6, 12, 18, 24, 30];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettings {
    pub weather_fetch: FetchPolicy,
    pub radar_fetch: FetchPolicy,
    pub youtube_default_fps: u8,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            weather_fetch: FetchPolicy::default(),
            radar_fetch: FetchPolicy::default(),
            youtube_default_fps: 6,
        }
    }
}

impl PluginSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        self.weather_fetch.validate()?;
        self.radar_fetch.validate()?;
        if !YOUTUBE_FPS_CHOICES.contains(&self.youtube_default_fps) {
            return Err(SettingsError::Invalid("youtube_default_fps"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub desired: DeviceSnapshot,
    /// Last configuration confirmed by a heartbeat, not merely sent to Pico.
    pub confirmed: DeviceSnapshot,
    pub online: bool,
    /// Last NVM persistence acknowledgement, before applied heartbeat.
    #[serde(default)]
    pub stored_revision: u64,
    conservative_offline_seconds: u32,
}

impl DeviceRecord {
    pub fn new(incarnation: u64) -> Self {
        let factory = DeviceSettings::default();
        Self {
            desired: DeviceSnapshot {
                incarnation,
                revision: 1,
                value: factory.clone(),
            },
            confirmed: DeviceSnapshot {
                incarnation,
                revision: 0,
                value: factory.clone(),
            },
            online: false,
            stored_revision: 0,
            conservative_offline_seconds: factory.offline_seconds,
        }
    }

    pub fn pending(&self) -> bool {
        self.desired.revision != self.confirmed.revision
    }

    pub fn liveness_timeout_seconds(&self) -> u32 {
        self.conservative_offline_seconds
    }

    pub fn mark_stored(&mut self, incarnation: u64, revision: u64) -> Result<(), SettingsError> {
        if incarnation != self.desired.incarnation {
            return Err(SettingsError::StaleIncarnation {
                received: incarnation,
                current: self.desired.incarnation,
            });
        }
        if revision > self.desired.revision {
            return Err(SettingsError::FutureRevision(revision));
        }
        self.stored_revision = self.stored_revision.max(revision);
        Ok(())
    }

    pub fn request_update(
        &mut self,
        expected_incarnation: u64,
        expected_revision: u64,
        desired: DeviceSettings,
    ) -> Result<u64, SettingsError> {
        desired.validate()?;
        if expected_incarnation != self.desired.incarnation {
            return Err(SettingsError::StaleIncarnation {
                received: expected_incarnation,
                current: self.desired.incarnation,
            });
        }
        if expected_revision != self.desired.revision {
            return Err(SettingsError::StaleRevision {
                expected: expected_revision,
                actual: self.desired.revision,
            });
        }
        if desired == self.desired.value {
            return Ok(self.desired.revision);
        }
        self.desired.revision += 1;
        self.conservative_offline_seconds = self
            .conservative_offline_seconds
            .max(desired.offline_seconds);
        self.desired.value = desired;
        Ok(self.desired.revision)
    }

    /// A heartbeat carrying the current desired revision confirms rollout.
    /// Older heartbeats leave the conservative liveness window in place.
    pub fn heartbeat(
        &mut self,
        incarnation: u64,
        applied_revision: u64,
    ) -> Result<(), SettingsError> {
        if incarnation != self.desired.incarnation {
            return Err(SettingsError::StaleIncarnation {
                received: incarnation,
                current: self.desired.incarnation,
            });
        }
        if applied_revision > self.desired.revision {
            return Err(SettingsError::FutureRevision(applied_revision));
        }
        self.online = true;
        if applied_revision == self.desired.revision {
            self.confirmed = self.desired.clone();
            self.conservative_offline_seconds = self.desired.value.offline_seconds;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsStore {
    pub next_incarnation: u64,
    pub devices: BTreeMap<DeviceId, DeviceRecord>,
    pub plugins: Versioned<PluginSettings>,
}

impl SettingsStore {
    pub fn new() -> Self {
        Self {
            next_incarnation: 1,
            devices: BTreeMap::new(),
            plugins: Versioned {
                revision: 1,
                value: PluginSettings::default(),
            },
        }
    }

    pub fn register_device(&mut self, id: DeviceId) -> &DeviceRecord {
        if !self.devices.contains_key(&id) {
            let incarnation = self.next_incarnation;
            self.next_incarnation += 1;
            self.devices
                .insert(id.clone(), DeviceRecord::new(incarnation));
        }
        self.devices.get(&id).expect("just registered")
    }

    pub fn delete_device(&mut self, id: &str) -> Option<DeviceRecord> {
        self.devices.remove(id)
    }

    pub fn update_device(
        &mut self,
        id: &str,
        expected_incarnation: u64,
        expected_revision: u64,
        desired: DeviceSettings,
    ) -> Result<u64, SettingsError> {
        self.devices
            .get_mut(id)
            .ok_or_else(|| SettingsError::UnknownDevice(id.into()))?
            .request_update(expected_incarnation, expected_revision, desired)
    }

    pub fn update_plugins(
        &mut self,
        expected_revision: u64,
        desired: PluginSettings,
    ) -> Result<u64, SettingsError> {
        desired.validate()?;
        if expected_revision != self.plugins.revision {
            return Err(SettingsError::StaleRevision {
                expected: expected_revision,
                actual: self.plugins.revision,
            });
        }
        if desired == self.plugins.value {
            return Ok(self.plugins.revision);
        }
        self.plugins.revision += 1;
        self.plugins.value = desired;
        Ok(self.plugins.revision)
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootSource {
    Nvm,
    FactoryFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncAction {
    PersistNvm(DeviceSnapshot),
    AckApplied { incarnation: u64, revision: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicoSync {
    pub active: DeviceSnapshot,
    pub persisted: Option<DeviceSnapshot>,
    pub pending: Option<DeviceSnapshot>,
    pub writing: Option<DeviceSnapshot>,
    pub attempt_active: bool,
    pub button_pressed: bool,
    highest_received: (u64, u64),
}

impl PicoSync {
    /// Boot selects a validated NVM snapshot before any network attempt.
    pub fn boot(nvm: Option<DeviceSnapshot>) -> (Self, BootSource) {
        let valid = nvm.filter(|record| record.value.validate().is_ok());
        let source = if valid.is_some() {
            BootSource::Nvm
        } else {
            BootSource::FactoryFallback
        };
        let active = valid.clone().unwrap_or_else(|| DeviceSnapshot {
            incarnation: 0,
            revision: 0,
            value: DeviceSettings::default(),
        });
        let highest_received = active.key();
        (
            Self {
                active,
                persisted: valid,
                pending: None,
                writing: None,
                attempt_active: false,
                button_pressed: false,
                highest_received,
            },
            source,
        )
    }

    pub fn busy(&self) -> bool {
        self.attempt_active || self.button_pressed
    }

    /// A duplicate desired revision is harmless; conflicting contents are rejected.
    pub fn receive(
        &mut self,
        desired: DeviceSnapshot,
    ) -> Result<Option<SyncAction>, SettingsError> {
        desired.value.validate()?;
        if desired.key() < self.highest_received {
            return Err(SettingsError::StaleSnapshot {
                incarnation: desired.incarnation,
                revision: desired.revision,
            });
        }
        for known in [
            Some(&self.active),
            self.persisted.as_ref(),
            self.pending.as_ref(),
            self.writing.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if known.key() == desired.key() {
                if known.value != desired.value {
                    return Err(SettingsError::RevisionConflict(desired.revision));
                }
                return if self.active.key() == desired.key() {
                    Ok(Some(SyncAction::AckApplied {
                        incarnation: desired.incarnation,
                        revision: desired.revision,
                    }))
                } else {
                    Ok(None)
                };
            }
        }
        self.highest_received = desired.key();
        self.pending = Some(desired);
        Ok(None)
    }

    /// The caller controls attempt and button state. This never disconnects a session.
    pub fn advance(&mut self) -> Option<SyncAction> {
        if self.busy() {
            return None;
        }
        if let Some(persisted) = &self.persisted {
            if persisted.key() == self.highest_received && persisted.key() > self.active.key() {
                self.active = persisted.clone();
                return Some(SyncAction::AckApplied {
                    incarnation: self.active.incarnation,
                    revision: self.active.revision,
                });
            }
        }
        if self.writing.is_none() {
            if let Some(pending) = self.pending.take() {
                self.writing = Some(pending.clone());
                return Some(SyncAction::PersistNvm(pending));
            }
        }
        None
    }

    pub fn persist_succeeded(
        &mut self,
        incarnation: u64,
        revision: u64,
    ) -> Result<Option<SyncAction>, SettingsError> {
        let writing = self.writing.take().ok_or(SettingsError::NoMatchingWrite {
            incarnation,
            revision,
        })?;
        if writing.key() != (incarnation, revision) {
            self.writing = Some(writing);
            return Err(SettingsError::NoMatchingWrite {
                incarnation,
                revision,
            });
        }
        self.persisted = Some(writing.clone());
        if self.busy() || writing.key() != self.highest_received {
            return Ok(None);
        }
        self.active = writing;
        Ok(Some(SyncAction::AckApplied {
            incarnation,
            revision,
        }))
    }

    pub fn persist_failed(&mut self, incarnation: u64, revision: u64) -> Result<(), SettingsError> {
        let writing = self.writing.take().ok_or(SettingsError::NoMatchingWrite {
            incarnation,
            revision,
        })?;
        if writing.key() != (incarnation, revision) {
            self.writing = Some(writing);
            return Err(SettingsError::NoMatchingWrite {
                incarnation,
                revision,
            });
        }
        if self
            .pending
            .as_ref()
            .is_none_or(|x| x.key() < writing.key())
        {
            self.pending = Some(writing);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_defaults_are_exact_and_valid() {
        let device = DeviceSettings::default();
        assert_eq!(
            (device.wifi_timeout_seconds, device.app_timeout_seconds),
            (15, 10)
        );
        assert_eq!(
            (device.retry_initial_seconds, device.retry_max_seconds),
            (5, 30)
        );
        assert_eq!((device.heartbeat_seconds, device.offline_seconds), (15, 45));
        assert_eq!(device.two_button_long_press_millis, 1_800);
        assert!(device.validate().is_ok());
        let plugins = PluginSettings::default();
        assert_eq!(
            plugins.weather_fetch,
            FetchPolicy {
                timeout_seconds: 20,
                retries: 2
            }
        );
        assert_eq!(
            plugins.radar_fetch,
            FetchPolicy {
                timeout_seconds: 20,
                retries: 2
            }
        );
        assert_eq!(plugins.youtube_default_fps, 6);
        assert_eq!(YOUTUBE_FPS_CHOICES, [6, 12, 18, 24, 30]);
        assert!(plugins.validate().is_ok());
    }

    #[test]
    fn device_validation_rejects_invalid_bounds_and_relations() {
        let value = DeviceSettings {
            wifi_timeout_seconds: 4,
            ..DeviceSettings::default()
        };
        assert_eq!(
            value.validate(),
            Err(SettingsError::Invalid("wifi_timeout_seconds"))
        );
        let value = DeviceSettings {
            app_timeout_seconds: 2,
            ..DeviceSettings::default()
        };
        assert_eq!(
            value.validate(),
            Err(SettingsError::Invalid("app_timeout_seconds"))
        );
        let value = DeviceSettings {
            retry_initial_seconds: 40,
            retry_max_seconds: 30,
            ..DeviceSettings::default()
        };
        assert_eq!(
            value.validate(),
            Err(SettingsError::Invalid("retry_max_seconds"))
        );
        let value = DeviceSettings {
            offline_seconds: 29,
            ..DeviceSettings::default()
        };
        assert_eq!(
            value.validate(),
            Err(SettingsError::Invalid("offline_seconds"))
        );
        let value = DeviceSettings {
            two_button_long_press_millis: 2_001,
            ..DeviceSettings::default()
        };
        assert_eq!(
            value.validate(),
            Err(SettingsError::Invalid("two_button_long_press_millis"))
        );
    }

    #[test]
    fn plugin_global_validation_and_revision_are_separate_from_device() {
        let mut store = SettingsStore::new();
        store.register_device("one".into());
        let mut desired = store.plugins.value.clone();
        desired.weather_fetch.timeout_seconds = 30;
        desired.youtube_default_fps = 12;
        assert_eq!(store.update_plugins(1, desired.clone()), Ok(2));
        assert_eq!(store.devices["one"].desired.revision, 1);
        assert_eq!(
            store.update_plugins(1, desired),
            Err(SettingsError::StaleRevision {
                expected: 1,
                actual: 2
            })
        );
        let mut bad = store.plugins.value.clone();
        bad.radar_fetch.retries = 6;
        assert_eq!(
            store.update_plugins(2, bad),
            Err(SettingsError::Invalid("jma_retries"))
        );
        let mut bad = store.plugins.value.clone();
        bad.youtube_default_fps = 10;
        assert_eq!(
            store.update_plugins(2, bad),
            Err(SettingsError::Invalid("youtube_default_fps"))
        );
        assert_eq!(store.plugins.revision, 2);
    }

    #[test]
    fn offline_web_update_remains_desired_until_matching_heartbeat() {
        let mut store = SettingsStore::new();
        store.register_device("one".into());
        let desired = DeviceSettings {
            wifi_timeout_seconds: 20,
            ..DeviceSettings::default()
        };
        assert_eq!(store.update_device("one", 1, 1, desired), Ok(2));
        let record = &store.devices["one"];
        assert!(!record.online);
        assert!(record.pending());
        assert_eq!(record.confirmed.revision, 0);
        store
            .devices
            .get_mut("one")
            .unwrap()
            .heartbeat(1, 1)
            .unwrap();
        assert!(store.devices["one"].pending());
        store
            .devices
            .get_mut("one")
            .unwrap()
            .heartbeat(1, 2)
            .unwrap();
        assert!(!store.devices["one"].pending());
        assert_eq!(
            store.devices["one"].confirmed.value.wifi_timeout_seconds,
            20
        );
    }

    #[test]
    fn stale_web_device_write_is_rejected_without_state_change() {
        let mut store = SettingsStore::new();
        store.register_device("one".into());
        let desired = DeviceSettings {
            app_timeout_seconds: 20,
            ..DeviceSettings::default()
        };
        store.update_device("one", 1, 1, desired).unwrap();
        let before = store.clone();
        assert_eq!(
            store.update_device("one", 1, 1, DeviceSettings::default()),
            Err(SettingsError::StaleRevision {
                expected: 1,
                actual: 2
            })
        );
        assert_eq!(store, before);
    }

    #[test]
    fn liveness_uses_max_of_old_and_new_until_current_heartbeat() {
        let mut record = DeviceRecord::new(1);
        record.heartbeat(1, 1).unwrap();
        let longer = DeviceSettings {
            offline_seconds: 60,
            ..DeviceSettings::default()
        };
        record.request_update(1, 1, longer).unwrap();
        assert_eq!(record.liveness_timeout_seconds(), 60);
        let shorter = DeviceSettings {
            offline_seconds: 30,
            ..DeviceSettings::default()
        };
        record.request_update(1, 2, shorter).unwrap();
        assert_eq!(record.liveness_timeout_seconds(), 60);
        record.heartbeat(1, 2).unwrap();
        assert_eq!(record.liveness_timeout_seconds(), 60);
        record.heartbeat(1, 3).unwrap();
        assert_eq!(record.liveness_timeout_seconds(), 30);
    }

    #[test]
    fn pico_boot_uses_valid_nvm_or_factory_before_network() {
        let saved = DeviceSettings {
            app_timeout_seconds: 20,
            ..DeviceSettings::default()
        };
        let nvm = DeviceSnapshot {
            incarnation: 1,
            revision: 7,
            value: saved.clone(),
        };
        let (pico, source) = PicoSync::boot(Some(nvm));
        assert_eq!(source, BootSource::Nvm);
        assert_eq!(pico.active.value, saved);
        assert_eq!(pico.active.revision, 7);
        let corrupt = DeviceSettings {
            offline_seconds: 1,
            ..DeviceSettings::default()
        };
        let (pico, source) = PicoSync::boot(Some(DeviceSnapshot {
            incarnation: 1,
            revision: 99,
            value: corrupt,
        }));
        assert_eq!(source, BootSource::FactoryFallback);
        assert_eq!(pico.active.revision, 0);
        assert_eq!(pico.active.value, DeviceSettings::default());
    }

    #[test]
    fn pico_defers_apply_during_attempt_or_press_then_acks_after_nvm_success() {
        let (mut pico, _) = PicoSync::boot(None);
        let desired = DeviceSettings {
            retry_initial_seconds: 7,
            ..DeviceSettings::default()
        };
        let snapshot = DeviceSnapshot {
            incarnation: 1,
            revision: 2,
            value: desired,
        };
        pico.attempt_active = true;
        pico.receive(snapshot.clone()).unwrap();
        assert_eq!(pico.advance(), None);
        assert_eq!(pico.active.revision, 0);
        pico.attempt_active = false;
        pico.button_pressed = true;
        assert_eq!(pico.advance(), None);
        pico.button_pressed = false;
        assert_eq!(
            pico.advance(),
            Some(SyncAction::PersistNvm(snapshot.clone()))
        );
        // A new attempt starts while the NVM write is in flight: keep old active values.
        pico.attempt_active = true;
        assert_eq!(pico.persist_succeeded(1, 2), Ok(None));
        assert_eq!(pico.active.revision, 0);
        assert_eq!(pico.persisted, Some(snapshot));
        pico.attempt_active = false;
        assert_eq!(
            pico.advance(),
            Some(SyncAction::AckApplied {
                incarnation: 1,
                revision: 2
            })
        );
        assert_eq!(pico.active.revision, 2);
    }

    #[test]
    fn pico_duplicate_revision_ack_and_conflicting_revision_rejection() {
        let (mut pico, _) = PicoSync::boot(None);
        let snapshot = DeviceSnapshot {
            incarnation: 1,
            revision: 1,
            value: DeviceSettings::default(),
        };
        pico.receive(snapshot.clone()).unwrap();
        assert_eq!(
            pico.advance(),
            Some(SyncAction::PersistNvm(snapshot.clone()))
        );
        assert_eq!(
            pico.persist_succeeded(1, 1),
            Ok(Some(SyncAction::AckApplied {
                incarnation: 1,
                revision: 1
            }))
        );
        assert_eq!(
            pico.receive(snapshot.clone()),
            Ok(Some(SyncAction::AckApplied {
                incarnation: 1,
                revision: 1
            }))
        );
        let mut conflict = snapshot;
        conflict.value.wifi_timeout_seconds = 20;
        assert_eq!(
            pico.receive(conflict),
            Err(SettingsError::RevisionConflict(1))
        );
    }

    #[test]
    fn failed_nvm_write_retries_without_ack_or_active_change() {
        let (mut pico, _) = PicoSync::boot(None);
        let snapshot = DeviceSnapshot {
            incarnation: 1,
            revision: 1,
            value: DeviceSettings::default(),
        };
        pico.receive(snapshot.clone()).unwrap();
        assert_eq!(
            pico.advance(),
            Some(SyncAction::PersistNvm(snapshot.clone()))
        );
        pico.persist_failed(1, 1).unwrap();
        assert_eq!(pico.active.revision, 0);
        assert_eq!(pico.persisted, None);
        assert_eq!(pico.advance(), Some(SyncAction::PersistNvm(snapshot)));
    }

    #[test]
    fn reregistration_resets_revision_scope_and_replaces_old_nvm() {
        let mut store = SettingsStore::new();
        let first_incarnation = store.register_device("one".into()).desired.incarnation;
        let old_nvm = DeviceSnapshot {
            incarnation: first_incarnation,
            revision: 7,
            value: DeviceSettings {
                wifi_timeout_seconds: 25,
                ..DeviceSettings::default()
            },
        };
        let (mut pico, source) = PicoSync::boot(Some(old_nvm.clone()));
        assert_eq!(source, BootSource::Nvm);
        store.delete_device("one");
        let fresh = store.register_device("one".into()).desired.clone();
        assert_ne!(fresh.incarnation, old_nvm.incarnation);
        assert_eq!(fresh.revision, 1);
        assert_eq!(fresh.value, DeviceSettings::default());
        assert_eq!(
            store.update_device(
                "one",
                old_nvm.incarnation,
                1,
                DeviceSettings {
                    wifi_timeout_seconds: 30,
                    ..DeviceSettings::default()
                }
            ),
            Err(SettingsError::StaleIncarnation {
                received: old_nvm.incarnation,
                current: fresh.incarnation
            })
        );
        assert_eq!(pico.receive(fresh.clone()), Ok(None));
        assert_eq!(pico.advance(), Some(SyncAction::PersistNvm(fresh.clone())));
        assert_eq!(
            pico.persist_succeeded(fresh.incarnation, fresh.revision),
            Ok(Some(SyncAction::AckApplied {
                incarnation: fresh.incarnation,
                revision: 1
            }))
        );
        assert_eq!(pico.active, fresh);
        assert_eq!(
            store
                .devices
                .get_mut("one")
                .unwrap()
                .heartbeat(old_nvm.incarnation, 1),
            Err(SettingsError::StaleIncarnation {
                received: old_nvm.incarnation,
                current: pico.active.incarnation
            })
        );
        assert!(store.devices["one"].pending());
        store
            .devices
            .get_mut("one")
            .unwrap()
            .heartbeat(pico.active.incarnation, 1)
            .unwrap();
        assert!(!store.devices["one"].pending());
    }

    #[test]
    fn stale_snapshot_is_explicitly_rejected_after_newer_revision_arrives() {
        let (mut pico, _) = PicoSync::boot(None);
        let first = DeviceSnapshot {
            incarnation: 1,
            revision: 1,
            value: DeviceSettings::default(),
        };
        let newer = DeviceSnapshot {
            incarnation: 1,
            revision: 2,
            value: DeviceSettings {
                wifi_timeout_seconds: 20,
                ..DeviceSettings::default()
            },
        };
        pico.receive(first.clone()).unwrap();
        pico.receive(newer).unwrap();
        assert_eq!(
            pico.receive(first),
            Err(SettingsError::StaleSnapshot {
                incarnation: 1,
                revision: 1
            })
        );
    }
}
