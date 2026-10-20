//! Generic prepare/plan/stage/save/publish/finish boundary. Plugin policy is opaque.
use crate::{
    application::AppError,
    plugin_jobs::{JobFailure, PluginPaths},
    web::SharedApp,
};
use serde_json::Value;
use std::{any::Any, collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub struct MaintenanceInput {
    pub paths: PluginPaths,
    pub now_unix: i64,
    pub revision: u64,
    pub snapshot: Arc<Value>,
}
pub struct PreparedMaintenance {
    pub proposal: Value,
    pub permit: Box<dyn Any + Send>,
}
pub struct MaintenancePlan {
    pub changed: bool,
    pub outcome: Value,
}
pub struct MaintenanceCommit {
    pub revision: u64,
    pub snapshot: Arc<Value>,
    pub references: BTreeSet<String>,
    pub outcome: Value,
}
pub trait PluginMaintenanceExecutor: Send + Sync {
    fn prepare(&self, input: MaintenanceInput) -> Result<Option<PreparedMaintenance>, JobFailure>;
    fn stage(
        &self,
        _prepared: &mut PreparedMaintenance,
        _outcome: &Value,
    ) -> Result<(), JobFailure> {
        Ok(())
    }
    /// Called only after the corresponding committed state is publicly visible.
    fn finish(
        &self,
        prepared: PreparedMaintenance,
        committed: &MaintenanceCommit,
    ) -> Result<(), JobFailure>;
}
fn failure(phase: &str, error: JobFailure) -> AppError {
    AppError::Internal(Box::new(std::io::Error::other(format!(
        "plugin maintenance {phase}: {} ({})",
        error.message, error.code
    ))))
}
/// Runs on a blocking worker. No plugin receives a mutable App or publication lock.
pub fn run_once(
    app: &SharedApp,
    plugin: &str,
    executor: &dyn PluginMaintenanceExecutor,
) -> Result<bool, AppError> {
    let input = app
        .read()
        .map_err(|_| AppError::Conflict("application unavailable"))?
        .maintenance_input(plugin)?;
    let revision = input.revision;
    let Some(prepared) = executor.prepare(input).map_err(|e| failure("prepare", e))? else {
        return Ok(false);
    };
    let mut prepared = Some(prepared);
    // Never wait for an application owner while holding a plugin's external guard.
    let mut owner = match app.try_lock() {
        Ok(owner) => owner,
        Err(crate::shared::AccessError::Busy) => return Ok(false),
        Err(_) => return Err(AppError::Conflict("application unavailable")),
    };
    owner.reserve_maintenance(plugin)?;
    let result = (|| {
        let plan = owner.plan_maintenance(
            plugin,
            revision,
            &prepared.as_ref().expect("prepared").proposal,
        )?;
        executor
            .stage(prepared.as_mut().expect("prepared"), &plan.outcome)
            .map_err(|e| failure("stage", e))?;
        let committed = owner.commit_maintenance(plan)?;
        owner.publish_now()?;
        executor
            .finish(prepared.take().expect("prepared"), &committed)
            .map_err(|e| failure("finish", e))?;
        Ok(true)
    })();
    owner.release_maintenance();
    drop(owner);
    result
}
