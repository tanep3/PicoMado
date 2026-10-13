//! Private, durable record of each submitted Codex prompt and its outcome.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::hash::sha256_hex;
use super::GenerationJob;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptRecord {
    pub job_id: String,
    pub slot: i64,
    pub stage: String,
    pub attempt: u32,
    pub model: String,
    pub submitted_prompt: String,
    pub prompt_sha256: String,
    pub argv_without_prompt: Vec<String>,
    pub prepared_at_unix: i64,
    pub finished_at_unix: Option<i64>,
    pub cli_returncode: Option<i32>,
    pub status: String,
    pub artifact_kind: Option<String>,
    pub artifact_sha256: Option<String>,
}

pub struct PromptEvidence {
    path: PathBuf,
    record: PromptRecord,
}

impl PromptEvidence {
    pub fn prepare(
        root: &Path,
        job: &GenerationJob,
        stage: &str,
        attempt: u32,
        model: &str,
        prompt: &str,
        command: &Command,
    ) -> Result<Self, &'static str> {
        let mut args: Vec<String> =
            std::iter::once(command.get_program().to_string_lossy().into_owned())
                .chain(
                    command
                        .get_args()
                        .map(|arg| arg.to_string_lossy().into_owned()),
                )
                .collect();
        if args.pop().as_deref() != Some(prompt) {
            return Err("prompt and command mismatch");
        }
        let dir = root.join(&job.request.job_id);
        fs::create_dir_all(&dir).map_err(|_| "prompt evidence unavailable")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                .map_err(|_| "prompt evidence unavailable")?;
        }
        let path = dir.join(format!("{stage}-{attempt}.json"));
        let record = PromptRecord {
            job_id: job.request.job_id.clone(),
            slot: job.request.slot,
            stage: stage.into(),
            attempt,
            model: model.into(),
            submitted_prompt: prompt.into(),
            prompt_sha256: sha256_hex(prompt.as_bytes()),
            argv_without_prompt: args,
            prepared_at_unix: unix_now(),
            finished_at_unix: None,
            cli_returncode: None,
            status: "prepared".into(),
            artifact_kind: None,
            artifact_sha256: None,
        };
        let value = Self { path, record };
        value.save()?;
        Ok(value)
    }

    pub fn process_finished(
        &mut self,
        status: &str,
        code: Option<i32>,
    ) -> Result<(), &'static str> {
        self.record.status = status.into();
        self.record.cli_returncode = code;
        self.record.finished_at_unix = Some(unix_now());
        self.save()
    }

    pub fn accepted(&mut self, kind: &str, bytes: &[u8]) -> Result<(), &'static str> {
        self.record.status = "artifact_returned".into();
        self.record.finished_at_unix = Some(unix_now());
        self.record.artifact_kind = Some(kind.into());
        self.record.artifact_sha256 = Some(sha256_hex(bytes));
        self.save()
    }

    pub fn rejected(&mut self) -> Result<(), &'static str> {
        self.record.status = "failed".into();
        self.record.finished_at_unix = Some(unix_now());
        self.save()
    }

    fn save(&self) -> Result<(), &'static str> {
        let data = serde_json::to_vec(&self.record).map_err(|_| "prompt evidence invalid")?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = self
            .path
            .with_extension(format!("pending-{}-{nanos}", std::process::id()));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts
            .open(&temp)
            .map_err(|_| "prompt evidence unavailable")?;
        if file.write_all(&data).and_then(|_| file.sync_all()).is_err() {
            let _ = fs::remove_file(&temp);
            return Err("prompt evidence unavailable");
        }
        if fs::rename(&temp, &self.path).is_err() {
            let _ = fs::remove_file(&temp);
            return Err("prompt evidence unavailable");
        }
        File::open(self.path.parent().ok_or("prompt evidence unavailable")?)
            .and_then(|f| f.sync_all())
            .map_err(|_| "prompt evidence unavailable")?;
        Ok(())
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caricature::{GenerationInput, GenerationRequest, GenerationSettings};
    #[test]
    fn records_prompt_command_result_and_artifact_hash() {
        let dir = tempfile::tempdir().unwrap();
        let job = GenerationJob::accept(GenerationRequest {
            job_id: "proof".into(),
            slot: 0,
            requested_at_unix: 0,
            input: GenerationInput::News,
            settings: GenerationSettings::default(),
        })
        .unwrap();
        let mut command = Command::new("codex");
        command.args(["exec", "--model", "gpt-6-luna", "prompt"]);
        let mut proof = PromptEvidence::prepare(
            dir.path(),
            &job,
            "news",
            1,
            "gpt-6-luna",
            "prompt",
            &command,
        )
        .unwrap();
        proof.process_finished("exited", Some(0)).unwrap();
        proof.accepted("news_json", b"abc").unwrap();
        let record: PromptRecord =
            serde_json::from_slice(&fs::read(dir.path().join("proof/news-1.json")).unwrap())
                .unwrap();
        assert_eq!(record.status, "artifact_returned");
        assert_eq!(record.cli_returncode, Some(0));
        assert_eq!(
            record.argv_without_prompt,
            ["codex", "exec", "--model", "gpt-6-luna"]
        );
        assert_eq!(record.prompt_sha256, sha256_hex(b"prompt"));
        assert_eq!(
            record.artifact_sha256.as_deref(),
            Some(sha256_hex(b"abc").as_str())
        );
        assert!(record.finished_at_unix.is_some());
    }
}
