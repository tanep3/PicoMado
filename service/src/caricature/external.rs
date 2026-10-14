//! Blocking Codex CLI and DPID work runs on a private worker thread. Call this
//! outside the application mutex. No HTTP/JMA fetch slot is consumed here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::evidence::PromptEvidence;
use super::hash::sha256_hex;
use super::templates::PromptSnapshot;
use super::{
    ensure_unused_source, Artwork, ArtworkMetadata, CaricatureError, GenerationInput,
    GenerationJob, GenerationStage, GenerationState, News, RetryPolicy, HEIGHT_EINK, HEIGHT_LCD,
    WIDTH_EINK, WIDTH_LCD,
};
use image::GenericImageView;

const NEWS_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["source_url","summary"],"properties":{"source_url":{"type":["string","null"]},"summary":{"type":["string","null"]}}}"#;
const MAX_PNG: usize = 20_000_000;

#[derive(Clone, Debug)]
pub struct ExternalOptions {
    pub codex: PathBuf,
    pub python: PathBuf,
    pub generated_root: PathBuf,
    pub work_root: PathBuf,
    /// Private durable records of the exact submitted prompts and models.
    pub evidence_root: PathBuf,
    pub template_root: PathBuf,
    pub cancel: Arc<AtomicBool>,
}

impl ExternalOptions {
    pub fn new(work_root: PathBuf, evidence_root: PathBuf, generated_root: PathBuf) -> Self {
        let template_root = work_root
            .parent()
            .unwrap_or(&work_root)
            .join("prompt_templates");
        Self {
            codex: "codex".into(),
            python: "python3".into(),
            generated_root,
            work_root,
            evidence_root,
            template_root,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GenerationResult {
    Validated {
        job: Box<GenerationJob>,
        artwork: Box<Artwork>,
        warning: Option<String>,
    },
    Failed {
        job: Box<GenerationJob>,
        stage: GenerationStage,
        reason: String,
    },
}

#[derive(Debug)]
struct WorkError(&'static str);

#[derive(Clone, Copy, Debug)]
struct CommandFailure {
    reason: &'static str,
    code: Option<i32>,
}

impl From<CommandFailure> for WorkError {
    fn from(value: CommandFailure) -> Self {
        Self(value.reason)
    }
}

fn process_evidence(result: &Result<i32, CommandFailure>) -> (&'static str, Option<i32>) {
    match result {
        Ok(code) => ("exited", Some(*code)),
        Err(failure) => {
            let status = match failure.reason {
                "generation cancelled" => "interrupted",
                "process timed out" => "timeout",
                "process launch failed"
                | "process output unavailable"
                | "process error capture unavailable" => "launch_error",
                _ => "failed",
            };
            (status, failure.code)
        }
    }
}

impl From<CaricatureError> for WorkError {
    fn from(_: CaricatureError) -> Self {
        Self("invalid generation data")
    }
}

/// Generate only. Publication is a separate, atomic host action. The returned
/// `job` contains attempt counters and reusable selected news for persistence.
pub async fn generate(
    job: GenerationJob,
    recent_sources: BTreeSet<String>,
    options: ExternalOptions,
) -> GenerationResult {
    let failed_job = job.clone();
    let cancel = options.cancel.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        let job_dir = options.work_root.join(&job.request.job_id);
        let mut result = generate_blocking(job, &recent_sources, &options);
        if let GenerationResult::Failed { reason, .. } = &mut result {
            if let Some(warning) = PromptSnapshot::saved_warning(&job_dir) {
                *reason = format!("{warning}; {reason}");
            }
        }
        let _ = sender.send(result);
    });
    let mut guard = WorkerGuard {
        cancel,
        thread: Some(thread),
    };
    let result = receiver.await.unwrap_or_else(|_| {
        failure(
            failed_job,
            GenerationStage::Image,
            WorkError("worker interrupted"),
        )
    });
    if let Some(thread) = guard.thread.take() {
        reap_thread(thread);
    }
    result
}

struct WorkerGuard {
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.cancel.store(true, Ordering::SeqCst);
            thread.thread().unpark();
            reap_thread(thread);
        }
    }
}

fn reap_thread(thread: std::thread::JoinHandle<()>) {
    let _ = std::thread::Builder::new()
        .name("caricature-reaper".into())
        .spawn(move || {
            let _ = thread.join();
        });
}

