//! One-shot effect runner contract. The executor is replaceable and must be
//! idempotent by outbox ID/token because a crash can replay an external call.

use crate::application::{App, AppError};
use crate::Effect;

pub enum Outcome {
    Completed,
    FetchResult(Result<String, String>),
    Deferred,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExecutorError {
    Retryable,
}

pub trait EffectExecutor {
    fn execute(&mut self, outbox_id: u64, effect: &Effect) -> Result<Outcome, ExecutorError>;
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub completed: usize,
    pub deferred: usize,
    pub failed: usize,
}

pub fn run_pending(
    app: &mut App,
    executor: &mut impl EffectExecutor,
    limit: usize,
) -> Result<RunReport, AppError> {
    let mut report = RunReport::default();
    for (id, effect) in app.pending_effects() {
        if report.completed + report.failed >= limit {
            break;
        }
        if !app.effect_pending(id) {
            continue;
        }
        match executor.execute(id, &effect) {
            Ok(Outcome::Completed) if !matches!(effect, Effect::Fetch(_)) => {
                app.complete_effect(id)?;
                report.completed += 1;
            }
            Ok(Outcome::FetchResult(result)) if matches!(effect, Effect::Fetch(_)) => {
                app.resolve_fetch(id, result)?;
                report.completed += 1;
            }
            Ok(Outcome::Deferred) => report.deferred += 1,
            Ok(Outcome::Completed) | Ok(Outcome::FetchResult(_)) | Err(_) => report.failed += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::App;
    use crate::FetchKey;

    struct MockExecutor;
    impl EffectExecutor for MockExecutor {
        fn execute(&mut self, _outbox_id: u64, effect: &Effect) -> Result<Outcome, ExecutorError> {
            if matches!(effect, Effect::Fetch(_)) {
                Ok(Outcome::FetchResult(Ok("ready-1".into())))
            } else {
                Ok(Outcome::Deferred)
            }
        }
    }

    #[test]
    fn mock_fetch_executor_replays_after_restart_and_commits_result() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let effect_id;
        {
            let mut app = App::open(&path).unwrap();
            let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
            effect_id = app
                .begin_fetch(
                    "desk",
                    token.incarnation,
                    FetchKey {
                        plugin: "weather".into(),
                        key: "tokyo".into(),
                    },
                )
                .unwrap();
        }
        let mut app = App::open(&path).unwrap();
        let report = run_pending(&mut app, &mut MockExecutor, 100).unwrap();
        assert_eq!(report.completed, 1);
        assert!(!app.effect_pending(effect_id));
        assert_eq!(
            run_pending(&mut app, &mut MockExecutor, 100)
                .unwrap()
                .completed,
            0
        );
    }

    #[test]
    fn deferred_display_effect_does_not_starve_fetch_work() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let fetch = app
            .begin_fetch(
                "desk",
                token.incarnation,
                FetchKey {
                    plugin: "weather".into(),
                    key: "tokyo".into(),
                },
            )
            .unwrap();
        let report = run_pending(&mut app, &mut MockExecutor, 1).unwrap();
        assert_eq!(report.completed, 1);
        assert!(report.deferred >= 1);
        assert!(!app.effect_pending(fetch));
    }
}
