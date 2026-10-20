// Included in transport::tests for the same production writer/fixtures.
type RecoveryProbe=crate::socket_progress::WriteProgress;
fn recovery_accept(p:&RecoveryProbe,n:usize){p.test_accept(n);}
struct RecoverySink {
    recorded:Arc<std::sync::Mutex<Vec<Message>>>, count:Arc<AtomicUsize>,
    probe:RecoveryProbe, hold:Duration, progress:bool,
    began:Option<tokio::time::Instant>, wait:Option<Pin<Box<tokio::time::Sleep>>>,
}
impl Sink<Message> for RecoverySink {
    type Error=();
    fn poll_ready(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
    fn start_send(self:Pin<&mut Self>,m:Message)->Result<(),()>{
        let s=self.get_mut();s.count.fetch_add(1,Ordering::SeqCst);s.recorded.lock().unwrap().push(m);Ok(())
    }
    fn poll_flush(self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<Result<(),()>>{
        use std::future::Future;
        let s=self.get_mut();
        if s.count.load(Ordering::SeqCst)!=3 {return Poll::Ready(Ok(()));}
        let began=*s.began.get_or_insert_with(tokio::time::Instant::now);
        if began.elapsed()>=s.hold{return Poll::Ready(Ok(()));}
        if s.wait.is_none(){s.wait=Some(Box::pin(tokio::time::sleep(Duration::from_millis(80))));}
        if s.wait.as_mut().unwrap().as_mut().poll(cx).is_ready(){
            if s.progress{recovery_accept(&s.probe,64);}
            s.wait=Some(Box::pin(tokio::time::sleep(Duration::from_millis(80))));
            let _=s.wait.as_mut().unwrap().as_mut().poll(cx);
        }
        Poll::Pending
    }
    fn poll_close(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Result<(),()>>{Poll::Ready(Ok(()))}
}
async fn recovery_fixture(hold:Duration,progress:bool,control:bool,replace_session:bool)->(&'static str,Duration){
    let tmp=tempfile::tempdir().unwrap();
    let app=Arc::new(std::sync::Mutex::new(app_at_fixture_date(&tmp.path().join("recovery.db"))));
    let caps=std::collections::BTreeSet::from(["lcd".into(),crate::protocol::FRAME_BYTES_CAPABILITY.into(),crate::protocol::LCD_PUSH_CAPABILITY.into()]);
    let token=app.lock().unwrap().hello("recovery",caps.clone(),1).unwrap();
    let recorded=Arc::new(std::sync::Mutex::new(Vec::new()));let count=Arc::new(AtomicUsize::new(0));
    let probe=RecoveryProbe::default();
    let sink=RecoverySink{recorded:recorded.clone(),count:count.clone(),probe:probe.clone(),hold,progress,began:None,wait:None};
    let (tx,rx)=tokio::sync::mpsc::channel(32);
    let task=tokio::spawn(writer_loop_with_progress(sink,rx,app.clone(),None,Some(probe)));
    tx.send(WriterCommand::Bind(token.clone())).await.unwrap();
    while count.load(Ordering::SeqCst)<1{tokio::time::sleep(Duration::from_millis(1)).await;}
    let began=std::time::Instant::now();
    app.lock().unwrap().queue_lcd_video(&token,12,1,37,vec![0x37;25600]).unwrap();
    while count.load(Ordering::SeqCst)<3&&!task.is_finished(){tokio::time::sleep(Duration::from_millis(1)).await;}
    if control {
        enqueue_reply(&tx,WireMessage::InputAccepted{input_context:None,selected:None,display_epoch:1},token.session_epoch).await.unwrap();
    }
    if replace_session{app.lock().unwrap().hello("recovery",caps,1).unwrap();}
    let mut stopped=false;
    while !task.is_finished(){
        if count.load(Ordering::SeqCst)>=9 && !stopped {
            if control {
                let found=recorded.lock().unwrap().iter().any(|m|match m{Message::Binary(b)=>Decoder::default().push(b).unwrap().iter().any(|x|matches!(x,WireMessage::InputAccepted{..})),_=>false});
                if !found {tokio::time::sleep(Duration::from_millis(1)).await;continue;}
            }
            tx.send(WriterCommand::Stop).await.ok();stopped=true;
        }
        assert!(began.elapsed()<Duration::from_secs(12));tokio::time::sleep(Duration::from_millis(2)).await;
    }
    (task.await.unwrap(),began.elapsed())
}
#[tokio::test]
async fn recovery_normal_transfer(){
    let (why,t)=recovery_fixture(Duration::ZERO,false,false,false).await;
    assert_eq!(why,"requested_stop");assert!(t<Duration::from_secs(1));
}
#[tokio::test]
async fn recovery_slow_kernel_write_progress_survives_old_two_second_limit(){
    let (why,t)=recovery_fixture(Duration::from_millis(3200),true,false,false).await;
    assert_eq!(why,"requested_stop");assert!(t>=Duration::from_secs(3)&&t<Duration::from_secs(5));
}
#[tokio::test]
async fn recovery_no_progress_expires_within_three_seconds(){
    let (why,t)=recovery_fixture(Duration::from_secs(15),false,false,false).await;
    assert_eq!(why,"stream_write_stall");assert!(t>=Duration::from_millis(2900)&&t<Duration::from_millis(3400));
}
#[tokio::test]
async fn recovery_continuous_progress_cannot_extend_total_frame_limit(){
    let (why,t)=recovery_fixture(Duration::from_secs(15),true,false,false).await;
    assert_eq!(why,"stream_write_stall");assert!(t>=Duration::from_millis(9800)&&t<Duration::from_millis(10400));
}
#[tokio::test]
async fn recovery_control_preempts_slow_fragment_without_splicing(){
    let (why,t)=recovery_fixture(Duration::from_secs(15),true,true,false).await;
    assert_eq!(why,"control_preempt_timeout");assert!(t<Duration::from_millis(600));
}
#[tokio::test]
async fn recovery_control_delivered_after_short_inflight_message(){
    let (why,t)=recovery_fixture(Duration::from_millis(160),true,true,false).await;
    assert_eq!(why,"requested_stop");assert!(t<Duration::from_millis(600));
}
#[tokio::test]
async fn recovery_replaced_session_cancels_inflight_message(){
    let (why,t)=recovery_fixture(Duration::from_secs(15),true,false,true).await;
    assert_eq!(why,"session_not_current");assert!(t<Duration::from_millis(400));
}
