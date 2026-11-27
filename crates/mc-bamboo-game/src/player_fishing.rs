//! 玩家抛竿和收竿：官方 `FishingRodItem.use` 与 `FishingHook.retrieve`。
//!
//! 抛竿时播放 `entity.fishing_bobber.throw`、读取钓竿上的 `lure`（每级 -5 秒 = 100 刻）
//! 与 `luck_of_the_sea`（每级 +1 幸运）、生成 `FishingHook` 并将 `player.fishing` 指向它、
//! 记一次 `ITEM_USED` 统计、发 `ITEM_INTERACT_START`。收竿时若钩住实体则拉拽并广播
//! 31 号实体事件（掉落物扣 3 耐久，其余实体扣 5 耐久）；若正在咬钩（`nibble > 0`）则按
//! `minecraft:gameplay/fishing` 战利品表结算
//! （`ORIGIN` / `TOOL` / `THIS_ENTITY` / `luck + player.getLuck()`），生成朝玩家飞去的
//! 掉落物、在 `(x, y + 0.5, z + 0.5)` 生成 `nextInt(6) + 1` 经验球、对 `#minecraft:fishes`
//! 记 `FISH_CAUGHT` 统计并触发 `minecraft:fishing_rod_hooked` 进度；若浮漂落地则耐久改为 2，
//! 最后发 `ITEM_INTERACT_FINISH`。两个游戏事件都照 `ItemStack.causeUseVibration` 受
//! `use_effects.interact_vibrations` 控制。
//!
//! Player casting and retrieving, matching vanilla 26.3 `FishingRodItem.use` and
//! `FishingHook.retrieve`.

use mc_bamboo_blocks::{
    BlockState, World,
    entity::{
        Entity, EntityKind, KindPublic,
        fishing_hook::{FishingHookCast, caught_item_velocity, should_stop_fishing},
    },
    game_event::{GameEvent, dispatch::EventContext},
};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::{
    items,
    registries::{SYNCED_REGISTRIES, attribute, custom_stat, data_component_type, entity_type},
    sounds,
};
use mc_bamboo_loot::{EntityView, LevelView, LootParams, LootStack, LootTable, Position};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
    packet::play::GameMode,
};
use mc_bamboo_world::store::ChunkStore;
use tracing::debug;

use crate::{
    inbound::PlayerId,
    player_weapon::{level_random_float, player_position, player_rotation},
    region::Region,
};

/// 手里是钓鱼竿时执行抛竿或收竿并返回真；否则什么都不做。
///
/// Casts or retrieves the bobber when holding a fishing rod; returns false otherwise.
pub(crate) fn use_rod(region: &mut Region, id: PlayerId, hand: Hand) -> bool {
    let Some(stack) = region
        .players
        .get(&id)
        .map(|player| player.inventory.held(hand).clone())
    else {
        return false;
    };
    if stack.item != items::FISHING_ROD {
        return false;
    }

    // 若之前记录的浮漂已经消失，先把悬空引用清掉再判断。
    // Clear any stale reference if the previously cast hook is already gone.
    prune_player_hook(region, id);

    let existing = region.players.get(&id).and_then(|player| player.fishing);
    if let Some(hook_uid) = existing {
        let dmg = retrieve_hook(region, id, hook_uid, &stack);
        if dmg > 0 {
            crate::durability::run(region, id, hand, |region| {
                let (mut level, ticks) = region.block_level();
                let world = World::new(&mut level, ticks);
                mc_bamboo_blocks::durability::hurt_hand(&world, hand, dmg);
            });
        }
        let pitch = 0.4 / (level_random_float(region) * 0.4 + 0.8);
        if let Some(pos) = player_position(region, id) {
            play_neutral_sound(
                region,
                pos,
                "minecraft:entity.fishing_bobber.retrieve",
                1.0,
                pitch,
            );
        }
        cause_use_vibration(region, id, &stack, GameEvent::ItemInteractFinish);
    } else {
        let pitch = 0.4 / (level_random_float(region) * 0.4 + 0.8);
        if let Some(pos) = player_position(region, id) {
            play_neutral_sound(
                region,
                pos,
                "minecraft:entity.fishing_bobber.throw",
                0.5,
                pitch,
            );
        }
        let Some((owner_uid, owner_uuid, owner_pos, owner_eye_y)) =
            region.players.get(&id).map(|player| {
                let pos = player.position();
                (
                    player.id.0,
                    *player.uuid.as_bytes(),
                    pos,
                    pos.y + f64::from(player.eye_height() as f32),
                )
            })
        else {
            return false;
        };
        let (owner_yaw, owner_pitch) = player_rotation(region, id);
        let lure_speed = fishing_lure_ticks(&stack);
        let luck = fishing_luck_bonus(&stack);
        let cast = FishingHookCast {
            owner: owner_uid,
            owner_uuid,
            owner_position: owner_pos,
            owner_eye_y,
            owner_yaw,
            owner_pitch,
            luck,
            lure_speed,
        };
        let hook_uid = {
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            world.spawn_fishing_hook_cast(&cast)
        };
        if let Some(player) = region.players.get_mut(&id) {
            player.fishing = Some(hook_uid);
        }
        crate::item_use_stats::award(region, id, stack.item);
        cause_use_vibration(region, id, &stack, GameEvent::ItemInteractStart);
    }
    true
}

