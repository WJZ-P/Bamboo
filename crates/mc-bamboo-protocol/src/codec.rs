//! Primitive types of the network protocol.

/// The longest encoding of a 32-bit VarInt.
pub const MAX_VARINT_LEN: usize = 5;

/// Why a packet could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("packet ended early")]
    UnexpectedEnd,
    #[error("VarInt is longer than 5 bytes")]
    VarIntTooLong,
    #[error("string has invalid length {0}")]
    InvalidStringLength(i32),
    #[error("string is longer than {max_chars} characters")]
    StringTooLong { max_chars: usize },
    #[error("string is not valid UTF-8")]
    InvalidUtf8,
    #[error("packet has {0} trailing bytes")]
    TrailingBytes(usize),
    #[error("unknown packet id {0:#04x}")]
    UnknownPacket(i32),
    #[error("unknown handshake intent {0}")]
    UnknownIntent(i32),
}

/// Decodes a VarInt from the start of `input`, returning its value and encoded length.
pub fn decode_varint(input: &[u8]) -> Result<(i32, usize), DecodeError> {
    let mut value = 0u32;
    for (index, &byte) in input.iter().take(MAX_VARINT_LEN).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((value as i32, index + 1));
        }
    }
    if input.len() < MAX_VARINT_LEN {
        Err(DecodeError::UnexpectedEnd)
    } else {
        Err(DecodeError::VarIntTooLong)
    }
}

/// Encodes `value`, returning a buffer and how many of its leading bytes are used.
pub fn encode_varint(value: i32) -> ([u8; MAX_VARINT_LEN], usize) {
    let mut bytes = [0; MAX_VARINT_LEN];
    let mut remaining = value as u32;
    let mut len = 0;
    while remaining >= 0x80 {
        bytes[len] = remaining as u8 | 0x80;
        remaining >>= 7;
        len += 1;
    }
    bytes[len] = remaining as u8;
    (bytes, len + 1)
}

/// Appends `value` to `out` as a VarInt.
pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let (bytes, len) = encode_varint(value);
    out.extend_from_slice(&bytes[..len]);
}

/// Appends `value` to `out` as a length-prefixed UTF-8 string.
pub fn write_string(out: &mut Vec<u8>, value: &str) {
    write_varint(out, value.len() as i32);
    out.extend_from_slice(value.as_bytes());
}

