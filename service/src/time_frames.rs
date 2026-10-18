//! Wall-minute scheduling, separate from hourly fetches and media workers.
use crate::{application::AppError, web::SharedApp};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

fn until_next_minute(now: Duration) -> Duration {
    Duration::from_secs(60) - Duration::new(now.as_secs() % 60, now.subsec_nanos())
}

pub async fn run(app: SharedApp, mut stop: watch::Receiver<bool>) -> Result<(), AppError> {
    let mut changes = app
        .lock()
        .map_err(|_| AppError::Conflict("application unavailable"))?
        .dispatch_notifications();
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        app.lock()
            .map_err(|_| AppError::Conflict("application unavailable"))?
            .refresh_timed_frames()?;
        // Recompute from wall time after every wake, including clock adjustments.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        tokio::select! {
            _ = tokio::time::sleep(until_next_minute(now)) => {},
            result = changes.changed() => {
                if result.is_err() { return Err(AppError::Conflict("application unavailable")); }
            },
            result = stop.changed() => {
                if result.is_err() || *stop.borrow() { return Ok(()); }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligns_to_wall_minute_without_period_drift() {
        assert_eq!(
            until_next_minute(Duration::new(59, 999_000_000)),
            Duration::from_millis(1)
        );
        assert_eq!(
            until_next_minute(Duration::from_secs(60)),
            Duration::from_secs(60)
        );
        assert_eq!(
            until_next_minute(Duration::new(123, 250_000_000)),
            Duration::from_millis(56_750)
        );
    }
}
