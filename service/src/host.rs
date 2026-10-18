//! Explicit host lifecycle wiring. The caller supplies a bound listener;
//! this module never chooses an address or starts itself.

use std::future::Future;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::application::AppError;
use crate::web::{self, SharedApp};

#[derive(Debug)]
pub enum HostError {
    Server(std::io::Error),
    Runner(AppError),
}

async fn wait_shutdown(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow() && receiver.changed().await.is_ok() {}
}

/// Runs the WebSocket/API server and an injected plugin runner together. A
/// shutdown signal stops both before this function returns.
pub async fn serve_with_runner<R, Fut>(
    listener: TcpListener,
    app: SharedApp,
    runner: R,
    shutdown: watch::Receiver<bool>,
) -> Result<(), HostError>
where
    R: FnOnce(SharedApp, watch::Receiver<bool>) -> Fut,
    Fut: Future<Output = Result<(), AppError>>,
{
    let (stop_tx, stop_rx) = watch::channel(false);
    let web_app = app.clone();
    let server = async move {
        use axum::serve::ListenerExt;
        use std::os::fd::AsRawFd;
        let listener = listener.tap_io(|stream| {
            let size = crate::lcd_stream::SOCKET_SEND_BUFFER_BYTES as libc::c_int;
            // SO_SNDBUF is doubled by Linux (metadata included), so this
            // bounds kernel storage to 64KiB rather than auto-growing it.
            let result = unsafe {
                libc::setsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const libc::c_int).cast(),
                    std::mem::size_of_val(&size) as libc::socklen_t,
                )
            };
            if result != 0 {
                eprintln!("picomado socket_send_limit_failed");
                unsafe {
                    libc::shutdown(stream.as_raw_fd(), libc::SHUT_RDWR);
                }
            }
        });
        axum::serve(listener, web::router(web_app))
            .with_graceful_shutdown(wait_shutdown(stop_rx))
            .await
    };
    let worker = runner(app, stop_tx.subscribe());
    tokio::pin!(server);
    tokio::pin!(worker);
    let drain = std::time::Duration::from_secs(10);
    tokio::select! {
        _ = wait_shutdown(shutdown) => {
            stop_tx.send(true).ok();
            let server_result=tokio::time::timeout(drain,&mut server).await
                .map_err(|_|HostError::Server(std::io::Error::new(std::io::ErrorKind::TimedOut,"server shutdown timed out")))?;
            let worker_result=tokio::time::timeout(drain,&mut worker).await
                .map_err(|_|HostError::Runner(AppError::Conflict("plugin runner shutdown timed out")))?;
            server_result.map_err(HostError::Server)?;
            worker_result.map_err(HostError::Runner)
        }
        worker_result = &mut worker => {
            stop_tx.send(true).ok();
            tokio::time::timeout(drain,&mut server).await
                .map_err(|_|HostError::Server(std::io::Error::new(std::io::ErrorKind::TimedOut,"server shutdown timed out")))?
                .map_err(HostError::Server)?;
            match worker_result {
                Err(error) => Err(HostError::Runner(error)),
                Ok(()) => Err(HostError::Runner(AppError::Conflict("plugin runner stopped unexpectedly"))),
            }
        }
        server_result = &mut server => {
            stop_tx.send(true).ok();
            tokio::time::timeout(drain,&mut worker).await
                .map_err(|_|HostError::Runner(AppError::Conflict("plugin runner shutdown timed out")))?
                .map_err(HostError::Runner)?;
            server_result.map_err(HostError::Server)?;
            Err(HostError::Server(std::io::Error::other("server stopped unexpectedly")))
        }
    }
}

pub async fn serve_jma_on_listener(
    listener: TcpListener,
    app: SharedApp,
    shutdown: watch::Receiver<bool>,
) -> Result<(), HostError> {
    serve_with_runner(listener, app, crate::fetch::run_jma_periodic, shutdown).await
}

/// Three independent worker loops share only short state transitions.
pub async fn serve_plugins_on_listener(
    listener: TcpListener,
    app: SharedApp,
    shutdown: watch::Receiver<bool>,
) -> Result<(), HostError> {
    serve_with_runner(
        listener,
        app,
        |app, stop| async move {
            let executors = crate::plugins::executors();
            tokio::try_join!(
                crate::fetch::run_jma_periodic(app.clone(), stop.clone()),
                crate::workers::run_work(app.clone(), executors.clone(), stop.clone()),
                crate::playback::run_playback(app.clone(), executors, stop.clone()),
                crate::time_frames::run(app, stop),
            )?;
            Ok(())
        },
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::App;
    use crate::content::FetchJob;
    use crate::fetch::{run_cycle, Clock, SourceFetcher};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct FakeClock(AtomicI64);
    impl Clock for FakeClock {
        fn unix_seconds(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
        fn today_jst(&self) -> String {
            "2026-10-12".into()
        }
    }
    struct FakeFetcher(Arc<AtomicUsize>);
    impl SourceFetcher for FakeFetcher {
        fn fetch(&mut self, job: &FetchJob) -> Result<Vec<u8>, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            match job.plugin.as_str() {
                "weather" => {
                    assert_eq!(job.source_key, "130000");
                    Ok(include_bytes!("../assets/jma-tokyo-20261012.json").to_vec())
                }
                "rain-radar" => {
                    assert_eq!(job.source_key, "nowc/hrpns");
                    Ok(include_bytes!("../assets/jma-radar-20261012.json").to_vec())
                }
                _ => panic!("unexpected plugin source"),
            }
        }
    }

    #[tokio::test]
    async fn loopback_host_runs_initial_and_repeated_fetch_then_stops() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        app.lock()
            .unwrap()
            .hello("desk", ["lcd".into()].into(), 1)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let (stop_tx, stop_rx) = watch::channel(false);
        let fake_count = count.clone();
        let task = tokio::spawn(serve_with_runner(
            listener,
            app.clone(),
            move |app, mut shutdown| async move {
                let clock = FakeClock(AtomicI64::new(1_000_000));
                let mut fetcher = FakeFetcher(fake_count);
                let mut tick = tokio::time::interval(Duration::from_millis(30));
                loop {
                    tokio::select! {
                        _=tick.tick()=>{
                            {let mut guard=app.lock().unwrap();run_cycle(&mut guard,&clock,&mut fetcher)?;}
                            clock.0.fetch_add(3600,Ordering::SeqCst);
                            tokio::task::yield_now().await;
                        }
                        changed=shutdown.changed()=>if changed.is_err()||*shutdown.borrow(){return Ok(());}
                    }
                }
            },
            stop_rx,
        ));
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if count.load(Ordering::SeqCst) >= 4 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let response = reqwest::Client::new()
            .get(format!("http://{address}/api/state"))
            .header(reqwest::header::CONNECTION, "close")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.bytes().await.unwrap();
        stop_tx.send(true).unwrap();
        // The host permits up to ten seconds to drain the server and runner.
        tokio::time::timeout(Duration::from_secs(12), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let after = count.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(count.load(Ordering::SeqCst), after);
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn worker_failure_stops_listener_and_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        let app = Arc::new(Mutex::new(
            App::open(&temp.path().join("state.db")).unwrap(),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (_tx, rx) = watch::channel(false);
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            serve_with_runner(
                listener,
                app,
                |_, _| async { Err(AppError::Conflict("test runner failure")) },
                rx,
            ),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Err(HostError::Runner(AppError::Conflict("test runner failure")))
        ));
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }
}
