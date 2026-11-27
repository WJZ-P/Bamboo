//! 玩家侧的 `location_changed` / `tick` 附魔效果接线。
//!
//! 官方 `LivingEntity.baseTick` 在 `Entity.commonTick` 推过 `tickCount` 之后跑
//! `EnchantmentHelper.tickEffects`，再在 `baseTick` 里比较 `blockPosition()` 和
//! `lastPos` 决定要不要 `runLocationChangedEffects`；换装备（`collectEquipmentChanges`）
//! 时先停掉旧效果再对当前装备立即重跑。玩家没有生物的单一 tick 函数，这一模块把
//! 这三处按顺序放进 [`tick`]，每刻由 `run_island_simulation` 在 `player_hurt::tick`
//! （也就是 `entity_ticks` 自增）之后调用。
//!
//! 玩家属性表每刻整体重建临时修饰符，所以灵魂疾行的两个 `attribute` 效果不直接
//! 加进 `Attributes`，而是把激活状态和等级留在 `Player` 上，由
//! `player_attributes::transient_modifiers` 读取。没有相关附魔时本模块不取随机数、
//! 不写方块、不改属性，进服 / 重生 / 属性对拍的行为不变。
//!
//! Player-side wiring of `location_changed` / `tick` enchantment effects.
//! Vanilla runs `tickEffects` after `Entity.commonTick` increments `tickCount`,
//! then compares `blockPosition()` with `lastPos` in `baseTick` before calling
//! `runLocationChangedEffects`; an equipment change stops old effects and
//! re-runs current ones immediately. Players have no single tick function, so
//! [`tick`] plays those three roles in that order each tick. Active soul-speed
//! modifiers live on `Player` because the attribute table rebuilds every tick.
//! With no relevant enchantments this module draws no RNG and changes nothing.

use mc_bamboo_blocks::{
    Aabb, BlockState, LegacyRandom,
    entity::mob::enchantment_effects::{self as fx, EffectId},
    world::{UPDATE_ALL, World},
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, registries::particle_type, tags::block_tags};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{EntityEvent, EquipmentSlot},
};

use crate::{
    inbound::PlayerId, links::encode_frames, player::Player, player_weapon::play_sound,
    region::Region,
};

/// `LivingEntity.entityEventForEquipmentBreak` 的靴子槽事件号。
const BREAK_FEET: u8 = 52;
/// `minecraft:particle.soul_escape` 的音量，官方 JSON 里是 0.6。
const SOUL_SOUND_VOLUME: f32 = 0.6;

/// 官方 `getBlockPosBelowThatAffectsMyMovement()`：`getOnPos(0.500001f)` 的脚下方块。
/// 玩家没有 `mainSupportingBlockPos` 那套支撑方块记录，用脚底下方一格的方块。
///
/// `getBlockPosBelowThatAffectsMyMovement()` = `getOnPos(0.500001f)`. Players do
/// not track a supporting block position, so use the cell below the feet.
fn movement_block(pos: Vec3) -> BlockPos {
    BlockPos::new(
        pos.x.floor() as i32,
        (pos.y - f64::from(0.500001_f32)).floor() as i32,
        pos.z.floor() as i32,
    )
}

/// 构造 `ENCHANTED_LOCATION` / `ENCHANTED_ENTITY` 上下文用的实体快照。
/// `horizontal_speed` 是官方 `MovementPredicate` 的输入：
/// `getKnownMovement().scale(20)` 的水平分量（玩家用客户端报的位移）。
///
/// The entity snapshot for the official loot contexts. `horizontal_speed` is
/// what `MovementPredicate` reads: `getKnownMovement().scale(20)` horizontally
/// (players use the last accepted client movement).
fn entity_state(player: &Player, chunks: &mc_bamboo_world::store::ChunkStore) -> fx::EntityState {
    let position = player.position();
    let known = player.motion.known_movement.scale(20.0);
    let below = movement_block(position);
    fx::EntityState {
        position,
        on_ground: player.on_ground,
        is_flying: player.sync.flying,
        has_vehicle: player.riding.vehicle.is_some(),
        horizontal_speed: (known.x * known.x + known.z * known.z).sqrt(),
        tick_count: player.entity_ticks,
        on_soul_speed_block: chunks
            .block(below)
            .is_some_and(|id| BlockState(id).is_tagged(block_tags::SOUL_SPEED_BLOCKS)),
    }
}

