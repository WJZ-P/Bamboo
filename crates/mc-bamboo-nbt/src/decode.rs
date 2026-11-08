//! NBT 解码，带官方 `NbtAccounter` 的配额和深度上限。
//!
//! 官方 `FriendlyByteBuf.readNbt` 读的是「类型字节加内容」，根节点没有名字，
//! 类型 0（`TAG_End`）表示没有标签。读出来的条目保持出现顺序：官方的 `CompoundTag`
//! 用 `HashMap`，写出去的键序由哈希决定，我们复述读到的顺序才能原样写回。

use crate::Tag;

/// 官方 `NbtAccounter.DEFAULT_NBT_QUOTA`，网络上的默认配额。
pub const DEFAULT_QUOTA: u64 = 2_097_152;

/// 官方 `NbtAccounter.MAX_STACK_DEPTH`。
pub const MAX_DEPTH: u32 = 512;

/// NBT 解码失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("NBT ended early")]
    UnexpectedEnd,
    #[error("unknown NBT tag type {0}")]
    UnknownTagType(u8),
    #[error("NBT list length cannot be negative: {0}")]
    NegativeLength(i32),
    #[error("NBT list has no element type but {0} elements")]
    MissingListType(i32),
    #[error("NBT tag is too big for the {quota} byte quota")]
    QuotaExceeded { quota: u64 },
    #[error("NBT tag is nested deeper than {max_depth}")]
    TooDeep { max_depth: u32 },
    #[error("NBT string is not valid modified UTF-8")]
    InvalidString,
    #[error("expected a TAG_Compound, found tag type {0}")]
    NotACompound(u8),
    #[error("expected a tag, found TAG_End")]
    EndTag,
}

/// 官方的 `NbtAccounter`：一次解码里所有标签共用一份配额和深度计数。
#[derive(Debug)]
pub struct Accounter {
    quota: u64,
    usage: u64,
    max_depth: u32,
    depth: u32,
}

impl Accounter {
    /// 官方 `NbtAccounter.create`。
    pub const fn new(quota: u64) -> Self {
        Self {
            quota,
            usage: 0,
            max_depth: MAX_DEPTH,
            depth: 0,
        }
    }

    /// 官方 `NbtAccounter.defaultQuota`，网络上大多数地方用它。
    pub const fn default_quota() -> Self {
        Self::new(DEFAULT_QUOTA)
    }

    /// 官方 `NbtAccounter.unlimitedHeap`，可信数据用它。
    pub const fn unlimited() -> Self {
        Self::new(u64::MAX)
    }

    fn account(&mut self, size: u64) -> Result<(), DecodeError> {
        // 官方用 long 相加，这里的每一项都来自长度前缀，饱和加法不会把超限算成没超限。
        let usage = self.usage.saturating_add(size);
        if usage > self.quota {
            return Err(DecodeError::QuotaExceeded { quota: self.quota });
        }
        self.usage = usage;
        Ok(())
    }

    fn account_each(&mut self, per_entry: u64, count: u64) -> Result<(), DecodeError> {
        self.account(per_entry.saturating_mul(count))
    }

    fn push_depth(&mut self) -> Result<(), DecodeError> {
        if self.depth >= self.max_depth {
            return Err(DecodeError::TooDeep {
                max_depth: self.max_depth,
            });
        }
        self.depth += 1;
        Ok(())
    }

    fn pop_depth(&mut self) {
        self.depth -= 1;
    }

    /// 已经计入的字节数，官方的 `getUsage`。
    pub const fn usage(&self) -> u64 {
        self.usage
    }
}

/// 官方 `ListTag.tryUnwrap`：列表里只有一个空名字键的复合标签是包装，读进来就拆掉。
///
/// 官方用这个办法在一个只能存同类元素的 `TAG_List` 里放类型不一样的值。
/// 内存里存的是拆开的值，写出去时 `ListTag.write` 会按需要重新包上。
fn unwrap_list_element(tag: Tag) -> Tag {
    match tag {
        Tag::Compound(mut entries) if entries.len() == 1 && entries[0].0.is_empty() => {
            entries.remove(0).1
        }
        other => other,
    }
}

/// 一段 NBT 的读取游标。
#[derive(Debug)]
pub struct Reader<'a> {
    input: &'a [u8],
    read: usize,
    accounter: Accounter,
}

