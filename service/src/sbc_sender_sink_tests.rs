use super::super::*;
use crate::application::{App,BluetoothAction};
use crate::content::ContentPlugin;
struct AudioFixture {
    _temp:tempfile::TempDir, app:SharedApp, session:SessionToken, frame:crate::FrameToken,
    paths:crate::plugin_jobs::PluginPaths, key:String,
    failures:std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
fn fixture(frames:u64,start:u64,rate:u32)->AudioFixture {
    let failures=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let temp = tempfile::tempdir().unwrap();
    let key = "a".repeat(64);
    let mut content = crate::local_music::LocalMusicContent::default();
    content.load_state(serde_json::json!({"tracks":{key.clone():{"key":key,"name":"fixture.wav","frames":frames}},"devices":{},"next_generation":1})).unwrap();
    let mut plugins = crate::plugins::PluginSet::default();
    plugins
        .register(
            Box::new(crate::local_music::LocalMusicPlugin),
            Box::new(TestAudioContent{inner:content,failures:failures.clone(),rate,start}),
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
                crate::audio_transport::SBC_CAPABILITY.into(),
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
        (0..frames * 2)
            .flat_map(|_| 10000i16.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let app = std::sync::Arc::new(crate::shared::Application::new(state));

    AudioFixture{_temp:temp,app,session,frame,paths,key,failures}
}
#[tokio::test]
async fn sbc_handshake_gates_source_and_eof_padding_is_bounded() {
    for (frames,start) in [(1u64,0u64),(127,0),(128,0),(129,0),(515,0),(1024,0),(1025,0),(2051,0),(515,7),(2051,19)] {
    let AudioFixture{_temp,app,session,frame,paths,key,..}=fixture(frames,start,48000);
    let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut sink=Box::pin(futures_util::sink::unfold(packets.clone(),|packets,message:Message|async move {
        if let Message::Binary(bytes)=message {packets.lock().unwrap().push(bytes.to_vec());}
        Ok::<_,std::convert::Infallible>(packets)
    }));
    let mut audio=SocketAudio::new(&session);let (_tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    assert!(packets.lock().unwrap().is_empty());
    app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
    // Prove WaitingReady never reads even an absent source, nor sends finite EOF.
    let source=paths.cache.join("audio").join(format!("{key}.pcm"));
    let pcm=std::fs::read(&source).unwrap();std::fs::remove_file(&source).unwrap();
    for _ in 0..3 {service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();}
    assert!(audio.sender.waiting_ready());assert!(audio.transfer.as_ref().unwrap().failure.is_none());
    assert_eq!(packets.lock().unwrap().len(),1);assert_eq!(packets.lock().unwrap()[0][5],28);
    let identity=audio.sender.identity().unwrap();
    let ready=crate::audio_transport::SbcReady{identity,first_frame:start,codec_epoch:7,rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32,capacity_frames:16384};
    let mut stale=ready;stale.identity.display_epoch+=1;audio.ready(stale);
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    assert_eq!(packets.lock().unwrap().len(),1);
    std::fs::write(&source,&pcm).unwrap();audio.ready(ready);
    for _ in 0..4 {
        let before=packets.lock().unwrap().iter().filter(|p|p[5]==30).count();
        service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
        let after=packets.lock().unwrap().iter().filter(|p|p[5]==30).count();assert!(after-before<=2);
    }
    let end=start+(frames-start).div_ceil(128)*128;assert!(end-frames<=127);
    assert_eq!(audio.transfer.as_ref().unwrap().final_frame,Some(end));assert_eq!(audio.sender.sent_end(),Some(end));
    let sent=packets.lock().unwrap();
    assert_eq!(sent.iter().filter(|p|p[5]==28).count(),1);assert_eq!(sent.iter().filter(|p|p[5]==26).count(),1);
    assert!(!sent.iter().any(|p|p[5]==22||p[5]==24));
    let mut encoder=crate::sbc_encoder::SbcEncoder::new(crate::sbc_encoder::SbcEncoderConfig{rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32}).unwrap();
    let mut offset=start;
    for packet in sent.iter().filter(|p|p[5]==30) {
        assert_eq!(u64::from_be_bytes(packet[38..46].try_into().unwrap()),offset);
        let count=(packet.len()-46)/77*128;assert!(count<=1024);
        assert_eq!(packet[46..],encoder.encode(&vec![0;count*4]).unwrap());offset+=count as u64;
    }
    assert_eq!(offset,end);assert_eq!(std::fs::read(&source).unwrap(),pcm);
    drop(sent);
    audio.credit(identity,end,end,false);
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    assert!(audio.transfer.as_ref().unwrap().drained);
    }
}

#[derive(Clone)]
struct TestAudioContent {
    inner:crate::local_music::LocalMusicContent,
    rate:u32,
    start:u64,
    failures:std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl ContentPlugin for TestAudioContent {
    fn clone_box(&self)->Box<dyn ContentPlugin>{Box::new(self.clone())}
    fn id(&self)->&'static str{"local-music"}
    fn publish(&mut self,c:&crate::content::PluginContext,s:&[u8],d:&str,t:&str)->Result<(),crate::content::ContentError>{self.inner.publish(c,s,d,t)}
    fn current_frame(&self,c:&crate::content::PluginContext)->Option<Vec<u8>>{self.inner.current_frame(c)}
    fn position(&self,c:&crate::content::PluginContext)->Option<crate::content::PluginPosition>{self.inner.position(c)}
    fn plan(&mut self,c:&crate::content::PluginContext,a:crate::content::ButtonAction)->Result<crate::content::PluginPlan,crate::content::ContentError>{self.inner.plan(c,a)}
    fn session_reset(&mut self,d:&str,i:u64){self.inner.session_reset(d,i)}
    fn delete_device(&mut self,d:&str){self.inner.delete_device(d)}
    fn web_state(&self)->serde_json::Value{self.inner.web_state()}
    fn web_state_at(&self,now:u64)->serde_json::Value{self.inner.web_state_at(now)}
    fn save_state(&self)->serde_json::Value{self.inner.save_state()}
    fn load_state(&mut self,v:serde_json::Value)->Result<(),crate::content::ContentError>{self.inner.load_state(v)}
    fn plan_command(&mut self,c:&crate::plugin_jobs::CommandContext,v:&serde_json::Value)->Result<crate::plugin_jobs::PluginWorkPlan,crate::content::ContentError>{self.inner.plan_command(c,v)}
    fn audio_playback(&self,c:&crate::content::PluginContext)->Option<crate::plugin_jobs::AudioPlaybackSpec>{let mut s=self.inner.audio_playback(c)?;s.sample_rate=self.rate;s.start_frame=self.start;Some(s)}
    fn audio_failed(&mut self,ctx:&crate::content::PluginContext,spec:&crate::plugin_jobs::AudioPlaybackSpec,consumed:u64,reason:crate::plugin_jobs::AudioPlaybackFailure)->Result<Option<crate::plugin_jobs::PluginWorkPlan>,crate::content::ContentError>{
        self.failures.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        if self.rate==48000 {self.inner.audio_failed(ctx,spec,consumed,reason)} else {
            assert_eq!(reason,crate::plugin_jobs::AudioPlaybackFailure::UnsupportedFormat);Ok(None)
        }
    }
}
#[tokio::test]
async fn invalid_source_reports_once_without_start_read_or_eof_and_rejects_stale_failure() {
    let AudioFixture{_temp,app,session,frame,paths,key,failures}=fixture(48000,0,44100);
    assert!(app.read().unwrap().audio_playback_for(&session).is_none());
    let (display,plugin,spec,_)=app.read().unwrap().audio_playback_offer(&session).unwrap();
    assert!(!spec.valid());
    let mut stale=spec.clone();stale.sample_rate=22050;
    assert!(!app.lock().unwrap().audio_playback_failed(&session,&display,&plugin,&stale,0,AudioPlaybackFailure::UnsupportedFormat).unwrap());
    assert!(!app.lock().unwrap().audio_playback_failed(&session,&display,&plugin,&spec,0,AudioPlaybackFailure::DeviceFailed).unwrap());
    let mut stale=display.clone();stale.display_epoch+=1;
    assert!(!app.lock().unwrap().audio_playback_failed(&session,&stale,&plugin,&spec,0,AudioPlaybackFailure::UnsupportedFormat).unwrap());
    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst),0);
    app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
    std::fs::remove_file(paths.cache.join("audio").join(format!("{key}.pcm"))).unwrap();
    let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut sink=Box::pin(futures_util::sink::unfold(packets.clone(),|packets,message:Message|async move {
        if let Message::Binary(bytes)=message{packets.lock().unwrap().push(bytes.to_vec());}
        Ok::<_,std::convert::Infallible>(packets)
    }));
    let mut audio=SocketAudio::new(&session);let (_tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
    for _ in 0..3 {service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();}
    assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst),1);
    assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::UnsupportedFormat));
    assert!(audio.transfer.as_ref().unwrap().encoder.is_none());assert_eq!(audio.sender.sent_end(),Some(0));
    assert!(packets.lock().unwrap().iter().all(|p|p[5]==23));
}

