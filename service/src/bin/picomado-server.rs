//! Explicit PicoMado user-service host. It reads only its own configuration,
//! creates its own data/cache directories, and stops on SIGTERM or Ctrl+C.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use picomado_core::{application::App, host::serve_plugins_on_listener};
use serde::Deserialize;

fn home() -> Result<PathBuf, String> {
    let value = std::env::var_os("HOME").ok_or("HOME is required")?;
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("HOME must be an absolute path without ..".into());
    }
    Ok(path)
}

fn expand_user_path(path: &Path, home: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("path must be UTF-8")?;
    let expanded = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        path.to_path_buf()
    };
    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("path must be absolute and contain no ..".into());
    }
    Ok(expanded)
}

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
        config.resolve_and_validate(&home()?)
    }

    fn resolve_and_validate(mut self, home: &Path) -> Result<Self, String> {
        self.database_path = expand_user_path(&self.database_path, home)?;
        self.plugins_root = expand_user_path(&self.plugins_root, home)?;
        if self.bind.port() == 0 || self.bind.ip().is_unspecified() || self.bind.ip().is_multicast()
        {
            return Err("bind must use a specific unicast address and nonzero port".into());
        }
        if self.plugins_root == Path::new("/") || self.plugins_root == home {
            return Err("plugins_root must be a dedicated directory".into());
        }
        if self.database_path.parent() != Some(self.plugins_root.join("data").as_path()) {
            return Err("database_path must be directly under plugins_root/data".into());
        }
        if self.database_path.file_name().is_none() {
            return Err("database_path must name a file".into());
        }
        Ok(self)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let config_path = match args.as_slice() {
        [_] => home()?.join(".config/PicoMado/config.toml"),
        [_, flag, path] if flag == "--config" => PathBuf::from(path),
        _ => return Err("usage: picomado-server [--config PATH]".into()),
    };
    let config = Config::load(&config_path)?;
    std::fs::create_dir_all(config.database_path.parent().expect("database parent"))?;
    for plugin in picomado_core::plugins::DEFAULT_ORDER {
        std::fs::create_dir_all(config.plugins_root.join(plugin).join("data"))?;
        std::fs::create_dir_all(config.plugins_root.join(plugin).join("cache"))?;
    }
    let app = Arc::new(picomado_core::shared::Application::new(
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
        let home = Path::new("/tmp/another-user");
        let parse = |body: &str| toml::from_str::<Config>(body).unwrap();
        let valid = "bind = '192.0.2.20:16153'\ndatabase_path = '~/.config/PicoMado/data/picomado.db'\nplugins_root = '~/.config/PicoMado'\n";
        let config = parse(valid).resolve_and_validate(home).unwrap();
        assert_eq!(config.plugins_root, home.join(".config/PicoMado"));
        assert_eq!(
            config.database_path,
            home.join(".config/PicoMado/data/picomado.db")
        );
        let legacy = "bind = '192.168.0.120:16153'\ndatabase_path = '/home/tane/.config/PicoMado/data/picomado.db'\nplugins_root = '/home/tane/.config/PicoMado'\n";
        assert!(parse(legacy).resolve_and_validate(home).is_ok());
        for bad in ["0.0.0.0", "224.0.0.1"] {
            assert!(parse(&valid.replace("192.0.2.20", bad))
                .resolve_and_validate(home)
                .is_err());
        }
        for bad in ["~/../escape/data/picomado.db", "/tmp/other.db"] {
            assert!(
                parse(&valid.replace("~/.config/PicoMado/data/picomado.db", bad))
                    .resolve_and_validate(home)
                    .is_err()
            );
        }
        assert!(toml::from_str::<Config>(&format!("{valid}unknown = true\n")).is_err());
        let absent = home.join(".config/PicoMado");
        assert!(!absent.exists()); // Validation never creates or migrates user data.
    }

    #[test]
    fn explicit_config_keeps_existing_data_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("existing");
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let database = data.join("picomado.db");
        std::fs::write(&database, b"existing database fixture").unwrap();
        let config_path = temp.path().join("custom.toml");
        std::fs::write(
            &config_path,
            format!(
                "bind = '127.0.0.1:16153'\ndatabase_path = '{}'\nplugins_root = '{}'\n",
                database.display(),
                root.display()
            ),
        )
        .unwrap();
        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.database_path, database);
        assert_eq!(
            std::fs::read(&database).unwrap(),
            b"existing database fixture"
        );
        assert!(!root.join("clock").exists());
    }
}
