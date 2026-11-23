//! 进度触发器的接入层：把游戏里已经发生的事件按触发器类型送给玩家进度。
//!
//! 条件判定复用 `mc-bamboo-loot` 的 `ItemPredicate` / `EntityPredicate` /
//! `LocationPredicate`，以及 `mc-bamboo-protocol` 的方块谓词；没有对应系统的触发器
//! （下界、末地、交易、钓鱼等）只在 `docs/advancements.md` 里列为已知差异。
//!
//! All 58 official trigger types parse; only those with a Bamboo event source fire.

use mc_bamboo_advancements::AdvancementTree;
use mc_bamboo_protocol::item::ItemStack;
use serde_json::Value;

use crate::{inbound::PlayerId, region::Region};

/// 已经接上游戏事件来源的触发器（22 种）。其余 36 种见 `docs/advancements.md` 的表。
pub const CONNECTED_TRIGGERS: &[&str] = &[
    "minecraft:tick",
    "minecraft:location",
    "minecraft:inventory_changed",
    "minecraft:recipe_unlocked",
    "minecraft:recipe_crafted",
    "minecraft:placed_block",
    "minecraft:item_used_on_block",
    "minecraft:consume_item",
    "minecraft:slept_in_bed",
    "minecraft:enter_block",
    "minecraft:filled_bucket",
    "minecraft:effects_changed",
    "minecraft:player_killed_entity",
    "minecraft:entity_killed_player",
    "minecraft:player_hurt_entity",
    "minecraft:entity_hurt_player",
    "minecraft:enchanted_item",
    "minecraft:tame_animal",
    "minecraft:bred_animals",
    "minecraft:used_totem",
    "minecraft:shot_crossbow",
    "minecraft:killed_by_arrow",
    "minecraft:item_durability_changed",
    "minecraft:using_item",
    "minecraft:player_interacted_with_entity",
    "minecraft:started_riding",
    "minecraft:any_block_use",
    "minecraft:default_block_use",
    "minecraft:fall_from_height",
    "minecraft:thrown_item_picked_up_by_player",
    "minecraft:summoned_entity",
    "minecraft:target_hit",
];

/// 借用被拆开的地方（实体 tick、`HurtParts`）只压一条事件，玩家 tick 末尾统一 dispatch。
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub player: PlayerId,
    pub trigger: &'static str,
    pub payload: Value,
}

/// 记录一条事件；不立即判定，避免和 `Region` 的可变借用冲突。
pub(crate) fn record(region: &mut Region, player: PlayerId, trigger: &'static str, payload: Value) {
    region.pending_triggers.push(PendingEvent {
        player,
        trigger,
        payload,
    });
}

/// 玩家 tick 里统一 dispatch 这一刻攒下的事件。
pub(crate) fn drain(region: &mut Region) {
    let events = std::mem::take(&mut region.pending_triggers);
    for event in events {
        fire(region, event.player, event.trigger, &event.payload);
    }
}