/// A cursor over the body of one packet.
#[derive(Debug)]
pub struct Reader<'a> {
    input: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input }
    }

    pub fn varint(&mut self) -> Result<i32, DecodeError> {
        let (value, len) = decode_varint(self.input)?;
        self.input = &self.input[len..];
        Ok(value)
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    /// Reads a string of at most `max_chars` UTF-16 code units, the unit the game counts in.
    pub fn string(&mut self, max_chars: usize) -> Result<&'a str, DecodeError> {
        let encoded_len = self.varint()?;
        let len = usize::try_from(encoded_len)
            .map_err(|_| DecodeError::InvalidStringLength(encoded_len))?;
        // One UTF-16 code unit takes at most three UTF-8 bytes.
        if len > max_chars * 3 {
            return Err(DecodeError::StringTooLong { max_chars });
        }

        let (bytes, rest) = self
            .input
            .split_at_checked(len)
            .ok_or(DecodeError::UnexpectedEnd)?;
        let string = std::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
        // UTF-8 is never shorter than UTF-16, so only long strings need counting.
        if len > max_chars && string.encode_utf16().count() > max_chars {
            return Err(DecodeError::StringTooLong { max_chars });
        }

        self.input = rest;
        Ok(string)
    }

    /// Ends decoding. A packet with bytes left over is malformed.
    pub fn finish(self) -> Result<(), DecodeError> {
        match self.input.len() {
            0 => Ok(()),
            trailing => Err(DecodeError::TrailingBytes(trailing)),
        }
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (bytes, rest) = self
            .input
            .split_first_chunk::<N>()
            .ok_or(DecodeError::UnexpectedEnd)?;
        self.input = rest;
        Ok(*bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARINT_VECTORS: &[(i32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (255, &[0xff, 0x01]),
        (25565, &[0xdd, 0xc7, 0x01]),
        (2_097_151, &[0xff, 0xff, 0x7f]),
        (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        (i32::MIN, &[0x80, 0x80, 0x80, 0x80, 0x08]),
    ];

    fn encoded_string(value: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, value);
        out
    }

    #[test]
    fn varint_matches_known_encodings() {
        for &(value, encoded) in VARINT_VECTORS {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            assert_eq!(out, encoded, "encoding {value}");
            assert_eq!(
                decode_varint(encoded),
                Ok((value, encoded.len())),
                "decoding {value}"
            );
        }
    }

    #[test]
    fn varint_ignores_bytes_after_its_end() {
        assert_eq!(decode_varint(&[0x01, 0xff, 0xff]), Ok((1, 1)));
    }

    #[test]
    fn varint_rejects_truncated_input() {
        assert_eq!(decode_varint(&[]), Err(DecodeError::UnexpectedEnd));
        assert_eq!(decode_varint(&[0x80]), Err(DecodeError::UnexpectedEnd));
        assert_eq!(
            decode_varint(&[0xff, 0xff, 0xff, 0xff]),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn varint_rejects_more_than_five_bytes() {
        assert_eq!(
            decode_varint(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]),
            Err(DecodeError::VarIntTooLong)
        );
    }

    #[test]
    fn string_round_trips_ascii_and_unicode() {
        for value in ["", "localhost", "竹子", "🎋"] {
            let encoded = encoded_string(value);
            let mut reader = Reader::new(&encoded);
            assert_eq!(reader.string(16), Ok(value));
            assert_eq!(reader.finish(), Ok(()));
        }
    }

    #[test]
    fn string_limit_counts_utf16_code_units() {
        // Three bytes in UTF-8 but one UTF-16 code unit.
        let encoded = encoded_string("竹");
        assert_eq!(Reader::new(&encoded).string(1), Ok("竹"));

        // Four bytes in UTF-8 and two UTF-16 code units.
        let encoded = encoded_string("🎋");
        assert_eq!(
            Reader::new(&encoded).string(1),
            Err(DecodeError::StringTooLong { max_chars: 1 })
        );
        assert_eq!(Reader::new(&encoded).string(2), Ok("🎋"));
    }

    #[test]
    fn string_rejects_a_length_over_the_byte_limit_before_reading() {
        let mut encoded = Vec::new();
        write_varint(&mut encoded, 4);
        assert_eq!(
            Reader::new(&encoded).string(1),
            Err(DecodeError::StringTooLong { max_chars: 1 })
        );
    }

    #[test]
    fn string_rejects_negative_length() {
        let mut encoded = Vec::new();
        write_varint(&mut encoded, -1);
        assert_eq!(
            Reader::new(&encoded).string(16),
            Err(DecodeError::InvalidStringLength(-1))
        );
    }

    #[test]
    fn string_rejects_invalid_utf8() {
        assert_eq!(
            Reader::new(&[0x02, 0xc3, 0x28]).string(16),
            Err(DecodeError::InvalidUtf8)
        );
    }

    #[test]
    fn string_rejects_missing_bytes() {
        assert_eq!(
            Reader::new(&[0x05, b'a']).string(16),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn integers_are_big_endian() {
        let mut reader = Reader::new(&[0x63, 0xdd, 0, 0, 0, 0, 0, 0, 0, 0x2a]);
        assert_eq!(reader.u16(), Ok(25565));
        assert_eq!(reader.i64(), Ok(42));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn finish_reports_trailing_bytes() {
        assert_eq!(
            Reader::new(&[1, 2, 3]).finish(),
            Err(DecodeError::TrailingBytes(3))
        );
    }
}
