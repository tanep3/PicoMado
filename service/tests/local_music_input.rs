//! MUSIC-PLAYER-20261107: exercise the real App input/aggregate boundary.
//! Metadata and a temporary database only: no decoder, speaker, socket or device.
use picomado_core::{
    application::{App, AppError, InputResult},
    clock::{ClockContent, ClockPlugin},
    content::{ButtonAction, ContentPlugin, MediaOperation},
    local_music::{LocalMusicContent, LocalMusicPlugin, SAMPLE_RATE},
    plugin_jobs::{AudioPlaybackSpec, PluginPaths},
    plugins::PluginSet,
    DisplayToken, Engine, Event, Plugin, Registry, SessionToken, Transition, View, World,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const PLUGIN: &str = "local-music";
type Offer = (DisplayToken, String, AudioPlaybackSpec, PluginPaths);

struct Fixture {
    app: App,
    _temp: tempfile::TempDir,
    key: String,
}
impl Fixture {
    fn new(frames: u64) -> Self {
        Self::with_tracks(frames, 2)
    }
    fn with_tracks(frames: u64, count: u8) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        let mut music = LocalMusicContent::default();
        // Use the same validated state loader as restart, without filesystem audio.
        let tracks: std::collections::BTreeMap<_, _> = (0..count)
            .map(|i| {
                let key = ((b'a' + i) as char).to_string().repeat(64);
                (
                    key.clone(),
                    json!({"key":key,"name":format!("fixture-{i}.wav"),"frames":frames}),
                )
            })
            .collect();
        music
            .load_state(json!({"tracks":tracks,"devices":{},"next_generation":1}))
            .unwrap();
        let mut plugins = PluginSet::default();
        plugins
            .register(Box::new(LocalMusicPlugin), Box::new(music))
            .unwrap();
        plugins
            .register(Box::new(ClockPlugin), Box::new(ClockContent::default()))
            .unwrap();
        let app = App::open_with_plugin_set(
            &temp.path().join("state.db"),
            &temp.path().join("plugins"),
            Arc::new(picomado_core::fetch::SystemClock),
            plugins,
        )
        .unwrap();
        Self {
            app,
            _temp: temp,
            key,
        }
    }
    fn hello(&mut self, id: &str) -> SessionToken {
        self.hello_with_caps(id, &["lcd", "bluetooth-audio-v1"])
    }
    fn hello_with_caps(&mut self, id: &str, caps: &[&str]) -> SessionToken {
        let token = self
            .app
            .hello(id, caps.iter().map(|c| (*c).into()).collect(), 1)
            .unwrap();
        assert_eq!(
            self.app.selection_for(&token).unwrap().0.as_deref(),
            Some(PLUGIN)
        );
        token
    }
    fn command(&mut self, id: &str, kind: &str) {
        let revision = self.app.plugin_web_view(PLUGIN).unwrap().revision;
        self.app
            .plugin_command(
                PLUGIN,
                revision,
                json!({
                    "kind":kind,"device_id":id,"key":self.key
                }),
            )
            .unwrap();
    }
    fn stopped(&mut self, id: &str) -> SessionToken {
        let token = self.hello(id);
        self.command(id, "play");
        self.command(id, "stop");
        assert!(self.app.audio_playback_for(&token).is_none());
        token
    }
    fn sub(&mut self, token: &SessionToken) {
        assert!(matches!(
            self.app
                .button_at(token, ButtonAction::Sub, "2026-11-07")
                .unwrap(),
            InputResult::Position(_)
        ));
    }
    fn state(&self, id: &str) -> Value {
        self.app.plugin_web_view(PLUGIN).unwrap().data["devices"][id].clone()
    }
}

fn same_offer(app: &App, token: &SessionToken, old: &Offer) -> bool {
    // Exact recheck used by the audio writer after an asynchronous source read.
    app.audio_playback_for(token)
        .is_some_and(|(display, plugin, spec, _)| {
            display == old.0 && plugin == old.1 && spec == old.2
        })
}

