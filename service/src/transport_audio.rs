// BT-AUDIO-20261107. The socket writer owns each bounded audio session.
struct AudioTransfer {
    display: crate::DisplayToken,
    plugin: String,
    spec: crate::plugin_jobs::AudioPlaybackSpec,
    paths: crate::plugin_jobs::PluginPaths,
    identity: crate::audio_transport::StreamIdentity,
    failed: bool,
    stopped: bool,
    final_frame: Option<u64>,
    last_credit: tokio::time::Instant,
}
struct SocketAudio {
    sender: crate::audio_transport::AudioSender,
    transfer: Option<AudioTransfer>,
}
impl SocketAudio {
    fn new(t:&SessionToken)->Self {Self{sender:crate::audio_transport::AudioSender::new(t.incarnation,t.session_epoch),transfer:None}}
    fn credit(&mut self,id:crate::audio_transport::StreamIdentity,accepted:u64,consumed:u64,failed:bool){
        let Some(t)=self.transfer.as_mut().filter(|t|t.identity==id) else{return};
        if failed {t.failed=true;return;}
        let progressed=Some(accepted)>self.sender.accepted_end()||Some(consumed)>self.sender.consumed_end();
        match self.sender.apply_credit(id,accepted,consumed) {
            Ok(true) if progressed=>t.last_credit=tokio::time::Instant::now(),
            Ok(true)=>{},
            Ok(false)=>{},Err(_)=>t.failed=true,
        }
    }
}
async fn service_audio<S:Sink<Message>+Unpin>(sink:&mut S,app:&SharedApp,session:&SessionToken,audio:&mut SocketAudio,rx:&mut tokio::sync::mpsc::Receiver<WriterCommand>,pending:&mut Option<WriterCommand>)->Result<(),()> {
    let selected={
        let guard=app.read().map_err(|_|())?;
        if guard.bluetooth_view(&session.device_id).observed==crate::bluetooth::Observed::Connected {
            guard.audio_playback_for(session)
        }else{None}
    };
    let same=match (&audio.transfer,&selected){
        (Some(t),Some((d,p,s,_)))=>&t.display==d&&&t.plugin==p&&&t.spec==s,
        (None,None)=>true,_=>false,
    };
    if !same {
        if let Some(t)=audio.transfer.take(){
            write_wire(sink,WireMessage::AudioStop{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id}).await.map_err(|_|())?;
        }
        audio.sender.stop();
        if let Some((display,plugin,spec,paths))=selected {
            let identity=audio.sender.start(display.display_epoch,spec.start_frame).map_err(|_|())?;
            write_wire(sink,WireMessage::AudioStart{incarnation:identity.incarnation,session_epoch:identity.session_epoch,display_epoch:identity.display_epoch,stream_id:identity.stream_id,first_frame:spec.start_frame,rate:spec.sample_rate}).await.map_err(|_|())?;
            audio.transfer=Some(AudioTransfer{display,plugin,spec,paths,identity,failed:false,stopped:false,final_frame:None,last_credit:tokio::time::Instant::now()});
        }
    }
    let Some(t)=audio.transfer.as_mut() else{return Ok(())};
    if t.failed {
        if !t.stopped {
            write_wire(sink,WireMessage::AudioStop{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id}).await.map_err(|_|())?;
            t.stopped=true;
        }
        return Ok(());
    }
    if t.final_frame.is_some_and(|end|audio.sender.consumed_end()==Some(end)){return Ok(());}
    if t.last_credit.elapsed()>Duration::from_secs(3){
        t.failed=true;t.stopped=true;
        write_wire(sink,WireMessage::AudioStop{incarnation:t.identity.incarnation,session_epoch:t.identity.session_epoch,display_epoch:t.identity.display_epoch,stream_id:t.identity.stream_id}).await.map_err(|_|())?;
        return Ok(());
    }
    // At most 512 PCM frames per scheduler turn; control can interrupt source IO.
    for _ in 0..2 {
        let count=audio.sender.allowance()/128*128;if count==0{break;}
        let offset=audio.sender.sent_end().ok_or(())?;
        if t.spec.frame_count.is_some_and(|n|offset>=n){break;}
        let result=tokio::select! {biased;
            command=rx.recv(),if pending.is_none()=>{*pending=Some(command.unwrap_or(WriterCommand::Stop));return Ok(());},
            result=tokio::time::timeout(Duration::from_millis(100),crate::plugins::read_audio_chunk(&t.plugin,t.spec.clone(),offset,count as u32,t.paths.clone()))=>result,
        };
        let mut bytes=match result {Ok(Ok(b))=>b,_=>{t.failed=true;break;}};
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
        let packet=crate::audio_transport::encode_chunk(t.identity,offset,&bytes).map_err(|_|())?;
        tokio::time::timeout(SEND_TIMEOUT,sink.send(Message::Binary(packet.into()))).await.map_err(|_|())?.map_err(|_|())?;
        audio.sender.record_sent(offset,bytes.len()/4).map_err(|_|())?;
    }
    Ok(())
}
