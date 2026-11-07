//! Silent wire-to-App checks for MUSIC-PLAYER-20261107.
use super::*;
use crate::content::{ContentPlugin, MediaOperation};
use serde_json::json;

fn setup() -> (tempfile::TempDir, SharedApp) {
    let temp = tempfile::tempdir().unwrap();
    let mut content = crate::local_music::LocalMusicContent::default();
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    content.load_state(json!({
        "tracks": {&a:{"key":a,"name":"one.wav","frames":4800000}, &b:{"key":b,"name":"two.wav","frames":4800000}},
        "devices":{},"next_generation":1
    })).unwrap();
    let mut set = crate::plugins::PluginSet::default();
    set.register(
        Box::new(crate::local_music::LocalMusicPlugin),
        Box::new(content),
    )
    .unwrap();
    set.register(
        Box::new(crate::clock::ClockPlugin),
        Box::new(crate::clock::ClockContent::default()),
    )
    .unwrap();
    let app = crate::application::App::open_with_plugin_set(
        &temp.path().join("state.db"),
        &temp.path().join("plugins"),
        std::sync::Arc::new(crate::fetch::SystemClock),
        set,
    )
    .unwrap();
    (
        temp,
        std::sync::Arc::new(crate::shared::Application::new(app)),
    )
}
fn submit(
    app: &SharedApp,
    current: &mut Option<SessionToken>,
    message: WireMessage,
) -> Result<Option<WireMessage>, &'static str> {
    let bytes = encode(&message).unwrap();
    let decoded = Decoder::default().push(&bytes).unwrap().remove(0);
    process(app, current, decoded, &mut AckDiagnostics::default())
}
fn hello(app: &SharedApp, id: &str, media: bool) -> Option<SessionToken> {
    let mut current = None;
    let mut caps = vec!["lcd".into(), "bluetooth-audio-v1".into()];
    if media {
        caps.push("media-input-v1".into());
    }
    let welcome = submit(
        app,
        &mut current,
        WireMessage::Hello {
            device_id: id.into(),
            capabilities: caps,
            screen_owner_epoch: 1,
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        matches!(welcome, WireMessage::Welcome { media_input_version, .. } if media_input_version == media.then_some(1))
    );
    current
}
fn media(token: &SessionToken, operation: MediaOperation) -> WireMessage {
    WireMessage::MediaInput {
        incarnation: token.incarnation,
        session_epoch: token.session_epoch,
        operation,
    }
}
#[test]
fn media_wire_roundtrip_unknown_operation_and_legacy_welcome() {
    for operation in [
        MediaOperation::TogglePlayback,
        MediaOperation::Cancel,
        MediaOperation::ChannelNext,
        MediaOperation::ChannelPrevious,
        MediaOperation::VolumeUp,
        MediaOperation::VolumeDown,
    ] {
        let message = WireMessage::MediaInput {
            incarnation: 2,
            session_epoch: 3,
            operation,
        };
        let bytes = encode(&message).unwrap();
        assert_eq!(bytes[5], 27);
        assert_eq!(
            Decoder::default().push(&bytes).unwrap(),
            vec![message.clone()]
        );
        #[cfg(feature = "ack-path-probe")]
        assert_eq!(crate::ack_probe::identity(&message), (3, 0, 27));
    }
    let old: WireMessage =
        serde_json::from_str(r#"{"type":"welcome","data":{"incarnation":1,"session_epoch":2}}"#)
            .unwrap();
    assert!(matches!(
        old,
        WireMessage::Welcome {
            media_input_version: None,
            ..
        }
    ));
    let payload =
        br#"{"type":"media_input","data":{"incarnation":2,"session_epoch":3,"operation":"play"}}"#;
    let mut bytes = ((payload.len() + 2) as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(&[1, 27]);
    bytes.extend_from_slice(payload);
    assert_eq!(
        Decoder::default().push(&bytes),
        Err(crate::protocol::ProtocolError::InvalidJson)
    );
}
#[test]
fn negotiated_media_wire_preserves_other_device_and_ignores_other_channel() {
    let (_temp, app) = setup();
    let mut a = hello(&app, "a", true);
    let mut b = hello(&app, "b", true);
    let at = a.clone().unwrap();
    let bt = b.clone().unwrap();
    for (current, token) in [(&mut a, &at), (&mut b, &bt)] {
        submit(
            &app,
            current,
            WireMessage::EnterSub {
                incarnation: token.incarnation,
                session_epoch: token.session_epoch,
            },
        )
        .unwrap();
    }

    submit(&app, &mut a, media(&at, MediaOperation::TogglePlayback)).unwrap();
    submit(&app, &mut b, media(&bt, MediaOperation::TogglePlayback)).unwrap();
    let bspec = app.read().unwrap().audio_playback_for(&bt).unwrap();
    submit(&app, &mut a, media(&at, MediaOperation::TogglePlayback)).unwrap();
    assert!(app.read().unwrap().audio_playback_for(&at).is_none());
    let still = app.read().unwrap().audio_playback_for(&bt).unwrap();
    assert_eq!((still.0, still.2), (bspec.0, bspec.2));
    submit(
        &app,
        &mut a,
        WireMessage::EnterSub {
            incarnation: at.incarnation,
            session_epoch: at.session_epoch,
        },
    )
    .unwrap();
    assert_eq!(
        app.read()
            .unwrap()
            .plugin_web_view("local-music")
            .unwrap()
            .data["devices"]["a"]["key"],
        "b".repeat(64)
    );
    assert!(app.read().unwrap().audio_playback_for(&at).is_none());
    submit(
        &app,
        &mut a,
        WireMessage::Down {
            incarnation: at.incarnation,
            session_epoch: at.session_epoch,
        },
    )
    .unwrap();
    let selection = app.read().unwrap().selection_for(&at);
    assert_eq!(selection.as_ref().unwrap().0.as_deref(), Some("clock"));
    submit(&app, &mut a, media(&at, MediaOperation::TogglePlayback)).unwrap();
    assert_eq!(app.read().unwrap().selection_for(&at), selection);
}
#[test]
fn legacy_bt_sub_stays_toggle_and_unnegotiated_or_stale_media_is_rejected() {
    let (_temp, app) = setup();
    let mut legacy = hello(&app, "legacy", false);
    let t = legacy.clone().unwrap();
    assert_eq!(
        submit(&app, &mut legacy, media(&t, MediaOperation::TogglePlayback)),
        Err("invalid_media_input")
    );
    {
        let mut app = app.lock().unwrap();
        let r = app.plugin_web_view("local-music").unwrap().revision;
        app.plugin_command(
            "local-music",
            r,
            json!({"kind":"play","device_id":"legacy","key":"a".repeat(64)}),
        )
        .unwrap();
    }
    let sub = WireMessage::EnterSub {
        incarnation: t.incarnation,
        session_epoch: t.session_epoch,
    };
    submit(&app, &mut legacy, sub.clone()).unwrap();
    assert!(app.read().unwrap().audio_playback_for(&t).is_none());
    submit(&app, &mut legacy, sub).unwrap();
    assert_eq!(
        app.read()
            .unwrap()
            .audio_playback_for(&t)
            .unwrap()
            .2
            .asset_id,
        "a".repeat(64)
    );
    let mut modern = hello(&app, "modern", true);
    let old = modern.clone().unwrap();
    assert_eq!(
        submit(
            &app,
            &mut modern,
            WireMessage::MediaInput {
                incarnation: old.incarnation + 1,
                session_epoch: old.session_epoch,
                operation: MediaOperation::TogglePlayback
            }
        ),
        Err("stale_session")
    );
    let _new = hello(&app, "modern", true);
    assert_eq!(
        submit(
            &app,
            &mut modern,
            media(&old, MediaOperation::TogglePlayback)
        ),
        Err("stale_session")
    );
}