fn generate_blocking(
    mut job: GenerationJob,
    recent: &BTreeSet<String>,
    options: &ExternalOptions,
) -> GenerationResult {
    if options.cancel.load(Ordering::SeqCst) {
        return failure(
            job,
            GenerationStage::Image,
            WorkError("generation cancelled"),
        );
    }
    if let Err(e) = job.request.validate() {
        return failure(job, GenerationStage::News, e.into());
    }
    if !job.recoverable() {
        return failure(job, GenerationStage::Image, WorkError("job is final"));
    }
    if let Err(e) = fs::create_dir_all(&options.work_root) {
        let _ = e;
        return failure(
            job,
            GenerationStage::Image,
            WorkError("work directory unavailable"),
        );
    }
    match load_checkpoint(&job, options) {
        Ok(Some(saved)) => job = saved,
        Ok(None) => {}
        Err(e) => return failure(job, GenerationStage::Image, e),
    }
    let templates = match PromptSnapshot::load(
        &options.template_root,
        &options.work_root.join(&job.request.job_id),
    ) {
        Ok(value) => value,
        Err(message) => return failure(job, GenerationStage::Image, WorkError(message)),
    };
    let artifact_path = options
        .work_root
        .join(&job.request.job_id)
        .join("validated-artwork.json");
    if artifact_path.exists() {
        if let Ok(data) = fs::read(&artifact_path) {
            if let Ok(artwork) = serde_json::from_slice::<Artwork>(&data) {
                if artwork.metadata.job_id == job.request.job_id && artwork.validate().is_ok() {
                    job.state = GenerationState::Validated;
                    return GenerationResult::Validated {
                        job: Box::new(job),
                        artwork: Box::new(artwork),
                        warning: templates.warning,
                    };
                }
            }
        }
        return failure(
            job,
            GenerationStage::Image,
            WorkError("saved validated artwork invalid"),
        );
    }
    if let Err(e) = preflight(options) {
        return failure(job, GenerationStage::Image, e);
    }
    let news = match &job.request.input {
        GenerationInput::CustomText(_) => None,
        GenerationInput::News => {
            if let Some(existing) = job.news.clone() {
                if validate_news(&existing, recent).is_err() {
                    return failure(
                        job,
                        GenerationStage::News,
                        WorkError("saved news source is unavailable or already used"),
                    );
                }
                Some(existing)
            } else {
                job.state = GenerationState::Researching;
                let policy = job.request.settings.news_retry.clone();
                let mut last = WorkError("news attempts exhausted");
                let stage_started = Instant::now();
                while job.news_attempts <= policy.retry_count {
                    let Some(timeout) = next_timeout(&policy, stage_started) else {
                        last = WorkError("news deadline reached");
                        break;
                    };
                    job.news_attempts += 1;
                    let deadline = Instant::now() + timeout;
                    if let Err(e) = checkpoint_job(&job, options) {
                        return failure(job, GenerationStage::News, e);
                    }
                    match news_attempt(&job, recent, options, &templates, deadline, last.0) {
                        Ok(selected) => {
                            job.news = Some(selected.clone());
                            job.state = GenerationState::NewsReady;
                            if let Err(e) = checkpoint_job(&job, options) {
                                return failure(job, GenerationStage::News, e);
                            }
                            break;
                        }
                        Err(err) => {
                            last = err;
                            if job.news_attempts <= policy.retry_count
                                && wait_cancelable(
                                    Duration::from_secs(policy.interval_seconds as u64),
                                    &options.cancel,
                                    stage_deadline(&policy, stage_started),
                                )
                                .is_err()
                            {
                                return failure(
                                    job,
                                    GenerationStage::News,
                                    WorkError("generation cancelled or news deadline reached"),
                                );
                            }
                        }
                    }
                }
                match job.news.clone() {
                    Some(news) => Some(news),
                    None => return failure(job, GenerationStage::News, last),
                }
            }
        }
    };
    let policy = job.request.settings.image_retry.clone();
    let stage_started = Instant::now();
    let mut last = WorkError("image attempts exhausted");
    while job.image_attempts <= policy.retry_count {
        let Some(timeout) = next_timeout(&policy, stage_started) else {
            last = WorkError("image deadline reached");
            break;
        };
        job.image_attempts += 1;
        job.state = GenerationState::Drawing;
        let deadline = Instant::now() + timeout;
        if let Err(e) = checkpoint_job(&job, options) {
            return failure(job, GenerationStage::Image, e);
        }
        match image_attempt(&job, news.as_ref(), options, &templates, deadline, last.0) {
            Ok(png) => match normalize(options, &png, deadline, &job) {
                Ok((eink_png, lcd_rgb565be)) => {
                    let source_urls = news
                        .as_ref()
                        .map(|n| vec![n.source_url.clone()])
                        .unwrap_or_default();
                    let artwork = Artwork {
                        id: format!("{}-{}", job.request.job_id, job.image_attempts),
                        metadata: ArtworkMetadata {
                            job_id: job.request.job_id.clone(),
                            slot: job.request.slot,
                            created_at_unix: unix_now(),
                            input: job.request.input.clone(),
                            source_urls,
                            news_summary: news.as_ref().map(|n| n.summary.clone()),
                            settings: job.request.settings.clone(),
                        },
                        original_sha256: Some(sha256_hex(&png)),
                        original_codec: None,
                        eink_png,
                        lcd_rgb565be,
                    };
                    if artwork.validate().is_err() {
                        last = WorkError("normalized artwork invalid");
                    } else if news
                        .as_ref()
                        .is_some_and(|n| ensure_unused_source(&n.source_url, recent).is_err())
                    {
                        return failure(
                            job,
                            GenerationStage::Image,
                            WorkError("source published in last 24 hours"),
                        );
                    } else {
                        job.state = GenerationState::Validated;
                        let data = match serde_json::to_vec(&artwork) {
                            Ok(v) => v,
                            Err(_) => {
                                return failure(
                                    job,
                                    GenerationStage::Image,
                                    WorkError("validated artwork serialization failed"),
                                )
                            }
                        };
                        if let Err(e) = write_private_atomic(&artifact_path, &data) {
                            return failure(job, GenerationStage::Image, e);
                        }
                        if let Err(e) = checkpoint_job(&job, options) {
                            return failure(job, GenerationStage::Image, e);
                        }
                        return GenerationResult::Validated {
                            job: Box::new(job),
                            artwork: Box::new(artwork),
                            warning: templates.warning,
                        };
                    }
                }
                Err(err) => last = err,
            },
            Err(err) => last = err,
        }
        if job.image_attempts <= policy.retry_count
            && wait_cancelable(
                Duration::from_secs(policy.interval_seconds as u64),
                &options.cancel,
                stage_deadline(&policy, stage_started),
            )
            .is_err()
        {
            return failure(
                job,
                GenerationStage::Image,
                WorkError("generation cancelled or image deadline reached"),
            );
        }
    }
    failure(job, GenerationStage::Image, last)
}

fn checkpoint_job(job: &GenerationJob, options: &ExternalOptions) -> Result<(), WorkError> {
    let dir = options.work_root.join(&job.request.job_id);
    fs::create_dir_all(&dir).map_err(|_| WorkError("job checkpoint unavailable"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|_| WorkError("job checkpoint unavailable"))?;
    }
    let data = serde_json::to_vec(job).map_err(|_| WorkError("job checkpoint invalid"))?;
    write_private_atomic(&dir.join("checkpoint.json"), &data)
}

