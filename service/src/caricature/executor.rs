//! Adapter to the common, generation-tokened plugin job worker.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin_jobs::{JobExecutionContext, JobFailure, JobOutcome, PluginJobExecutor};

use super::external::{self, ExternalOptions, GenerationResult};
use super::store::{ArtworkStore, Publication};
use super::{recent_source_keys, Artwork, GenerationJob, PublishedSource, ID};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaricatureWork {
    Generate {
        job: Box<GenerationJob>,
    },
    ShowHistory {
        artwork_id: String,
        device_id: String,
        incarnation: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaricatureSuccess {
    Published {
        job: Box<GenerationJob>,
        publication: Box<Publication>,
    },
    HistoryFrame {
        artwork: Box<Artwork>,
        device_id: String,
        incarnation: u64,
    },
}

#[derive(Clone, Debug)]
pub struct CaricatureExecutor {
    pub codex: PathBuf,
    pub python: PathBuf,
    pub generated_root: PathBuf,
}

impl Default for CaricatureExecutor {
    fn default() -> Self {
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
        Self {
            codex: "codex".into(),
            python: "python3".into(),
            generated_root: codex_home
                .unwrap_or_else(|| PathBuf::from(".codex"))
                .join("generated_images"),
        }
    }
}

impl PluginJobExecutor for CaricatureExecutor {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome> {
        let executor = self.clone();
        Box::pin(async move {
            if context.token.plugin != ID {
                return failed("invalid_job", "Invalid caricature job");
            }
            let work: CaricatureWork = match serde_json::from_value(payload) {
                Ok(v) => v,
                Err(_) => return failed("invalid_job", "Invalid caricature job"),
            };
            if context.is_cancelled() {
                return JobOutcome::Cancelled;
            }
            let store = ArtworkStore::new(context.paths.data.join("artwork"));
            match work {
                CaricatureWork::Generate { job } => {
                    let job = *job;
                    if job.request.validate().is_err() {
                        return failed("invalid_job", "Invalid caricature request");
                    }
                    // The latest pointer is the publication commit point. A worker can
                    // finish it just before the common state transaction/restart.
                    match store.latest() {
                        Ok(Some(publication))
                            if publication.artwork.metadata.job_id == job.request.job_id =>
                        {
                            if publication.artwork.metadata.slot != job.request.slot
                                || publication.artwork.metadata.settings != job.request.settings
                                || publication.artwork.metadata.input != job.request.input
                            {
                                return failed(
                                    "archive_mismatch",
                                    "Published artwork does not match job",
                                );
                            }
                            let mut recovered = job;
                            recovered.state = super::GenerationState::Validated;
                            return success(CaricatureSuccess::Published {
                                job: Box::new(recovered),
                                publication: Box::new(publication),
                            });
                        }
                        Ok(_) => {}
                        Err(_) => {
                            return failed("archive_unavailable", "Artwork archive unavailable")
                        }
                    }
                    let history = match store.history() {
                        Ok(v) => v,
                        Err(_) => {
                            return failed("archive_unavailable", "Artwork archive unavailable")
                        }
                    };
                    let sources: Vec<_> = history
                        .iter()
                        .flat_map(|entry| {
                            entry.artwork.metadata.source_urls.iter().map(move |url| {
                                PublishedSource {
                                    source_url: url.clone(),
                                    published_at_unix: entry.published_at_unix,
                                }
                            })
                        })
                        .collect();
                    let recent: BTreeSet<String> = recent_source_keys(&sources, unix_now());
                    let mut options = ExternalOptions::new(
                        context.paths.data.join("work"),
                        context.paths.data.join("prompt_evidence"),
                        executor.generated_root,
                    );
                    options.codex = executor.codex;
                    options.python = executor.python;
                    let cancel_flag = options.cancel.clone();
                    let mut cancel = context.cancel.clone();
                    context.progress.report(Some(5), "Generating artwork");
                    let generation = external::generate(job, recent, options);
                    tokio::pin!(generation);
                    let result = tokio::select! {
                        result = &mut generation => result,
                        _ = wait_cancel(&mut cancel) => {
                            cancel_flag.store(true, Ordering::SeqCst);
                            let _ = generation.await;
                            return JobOutcome::Cancelled;
                        }
                    };
                    let (job, artwork) = match result {
                        GenerationResult::Validated { job, artwork } => (*job, *artwork),
                        GenerationResult::Failed { stage, reason, .. } => {
                            let code = if matches!(stage, super::GenerationStage::News) {
                                "news_failed"
                            } else {
                                "image_failed"
                            };
                            return failed(code, &reason);
                        }
                    };
                    if *cancel.borrow() {
                        return JobOutcome::Cancelled;
                    }
                    let artwork_id = artwork.id.clone();
                    let publication = match store.publish(artwork, unix_now()) {
                        Ok(v) => v,
                        Err(_) => match store.latest() {
                            Ok(Some(latest))
                                if latest.artwork.id == artwork_id
                                    && latest.artwork.metadata.job_id == job.request.job_id =>
                            {
                                latest
                            }
                            _ => return failed("publish_failed", "Artwork publication failed"),
                        },
                    };
                    context.progress.report(Some(100), "Artwork published");
                    success(CaricatureSuccess::Published {
                        job: Box::new(job),
                        publication: Box::new(publication),
                    })
                }
                CaricatureWork::ShowHistory {
                    artwork_id,
                    device_id,
                    incarnation,
                } => {
                    let history = match store.history() {
                        Ok(v) => v,
                        Err(_) => {
                            return failed("archive_unavailable", "Artwork archive unavailable")
                        }
                    };
                    let Some(entry) = history.into_iter().find(|p| p.artwork.id == artwork_id)
                    else {
                        return failed("unknown_artwork", "Artwork not found");
                    };
                    success(CaricatureSuccess::HistoryFrame {
                        artwork: Box::new(entry.artwork),
                        device_id,
                        incarnation,
                    })
                }
            }
        })
    }
}

async fn wait_cancel(receiver: &mut tokio::sync::watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            break;
        }
    }
}

