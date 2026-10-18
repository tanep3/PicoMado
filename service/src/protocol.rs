//! Versioned, length-framed application messages carried in binary WebSocket
//! messages. A WebSocket boundary need not align with an application boundary.

use serde::{Deserialize, Serialize};

use crate::settings::DeviceSnapshot;
use crate::{DrawFinish, FrameToken, PicoScreen};

pub const VERSION: u8 = 1;
/// Provisional control-message cap pending Pico memory measurement.
pub const MAX_CONTROL_BYTES: usize = 4096;
/// Provisional cap for one received WebSocket binary chunk.
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
/// Provisional image fragment payload. A 160×80 RGB565 image needs seven fragments.
pub const IMAGE_FRAGMENT_BYTES: usize = 4096;
pub const LCD_FRAME_BYTES: usize = 160 * 80 * 2;
pub const FRAME_BYTES_CAPABILITY: &str = "frame-bytes-v1";
pub const FRAME_BYTES_KIND: u8 = 18;
pub const LCD_PUSH_CAPABILITY: &str = "lcd-push-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamFrame {
    pub fps: u32,
    pub sequence: u64,
    pub generation: u64,
}

fn valid_frame_contract(width: u16, height: u16, format: &str, byte_len: u32) -> bool {
    (width == 160 && height == 80 && format == "rgb565be" && byte_len as usize == LCD_FRAME_BYTES)
        || (width == 250
            && height == 122
            && format == "mono1"
            && byte_len as usize == crate::eink::FRAME_BYTES)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputContext {
    pub incarnation: u64,
    pub session_epoch: u64,
    pub display_epoch: u64,
    pub mode: String,
}

/// Capability-negotiated clock-only partial refresh policy. Pixel bytes and
/// render acknowledgements retain the ordinary mono1 contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EinkRefresh {
    pub full_refresh_minutes: u16,
    pub minute: u64,
}
pub const EINK_CLOCK_CAPABILITY: &str = "eink-clock-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WireMessage {
    Hello {
        device_id: String,
        capabilities: Vec<String>,
        screen_owner_epoch: u64,
    },
    Welcome {
        incarnation: u64,
        session_epoch: u64,
    },
    Settings {
        snapshot: DeviceSnapshot,
    },
    SettingsStored {
        incarnation: u64,
        revision: u64,
    },
    Heartbeat {
        incarnation: u64,
        session_epoch: u64,
        applied_revision: u64,
    },
    Next {
        incarnation: u64,
        session_epoch: u64,
    },
    Power {
        incarnation: u64,
        session_epoch: u64,
    },
    ReceiptAck {
        token: FrameToken,
    },
    RenderedAck {
        token: FrameToken,
    },
    InputAccepted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_context: Option<InputContext>,
        selected: Option<String>,
        display_epoch: u64,
    },
    Error {
        code: String,
    },
    FrameStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        eink_refresh: Option<EinkRefresh>,
        token: FrameToken,
        /// Optional for legacy frames; a new frame carries its logical mode.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Present only for capability-negotiated LCD push video.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream: Option<StreamFrame>,
        width: u16,
        height: u16,
        format: String,
        byte_len: u32,
        crc32: u32,
    },
    FrameChunk {
        token: FrameToken,
        offset: u32,
        bytes: Vec<u8>,
    },
    EnterSub {
        incarnation: u64,
        session_epoch: u64,
    },
    Up {
        incarnation: u64,
        session_epoch: u64,
    },
    Down {
        incarnation: u64,
        session_epoch: u64,
    },
    StreamStatus {
        token: FrameToken,
        generation: u64,
        received_frames: u64,
        rendered_frames: u64,
        dropped_frames: u64,
        elapsed_ms: u64,
        max_gap_us: u64,
    },
    PluginPosition {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_context: Option<InputContext>,
        plugin: String,
        dataset_id: String,
        target_date: String,
        index: usize,
    },
}