fn ready_for(audio:&SocketAudio)->crate::audio_transport::SbcReady {
    crate::audio_transport::SbcReady{identity:audio.sender.identity().unwrap(),first_frame:audio.transfer.as_ref().unwrap().spec.start_frame,codec_epoch:7,rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32,capacity_frames:16384}
}
fn receipt_current(app:&SharedApp){
    let mut state=app.lock().unwrap();
    let frame=state.pending_effects().into_iter().find_map(|(_,e)|if let crate::Effect::SendFrame{token,..}=e {state.frame_is_current(&token).then_some(token)}else{None}).unwrap();
    state.terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
}
#[tokio::test]
async fn sbc_volume_history_credit_bound_and_pause_new_generation_do_not_replay_old_audio() {
    let AudioFixture{_temp,app,session,frame,paths,key,..}=fixture(48000,0,48000);
    app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
    let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut sink=Box::pin(futures_util::sink::unfold(packets.clone(),|packets,message:Message|async move {
        if let Message::Binary(bytes)=message {packets.lock().unwrap().push(bytes.to_vec());}Ok::<_,std::convert::Infallible>(packets)
    }));
    let mut audio=SocketAudio::new(&session);let (_tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    let ready=ready_for(&audio);audio.ready(ready);
    let mut expected=crate::sbc_encoder::SbcEncoder::new(crate::sbc_encoder::SbcEncoderConfig{rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32}).unwrap();
    for (turn,value) in [0i16,100,0].into_iter().enumerate() {
        if turn==1 {
            app.lock().unwrap().bluetooth_command("audio",session.incarnation,4,BluetoothAction::SetVolume{volume:crate::bluetooth::Volume::try_from(10).unwrap()}).unwrap();
            app.lock().unwrap().bluetooth_command("audio",session.incarnation,5,BluetoothAction::SetMuted{muted:false}).unwrap();
        } else if turn==2 {app.lock().unwrap().bluetooth_command("audio",session.incarnation,6,BluetoothAction::SetMuted{muted:true}).unwrap();}
        let before=packets.lock().unwrap().len();
        service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
        let sent=packets.lock().unwrap();let chunks:Vec<_>=sent[before..].iter().filter(|p|p[5]==30).collect();assert_eq!(chunks.len(),2);
        for packet in chunks {assert_eq!(packet[46..],expected.encode(&value.to_le_bytes().repeat(2048)).unwrap());}
        assert_eq!(audio.sender.identity(),Some(ready.identity));
    }
    let before_fill=packets.lock().unwrap().clone();
    for _ in 0..8 {service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();}
    assert_eq!(audio.sender.sent_end().unwrap()-audio.sender.consumed_end().unwrap(),16384);
    assert_eq!(audio.sender.allowance(),0);assert_eq!(&packets.lock().unwrap()[..before_fill.len()],before_fill.as_slice());
    // Queued old gain is bounded to 16384/48000 seconds on this transport;
    // gain does not retroactively mutate the packets or reset the stream.
    assert_eq!(audio.sender.capacity(),16384);
    let revision=app.read().unwrap().plugin_web_view("local-music").unwrap().revision;
    app.lock().unwrap().plugin_command("local-music",revision,serde_json::json!({"kind":"pause","device_id":"audio"})).unwrap();
    let stopped_at=packets.lock().unwrap().len();
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    assert!(audio.transfer.is_none());assert!(audio.sender.identity().is_none());audio.ready(ready);
    assert!(audio.sender.identity().is_none());
    let revision=app.read().unwrap().plugin_web_view("local-music").unwrap().revision;
    app.lock().unwrap().plugin_command("local-music",revision,serde_json::json!({"kind":"play","device_id":"audio","key":key})).unwrap();
    receipt_current(&app);
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    let fresh=ready_for(&audio);assert_ne!(fresh.identity,ready.identity);assert!(audio.sender.waiting_ready());
    audio.ready(ready);assert!(audio.sender.waiting_ready());
    assert!(!packets.lock().unwrap()[stopped_at..].iter().any(|p|p[5]==30));
    audio.ready(fresh);service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    let sent=packets.lock().unwrap();let chunk=sent[stopped_at..].iter().find(|p|p[5]==30).unwrap();
    let mut pristine=crate::sbc_encoder::SbcEncoder::new(crate::sbc_encoder::SbcEncoderConfig{rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32}).unwrap();
    assert_eq!(chunk[46..],pristine.encode(&vec![0;4096]).unwrap());drop(sent);
    assert!(std::fs::read(paths.cache.join("audio").join(format!("{key}.pcm"))).unwrap().chunks_exact(2).all(|b|i16::from_le_bytes([b[0],b[1]])==10000));
    let next=app.lock().unwrap().hello("audio",["lcd".into(),crate::protocol::BLUETOOTH_AUDIO_CAPABILITY.into(),crate::audio_transport::SBC_CAPABILITY.into()].into(),1).unwrap();
    let before=packets.lock().unwrap().len();
    service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
    assert!(audio.transfer.is_none());let mut rebound=SocketAudio::new(&next);rebound.ready(fresh);assert!(rebound.transfer.is_none());
    assert!(!packets.lock().unwrap()[before..].iter().any(|p|p[5]==30));
}
#[tokio::test]
async fn ready_tuple_failure_reaches_current_music_ui_once_without_fallback() {
    for mismatch in 0..3 {
        let AudioFixture{_temp,app,session,frame,failures,..}=fixture(48000,0,48000);
        app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
        let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut sink=Box::pin(futures_util::sink::unfold(packets.clone(),|packets,message:Message|async move {if let Message::Binary(bytes)=message{packets.lock().unwrap().push(bytes.to_vec());}Ok::<_,std::convert::Infallible>(packets)}));
        let mut audio=SocketAudio::new(&session);let (_tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
        service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
        let good=ready_for(&audio);let mut bad=good;
        match mismatch {0=>bad.rate=44100,1=>bad.codec_epoch=0,_=>bad.capacity_frames=4096}
        audio.ready(bad);audio.ready(good);
        for _ in 0..3 {service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();}
        assert_eq!(failures.load(std::sync::atomic::Ordering::SeqCst),1);
        let view=app.read().unwrap().plugin_web_view("local-music").unwrap();
        assert_eq!(view.data["devices"]["audio"]["playing"],false);
        assert_eq!(view.data["devices"]["audio"]["position_ms"],0);
        assert_eq!(view.data["devices"]["audio"]["error"]["code"],"codec_mismatch");
        let sent=packets.lock().unwrap();assert_eq!(sent.iter().filter(|p|p[5]==28).count(),1);assert_eq!(sent.iter().filter(|p|p[5]==23).count(),1);
        assert!(!sent.iter().any(|p|matches!(p[5],22|24|26|30)));
    }
}

#[tokio::test]
async fn arbitrary_partial_pcm_reads_survive_ticks_and_empty_shortage_without_padding() {
    for pieces in [vec![64usize,0,64],vec![1,127,1,127,17]] {
        let start=7;let frames=start+pieces.iter().sum::<usize>() as u64;
        let AudioFixture{_temp,app,session,frame,..}=fixture(frames,start,48000);
        app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
        let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut sink=Box::pin(futures_util::sink::unfold(packets.clone(),|packets,message:Message|async move {if let Message::Binary(bytes)=message{packets.lock().unwrap().push(bytes.to_vec());}Ok::<_,std::convert::Infallible>(packets)}));
        let mut audio=SocketAudio::new(&session);let (_tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
        let mut reads=Vec::new();let mut pieces=std::collections::VecDeque::from(pieces);
        let mut reader=|_:String,_:crate::plugin_jobs::AudioPlaybackSpec,offset:u64,max:u32,_:crate::plugin_jobs::PluginPaths| {
            let count=pieces.pop_front().expect("no read after EOF");assert!(count<=max as usize);
            reads.push((offset,count));std::future::ready(Ok(vec![0;count*4]))
        };
        service_audio_with_reader(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending,&mut reader).await.unwrap();
        assert!(audio.sender.waiting_ready());let ready=ready_for(&audio);audio.ready(ready);
        let mut expected_read=start;
        for _ in 0..8 {
            service_audio_with_reader(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending,&mut reader).await.unwrap();
            assert!(audio.transfer.as_ref().unwrap().failure.is_none());
            if audio.transfer.as_ref().unwrap().final_frame.is_some(){break;}
            assert!(audio.transfer.as_ref().unwrap().pending_pcm.len()<512);
        }
        drop(reader);
        for (offset,count) in reads {assert_eq!(offset,expected_read);expected_read+=count as u64;}
        assert_eq!(expected_read,frames);assert!(pieces.is_empty());
        let end=start+(frames-start).div_ceil(128)*128;
        assert_eq!(audio.sender.sent_end(),Some(end));assert_eq!(audio.transfer.as_ref().unwrap().final_frame,Some(end));
        assert!(audio.transfer.as_ref().unwrap().pending_pcm.is_empty());
        let sent=packets.lock().unwrap();assert_eq!(sent.iter().filter(|p|p[5]==26).count(),1);
        let mut next=start;
        let mut expected=crate::sbc_encoder::SbcEncoder::new(crate::sbc_encoder::SbcEncoderConfig{rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32}).unwrap();
        for packet in sent.iter().filter(|p|p[5]==30) {
            assert_eq!(u64::from_be_bytes(packet[38..46].try_into().unwrap()),next);
            let count=(packet.len()-46)/77*128;next+=count as u64;
            assert_eq!(packet[46..],expected.encode(&vec![0;count*4]).unwrap());
        }
        assert_eq!(next,end);
    }
}

#[tokio::test]
async fn blocked_finish_rechecks_cancel_failure_ready_and_channel_close_but_preserves_control_yield() {
    for scenario in 0..5 {
        let AudioFixture{_temp,app,session,frame,..}=fixture(127,0,48000);
        app.lock().unwrap().terminal_event(crate::Event::ReceiptAck{token:frame}).unwrap();
        app.lock().unwrap().bluetooth_command("audio",session.incarnation,4,BluetoothAction::SetVolume{volume:crate::bluetooth::Volume::try_from(10).unwrap()}).unwrap();
        app.lock().unwrap().bluetooth_command("audio",session.incarnation,5,BluetoothAction::SetMuted{muted:false}).unwrap();
        let packets=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let entered=std::sync::Arc::new(tokio::sync::Notify::new());
        let release=std::sync::Arc::new(tokio::sync::Notify::new());
        let mut sink=Box::pin(futures_util::sink::unfold((packets.clone(),entered.clone(),release.clone()),|(packets,entered,release),message:Message|async move {
            if let Message::Binary(bytes)=message {
                let finish=bytes.get(5)==Some(&26);packets.lock().unwrap().push(bytes.to_vec());
                if finish {entered.notify_one();release.notified().await;}
            }
            Ok::<_,std::convert::Infallible>((packets,entered,release))
        }));
        let mut audio=SocketAudio::new(&session);let (tx,mut rx)=tokio::sync::mpsc::channel(8);let mut tx=Some(tx);let mut pending=None;
        service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending).await.unwrap();
        let ready=ready_for(&audio);audio.ready(ready);
        tokio::time::timeout(Duration::from_secs(5),async {
            let control=async {
                entered.notified().await;
                let command=match scenario {
                    0=>Some(WriterCommand::Stop),
                    1=>Some(WriterCommand::AudioCredit(ready.identity,0,0,true)),
                    2=>{let mut conflict=ready;conflict.bitpool+=1;Some(WriterCommand::AudioReady(conflict))},
                    3=>None,
                    _=>Some(WriterCommand::reply(WireMessage::Error{code:"test_control".into()})),
                };
                if let Some(command)=command {tx.as_ref().unwrap().send(command).await.unwrap();} else {drop(tx.take());}
                release.notify_one();
            };
            let (result,())=tokio::join!(service_audio(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending),control);
            result.unwrap();
        }).await.expect("blocked-Finish control fixture timed out");
        assert!(!packets.lock().unwrap().iter().any(|p|p[5]==30));
        assert_eq!(audio.sender.sent_end(),Some(0));
        match scenario {
            0|3=>assert!(matches!(pending,Some(WriterCommand::Stop))),
            1=>assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::DeviceFailed)),
            2=>assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::CodecMismatch)),
            _=>{
                assert!(matches!(pending.take(),Some(WriterCommand::Reply(_))));
                assert_eq!(audio.transfer.as_ref().unwrap().pending_pcm.len(),127*4);
                assert_eq!(audio.transfer.as_ref().unwrap().final_frame,Some(128));
                service_audio_with_reader(&mut sink,&app,&session,&mut audio,&mut rx,&mut pending,
                    |_,_,_,_,_|->std::future::Ready<Result<Vec<u8>,crate::plugin_jobs::JobFailure>>{panic!("pending final PCM must not be reread")}).await.unwrap();
                assert_eq!(audio.sender.sent_end(),Some(128));
                assert!(audio.transfer.as_ref().unwrap().pending_pcm.is_empty());
                let sent=packets.lock().unwrap();assert_eq!(sent.iter().filter(|p|p[5]==26).count(),1);assert_eq!(sent.iter().filter(|p|p[5]==30).count(),1);
                let mut pristine=crate::sbc_encoder::SbcEncoder::new(crate::sbc_encoder::SbcEncoderConfig{rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32}).unwrap();
                let mut raw=vec![0;128*4];
                for sample in raw[..127*4].chunks_exact_mut(2){sample.copy_from_slice(&100i16.to_le_bytes());}
                let expected=pristine.encode(&raw).unwrap();
                assert_eq!(sent.iter().find(|p|p[5]==30).unwrap()[46..],expected);
                assert_ne!(pristine.encode(&raw).unwrap(),expected,"fixture must distinguish a double history advance");
            },
        }
    }
}
