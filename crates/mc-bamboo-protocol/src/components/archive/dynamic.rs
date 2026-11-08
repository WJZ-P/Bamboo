//! 存档编解码器的中间值，以及官方 `HashOps` 的 CRC32C 哈希。
//!
//! 官方把一个值同时编到 NBT 和 `HashOps` 两种表示上，两种 `DynamicOps` 对布尔值的
//! 表示不一样：NBT 里布尔就是一个字节，`HashOps` 里布尔有自己的标签。所以中间值
//! 必须把布尔和字节分开，否则算出来的组件哈希会对不上。
//!
//! 映射的键在官方全是字符串，这里也直接存字符串。

use mc_bamboo_nbt::Tag;

/// 官方 `HashOps` 的标签字节。
const TAG_EMPTY: u8 = 1;
const TAG_MAP_START: u8 = 2;
const TAG_MAP_END: u8 = 3;
const TAG_LIST_START: u8 = 4;
const TAG_LIST_END: u8 = 5;
const TAG_BYTE: u8 = 6;
const TAG_SHORT: u8 = 7;
const TAG_INT: u8 = 8;
const TAG_LONG: u8 = 9;
const TAG_FLOAT: u8 = 10;
const TAG_DOUBLE: u8 = 11;
const TAG_STRING: u8 = 12;
const TAG_BOOLEAN: u8 = 13;
const TAG_BYTE_ARRAY_START: u8 = 14;
const TAG_BYTE_ARRAY_END: u8 = 15;
const TAG_INT_ARRAY_START: u8 = 16;
const TAG_INT_ARRAY_END: u8 = 17;
const TAG_LONG_ARRAY_START: u8 = 18;
const TAG_LONG_ARRAY_END: u8 = 19;

