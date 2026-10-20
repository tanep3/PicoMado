//! Protection ledger and retention planning. Destructive maintenance is gated
//! by both a build feature and an explicit runtime opt-in.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, UtcOffset};

use super::store::{ArtworkStore, Publication};
use super::{CaricatureError, OriginalCodec};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionSnapshot {
    pub revision: u64,
    pub protected_ids: BTreeSet<String>,
}

pub struct ProtectionLedger {
    root: PathBuf,
}

pub(crate) struct ProtectionGuard {
    lock: File,
}
impl Drop for ProtectionGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = unsafe { libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl ProtectionLedger {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn locked<T>(
        &self,
        action: impl FnOnce() -> Result<T, CaricatureError>,
    ) -> Result<T, CaricatureError> {
        let _guard = self.lock_guard()?;
        action()
    }

    pub(crate) fn lock_guard(&self) -> Result<ProtectionGuard, CaricatureError> {
        fs::create_dir_all(&self.root).map_err(|_| CaricatureError::Io)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.root.join(".protection.lock"))
            .map_err(|_| CaricatureError::Io)?;
        #[cfg(unix)]
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(CaricatureError::Io);
        }
        Ok(ProtectionGuard { lock })
    }

    pub(crate) fn read(&self) -> Result<ProtectionSnapshot, CaricatureError> {
        let data = match fs::read(self.root.join("protection.json")) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProtectionSnapshot::default());
            }
            Err(_) => return Err(CaricatureError::Io),
        };
        serde_json::from_slice(&data).map_err(|_| CaricatureError::InvalidSource)
    }

    pub fn snapshot(&self) -> Result<ProtectionSnapshot, CaricatureError> {
        self.locked(|| self.read())
    }

    pub fn update(
        &self,
        store: &ArtworkStore,
        expected_revision: u64,
        ids: &[String],
        protect: bool,
    ) -> Result<ProtectionSnapshot, CaricatureError> {
        self.update_guarded(store, expected_revision, ids, protect, || {
            Err(CaricatureError::InvalidTransition)
        })
    }

    pub(crate) fn update_guarded(
        &self,
        store: &ArtworkStore,
        expected_revision: u64,
        ids: &[String],
        protect: bool,
        current: impl FnOnce() -> Result<
            (std::sync::Arc<serde_json::Value>, BTreeSet<String>),
            CaricatureError,
        >,
    ) -> Result<ProtectionSnapshot, CaricatureError> {
        if ids.is_empty() || ids.len() > 100 || ids.iter().any(|id| !valid_retention_id(id)) {
            return Err(CaricatureError::InvalidRequest);
        }
        self.locked(|| {
            let mut snapshot = self.read()?;
            if snapshot.revision != expected_revision {
                return Err(CaricatureError::Stale);
            }
            let pending: BTreeSet<_> = if protect {
                ids.iter()
                    .filter(|id| pending_path(&self.root, id).exists())
                    .cloned()
                    .collect()
            } else {
                BTreeSet::new()
            };
            let current = if pending.is_empty() {
                None
            } else {
                Some(current()?)
            };
            for id in ids {
                if let Some((snapshot, references)) = &current {
                    if pending.contains(id)
                        && !references.contains(id)
                        && !snapshot_references_artwork(snapshot, id)?
                    {
                        return Err(CaricatureError::InvalidRequest);
                    }
                }
                if store.artwork(id)?.is_none() {
                    return Err(CaricatureError::InvalidRequest);
                }
            }
            for id in ids {
                if protect {
                    snapshot.protected_ids.insert(id.clone());
                } else {
                    snapshot.protected_ids.remove(id);
                }
            }
            snapshot.revision = snapshot.revision.saturating_add(1);
            let data = serde_json::to_vec(&snapshot).map_err(|_| CaricatureError::InvalidSource)?;
            atomic_write(&self.root.join("protection.json"), &data)?;
            Ok(snapshot)
        })
    }
}

pub fn deletion_enabled() -> bool {
    cfg!(feature = "caricature-retention-delete")
        && std::env::var("PICOMADO_CARICATURE_DELETE").as_deref() == Ok("1")
}

pub(crate) fn pending_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!(".retention-pending-{id}.json"))
}

