//! Dedicated immutable artwork archive. The caller serializes `publish` calls
//! with its job ledger transaction/lock. Delivery never changes these files.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::hash::sha256_hex;
use super::{
    ensure_unused_source, recent_source_keys, Artwork, CaricatureError, OriginalCodec,
    PublishedSource,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Publication {
    pub artwork: Artwork,
    pub publish_seq: u64,
    pub published_at_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LatestPointer {
    artwork_id: String,
    publish_seq: u64,
    published_at_unix: i64,
}

pub struct ArtworkStore {
    root: PathBuf,
}

impl ArtworkStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn reconcile_latest(&self) -> Result<(), CaricatureError> {
        let Some(latest) = self.latest()? else {
            return Ok(());
        };
        let path = self
            .root
            .join("publications")
            .join(format!("{:020}.json", latest.publish_seq));
        if !path.exists() {
            fs::create_dir_all(path.parent().ok_or(CaricatureError::Io)?)
                .map_err(|_| CaricatureError::Io)?;
            let pointer = LatestPointer {
                artwork_id: latest.artwork.id,
                publish_seq: latest.publish_seq,
                published_at_unix: latest.published_at_unix,
            };
            write_new_sync(
                &path,
                &serde_json::to_vec(&pointer).map_err(|_| CaricatureError::InvalidSource)?,
            )?;
        }
        Ok(())
    }

    fn artwork_path(&self, id: &str) -> Result<PathBuf, CaricatureError> {
        if id.is_empty()
            || id.len() > 100
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(CaricatureError::InvalidSource);
        }
        Ok(self.root.join("archive").join(format!("{id}.json")))
    }

    fn original_path(&self, artwork: &Artwork) -> Result<PathBuf, CaricatureError> {
        let extension = match artwork.original_codec {
            Some(OriginalCodec::HalfWebpQ90) => "original.webp",
            None => "original.png",
        };
        Ok(self.artwork_path(&artwork.id)?.with_extension(extension))
    }

    /// Legacy artwork has no original. A missing sidecar on new artwork leaves
    /// its saved display variants usable, but cannot produce a new resolution.
    pub fn original(&self, artwork: &Artwork) -> Result<Option<Vec<u8>>, CaricatureError> {
        let Some(expected) = &artwork.original_sha256 else {
            return Ok(None);
        };
        let bytes = match fs::read(self.original_path(artwork)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CaricatureError::Io),
        };
        if sha256_hex(&bytes) != *expected {
            return Err(CaricatureError::InvalidSource);
        }
        if artwork.original_codec == Some(OriginalCodec::HalfWebpQ90) {
            super::originals::validate_half_webp(&bytes, None)?;
        }
        Ok(Some(bytes))
    }

    pub fn artwork(&self, id: &str) -> Result<Option<Artwork>, CaricatureError> {
        let path = self.artwork_path(id)?;
        let bytes = match fs::read(path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CaricatureError::Io),
        };
        let art: Artwork =
            serde_json::from_slice(&bytes).map_err(|_| CaricatureError::InvalidSource)?;
        if art.id != id {
            return Err(CaricatureError::InvalidSource);
        }
        art.validate()?;
        let png_path = self.root.join("archive").join(format!("{id}.png"));
        if fs::read(png_path).map_err(|_| CaricatureError::Io)? != art.eink_png {
            return Err(CaricatureError::InvalidSource);
        }
        Ok(Some(art))
    }

    pub fn latest(&self) -> Result<Option<Publication>, CaricatureError> {
        let pointer_path = self.root.join("latest.json");
        let bytes = match fs::read(pointer_path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CaricatureError::Io),
        };
        let pointer: LatestPointer =
            serde_json::from_slice(&bytes).map_err(|_| CaricatureError::InvalidSource)?;
        let artwork = self
            .artwork(&pointer.artwork_id)?
            .ok_or(CaricatureError::InvalidSource)?;
        Ok(Some(Publication {
            artwork,
            publish_seq: pointer.publish_seq,
            published_at_unix: pointer.published_at_unix,
        }))
    }

    pub fn history(&self) -> Result<Vec<Publication>, CaricatureError> {
        self.reconcile_latest()?;
        let directory = self.root.join("publications");
        let mut out = Vec::new();
        let entries = match fs::read_dir(directory) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(_) => return Err(CaricatureError::Io),
        };
        for entry in entries {
            let path = entry.map_err(|_| CaricatureError::Io)?.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let data = fs::read(path).map_err(|_| CaricatureError::Io)?;
            let pointer: LatestPointer =
                serde_json::from_slice(&data).map_err(|_| CaricatureError::InvalidSource)?;
            let artwork = self
                .artwork(&pointer.artwork_id)?
                .ok_or(CaricatureError::InvalidSource)?;
            out.push(Publication {
                artwork,
                publish_seq: pointer.publish_seq,
                published_at_unix: pointer.published_at_unix,
            });
        }
        if let Some(latest) = self.latest()? {
            if !out.iter().any(|p| p.publish_seq == latest.publish_seq) {
                out.push(latest);
            }
        }
        out.sort_by_key(|p| p.publish_seq);
        Ok(out)
    }

    pub fn day(&self, jst_day: &str) -> Result<Vec<Publication>, CaricatureError> {
        let mut entries = self.history()?;
        entries.retain(|p| p.artwork.jst_day().ok().as_deref() == Some(jst_day));
        Ok(entries)
    }

    pub fn month(&self, jst_month: &str) -> Result<Vec<Publication>, CaricatureError> {
        let bytes = jst_month.as_bytes();
        if bytes.len() != 7
            || bytes[4] != b'-'
            || !bytes[..4].iter().chain(&bytes[5..]).all(u8::is_ascii_digit)
            || !(1..=12).contains(&jst_month[5..].parse::<u8>().unwrap_or(0))
        {
            return Err(CaricatureError::InvalidRequest);
        }
        let mut entries = self.history()?;
        entries.retain(|p| {
            p.artwork
                .jst_day()
                .is_ok_and(|day| day.starts_with(jst_month))
        });
        Ok(entries)
    }

    /// Atomic latest switch occurs only after immutable bytes are reread and
    /// validated. A failure keeps the old latest and all older history intact.
    pub fn publish(&self, artwork: Artwork, now_unix: i64) -> Result<Publication, CaricatureError> {
        self.publish_inner(artwork, None, now_unix)
    }

    pub fn publish_with_original(
        &self,
        artwork: Artwork,
        original: &[u8],
        now_unix: i64,
    ) -> Result<Publication, CaricatureError> {
        self.publish_inner(artwork, Some(original), now_unix)
    }

    fn publish_inner(
        &self,
        artwork: Artwork,
        original: Option<&[u8]>,
        now_unix: i64,
    ) -> Result<Publication, CaricatureError> {
        artwork.validate()?;
        match (&artwork.original_sha256, original, artwork.original_codec) {
            (Some(expected), Some(bytes), Some(OriginalCodec::HalfWebpQ90))
                if sha256_hex(bytes) == *expected
                    && super::originals::validate_half_webp(bytes, None).is_ok() => {}
            (Some(expected), Some(bytes), None)
                if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && (1..=20_000_000).contains(&bytes.len())
                    && sha256_hex(bytes) == *expected
                    && image::load_from_memory(bytes).is_ok() => {}
            (None, None, None) => {}
            _ => return Err(CaricatureError::InvalidSource),
        }
        if let Some(latest) = self.latest()? {
            if latest.artwork.id == artwork.id {
                self.reconcile_latest()?;
                return if latest.artwork == artwork {
                    Ok(latest)
                } else {
                    Err(CaricatureError::InvalidSource)
                };
            }
        }
        let existing = self.history()?;
        if existing.iter().any(|p| p.artwork.id == artwork.id) {
            return Err(CaricatureError::InvalidSource);
        }
        let published: Vec<_> = existing
            .iter()
            .flat_map(|p| {
                p.artwork
                    .metadata
                    .source_urls
                    .iter()
                    .map(move |url| PublishedSource {
                        source_url: url.clone(),
                        published_at_unix: p.published_at_unix,
                    })
            })
            .collect();
        let recent = recent_source_keys(&published, now_unix);
        for source in &artwork.metadata.source_urls {
            ensure_unused_source(source, &recent)?;
        }
        let path = self.artwork_path(&artwork.id)?;
        fs::create_dir_all(path.parent().ok_or(CaricatureError::Io)?)
            .map_err(|_| CaricatureError::Io)?;
        fs::create_dir_all(self.root.join("publications")).map_err(|_| CaricatureError::Io)?;
        let data = serde_json::to_vec(&artwork).map_err(|_| CaricatureError::InvalidSource)?;
        if path.exists() {
            if fs::read(&path).map_err(|_| CaricatureError::Io)? != data {
                return Err(CaricatureError::InvalidSource);
            }
        } else {
            write_new_sync(&path, &data)?;
        }
        let png_path = self
            .root
            .join("archive")
            .join(format!("{}.png", artwork.id));
        if png_path.exists() {
            if fs::read(&png_path).map_err(|_| CaricatureError::Io)? != artwork.eink_png {
                return Err(CaricatureError::InvalidSource);
            }
        } else {
            write_new_sync(&png_path, &artwork.eink_png)?;
        }
        if let Some(original) = original {
            let original_path = self.original_path(&artwork)?;
            if original_path.exists() {
                if fs::read(&original_path).map_err(|_| CaricatureError::Io)? != original {
                    return Err(CaricatureError::InvalidSource);
                }
            } else {
                write_new_sync(&original_path, original)?;
            }
            if self.original(&artwork)?.as_deref() != Some(original) {
                return Err(CaricatureError::InvalidSource);
            }
        }
        if self.artwork(&artwork.id)?.as_ref() != Some(&artwork) {
            return Err(CaricatureError::InvalidSource);
        }
        let next_seq = self
            .latest()?
            .map_or(1, |p| p.publish_seq.saturating_add(1));
        let pointer = LatestPointer {
            artwork_id: artwork.id.clone(),
            publish_seq: next_seq,
            published_at_unix: now_unix,
        };
        let pointer_bytes =
            serde_json::to_vec(&pointer).map_err(|_| CaricatureError::InvalidSource)?;
        let history_path = self
            .root
            .join("publications")
            .join(format!("{next_seq:020}.json"));
        // A failed pre-commit attempt may have left an unpublished entry for
        // this sequence. The latest pointer, never that entry, defines commit.
        if history_path.exists() {
            fs::remove_file(&history_path).map_err(|_| CaricatureError::Io)?;
        }
        atomic_replace(&self.root.join("latest.json"), &pointer_bytes)?;
        // Publication has committed. The index is recoverable from latest;
        // failure here cannot turn a published artwork into a failed job.
        let _ = write_new_sync(&history_path, &pointer_bytes);
        Ok(Publication {
            artwork,
            publish_seq: next_seq,
            published_at_unix: now_unix,
        })
    }
}

