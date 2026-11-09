//! 营火发给客户端的 `getUpdateTag`。
//!
//! 官方只写 `Items`：每个非空格子一条 `ItemStackWithSlot`，空表也留着。
//! 食物画在营火上靠的就是这段数据。进度不在这个标签里。
//!
//! The campfire `getUpdateTag` sent to the client.
//!
//! Vanilla writes only `Items`: one `ItemStackWithSlot` per occupied slot, and the empty list is
//! kept. Food rendered on the campfire comes from this tag. Progress is not in it.

use mc_bamboo_blocks::behaviour::machines::CampfireEntity;
use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::stack::encode_stack, item::ItemStack};

use crate::region::Region;

/// 把这一格营火的物品写成 `getUpdateTag`，并让广播重发这一格。
///
/// Write this campfire's items as `getUpdateTag` and mark the block to be broadcast again.
pub(crate) fn write_update_tag(region: &mut Region, pos: BlockPos) {
    let bytes = {
        let (level, _) = region.block_level();
        let Some(entity) = level.block_entities.get::<CampfireEntity>(pos) else {
            return;
        };
        let mut out = Vec::new();
        items_tag(entity.items()).write_network(&mut out);
        out
    };
    region.chunks.set_update_tag(pos, Some(bytes));
    region.block_changes.changed.push(pos);
}

fn items_tag(items: &[ItemStack]) -> Tag {
    let entries = items
        .iter()
        .enumerate()
        .filter_map(|(slot, stack)| stack_tag(slot, stack))
        .collect();
    Tag::Compound(vec![("Items".to_owned(), Tag::List(entries))])
}

fn stack_tag(slot: usize, stack: &ItemStack) -> Option<Tag> {
    if stack.is_empty() {
        return None;
    }
    let encoded = encode_stack(stack.item, stack.count, &stack.patch()).ok()?;
    let Tag::Compound(mut entries) = encoded.to_tag() else {
        return None;
    };
    let slot = i8::try_from(slot).unwrap_or(0);
    entries.insert(0, ("Slot".to_owned(), Tag::Byte(slot)));
    Some(Tag::Compound(entries))
}