fn wait_until(app: &App, mono_ms: u64) {
    while app.monotonic_millis() < mono_ms {
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn sub_commits_play_pause_resume_and_rejects_previous_audio_offer() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let token = f.stopped("one");
    let before = f.app.selection_for(&token).unwrap().1;
    f.sub(&token);
    let first = f
        .app
        .audio_playback_for(&token)
        .expect("accepted Sub must retain plugin mutation");
    assert_eq!(first.2.start_frame, 0);
    assert!(first.0.display_epoch > before);
    assert_eq!(f.app.session_mode(&token), Some("normal"));
    wait_until(&f.app, first.2.started_mono_ms + 5);
    f.sub(&token);
    assert!(
        f.app.audio_playback_for(&token).is_none(),
        "second Sub pauses"
    );
    assert!(!same_offer(&f.app, &token, &first));
    let paused = f.state("one");
    assert_eq!(paused["playing"], false);
    assert!(paused["position_ms"].as_u64().unwrap() >= 5);
    wait_until(&f.app, f.app.monotonic_millis() + 5);
    assert_eq!(f.state("one"), paused, "pause holds position");
    f.sub(&token);
    let resumed = f.app.audio_playback_for(&token).expect("third Sub resumes");
    assert_eq!(
        resumed.2.start_frame / (SAMPLE_RATE as u64 / 1000),
        paused["position_ms"].as_u64().unwrap()
    );
    assert!(resumed.2.generation > first.2.generation);
    assert!(resumed.0.display_epoch > first.0.display_epoch);
    assert!(!same_offer(&f.app, &token, &first));
}

#[test]
fn one_sub_replays_an_ended_track_from_the_start() {
    let mut f = Fixture::new(48); // One millisecond of metadata, no audio is read.
    let token = f.hello("one");
    f.command("one", "play");
    let ended = f.app.audio_playback_for(&token).unwrap();
    wait_until(&f.app, ended.2.started_mono_ms + 2);
    assert_eq!(f.state("one")["playing"], false);
    f.sub(&token);
    let replay = f
        .app
        .audio_playback_for(&token)
        .expect("ended playback needs only one Sub");
    assert_eq!(replay.2.start_frame, 0);
    assert!(replay.2.generation > ended.2.generation);
    assert!(replay.2.started_mono_ms > ended.2.started_mono_ms);
    assert!(!same_offer(&f.app, &token, &ended));
}

#[test]
fn sub_changes_only_the_target_device() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let a = f.stopped("a");
    let b = f.stopped("b");
    f.sub(&a);
    f.sub(&b);
    let b_offer = f.app.audio_playback_for(&b).unwrap();
    f.sub(&a);
    assert!(f.app.audio_playback_for(&a).is_none());
    assert!(same_offer(&f.app, &b, &b_offer));
    f.sub(&a);
    assert!(f.app.audio_playback_for(&a).is_some());
    assert!(same_offer(&f.app, &b, &b_offer));
}

#[test]
fn normal_up_down_still_select_channels_without_changing_tracks() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let token = f.hello("one");
    f.command("one", "play");
    let first = f.app.audio_playback_for(&token).unwrap();
    f.app.button(&token, ButtonAction::Up).unwrap();
    assert_eq!(
        f.app.selection_for(&token).unwrap().0.as_deref(),
        Some("clock")
    );
    assert!(f.app.audio_playback_for(&token).is_none());
    assert!(!same_offer(&f.app, &token, &first));
    f.app.button(&token, ButtonAction::Down).unwrap();
    assert_eq!(
        f.app.selection_for(&token).unwrap().0.as_deref(),
        Some(PLUGIN)
    );
    let returned = f.app.audio_playback_for(&token).unwrap();
    assert_eq!(returned.2.asset_id, first.2.asset_id, "navigation keeps the track");
    assert!(returned.2.generation > first.2.generation, "reentry resumes a fresh transfer");
    assert!(
        !same_offer(&f.app, &token, &first),
        "display epoch still fences old reads"
    );
}

