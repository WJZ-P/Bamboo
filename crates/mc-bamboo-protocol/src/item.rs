//! 物品堆的网络格式。

use mc_bamboo_data::items::{AIR, ITEMS};

use crate::codec::{DecodeError, Reader, write_varint};

/// 官方校验的数量上限（`ItemStack.MAP_CODEC`）。
const MAX_COUNT: i32 = 99;

/// 组件补丁里增加项和移除项各自的上限。
///
/// 官方不限制，但组件类型一共只有 122 种，合法的补丁不会有重复项。
/// 没有这个上限时，畸形包里每两个字节就能换来一个几十字节的条目。
const MAX_PATCH_ENTRIES: usize = 256;

/// 一堆物品。组件的值是不透明的字节，服务端不解析。
///
/// 这些字节来自客户端，服务端没有校验过，所以只能发回给发来它的玩家。发给其他玩家时用
/// [`write_without_components`](Self::write_without_components)，否则内容非法时对方会断线。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemStack {
    /// `minecraft:item` 注册表 ID。
    pub item: i32,
    pub count: i32,
    /// 增加或覆盖的组件：`minecraft:data_component_type` 注册表 ID 和值的字节。
    pub added: Vec<(i32, Vec<u8>)>,
    /// 移除的组件类型。
    pub removed: Vec<i32>,
}

impl ItemStack {
    /// 空物品。
    pub const fn empty() -> Self {
        Self::new(AIR, 0)
    }

    /// 没有组件的物品。
    pub const fn new(item: i32, count: i32) -> Self {
        Self {
            item,
            count,
            added: Vec::new(),
            removed: Vec::new(),
        }
    }

    /// 和官方一样，数量不大于零或者物品是空气都算空。
    pub const fn is_empty(&self) -> bool {
        self.count <= 0 || self.item == AIR
    }

    /// 解码 `set_creative_mode_slot` 用的不可信格式：每个组件的值前面有字节长度。
    ///
    /// 和官方一样校验物品已注册、`count` 在 `1..=99` 以内：未注册的 ID 进了 `set_equipment`，
    /// 其他玩家会断线。组件的内容不解析，所以不校验，见 [`ItemStack`]。
    pub fn read_delimited(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let count = reader.varint()?;
        if count <= 0 {
            return Ok(Self::empty());
        }

        let item = reader.varint()?;
        let added_len = reader.count(MAX_PATCH_ENTRIES)?;
        let removed_len = reader.count(MAX_PATCH_ENTRIES)?;

        let mut added = Vec::with_capacity(added_len);
        for _ in 0..added_len {
            let kind = reader.varint()?;
            let value_len = reader.count(usize::MAX)?;
            added.push((kind, reader.bytes(value_len)?.to_vec()));
        }
        let mut removed = Vec::with_capacity(removed_len);
        for _ in 0..removed_len {
            removed.push(reader.varint()?);
        }

        // 官方构造出来的物品是空的，所以不走后面的校验。
        if item == AIR {
            return Ok(Self::empty());
        }
        if !usize::try_from(item).is_ok_and(|id| id < ITEMS.len()) {
            return Err(DecodeError::InvalidItem(item));
        }
        if count > MAX_COUNT {
            return Err(DecodeError::InvalidItemCount(count));
        }
        Ok(Self {
            item,
            count,
            added,
            removed,
        })
    }

    /// 按可信格式写入，也就是所有 clientbound 包用的格式：组件的值前面没有长度。
    ///
    /// 只能用在发给物品主人的包里，原因见 [`ItemStack`]。
    pub fn write(&self, out: &mut Vec<u8>) {
        if self.write_empty_or_header(out) {
            return;
        }
        write_varint(out, self.added.len() as i32);
        write_varint(out, self.removed.len() as i32);
        for (kind, value) in &self.added {
            write_varint(out, *kind);
            out.extend_from_slice(value);
        }
        for &kind in &self.removed {
            write_varint(out, kind);
        }
    }

    /// 只写物品和数量，不带组件。发给其他玩家时用它。
    pub fn write_without_components(&self, out: &mut Vec<u8>) {
        if self.write_empty_or_header(out) {
            return;
        }
        write_varint(out, 0);
        write_varint(out, 0);
    }

