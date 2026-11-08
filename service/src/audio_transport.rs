//! BT-AUDIO-20261107: bounded PCM/SBC sender credit and codecs, without IO.
pub const AUDIO_CHUNK_KIND: u8 = 24;
pub const PCM_FRAME_BYTES: usize = 4;
pub const MAX_CHUNK_FRAMES: usize = 256;
pub const RING_FRAMES: u64 = 2048;
pub const LARGE_RING_FRAMES: u64 = 4096;
pub const LARGE_RING_CAPABILITY: &str = "audio-buffer-4096-v1";
pub const SBC_CAPABILITY: &str = "audio-sbc-v1";
pub const SBC_CHUNK_KIND: u8 = 30;
pub const SBC_RING_FRAMES: u64 = 16384;
pub const SBC_BLOCK_FRAMES: usize = 128;
pub const SBC_MAX_CHUNK_FRAMES: usize = 1024;
const HEADER_BYTES: usize = 46; // length + version/kind + five big-endian u64s

/// Attenuate a disposable s16le stereo packet. Source assets are never modified.
/// Squared control: 10 -> 0.01 gain (-40 dB), 100 -> unity, 0 -> exact silence.
pub fn apply_volume(pcm: &mut [u8], volume: u8) -> Result<(), AudioError> {
    if volume > 100 {
        return Err(AudioError::InvalidVolume);
    }
    if pcm.len() % PCM_FRAME_BYTES != 0 {
        return Err(AudioError::InvalidLength);
    }
    if volume == 0 {
        pcm.fill(0);
        return Ok(());
    }
    if volume == 100 {
        return Ok(());
    }
    let gain = i32::from(volume) * i32::from(volume);
    for sample in pcm.chunks_exact_mut(2) {
        let value = i32::from(i16::from_le_bytes([sample[0], sample[1]]));
        let attenuated =
            (value * gain / 10000).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        sample.copy_from_slice(&attenuated.to_le_bytes());
    }
    Ok(())
}

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
    InvalidCapacity,
    InvalidVolume,
    InvalidConfiguration,
    ConflictingReady,
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
/// Source reads remain canonical PCM and cannot expand a declared asset duration.
pub fn validate_source_read(offset:u64,requested:usize,declared_end:Option<u64>,bytes:usize)->Result<u64,AudioError> {
    if bytes%PCM_FRAME_BYTES!=0 || bytes/PCM_FRAME_BYTES>requested {return Err(AudioError::InvalidLength);}
    let end=offset.checked_add((bytes/PCM_FRAME_BYTES) as u64).ok_or(AudioError::Exhausted)?;
    if declared_end.is_some_and(|n|end>n) {return Err(AudioError::InvalidLength);}
    Ok(end)
}
/// Untrusted Ready facts. Admission belongs to AudioSender, never the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbcReady {
    pub identity: StreamIdentity,
    pub first_frame: u64,
    pub codec_epoch: u64,
    pub rate: u32,
    pub channels: u8,
    pub blocks: u8,
    pub subbands: u8,
    pub channel_mode: u8,
    pub allocation: u8,
    pub bitpool: u8,
    pub capacity_frames: u64,
}
impl SbcReady {
    pub fn valid(&self) -> bool {
        self.codec_epoch != 0 && self.rate == 48000 && self.channels == 2
            && self.blocks == 16 && self.subbands == 8
            && matches!(self.channel_mode, 2 | 3) && self.allocation <= 1
            && (2..=53).contains(&self.bitpool) && self.capacity_frames == SBC_RING_FRAMES
    }
    pub fn frame_bytes(&self) -> usize { 12 + usize::from(self.channel_mode == 3) + 2 * usize::from(self.bitpool) }
    fn header(&self) -> u8 { 0xf1 | (self.channel_mode << 2) | (self.allocation << 1) }
}
fn crc8(bytes: impl Iterator<Item=u8>) -> u8 {
    let mut crc=0x0fu8;
    for byte in bytes {
        for bit in (0..8).rev() {
            let feedback=((crc >> 7) ^ (byte >> bit)) & 1;
            crc <<= 1;
            if feedback != 0 { crc ^= 0x1d; }
        }
    }
    crc
}
/// Validate every protected header before publishing a bounded kind-30 chunk.
pub fn encode_sbc_chunk(ready: SbcReady, first_frame: u64, sbc: &[u8]) -> Result<Vec<u8>, AudioError> {
    if !ready.valid() || ready.identity.stream_id == 0 { return Err(AudioError::InvalidConfiguration); }
    let size=ready.frame_bytes();
    if sbc.is_empty() || sbc.len()%size != 0 || sbc.len()/size > 8 { return Err(AudioError::InvalidLength); }
    first_frame.checked_add((sbc.len()/size*SBC_BLOCK_FRAMES) as u64).ok_or(AudioError::Exhausted)?;
    for frame in sbc.chunks_exact(size) {
        let protected_end=12 + usize::from(ready.channel_mode == 3);
        if frame[0] != 0x9c || frame[1] != ready.header() || frame[2] != ready.bitpool
            || (ready.channel_mode == 3 && frame[4]&1 != 0)
            || frame[3] != crc8(frame[1..3].iter().chain(frame[4..protected_end].iter()).copied()) {
            return Err(AudioError::InvalidConfiguration);
        }
    }
    let mut out=Vec::with_capacity(HEADER_BYTES+sbc.len());
    out.extend_from_slice(&((HEADER_BYTES-4+sbc.len()) as u32).to_be_bytes());
    out.extend_from_slice(&[1,SBC_CHUNK_KIND]);
    for field in [ready.identity.incarnation,ready.identity.session_epoch,ready.identity.display_epoch,ready.identity.stream_id,first_frame] { out.extend_from_slice(&field.to_be_bytes()); }
    out.extend_from_slice(sbc);
    Ok(out)
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum StreamFormat { Pcm, WaitingSbc, Sbc(SbcReady) }
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stream {
    identity: StreamIdentity,
    start: u64,
    sent: u64,
    accepted: u64,
    consumed: u64,
    format: StreamFormat,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSender {
    incarnation: u64,
    session_epoch: u64,
    last_stream_id: u64,
    capacity: u64,
    active: Option<Stream>,
}
impl AudioSender {
    pub fn new(incarnation: u64, session_epoch: u64) -> Self {
        Self {
            incarnation,
            session_epoch,
            last_stream_id: 0,
            capacity: RING_FRAMES,
            active: None,
        }
    }
    /// Negotiated capacity changes only between streams. Legacy peers retain 2048.
    pub fn set_capacity(&mut self, frames: u64) -> Result<(), AudioError> {
        if self.active.is_some() || !matches!(frames, RING_FRAMES | LARGE_RING_FRAMES) {
            return Err(AudioError::InvalidCapacity);
        }
        self.capacity = frames;
        Ok(())
    }
    pub fn capacity(&self) -> u64 {
        self.capacity
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
        if self.capacity == SBC_RING_FRAMES { self.capacity = RING_FRAMES; }
        self.active = Some(Stream {
            identity,
            start: start_frame,
            sent: start_frame,
            accepted: start_frame,
            consumed: start_frame,
            format: StreamFormat::Pcm,
        });
        self.last_stream_id = stream_id;
        Ok(identity)
    }
    /// Start only after current-session capability negotiation selected SBC.
    pub fn start_sbc(&mut self, display_epoch:u64, start_frame:u64) -> Result<StreamIdentity,AudioError> {
        let identity=self.start(display_epoch,start_frame)?;
        self.capacity=SBC_RING_FRAMES;
        self.active.as_mut().unwrap().format=StreamFormat::WaitingSbc;
        Ok(identity)
    }
    pub fn waiting_ready(&self) -> bool { self.active.as_ref().is_some_and(|s| matches!(s.format,StreamFormat::WaitingSbc)) }
    pub fn sbc_ready(&self) -> Option<SbcReady> { self.active.as_ref().and_then(|s| if let StreamFormat::Sbc(r)=s.format {Some(r)} else {None}) }
    /// Stale identities are ignored before validation. Duplicate exact Ready is harmless.
    pub fn admit_ready(&mut self, ready:SbcReady) -> Result<bool,AudioError> {
        let Some(s)=self.active.as_mut().filter(|s|s.identity==ready.identity) else { return Ok(false); };
        match s.format {
            StreamFormat::Pcm=>Ok(false),
            StreamFormat::Sbc(old)=>if old==ready {Ok(false)} else {Err(AudioError::ConflictingReady)},
            StreamFormat::WaitingSbc=>{
                if !ready.valid() || ready.first_frame!=s.start {return Err(AudioError::InvalidConfiguration);}
                s.format=StreamFormat::Sbc(ready); Ok(true)
            }
        }
    }
    pub fn stop(&mut self) {
        self.active = None;
        if self.capacity == SBC_RING_FRAMES { self.capacity=RING_FRAMES; }
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
            let (limit,alignment)=match s.format {StreamFormat::Pcm=>(MAX_CHUNK_FRAMES,1),StreamFormat::WaitingSbc=>return 0,StreamFormat::Sbc(_)=>(SBC_MAX_CHUNK_FRAMES,SBC_BLOCK_FRAMES)};
            let count=(self.capacity.saturating_sub(s.sent - s.consumed))
                .min(limit as u64).min(u64::MAX-s.sent) as usize;
            count/alignment*alignment
        })
    }
    /// Record only a successfully enqueued whole chunk; caller must serialize enqueue/record.
    /// On ambiguous/partial transport failure stop this stream rather than reclaim credit.
    pub fn record_sent(&mut self, first_frame: u64, frames: usize) -> Result<(), AudioError> {
        let s = self.active.as_ref().ok_or(AudioError::Inactive)?;
        if first_frame != s.sent {
            return Err(AudioError::NonContiguous);
        }
        let (limit,alignment)=match s.format {StreamFormat::Pcm=>(MAX_CHUNK_FRAMES,1),StreamFormat::WaitingSbc=>return Err(AudioError::Inactive),StreamFormat::Sbc(_)=>(SBC_MAX_CHUNK_FRAMES,SBC_BLOCK_FRAMES)};
        if frames == 0 || frames > limit || frames%alignment != 0 {
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
        if consumed < s.start
            || accepted < s.start
            || consumed > accepted
            || accepted > s.sent
            || accepted - consumed > self.capacity
            || (matches!(s.format,StreamFormat::Sbc(_)) && ((accepted-s.start)%128!=0 || (consumed-s.start)%128!=0))
        {
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
    fn volume_is_exact_silence_squared_attenuation_and_unity() {
        let source: Vec<u8> = [i16::MIN, -10000, -1, 0, 1, 10000, i16::MAX, 1234]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect();
        let mut muted = source.clone();
        apply_volume(&mut muted, 0).unwrap();
        assert!(muted.iter().all(|b| *b == 0));
        let mut unity = source.clone();
        apply_volume(&mut unity, 100).unwrap();
        assert_eq!(unity, source);
        let mut quiet = source.clone();
        apply_volume(&mut quiet, 10).unwrap();
        let values: Vec<_> = quiet
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(values, vec![-327, -100, 0, 0, 0, 100, 327, 12]);
        for volume in 0..=100 {
            let mut bytes = source.clone();
            apply_volume(&mut bytes, volume).unwrap();
            for (a, b) in bytes.chunks_exact(2).zip(source.chunks_exact(2)) {
                assert!(
                    i32::from(i16::from_le_bytes([a[0], a[1]])).abs()
                        <= i32::from(i16::from_le_bytes([b[0], b[1]])).abs()
                );
            }
        }
        let mut invalid = source.clone();
        assert_eq!(
            apply_volume(&mut invalid, 101),
            Err(AudioError::InvalidVolume)
        );
        assert_eq!(invalid, source);
        assert_eq!(
            apply_volume(&mut [1, 2, 3], 10),
            Err(AudioError::InvalidLength)
        );
    }
    #[test]
    fn negotiated_capacity_is_bounded_and_changes_only_between_streams() {
        let mut sender = AudioSender::new(1, 2);
        assert_eq!(sender.capacity(), 2048);
        assert_eq!(sender.set_capacity(8192), Err(AudioError::InvalidCapacity));
        sender.set_capacity(4096).unwrap();
        let id = sender.start(3, 0).unwrap();
        assert_eq!(sender.set_capacity(2048), Err(AudioError::InvalidCapacity));
        for n in 0..16 {
            assert_eq!(sender.allowance(), 256);
            sender.record_sent(n * 256, 256).unwrap();
        }
        assert_eq!(sender.allowance(), 0);
        assert_eq!(sender.record_sent(4096, 1), Err(AudioError::NoCredit));
        sender.apply_credit(id, 4096, 256).unwrap();
        sender.record_sent(4096, 256).unwrap();
        assert!(!sender.apply_credit(id, 4096, 256).unwrap());
        assert_eq!(sender.allowance(), 0);
        assert_eq!(
            sender.apply_credit(id, 4352, 0),
            Err(AudioError::InvalidCredit)
        );
        sender.stop();
        sender.set_capacity(2048).unwrap();
        let fresh = sender.start(4, 0).unwrap();
        assert_ne!(id, fresh);
        for n in 0..8 {
            sender.record_sent(n * 256, 256).unwrap();
        }
        assert_eq!(sender.allowance(), 0);
        assert!(!sender.apply_credit(id, 4096, 4096).unwrap());
    }
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

#[cfg(test)]
mod sbc_tests {
    use super::*;
    fn ready(id:StreamIdentity,start:u64)->SbcReady {SbcReady{identity:id,first_frame:start,codec_epoch:1,rate:48000,channels:2,blocks:16,subbands:8,channel_mode:3,allocation:0,bitpool:32,capacity_frames:16384}}
    fn frame(r:SbcReady)->Vec<u8>{let mut f=vec![0;r.frame_bytes()];f[0]=0x9c;f[1]=r.header();f[2]=r.bitpool;let end=12+usize::from(r.channel_mode==3);f[3]=crc8(f[1..3].iter().chain(f[4..end].iter()).copied());f}
    #[test]
    fn handshake_pins_tuple_identity_and_bounds_credit_in_canonical_units(){
        let mut s=AudioSender::new(1,2);assert_eq!(s.set_capacity(SBC_RING_FRAMES),Err(AudioError::InvalidCapacity));
        let id=s.start_sbc(3,7).unwrap();let r=ready(id,7);
        assert_eq!(s.capacity(),16384);assert!(s.waiting_ready());assert_eq!(s.allowance(),0);
        assert_eq!(s.record_sent(7,128),Err(AudioError::Inactive));
        let mut stale=r;stale.identity.stream_id+=1;stale.bitpool=255;assert_eq!(s.admit_ready(stale),Ok(false));assert!(s.waiting_ready());
        assert_eq!(s.admit_ready(r),Ok(true));assert_eq!(s.admit_ready(r),Ok(false));
        assert_eq!(s.record_sent(7,127),Err(AudioError::InvalidLength));
        for n in 0..16 {assert_eq!(s.allowance(),1024);s.record_sent(7+n*1024,1024).unwrap();}
        assert_eq!(s.allowance(),0);s.apply_credit(id,16391,135).unwrap();assert_eq!(s.allowance(),128);
        s.record_sent(16391,128).unwrap();assert_eq!(s.allowance(),0);
        assert!(!s.apply_credit(id,16391,135).unwrap());
        let mut conflict=r;conflict.codec_epoch+=1;assert_eq!(s.admit_ready(conflict),Err(AudioError::ConflictingReady));
        assert_eq!(s.sbc_ready(),Some(r));s.stop();assert_eq!(s.admit_ready(r),Ok(false));
        let next=s.start(4,1).unwrap();assert_ne!(next,id);assert_eq!(s.capacity(),2048);assert_eq!(s.allowance(),256);
    }
    #[test]
    fn source_bounds_reject_extension_and_overflow_before_padding() {
        assert_eq!(validate_source_read(7,1024,Some(136),129*4),Ok(136));
        assert_eq!(validate_source_read(7,1024,Some(136),130*4),Err(AudioError::InvalidLength));
        assert_eq!(validate_source_read(7,128,None,129*4),Err(AudioError::InvalidLength));
        assert_eq!(validate_source_read(7,128,None,3),Err(AudioError::InvalidLength));
        assert_eq!(validate_source_read(u64::MAX,128,None,4),Err(AudioError::Exhausted));
        assert_eq!(validate_source_read(7,128,None,0),Ok(7));
    }
    #[test]
    fn every_unsupported_ready_field_fails_without_admission(){
        let mut s=AudioSender::new(1,2);let id=s.start_sbc(3,9).unwrap();let r=ready(id,9);
        let mut invalid=vec![];
        macro_rules! bad {($field:ident,$val:expr)=>{{let mut b=r;b.$field=$val;invalid.push(b);}}}
        bad!(first_frame,10);bad!(rate,44100);bad!(channels,1);bad!(blocks,8);bad!(subbands,4);bad!(channel_mode,1);bad!(allocation,2);bad!(bitpool,1);bad!(bitpool,54);bad!(capacity_frames,4096);bad!(codec_epoch,0);
        for b in invalid {assert_eq!(s.admit_ready(b),Err(AudioError::InvalidConfiguration));assert!(s.waiting_ready());assert_eq!(s.allowance(),0);}
        let mut near=AudioSender::new(1,2);let id=near.start_sbc(3,u64::MAX-127).unwrap();near.admit_ready(ready(id,u64::MAX-127)).unwrap();assert_eq!(near.allowance(),0);
    }
    #[test]
    fn complete_sbc_packet_validates_every_frame_and_protected_header(){
        for mode in [2,3] {for allocation in [0,1] {for bitpool in 2..=53 {
            let id=StreamIdentity{incarnation:1,session_epoch:2,display_epoch:3,stream_id:4};let mut r=ready(id,9);r.channel_mode=mode;r.allocation=allocation;r.bitpool=bitpool;
            let f=frame(r);let payload=f.repeat(8);let bytes=encode_sbc_chunk(r,9,&payload).unwrap();
            assert_eq!(bytes[5],30);assert_eq!(bytes.len(),46+8*r.frame_bytes());assert_eq!(&bytes[38..46],&9u64.to_be_bytes());assert!(bytes.len()<=998);assert!(decode_chunk(&bytes).is_err());
            assert!(encode_sbc_chunk(r,0,&[]).is_err());assert!(encode_sbc_chunk(r,0,&f.repeat(9)).is_err());assert!(encode_sbc_chunk(r,0,&payload[..payload.len()-1]).is_err());
            assert!(encode_sbc_chunk(r,u64::MAX-1023,&payload).is_err());
            for field in [0,1,2,3,4,11] {let mut bad=payload.clone();bad[7*r.frame_bytes()+field]^=1;assert!(encode_sbc_chunk(r,9,&bad).is_err());}
        }}}
    }
}
