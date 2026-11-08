//! `DebugStickItem`。
//!
//! 左键在属性之间移动，右键改当前属性的值。权限不够时什么都不做。
//! 方块本身不会被挖掉，那是 `canDestroyBlock` 的事。
//!
//! Left click moves between properties. Right click changes the current property.
//! Nothing happens without permission. The block is not broken; that is `canDestroyBlock`.

use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};

use crate::{
    context::UseContext,
    interact::InteractionResult,
    level::Effect,
    state::BlockState,
    world::{UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, World},
};

const EMPTY_KEY: &str = "item.minecraft.debug_stick.empty";
const UPDATE_KEY: &str = "item.minecraft.debug_stick.update";
/// `DebugStickItem.handleInteraction` 右键 `setBlock` 用的标志：只更新客户端和已知形状。
///
/// Flags `DebugStickItem.handleInteraction` passes to `setBlock` on a right click.
const CYCLE_FLAGS: u32 = UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE;

/// 左键选中的属性，以及提示里要显示的当前值。
///
/// The property a left click selects, and the current value the overlay shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickSelect {
    /// 方块没有属性。提示 `item.minecraft.debug_stick.empty`。
    Empty { block_name: &'static str },
    /// 选中了 `property`，当前值是 `value`。
    Property {
        property: &'static str,
        value: &'static str,
    },
}

/// `handleInteraction` 在 `cycle == false` 时挑下一个属性。方块没有属性时是 [`StickSelect::Empty`]。
///
/// `stored` 是组件里这个方块记下的属性名，没有就是 `None`。
///
/// The `cycle == false` half of `handleInteraction`. [`StickSelect::Empty`] means the block
/// has no properties.
///
/// `stored` is the property name saved for this block, or `None`.
pub fn select(state: BlockState, stored: Option<&str>, backward: bool) -> StickSelect {
    let properties = state.block().properties;
    if properties.is_empty() {
        return StickSelect::Empty {
            block_name: state.block().name,
        };
    }
    let names: Vec<&str> = properties.iter().map(|property| property.name).collect();
    let property = relative(&names, stored, backward);
    let value = state.property_text(property).unwrap_or(property);
    StickSelect::Property { property, value }
}

/// `DebugStickItem.useOn`。没有权限时直接失败，和原来的 oracle 一样。
pub(crate) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    // `handleInteraction` 的第一行就是 `canUseGameMasterBlocks`。
    //
    // The first line of `handleInteraction` is `canUseGameMasterBlocks`.
    if !ctx.actor.can_use_game_master_blocks {
        return InteractionResult::Fail;
    }
    let pos = ctx.clicked_pos();
    let state = world.block_state(pos);
    let properties = state.block().properties;
    if properties.is_empty() {
        overlay(world, EMPTY_KEY, Some(state.block().name), None);
        return InteractionResult::Fail;
    }
    let Some(stored) = stored_property(ctx.stack, ctx.item, state.block().name) else {
        return InteractionResult::Fail;
    };
    let names: Vec<&str> = properties.iter().map(|property| property.name).collect();
    // 记下来的名字必须是这个方块现在的属性。没有，或者不是，就用第一个。
    //
    // The saved name has to be a property this block still has. Otherwise use the first.
    let property = names
        .iter()
        .copied()
        .find(|name| stored.as_deref() == Some(*name))
        .or_else(|| names.first().copied());
    let Some(property) = property else {
        return InteractionResult::Fail;
    };
    let current = state.property_text(property);
    let values = property_values(state, property);
    let Some(next) = values
        .as_ref()
        .map(|values| relative(values, current, ctx.actor.secondary_use))
    else {
        return InteractionResult::Fail;
    };
    let new_state = state.try_with_text(property, next).unwrap_or(state);
    // 值和原来一样时 `setBlock` 不改世界，提示仍然要发。
    //
    // `setBlock` leaves the world alone when the value does not change. The overlay is still sent.
    world.set_block(pos, new_state, CYCLE_FLAGS);
    overlay(world, UPDATE_KEY, Some(property), Some(next));
    InteractionResult::Success
}

/// 组件被移除时返回 `None`。组件在、只是没有这个方块的记录时，返回 `Some(None)`。
fn stored_property(
    stack: Option<&ItemStack>,
    item: i32,
    block_name: &str,
) -> Option<Option<String>> {
    let owned = match stack {
        Some(stack) => {
            let patch = stack.patch();
            match defaults::resolve(stack.item, &patch, kinds::DEBUG_STICK_STATE) {
                Some(ComponentValue::DebugStickState(tag)) => tag.clone(),
                _ => return None,
            }
        }
        None => match defaults::of_item(item).get(kinds::DEBUG_STICK_STATE) {
            Some(ComponentValue::DebugStickState(tag)) => tag.clone(),
            _ => return None,
        },
    };
    let tag = &owned;
    let Tag::Compound(entries) = tag else {
        return Some(None);
    };
    let stored = entries.iter().find_map(|(key, value)| {
        if key == block_name
            && let Tag::String(property) = value
        {
            Some(property.clone())
        } else {
            None
        }
    });
    Some(stored)
}

fn property_values(state: BlockState, name: &str) -> Option<Vec<&'static str>> {
    state
        .block()
        .properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.values.to_vec())
}

fn overlay(
    world: &mut World,
    key: &'static str,
    first: Option<&'static str>,
    second: Option<&'static str>,
) {
    world.push_effect(Effect::Overlay { key, first, second });
}

/// `Util.findNextInIterable` / `findPreviousInIterable`。
///
/// 往前走时，当前值缺失或不在列表里就取第一个，走到头回到第一个。
/// 往回走时，当前值缺失或不在列表里就取最后一个，已经是第一个则取最后一个。
fn relative<'a>(values: &[&'a str], current: Option<&str>, backward: bool) -> &'a str {
    if backward {
        previous(values, current)
    } else {
        next(values, current)
    }
}

fn next<'a>(values: &[&'a str], current: Option<&str>) -> &'a str {
    let Some(current) = current else {
        return values[0];
    };
    match values.iter().position(|value| *value == current) {
        Some(index) if index + 1 < values.len() => values[index + 1],
        _ => values[0],
    }
}

fn previous<'a>(values: &[&'a str], current: Option<&str>) -> &'a str {
    let Some(index) = current.and_then(|current| values.iter().position(|value| *value == current))
    else {
        return values[values.len() - 1];
    };
    if index == 0 {
        values[values.len() - 1]
    } else {
        values[index - 1]
    }
}
