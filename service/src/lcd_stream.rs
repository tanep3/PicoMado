//! Negotiated LCD video: one current frame plus one replaceable latest frame.
//! No per-frame persistence, and application drawing ACKs never grant credit.

use crate::{FrameToken, SessionToken};
use std::sync::Arc;
use tokio::sync::watch;

pub const SOCKET_SEND_BUFFER_BYTES: usize = 32 * 1024;
pub const WEBSOCKET_WRITE_BUFFER_BYTES: usize = 32 * 1024;
pub const MAX_FPS: u32 = 30;

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub token: FrameToken,
    pub fps: u32,
    pub sequence: u64,
    pub generation: u64,
    pub mode: &'static str,
    pub pixels: Arc<Vec<u8>>,
}

pub struct VideoSlot {
    pub session: SessionToken,
    pub latest: watch::Sender<Option<Arc<VideoFrame>>>,
    pub last_issued: u64,
    pub last_reported: u64,
    pub first_issued: u64,
    pub generation: u64,
}

impl VideoSlot {
    pub fn new(session: SessionToken) -> Self {
        Self {
            session,
            latest: watch::channel(None).0,
            last_issued: 0,
            last_reported: 0,
            first_issued: 0,
            generation: 0,
        }
    }
    pub fn publish(&mut self, frame: VideoFrame) {
        if self.first_issued == 0 || self.generation != frame.generation {
            self.first_issued = frame.token.frame_id;
            self.generation = frame.generation;
        }
        self.last_issued = frame.token.frame_id;
        self.latest.send_replace(Some(Arc::new(frame)));
    }
    pub fn clear(&mut self) {
        self.latest.send_replace(None);
    }
}

/// One immutable current frame. Pending replacements live only in VideoSlot.
pub struct VideoTransfer {
    pub frame: Arc<VideoFrame>,
    part: usize,
    pub started: std::time::Instant,
}
impl VideoTransfer {
    pub fn new(frame: Arc<VideoFrame>) -> Self {
        Self {
            frame,
            part: 0,
            started: std::time::Instant::now(),
        }
    }
    pub fn next_packet(&mut self) -> Result<Option<Vec<u8>>, crate::protocol::ProtocolError> {
        use crate::protocol::{StreamFrame, WireMessage, IMAGE_FRAGMENT_BYTES};
        let result = if self.part == 0 {
            crate::protocol::encode(&WireMessage::FrameStart {
                token: self.frame.token.clone(),
                mode: Some(self.frame.mode.into()),
                eink_refresh: None,
                stream: Some(StreamFrame {
                    fps: self.frame.fps,
                    sequence: self.frame.sequence,
                    generation: self.frame.generation,
                }),
                width: 160,
                height: 80,
                format: "rgb565be".into(),
                byte_len: self.frame.pixels.len() as u32,
                crc32: crc32fast::hash(&self.frame.pixels),
            })?
        } else {
            let offset = (self.part - 1) * IMAGE_FRAGMENT_BYTES;
            if offset >= self.frame.pixels.len() {
                return Ok(None);
            }
            let end = (offset + IMAGE_FRAGMENT_BYTES).min(self.frame.pixels.len());
            crate::protocol::encode_frame_bytes(&WireMessage::FrameChunk {
                token: self.frame.token.clone(),
                offset: offset as u32,
                bytes: self.frame.pixels[offset..end].to_vec(),
            })?
        };
        self.part += 1;
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Decoder, FrameAssembler, WireMessage};
    fn frame(id: u64, fps: u32) -> VideoFrame {
        VideoFrame {
            token: FrameToken {
                device_id: "lcd".into(),
                incarnation: 1,
                session_epoch: 1,
                display_epoch: 1,
                screen_owner_epoch: 1,
                frame_id: id,
            },
            fps,
            sequence: id,
            generation: 1,
            mode: "normal",
            pixels: Arc::new(vec![id as u8; 25_600]),
        }
    }
    #[test]
    fn latest_slots_skip_overload_and_finish_started_frames_at_6_24_30_fps() {
        for fps in [6, 24, 30] {
            let session = SessionToken {
                device_id: "lcd".into(),
                incarnation: 1,
                session_epoch: 1,
            };
            let mut slot = VideoSlot::new(session);
            let mut rx = slot.latest.subscribe();
            slot.publish(frame(1, fps));
            let first = rx.borrow_and_update().clone().unwrap();
            let mut transfer = VideoTransfer::new(first);
            let mut decoder = Decoder::default();
            let mut assembler = FrameAssembler::default();
            let start = transfer.next_packet().unwrap().unwrap();
            assert!(matches!(
                decoder.push(&start).unwrap()[0],
                WireMessage::FrameStart {
                    stream: Some(_),
                    ..
                }
            ));
            assembler
                .accept(Decoder::default().push(&start).unwrap().remove(0))
                .unwrap();
            slot.publish(frame(2, fps));
            let discarded = Arc::downgrade(slot.latest.borrow().as_ref().unwrap());
            for n in 3..=3000 {
                slot.publish(frame(n, fps));
            }
            assert!(
                discarded.upgrade().is_none(),
                "unsent old frames must be released"
            );
            let mut complete = None;
            while let Some(packet) = transfer.next_packet().unwrap() {
                assert!(packet.len() < 4300);
                for message in decoder.push(&packet).unwrap() {
                    if let Some(frame) = assembler.accept(message).unwrap() {
                        complete = Some(frame);
                    }
                }
            }
            let (token, pixels) = complete.unwrap();
            assert_eq!(token.frame_id, 1);
            assert!(pixels.iter().all(|b| *b == 1));
            assert_eq!(
                rx.borrow_and_update().as_ref().unwrap().token.frame_id,
                3000
            );
            assert_eq!(
                transfer.frame.pixels.len() + slot.latest.borrow().as_ref().unwrap().pixels.len(),
                51_200
            );
        }
    }
}
