//! BT-AUDIO-20261107: bounded host SBC codec adapter, without IO.
//! One stream owns one encoder. Global C scratch is serialized internally.
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

const PCM_BLOCK_BYTES: usize = 128 * 2 * 2;
const MAX_BLOCKS: usize = 8;
const MAX_FRAME_BYTES: usize = 119;
static ENCODER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbcEncoderConfig {
    pub rate: u32,
    pub channels: u8,
    pub blocks: u8,
    pub subbands: u8,
    /// SBC header value: 2 = stereo; 3 = joint stereo.
    pub channel_mode: u8,
    /// SBC header value: 0 = loudness; 1 = SNR.
    pub allocation: u8,
    pub bitpool: u8,
}
impl SbcEncoderConfig {
    pub fn valid(self) -> bool {
        self.rate == 48_000
            && self.channels == 2
            && self.blocks == 16
            && self.subbands == 8
            && matches!(self.channel_mode, 2 | 3)
            && self.allocation <= 1
            && (2..=53).contains(&self.bitpool)
    }
    pub fn frame_bytes(self) -> Option<usize> {
        self.valid()
            .then_some(12 + usize::from(self.channel_mode == 3) + 2 * self.bitpool as usize)
    }
    fn header(self) -> u8 {
        0xf1 | self.channel_mode << 2 | self.allocation << 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderError {
    InvalidConfiguration,
    InvalidPcmLength,
    AllocationFailed,
    CodecFailed,
    InvalidEncodedFrame,
    Poisoned,
}

extern "C" {
    fn pm_sbc_encoder_new(
        rate: u32,
        channels: u8,
        blocks: u8,
        subbands: u8,
        mode: u8,
        allocation: u8,
        bitpool: u8,
    ) -> *mut c_void;
    fn pm_sbc_encoder_encode(
        encoder: *mut c_void,
        pcm: *const u8,
        pcm_bytes: u32,
        output: *mut u8,
        output_capacity: u32,
    ) -> i32;
    fn pm_sbc_encoder_free(encoder: *mut c_void);
}

pub struct SbcEncoder {
    context: NonNull<c_void>,
    config: SbcEncoderConfig,
    failed: bool,
}
// SAFETY: The opaque allocation is owned exclusively; encode requires &mut self.
// Every init/encode call takes the same global lock, protecting the C library's
// global scratch across streams. Moving the owner does not move the C allocation.
unsafe impl Send for SbcEncoder {}

impl SbcEncoder {
    pub fn new(config: SbcEncoderConfig) -> Result<Self, EncoderError> {
        if !config.valid() {
            return Err(EncoderError::InvalidConfiguration);
        }
        let _guard = ENCODER_LOCK.lock().map_err(|_| EncoderError::Poisoned)?;
        // SAFETY: All primitive parameters have been validated and are validated
        // again by the C wrapper. The returned allocation has one owner.
        let raw = unsafe {
            pm_sbc_encoder_new(
                config.rate,
                config.channels,
                config.blocks,
                config.subbands,
                config.channel_mode,
                config.allocation,
                config.bitpool,
            )
        };
        let context = NonNull::new(raw).ok_or(EncoderError::AllocationFailed)?;
        Ok(Self {
            context,
            config,
            failed: false,
        })
    }
    pub fn config(&self) -> SbcEncoderConfig {
        self.config
    }

    /// Encode 1..8 whole 128-frame s16le stereo blocks, retaining filter history.
    /// Gain and final-block zero padding belong to the caller, before encoding.
    /// Invalid input does not alter state. A C/output failure poisons this stream.
    pub fn encode(&mut self, pcm_s16le: &[u8]) -> Result<Vec<u8>, EncoderError> {
        if pcm_s16le.is_empty()
            || pcm_s16le.len() > MAX_BLOCKS * PCM_BLOCK_BYTES
            || pcm_s16le.len() % PCM_BLOCK_BYTES != 0
        {
            return Err(EncoderError::InvalidPcmLength);
        }
        if self.failed {
            return Err(EncoderError::CodecFailed);
        }
        let _guard = ENCODER_LOCK.lock().map_err(|_| EncoderError::Poisoned)?;
        let length = self
            .config
            .frame_bytes()
            .ok_or(EncoderError::InvalidConfiguration)?;
        let mut output = Vec::with_capacity(pcm_s16le.len() / PCM_BLOCK_BYTES * length);
        for pcm in pcm_s16le.chunks_exact(PCM_BLOCK_BYTES) {
            let mut encoded = [0u8; MAX_FRAME_BYTES];
            // SAFETY: Owned context, exact readable 512-byte input, writable
            // 119-byte output, held global lock; C retains no input/output ptrs.
            let result = unsafe {
                pm_sbc_encoder_encode(
                    self.context.as_ptr(),
                    pcm.as_ptr(),
                    PCM_BLOCK_BYTES as u32,
                    encoded.as_mut_ptr(),
                    MAX_FRAME_BYTES as u32,
                )
            };
            if result != length as i32 {
                self.failed = true;
                return Err(EncoderError::CodecFailed);
            }
            if !valid_encoded_frame(self.config, &encoded[..length]) {
                self.failed = true;
                return Err(EncoderError::InvalidEncodedFrame);
            }
            output.extend_from_slice(&encoded[..length]);
        }
        Ok(output)
    }
}
impl Drop for SbcEncoder {
    fn drop(&mut self) {
        // SAFETY: The pointer was allocated by this ABI and has one live owner.
        // Free touches only its allocation, not the encoder's shared scratch.
        unsafe {
            pm_sbc_encoder_free(self.context.as_ptr());
        }
    }
}

fn valid_encoded_frame(config: SbcEncoderConfig, frame: &[u8]) -> bool {
    if Some(frame.len()) != config.frame_bytes()
        || frame.len() < 16
        || frame[0] != 0x9c
        || frame[1] != config.header()
        || frame[2] != config.bitpool
    {
        return false;
    }
    let joint = config.channel_mode == 3;
    if joint && frame[4] & 1 != 0 {
        return false;
    }
    let mut crc: u8 = 0x0f;
    for byte in frame[1..3]
        .iter()
        .chain(frame[4..if joint { 13 } else { 12 }].iter())
    {
        for bit in (0..8).rev() {
            let xor = ((crc >> 7) ^ (byte >> bit)) & 1;
            crc <<= 1;
            if xor != 0 {
                crc ^= 0x1d;
            }
        }
    }
    crc == frame[3]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> SbcEncoderConfig {
        SbcEncoderConfig {
            rate: 48000,
            channels: 2,
            blocks: 16,
            subbands: 8,
            channel_mode: 3,
            allocation: 0,
            bitpool: 32,
        }
    }
    fn pcm(blocks: usize, seed: i16) -> Vec<u8> {
        (0..blocks * 256)
            .flat_map(|i| {
                (i as i16)
                    .wrapping_mul(231)
                    .wrapping_add(seed)
                    .to_le_bytes()
            })
            .collect()
    }
    #[test]
    fn every_supported_configuration_has_exact_frames_and_crc() {
        let pcm = pcm(8, -28000);
        for mode in 2..=3 {
            for allocation in 0..=1 {
                for bitpool in 2..=53 {
                    let config = SbcEncoderConfig {
                        channel_mode: mode,
                        allocation,
                        bitpool,
                        ..config()
                    };
                    let mut encoder = SbcEncoder::new(config).unwrap();
                    let bytes = encoder.encode(&pcm).unwrap();
                    assert_eq!(bytes.len(), 8 * config.frame_bytes().unwrap());
                    for frame in bytes.chunks_exact(config.frame_bytes().unwrap()) {
                        assert!(valid_encoded_frame(config, frame));
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_unsupported_configuration_without_calling_codec() {
        let valid = config();
        let invalid = [
            SbcEncoderConfig {
                rate: 44100,
                ..valid
            },
            SbcEncoderConfig {
                channels: 1,
                ..valid
            },
            SbcEncoderConfig { blocks: 8, ..valid },
            SbcEncoderConfig {
                subbands: 4,
                ..valid
            },
            SbcEncoderConfig {
                channel_mode: 1,
                ..valid
            },
            SbcEncoderConfig {
                channel_mode: 4,
                ..valid
            },
            SbcEncoderConfig {
                allocation: 2,
                ..valid
            },
            SbcEncoderConfig {
                bitpool: 1,
                ..valid
            },
            SbcEncoderConfig {
                bitpool: 54,
                ..valid
            },
        ];
        for config in invalid {
            assert!(!config.valid());
            assert_eq!(config.frame_bytes(), None);
            assert!(matches!(
                SbcEncoder::new(config),
                Err(EncoderError::InvalidConfiguration)
            ));
        }
    }
    #[test]
    fn input_bounds_and_bad_input_preserve_history() {
        let mut encoder = SbcEncoder::new(config()).unwrap();
        for len in [0, 1, 511, 513, 4097, 4608] {
            assert_eq!(
                encoder.encode(&vec![0; len]),
                Err(EncoderError::InvalidPcmLength)
            );
        }
        let bytes = pcm(1, -1);
        assert_eq!(
            encoder.encode(&bytes).unwrap(),
            SbcEncoder::new(config()).unwrap().encode(&bytes).unwrap()
        );
    }
    #[test]
    fn chunks_keep_filter_history_and_interleaved_streams_stay_independent() {
        let a = pcm(8, -28000);
        let b = pcm(8, 30000);
        let expected_a = SbcEncoder::new(config()).unwrap().encode(&a).unwrap();
        let expected_b = SbcEncoder::new(config()).unwrap().encode(&b).unwrap();
        let mut ea = SbcEncoder::new(config()).unwrap();
        let mut eb = SbcEncoder::new(config()).unwrap();
        let mut actual_a = vec![];
        let mut actual_b = vec![];
        for (pa, pb) in a.chunks_exact(512).zip(b.chunks_exact(512)) {
            actual_a.extend(ea.encode(pa).unwrap());
            actual_b.extend(eb.encode(pb).unwrap());
        }
        assert_eq!(actual_a, expected_a);
        assert_eq!(actual_b, expected_b);
        // Reinitializing each block must not accidentally pass this fixture.
        let reset: Vec<_> = a
            .chunks_exact(512)
            .flat_map(|p| SbcEncoder::new(config()).unwrap().encode(p).unwrap())
            .collect();
        assert_ne!(reset, actual_a);
    }
    #[test]
    fn independent_threads_are_serialized_and_owner_is_send() {
        let pcm = pcm(8, 1200);
        let expected = SbcEncoder::new(config()).unwrap().encode(&pcm).unwrap();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let encoder = SbcEncoder::new(config()).unwrap();
                let pcm = pcm.clone();
                std::thread::spawn(move || {
                    let mut encoder = encoder;
                    encoder.encode(&pcm).unwrap()
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), expected);
        }
    }
    #[test]
    fn fixed_cross_language_vectors_remain_reproducible() {
        let vectors = include_str!("../tests/fixtures/sbc-v1-frames.txt");
        for line in vectors
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 4);
            let config = SbcEncoderConfig {
                channel_mode: fields[0].parse().unwrap(),
                allocation: fields[1].parse().unwrap(),
                bitpool: fields[2].parse().unwrap(),
                ..config()
            };
            let expected: Vec<u8> = fields[3]
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(
                SbcEncoder::new(config)
                    .unwrap()
                    .encode(&pcm(1, -28000))
                    .unwrap(),
                expected
            );
        }
    }
    #[test]
    fn malformed_protected_header_is_rejected() {
        let frame = SbcEncoder::new(config())
            .unwrap()
            .encode(&pcm(1, 0))
            .unwrap();
        for index in 0..13 {
            let mut bad = frame.clone();
            bad[index] ^= 1;
            assert!(!valid_encoded_frame(config(), &bad), "index {index}");
        }
        assert!(!valid_encoded_frame(config(), &frame[..76]));
        let mut extra = frame.clone();
        extra.push(0);
        assert!(!valid_encoded_frame(config(), &extra));
    }
}
