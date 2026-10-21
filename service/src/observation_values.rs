// Generated; edit contracts/observation-domain.json. 77898d6071d480b36086ae67c327c7b12e59d5e7870cdbfe733a32bec5296284
#![allow(dead_code, non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PicoSendAttemptOrdinal(u32);
impl PicoSendAttemptOrdinal { pub fn from_wire(v:u32)->Self{Self(v)} pub fn wire(self)->u32{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostDecodedOrdinal(u64);
impl HostDecodedOrdinal { pub fn from_wire(v:u64)->Self{Self(v)} pub fn wire(self)->u64{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostProcessedOrdinal(u64);
impl HostProcessedOrdinal { pub fn from_wire(v:u64)->Self{Self(v)} pub fn wire(self)->u64{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PicoMicros(u64);
impl PicoMicros { pub fn from_wire(v:u64)->Self{Self(v)} pub fn wire(self)->u64{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostProbeMicros(u64);
impl HostProbeMicros { pub fn from_wire(v:u64)->Self{Self(v)} pub fn wire(self)->u64{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PiMonotonicNs(u64);
impl PiMonotonicNs { pub fn from_wire(v:u64)->Self{Self(v)} pub fn wire(self)->u64{self.0} }
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all="snake_case")]
pub enum PicoState {IDLE,ARMED,RECORDING,FROZEN,PREPARED}
#[derive(Clone, Copy, Debug)]
pub enum PicoEvent {ARM_CONNECTED,ARM_DETACHED,CLOSE_GATE,FREEZE,OPEN_GATE,RELEASE,START}
pub struct PicoWindow { state:PicoState }
impl PicoWindow { pub fn new()->Self{Self{state:PicoState::IDLE}} pub fn state(&self)->PicoState{self.state}
pub fn apply(&mut self,event:PicoEvent)->Result<(), &'static str>{self.state=match(self.state,event){
(PicoState::IDLE,PicoEvent::ARM_CONNECTED)=>PicoState::PREPARED,
(PicoState::IDLE,PicoEvent::ARM_DETACHED)=>PicoState::ARMED,
(PicoState::PREPARED,PicoEvent::CLOSE_GATE)=>PicoState::ARMED,
(PicoState::ARMED,PicoEvent::OPEN_GATE)=>PicoState::PREPARED,
(PicoState::ARMED,PicoEvent::START)=>PicoState::RECORDING,
(PicoState::PREPARED,PicoEvent::FREEZE)=>PicoState::FROZEN,
(PicoState::ARMED,PicoEvent::FREEZE)=>PicoState::FROZEN,
(PicoState::RECORDING,PicoEvent::FREEZE)=>PicoState::FROZEN,
(PicoState::FROZEN,PicoEvent::RELEASE)=>PicoState::IDLE,
_=>return Err("invalid transition"),};Ok(())}}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all="snake_case")]
pub enum HostState {Idle,Armed,Recording,Frozen}
#[derive(Clone, Copy, Debug)]
pub enum HostEvent {Arm,Freeze,Release,Start}
pub struct HostWindow { state:HostState }
impl HostWindow { pub fn new()->Self{Self{state:HostState::Idle}} pub fn state(&self)->HostState{self.state}
pub fn apply(&mut self,event:HostEvent)->Result<(), &'static str>{self.state=match(self.state,event){
(HostState::Idle,HostEvent::Arm)=>HostState::Armed,
(HostState::Armed,HostEvent::Start)=>HostState::Recording,
(HostState::Armed,HostEvent::Freeze)=>HostState::Frozen,
(HostState::Recording,HostEvent::Freeze)=>HostState::Frozen,
(HostState::Frozen,HostEvent::Release)=>HostState::Idle,
_=>return Err("invalid transition"),};Ok(())}}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all="snake_case")]
pub enum EvidenceState {Draft,Assessed,Confirmed,Rejected}
#[derive(Clone, Copy, Debug)]
pub enum EvidenceEvent {Assess,Confirm,Reject}
pub struct EvidenceWindow { state:EvidenceState }
impl EvidenceWindow { pub fn new()->Self{Self{state:EvidenceState::Draft}} pub fn state(&self)->EvidenceState{self.state}
pub fn apply(&mut self,event:EvidenceEvent)->Result<(), &'static str>{self.state=match(self.state,event){
(EvidenceState::Draft,EvidenceEvent::Assess)=>EvidenceState::Assessed,
(EvidenceState::Assessed,EvidenceEvent::Confirm)=>EvidenceState::Confirmed,
(EvidenceState::Assessed,EvidenceEvent::Reject)=>EvidenceState::Rejected,
_=>return Err("invalid transition"),};Ok(())}}
