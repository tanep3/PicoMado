//! BT-AUDIO-20261107: bounded, IO-free per-device Bluetooth intent and observations.
//! Persist only DesiredBluetooth; session/command state must never be replayed.
use serde::{Deserialize, Serialize};

pub const MAX_CANDIDATES: usize = 32;
pub const COMMAND_TTL_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BluetoothAddress(String);
impl TryFrom<String> for BluetoothAddress {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 17
            || !value.bytes().enumerate().all(|(i, b)| {
                if i % 3 == 2 {
                    b == b':'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
        {
            return Err("invalid Bluetooth address".into());
        }
        Ok(Self(value.to_ascii_uppercase()))
    }
}
impl From<BluetoothAddress> for String {
    fn from(value: BluetoothAddress) -> Self {
        value.0
    }
}

/// Per-device digital attenuation control, never an acoustic loudness guarantee.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Volume(u8);
impl TryFrom<u8> for Volume {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 100 {
            Ok(Self(value))
        } else {
            Err("volume must be 0..100")
        }
    }
}
impl From<Volume> for u8 {
    fn from(value: Volume) -> Self {
        value.0
    }
}
impl Volume {
    pub fn value(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StoredDesiredBluetooth")]
pub struct DesiredBluetooth {
    pub volume: Volume,
    pub muted: bool,
    pub enabled: bool,
    pub target: Option<BluetoothAddress>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDesiredBluetooth {
    #[serde(default)]
    volume: Volume,
    #[serde(default, deserialize_with = "explicit_muted")]
    muted: Option<bool>,
    enabled: bool,
    target: Option<BluetoothAddress>,
}
fn explicit_muted<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    bool::deserialize(d).map(Some)
}
impl TryFrom<StoredDesiredBluetooth> for DesiredBluetooth {
    type Error = &'static str;
    fn try_from(value: StoredDesiredBluetooth) -> Result<Self, Self::Error> {
        Ok(Self { muted: value.muted.unwrap_or(value.volume.value() == 0),
            volume: value.volume, enabled: value.enabled, target: value.target })
    }
}
impl Default for DesiredBluetooth {
    fn default() -> Self {
        Self { volume: Volume::default(), muted: true, enabled: false, target: None }
    }
}
impl DesiredBluetooth {
    pub fn effective_volume(&self) -> u8 {
        if self.muted { 0 } else { self.volume.value() }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub address: BluetoothAddress,
    pub name: String,
    pub a2dp: Option<bool>,
}
impl Candidate {
    fn validate(&self) -> Result<(), Error> {
        if self.name.len() > 248 || self.name.chars().any(char::is_control) {
            Err(Error::InvalidInput)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observed {
    Offline,
    Unsupported,
    Disabled,
    Stopping,
    Idle,
    Discovering,
    Pairing,
    Connecting,
    Connected,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Offline,
    Unsupported,
    Disabled,
    Stale,
    Expired,
    InvalidInput,
    Capacity,
    Exhausted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandToken {
    session: u64,
    generation: u64,
    command_id: u64,
    expires_ms: u64,
}
impl CommandToken {
    pub(crate) fn is_expired(self,now:u64)->bool {now>=self.expires_ms}
    pub(crate) fn diagnostic(&self, now:u64)->String {
        format!("session_epoch={} generation={} command_id={} now_ms={} expires_ms={} remaining_ms={}",self.session,self.generation,self.command_id,now,self.expires_ms,self.expires_ms.saturating_sub(now))
    }
}
impl Effect {
    pub(crate) fn command_token(&self)->CommandToken {
        match self {Self::Stop{token}|Self::Discover{token}|Self::Connect{token,..}=>*token}
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    Stop {
        token: CommandToken,
    },
    Discover {
        token: CommandToken,
    },
    Connect {
        token: CommandToken,
        target: BluetoothAddress,
        pair: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothDevice {
    desired: DesiredBluetooth,
    observed: Observed,
    session: Option<u64>,
    supported: bool,
    generation: u64,
    next_command: u64,
    pending: Option<CommandToken>,
    candidates: Vec<Candidate>,
    connection: Option<CommandToken>,
}
impl BluetoothDevice {
    pub fn new(desired: DesiredBluetooth) -> Self {
        Self {
            desired,
            observed: Observed::Offline,
            session: None,
            supported: false,
            generation: 0,
            next_command: 0,
            pending: None,
            candidates: Vec::new(),
            connection: None,
        }
    }
    pub fn desired(&self) -> &DesiredBluetooth {
        &self.desired
    }
    /// A local gain preference does not invalidate a wireless command or link.
    pub fn set_muted(&mut self, muted: bool) {
        self.desired.muted = muted;
    }
    pub fn set_volume(&mut self, volume: Volume) {
        self.desired.volume = volume;
    }
    pub fn observed(&self) -> Observed {
        self.observed
    }
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
    pub fn pending(&self) -> Option<CommandToken> {
        self.pending
    }
    /// Caller binds this aggregate to the existing device incarnation/revision.
    /// Rebinding invalidates all transient operations, even when a session ID repeats.
    pub fn bind(&mut self, session: Option<u64>, supported: bool) -> Result<(), Error> {
        self.invalidate()?;
        self.session = session;
        self.supported = supported;
        self.observed = if session.is_none() {
            Observed::Offline
        } else if !supported {
            Observed::Unsupported
        } else if !self.desired.enabled {
            Observed::Disabled
        } else {
            Observed::Idle
        };
        Ok(())
    }
    fn invalidate(&mut self) -> Result<(), Error> {
        self.generation = self.generation.checked_add(1).ok_or(Error::Exhausted)?;
        self.pending = None;
        self.connection = None;
        self.candidates.clear();
        Ok(())
    }
    fn token(&mut self, now_ms: u64) -> Result<CommandToken, Error> {
        let session = self.session.ok_or(Error::Offline)?;
        if !self.supported {
            return Err(Error::Unsupported);
        }
        let command_id = self.next_command.checked_add(1).ok_or(Error::Exhausted)?;
        let expires_ms = now_ms.checked_add(COMMAND_TTL_MS).ok_or(Error::Exhausted)?;
        self.invalidate()?;
        self.next_command = command_id;
        let token = CommandToken {
            session,
            generation: self.generation,
            command_id,
            expires_ms,
        };
        self.pending = Some(token);
        Ok(token)
    }
    pub fn set_enabled(&mut self, enabled: bool, now_ms: u64) -> Result<Option<Effect>, Error> {
        self.invalidate()?;
        self.desired.enabled = enabled;
        if self.session.is_none() || !self.supported {
            return Ok(None);
        }
        if !enabled {
            let token = self.token(now_ms)?;
            // Desired OFF is immediate; actual radio stop is still pending.
            self.observed = Observed::Stopping;
            return Ok(Some(Effect::Stop { token }));
        }
        self.observed = Observed::Idle;
        if let Some(target) = self.desired.target.clone() {
            let token = self.token(now_ms)?;
            self.observed = Observed::Connecting;
            Ok(Some(Effect::Connect {
                token,
                target,
                pair: false,
            }))
        } else {
            Ok(None)
        }
    }
    pub fn discover(&mut self, now_ms: u64) -> Result<Effect, Error> {
        if !self.desired.enabled {
            return Err(Error::Disabled);
        }
        let token = self.token(now_ms)?;
        self.observed = Observed::Discovering;
        Ok(Effect::Discover { token })
    }
    fn check(&self, token: CommandToken, now_ms: u64) -> Result<(), Error> {
        if self.pending != Some(token) {
            return Err(Error::Stale);
        }
        if token.is_expired(now_ms) {
            return Err(Error::Expired);
        }
        Ok(())
    }
    pub fn candidate(
        &mut self,
        token: CommandToken,
        now_ms: u64,
        candidate: Candidate,
    ) -> Result<(), Error> {
        self.check(token, now_ms)?;
        if self.observed != Observed::Discovering {
            return Err(Error::Stale);
        }
        candidate.validate()?;
        if let Some(existing) = self
            .candidates
            .iter_mut()
            .find(|c| c.address == candidate.address)
        {
            *existing = candidate;
        } else if self.candidates.len() < MAX_CANDIDATES {
            self.candidates.push(candidate);
        } else {
            return Err(Error::Capacity);
        }
        Ok(())
    }
    pub fn pair(
        &mut self,
        discovery: CommandToken,
        now_ms: u64,
        target: &BluetoothAddress,
    ) -> Result<Effect, Error> {
        self.check(discovery, now_ms)?;
        if !matches!(self.observed, Observed::Discovering | Observed::Idle)
            || !self
                .candidates
                .iter()
                .any(|c| &c.address == target && c.a2dp != Some(false))
        {
            return Err(Error::InvalidInput);
        }
        let token = self.token(now_ms)?;
        self.desired.target = Some(target.clone());
        self.observed = Observed::Pairing;
        Ok(Effect::Connect {
            token,
            target: target.clone(),
            pair: true,
        })
    }
    pub fn complete(
        &mut self,
        token: CommandToken,
        now_ms: u64,
        success: bool,
    ) -> Result<(), Error> {
        self.check(token, now_ms)?;
        if !matches!(
            self.observed,
            Observed::Discovering | Observed::Pairing | Observed::Connecting | Observed::Stopping
        ) {
            return Err(Error::Stale);
        }
        self.observed = if !success {
            Observed::Failed
        } else if !self.desired.enabled {
            Observed::Disabled
        } else {
            match self.observed {
                Observed::Pairing | Observed::Connecting => Observed::Connected,
                Observed::Discovering => Observed::Idle,
                _ => return Err(Error::Stale),
            }
        };
        if self.observed == Observed::Connected {
            self.connection = Some(token);
        }
        if !success {
            self.candidates.clear();
        }
        // Discovery stays selectable until its deadline or a newer command.
        if self.observed != Observed::Idle {
            self.pending = None;
        }
        Ok(())
    }
    /// Start a fresh saved-target connection after bind, never replay a transient command.
    /// No-op when connected, busy, disabled, offline, unsupported, or targetless.
    pub fn reconcile(&mut self, now_ms: u64) -> Result<Option<Effect>, Error> {
        if self.session.is_none()
            || !self.supported
            || !self.desired.enabled
            || self.pending.is_some()
            || self.observed == Observed::Connected
        {
            return Ok(None);
        }
        let Some(target) = self.desired.target.clone() else {
            return Ok(None);
        };
        let token = self.token(now_ms)?;
        self.observed = Observed::Connecting;
        Ok(Some(Effect::Connect {
            token,
            target,
            pair: false,
        }))
    }
    /// A link-down observation uses its original connection identity, not its command deadline.
    /// Once established a link may remain live longer than the command timeout.
    pub fn disconnected(&mut self, connection: CommandToken) -> Result<(), Error> {
        if self.connection != Some(connection) || self.observed != Observed::Connected {
            return Err(Error::Stale);
        }
        self.connection = None;
        self.observed = Observed::Idle;
        Ok(())
    }
    pub fn expire(&mut self, now_ms: u64) -> bool {
        if self.pending.is_some_and(|t| t.is_expired(now_ms)) {
            self.pending = None;
            self.candidates.clear();
            if self.observed != Observed::Idle {
                self.observed = Observed::Failed;
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device() -> BluetoothDevice {
        let mut d = BluetoothDevice::new(DesiredBluetooth::default());
        d.bind(Some(1), true).unwrap();
        d.set_enabled(true, 0).unwrap();
        d
    }
    fn candidate(n: usize) -> Candidate {
        Candidate {
            address: BluetoothAddress::try_from(format!("00:01:02:03:04:{n:02X}")).unwrap(),
            name: "Speaker".into(),
            a2dp: Some(true),
        }
    }
    #[test]
    fn mute_migration_preserves_effective_volume_and_strict_types() {
        assert!(DesiredBluetooth::default().muted);
        for volume in [0,10,100] {
            let old = format!(r#"{{"enabled":true,"target":null,"volume":{volume}}}"#);
            let d: DesiredBluetooth = serde_json::from_str(&old).unwrap();
            assert_eq!(d.muted, volume == 0);
            assert_eq!(d.effective_volume(), volume);
            for muted in [true,false] {
                let mut value: serde_json::Value=serde_json::from_str(&old).unwrap();
                value["muted"]=muted.into();
                let d: DesiredBluetooth=serde_json::from_value(value).unwrap();
                assert_eq!(d.volume.value(),volume);
                assert_eq!(d.effective_volume(),if muted {0} else {volume});
                assert_eq!(serde_json::from_str::<DesiredBluetooth>(&serde_json::to_string(&d).unwrap()).unwrap(),d);
            }
        }
        for extra in [r#""muted":null"#,r#""muted":0"#,r#""muted":"true""#,r#""unknown":true"#] {
            assert!(serde_json::from_str::<DesiredBluetooth>(&format!(r#"{{"enabled":false,"target":null,{extra}}}"#)).is_err());
        }
    }
    #[test]
    fn volume_missing_migrates_to_mute_and_invalid_values_are_rejected() {
        let old: DesiredBluetooth =
            serde_json::from_str(r#"{"enabled":true,"target":null}"#).unwrap();
        assert_eq!(old.volume.value(), 0);
        for value in ["-1", "101", "256", "10.5", "null", "\"10\""] {
            assert!(serde_json::from_str::<Volume>(value).is_err());
        }
        for value in [0, 10, 100] {
            assert_eq!(
                serde_json::from_str::<Volume>(&value.to_string())
                    .unwrap()
                    .value(),
                value
            );
        }
        let mut d = device();
        d.discover(0).unwrap();
        let pending = d.pending();
        d.set_volume(Volume::try_from(10).unwrap());
        assert_eq!(d.pending(), pending);
        assert_eq!(d.observed(), Observed::Discovering);
    }
    #[test]
    fn stale_and_isolated() {
        let mut a = device();
        let b = device();
        a.discover(0).unwrap();
        let old = a.pending().unwrap();
        a.discover(1).unwrap();
        assert_eq!(a.candidate(old, 2, candidate(0)), Err(Error::Stale));
        assert!(b.candidates().is_empty());
        let current = a.pending().unwrap();
        a.bind(Some(1), true).unwrap();
        assert_eq!(a.complete(current, 2, true), Err(Error::Stale));
    }
    #[test]
    fn bounded_and_validated() {
        let mut d = device();
        d.discover(0).unwrap();
        let t = d.pending().unwrap();
        for n in 0..32 {
            d.candidate(t, 1, candidate(n)).unwrap();
        }
        assert_eq!(d.candidate(t, 1, candidate(32)), Err(Error::Capacity));
        d.candidate(t, 1, candidate(0)).unwrap();
        assert_eq!(d.candidates().len(), 32);
        assert!(serde_json::from_str::<DesiredBluetooth>(
            r#"{"enabled":true,"target":null,"keys":"secret"}"#
        )
        .is_err());
        assert!(BluetoothAddress::try_from("bad".to_string()).is_err());
    }
    #[test]
    fn pair_observed_only_and_off_cancels() {
        let mut d = device();
        d.discover(0).unwrap();
        let scan = d.pending().unwrap();
        d.candidate(scan, 1, candidate(0)).unwrap();
        d.complete(scan, 2, true).unwrap();
        d.pair(scan, 3, &candidate(0).address).unwrap();
        let connect = d.pending().unwrap();
        assert_eq!(d.observed(), Observed::Pairing);
        d.complete(connect, 4, false).unwrap();
        assert_eq!(d.observed(), Observed::Failed);
        d.set_enabled(true, 5).unwrap();
        let connect = d.pending().unwrap();
        d.set_enabled(false, 6).unwrap();
        assert_eq!(d.complete(connect, 7, true), Err(Error::Stale));
        assert!(d.desired().target.is_some());
        d.complete(d.pending().unwrap(), 8, true).unwrap();
        assert_eq!(d.observed(), Observed::Disabled);
    }
    #[test]
    fn offline_and_expiration() {
        let mut d = BluetoothDevice::new(DesiredBluetooth::default());
        assert_eq!(d.set_enabled(true, 0), Ok(None));
        assert_eq!(d.discover(0), Err(Error::Offline));
        d.bind(Some(1), true).unwrap();
        d.discover(0).unwrap();
        let t = d.pending().unwrap();
        assert_eq!(
            d.candidate(t, COMMAND_TTL_MS, candidate(0)),
            Err(Error::Expired)
        );
        assert!(d.expire(COMMAND_TTL_MS));
        assert!(!d.expire(COMMAND_TTL_MS));
    }
    #[test]
    fn completed_discovery_expiry_is_not_failure() {
        let mut d = device();
        d.discover(0).unwrap();
        let t = d.pending().unwrap();
        d.candidate(t, 1, candidate(0)).unwrap();
        d.complete(t, 2, true).unwrap();
        assert_eq!(d.complete(t, 3, false), Err(Error::Stale));
        assert_eq!(d.complete(t, 3, true), Err(Error::Stale));
        assert!(d.expire(COMMAND_TTL_MS));
        assert_eq!(d.observed(), Observed::Idle);
        assert!(d.candidates().is_empty());
    }
    #[test]
    fn link_identity_and_fresh_reconnect() {
        let mut d = BluetoothDevice::new(DesiredBluetooth {
            enabled: true,
            target: Some(candidate(0).address),
            volume: Volume::default(),
            muted: true,
        });
        d.bind(Some(1), true).unwrap();
        assert!(matches!(
            d.reconcile(0).unwrap(),
            Some(Effect::Connect { pair: false, .. })
        ));
        let t = d.pending().unwrap();
        assert_eq!(d.reconcile(1), Ok(None));
        d.complete(t, 2, true).unwrap();
        assert_eq!(d.complete(t, 3, true), Err(Error::Stale));
        assert!(!d.expire(COMMAND_TTL_MS));
        assert_eq!(d.reconcile(COMMAND_TTL_MS), Ok(None));
        d.disconnected(t).unwrap();
        assert_eq!(d.disconnected(t), Err(Error::Stale));
        d.reconcile(COMMAND_TTL_MS).unwrap();
        let fresh = d.pending().unwrap();
        assert_ne!(t, fresh);
        d.bind(Some(1), true).unwrap();
        assert_eq!(
            d.complete(fresh, COMMAND_TTL_MS + 1, true),
            Err(Error::Stale)
        );
        d.reconcile(COMMAND_TTL_MS + 2).unwrap();
        let rebound = d.pending().unwrap();
        d.complete(rebound, COMMAND_TTL_MS + 3, true).unwrap();
        assert_eq!(d.disconnected(t), Err(Error::Stale));
        assert_eq!(d.observed(), Observed::Connected);
    }
    #[test]
    fn command_token_wire_rejects_unknown_fields() {
        let mut d = device();
        d.discover(0).unwrap();
        let t = d.pending().unwrap();
        let mut json = serde_json::to_value(t).unwrap();
        assert_eq!(
            serde_json::from_value::<CommandToken>(json.clone()).unwrap(),
            t
        );
        json["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<CommandToken>(json).is_err());
        d.set_enabled(false, 1).unwrap();
        assert_eq!(d.observed(), Observed::Stopping);
        assert!(d.expire(COMMAND_TTL_MS + 1));
        assert_eq!(d.observed(), Observed::Failed);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Report {
    Candidate { candidate: Candidate },
    Complete { success: bool },
    Disconnected,
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn bluetooth_deadline_diagnostic_boundary_and_no_target() {
        let token=CommandToken{session:1,generation:2,command_id:3,expires_ms:30};
        assert!(token.diagnostic(29).contains("remaining_ms=1"));
        assert!(token.diagnostic(30).contains("remaining_ms=0"));
        assert!(token.diagnostic(u64::MAX).contains("remaining_ms=0"));
        let effect=Effect::Connect{token,target:BluetoothAddress::try_from("AA:BB:CC:DD:EE:FF".to_string()).unwrap(),pair:false};
        assert_eq!(effect.command_token(),token);
        assert!(!effect.command_token().diagnostic(0).contains("AA:BB"));
    }
}
