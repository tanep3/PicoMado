//! Small, single-pass prompt templates kept beside the caricature's durable data.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: u64 = 32 * 1024;
const NEWS_DEFAULT: &str = include_str!("../../assets/caricature-news-prompt.txt");
const IMAGE_DEFAULT: &str = include_str!("../../assets/caricature-image-prompt.txt");
const HEADING_DEFAULT: &str = include_str!("../../assets/caricature-heading-prompt.txt");
const SNAPSHOT_LIMIT: u64 = 20 * LIMIT;
const NEWS_KEYS: &[&str] = &["topic", "utc_time", "recent_sources", "feedback"];
const IMAGE_KEYS: &[&str] = &["style", "brief", "work_dir", "feedback"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptSnapshot {
    pub news: String,
    pub image: String,
    #[serde(default = "default_heading")]
    pub heading: String,
    pub warning: Option<String>,
}

fn default_heading() -> String {
    HEADING_DEFAULT.into()
}

impl PromptSnapshot {
    pub fn saved_warning(job_dir: &Path) -> Option<String> {
        let data = read_limited(&job_dir.join("prompt-templates.json"), SNAPSHOT_LIMIT).ok()?;
        serde_json::from_slice::<Self>(&data).ok()?.warning
    }

    pub fn load(root: &Path, job_dir: &Path) -> Result<Self, &'static str> {
        fs::create_dir_all(job_dir).map_err(|_| "prompt template snapshot unavailable")?;
        let snapshot_path = job_dir.join("prompt-templates.json");
        if snapshot_path.exists() {
            let data = read_limited(&snapshot_path, SNAPSHOT_LIMIT)
                .map_err(|_| "saved prompt template snapshot unavailable")?;
            let snapshot: Self = serde_json::from_slice(&data)
                .map_err(|_| "saved prompt template snapshot invalid")?;
            validate(&snapshot.news, NEWS_KEYS)
                .map_err(|_| "saved news prompt template invalid")?;
            validate(&snapshot.image, IMAGE_KEYS)
                .map_err(|_| "saved image prompt template invalid")?;
            validate(&snapshot.heading, &[])
                .map_err(|_| "saved heading prompt template invalid")?;
            return Ok(snapshot);
        }
        let (news, news_warning) = load_one(root, "news.txt", NEWS_DEFAULT, NEWS_KEYS);
        let (image, image_warning) = load_one(root, "image.txt", IMAGE_DEFAULT, IMAGE_KEYS);
        let (heading, heading_warning) = load_one(root, "heading.txt", HEADING_DEFAULT, &[]);
        let warnings: Vec<_> = [news_warning, image_warning, heading_warning]
            .into_iter()
            .flatten()
            .collect();
        let snapshot = Self {
            news,
            image,
            heading,
            warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        };
        let data = serde_json::to_vec(&snapshot).map_err(|_| "prompt template snapshot invalid")?;
        atomic_write(&snapshot_path, &data).map_err(|_| "prompt template snapshot unavailable")?;
        Ok(snapshot)
    }

    pub fn news_prompt(&self, fields: &[(&str, String)]) -> Result<String, &'static str> {
        expand(&self.news, NEWS_KEYS, fields)
    }

    pub fn image_prompt(&self, fields: &[(&str, String)]) -> Result<String, &'static str> {
        expand(&self.image, IMAGE_KEYS, fields)
    }
}

fn load_one(root: &Path, name: &str, default: &str, keys: &[&str]) -> (String, Option<String>) {
    let path = root.join(name);
    if fs::create_dir_all(root).is_ok() && !path.exists() {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut file) = opts.open(&path) {
            let _ = file.write_all(default.as_bytes());
            let _ = file.sync_all();
        }
    }
    match read_template(&path, keys) {
        Ok(source) => {
            let backup = root.join(format!(".{name}.last-good"));
            let warning = atomic_write(&backup, source.as_bytes())
                .err()
                .map(|_| format!("{name}: could not save last valid template"));
            (source, warning)
        }
        Err(reason) => {
            let backup = root.join(format!(".{name}.last-good"));
            if let Ok(source) = read_template(&backup, keys) {
                (
                    source,
                    Some(format!("{name}: {reason}; using last valid template")),
                )
            } else {
                (
                    default.into(),
                    Some(format!("{name}: {reason}; using bundled default")),
                )
            }
        }
    }
}

