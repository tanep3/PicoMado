//! Explicit, manually stopped LAN trial host. It is never started by tests or
//! by importing the library, and accepts only the requested trial address.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use picomado_core::{application::App, host::serve_jma_on_listener};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6
        || args[1] != "--temporary"
        || args[2] != "--bind"
        || args[4] != "--db"
        || args[3] != "192.168.0.120:16153"
    {
        return Err(
            "usage: temporary_lan_host --temporary --bind 192.168.0.120:16153 --db PATH".into(),
        );
    }
    let db = PathBuf::from(&args[5]);
    if db.as_os_str().is_empty() {
        return Err("database path required".into());
    }
    let app = Arc::new(Mutex::new(
        App::open(&db).map_err(|e| format!("database open failed: {e:?}"))?,
    ));
    let listener = tokio::net::TcpListener::bind(&args[3]).await?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let host = serve_jma_on_listener(listener, app, stop_rx);
    tokio::pin!(host);
    tokio::select! {
        result=&mut host => result.map_err(|e|format!("host stopped: {e:?}").into()),
        result=tokio::signal::ctrl_c() => {
            result?;
            stop_tx.send(true).ok();
            host.await.map_err(|e|format!("host stop failed: {e:?}").into())
        }
    }
}
