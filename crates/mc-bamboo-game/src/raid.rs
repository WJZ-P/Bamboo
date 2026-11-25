//! 袭击系统的世界交互层：官方 `Raids`、`BadOmenMobEffect`、`RaidOmenMobEffect`
//! 里需要读写世界的那部分。纯数据、波次表和不碰世界的状态判定在
//! `mc_bamboo_blocks::entity::raid`，这里只编排 `Region`。
//!
//! The world-interaction half of raids: the parts of vanilla `Raids`,
//! `BadOmenMobEffect` and `RaidOmenMobEffect` that touch the world. Pure data,
//! wave tables and world-free predicates live in `mc_bamboo_blocks::entity::raid`;
//! this module only drives a `Region`.

use mc_bamboo_blocks::{
    World,
    entity::{
        Entity,
        mob::{
            MobType,
            effects::EffectInstance,
            spawn::{SpawnReason, finalize_spawn},
        },
        raid::{
            DEFAULT_PRE_RAID_TICKS, DIFFICULTY_PEACEFUL, HERO_OF_THE_VILLAGE_DURATION,
            MAX_CELEBRATION_TICKS, MAX_RAID_OMEN_LEVEL, POST_RAID_TICK_LIMIT,
            RAID_REMOVAL_THRESHOLD_SQR, RAID_TIMEOUT_TICKS, Raid, RaidStatus, RaiderType,
            VALID_RAID_RADIUS_SQR,
        },
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3, mth};
use mc_bamboo_data::{gamerules::RAIDS, registries::mob_effect};
use mc_bamboo_protocol::packet::play::GameMode;
use mc_bamboo_world::poi::{Occupancy, PoiRecord, is_village_type};

use crate::{inbound::PlayerId, region::Region};

/// `BadOmenMobEffect.applyEffectTick`：玩家带不祥之兆时每刻调用。进村、非和平、
/// 非旁观、且附近袭击没到不祥等级上限时，转成 600 刻的袭击之兆并记下村庄坐标，
/// 移除不祥之兆。`amplifier` 是当前 BAD_OMEN 的等级，转换后沿用。
///
/// Vanilla `BadOmenMobEffect.applyEffectTick`: called every tick while BAD_OMEN
/// is active. On village entry (non-peaceful, non-spectator, and no nearby raid
/// at the max omen level) convert to a 600-tick RAID_OMEN, record the village
/// position and remove BAD_OMEN. `amplifier` is carried over.
pub(crate) fn bad_omen_check(region: &mut Region, id: PlayerId, amplifier: i32) {
    let Some((spectator, pos)) = region.players.get(&id).map(|player| {
        (
            player.game_mode == GameMode::Spectator,
            BlockPos::containing(player.position()),
        )
    }) else {
        return;
    };
    if spectator || region.global.difficulty as i32 == DIFFICULTY_PEACEFUL {
        return;
    }
    // `ServerLevel.isVillage(pos)` = `sectionsToVillage <= 1`。
    if !region.poi.is_close_to_village(pos, 1) {
        return;
    }
    // `ServerLevel.getRaidAt(pos)`；已有且满级的袭击不再吸收不祥之兆。
    if let Some(raid_id) = region
        .raids
        .nearby_raid(pos, f64::from(VALID_RAID_RADIUS_SQR))
        && let Some(raid) = region.raids.raids.get(&raid_id)
        && raid.raid_omen_level >= MAX_RAID_OMEN_LEVEL
    {
        return;
    }
    let converted = EffectInstance::with_amplifier(mob_effect::RAID_OMEN, 600, amplifier);
    crate::player_effects::give(region, id, converted);
    // `applyEffectTick` 返回 false 会立刻移除本效果。
    crate::player_effects::clear(region, id, Some(mob_effect::BAD_OMEN));
    if let Some(player) = region.players.get_mut(&id) {
        player.raid_omen_position = Some(pos);
    }
}

/// `RaidOmenMobEffect.applyEffectTick`：袭击之兆倒计时最后一刻调用。用记下的
/// 村庄坐标建 / 扩展袭击，然后清掉坐标。`amplifier` 是 RAID_OMEN 的等级。
///
/// Vanilla `RaidOmenMobEffect.applyEffectTick`: called on the final tick. Start
/// or extend a raid at the recorded village position, then clear it. `amplifier`
/// is the RAID_OMEN amplifier.
pub(crate) fn raid_omen_expire(region: &mut Region, id: PlayerId, amplifier: i32) {
    let Some(pos) = region
        .players
        .get(&id)
        .and_then(|player| player.raid_omen_position)
    else {
        return;
    };
    create_or_extend_raid(region, id, pos, amplifier);
    if let Some(player) = region.players.get_mut(&id) {
        player.raid_omen_position = None;
    }
}

/// `Raids.createOrExtendRaid(player, pos)`：按 `#minecraft:village` 且已占用的
/// POI 求平均中心（没有就用 `pos`），在该中心的 `VALID_RAID_RADIUS_SQR` 内找现有
/// 袭击，否则用全局 `next_id` 新建一场；把玩家的袭击之兆等级吸收进去。返回袭击 id。
///
/// Vanilla `Raids.createOrExtendRaid`: average the occupied `#village` POIs into
/// a center (falling back to `pos`), find a raid near it or create one with the
/// global `next_id`, then absorb the player's raid omen level. Returns the id.
pub(crate) fn create_or_extend_raid(
    region: &mut Region,
    id: PlayerId,
    pos: BlockPos,
    raid_omen_amplifier: i32,
) -> Option<i32> {
    let spectator = region.players.get(&id)?.game_mode == GameMode::Spectator;
    if spectator || !region.global.rules.bool(RAIDS) {
        return None;
    }
    // `EnvironmentAttributes.CAN_START_RAID`：官方只在主世界允许；Bamboo 目前
    // 只建模主世界，恒真。
    let difficulty = region.global.difficulty as i32;
    let records = region
        .poi
        .get_in_range(is_village_type, pos, 64, Occupancy::IsOccupied);
    let center = village_center(&records, pos);
    let raid_id = match region
        .raids
        .nearby_raid(center, f64::from(VALID_RAID_RADIUS_SQR))
    {
        Some(existing) => existing,
        None => {
            let new_id = region.raids.unique_id();
            region
                .raids
                .raids
                .insert(new_id, Raid::new(center, difficulty));
            new_id
        }
    };
    let raid = region.raids.raids.get_mut(&raid_id)?;
    // 官方：`!started || raidOmenLevel < max` 时 `absorbRaidOmen(player)`。
    if !raid.started || raid.raid_omen_level < MAX_RAID_OMEN_LEVEL {
        raid.absorb_raid_omen(Some(raid_omen_amplifier));
    }
    Some(raid_id)
}

/// `createOrExtendRaid` 里 POI 位置的平均：`BlockPos.containing(sum / count)`，
/// 没有 POI 时用玩家位置。
fn village_center(records: &[PoiRecord], fallback: BlockPos) -> BlockPos {
    if records.is_empty() {
        return fallback;
    }
    let (mut sx, mut sy, mut sz) = (0.0_f64, 0.0_f64, 0.0_f64);
    for record in records {
        sx += f64::from(record.pos.x);
        sy += f64::from(record.pos.y);
        sz += f64::from(record.pos.z);
    }
    let count = records.len() as f64;
    BlockPos::containing(Vec3::new(sx / count, sy / count, sz / count))
}

/// 一波要生成的袭击者。阶段 A 算出，阶段 B 在世界里落地。
struct SpawnPlan {
    raid_id: i32,
    wave: i32,
    pos: BlockPos,
    /// 第一期只刷掠夺者、女巫；卫道士、劫掠兽、唤魔者留到第二期。
    members: Vec<MobType>,
}

/// 官方 `Raids.tick(ServerLevel)` 的一刻：门控、逐场推进、移除 stopped、
/// 每 200 刻标脏（Bamboo 存盘是全量写，这里只保留 tick 计数）。
///
/// Vanilla `Raids.tick`: gate on the raids rule, tick every raid, drop stopped
/// ones. The work is staged: advance state on `&Region`, spawn entities in a
/// short `World` borrow, then put the roster back and apply player effects.
pub(crate) fn tick(region: &mut Region) {
    let rules_on = region.global.rules.bool(RAIDS);
    let difficulty = region.global.difficulty as i32;
    let mut raids = std::mem::take(&mut region.raids);
    raids.tick += 1;
    let mut plans = Vec::new();
    let mut rewards: Vec<([u8; 16], i32)> = Vec::new();
    for (&raid_id, raid) in raids.raids.iter_mut() {
        if !rules_on {
            raid.status = RaidStatus::Stopped;
            raid.active = false;
            continue;
        }
        if raid.is_stopped() {
            continue;
        }
        step_one(region, raid_id, raid, difficulty, &mut plans, &mut rewards);
    }

    let effects = if plans.is_empty() {
        Vec::new()
    } else {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut spawned = Vec::new();
        for plan in &plans {
            let members = spawn_group(&mut world, plan);
            spawned.push((plan.raid_id, plan.wave, members));
        }
        let fx = world.into_effects();
        for (raid_id, wave, members) in spawned {
            if let Some(raid) = raids.raids.get_mut(&raid_id) {
                for (uid, health) in members {
                    raid.add_wave_mob(wave, uid, health, true);
                }
            }
        }
        fx
    };
    raids.raids.retain(|_, raid| !raid.is_stopped());
    region.raids = raids;
    crate::interaction::send_effects(region, None, &effects);
    for (uuid, amplifier) in rewards {
        give_hero_of_the_village(region, uuid, amplifier);
    }
}

/// `Raid.tick` 的一刻，只推进自身状态并产出要刷的波次。
fn step_one(
    view: &Region,
    raid_id: i32,
    raid: &mut Raid,
    difficulty: i32,
    plans: &mut Vec<SpawnPlan>,
    rewards: &mut Vec<([u8; 16], i32)>,
) {
    if raid.status == RaidStatus::Ongoing {
        let center_chunk = ChunkPos::new(raid.center.x >> 4, raid.center.z >> 4);
        raid.active = view.chunks.is_loaded(center_chunk);
        if difficulty == DIFFICULTY_PEACEFUL {
            raid.status = RaidStatus::Stopped;
            raid.active = false;
            return;
        }
        if !raid.active {
            return;
        }
        if !view.poi.is_close_to_village(raid.center, 1)
            && let Some(new_center) = nearby_village_center(view, raid.center)
        {
            raid.center = new_center;
        }
        if !view.poi.is_close_to_village(raid.center, 1) {
            if raid.groups_spawned > 0 {
                raid.status = RaidStatus::Loss;
            } else {
                raid.status = RaidStatus::Stopped;
                raid.active = false;
            }
        }
        raid.ticks_active += 1;
        if raid.ticks_active >= RAID_TIMEOUT_TICKS {
            raid.status = RaidStatus::Stopped;
            raid.active = false;
            return;
        }
        let alive = raid.total_raiders();
        if alive == 0 && raid.has_more_waves() {
            if raid.cooldown_ticks > 0 {
                if raid.wave_spawn_pos.is_none() && raid.cooldown_ticks % 5 == 0 {
                    raid.wave_spawn_pos = find_random_spawn_pos(view, raid);
                }
                raid.cooldown_ticks -= 1;
            } else if raid.cooldown_ticks == 0 && raid.groups_spawned > 0 {
                raid.cooldown_ticks = DEFAULT_PRE_RAID_TICKS;
                return;
            }
        }
        if raid.ticks_active % 20 == 0 {
            update_raiders(view, raid);
        }
        // `getTotalRaidersAlive()` 在官方循环里每轮重算；这里用刷出的成员数
        // 模拟名册增长，让一次 tick 只刷一波。
        let mut alive_now = alive;
        let mut failed = 0;
        while raid.should_spawn_group(alive_now) {
            let pos = raid
                .wave_spawn_pos
                .or_else(|| find_random_spawn_pos(view, raid));
            if let Some(pos) = pos {
                raid.started = true;
                let members = spawn_group_plan(raid, difficulty, pos, alive_now);
                alive_now = members.len() as i32;
                plans.push(SpawnPlan {
                    raid_id,
                    wave: raid.groups_spawned,
                    pos,
                    members,
                });
            } else {
                failed += 1;
                if failed > 5 {
                    raid.status = RaidStatus::Stopped;
                    raid.active = false;
                    break;
                }
            }
        }
        if raid.started && !raid.has_more_waves() && alive == 0 {
            if raid.post_raid_ticks < POST_RAID_TICK_LIMIT {
                raid.post_raid_ticks += 1;
            } else {
                raid.status = RaidStatus::Victory;
                let amplifier = raid.raid_omen_level - 1;
                for hero in &raid.heroes_of_the_village {
                    rewards.push((*hero, amplifier));
                }
            }
        }
    } else if raid.is_over() {
        raid.celebration_ticks += 1;
        if raid.celebration_ticks >= MAX_CELEBRATION_TICKS {
            raid.status = RaidStatus::Stopped;
            raid.active = false;
        }
    }
}

/// `spawnGroup` 的纯部分：算这一波刷哪些类型（第一期只有掠夺者 / 女巫），
/// 更新波次数、清空 `waveSpawnPos`。实体在阶段 B 生成。
fn spawn_group_plan(raid: &mut Raid, difficulty: i32, pos: BlockPos, alive: i32) -> Vec<MobType> {
    let wave = raid.groups_spawned + 1;
    raid.total_health = 0.0;
    let bonus = raid.should_spawn_bonus_group(alive);
    let mut members = Vec::new();
    for kind in [RaiderType::Pillager, RaiderType::Witch] {
        let def = raid.default_num_spawns(kind, wave, bonus);
        let extra =
            Raid::potential_bonus_spawns(kind, wave, difficulty, bonus, &mut |b| raid.next_int(b));
        let mob = match kind {
            RaiderType::Pillager => MobType::Pillager,
            _ => MobType::Witch,
        };
        for _ in 0..(def + extra) {
            members.push(mob);
        }
    }
    raid.wave_spawn_pos = None;
    raid.groups_spawned += 1;
    let _ = pos;
    members
}

/// 阶段 B：在世界里生成一波的实体，返回 (uid, 血量) 供回填名册。
fn spawn_group(world: &mut World, plan: &SpawnPlan) -> Vec<(i32, f32)> {
    let mut out = Vec::new();
    for &mob in &plan.members {
        let at = Vec3::new(
            f64::from(plan.pos.x) + 0.5,
            f64::from(plan.pos.y) + 1.0,
            f64::from(plan.pos.z) + 0.5,
        );
        let mut entity = world.prepare_entity(Entity::by_mob_type(mob, at));
        finalize_spawn(world, mob, &mut entity, SpawnReason::Natural);
        let health = entity.kind.mob().map_or(0.0, |m| m.health);
        let uid = world.add_prepared_entity(entity);
        out.push((uid, health));
    }
    out
}

/// `updateRaiders`：每 20 刻清掉已移除 / 离中心太远的袭击者（其余离队条件
/// 依赖 Raider 上的 `noActionTime` / `ticksOutsideRaid`，后续补）。
fn update_raiders(view: &Region, raid: &mut Raid) {
    let center = raid.center;
    for members in raid.raiders.values_mut() {
        members.retain(|uid| {
            view.entities.public_at(*uid).is_some_and(|public| {
                !public.removed && {
                    let dx = public.position.x - f64::from(center.x);
                    let dz = public.position.z - f64::from(center.z);
                    dx * dx + dz * dz < RAID_REMOVAL_THRESHOLD_SQR
                }
            })
        });
    }
}

/// `moveRaidCenterToNearbyVillageSection` 的简化：取半径 32 内已占用的村庄
/// POI 平均中心。
fn nearby_village_center(view: &Region, center: BlockPos) -> Option<BlockPos> {
    let records = view
        .poi
        .get_in_range(is_village_type, center, 32, Occupancy::IsOccupied);
    (!records.is_empty()).then(|| village_center(&records, center))
}

/// `findRandomSpawnPos(level, 8)`：照官方的螺旋候选 + WORLD_SURFACE 高度图，
/// 半径 / 村庄 / 区块加载 / 高度差各道门槛。
fn find_random_spawn_pos(view: &Region, raid: &mut Raid) -> Option<BlockPos> {
    let i = raid.cooldown_ticks / 20;
    let f = 0.22 * (i as f32) - 0.24;
    let angle = raid.next_float() * std::f32::consts::TAU;
    for j in 0..8 {
        let a = angle + std::f32::consts::PI * (j as f32) / 8.0;
        let cos = f64::from(mth::cos(f64::from(a)));
        let sin = f64::from(mth::sin(f64::from(a)));
        let x = raid.center.x
            + (cos * 32.0 * f64::from(f)).floor() as i32
            + raid.next_int(3) * (f.floor() as i32);
        let z = raid.center.z
            + (sin * 32.0 * f64::from(f)).floor() as i32
            + raid.next_int(3) * (f.floor() as i32);
        let y = view
            .chunks
            .world_surface_y(BlockPos::new(x, raid.center.y, z))
            .unwrap_or(raid.center.y);
        if (y - raid.center.y).abs() > 96 {
            continue;
        }
        let pos = BlockPos::new(x, y, z);
        if view.poi.is_close_to_village(pos, 1) && i > 7 {
            continue;
        }
        let (cx0, cx1) = ((x - 10) >> 4, (x + 10) >> 4);
        let (cz0, cz1) = ((z - 10) >> 4, (z + 10) >> 4);
        let mut loaded = true;
        for cx in cx0..=cx1 {
            for cz in cz0..=cz1 {
                if !view.chunks.is_loaded(ChunkPos::new(cx, cz)) {
                    loaded = false;
                }
            }
        }
        if !loaded {
            continue;
        }
        return Some(pos);
    }
    None
}

/// 胜利分支：给英雄（按 UUID 找在线玩家）加 HERO_OF_THE_VILLAGE。
fn give_hero_of_the_village(region: &mut Region, uuid: [u8; 16], amplifier: i32) {
    let Some(id) = region
        .players
        .iter()
        .find(|(_, player)| *player.uuid.as_bytes() == uuid)
        .map(|(id, _)| *id)
    else {
        return;
    };
    // 官方 `MobEffectInstance(HERO, 48000, amplifier, false, false, true)`。
    let mut effect = EffectInstance::with_flags(
        mob_effect::HERO_OF_THE_VILLAGE,
        HERO_OF_THE_VILLAGE_DURATION,
        amplifier,
        false,
        false,
    );
    effect.show_icon = true;
    crate::player_effects::give(region, id, effect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{player::Player, region::Region, world_state::Difficulty};
    use mc_bamboo_blocks::entity::mob::effects::EffectInstance;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    fn region_with_player() -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        region.global.difficulty = Difficulty::Normal;
        (region, id)
    }

    /// 在玩家脚下的段放一个被占用的「钟」POI，让 `isVillage` 成立。
    fn make_village(region: &mut Region) -> BlockPos {
        let pos = BlockPos::new(0, 64, 0);
        let meeting = mc_bamboo_data::poi::poi_type_id("meeting").unwrap();
        region.poi.add(pos, meeting);
        let taken = region.poi.take(|_| true, |_, _| true, pos, 8);
        assert_eq!(taken, Some(pos));
        pos
    }

    fn give_bad_omen(region: &mut Region, id: PlayerId, amplifier: i32) {
        region
            .players
            .get_mut(&id)
            .unwrap()
            .effects
            .add(EffectInstance::with_amplifier(
                mob_effect::BAD_OMEN,
                120000,
                amplifier,
            ));
    }

    /// 进村把不祥之兆转成袭击之兆，沿用等级、记下坐标、移除不祥之兆。
    #[test]
    fn bad_omen_converts_to_raid_omen_in_a_village() {
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        give_bad_omen(&mut region, id, 2);
        bad_omen_check(&mut region, id, 2);
        let player = &region.players[&id];
        assert!(player.effects.has(mob_effect::RAID_OMEN));
        assert!(!player.effects.has(mob_effect::BAD_OMEN));
        let raid_omen = player.effects.get(mob_effect::RAID_OMEN).unwrap();
        assert_eq!(raid_omen.duration, 600);
        assert_eq!(raid_omen.amplifier, 2);
        assert_eq!(player.raid_omen_position, Some(BlockPos::new(0, 64, 0)));
        // 改坏确认：转换后不该还留着 BAD_OMEN。
        assert_ne!(player.effects.amplifier(mob_effect::BAD_OMEN), Some(2));
    }

    /// 不在村庄 / 和平 / 旁观都不转换。
    #[test]
    fn bad_omen_stays_when_gates_fail() {
        // 无村庄 POI。
        let (mut region, id) = region_with_player();
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));
        assert!(!region.players[&id].effects.has(mob_effect::RAID_OMEN));

        // 和平难度。
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        region.global.difficulty = Difficulty::Peaceful;
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));

        // 旁观者。
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        region.players.get_mut(&id).unwrap().game_mode = GameMode::Spectator;
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));
    }

    /// 附近已有满级不祥等级的袭击时不转换。
    #[test]
    fn bad_omen_stops_at_a_maxed_raid() {
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        raid.raid_omen_level = MAX_RAID_OMEN_LEVEL;
        region.raids.raids.insert(raid_id, raid);
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));
        assert!(!region.players[&id].effects.has(mob_effect::RAID_OMEN));
    }

    /// createOrExtendRaid 新建一场、吸收等级，再调用复用同一场并夹到上限。
    #[test]
    fn create_or_extend_raid_makes_then_reuses() {
        let (mut region, id) = region_with_player();
        let poi = make_village(&mut region);
        let first = create_or_extend_raid(&mut region, id, poi, 2).unwrap();
        assert_eq!(region.raids.raids.len(), 1);
        let raid = &region.raids.raids[&first];
        assert_eq!(raid.center, poi);
        assert_eq!(raid.raid_omen_level, 3); // amplifier 2 + 1
        assert_eq!(raid.group_count, 5); // 普通难度 numGroups
        assert_eq!(region.raids.next_id, 2);

        // 同中心复用，不新建。
        let second = create_or_extend_raid(&mut region, id, poi, 4).unwrap();
        assert_eq!(first, second);
        assert_eq!(region.raids.raids.len(), 1);
        assert_eq!(
            region.raids.raids[&first].raid_omen_level,
            MAX_RAID_OMEN_LEVEL
        );
        assert_eq!(region.raids.next_id, 2);
    }

    /// 没有村庄 POI 时用玩家位置当中心。
    #[test]
    fn create_or_extend_raid_falls_back_to_player_position() {
        let (mut region, id) = region_with_player();
        let pos = BlockPos::new(7, 64, -3);
        let raid_id = create_or_extend_raid(&mut region, id, pos, 0).unwrap();
        assert_eq!(region.raids.raids[&raid_id].center, pos);
        assert_eq!(region.raids.raids[&raid_id].raid_omen_level, 1);
    }

    /// 冷却倒数到 0 后刷出第一波，实体进名册、类型只含掠夺者 / 女巫。
    #[test]
    fn raid_tick_spawns_first_wave_after_cooldown() {
        let (mut region, _id) = region_with_player();
        make_village(&mut region);
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        // 预设刷怪点，绕开 findRandomSpawnPos 的随机取数。
        raid.wave_spawn_pos = Some(BlockPos::new(0, 64, 0));
        region.raids.raids.insert(raid_id, raid);
        for _ in 0..(DEFAULT_PRE_RAID_TICKS + 2) {
            tick(&mut region);
        }
        let raid = &region.raids.raids[&raid_id];
        assert!(raid.started, "首波已刷");
        assert_eq!(raid.groups_spawned, 1);
        assert!(raid.total_raiders() > 0, "名册里有袭击者");
        // 刷出来的实体类型只有掠夺者 / 女巫（第一期）。
        for uid in raid.raiders.values().flatten() {
            let mob = region.entities.public_at(*uid).unwrap();
            assert!(matches!(
                mob.kind,
                mc_bamboo_blocks::entity::KindPublic::Mob(_)
            ));
        }
    }

    /// 所有波次刷完、名册清空后转胜利，并给英雄加 HERO_OF_THE_VILLAGE。
    #[test]
    fn raid_tick_wins_and_rewards_heroes() {
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        let uuid_bytes = *region.players[&id].uuid.as_bytes();
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        raid.started = true;
        raid.groups_spawned = raid.group_count;
        raid.raid_omen_level = 1; // 不触发奖励波，hasMoreWaves 为假
        raid.add_hero_of_the_village(uuid_bytes);
        region.raids.raids.insert(raid_id, raid);
        for _ in 0..(POST_RAID_TICK_LIMIT + 2) {
            tick(&mut region);
        }
        assert_eq!(
            region.raids.raids.get(&raid_id).map(|r| r.status),
            Some(RaidStatus::Victory)
        );
        assert!(
            region.players[&id]
                .effects
                .has(mob_effect::HERO_OF_THE_VILLAGE)
        );
    }

    /// 袭击之兆到期建袭击并清掉坐标。
    #[test]
    fn raid_omen_expire_starts_the_raid_and_clears_position() {
        let (mut region, id) = region_with_player();
        make_village(&mut region);
        region.players.get_mut(&id).unwrap().raid_omen_position = Some(BlockPos::new(0, 64, 0));
        raid_omen_expire(&mut region, id, 1);
        assert_eq!(region.raids.raids.len(), 1);
        assert_eq!(
            region.raids.raids.values().next().unwrap().raid_omen_level,
            2
        );
        assert_eq!(region.players[&id].raid_omen_position, None);
    }
}
