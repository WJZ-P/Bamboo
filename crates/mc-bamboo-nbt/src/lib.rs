//! NBT：Minecraft 用来存储结构化数据的二进制格式。
//!
//! 目前只实现网络上用到的编码和解码。读存档还要认文件格式的根节点名字，等做到存档再加。

mod decode;
pub mod snbt;

pub use decode::{Accounter, DEFAULT_QUOTA, DecodeError, MAX_DEPTH, Reader};
pub use snbt::{
    SnbtError, java_double_to_string, java_float_to_string, parse_snbt, parse_snbt_compound,
    parse_snbt_prefix, quote_and_escape,
};

/// 一个 NBT 值。
///
/// [`Clone`] 走堆上的显式栈。派生的克隆会按嵌套深度递归。深度超过 512 的树不要靠
/// 自动 `Drop` 拆掉，调用 [`Tag::release`]：自动 `Drop` 也是递归的，在连接任务的栈上会把进程打崩。
///
/// [`Clone`] uses an explicit heap stack. A derived clone would recurse with the
/// nesting. Do not rely on automatic `Drop` for a tree deeper than a few dozen
/// levels; call [`Tag::release`]. Automatic `Drop` recurses too, and that overflows
/// the connection task.
#[derive(Debug, PartialEq)]
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
const COMPOUND_ID: u8 = 10;

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
        write_payload(self, out);
    }
}

impl Clone for Tag {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Start(&'a Tag),
            List {
                values: &'a [Tag],
                index: usize,
                built: Vec<Tag>,
            },
            Compound {
                entries: &'a [(String, Tag)],
                index: usize,
                built: Vec<(String, Tag)>,
            },
        }

        let mut stack = vec![Task::Start(self)];
        let mut done = Vec::new();
        while let Some(task) = stack.pop() {
            match task {
                Task::Start(tag) => match tag {
                    Tag::Byte(value) => done.push(Tag::Byte(*value)),
                    Tag::Short(value) => done.push(Tag::Short(*value)),
                    Tag::Int(value) => done.push(Tag::Int(*value)),
                    Tag::Long(value) => done.push(Tag::Long(*value)),
                    Tag::Float(value) => done.push(Tag::Float(*value)),
                    Tag::Double(value) => done.push(Tag::Double(*value)),
                    Tag::ByteArray(values) => done.push(Tag::ByteArray(values.clone())),
                    Tag::String(value) => done.push(Tag::String(value.clone())),
                    Tag::IntArray(values) => done.push(Tag::IntArray(values.clone())),
                    Tag::LongArray(values) => done.push(Tag::LongArray(values.clone())),
                    Tag::List(values) => stack.push(Task::List {
                        values,
                        index: 0,
                        built: Vec::with_capacity(values.len()),
                    }),
                    Tag::Compound(entries) => stack.push(Task::Compound {
                        entries,
                        index: 0,
                        built: Vec::with_capacity(entries.len()),
                    }),
                },
                Task::List {
                    values,
                    index,
                    mut built,
                } => {
                    if index > 0 {
                        built.push(done.pop().expect("列表元素"));
                    }
                    if index == values.len() {
                        done.push(Tag::List(built));
                    } else {
                        stack.push(Task::List {
                            values,
                            index: index + 1,
                            built,
                        });
                        stack.push(Task::Start(&values[index]));
                    }
                }
                Task::Compound {
                    entries,
                    index,
                    mut built,
                } => {
                    if index > 0 {
                        let (key, _) = &entries[index - 1];
                        built.push((key.clone(), done.pop().expect("复合标签的值")));
                    }
                    if index == entries.len() {
                        done.push(Tag::Compound(built));
                    } else {
                        stack.push(Task::Compound {
                            entries,
                            index: index + 1,
                            built,
                        });
                        stack.push(Task::Start(&entries[index].1));
                    }
                }
            }
        }
        done.pop().expect("克隆出来的标签")
    }
}