impl WireMessage {
    fn kind(&self) -> u8 {
        match self {
            Self::Hello { .. } => 1,
            Self::Welcome { .. } => 2,
            Self::Settings { .. } => 3,
            Self::SettingsStored { .. } => 4,
            Self::Heartbeat { .. } => 5,
            Self::Next { .. } => 6,
            Self::Power { .. } => 7,
            Self::ReceiptAck { .. } => 8,
            Self::RenderedAck { .. } => 9,
            Self::InputAccepted { .. } => 10,
            Self::Error { .. } => 11,
            Self::FrameStart { .. } => 12,
            Self::FrameChunk { .. } => 13,
            Self::EnterSub { .. } => 14,
            Self::Up { .. } => 15,
            Self::Down { .. } => 16,
            Self::PluginPosition { .. } => 17,
            Self::StreamStatus { .. } => 19,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidLength,
    TooLarge,
    UnsupportedVersion,
    UnknownKind,
    KindMismatch,
    InvalidJson,
    InvalidFrame,
}

/// Mock-terminal/reference receiver. Physical drawing is outside this type;
/// callers may emit ReceiptAck only after finish and RenderedAck after drawing.
#[derive(Default)]
pub struct FrameAssembler {
    active: Option<(FrameToken, usize, u32, Vec<u8>)>,
}

impl FrameAssembler {
    pub fn reset(&mut self) {
        self.active = None;
    }
    pub fn accept(
        &mut self,
        message: WireMessage,
    ) -> Result<Option<(FrameToken, Vec<u8>)>, ProtocolError> {
        match message {
            WireMessage::FrameStart {
                token,
                width,
                height,
                format,
                byte_len,
                crc32,
                ..
            } => {
                if !valid_frame_contract(width, height, &format, byte_len) {
                    return Err(ProtocolError::InvalidFrame);
                }
                self.active = Some((
                    token,
                    byte_len as usize,
                    crc32,
                    Vec::with_capacity(byte_len as usize),
                ));
                Ok(None)
            }
            WireMessage::FrameChunk {
                token,
                offset,
                bytes,
            } => {
                let Some((active, expected, crc, buffer)) = self.active.as_mut() else {
                    return Err(ProtocolError::InvalidFrame);
                };
                if *active != token
                    || offset as usize != buffer.len()
                    || bytes.is_empty()
                    || bytes.len() > IMAGE_FRAGMENT_BYTES
                    || buffer.len() + bytes.len() > *expected
                {
                    self.active = None;
                    return Err(ProtocolError::InvalidFrame);
                }
                buffer.extend_from_slice(&bytes);
                if buffer.len() == *expected {
                    if crc32fast::hash(buffer) != *crc {
                        self.active = None;
                        return Err(ProtocolError::InvalidFrame);
                    }
                    let (token, _, _, pixels) = self.active.take().expect("active frame");
                    Ok(Some((token, pixels)))
                } else {
                    Ok(None)
                }
            }
            _ => Err(ProtocolError::InvalidFrame),
        }
    }
}

/// Mock terminal's complete receive-to-draw gate. Firmware must implement the
/// same checks before sending ACKs or touching the physical display.
pub struct GatedFrameReceiver {
    pub screen: PicoScreen,
    assembly: FrameAssembler,
}
impl GatedFrameReceiver {
    pub fn new(screen: PicoScreen) -> Self {
        Self {
            screen,
            assembly: FrameAssembler::default(),
        }
    }
    pub fn accept(&mut self, message: WireMessage) -> Result<Vec<WireMessage>, ProtocolError> {
        match &message {
            WireMessage::FrameStart { token, .. } => {
                if !self.screen.can_begin(token) {
                    self.assembly.reset();
                    return Ok(Vec::new());
                }
            }
            WireMessage::FrameChunk { token, .. } => {
                if !self.screen.can_begin(token) {
                    self.assembly.reset();
                    return Ok(Vec::new());
                }
            }
            _ => return Err(ProtocolError::InvalidFrame),
        }
        if let Some((token, _pixels)) = self.assembly.accept(message)? {
            if !self.screen.begin(token.clone()) {
                return Ok(Vec::new());
            }
            Ok(vec![WireMessage::ReceiptAck { token }])
        } else {
            Ok(Vec::new())
        }
    }
    pub fn complete_draw(&mut self, token: FrameToken) -> Option<WireMessage> {
        match self.screen.finish(token) {
            DrawFinish::RenderedAck(token) => Some(WireMessage::RenderedAck { token }),
            _ => None,
        }
    }
}

pub fn encode(message: &WireMessage) -> Result<Vec<u8>, ProtocolError> {
    validate_image_message(message)?;
    let payload = serde_json::to_vec(message).map_err(|_| ProtocolError::InvalidJson)?;
    let len = payload.len() + 2;
    let limit = if matches!(message, WireMessage::FrameChunk { .. }) {
        MAX_CHUNK_BYTES
    } else {
        MAX_CONTROL_BYTES
    };
    if len > limit {
        return Err(ProtocolError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(len + 4);
    bytes.extend_from_slice(&(len as u32).to_be_bytes());
    bytes.push(VERSION);
    bytes.push(message.kind());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Negotiated image bytes; controls and non-negotiating terminals keep JSON.
pub fn encode_frame_bytes(message: &WireMessage) -> Result<Vec<u8>, ProtocolError> {
    validate_image_message(message)?;
    let WireMessage::FrameChunk {
        token,
        offset,
        bytes: pixels,
    } = message
    else {
        return Err(ProtocolError::KindMismatch);
    };
    let id = token.device_id.as_bytes();
    if id.is_empty() || id.len() > 50 {
        return Err(ProtocolError::InvalidLength);
    }
    let length = 2 + 1 + id.len() + 44 + pixels.len();
    let mut bytes = Vec::with_capacity(length + 4);
    bytes.extend_from_slice(&(length as u32).to_be_bytes());
    bytes.extend_from_slice(&[VERSION, FRAME_BYTES_KIND, id.len() as u8]);
    bytes.extend_from_slice(id);
    for value in [
        token.incarnation,
        token.session_epoch,
        token.display_epoch,
        token.screen_owner_epoch,
        token.frame_id,
    ] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.extend_from_slice(pixels);
    Ok(bytes)
}
fn decode_frame_bytes(body: &[u8]) -> Result<WireMessage, ProtocolError> {
    let id_len = usize::from(*body.first().ok_or(ProtocolError::InvalidLength)?);
    let header = 1 + id_len + 44;
    if id_len == 0
        || id_len > 50
        || body.len() <= header
        || body.len() - header > IMAGE_FRAGMENT_BYTES
    {
        return Err(ProtocolError::InvalidLength);
    }
    let id = std::str::from_utf8(&body[1..1 + id_len])
        .map_err(|_| ProtocolError::InvalidJson)?
        .to_owned();
    let fields = &body[1 + id_len..header];
    let value = |index: usize| {
        u64::from_be_bytes(
            fields[index * 8..index * 8 + 8]
                .try_into()
                .expect("validated header"),
        )
    };
    let token = FrameToken {
        device_id: id,
        incarnation: value(0),
        session_epoch: value(1),
        display_epoch: value(2),
        screen_owner_epoch: value(3),
        frame_id: value(4),
    };
    Ok(WireMessage::FrameChunk {
        token,
        offset: u32::from_be_bytes(fields[40..44].try_into().expect("validated offset")),
        bytes: body[header..].to_vec(),
    })
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<WireMessage>, ProtocolError> {
        if bytes.len() > MAX_CHUNK_BYTES
            || self.buffer.len().saturating_add(bytes.len()) > MAX_CHUNK_BYTES
        {
            self.buffer.clear();
            return Err(ProtocolError::TooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        loop {
            if self.buffer.len() < 4 {
                break;
            }
            let length =
                u32::from_be_bytes(self.buffer[..4].try_into().expect("four bytes")) as usize;
            if length < 2 {
                self.buffer.clear();
                return Err(ProtocolError::InvalidLength);
            }
            if length > MAX_CHUNK_BYTES {
                self.buffer.clear();
                return Err(ProtocolError::TooLarge);
            }
            if self.buffer.len() >= 6
                && self.buffer[5] != 13
                && self.buffer[5] != FRAME_BYTES_KIND
                && length > MAX_CONTROL_BYTES
            {
                self.buffer.clear();
                return Err(ProtocolError::TooLarge);
            }
            if self.buffer.len() < length + 4 {
                break;
            }
            let frame = &self.buffer[4..4 + length];
            if frame[0] != VERSION {
                self.buffer.clear();
                return Err(ProtocolError::UnsupportedVersion);
            }
            if !(1..=19).contains(&frame[1]) {
                self.buffer.clear();
                return Err(ProtocolError::UnknownKind);
            }
            let kind = frame[1];
            let decoded = if kind == FRAME_BYTES_KIND {
                decode_frame_bytes(&frame[2..])
            } else {
                serde_json::from_slice(&frame[2..]).map_err(|_| ProtocolError::InvalidJson)
            };
            let message: WireMessage = match decoded {
                Ok(message) => message,
                Err(error) => {
                    self.buffer.clear();
                    return Err(error);
                }
            };
            if message.kind() != kind
                && !(kind == FRAME_BYTES_KIND && matches!(message, WireMessage::FrameChunk { .. }))
            {
                self.buffer.clear();
                return Err(ProtocolError::KindMismatch);
            }
            if validate_image_message(&message).is_err() {
                self.buffer.clear();
                return Err(ProtocolError::InvalidLength);
            }
            messages.push(message);
            self.buffer.drain(..length + 4);
        }
        Ok(messages)
    }
}

fn validate_image_message(message: &WireMessage) -> Result<(), ProtocolError> {
    match message {
        WireMessage::FrameStart {
            eink_refresh: Some(policy),
            width,
            height,
            format,
            ..
        } if *width != 250
            || *height != 122
            || format != "mono1"
            || !(1..=60).contains(&policy.full_refresh_minutes) =>
        {
            Err(ProtocolError::InvalidFrame)
        }
        WireMessage::FrameStart {
            width,
            height,
            format,
            byte_len,
            ..
        } if !valid_frame_contract(*width, *height, format, *byte_len) => {
            Err(ProtocolError::InvalidLength)
        }
        WireMessage::FrameChunk { bytes, .. }
            if bytes.is_empty() || bytes.len() > IMAGE_FRAGMENT_BYTES =>
        {
            Err(ProtocolError::InvalidLength)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_image_chunks_preserve_tokens_pixels_crc_and_reduce_wire_size() {
        for (width, height, format, length) in [
            (160, 80, "rgb565be", LCD_FRAME_BYTES),
            (250, 122, "mono1", crate::eink::FRAME_BYTES),
        ] {
            let token = FrameToken {
                device_id: "desk".into(),
                incarnation: 0x0102030405060708,
                session_epoch: 0x1122334455667788,
                display_epoch: 9,
                screen_owner_epoch: 10,
                frame_id: 11,
            };
            let pixels: Vec<u8> = (0..length).map(|i| (i % 256) as u8).collect();
            let start = WireMessage::FrameStart {
                eink_refresh: None,
                stream: None,
                token: token.clone(),
                mode: Some("normal".into()),
                width,
                height,
                format: format.into(),
                byte_len: length as u32,
                crc32: crc32fast::hash(&pixels),
            };
            let mut assembler = FrameAssembler::default();
            assembler.accept(start.clone()).unwrap();
            let mut binary_bytes = encode(&start).unwrap().len();
            let mut json_bytes = binary_bytes;
            let mut result = None;
            for (index, chunk) in pixels.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
                let message = WireMessage::FrameChunk {
                    token: token.clone(),
                    offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                    bytes: chunk.to_vec(),
                };
                let encoded = encode_frame_bytes(&message).unwrap();
                assert_eq!(encoded[5], FRAME_BYTES_KIND);
                binary_bytes += encoded.len();
                json_bytes += encode(&message).unwrap().len();
                let mut decoder = Decoder::default();
                let mut decoded = Vec::new();
                for part in encoded.chunks(17) {
                    decoded.extend(decoder.push(part).unwrap());
                }
                assert_eq!(decoded, vec![message]);
                result = assembler.accept(decoded.remove(0)).unwrap().or(result);
                for bad_id_length in [0, 255] {
                    let mut invalid = encoded.clone();
                    invalid[6] = bad_id_length;
                    assert!(Decoder::default().push(&invalid).is_err());
                }
            }
            assert_eq!(result.unwrap(), (token.clone(), pixels.clone()));
            if length == LCD_FRAME_BYTES {
                assert!(binary_bytes < 27000 && json_bytes > 85000);
            }
            assert!(binary_bytes * 3 < json_bytes);
            let mut corrupt = FrameAssembler::default();
            corrupt.accept(start).unwrap();
            let mut bad = pixels;
            bad[0] ^= 1;
            let mut crc_failed = false;
            for (index, chunk) in bad.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
                let message = WireMessage::FrameChunk {
                    token: token.clone(),
                    offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                    bytes: chunk.to_vec(),
                };
                let decoded = Decoder::default()
                    .push(&encode_frame_bytes(&message).unwrap())
                    .unwrap()
                    .remove(0);
                if corrupt.accept(decoded).is_err() {
                    crc_failed = true;
                }
            }
            assert!(crc_failed);
        }
    }

    use crate::{LocalScreen, ScreenKind};

    fn hello() -> WireMessage {
        WireMessage::Hello {
            device_id: "desk".into(),
            capabilities: vec!["lcd".into()],
            screen_owner_epoch: 1,
        }
    }

    #[test]
    fn partial_and_concatenated_frames_decode_in_order() {
        let first = encode(&hello()).unwrap();
        let second = encode(&WireMessage::Next {
            incarnation: 1,
            session_epoch: 2,
        })
        .unwrap();
        let mut decoder = Decoder::default();
        assert!(decoder.push(&first[..3]).unwrap().is_empty());
        assert!(decoder.push(&first[3..7]).unwrap().is_empty());
        let mut rest = first[7..].to_vec();
        rest.extend_from_slice(&second);
        assert_eq!(
            decoder.push(&rest).unwrap(),
            vec![
                hello(),
                WireMessage::Next {
                    incarnation: 1,
                    session_epoch: 2
                }
            ]
        );
    }

    #[test]
    fn rejects_length_version_kind_and_invalid_json() {
        let mut decoder = Decoder::default();
        assert_eq!(
            decoder.push(&0u32.to_be_bytes()),
            Err(ProtocolError::InvalidLength)
        );
        let mut oversized_control = (MAX_CONTROL_BYTES as u32 + 1).to_be_bytes().to_vec();
        oversized_control.extend_from_slice(&[VERSION, 1]);
        assert_eq!(
            decoder.push(&oversized_control),
            Err(ProtocolError::TooLarge)
        );
        let mut frame = encode(&hello()).unwrap();
        frame[4] = 2;
        assert_eq!(decoder.push(&frame), Err(ProtocolError::UnsupportedVersion));
        frame[4] = VERSION;
        frame[5] = 99;
        assert_eq!(decoder.push(&frame), Err(ProtocolError::UnknownKind));
        frame[5] = 6;
        assert_eq!(decoder.push(&frame), Err(ProtocolError::KindMismatch));
        frame[5] = 1;
        frame[6] = b'!';
        assert_eq!(decoder.push(&frame), Err(ProtocolError::InvalidJson));
    }

    #[test]
    fn image_receiver_checks_size_order_token_and_crc() {
        let token = FrameToken {
            device_id: "a".into(),
            incarnation: 1,
            session_epoch: 2,
            display_epoch: 3,
            screen_owner_epoch: 4,
            frame_id: 5,
        };
        let bytes = vec![0x5a; LCD_FRAME_BYTES];
        let start = WireMessage::FrameStart {
            eink_refresh: None,
            stream: None,
            mode: None,
            token: token.clone(),
            width: 160,
            height: 80,
            format: "rgb565be".into(),
            byte_len: bytes.len() as u32,
            crc32: crc32fast::hash(&bytes),
        };
        let mut receiver = FrameAssembler::default();
        assert_eq!(receiver.accept(start.clone()), Ok(None));
        assert_eq!(
            receiver.accept(WireMessage::FrameChunk {
                token: token.clone(),
                offset: 1,
                bytes: bytes[..100].to_vec()
            }),
            Err(ProtocolError::InvalidFrame)
        );
        assert_eq!(receiver.accept(start.clone()), Ok(None));
        let mut result = None;
        for (index, part) in bytes.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
            let msg = WireMessage::FrameChunk {
                token: token.clone(),
                offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                bytes: part.to_vec(),
            };
            let coded = encode(&msg).unwrap();
            let decoded = Decoder::default().push(&coded).unwrap().remove(0);
            result = receiver.accept(decoded).unwrap().or(result);
        }
        assert_eq!(result, Some((token.clone(), bytes.clone())));
        let mut wrong = start;
        if let WireMessage::FrameStart { crc32, .. } = &mut wrong {
            *crc32 ^= 1;
        }
        receiver.accept(wrong).unwrap();
        for (index, part) in bytes.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
            let outcome = receiver.accept(WireMessage::FrameChunk {
                token: token.clone(),
                offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                bytes: part.to_vec(),
            });
            if index == 6 {
                assert_eq!(outcome, Err(ProtocolError::InvalidFrame));
            } else {
                assert_eq!(outcome, Ok(None));
            }
        }
        assert!(matches!(
            encode(&WireMessage::FrameStart {
                eink_refresh: None,
                stream: None,
                mode: None,
                token,
                width: 1,
                height: 80,
                format: "rgb565be".into(),
                byte_len: LCD_FRAME_BYTES as u32,
                crc32: 0
            }),
            Err(ProtocolError::InvalidLength)
        ));
    }

    #[test]
    fn gated_receiver_never_acks_bad_length_or_crc() {
        let token = FrameToken {
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            frame_id: 1,
        };
        let screen = PicoScreen {
            kind: crate::ScreenKind::Lcd,
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 1,
            display_epoch: 1,
            screen_owner_epoch: 1,
            local: None,
            drawing: None,
            pending_standby: false,
        };
        let mut terminal = GatedFrameReceiver::new(screen);
        let pixels = vec![7; LCD_FRAME_BYTES];
        let start = WireMessage::FrameStart {
            eink_refresh: None,
            stream: None,
            mode: None,
            token: token.clone(),
            width: 160,
            height: 80,
            format: "rgb565be".into(),
            byte_len: LCD_FRAME_BYTES as u32,
            crc32: crc32fast::hash(&pixels),
        };
        let mut invalid_length = start.clone();
        if let WireMessage::FrameStart { byte_len, .. } = &mut invalid_length {
            *byte_len -= 1;
        }
        assert_eq!(
            terminal.accept(invalid_length),
            Err(ProtocolError::InvalidFrame)
        );
        assert!(terminal.complete_draw(token.clone()).is_none());
        let mut invalid_crc = start;
        if let WireMessage::FrameStart { crc32, .. } = &mut invalid_crc {
            *crc32 ^= 1;
        }
        assert!(terminal.accept(invalid_crc).unwrap().is_empty());
        for (index, part) in pixels.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
            let result = terminal.accept(WireMessage::FrameChunk {
                token: token.clone(),
                offset: (index * IMAGE_FRAGMENT_BYTES) as u32,
                bytes: part.to_vec(),
            });
            if index == 6 {
                assert_eq!(result, Err(ProtocolError::InvalidFrame));
            } else {
                assert!(result.unwrap().is_empty());
            }
        }
        assert!(terminal.complete_draw(token).is_none());
    }

    #[test]
    fn gated_receiver_suppresses_old_frames_and_draw_acks_after_owner_changes() {
        let token = FrameToken {
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 2,
            display_epoch: 3,
            screen_owner_epoch: 4,
            frame_id: 5,
        };
        let screen = PicoScreen {
            kind: ScreenKind::Lcd,
            device_id: "desk".into(),
            incarnation: 1,
            session_epoch: 2,
            display_epoch: 3,
            screen_owner_epoch: 4,
            local: None,
            drawing: None,
            pending_standby: false,
        };
        let pixels = vec![0x5a; LCD_FRAME_BYTES];
        let start = WireMessage::FrameStart {
            eink_refresh: None,
            stream: None,
            mode: None,
            token: token.clone(),
            width: 160,
            height: 80,
            format: "rgb565be".into(),
            byte_len: LCD_FRAME_BYTES as u32,
            crc32: crc32fast::hash(&pixels),
        };
        let chunks: Vec<_> = pixels
            .chunks(IMAGE_FRAGMENT_BYTES)
            .enumerate()
            .map(|(i, bytes)| WireMessage::FrameChunk {
                token: token.clone(),
                offset: (i * IMAGE_FRAGMENT_BYTES) as u32,
                bytes: bytes.to_vec(),
            })
            .collect();
        for scenario in 0..4 {
            let mut terminal = GatedFrameReceiver::new(screen.clone());
            assert!(terminal.accept(start.clone()).unwrap().is_empty());
            assert!(terminal.accept(chunks[0].clone()).unwrap().is_empty());
            match scenario {
                0 => terminal.screen.display_epoch += 1, // plugin switch
                1 => terminal.screen.display_epoch += 1, // sub cancel
                2 => terminal.screen.take_local(LocalScreen::Standby),
                _ => terminal.screen.session_epoch += 1, // reconnect
            }
            for chunk in chunks.iter().skip(1) {
                assert!(terminal.accept(chunk.clone()).unwrap().is_empty());
            }
            assert!(terminal.complete_draw(token.clone()).is_none());
        }
        let mut terminal = GatedFrameReceiver::new(screen.clone());
        terminal.accept(start).unwrap();
        for chunk in chunks {
            let replies = terminal.accept(chunk).unwrap();
            if !replies.is_empty() {
                assert_eq!(
                    replies,
                    vec![WireMessage::ReceiptAck {
                        token: token.clone()
                    }]
                );
            }
        }
        terminal.screen.take_local(LocalScreen::Standby);
        assert!(terminal.complete_draw(token.clone()).is_none());
        let mut fresh_screen = screen;
        fresh_screen.display_epoch += 1;
        let fresh = FrameToken {
            display_epoch: 4,
            frame_id: 6,
            ..token
        };
        let mut fresh_receiver = GatedFrameReceiver::new(fresh_screen);
        fresh_receiver
            .accept(WireMessage::FrameStart {
                eink_refresh: None,
                stream: None,
                mode: None,
                token: fresh.clone(),
                width: 160,
                height: 80,
                format: "rgb565be".into(),
                byte_len: LCD_FRAME_BYTES as u32,
                crc32: crc32fast::hash(&pixels),
            })
            .unwrap();
        let mut receipt = None;
        for (i, bytes) in pixels.chunks(IMAGE_FRAGMENT_BYTES).enumerate() {
            receipt = fresh_receiver
                .accept(WireMessage::FrameChunk {
                    token: fresh.clone(),
                    offset: (i * IMAGE_FRAGMENT_BYTES) as u32,
                    bytes: bytes.to_vec(),
                })
                .unwrap()
                .pop()
                .or(receipt);
        }
        assert_eq!(
            receipt,
            Some(WireMessage::ReceiptAck {
                token: fresh.clone()
            })
        );
        assert_eq!(
            fresh_receiver.complete_draw(fresh.clone()),
            Some(WireMessage::RenderedAck { token: fresh })
        );
    }
}