#[test]
fn reconnect_and_recreate_reject_stale_sub_and_do_not_autoplay() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let old = f.hello("one");
    f.command("one", "play");
    let old_offer = f.app.audio_playback_for(&old).unwrap();
    let current = f.hello("one");
    assert_ne!(old.session_epoch, current.session_epoch);
    assert!(f.app.audio_playback_for(&current).is_none());
    let before = f.state("one");
    assert!(matches!(
        f.app.button(&old, ButtonAction::Sub),
        Err(AppError::Conflict("stale session"))
    ));
    assert!(!f
        .app
        .terminal_event(Event::PluginInput {
            token: old.clone(),
            name: "toggle-playback".into()
        })
        .unwrap());
    assert_eq!(f.state("one"), before);
    assert!(!same_offer(&f.app, &old, &old_offer));
    f.sub(&current);
    assert!(f.app.audio_playback_for(&current).is_some());
    f.app.delete_device("one", current.incarnation).unwrap();
    let recreated = f.hello("one");
    assert_ne!(recreated.incarnation, current.incarnation);
    assert!(f.app.button(&current, ButtonAction::Sub).is_err());
    f.app.button(&recreated, ButtonAction::Sub).unwrap();
    assert!(
        f.app.audio_playback_for(&recreated).is_none(),
        "deleted selection is not reused"
    );
    assert!(f.state("one").is_null());
}

#[test]
fn unknown_or_wrong_mode_input_has_no_music_candidate() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let token = f.stopped("one");
    for name in ["up", "down", "unknown"] {
        assert!(!f
            .app
            .terminal_event(Event::PluginInput {
                token: token.clone(),
                name: name.into()
            })
            .unwrap());
    }
    f.app.button(&token, ButtonAction::Power).unwrap();
    assert_ne!(f.app.session_mode(&token), Some("normal"));
    assert!(!f
        .app
        .terminal_event(Event::PluginInput {
            token: token.clone(),
            name: "toggle-playback".into()
        })
        .unwrap());
    f.app.button(&token, ButtonAction::Sub).unwrap();
    assert!(f.app.audio_playback_for(&token).is_none());
}

#[test]
fn music_rule_targets_only_the_input_device_and_selected_plugin() {
    let mut registry = Registry::default();
    registry.register(Box::new(LocalMusicPlugin)).unwrap();
    let engine = Engine::new(registry);
    let mut world = World::new(vec![PLUGIN.into()]);
    for id in ["a", "b"] {
        world = engine
            .handle(
                &world,
                &Event::Hello {
                    device_id: id.into(),
                    capabilities: ["lcd".into()].into(),
                    screen_owner_epoch: 1,
                },
            )
            .unwrap()
            .state;
    }
    let device = &world.devices["a"];
    let token = SessionToken {
        device_id: "a".into(),
        incarnation: device.incarnation,
        session_epoch: device.session_epoch,
    };
    let event = Event::PluginInput {
        token: token.clone(),
        name: "toggle-playback".into(),
    };
    let view = View(&world);
    let rules = LocalMusicPlugin;
    let candidates: Vec<_> = rules.candidates(&view, &event).collect();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].scope, token.device_id);
    assert_eq!(
        candidates[0].transition,
        Transition::PluginDataChanged {
            device_id: token.device_id.clone(),
            incarnation: token.incarnation,
            plugin: PLUGIN.into()
        }
    );
    let changed = engine.handle(&world, &event).unwrap();
    assert!(changed.applied.is_some());
    assert_eq!(changed.state.devices["b"], world.devices["b"]);
    // Query fixtures exercise rule refusal even before the Engine's selected-plugin route.
    world.devices.get_mut("a").unwrap().selected = Some("clock".into());
    assert_eq!(rules.candidates(&View(&world), &event).count(), 0);
}