impl<'a> Reader<'a> {
    pub const fn new(input: &'a [u8], accounter: Accounter) -> Self {
        Self {
            input,
            read: 0,
            accounter,
        }
    }

    /// 已经读掉的字节数。
    pub const fn read(&self) -> usize {
        self.read
    }

    /// 官方 `NbtIo.readAnyTag`：读类型字节，0 表示没有标签。
    pub fn any_tag(&mut self) -> Result<Option<Tag>, DecodeError> {
        let kind = self.u8()?;
        if kind == 0 {
            return Ok(None);
        }
        Ok(Some(self.payload(kind)?))
    }

    /// 官方 `ByteBufCodecs.COMPOUND_TAG`：必须是一个非空类型的 `TAG_Compound`。
    pub fn compound(&mut self) -> Result<Vec<(String, Tag)>, DecodeError> {
        let kind = self.u8()?;
        match kind {
            0 => Err(DecodeError::EndTag),
            10 => self.compound_payload(),
            other => {
                // 官方先完整读出标签再发现类型不对，所以畸形深度也要在这里挡住。
                self.payload(other)?;
                Err(DecodeError::NotACompound(other))
            }
        }
    }

    /// 官方 `ByteBufCodecs.TAG`：必须有标签，类型不限。
    pub fn tag(&mut self) -> Result<Tag, DecodeError> {
        self.any_tag()?.ok_or(DecodeError::EndTag)
    }