/// 靴子槽上的附魔，按官方 `runIterationOnItem` 的槽顺序整理。
fn equipped(player: &Player) -> Vec<(EquipmentSlot, &'static str, i32)> {
    let empty = ItemStack::empty();
    let stacks = [(
        EquipmentSlot::Feet,
        player
            .inventory
            .equipment(EquipmentSlot::Feet)
            .unwrap_or(&empty),
    )];
    fx::equipped_enchantments(&stacks)
}

/// 这一刻的玩家附魔效果：TICK 组件、装备变化、跨格检测。
///
/// One tick of player enchantment effects: TICK components, equipment changes,
/// then the block-position check.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        let enchants = equipped(player);
        if enchants.is_empty()
            && player.location_effects == fx::ActiveLocationEffects::default()
            && player.soul_speed_level == 0
        {
            // 仍要维护靴子快照，否则换上附魔靴子的那一刻会漏掉「装备变化」。
            // Keep the boots snapshot so equipping enchanted boots is not missed.
            let boots = player
                .inventory
                .equipment(EquipmentSlot::Feet)
                .cloned()
                .unwrap_or_else(ItemStack::empty);
            if let Some(player) = region.players.get_mut(&id) {
                player.last_location_boots = boots;
            }
            continue;
        }
        // 官方 `stopLocationBasedEffects` 在旧靴子被换掉时撤掉所有仍激活的效果，
        // 再对当前装备立即 `runLocationChangedEffects`（不等跨格）。
        let boots = player
            .inventory
            .equipment(EquipmentSlot::Feet)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        let boots_changed = boots != player.last_location_boots;
        let mut state = entity_state(player, &region.chunks);
        // 官方 `commonTick` 在 `ServerPlayer.tick` 之前把 tickCount 加一；
        // 这一模块在 `player_hurt::tick`（下一次自增）之前跑。
        state.tick_count = state.tick_count.wrapping_add(1);
        let position_block = BlockPos::containing(state.position);
        let crossed = player.last_location_pos != Some(position_block);

        // 谓词取数按官方顺序：TICK 的 `random_chance` 在 `baseTick` 开头，
        // `detectEquipmentUpdates` 在实体 tick 里先停旧效果，
        // 然后 `onChangedBlock` / 换装备的立即重跑才轮到 LOCATION_CHANGED。
        // 都读 `level.random`；先算好再借玩家表，避免闭包同时借用 region 的两部分。
        let mut predicates_rng = || region.random.source().next_float();
        let tick_actions = fx::tick_effects(&state, &enchants, &mut predicates_rng);
        let mut active = player.location_effects;
        let stop_actions = if boots_changed {
            fx::stop_location_based(&mut active, EquipmentSlot::Feet)
        } else {
            Vec::new()
        };
        // 装备变化时官方先停旧效果，立即重跑的那次看到的是「未激活」状态。
        let location_actions = if boots_changed || crossed {
            fx::run_location_changed(&state, &enchants, &mut active, &mut predicates_rng)
        } else {
            Vec::new()
        };
        let mut actions = tick_actions;
        actions.extend(stop_actions);
        actions.extend(location_actions);
        {
            let player = region.players.get_mut(&id).expect("player");
            player.last_location_boots = boots;
            if boots_changed || crossed {
                player.last_location_pos = Some(position_block);
            }
            player.location_effects = active;
            player.soul_speed_level = if active.is_active(EffectId::SoulSpeedAttributes) {
                enchants
                    .iter()
                    .find(|(slot, name, _)| {
                        *name == fx::SOUL_SPEED && fx::matching_slot(name, *slot)
                    })
                    .map_or(0, |&(_, _, level)| level)
            } else {
                0
            };
        }
        apply_actions(region, id, actions);
    }
}