/// 编解码器的中间值。变体的名字对应官方的 `DynamicOps` 操作。
///
/// [`Clone`] 和 [`Dyn::release`] 走堆上的显式栈。自动 `Drop` 仍会递归，深树要调用 `release`。
///
/// [`Clone`] and [`Dyn::release`] use an explicit heap stack. Automatic `Drop`
/// still recurses; call `release` for a deep tree.
#[derive(Debug, PartialEq)]
pub enum Dyn {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// 官方 `createBoolean`。在 NBT 里写成一个字节。
    Bool(bool),
    String(String),
    List(Vec<Dyn>),
    Map(Vec<(String, Dyn)>),
    ByteArray(Vec<i8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Clone for Dyn {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Start(&'a Dyn),
            List {
                values: &'a [Dyn],
                index: usize,
                built: Vec<Dyn>,
            },
            Map {
                entries: &'a [(String, Dyn)],
                index: usize,
                built: Vec<(String, Dyn)>,
            },
        }

        let mut stack = vec![Task::Start(self)];
        let mut done = Vec::new();
        while let Some(task) = stack.pop() {
            match task {
                Task::Start(value) => match value {
                    Dyn::Byte(value) => done.push(Dyn::Byte(*value)),
                    Dyn::Short(value) => done.push(Dyn::Short(*value)),
                    Dyn::Int(value) => done.push(Dyn::Int(*value)),
                    Dyn::Long(value) => done.push(Dyn::Long(*value)),
                    Dyn::Float(value) => done.push(Dyn::Float(*value)),
                    Dyn::Double(value) => done.push(Dyn::Double(*value)),
                    Dyn::Bool(value) => done.push(Dyn::Bool(*value)),
                    Dyn::String(value) => done.push(Dyn::String(value.clone())),
                    Dyn::ByteArray(values) => done.push(Dyn::ByteArray(values.clone())),
                    Dyn::IntArray(values) => done.push(Dyn::IntArray(values.clone())),
                    Dyn::LongArray(values) => done.push(Dyn::LongArray(values.clone())),
                    Dyn::List(values) => stack.push(Task::List {
                        values,
                        index: 0,
                        built: Vec::with_capacity(values.len()),
                    }),
                    Dyn::Map(entries) => stack.push(Task::Map {
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
                        built.push(done.pop().expect("list element"));
                    }
                    if index == values.len() {
                        done.push(Dyn::List(built));
                    } else {
                        stack.push(Task::List {
                            values,
                            index: index + 1,
                            built,
                        });
                        stack.push(Task::Start(&values[index]));
                    }
                }
                Task::Map {
                    entries,
                    index,
                    mut built,
                } => {
                    if index > 0 {
                        let (key, _) = &entries[index - 1];
                        built.push((key.clone(), done.pop().expect("map value")));
                    }
                    if index == entries.len() {
                        done.push(Dyn::Map(built));
                    } else {
                        stack.push(Task::Map {
                            entries,
                            index: index + 1,
                            built,
                        });
                        stack.push(Task::Start(&entries[index].1));
                    }
                }
            }
        }
        done.pop().expect("cloned value")
    }
}

impl Dyn {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub fn list(values: Vec<Dyn>) -> Self {
        Self::List(values)
    }

    /// 一个空映射。官方 `MapCodec.unitCodec` 编出来的就是这个。
    pub fn empty_map() -> Self {
        Self::Map(Vec::new())
    }

    /// 拆掉一棵可能很深的中间值，不使用调用栈。
    ///
    /// Dismantle a value that may be very deep, without using the call stack.
    pub fn release(self) {
        let mut stack = vec![self];
        while let Some(value) = stack.pop() {
            match value {
                Self::List(values) => stack.extend(values),
                Self::Map(entries) => stack.extend(entries.into_iter().map(|(_, value)| value)),
                _ => {}
            }
        }
    }

    /// 官方 `Objects.equals` 的比较规则。
    ///
    /// 浮点数按位比较：`NaN` 等于 `NaN`，但 `0.0` 和 `-0.0` 不相等。
    /// `optionalFieldOf(name, default)` 就是按这个结果决定要不要写字段。
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Float(first), Self::Float(second)) => {
                (first.is_nan() && second.is_nan()) || first.to_bits() == second.to_bits()
            }
            (Self::Double(first), Self::Double(second)) => {
                (first.is_nan() && second.is_nan()) || first.to_bits() == second.to_bits()
            }
            _ => self == other,
        }
    }

    /// 从一个现成的 NBT 标签转过来。
    ///
    /// 用于原样保存合法输入的组件（`lock` 这类只在存档里有 `Codec` 的组件），
    /// 转换之后布尔和数值的标签类型都保留，写回去不会有变化。
    pub fn from_tag(tag: &Tag) -> Self {
        crate::stack_hop::guard(|| Self::from_tag_frame(tag))
    }

    fn from_tag_frame(tag: &Tag) -> Self {
        match tag {
            Tag::Byte(value) => Self::Byte(*value),
            Tag::Short(value) => Self::Short(*value),
            Tag::Int(value) => Self::Int(*value),
            Tag::Long(value) => Self::Long(*value),
            Tag::Float(value) => Self::Float(*value),
            Tag::Double(value) => Self::Double(*value),
            Tag::String(value) => Self::String(value.clone()),
            Tag::List(values) => Self::List(values.iter().map(Self::from_tag).collect()),
            Tag::Compound(entries) => Self::Map(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from_tag(value)))
                    .collect(),
            ),
            Tag::ByteArray(values) => Self::ByteArray(values.clone()),
            Tag::IntArray(values) => Self::IntArray(values.clone()),
            Tag::LongArray(values) => Self::LongArray(values.clone()),
        }
    }

    /// 变成 NBT 标签。布尔写回字节，映射的键必须都是字符串。
    pub fn to_tag(&self) -> Tag {
        crate::stack_hop::guard(|| self.to_tag_frame())
    }

    fn to_tag_frame(&self) -> Tag {
        match self {
            Self::Byte(value) => Tag::Byte(*value),
            Self::Short(value) => Tag::Short(*value),
            Self::Int(value) => Tag::Int(*value),
            Self::Long(value) => Tag::Long(*value),
            Self::Float(value) => Tag::Float(*value),
            Self::Double(value) => Tag::Double(*value),
            Self::Bool(value) => Tag::Byte(i8::from(*value)),
            Self::String(value) => Tag::String(value.clone()),
            Self::List(values) => Tag::List(values.iter().map(Self::to_tag).collect()),
            Self::Map(entries) => Tag::Compound(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_tag()))
                    .collect(),
            ),
            Self::ByteArray(values) => Tag::ByteArray(values.clone()),
            Self::IntArray(values) => Tag::IntArray(values.clone()),
            Self::LongArray(values) => Tag::LongArray(values.clone()),
        }
    }

    /// 官方的 `HashOps` 哈希，返回 `HashCode.asInt()` 那四个字节。
    pub fn hash(&self) -> u32 {
        let mut payload = Vec::new();
        self.write_hash_payload(&mut payload);
        crc32c(&payload)
    }

    /// 官方 `HashCode.asInt()`：哈希的小端字节序值。
    pub fn hash_int(&self) -> i32 {
        self.hash() as i32
    }

    fn write_hash_payload(&self, out: &mut Vec<u8>) {
        match self {
            Self::Byte(value) => {
                out.push(TAG_BYTE);
                out.push(*value as u8);
            }
            Self::Short(value) => {
                out.push(TAG_SHORT);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Int(value) => {
                out.push(TAG_INT);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Long(value) => {
                out.push(TAG_LONG);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Float(value) => {
                out.push(TAG_FLOAT);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Double(value) => {
                out.push(TAG_DOUBLE);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Bool(value) => {
                out.push(TAG_BOOLEAN);
                out.push(u8::from(*value));
            }
            Self::String(value) => {
                out.push(TAG_STRING);
                // 官方按 UTF-16 码元数写长度，再写 UTF-16LE。
                let units: Vec<u16> = value.encode_utf16().collect();
                out.extend_from_slice(&(units.len() as u32).to_le_bytes());
                for unit in units {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
            }
            Self::List(values) => {
                out.push(TAG_LIST_START);
                // 官方 `HashOps.createList` 写入的是每个元素 `HashCode.asBytes()`，
                // 也就是那个元素哈希的 4 个小端字节，不是元素自己的载荷。
                //
                // `HashOps.createList` writes each element's `HashCode.asBytes()`
                // (the 4 little-endian bytes of that element's hash), not its payload.
                for value in values {
                    out.extend_from_slice(&value.hash().to_le_bytes());
                }
                out.push(TAG_LIST_END);
            }
            Self::Map(entries) => {
                out.push(TAG_MAP_START);
                let mut hashed: Vec<(u32, u32)> = entries
                    .iter()
                    .map(|(key, value)| (hash_of(&Self::String(key.clone())), value.hash()))
                    .collect();
                hashed.sort_unstable();
                for (key, value) in hashed {
                    out.extend_from_slice(&key.to_le_bytes());
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.push(TAG_MAP_END);
            }
            Self::ByteArray(values) => {
                out.push(TAG_BYTE_ARRAY_START);
                out.extend(values.iter().map(|value| *value as u8));
                out.push(TAG_BYTE_ARRAY_END);
            }
            Self::IntArray(values) => {
                out.push(TAG_INT_ARRAY_START);
                for value in values {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.push(TAG_INT_ARRAY_END);
            }
            Self::LongArray(values) => {
                out.push(TAG_LONG_ARRAY_START);
                for value in values {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.push(TAG_LONG_ARRAY_END);
            }
        }
    }
}

/// 一个值的哈希，避免在映射排序时再分配一次负载。
fn hash_of(value: &Dyn) -> u32 {
    value.hash()
}

/// 官方 `HashOps.empty()`。
pub fn empty_hash() -> u32 {
    crc32c(&[TAG_EMPTY])
}

/// CRC-32C（Castagnoli），参数和 Guava 的 `Hashing.crc32c()` 一致。
fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc = CRC32C_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// 反射多项式 `0x82F63B78` 的查表。
const CRC32C_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0x82f6_3b78
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: u32) -> String {
        format!("{value:08x}")
    }

    /// 空值的 `HashCode.asInt()`。`asBytes()` 是这四个字节的小端序。
    ///
    /// `HashCode.asInt()` of the empty value. `asBytes()` is those four bytes
    /// in little-endian order.
    #[test]
    fn the_empty_value_matches_the_official_probe() {
        assert_eq!(hex(empty_hash()), "a016d052");
    }

    /// 这些向量是 Guava `Hashing.crc32c()` 的 `HashCode.asInt()`，写入方式和
    /// `HashOps` 的 `putByte` / `putInt` / `putBytes(asBytes())` 相同。
    ///
    /// These vectors are Guava `Hashing.crc32c()` `HashCode.asInt()` values,
    /// written the same way `HashOps` uses `putByte`, `putInt`, and
    /// `putBytes(asBytes())`.
    #[test]
    fn primitives_match_the_official_probe() {
        let cases = [
            (Dyn::Int(1), "5d50d71c"),
            (Dyn::Byte(1), "6ac5a5e3"),
            (Dyn::Short(258), "c1ba74fe"),
            (Dyn::Long(1), "be94315b"),
            (Dyn::Float(1.0), "65042d90"),
            (Dyn::Double(1.0), "3053cdf6"),
            (Dyn::string("ab"), "493e5675"),
            (Dyn::string("竹"), "6f953072"),
            (Dyn::string("🎫"), "58cc4e6a"),
            (Dyn::Bool(true), "c336cec2"),
            (Dyn::list(vec![Dyn::Int(1), Dyn::Int(2)]), "ae1b0624"),
            (Dyn::Map(vec![("a".to_owned(), Dyn::Int(1))]), "f982bbfe"),
            (Dyn::ByteArray(vec![1, 2]), "a671a4e7"),
            (Dyn::IntArray(vec![1, 2]), "08416b0b"),
            (Dyn::LongArray(vec![1, 2]), "901cd967"),
        ];
        for (value, expected) in cases {
            assert_eq!(hex(value.hash()), expected, "{value:?} 的格式和官方对不上");
        }
    }

    #[test]
    fn map_order_does_not_matter() {
        let first = Dyn::Map(vec![
            ("a".to_owned(), Dyn::Int(1)),
            ("b".to_owned(), Dyn::Int(2)),
        ]);
        let second = Dyn::Map(vec![
            ("b".to_owned(), Dyn::Int(2)),
            ("a".to_owned(), Dyn::Int(1)),
        ]);
        assert_eq!(first.hash(), second.hash());
    }

    #[test]
    fn crc32c_standard_check_value() {
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
    }
}
