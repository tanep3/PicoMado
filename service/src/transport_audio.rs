use crate::plugin_jobs::AudioPlaybackFailure;

// BT-AUDIO-20261107. The socket writer owns each bounded audio session.
struct AudioTransfer {
    display: crate::DisplayToken,
    plugin: String,
    spec: crate::plugin_jobs::AudioPlaybackSpec,
    paths: crate::plugin_jobs::PluginPaths,
    identity: crate::audio_transport::StreamIdentity,
    failure: Option<AudioPlaybackFailure>,
    failure_reported: bool,
    stopped: bool,
    drained: bool,
    final_frame: Option<u64>,
    last_credit: tokio::time::Instant,
}
struct SocketAudio {
    sender: crate::audio_transport::AudioSender,
    transfer: Option<AudioTransfer>,
    waiting_display: Option<crate::DisplayToken>,
}
impl SocketAudio {
    fn new(t:&SessionToken)->Self {Self{sender:crate::audio_transport::AudioSender::new(t.incarnation,t.session_epoch),transfer:None,waiting_display:None}}
    fn credit(&mut self,id:crate::audio_transport::StreamIdentity,accepted:u64,consumed:u64,failed:bool){
        let Some(t)=self.transfer.as_mut().filter(|t|t.identity==id) else{return};
        Self::apply_credit(&mut self.sender,t,id,accepted,consumed,failed);
    }
    fn apply_credit(sender:&mut crate::audio_transport::AudioSender,t:&mut AudioTransfer,id:crate::audio_transport::StreamIdentity,accepted:u64,consumed:u64,failed:bool){
        if t.identity!=id || t.failure.is_some(){return;}
        if sender.accepted_end()==Some(t.spec.start_frame) && accepted>t.spec.start_frame {
            eprintln!("picomado audio_first_credit stream={} accepted={} consumed={}",id.stream_id,accepted,consumed);
        }
        // Terminal status carries useful final credit too. Validate before adoption;
        // invalid/reordered snapshots must not move the last confirmed position.
        match sender.apply_credit(id,accepted,consumed) {
            Ok(true)=>t.last_credit=tokio::time::Instant::now(),
            Ok(false)=>{},
            Err(error)=>{
                eprintln!("picomado audio_credit_invalid stream={} sent={:?} accepted={} consumed={} error={:?}",id.stream_id,sender.sent_end(),accepted,consumed,error);
                t.failure=Some(AudioPlaybackFailure::InvalidCredit);
            },
        }
        if failed {
            eprintln!("picomado audio_device_failed stream={} session={} display={} sent={:?} accepted={} consumed={}",id.stream_id,id.session_epoch,id.display_epoch,sender.sent_end(),accepted,consumed);
            t.failure.get_or_insert(AudioPlaybackFailure::DeviceFailed);
        }
    }
    fn publish_failure(&mut self,app:&SharedApp,session:&SessionToken)->Result<(),()> {
        let Some(t)=self.transfer.as_mut() else{return Ok(())};
        let Some(reason)=t.failure.filter(|_|!t.failure_reported) else{return Ok(())};
        let consumed=self.sender.consumed_end().ok_or(())?;
        // Publish before trying AudioStop: a broken socket must not hide failure.
        app.lock().map_err(|_|())?.audio_playback_failed(session,&t.display,&t.plugin,&t.spec,consumed,reason).map_err(|_|())?;
        t.failure_reported=true;
        Ok(())
    }
}
// Keep one source read (including its original timeout) alive across credit events.
async fn read_audio_with_credits<F,C>(read:F,rx:&mut tokio::sync::mpsc::Receiver<WriterCommand>,pending:&mut Option<WriterCommand>,mut credit:C)->Option<F::Output>
where F:std::future::Future,C:FnMut(crate::audio_transport::StreamIdentity,u64,u64,bool)->bool {
    if pending.is_some(){return None;}
    tokio::pin!(read);
    loop {tokio::select! {
        command=rx.recv()=>match command {
            Some(WriterCommand::AudioCredit(id,accepted,consumed,failed))=>{
                if !credit(id,accepted,consumed,failed){return None;}
            },
            other=>{*pending=Some(other.unwrap_or(WriterCommand::Stop));return None;}
        },
        result=&mut read=>return Some(result),
    }}
}
async fn service_audio<S:Sink<Message>+Unpin>(sink:&mut S,app:&SharedApp,session:&SessionToken,audio:&mut SocketAudio,rx:&mut tokio::sync::mpsc::Receiver<WriterCommand>,pending:&mut Option<WriterCommand>)->Result<(),()> {
    audio.publish_failure(app,session)?;
    let selected={
        let guard=app.read().map_err(|_|())?;
        if guard.bluetooth_view(&session.device_id).observed==crate::bluetooth::Observed::Connected {
            let offer=guard.audio_playback_for(session);
            if let Some((display,_,_,_))=offer.as_ref() {
                // FrameStart establishes the device display epoch. The writer's
                // image branch yields, so a ready audio tick can otherwise send
                // AudioStart first. Wait only for receipt, never physical render.
                let continuing=audio.transfer.as_ref().is_some_and(|t| &t.display==display);
                let received=continuing || guard.audio_display_received(display);
                if !continuing && !received {
                    if audio.waiting_display.as_ref()!=Some(display) {
                        eprintln!("picomado audio_wait_display id={} session={} display={}",session.device_id,display.session_epoch,display.display_epoch);
                        audio.waiting_display=Some(display.clone());
                    }
                    None
                } else { audio.waiting_display=None;offer }
            } else { audio.waiting_display=None;None }
        }else{None}
    };
    let same=match (&audio.transfer,&selected){
        (Some(t),Some((d,p,s,_)))=>&t.display==d&&&t.plugin==p&&&t.spec==s,
        (None,None)=>true,_=>false,
    };
    if !same {
        if let Some(t)=audio.transfer.take(){
            eprintln!("picomado audio_offer_changed id={} stream={} sent={:?} consumed={:?}",session.device_id,t.identity.stream_id,audio.sender.sent_end(),audio.sender.consumed_end());
            if !t.stopped {write_wire(sink,WireMessage::AudioStop{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id}).await.map_err(|_|())?;}
        }
        audio.sender.stop();
        if let Some((display,plugin,spec,paths))=selected {
            let capacity=app.read().map_err(|_|())?.audio_buffer_capacity(session);
            audio.sender.set_capacity(capacity).map_err(|_|())?;
            let identity=audio.sender.start(display.display_epoch,spec.start_frame).map_err(|_|())?;
            write_wire(sink,WireMessage::AudioStart{incarnation:identity.incarnation,session_epoch:identity.session_epoch,display_epoch:identity.display_epoch,stream_id:identity.stream_id,first_frame:spec.start_frame,rate:spec.sample_rate}).await.map_err(|_|())?;
            eprintln!("picomado audio_start_sent id={} stream={} session={} display={} rate={} first={} frames={:?}",session.device_id,identity.stream_id,identity.session_epoch,identity.display_epoch,spec.sample_rate,spec.start_frame,spec.frame_count);
            audio.transfer=Some(AudioTransfer{display,plugin,spec,paths,identity,failure:None,failure_reported:false,stopped:false,drained:false,final_frame:None,last_credit:tokio::time::Instant::now()});
        }
    }
    let Some(t)=audio.transfer.as_mut() else{return Ok(())};
    if t.failure.is_some() {
        if !t.stopped {
            write_wire(sink,WireMessage::AudioStop{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id}).await.map_err(|_|())?;
            t.stopped=true;
        }
        return Ok(());
    }
    if t.final_frame.is_some_and(|end|audio.sender.consumed_end()==Some(end)){
        if !t.drained {eprintln!("picomado audio_drained id={} stream={} final={:?}",session.device_id,t.identity.stream_id,t.final_frame);t.drained=true;}
        return Ok(());
    }
    if t.last_credit.elapsed()>Duration::from_secs(3){
        eprintln!("picomado audio_credit_timeout id={} stream={} sent={:?} accepted={:?} consumed={:?}",session.device_id,t.identity.stream_id,audio.sender.sent_end(),audio.sender.accepted_end(),audio.sender.consumed_end());
        t.failure=Some(AudioPlaybackFailure::CreditTimeout);
        let id=t.identity;
        audio.publish_failure(app,session)?;
        write_wire(sink,WireMessage::AudioStop{incarnation:id.incarnation,session_epoch:id.session_epoch,display_epoch:id.display_epoch,stream_id:id.stream_id}).await.map_err(|_|())?;
        audio.transfer.as_mut().unwrap().stopped=true;
        return Ok(());
    }
    // At most 512 PCM frames per scheduler turn; control can interrupt source IO.
    for _ in 0..2 {
        let count=audio.sender.allowance()/128*128;if count==0{break;}
        let offset=audio.sender.sent_end().ok_or(())?;
        if t.spec.frame_count.is_some_and(|n|offset>=n){break;}
        let plugin=t.plugin.clone();
        let read=tokio::time::timeout(Duration::from_millis(100),crate::plugins::read_audio_chunk(&plugin,t.spec.clone(),offset,count as u32,t.paths.clone()));
        let Some(result)=read_audio_with_credits(read,rx,pending,|id,accepted,consumed,failed|{
            SocketAudio::apply_credit(&mut audio.sender,t,id,accepted,consumed,failed);
            t.failure.is_none()
        }).await else{audio.publish_failure(app,session)?;return Ok(());};
        let mut bytes=match result {
            Ok(Ok(b))=>b,
            Ok(Err(_))=>{eprintln!("picomado audio_read_failed id={} stream={} offset={} count={}",session.device_id,t.identity.stream_id,offset,count);t.failure=Some(AudioPlaybackFailure::SourceReadFailed);break;},
            Err(_)=>{eprintln!("picomado audio_read_timeout id={} stream={} offset={} count={}",session.device_id,t.identity.stream_id,offset,count);t.failure=Some(AudioPlaybackFailure::SourceReadTimeout);break;},
        };
        if bytes.is_empty(){break;}
        let still_current={let guard=app.read().map_err(|_|())?;
            guard.bluetooth_view(&session.device_id).observed==crate::bluetooth::Observed::Connected &&
            guard.audio_playback_for(session).is_some_and(|(d,p,s,_)|d==t.display&&p==t.plugin&&s==t.spec)
        };
        if !still_current {break;}
        if t.spec.frame_count==Some(offset+(bytes.len()/4) as u64) {
            let frames=bytes.len()/4;
            let padded=frames.div_ceil(128)*128;
            bytes.resize(padded*4,0);
            t.final_frame=Some(offset+padded as u64);
            write_wire(sink,WireMessage::AudioFinish{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id,final_frame:offset+padded as u64}).await.map_err(|_|())?;
        }
        // Apply the latest per-device gain immediately before sending. Updating
        // gain does not change the source spec, display token or stream identity.
        let volume=app.read().map_err(|_|())?.audio_volume(session);
        crate::audio_transport::apply_volume(&mut bytes,volume).map_err(|_|())?;
        let packet=crate::audio_transport::encode_chunk(t.identity,offset,&bytes).map_err(|_|())?;
        tokio::time::timeout(SEND_TIMEOUT,sink.send(Message::Binary(packet.into()))).await.map_err(|_|())?.map_err(|_|())?;
        audio.sender.record_sent(offset,bytes.len()/4).map_err(|_|())?;
        if offset==t.spec.start_frame {eprintln!("picomado audio_first_pcm_sent id={} stream={} frames={}",session.device_id,t.identity.stream_id,bytes.len()/4);}
    }
    audio.publish_failure(app,session)?;
    Ok(())
}

