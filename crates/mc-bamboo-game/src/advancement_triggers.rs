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

/// 已经接上游戏事件来源的触发器。其余见 `docs/advancements.md` 的表。
pub const CONNECTED_TRIGGERS: &[&str] = &[
    "minecraft:tick",
    "minecraft:location",
    "minecraft:inventory_changed",
    "minecraft:recipe_unlocked",
    "minecraft:recipe_crafted",
    "minecraft:placed_block",
    "minecraft:item_used_on_block",
    "minecraft:any_block_use",
    "minecraft:default_block_use",
    "minecraft:consume_item",
    "minecraft:slept_in_bed",
    "minecraft:effects_changed",
    "minecraft:player_hurt_entity",
    "minecraft:player_killed_entity",
    "minecraft:player_interacted_with_entity",
    "minecraft:started_riding",
    "minecraft:shot_crossbow",
    "minecraft:using_item",
    "minecraft:enchanted_item",
    "minecraft:tame_animal",
    "minecraft:bred_animals",
    "minecraft:entity_killed_player",
    "minecraft:entity_hurt_player",
    "minecraft:killed_by_arrow",
    "minecraft:item_durability_changed",
    "minecraft:fall_from_height",
    "minecraft:thrown_item_picked_up_by_player",
    "minecraft:summoned_entity",
    "minecraft:enter_block",
    "minecraft:filled_bucket",
    "minecraft:crafter_recipe_crafted",
    "minecraft:cured_zombie_villager",
    "minecraft:villager_trade",
    "minecraft:fishing_rod_hooked",
    "minecraft:used_ender_eye",
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
        let matched = conditions.is_some_and(|conditions| matches(trigger, payload, conditions));
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

/// 一条事件能不能满足一个判据。`matches_trigger` 是它给对拍用的 JSON 版入口。
///
/// 本层只认识 Bamboo 已有系统能表达的条件字段；出现没建模的字段一律不算匹配，
/// 不白送判据。实体谓词里 `minecraft:entity_type` 按注册表名字比，其余子谓词
/// （花色、位置、骑乘等）暂不建模。
pub(crate) fn matches(trigger: &str, payload: &Value, conditions: &Value) -> bool {
    let empty = serde_json::Map::new();
    let fields = conditions.as_object().unwrap_or(&empty);
    let object = conditions.as_object();
    // 全触发器共同的 `player` 谓词：官方在每个 TriggerInstance 里都检查它。
    // Bamboo 只实现了「无条件」这一种，写了实体子谓词就判不匹配。
    if let Some(player) = fields.get("player")
        && (player
            .as_object()
            .is_some_and(|map| map.len() > 1 || map.keys().any(|key| key != "type"))
            || !entity_predicate_matches(player, payload.get("player")))
    {
        return false;
    }
    let known_conditions = |allowed: &[&str]| {
        fields
            .keys()
            .all(|key| key == "player" || allowed.contains(&key.as_str()))
    };
    match trigger {
        // 官方 `SimpleCriterionTrigger` 的判据没有条件字段（或只有 player）。
        "minecraft:tick"
        | "minecraft:any_block_use"
        | "minecraft:default_block_use"
        | "minecraft:cured_zombie_villager" => {
            known_conditions(&["zombie", "villager"])
                && ["zombie", "villager"].iter().all(|key| {
                    fields.get(*key).is_none_or(|expected| {
                        entity_predicate_matches(expected, payload.get(*key))
                    })
                })
        }
        "minecraft:shot_crossbow" | "minecraft:started_riding" => known_conditions(&[]),
        // 只用事件里的一个 id / 方块 / 物品名字；`location` 那种位置谓词没建模。
        "minecraft:slept_in_bed"
        | "minecraft:effects_changed"
        | "minecraft:recipe_crafted"
        | "minecraft:using_item"
        | "minecraft:recipe_unlocked"
        | "minecraft:placed_block"
        | "minecraft:item_used_on_block"
        | "minecraft:consume_item" => {
            known_conditions(&[
                "item", "recipe", "recipes", "block", "blocks", "id", "location", "biome",
            ]) && matches_item_or_id(conditions, payload)
        }
        "minecraft:inventory_changed" => oracle_inventory_changed(conditions, payload),
        "minecraft:location" => location_matches(conditions, payload),
        "minecraft:player_killed_entity"
        | "minecraft:player_hurt_entity"
        | "minecraft:player_interacted_with_entity" => {
            known_conditions(&["entity", "victims", "source", "damage"])
                && matches_entity(conditions, payload)
        }
        // 交易：`TradeTrigger` 判据带结果物品谓词和村民实体谓词。
        "minecraft:villager_trade" => {
            known_conditions(&["item", "villager"])
                && fields
                    .get("item")
                    .is_none_or(|item| item_predicate_matches(item, payload.get("item")))
                && fields.get("villager").is_none_or(|villager| {
                    entity_predicate_matches(villager, payload.get("villager"))
                })
        }
        // 下面这一组是本期新接的入口。
        "minecraft:enchanted_item" => {
            known_conditions(&["item", "levels"]) && matches_item_field(conditions, payload)
        }
        "minecraft:item_durability_changed" => {
            known_conditions(&["item", "durability", "delta"])
                && matches_item_field(conditions, payload)
        }
        "minecraft:filled_bucket" | "minecraft:crafter_recipe_crafted" => {
            known_conditions(&["item", "recipes"]) && matches_item_field(conditions, payload)
        }
        "minecraft:enter_block" => {
            known_conditions(&["block", "blocks", "state"])
                && !fields.contains_key("state")
                && matches_id(conditions, payload)
        }
        "minecraft:tame_animal" | "minecraft:summoned_entity" => {
            known_conditions(&["entity"]) && entity_field_matches(conditions, payload, "entity")
        }
        "minecraft:bred_animals" => {
            known_conditions(&["child", "parent", "partner"])
                && ["child", "parent", "partner"].iter().all(|key| {
                    fields.get(*key).is_none_or(|expected| {
                        entity_predicate_matches(expected, payload.get(*key))
                    })
                })
        }
        "minecraft:entity_killed_player" => {
            known_conditions(&["entity", "killing_blow"])
                && fields.get("killing_blow").is_none()
                && entity_field_matches(conditions, payload, "entity")
        }
        "minecraft:entity_hurt_player" => {
            known_conditions(&["damage"]) && damage_field_matches(conditions, payload)
        }
        "minecraft:killed_by_arrow" => {
            known_conditions(&["fired_from_weapon", "victims", "unique_entity_types"])
                && fields.get("victims").is_none_or(|victims| {
                    victims.as_array().is_some_and(|expected| {
                        let actual = payload.get("victims").and_then(Value::as_array);
                        actual.is_some_and(|actual| {
                            actual.len() >= expected.len()
                                && expected.iter().enumerate().all(|(index, value)| {
                                    entity_predicate_matches(value, actual.get(index))
                                })
                        })
                    })
                })
                && fields.get("fired_from_weapon").is_none_or(|weapon| {
                    item_predicate_matches(weapon, payload.get("fired_from_weapon"))
                })
                && min_max_matches(
                    fields.get("unique_entity_types"),
                    payload.get("unique_entity_types"),
                )
        }
        "minecraft:fall_from_height" => {
            known_conditions(&["start_position", "distance"])
                && fields.get("start_position").is_none_or(|start| {
                    let Some(position) = start.get("position") else {
                        return false;
                    };
                    bound_matches(
                        position.get("y"),
                        payload.get("start_y").and_then(Value::as_f64),
                    )
                })
                && fields.get("distance").is_none_or(|distance| {
                    bound_matches(
                        distance.get("y"),
                        payload
                            .get("distance")
                            .and_then(|d| d.get("y"))
                            .and_then(Value::as_f64),
                    )
                })
        }
        "minecraft:thrown_item_picked_up_by_player" => {
            known_conditions(&["item", "entity"])
                && matches_item_field(conditions, payload)
                && entity_field_matches(conditions, payload, "entity")
        }
        // `FishingRodHookedTrigger`：`rod`、`entity`（被钩实体或浮漂）与 `item`（钓上物品或被钩掉落物）。
        "minecraft:fishing_rod_hooked" => {
            known_conditions(&["rod", "entity", "item"])
                && fields
                    .get("rod")
                    .is_none_or(|rod| item_predicate_matches(rod, payload.get("rod")))
                && fields
                    .get("entity")
                    .is_none_or(|entity| entity_predicate_matches(entity, payload.get("entity")))
                && fields.get("item").is_none_or(|expected| {
                    let hooked_match = payload
                        .get("hooked_item")
                        .is_some_and(|actual| item_predicate_matches(expected, Some(actual)));
                    let caught_match =
                        payload
                            .get("items")
                            .and_then(Value::as_array)
                            .is_some_and(|items| {
                                items
                                    .iter()
                                    .any(|actual| item_predicate_matches(expected, Some(actual)))
                            });
                    hooked_match || caught_match
                })
        }
        // `UsedEnderEyeTrigger`：`distance` 是 `MinMaxBounds.Doubles`，按水平距离平方判定。
        "minecraft:used_ender_eye" => {
            known_conditions(&["distance"])
                && bound_sqr_matches(
                    fields.get("distance"),
                    payload.get("distance_sqr").and_then(Value::as_f64),
                )
        }
        _ => object.is_some_and(|map| map.is_empty()),
    }
}

/// 官方 `Criterion.CODEC` 实例的判定对拍入口。事件和条件都是 JSON 文本。
#[cfg(any(test, feature = "oracle-samples"))]
pub fn matches_trigger(trigger: &str, event: &str, conditions: &str) -> bool {
    let payload = event.split_once('|').map(|(_, json)| json).unwrap_or("{}");
    let payload: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    let conditions: Value = serde_json::from_str(conditions).unwrap_or(Value::Null);
    matches(trigger, &payload, &conditions)
}

/// 旧题目的 `matches_id`，另外支持 `{"item":{"items":"..."}}` 这种 ItemPredicate。
fn matches_item_or_id(conditions: &Value, payload: &Value) -> bool {
    let keys = [
        "item", "recipe", "recipes", "block", "blocks", "id", "location", "biome",
    ];
    keys.iter().all(|&key| {
        let Some(expected) = conditions.get(key) else {
            return true;
        };
        let Some(actual) = payload.get(key) else {
            return false;
        };
        expected == actual || item_predicate_matches(expected, Some(actual))
    })
}

/// JSON 里的 `id` / `item` / `recipe` / `block` 字段和事件同名时算匹配。
fn matches_id(conditions: &Value, payload: &Value) -> bool {
    let keys = [
        "item", "recipe", "recipes", "block", "blocks", "id", "location", "biome",
    ];
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

/// `item` / `recipes` 字段：条件里的物品 / 配方 HolderSet 和事件里的名字。
fn matches_item_field(conditions: &Value, payload: &Value) -> bool {
    for key in ["item", "recipes"] {
        let Some(expected) = conditions.get(key) else {
            continue;
        };
        let Some(actual) = payload.get(key) else {
            return false;
        };
        if !item_predicate_matches(expected, Some(actual)) {
            return false;
        }
    }
    true
}

/// 对拍的事件输入只有一件物品，照官方 `InventoryChangeTrigger.matches` 的
/// 单谓词快速路径判定：每条 `ItemPredicate` 都要命中这一堆。
fn oracle_inventory_changed(conditions: &Value, payload: &Value) -> bool {
    if fields_contain(conditions, "slots") {
        return false;
    }
    let Some(event_item) = payload.get("items").and_then(|items| items.get(0)) else {
        return false;
    };
    let Some(predicates) = conditions.get("items").and_then(Value::as_array) else {
        return true;
    };
    predicates
        .iter()
        .all(|predicate| item_predicate_matches(predicate, Some(event_item)))
}

fn fields_contain(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|map| map.contains_key(key))
}

/// `MinMaxBounds.Doubles` 的 `{"min": ...}` / `{"max": ...}`。
fn bound_matches(expected: Option<&Value>, actual: Option<f64>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual) = actual else {
        return false;
    };
    let min_ok = expected
        .get("min")
        .and_then(Value::as_f64)
        .is_none_or(|min| actual >= min);
    let max_ok = expected
        .get("max")
        .and_then(Value::as_f64)
        .is_none_or(|max| actual <= max);
    min_ok && max_ok
}