    fn payload(&mut self, kind: u8) -> Result<Tag, DecodeError> {
        match kind {
            1 => {
                self.accounter.account(9)?;
                Ok(Tag::Byte(self.i8()?))
            }
            2 => {
                self.accounter.account(10)?;
                Ok(Tag::Short(i16::from_be_bytes(self.array()?)))
            }
            3 => {
                self.accounter.account(12)?;
                Ok(Tag::Int(i32::from_be_bytes(self.array()?)))
            }
            4 => {
                self.accounter.account(16)?;
                Ok(Tag::Long(i64::from_be_bytes(self.array()?)))
            }
            5 => {
                self.accounter.account(12)?;
                Ok(Tag::Float(f32::from_be_bytes(self.array()?)))
            }
            6 => {
                self.accounter.account(16)?;
                Ok(Tag::Double(f64::from_be_bytes(self.array()?)))
            }
            7 => {
                self.accounter.account(24)?;
                let len = self.array_len(1)?;
                let bytes = self.bytes(len)?;
                Ok(Tag::ByteArray(bytes.iter().map(|&b| b as i8).collect()))
            }
            8 => Ok(Tag::String(self.string()?)),
            9 => {
                self.accounter.push_depth()?;
                let list = self.list_payload();
                self.accounter.pop_depth();
                list.map(Tag::List)
            }
            10 => {
                self.accounter.push_depth()?;
                let entries = self.compound_payload();
                self.accounter.pop_depth();
                entries.map(Tag::Compound)
            }
            11 => {
                self.accounter.account(24)?;
                let len = self.array_len(4)?;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(i32::from_be_bytes(self.array()?));
                }
                Ok(Tag::IntArray(values))
            }
            12 => {
                self.accounter.account(24)?;
                let len = self.array_len(8)?;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(i64::from_be_bytes(self.array()?));
                }
                Ok(Tag::LongArray(values))
            }
            other => Err(DecodeError::UnknownTagType(other)),
        }
    }

    /// 数组的长度前缀。官方的 `accountBytes(每项字节数, 长度)` 遇到负数会抛异常。
    fn array_len(&mut self, per_entry: u64) -> Result<usize, DecodeError> {
        let len = i32::from_be_bytes(self.array()?);
        let len = usize::try_from(len).map_err(|_| DecodeError::NegativeLength(len))?;
        self.accounter.account_each(per_entry, len as u64)?;
        // 配额已经限制了长度，但不限配额时还要靠剩余字节数兜住一次大分配。
        if len.saturating_mul(per_entry as usize) > self.input.len() - self.read {
            return Err(DecodeError::UnexpectedEnd);
        }
        Ok(len)
    }

    fn list_payload(&mut self) -> Result<Vec<Tag>, DecodeError> {
        self.accounter.account(36)?;
        let kind = self.u8()?;
        let count = i32::from_be_bytes(self.array()?);
        if count < 0 {
            return Err(DecodeError::NegativeLength(count));
        }
        if kind == 0 && count > 0 {
            return Err(DecodeError::MissingListType(count));
        }
        self.accounter.account_each(4, count as u64)?;

        let mut values = Vec::new();
        for _ in 0..count {
            values.push(unwrap_list_element(self.payload(kind)?));
        }
        Ok(values)
    }

    fn compound_payload(&mut self) -> Result<Vec<(String, Tag)>, DecodeError> {
        self.accounter.account(48)?;
        let mut entries: Vec<(String, Tag)> = Vec::new();
        loop {
            let kind = self.u8()?;
            if kind == 0 {
                return Ok(entries);
            }
            let key = self.key()?;
            let value = self.payload(kind)?;
            // 官方用 `Map.put`：重复的键只替换值，不再记一份条目开销。
            match entries.iter_mut().find(|(name, _)| *name == key) {
                Some(entry) => entry.1 = value,
                None => {
                    self.accounter.account(36)?;
                    entries.push((key, value));
                }
            }
        }
    }

    /// 官方 `CompoundTag.readString`：先读字符串，再按它的长度记账。
    fn key(&mut self) -> Result<String, DecodeError> {
        let key = self.modified_utf8()?;
        self.accounter.account(28)?;
        self.accounter
            .account_each(2, key.encode_utf16().count() as u64)?;
        Ok(key)
    }

    /// 官方 `StringTag` 的 `readAccounted`：先记自身大小，再读字符串。
    fn string(&mut self) -> Result<String, DecodeError> {
        self.accounter.account(36)?;
        let value = self.modified_utf8()?;
        self.accounter
            .account_each(2, value.encode_utf16().count() as u64)?;
        Ok(value)
    }

    /// Java 的 `DataInput.readUTF`：两字节长度，内容是 Modified UTF-8。
    ///
    /// 超出基本平面的字符是两个代理项各占三字节，这里要重新合成一个 `char`。
    /// Java 允许落单的代理项，Rust 的 `String` 放不下，这种内容只能报错。
    fn modified_utf8(&mut self) -> Result<String, DecodeError> {
        let len = usize::from(u16::from_be_bytes(self.array()?));
        let bytes = self.bytes(len)?;

        let mut units: Vec<u16> = Vec::with_capacity(len);
        let mut index = 0;
        while index < bytes.len() {
            let first = bytes[index];
            let (unit, width) = match first >> 4 {
                0..=7 => (u16::from(first), 1),
                12 | 13 => {
                    let second = *bytes.get(index + 1).ok_or(DecodeError::InvalidString)?;
                    if second & 0xc0 != 0x80 {
                        return Err(DecodeError::InvalidString);
                    }
                    ((u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f), 2)
                }
                14 => {
                    let second = *bytes.get(index + 1).ok_or(DecodeError::InvalidString)?;
                    let third = *bytes.get(index + 2).ok_or(DecodeError::InvalidString)?;
                    if second & 0xc0 != 0x80 || third & 0xc0 != 0x80 {
                        return Err(DecodeError::InvalidString);
                    }
                    (
                        (u16::from(first & 0x0f) << 12)
                            | (u16::from(second & 0x3f) << 6)
                            | u16::from(third & 0x3f),
                        3,
                    )
                }
                _ => return Err(DecodeError::InvalidString),
            };
            units.push(unit);
            index += width;
        }

        String::from_utf16(&units).map_err(|_| DecodeError::InvalidString)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        let [value] = self.array()?;
        Ok(value)
    }

    fn i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.u8()? as i8)
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let rest = &self.input[self.read..];
        let bytes = rest.get(..len).ok_or(DecodeError::UnexpectedEnd)?;
        self.read += len;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let rest = &self.input[self.read..];
        let (bytes, _) = rest
            .split_first_chunk::<N>()
            .ok_or(DecodeError::UnexpectedEnd)?;
        self.read += N;
        Ok(*bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> Result<Option<Tag>, DecodeError> {
        let mut reader = Reader::new(bytes, Accounter::default_quota());
        let tag = reader.any_tag()?;
        assert_eq!(reader.read(), bytes.len(), "读完了全部字节");
        Ok(tag)
    }

    /// 写出去再读回来，内容和字节都要一致。
    fn round_trip(tag: &Tag) {
        let mut bytes = Vec::new();
        tag.write_network(&mut bytes);
        assert_eq!(decode(&bytes), Ok(Some(tag.clone())));
    }

    #[test]
    fn end_tag_means_no_tag() {
        assert_eq!(decode(&[0]), Ok(None));
    }

    #[test]
    fn numbers_round_trip() {
        for tag in [
            Tag::Byte(-1),
            Tag::Short(258),
            Tag::Int(i32::MIN),
            Tag::Long(i64::MAX),
            Tag::Float(1.5),
            Tag::Double(-2.5),
        ] {
            round_trip(&tag);
        }
    }

    #[test]
    fn arrays_round_trip() {
        round_trip(&Tag::ByteArray(vec![-1, 0, 1]));
        round_trip(&Tag::IntArray(vec![1, -2]));
        round_trip(&Tag::LongArray(vec![1, -2]));
        round_trip(&Tag::ByteArray(Vec::new()));
    }

    #[test]
    fn strings_round_trip_including_modified_utf8() {
        for value in ["", "hi", "竹子", "🎋", "\0", "a\0竹🎋"] {
            round_trip(&Tag::String(value.to_owned()));
        }
    }

    #[test]
    fn compounds_keep_the_order_they_were_read_in() {
        let tag = Tag::Compound(vec![
            ("b".to_owned(), Tag::Byte(1)),
            ("a".to_owned(), Tag::Int(2)),
        ]);
        round_trip(&tag);
    }

    #[test]
    fn lists_round_trip() {
        round_trip(&Tag::List(vec![Tag::Byte(1), Tag::Byte(2)]));
        round_trip(&Tag::List(Vec::new()));
        round_trip(&Tag::List(vec![Tag::Compound(vec![(
            "a".to_owned(),
            Tag::String("x".to_owned()),
        )])]));
    }

    #[test]
    fn a_repeated_key_keeps_only_the_last_value() {
        // 09 是 TAG_List 的类型号，这里是手写的两个同名键。
        let bytes = [10, 1, 0, 1, b'a', 7, 3, 0, 1, b'a', 0, 0, 0, 9, 0];
        assert_eq!(
            decode(&bytes),
            Ok(Some(Tag::Compound(vec![("a".to_owned(), Tag::Int(9))])))
        );
    }

    #[test]
    fn a_mixed_list_round_trips_through_the_official_wrappers() {
        // 官方把类型不一致的元素各自包进 `{"": 值}`，读的时候拆开，写的时候再包上。
        let mixed = Tag::List(vec![
            Tag::String("a".to_owned()),
            Tag::Int(1),
            Tag::Compound(vec![("text".to_owned(), Tag::String("b".to_owned()))]),
        ]);
        let mut bytes = Vec::new();
        mixed.write_network(&mut bytes);
        // 元素类型是 10，三个元素都带名字为空的外壳。
        assert_eq!(bytes[..6], [9, 10, 0, 0, 0, 3]);
        assert_eq!(bytes[6..12], [8, 0, 0, 0, 1, b'a']);
        assert_eq!(decode(&bytes), Ok(Some(mixed)));
    }

    #[test]
    fn a_wrapper_inside_a_mixed_list_is_wrapped_again() {
        // 内容本身就是 `{"": ...}` 的复合标签不能原样写，否则读回来会被拆掉。
        let wrapper = Tag::Compound(vec![(String::new(), Tag::Byte(1))]);
        let list = Tag::List(vec![wrapper.clone(), Tag::Int(2)]);
        let mut bytes = Vec::new();
        list.write_network(&mut bytes);
        assert_eq!(decode(&bytes), Ok(Some(list)));
    }

    #[test]
    fn rejects_an_unknown_tag_type() {
        assert_eq!(decode(&[13]), Err(DecodeError::UnknownTagType(13)));
        assert_eq!(decode(&[255]), Err(DecodeError::UnknownTagType(255)));
    }

    #[test]
    fn rejects_truncated_input() {
        assert_eq!(decode(&[]), Err(DecodeError::UnexpectedEnd));
        assert_eq!(decode(&[3, 0, 0]), Err(DecodeError::UnexpectedEnd));
        assert_eq!(
            decode(&[10, 1, 0, 1, b'a']),
            Err(DecodeError::UnexpectedEnd)
        );
        // 没有结束标记的复合标签。
        assert_eq!(
            decode(&[10, 1, 0, 1, b'a', 7]),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn rejects_a_negative_length() {
        let bytes = [7, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(decode(&bytes), Err(DecodeError::NegativeLength(-1)));
        let bytes = [9, 1, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(decode(&bytes), Err(DecodeError::NegativeLength(-1)));
    }

    #[test]
    fn rejects_a_list_without_an_element_type() {
        let bytes = [9, 0, 0, 0, 0, 1];
        assert_eq!(decode(&bytes), Err(DecodeError::MissingListType(1)));
        // 空列表可以没有类型。
        let bytes = [9, 0, 0, 0, 0, 0];
        assert_eq!(decode(&bytes), Ok(Some(Tag::List(Vec::new()))));
    }

    #[test]
    fn rejects_a_length_that_cannot_fit_in_the_input() {
        // 配额装得下这个长度，但输入里没有这么多字节。
        let bytes = [11, 0, 0x04, 0, 0];
        assert_eq!(decode(&bytes), Err(DecodeError::UnexpectedEnd));
    }

    #[test]
    fn rejects_deep_nesting() {
        // 512 层列表可以，513 层不行。官方的 `MAX_STACK_DEPTH` 是 512。
        // 列表里的元素不再带类型字节，所以只有最外层写了类型。
        let nest = |lists: usize| {
            let mut bytes = vec![9u8];
            for _ in 1..lists {
                bytes.extend([9, 0, 0, 0, 1]);
            }
            bytes.extend([0, 0, 0, 0, 0]);
            bytes
        };

        let deepest = nest(512);
        let mut reader = Reader::new(&deepest, Accounter::default_quota());
        assert!(reader.any_tag().is_ok());

        let too_deep = nest(513);
        let mut reader = Reader::new(&too_deep, Accounter::default_quota());
        assert_eq!(
            reader.any_tag(),
            Err(DecodeError::TooDeep {
                max_depth: MAX_DEPTH
            })
        );
    }

    #[test]
    fn rejects_a_tag_over_the_quota() {
        let mut bytes = vec![10];
        for index in 0..1000u32 {
            bytes.push(4);
            let name = index.to_string();
            bytes.extend((name.len() as u16).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend(0i64.to_be_bytes());
        }
        bytes.push(0);

        // 默认配额装得下，很小的配额装不下。
        let mut reader = Reader::new(&bytes, Accounter::default_quota());
        assert!(reader.any_tag().is_ok());

        let mut reader = Reader::new(&bytes, Accounter::new(1024));
        assert_eq!(
            reader.any_tag(),
            Err(DecodeError::QuotaExceeded { quota: 1024 })
        );
    }

    #[test]
    fn rejects_a_lone_surrogate() {
        let bytes = [8, 0, 3, 0xed, 0xa0, 0xbc];
        assert_eq!(decode(&bytes), Err(DecodeError::InvalidString));
    }

    #[test]
    fn rejects_a_malformed_string() {
        for body in [[0xf0, 0x80, 0x80], [0xc0, 0x20, 0x20], [0xe0, 0x80, 0x20]] {
            let bytes = [8, 0, 3, body[0], body[1], body[2]];
            assert_eq!(decode(&bytes), Err(DecodeError::InvalidString));
        }
    }

    #[test]
    fn compound_requires_a_compound_tag() {
        let mut reader = Reader::new(&[10, 0], Accounter::default_quota());
        assert_eq!(reader.compound(), Ok(Vec::new()));

        let mut reader = Reader::new(&[0], Accounter::default_quota());
        assert_eq!(reader.compound(), Err(DecodeError::EndTag));

        let mut reader = Reader::new(&[1, 7], Accounter::default_quota());
        assert_eq!(reader.compound(), Err(DecodeError::NotACompound(1)));
    }

    #[test]
    fn usage_counts_every_tag() {
        let mut reader = Reader::new(&[1, 7], Accounter::default_quota());
        assert_eq!(reader.any_tag(), Ok(Some(Tag::Byte(7))));
        assert_eq!(reader.accounter.usage(), 9);
    }
}
