//! 物品堆的网络格式。

use mc_bamboo_data::{
    items::{AIR, ITEMS},
    registries::data_component_type as kinds,
};

use crate::{
    codec::{DecodeError, Reader, write_varint},
    components::{ComponentPatch, ComponentValue, defaults, validate},
};

/// 官方校验的数量上限（`ItemStack.MAP_CODEC`）。
const MAX_COUNT: i32 = 99;

/// 组件补丁里增加项和移除项各自的上限。
///
/// 官方不限制，但组件类型一共只有 122 种，合法的补丁不会有重复项。
/// 没有这个上限时，畸形包里每两个字节就能换来一个几十字节的条目。
const MAX_PATCH_ENTRIES: usize = 256;

/// 一堆物品。组件的值在这里是不透明的字节，类型化的视图在 [`crate::components`]。
///
/// 字节来自客户端时，`set_creative_mode_slot` 已经按官方 `Codec` 解过并写回。
/// 发给任何玩家都用 [`write`](Self::write)，组件原样出去。
///
/// Bytes from a client have already been decoded and written back through the official
/// codec. [`write`](Self::write) sends that patch to every player.
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
    /// 和官方一样校验物品已注册、`count` 在 `1..=99` 以内，再解析每一项组件的值：官方的
    /// `ItemStack.validatedStreamCodec` 在这里用 `ItemStack.CODEC.encodeStart` 把整个物品
    /// 过一遍 `Codec`，解不开就抛 `DecoderException`，连接断开。我们解不开组件时返回错误，
    /// 连接层照样断开，结果一致。
    ///
    /// 注意官方在这里并不调用 `validateComponents`：数量超过上限、组件互相矛盾都是
    /// `handleSetCreativeModeSlot` 里静默忽略这一格，见 `mc-bamboo-game` 的 `inventory`。
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
        let stack = Self {
            item,
            count,
            added,
            removed,
        };
        // 官方 `validatedStreamCodec`：每项组件按 `Codec` 解，再按解出来的值写回去，
        // 最后整堆物品还要过得了 `ItemStack.CODEC`。
        //
        // `validatedStreamCodec`: decode every component, store what its codec writes
        // back, then require the whole stack to encode through `ItemStack.CODEC`.
        let patch = ComponentPatch::from_opaque(&stack)?;
        match crate::components::archive::stack::encode_stack(item, count, &patch) {
            Ok(encoded) => encoded.release(),
            Err(_) => {
                patch.release();
                return Err(DecodeError::InvalidComponentValue(
                    "item stack codec rejected the value",
                ));
            }
        }
        let (added, removed) = patch.to_opaque();
        patch.release();
        Ok(Self {
            item,
            count,
            added,
            removed,
        })
    }

    /// 按可信格式写入，也就是所有 clientbound 包用的格式：组件的值前面没有长度。
    ///
    /// 补丁原样写出去，和官方一样。
    ///
    /// Trusted clientbound format: component values have no length prefix.
    /// The patch is written as stored, matching vanilla.
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

    /// 官方 `ItemStack.getMaxStackSize`：默认组件被补丁覆盖之后的 `max_stack_size`。
    ///
    /// 空物品堆的组件表是空的（官方 `getComponents`），兜底值和官方一样是 1。
    /// 组件解不开时按默认组件算：解不开的物品进不了世界，调用方已经校验过。
    pub fn max_stack_size(&self) -> i32 {
        if self.is_empty() {
            return 1;
        }
        validate::max_stack_size(self.item, &self.patch())
    }

    /// 官方 `ItemStack.isStackable`：上限大于一，而且没有掉过耐久。
    pub fn is_stackable(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        let patch = self.patch();
        validate::max_stack_size(self.item, &patch) > 1 && !self.damage_is_positive(&patch)
    }

    /// 官方 `ItemStack.isDamaged`：能损坏，而且耐久掉过。
    ///
    /// `ItemStack.isDamaged`: the item can take damage, and its damage is above zero.
    pub fn is_damaged(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        let patch = self.patch();
        self.damage_is_positive(&patch)
    }

    /// 官方 `ItemStack.isSameItem`：只看物品，空物品堆都是空气。
    pub fn is_same_item(&self, other: &Self) -> bool {
        self.item_or_air() == other.item_or_air()
    }

    /// 官方 `ItemStack.isSameItemSameComponents`。空物品堆之间也算相同。
    pub fn is_same_item_same_components(&self, other: &Self) -> bool {
        if !self.is_same_item(other) {
            return false;
        }
        if self.is_empty() && other.is_empty() {
            return true;
        }
        self.patch().same_as(&other.patch())
    }

    /// 官方 `ItemStack.getItem`：空物品堆是空气。
    const fn item_or_air(&self) -> i32 {
        if self.is_empty() { AIR } else { self.item }
    }

    /// 官方 `ItemStack.matches`：同种物品、同样的组件，数量也相同。
    pub fn matches(&self, other: &Self) -> bool {
        self.count == other.count && self.is_same_item_same_components(other)
    }

    /// 类型化的组件补丁。解不开时当成空补丁。
    pub fn patch(&self) -> ComponentPatch {
        ComponentPatch::from_opaque(self).unwrap_or_default()
    }

    /// 官方 `ItemStack.isDamaged` 里看补丁的那一半。
    ///
    /// The patch half of `ItemStack.isDamaged`.
    fn damage_is_positive(&self, patch: &ComponentPatch) -> bool {
        let resolve = |kind| defaults::resolve(self.item, patch, kind);
        let int = |kind| match resolve(kind) {
            Some(ComponentValue::MaxDamage(value) | ComponentValue::Damage(value)) => Some(*value),
            _ => None,
        };
        // `isDamageableItem`：有耐久上限、没有不可破坏、而且有当前耐久。
        let (Some(max_damage), Some(damage)) = (int(kinds::MAX_DAMAGE), int(kinds::DAMAGE)) else {
            return false;
        };
        if resolve(kinds::UNBREAKABLE).is_some() {
            return false;
        }
        // `getDamageValue` 把值夹在 0 和上限之间。
        damage.clamp(0, max_damage) > 0
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
    use mc_bamboo_data::items;
    use mc_bamboo_nbt::Tag;

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

    /// `lock` 线上是 NBT，解过 `Codec` 之后原样发给所有人。
    ///
    /// `lock` is NBT. After the codec accepts it, every player receives the same bytes.
    fn locked() -> ItemStack {
        let mut lock = Vec::new();
        Tag::Compound(Vec::new()).write_network(&mut lock);
        ItemStack {
            item: 1,
            count: 2,
            added: vec![(kinds::LOCK, lock)],
            removed: Vec::new(),
        }
    }

    #[test]
    fn empty_stack_is_a_single_zero() {
        assert!(ItemStack::empty().is_empty());
        assert_eq!(trusted(&ItemStack::empty()), [0]);
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
    fn every_player_sees_the_same_components() {
        assert_eq!(trusted(&with_components()), hex("02 01 01 01 01 10 0c"));
        // 空的 `lock` 是一个空复合标签，发给谁都是这份字节。
        // An empty `lock` is an empty compound, and every player gets those bytes.
        assert_eq!(trusted(&locked()), hex("02 01 01 00 51 0a 00"));
        assert_eq!(locked().added.len(), 1);
    }

    #[test]
    fn max_stack_size_and_stackability_come_from_the_components() {
        assert_eq!(ItemStack::new(items::STONE, 1).max_stack_size(), 64);
        assert_eq!(ItemStack::new(items::DIAMOND_SWORD, 1).max_stack_size(), 1);
        assert!(ItemStack::new(items::STONE, 1).is_stackable());
        assert!(!ItemStack::new(items::DIAMOND_SWORD, 1).is_stackable());

        // 补丁里的上限盖过默认组件。
        let mut stack = ItemStack::new(items::STONE, 1);
        stack.added.push((kinds::MAX_STACK_SIZE, vec![1]));
        assert_eq!(stack.max_stack_size(), 1);
        assert!(!stack.is_stackable());
    }

    #[test]
    fn a_damaged_tool_does_not_stack() {
        let mut stack = ItemStack::new(items::DIAMOND_SWORD, 1);
        stack.added.push((kinds::MAX_STACK_SIZE, vec![2]));
        assert!(stack.is_stackable());

        // 掉过耐久的工具不能叠，`isDamaged` 为真。
        stack.added.push((kinds::DAMAGE, vec![3]));
        assert!(!stack.is_stackable());
        // 耐久是 0 时没掉过。
        stack.added.last_mut().expect("damage").1 = vec![0];
        assert!(stack.is_stackable());
        // 不可破坏的工具不算能损坏。
        stack.added.last_mut().expect("damage").1 = vec![3];
        stack.added.push((kinds::UNBREAKABLE, Vec::new()));
        assert!(stack.is_stackable());
    }

    #[test]
    fn the_same_components_in_another_order_are_still_the_same() {
        let mut first = ItemStack::new(1, 1);
        first.added.push((kinds::MAX_STACK_SIZE, vec![9]));
        first.added.push((kinds::REPAIR_COST, vec![1]));
        let mut second = ItemStack::new(1, 1);
        second.added.push((kinds::REPAIR_COST, vec![1]));
        second.added.push((kinds::MAX_STACK_SIZE, vec![9]));

        assert!(first.is_same_item_same_components(&second));
        assert!(first.matches(&second));

        second.added.pop();
        assert!(!first.is_same_item_same_components(&second));
        assert!(first.is_same_item(&second));
    }

    #[test]
    fn empty_stacks_are_all_the_same_item() {
        let empty = ItemStack::empty();
        let zero_stone = ItemStack::new(1, 0);
        assert!(empty.is_same_item(&zero_stone));
        assert!(empty.is_same_item_same_components(&zero_stone));
        assert!(!empty.is_same_item(&ItemStack::new(1, 1)));
    }

    #[test]
    fn a_component_that_cannot_be_decoded_closes_the_connection() {
        // `enchantable` 的值必须是正数，官方的构造方法在这里抛异常。
        let mut bytes = hex("01 01 01 00");
        write_varint(&mut bytes, kinds::ENCHANTABLE);
        write_varint(&mut bytes, 1);
        bytes.push(0);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::InvalidComponentValue(
                "enchantment value must be positive"
            ))
        );
    }

    #[test]
    fn an_unknown_component_type_closes_the_connection() {
        let mut bytes = hex("01 01 01 00");
        write_varint(&mut bytes, 1000);
        write_varint(&mut bytes, 0);
        assert_eq!(decode(&bytes), Err(DecodeError::UnknownComponent(1000)));
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
