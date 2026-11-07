//! NBT：Minecraft 用来存储结构化数据的二进制格式。
//!
//! 目前只实现网络上用到的编码。读取存档时才需要解码，等做到存档再加。

/// 一个 NBT 值。
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    /// 编码后超过 65535 字节的部分不写。
    String(String),
    /// 列表里的元素类型必须相同。
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

const END_ID: u8 = 0;

/// 字符串编码后的字节数上限，长度前缀只有两个字节。
const MAX_STRING_BYTES: usize = u16::MAX as usize;

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(_) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }

    /// 按网络格式写入：先写类型，再写内容。和文件格式不同，根节点没有名字。
    pub fn write_network(&self, out: &mut Vec<u8>) {
        out.push(self.id());
        self.write_payload(out);
    }

    fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Self::Byte(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Short(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Long(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Float(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Double(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::ByteArray(values) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
            Self::String(value) => write_modified_utf8(out, value),
            Self::List(values) => {
                out.push(values.first().map_or(END_ID, Self::id));
                write_len(out, values.len());
                for value in values {
                    value.write_payload(out);
                }
            }
            Self::Compound(entries) => {
                for (name, value) in entries {
                    out.push(value.id());
                    write_modified_utf8(out, name);
                    value.write_payload(out);
                }
                out.push(END_ID);
            }
            Self::IntArray(values) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
            Self::LongArray(values) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
        }
    }
}

fn write_len(out: &mut Vec<u8>, len: usize) {
    out.extend_from_slice(&(len as i32).to_be_bytes());
}

/// 写入 Java 的 Modified UTF-8：两字节长度，后面跟编码后的内容。
///
/// 和标准 UTF-8 的差别有两处：NUL 写成两个字节，
/// 超出基本平面的字符按 UTF-16 的两个代理项分别编码，各占三个字节。
///
/// 长度前缀放不下的字符不写。官方这时抛异常，这个包发不出去。这里没有错误路径，
/// 而长度写错后后面的字节全部错位，客户端解码失败就断线了，所以宁可少写。
fn write_modified_utf8(out: &mut Vec<u8>, value: &str) {
    let length_at = out.len();
    out.extend_from_slice(&[0, 0]);
    let content_at = out.len();

    let mut units = [0; 2];
    for character in value.chars() {
        let character_at = out.len();
        for unit in character.encode_utf16(&mut units).iter().copied() {
            match unit {
                0x0001..=0x007f => out.push(unit as u8),
                0x0000 | 0x0080..=0x07ff => {
                    out.push(0xc0 | (unit >> 6) as u8);
                    out.push(0x80 | (unit & 0x3f) as u8);
                }
                _ => {
                    out.push(0xe0 | (unit >> 12) as u8);
                    out.push(0x80 | ((unit >> 6) & 0x3f) as u8);
                    out.push(0x80 | (unit & 0x3f) as u8);
                }
            }
        }
        // 按字符回退，不会留下半个代理对。
        if out.len() - content_at > MAX_STRING_BYTES {
            out.truncate(character_at);
            break;
        }
    }

    let encoded_len = (out.len() - content_at) as u16;
    out[length_at..content_at].copy_from_slice(&encoded_len.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_bytes(tag: &Tag) -> Vec<u8> {
        let mut out = Vec::new();
        tag.write_network(&mut out);
        out
    }

    #[test]
    fn writes_numbers_big_endian() {
        assert_eq!(network_bytes(&Tag::Byte(-1)), [1, 0xff]);
        assert_eq!(network_bytes(&Tag::Short(258)), [2, 1, 2]);
        assert_eq!(network_bytes(&Tag::Int(1)), [3, 0, 0, 0, 1]);
        assert_eq!(network_bytes(&Tag::Float(1.0)), [5, 0x3f, 0x80, 0, 0]);
    }

    #[test]
    fn writes_plain_strings() {
        assert_eq!(
            network_bytes(&Tag::String("hi".to_owned())),
            [8, 0, 2, b'h', b'i']
        );
    }

    #[test]
    fn writes_chinese_as_three_byte_units() {
        assert_eq!(
            network_bytes(&Tag::String("竹".to_owned())),
            [8, 0, 3, 0xe7, 0xab, 0xb9]
        );
    }

    #[test]
    fn writes_nul_as_two_bytes() {
        assert_eq!(
            network_bytes(&Tag::String("\0".to_owned())),
            [8, 0, 2, 0xc0, 0x80]
        );
    }

    #[test]
    fn writes_supplementary_characters_as_surrogate_pairs() {
        assert_eq!(
            network_bytes(&Tag::String("🎋".to_owned())),
            [8, 0, 6, 0xed, 0xa0, 0xbc, 0xed, 0xbe, 0x8b]
        );
    }

    /// 字符串标签内容的编码结果，确认长度前缀和实际字节数一致。
    fn string_content(value: String) -> Vec<u8> {
        let bytes = network_bytes(&Tag::String(value));
        let len = u16::from_be_bytes([bytes[1], bytes[2]]);
        assert_eq!(usize::from(len), bytes.len() - 3);
        bytes[3..].to_vec()
    }

    #[test]
    fn keeps_a_string_that_exactly_fills_the_length_prefix() {
        assert_eq!(string_content("a".repeat(65535)).len(), 65535);
        assert_eq!(string_content("竹".repeat(21845)).len(), 65535);
    }

    #[test]
    fn drops_the_characters_the_length_prefix_cannot_hold() {
        assert_eq!(string_content("a".repeat(65536)).len(), 65535);
        assert_eq!(string_content("a".repeat(100_000)).len(), 65535);
        assert_eq!(string_content("竹".repeat(21846)).len(), 65535);
    }

    #[test]
    fn long_strings_are_not_cut_inside_a_character() {
        // 最后一个“竹”只剩 1 个字节的空间，整个字符都不写。
        let content = string_content(format!("ab{}", "竹".repeat(21845)));
        assert_eq!(content.len(), 2 + 21844 * 3);

        // 代理对占 6 个字节，放不下时两半都不写。
        let content = string_content(format!("a{}", "🎋".repeat(10923)));
        assert_eq!(content.len(), 1 + 10922 * 6);
        assert_eq!(
            content[content.len() - 6..],
            [0xed, 0xa0, 0xbc, 0xed, 0xbe, 0x8b]
        );
    }

    #[test]
    fn a_long_string_leaves_the_following_tags_intact() {
        let tag = Tag::Compound(vec![
            ("a".to_owned(), Tag::String("x".repeat(70_000))),
            ("b".to_owned(), Tag::Byte(7)),
        ]);
        let bytes = network_bytes(&tag);
        assert_eq!(bytes[..4], [10, 8, 0, 1]);
        assert_eq!(bytes[5..7], [0xff, 0xff]);
        assert_eq!(bytes[7 + 65535..], [1, 0, 1, b'b', 7, 0]);
    }

    #[test]
    fn writes_lists_with_their_element_type() {
        assert_eq!(
            network_bytes(&Tag::List(vec![Tag::Byte(1), Tag::Byte(2)])),
            [9, 1, 0, 0, 0, 2, 1, 2]
        );
        assert_eq!(network_bytes(&Tag::List(Vec::new())), [9, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn writes_compounds_with_named_entries_and_an_end_marker() {
        let tag = Tag::Compound(vec![("a".to_owned(), Tag::Byte(7))]);
        assert_eq!(network_bytes(&tag), [10, 1, 0, 1, b'a', 7, 0]);
    }

    #[test]
    fn writes_arrays_with_a_length_prefix() {
        assert_eq!(
            network_bytes(&Tag::IntArray(vec![1])),
            [11, 0, 0, 0, 1, 0, 0, 0, 1]
        );
        assert_eq!(
            network_bytes(&Tag::LongArray(vec![1])),
            [12, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1]
        );
    }
}
