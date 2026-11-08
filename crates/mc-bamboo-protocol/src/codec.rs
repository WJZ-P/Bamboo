//! 网络协议的基础数据类型。

use mc_bamboo_core::{BlockPos, Vec3};
use uuid::Uuid;

/// 32 位 VarInt 编码后的最大长度。
pub const MAX_VARINT_LEN: usize = 5;
/// 64 位 VarLong 编码后的最大长度。
pub const MAX_VARLONG_LEN: usize = 10;

/// 数据包解码失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("packet ended early")]
    UnexpectedEnd,
    #[error("VarInt is longer than 5 bytes")]
    VarIntTooLong,
    #[error("VarLong is longer than 10 bytes")]
    VarLongTooLong,
    #[error("string has invalid length {0}")]
    InvalidStringLength(i32),
    #[error("string is longer than {max_chars} characters")]
    StringTooLong { max_chars: usize },
    #[error("string is not valid UTF-8")]
    InvalidUtf8,
    #[error("packet has {0} trailing bytes")]
    TrailingBytes(usize),
    #[error("invalid element count {0}")]
    InvalidCount(i32),
    #[error("unknown packet id {0:#04x}")]
    UnknownPacket(i32),
    #[error("unknown handshake intent {0}")]
    UnknownIntent(i32),
    #[error("enum value {0} is out of range")]
    InvalidEnum(i32),
    #[error("invalid item id {0}")]
    InvalidItem(i32),
    #[error("invalid item count {0}")]
    InvalidItemCount(i32),
    #[error("malformed NBT: {0}")]
    Nbt(#[from] mc_bamboo_nbt::DecodeError),
    #[error("not a valid resource location")]
    InvalidIdentifier,
    #[error("unknown data component type {0}")]
    UnknownComponent(i32),
    #[error("data component type {0} appears twice in one patch")]
    DuplicateComponent(i32),
    #[error("id {id} is not registered in {registry}")]
    UnknownRegistryId { registry: &'static str, id: i32 },
    #[error("{registry} has no tag with this name")]
    UnknownTag { registry: &'static str },
    #[error("item stacks are nested deeper than {max_depth}")]
    StacksTooDeep { max_depth: u32 },
    #[error("component value is out of range: {0}")]
    InvalidComponentValue(&'static str),
    #[error("malformed text component: {0}")]
    InvalidTextComponent(&'static str),
}

/// 从 `input` 开头解码一个 VarInt，返回它的值和占用的字节数。
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

/// 编码 `value`，返回缓冲区和其中实际使用的字节数。
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

/// 把 `value` 以 VarInt 形式追加到 `out`。
pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let (bytes, len) = encode_varint(value);
    out.extend_from_slice(&bytes[..len]);
}

/// 把 `value` 以带长度前缀的 UTF-8 字符串形式追加到 `out`。
pub fn write_string(out: &mut Vec<u8>, value: &str) {
    write_varint(out, value.len() as i32);
    out.extend_from_slice(value.as_bytes());
}

/// 把 `value` 以带长度前缀的字节数组形式追加到 `out`。
pub fn write_byte_array(out: &mut Vec<u8>, value: &[u8]) {
    write_varint(out, value.len() as i32);
    out.extend_from_slice(value);
}

/// 把 `value` 以 VarLong 形式追加到 `out`。
pub fn write_varlong(out: &mut Vec<u8>, value: i64) {
    let mut remaining = value as u64;
    while remaining >= 0x80 {
        out.push(remaining as u8 | 0x80);
        remaining >>= 7;
    }
    out.push(remaining as u8);
}

/// 把布尔值写成一个字节。
pub fn write_bool(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}

/// 把 UUID 写成 16 个字节。
pub fn write_uuid(out: &mut Vec<u8>, value: Uuid) {
    out.extend_from_slice(value.as_bytes());
}

/// 把 `value` 按大端写成 2 个字节。
pub fn write_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// 把 `value` 按大端写成 4 个字节。
pub fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// 把 `value` 按大端写成 8 个字节。
pub fn write_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// 把 `value` 按大端写成 4 个字节。
pub fn write_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// 把 `value` 按大端写成 8 个字节。
pub fn write_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// 把方块坐标写成一个 64 位整数。
pub fn write_block_pos(out: &mut Vec<u8>, pos: BlockPos) {
    write_i64(out, pos.pack());
}

/// 依次写入 x、y、z 三个 Double。
pub fn write_vec3(out: &mut Vec<u8>, value: Vec3) {
    for coordinate in [value.x, value.y, value.z] {
        write_f64(out, coordinate);
    }
}

/// 单个数据包内容的读取游标。
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

    pub fn varlong(&mut self) -> Result<i64, DecodeError> {
        let mut value = 0u64;
        for index in 0..MAX_VARLONG_LEN {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(value as i64);
            }
        }
        Err(DecodeError::VarLongTooLong)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        let [value] = self.array()?;
        Ok(value)
    }

    pub fn i8(&mut self) -> Result<i8, DecodeError> {
        Ok(i8::from_be_bytes(self.array()?))
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_be_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_be_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_be_bytes(self.array()?))
    }

    /// 和官方一样，非零字节都算 `true`。
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.u8()? != 0)
    }

    pub fn uuid(&mut self) -> Result<Uuid, DecodeError> {
        Ok(Uuid::from_bytes(self.array()?))
    }

    pub fn block_pos(&mut self) -> Result<BlockPos, DecodeError> {
        Ok(BlockPos::unpack(self.i64()?))
    }

    /// 取走接下来的 `len` 个字节。
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let (bytes, rest) = self
            .input
            .split_at_checked(len)
            .ok_or(DecodeError::UnexpectedEnd)?;
        self.input = rest;
        Ok(bytes)
    }

    /// 读取带长度前缀的字节数组，长度限制在 `max` 以内。
    pub fn byte_array(&mut self, max: usize) -> Result<&'a [u8], DecodeError> {
        let len = self.count(max)?;
        self.bytes(len)
    }

    /// 读取列表的元素个数，并限制在 `max` 以内。
    pub fn count(&mut self, max: usize) -> Result<usize, DecodeError> {
        let count = self.varint()?;
        usize::try_from(count)
            .ok()
            .filter(|&count| count <= max)
            .ok_or(DecodeError::InvalidCount(count))
    }

    /// 读取长度不超过 `max_chars` 的字符串。游戏按 UTF-16 码元计数，这里保持一致。
    pub fn string(&mut self, max_chars: usize) -> Result<&'a str, DecodeError> {
        let encoded_len = self.varint()?;
        let len = usize::try_from(encoded_len)
            .map_err(|_| DecodeError::InvalidStringLength(encoded_len))?;
        // 一个 UTF-16 码元最多占三个 UTF-8 字节。
        if len > max_chars * 3 {
            return Err(DecodeError::StringTooLong { max_chars });
        }

        let (bytes, rest) = self
            .input
            .split_at_checked(len)
            .ok_or(DecodeError::UnexpectedEnd)?;
        let string = std::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
        // UTF-8 的字节数不会少于 UTF-16 码元数，所以只有较长的字符串才需要实际计数。
        if len > max_chars && string.encode_utf16().count() > max_chars {
            return Err(DecodeError::StringTooLong { max_chars });
        }

        self.input = rest;
        Ok(string)
    }

    /// 取走还没读的全部字节。
    pub fn rest(self) -> &'a [u8] {
        self.input
    }

    /// 还没读的字节，不移动游标。
    pub const fn remaining(&self) -> &'a [u8] {
        self.input
    }

    /// 结束解码。还有剩余字节的数据包是畸形的。
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
    fn varlong_matches_known_encodings() {
        let cases: [(i64, &[u8]); 4] = [
            (0, &[0x00]),
            (300, &[0xac, 0x02]),
            (
                i64::MAX,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
            ),
            (
                -1,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
        ];
        for (value, encoded) in cases {
            let mut out = Vec::new();
            write_varlong(&mut out, value);
            assert_eq!(out, encoded, "encoding {value}");
            let mut reader = Reader::new(encoded);
            assert_eq!(reader.varlong(), Ok(value), "decoding {value}");
            assert_eq!(reader.finish(), Ok(()));
        }
    }

    #[test]
    fn varlong_rejects_truncated_and_overlong_input() {
        assert_eq!(
            Reader::new(&[0x80, 0x80]).varlong(),
            Err(DecodeError::UnexpectedEnd)
        );
        assert_eq!(
            Reader::new(&[0x80; MAX_VARLONG_LEN + 1]).varlong(),
            Err(DecodeError::VarLongTooLong)
        );
    }

    #[test]
    fn uuid_round_trips() {
        let value = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        let mut out = Vec::new();
        write_uuid(&mut out, value);
        assert_eq!(Reader::new(&out).uuid(), Ok(value));
    }

    #[test]
    fn count_rejects_negative_and_oversized_values() {
        assert_eq!(Reader::new(&[3]).count(3), Ok(3));
        assert_eq!(
            Reader::new(&[4]).count(3),
            Err(DecodeError::InvalidCount(4))
        );

        let mut negative = Vec::new();
        write_varint(&mut negative, -1);
        assert_eq!(
            Reader::new(&negative).count(3),
            Err(DecodeError::InvalidCount(-1))
        );
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
        // UTF-8 占三个字节，但只有一个 UTF-16 码元。
        let encoded = encoded_string("竹");
        assert_eq!(Reader::new(&encoded).string(1), Ok("竹"));

        // UTF-8 占四个字节，对应两个 UTF-16 码元。
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
    fn fixed_width_numbers_round_trip() {
        let mut out = Vec::new();
        write_i16(&mut out, -2);
        write_i32(&mut out, -3);
        write_i64(&mut out, -4);
        write_f32(&mut out, 1.5);
        write_f64(&mut out, -2.5);

        let mut reader = Reader::new(&out);
        assert_eq!(reader.i16(), Ok(-2));
        assert_eq!(reader.i32(), Ok(-3));
        assert_eq!(reader.i64(), Ok(-4));
        assert_eq!(reader.f32(), Ok(1.5));
        assert_eq!(reader.f64(), Ok(-2.5));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn signed_bytes_keep_their_sign() {
        assert_eq!(Reader::new(&[0xff]).i8(), Ok(-1));
    }

    #[test]
    fn any_non_zero_byte_is_true() {
        assert_eq!(Reader::new(&[0]).bool(), Ok(false));
        assert_eq!(Reader::new(&[1]).bool(), Ok(true));
        assert_eq!(Reader::new(&[0x80]).bool(), Ok(true));
    }

    #[test]
    fn block_pos_round_trips() {
        let pos = BlockPos::new(18, -61, -7);
        let mut out = Vec::new();
        write_block_pos(&mut out, pos);

        assert_eq!(out, [0x00, 0x00, 0x04, 0xbf, 0xff, 0xff, 0x9f, 0xc3]);
        assert_eq!(Reader::new(&out).block_pos(), Ok(pos));
    }

    #[test]
    fn bytes_takes_exactly_what_was_asked() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(reader.bytes(2), Ok(&[1, 2][..]));
        assert_eq!(reader.bytes(2), Err(DecodeError::UnexpectedEnd));
        assert_eq!(reader.u8(), Ok(3));
    }

    #[test]
    fn byte_arrays_round_trip() {
        let mut out = Vec::new();
        write_byte_array(&mut out, &[]);
        write_byte_array(&mut out, &[7, 8, 9]);
        assert_eq!(out, [0x00, 0x03, 7, 8, 9]);

        let mut reader = Reader::new(&out);
        assert_eq!(reader.byte_array(16), Ok(&[][..]));
        assert_eq!(reader.byte_array(16), Ok(&[7, 8, 9][..]));
        reader.finish().expect("读完了");
    }

    #[test]
    fn byte_array_rejects_a_length_it_cannot_trust() {
        // 超过上限的长度，连后面的字节都不看。
        assert_eq!(
            Reader::new(&[0x04, 1, 2, 3, 4]).byte_array(3),
            Err(DecodeError::InvalidCount(4))
        );
        assert_eq!(
            Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x0f]).byte_array(3),
            Err(DecodeError::InvalidCount(-1))
        );
        assert_eq!(
            Reader::new(&[0x04, 1, 2]).byte_array(16),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn finish_reports_trailing_bytes() {
        assert_eq!(
            Reader::new(&[1, 2, 3]).finish(),
            Err(DecodeError::TrailingBytes(3))
        );
    }
}
