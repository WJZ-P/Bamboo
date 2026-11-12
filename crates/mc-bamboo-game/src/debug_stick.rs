//! 调试棒的左键。
//!
//! 右键在方块层的 `DebugStickItem.useOn`。这里对应 `canDestroyBlock` 里
//! `cycle == false` 的那次 `handleInteraction`：改组件，不改方块。
//!
//! Left click of the debug stick.
//!
//! The right click is `DebugStickItem.useOn` in the block layer. This is the
//! `cycle == false` `handleInteraction` inside `canDestroyBlock`: it changes the
//! component and not the block.

use mc_bamboo_blocks::{Actor, BlockState, StickSelect, debug_stick_select};
use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
    text::Component,
};

use crate::{
    book::{self, set_component},
    inbound::PlayerId,
    region::Region,
};

const EMPTY_KEY: &str = "item.minecraft.debug_stick.empty";
const SELECT_KEY: &str = "item.minecraft.debug_stick.select";

/// 主手是调试棒时处理左键并返回 `true`，调用方不再挖方块。
///
/// Handles a left click when the main hand is a debug stick and returns `true`.
/// The caller then does not break the block.
pub(crate) fn on_left_click(
    region: &mut Region,
    id: PlayerId,
    actor: &Actor,
    pos: BlockPos,
) -> bool {
    if actor.main_hand != items::DEBUG_STICK {
        return false;
    }
    // `canDestroyBlock` 对调试棒恒返回 false。没权限时不提示、不改组件。
    //
    // `canDestroyBlock` always returns false for a debug stick. Without permission there
    // is no overlay and the component stays put.
    if actor.can_use_game_master_blocks {
        select(region, id, actor, pos);
    }
    true
}

fn select(region: &mut Region, id: PlayerId, actor: &Actor, pos: BlockPos) {
    let Some(state_id) = region.chunks.block(pos) else {
        return;
    };
    let state = BlockState(state_id);
    if matches!(
        debug_stick_select(state, None, false),
        StickSelect::Empty { .. }
    ) {
        overlay(region, id, EMPTY_KEY, state.block().name, None);
        return;
    }
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let slot = player.inventory.hand_slot(Hand::Main);
    let Some(tag) = stick_tag(&player.inventory.slots[slot]) else {
        return;
    };
    let block_name = state.block().name;
    let stored = match &tag {
        Tag::Compound(entries) => entries.iter().find_map(|(key, value)| {
            if key == block_name
                && let Tag::String(property) = value
            {
                Some(property.clone())
            } else {
                None
            }
        }),
        _ => None,
    };
    let StickSelect::Property { property, value } =
        debug_stick_select(state, stored.as_deref(), actor.secondary_use)
    else {
        return;
    };
    let updated = with_property(&tag, block_name, property);
    {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        set_component(
            &mut player.inventory.slots[slot],
            kinds::DEBUG_STICK_STATE,
            &ComponentValue::DebugStickState(updated),
        );
    }
    book::sync_edited_slot(region, id, slot);
    overlay(region, id, SELECT_KEY, property, Some(value));
}

fn stick_tag(stack: &ItemStack) -> Option<Tag> {
    match defaults::resolve(stack.item, &stack.patch(), kinds::DEBUG_STICK_STATE) {
        Some(ComponentValue::DebugStickState(tag)) => Some(tag.clone()),
        _ => None,
    }
}

fn with_property(tag: &Tag, block: &str, property: &str) -> Tag {
    let mut entries = match tag {
        Tag::Compound(entries) => entries.clone(),
        _ => Vec::new(),
    };
    let value = Tag::String(property.to_owned());
    if let Some(entry) = entries.iter_mut().find(|(key, _)| key == block) {
        entry.1 = value;
    } else {
        entries.push((block.to_owned(), value));
    }
    Tag::Compound(entries)
}

fn overlay(region: &mut Region, id: PlayerId, key: &str, first: &str, second: Option<&str>) {
    let mut args = vec![first.into()];
    if let Some(second) = second {
        args.push(second.into());
    }
    let content = Component::translate(key, args);
    // 官方 `DebugStickItem` 走 `sendOverlayMessage` → `sendSystemMessage(message, true)`，
    // overlay 恒发（隐藏聊天也收）。
    //
    // Official `DebugStickItem` uses `sendOverlayMessage` → `sendSystemMessage(message, true)`;
    // overlay messages always arrive, even for hidden chat.
    crate::chat::send_system_message(region, id, &content, true);
}
