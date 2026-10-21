//! Pure provisional readiness relation. Final capture still requires gate/COPY proof.
use super::values::{PicoSendAttemptOrdinal,HostProcessedOrdinal};
pub struct BoundStream { _connection:u64, _session:u64 }
impl BoundStream {
    pub fn from_locked_state(connection:u64,session:u64)->Option<Self>{
        if connection==0||session==0{None}else{Some(Self{_connection:connection,_session:session})}
    }
}
pub fn same_processed_prefix_candidate(sent:PicoSendAttemptOrdinal,processed:HostProcessedOrdinal,_binding:BoundStream)->bool{
    u64::from(sent.wire())==processed.wire()
}