    /// 写入数量和物品。空物品只有一个零，这时返回 `true`，后面不能再写内容。
    fn write_empty_or_header(&self, out: &mut Vec<u8>) -> bool {
        if self.is_empty() {
            write_varint(out, 0);
            return true;
        }
        write_varint(out, self.count);
        write_varint(out, self.item);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::hex;

    fn decode(bytes: &[u8]) -> Result<ItemStack, DecodeError> {
        ItemStack::read_delimited(&mut Reader::new(bytes))
    }

    fn with_components() -> ItemStack {
        ItemStack {
            item: 1,
            count: 2,
            added: vec![(1, vec![0x10])],
            removed: vec![12],
        }
    }

    fn trusted(stack: &ItemStack) -> Vec<u8> {
        let mut out = Vec::new();
        stack.write(&mut out);
        out
    }

    fn without_components(stack: &ItemStack) -> Vec<u8> {
        let mut out = Vec::new();
        stack.write_without_components(&mut out);
        out
    }

    #[test]
    fn empty_stack_is_a_single_zero() {
        assert!(ItemStack::empty().is_empty());
        assert_eq!(trusted(&ItemStack::empty()), [0]);
        assert_eq!(without_components(&ItemStack::empty()), [0]);
        assert_eq!(decode(&[0]), Ok(ItemStack::empty()));
    }

    #[test]
    fn stack_without_components_matches_official_bytes() {
        let stack = ItemStack::new(1, 64);
        assert_eq!(decode(&hex("40 01 00 00")), Ok(stack.clone()));
        assert_eq!(trusted(&stack), hex("40 01 00 00"));
    }

    #[test]
    fn delimited_components_lose_their_length_when_trusted() {
        let stack = decode(&hex("02 01 01 01 01 01 10 0c")).unwrap();
        assert_eq!(stack, with_components());
        assert_eq!(trusted(&stack), hex("02 01 01 01 01 10 0c"));
    }

    #[test]
    fn component_patch_example_from_the_entity_spec() {
        let stack = decode(&hex("01 05 01 01 01 01 05 02")).unwrap();
        assert_eq!(
            stack,
            ItemStack {
                item: 5,
                count: 1,
                added: vec![(1, vec![5])],
                removed: vec![2],
            }
        );
        assert_eq!(trusted(&stack), hex("01 05 01 01 01 05 02"));
    }

    #[test]
    fn other_players_never_see_the_components() {
        assert_eq!(without_components(&with_components()), hex("02 01 00 00"));
    }

    #[test]
    fn writing_ignores_components_of_an_empty_stack() {
        let mut stack = with_components();
        stack.count = 0;
        assert!(stack.is_empty());
        assert_eq!(trusted(&stack), [0]);
    }

    #[test]
    fn air_counts_as_empty_even_with_a_count() {
        assert!(ItemStack::new(0, 5).is_empty());
        assert_eq!(decode(&hex("05 00 00 00")), Ok(ItemStack::empty()));
        // 官方不校验空物品，所以这里的数量超限不是错误。
        assert_eq!(decode(&hex("64 00 00 00")), Ok(ItemStack::empty()));
    }

    #[test]
    fn negative_count_is_empty_and_has_nothing_after_it() {
        assert_eq!(decode(&hex("ff ff ff ff 0f")), Ok(ItemStack::empty()));
    }

    #[test]
    fn count_is_limited_to_what_the_game_accepts() {
        assert_eq!(decode(&hex("63 01 00 00")), Ok(ItemStack::new(1, 99)));
        assert_eq!(
            decode(&hex("64 01 00 00")),
            Err(DecodeError::InvalidItemCount(100))
        );
    }

    #[test]
    fn rejects_a_negative_item_id() {
        assert_eq!(
            decode(&hex("01 ff ff ff ff 0f 00 00")),
            Err(DecodeError::InvalidItem(-1))
        );
    }

    #[test]
    fn rejects_an_item_id_that_is_not_registered() {
        let last = ITEMS.len() as i32 - 1;

        let mut bytes = vec![1];
        write_varint(&mut bytes, last);
        bytes.extend([0, 0]);
        assert_eq!(decode(&bytes), Ok(ItemStack::new(last, 1)));

        let mut bytes = vec![1];
        write_varint(&mut bytes, last + 1);
        bytes.extend([0, 0]);
        assert_eq!(decode(&bytes), Err(DecodeError::InvalidItem(last + 1)));
    }

    #[test]
    fn rejects_truncated_components() {
        assert_eq!(
            decode(&hex("01 01 01 00 01 05 00")),
            Err(DecodeError::UnexpectedEnd)
        );
        assert_eq!(decode(&hex("01 01 00 01")), Err(DecodeError::UnexpectedEnd));
    }

    #[test]
    fn rejects_negative_or_oversized_patch_sizes() {
        assert_eq!(
            decode(&hex("01 01 ff ff ff ff 0f 00")),
            Err(DecodeError::InvalidCount(-1))
        );

        let mut bytes = hex("01 01");
        write_varint(&mut bytes, MAX_PATCH_ENTRIES as i32 + 1);
        write_varint(&mut bytes, 0);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::InvalidCount(MAX_PATCH_ENTRIES as i32 + 1))
        );
    }

    #[test]
    fn rejects_a_negative_value_length() {
        assert_eq!(
            decode(&hex("01 01 01 00 01 ff ff ff ff 0f")),
            Err(DecodeError::InvalidCount(-1))
        );
    }
}