fn load_checkpoint(
    job: &GenerationJob,
    options: &ExternalOptions,
) -> Result<Option<GenerationJob>, WorkError> {
    let path = options
        .work_root
        .join(&job.request.job_id)
        .join("checkpoint.json");
    let data = match fs::read(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(WorkError("job checkpoint unreadable")),
    };
    let saved: GenerationJob =
        serde_json::from_slice(&data).map_err(|_| WorkError("job checkpoint invalid"))?;
    if saved.request != job.request {
        return Err(WorkError("job checkpoint request mismatch"));
    }
    Ok(Some(saved))
}

fn failure(mut job: GenerationJob, stage: GenerationStage, error: WorkError) -> GenerationResult {
    let reason = error.0.to_string();
    job.state = GenerationState::Failed {
        stage: stage.clone(),
        reason: reason.clone(),
    };
    GenerationResult::Failed {
        job: Box::new(job),
        stage,
        reason,
    }
}

fn next_timeout(policy: &RetryPolicy, started: Instant) -> Option<Duration> {
    let attempt = Duration::from_secs(policy.attempt_timeout_seconds as u64);
    if policy.deadline_seconds == 0 {
        return Some(attempt);
    }
    let remaining =
        Duration::from_secs(policy.deadline_seconds as u64).checked_sub(started.elapsed())?;
    if remaining.is_zero() {
        None
    } else {
        Some(attempt.min(remaining))
    }
}

fn stage_deadline(policy: &RetryPolicy, started: Instant) -> Option<Instant> {
    (policy.deadline_seconds != 0)
        .then(|| started + Duration::from_secs(policy.deadline_seconds as u64))
}

fn wait_cancelable(
    interval: Duration,
    cancel: &AtomicBool,
    stage_until: Option<Instant>,
) -> Result<(), WorkError> {
    let target = Instant::now() + interval;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(WorkError("generation cancelled"));
        }
        let now = Instant::now();
        if stage_until.is_some_and(|end| now >= end && now < target) {
            return Err(WorkError("retry deadline reached"));
        }
        if now >= target {
            return Ok(());
        }
        let remaining = target
            .saturating_duration_since(now)
            .min(Duration::from_millis(25));
        std::thread::park_timeout(remaining);
    }
}

fn preflight(options: &ExternalOptions) -> Result<(), WorkError> {
    let mut python = Command::new(&options.python);
    python.args(["-B", "-c", "import numpy, pepedpid, PIL"]);
    run_command(
        python,
        Instant::now() + Duration::from_secs(20),
        None,
        None,
        &options.cancel,
    )
    .map_err(|_| WorkError("DPID runtime unavailable"))?;
    let features_path = options
        .work_root
        .join(format!("features-{}.txt", std::process::id()));
    let mut features = Command::new(&options.codex);
    features.args(["features", "list"]);
    run_command(
        features,
        Instant::now() + Duration::from_secs(15),
        Some(&features_path),
        None,
        &options.cancel,
    )
    .map_err(|_| WorkError("Codex CLI unavailable"))?;
    let output = bounded_read(&features_path, 100_000)
        .map_err(|_| WorkError("Codex features unavailable"))?;
    let _ = fs::remove_file(features_path);
    if !String::from_utf8_lossy(&output).lines().any(|line| {
        let words: Vec<_> = line.split_whitespace().collect();
        words.first() == Some(&"image_generation") && words.last() == Some(&"true")
    }) {
        return Err(WorkError("Codex image generation unavailable"));
    }
    let mut login = Command::new(&options.codex);
    login.args(["login", "status"]);
    run_command(
        login,
        Instant::now() + Duration::from_secs(15),
        None,
        None,
        &options.cancel,
    )
    .map_err(|_| WorkError("Codex authentication unavailable"))?;
    Ok(())
}

fn attempt_dir(
    job: &GenerationJob,
    stage: &str,
    options: &ExternalOptions,
    attempt: u32,
) -> Result<PathBuf, WorkError> {
    let path = options
        .work_root
        .join(&job.request.job_id)
        .join(stage)
        .join(attempt.to_string());
    fs::create_dir_all(&path).map_err(|_| WorkError("attempt directory unavailable"))?;
    Ok(path)
}

fn news_attempt(
    job: &GenerationJob,
    recent: &BTreeSet<String>,
    options: &ExternalOptions,
    templates: &PromptSnapshot,
    deadline: Instant,
    feedback: &str,
) -> Result<News, WorkError> {
    let work = attempt_dir(job, "news", options, job.news_attempts)?;
    fs::write(work.join("schema.json"), NEWS_SCHEMA)
        .map_err(|_| WorkError("news schema write failed"))?;
    let output = work.join("news.json");
    let _ = fs::remove_file(&output);
    let prompt = news_prompt(
        templates,
        &job.request.settings.topic.prompt,
        recent,
        feedback,
    )?;
    let mut cmd = Command::new(&options.codex);
    cmd.current_dir(&work)
        .args(["exec", "--ephemeral", "--skip-git-repo-check", "--model"])
        .arg(&job.request.settings.news_model)
        .args(["--sandbox", "read-only", "--output-schema"])
        .arg(work.join("schema.json"))
        .arg("--output-last-message")
        .arg(&output)
        .arg(&prompt);
    let mut proof = PromptEvidence::prepare(
        &options.evidence_root,
        job,
        "news",
        job.news_attempts,
        &job.request.settings.news_model,
        &prompt,
        &cmd,
    )
    .map_err(WorkError)?;
    let status = run_command(cmd, deadline, None, None, &options.cancel);
    let (outcome, code) = process_evidence(&status);
    proof.process_finished(outcome, code).map_err(WorkError)?;
    status.map_err(WorkError::from)?;
    let result = (|| {
        let bytes = bounded_read(&output, 10_000)?;
        let news: News =
            serde_json::from_slice(&bytes).map_err(|_| WorkError("news output invalid"))?;
        validate_news(&news, recent)?;
        Ok((news, bytes))
    })();
    match result {
        Ok((news, bytes)) => {
            proof.accepted("news_json", &bytes).map_err(WorkError)?;
            Ok(news)
        }
        Err(e) => {
            proof.rejected().map_err(WorkError)?;
            Err(e)
        }
    }
}

