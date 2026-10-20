// Common revision, snapshot and publication support; no plugin-specific payload keys.
pub(crate) struct PlannedMaintenance {
    plugin: String,
    revision: u64,
    content: Box<dyn ContentPlugin>,
    snapshot: Arc<serde_json::Value>,
    changed: bool,
    pub(crate) outcome: serde_json::Value,
}
impl App {
    pub(crate) fn maintenance_input(
        &self,
        plugin: &str,
    ) -> Result<crate::maintenance::MaintenanceInput, AppError> {
        Ok(crate::maintenance::MaintenanceInput {
            paths: self.plugin_paths(plugin).ok_or(AppError::NotFound)?,
            now_unix: self.clock.unix_seconds(),
            revision: self
                .runtime
                .managed_revisions
                .get(plugin)
                .copied()
                .unwrap_or(1),
            snapshot: self
                .runtime
                .plugin_states
                .get(plugin)
                .cloned()
                .unwrap_or_else(|| Arc::new(serde_json::Value::Null)),
        })
    }
    pub(crate) fn plan_maintenance(
        &self,
        plugin: &str,
        revision: u64,
        proposal: &serde_json::Value,
    ) -> Result<PlannedMaintenance, AppError> {
        if self
            .runtime
            .managed_revisions
            .get(plugin)
            .copied()
            .unwrap_or(1)
            != revision
        {
            return Err(AppError::Conflict("stale plugin maintenance"));
        }
        let mut content = self.contents.get(plugin).ok_or(AppError::NotFound)?.clone();
        let jobs: Vec<_> = self
            .runtime
            .jobs
            .jobs
            .values()
            .filter(|r| r.job.token.plugin == plugin && !r.status.finished())
            .map(|r| &r.job)
            .collect();
        let plan = content
            .plan_maintenance(&self.command_context(), proposal, &jobs)
            .map_err(|_| AppError::Conflict("plugin maintenance plan rejected"))?;
        let snapshot = if plan.changed {
            Arc::new(content.save_state())
        } else {
            self.runtime
                .plugin_states
                .get(plugin)
                .cloned()
                .unwrap_or_else(|| Arc::new(serde_json::Value::Null))
        };
        Ok(PlannedMaintenance {
            plugin: plugin.into(),
            revision,
            content,
            snapshot,
            changed: plan.changed,
            outcome: plan.outcome,
        })
    }
    pub(crate) fn commit_maintenance(
        &mut self,
        plan: PlannedMaintenance,
    ) -> Result<crate::maintenance::MaintenanceCommit, AppError> {
        let PlannedMaintenance {
            plugin,
            revision,
            content,
            snapshot,
            changed,
            outcome,
        } = plan;
        if self
            .runtime
            .managed_revisions
            .get(&plugin)
            .copied()
            .unwrap_or(1)
            != revision
        {
            return Err(AppError::Conflict("stale plugin maintenance"));
        }
        let next_revision = if changed {
            revision
                .checked_add(1)
                .ok_or(AppError::Conflict("plugin revision exhausted"))?
        } else {
            revision
        };
        if changed {
            self.change(|_, state| {
                state.plugin_states.insert(plugin.clone(), snapshot.clone());
                state
                    .managed_revisions
                    .insert(plugin.clone(), next_revision);
                Ok(())
            })?;
            self.contents.insert(plugin.clone(), content);
        }
        Ok(crate::maintenance::MaintenanceCommit {
            revision: next_revision,
            snapshot,
            references: self.plugin_retention_references(&plugin),
            outcome,
        })
    }
    pub(crate) fn begin_maintenance_scope(&mut self, plugin: &str) -> Result<(), AppError> {
        if !self.contents.contains_key(plugin) {
            return Err(AppError::NotFound);
        }
        if self.maintenance_plugin.is_some() {
            return Err(AppError::Conflict("maintenance already active"));
        }
        self.maintenance_plugin = Some(plugin.into());
        Ok(())
    }
    pub(crate) fn end_maintenance_scope(&mut self) {
        self.maintenance_plugin = None;
        self.dispatch.send_replace(());
    }
    pub(crate) fn maintenance_blocks(&self, plugin: Option<&str>) -> bool {
        self.maintenance_plugin
            .as_deref()
            .is_some_and(|reserved| Some(reserved) == plugin)
    }
    /// Used by an external plugin adapter under its own guard; immutable only.
    pub(crate) fn maintenance_references(
        &self,
        plugin: &str,
    ) -> Result<(Arc<serde_json::Value>, BTreeSet<String>), AppError> {
        let input = self.maintenance_input(plugin)?;
        Ok((input.snapshot, self.plugin_retention_references(plugin)))
    }
}