/// `MinMaxBounds.Doubles.matchesSqr`：比较 `distance_sqr` 与 `min^2` / `max^2`。
///
/// `MinMaxBounds.Doubles.matchesSqr`: compare `distance_sqr` against `min^2` and `max^2`.
fn bound_sqr_matches(expected: Option<&Value>, actual_sqr: Option<f64>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual_sqr) = actual_sqr else {
        return false;
    };
    if let Some(exact) = expected.as_f64() {
        return actual_sqr == exact * exact;
    }
    let min_ok = expected
        .get("min")
        .and_then(Value::as_f64)
        .is_none_or(|min| actual_sqr >= min * min);
    let max_ok = expected
        .get("max")
        .and_then(Value::as_f64)
        .is_none_or(|max| actual_sqr <= max * max);
    min_ok && max_ok
}

/// 一个 `ItemPredicate`（或单个名字、名字数组）和事件里的物品名字比。
fn item_predicate_matches(expected: &Value, actual: Option<&Value>) -> bool {
    let Some(actual) = actual.and_then(Value::as_str) else {
        return false;
    };
    let names: Vec<&str> = match expected {
        Value::String(name) => vec![name.as_str()],
        Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
        Value::Object(_) => match expected.get("items") {
            Some(Value::String(name)) => vec![name.as_str()],
            Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    names.iter().any(|name| {
        *name == actual
            || name.strip_prefix('#').is_some_and(|tag| {
                mc_bamboo_data::item_tag(tag).is_some_and(|ids| {
                    ids.iter().any(|&id| {
                        mc_bamboo_data::items::ITEMS
                            .get(id as usize)
                            .is_some_and(|item| item.name == actual)
                    })
                })
            })
    })
}

/// `MinMaxBounds.Ints`：`{"min": n}` / `{"max": n}` / 一个整数 / 缺省。
fn min_max_matches(expected: Option<&Value>, actual: Option<&Value>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual) = actual.and_then(Value::as_i64) else {
        return false;
    };
    if let Some(exact) = expected.as_i64() {
        return exact == actual;
    }
    let min_ok = expected
        .get("min")
        .and_then(Value::as_i64)
        .is_none_or(|min| actual >= min);
    let max_ok = expected
        .get("max")
        .and_then(Value::as_i64)
        .is_none_or(|max| actual <= max);
    min_ok && max_ok
}

/// 官方 `LocationTrigger`：条件里的 `position` / `biome` / `dimension` 都要对得上。
fn location_matches(conditions: &Value, payload: &Value) -> bool {
    // `location` / `position` / `smokey` / `light` 等本层没建模：出现就不算匹配。
    let known = [
        "biome",
        "dimension",
        "feature",
        "position",
        "x",
        "y",
        "z",
        "player",
    ];
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

/// 26.3 的实体谓词字段：`{"type":"minecraft:entity_properties","predicate":{...}}`。
/// 只实现了 `minecraft:entity_type`；事件里 `{"type":"minecraft:cow"}` 直接给出类型名。
fn entity_predicate_matches(expected: &Value, actual: Option<&Value>) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    let actual_type = actual
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| actual.as_str())
        .unwrap_or("");
    let Some(predicate) = expected.get("predicate") else {
        // 事件里没给实体、条件也没写子谓词时只要求事件带着这个实体。
        return !actual_type.is_empty();
    };
    if let Some(wanted) = predicate.get("minecraft:entity_type") {
        let wanted = wanted.as_str().unwrap_or("");
        return wanted == actual_type;
    }
    // 没有建模的子谓词（花色、位置、骑乘等）一律不匹配。
    predicate.as_object().is_some_and(|map| map.is_empty())
}