/// `ItemStack.causeUseVibration(player, event)`：只有 `use_effects` 组件存在且
/// `interact_vibrations` 为真时才发事件。物品的 `use_effects` 缺省来自官方
/// `COMMON_ITEM_COMPONENTS`（`UseEffects.DEFAULT`，`interact_vibrations = true`），
/// 被物品组件补丁移除或改假的都不发。
///
/// `ItemStack.causeUseVibration(player, event)`: emits only when the `use_effects`
/// component is present with `interact_vibrations = true`.
fn cause_use_vibration(region: &mut Region, id: PlayerId, stack: &ItemStack, event: GameEvent) {
    // 对应 `FishingRodItem.use` 里对 `stack.causeUseVibration` 的调用。
    #[cfg(test)]
    tests::observe_use_vibration(event, stack);
    let patch = stack.patch();
    let emitting = matches!(
        defaults::resolve(stack.item, &patch, data_component_type::USE_EFFECTS),
        Some(ComponentValue::UseEffects {
            interact_vibrations: true,
            ..
        })
    );
    if !emitting {
        return;
    }
    let Some((position, context)) = region.players.get(&id).map(|player| {
        (
            player.position(),
            EventContext {
                sneaking: player.sync.is_crouching(),
                spectator: player.game_mode == GameMode::Spectator,
                source_player: Some(id.0),
                source_entity: Some(id.0),
                ..Default::default()
            },
        )
    }) else {
        return;
    };
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        // `Entity.gameEvent(Holder)` 用实体精确坐标和 `Context.of(entity)`。
        world.game_event(event, position, context);
        world.into_effects()
    };
    crate::interaction::send_effects(region, Some(id), &effects);
}

/// 每刻清理已经消失的 `player.fishing` 引用。
///
/// Clears stale `player.fishing` references whose bobber entity was discarded.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        prune_player_hook(region, id);
    }
}

/// 玩家离线时移除其还在世界里的浮漂（`FishingHook.remove`）。
///
/// Removes the player's active bobber when they leave.
pub(crate) fn remove_on_leave(region: &mut Region, id: PlayerId) {
    let hook = region
        .players
        .get_mut(&id)
        .and_then(|player| player.fishing.take());
    if let Some(uid) = hook {
        region.entities.remove(uid);
    }
}

fn prune_player_hook(region: &mut Region, id: PlayerId) {
    let hook_uid = region.players.get(&id).and_then(|player| player.fishing);
    let Some(uid) = hook_uid else {
        return;
    };
    let alive = region
        .entities
        .get(uid)
        .is_some_and(|entity| !entity.removed && matches!(entity.kind, EntityKind::FishingHook(_)));
    if !alive && let Some(player) = region.players.get_mut(&id) {
        player.fishing = None;
    }
}