pub(crate) fn begin_pending_delete(root: &Path, id: &str) -> Result<(), CaricatureError> {
    atomic_write(&pending_path(root, id), b"{}")
}

/// The plugin owns its bootstrap policy, gate, paths and recovery IO.
pub(crate) struct RetentionRecovery;
impl crate::plugin_jobs::PluginStartupExecutor for RetentionRecovery {
    fn recover(
        &self,
        paths: &crate::plugin_jobs::PluginPaths,
        database: &rusqlite::Connection,
    ) -> Result<(), std::io::Error> {
        if !deletion_enabled() {
            return Ok(());
        }
        recover_pending_deletes(
            &ProtectionLedger::new(paths.data.join("artwork")),
            &paths.cache.join("artwork_variants"),
            database,
        )
        .map_err(|error| {
            std::io::Error::other(format!("caricature retention recovery failed: {error:?}"))
        })
    }
}

/// Finish a database-first deletion after a crash. Caller holds the ledger lock.
pub(crate) fn recover_pending_deletes(
    ledger: &ProtectionLedger,
    cache_root: &Path,
    connection: &rusqlite::Connection,
) -> Result<(), CaricatureError> {
    ledger.locked(|| {
        if !ledger.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&ledger.root).map_err(|_| CaricatureError::Io)? {
            let name = entry.map_err(|_| CaricatureError::Io)?.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name
                .strip_prefix(".retention-pending-")
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(CaricatureError::InvalidSource);
            }
            match finish_pending_delete(ledger, cache_root, connection, id) {
                Ok(()) | Err(CaricatureError::InvalidTransition) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CaricatureError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("pending-{}-{stamp}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(path.parent().unwrap())?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|_| CaricatureError::Io)
}

/// JST calendar date on which an artwork reaches its three-month lifetime.
pub fn expiration_day(created_at_unix: i64) -> Result<Date, CaricatureError> {
    let created = OffsetDateTime::from_unix_timestamp(created_at_unix)
        .map_err(|_| CaricatureError::InvalidSource)?
        .to_offset(UtcOffset::from_hms(9, 0, 0).expect("JST"))
        .date();
    let month_index = created.month() as i32 - 1 + 3;
    let year = created.year() + month_index / 12;
    let month = Month::try_from((month_index % 12 + 1) as u8)
        .map_err(|_| CaricatureError::InvalidSource)?;
    for day in (1..=created.day()).rev() {
        if let Ok(date) = Date::from_calendar_date(year, month, day) {
            return Ok(date);
        }
    }
    Err(CaricatureError::InvalidSource)
}

pub fn expired_candidates(
    publications: &[Publication],
    today_jst: Date,
    protection: &ProtectionSnapshot,
    displayed_or_in_use: &BTreeSet<String>,
) -> Result<Vec<String>, CaricatureError> {
    let mut candidates = Vec::new();
    for publication in publications {
        let id = &publication.artwork.id;
        if !protection.protected_ids.contains(id)
            && !displayed_or_in_use.contains(id)
            && expiration_day(publication.artwork.metadata.created_at_unix)? <= today_jst
        {
            candidates.push(id.clone());
        }
    }
    Ok(candidates)
}

/// Only fully published jobs with a verified archived WebP can lose their
/// staging files. Retry, failed, unfinished and legacy jobs remain untouched.
pub fn cleanup_published_work(
    store: &ArtworkStore,
    work_root: &Path,
    now_unix: i64,
    active_job_ids: &BTreeSet<String>,
) -> Result<usize, CaricatureError> {
    if !work_root.exists() {
        return Ok(0);
    }
    let canonical_root = work_root.canonicalize().map_err(|_| CaricatureError::Io)?;
    let mut removed = 0;
    for publication in store.history()? {
        let art = &publication.artwork;
        if art.original_codec != Some(OriginalCodec::HalfWebpQ90)
            || publication.published_at_unix > now_unix.saturating_sub(86_400)
            || store.original(art)?.is_none()
            || active_job_ids.contains(&art.metadata.job_id)
            || art.metadata.job_id.is_empty()
            || !art
                .metadata
                .job_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            continue;
        }
        let path = work_root.join(&art.metadata.job_id);
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            continue;
        }
        let canonical = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(CaricatureError::Io),
        };
        if canonical.parent() != Some(canonical_root.as_path()) {
            continue;
        }
        fs::remove_dir_all(canonical).map_err(|_| CaricatureError::Io)?;
        removed += 1;
    }
    Ok(removed)
}