/// 条件里的 `entity` 是实体谓词，事件里同名字段给出被比较的实体。
fn entity_field_matches(conditions: &Value, payload: &Value, field: &str) -> bool {
    let Some(expected) = conditions.get(field) else {
        return true;
    };
    entity_predicate_matches(expected, payload.get(field))
}

/// `DamagePredicate`：本期只认 `blocked` 和 `type.tags[].expected/id`。
fn damage_field_matches(conditions: &Value, payload: &Value) -> bool {
    let Some(damage) = conditions.get("damage") else {
        return true;
    };
    let Some(event) = payload.get("damage") else {
        return false;
    };
    if let Some(blocked) = damage.get("blocked")
        && event.get("blocked") != Some(blocked)
    {
        return false;
    }
    if let Some(kind) = damage.get("type") {
        let tags = kind
            .get("tags")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for tag in tags {
            let wanted = tag.get("expected").and_then(Value::as_bool).unwrap_or(true);
            let id = tag.get("id").and_then(Value::as_str).unwrap_or("");
            let present = id == "#minecraft:is_projectile"
                && event.get("projectile").and_then(Value::as_bool) == Some(true);
            if present != wanted {
                return false;
            }
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
    // 从 `ChunkStore::biome_at` 查群系注册表 ID，再映射到完整名字。
    let biome = region
        .chunks
        .biome_at(mc_bamboo_core::BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        ))
        .and_then(|id| mc_bamboo_data::BIOME_CLIMATES.get(id as usize))
        .map(|climate| climate.name)
        .unwrap_or("minecraft:plains");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_triggers_are_unique_and_known() {
        let tree = AdvancementTree::global();
        let set: std::collections::BTreeSet<&str> = CONNECTED_TRIGGERS.iter().copied().collect();
        assert_eq!(set.len(), CONNECTED_TRIGGERS.len());
        assert_eq!(CONNECTED_TRIGGERS.len(), 35);
        for trigger in CONNECTED_TRIGGERS {
            assert!(tree.triggers.contains(trigger), "{trigger} 不在注册表里");
        }
        assert!(!CONNECTED_TRIGGERS.contains(&"minecraft:used_totem"));
        assert!(!CONNECTED_TRIGGERS.contains(&"minecraft:target_hit"));
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

    #[test]
    fn entity_predicates_compare_the_official_entity_type_name() {
        let conditions = serde_json::json!({
            "entity": {"type": "minecraft:entity_properties", "entity": "this",
                       "predicate": {"minecraft:entity_type": "minecraft:cow"}}
        });
        assert!(matches(
            "minecraft:tame_animal",
            &serde_json::json!({"entity": {"type": "minecraft:cow"}}),
            &conditions,
        ));
        assert!(!matches(
            "minecraft:tame_animal",
            &serde_json::json!({"entity": {"type": "minecraft:wolf"}}),
            &conditions,
        ));
    }

    #[test]
    fn used_ender_eye_matches_distance_squared_bounds() {
        let conditions = serde_json::json!({"distance": {"min": 10.0, "max": 20.0}});
        assert!(matches(
            "minecraft:used_ender_eye",
            &serde_json::json!({"distance_sqr": 100.0}),
            &conditions,
        ));
        assert!(matches(
            "minecraft:used_ender_eye",
            &serde_json::json!({"distance_sqr": 400.0}),
            &conditions,
        ));
        assert!(!matches(
            "minecraft:used_ender_eye",
            &serde_json::json!({"distance_sqr": 99.9}),
            &conditions,
        ));
        assert!(!matches(
            "minecraft:used_ender_eye",
            &serde_json::json!({"distance_sqr": 400.1}),
            &conditions,
        ));
    }
}
