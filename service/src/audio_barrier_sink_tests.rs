use super::super::*;
use crate::application::{App, BluetoothAction};
use crate::content::ContentPlugin;

#[tokio::test]
async fn ready_audio_tick_waits_for_receipt_then_starts_once_without_render() {
    let temp = tempfile::tempdir().unwrap();
    let key = "a".repeat(64);
    let mut content = crate::local_music::LocalMusicContent::default();
    content.load_state(serde_json::json!({"tracks":{key.clone():{"key":key,"name":"fixture.wav","frames":48000}},"devices":{},"next_generation":1})).unwrap();
    let mut plugins = crate::plugins::PluginSet::default();
    plugins
        .register(
            Box::new(crate::local_music::LocalMusicPlugin),
            Box::new(content),
        )
        .unwrap();
    let mut state = App::open_with_plugin_set(
        &temp.path().join("state.sqlite"),
        temp.path(),
        std::sync::Arc::new(crate::fetch::SystemClock),
        plugins,
    )
    .unwrap();
    let session = state
        .hello(
            "audio",
            [
                "lcd".into(),
                crate::protocol::BLUETOOTH_AUDIO_CAPABILITY.into(),
            ]
            .into(),
            1,
        )
        .unwrap();
    let old = state
        .pending_effects()
        .into_iter()
        .find_map(|(_, e)| {
            if let crate::Effect::SendFrame { token, .. } = e {
                Some(token)
            } else {
                None
            }
        })
        .unwrap();
    state
        .bluetooth_command(
            "audio",
            session.incarnation,
            1,
            BluetoothAction::SetEnabled { enabled: true },
        )
        .unwrap();
    let scan = state
        .bluetooth_command(
            "audio",
            session.incarnation,
            2,
            BluetoothAction::Discover {},
        )
        .unwrap()
        .pending
        .unwrap();
    let address =
        crate::bluetooth::BluetoothAddress::try_from("00:11:22:33:44:55".to_owned()).unwrap();
    state
        .apply_bluetooth_report(
            &session,
            scan,
            crate::bluetooth::Report::Candidate {
                candidate: crate::bluetooth::Candidate {
                    address: address.clone(),
                    name: "fixture".into(),
                    a2dp: Some(true),
                },
            },
        )
        .unwrap();
    let connect = state
        .bluetooth_command(
            "audio",
            session.incarnation,
            3,
            BluetoothAction::Pair {
                discovery: scan,
                target: address,
            },
        )
        .unwrap()
        .pending
        .unwrap();
    state
        .apply_bluetooth_report(
            &session,
            connect,
            crate::bluetooth::Report::Complete { success: true },
        )
        .unwrap();
    state
        .plugin_command(
            "local-music",
            1,
            serde_json::json!({"kind":"play","device_id":"audio","key":key}),
        )
        .unwrap();
    let frame = state
        .pending_effects()
        .into_iter()
        .find_map(|(_, e)| {
            if let crate::Effect::SendFrame { token, .. } = e {
                state.frame_is_current(&token).then_some(token)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(frame.display_epoch, old.display_epoch);
    let paths = state.plugin_paths("local-music").unwrap();
    std::fs::create_dir_all(paths.cache.join("audio")).unwrap();
    std::fs::write(
        paths.cache.join("audio").join(format!("{key}.pcm")),
        (0..48000 * 2)
            .flat_map(|_| 10000i16.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let app = std::sync::Arc::new(crate::shared::Application::new(state));
    let packets = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = packets.clone();
    let mut sink = Box::pin(futures_util::sink::unfold(
        recorded,
        |recorded, message: Message| async move {
            if let Message::Binary(bytes) = message {
                recorded.lock().unwrap().push(bytes.to_vec());
            }
            Ok::<_, std::convert::Infallible>(recorded)
        },
    ));
    let mut audio = SocketAudio::new(&session);
    let (_tx, mut rx) = tokio::sync::mpsc::channel(8);
    let mut pending = None;
    for _ in 0..3 {
        service_audio(&mut sink, &app, &session, &mut audio, &mut rx, &mut pending)
            .await
            .unwrap();
    }
    assert!(
        packets.lock().unwrap().is_empty(),
        "tick before FrameStart/receipt must send no audio"
    );
    assert!(!app
        .lock()
        .unwrap()
        .terminal_event(crate::Event::ReceiptAck { token: old })
        .unwrap());
    service_audio(&mut sink, &app, &session, &mut audio, &mut rx, &mut pending)
        .await
        .unwrap();
    assert!(
        packets.lock().unwrap().is_empty(),
        "old receipt cannot release start"
    );
    assert!(app
        .lock()
        .unwrap()
        .terminal_event(crate::Event::ReceiptAck { token: frame })
        .unwrap());
    assert!(app.read().unwrap().view().devices[0]
        .last_rendered_frame
        .is_none());
    for _ in 0..3 {
        service_audio(&mut sink, &app, &session, &mut audio, &mut rx, &mut pending)
            .await
            .unwrap();
    }
    let first_end = packets.lock().unwrap().len();
    let identity = audio.sender.identity().unwrap();
    {
        let sent = packets.lock().unwrap();
        assert_eq!(sent.iter().filter(|p| p.get(5) == Some(&22)).count(), 1);
        let pcm: Vec<_> = sent.iter().filter(|p| p.get(5) == Some(&24)).collect();
        assert!(!pcm.is_empty(), "PCM requires no RenderedAck");
        assert!(
            pcm.iter().all(|p| p[46..].iter().all(|b| *b == 0)),
            "default volume must be exact mute"
        );
        assert_eq!(sent[0][5], 22);
    }
    app.lock()
        .unwrap()
        .bluetooth_command(
            "audio",
            session.incarnation,
            4,
            BluetoothAction::SetVolume {
                volume: crate::bluetooth::Volume::try_from(10).unwrap(),
            },
        )
        .unwrap();
    assert_eq!(app.read().unwrap().audio_volume(&session), 0);
    app.lock().unwrap().bluetooth_command("audio",session.incarnation,5,
        BluetoothAction::SetMuted { muted:false }).unwrap();
    service_audio(&mut sink, &app, &session, &mut audio, &mut rx, &mut pending)
        .await
        .unwrap();
    let second_end = packets.lock().unwrap().len();
    {
        let sent = packets.lock().unwrap();
        let pcm: Vec<_> = sent[first_end..]
            .iter()
            .filter(|p| p.get(5) == Some(&24))
            .collect();
        assert!(!pcm.is_empty());
        assert!(pcm.iter().all(|p| p[46..]
            .chunks_exact(2)
            .all(|b| i16::from_le_bytes([b[0], b[1]]) == 100)));
        assert_eq!(sent.iter().filter(|p| p.get(5) == Some(&22)).count(), 1);
    }
    assert_eq!(audio.sender.identity(), Some(identity));
    let end = audio.sender.sent_end().unwrap();
    audio.credit(identity, end, end, false);
    app.lock()
        .unwrap()
        .bluetooth_command(
            "audio",
            session.incarnation,
            6,
            BluetoothAction::SetMuted { muted:true },
        )
        .unwrap();
    service_audio(&mut sink, &app, &session, &mut audio, &mut rx, &mut pending)
        .await
        .unwrap();
    assert_eq!(app.read().unwrap().bluetooth_view("audio").desired.volume.value(),10);
    let sent = packets.lock().unwrap();
    let pcm: Vec<_> = sent[second_end..]
        .iter()
        .filter(|p| p.get(5) == Some(&24))
        .collect();
    assert!(!pcm.is_empty());
    assert!(pcm.iter().all(|p| p[46..].iter().all(|b| *b == 0)));
    assert_eq!(sent.iter().filter(|p| p.get(5) == Some(&22)).count(), 1);
    assert_eq!(audio.sender.identity(), Some(identity));
    assert_eq!(
        std::fs::read(paths.cache.join("audio").join(format!("{key}.pcm"))).unwrap()[..4],
        [0x10, 0x27, 0x10, 0x27]
    );
    let muted_end=sent.len();
    drop(sent);
    let end=audio.sender.sent_end().unwrap();audio.credit(identity,end,end,false);
    app.lock().unwrap().bluetooth_command("audio",session.incarnation,7,
        BluetoothAction::SetMuted { muted:false }).unwrap();
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    let sent=packets.lock().unwrap();
    let pcm:Vec<_>=sent[muted_end..].iter().filter(|p|p.get(5)==Some(&24)).collect();
    assert!(!pcm.is_empty());
    assert!(pcm.iter().all(|p|p[46..].chunks_exact(2).all(|b|i16::from_le_bytes([b[0],b[1]])==100)));
    assert_eq!(sent.iter().filter(|p|p.get(5)==Some(&22)).count(),1);
    assert_eq!(audio.sender.identity(),Some(identity));

}