fn write_new_sync(path: &Path, data: &[u8]) -> Result<(), CaricatureError> {
    let temp = temporary_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| CaricatureError::Io)?;
    if file.write_all(data).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(CaricatureError::Io);
    }
    if fs::hard_link(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(CaricatureError::Io);
    }
    let _ = fs::remove_file(&temp);
    sync_parent(path)
}

fn atomic_replace(path: &Path, data: &[u8]) -> Result<(), CaricatureError> {
    let temp = temporary_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| CaricatureError::Io)?;
    if file.write_all(data).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(CaricatureError::Io);
    }
    if fs::rename(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(CaricatureError::Io);
    }
    sync_parent(path)
}

fn temporary_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("pending-{}-{nanos}", std::process::id()))
}

fn sync_parent(path: &Path) -> Result<(), CaricatureError> {
    File::open(path.parent().ok_or(CaricatureError::Io)?)
        .and_then(|f| f.sync_all())
        .map_err(|_| CaricatureError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caricature::{
        ArtworkMetadata, GenerationInput, GenerationSettings, HEIGHT_LCD, WIDTH_LCD,
    };
    fn art(id: &str, ts: i64) -> Artwork {
        let png = crate::caricature::test_canonical_png();
        Artwork {
            id: id.into(),
            metadata: ArtworkMetadata {
                job_id: id.into(),
                slot: 0,
                created_at_unix: ts,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: None,
            original_codec: None,
            eink_png: png,
            lcd_rgb565be: vec![0; WIDTH_LCD * HEIGHT_LCD * 2],
        }
    }
    #[test]
    fn immutable_history_and_atomic_latest() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf());
        let first = store.publish(art("first", 0), 0).unwrap();
        let second = store.publish(art("second", 86_400), 86_400).unwrap();
        assert_eq!(
            store.latest().unwrap().unwrap().artwork.id,
            second.artwork.id
        );
        assert_eq!(store.day("1970-01-01").unwrap().len(), 1);
        assert_eq!(store.history().unwrap().len(), 2);
        let mut collision = first.artwork;
        collision.lcd_rgb565be[0] = 1;
        assert_eq!(
            store.publish(collision, 86_401),
            Err(CaricatureError::InvalidSource)
        );
        assert_eq!(store.latest().unwrap().unwrap().artwork.id, "second");
    }
    #[test]
    fn original_sidecar_is_immutable_and_legacy_schema_stays_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf());
        let legacy = store.publish(art("legacy", 0), 0).unwrap();
        assert_eq!(store.original(&legacy.artwork).unwrap(), None);
        let json = fs::read(dir.path().join("archive/legacy.json")).unwrap();
        assert!(!String::from_utf8_lossy(&json).contains("original_sha256"));
        assert_eq!(
            serde_json::from_slice::<Artwork>(&json).unwrap(),
            legacy.artwork
        );

        let source = crate::caricature::test_canonical_png();
        let mut newer = art("newer", 86_400);
        newer.original_sha256 = Some(sha256_hex(&source));
        assert_eq!(
            store.publish(newer.clone(), 86_400),
            Err(CaricatureError::InvalidSource)
        );
        assert_eq!(store.latest().unwrap().unwrap().artwork.id, "legacy");
        let published = store
            .publish_with_original(newer.clone(), &source, 86_400)
            .unwrap();
        assert_eq!(
            store.original(&published.artwork).unwrap(),
            Some(source.clone())
        );
        assert_eq!(store.history().unwrap().len(), 2);
        assert_eq!(
            store.publish_with_original(newer, &legacy.artwork.eink_png[..10], 86_401),
            Err(CaricatureError::InvalidSource)
        );
        fs::remove_file(dir.path().join("archive/newer.original.png")).unwrap();
        assert_eq!(store.original(&published.artwork).unwrap(), None);
        assert!(store.artwork("newer").unwrap().is_some());
    }
    #[cfg(unix)]
    #[test]
    fn index_write_failure_after_latest_commit_is_reconciled() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf());
        store.publish(art("first", 0), 0).unwrap();
        let index_dir = dir.path().join("publications");
        fs::set_permissions(&index_dir, fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.publish(art("second", 1), 1);
        fs::set_permissions(&index_dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result.unwrap().artwork.id, "second");
        assert_eq!(store.latest().unwrap().unwrap().artwork.id, "second");
        assert_eq!(store.history().unwrap().len(), 2);
    }
}