fn news_prompt(
    templates: &PromptSnapshot,
    topic_prompt: &str,
    recent: &BTreeSet<String>,
    feedback: &str,
) -> Result<String, WorkError> {
    templates
        .news_prompt(&[
            ("topic", topic_prompt.into()),
            ("utc_time", unix_now().to_string()),
            (
                "recent_sources",
                serde_json::to_string(recent).unwrap_or_default(),
            ),
            ("feedback", feedback.into()),
        ])
        .map_err(|_| WorkError("news prompt template invalid"))
}

fn validate_news(news: &News, recent: &BTreeSet<String>) -> Result<(), WorkError> {
    if news.summary.trim().is_empty() || news.summary.chars().count() > 4000 {
        return Err(WorkError("news summary invalid"));
    }
    public_source_url(&news.source_url)?;
    ensure_unused_source(&news.source_url, recent)
        .map_err(|_| WorkError("source published in last 24 hours"))
}

fn public_source_url(value: &str) -> Result<(), WorkError> {
    let key = super::source_url_key(value).map_err(|_| WorkError("source URL invalid"))?;
    if key.len() > 2048 {
        return Err(WorkError("source URL invalid"));
    }
    let (_, tail) = key
        .split_once("://")
        .ok_or(WorkError("source URL invalid"))?;
    let authority = tail.split('/').next().unwrap_or("");
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']').ok_or(WorkError("source URL invalid"))?;
        let rest = &authority[end + 1..];
        (&authority[1..end], rest.strip_prefix(':').unwrap_or(""))
    } else {
        authority.split_once(':').unwrap_or((authority, ""))
    };
    if host.is_empty() || !["", "80", "443"].contains(&port) {
        return Err(WorkError("source URL invalid"));
    }
    let port: u16 = if port.is_empty() {
        443
    } else {
        port.parse().map_err(|_| WorkError("source URL invalid"))?
    };
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| WorkError("source DNS unavailable"))?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(WorkError("source URL is not public"));
    }
    Ok(())
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_multicast()
                && !v.is_broadcast()
                && !v.is_unspecified()
                && o[0] != 0
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 192 && o[1] == 0 && (o[2] == 0 || o[2] == 2))
                && !(o[0] == 192 && o[1] == 88 && o[2] == 99)
                && !(o[0] == 198 && (o[1] == 18 || o[1] == 19 || (o[1] == 51 && o[2] == 100)))
                && !(o[0] == 203 && o[1] == 0 && o[2] == 113)
                && o[0] < 224
        }
        IpAddr::V6(v) => {
            if let Some(mapped) = v.to_ipv4_mapped() {
                return is_public(IpAddr::V4(mapped));
            }
            let o = v.octets();
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_unique_local()
                && !v.is_unicast_link_local()
                && !(o[0] == 0x20 && o[1] == 0x01 && o[2] == 0x0d && o[3] == 0xb8)
        }
    }
}

fn image_attempt(
    job: &GenerationJob,
    news: Option<&News>,
    options: &ExternalOptions,
    templates: &PromptSnapshot,
    deadline: Instant,
    feedback: &str,
) -> Result<Vec<u8>, WorkError> {
    let work = attempt_dir(job, "image", options, job.image_attempts)?;
    let prompt = image_prompt(templates, job, news, &work, feedback)?;
    let message = work.join("result.txt");
    let stderr_path = work.join("stderr.txt");
    let _ = fs::remove_file(&message);
    let _ = fs::remove_file(&stderr_path);
    let before = generated_dirs(&options.generated_root);
    let local_before = local_png_hashes(&work)?;
    let started = SystemTime::now();
    let mut cmd = Command::new(&options.codex);
    cmd.current_dir(&work)
        .args(["exec", "--ephemeral", "--skip-git-repo-check", "--model"])
        .arg(&job.request.settings.image_model)
        .args(["--sandbox", "workspace-write", "--cd"])
        .arg(&work)
        .arg("--output-last-message")
        .arg(&message)
        .arg(&prompt);
    let mut proof = PromptEvidence::prepare(
        &options.evidence_root,
        job,
        "image",
        job.image_attempts,
        &job.request.settings.image_model,
        &prompt,
        &cmd,
    )
    .map_err(WorkError)?;
    let status = run_command(cmd, deadline, None, Some(&stderr_path), &options.cancel);
    let (outcome, code) = process_evidence(&status);
    proof.process_finished(outcome, code).map_err(WorkError)?;
    let stderr = bounded_optional_read(&stderr_path, 100_000)?;
    let _ = fs::remove_file(&stderr_path);
    status.map_err(WorkError::from)?;
    let msg = bounded_optional_read(&message, 10_000)?;
    let _ = fs::remove_file(&message);
    let mut paths_text = String::from_utf8_lossy(&msg).into_owned();
    paths_text.push('\n');
    paths_text.push_str(&String::from_utf8_lossy(&stderr));
    let after = generated_dirs(&options.generated_root);
    let new_dirs: BTreeSet<_> = after.difference(&before).cloned().collect();
    for candidate in png_paths(&paths_text).into_iter().rev() {
        if !candidate.is_absolute() || candidate.extension().is_none_or(|e| e != "png") {
            continue;
        }
        let Ok(path) = candidate.canonicalize() else {
            continue;
        };
        let local = path.starts_with(
            work.canonicalize()
                .map_err(|_| WorkError("work directory unavailable"))?,
        );
        let generated = path.parent().is_some_and(|p| new_dirs.contains(p));
        if !local && !generated {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if meta
                .modified()
                .is_ok_and(|m| m >= started - Duration::from_secs(2))
            {
                if let Ok(bytes) = bounded_read(&path, MAX_PNG) {
                    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                        && (!local
                            || local_before
                                .get(&path)
                                .is_none_or(|old| *old != sha256_hex(&bytes)))
                    {
                        proof.accepted("original_png", &bytes).map_err(WorkError)?;
                        return Ok(bytes);
                    }
                }
            }
        }
    }
    proof.rejected().map_err(WorkError)?;
    Err(WorkError("image attempt returned no attributable PNG"))
}

