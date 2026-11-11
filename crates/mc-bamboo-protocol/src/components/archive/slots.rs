//! 带槽位的物品列表（官方 `ItemStackWithSlot.CODEC`）的存档 NBT 编解码。
//!
//! 一个物品的存档标签是 [`crate::components::archive::stack`] 里的 `ItemStack.CODEC`；
//! 这一层只补 `Slot`：`ExtraCodecs.UNSIGNED_BYTE` 是无符号映射的 `Codec.BYTE`，写在
//! 同一个复合标签里，用 `optionalAlwaysPresentFieldOf`（读可省、写必写）。
//! 玩家物品栏的 `Inventory` / `EnderItems`、箱子等容器的 `Items` 都用这一份。
//!
//! 读一个列表时解不开的条目按官方 `TagValueInput$TypedListWrapper` 处理：报告问题并跳过
//! 这一条，其余条目照常解出来，不因为一条坏数据让整个列表读不了。容器装载时槽位
//! 越界的条目也按官方 `ItemStackWithSlot.isValidInContainer` 跳过；槽位重复时后一条覆盖
//! 前一条（`Inventory.load` 的 `setItem` / `ContainerHelper.loadAllItems` 的 `set`）。
//! 列表本身不是 `ListTag` 时按 `listOrEmpty` 当成空列表并报告一条问题。
//!
//! `ItemStack.CODEC` 是 `MapCodec`，物品复合标签里多余的键会被忽略，写回时不再出现。
//! `id`、`count`、`components` 之外的数据不在这里原样保留；D9 的保留只用于玩家文件
//! 和 `level.dat` 的复合标签。
//!
//! Persistent NBT for slotted item lists (the official `ItemStackWithSlot.CODEC`).
//!
//! A stack's own tag is `ItemStack.CODEC` in [`crate::components::archive::stack`]. This
//! layer adds `Slot`: `ExtraCodecs.UNSIGNED_BYTE` is `Codec.BYTE` with an unsigned
//! mapping, flattened into the same compound by `optionalAlwaysPresentFieldOf` (optional
//! on read, always written). Player inventories, ender chests and container `Items`
//! all use it.

use mc_bamboo_nbt::Tag;

use super::{
    read::{self, Error, Result},
    stack,
};
use crate::{components::ComponentPatch, item::ItemStack};

/// `UNSIGNED_BYTE` 的范围，也是磁盘上槽位的上限。
pub const MAX_SLOT: i32 = 255;
/// 玩家物品栏的格数（官方 `Inventory.items` 是 36 格）。
///
/// The player inventory size (the official `Inventory.items` has 36 slots).
pub const INVENTORY_SLOTS: usize = 36;
/// 末影箱的格数（官方 `PlayerEnderChestContainer` 是 27 格）。
///
/// The ender chest size (the official `PlayerEnderChestContainer` has 27 slots).
pub const ENDER_CHEST_SLOTS: usize = 27;

/// 一条带槽位的物品。`slot` 是 0..=[`MAX_SLOT`] 的无符号字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlottedItem {
    pub slot: i32,
    pub stack: ItemStack,
}

/// 列表里一条解不开的条目，交给调用方记日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemProblem {
    /// 在原始列表里的下标。
    pub index: usize,
    /// 出错的原因。
    pub message: String,
}

/// 一个列表的解出结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedItems {
    /// 解出来并且槽位在 `0..size` 内的条目，槽位重复时只留最后一条，按槽位升序。
    pub items: Vec<SlottedItem>,
    /// 跳过的条目和整张列表本身的问题，顺序和原始列表一致。
    pub problems: Vec<ItemProblem>,
}

/// 一个物品 ⇄ 一个 NBT 复合标签（不带 `Slot`）。
///
/// `count` 必须在 1..=99 内，物品不能是空气；错误返回 [`Error`]，不 panic。
pub fn encode_item(stack: &ItemStack) -> Result<Tag> {
    let patch =
        ComponentPatch::from_opaque(stack).map_err(|error| Error::new(error.to_string()))?;
    let encoded = stack::encode_stack(stack.item, stack.count, &patch)?;
    let tag = encoded.to_tag();
    encoded.release();
    patch.release();
    Ok(tag)
}

/// 一个 NBT 复合标签 ⇄ 一个物品（不带 `Slot`）。多余的键按官方 `MapCodec` 忽略。
pub fn decode_item(tag: &Tag) -> Result<ItemStack> {
    let (item, count, patch) = stack::decode_stack(tag)?;
    let (added, removed) = patch.to_opaque();
    patch.release();
    Ok(ItemStack {
        item,
        count,
        added,
        removed,
    })
}

