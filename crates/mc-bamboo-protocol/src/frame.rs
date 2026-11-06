//! Length-prefixed frames that carry packets over TCP.

use crate::{
    codec::{decode_varint, encode_varint, write_varint},
    packet::ClientboundPacket,
};

/// Frame lengths are limited to 21 bits, which fit in three VarInt bytes.
pub const MAX_FRAME_LEN: usize = (1 << 21) - 1;

const MAX_HEADER_LEN: usize = 3;

/// Why a byte stream is not a valid sequence of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame length is wider than 21 bits")]
    LengthTooWide,
    #[error("frame length cannot be zero")]
    Empty,
    #[error("frame of {len} bytes exceeds the limit of {max} bytes")]
    TooLong { len: usize, max: usize },
}

/// The extent of one complete frame at the start of a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameBounds {
    pub header_len: usize,
    pub payload_len: usize,
}

/// Looks for a complete frame at the start of `buffer`.
///
/// Returns `Ok(None)` while more bytes are needed. A frame over `max_len` is rejected as soon
/// as its header arrives, so its payload never has to be buffered.
pub fn find_frame(buffer: &[u8], max_len: usize) -> Result<Option<FrameBounds>, FrameError> {
    let header = &buffer[..buffer.len().min(MAX_HEADER_LEN)];
    let (payload_len, header_len) = match decode_varint(header) {
        Ok((len, header_len)) => (len as usize, header_len),
        Err(_) if header.len() == MAX_HEADER_LEN => return Err(FrameError::LengthTooWide),
        Err(_) => return Ok(None),
    };

    if payload_len == 0 {
        return Err(FrameError::Empty);
    }
    if payload_len > max_len {
        return Err(FrameError::TooLong {
            len: payload_len,
            max: max_len,
        });
    }
    if buffer.len() < header_len + payload_len {
        return Ok(None);
    }
    Ok(Some(FrameBounds {
        header_len,
        payload_len,
    }))
}

/// Appends `packet` to `out` as one frame.
///
/// Appending lets several packets share a buffer and reach the socket in a single write.
/// On error `out` is left as it was.
pub fn append_frame<P: ClientboundPacket>(out: &mut Vec<u8>, packet: &P) -> Result<(), FrameError> {
    let frame_start = out.len();
    out.extend_from_slice(&[0; MAX_HEADER_LEN]);
    let payload_start = out.len();
    write_varint(out, P::ID);
    packet.write_body(out);

    let payload_len = out.len() - payload_start;
    if payload_len > MAX_FRAME_LEN {
        out.truncate(frame_start);
        return Err(FrameError::TooLong {
            len: payload_len,
            max: MAX_FRAME_LEN,
        });
    }

    // The length is only known now, so close the gap left by a header under three bytes.
    let (header, header_len) = encode_varint(payload_len as i32);
    out[frame_start..frame_start + header_len].copy_from_slice(&header[..header_len]);
    out.copy_within(payload_start.., frame_start + header_len);
    out.truncate(frame_start + header_len + payload_len);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet whose encoded body is `len` zero bytes.
    struct Filler {
        len: usize,
    }

    impl ClientboundPacket for Filler {
        const ID: i32 = 0x2a;

        fn write_body(&self, out: &mut Vec<u8>) {
            out.resize(out.len() + self.len, 0);
        }
    }

    #[test]
    fn waits_for_a_complete_header() {
        assert_eq!(find_frame(&[], MAX_FRAME_LEN), Ok(None));
        assert_eq!(find_frame(&[0x80], MAX_FRAME_LEN), Ok(None));
        assert_eq!(find_frame(&[0x80, 0x80], MAX_FRAME_LEN), Ok(None));
    }

    #[test]
    fn waits_for_a_complete_payload() {
        assert_eq!(find_frame(&[0x03, 1, 2], MAX_FRAME_LEN), Ok(None));
    }

    #[test]
    fn finds_a_complete_frame_and_ignores_what_follows() {
        let bounds = FrameBounds {
            header_len: 1,
            payload_len: 2,
        };
        assert_eq!(find_frame(&[0x02, 1, 2], MAX_FRAME_LEN), Ok(Some(bounds)));
        assert_eq!(
            find_frame(&[0x02, 1, 2, 0x05, 9], MAX_FRAME_LEN),
            Ok(Some(bounds))
        );
    }

    #[test]
    fn rejects_zero_length() {
        assert_eq!(find_frame(&[0x00], MAX_FRAME_LEN), Err(FrameError::Empty));
    }

    #[test]
    fn rejects_a_length_wider_than_21_bits() {
        assert_eq!(
            find_frame(&[0x80, 0x80, 0x80, 0x01], MAX_FRAME_LEN),
            Err(FrameError::LengthTooWide)
        );
    }

    #[test]
    fn rejects_an_oversized_frame_from_its_header_alone() {
        assert_eq!(
            find_frame(&[0xff, 0xff, 0x7f], 1024),
            Err(FrameError::TooLong {
                len: MAX_FRAME_LEN,
                max: 1024
            })
        );
    }

    #[test]
    fn appended_frames_can_be_found_at_every_header_width() {
        // Payload lengths are one larger than the body because of the packet id.
        for body_len in [0, 126, 127, 16_382, 16_383, MAX_FRAME_LEN - 1] {
            let mut out = vec![0xee];
            append_frame(&mut out, &Filler { len: body_len }).unwrap();

            assert_eq!(out[0], 0xee, "bytes before the frame must stay intact");
            let frame = &out[1..];
            let bounds = find_frame(frame, MAX_FRAME_LEN).unwrap().unwrap();
            assert_eq!(bounds.payload_len, body_len + 1);
            assert_eq!(frame.len(), bounds.header_len + bounds.payload_len);
            assert_eq!(frame[bounds.header_len], 0x2a);
            assert!(frame[bounds.header_len + 1..].iter().all(|&byte| byte == 0));
        }
    }

    #[test]
    fn appends_several_frames_to_one_buffer() {
        let mut out = Vec::new();
        append_frame(&mut out, &Filler { len: 1 }).unwrap();
        append_frame(&mut out, &Filler { len: 2 }).unwrap();
        assert_eq!(out, [0x02, 0x2a, 0, 0x03, 0x2a, 0, 0]);
    }

    #[test]
    fn oversized_packet_leaves_the_buffer_unchanged() {
        let mut out = vec![1, 2, 3];
        assert_eq!(
            append_frame(&mut out, &Filler { len: MAX_FRAME_LEN }),
            Err(FrameError::TooLong {
                len: MAX_FRAME_LEN + 1,
                max: MAX_FRAME_LEN
            })
        );
        assert_eq!(out, [1, 2, 3]);
    }
}
