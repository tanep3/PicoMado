//! Disposable boundary facts. Pure Rules never call this module. Producers do
//! no IO and never wait for the consumer; capture loss has its own sequence.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum Component { Input=1, Api=2, Network=3, Frame=4, Display=5, State=6, Effect=7 }
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase { Received=1, Dispatched=2, Pending=3, Completed=4, Rejected=5, Error=6, Cancelled=7, Committed=8 }
#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub sequence:u64, pub us:u64, pub component:Component, pub phase:Phase,
    pub operation:u64, pub parent:u64, pub revision:u64, pub argument:u64,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Status { pub sequence:u64, pub dropped:u64, pub enabled:bool }

#[cfg(feature="boundary-observation")]
mod enabled {
    use super::*;
    use std::{collections::VecDeque, sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU64,Ordering}},time::Instant};
    pub const CAPACITY:usize=256;
    struct Inner {
        start:Instant, enabled:AtomicBool, sequence:AtomicU64, dropped:AtomicU64,
        queue:Mutex<VecDeque<Fact>>,
    }
    #[derive(Clone)]
    pub struct Source(Arc<Inner>);
    impl Default for Source {
        fn default()->Self { Self(Arc::new(Inner {
            start:Instant::now(),enabled:AtomicBool::new(false),sequence:AtomicU64::new(0),
            dropped:AtomicU64::new(0),queue:Mutex::new(VecDeque::with_capacity(CAPACITY)),
        })) }
    }
    impl Source {
        pub fn enable(&self,on:bool){self.0.enabled.store(on,Ordering::Relaxed);}
        pub fn enabled(&self)->bool {self.0.enabled.load(Ordering::Relaxed)}
        pub fn record(&self,component:Component,phase:Phase,operation:u64,parent:u64,revision:u64,argument:u64)->u64 {
            if !self.enabled(){return 0;}
            let Ok(old)=self.0.sequence.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|v.checked_add(1)) else {
                self.enable(false);return 0;
            };
            let sequence=old+1;
            let fact=Fact { sequence,us:self.0.start.elapsed().as_micros().min(u64::MAX as u128) as u64,
                component,phase,operation:if operation==0 {sequence}else{operation},parent,revision,argument };
            if let Ok(mut queue)=self.0.queue.try_lock(){
                if queue.len()<CAPACITY {queue.push_back(fact);return sequence;}
            }
            let _=self.0.dropped.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|Some(v.saturating_add(1)));
            sequence
        }
        /// Cold consumer only. Call outside the shared App lock.
        pub fn drain(&self)->Vec<Fact>{self.0.queue.lock().expect("observation consumer").drain(..).collect()}
        pub fn status(&self)->Status {Status {sequence:self.0.sequence.load(Ordering::Relaxed),dropped:self.0.dropped.load(Ordering::Relaxed),enabled:self.enabled()}}
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test] fn saturation_and_contention_do_not_block_or_hide_gaps(){
            let s=Source::default();assert_eq!(s.record(Component::Api,Phase::Received,0,0,0,0),0);
            s.enable(true);
            for _ in 0..CAPACITY+1{s.record(Component::Api,Phase::Received,0,0,0,0);}
            assert_eq!(s.status().dropped,1);assert_eq!(s.drain().len(),CAPACITY);
            let lock=s.0.queue.lock().unwrap();s.record(Component::Api,Phase::Received,0,0,0,0);drop(lock);
            assert_eq!(s.status().dropped,2);
            let last=s.record(Component::Api,Phase::Completed,7,3,9,4);
            let row=s.drain().pop().unwrap();assert_eq!(row.sequence,last);assert_eq!(row.parent,3);
            s.enable(false);assert_eq!(s.status().sequence,last);
        }
    }
}
#[cfg(feature="boundary-observation")]
pub use enabled::Source;
#[cfg(not(feature="boundary-observation"))]
#[derive(Clone,Default)]
pub struct Source;
#[cfg(not(feature="boundary-observation"))]
impl Source {
    pub fn enable(&self,_on:bool){}
    pub fn enabled(&self)->bool{false}
    pub fn record(&self,_c:Component,_p:Phase,_o:u64,_r:u64,_v:u64,_a:u64)->u64{0}
    pub fn drain(&self)->Vec<Fact>{Vec::new()}
    pub fn status(&self)->Status{Status::default()}
}