/// 一个带槽位的物品 ⇄ 一个 NBT 复合标签。
///
/// 槽位超出 0..=[`MAX_SLOT`] 返回错误；`Slot` 一定写，即使等于默认的 0。
pub fn encode_slotted(item: &SlottedItem) -> Result<Tag> {
    if item.slot < 0 || item.slot > MAX_SLOT {
        return Err(Error::new(format!("slot {} is out of range", item.slot)));
    }
    let Tag::Compound(mut entries) = encode_item(&item.stack)? else {
        return Err(Error::new("item codec did not produce a compound"));
    };
    // `ItemStackWithSlot.CODEC` 的 `group` 先写 `Slot`，再平铺 `ItemStack.MAP_CODEC`。
    entries.insert(0, ("Slot".to_owned(), Tag::Byte(item.slot as i8)));
    Ok(Tag::Compound(entries))
}

/// 一个 NBT 复合标签 ⇄ 一个带槽位的物品。
///
/// `Slot` 可省（默认 0），`UNSIGNED_BYTE` 按无符号读。
pub fn decode_slotted(tag: &Tag) -> Result<SlottedItem> {
    let entries = read::compound(tag)?;
    let slot = match read::field(entries, "Slot") {
        Some(tag) => unsigned_byte(tag)?,
        None => 0,
    };
    Ok(SlottedItem {
        slot,
        stack: decode_item(tag)?,
    })
}

/// 一个带槽位的列表 ⇄ 一个 NBT 列表。空物品不写条目，条目按输入顺序保留。
pub fn encode_list(items: &[SlottedItem]) -> Result<Tag> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if item.stack.is_empty() {
            continue;
        }
        out.push(encode_slotted(item)?);
    }
    Ok(Tag::List(out))
}

/// 一个 NBT 列表 ⇄ 带槽位的物品，按官方的容器尺寸过滤槽位。
///
/// `size` 是容器的格数（玩家物品栏 [`INVENTORY_SLOTS`]、末影箱
/// [`ENDER_CHEST_SLOTS`]、箱子等容器各自的格数）。槽位本身是一个无符号字节，所以
/// `size` 按 [`MAX_SLOT`] + 1 截断，坏输入也不会换来一个大数组。槽位越界、条目
/// 解不开都只记进 [`DecodedItems::problems`]，其余条目照常解出来。参数不是
/// `ListTag` 时按官方 `listOrEmpty` 当成空列表并报告，不返回 `Err`；
/// 输入当不可信数据，不 panic。
pub fn decode_list(tag: &Tag, size: usize) -> DecodedItems {
    let size = size.min(MAX_SLOT as usize + 1);
    let values = match tag {
        Tag::List(values) => values.as_slice(),
        _ => {
            return DecodedItems {
                items: Vec::new(),
                problems: vec![ItemProblem {
                    index: 0,
                    message: format!("{} is not a list tag, reading it as empty", tag_name(tag)),
                }],
            };
        }
    };
    let mut problems = Vec::new();
    // 官方把每条依次 `setItem`，重复的槽位后一条覆盖前一条；输出按槽位升序。
    let mut slots: Vec<Option<SlottedItem>> = vec![None; size];
    for (index, value) in values.iter().enumerate() {
        match decode_slotted(value) {
            Ok(item) => {
                let entry = usize::try_from(item.slot)
                    .ok()
                    .and_then(|slot| slots.get_mut(slot));
                match entry {
                    Some(entry) => *entry = Some(item),
                    None => problems.push(ItemProblem {
                        index,
                        message: format!(
                            "slot {} is outside the container of {size} slots",
                            item.slot
                        ),
                    }),
                }
            }
            Err(error) => problems.push(ItemProblem {
                index,
                message: error.message,
            }),
        }
    }
    DecodedItems {
        items: slots.into_iter().flatten().collect(),
        problems,
    }
}

/// 官方 `UNSIGNED_BYTE`：`Codec.BYTE` 从任意数值标签解出时按 Java 的 `byteValue()`
/// 截断，再 `UnsignedBytes.toInt`。负数（无符号 128..=255）是合法的槽位。
fn unsigned_byte(tag: &Tag) -> Result<i32> {
    let value = read::java_i32(tag)? as i8;
    Ok(i32::from(value) & 0xff)
}

