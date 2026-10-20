//! Adapter to the common, generation-tokened plugin job worker.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin_jobs::{JobExecutionContext, JobFailure, JobOutcome, PluginJobExecutor};

use super::external::{self, ExternalOptions, GenerationResult};
use super::hash::sha256_hex;
use super::originals::half_webp_quality90;
use super::retention::{cleanup_published_work, deletion_enabled};
use super::store::{ArtworkStore, Publication};
use super::variants::{VariantCache, VariantSpec};
use super::{recent_source_keys, Artwork, GenerationJob, OriginalCodec, PublishedSource, ID};

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
    CleanupWork {
        now_unix: i64,
        active_job_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaricatureSuccess {
    Published {
        job: Box<GenerationJob>,
        publication: Box<Publication>,
        #[serde(default)]
        template_warning: Option<String>,
    },
    HistoryFrame {
        artwork: Box<Artwork>,
        device_id: String,
        incarnation: u64,
    },
    WorkCleaned {
        removed: usize,
    },
}

#[derive(Clone, Debug)]
pub struct CaricatureExecutor {
    pub codex: PathBuf,
    pub python: PathBuf,
    pub generated_root: PathBuf,
    pub retention: std::sync::Arc<super::retention::RetentionMaintenance>,
}

impl Default for CaricatureExecutor {
    fn default() -> Self {
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
        Self {
            retention: Default::default(),
            codex: "codex".into(),
            python: "python3".into(),
            generated_root: codex_home
                .unwrap_or_else(|| PathBuf::from(".codex"))
                .join("generated_images"),
        }
    }
}

impl PluginJobExecutor for CaricatureExecutor {
    fn maintenance(&self) -> Option<&dyn crate::maintenance::PluginMaintenanceExecutor> {
        self.retention.enabled().then_some(self.retention.as_ref())
    }
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
            let cache = VariantCache::new(context.paths.cache.join("artwork_variants"));
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
                                template_warning: None,
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
                    options.python = executor.python.clone();
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
                    let (job, mut artwork, template_warning) = match result {
                        GenerationResult::Validated {
                            job,
                            artwork,
                            warning,
                        } => (*job, *artwork, warning),
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
                    let original_path = context
                        .paths
                        .data
                        .join("work")
                        .join(&job.request.job_id)
                        .join("normalize")
                        .join(job.image_attempts.to_string())
                        .join("original.png");
                    let original = match fs::read(original_path) {
                        Ok(bytes) => bytes,
                        Err(_) => return failed("publish_failed", "Original artwork unavailable"),
                    };
                    let archived_original = match half_webp_quality90(&original) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            return failed("publish_failed", "Original WebP conversion failed")
                        }
                    };
                    artwork.original_sha256 = Some(sha256_hex(&archived_original));
                    artwork.original_codec = Some(OriginalCodec::HalfWebpQ90);
                    let publication = match store.publish_with_original(
                        artwork,
                        &archived_original,
                        unix_now(),
                    ) {
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
                    // Native LCD/E-ink variants are already embedded in the archive;
                    // only other profiles receive a separate cache file.
                    context.progress.report(Some(100), "Artwork published");
                    success(CaricatureSuccess::Published {
                        job: Box::new(job),
                        publication: Box::new(publication),
                        template_warning,
                    })
                }
                CaricatureWork::ShowHistory {
                    artwork_id,
                    device_id,
                    incarnation,
                } => {
                    let artwork = match store.artwork(&artwork_id) {
                        Ok(v) => v,
                        Err(_) => {
                            return failed("archive_unavailable", "Artwork archive unavailable")
                        }
                    };
                    let Some(mut artwork) = artwork else {
                        return failed("unknown_artwork", "Artwork not found");
                    };
                    if let Ok(variant) = cache.get_or_render(
                        &store,
                        &artwork,
                        VariantSpec::lcd(&artwork),
                        &executor.python,
                    ) {
                        artwork.lcd_rgb565be = variant.bytes;
                    }
                    success(CaricatureSuccess::HistoryFrame {
                        artwork: Box::new(artwork),
                        device_id,
                        incarnation,
                    })
                }
                CaricatureWork::CleanupWork {
                    now_unix,
                    active_job_ids,
                } => {
                    if !deletion_enabled() {
                        return success(CaricatureSuccess::WorkCleaned { removed: 0 });
                    }
                    let work_root = context.paths.data.join("work");
                    match tokio::task::spawn_blocking(move || {
                        cleanup_published_work(
                            &store,
                            &work_root,
                            now_unix,
                            &active_job_ids.into_iter().collect(),
                        )
                    })
                    .await
                    {
                        Ok(Ok(removed)) => success(CaricatureSuccess::WorkCleaned { removed }),
                        _ => failed("cleanup_failed", "Artwork work cleanup failed"),
                    }
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
            original_sha256: None,
            original_codec: None,
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
            retention: Default::default(),
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
        let CaricatureSuccess::Published {
            job, publication, ..
        } = serde_json::from_value(value).unwrap()
        else {
            panic!("wrong success");
        };
        assert_eq!(job.state, GenerationState::Validated);
        assert_eq!(publication.artwork.id, artwork.id);
        assert_eq!(store.history().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn history_sub_frame_uses_shared_variant_cache() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let cache_root = temp.path().join("cache");
        let store = ArtworkStore::new(data.join("artwork"));
        let artwork = Artwork {
            id: "history-1".into(),
            metadata: ArtworkMetadata {
                job_id: "history".into(),
                slot: 0,
                created_at_unix: 0,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: None,
            original_codec: None,
            eink_png: crate::caricature::test_canonical_png(),
            lcd_rgb565be: vec![0x55; WIDTH_LCD * HEIGHT_LCD * 2],
        };
        store.publish(artwork.clone(), 0).unwrap();
        let executor = CaricatureExecutor {
            codex: "/absent/codex".into(),
            python: "/absent/python".into(),
            generated_root: temp.path().join("none"),
            retention: Default::default(),
        };
        for _ in 0..2 {
            let (_sender, receiver) = tokio::sync::watch::channel(false);
            let context = JobExecutionContext {
                token: JobToken {
                    plugin: ID.into(),
                    slot: "history".into(),
                    generation: 1,
                },
                paths: PluginPaths {
                    data: data.clone(),
                    cache: cache_root.clone(),
                },
                cancel: receiver,
                progress: JobProgressSink::default(),
            };
            let result = executor
                .execute(
                    serde_json::to_value(CaricatureWork::ShowHistory {
                        artwork_id: artwork.id.clone(),
                        device_id: "device-a".into(),
                        incarnation: 3,
                    })
                    .unwrap(),
                    context,
                )
                .await;
            let JobOutcome::Succeeded(value) = result else {
                panic!("history failed")
            };
            let CaricatureSuccess::HistoryFrame {
                artwork: shown,
                device_id,
                incarnation,
            } = serde_json::from_value(value).unwrap()
            else {
                panic!("wrong result")
            };
            assert_eq!(shown.lcd_rgb565be, artwork.lcd_rgb565be);
            assert_eq!((device_id.as_str(), incarnation), ("device-a", 3));
        }
        assert!(!cache_root.join("artwork_variants").exists());
    }
}
