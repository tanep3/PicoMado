//! Explicit PicoMado user-service host. It reads only its own configuration,
//! creates its own data/cache directories, and stops on SIGTERM or Ctrl+C.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use picomado_core::{application::App, host::serve_plugins_on_listener};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = "/home/tane/.config/PicoMado/config.toml";
const DATABASE_PATH: &str = "/home/tane/.config/PicoMado/data/picomado.db";
const PLUGINS_ROOT: &str = "/home/tane/.config/PicoMado";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    bind: SocketAddr,
    database_path: PathBuf,
    plugins_root: PathBuf,
}

impl Config {
    fn load(path: &Path) -> Result<Self, String> {
        let body = std::fs::read_to_string(path).map_err(|e| format!("config read failed: {e}"))?;
        let config: Self = toml::from_str(&body).map_err(|e| format!("config invalid: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.bind
            != "192.168.0.120:16153"
                .parse::<SocketAddr>()
                .expect("fixed address")
        {
            return Err("bind must be 192.168.0.120:16153".into());
        }
        if self.database_path != Path::new(DATABASE_PATH)
            || self.plugins_root != Path::new(PLUGINS_ROOT)
        {
            return Err("data and cache paths must stay under /home/tane/.config/PicoMado".into());
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let config_path = match args.as_slice() {
        [_] => PathBuf::from(DEFAULT_CONFIG),
        [_, flag, path] if flag == "--config" => PathBuf::from(path),
        _ => return Err("usage: picomado-server [--config PATH]".into()),
    };
    let config = Config::load(&config_path)?;
    std::fs::create_dir_all(config.database_path.parent().expect("database parent"))?;
    for plugin in picomado_core::plugins::DEFAULT_ORDER {
        std::fs::create_dir_all(config.plugins_root.join(plugin).join("data"))?;
        std::fs::create_dir_all(config.plugins_root.join(plugin).join("cache"))?;
    }
    let app = Arc::new(Mutex::new(
        App::open_with_plugin_root(&config.database_path, &config.plugins_root)
            .map_err(|e| format!("database open failed: {e:?}"))?,
    ));
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let host = serve_plugins_on_listener(listener, app, stop_rx);
    tokio::pin!(host);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = &mut host => result.map_err(|e| format!("host stopped: {e:?}").into()),
        result = tokio::signal::ctrl_c() => {
            result?;
            stop_tx.send(true).ok();
            host.await.map_err(|e| format!("host stop failed: {e:?}").into())
        }
        _ = terminate.recv() => {
            stop_tx.send(true).ok();
            host.await.map_err(|e| format!("host stop failed: {e:?}").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_config_requires_dedicated_lan_and_data_paths() {
        let valid = format!(
            "bind = '192.168.0.120:16153'\ndatabase_path = '{DATABASE_PATH}'\nplugins_root = '{PLUGINS_ROOT}'\n"
        );
        let config: Config = toml::from_str(&valid).unwrap();
        config.validate().unwrap();
        let wildcard: Config = toml::from_str(&valid.replace("192.168.0.120", "0.0.0.0")).unwrap();
        assert!(wildcard.validate().is_err());
        let foreign: Config =
            toml::from_str(&valid.replace(DATABASE_PATH, "/tmp/other.db")).unwrap();
        assert!(foreign.validate().is_err());
    }
}