impl Tag {
    /// 拆掉一棵可能很深的标签，不使用调用栈。
    ///
    /// 自动 `Drop` 会按层递归。连接任务的栈装不下 512 层。
    ///
    /// Dismantle a tag that may be very deep, without using the call stack.
    /// Automatic `Drop` recurses per level, and the connection task cannot hold 512 frames.
    pub fn release(self) {
        let mut stack = vec![self];
        while let Some(tag) = stack.pop() {
            match tag {
                Tag::List(values) => stack.extend(values),
                Tag::Compound(entries) => stack.extend(entries.into_iter().map(|(_, value)| value)),
                _ => {}
            }
        }
    }
}

/// 写标签内容。嵌套用显式栈，和 [`Tag::write_network`] 的类型字节分开。
fn write_payload(tag: &Tag, out: &mut Vec<u8>) {
    enum Job<'a> {
        Payload(&'a Tag),
        List {
            values: &'a [Tag],
            index: usize,
            element_type: u8,
        },
        Compound {
            entries: &'a [(String, Tag)],
            index: usize,
        },
        /// 异类列表元素外面那层包装复合标签的结束字节。
        WrapperEnd,
    }

    let mut stack = vec![Job::Payload(tag)];
    while let Some(job) = stack.pop() {
        match job {
            Job::Payload(Tag::Byte(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::Short(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::Int(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::Long(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::Float(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::Double(value)) => out.extend_from_slice(&value.to_be_bytes()),
            Job::Payload(Tag::ByteArray(values)) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
            Job::Payload(Tag::String(value)) => write_modified_utf8(out, value),
            Job::Payload(Tag::IntArray(values)) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
            Job::Payload(Tag::LongArray(values)) => {
                write_len(out, values.len());
                out.extend(values.iter().flat_map(|value| value.to_be_bytes()));
            }
            Job::Payload(Tag::List(values)) => {
                let element_type = raw_element_type(values);
                out.push(element_type);
                write_len(out, values.len());
                stack.push(Job::List {
                    values,
                    index: 0,
                    element_type,
                });
            }
            Job::Payload(Tag::Compound(entries)) => stack.push(Job::Compound { entries, index: 0 }),
            Job::List {
                values,
                index,
                element_type,
            } => {
                if index == values.len() {
                    continue;
                }
                stack.push(Job::List {
                    values,
                    index: index + 1,
                    element_type,
                });
                let value = &values[index];
                if element_type == COMPOUND_ID && needs_wrapper(value) {
                    // 官方 `ListTag.wrapIfNeeded`：类型不一致的元素各自包进 `{"": 值}`。
                    out.push(value.id());
                    write_modified_utf8(out, "");
                    stack.push(Job::WrapperEnd);
                }
                stack.push(Job::Payload(value));
            }
            Job::Compound { entries, index } => {
                if index == entries.len() {
                    out.push(END_ID);
                    continue;
                }
                stack.push(Job::Compound {
                    entries,
                    index: index + 1,
                });
                let (name, value) = &entries[index];
                out.push(value.id());
                write_modified_utf8(out, name);
                stack.push(Job::Payload(value));
            }
            Job::WrapperEnd => out.push(END_ID),
        }
    }
}

/// 官方 `ListTag.identifyRawElementType`：元素类型一致时就用它，混在一起时写成复合标签。
fn raw_element_type(values: &[Tag]) -> u8 {
    let mut uniform = END_ID;
    for value in values {
        let id = value.id();
        if uniform == END_ID {
            uniform = id;
        } else if uniform != id {
            return COMPOUND_ID;
        }
    }
    uniform
}

/// 官方 `ListTag.wrapIfNeeded`：只有本身不是包装的复合标签能原样写出去。
///
/// 官方 `ListTag.isWrapper` 说的包装就是「只有一个空名字键的复合标签」。
fn needs_wrapper(value: &Tag) -> bool {
    match value {
        Tag::Compound(entries) => matches!(&entries[..], [(name, _)] if name.is_empty()),
        _ => true,
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
