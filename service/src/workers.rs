//! Independent long-work worker. JMA scheduling never waits for this queue.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use crate::application::AppError;
use crate::plugin_jobs::{
    JobExecutionContext, JobFailure, JobOutcome, JobProgressSink, JobToken, PluginJobExecutor,
};
use crate::web::SharedApp;

pub type Executors = BTreeMap<String, Arc<dyn PluginJobExecutor>>;

struct Running {
    token: JobToken,
    cancel: watch::Sender<bool>,
}

pub async fn run_work(
    app: SharedApp,
    executors: Executors,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), AppError> {
    let mut notifications = app
        .lock()
        .map_err(|_| AppError::Conflict("application unavailable"))?
        .work_notifications();
    let mut tasks = JoinSet::new();
    let mut running = BTreeMap::<u64, Running>::new();
    let (progress_tx, mut progress_rx) = mpsc::channel(64);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut stopping = false;
    let mut inspect = true;
    loop {
        if inspect && !stopping {
            inspect = false;
            let jobs = {
                let mut guard = app
                    .lock()
                    .map_err(|_| AppError::Conflict("application unavailable"))?;
                for task in running.values() {
                    if guard.plugin_work_cancelled(&task.token) {
                        task.cancel.send_replace(true);
                    }
                }
                guard.claim_plugin_work()?
            };
            for job in jobs {
                let token = job.token.clone();
                let paths = app
                    .lock()
                    .map_err(|_| AppError::Conflict("application unavailable"))?
                    .plugin_paths(&token.plugin)
                    .ok_or(AppError::NotFound)?;
                let executor = executors.get(&token.plugin).cloned();
                let (cancel_tx, cancel_rx) = watch::channel(false);
                running.insert(
                    token.generation,
                    Running {
                        token: token.clone(),
                        cancel: cancel_tx.clone(),
                    },
                );
                let context = JobExecutionContext {
                    token: token.clone(),
                    paths,
                    cancel: cancel_rx,
                    progress: JobProgressSink::new(token.clone(), progress_tx.clone()),
                };
                tasks.spawn(async move {
                    let Some(executor) = executor else {
                        return (token, JobOutcome::Failed(JobFailure::unsupported()));
                    };
                    let future = executor.execute(job.payload, context);
                    tokio::pin!(future);
                    let outcome = tokio::select! {
                        outcome = &mut future => outcome,
                        _ = tokio::time::sleep(Duration::from_secs(job.timeout_seconds)) => {
                            // Executors must stop/wait child processes on cancellation.
                            cancel_tx.send_replace(true);
                            let _ = future.await;
                            JobOutcome::Failed(JobFailure::new("timeout", "Plugin job timed out"))
                        }
                    };
                    (token, outcome)
                });
            }
        }
        if stopping && tasks.is_empty() {
            return Ok(());
        }
        tokio::select! {
            changed = shutdown.changed(), if !stopping => {
                if changed.is_err() || *shutdown.borrow() {
                    stopping = true;
                    // Leave durable jobs unfinished for restart recovery. Normal
                    // host shutdown is distinct from a user's Cancel command.
                    for task in running.values() { task.cancel.send_replace(true); }
                }
            }
            _ = tick.tick(), if !stopping => {
                app.lock().map_err(|_| AppError::Conflict("application unavailable"))?.plan_scheduled_work()?;
                inspect = true;
            }
            changed = notifications.changed(), if !stopping => {
                if changed.is_err() { return Err(AppError::Conflict("application unavailable")); }
                inspect = true;
            }
            Some((token, progress)) = progress_rx.recv() => {
                if !stopping {
                    app.lock().map_err(|_| AppError::Conflict("application unavailable"))?.plugin_work_progress(token, progress)?;
                }
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                let (token, outcome) = result.map_err(|_| AppError::Conflict("plugin worker interrupted"))?;
                running.remove(&token.generation);
                if !stopping {
                    // Preserve prepared metadata/source/conversion before a
                    // cancellation outcome can make further progress stale.
                    while let Ok((progress_token,progress))=progress_rx.try_recv() {
                        app.lock().map_err(|_| AppError::Conflict("application unavailable"))?.plugin_work_progress(progress_token,progress)?;
                    }
                    app.lock().map_err(|_| AppError::Conflict("application unavailable"))?.complete_plugin_work(token, outcome)?;
                    inspect = true;
                }
            }
        }
    }
}