fn read_template(path: &Path, keys: &[&str]) -> Result<String, &'static str> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("symbolic link rejected");
    }
    let data = read_limited(path, LIMIT)?;
    let source = String::from_utf8(data).map_err(|_| "not UTF-8")?;
    validate(&source, keys)?;
    Ok(source)
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, &'static str> {
    let file = File::open(path).map_err(|_| "file unavailable")?;
    let mut data = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|_| "file unavailable")?;
    if data.len() as u64 > limit {
        return Err("file too large");
    }
    Ok(data)
}

fn validate(source: &str, keys: &[&str]) -> Result<(), &'static str> {
    if source.trim().is_empty() {
        return Err("empty template");
    }
    let fields: Vec<_> = keys.iter().map(|key| (*key, String::new())).collect();
    expand(source, keys, &fields).map(|_| ())
}

fn expand(source: &str, keys: &[&str], fields: &[(&str, String)]) -> Result<String, &'static str> {
    let mut rest = source;
    let mut output = String::new();
    let mut seen = Vec::new();
    while let Some(start) = rest.find("{{") {
        if rest[..start].contains("}}") {
            return Err("unmatched closing placeholder");
        }
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail.find("}}").ok_or("unclosed placeholder")?;
        let key = &tail[..end];
        if !keys.contains(&key) {
            return Err("unknown placeholder");
        }
        if seen.contains(&key) {
            return Err("duplicate placeholder");
        }
        let value = fields
            .iter()
            .find(|(name, _)| *name == key)
            .ok_or("missing placeholder value")?;
        output.push_str(&value.1);
        seen.push(key);
        rest = &tail[end + 2..];
    }
    if rest.contains("}}") {
        return Err("unmatched closing placeholder");
    }
    output.push_str(rest);
    if seen.len() != keys.len() {
        return Err("required placeholder missing");
    }
    Ok(output)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), ()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())?
        .as_nanos();
    let tmp = path.with_extension(format!("tmp-{}-{nanos}", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp).map_err(|_| ())?;
    if file.write_all(data).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(());
    }
    if fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_to_new_jobs_and_retries_keep_their_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("templates");
        let first_dir = temp.path().join("job1");
        fs::create_dir_all(&first_dir).unwrap();
        let first = PromptSnapshot::load(&root, &first_dir).unwrap();
        assert!(root.join("news.txt").exists());
        assert!(root.join("image.txt").exists());
        assert!(root.join("heading.txt").exists());
        assert!(first.warning.is_none());
        let updated = IMAGE_DEFAULT.replace("作品を1枚", "編集済みの作品を1枚");
        fs::write(root.join("image.txt"), updated).unwrap();
        let updated_news = NEWS_DEFAULT.replace("Webを調べ", "編集後に調べ");
        fs::write(root.join("news.txt"), updated_news).unwrap();
        fs::write(root.join("heading.txt"), "編集した見出し指示。").unwrap();
        let second_dir = temp.path().join("job2");
        fs::create_dir_all(&second_dir).unwrap();
        let second = PromptSnapshot::load(&root, &second_dir).unwrap();
        assert!(second.image.contains("編集済みの作品を1枚"));
        assert!(second.news.contains("編集後に調べ"));
        assert_eq!(second.heading, "編集した見出し指示。");
        assert_eq!(
            PromptSnapshot::load(&root, &first_dir).unwrap().heading,
            HEADING_DEFAULT
        );
        assert!(!PromptSnapshot::load(&root, &first_dir)
            .unwrap()
            .image
            .contains("編集済みの作品を1枚"));
        assert!(!PromptSnapshot::load(&root, &first_dir)
            .unwrap()
            .news
            .contains("編集後に調べ"));
    }

    #[test]
    fn invalid_edit_uses_last_good_and_first_invalid_uses_bundled_default() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("templates");
        let first_dir = temp.path().join("job1");
        fs::create_dir_all(&first_dir).unwrap();
        let first = PromptSnapshot::load(&root, &first_dir).unwrap();
        let edited = first.news.replace("Webを調べ", "編集後に調べ");
        fs::write(root.join("news.txt"), &edited).unwrap();
        let valid_dir = temp.path().join("job2");
        fs::create_dir_all(&valid_dir).unwrap();
        PromptSnapshot::load(&root, &valid_dir).unwrap();
        fs::write(root.join("news.txt"), "{{unknown}}").unwrap();
        let bad_dir = temp.path().join("job3");
        fs::create_dir_all(&bad_dir).unwrap();
        let restored = PromptSnapshot::load(&root, &bad_dir).unwrap();
        assert_eq!(restored.news, edited);
        assert!(restored.warning.unwrap().contains("last valid"));

        fs::write(root.join("image.txt"), "{{style}} {{unknown}}").unwrap();
        let bad_image_dir = temp.path().join("job4");
        let image_fallback = PromptSnapshot::load(&root, &bad_image_dir).unwrap();
        assert_eq!(image_fallback.image, IMAGE_DEFAULT);
        assert!(image_fallback.warning.unwrap().contains("image.txt"));

        let fresh = temp.path().join("fresh");
        fs::create_dir_all(&fresh).unwrap();
        let fresh_root = temp.path().join("fresh_templates");
        fs::create_dir_all(&fresh_root).unwrap();
        fs::write(fresh_root.join("news.txt"), "{{topic}}").unwrap();
        let fallback = PromptSnapshot::load(&fresh_root, &fresh).unwrap();
        assert_eq!(fallback.news, NEWS_DEFAULT);
        assert!(fallback.warning.unwrap().contains("bundled default"));
    }

    #[test]
    fn placeholders_are_strict_and_inserted_values_are_not_reexpanded() {
        assert_eq!(
            validate("{{topic}}", NEWS_KEYS),
            Err("required placeholder missing")
        );
        assert_eq!(
            validate("{{topic}}{{topic}}", NEWS_KEYS),
            Err("duplicate placeholder")
        );
        assert_eq!(validate("{{topic", NEWS_KEYS), Err("unclosed placeholder"));
        assert_eq!(
            validate(
                "{{topic}}{{utc_time}}{{recent_sources}}{{feedback}}{{oops}}",
                NEWS_KEYS
            ),
            Err("unknown placeholder")
        );
        let output = expand(
            "{{topic}} {{utc_time}} {{recent_sources}} {{feedback}}",
            NEWS_KEYS,
            &[
                ("topic", "literal {{feedback}}".into()),
                ("utc_time", "1".into()),
                ("recent_sources", "[]".into()),
                ("feedback", "none".into()),
            ],
        )
        .unwrap();
        assert_eq!(output, "literal {{feedback}} 1 [] none");
    }

    #[test]
    fn heading_invalid_edit_falls_back_and_old_snapshots_keep_the_original_instruction() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("templates");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("heading.txt"), "   \n").unwrap();
        let initial = PromptSnapshot::load(&root, &temp.path().join("first")).unwrap();
        assert_eq!(initial.heading, HEADING_DEFAULT);
        assert!(initial
            .warning
            .unwrap()
            .contains("heading.txt: empty template; using bundled default"));
        fs::write(root.join("heading.txt"), "日本語で見出しを書いてください。").unwrap();
        let good = PromptSnapshot::load(&root, &temp.path().join("good")).unwrap();
        fs::write(root.join("heading.txt"), "{{unknown}}").unwrap();
        let fallback = PromptSnapshot::load(&root, &temp.path().join("bad")).unwrap();
        assert_eq!(fallback.heading, good.heading);
        assert!(fallback
            .warning
            .unwrap()
            .contains("heading.txt: unknown placeholder; using last valid"));
        let mut old = serde_json::to_value(&good).unwrap();
        old.as_object_mut().unwrap().remove("heading");
        let old_dir = temp.path().join("old");
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(
            old_dir.join("prompt-templates.json"),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        assert_eq!(
            PromptSnapshot::load(&root, &old_dir).unwrap().heading,
            HEADING_DEFAULT
        );
    }
}