#[test]
fn physical_sub_cycles_new_bt_and_non_bt_devices_preserving_playback() {
    for caps in [
        vec!["lcd", "bluetooth-audio-v1", "media-input-v1"],
        vec!["lcd"],
    ] {
        let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
        let t = f.hello_with_caps("one", &caps);
        f.command("one", "play");
        let first = f.app.audio_playback_for(&t).unwrap();
        f.sub(&t);
        let next = f.app.audio_playback_for(&t).unwrap();
        assert_eq!(next.2.asset_id, "b".repeat(64));
        assert_eq!(next.2.start_frame, 0);
        assert!(next.2.generation > first.2.generation);
        assert!(!same_offer(&f.app, &t, &first));
        f.sub(&t);
        assert_eq!(f.app.audio_playback_for(&t).unwrap().2.asset_id, f.key);
        f.command("one", "pause");
        f.sub(&t);
        assert!(f.app.audio_playback_for(&t).is_none());
        assert_eq!(f.state("one")["key"], "b".repeat(64));
        assert_eq!(f.state("one")["position_ms"], 0);
    }
}

#[test]
fn media_short_toggle_and_channel_navigation_keep_their_existing_meaning() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let t = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
    f.sub(&t); // Physical Sub selects the first track without autoplay.
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    let first = f.app.audio_playback_for(&t).unwrap();
    wait_until(&f.app, first.2.started_mono_ms + 5);
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    assert!(f.app.audio_playback_for(&t).is_none());
    let paused = f.state("one");
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    let resumed = f.app.audio_playback_for(&t).unwrap();
    assert_eq!(
        resumed.2.start_frame / 48,
        paused["position_ms"].as_u64().unwrap()
    );
    f.app.media_input(&t, MediaOperation::ChannelNext).unwrap();
    assert_eq!(f.app.selection_for(&t).unwrap().0.as_deref(), Some("clock"));
    let before = f.app.selection_for(&t);
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    assert_eq!(f.app.selection_for(&t), before);
    f.app
        .media_input(&t, MediaOperation::ChannelPrevious)
        .unwrap();
    assert_eq!(f.app.audio_playback_for(&t).unwrap().2.asset_id, f.key);
    f.app.media_input(&t, MediaOperation::Cancel).unwrap();
    assert_eq!(f.app.session_mode(&t), Some("standby"));
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    assert!(f.app.audio_playback_for(&t).is_none());
}

#[test]
fn zero_one_unselected_and_ended_tracks_have_bounded_semantics() {
    for count in [0, 1] {
        let mut f = Fixture::with_tracks(SAMPLE_RATE as u64 * 60, count);
        let t = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
        f.app
            .media_input(&t, MediaOperation::TogglePlayback)
            .unwrap();
        assert!(f.state("one").is_null()); // Toggle does not select an unknown track.
        f.app.button(&t, ButtonAction::Sub).unwrap();
        assert!(f.app.audio_playback_for(&t).is_none());
        if count == 0 {
            f.app
                .media_input(&t, MediaOperation::TogglePlayback)
                .unwrap();
            assert!(f.state("one").is_null());
        } else {
            f.app
                .media_input(&t, MediaOperation::TogglePlayback)
                .unwrap();
            let playing = f.app.audio_playback_for(&t).unwrap();
            f.sub(&t);
            assert!(same_offer(&f.app, &t, &playing));
        }
    }
    let mut f = Fixture::new(48);
    let t = f.hello_with_caps("ended", &["lcd", "media-input-v1"]);
    f.sub(&t);
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    let first = f.app.audio_playback_for(&t).unwrap();
    wait_until(&f.app, first.2.started_mono_ms + 2);
    f.app
        .media_input(&t, MediaOperation::TogglePlayback)
        .unwrap();
    let replay = f.app.audio_playback_for(&t).unwrap();
    assert_eq!(replay.2.start_frame, 0);
    assert!(replay.2.generation > first.2.generation);
    wait_until(&f.app, replay.2.started_mono_ms + 2);
    f.sub(&t);
    assert!(f.app.audio_playback_for(&t).is_none());
    assert_eq!(f.state("ended")["key"], "b".repeat(64));
}

