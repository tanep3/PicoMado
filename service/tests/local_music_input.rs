//! MUSIC-PLAYER-20261107: exercise the real App input/aggregate boundary.
//! Metadata and a temporary database only: no decoder, speaker, socket or device.
use picomado_core::{
    application::{App, AppError, InputResult},
    clock::{ClockContent, ClockPlugin},
    content::{ButtonAction, ContentPlugin},
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
        let temp = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        let mut music = LocalMusicContent::default();
        // Use the same validated state loader as restart, without filesystem audio.
        music
            .load_state(json!({
                "tracks": {
                    &key: {"key":key,"name":"fixture.wav","frames":frames},
                    "b".repeat(64): {"key":"b".repeat(64),"name":"second.wav","frames":frames}
                },
                "devices": {}, "next_generation": 1
            }))
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
        let token = self.app.hello(id, ["lcd".into()].into(), 1).unwrap();
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
    assert_eq!(
        returned.2, first.2,
        "channel navigation does not skip tracks"
    );
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
