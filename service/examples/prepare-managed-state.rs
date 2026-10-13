//! Offline preparation of a stopped service's dedicated SQLite database.
//! Validate the local-video snapshot through the plugin, import it only when
//! the legacy placeholder has no history, and pause hourly image generation.

use picomado_core::application::App;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: prepare-managed-state DATABASE PLUGINS_ROOT [VIDEO_SNAPSHOT]".into());
    }
    if let Some(snapshot) = args.get(3) {
        let video: Value = serde_json::from_slice(&std::fs::read(snapshot)?)?;
        let mut youtube = picomado_core::plugins::contents()
            .remove("youtube")
            .ok_or("missing YouTube plugin")?;
        youtube
            .load_state(video.clone())
            .map_err(|_| "invalid local video snapshot")?;
        let mut connection = Connection::open(&args[1])?;
        let (schema, body): (i64, String) = connection.query_row(
            "SELECT schema_version,body FROM app_state WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if schema != 2 {
            return Err("unexpected database schema".into());
        }
        let mut state: Value = serde_json::from_str(&body)?;
        let old = &state["plugin_states"]["youtube"];
        if !old.is_null()
            && old["stored"]["videos"]
                .as_object()
                .is_none_or(|videos| !videos.is_empty())
        {
            return Err(
                "preserve existing YouTube history: import requires empty legacy state".into(),
            );
        }
        state["plugin_states"]["youtube"] = video;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE app_state SET body=?1 WHERE id=1",
            params![serde_json::to_string(&state)?],
        )?;
        transaction.commit()?;
    }
    let mut app = App::open_with_plugin_root(Path::new(&args[1]), Path::new(&args[2]))
        .map_err(|_| "prepared database validation failed")?;
    let revision = app
        .plugin_web_view("caricature")
        .ok_or("missing caricature plugin")?
        .revision;
    let view = app
        .plugin_command(
            "caricature",
            revision,
            serde_json::json!({
                "kind":"set_automatic_generation","enabled":false,
            }),
        )
        .map_err(|_| "unable to pause hourly generation")?;
    if view.data["settings"]["automatic_generation"] != false {
        return Err("hourly generation was not paused".into());
    }
    println!("prepared managed state; hourly generation paused; models unchanged");
    Ok(())
}