#[cfg(test)]
mod audio_read_credit_tests {
    use super::*;
    fn id()->crate::audio_transport::StreamIdentity{crate::audio_transport::StreamIdentity{incarnation:1,session_epoch:2,display_epoch:3,stream_id:4}}
    fn transfer() -> SocketAudio {
        let session=SessionToken{device_id:"test".into(),incarnation:1,session_epoch:2};
        let mut audio=SocketAudio::new(&session);
        let identity=audio.sender.start(3,48000).unwrap();
        for offset in [48000,48256,48512,48768] {audio.sender.record_sent(offset,256).unwrap();}
        audio.transfer=Some(AudioTransfer {
            display:crate::DisplayToken{device_id:"test".into(),incarnation:1,session_epoch:2,display_epoch:3,screen_owner_epoch:1,settings_revision:1},
            plugin:"future-audio-plugin".into(),
            spec:crate::plugin_jobs::AudioPlaybackSpec{asset_id:"test".into(),generation:1,sample_rate:48000,channels:2,
                frame_count:Some(96000),start_frame:48000,started_mono_ms:0,source:serde_json::Value::Null},
            paths:crate::plugin_jobs::PluginPaths{data:"unused".into(),cache:"unused".into()},
            identity,failure:None,failure_reported:false,stopped:false,drained:false,final_frame:None,
            last_credit:tokio::time::Instant::now(),
        });
        audio
    }
    #[test]
    fn terminal_credit_is_validated_and_frozen_independently_of_sent_position() {
        let mut audio=transfer();let identity=audio.sender.identity().unwrap();
        audio.credit(identity,48768,48512,false);
        audio.credit(identity,49024,48768,true);
        assert_eq!(audio.sender.consumed_end(),Some(48768));
        assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::DeviceFailed));
        audio.credit(identity,49024,49024,false);
        assert_eq!(audio.sender.consumed_end(),Some(48768),"terminal position is frozen");
        for (accepted,consumed) in [(49025,48768),(49024,49025),(48768,47999)] {
            let mut audio=transfer();let identity=audio.sender.identity().unwrap();
            audio.credit(identity,48768,48512,false);
            audio.credit(identity,accepted,consumed,true);
            assert_eq!(audio.sender.consumed_end(),Some(48512));
            assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::InvalidCredit));
        }
        let mut audio=transfer();let identity=audio.sender.identity().unwrap();
        audio.credit(identity,48768,48512,false);
        audio.credit(identity,48512,48256,true);
        assert_eq!(audio.sender.consumed_end(),Some(48512),"regressed terminal credit cannot rewind");
        assert_eq!(audio.transfer.as_ref().unwrap().failure,Some(AudioPlaybackFailure::DeviceFailed));
    }
    #[test]
    fn old_stream_session_display_and_incarnation_cannot_fail_current_audio() {
        let mut audio=transfer();let identity=audio.sender.identity().unwrap();
        let mut old=identity;old.stream_id+=1;audio.credit(old,u64::MAX,u64::MAX,true);
        let mut old=identity;old.session_epoch+=1;audio.credit(old,u64::MAX,u64::MAX,true);
        let mut old=identity;old.display_epoch+=1;audio.credit(old,u64::MAX,u64::MAX,true);
        let mut old=identity;old.incarnation+=1;audio.credit(old,u64::MAX,u64::MAX,true);
        assert!(audio.transfer.as_ref().unwrap().failure.is_none());
        assert_eq!(audio.sender.consumed_end(),Some(48000));
    }
    #[tokio::test]
    async fn failure_interrupts_pending_source_read_without_restarting_it() {
        let mut audio=transfer();let identity=audio.sender.identity().unwrap();
        let(tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
        tx.send(WriterCommand::AudioCredit(identity,49024,48768,true)).await.unwrap();
        let result=tokio::time::timeout(Duration::from_millis(100),read_audio_with_credits(
            std::future::pending::<()>(),&mut rx,&mut pending,|id,accepted,consumed,failed|{
                audio.credit(id,accepted,consumed,failed);audio.transfer.as_ref().unwrap().failure.is_none()
            })).await.unwrap();
        assert!(result.is_none());assert!(pending.is_none());
        assert_eq!(audio.sender.consumed_end(),Some(48768));
    }
    #[tokio::test]
    async fn credit_keeps_one_read_alive_but_stop_interrupts() {
        let(tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
        // The read cannot complete until all four credits have actually been
        // applied. No scheduler-speed assumption or sleep race is involved.
        for _ in 0..4 {tx.send(WriterCommand::AudioCredit(id(),0,0,false)).await.unwrap();}
        let (complete_tx,complete_rx)=tokio::sync::oneshot::channel();
        let mut complete_tx=Some(complete_tx);
        let mut credits=0;
        let result=tokio::time::timeout(Duration::from_secs(5),read_audio_with_credits(
            async{complete_rx.await.unwrap();vec![42;1024]},&mut rx,&mut pending,
            |_,_,_,_|{credits+=1;if credits==4 {complete_tx.take().unwrap().send(()).unwrap();}true}
        )).await.expect("credit processing deadlocked");
        assert_eq!(result,Some(vec![42;1024]));assert_eq!(credits,4);assert!(pending.is_none());
        drop(tx);
        let(tx,mut rx)=tokio::sync::mpsc::channel(8);tx.send(WriterCommand::Stop).await.unwrap();
        let result=tokio::time::timeout(Duration::from_millis(100),read_audio_with_credits(std::future::pending::<()>(),&mut rx,&mut pending,|_,_,_,_|true)).await.unwrap();
        assert!(result.is_none());assert!(matches!(pending,Some(WriterCommand::Stop)));
    }
    #[tokio::test]
    async fn credit_does_not_extend_read_deadline() {
        let(tx,mut rx)=tokio::sync::mpsc::channel(8);let mut pending=None;
        let producer=tokio::spawn(async move{loop{if tx.send(WriterCommand::AudioCredit(id(),0,0,false)).await.is_err(){break;}tokio::time::sleep(Duration::from_millis(1)).await;}});
        let read=tokio::time::timeout(Duration::from_millis(20),std::future::pending::<()>());
        let result=tokio::time::timeout(Duration::from_millis(200),read_audio_with_credits(read,&mut rx,&mut pending,|_,_,_,_|true)).await.unwrap();
        assert!(result.unwrap().is_err());assert!(pending.is_none());producer.abort();
    }
}