/// `FishingHook.retrieve(ItemStack)`：结算拉拽实体或战利品、经验球、统计与进度，返回耐久消耗。
fn retrieve_hook(region: &mut Region, id: PlayerId, hook_uid: i32, rod: &ItemStack) -> i32 {
    let Some(mut hook_entity) = region.entities.take(hook_uid) else {
        if let Some(player) = region.players.get_mut(&id) {
            player.fishing = None;
        }
        return 0;
    };
    if let Some(player) = region.players.get_mut(&id) {
        player.fishing = None;
    }

    let Some((owner_view, owner_pos, player_luck)) = region.players.get(&id).map(|player| {
        (
            player.view(id),
            player.position(),
            player.attributes.value(attribute::LUCK) as f32,
        )
    }) else {
        return 0;
    };

    if hook_entity.removed || should_stop_fishing(hook_entity.position, &owner_view) {
        return 0;
    }

    let hook_pos = hook_entity.position;
    let on_ground = hook_entity.on_ground;
    let (hooked_in, nibble, open_water, hook_luck) = match &hook_entity.kind {
        EntityKind::FishingHook(hook) => (hook.hooked_in, hook.nibble, hook.open_water, hook.luck),
        _ => return 0,
    };

    let rod_name = item_name(rod.item);
    let mut dmg = 0;

    if let Some(target_uid) = hooked_in {
        let (hooked_is_item, target_type, hooked_item_name) =
            describe_hooked_target(region, target_uid);
        {
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            world.pull_hooked_entity(hook_pos, owner_pos, target_uid);
        }
        // 触发 `fishing_rod_hooked` 进度（`entity = hookedIn`，`items = []`）。
        let mut payload = serde_json::json!({
            "rod": rod_name,
            "entity": { "type": target_type },
            "items": [],
        });
        if let Some(item) = hooked_item_name {
            payload["hooked_item"] = serde_json::Value::String(item.to_owned());
        }
        crate::advancement_triggers::record(region, id, "minecraft:fishing_rod_hooked", payload);
        // 广播 31 号实体事件（客户端用于对本地玩家执行 `pullEntity`）。
        if let EntityKind::FishingHook(hook) = &mut hook_entity.kind {
            hook.pending_pull_event = true;
        }
        region.entities.keep_departed_events(&hook_entity);
        dmg = if hooked_is_item { 3 } else { 5 };
    } else if nibble > 0 {
        let loot_stacks = evaluate_fishing_loot(
            region,
            hook_pos,
            rod,
            open_water,
            hook_luck as f32 + player_luck,
        );
        let caught: Vec<ItemStack> = loot_stacks
            .iter()
            .map(crate::loot_stacks::loot_stack)
            .collect();
        let caught_names: Vec<&str> = caught.iter().map(|s| item_name(s.item)).collect();
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:fishing_rod_hooked",
            serde_json::json!({
                "rod": rod_name,
                "entity": { "type": "minecraft:fishing_bobber" },
                "items": caught_names,
            }),
        );
        let item_vel = caught_item_velocity(owner_pos, hook_pos);
        let orb_pos = Vec3::new(owner_pos.x, owner_pos.y + 0.5, owner_pos.z + 0.5);
        let mut fish_caught_count = 0;
        {
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            for stack in caught {
                let is_fish = mc_bamboo_data::item_tag("minecraft:fishes")
                    .is_some_and(|ids| ids.contains(&stack.item));
                let mut item_entity = world.prepare_entity(Entity::ground_item(hook_pos, stack));
                item_entity.delta = item_vel;
                world.add_prepared_entity(item_entity);
                let xp = hook_entity.random.next_int_bounded(6) + 1;
                world.spawn_entity(Entity::experience_orb(orb_pos, xp));
                if is_fish {
                    fish_caught_count += 1;
                }
            }
        }
        if fish_caught_count > 0 {
            crate::item_use_stats::award_custom(
                region,
                id,
                custom_stat::FISH_CAUGHT,
                fish_caught_count,
            );
        }
        dmg = 1;
    }

    if on_ground {
        dmg = 2;
    }
    // 浮漂已从表里取出且不再放回，即完成 `discard()`。
    dmg
}

/// 评估官方 `minecraft:gameplay/fishing` 战利品表。
///
/// 照 26.3 字节码只传 `ORIGIN`、`TOOL`、`THIS_ENTITY`（浮漂，带 `in_open_water`）和 `luck`。
fn evaluate_fishing_loot(
    region: &Region,
    hook_pos: Vec3,
    rod: &ItemStack,
    open_water: bool,
    luck: f32,
) -> Vec<LootStack> {
    let Ok(table) = LootTable::from_name("minecraft:gameplay/fishing") else {
        return Vec::new();
    };
    #[cfg(test)]
    tests::observe_fishing_luck(luck);
    let origin = Position::new(hook_pos.x, hook_pos.y, hook_pos.z);
    let view = FishingLevelView {
        chunks: &region.chunks,
        global: &region.global,
    };
    let mut hook_view = EntityView::new(entity_type::FISHING_BOBBER);
    hook_view.is_fishing_hook = true;
    hook_view.in_open_water = open_water;
    hook_view.position = Some(origin);

    let mut params = LootParams::new(luck).with_origin(origin);
    params.level = Some(&view);
    params.tool = Some(item_stack_to_loot_stack(rod));
    params.this_entity = Some(hook_view);

    match table.get_random_items_with_seed(
        &params,
        0,
        Some(&region.loot_sequences),
        region.random.source(),
    ) {
        Ok(stacks) => stacks,
        Err(error) => {
            debug!(%error, "fishing loot evaluation failed");
            Vec::new()
        }
    }
}