fn tag_name(tag: &Tag) -> &'static str {
    match tag {
        Tag::Byte(_) => "byte",
        Tag::Short(_) => "short",
        Tag::Int(_) => "int",
        Tag::Long(_) => "long",
        Tag::Float(_) => "float",
        Tag::Double(_) => "double",
        Tag::ByteArray(_) => "byte array",
        Tag::String(_) => "string",
        Tag::List(_) => "list",
        Tag::Compound(_) => "compound",
        Tag::IntArray(_) => "int array",
        Tag::LongArray(_) => "long array",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{ComponentPatch, ComponentValue, PatchEntry};
    use mc_bamboo_data::{items::STONE, registries::data_component_type as kinds};

    fn item(slot: i32, count: i32) -> SlottedItem {
        SlottedItem {
            slot,
            stack: ItemStack::new(STONE, count),
        }
    }

    fn with_component(count: i32) -> ItemStack {
        let mut patch = ComponentPatch::new();
        patch.put(kinds::DAMAGE, PatchEntry::Set(ComponentValue::Damage(3)));
        let (added, removed) = patch.to_opaque();
        patch.release();
        ItemStack {
            item: STONE,
            count,
            added,
            removed,
        }
    }

    #[test]
    fn slot_is_always_written_as_a_byte() {
        let tag = encode_slotted(&item(3, 1)).unwrap();
        let Tag::Compound(entries) = &tag else {
            panic!("not a compound");
        };
        assert_eq!(entries[0].0, "Slot");
        assert_eq!(entries[0].1, Tag::Byte(3));
        assert_eq!(
            read::field(entries, "id"),
            Some(&Tag::String("minecraft:stone".to_owned()))
        );
        assert_eq!(read::field(entries, "count"), Some(&Tag::Int(1)));
        assert!(read::field(entries, "components").is_none());
    }

    #[test]
    fn unsigned_slots_round_trip() {
        for raw in [-1_i8, -128, 0, 127] {
            let slot = i32::from(raw) & 0xff;
            let tag = encode_slotted(&item(slot, 1)).unwrap();
            let decoded = decode_slotted(&tag).unwrap();
            assert_eq!(decoded.slot, slot);
            assert_eq!(decoded.stack, item(slot, 1).stack);
        }
    }

    #[test]
    fn slot_can_be_omitted_on_read() {
        let Tag::Compound(entries) = encode_item(&ItemStack::new(STONE, 1)).unwrap() else {
            panic!("not a compound");
        };
        let decoded = decode_slotted(&Tag::Compound(entries)).unwrap();
        assert_eq!(decoded.slot, 0);
    }

    #[test]
    fn extra_keys_are_ignored() {
        let tag = Tag::Compound(vec![
            ("Slot".to_owned(), Tag::Byte(2)),
            ("id".to_owned(), Tag::String("minecraft:stone".to_owned())),
            ("count".to_owned(), Tag::Int(1)),
            ("bamboo:extra".to_owned(), Tag::Int(77)),
        ]);
        let decoded = decode_slotted(&tag).unwrap();
        let Tag::Compound(entries) = encode_slotted(&decoded).unwrap() else {
            panic!("not a compound");
        };
        assert!(entries.iter().all(|(key, _)| key != "bamboo:extra"));
    }

    #[test]
    fn components_round_trip() {
        let stack = with_component(9);
        let tag = encode_slotted(&SlottedItem {
            slot: 7,
            stack: stack.clone(),
        })
        .unwrap();
        assert_eq!(decode_slotted(&tag).unwrap().stack, stack);
    }

    #[test]
    fn bad_entries_are_reported_and_skipped() {
        let tag = Tag::List(vec![
            encode_slotted(&item(0, 1)).unwrap(),
            Tag::Int(4), // 不是复合标签
            Tag::Compound(vec![
                // 没有 id
                ("Slot".to_owned(), Tag::Byte(2)),
            ]),
            Tag::Compound(vec![
                // 数量越界
                ("Slot".to_owned(), Tag::Byte(3)),
                ("id".to_owned(), Tag::String("minecraft:stone".to_owned())),
                ("count".to_owned(), Tag::Int(100)),
            ]),
            encode_slotted(&item(4, 99)).unwrap(),
        ]);
        let decoded = decode_list(&tag, INVENTORY_SLOTS);
        assert_eq!(decoded.items, vec![item(0, 1), item(4, 99)]);
        assert_eq!(
            decoded
                .problems
                .iter()
                .map(|problem| problem.index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn out_of_range_slots_are_skipped_and_duplicates_use_the_last() {
        let tag = Tag::List(vec![
            encode_slotted(&item(255, 1)).unwrap(),
            encode_slotted(&item(2, 1)).unwrap(),
            encode_slotted(&item(2, 99)).unwrap(),
        ]);
        let decoded = decode_list(&tag, INVENTORY_SLOTS);
        assert_eq!(decoded.items, vec![item(2, 99)]);
        assert_eq!(decoded.problems.len(), 1);
        assert_eq!(decoded.problems[0].index, 0);
    }

    #[test]
    fn non_list_is_read_as_empty_with_a_problem() {
        let decoded = decode_list(&Tag::Int(7), INVENTORY_SLOTS);
        assert!(decoded.items.is_empty());
        assert_eq!(decoded.problems.len(), 1);
    }

    #[test]
    fn empty_stacks_are_not_written() {
        let tag = encode_list(&[
            SlottedItem {
                slot: 0,
                stack: ItemStack::empty(),
            },
            item(1, 1),
        ])
        .unwrap();
        let Tag::List(values) = tag else {
            panic!("not a list");
        };
        assert_eq!(values.len(), 1);
        assert_eq!(
            decode_list(&Tag::List(values), INVENTORY_SLOTS).items,
            vec![item(1, 1)]
        );
    }

    #[test]
    fn invalid_count_and_slot_return_errors() {
        assert!(encode_item(&ItemStack::new(STONE, 100)).is_err());
        assert!(
            encode_slotted(&SlottedItem {
                slot: 256,
                stack: ItemStack::new(STONE, 1),
            })
            .is_err()
        );
    }
}
