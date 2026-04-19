//! Length-prefixed JSON codec. Each frame is `u32 BE length` + JSON bytes.
//!
//! Uses `tokio_util::codec::LengthDelimitedCodec` for the byte framing so we
//! inherit its battle-tested partial-read handling. The JSON layer on top
//! rejects frames over [`protocol::MAX_MESSAGE_BYTES`] and surfaces parse
//! errors as structured errors (so the dispatch loop can send a
//! JSON-RPC `ParseError` back to the client instead of disconnecting).
//!
//! Alternative considered: newline-delimited JSON. Rejected because it
//! forces us to either escape/quote newlines inside payloads (ugly + easy
//! to get wrong on the client side) or cap payload size at whatever fits
//! in one line (fragile). Length-prefix is unambiguous and $O(1)$ at the
//! receiver.

use std::fmt;
use std::io;

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::protocol::{MAX_MESSAGE_BYTES, Message};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("frame too large: {size} > {max}")]
    FrameTooLarge { size: usize, max: usize },
    #[error("malformed JSON: {0}")]
    Json(serde_json::Error),
}

/// One codec that both encodes [`Message`]s out and decodes [`Message`]s in.
/// Wraps `LengthDelimitedCodec` for the framing half.
pub struct JsonFrameCodec {
    frames: LengthDelimitedCodec,
}

impl fmt::Debug for JsonFrameCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonFrameCodec").finish_non_exhaustive()
    }
}

impl Default for JsonFrameCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonFrameCodec {
    pub fn new() -> Self {
        let frames = LengthDelimitedCodec::builder()
            .length_field_length(4)
            .length_field_offset(0)
            .max_frame_length(MAX_MESSAGE_BYTES)
            .big_endian()
            .new_codec();
        Self { frames }
    }
}

impl Decoder for JsonFrameCodec {
    type Item = Message;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let frame = match self.frames.decode(src) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(e) => {
                // tokio_util returns an io::Error::InvalidData for oversized frames.
                if e.kind() == io::ErrorKind::InvalidData {
                    return Err(CodecError::FrameTooLarge {
                        size: usize::MAX,
                        max: MAX_MESSAGE_BYTES,
                    });
                }
                return Err(CodecError::Io(e));
            }
        };
        let msg: Message = serde_json::from_slice(&frame).map_err(CodecError::Json)?;
        Ok(Some(msg))
    }
}

impl Encoder<Message> for JsonFrameCodec {
    type Error = CodecError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = serde_json::to_vec(&item).map_err(CodecError::Json)?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(CodecError::FrameTooLarge { size: bytes.len(), max: MAX_MESSAGE_BYTES });
        }
        self.frames.encode(bytes.into(), dst).map_err(CodecError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Notification, Request};

    #[test]
    fn encode_decode_request_roundtrip() {
        let mut codec = JsonFrameCodec::new();
        let req = Request::new("ping", serde_json::json!({}));
        let mut buf = BytesMut::new();
        codec.encode(Message::Request(req.clone()), &mut buf).unwrap();

        let decoded = codec.decode(&mut buf).unwrap().expect("a frame");
        match decoded {
            Message::Request(r) => {
                assert_eq!(r.method, "ping");
                assert_eq!(r.id, req.id);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn decode_partial_returns_none() {
        let mut codec = JsonFrameCodec::new();
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0, 0, 0, 10]); // claims 10 bytes to follow
        buf.extend_from_slice(b"abc"); // only 3 there
        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none(), "partial frame should not yield a message");
    }

    #[test]
    fn two_messages_in_one_buffer() {
        let mut codec = JsonFrameCodec::new();
        let mut buf = BytesMut::new();
        codec
            .encode(Message::Notification(Notification::new("a", serde_json::json!(1))), &mut buf)
            .unwrap();
        codec
            .encode(Message::Notification(Notification::new("b", serde_json::json!(2))), &mut buf)
            .unwrap();

        let m1 = codec.decode(&mut buf).unwrap().unwrap();
        let m2 = codec.decode(&mut buf).unwrap().unwrap();
        match (m1, m2) {
            (Message::Notification(a), Message::Notification(b)) => {
                assert_eq!(a.method, "a");
                assert_eq!(b.method, "b");
            }
            _ => panic!("expected two notifications"),
        }
    }

    #[test]
    fn bad_json_surfaces_error() {
        let mut codec = JsonFrameCodec::new();
        let mut buf = BytesMut::new();
        let payload = b"not json";
        let len = (payload.len() as u32).to_be_bytes();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(payload);
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(err, CodecError::Json(_)));
    }
}
