//! `/give` 和 `/clear`。物品栏沿用现有的 `inventory` 接口。
//!
//! `/give` and `/clear`. Inventory changes go through the existing `inventory` helpers.

use mc_bamboo_command::{ItemPredicateInput, ParsedCommand};
use mc_bamboo_data::items::ITEMS;
use mc_bamboo_protocol::{
    item::ItemStack,
    text::{Component, TranslateArg},
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        select,
    },
    global::Global,
    inbound::PlayerId,
    inventory,
    region::Region,
};

pub(crate) fn give(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = required_entity(parsed, "targets")?;
    let targets = select::players(region, id, level_of(region, id), selector)?;
    let item = parsed
        .get("item")
        .and_then(|value| value.as_item())
        .ok_or_else(|| fail("command.failed"))?;
    let count = parsed
        .get("count")
        .and_then(|value| value.as_integer())
        .unwrap_or(1);
    let max_count = item.item.max_stack_size.saturating_mul(100);
    if count > max_count {
        return Err(fail_with(
            "commands.give.failed.toomanyitems",
            [
                TranslateArg::from(max_count),
                Component::translate(description_id(item.item), []).into(),
            ],
        ));
    }
    let item_id = registry_id(item.item);
    let stack_limit = item.item.max_stack_size.max(1);
    let display = Component::translate(description_id(item.item), []);
    let mut names = Vec::new();
    for target in &targets {
        let creative = region.players.get(target).is_some_and(|player| {
            player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative
        });
        let Some(player) = region.players.get_mut(target) else {
            continue;
        };
        let mut remaining = count;
        while remaining > 0 {
            let size = remaining.min(stack_limit);
            remaining -= size;
            // 创造模式装不下就按官方清掉。`Inventory.add` 不生成物品实体，也不碰 `level.random`。
            //
            // Creative leftovers are discarded as vanilla does. `Inventory.add` spawns no
            // item entity and does not touch `level.random`.
            let _ = player
                .inventory
                .add(ItemStack::new(item_id, size), creative);
        }
        names.push(player.name.clone());
    }
    for target in &targets {
        send_slots(region, *target);
    }
    let message = if names.len() == 1 {
        Component::translate(
            "commands.give.success.single",
            [
                TranslateArg::from(count),
                display.into(),
                Component::text(names[0].clone()).into(),
            ],
        )
    } else {
        Component::translate(
            "commands.give.success.multiple",
            [
                TranslateArg::from(count),
                display.into(),
                TranslateArg::from(names.len() as i32),
            ],
        )
    };
    feedback::success(region, id, message, true);
    Ok(())
}

pub(crate) fn clear(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let level = level_of(region, id);
    let targets = if let Some(selector) = parsed.get("targets").and_then(|value| value.as_entity())
    {
        select::players(region, id, level, selector)?
    } else {
        vec![id]
    };
    let predicate = parsed
        .get("item")
        .and_then(|value| value.as_item_predicate())
        .cloned();
    let max_count = parsed
        .get("maxCount")
        .and_then(|value| value.as_integer())
        .unwrap_or(-1);
    let counting = max_count == 0;
    let mut total = 0i32;
    let mut only: Option<PlayerId> = None;
    let mut count = 0i32;
    let mut only_nonzero: Option<PlayerId> = None;
    let mut nonzero = 0i32;
    for target in &targets {
        let removed = {
            let Some(player) = region.players.get_mut(target) else {
                continue;
            };
            inventory::clear_or_count(
                &mut player.inventory,
                &|stack| matches_item(predicate.as_ref(), stack),
                counting,
                max_count,
            )
        };
        if !counting {
            send_slots(region, *target);
        }
        total += removed;
        count += 1;
        only = if count == 1 { Some(*target) } else { None };
        if removed != 0 {
            nonzero += 1;
            only_nonzero = if nonzero == 1 { Some(*target) } else { None };
        }
    }
    if total == 0 {
        return Err(clear_failure(region, only, count));
    }
    let message = clear_success(region, counting, only_nonzero, nonzero, total);
    feedback::success(region, id, message, true);
    Ok(())
}

fn matches_item(predicate: Option<&ItemPredicateInput>, stack: &ItemStack) -> bool {
    let Some(predicate) = predicate else {
        return true;
    };
    ITEMS
        .get(stack.item as usize)
        .is_some_and(|item| predicate.test(item))
}

fn clear_failure(region: &Region, only: Option<PlayerId>, count: i32) -> CommandFail {
    if let Some(id) = only {
        let name = player_name(region, id);
        fail_with("clear.failed.single", [Component::text(name).into()])
    } else {
        fail_with("clear.failed.multiple", [TranslateArg::from(count)])
    }
}

fn clear_success(
    region: &Region,
    counting: bool,
    only: Option<PlayerId>,
    players: i32,
    total: i32,
) -> Component {
    let key = match (counting, only.is_some()) {
        (true, true) => "commands.clear.test.single",
        (true, false) => "commands.clear.test.multiple",
        (false, true) => "commands.clear.success.single",
        (false, false) => "commands.clear.success.multiple",
    };
    if let Some(id) = only {
        Component::translate(
            key,
            [
                TranslateArg::from(total),
                Component::text(player_name(region, id)).into(),
            ],
        )
    } else {
        Component::translate(key, [TranslateArg::from(total), players.into()])
    }
}

/// 方块物品用 `block.minecraft.*`，其他物品用 `item.minecraft.*`。
///
/// Block items use `block.minecraft.*`. Other items use `item.minecraft.*`.
fn description_id(item: &mc_bamboo_data::Item) -> String {
    let kind = if item.block.is_some() {
        "block"
    } else {
        "item"
    };
    format!("{kind}.{}", item.name.replace(':', "."))
}

fn registry_id(item: &mc_bamboo_data::Item) -> i32 {
    // `ITEMS` 是 `const` 切片，指针不稳定，按名字找注册表下标。
    //
    // `ITEMS` is a `const` slice, so pointers are not stable. Look the id up by name.
    ITEMS
        .iter()
        .position(|candidate| candidate.name == item.name)
        .unwrap_or(0) as i32
}

fn required_entity<'a>(
    parsed: &'a ParsedCommand,
    name: &str,
) -> Result<&'a mc_bamboo_command::EntitySelector, CommandFail> {
    parsed
        .get(name)
        .and_then(|value| value.as_entity())
        .ok_or_else(|| fail("command.failed"))
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(0)
}

fn player_name(region: &Region, id: PlayerId) -> String {
    region
        .players
        .get(&id)
        .map(|player| player.name.clone())
        .unwrap_or_default()
}

fn send_slots(region: &mut Region, id: PlayerId) {
    let Region { players, links, .. } = region;
    if let Some(player) = players.get_mut(&id) {
        inventory::send_all_slots(player, links);
    }
}