#[test]
fn media_rejects_stale_and_unnegotiated_input() {
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let old = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
    let current = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
    assert!(f
        .app
        .media_input(&old, MediaOperation::TogglePlayback)
        .is_err());
    let legacy = f.hello("legacy");
    assert!(f
        .app
        .media_input(&legacy, MediaOperation::TogglePlayback)
        .is_err());
    f.app.delete_device("one", current.incarnation).unwrap();
    let _new = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
    assert!(f
        .app
        .media_input(&current, MediaOperation::TogglePlayback)
        .is_err());
}

#[test]
fn media_volume_uses_current_revision_clamps_and_never_unmutes() {
    use picomado_core::{application::BluetoothAction, bluetooth::Volume};
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let a = f.hello_with_caps("a", &["lcd", "bluetooth-audio-v1", "media-input-v1"]);
    let b = f.hello_with_caps("b", &["lcd", "bluetooth-audio-v1", "media-input-v1"]);
    let initial = f.app.bluetooth_view("a");
    f.app
        .bluetooth_command(
            "a",
            a.incarnation,
            initial.revision,
            BluetoothAction::SetVolume {
                volume: Volume::try_from(98).unwrap(),
            },
        )
        .unwrap();
    let stale = f.app.bluetooth_view("a").revision;
    f.app.media_input(&a, MediaOperation::VolumeUp).unwrap();
    let upper = f.app.bluetooth_view("a");
    assert_eq!(upper.desired.volume.value(), 100);
    assert!(upper.desired.muted);
    assert_eq!(f.app.audio_volume(&a), 0);
    f.app.media_input(&a, MediaOperation::VolumeUp).unwrap();
    assert_eq!(f.app.bluetooth_view("a").revision, upper.revision);
    assert!(f
        .app
        .bluetooth_command(
            "a",
            a.incarnation,
            stale,
            BluetoothAction::SetMuted { muted: false }
        )
        .is_err());
    f.app.media_input(&a, MediaOperation::VolumeDown).unwrap();
    let down = f.app.bluetooth_view("a");
    assert_eq!(down.desired.volume.value(), 95);
    assert!(down.desired.muted);
    assert_eq!(f.app.bluetooth_view("b").desired.volume.value(), 0);
    f.app
        .bluetooth_command(
            "a",
            a.incarnation,
            down.revision,
            BluetoothAction::SetMuted { muted: false },
        )
        .unwrap();
    f.app.media_input(&a, MediaOperation::VolumeDown).unwrap();
    assert_eq!(f.app.audio_volume(&a), 90); // A latest unmute is not undone either.
    assert!(!f.app.bluetooth_view("a").desired.muted);
    for _ in 0..20 {
        f.app.media_input(&a, MediaOperation::VolumeDown).unwrap();
    }
    let lower = f.app.bluetooth_view("a");
    assert_eq!(lower.desired.volume.value(), 0);
    assert!(!lower.desired.muted);
    f.app.media_input(&a, MediaOperation::VolumeDown).unwrap();
    assert_eq!(f.app.bluetooth_view("a").revision, lower.revision);
    assert_eq!(f.app.audio_volume(&b), 0);
    let mut plugins = PluginSet::default();
    plugins
        .register(
            Box::new(LocalMusicPlugin),
            Box::new(LocalMusicContent::default()),
        )
        .unwrap();
    plugins
        .register(Box::new(ClockPlugin), Box::new(ClockContent::default()))
        .unwrap();
    let reopened = App::open_with_plugin_set(
        &f._temp.path().join("state.db"),
        &f._temp.path().join("plugins"),
        Arc::new(picomado_core::fetch::SystemClock),
        plugins,
    )
    .unwrap();
    assert_eq!(reopened.bluetooth_view("a").desired, lower.desired);
}

