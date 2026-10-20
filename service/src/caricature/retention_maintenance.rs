// Included in retention: all schedule, filesystem and payload policy belongs here.
use crate::content::{ContentError, ContentPlugin};
use crate::maintenance::{
    MaintenanceCommit, MaintenanceInput, MaintenancePlan, PluginMaintenanceExecutor,
    PreparedMaintenance,
};
use crate::plugin_jobs::{JobFailure, PluginJob};
use std::collections::BTreeMap;
use std::sync::Mutex;

const RETENTION_BATCH: usize = 32;
#[derive(Serialize, Deserialize)]
struct RetentionProposal {
    today: String,
    daily: bool,
    expired: BTreeSet<String>,
    pending: BTreeSet<String>,
    protected: BTreeSet<String>,
}
#[derive(Serialize, Deserialize)]
struct RetentionOutcome {
    today: String,
    daily: bool,
    removed: Vec<String>,
    abort: Vec<String>,
    more: bool,
}
struct RetentionPermit {
    ledger: ProtectionLedger,
    cache: PathBuf,
    candidates: BTreeSet<String>,
    pending: BTreeSet<String>,
    _guard: ProtectionGuard,
}
#[derive(Debug, Default)]
pub struct RetentionMaintenance {
    last_days: Mutex<BTreeMap<PathBuf, String>>,
    #[cfg(test)]
    force_enabled: bool,
}
impl RetentionMaintenance {
    pub fn enabled(&self) -> bool {
        #[cfg(test)]
        if self.force_enabled {
            return true;
        }
        deletion_enabled()
    }
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            force_enabled: true,
            ..Default::default()
        }
    }
}
fn valid_retention_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn pending_ids(root: &Path) -> Result<BTreeSet<String>, CaricatureError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(_) => return Err(CaricatureError::Io),
    };
    let mut ids = BTreeSet::new();
    for entry in entries {
        let name = entry.map_err(|_| CaricatureError::Io)?.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(id) = name
            .strip_prefix(".retention-pending-")
            .and_then(|s| s.strip_suffix(".json"))
        {
            if !valid_retention_id(id) {
                return Err(CaricatureError::InvalidSource);
            }
            ids.insert(id.into());
        }
    }
    Ok(ids)
}
fn retention_failure(_: CaricatureError) -> JobFailure {
    JobFailure::new("retention_io", "Artwork retention deferred")
}
fn invalid_retention() -> JobFailure {
    JobFailure::new("retention_plan", "Invalid artwork retention plan")
}
impl PluginMaintenanceExecutor for RetentionMaintenance {
    fn prepare(&self, input: MaintenanceInput) -> Result<Option<PreparedMaintenance>, JobFailure> {
        if !self.enabled() {
            return Ok(None);
        }
        let now = OffsetDateTime::from_unix_timestamp(input.now_unix)
            .map_err(|_| invalid_retention())?
            .to_offset(UtcOffset::from_hms(9, 0, 0).expect("JST"));
        let today = now.date().to_string();
        let root = input.paths.data.join("artwork");
        let daily = now.hour() == 3
            && self
                .last_days
                .lock()
                .map_err(|_| invalid_retention())?
                .get(&root)
                != Some(&today);
        // Avoid creating directories/locks outside the window without recovery work.
        if !daily && pending_ids(&root).map_err(retention_failure)?.is_empty() {
            return Ok(None);
        }
        let ledger = ProtectionLedger::new(root);
        let guard = ledger.lock_guard().map_err(retention_failure)?;
        let protection = ledger.read().map_err(retention_failure)?;
        let pending = pending_ids(&ledger.root).map_err(retention_failure)?;
        if !daily
            && pending
                .iter()
                .all(|id| protection.protected_ids.contains(id))
        {
            return Ok(None);
        }
        // Recover actionable intents first; a protected committed intent must
        // not starve unrelated daily expiry or cause repeated empty publication.
        let recovering = pending
            .iter()
            .any(|id| !protection.protected_ids.contains(id));
        let expired: BTreeSet<_> = if daily && !recovering {
            expired_candidates(
                &ArtworkStore::new(ledger.root.clone())
                    .history()
                    .map_err(retention_failure)?,
                now.date(),
                &protection,
                &BTreeSet::new(),
            )
            .map_err(retention_failure)?
            .into_iter()
            .collect()
        } else {
            BTreeSet::new()
        };
        let candidates = pending.union(&expired).cloned().collect();
        let proposal = RetentionProposal {
            today,
            daily: daily && !recovering,
            expired,
            pending: pending.clone(),
            protected: protection.protected_ids,
        };
        Ok(Some(PreparedMaintenance {
            proposal: serde_json::to_value(proposal).map_err(|_| invalid_retention())?,
            permit: Box::new(RetentionPermit {
                ledger,
                cache: input.paths.cache.join("artwork_variants"),
                candidates,
                pending,
                _guard: guard,
            }),
        }))
    }
    fn stage(
        &self,
        prepared: &mut PreparedMaintenance,
        outcome: &serde_json::Value,
    ) -> Result<(), JobFailure> {
        let permit = prepared
            .permit
            .downcast_ref::<RetentionPermit>()
            .ok_or_else(invalid_retention)?;
        let outcome: RetentionOutcome =
            serde_json::from_value(outcome.clone()).map_err(|_| invalid_retention())?;
        validate_outcome(permit, &outcome)?;
        for id in &outcome.removed {
            begin_pending_delete(&permit.ledger.root, id).map_err(retention_failure)?;
        }
        Ok(())
    }
    fn finish(
        &self,
        prepared: PreparedMaintenance,
        committed: &MaintenanceCommit,
    ) -> Result<(), JobFailure> {
        let permit = prepared
            .permit
            .downcast::<RetentionPermit>()
            .map_err(|_| invalid_retention())?;
        let outcome: RetentionOutcome =
            serde_json::from_value(committed.outcome.clone()).map_err(|_| invalid_retention())?;
        validate_outcome(&permit, &outcome)?;
        for id in &outcome.removed {
            finish_pending_delete_snapshot(
                &permit.ledger,
                &permit.cache,
                &committed.snapshot,
                &committed.references,
                id,
            )
            .map_err(retention_failure)?;
        }
        for id in &outcome.abort {
            remove_pending_marker(&permit.ledger.root, id).map_err(retention_failure)?;
        }
        if outcome.daily && !outcome.more {
            self.last_days
                .lock()
                .map_err(|_| invalid_retention())?
                .insert(permit.ledger.root.clone(), outcome.today);
        }
        Ok(())
    }
}
fn validate_outcome(
    permit: &RetentionPermit,
    outcome: &RetentionOutcome,
) -> Result<(), JobFailure> {
    if outcome.removed.len() > RETENTION_BATCH
        || outcome
            .removed
            .iter()
            .any(|id| !valid_retention_id(id) || !permit.candidates.contains(id))
        || outcome.abort.iter().any(|id| !permit.pending.contains(id))
    {
        return Err(invalid_retention());
    }
    Ok(())
}
pub(crate) fn plan_retention(
    content: &mut super::CaricatureContent,
    value: &serde_json::Value,
    jobs: &[&PluginJob],
) -> Result<MaintenancePlan, ContentError> {
    let proposal: RetentionProposal =
        serde_json::from_value(value.clone()).map_err(|_| ContentError::InvalidSource)?;
    let references = content.retention_references_for_jobs(jobs);
    let history: BTreeSet<_> = content
        .history
        .iter()
        .map(|entry| entry.artwork_id.clone())
        .collect();
    let mut outcome = RetentionOutcome {
        today: proposal.today,
        daily: proposal.daily,
        removed: Vec::new(),
        abort: Vec::new(),
        more: false,
    };
    let mut changed = false;
    for id in proposal.pending.union(&proposal.expired) {
        if !valid_retention_id(id) {
            return Err(ContentError::InvalidSource);
        }
        let pending = proposal.pending.contains(id);
        if references.contains(id) || (proposal.protected.contains(id) && history.contains(id)) {
            if pending {
                outcome.abort.push(id.clone());
            }
            continue;
        }
        if proposal.protected.contains(id) {
            continue;
        } // committed protected intent remains.
        let exists = history.contains(id);
        if exists && !proposal.expired.contains(id) {
            if pending {
                outcome.abort.push(id.clone());
            }
            continue;
        }
        if !exists && !pending {
            continue;
        }
        if outcome.removed.len() == RETENTION_BATCH {
            outcome.more = true;
            continue;
        }
        if exists {
            content.retire_artwork(id)?;
            changed = true;
        }
        outcome.removed.push(id.clone());
    }
    Ok(MaintenancePlan {
        changed,
        outcome: serde_json::to_value(outcome).map_err(|_| ContentError::InvalidSource)?,
    })
}
