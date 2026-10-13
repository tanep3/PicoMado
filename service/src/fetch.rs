//! One bounded plugin fetch cycle. The host supplies a clock and invokes this
//! after startup and on a timer; no listener or background worker is started.

use crate::application::{App, AppError};
use crate::content::FetchJob;
use futures_util::StreamExt;
use std::collections::BTreeMap;

pub trait Clock {
    fn unix_seconds(&self) -> i64;
    fn today_jst(&self) -> String;
}

pub trait SourceFetcher {
    fn fetch(&mut self, job: &FetchJob) -> Result<Vec<u8>, String>;
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct FetchReport {
    pub planned: usize,
    pub applied: usize,
    pub stale: usize,
    pub attempts: usize,
}

fn source_groups(
    jobs: Vec<FetchJob>,
) -> BTreeMap<(String, String, String, u32, u8), Vec<FetchJob>> {
    let mut groups = BTreeMap::new();
    for job in jobs {
        groups
            .entry((
                job.plugin.clone(),
                job.source_key.clone(),
                job.today_jst.clone(),
                job.timeout_seconds,
                job.retries,
            ))
            .or_insert_with(Vec::new)
            .push(job);
    }
    groups
}

pub fn run_cycle(
    app: &mut App,
    clock: &impl Clock,
    fetcher: &mut impl SourceFetcher,
) -> Result<FetchReport, AppError> {
    let now = clock.unix_seconds();
    let jobs = app.plan_plugin_fetches(now, &clock.today_jst())?;
    let mut report = FetchReport {
        planned: jobs.len(),
        ..FetchReport::default()
    };
    for group in source_groups(jobs).into_values() {
        let source = &group[0];
        let mut result = Err("not attempted".to_owned());
        for _ in 0..=source.retries {
            report.attempts += 1;
            result = fetcher.fetch(source);
            if result.is_ok() {
                break;
            }
        }
        for job in group {
            if app.complete_plugin_fetch(&job, clock.unix_seconds(), result.clone())? {
                report.applied += 1;
            } else {
                report.stale += 1;
            }
        }
    }
    Ok(report)
}

/// A future host can call this without holding the application lock during HTTP.
/// The adapter sends only supported public office codes to JMA.
pub async fn run_jma_cycle(app: &crate::web::SharedApp) -> Result<FetchReport, AppError> {
    let clock = SystemClock;
    let jobs = {
        let mut guard = app
            .lock()
            .map_err(|_| AppError::Conflict("application unavailable"))?;
        guard.plan_plugin_fetches(clock.unix_seconds(), &clock.today_jst())?
    };
    let mut report = FetchReport {
        planned: jobs.len(),
        ..FetchReport::default()
    };
    let groups = source_groups(jobs);
    let fetched = futures_util::stream::iter(groups.into_values().map(|group| async move {
        let source = &group[0];
        let mut result = Err("not attempted".to_owned());
        let mut attempts = 0;
        for _ in 0..=source.retries {
            attempts += 1;
            result = crate::plugins::fetch(source).await;
            if result.is_ok() {
                break;
            }
        }
        (group, result, attempts)
    }))
    .buffer_unordered(2);
    tokio::pin!(fetched);
    while let Some((group, result, attempts)) = fetched.next().await {
        report.attempts += attempts;
        let mut guard = app
            .lock()
            .map_err(|_| AppError::Conflict("application unavailable"))?;
        for job in group {
            if guard.complete_plugin_fetch(&job, clock.unix_seconds(), result.clone())? {
                report.applied += 1;
            } else {
                report.stale += 1;
            }
        }
    }
    Ok(report)
}

/// Starts only when explicitly called by a host; shutting down the host stops
/// this loop. The first cycle runs immediately; frequent light checks pick up
/// newly registered/selected devices and clock-day changes without waiting for
/// the hourly refresh deadline owned by each plugin.
pub async fn run_jma_periodic(
    app: crate::web::SharedApp,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), AppError> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                tokio::select! {
                    result = run_jma_cycle(&app) => { result?; }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            let mut guard = app.lock().map_err(|_| AppError::Conflict("application unavailable"))?;
                            guard.cancel_plugin_fetches()?;
                            return Ok(());
                        }
                    }
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
            }
        }
    }
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn unix_seconds(&self) -> i64 {
        time::OffsetDateTime::now_utc().unix_timestamp()
    }
    fn today_jst(&self) -> String {
        let now = time::OffsetDateTime::now_utc()
            .to_offset(time::UtcOffset::from_hms(9, 0, 0).expect("JST offset"));
        format!(
            "{:04}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ButtonAction;
    use crate::settings::PluginSettings;
    use crate::weather::{render_error_lcd, LOCATIONS};
    use std::collections::VecDeque;

    struct FakeClock {
        now: i64,
        today: &'static str,
    }
    impl Clock for FakeClock {
        fn unix_seconds(&self) -> i64 {
            self.now
        }
        fn today_jst(&self) -> String {
            self.today.into()
        }
    }
    #[derive(Default)]
    struct FakeFetcher {
        answers: VecDeque<Result<Vec<u8>, String>>,
        calls: Vec<(String, u32)>,
    }
    impl SourceFetcher for FakeFetcher {
        fn fetch(&mut self, job: &FetchJob) -> Result<Vec<u8>, String> {
            self.calls
                .push((job.source_key.clone(), job.timeout_seconds));
            if job.plugin == "rain-radar" {
                return Ok(include_bytes!("../assets/jma-radar-20261012.json").to_vec());
            }
            self.answers
                .pop_front()
                .unwrap_or_else(|| Err("missing fake answer".into()))
        }
    }
    fn source() -> Vec<u8> {
        include_bytes!("../assets/jma-tokyo-20261012.json").to_vec()
    }
    fn select_weather(app: &mut App, token: &crate::SessionToken) {
        app.button(token, ButtonAction::Down).unwrap();
        app.button(token, ButtonAction::Down).unwrap();
    }

    #[test]
    fn cached_success_hourly_refresh_retry_error_and_browse_pin() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let app_clock: std::sync::Arc<dyn Clock + Send + Sync> = std::sync::Arc::new(FakeClock {
            now: 1_000_000,
            today: "2026-10-12",
        });
        let mut app = App::open_with_clock(&path, app_clock.clone()).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_weather(&mut app, &token);
        let mut clock = FakeClock {
            now: 1_000_000,
            today: "2026-10-12",
        };
        let mut fetcher = FakeFetcher::default();
        fetcher.answers.push_back(Ok(source()));
        let first = run_cycle(&mut app, &clock, &mut fetcher).unwrap();
        assert_eq!((first.planned, first.applied, first.attempts), (2, 2, 2));
        assert_eq!(
            fetcher.calls,
            [("nowc/hrpns".into(), 20), ("130000".into(), 20)]
        );
        let first_id = app.plugin_position(&token).unwrap().dataset_id;
        assert_eq!(
            run_cycle(&mut app, &clock, &mut fetcher).unwrap().planned,
            0
        );
        clock.now += 3599;
        assert_eq!(
            run_cycle(&mut app, &clock, &mut fetcher).unwrap().planned,
            0
        );
        app.button(&token, ButtonAction::Sub).unwrap();
        app.button(&token, ButtonAction::Down).unwrap();
        assert_eq!(app.plugin_position(&token).unwrap().index, 1);
        clock.now += 1;
        fetcher.answers.push_back(Ok(source()));
        assert_eq!(
            run_cycle(&mut app, &clock, &mut fetcher).unwrap().applied,
            2
        );
        assert_eq!(app.plugin_position(&token).unwrap().dataset_id, first_id);
        app.button(&token, ButtonAction::Power).unwrap();
        assert_ne!(app.plugin_position(&token).unwrap().dataset_id, first_id);
        assert_eq!(app.plugin_position(&token).unwrap().index, 0);
        let last_success = app.plugin_device_config("desk", "weather").unwrap()
            ["last_success_unix"]
            .as_i64()
            .unwrap();
        clock.now += 3600;
        for _ in 0..3 {
            fetcher.answers.push_back(Err("test failure".into()));
        }
        let failed = run_cycle(&mut app, &clock, &mut fetcher).unwrap();
        assert_eq!((failed.planned, failed.attempts), (2, 4));
        let config = app.plugin_device_config("desk", "weather").unwrap();
        assert_eq!(config["last_success_unix"], last_success);
        assert_eq!(config["error"], true);
        assert!(app
            .pending_effects()
            .iter()
            .any(|(_, effect)| matches!(effect, crate::Effect::SendFrame { .. })));
        assert_eq!(
            render_error_lcd(&LOCATIONS[0]).len(),
            crate::protocol::LCD_FRAME_BYTES
        );
        drop(app);
        let mut reopened = App::open_with_clock(&path, app_clock).unwrap();
        let new_token = reopened.hello("desk", ["lcd".into()].into(), 2).unwrap();
        assert_eq!(
            reopened.plugin_device_config("desk", "weather").unwrap()["error"],
            true
        );
        assert_eq!(reopened.plugin_position(&new_token).unwrap().index, 0);
        assert!(reopened
            .pending_effects()
            .iter()
            .any(|(_, effect)| matches!(effect, crate::Effect::SendFrame { .. })));
    }

    #[test]
    fn cancelled_inflight_fetch_can_be_replanned_without_waiting_an_hour() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        let old = app
            .plan_plugin_fetches(100, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        app.cancel_plugin_fetches().unwrap();
        assert!(!app.complete_plugin_fetch(&old, 101, Ok(source())).unwrap());
        let fresh = app
            .plan_plugin_fetches(101, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        assert!(fresh.generation > old.generation);
    }

    #[test]
    fn selected_region_change_discards_old_frame_and_fetch_result() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_weather(&mut app, &token);
        let first = app
            .plan_plugin_fetches(100, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        app.complete_plugin_fetch(&first, 100, Ok(source()))
            .unwrap();
        let old_frame = app
            .pending_effects()
            .into_iter()
            .find_map(|(_, effect)| match effect {
                crate::Effect::SendFrame { token, .. } => Some(token),
                _ => None,
            })
            .unwrap();
        let old_fetch = app
            .plan_plugin_fetches(3700, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        app.set_plugin_device_config(
            "desk",
            token.incarnation,
            "weather",
            1,
            serde_json::json!({"location_id":"ogasawara"}),
        )
        .unwrap();
        assert!(!app.frame_is_current(&old_frame));
        assert!(!app
            .complete_plugin_fetch(&old_fetch, 3701, Ok(source()))
            .unwrap());
        assert!(app.plugin_position(&token).is_none());
        let fresh = app
            .plan_plugin_fetches(3701, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        assert_eq!(fresh.config_revision, 2);
        assert!(app
            .complete_plugin_fetch(&fresh, 3702, Ok(source()))
            .unwrap());
        assert_eq!(app.plugin_position(&token).unwrap().index, 0);
    }

    #[test]
    fn offline_location_is_isolated_and_old_fetch_cannot_overwrite_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
        let a = app.hello("a", ["lcd".into()].into(), 1).unwrap();
        let b = app.hello("b", ["lcd".into()].into(), 1).unwrap();
        app.disconnect(b.clone()).unwrap();
        let jobs = app.plan_plugin_fetches(100, "2026-10-12").unwrap();
        assert_eq!(jobs.len(), 4);
        let old_b = jobs
            .iter()
            .find(|j| j.device_id == "b" && j.plugin == "weather")
            .unwrap()
            .clone();
        assert_eq!(old_b.timeout_seconds, 20);
        let revision = app
            .set_plugin_device_config(
                "b",
                b.incarnation,
                "weather",
                1,
                serde_json::json!({"location_id":"ogasawara"}),
            )
            .unwrap();
        assert_eq!(revision, 2);
        assert!(!app
            .complete_plugin_fetch(&old_b, 101, Ok(source()))
            .unwrap());
        assert_eq!(
            app.plugin_device_config("a", "weather").unwrap()["location_id"],
            "tokyo-station"
        );
        assert_eq!(
            app.plugin_device_config("b", "weather").unwrap()["location_id"],
            "ogasawara"
        );
        app.complete_plugin_fetch(
            jobs.iter()
                .find(|j| j.device_id == "a" && j.plugin == "weather")
                .unwrap(),
            101,
            Ok(source()),
        )
        .unwrap();
        let mut policy = PluginSettings::default();
        policy.weather_fetch.timeout_seconds = 30;
        policy.weather_fetch.retries = 1;
        app.set_plugin_settings(1, policy).unwrap();
        let new_jobs: Vec<_> = app
            .plan_plugin_fetches(102, "2026-10-12")
            .unwrap()
            .into_iter()
            .filter(|j| j.plugin == "weather")
            .collect();
        assert_eq!(new_jobs.len(), 1);
        assert_eq!(new_jobs[0].device_id, "b");
        assert_eq!((new_jobs[0].timeout_seconds, new_jobs[0].retries), (30, 1));
        app.complete_plugin_fetch(&new_jobs[0], 103, Ok(source()))
            .unwrap();
        drop(app);
        let app = App::open(&path).unwrap();
        assert_eq!(
            app.plugin_device_config("b", "weather").unwrap()["location_id"],
            "ogasawara"
        );
        assert_eq!(
            app.plugin_device_config("a", "weather").unwrap()["location_id"],
            "tokyo-station"
        );
        assert_eq!(
            app.plugin_device_config("b", "weather").unwrap()["last_success_unix"],
            103
        );
        assert_ne!(a.incarnation, b.incarnation);
    }

    #[test]
    fn shared_source_is_fetched_once_but_applied_with_each_device_generation() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        app.hello("a", ["lcd".into()].into(), 1).unwrap();
        app.hello("b", ["lcd".into()].into(), 1).unwrap();
        let mut fetcher = FakeFetcher::default();
        fetcher.answers.push_back(Ok(source()));
        let clock = FakeClock {
            now: 100,
            today: "2026-10-12",
        };
        let report = run_cycle(&mut app, &clock, &mut fetcher).unwrap();
        assert_eq!((report.planned, report.applied, report.attempts), (4, 4, 2));
        assert_eq!(
            fetcher.calls,
            [("nowc/hrpns".into(), 20), ("130000".into(), 20)]
        );
        for id in ["a", "b"] {
            assert_eq!(
                app.plugin_device_config(id, "weather").unwrap()["last_success_unix"],
                100
            );
            assert_eq!(
                app.plugin_device_config(id, "rain-radar").unwrap()["last_success_unix"],
                100
            );
        }
    }

    #[test]
    fn failure_uses_success_freshness_and_browse_keeps_its_position_while_showing_error() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_weather(&mut app, &token);
        let first = app
            .plan_plugin_fetches(100, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        app.complete_plugin_fetch(&first, 100, Ok(source()))
            .unwrap();
        app.button_at(&token, ButtonAction::Sub, "2026-10-12")
            .unwrap();
        app.button_at(&token, ButtonAction::Down, "2026-10-12")
            .unwrap();
        let pinned = app.plugin_position(&token).unwrap();
        let failed = app
            .plan_plugin_fetches(3700, "2026-10-12")
            .unwrap()
            .into_iter()
            .find(|j| j.plugin == "weather")
            .unwrap();
        app.complete_plugin_fetch(&failed, 3700, Err("offline".into()))
            .unwrap();
        assert_eq!(app.plugin_position(&token).unwrap(), pinned);
        let (frame, handle) = app
            .pending_effects()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                crate::Effect::SendFrame {
                    token,
                    asset_handle,
                    ..
                } => Some((token, asset_handle)),
                _ => None,
            })
            .next_back()
            .unwrap();
        assert!(app.frame_is_current(&frame));
        assert_eq!(
            &*app.frame_asset(&handle).unwrap(),
            &render_error_lcd(&LOCATIONS[0])
        );
        assert!(app
            .plan_plugin_fetches(3759, "2026-10-12")
            .unwrap()
            .into_iter()
            .all(|j| j.plugin != "weather"));
        assert!(app
            .plan_plugin_fetches(3760, "2026-10-12")
            .unwrap()
            .into_iter()
            .any(|j| j.plugin == "weather"));
    }

    #[test]
    fn midnight_cancel_rebases_to_actual_day_and_missing_day_never_uses_yesterday() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        select_weather(&mut app, &token);
        app.publish_plugin_source("weather", &token, 1, &source(), "old", "2026-10-12")
            .unwrap();
        app.button_at(&token, ButtonAction::Sub, "2026-10-12")
            .unwrap();
        app.button_at(&token, ButtonAction::Down, "2026-10-12")
            .unwrap();
        let pinned = app.plugin_position(&token).unwrap();
        assert_eq!(pinned.target_date, "2026-10-13");
        app.button_at(&token, ButtonAction::Power, "2026-10-13")
            .unwrap();
        let normal = app.plugin_position(&token).unwrap();
        assert_eq!(
            (normal.target_date.as_str(), normal.index),
            ("2026-10-13", 0)
        );
        app.button_at(&token, ButtonAction::Sub, "2026-10-19")
            .unwrap();
        assert!(app.plugin_position(&token).is_none());
        let (frame, handle) = app
            .pending_effects()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                crate::Effect::SendFrame {
                    token,
                    asset_handle,
                    ..
                } => Some((token, asset_handle)),
                _ => None,
            })
            .next_back()
            .unwrap();
        assert!(app.frame_is_current(&frame));
        assert_eq!(
            &*app.frame_asset(&handle).unwrap(),
            &render_error_lcd(&LOCATIONS[0])
        );
    }
}
