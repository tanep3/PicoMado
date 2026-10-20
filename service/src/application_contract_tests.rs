use super::*;
use crate::content::{PluginPlan, ContentError};

struct Rules(&'static str);
impl crate::Plugin for Rules {
    fn id(&self)->&str{self.0}
    fn supports(&self,caps:&BTreeSet<String>)->bool{caps.contains("lcd")}
    fn candidates<'a>(&'a self,_q:&'a dyn crate::WorldQuery,_e:&'a Event)->Box<dyn Iterator<Item=crate::Candidate>+'a>{Box::new(std::iter::empty())}
}
#[derive(Clone)]
struct Content {id:&'static str,video:bool}
impl ContentPlugin for Content {
    fn clone_box(&self)->Box<dyn ContentPlugin>{Box::new(self.clone())}
    fn id(&self)->&'static str{self.id}
    fn publish(&mut self,_ctx:&PluginContext,_source:&[u8],_id:&str,_day:&str)->Result<(),ContentError>{Ok(())}
    fn position(&self,_ctx:&PluginContext)->Option<PluginPosition>{None}
    fn plan(&mut self,_ctx:&PluginContext,_action:ButtonAction)->Result<PluginPlan,ContentError>{Err(ContentError::Unsupported)}
    fn session_reset(&mut self,_id:&str,_inc:u64){}
    fn delete_device(&mut self,_id:&str){}
    fn current_frame(&self,_ctx:&PluginContext)->Option<Vec<u8>>{(!self.video).then(||vec![7;25600])}
    fn playback(&self,_ctx:&PluginContext)->Option<crate::plugin_jobs::PlaybackSpec>{
        self.video.then(||crate::plugin_jobs::PlaybackSpec{
            asset_id:"fixture".into(),generation:1,fps:6,frame_count:20,started_mono_ms:0,source:serde_json::Value::Null,
        })
    }
}
fn composition()->crate::plugins::PluginSet {
    let mut set=crate::plugins::PluginSet::default();
    for (id,video) in [("fixture-static",false),("fixture-video",true),("extension",false)]{
        set.register(Box::new(Rules(id)),Box::new(Content{id,video})).unwrap();
    }
    set
}
#[test]
fn new_static_and_video_plugins_use_unchanged_application_and_transport_contracts(){
    let temp=tempfile::tempdir().unwrap();let db=temp.path().join("state.db");
    let mut app=App::open_with_plugin_set(&db,temp.path(),Arc::new(crate::fetch::SystemClock),composition()).unwrap();
    let caps=["lcd",crate::protocol::FRAME_BYTES_CAPABILITY,crate::protocol::LCD_PUSH_CAPABILITY].into_iter().map(String::from).collect();
    let token=app.hello("fixture-device",caps,1).unwrap();
    assert_eq!(app.view().devices[0].selected.as_deref(),Some("fixture-static"));
    app.queue_selected_cached_frame(&token).unwrap();
    assert!(app.pending_effects().iter().any(|(_,e)|matches!(e,Effect::SendFrame{..})));
    app.button(&token,ButtonAction::Down).unwrap();
    assert_eq!(app.playback_offers()[0].plugin,"fixture-video");
    let rx=app.subscribe_lcd_video(&token).unwrap();
    app.queue_lcd_video(&token,6,0,1,vec![3;25600]).unwrap();
    let old=rx.borrow().as_ref().unwrap().clone();
    assert!(app.lcd_video_is_current(&old));
    app.button(&token,ButtonAction::Down).unwrap();
    assert_eq!(app.view().devices[0].selected.as_deref(),Some("extension"));
    assert!(!app.lcd_video_is_current(&old));
    drop(app);
    let app=App::open_with_plugin_set(&db,temp.path(),Arc::new(crate::fetch::SystemClock),composition()).unwrap();
    assert_eq!(app.view().devices[0].selected.as_deref(),Some("extension"));
}
#[cfg(feature="boundary-observation")]
#[test]
fn full_observer_preserves_selection_and_persistence(){
    let temp=tempfile::tempdir().unwrap();let mut app=App::open(&temp.path().join("state.db")).unwrap();
    let token=app.hello("fixture-device",["lcd".into()].into(),1).unwrap();
    let source=app.observation_source();source.enable(true);
    for _ in 0..100 {app.button(&token,ButtonAction::Down).unwrap();}
    assert!(source.status().dropped>0);
    let expected=app.view().devices[0].selected.clone();drop(app);
    let restored=App::open(&temp.path().join("state.db")).unwrap();
    assert_eq!(restored.view().devices[0].selected,expected);
}

#[cfg(feature="boundary-observation")]
#[test]
fn causes_stay_with_exact_frame_and_view_across_overlapping_inputs(){
    use crate::observation::Cause;
    let temp=tempfile::tempdir().unwrap();let mut app=App::open_with_plugin_set(&temp.path().join("db"),temp.path(),Arc::new(crate::fetch::SystemClock),composition()).unwrap();
    let token=app.hello("fixture",["lcd".into()].into(),42).unwrap();
    let initial=app.pending_effects().into_iter().find_map(|(_,e)|if let Effect::SendFrame{token,..}=e{Some(token)}else{None}).unwrap();
    let a=Cause{version:1,boot:101,operation:201};let b=Cause{version:1,boot:101,operation:202};
    app.with_observation_cause("fixture",Some(a),|app|app.button(&token,ButtonAction::Down)).unwrap();
    // Video starts asynchronously after the accepted view change.
    app.queue_lcd_frame(&token,"fixture-video",vec![3;25600]).unwrap();
    let frame_a=app.pending_effects().into_iter().find_map(|(_,e)|if let Effect::SendFrame{token,..}=e{Some(token)}else{None}).unwrap();
    assert_eq!(app.frame_observation_cause(&frame_a),Some(a));
    let mut transfer=crate::lcd_stream::VideoTransfer::new_observed(Arc::new(crate::lcd_stream::VideoFrame{token:frame_a.clone(),fps:6,sequence:1,generation:1,mode:"normal",pixels:Arc::new(vec![3;25600])}),Some(a));
    app.with_observation_cause("fixture",Some(b),|app|app.button(&token,ButtonAction::Down)).unwrap();
    let metadata=crate::protocol::Decoder::default().push_observed(&transfer.next_packet().unwrap().unwrap()).unwrap();
    assert_eq!(metadata[0].cause,Some(a),"in-flight video retains its immutable cause");
    let frame_b=app.pending_effects().into_iter().find_map(|(_,e)|if let Effect::SendFrame{token,..}=e{Some(token)}else{None}).unwrap();
    assert_eq!(app.frame_observation_cause(&frame_b),Some(b));assert_eq!(app.frame_observation_cause(&frame_a),Some(a));assert_eq!(app.frame_observation_cause(&initial),None);
    let unknown=FrameToken{frame_id:u64::MAX,display_epoch:u64::MAX,..frame_b.clone()};assert_eq!(app.frame_observation_cause(&unknown),None);
}
