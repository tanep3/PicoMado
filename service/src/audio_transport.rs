//! BT-AUDIO-20261107: bounded PCM sender credit and kind-24 codec, without IO.
pub const AUDIO_CHUNK_KIND: u8 = 24;
pub const PCM_FRAME_BYTES: usize = 4;
pub const MAX_CHUNK_FRAMES: usize = 256;
pub const RING_FRAMES: u64 = 2048;
const HEADER_BYTES: usize = 46; // length + version/kind + five big-endian u64s

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamIdentity {
    pub incarnation: u64,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub stream_id: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError {
    InvalidLength,
    InvalidEnvelope,
    Exhausted,
    Inactive,
    NonContiguous,
    NoCredit,
    InvalidCredit,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedChunk<'a> {
    pub identity: StreamIdentity,
    pub first_frame: u64,
    /// Signed 16-bit little-endian interleaved left/right samples, unmodified.
    pub pcm: &'a [u8],
}
fn validate_pcm(pcm: &[u8], first_frame: u64) -> Result<(), AudioError> {
    if pcm.is_empty()
        || pcm.len() > MAX_CHUNK_FRAMES * PCM_FRAME_BYTES
        || pcm.len() % PCM_FRAME_BYTES != 0
    {
        return Err(AudioError::InvalidLength);
    }
    first_frame
        .checked_add((pcm.len() / PCM_FRAME_BYTES) as u64)
        .ok_or(AudioError::Exhausted)?;
    Ok(())
}
pub fn encode_chunk(
    identity: StreamIdentity,
    first_frame: u64,
    pcm: &[u8],
) -> Result<Vec<u8>, AudioError> {
    validate_pcm(pcm, first_frame)?;
    if identity.stream_id == 0 {
        return Err(AudioError::InvalidEnvelope);
    }
    let mut output = Vec::with_capacity(HEADER_BYTES + pcm.len());
    output.extend_from_slice(&((HEADER_BYTES - 4 + pcm.len()) as u32).to_be_bytes());
    output.extend_from_slice(&[1, AUDIO_CHUNK_KIND]);
    for value in [
        identity.incarnation,
        identity.session_epoch,
        identity.display_epoch,
        identity.stream_id,
        first_frame,
    ] {
        output.extend_from_slice(&value.to_be_bytes());
    }
    output.extend_from_slice(pcm);
    Ok(output)
}
/// Decode exactly one complete application frame; outer transport owns fragmentation.
pub fn decode_chunk(bytes: &[u8]) -> Result<DecodedChunk<'_>, AudioError> {
    if bytes.len() < HEADER_BYTES || bytes.len() > HEADER_BYTES + MAX_CHUNK_FRAMES * PCM_FRAME_BYTES
    {
        return Err(AudioError::InvalidLength);
    }
    let length = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    if length != bytes.len() - 4 || bytes[4] != 1 || bytes[5] != AUDIO_CHUNK_KIND {
        return Err(AudioError::InvalidEnvelope);
    }
    let field =
        |index: usize| u64::from_be_bytes(bytes[6 + index * 8..14 + index * 8].try_into().unwrap());
    let identity = StreamIdentity {
        incarnation: field(0),
        session_epoch: field(1),
        display_epoch: field(2),
        stream_id: field(3),
    };
    if identity.stream_id == 0 {
        return Err(AudioError::InvalidEnvelope);
    }
    let first_frame = field(4);
    let pcm = &bytes[HEADER_BYTES..];
    validate_pcm(pcm, first_frame)?;
    Ok(DecodedChunk {
        identity,
        first_frame,
        pcm,
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stream {
    identity: StreamIdentity,
    start: u64,
    sent: u64,
    accepted: u64,
    consumed: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSender {
    incarnation: u64,
    session_epoch: u64,
    last_stream_id: u64,
    active: Option<Stream>,
}
impl AudioSender {
    pub fn new(incarnation: u64, session_epoch: u64) -> Self {
        Self {
            incarnation,
            session_epoch,
            last_stream_id: 0,
            active: None,
        }
    }
    /// Allocate a new stream even for a repeat start at the same position.
    /// Retain this allocator for the entire transport session, including across stop.
    pub fn start(
        &mut self,
        display_epoch: u64,
        start_frame: u64,
    ) -> Result<StreamIdentity, AudioError> {
        let stream_id = self
            .last_stream_id
            .checked_add(1)
            .ok_or(AudioError::Exhausted)?;
        let identity = StreamIdentity {
            incarnation: self.incarnation,
            session_epoch: self.session_epoch,
            display_epoch,
            stream_id,
        };
        self.active = Some(Stream {
            identity,
            start: start_frame,
            sent: start_frame,
            accepted: start_frame,
            consumed: start_frame,
        });
        self.last_stream_id = stream_id;
        Ok(identity)
    }
    pub fn stop(&mut self) {
        self.active = None;
    }
    pub fn identity(&self) -> Option<StreamIdentity> {
        self.active.as_ref().map(|s| s.identity)
    }
    pub fn sent_end(&self) -> Option<u64> {
        self.active.as_ref().map(|s| s.sent)
    }
    pub fn accepted_end(&self) -> Option<u64> {
        self.active.as_ref().map(|s| s.accepted)
    }
    pub fn consumed_end(&self) -> Option<u64> {
        self.active.as_ref().map(|s| s.consumed)
    }
    pub fn allowance(&self) -> usize {
        self.active.as_ref().map_or(0, |s| {
            (RING_FRAMES.saturating_sub(s.sent - s.consumed))
                .min(MAX_CHUNK_FRAMES as u64)
                .min(u64::MAX - s.sent) as usize
        })
    }
    /// Record only a successfully enqueued whole chunk; caller must serialize enqueue/record.
    /// On ambiguous/partial transport failure stop this stream rather than reclaim credit.
    pub fn record_sent(&mut self, first_frame: u64, frames: usize) -> Result<(), AudioError> {
        let s = self.active.as_ref().ok_or(AudioError::Inactive)?;
        if first_frame != s.sent {
            return Err(AudioError::NonContiguous);
        }
        if frames == 0 || frames > MAX_CHUNK_FRAMES {
            return Err(AudioError::InvalidLength);
        }
        if frames > self.allowance() {
            return Err(AudioError::NoCredit);
        }
        self.active.as_mut().unwrap().sent = first_frame
            .checked_add(frames as u64)
            .ok_or(AudioError::Exhausted)?;
        Ok(())
    }
    /// Cumulative absolute frame positions. Duplicate/reordered snapshots cannot create capacity.
    /// A stale stream is ignored before inspecting its counters.
    pub fn apply_credit(
        &mut self,
        identity: StreamIdentity,
        accepted: u64,
        consumed: u64,
    ) -> Result<bool, AudioError> {
        let Some(s) = self.active.as_mut().filter(|s| s.identity == identity) else {
            return Ok(false);
        };
        if consumed < s.start || accepted < s.start || consumed > accepted || accepted > s.sent {
            return Err(AudioError::InvalidCredit);
        }
        if accepted < s.accepted || consumed < s.consumed {
            return Ok(false);
        }
        let changed = accepted != s.accepted || consumed != s.consumed;
        s.accepted = accepted;
        s.consumed = consumed;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_credit_duplicate_reordered_and_underflow() {
        let mut sender = AudioSender::new(1, 2);
        let id = sender.start(3, 100).unwrap();
        for n in 0..8 {
            assert_eq!(sender.allowance(), 256);
            sender.record_sent(100 + n * 256, 256).unwrap();
        }
        assert_eq!(sender.allowance(), 0);
        assert_eq!(sender.record_sent(2148, 1), Err(AudioError::NoCredit));
        assert!(sender.apply_credit(id, 2148, 356).unwrap());
        assert_eq!(sender.allowance(), 256);
        sender.record_sent(2148, 256).unwrap();
        assert_eq!(sender.allowance(), 0);
        assert!(!sender.apply_credit(id, 2148, 356).unwrap());
        assert_eq!(sender.allowance(), 0);
        assert!(!sender.apply_credit(id, 1124, 100).unwrap());
        assert_eq!(sender.allowance(), 0);
        assert_eq!(
            sender.apply_credit(id, 2405, 356),
            Err(AudioError::InvalidCredit)
        );
        assert_eq!(
            sender.apply_credit(id, 2404, 2405),
            Err(AudioError::InvalidCredit)
        );
        assert_eq!(
            sender.apply_credit(id, 2404, 99),
            Err(AudioError::InvalidCredit)
        );
        assert!(sender.apply_credit(id, 2404, 2404).unwrap()); // all consumed/underflow, bounded refill
        assert_eq!(sender.allowance(), 256);
    }
    #[test]
    fn replacement_stop_and_overflow() {
        let mut sender = AudioSender::new(1, 2);
        let old = sender.start(3, 0).unwrap();
        sender.record_sent(0, 1).unwrap();
        sender.stop();
        assert_eq!(sender.allowance(), 0);
        let id = sender.start(4, u64::MAX - 1).unwrap();
        assert_eq!(id.stream_id, old.stream_id + 1);
        assert!(!sender.apply_credit(old, u64::MAX, u64::MAX).unwrap());
        assert_eq!(sender.allowance(), 1);
        sender.record_sent(u64::MAX - 1, 1).unwrap();
        assert_eq!(sender.allowance(), 0);
        assert_eq!(sender.record_sent(0, 1), Err(AudioError::NonContiguous));
        sender.last_stream_id = u64::MAX;
        let before = sender.clone();
        assert_eq!(sender.start(5, 0), Err(AudioError::Exhausted));
        assert_eq!(sender, before);
    }
    #[test]
    fn codec_exact_layout_and_rejections() {
        let id = StreamIdentity {
            incarnation: 1,
            session_epoch: 2,
            display_epoch: 3,
            stream_id: 4,
        };
        let pcm = [0x00, 0x80, 0xff, 0x7f];
        let bytes = encode_chunk(id, 5, &pcm).unwrap();
        assert_eq!(&bytes[..6], &[0, 0, 0, 46, 1, 24]);
        assert_eq!(&bytes[30..38], &4u64.to_be_bytes());
        let decoded = decode_chunk(&bytes).unwrap();
        assert_eq!(decoded.identity, id);
        assert_eq!(decoded.first_frame, 5);
        assert_eq!(decoded.pcm, pcm);
        for n in 0..bytes.len() {
            assert!(decode_chunk(&bytes[..n]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_chunk(&extra).is_err());
        for index in [3, 4, 5] {
            let mut bad = bytes.clone();
            bad[index] ^= 1;
            assert!(decode_chunk(&bad).is_err());
        }
        assert!(encode_chunk(id, 0, &[]).is_err());
        assert!(encode_chunk(id, 0, &[0; 3]).is_err());
        assert!(encode_chunk(id, 0, &[0; 1028]).is_err());
        assert!(encode_chunk(id, u64::MAX, &pcm).is_err());
        assert_eq!(
            decode_chunk(&encode_chunk(id, 0, &[0; 1024]).unwrap())
                .unwrap()
                .pcm
                .len(),
            1024
        );
    }
}