/// 把动作落地：维护灵魂疾行修饰符等级、结冰、扣耐久、粒子与音效。
///
/// Apply actions: keep the soul-speed level, freeze water, damage boots, emit
/// soul particles and sound.
fn apply_actions(region: &mut Region, id: PlayerId, actions: Vec<fx::Action>) {
    // `rebuild` 在本模块之前跑；修饰符状态变了要立刻重建一次，让这一刻读到的
    // 属性和同步包与新激活状态一致（官方在 baseTick 里当场加/撤修饰符）。
    let mut rebuild = false;
    for action in actions {
        match action {
            fx::Action::AddModifier { .. } | fx::Action::RemoveModifier { .. } => {
                rebuild = true;
            }
            fx::Action::Freeze(req) => freeze(region, &req),
            fx::Action::DamageItem { slot, amount } => damage_boots(region, id, slot, amount),
            fx::Action::SoulParticles { position } => soul_particles(region, id, position),
            fx::Action::SoulSound { position } => soul_sound(region, id, position),
        }
    }
    if rebuild {
        let on_solid = {
            let Region {
                players, chunks, ..
            } = region;
            players
                .get(&id)
                .is_some_and(|player| crate::player_attributes::frost_block_solid(player, chunks))
        };
        if let Some(player) = region.players.get_mut(&id) {
            crate::player_attributes::rebuild_player(player, on_solid);
        }
    }
}

/// `replace_disk` 的四个方块谓词：water 方块/流体、上方空气、无实体挡放置。
/// 官方 `unobstructed` 只看 `blocksBuilding` 的实体，Bamboo 目前登记的生物都属于
/// 这一类；玩家自己也在查询范围内（官方包含发出者）。
///
/// The four `replace_disk` block predicates. Vanilla's `unobstructed` only
/// rejects `blocksBuilding` entities; Bamboo's registered mobs all qualify, and
/// the player doing the freezing is included as on the official side.
fn freeze_cells(region: &Region, req: &fx::FreezeRequest) -> Vec<BlockPos> {
    fx::freeze_cells(req, |pos| {
        let state = region.chunks.block(pos).map(BlockState);
        let is_water = state.is_some_and(|state| state.is(blocks::WATER));
        let air_above = region
            .chunks
            .block(pos.above())
            .is_some_and(|id| BlockState(id).is_tagged(block_tags::AIR));
        let cell = Aabb::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
            f64::from(pos.x.saturating_add(1)),
            f64::from(pos.y.saturating_add(1)),
            f64::from(pos.z.saturating_add(1)),
        );
        let mut blocked = region
            .players
            .values()
            .any(|other| !other.dead && other.bounding_box().intersects(cell));
        if !blocked {
            region.entities.for_each_in(cell, &mut |public| {
                if public.bounding_box().intersects(cell) {
                    blocked = true;
                }
            });
        }
        fx::FreezeCell {
            is_water,
            air_above,
            unobstructed: !blocked,
        }
    })
}