/// 官方 `InventoryChangedTrigger`：条件里的 `items` 是一组物品谓词，玩家物品栏里
/// 每堆都要能匹配其中一条。Bamboo 的物品堆没有战利品上下文，组件匹配按注册表 ID。
fn item_predicates_match(conditions: &Value, stacks: &[ItemStack]) -> bool {
    let Some(items) = conditions.get("items").and_then(Value::as_array) else {
        return true;
    };
    for value in items {
        let ids = match value.get("items") {
            Some(Value::String(name)) => vec![name.as_str()],
            Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        // 标签用 `#minecraft:...`，单个物品用完整名字。
        let matches = |stack: &ItemStack| -> bool {
            for name in &ids {
                if let Some(tag) = name.strip_prefix('#')
                    && let Some(ids) = mc_bamboo_data::item_tag(tag)
                {
                    if ids.contains(&stack.item) {
                        return true;
                    }
                } else if mc_bamboo_data::item_by_name(name).is_some_and(|item| {
                    mc_bamboo_data::items::ITEMS
                        .iter()
                        .position(|candidate| candidate.name == item.name)
                        == Some(stack.item as usize)
                }) {
                    return true;
                }
            }
            false
        };
        if stacks
            .iter()
            .any(|stack| !stack.is_empty() && matches(stack))
        {
            continue;
        }
        return false;
    }
    true
}

/// 按触发器名字给一名玩家送出一次事件。`payload` 是事件原文，判定用它。
pub(crate) fn fire(region: &mut Region, id: PlayerId, trigger: &str, payload: &Value) {
    let tree = AdvancementTree::global();
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let listeners: Vec<(usize, String)> = player
        .advancements
        .listeners
        .by_trigger
        .get(trigger)
        .cloned()
        .unwrap_or_default();
    let mut completed = Vec::new();
    for (index, criterion) in listeners {
        let conditions = tree.nodes[index]
            .advancement
            .criteria
            .iter()
            .find(|(name, _)| *name == criterion)
            .map(|(_, value)| &value.conditions);
        let matched = conditions.is_some_and(|conditions| match trigger {
            // 无条件的简单触发器：官方 `SimpleCriterionTrigger` 的判据没有条件字段。
            "minecraft:tick"
            | "minecraft:enter_block"
            | "minecraft:used_totem"
            | "minecraft:shot_crossbow"
            | "minecraft:started_riding"
            | "minecraft:any_block_use"
            | "minecraft:default_block_use"
            | "minecraft:killed_by_arrow"
            | "minecraft:fall_from_height"
            | "minecraft:target_hit"
            | "minecraft:summoned_entity"
            | "minecraft:thrown_item_picked_up_by_player"
            | "minecraft:player_interacted_with_entity"
            | "minecraft:bred_animals"
            | "minecraft:tame_animal" => conditions.as_object().is_some_and(|map| map.is_empty()),
            // 带 id / 位置 / 实体字段的触发器：只认本层能完整表达的字段。
            "minecraft:slept_in_bed"
            | "minecraft:filled_bucket"
            | "minecraft:effects_changed"
            | "minecraft:recipe_crafted"
            | "minecraft:using_item"
            | "minecraft:enchanted_item"
            | "minecraft:item_durability_changed"
            | "minecraft:recipe_unlocked"
            | "minecraft:placed_block"
            | "minecraft:item_used_on_block"
            | "minecraft:consume_item" => matches_id(conditions, payload),
            "minecraft:inventory_changed" => item_predicates_match(conditions, &[]),
            "minecraft:location" => location_matches(conditions, payload),
            "minecraft:player_killed_entity"
            | "minecraft:entity_killed_player"
            | "minecraft:player_hurt_entity"
            | "minecraft:entity_hurt_player" => matches_entity(conditions, payload),
            _ => false,
        });
        if matched {
            let was_done = player
                .advancements
                .progress
                .get(&index)
                .is_some_and(|progress| progress.is_done());
            player.advancements.award(tree, index, &criterion);
            if !was_done
                && player
                    .advancements
                    .progress
                    .get(&index)
                    .is_some_and(|progress| progress.is_done())
            {
                completed.push(index);
            }
        }
    }
    for index in completed {
        crate::advancement_rewards::grant(region, id, index);
    }
}

/// JSON 里的 `id` / `item` / `recipe` / `block` 字段和事件同名时算匹配。
fn matches_id(conditions: &Value, payload: &Value) -> bool {
    let keys = [
        "item", "recipe", "recipes", "block", "blocks", "id", "location", "biome",
    ];
    if conditions
        .as_object()
        .is_some_and(|map| map.keys().any(|key| !keys.contains(&key.as_str())))
    {
        return false;
    }
    keys.iter().all(|&key| {
        let Some(expected) = conditions.get(key) else {
            return true;
        };
        let Some(actual) = payload.get(key) else {
            return false;
        };
        expected == actual
    })
}

/// 官方 `LocationTrigger`：条件里的 `position` / `biome` / `dimension` 都要对得上。
fn location_matches(conditions: &Value, payload: &Value) -> bool {
    // `location` / `position` / `smokey` / `light` 等本层没建模：出现就不算匹配。
    let known = ["biome", "dimension", "feature", "position", "x", "y", "z"];
    if conditions
        .as_object()
        .is_some_and(|map| map.keys().any(|key| !known.contains(&key.as_str())))
    {
        return false;
    }
    for key in ["biome", "dimension", "feature"] {
        if let Some(expected) = conditions.get(key)
            && payload.get(key) != Some(expected)
        {
            return false;
        }
    }
    if let Some(position) = conditions.get("position") {
        // 只有数据包给了明确坐标时才比较，范围谓词留给战利品层的谓词实现。
        for key in ["x", "y", "z"] {
            if let (Some(expected), Some(actual)) = (
                position.get(key).and_then(Value::as_f64),
                payload.get(key).and_then(Value::as_f64),
            ) && expected != actual
            {
                return false;
            }
        }
    }
    true
}

/// 四个战斗触发器：条件里的 `entity` / `victim` / `source` 是实体谓词，
/// 只按 `type` 字段匹配 Bamboo 已有的实体类型注册表 ID。
fn matches_entity(conditions: &Value, payload: &Value) -> bool {
    for key in [
        "entity",
        "victims",
        "source",
        "child",
        "partner",
        "bystander",
    ] {
        let Some(expected) = conditions.get(key) else {
            continue;
        };
        let Some(actual) = payload.get(key) else {
            return false;
        };
        let Some(Value::String(expected_type)) = expected.get("type") else {
            continue;
        };
        if actual.get("type").and_then(Value::as_str) != Some(expected_type.as_str()) {
            return false;
        }
    }
    true
}

/// 官方 `ServerPlayer.tick` 每刻触发 TICK（26.3 字节码里没有模 20）。
pub(crate) fn trigger_tick(region: &mut Region, player: PlayerId) {
    fire(region, player, "minecraft:tick", &Value::Null);
}

/// 官方 `ServerPlayer.doTick` 在 `tickCount % 20 == 0` 时触发 LOCATION。
pub(crate) fn trigger_location(region: &mut Region, player: PlayerId) {
    let Some(player) = region.players.get(&player) else {
        return;
    };
    let pos = player.position();
    // 群系由战利品上下文在后续阶段接入；缺省按主世界的 plains 上报。
    let biome = "minecraft:plains";
    let payload = serde_json::json!({
        "x": pos.x,
        "y": pos.y,
        "z": pos.z,
        "biome": biome,
        "dimension": "minecraft:overworld",
    });
    fire(region, player.id, "minecraft:location", &payload);
}

/// 玩家物品栏变化后调用，照官方 `InventoryChangedTrigger.trigger`。
pub(crate) fn inventory_changed(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let stacks = player.inventory.slots.clone();
    let ids: Vec<&str> = stacks
        .iter()
        .filter(|stack| !stack.is_empty())
        .filter_map(|stack| {
            mc_bamboo_data::items::ITEMS
                .get(stack.item as usize)
                .map(|item| item.name)
        })
        .collect();
    let payload = serde_json::json!({ "items": ids });
    let tree = AdvancementTree::global();
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let listeners: Vec<(usize, String)> = player
        .advancements
        .listeners
        .by_trigger
        .get("minecraft:inventory_changed")
        .cloned()
        .unwrap_or_default();
    let mut completed = Vec::new();
    for (index, criterion) in listeners {
        let conditions = tree.nodes[index]
            .advancement
            .criteria
            .iter()
            .find(|(name, _)| *name == criterion)
            .map(|(_, value)| &value.conditions);
        if conditions.is_some_and(|conditions| item_predicates_match(conditions, &stacks)) {
            let was_done = player
                .advancements
                .progress
                .get(&index)
                .is_some_and(|p| p.is_done());
            player.advancements.award(tree, index, &criterion);
            if !was_done
                && player
                    .advancements
                    .progress
                    .get(&index)
                    .is_some_and(|p| p.is_done())
            {
                completed.push(index);
            }
        }
    }
    for index in completed {
        crate::advancement_rewards::grant(region, id, index);
    }
    let _ = payload;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_triggers_are_unique_and_known() {
        let tree = AdvancementTree::global();
        let set: std::collections::BTreeSet<&str> = CONNECTED_TRIGGERS.iter().copied().collect();
        assert_eq!(set.len(), CONNECTED_TRIGGERS.len());
        for trigger in CONNECTED_TRIGGERS {
            assert!(tree.triggers.contains(trigger), "{trigger} 不在注册表里");
        }
    }

    #[test]
    fn item_predicate_matches_a_tag_or_an_item() {
        let conditions = serde_json::json!({"items": [{"items": "#minecraft:oak_logs"}]});
        let mut stack = ItemStack::new(
            mc_bamboo_data::items::ITEMS
                .iter()
                .position(|item| item.name == "minecraft:oak_log")
                .unwrap() as i32,
            1,
        );
        assert!(item_predicates_match(&conditions, &[stack.clone()]));
        stack.item = mc_bamboo_data::items::STONE;
        assert!(!item_predicates_match(&conditions, &[stack]));
    }
}