// This whole-file deletion path is compiled only for isolated tests. The
// production release can plan expiry but cannot call an irreversible delete.
#[cfg(test)]
fn delete_bundle_for_test(
    ledger: &ProtectionLedger,
    cache_root: &Path,
    connection: &mut rusqlite::Connection,
    id: &str,
    today: Date,
    in_use: &BTreeSet<String>,
    stop_after_database: bool,
) -> Result<(), CaricatureError> {
    ledger.locked(|| {
        let store = ArtworkStore::new(ledger.root.clone());
        let protection = ledger.read()?;
        if !expired_candidates(&store.history()?, today, &protection, in_use)?
            .iter()
            .any(|item| item == id)
        {
            return Err(CaricatureError::InvalidTransition);
        }
        let body: String = connection
            .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(|_| CaricatureError::Io)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&body).map_err(|_| CaricatureError::InvalidSource)?;
        let plugin = value
            .pointer_mut("/plugin_states/caricature")
            .ok_or(CaricatureError::InvalidSource)?;
        if plugin.pointer("/latest/id").and_then(|x| x.as_str()) == Some(id)
            || plugin
                .pointer("/state/latest_artwork_id")
                .and_then(|x| x.as_str())
                == Some(id)
        {
            return Err(CaricatureError::InvalidTransition);
        }
        let history = plugin
            .get_mut("history")
            .and_then(|x| x.as_array_mut())
            .ok_or(CaricatureError::InvalidSource)?;
        let before = history.len();
        history.retain(|entry| entry.get("artwork_id").and_then(|x| x.as_str()) != Some(id));
        if history.len() == before {
            return Err(CaricatureError::InvalidSource);
        }
        if let Some(jobs) = plugin
            .pointer_mut("/state/jobs")
            .and_then(|x| x.as_object_mut())
        {
            jobs.retain(|_, job| {
                job.pointer("/state/Published/artwork_id")
                    .and_then(|x| x.as_str())
                    != Some(id)
            });
        }
        begin_pending_delete(&ledger.root, id)?;
        let tx = connection.transaction().map_err(|_| CaricatureError::Io)?;
        tx.execute(
            "UPDATE app_state SET body=?1 WHERE id=1",
            [serde_json::to_string(&value).map_err(|_| CaricatureError::InvalidSource)?],
        )
        .map_err(|_| CaricatureError::Io)?;
        tx.commit().map_err(|_| CaricatureError::Io)?;
        if stop_after_database {
            return Ok(());
        }
        finish_pending_delete(ledger, cache_root, connection, id)
    })
}