fn image_prompt(
    templates: &PromptSnapshot,
    job: &GenerationJob,
    news: Option<&News>,
    work: &Path,
    feedback: &str,
) -> Result<String, WorkError> {
    let brief = match &job.request.input {
        GenerationInput::CustomText(s) => format!(
            "Custom text: {}. Do not research news or add a source URL.",
            serde_json::to_string(s).unwrap_or_default()
        ),
        GenerationInput::News => format!(
            "Sourced text: {}",
            serde_json::to_string(&news).unwrap_or_default()
        ),
    };
    const HEADING: &str = "一言なんの画像か見出しをそえてください。";
    let mut style = job.request.settings.style.prompt.clone();
    if job.request.settings.include_heading && !style.contains(HEADING) {
        style.push_str(HEADING);
    }
    templates
        .image_prompt(&[
            ("style", style),
            ("brief", brief),
            ("work_dir", work.display().to_string()),
            ("feedback", feedback.into()),
        ])
        .map_err(|_| WorkError("image prompt template invalid"))
}

fn generated_dirs(root: &Path) -> BTreeSet<PathBuf> {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            p.is_dir().then(|| p.canonicalize().ok()).flatten()
        })
        .collect()
}

fn local_png_hashes(root: &Path) -> Result<BTreeMap<PathBuf, String>, WorkError> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).map_err(|_| WorkError("work directory unavailable"))? {
            let path = entry
                .map_err(|_| WorkError("work directory unavailable"))?
                .path();
            if path.is_dir() {
                if pending.len() + result.len() < 1000 {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "png") {
                if let Ok(bytes) = bounded_read(&path, MAX_PNG) {
                    if let Ok(canonical) = path.canonicalize() {
                        result.insert(canonical, sha256_hex(&bytes));
                    }
                }
            }
        }
    }
    Ok(result)
}

fn png_paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in text.lines() {
        for (start, _) in line.match_indices('/') {
            let preceding = line[..start].chars().next_back();
            if preceding.is_some_and(|c| !c.is_whitespace() && !"(<`\"'".contains(c)) {
                continue;
            }
            let Some(end) = line[start..].find(".png") else {
                continue;
            };
            let path_end = start + end + 4;
            if line[path_end..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace() && !")]>`\"',".contains(c))
            {
                continue;
            }
            let value = &line[start..path_end];
            if !value.contains(['<', '>', '`', '"', '\'', '(', ')']) {
                paths.push(PathBuf::from(value));
            }
        }
    }
    paths
}

fn normalize(
    options: &ExternalOptions,
    original: &[u8],
    deadline: Instant,
    job: &GenerationJob,
) -> Result<(Vec<u8>, Vec<u8>), WorkError> {
    let work = attempt_dir(job, "normalize", options, job.image_attempts)?;
    let input = work.join("original.png");
    let output = work.join("canonical.png");
    fs::write(&input, original).map_err(|_| WorkError("image staging failed"))?;
    let mut cmd = Command::new(&options.python);
    cmd.arg("-B")
        .arg("-c")
        .arg(include_str!("dpid.py"))
        .arg(&input)
        .arg(&output)
        .arg(job.request.settings.dpid_lambda.to_string())
        .arg(job.request.settings.threshold.to_string())
        .current_dir(&work);
    run_command(cmd, deadline, None, None, &options.cancel)
        .map_err(|_| WorkError("DPID normalization failed"))?;
    let eink = bounded_read(&output, MAX_PNG)?;
    validate_canonical_png(&eink)?;
    let lcd = super::variants::render_rgb565(original, WIDTH_LCD as u32, HEIGHT_LCD as u32)
        .map_err(|_| WorkError("LCD normalization failed"))?;
    Ok((eink, lcd))
}

