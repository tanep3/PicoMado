//! MUSIC-PLAYER-20261107: silent lifecycle and persistence boundary regressions.
use super::*;
use serde_json::json;
const MUSIC: &str = "local-music";
fn open(temp: &tempfile::TempDir, clock_channel: bool) -> App {
    let mut music = crate::local_music::LocalMusicContent::default();
    let key = "a".repeat(64);
    music.load_state(json!({"tracks":{&key:{"key":key,"name":"one.wav","frames":4800,"streaming":true}},"devices":{},"next_generation":1})).unwrap();
    let mut set = crate::plugins::PluginSet::default();
    set.register(Box::new(crate::local_music::LocalMusicPlugin), Box::new(music)).unwrap();
    if clock_channel { set.register(Box::new(crate::clock::ClockPlugin), Box::new(crate::clock::ClockContent::default())).unwrap(); }
    App::open_with_plugin_set(&temp.path().join("db"), &temp.path().join("plugins"), Arc::new(crate::fetch::SystemClock), set).unwrap()
}
fn hello(app: &mut App) -> SessionToken { app.hello("a", ["lcd".into(), "media-input-v1".into()].into(), 1).unwrap() }
fn command(app: &mut App, kind: &str) {
    let revision = app.plugin_web_view(MUSIC).unwrap().revision;
    app.plugin_command(MUSIC, revision, json!({"kind":kind,"device_id":"a","key":"a".repeat(64)})).unwrap();
}
#[test]
fn navigation_save_failure_rejects_entry_and_exit_without_partial_playback() {
    let temp = tempfile::tempdir().unwrap(); let mut app = open(&temp, true); let token = hello(&mut app);
    command(&mut app, "play"); command(&mut app, "stop");
    app.button(&token, ButtonAction::Up).unwrap();
    let before = app.plugin_web_view(MUSIC).unwrap(); let selection = app.selection_for(&token);
    app.pause_next_persistence(Box::new(|| Err(AppError::Conflict("injected navigation save failure"))));
    assert!(app.button(&token, ButtonAction::Down).is_err());
    assert_eq!(app.selection_for(&token), selection);
    assert_eq!(app.plugin_web_view(MUSIC).unwrap().revision, before.revision);
    assert_eq!(app.plugin_web_view(MUSIC).unwrap().data, before.data);
    assert!(app.audio_playback_for(&token).is_none());
    assert_eq!(open(&temp, true).plugin_web_view(MUSIC).unwrap().data["devices"]["a"]["resting"], "stopped");
    app.button(&token, ButtonAction::Down).unwrap();
    let playing = app.audio_playback_for(&token).unwrap(); let selection = app.selection_for(&token);
    app.pause_next_persistence(Box::new(|| Err(AppError::Conflict("injected navigation save failure"))));
    assert!(app.button(&token, ButtonAction::Up).is_err());
    assert_eq!(app.selection_for(&token), selection);
    assert_eq!(app.audio_playback_for(&token).unwrap().2, playing.2);
}
#[test]
fn navigation_streaming_consumed_position_eof_and_stale_progress() {
    let temp = tempfile::tempdir().unwrap(); let mut app = open(&temp, true); let token = hello(&mut app);
    command(&mut app, "play");
    let first = app.audio_playback_for(&token).unwrap();
    app.audio_playback_progress(&token, &first.0, MUSIC, &first.2, 6400, false);
    app.button(&token, ButtonAction::Up).unwrap();
    let paused_position = app.plugin_web_view(MUSIC).unwrap().data["devices"]["a"]["position_ms"].clone();
    assert_eq!(paused_position, 133);
    app.audio_playback_progress(&token, &first.0, MUSIC, &first.2, 8000, false);
    assert_eq!(app.plugin_web_view(MUSIC).unwrap().data["devices"]["a"]["position_ms"], paused_position);
    app.button(&token, ButtonAction::Down).unwrap();
    let resumed = app.audio_playback_for(&token).unwrap(); assert_eq!(resumed.2.start_frame, 6400);
    app.audio_playback_progress(&token, &resumed.0, MUSIC, &resumed.2, 6500, true);
    assert!(app.audio_playback_for(&token).is_none());
    app.button(&token, ButtonAction::Up).unwrap(); app.button(&token, ButtonAction::Down).unwrap();
    assert_eq!(app.audio_playback_for(&token).unwrap().2.start_frame, 0);
}
#[test]
fn navigation_restart_paused_stopped_and_single_channel_no_edge() {
    for kind in ["pause", "stop"] {
        let temp = tempfile::tempdir().unwrap(); let mut app = open(&temp, true); let token = hello(&mut app);
        command(&mut app, "play"); command(&mut app, kind);
        drop(app); let mut app = open(&temp, true); let fresh = hello(&mut app);
        assert!(app.audio_playback_for(&fresh).is_none());
        assert!(app.button(&token, ButtonAction::Up).is_err());
        app.button(&fresh, ButtonAction::Up).unwrap(); app.button(&fresh, ButtonAction::Down).unwrap();
        assert_eq!(app.audio_playback_for(&fresh).is_some(), kind == "stop");
        if kind == "pause" {
            app.media_input(&fresh, crate::content::MediaOperation::TogglePlayback).unwrap();
            assert!(app.audio_playback_for(&fresh).is_some());
        }
    }
    let temp = tempfile::tempdir().unwrap(); let mut app = open(&temp, false); let token = hello(&mut app);
    command(&mut app, "play"); command(&mut app, "stop");
    let revision = app.plugin_web_view(MUSIC).unwrap().revision;
    for _ in 0..2 { app.button(&token, ButtonAction::Down).unwrap(); }
    assert!(app.audio_playback_for(&token).is_none());
    assert_eq!(app.plugin_web_view(MUSIC).unwrap().revision, revision);
}