pub(crate) fn finish_pending_delete(
    ledger: &ProtectionLedger,
    cache_root: &Path,
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<(), CaricatureError> {
    let body: String = connection
        .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
            row.get(0)
        })
        .map_err(|_| CaricatureError::Io)?;
    let root: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| CaricatureError::InvalidSource)?;
    let plugin = root
        .pointer("/plugin_states/caricature")
        .ok_or(CaricatureError::InvalidSource)?;
    let jobs: crate::plugin_jobs::JobState = match root.get("jobs") {
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|_| CaricatureError::InvalidSource)?
        }
        None => Default::default(),
    };
    let own: Vec<_> = jobs
        .jobs
        .values()
        .filter(|r| r.job.token.plugin == super::ID && !r.status.finished())
        .map(|r| &r.job)
        .collect();
    let refs = crate::content::ContentPlugin::retention_references_for_jobs(
        &super::CaricatureContent::default(),
        &own,
    );
    match finish_pending_delete_snapshot(ledger, cache_root, plugin, &refs, id)? {
        PendingDeleteResult::Referenced => Err(CaricatureError::InvalidTransition),
        _ => Ok(()),
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PendingDeleteResult {
    Removed,
    Referenced,
    Protected,
}

pub(crate) fn snapshot_references_artwork(
    plugin: &serde_json::Value,
    id: &str,
) -> Result<bool, CaricatureError> {
    let history = plugin
        .get("history")
        .and_then(|v| v.as_array())
        .ok_or(CaricatureError::InvalidSource)?;
    let mut found = false;
    for entry in history {
        let existing = entry
            .get("artwork_id")
            .and_then(|v| v.as_str())
            .ok_or(CaricatureError::InvalidSource)?;
        found |= existing == id;
    }
    Ok(found
        || plugin.pointer("/latest/id").and_then(|v| v.as_str()) == Some(id)
        || plugin
            .pointer("/state/latest_artwork_id")
            .and_then(|v| v.as_str())
            == Some(id))
}
fn remove_pending_marker(root: &Path, id: &str) -> Result<(), CaricatureError> {
    match fs::remove_file(pending_path(root, id)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CaricatureError::Io),
    }
    File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| CaricatureError::Io)
}
fn finish_pending_delete_snapshot(
    ledger: &ProtectionLedger,
    cache_root: &Path,
    plugin: &serde_json::Value,
    references: &BTreeSet<String>,
    id: &str,
) -> Result<PendingDeleteResult, CaricatureError> {
    if !pending_path(&ledger.root, id).exists() {
        return Ok(PendingDeleteResult::Removed);
    }
    if snapshot_references_artwork(plugin, id)? || references.contains(id) {
        remove_pending_marker(&ledger.root, id)?;
        return Ok(PendingDeleteResult::Referenced);
    }
    if ledger.read()?.protected_ids.contains(id) {
        return Ok(PendingDeleteResult::Protected);
    }
    let latest = ledger.root.join("latest.json");
    if latest.exists() {
        let pointer: serde_json::Value =
            serde_json::from_slice(&fs::read(&latest).map_err(|_| CaricatureError::Io)?)
                .map_err(|_| CaricatureError::InvalidSource)?;
        if pointer.get("artwork_id").and_then(|x| x.as_str()) == Some(id) {
            fs::remove_file(latest).map_err(|_| CaricatureError::Io)?;
        }
    }
    let publications = ledger.root.join("publications");
    let entries = match fs::read_dir(&publications) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(CaricatureError::Io),
    };
    for entry in entries.into_iter().flatten() {
        let path = entry.map_err(|_| CaricatureError::Io)?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let data = fs::read(&path).map_err(|_| CaricatureError::Io)?;
            let pointer: serde_json::Value =
                serde_json::from_slice(&data).map_err(|_| CaricatureError::InvalidSource)?;
            if pointer.get("artwork_id").and_then(|x| x.as_str()) == Some(id) {
                fs::remove_file(path).map_err(|_| CaricatureError::Io)?;
            }
        }
    }
    for extension in ["json", "png", "original.png", "original.webp"] {
        let path = ledger
            .root
            .join("archive")
            .join(format!("{id}.{extension}"));
        if path.exists() {
            fs::remove_file(path).map_err(|_| CaricatureError::Io)?;
        }
    }
    if cache_root.exists() {
        for entry in fs::read_dir(cache_root).map_err(|_| CaricatureError::Io)? {
            let path = entry.map_err(|_| CaricatureError::Io)?.path();
            if path
                .file_name()
                .and_then(|x| x.to_str())
                .is_some_and(|name| name.starts_with(&format!("{id}-")) && name.ends_with(".bin"))
            {
                fs::remove_file(path).map_err(|_| CaricatureError::Io)?;
            }
        }
    }
    for directory in [
        &ledger.root,
        &ledger.root.join("archive"),
        &publications,
        &cache_root.to_path_buf(),
    ] {
        if directory.exists() {
            File::open(directory)
                .and_then(|f| f.sync_all())
                .map_err(|_| CaricatureError::Io)?;
        }
    }
    remove_pending_marker(&ledger.root, id)?;
    Ok(PendingDeleteResult::Removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caricature::{
        hash::sha256_hex, originals::half_webp_quality90, Artwork, ArtworkMetadata,
        GenerationInput, GenerationSettings, HEIGHT_LCD, WIDTH_LCD,
    };

    fn unix_jst(year: i32, month: Month, day: u8) -> i64 {
        Date::from_calendar_date(year, month, day)
            .unwrap()
            .with_hms(0, 0, 0)
            .unwrap()
            .assume_offset(UtcOffset::from_hms(9, 0, 0).unwrap())
            .unix_timestamp()
    }

    fn art(id: &str, timestamp: i64) -> Artwork {
        Artwork {
            id: id.into(),
            metadata: ArtworkMetadata {
                job_id: id.into(),
                slot: 0,
                created_at_unix: timestamp,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: None,
            original_codec: None,
            eink_png: crate::caricature::test_canonical_png(),
            lcd_rgb565be: vec![0; WIDTH_LCD * HEIGHT_LCD * 2],
        }
    }

    #[test]
    fn calendar_month_end_protection_and_display_refs() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().join("archive"));
        let ledger = ProtectionLedger::new(dir.path().join("archive"));
        let jan31 = unix_jst(2026, Month::January, 31);
        let feb28 = unix_jst(2026, Month::February, 28);
        assert_eq!(expiration_day(jan31).unwrap().to_string(), "2026-04-30");
        assert_eq!(expiration_day(feb28).unwrap().to_string(), "2026-05-28");
        assert_eq!(
            expiration_day(unix_jst(2024, Month::November, 30))
                .unwrap()
                .to_string(),
            "2025-02-28"
        );
        store.publish(art("old", jan31), jan31).unwrap();
        store.publish(art("shown", jan31 + 1), jan31 + 1).unwrap();
        let snapshot = ledger.snapshot().unwrap();
        assert_eq!(snapshot.revision, 0);
        let protected = ledger.update(&store, 0, &["old".into()], true).unwrap();
        assert!(protected.protected_ids.contains("old"));
        assert_eq!(
            ledger.update(&store, 0, &["shown".into()], true),
            Err(CaricatureError::Stale)
        );
        assert!(ledger.update(&store, 1, &["missing".into()], true).is_err());
        let today = Date::from_calendar_date(2026, Month::April, 30).unwrap();
        let pinned = BTreeSet::from(["shown".into()]);
        assert!(
            expired_candidates(&store.history().unwrap(), today, &protected, &pinned)
                .unwrap()
                .is_empty()
        );
        let released = ledger.update(&store, 1, &["old".into()], false).unwrap();
        assert_eq!(
            expired_candidates(&store.history().unwrap(), today, &released, &pinned).unwrap(),
            vec!["old"]
        );
        assert_eq!(ledger.snapshot().unwrap(), released);
    }

    #[test]
    fn work_cleanup_requires_published_verified_webp_and_grace() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().join("archive"));
        let root = dir.path().join("work");
        let source = crate::caricature::test_canonical_png();
        let webp = half_webp_quality90(&source).unwrap();
        let mut published = art("published", 0);
        published.original_sha256 = Some(sha256_hex(&webp));
        published.original_codec = Some(OriginalCodec::HalfWebpQ90);
        store.publish_with_original(published, &webp, 0).unwrap();
        store.publish(art("legacy", 1), 1).unwrap();
        for id in ["published", "legacy", "unfinished"] {
            let path = root.join(id).join("normalize/1");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("original.png"), &source).unwrap();
        }
        assert_eq!(
            cleanup_published_work(&store, &root, 86_399, &BTreeSet::new()).unwrap(),
            0
        );
        assert_eq!(
            cleanup_published_work(&store, &root, 86_400, &BTreeSet::from(["published".into()]))
                .unwrap(),
            0
        );
        assert_eq!(
            cleanup_published_work(&store, &root, 86_400, &BTreeSet::new()).unwrap(),
            1
        );
        assert!(!root.join("published").exists());
        assert!(root.join("legacy").exists());
        assert!(root.join("unfinished").exists());
        assert_eq!(
            cleanup_published_work(&store, &root, 200_000, &BTreeSet::new()).unwrap(),
            0
        );
    }

    #[test]
    fn isolated_bundle_delete_recovers_after_db_commit_without_losing_settings() {
        use crate::application::App;
        use crate::caricature::{CaricatureContent, HistoryEntry};
        use rusqlite::Connection;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");
        let app = App::open(&db_path).unwrap();
        let paths = app.plugin_paths("caricature").unwrap();
        drop(app);
        let store = ArtworkStore::new(paths.data.join("artwork"));
        let ledger = ProtectionLedger::new(paths.data.join("artwork"));
        let cache = paths.cache.join("artwork_variants");
        fs::create_dir_all(&cache).unwrap();
        let original = crate::caricature::test_canonical_png();
        let webp = half_webp_quality90(&original).unwrap();
        let mut old = art("old", unix_jst(2026, Month::January, 31));
        old.original_sha256 = Some(sha256_hex(&webp));
        old.original_codec = Some(OriginalCodec::HalfWebpQ90);
        let old_pub = store.publish_with_original(old, &webp, 1).unwrap();
        let protected_pub = store
            .publish(art("protected", unix_jst(2026, Month::January, 31)), 2)
            .unwrap();
        let latest_pub = store
            .publish(art("latest", unix_jst(2026, Month::April, 29)), 3)
            .unwrap();
        fs::write(cache.join("old-key.bin"), b"cached").unwrap();
        let publications = [&old_pub, &protected_pub, &latest_pub];
        let mut state = serde_json::to_value(CaricatureContent::default()).unwrap();
        state["history"] = serde_json::to_value(
            publications
                .iter()
                .map(|entry| HistoryEntry {
                    artwork_id: entry.artwork.id.clone(),
                    metadata: entry.artwork.metadata.clone(),
                    publish_seq: entry.publish_seq,
                    published_at_unix: entry.published_at_unix,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut connection = Connection::open(&db_path).unwrap();
        let body: String = connection
            .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let mut body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let settings_before = body["settings"].clone();
        body["plugin_states"]["caricature"] = state;
        connection
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [body.to_string()],
            )
            .unwrap();
        ledger
            .update(&store, 0, &["protected".into()], true)
            .unwrap();
        let today = Date::from_calendar_date(2026, Month::April, 30).unwrap();
        let in_use = BTreeSet::from(["latest".into()]);
        let mut displaying = body.clone();
        displaying["plugin_states"]["caricature"]["latest"] =
            serde_json::to_value(&old_pub.artwork).unwrap();
        displaying["plugin_states"]["caricature"]["state"]["latest_artwork_id"] =
            serde_json::json!("old");
        connection
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [displaying.to_string()],
            )
            .unwrap();
        assert_eq!(
            delete_bundle_for_test(
                &ledger,
                &cache,
                &mut connection,
                "old",
                today,
                &BTreeSet::new(),
                false
            ),
            Err(CaricatureError::InvalidTransition)
        );
        assert!(store.artwork("old").unwrap().is_some());
        connection
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [body.to_string()],
            )
            .unwrap();
        assert_eq!(
            delete_bundle_for_test(
                &ledger,
                &cache,
                &mut connection,
                "protected",
                today,
                &in_use,
                false
            ),
            Err(CaricatureError::InvalidTransition)
        );
        assert_eq!(
            delete_bundle_for_test(
                &ledger,
                &cache,
                &mut connection,
                "latest",
                today,
                &in_use,
                false
            ),
            Err(CaricatureError::InvalidTransition)
        );
        begin_pending_delete(&paths.data.join("artwork"), "old").unwrap();
        recover_pending_deletes(&ledger, &cache, &connection).unwrap();
        assert!(!pending_path(&paths.data.join("artwork"), "old").exists());
        assert!(store.artwork("old").unwrap().is_some());
        delete_bundle_for_test(
            &ledger,
            &cache,
            &mut connection,
            "old",
            today,
            &in_use,
            true,
        )
        .unwrap();
        assert!(store.artwork("old").unwrap().is_some());
        let interim: String = connection
            .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(
            !serde_json::from_str::<serde_json::Value>(&interim).unwrap()["plugin_states"]
                ["caricature"]["history"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["artwork_id"] == "old")
        );
        recover_pending_deletes(&ledger, &cache, &connection).unwrap();
        assert!(store.artwork("old").unwrap().is_none());
        assert!(!cache.join("old-key.bin").exists());
        assert!(!dir
            .path()
            .join("plugins/caricature/data/artwork/archive/old.original.webp")
            .exists());
        assert_eq!(store.history().unwrap().len(), 2);
        assert!(store.artwork("protected").unwrap().is_some());
        assert!(store.artwork("latest").unwrap().is_some());
        drop(connection);
        let reopened = App::open(&db_path).unwrap();
        let view = reopened.plugin_web_view("caricature").unwrap();
        assert_eq!(view.data["history"].as_array().unwrap().len(), 2);
        let after: String = Connection::open(&db_path)
            .unwrap()
            .query_row("SELECT body FROM app_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&after).unwrap()["settings"],
            settings_before
        );
    }

    #[test]
    fn app_sweep_respects_live_latest_protection_and_reclaims_after_view_moves() {
        use crate::application::App;
        use crate::caricature::{CaricatureContent, HistoryEntry};
        use rusqlite::Connection;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let app = App::open(&db).unwrap();
        let paths = app.plugin_paths("caricature").unwrap();
        drop(app);
        let root = paths.data.join("artwork");
        let store = ArtworkStore::new(root.clone());
        let ledger = ProtectionLedger::new(root);
        let old = store
            .publish(art("old", unix_jst(2026, Month::January, 31)), 1)
            .unwrap();
        let protected = store
            .publish(art("protected", unix_jst(2026, Month::January, 31)), 2)
            .unwrap();
        let current = store
            .publish(art("current", unix_jst(2026, Month::April, 29)), 3)
            .unwrap();
        let publications = [&old, &protected, &current];
        let mut state = serde_json::to_value(CaricatureContent::default()).unwrap();
        state["latest"] = serde_json::to_value(&old.artwork).unwrap();
        state["state"]["latest_artwork_id"] = serde_json::json!("old");
        state["history"] = serde_json::to_value(
            publications
                .iter()
                .map(|p| HistoryEntry {
                    artwork_id: p.artwork.id.clone(),
                    metadata: p.artwork.metadata.clone(),
                    publish_seq: p.publish_seq,
                    published_at_unix: p.published_at_unix,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let connection = Connection::open(&db).unwrap();
        let body: String = connection
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let settings = body["settings"].clone();
        body["plugin_states"]["caricature"] = state;
        connection
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [body.to_string()],
            )
            .unwrap();
        drop(connection);
        ledger
            .update(&store, 0, &["protected".into()], true)
            .unwrap();
        struct Clock;
        impl crate::fetch::Clock for Clock {
            fn unix_seconds(&self) -> i64 { unix_jst(2026, Month::April, 30) + 3 * 3600 }
            fn today_jst(&self) -> String { "2026-04-30".into() }
        }
        let app = std::sync::Arc::new(crate::shared::Application::new(
            App::open_with_clock(&db, std::sync::Arc::new(Clock)).unwrap()));
        crate::maintenance::run_once(&app, super::super::ID, &RetentionMaintenance::for_test()).unwrap();
        assert!(store.artwork("old").unwrap().is_some());
        drop(app);
        body["plugin_states"]["caricature"]["latest"] =
            serde_json::to_value(&current.artwork).unwrap();
        body["plugin_states"]["caricature"]["state"]["latest_artwork_id"] =
            serde_json::json!("current");
        let connection = Connection::open(&db).unwrap();
        connection
            .execute(
                "UPDATE app_state SET body=?1 WHERE id=1",
                [body.to_string()],
            )
            .unwrap();
        drop(connection);
        let app = std::sync::Arc::new(crate::shared::Application::new(
            App::open_with_clock(&db, std::sync::Arc::new(Clock)).unwrap()));
        crate::maintenance::run_once(&app, super::super::ID, &RetentionMaintenance::for_test()).unwrap();
        assert!(store.artwork("old").unwrap().is_none());
        assert!(store.artwork("protected").unwrap().is_some());
        assert!(store.artwork("current").unwrap().is_some());
        assert_eq!(
            app.read().unwrap().plugin_web_view("caricature").unwrap().data["history"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        drop(app);
        let body: String = Connection::open(&db)
            .unwrap()
            .query_row("SELECT body FROM app_state WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["settings"],
            settings
        );
    }
    include!("retention_maintenance_tests.rs");

}

include!("retention_maintenance.rs");
