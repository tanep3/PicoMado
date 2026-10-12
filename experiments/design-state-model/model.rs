//! Design contract model only: no network, device, database, or production runtime.
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode { Normal, Sub, Menu, ConnectionMenu, Standby, Connecting }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen { Normal, Connecting, Menu, Standby }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View { Plugin(u8), Logo, Loading, Data { snapshot: u64, day: u32, region: u8 }, Error }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token { incarnation: u64, revision: u64, session: u64, display: u64, request: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame { token: Token, local_owner: u64, id: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dataset { id: u64, day: u32, region: u8 }
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pico { screen: Screen, owner: u64, attempt: u64, pending: Option<Frame>, drawing: Option<Frame>, needs_repaint: bool, pending_standby: bool, is_eink: bool }
impl Pico {
    fn local(&mut self, next: Screen) {
        if next == Screen::Standby && self.is_eink && self.drawing.is_some() {
            self.pending_standby = true; self.owner += 1; self.pending = None; return;
        }
        self.screen = next;
        self.owner += 1;
        self.pending = None;
        self.needs_repaint |= self.drawing.is_some();
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Device {
    incarnation: u64, revision: u64, session: u64, display: u64, fetch_generation: u64,
    online: bool, mode: Mode, enabled: [bool; 3], firmware_support: [bool; 3], web_profile: [bool; 3], selected: Option<u8>,
    region: u8, today: u32, latest: Option<Dataset>, pinned: Option<Dataset>,
    view: View, accepted_results: u32, pico: Pico,
}
impl Device {
    fn token(&self) -> Token { Token { incarnation: self.incarnation, revision: self.revision, session: self.session, display: self.display, request: self.fetch_generation } }
    fn current(&self, t: Token) -> bool { t.incarnation == self.incarnation && t.revision == self.revision && t.session == self.session && t.display == self.display }
    fn current_inc_rev(&self, t: Token) -> bool { t.incarnation == self.incarnation && t.revision == self.revision }
    fn current_fetch(&self, t: Token) -> bool { self.current_inc_rev(t) && t.request == self.fetch_generation }
    fn normal_view(&self) -> View {
        if self.selected.is_none() { return View::Logo; }
        match self.latest {
            Some(d) if d.day == self.today && d.region == self.region => View::Data { snapshot: d.id, day: d.day, region: d.region },
            _ => View::Loading,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct State { next_incarnation: u64, device: Option<Device> }
impl State { fn empty() -> Self { Self { next_incarnation: 0, device: None } } }
struct WorldQuery<'a>(&'a State);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect { CloseSession, RequestDisplay, RepaintLocal, RenderedAck(u64), Fetch }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Completion { Save, Send, Ack }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Hello, Delete, Power, EnterSub, EnterMenu, WebOff(u8), WebOn(u8), Local(Screen),
    ConnectionCancel, AttemptDone(u64), ConnectionLost,
    Receive(Frame), StartDraw, CompleteDraw, Completion(Token, Completion),
    SetRegion(u8), Refresh, FetchOk(Token, Dataset), FetchFailed(Token),
    BeginBrowse, CancelBrowse, Midnight(u32),
}
type Transition = fn(State, &Event) -> (State, Vec<Effect>);
#[derive(Clone, Copy)]
struct Candidate { priority: u8, transition: Transition }
type Rule = fn(&WorldQuery<'_>, &Event) -> Option<Candidate>;

fn power_normal(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    matches!((q.0.device.as_ref().map(|d| d.mode), e), (Some(Mode::Normal), Event::Power))
        .then_some(Candidate { priority: 3, transition: apply_power_normal })
}
fn power_sub(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    matches!((q.0.device.as_ref().map(|d| d.mode), e), (Some(Mode::Sub), Event::Power))
        .then_some(Candidate { priority: 3, transition: apply_power_sub })
}
fn power_menu(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    matches!((q.0.device.as_ref().map(|d| d.mode), e), (Some(Mode::Menu | Mode::ConnectionMenu), Event::Power))
        .then_some(Candidate { priority: 3, transition: apply_power_menu })
}
fn kernel_rule(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    let valid = match e {
        Event::Hello => true,
        Event::Delete => q.0.device.is_some(),
        Event::EnterSub | Event::EnterMenu => q.0.device.as_ref().is_some_and(|d| d.mode == Mode::Normal),
        Event::WebOff(i) => q.0.device.as_ref().is_some_and(|d| (*i as usize) < d.enabled.len()),
        Event::WebOn(i) => q.0.device.as_ref().is_some_and(|d| (*i as usize) < d.enabled.len() && d.firmware_support[*i as usize] && d.web_profile[*i as usize]),
        Event::Completion(t,k) => q.0.device.as_ref().is_some_and(|d| match k { Completion::Save => d.current_inc_rev(*t), Completion::Send | Completion::Ack => d.current(*t) }),
        _ => false,
    };
    valid.then_some(Candidate { priority: 2, transition: apply_kernel })
}
fn pico_rule(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    let Some(d) = q.0.device.as_ref() else { return None };
    let valid = match e {
        Event::Local(_) | Event::ConnectionCancel | Event::ConnectionLost => true,
        Event::AttemptDone(a) => *a == d.pico.attempt && d.pico.screen == Screen::Connecting,
        Event::Receive(f) => d.mode != Mode::Standby && !d.pico.pending_standby && d.pico.screen == Screen::Normal && d.current(f.token) && f.local_owner == d.pico.owner,
        Event::StartDraw => d.pico.pending.is_some_and(|f| !d.pico.pending_standby && d.pico.screen == Screen::Normal && d.current(f.token) && f.local_owner == d.pico.owner),
        Event::CompleteDraw => d.pico.drawing.is_some(),
        _ => false,
    };
    valid.then_some(Candidate { priority: 2, transition: apply_pico })
}
fn weather_rule(q: &WorldQuery<'_>, e: &Event) -> Option<Candidate> {
    let Some(d) = q.0.device.as_ref() else { return None };
    let valid = match e {
        Event::SetRegion(_) | Event::Refresh | Event::Midnight(_) => true,
        Event::FetchOk(t,data) => d.current_fetch(*t) && data.region == d.region,
        Event::FetchFailed(t) => d.current_fetch(*t),
        Event::BeginBrowse => d.mode == Mode::Normal && d.latest.is_some(),
        Event::CancelBrowse => d.mode == Mode::Sub,
        _ => false,
    };
    valid.then_some(Candidate { priority: 1, transition: apply_weather })
}
fn step(state: State, event: Event) -> (State, Vec<Effect>) {
    // The iterator queries each registered rule only as it is advanced.
    let q = WorldQuery(&state);
    let rules: [Rule; 6] = [power_normal, power_sub, power_menu, kernel_rule, pico_rule, weather_rule];
    let mut candidates = rules.into_iter().filter_map(|rule| rule(&q, &event));
    let Some(mut chosen) = candidates.next() else { return (state, vec![]) };
    for c in candidates {
        if c.priority > chosen.priority { chosen = c; }
        else if c.priority == chosen.priority { panic!("ambiguous transition candidates"); }
    }
    (chosen.transition)(state, &event)
}
fn apply_power_normal(mut s: State, _: &Event) -> (State, Vec<Effect>) {
    let d = s.device.as_mut().unwrap(); d.mode = Mode::Standby; d.display += 1; d.pico.local(Screen::Standby); (s, vec![])
}
fn apply_power_sub(mut s: State, _: &Event) -> (State, Vec<Effect>) {
    let d = s.device.as_mut().unwrap(); d.mode = Mode::Normal; d.pinned = None; d.display += 1; d.view = d.normal_view(); (s, vec![Effect::RequestDisplay])
}
fn apply_power_menu(mut s: State, _: &Event) -> (State, Vec<Effect>) {
    let d = s.device.as_mut().unwrap();
    d.mode = if d.mode == Mode::ConnectionMenu { Mode::Connecting } else { Mode::Normal };
    if d.mode == Mode::Connecting { d.pico.local(Screen::Connecting); } else { d.view = d.normal_view(); }
    d.display += 1; (s, vec![])
}
fn apply_kernel(mut s: State, e: &Event) -> (State, Vec<Effect>) {
    match e {
        Event::Hello => {
            if let Some(d) = s.device.as_mut() { d.online = true; d.session += 1; d.display += 1; d.mode = Mode::Normal; d.pico.local(Screen::Normal); d.view = d.normal_view(); }
            else {
                s.next_incarnation += 1;
                s.device = Some(Device { incarnation: s.next_incarnation, revision: 0, session: 1, display: 1, fetch_generation: 0,
                    online: true, mode: Mode::Normal, enabled: [true; 3], firmware_support: [true; 3], web_profile: [true; 3], selected: Some(0), region: 0, today: 1,
                    latest: None, pinned: None, view: View::Plugin(0), accepted_results: 0,
                    pico: Pico { screen: Screen::Normal, owner: 1, attempt: 1, pending: None, drawing: None, needs_repaint: false, pending_standby: false, is_eink: false } });
            }
            (s, vec![Effect::RequestDisplay])
        }
        Event::Delete => { s.device = None; (s, vec![Effect::CloseSession]) }
        Event::EnterSub => { s.device.as_mut().unwrap().mode = Mode::Sub; (s, vec![]) }
        Event::EnterMenu => { s.device.as_mut().unwrap().mode = Mode::Menu; (s, vec![]) }
        Event::WebOff(i) => {
            let d=s.device.as_mut().unwrap(); d.enabled[*i as usize]=false; d.revision+=1;
            if d.selected == Some(*i) {
                d.selected=d.enabled.iter().position(|x| *x).map(|n| n as u8);
                d.mode=Mode::Normal; d.pinned=None; d.display+=1;
                d.view=d.selected.map(View::Plugin).unwrap_or(View::Logo);
                (s, vec![Effect::RequestDisplay])
            } else { (s, vec![]) }
        }
        Event::WebOn(i) => {
            let d=s.device.as_mut().unwrap(); d.enabled[*i as usize]=true; d.revision+=1;
            if d.selected.is_none() { d.selected=Some(*i); d.mode=Mode::Normal; d.display+=1; d.view=View::Plugin(*i); return (s,vec![Effect::RequestDisplay]); }
            (s,vec![])
        }
        Event::Completion(_,_) => { s.device.as_mut().unwrap().accepted_results+=1; (s, vec![]) }
        _ => unreachable!(),
    }
}
fn apply_pico(mut s: State, e: &Event) -> (State, Vec<Effect>) {
    let d=s.device.as_mut().unwrap();
    match e {
        Event::Local(screen) => { d.pico.local(*screen); if *screen != Screen::Normal { d.display += 1; } (s, vec![]) }
        Event::ConnectionCancel => { d.pico.attempt+=1; d.pico.local(Screen::Menu); d.mode=Mode::ConnectionMenu; d.display+=1; (s, vec![]) }
        Event::AttemptDone(_) => { d.pico.local(Screen::Normal); (s, vec![]) }
        Event::ConnectionLost => { d.online=false; d.display+=1; d.mode=Mode::Connecting; d.pico.local(Screen::Connecting); (s, vec![]) }
        Event::Receive(frame) => { d.pico.pending=Some(*frame); (s, vec![]) }
        Event::StartDraw => { d.pico.drawing=d.pico.pending.take(); (s, vec![]) }
        Event::CompleteDraw => {
            let frame=d.pico.drawing.take().unwrap();
            if d.pico.pending_standby { d.pico.pending_standby=false; d.pico.screen=Screen::Standby; d.pico.needs_repaint=false; return (s, vec![]); }
            if d.pico.needs_repaint || d.pico.screen != Screen::Normal || !d.current(frame.token) || frame.local_owner != d.pico.owner {
                d.pico.needs_repaint=false; (s, vec![Effect::RepaintLocal])
            } else { (s, vec![Effect::RenderedAck(frame.id)]) }
        }
        _ => unreachable!(),
    }
}
fn apply_weather(mut s: State, e: &Event) -> (State, Vec<Effect>) {
    let d=s.device.as_mut().unwrap();
    match e {
        Event::SetRegion(region) => { d.region=*region; d.revision+=1; d.fetch_generation+=1; d.latest=None; d.pinned=None; d.mode=Mode::Normal; d.display+=1; d.view=View::Loading; (s, vec![Effect::Fetch]) }
        Event::FetchOk(_,data) => {
            d.latest=Some(*data);
            if d.mode != Mode::Sub { d.view=d.normal_view(); }
            (s, vec![Effect::RequestDisplay])
        }
        Event::FetchFailed(_) => { d.view=View::Error; (s, vec![Effect::RequestDisplay]) }
        Event::BeginBrowse => {
            let data=d.latest.unwrap(); d.pinned=Some(data); d.mode=Mode::Sub;
            d.view=View::Data { snapshot:data.id,day:data.day,region:data.region }; (s,vec![])
        }
        Event::CancelBrowse => { d.pinned=None; d.mode=Mode::Normal; d.display+=1; d.view=d.normal_view(); (s,vec![Effect::RequestDisplay]) }
        Event::Refresh => { d.fetch_generation+=1; (s,vec![Effect::Fetch]) }
        Event::Midnight(day) => { d.fetch_generation+=1; d.today=*day; if d.mode != Mode::Sub { d.view=d.normal_view(); } (s,vec![Effect::Fetch]) }
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueKind { Video, Control }
struct SendQueue { limit: usize, control_reserve: usize, bytes: usize, q: VecDeque<(QueueKind,usize)> }
impl SendQueue {
    fn push(&mut self, kind: QueueKind, bytes: usize) -> bool {
        let allowed=if kind==QueueKind::Video { self.limit-self.control_reserve } else { self.limit };
        while self.bytes+bytes>allowed {
            let Some(pos)=self.q.iter().position(|(k,_)| *k==QueueKind::Video) else { return false };
            self.bytes-=self.q.remove(pos).unwrap().1;
        }
        self.q.push_back((kind,bytes)); self.bytes+=bytes; true
    }
}

// Two-device scheduler model: a bounded permit pool and alternating ready queues.
struct Scheduler { queues: [VecDeque<u8>; 2], cursor: usize, running: usize, limit: usize }
impl Scheduler {
    fn take(&mut self) -> Option<(usize,u8)> {
        if self.running >= self.limit { return None; }
        for offset in 0..2 {
            let index=(self.cursor+offset)%2;
            if let Some(job)=self.queues[index].pop_front() {
                self.cursor=(index+1)%2; self.running+=1; return Some((index,job));
            }
        }
        None
    }
    fn done(&mut self) { assert!(self.running>0); self.running-=1; }
}

#[cfg(test)] mod tests {
    use super::*;
    fn boot() -> State { step(State::empty(),Event::Hello).0 }
    fn d(s:&State)->&Device { s.device.as_ref().unwrap() }
    #[test] fn power_is_mode_scoped_and_menu_never_standby() {
        let s=boot(); let (s,_) =step(s,Event::Power); assert_eq!(d(&s).mode,Mode::Standby);
        let s=boot(); let (s,_) =step(s,Event::EnterSub); let (s,_) =step(s,Event::Power); assert_eq!(d(&s).mode,Mode::Normal);
        let s=boot(); let (s,_) =step(s,Event::EnterMenu); let (s,_) =step(s,Event::Power); assert_eq!(d(&s).mode,Mode::Normal);
    }
    #[test] fn deletion_incarnation_rejects_old_save_send_ack_even_with_same_revision() {
        let s=boot(); let old=d(&s).token(); let (s,_) =step(s,Event::Delete); let (s,_) =step(s,Event::Hello);
        assert_ne!(d(&s).incarnation,old.incarnation); assert_eq!(d(&s).revision,old.revision);
        let before=s.clone();
        for kind in [Completion::Save,Completion::Send,Completion::Ack] {
            let (next,effects)=step(s.clone(),Event::Completion(old,kind)); assert_eq!(next,before); assert!(effects.is_empty());
        }
        let fresh=d(&s).token(); let (s,_) =step(s,Event::Completion(fresh,Completion::Ack)); assert_eq!(d(&s).accepted_results,1);
    }
    #[test] fn eink_standby_waits_for_noninterruptible_draw_then_holds_image() {
        let mut s=boot(); s.device.as_mut().unwrap().pico.is_eink=true;
        let frame=Frame {token:d(&s).token(),local_owner:d(&s).pico.owner,id:55};
        let (s,_) =step(s,Event::Receive(frame)); let (s,_) =step(s,Event::StartDraw);
        let (s,_) =step(s,Event::Power);
        assert!(d(&s).pico.pending_standby); assert_eq!(d(&s).pico.screen,Screen::Normal);
        let (s,e)=step(s,Event::CompleteDraw);
        assert!(e.is_empty()); assert_eq!(d(&s).pico.screen,Screen::Standby);
        let before=s.clone(); let (s,e)=step(s,Event::Receive(frame)); assert_eq!(s,before); assert!(e.is_empty());
    }
    #[test] fn queued_frame_rejected_by_local_takeover_and_new_server_epoch() {
        let s=boot(); let frame=Frame {token:d(&s).token(),local_owner:d(&s).pico.owner,id:9};
        let (s,_) =step(s,Event::Receive(frame)); assert!(d(&s).pico.pending.is_some());
        let (s,_) =step(s,Event::Local(Screen::Menu)); assert!(d(&s).pico.pending.is_none());
        let before=s.clone(); let (s,e)=step(s,Event::Receive(frame)); assert_eq!(s,before); assert!(e.is_empty());
    }
    #[test] fn noninterruptible_draw_repaints_local_screen_without_old_ack() {
        let s=boot(); let frame=Frame {token:d(&s).token(),local_owner:d(&s).pico.owner,id:12};
        let (s,_) =step(s,Event::Receive(frame)); let (s,_) =step(s,Event::StartDraw);
        let (s,_) =step(s,Event::Local(Screen::Standby)); let (s,e)=step(s,Event::CompleteDraw);
        assert_eq!(e,vec![Effect::RepaintLocal]); assert_eq!(d(&s).pico.screen,Screen::Standby);
    }
    #[test] fn cancelled_connection_completion_and_old_session_are_rejected() {
        let s=boot(); let old=d(&s).token(); let attempt=d(&s).pico.attempt;
        let (s,_) =step(s,Event::ConnectionCancel); let before=s.clone();
        let (s,e)=step(s,Event::AttemptDone(attempt)); assert_eq!(s,before); assert!(e.is_empty());
        let (s,_) =step(s,Event::Hello); let (s,e)=step(s,Event::Completion(old,Completion::Ack)); assert!(e.is_empty()); assert_eq!(d(&s).accepted_results,0);
    }
    #[test] fn disabling_selected_plugin_immediately_reselects_and_clears_submode() {
        let s=boot(); let (s,_) =step(s,Event::EnterSub); let old_display=d(&s).display;
        let (s,_) =step(s,Event::WebOff(0)); assert_eq!(d(&s).selected,Some(1)); assert_eq!(d(&s).mode,Mode::Normal); assert!(d(&s).display>old_display);
        let (s,_) =step(s,Event::ConnectionLost); let (s,_) =step(s,Event::Hello); assert_eq!(d(&s).selected,Some(1));
        let (s,_) =step(s,Event::WebOff(1)); let (s,_) =step(s,Event::WebOff(2)); assert_eq!(d(&s).view,View::Logo);
    }
    #[test] fn older_request_same_incarnation_and_revision_cannot_replace_new_dataset() {
        let s=boot(); let old=d(&s).token(); let (s,_) =step(s,Event::Refresh);
        assert_eq!(d(&s).revision,old.revision);
        let fresh=d(&s).token(); let (s,_) =step(s,Event::FetchOk(fresh,Dataset{id:3,day:1,region:0}));
        let before=s.clone(); let (s,e)=step(s,Event::FetchOk(old,Dataset{id:2,day:1,region:0}));
        assert_eq!(s,before); assert!(e.is_empty());
    }
    #[test] fn region_change_rejects_old_result_and_keeps_dataset_label_atomic() {
        let s=boot(); let old=d(&s).token(); let (s,_) =step(s,Event::SetRegion(2));
        let before=s.clone(); let (s,e)=step(s,Event::FetchOk(old,Dataset{id:7,day:1,region:0})); assert_eq!(s,before); assert!(e.is_empty());
        let token=d(&s).token(); let (s,_) =step(s,Event::FetchOk(token,Dataset{id:8,day:1,region:2}));
        assert_eq!(d(&s).view,View::Data{snapshot:8,day:1,region:2});
    }
    #[test] fn browse_snapshot_survives_refresh_and_midnight_then_cancel_uses_new_today() {
        let s=boot(); let token=d(&s).token(); let (s,_) =step(s,Event::FetchOk(token,Dataset{id:1,day:1,region:0}));
        let (s,_) =step(s,Event::BeginBrowse); let (s,_) =step(s,Event::Midnight(2));
        let token=d(&s).token(); let (s,_) =step(s,Event::FetchOk(token,Dataset{id:2,day:2,region:0}));
        assert_eq!(d(&s).view,View::Data{snapshot:1,day:1,region:0});
        let (s,_) =step(s,Event::CancelBrowse); assert_eq!(d(&s).view,View::Data{snapshot:2,day:2,region:0});
    }
    #[test] fn midnight_without_new_dataset_and_final_failure_do_not_mislabel_old_day() {
        let s=boot(); let token=d(&s).token(); let (s,_) =step(s,Event::FetchOk(token,Dataset{id:1,day:1,region:0}));
        let (s,_) =step(s,Event::Midnight(2)); assert_eq!(d(&s).view,View::Loading);
        let old=d(&s).token(); let (s,_) =step(s,Event::SetRegion(3));
        let before=s.clone(); let (s,_) =step(s,Event::FetchFailed(old)); assert_eq!(s,before);
        let current=d(&s).token(); let (s,_) =step(s,Event::FetchFailed(current)); assert_eq!(d(&s).view,View::Error);
    }
    #[test] fn byte_bounded_queue_drops_stale_video_and_reserves_control() {
        let mut q=SendQueue {limit:100,control_reserve:20,bytes:0,q:VecDeque::new()};
        assert!(q.push(QueueKind::Video,60)); assert!(q.push(QueueKind::Video,60));
        assert_eq!(q.q.iter().filter(|(k,_)| *k==QueueKind::Video).count(),1);
        assert!(q.push(QueueKind::Control,20)); assert_eq!(q.bytes,80);
        assert!(!q.push(QueueKind::Video,100)); assert!(q.bytes<=q.limit);
    }
    #[test] fn worker_permits_and_round_robin_prevent_busy_device_starvation() {
        let mut q=Scheduler {queues:[VecDeque::from([1,2,3]),VecDeque::from([9])],cursor:0,running:0,limit:2};
        assert_eq!(q.take(),Some((0,1))); assert_eq!(q.take(),Some((1,9)));
        assert_eq!(q.take(),None); assert_eq!(q.running,2);
        q.done(); assert_eq!(q.take(),Some((0,2)));
    }
    #[test] fn web_cannot_enable_unsupported_firmware_capability() {
        let mut s=boot(); s.device.as_mut().unwrap().firmware_support[2]=false;
        let (s,_) =step(s,Event::WebOff(2)); let before=s.clone();
        let (s,e)=step(s,Event::WebOn(2)); assert_eq!(s,before); assert!(e.is_empty());
        let mut s=s; s.device.as_mut().unwrap().web_profile[1]=false;
        let (s,_) =step(s,Event::WebOff(1)); let before=s.clone();
        let (s,e)=step(s,Event::WebOn(1)); assert_eq!(s,before); assert!(e.is_empty());
    }
    #[test] fn reordered_old_fetch_after_new_registration_has_no_effect() {
        let s=boot(); let old=d(&s).token(); let (s,_) =step(s,Event::Delete); let (s,_) =step(s,Event::Hello);
        let fresh=d(&s).token(); let (s,_) =step(s,Event::FetchOk(fresh,Dataset{id:2,day:1,region:0}));
        let before=s.clone(); let (s,e)=step(s,Event::FetchOk(old,Dataset{id:1,day:1,region:0}));
        assert_eq!(s,before); assert!(e.is_empty());
    }
}