fn item_stack_to_loot_stack(stack: &ItemStack) -> LootStack {
    let mut loot = LootStack::new(stack.item, stack.count);
    let patch = stack.patch();
    for (name, value) in crate::loot_stacks::component_json(&patch) {
        loot.set_component(&name, value);
    }
    loot
}

fn describe_hooked_target(
    region: &Region,
    target_uid: i32,
) -> (bool, &'static str, Option<&'static str>) {
    if region.players.contains_key(&PlayerId(target_uid)) {
        return (false, "minecraft:player", None);
    }
    if let Some(public) = region.entities.public_at(target_uid) {
        match &public.kind {
            KindPublic::Item { stack, .. } => {
                return (true, "minecraft:item", Some(item_name(stack.item)));
            }
            other => {
                let name = entity_type::name_of(other.entity_type()).unwrap_or("minecraft:pig");
                return (false, name, None);
            }
        }
    }
    (false, "minecraft:fishing_bobber", None)
}

fn item_name(item_id: i32) -> &'static str {
    items::ITEMS
        .get(item_id as usize)
        .map(|item| item.name)
        .unwrap_or("minecraft:air")
}

/// `EnchantmentHelper.getFishingTimeReduction(level, rod, player) * 20.0F` 取整。
///
/// `lure.json`：`fishing_time_reduction` 为线性 `5.0 * level` 秒，换算成刻即 `100 * level`。
pub(crate) fn fishing_lure_ticks(stack: &ItemStack) -> i32 {
    let level = enchantment_level(stack, "minecraft:lure");
    if level <= 0 {
        return 0;
    }
    let seconds = 5.0_f32 * level as f32;
    (seconds * 20.0_f32) as i32
}

/// `EnchantmentHelper.getFishingLuckBonus(level, rod, player)`。
///
/// `luck_of_the_sea`：`fishing_luck_bonus` 为线性 `1.0 * level`。
pub(crate) fn fishing_luck_bonus(stack: &ItemStack) -> i32 {
    enchantment_level(stack, "minecraft:luck_of_the_sea").max(0)
}

fn enchantment_level(stack: &ItemStack, name: &str) -> i32 {
    let Some(id) = enchantment_id(name) else {
        return 0;
    };
    let patch = stack.patch();
    let default = defaults::resolve(stack.item, &patch, data_component_type::ENCHANTMENTS);
    let Some(ComponentValue::Enchantments(levels)) =
        patch.get(data_component_type::ENCHANTMENTS).or(default)
    else {
        return 0;
    };
    levels
        .iter()
        .find_map(|&(entry_id, level)| (entry_id == id).then_some(level))
        .unwrap_or(0)
}

fn enchantment_id(name: &str) -> Option<i32> {
    SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")?
        .entries
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
}

fn play_neutral_sound(region: &mut Region, position: Vec3, sound: &str, volume: f32, pitch: f32) {
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.play_sound(
            position,
            sounds::sound_event(sound),
            sounds::NEUTRAL,
            volume,
            pitch,
            false,
        );
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

struct FishingLevelView<'a> {
    chunks: &'a ChunkStore,
    global: &'a crate::global::GlobalView,
}