#[test]
fn media_channel_and_cancel_preserve_weather_sub_navigation() {
    struct Day;
    impl picomado_core::fetch::Clock for Day {
        fn unix_seconds(&self) -> i64 {
            1791792000
        }
        fn today_jst(&self) -> String {
            "2026-10-12".into()
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let mut plugins = PluginSet::default();
    plugins
        .register(
            Box::new(picomado_core::weather::WeatherPlugin),
            Box::new(picomado_core::weather::WeatherContent::default()),
        )
        .unwrap();
    let mut app = App::open_with_plugin_set(
        &temp.path().join("state.db"),
        &temp.path().join("plugins"),
        Arc::new(Day),
        plugins,
    )
    .unwrap();
    let t = app
        .hello("weather", ["lcd".into(), "media-input-v1".into()].into(), 1)
        .unwrap();
    app.publish_plugin_source(
        "weather",
        &t,
        1,
        include_bytes!("../assets/jma-tokyo-20261012.json"),
        "cached",
        "2026-10-12",
    )
    .unwrap();
    app.button(&t, ButtonAction::Sub).unwrap();
    assert_eq!(app.session_mode(&t), Some("sub"));
    let start = app.plugin_position(&t).unwrap().index;
    app.media_input(&t, MediaOperation::ChannelNext).unwrap();
    assert_eq!(app.plugin_position(&t).unwrap().index, start + 1);
    app.media_input(&t, MediaOperation::TogglePlayback).unwrap();
    assert_eq!(app.plugin_position(&t).unwrap().index, start + 1);
    app.media_input(&t, MediaOperation::ChannelPrevious)
        .unwrap();
    assert_eq!(app.plugin_position(&t).unwrap().index, start);
    app.media_input(&t, MediaOperation::Cancel).unwrap();
    assert_eq!(app.session_mode(&t), Some("normal"));
}

#[test]
fn audio_failure_stops_at_consumed_position_once_and_only_explicit_play_resumes() {
    use picomado_core::plugin_jobs::AudioPlaybackFailure;
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let one = f.hello("one"); let two = f.hello("two");
    f.command("one", "play"); f.command("two", "play");
    let first = f.app.audio_playback_for(&one).unwrap();
    let other = f.app.audio_playback_for(&two).unwrap();
    let before_revision = f.app.plugin_web_view(PLUGIN).unwrap().revision;
    wait_until(&f.app, first.2.started_mono_ms + 5);
    assert!(same_offer(&f.app, &one, &first), "elapsed time is not part of the offer fence");
    assert!(f.app.audio_playback_failed(&one, &first.0, &first.1, &first.2,
        6912, AudioPlaybackFailure::DeviceFailed).unwrap());
    let failed = f.state("one");
    assert_eq!(failed["playing"], false);
    assert_eq!(failed["position_ms"], 144);
    assert_eq!(failed["error"]["code"], "device_failed");
    assert!(failed["error"]["message"].as_str().unwrap().contains("失敗"));
    assert!(f.app.audio_playback_for(&one).is_none());
    assert!(same_offer(&f.app, &two, &other));
    assert_eq!(f.app.plugin_web_view(PLUGIN).unwrap().revision, before_revision + 1);
    assert!(!f.app.audio_playback_failed(&one, &first.0, &first.1, &first.2,
        6912, AudioPlaybackFailure::DeviceFailed).unwrap());
    assert_eq!(f.app.plugin_web_view(PLUGIN).unwrap().revision, before_revision + 1);
    wait_until(&f.app, f.app.monotonic_millis() + 5);
    assert_eq!(f.state("one"), failed, "failed position must not advance with time");
    f.command("one", "play");
    let resumed = f.app.audio_playback_for(&one).unwrap();
    assert_eq!(resumed.2.start_frame, 6912);
    assert!(resumed.2.generation > first.2.generation);
    assert!(f.state("one")["error"].is_null());
    assert!(!f.app.audio_playback_failed(&one, &first.0, &first.1, &first.2,
        8000, AudioPlaybackFailure::DeviceFailed).unwrap());
    assert!(same_offer(&f.app, &one, &resumed));
}

#[test]
fn audio_failure_rejects_other_device_new_track_session_and_display() {
    use picomado_core::plugin_jobs::AudioPlaybackFailure;
    let mut f = Fixture::new(SAMPLE_RATE as u64 * 60);
    let one = f.hello_with_caps("one", &["lcd", "media-input-v1"]);
    let two = f.hello("two");
    f.command("one", "play"); f.command("two", "play");
    let old = f.app.audio_playback_for(&one).unwrap();
    assert!(!f.app.audio_playback_failed(&two, &old.0, &old.1, &old.2, 128,
        AudioPlaybackFailure::DeviceFailed).unwrap());
    let mut bad_display = old.0.clone(); bad_display.display_epoch += 1;
    assert!(!f.app.audio_playback_failed(&one, &bad_display, &old.1, &old.2, 128,
        AudioPlaybackFailure::DeviceFailed).unwrap());
    let mut bad_generation = old.2.clone(); bad_generation.generation += 1;
    assert!(!f.app.audio_playback_failed(&one, &old.0, &old.1, &bad_generation, 128,
        AudioPlaybackFailure::DeviceFailed).unwrap());
    f.sub(&one);
    let replacement = f.app.audio_playback_for(&one).unwrap();
    assert_ne!(replacement.2.asset_id, old.2.asset_id);
    assert!(!f.app.audio_playback_failed(&one, &old.0, &old.1, &old.2, 128,
        AudioPlaybackFailure::DeviceFailed).unwrap());
    assert!(same_offer(&f.app, &one, &replacement));
    let new_session = f.hello("one"); f.command("one", "play");
    let latest = f.app.audio_playback_for(&new_session).unwrap();
    assert!(!f.app.audio_playback_failed(&one, &replacement.0, &replacement.1, &replacement.2, 128,
        AudioPlaybackFailure::DeviceFailed).unwrap());
    assert!(same_offer(&f.app, &new_session, &latest));
}

#[test]
fn failure_clamps_terminal_padding_and_stop_clears_device_error() {
    use picomado_core::plugin_jobs::AudioPlaybackFailure;
    let mut f = Fixture::new(1000);
    let session = f.hello("one"); f.command("one", "play");
    let offer = f.app.audio_playback_for(&session).unwrap();
    assert!(f.app.audio_playback_failed(&session, &offer.0, &offer.1, &offer.2, 1024,
        AudioPlaybackFailure::CreditTimeout).unwrap());
    assert_eq!(f.state("one")["position_ms"], 1000 * 1000 / SAMPLE_RATE);
    f.command("one", "stop");
    assert!(f.state("one")["error"].is_null());
    assert_eq!(f.state("one")["position_ms"], 0);
    assert!(f.app.audio_playback_for(&session).is_none());
}

#[test]
fn navigation_stopped_autoplays_but_pause_stays_paused_until_avrcp_toggle() {
    let mut f = Fixture::new(4800000);
    let a = f.hello_with_caps("a", &["lcd", "bluetooth-audio-v1", "media-input-v1"]);
    let b = f.hello_with_caps("b", &["lcd", "bluetooth-audio-v1", "media-input-v1"]);
    f.command("a", "play"); f.command("a", "stop");
    f.command("b", "play"); let other = f.app.audio_playback_for(&b).unwrap();
    f.app.media_input(&a, MediaOperation::ChannelNext).unwrap();
    assert!(f.app.audio_playback_for(&a).is_none());
    let rev = f.app.plugin_web_view(PLUGIN).unwrap().revision;
    f.app.media_input(&a, MediaOperation::ChannelPrevious).unwrap();
    assert_eq!(f.app.audio_playback_for(&a).unwrap().2.start_frame, 0);
    assert_eq!(f.app.plugin_web_view(PLUGIN).unwrap().revision, rev + 1);
    assert!(same_offer(&f.app, &b, &other));
    f.app.media_input(&a, MediaOperation::TogglePlayback).unwrap();
    let paused = f.state("a"); assert_eq!(paused["resting"], "paused");
    f.app.media_input(&a, MediaOperation::ChannelNext).unwrap();
    f.app.media_input(&a, MediaOperation::ChannelPrevious).unwrap();
    assert!(f.app.audio_playback_for(&a).is_none()); assert_eq!(f.state("a"), paused);
    f.app.media_input(&a, MediaOperation::TogglePlayback).unwrap();
    assert!(f.app.audio_playback_for(&a).is_some());
    assert!(same_offer(&f.app, &b, &other));
}

#[test]
fn navigation_failure_no_selection_stale_hello_and_mute_stay_safe() {
    use picomado_core::{application::BluetoothAction, plugin_jobs::AudioPlaybackFailure};
    let mut f = Fixture::new(4800000); let a = f.stopped("a");
    let rev = f.app.bluetooth_view("a").revision;
    f.app.bluetooth_command("a", a.incarnation, rev, BluetoothAction::SetMuted { muted: true }).unwrap();
    let bt = f.app.bluetooth_view("a");
    f.app.button(&a, ButtonAction::Up).unwrap(); f.app.button(&a, ButtonAction::Down).unwrap();
    assert_eq!(f.app.audio_volume(&a), 0);
    assert_eq!(f.app.bluetooth_view("a").desired, bt.desired);
    assert_eq!(f.app.bluetooth_view("a").revision, bt.revision);
    let spec = f.app.audio_playback_for(&a).unwrap();
    f.app.audio_playback_failed(&a, &spec.0, &spec.1, &spec.2, 256, AudioPlaybackFailure::DeviceFailed).unwrap();
    let failed = f.state("a");
    f.app.button(&a, ButtonAction::Up).unwrap(); f.app.button(&a, ButtonAction::Down).unwrap();
    assert!(f.app.audio_playback_for(&a).is_none()); assert_eq!(f.state("a"), failed);
    f.command("a", "stop");
    let new = f.hello("a"); assert!(f.app.audio_playback_for(&new).is_none());
    assert!(f.app.button(&a, ButtonAction::Up).is_err());
    assert!(f.app.audio_playback_for(&new).is_none());
    let b = f.hello("b");
    f.app.button(&b, ButtonAction::Up).unwrap(); f.app.button(&b, ButtonAction::Down).unwrap();
    assert!(f.state("b").is_null()); assert!(f.app.audio_playback_for(&b).is_none());
}

#[test]
fn navigation_standby_hello_does_not_autoplay_and_legacy_eof_restarts_on_entry() {
    let mut f = Fixture::new(48); let a = f.stopped("a");
    f.app.button(&a, ButtonAction::Power).unwrap();
    assert_eq!(f.app.session_mode(&a), Some("standby"));
    f.app.button(&a, ButtonAction::Power).unwrap();
    assert!(f.app.audio_playback_for(&a).is_none(), "Power in standby is an existing no-op");
    let a = f.hello("a");
    assert!(f.app.audio_playback_for(&a).is_none(), "Hello is not deliberate navigation");
    f.app.button(&a, ButtonAction::Up).unwrap(); f.app.button(&a, ButtonAction::Down).unwrap();
    let played = f.app.audio_playback_for(&a).unwrap();
    wait_until(&f.app, played.2.started_mono_ms + 2);
    assert_eq!(f.state("a")["playing"], false);
    f.app.button(&a, ButtonAction::Up).unwrap(); f.app.button(&a, ButtonAction::Down).unwrap();
    let replay = f.app.audio_playback_for(&a).unwrap();
    assert_eq!(replay.2.start_frame, 0); assert!(replay.2.generation > played.2.generation);
    f.command("a", "stop");
    f.app.terminal_event(Event::PluginInput { token: a.clone(), name: "toggle-playback".into() }).unwrap();
    assert!(f.app.audio_playback_for(&a).is_none(), "redraw event is not a navigation edge");
}