fn failed(code: &str, message: &str) -> JobOutcome {
    JobOutcome::Failed(JobFailure::new(code, message))
}

fn success(value: CaricatureSuccess) -> JobOutcome {
    match serde_json::to_value(value) {
        Ok(value) => JobOutcome::Succeeded(value),
        Err(_) => failed("result_invalid", "Artwork result invalid"),
    }
}

fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caricature::{
        ArtworkMetadata, GenerationInput, GenerationRequest, GenerationSettings, GenerationState,
        HEIGHT_LCD, WIDTH_LCD,
    };
    use crate::plugin_jobs::{JobProgressSink, JobToken, PluginPaths};

    #[tokio::test]
    async fn committed_job_recovers_before_own_source_dedup_or_cli() {
        let temp = tempfile::tempdir().unwrap();
        let settings = GenerationSettings::default();
        let request = GenerationRequest {
            job_id: "committed".into(),
            slot: 0,
            requested_at_unix: 0,
            input: GenerationInput::News,
            settings: settings.clone(),
        };
        let job = GenerationJob::accept(request.clone()).unwrap();
        let artwork = Artwork {
            id: "committed-1".into(),
            metadata: ArtworkMetadata {
                job_id: request.job_id.clone(),
                slot: request.slot,
                created_at_unix: 0,
                input: request.input.clone(),
                source_urls: vec!["https://1.1.1.1/news".into()],
                news_summary: Some("news".into()),
                settings,
            },
            eink_png: crate::caricature::test_canonical_png(),
            lcd_rgb565be: vec![0; WIDTH_LCD * HEIGHT_LCD * 2],
        };
        let store = ArtworkStore::new(temp.path().join("data/artwork"));
        store.publish(artwork.clone(), 1).unwrap();
        let (_sender, receiver) = tokio::sync::watch::channel(false);
        let context = JobExecutionContext {
            token: JobToken {
                plugin: ID.into(),
                slot: "hourly".into(),
                generation: 1,
            },
            paths: PluginPaths {
                data: temp.path().join("data"),
                cache: temp.path().join("cache"),
            },
            cancel: receiver,
            progress: JobProgressSink::default(),
        };
        let executor = CaricatureExecutor {
            codex: "/absent/codex".into(),
            python: "/absent/python".into(),
            generated_root: temp.path().join("none"),
        };
        let result = executor
            .execute(
                serde_json::to_value(CaricatureWork::Generate { job: Box::new(job) }).unwrap(),
                context,
            )
            .await;
        let JobOutcome::Succeeded(value) = result else {
            panic!("committed job did not recover");
        };
        let CaricatureSuccess::Published { job, publication } =
            serde_json::from_value(value).unwrap()
        else {
            panic!("wrong success");
        };
        assert_eq!(job.state, GenerationState::Validated);
        assert_eq!(publication.artwork.id, artwork.id);
        assert_eq!(store.history().unwrap().len(), 1);
    }
}