/// Render a non-native E-ink size in the worker, using the same DPID code.
pub(super) fn render_eink_variant(
    source: &[u8],
    width: u32,
    height: u32,
    lambda: f64,
    threshold: u8,
    python: &Path,
    cache_root: &Path,
) -> Result<Vec<u8>, CaricatureError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let work = cache_root.join(format!("render-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&work).map_err(|_| CaricatureError::Io)?;
    let result = (|| {
        let input = work.join("input.png");
        let output = work.join("output.png");
        fs::write(&input, source).map_err(|_| CaricatureError::Io)?;
        let mut cmd = Command::new(python);
        cmd.arg("-B")
            .arg("-c")
            .arg(include_str!("dpid.py"))
            .arg(&input)
            .arg(&output)
            .arg(lambda.to_string())
            .arg(threshold.to_string())
            .arg(width.to_string())
            .arg(height.to_string())
            .current_dir(&work);
        run_command(
            cmd,
            Instant::now() + Duration::from_secs(30),
            None,
            None,
            &AtomicBool::new(false),
        )
        .map_err(|_| CaricatureError::InvalidSource)?;
        bounded_read(&output, MAX_PNG).map_err(|_| CaricatureError::InvalidSource)
    })();
    let _ = fs::remove_dir_all(work);
    result
}

fn validate_canonical_png(data: &[u8]) -> Result<(), WorkError> {
    if data.len() < 33
        || !data.starts_with(b"\x89PNG\r\n\x1a\n")
        || &data[12..16] != b"IHDR"
        || u32::from_be_bytes(data[16..20].try_into().unwrap()) != WIDTH_EINK
        || u32::from_be_bytes(data[20..24].try_into().unwrap()) != HEIGHT_EINK
        || data[24] != 1
        || data[25] != 0
        || data[26..29] != [0, 0, 0]
    {
        return Err(WorkError("canonical PNG profile invalid"));
    }
    let image = image::load_from_memory(data).map_err(|_| WorkError("canonical PNG invalid"))?;
    if image.dimensions() != (WIDTH_EINK, HEIGHT_EINK) {
        return Err(WorkError("canonical PNG invalid"));
    }
    Ok(())
}

fn bounded_read(path: &Path, max: usize) -> Result<Vec<u8>, WorkError> {
    let meta = path.metadata().map_err(|_| WorkError("artifact missing"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > max as u64 {
        return Err(WorkError("artifact size invalid"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| WorkError("artifact unreadable"))?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| WorkError("artifact unreadable"))?;
    if bytes.is_empty() || bytes.len() > max {
        return Err(WorkError("artifact size invalid"));
    }
    Ok(bytes)
}

fn bounded_optional_read(path: &Path, max: usize) -> Result<Vec<u8>, WorkError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    if path.metadata().is_ok_and(|m| m.len() == 0) {
        return Ok(Vec::new());
    }
    bounded_read(path, max)
}

fn run_command(
    mut cmd: Command,
    deadline: Instant,
    output: Option<&Path>,
    stderr: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<i32, CommandFailure> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CommandFailure {
            reason: "generation cancelled",
            code: None,
        });
    }
    cmd.stdin(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);
    if let Some(path) = stderr {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        cmd.stderr(Stdio::from(opts.open(path).map_err(|_| {
            CommandFailure {
                reason: "process error capture unavailable",
                code: None,
            }
        })?));
    } else {
        cmd.stderr(Stdio::null());
    }
    if let Some(path) = output {
        cmd.stdout(Stdio::from(fs::File::create(path).map_err(|_| {
            CommandFailure {
                reason: "process output unavailable",
                code: None,
            }
        })?));
    } else {
        cmd.stdout(Stdio::null());
    }
    let mut child = cmd.spawn().map_err(|_| CommandFailure {
        reason: "process launch failed",
        code: None,
    })?;
    loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_group(child);
            return Err(CommandFailure {
                reason: "generation cancelled",
                code: None,
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(status.code().unwrap_or(0))
                } else {
                    Err(CommandFailure {
                        reason: "process returned failure",
                        code: status.code(),
                    })
                }
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::park_timeout(Duration::from_millis(25))
            }
            Ok(None) => {
                terminate_group(child);
                return Err(CommandFailure {
                    reason: "process timed out",
                    code: None,
                });
            }
            Err(_) => {
                terminate_group(child);
                return Err(CommandFailure {
                    reason: "process interrupted",
                    code: None,
                });
            }
        }
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

fn terminate_group(mut child: std::process::Child) {
    #[cfg(unix)]
    {
        let group = child.id() as i32;
        if group > 0 {
            // Child starts a new process group. Terminate the whole CLI/DPID
            // tree, then force-kill descendants that ignored SIGTERM.
            unsafe {
                let _ = kill(-group, 15);
            }
            let grace = Instant::now() + Duration::from_millis(250);
            while Instant::now() < grace {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
            unsafe {
                let _ = kill(-group, 9);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let reap_until = Instant::now() + Duration::from_secs(1);
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        if Instant::now() >= reap_until {
            break;
        }
        std::thread::park_timeout(Duration::from_millis(20));
    }
    reap_child(child);
}

fn reap_child(mut child: std::process::Child) {
    let _ = std::thread::Builder::new()
        .name("caricature-child-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

fn write_private_atomic(path: &Path, data: &[u8]) -> Result<(), WorkError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("pending-{}-{nanos}", std::process::id()));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&temp)
        .map_err(|_| WorkError("job checkpoint unavailable"))?;
    if file.write_all(data).and_then(|_| file.sync_all()).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(WorkError("job checkpoint unavailable"));
    }
    if fs::rename(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err(WorkError("job checkpoint unavailable"));
    }
    fs::File::open(
        path.parent()
            .ok_or(WorkError("job checkpoint unavailable"))?,
    )
    .and_then(|dir| dir.sync_all())
    .map_err(|_| WorkError("job checkpoint unavailable"))?;
    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::super::{GenerationRequest, GenerationSettings};
    use super::*;

    fn test_templates() -> PromptSnapshot {
        PromptSnapshot {
            news: include_str!("../../assets/caricature-news-prompt.txt").into(),
            image: include_str!("../../assets/caricature-image-prompt.txt").into(),
            warning: None,
        }
    }

    #[test]
    fn heading_instruction_only_appears_in_enabled_job_snapshot() {
        let phrase = "一言なんの画像か見出しをそえてください。";
        let news = News {
            source_url: "https://example.com/news".into(),
            summary: "確認したニュース".into(),
        };
        for (job_id, slot, input) in [
            ("automatic", 0, GenerationInput::News),
            ("manual_news", -1, GenerationInput::News),
            (
                "manual_custom",
                -2,
                GenerationInput::CustomText("雲の上の猫".into()),
            ),
        ] {
            let mut settings = GenerationSettings::default();
            settings.style.prompt = "白黒の面を整理し、細密な網点や長文の台詞は使わない。".into();
            let request = GenerationRequest {
                job_id: job_id.into(),
                slot,
                requested_at_unix: 0,
                input,
                settings,
            };
            let old_job = GenerationJob::accept(request.clone()).unwrap();
            let sourced_news = matches!(&request.input, GenerationInput::News).then_some(&news);
            let old_prompt = image_prompt(
                &test_templates(),
                &old_job,
                sourced_news,
                Path::new("/tmp"),
                "",
            )
            .unwrap();
            assert!(!old_prompt.contains(phrase), "{job_id}");
            let mut next = request;
            next.settings.include_heading = true;
            let new_job = GenerationJob::accept(next).unwrap();
            let new_prompt = image_prompt(
                &test_templates(),
                &new_job,
                sourced_news,
                Path::new("/tmp"),
                "",
            )
            .unwrap();
            assert_eq!(new_prompt.matches(phrase).count(), 1, "{job_id}");
            let style = &old_job.request.settings.style.prompt;
            assert!(
                new_prompt.contains(&format!("{style}{phrase} Draw")),
                "{job_id}"
            );
            assert_eq!(
                new_prompt,
                old_prompt.replacen(
                    &format!("{style} Draw an original composition."),
                    &format!("{style}{phrase} Draw an original composition."),
                    1
                ),
                "{job_id}"
            );
            assert_eq!(
                image_prompt(
                    &test_templates(),
                    &old_job,
                    sourced_news,
                    Path::new("/tmp"),
                    ""
                )
                .unwrap(),
                old_prompt,
                "{job_id}"
            );
        }
    }
    #[test]
    fn heading_already_in_selected_style_is_not_duplicated() {
        let phrase = "一言なんの画像か見出しをそえてください。";
        let mut settings = GenerationSettings::default();
        settings.style.prompt = format!("昭和のキラキラ少女漫画調。{phrase}");
        settings.include_heading = true;
        let job = GenerationJob::accept(GenerationRequest {
            job_id: "existing_heading".into(),
            slot: -1,
            requested_at_unix: 0,
            input: GenerationInput::CustomText("雲の上の猫".into()),
            settings,
        })
        .unwrap();
        let prompt = image_prompt(&test_templates(), &job, None, Path::new("/tmp"), "").unwrap();
        assert_eq!(prompt.matches(phrase).count(), 1);
        assert!(prompt.contains(&format!("昭和のキラキラ少女漫画調。{phrase} Draw")));
    }
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn retry_limits_and_url_rules() {
        let policy = RetryPolicy {
            retry_count: 2,
            interval_seconds: 0,
            deadline_seconds: 1,
            attempt_timeout_seconds: 10,
        };
        assert!(next_timeout(&policy, Instant::now()).unwrap() <= Duration::from_secs(1));
        assert!(!is_public("127.0.0.1".parse().unwrap()));
        assert!(is_public("1.1.1.1".parse().unwrap()));
    }
    #[test]
    fn waits_and_worker_drop_do_not_block_cancellation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            trigger.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        assert!(wait_cancelable(Duration::from_secs(2), &cancel, None).is_err());
        assert!(started.elapsed() < Duration::from_millis(400));
        setter.join().unwrap();
        let worker = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(500)));
        let guard = WorkerGuard {
            cancel: Arc::new(AtomicBool::new(false)),
            thread: Some(worker),
        };
        let started = Instant::now();
        drop(guard);
        assert!(started.elapsed() < Duration::from_millis(250));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn timeout_terminates_process_group_descendants() {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("child.pid");
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(format!(
            "sleep 30 & echo $! > '{}'; wait",
            pid_file.display()
        ));
        let started = Instant::now();
        let result = run_command(
            cmd,
            started + Duration::from_millis(200),
            None,
            None,
            &AtomicBool::new(false),
        );
        assert_eq!(result.unwrap_err().reason, "process timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = fs::read_to_string(pid_file).unwrap();
        let proc_stat =
            fs::read_to_string(format!("/proc/{}/stat", pid.trim())).unwrap_or_default();
        assert!(proc_stat.is_empty() || proc_stat.split(' ').nth(2) == Some("Z"));
    }
    #[test]
    fn rejects_touched_existing_png_and_accepts_new_markdown_path_with_spaces() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = temp.path().join("fixture.png");
        let png = crate::caricature::test_canonical_png();
        fs::write(&fixture, &png).unwrap();
        let codex = temp.path().join("fake-codex");
        script(
            &codex,
            &format!(
                r##"#!/bin/sh
previous=''; out=''
for arg in "$@"; do
  if [ "$previous" = --output-last-message ]; then out="$arg"; fi
  previous="$arg"
done
case "$PWD" in
  */image/1)
    touch "$PWD/stale.png"
    printf '[old](%s/stale.png)\n' "$PWD" > "$out"
    ;;
  */image/2)
    cp '{}' "$PWD/fresh art.png"
    printf '[new](%s/fresh art.png)\n' "$PWD" > "$out"
    ;;
esac
"##,
                fixture.display()
            ),
        );
        let options = ExternalOptions {
            codex,
            python: "python3".into(),
            generated_root: temp.path().join("generated"),
            work_root: temp.path().join("work"),
            evidence_root: temp.path().join("evidence"),
            template_root: temp.path().join("templates"),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let request = GenerationRequest {
            job_id: "attribution".into(),
            slot: -1,
            requested_at_unix: 0,
            input: GenerationInput::CustomText("drawing".into()),
            settings: GenerationSettings::default(),
        };
        let mut job = GenerationJob::accept(request).unwrap();
        job.image_attempts = 1;
        let work = attempt_dir(&job, "image", &options, 1).unwrap();
        fs::write(work.join("stale.png"), &png).unwrap();
        assert_eq!(
            image_attempt(
                &job,
                None,
                &options,
                &test_templates(),
                Instant::now() + Duration::from_secs(2),
                ""
            )
            .unwrap_err()
            .0,
            "image attempt returned no attributable PNG"
        );
        job.image_attempts = 2;
        assert_eq!(
            image_attempt(
                &job,
                None,
                &options,
                &test_templates(),
                Instant::now() + Duration::from_secs(2),
                ""
            )
            .unwrap(),
            png
        );
        let proof: serde_json::Value = serde_json::from_slice(
            &fs::read(options.evidence_root.join("attribution/image-2.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(proof["status"], "artifact_returned");
        assert_eq!(proof["artifact_sha256"], sha256_hex(&png));
    }
    fn script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[tokio::test]
    async fn fake_cli_retries_image_and_reuses_selected_news() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = temp.path().join("fixture.png");
        fs::write(&fixture, crate::caricature::test_canonical_png()).unwrap();
        let later_template = temp.path().join("later-image.txt");
        fs::write(
            &later_template,
            test_templates()
                .image
                .replace("one original illustration", "one later illustration"),
        )
        .unwrap();
        let codex = temp.path().join("fake-codex");
        script(
            &codex,
            &format!(
                r##"#!/bin/sh
if [ "$1" = features ]; then echo 'image_generation stable true'; exit 0; fi
if [ "$1" = login ]; then exit 0; fi
previous=''
out=''
news=''
for arg in "$@"; do
  if [ "$previous" = --output-last-message ]; then out="$arg"; fi
  if [ "$arg" = --output-schema ]; then news=1; fi
  previous="$arg"
done
if [ "$news" = 1 ]; then
  printf '%s' '{{"source_url":"https://1.1.1.1/article","summary":"Verified summary"}}' > "$out"
  exit 0
fi
case "$PWD" in */image/1) cp '{}' '{}'; exit 9 ;; esac
cp '{}' "$PWD/generated.png"
printf '%s\n' "$PWD/generated.png" > "$out"
"##,
                later_template.display(),
                temp.path().join("templates/image.txt").display(),
                fixture.display()
            ),
        );
        let python = temp.path().join("fake-python");
        script(
            &python,
            &format!(
                "#!/bin/sh\nif [ $# -eq 3 ]; then exit 0; fi\ncp '{}' \"$5\"\n",
                fixture.display()
            ),
        );
        let options = ExternalOptions {
            codex,
            python,
            generated_root: temp.path().join("generated"),
            work_root: temp.path().join("work"),
            evidence_root: temp.path().join("evidence"),
            template_root: temp.path().join("templates"),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        fs::create_dir_all(&options.template_root).unwrap();
        fs::write(
            options.template_root.join("news.txt"),
            test_templates()
                .news
                .replace("Webを調べ", "編集済みニュース指示"),
        )
        .unwrap();
        fs::write(
            options.template_root.join("image.txt"),
            test_templates()
                .image
                .replace("one original illustration", "one edited illustration"),
        )
        .unwrap();
        let mut request = GenerationRequest {
            job_id: "fake_job".into(),
            slot: 0,
            requested_at_unix: 0,
            input: GenerationInput::News,
            settings: GenerationSettings::default(),
        };
        request.settings.include_heading = true;
        let job = GenerationJob::accept(request).unwrap();
        let result = generate(job, BTreeSet::new(), options.clone()).await;
        let GenerationResult::Validated { job, artwork, .. } = result else {
            panic!("fake generation failed");
        };
        assert_eq!((job.news_attempts, job.image_attempts), (1, 2));
        assert_eq!(job.news.unwrap().summary, "Verified summary");
        assert_eq!(artwork.lcd_rgb565be.len(), 160 * 80 * 2);
        assert_eq!(
            artwork.metadata.source_urls,
            vec!["https://1.1.1.1/article"]
        );
        assert!(options.evidence_root.join("fake_job/news-1.json").exists());
        assert!(options.evidence_root.join("fake_job/image-2.json").exists());
        let news_evidence: serde_json::Value = serde_json::from_slice(
            &fs::read(options.evidence_root.join("fake_job/news-1.json")).unwrap(),
        )
        .unwrap();
        assert!(news_evidence["submitted_prompt"]
            .as_str()
            .unwrap()
            .contains("編集済みニュース指示"));
        let evidence: serde_json::Value = serde_json::from_slice(
            &fs::read(options.evidence_root.join("fake_job/image-2.json")).unwrap(),
        )
        .unwrap();
        assert!(evidence["submitted_prompt"]
            .as_str()
            .unwrap()
            .contains("一言なんの画像か見出しをそえてください。"));
        assert!(evidence["submitted_prompt"]
            .as_str()
            .unwrap()
            .contains("one edited illustration"));
        assert!(!evidence["submitted_prompt"]
            .as_str()
            .unwrap()
            .contains("one later illustration"));
        let resume_request = GenerationRequest {
            job_id: "resume_job".into(),
            slot: 1,
            requested_at_unix: 3600,
            input: GenerationInput::News,
            settings: GenerationSettings::default(),
        };
        let mut checkpoint = GenerationJob::accept(resume_request.clone()).unwrap();
        checkpoint.news = Some(News {
            source_url: "https://1.1.1.1/article".into(),
            summary: "Saved news".into(),
        });
        checkpoint.news_attempts = 1;
        checkpoint.image_attempts = 1;
        checkpoint.state = GenerationState::Drawing;
        checkpoint_job(&checkpoint, &options).unwrap();
        let resumed = generate(
            GenerationJob::accept(resume_request).unwrap(),
            BTreeSet::new(),
            options.clone(),
        )
        .await;
        let GenerationResult::Validated { job, .. } = resumed else {
            panic!("resume failed");
        };
        assert_eq!((job.news_attempts, job.image_attempts), (1, 2));
        assert_eq!(job.news.unwrap().summary, "Saved news");
        assert!(!options
            .evidence_root
            .join("resume_job/news-1.json")
            .exists());
        let custom = GenerationRequest {
            job_id: "custom_job".into(),
            slot: -1,
            requested_at_unix: 0,
            input: GenerationInput::CustomText("月面の喫茶店".into()),
            settings: GenerationSettings::default(),
        };
        let result = generate(
            GenerationJob::accept(custom).unwrap(),
            BTreeSet::new(),
            options.clone(),
        )
        .await;
        let GenerationResult::Validated { job, artwork, .. } = result else {
            panic!("fake custom generation failed");
        };
        assert_eq!((job.news_attempts, job.image_attempts), (0, 2));
        assert!(artwork.metadata.source_urls.is_empty());
        assert!(!options
            .evidence_root
            .join("custom_job/news-1.json")
            .exists());
    }
}