impl LevelView for FishingLevelView<'_> {
    fn command_storage(&self, name: &str) -> Option<mc_bamboo_nbt::Tag> {
        Some(self.global.command_storage.get(name))
    }

    fn score(&self, holder: &str, objective: &str) -> Option<Option<i32>> {
        Some(self.global.scoreboard.score_value(holder, objective))
    }

    fn is_loaded(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        Some(self.chunks.block(BlockPos::new(x, y, z)).is_some())
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        self.chunks.block(BlockPos::new(x, y, z)).map(BlockState)
    }

    fn biome(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        self.chunks
            .biome_at(BlockPos::new(x, y, z))
            .map(|id| id as i32)
    }

    fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    fn is_raining(&self) -> Option<bool> {
        Some(self.global.block_weather.is_raining())
    }

    fn is_thundering(&self) -> Option<bool> {
        Some(self.global.block_weather.is_thundering())
    }

    fn day_time(&self) -> Option<i64> {
        None
    }

    fn game_time(&self) -> Option<i64> {
        None
    }

    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use mc_bamboo_blocks::{
        behaviour::misc::sculk_entity::{SculkSensorEntity, create_block_entity},
        entity::{
            Entity, EntityKind,
            fishing_hook::FishHookState,
            mob::{MobType, effects::EffectInstance},
        },
    };
    use mc_bamboo_core::{BlockPos, Hand, Vec3};
    use mc_bamboo_data::{
        blocks, items,
        packets::play::clientbound,
        registries::{attribute, data_component_type, entity_type, mob_effect},
    };
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry},
        item::ItemStack,
        packet::play::{GameMode, PlayRequest},
    };

    use super::*;
    use crate::world::tests::Harness;

    // 收竿结算时实际传进战利品表的幸运值；测试用它确认 `getLuck()` 的链路。
    thread_local! {
        static FISHING_LUCK: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
        // `causeUseVibration` 的调用记录。`ITEM_INTERACT_START` 官方不在
        // `#minecraft:vibrations` 里，感测体不会记成候选，所以这里直接记调用。
        static USE_VIBRATIONS: RefCell<Vec<(GameEvent, ItemStack)>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn observe_fishing_luck(luck: f32) {
        FISHING_LUCK.with(|seen| seen.borrow_mut().push(luck));
    }

    fn take_fishing_luck() -> Vec<f32> {
        FISHING_LUCK.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
    }

    pub(super) fn observe_use_vibration(event: GameEvent, stack: &ItemStack) {
        USE_VIBRATIONS.with(|seen| seen.borrow_mut().push((event, stack.clone())));
    }

    fn take_use_vibrations() -> Vec<(GameEvent, ItemStack)> {
        USE_VIBRATIONS.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
    }

    /// `ItemStack.causeUseVibration` 的门控：组件不存在或 `interact_vibrations = false`
    /// 时不发事件。
    fn emits_use_vibration(stack: &ItemStack) -> bool {
        matches!(
            defaults::resolve(stack.item, &stack.patch(), data_component_type::USE_EFFECTS),
            Some(ComponentValue::UseEffects {
                interact_vibrations: true,
                ..
            })
        )
    }

    fn enchanted_rod(enchants: &[(&str, i32)]) -> ItemStack {
        let mut stack = ItemStack::new(items::FISHING_ROD, 1);
        let entries: Vec<(i32, i32)> = enchants
            .iter()
            .map(|&(name, level)| (enchantment_id(name).expect(name), level))
            .collect();
        let mut patch = ComponentPatch::new();
        patch.put(
            data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(entries)),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        stack
    }

    #[test]
    fn lure_and_luck_enchantments_match_vanilla_values() {
        let plain = ItemStack::new(items::FISHING_ROD, 1);
        assert_eq!(fishing_lure_ticks(&plain), 0);
        assert_eq!(fishing_luck_bonus(&plain), 0);

        let rod = enchanted_rod(&[("minecraft:lure", 3), ("minecraft:luck_of_the_sea", 2)]);
        assert_eq!(fishing_lure_ticks(&rod), 300);
        assert_eq!(fishing_luck_bonus(&rod), 2);
    }

    #[test]
    fn casting_and_retrieving_spawns_and_removes_bobber_and_awards_stat() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("angler");
        {
            let player = server.region().players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack::new(items::FISHING_ROD, 1);
            player.game_mode = GameMode::Survival;
        }
        client.take_frames();

        // 抛竿。
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();
        let hook_uid = server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .fishing
            .expect("bobber should be cast");
        let hook_pub = server
            .region()
            .entities
            .public_at(hook_uid)
            .expect("bobber in entity store");
        assert_eq!(hook_pub.kind.entity_type(), entity_type::FISHING_BOBBER);

        // 切换手持物品后下一刻浮漂自动消失（shouldStopFishing）。
        {
            let player = server.region().players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack::new(items::STONE, 1);
        }
        server.tick();
        assert!(server.region().entities.get(hook_uid).is_none());
        assert!(
            server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .fishing
                .is_none()
        );
    }

    #[test]
    fn retrieving_with_biting_nibble_spawns_loot_xp_and_damages_rod() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("angler");
        let pos = server.region().players.get(&client.id).unwrap().position();
        let center = BlockPos::containing(pos);
        // 建一片 5×4×5 露天水池。
        for dy in -1..=0 {
            for dz in -2..=2 {
                for dx in -2..=2 {
                    server.region().chunks.set_block(
                        BlockPos::new(center.x + dx, center.y + dy, center.z + dz),
                        blocks::WATER,
                    );
                }
            }
        }
        let slot = {
            let player = server.region().players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack::new(items::FISHING_ROD, 1);
            player.game_mode = GameMode::Survival;
            slot
        };

        // 抛竿。
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();
        let hook_uid = server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .fishing
            .expect("bobber cast");

        // 直接将浮漂置为水面咬钩态（nibble = 20），验证收竿结算完整链路。
        {
            let mut hook = server.region().entities.take(hook_uid).unwrap();
            hook.position = Vec3::new(pos.x, pos.y + 0.5, pos.z + 2.0);
            hook.on_ground = false;
            if let EntityKind::FishingHook(state) = &mut hook.kind {
                state.state = FishHookState::Bobbing;
                state.nibble = 20;
                state.biting = true;
                state.open_water = true;
            }
            server.region().entities.put(hook);
        }
        client.take_frames();

        // 收竿。
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 2,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();

        assert!(
            server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .fishing
                .is_none()
        );
        let rod = &server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .inventory
            .slots[slot];
        assert_eq!(mc_bamboo_blocks::durability::damage_of(rod), Some(1));

        // 应同时生成掉落物与经验球（经验球生在玩家身旁，同刻可能已被玩家直接捡起）。
        let mut saw_item = false;
        let mut xp_value = 0;
        for uid in server.region().entities.ids() {
            if let Some(pub_e) = server.region().entities.public_at(uid) {
                match pub_e.kind {
                    KindPublic::Item { .. } => saw_item = true,
                    KindPublic::ExperienceOrb { value } => xp_value += value,
                    _ => {}
                }
            }
        }
        let player_xp = server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .experience
            .total;
        assert!(
            saw_item,
            "retrieve with nibble > 0 must spawn a caught item"
        );
        assert!(
            (1..=6).contains(&(xp_value + player_xp)),
            "retrieve with nibble > 0 must spawn an experience orb (1..=6), got orb={xp_value} player_xp={player_xp}"
        );
    }

    #[test]
    fn retrieving_hooked_cow_pulls_it_broadcasts_event_31_and_costs_5_durability() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("angler");
        let pos = server.region().players.get(&client.id).unwrap().position();
        let cow_uid = server.region().entities.spawn(Entity::by_mob_type(
            MobType::Cow,
            Vec3::new(pos.x + 5.0, pos.y, pos.z),
        ));
        let slot = {
            let player = server.region().players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack::new(items::FISHING_ROD, 1);
            player.game_mode = GameMode::Survival;
            slot
        };

        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();
        let hook_uid = server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .fishing
            .expect("bobber cast");
        {
            let mut hook = server.region().entities.take(hook_uid).unwrap();
            hook.position = Vec3::new(pos.x + 5.0, pos.y + 1.0, pos.z);
            hook.on_ground = false;
            if let EntityKind::FishingHook(state) = &mut hook.kind {
                state.state = FishHookState::HookedInEntity;
                state.hooked_in = Some(cow_uid);
            }
            server.region().entities.put(hook);
        }
        client.take_frames();

        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 2,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        server.tick();

        let rod = &server
            .region()
            .players
            .get(&client.id)
            .unwrap()
            .inventory
            .slots[slot];
        assert_eq!(mc_bamboo_blocks::durability::damage_of(rod), Some(5));
        let cow = server.region().entities.get(cow_uid).expect("cow alive");
        assert!(
            cow.delta.x < -0.1,
            "cow should be pulled toward player, got {:?}",
            cow.delta
        );
        let frames = client.take_frames();
        assert!(
            frames.iter().any(|frame| {
                frame.id == clientbound::ENTITY_EVENT
                    && frame.body.len() == 5
                    && i32::from_be_bytes(frame.body[0..4].try_into().unwrap()) == hook_uid
                    && frame.body[4] == 31
            }),
            "retrieve hooked entity must broadcast entity event 31"
        );
    }

    #[test]
    fn luck_potion_effect_raises_the_luck_passed_to_the_fishing_loot_table() {
        // 两位玩家用同一根钓竿、同样的浮漂状态和同一条战利品表随机序列结算：
        // 有幸运药水 II 的那位 `getLuck()` 应该是 3，没有的是 0。
        fn retrieve_with_luck(effect: Option<EffectInstance>) -> f32 {
            let mut server = Harness::new(2);
            let client = server.join_and_settle("angler");
            let pos = server.region().players.get(&client.id).unwrap().position();
            let slot = {
                let player = server.region().players.get_mut(&client.id).unwrap();
                let slot = player.inventory.hand_slot(Hand::Main);
                player.inventory.slots[slot] = ItemStack::new(items::FISHING_ROD, 1);
                player.game_mode = GameMode::Survival;
                if let Some(effect) = effect {
                    player.effects.add(effect);
                    // 官方 `onEffectAdded` 会刷新属性；Bamboo 每刻重建前也要能读到。
                    crate::player_attributes::rebuild_player(player, false);
                }
                slot
            };
            let _ = slot;
            server.request(
                &client,
                PlayRequest::UseItem {
                    hand: Hand::Main,
                    sequence: 1,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            );
            server.tick();
            let hook_uid = server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .fishing
                .expect("bobber cast");
            {
                let mut hook = server.region().entities.take(hook_uid).unwrap();
                hook.position = Vec3::new(pos.x, pos.y + 0.5, pos.z + 1.0);
                hook.on_ground = false;
                if let EntityKind::FishingHook(state) = &mut hook.kind {
                    state.state = FishHookState::Bobbing;
                    state.nibble = 20;
                    state.biting = true;
                    state.open_water = true;
                    state.luck = 2;
                }
                server.region().entities.put(hook);
            }
            server.request(
                &client,
                PlayRequest::UseItem {
                    hand: Hand::Main,
                    sequence: 2,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            );
            server.tick();
            let seen = take_fishing_luck();
            assert_eq!(seen.len(), 1, "结算必须走一次战利品表求值");
            seen[0]
        }

        let plain = retrieve_with_luck(None);
        let lucky = retrieve_with_luck(Some(EffectInstance::with_amplifier(
            mob_effect::LUCK,
            600,
            2,
        )));
        assert_eq!(plain, 2.0, "无效果时只有 luck_of_the_sea 类的浮漂幸运值");
        assert_eq!(
            lucky, 5.0,
            "幸运药水 II 给 `generic.luck` +3，结算时加到浮漂幸运值上"
        );

        // 属性表本身也照官方 `MobEffect.addAttributeModifiers` 挂了修饰符。
        let mut player = crate::player::Player::new(
            crate::inbound::PlayerId(1),
            uuid::Uuid::nil(),
            "Lucky".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::default(),
        );
        player
            .effects
            .add(EffectInstance::with_amplifier(mob_effect::LUCK, 600, 2));
        crate::player_attributes::rebuild_player(&mut player, false);
        assert_eq!(player.attributes.value(attribute::LUCK), 3.0);
        assert_eq!(player.attributes.value(attribute::LUCK) as f32, 3.0);
    }

    /// 在 `pos` 放一个幽匿感测体，按 `World::set_block_entity` 登记监听者。
    fn place_sensor(region: &mut Region, pos: BlockPos) {
        region.chunks.set_block(pos, blocks::SCULK_SENSOR);
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.set_block_entity(create_block_entity(pos, false));
    }

    /// 感测体当前记下的振动候选（事件、源坐标、候选刻）。
    fn sensor_candidate(region: &mut Region, pos: BlockPos) -> Option<(GameEvent, Vec3, i64)> {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        world
            .block_entity::<SculkSensorEntity>(pos)
            .and_then(SculkSensorEntity::candidate)
    }

    #[test]
    fn casting_and_retrieving_emit_the_two_item_interact_vibrations() {
        // 直接调 `use_rod`，不跑方块实体刻，才能观察到 FINISH 记下的候选。
        let mut server = Harness::new(2);
        let client = server.join_and_settle("angler");
        let region = server.player_region(&client);
        let pos = region.players.get(&client.id).unwrap().position();
        let sensor = BlockPos::containing(pos);
        place_sensor(region, sensor);
        {
            let player = region.players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack::new(items::FISHING_ROD, 1);
            player.game_mode = GameMode::Survival;
        }
        let _ = take_use_vibrations();

        // 抛竿：START。官方 `#minecraft:vibrations` 标签不含 START，感测体只收到
        // post、不会记候选；这里比对调用记录。
        assert!(use_rod(region, client.id, Hand::Main));
        assert_eq!(
            take_use_vibrations()
                .iter()
                .map(|(event, _)| *event)
                .collect::<Vec<_>>(),
            [GameEvent::ItemInteractStart]
        );
        assert!(
            sensor_candidate(region, sensor).is_none(),
            "官方 `ITEM_INTERACT_START` 不在 `#vibrations` 里，感测体不记候选"
        );

        // 收竿：FINISH 在 `#vibrations` 里，感测体应当记下候选，
        // 源坐标是玩家的精确位置（感测体放在玩家脚下）。
        assert!(use_rod(region, client.id, Hand::Main));
        assert_eq!(
            take_use_vibrations()
                .iter()
                .map(|(event, _)| *event)
                .collect::<Vec<_>>(),
            [GameEvent::ItemInteractFinish]
        );
        let (event, source, _) = sensor_candidate(region, sensor).expect("收竿振动");
        assert_eq!(event, GameEvent::ItemInteractFinish);
        assert_eq!(source, pos, "源坐标是玩家的精确位置");
    }

    #[test]
    fn use_effects_component_gates_the_item_interact_vibrations() {
        let mut rod = ItemStack::new(items::FISHING_ROD, 1);
        assert!(
            emits_use_vibration(&rod),
            "缺省 `use_effects` 的 interact_vibrations 为真"
        );

        let mut disabled = ComponentPatch::new();
        disabled.put(
            data_component_type::USE_EFFECTS,
            PatchEntry::Set(ComponentValue::UseEffects {
                can_sprint: true,
                interact_vibrations: false,
                speed_multiplier: 1.0,
            }),
        );
        let (added, removed) = disabled.to_opaque();
        rod.added = added;
        rod.removed = removed;
        assert!(!emits_use_vibration(&rod));

        let mut removed = ComponentPatch::new();
        removed.put(data_component_type::USE_EFFECTS, PatchEntry::Removed);
        let (added, gone) = removed.to_opaque();
        rod.added = added;
        rod.removed = gone;
        assert!(
            !emits_use_vibration(&rod),
            "组件被移除时 `causeUseVibration` 直接返回"
        );

        // 真正抛竿一次：关掉 `interact_vibrations` 的钓竿不产生任何游戏事件。
        let mut server = Harness::new(2);
        let client = server.join_and_settle("angler");
        let region = server.player_region(&client);
        {
            let player = region.players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = rod;
            player.game_mode = GameMode::Survival;
        }
        let _ = take_use_vibrations();
        assert!(use_rod(region, client.id, Hand::Main));
        let seen = take_use_vibrations();
        assert_eq!(
            seen.len(),
            1,
            "`causeUseVibration` 被调用，但组件门控内部决定不 post"
        );
        assert_eq!(seen[0].0, GameEvent::ItemInteractStart);
        assert!(!emits_use_vibration(&seen[0].1));
    }

    #[test]
    fn fishing_rod_hooked_advancement_matches_caught_items_and_hooked_entity() {
        let cond_fish = serde_json::json!({
            "item": { "items": "#minecraft:fishes" }
        });
        assert!(crate::advancement_triggers::matches(
            "minecraft:fishing_rod_hooked",
            &serde_json::json!({
                "rod": "minecraft:fishing_rod",
                "entity": { "type": "minecraft:fishing_bobber" },
                "items": ["minecraft:cod"]
            }),
            &cond_fish,
        ));
        assert!(!crate::advancement_triggers::matches(
            "minecraft:fishing_rod_hooked",
            &serde_json::json!({
                "rod": "minecraft:fishing_rod",
                "entity": { "type": "minecraft:fishing_bobber" },
                "items": ["minecraft:stick"]
            }),
            &cond_fish,
        ));
        let cond_cow = serde_json::json!({
            "entity": {
                "type": "minecraft:entity_properties",
                "predicate": { "minecraft:entity_type": "minecraft:cow" }
            }
        });
        assert!(crate::advancement_triggers::matches(
            "minecraft:fishing_rod_hooked",
            &serde_json::json!({
                "rod": "minecraft:fishing_rod",
                "entity": { "type": "minecraft:cow" },
                "items": []
            }),
            &cond_cow,
        ));
    }
}