/// 用正式 `World::set_block_and_update` 写 `frosted_ice`：它会跑 `onPlace`
/// 排好融化计划刻、通知邻居并登记这一格的广播。
///
/// Use the real `World::set_block_and_update` so `onPlace` schedules the melt
/// tick, notifies neighbours and records the block change for broadcast.
fn freeze(region: &mut Region, req: &fx::FreezeRequest) {
    let cells = freeze_cells(region, req);
    if cells.is_empty() {
        return;
    }
    let ice = BlockState(blocks::FROSTED_ICE);
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        for pos in cells {
            let _ = world.set_block(pos, ice, UPDATE_ALL);
        }
        world.into_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

/// `change_item_damage` 的 `hurtAndBreak`：玩家装备破损时广播实体事件 52 并把
/// 那一格发给客户端，随后停掉这件靴子的 location 效果。
///
/// `change_item_damage`'s `hurtAndBreak`: on break, broadcast entity event 52,
/// send the inventory slot and stop this item's location effects.
fn damage_boots(region: &mut Region, id: PlayerId, slot: EquipmentSlot, amount: i32) {
    if slot != EquipmentSlot::Feet {
        return;
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let Some(stack) = player.inventory.equipment_mut(slot) else {
        return;
    };
    // 官方玩家的 `ItemStack.hurtAndBreak` 用玩家自己的随机源 `this.random`。
    // `JavaRandom` 和 `LegacyRandom` 同算法，用内部种子做个等价的取数器。
    let random = LegacyRandom::new(0);
    random.set_seed_raw(player.random.state());
    let broke = mc_bamboo_blocks::durability::hurt_equipment(stack, amount, &random);
    player.random = mc_bamboo_blocks::entity::JavaRandom::from_state(random.seed());
    if !broke {
        return;
    }
    player.location_effects = fx::ActiveLocationEffects::default();
    player.soul_speed_level = 0;
    player.last_location_boots = stack.clone();
    let entity_id = player.id.0;
    let menu_slot = 8_u16;
    let frames = encode_frames(&EntityEvent {
        entity_id,
        event: BREAK_FEET,
    });
    region.links.send_frames(id, &frames);
    if let Some(player) = region.players.get(&id) {
        player.sync.send_to_viewers(&mut region.links, &frames);
    }
    if !region.menus.contains_key(&id) {
        crate::inventory::send_slot(
            region.players.get_mut(&id).expect("player"),
            &mut region.links,
            usize::from(menu_slot),
        );
    }
    crate::player_attributes::rebuild_player(region.players.get_mut(&id).expect("player"), false);
}

/// `SpawnParticlesEffect.apply`：x/z 各取一个实体随机数（in_bounding_box），
/// 其余位置和速度是常数，不额外取数。官方 `sendParticles` 给 32 格内每人一包。
///
/// `SpawnParticlesEffect.apply`: two entity random draws (in_bounding_box x/z),
/// all other coordinates and velocities are constants. Vanilla sends one packet
/// to every player within 32 blocks.
fn soul_particles(region: &mut Region, id: PlayerId, position: Vec3) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let width = 0.6_f32;
    let height = 1.8_f32;
    // 官方顺序：x（in_bounding_box）、y（entity_position，不取数）、z（in_bounding_box）。
    let dx = (player.random.next_double() - 0.5) * f64::from(width);
    let y = position.y + f64::from(height / 2.0) + 0.1;
    let dz = (player.random.next_double() - 0.5) * f64::from(width);
    let mut options = Vec::new();
    mc_bamboo_protocol::codec::write_varint(&mut options, particle_type::SOUL);
    let frames = encode_frames(&mc_bamboo_protocol::packet::play::LevelParticles {
        options: &options,
        override_limiter: false,
        always_show: false,
        x: position.x + dx,
        y,
        z: position.z + dz,
        x_dist: 0.0,
        y_dist: 0.0,
        z_dist: 0.0,
        max_speed: 1.0,
        count: 0,
        randomization: mc_bamboo_protocol::packet::play::Randomization::Default,
    });
    let viewers: Vec<PlayerId> = region
        .players
        .values()
        .filter(|viewer| {
            // `BlockPos.closerToCenterThan(pos, 32)`：到方块中心的距离平方小于 32²。
            let block = BlockPos::containing(viewer.position());
            let cx = f64::from(block.x) + 0.5;
            let cy = f64::from(block.y) + 0.5;
            let cz = f64::from(block.z) + 0.5;
            (cx - position.x).powi(2) + (cy - position.y).powi(2) + (cz - position.z).powi(2)
                < 32.0 * 32.0
        })
        .map(|viewer| viewer.id)
        .collect();
    for viewer in viewers {
        region.links.send_frames(viewer, &frames);
    }
}

/// `PlaySoundEffect.apply`：pitch 取 uniform(0.6, 1.0)，音量常数 0.6，
/// 声音类别是玩家的 `PLAYERS`。官方用实体随机源取样。
///
/// `PlaySoundEffect.apply`: pitch is uniform(0.6, 1.0) from the entity random,
/// volume is constant 0.6 and the source is the player's `PLAYERS` category.
fn soul_sound(region: &mut Region, id: PlayerId, position: Vec3) {
    let pitch = match region.players.get_mut(&id) {
        Some(player) => 0.6 + player.random.next_float() * 0.4,
        None => return,
    };
    play_sound(
        region,
        Some(position),
        "minecraft:particle.soul_escape",
        SOUL_SOUND_VOLUME,
        pitch,
    );
}

/// 供对拍/测试直接求一次 `location_changed`，不经过世界 tick。
///
/// Evaluate one `location_changed` pass directly for oracles and tests.
pub fn replay_location_changed(
    state: &fx::EntityState,
    enchants: &[(EquipmentSlot, &'static str, i32)],
    active: &mut fx::ActiveLocationEffects,
    rng: &mut dyn FnMut() -> f32,
) -> Vec<fx::Action> {
    fx::run_location_changed(state, enchants, active, rng)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    use super::*;

    /// 一双带指定附魔的靴子。
    fn boots(name: &str, level: i32) -> ItemStack {
        let id = mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|r| r.name == "minecraft:enchantment")
            .and_then(|r| r.entries.iter().position(|e| *e == name))
            .expect("enchantment exists") as i32;
        let mut patch = ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(id, level)])),
        );
        let (added, removed) = patch.to_opaque();
        ItemStack {
            item: mc_bamboo_data::items::DIAMOND_BOOTS,
            count: 1,
            bundle_selected: None,
            added,
            removed,
        }
    }

    fn soul_state(on_soul: bool) -> fx::EntityState {
        fx::EntityState {
            position: Vec3::new(0.5, 64.0, 0.5),
            on_ground: true,
            on_soul_speed_block: on_soul,
            ..fx::EntityState::default()
        }
    }

    /// 站上灵魂沙激活两个修饰符，离开后撤掉；等级 3 时移速加成是 0.0615。
    #[test]
    fn soul_speed_activate_and_deactivate() {
        let enchants = vec![(EquipmentSlot::Feet, fx::SOUL_SPEED, 3)];
        let mut active = fx::ActiveLocationEffects::default();
        let mut rng = || 1.0_f32;
        let added = replay_location_changed(&soul_state(true), &enchants, &mut active, &mut rng);
        assert_eq!(added.len(), 2);
        let fx::Action::AddModifier { amount, .. } = &added[0] else {
            panic!("expected a movement-speed modifier");
        };
        assert_eq!(*amount, f64::from(0.0405_f32 + 0.0105_f32 * 2.0));
        assert!(active.is_active(EffectId::SoulSpeedAttributes));
        let removed = replay_location_changed(&soul_state(false), &enchants, &mut active, &mut rng);
        assert_eq!(removed.len(), 2);
        assert!(
            removed
                .iter()
                .all(|action| matches!(action, fx::Action::RemoveModifier { .. }))
        );
        assert!(!active.is_active(EffectId::SoulSpeedAttributes));
    }

    /// 冰霜行者两级的半径是 level+2，且圆盘格数随等级增加。
    #[test]
    fn frost_walker_radius_per_level() {
        let radius = |level: i32| -> Vec<BlockPos> {
            let enchants = vec![(EquipmentSlot::Feet, fx::FROST_WALKER, level)];
            let mut active = fx::ActiveLocationEffects::default();
            let mut rng = || 1.0_f32;
            let actions =
                replay_location_changed(&soul_state(false), &enchants, &mut active, &mut rng);
            let fx::Action::Freeze(req) = actions[0] else {
                panic!("expected a freeze request");
            };
            assert_eq!(req.radius, level + 2);
            fx::freeze_cells(&req, |_| fx::FreezeCell {
                is_water: true,
                air_above: true,
                unobstructed: true,
            })
        };
        assert!(radius(2).len() > radius(1).len());
        assert!(radius(1).iter().all(|pos| pos.y == 63));
    }

    /// 玩家属性表在灵魂疾行激活时会带上两个 `soul_speed/feet` 修饰符，
    /// 停机后只剩基础值。
    #[test]
    fn player_attributes_follow_active_soul_speed() {
        let mut player = Player::new(
            PlayerId(1),
            uuid::Uuid::from_u128(1),
            "Soul".to_owned(),
            mc_bamboo_protocol::packet::play::GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        player.inventory.slots[8] = boots(fx::SOUL_SPEED, 2);
        let mut region = Region::new(2, 20, false);
        region.insert_player(player);
        let id = PlayerId(1);
        let enchants = equipped(&region.players[&id]);
        assert_eq!(enchants.len(), 1);
        let mut active = fx::ActiveLocationEffects::default();
        let mut rng = || 1.0_f32;
        let mut state = entity_state(&region.players[&id], &region.chunks);
        state.on_soul_speed_block = true;
        let actions = replay_location_changed(&state, &enchants, &mut active, &mut rng);
        assert_eq!(actions.len(), 2);
        {
            let player = region.players.get_mut(&id).expect("player");
            player.location_effects = active;
            player.soul_speed_level = 2;
            crate::player_attributes::rebuild_player(player, false);
        }
        let player = &region.players[&id];
        let speed = player
            .attributes
            .value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED);
        assert!(speed > 0.1);
        let modifiers = crate::player_attributes::syncable_states(player);
        let soul = modifiers
            .iter()
            .find(|state| state.0 == mc_bamboo_data::registries::attribute::MOVEMENT_SPEED)
            .expect("movement speed is syncable");
        assert!(
            soul.2
                .iter()
                .any(|(id, _, _)| id == "minecraft:enchantment.soul_speed/feet")
        );
        {
            let player = region.players.get_mut(&id).expect("player");
            player.location_effects = fx::ActiveLocationEffects::default();
            player.soul_speed_level = 0;
            crate::player_attributes::rebuild_player(player, false);
        }
        let modifiers = crate::player_attributes::syncable_states(&region.players[&id]);
        assert!(modifiers.iter().all(|state| {
            state
                .2
                .iter()
                .all(|(id, _, _)| id != "minecraft:enchantment.soul_speed/feet")
        }));
    }

    /// 一名站在水面上方、穿冰霜行者靴子的玩家；`tick` 一次。
    fn frost_region(level: i32) -> (Region, PlayerId) {
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            uuid::Uuid::from_u128(1),
            "Frost".to_owned(),
            mc_bamboo_protocol::packet::play::GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 1.0, 0.5),
        );
        player.on_ground = true;
        player.inventory.slots[8] = boots(fx::FROST_WALKER, level);
        let mut region = Region::new(2, 20, false);
        // 玩家脚下 y=0 一片水，y=1 是空气（超平坦的地面在 y=-63，不碍事）。
        for x in -6..=6 {
            for z in -6..=6 {
                region
                    .chunks
                    .set_block(BlockPos::new(x, 0, z), blocks::WATER);
                region.chunks.set_block(BlockPos::new(x, 1, z), blocks::AIR);
            }
        }
        region.insert_player(player);
        (region, id)
    }

    /// 冰霜行者走过水面：一级 / 二级结冰的格子数不同，且都排了融化计划刻。
    #[test]
    fn tick_freezes_water_and_schedules_melt() {
        let count = |level: i32| -> (usize, usize) {
            let (mut region, _) = frost_region(level);
            tick(&mut region);
            let ice = (-6..=6)
                .flat_map(|x| (-6..=6).map(move |z| BlockPos::new(x, 0, z)))
                .filter(|pos| region.chunks.block(*pos) == Some(blocks::FROSTED_ICE))
                .count();
            let scheduled = region
                .block_ticks
                .pending()
                .filter(|tick| {
                    mc_bamboo_data::blocks::BLOCKS
                        .get(tick.block)
                        .is_some_and(|block| block.default_state == blocks::FROSTED_ICE)
                })
                .count();
            (ice, scheduled)
        };
        let (ice1, melt1) = count(1);
        let (ice2, melt2) = count(2);
        assert!(ice1 > 0 && ice2 > ice1);
        assert_eq!(melt1, ice1);
        assert_eq!(melt2, ice2);
    }

    /// 换掉靴子时立刻停机：撤掉旧的灵魂疾行修饰符，并且不产生新效果。
    #[test]
    fn tick_stops_effects_when_boots_change() {
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            uuid::Uuid::from_u128(1),
            "Soul".to_owned(),
            mc_bamboo_protocol::packet::play::GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 1.0, 0.5),
        );
        player.on_ground = true;
        player.inventory.slots[8] = boots(fx::SOUL_SPEED, 3);
        let mut region = Region::new(2, 20, false);
        region
            .chunks
            .set_block(BlockPos::new(0, 0, 0), blocks::SOUL_SAND);
        region.insert_player(player);
        tick(&mut region);
        assert_eq!(region.players[&id].soul_speed_level, 3);
        assert!(
            region.players[&id]
                .location_effects
                .is_active(EffectId::SoulSpeedAttributes)
        );
        // 换成没有附魔的靴子：下一刻必须撤掉修饰符。
        region.players.get_mut(&id).expect("player").inventory.slots[8] =
            ItemStack::new(mc_bamboo_data::items::DIAMOND_BOOTS, 1);
        tick(&mut region);
        assert_eq!(region.players[&id].soul_speed_level, 0);
        assert!(
            !region.players[&id]
                .location_effects
                .is_active(EffectId::SoulSpeedAttributes)
        );
        let states = crate::player_attributes::syncable_states(&region.players[&id]);
        assert!(states.iter().all(|state| {
            state
                .2
                .iter()
                .all(|(id, _, _)| id != "minecraft:enchantment.soul_speed/feet")
        }));
    }

    /// 改坏确认 1：半径基数被改成 2 时，等级 1 的结冰格数随之变化。
    #[test]
    fn break_check_freeze_radius_constant() {
        let req = |radius: i32| fx::FreezeRequest {
            origin: BlockPos::new(0, 0, 0),
            center_x: 0.0,
            center_z: 0.0,
            radius,
            height: 1,
        };
        let all = fx::FreezeCell {
            is_water: true,
            air_above: true,
            unobstructed: true,
        };
        let good = fx::freeze_cells(&req(3), |_| all).len();
        let broken = fx::freeze_cells(&req(2), |_| all).len();
        assert_ne!(good, broken);
        assert!(good > broken);
    }

    /// 改坏确认 2：`soul_speed_level` 被清零时，重构出的属性表少掉修饰符。
    #[test]
    fn break_check_soul_speed_state_reaches_attributes() {
        let mut player = Player::new(
            PlayerId(1),
            uuid::Uuid::from_u128(1),
            "Soul".to_owned(),
            mc_bamboo_protocol::packet::play::GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 1.0, 0.5),
        );
        player.location_effects = fx::ActiveLocationEffects::default();
        let mut active = fx::ActiveLocationEffects::default();
        let state = soul_state(true);
        assert!(
            !replay_location_changed(
                &state,
                &[(EquipmentSlot::Feet, fx::SOUL_SPEED, 3)],
                &mut active,
                &mut || 1.0_f32
            )
            .is_empty()
        );
        player.location_effects = active;
        player.soul_speed_level = 3;
        crate::player_attributes::rebuild_player(&mut player, false);
        let good = crate::player_attributes::syncable_states(&player)
            .iter()
            .flat_map(|state| state.2.iter())
            .any(|(id, _, _)| id == "minecraft:enchantment.soul_speed/feet");
        player.soul_speed_level = 0;
        crate::player_attributes::rebuild_player(&mut player, false);
        let broken = crate::player_attributes::syncable_states(&player)
            .iter()
            .flat_map(|state| state.2.iter())
            .any(|(id, _, _)| id == "minecraft:enchantment.soul_speed/feet");
        assert!(good);
        assert!(!broken);
    }
}
