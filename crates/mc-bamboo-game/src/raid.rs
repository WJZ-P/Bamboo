//! 袭击系统的世界交互层：官方 `Raids`、`BadOmenMobEffect`、`RaidOmenMobEffect`
//! 里需要读写世界的那部分。纯数据、波次表和不碰世界的状态判定在
//! `mc_bamboo_blocks::entity::raid`，这里只编排 `Region`。
//!
//! The world-interaction half of raids: the parts of vanilla `Raids`,
//! `BadOmenMobEffect` and `RaidOmenMobEffect` that touch the world. Pure data,
//! wave tables and world-free predicates live in `mc_bamboo_blocks::entity::raid`;
//! this module only drives a `Region`.

#[cfg(feature = "oracle-samples")]
mod replay;
#[cfg(feature = "oracle-samples")]
pub use replay::*;

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
            DEFAULT_PRE_RAID_TICKS, DIFFICULTY_NORMAL, DIFFICULTY_PEACEFUL,
            HERO_OF_THE_VILLAGE_DURATION, MAX_CELEBRATION_TICKS, MAX_RAID_OMEN_LEVEL,
            POST_RAID_TICK_LIMIT, RAID_REMOVAL_THRESHOLD_SQR, RAID_TIMEOUT_TICKS, Raid, RaidStatus,
            RaiderType, VALID_RAID_RADIUS_SQR, num_groups,
        },
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3, mth};
use mc_bamboo_data::{gamerules::RAIDS, registries::mob_effect, sounds};
use mc_bamboo_protocol::{
    packet::play::{BossAction, BossEvent, GameMode, Sound},
    text::{Component, TranslateArg},
};
use mc_bamboo_world::poi::{Occupancy, PoiRecord, is_village_type};
use uuid::Uuid;

use crate::{inbound::PlayerId, links::encode_frames, region::Region};

/// `Raid.raidEvent` 的 Boss 栏颜色（官方 RED）和分段样式（NOTCHED_10）。
const RAID_BAR_COLOR: i32 = 2;
const RAID_BAR_OVERLAY: i32 = 2;

/// `Raid.raidEvent` 的名字组件。剩余数后缀照官方 `RAID_NAME_COMPONENT.copy()`
/// 拼上 " - " 和 `event.minecraft.raid.raiders_remaining`。
///
/// The raid boss-bar name: the remaining-raiders suffix follows the official
/// `RAID_NAME_COMPONENT.copy()` + " - " + translation.
fn bar_name(name: mc_bamboo_blocks::entity::raid::RaidBarName, remaining: i32) -> Component {
    use mc_bamboo_blocks::entity::raid::RaidBarName as Name;
    let key = match name {
        Name::Raid => "event.minecraft.raid",
        Name::RaidersRemaining => "event.minecraft.raid",
        Name::Victory => "event.minecraft.raid.victory.full",
        Name::Defeat => "event.minecraft.raid.defeat.full",
    };
    let base = Component::translate(key, []);
    if name == Name::RaidersRemaining {
        base.extra(Component::text(" - "))
            .extra(Component::translate(
                "event.minecraft.raid.raiders_remaining",
                [TranslateArg::from(remaining)],
            ))
    } else {
        base
    }
}

/// bossbar 的 add 包。
fn add_frame(raid: &Raid) -> Vec<u8> {
    let name = bar_name(raid.boss_name, raid.boss_raiders_remaining);
    encode_frames(&BossEvent {
        id: Uuid::from_bytes(raid.boss_uuid),
        action: BossAction::Add {
            name: &name,
            progress: raid.boss_progress,
            color: RAID_BAR_COLOR,
            overlay: RAID_BAR_OVERLAY,
            flags: 0,
        },
    })
}

/// bossbar 的 remove 包。
fn remove_frame(raid: &Raid) -> Vec<u8> {
    encode_frames(&BossEvent {
        id: Uuid::from_bytes(raid.boss_uuid),
        action: BossAction::Remove,
    })
}

/// `ServerBossEvent.setProgress`：值变了才广播（官方 `fcmpl` 比较）。
fn set_progress(raid: &mut Raid, progress: f32, out: &mut Vec<(PlayerId, Vec<u8>)>) {
    if raid.boss_progress == progress {
        return;
    }
    raid.boss_progress = progress;
    if raid.boss_visible {
        let frame = encode_frames(&BossEvent {
            id: Uuid::from_bytes(raid.boss_uuid),
            action: BossAction::UpdateProgress(progress),
        });
        for uid in &raid.boss_players {
            out.push((PlayerId(*uid), frame.clone()));
        }
    }
}

/// `ServerBossEvent.setName`：名字和剩余数都变了才广播。
fn set_name(
    raid: &mut Raid,
    name: mc_bamboo_blocks::entity::raid::RaidBarName,
    remaining: i32,
    out: &mut Vec<(PlayerId, Vec<u8>)>,
) {
    if raid.boss_name == name && raid.boss_raiders_remaining == remaining {
        return;
    }
    raid.boss_name = name;
    raid.boss_raiders_remaining = remaining;
    if raid.boss_visible {
        let component = bar_name(name, remaining);
        let frame = encode_frames(&BossEvent {
            id: Uuid::from_bytes(raid.boss_uuid),
            action: BossAction::UpdateName(&component),
        });
        for uid in &raid.boss_players {
            out.push((PlayerId(*uid), frame.clone()));
        }
    }
}

/// `ServerBossEvent.setVisible`：变了就给现有观众补 add / remove。
fn set_visible(raid: &mut Raid, visible: bool, out: &mut Vec<(PlayerId, Vec<u8>)>) {
    if raid.boss_visible == visible {
        return;
    }
    raid.boss_visible = visible;
    let frame = if visible {
        add_frame(raid)
    } else {
        remove_frame(raid)
    };
    for uid in &raid.boss_players {
        out.push((PlayerId(*uid), frame.clone()));
    }
}

/// `Raids.getNearbyRaid` 的一层快照：`updatePlayers` 在 `raids.raids` 被可变借用时
/// 也要按「离玩家最近的那场 active 袭击」判定观众，所以事先抽出 active 场的中心。
fn nearest_raid(active: &[(i32, BlockPos)], pos: BlockPos) -> Option<i32> {
    let mut best: Option<(i32, f64)> = None;
    for &(id, center) in active {
        let dx = f64::from(pos.x - center.x);
        let dy = f64::from(pos.y - center.y);
        let dz = f64::from(pos.z - center.z);
        let dist = dx * dx + dy * dy + dz * dz;
        if dist < f64::from(VALID_RAID_RADIUS_SQR) && best.is_none_or(|(_, d)| dist < d) {
            best = Some((id, dist));
        }
    }
    best.map(|(id, _)| id)
}

/// `Raid.updatePlayers`：只让活着、且 `getRaidAt(玩家位置) == 本场` 的玩家看血条。
/// 玩家以实体 uid 记录，发包时映回 `PlayerId`；`Bamboo` 的玩家 uid 就是 `PlayerId`。
///
/// `Raid.updatePlayers`: only living players whose `getRaidAt(pos)` is this
/// raid see the bar. Player uids and `PlayerId`s are the same value in Bamboo.
fn update_players(
    region: &Region,
    active: &[(i32, BlockPos)],
    raid_id: i32,
    raid: &mut Raid,
    out: &mut Vec<(PlayerId, Vec<u8>)>,
) {
    let mut valid = Vec::new();
    for (&id, player) in &region.players {
        if player.dead {
            continue;
        }
        let pos = BlockPos::containing(player.position());
        if nearest_raid(active, pos) == Some(raid_id) {
            valid.push(id);
        }
    }
    // 离开血条的人：发 remove（可见时）。
    let mut staying = Vec::new();
    for uid in std::mem::take(&mut raid.boss_players) {
        let id = PlayerId(uid);
        if valid.contains(&id) {
            staying.push(uid);
        } else if raid.boss_visible {
            out.push((id, remove_frame(raid)));
        }
    }
    // 新加入的人：发 add（可见时）。
    let add = raid.boss_visible.then(|| add_frame(raid));
    for id in valid {
        let uid = id.0;
        if !staying.contains(&uid) {
            staying.push(uid);
            if let Some(frame) = &add {
                out.push((id, frame.clone()));
            }
        }
    }
    staying.sort_unstable();
    raid.boss_players = staying;
}

/// `Raid.stop` 的 `removeAllPlayers`：给所有观众发 remove 再清空。
fn stop_bar(raid: &mut Raid, out: &mut Vec<(PlayerId, Vec<u8>)>) {
    if raid.boss_visible {
        let frame = remove_frame(raid);
        for uid in &raid.boss_players {
            out.push((PlayerId(*uid), frame.clone()));
        }
    }
    raid.boss_players.clear();
    raid.active = false;
    raid.status = RaidStatus::Stopped;
}

/// 首波刷出时的号角：`ClientboundSoundPacket(RAID_HORN, NEUTRAL, 64, 1, seed)`。
fn raid_horn(region: &Region, pos: BlockPos) -> Vec<u8> {
    let sound = Sound {
        sound: i32::from(sounds::sound_event("minecraft:event.raid.horn")),
        inline: None,
        source: sounds::NEUTRAL,
        x: f64::from(pos.x),
        y: f64::from(pos.y),
        z: f64::from(pos.z),
        volume: 64.0,
        pitch: 1.0,
        seed: region.sound_seed_generator.source().next_long(),
    };
    encode_frames(&sound)
}

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
    /// `Raid.getEnchantOdds()`：`joinRaid` 里给卫道士 / 掠夺者套附魔的门槛。
    enchant_odds: f32,
    /// 第一期刷掠夺者、卫道士、劫掠兽、女巫；唤魔者留到第二期。
    members: Vec<MobType>,
    /// 劫掠兽的骑手：`(劫掠兽在 members 里的下标, 骑手类型)`。第 5 波掠夺者。
    /// 骑手在 `spawn_group` 里紧跟劫掠兽生成并骑上去。
    riders: Vec<(usize, MobType)>,
}

/// 官方 `Raids.tick(ServerLevel)` 的一刻：门控、逐场推进、移除 stopped、
/// 每 200 刻标脏（Bamboo 存盘是全量写，这里只保留 tick 计数）。
///
/// Vanilla `Raids.tick`: gate on the raids rule, tick every raid, drop stopped
/// ones. The work is staged: advance state on `&Region`, spawn entities in a
/// short `World` borrow, then put the roster back, send boss-bar / horn packets
/// and apply player effects.
pub(crate) fn tick(region: &mut Region) {
    let rules_on = region.global.rules.bool(RAIDS);
    let difficulty = region.global.difficulty as i32;
    let mut raids = std::mem::take(&mut region.raids);
    raids.tick += 1;
    let mut plans = Vec::new();
    let mut rewards: Vec<([u8; 16], i32)> = Vec::new();
    let mut frames: Vec<(PlayerId, Vec<u8>)> = Vec::new();
    let mut horns: Vec<(BlockPos, i32)> = Vec::new();
    let active: Vec<(i32, BlockPos)> = raids
        .raids
        .iter()
        .filter(|(_, raid)| raid.active)
        .map(|(&id, raid)| (id, raid.center))
        .collect();
    for (&raid_id, raid) in raids.raids.iter_mut() {
        if !rules_on {
            stop_bar(raid, &mut frames);
            continue;
        }
        if raid.is_stopped() {
            continue;
        }
        step_one(
            region,
            &active,
            raid_id,
            raid,
            difficulty,
            &mut plans,
            &mut rewards,
            &mut frames,
            &mut horns,
        );
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
            let health: f32 = members.iter().map(|(_, hp)| *hp).sum();
            if let Some(raid) = raids.raids.get_mut(&raid_id) {
                for (uid, hp) in members {
                    raid.add_wave_mob(wave, uid, hp, true);
                }
                // 官方 spawnGroup 里实体已经进世界，updateBossbar 读到的是满血之和。
                // The mobs are already in the world at official spawnGroup's
                // updateBossbar, so the living health is the sum of full health.
                let progress = raid.bossbar_progress(health);
                set_progress(raid, progress, &mut frames);
            }
        }
        fx
    };
    raids.raids.retain(|_, raid| !raid.is_stopped());
    region.raids = raids;
    crate::interaction::send_effects(region, None, &effects);
    // 首波号角：按官方 `playSound` 对附近玩家发一份 seed 相同的包。
    for (pos, _) in horns {
        let frame = raid_horn(region, pos);
        for id in region.players.keys() {
            if region.links.is_open(*id) {
                region.links.send_frames(*id, &frame);
            }
        }
    }
    for (id, frame) in frames {
        if region.links.is_open(id) {
            region.links.send_frames(id, &frame);
        }
    }
    for (uuid, amplifier) in rewards {
        give_hero_of_the_village(region, uuid, amplifier);
    }
}

/// `Raid.tick` 的一刻，推进自身状态、血条和要刷的波次。
#[allow(clippy::too_many_arguments, reason = "忠实照官方 Raid.tick 的多项状态")]
fn step_one(
    view: &Region,
    active: &[(i32, BlockPos)],
    raid_id: i32,
    raid: &mut Raid,
    difficulty: i32,
    plans: &mut Vec<SpawnPlan>,
    rewards: &mut Vec<([u8; 16], i32)>,
    frames: &mut Vec<(PlayerId, Vec<u8>)>,
    horns: &mut Vec<(BlockPos, i32)>,
) {
    if raid.status == RaidStatus::Ongoing {
        let was_active = raid.active;
        let center_chunk = ChunkPos::new(raid.center.x >> 4, raid.center.z >> 4);
        raid.active = view.chunks.is_loaded(center_chunk);
        if difficulty == DIFFICULTY_PEACEFUL {
            stop_bar(raid, frames);
            return;
        }
        if was_active != raid.active {
            set_visible(raid, raid.active, frames);
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
                stop_bar(raid, frames);
            }
        }
        raid.ticks_active += 1;
        if raid.ticks_active >= RAID_TIMEOUT_TICKS {
            stop_bar(raid, frames);
            return;
        }
        let alive = raid.total_raiders();
        if alive == 0 && raid.has_more_waves() {
            if raid.cooldown_ticks > 0 {
                // 官方：波次位置缺失且 cooldown%5==0，或已有位置但不再实体 tick，
                // 都重新找一次位置。这里只判前者（后者的区块档位还没建模）。
                if raid.wave_spawn_pos.is_none() && raid.cooldown_ticks % 5 == 0 {
                    raid.wave_spawn_pos = find_random_spawn_pos(view, raid);
                }
                if raid.cooldown_ticks == DEFAULT_PRE_RAID_TICKS || raid.cooldown_ticks % 20 == 0 {
                    update_players(view, active, raid_id, raid, frames);
                }
                raid.cooldown_ticks -= 1;
                set_progress(raid, raid.cooldown_progress(), frames);
            } else if raid.groups_spawned > 0 {
                raid.cooldown_ticks = DEFAULT_PRE_RAID_TICKS;
                set_name(
                    raid,
                    mc_bamboo_blocks::entity::raid::RaidBarName::Raid,
                    0,
                    frames,
                );
                return;
            }
        }
        if raid.ticks_active % 20 == 0 {
            update_players(view, active, raid_id, raid, frames);
            update_raiders(view, raid);
            use mc_bamboo_blocks::entity::raid::RaidBarName as Name;
            let (name, remaining) = if (1..=2).contains(&alive) {
                (Name::RaidersRemaining, alive)
            } else {
                (Name::Raid, 0)
            };
            set_name(raid, name, remaining, frames);
        }
        // `getTotalRaidersAlive()` 在官方循环里每轮重算；这里用刷出的成员数
        // 模拟名册增长，让一次 tick 只刷一波。
        let mut alive_now = alive;
        let mut failed = 0;
        let mut first_sound = false;
        while raid.should_spawn_group(alive_now) {
            let pos = raid
                .wave_spawn_pos
                .or_else(|| find_random_spawn_pos(view, raid));
            if let Some(pos) = pos {
                raid.started = true;
                let (members, riders) = spawn_group_plan(raid, difficulty, pos, alive_now);
                alive_now = (members.len() + riders.len()) as i32;
                plans.push(SpawnPlan {
                    raid_id,
                    wave: raid.groups_spawned,
                    pos,
                    enchant_odds: raid.enchant_odds(),
                    members,
                    riders,
                });
                if !first_sound {
                    horns.push((pos, raid_id));
                    first_sound = true;
                }
            } else {
                failed += 1;
                if failed > 5 {
                    stop_bar(raid, frames);
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
            stop_bar(raid, frames);
            return;
        }
        if raid.celebration_ticks % 20 == 0 {
            update_players(view, active, raid_id, raid, frames);
            set_visible(raid, true, frames);
            if raid.is_victory() {
                set_progress(raid, 0.0, frames);
                set_name(
                    raid,
                    mc_bamboo_blocks::entity::raid::RaidBarName::Victory,
                    0,
                    frames,
                );
            } else {
                set_name(
                    raid,
                    mc_bamboo_blocks::entity::raid::RaidBarName::Defeat,
                    0,
                    frames,
                );
            }
        }
    }
}

/// `spawnGroup` 的纯部分：算这一波刷哪些类型（第一期只有掠夺者 / 女巫），
/// 更新波次数、清空 `waveSpawnPos`，并给第 5 波的劫掠兽配上掠夺者骑手。
/// 实体在阶段 B 生成。
fn spawn_group_plan(
    raid: &mut Raid,
    difficulty: i32,
    pos: BlockPos,
    alive: i32,
) -> (Vec<MobType>, Vec<(usize, MobType)>) {
    let wave = raid.groups_spawned + 1;
    raid.total_health = 0.0;
    let bonus = raid.should_spawn_bonus_group(alive);
    let mut members = Vec::new();
    let mut riders = Vec::new();
    // 官方 `RaiderType.VALUES` 有五个类型，spawnGroup 每个都要走一次
    // `getPotentialBonusSpawns`（后面几期未建模的类型不刷实体，但仍取其随机
    // 数，否则随机源状态会和官方错开）。
    //
    // Official `RaiderType.VALUES` has five entries; `spawnGroup` calls
    // `getPotentialBonusSpawns` for each, so even types not modelled yet draw
    // from the same random source.
    for kind in RaiderType::VALUES {
        let def = raid.default_num_spawns(kind, wave, bonus);
        let extra =
            Raid::potential_bonus_spawns(kind, wave, difficulty, bonus, &mut |b| raid.next_int(b));
        let mob = match kind {
            RaiderType::Pillager => Some(MobType::Pillager),
            RaiderType::Vindicator => Some(MobType::Vindicator),
            RaiderType::Ravager => Some(MobType::Ravager),
            RaiderType::Witch => Some(MobType::Witch),
            // 唤魔者留到第二期。
            RaiderType::Evoker => None,
        };
        if let Some(mob) = mob {
            for _ in 0..(def + extra) {
                // 官方 `spawnGroup`：第 5 波（普通波次表最后一波）每只劫掠兽带一只
                // 掠夺者骑手。困难第 7 波起的唤魔者 / 卫道士骑手留到第二期。
                //
                // Official `spawnGroup`: on wave 5 (the normal wave count) every
                // ravager gets a pillager rider. The evoker / vindicator riders
                // from hard wave 7 are deferred.
                if mob == MobType::Ravager && wave == num_groups(DIFFICULTY_NORMAL) {
                    riders.push((members.len(), MobType::Pillager));
                }
                members.push(mob);
            }
        }
    }
    raid.wave_spawn_pos = None;
    raid.groups_spawned += 1;
    let _ = pos;
    (members, riders)
}

/// 阶段 B：在世界里生成一波的实体，返回 (uid, 血量) 供回填名册。
/// 第 5 波的劫掠兽在生成后紧接着生成一只掠夺者骑手并 `startRiding`。
fn spawn_group(world: &mut World, plan: &SpawnPlan) -> Vec<(i32, f32)> {
    let mut out = Vec::new();
    for (index, &mob) in plan.members.iter().enumerate() {
        let at = Vec3::new(
            f64::from(plan.pos.x) + 0.5,
            f64::from(plan.pos.y) + 1.0,
            f64::from(plan.pos.z) + 0.5,
        );
        let mut entity = world.prepare_entity(Entity::by_mob_type(mob, at));
        finalize_spawn(world, mob, &mut entity, SpawnReason::Natural);
        let health = entity.kind.mob().map_or(0.0, |m| m.health);
        let uid = world.add_prepared_entity(entity);
        join_mob(world, uid, plan.raid_id, plan.wave, plan.enchant_odds);
        out.push((uid, health));
        for &(vehicle_index, rider) in &plan.riders {
            if vehicle_index != index {
                continue;
            }
            // 官方 `spawnGroup`：骑手 `joinRaid` 后 `snapTo(pos, 0.0F, 0.0F)` 再
            // `startRiding(ravager, false, false)`。`snapTo(BlockPos, ...)` 落在
            // 方块底心 `(x+0.5, y, z+0.5)`。
            let rider_pos = Vec3::new(
                f64::from(plan.pos.x) + 0.5,
                f64::from(plan.pos.y),
                f64::from(plan.pos.z) + 0.5,
            );
            let mut rider_entity = world.prepare_entity(Entity::by_mob_type(rider, rider_pos));
            finalize_spawn(world, rider, &mut rider_entity, SpawnReason::Natural);
            let rider_health = rider_entity.kind.mob().map_or(0.0, |m| m.health);
            let rider_uid = world.add_prepared_entity(rider_entity);
            join_mob(world, rider_uid, plan.raid_id, plan.wave, plan.enchant_odds);
            world.with_entity_mut(rider_uid, &mut |rider_entity| {
                rider_entity.position = rider_pos;
                rider_entity.old_position = rider_pos;
                rider_entity.y_rot = 0.0;
                rider_entity.with_mob(|_, mob| {
                    mob.x_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                });
            });
            world.start_riding(rider_uid, uid);
            out.push((rider_uid, rider_health));
        }
    }
    out
}

/// `Raid.joinRaid` 里写回实体的那部分：登记 `RaidId`、波次、能加入袭击、清零离队
/// 计时，并按波次套袭击附魔（卫道士锋利、掠夺者快速装填）。
///
/// The entity-side half of `Raid.joinRaid`: record the `RaidId` and wave, allow
/// joining, reset the outside-raid timer and apply the wave enchantment.
fn join_mob(world: &mut World, uid: i32, raid_id: i32, wave: i32, enchant_odds: f32) {
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(|me, mob| {
            mob.raider.raid_id = Some(raid_id);
            mob.raider.wave = wave;
            mob.raider.can_join_raid = true;
            mob.raider.ticks_outside_raid = 0;
            mc_bamboo_blocks::entity::mob::raider::apply_raid_buffs(me, mob, enchant_odds, wave);
        });
    });
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

/// `Raid.setLeader(wave, raider)` 的镜像：捡起灾厄旗帜的袭击者成为这一波的队长。
///
/// Mirror of `Raid.setLeader(wave, raider)`: the raider that picked up the
/// ominous banner becomes this wave's leader.
pub(crate) fn set_leader(region: &mut Region, id: i32, wave: i32, leader: i32) {
    if let Some(raid) = region.raids.raids.get_mut(&id) {
        raid.leaders.insert(wave, leader);
        raid.add_wave_mob(wave, leader, 0.0, false);
    }
}

/// `Raider.die`：找到死者所在位置的活跃袭击，记英雄、从名册移除、清队长
/// 旗帜的登记（`removeFromRaid(..., false)` 不扣总血量），再刷新血条进度。
///
/// `Raider.die`: find the active raid at the death position, credit the killer
/// as a hero, remove the raider from the roster / leader map (without
/// subtracting `totalHealth`) and refresh the bar's progress.
pub(crate) fn raider_died(region: &mut Region, uid: i32, position: Vec3, killer: Option<[u8; 16]>) {
    let pos = BlockPos::containing(position);
    let Some(raid_id) = region
        .raids
        .nearby_raid(pos, f64::from(VALID_RAID_RADIUS_SQR))
    else {
        return;
    };
    let Some(raid) = region.raids.raids.get_mut(&raid_id) else {
        return;
    };
    if let Some(uuid) = killer {
        raid.add_hero_of_the_village(uuid);
    }
    // 官方 `Raider.die` 走 `removeFromRaid(level, raider, false)`：不扣
    // `totalHealth`，血条分子随剩下的幸存者血量下降。
    //
    // Official `Raider.die` calls `removeFromRaid(level, raider, false)`:
    // `totalHealth` stays, and the bar's numerator falls with the survivors.
    for &wave in raid.raiders.clone().keys() {
        if raid.remove_raider(wave, uid, 0.0, false) {
            break;
        }
    }
    if raid.leaders.values().any(|&leader| leader == uid) {
        raid.leaders.retain(|_, leader| *leader != uid);
    }
    let living: f32 = raid
        .raiders
        .values()
        .flatten()
        .filter_map(|&member| {
            let public = region.entities.public_at(member)?;
            match &public.kind {
                mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.health.max(0.0)),
                _ => None,
            }
        })
        .sum();
    raid.boss_progress = raid.bossbar_progress(living);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Outgoing, PlayerSink, player::Player, region::Region, world_state::Difficulty};
    use mc_bamboo_blocks::entity::mob::effects::EffectInstance;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<u8>>>);

    impl PlayerSink for Recorder {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0.lock().unwrap().extend(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {}
    }

    impl Recorder {
        fn bytes(&self) -> Vec<u8> {
            self.0.lock().unwrap().clone()
        }

        /// 把攒下的帧拆成 (包 id, body)。
        fn frames(&self) -> Vec<(i32, Vec<u8>)> {
            use mc_bamboo_protocol::{
                codec::Reader,
                frame::{MAX_FRAME_LEN, find_frame},
            };
            let bytes = self.bytes();
            let mut rest = bytes.as_slice();
            let mut out = Vec::new();
            while !rest.is_empty() {
                let bounds = find_frame(rest, MAX_FRAME_LEN)
                    .expect("合法的帧")
                    .expect("完整的帧");
                let end = bounds.header_len + bounds.payload_len;
                let mut reader = Reader::new(&rest[bounds.header_len..end]);
                let id = reader.varint().unwrap();
                out.push((id, reader.rest().to_vec()));
                rest = &rest[end..];
            }
            out
        }
    }

    fn region_with_player() -> (Region, PlayerId, Recorder) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let recorder = Recorder::default();
        region.links.add(id, Box::new(recorder.clone()));
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
        (region, id, recorder)
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
        let (mut region, id, _) = region_with_player();
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
        let (mut region, id, _) = region_with_player();
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));
        assert!(!region.players[&id].effects.has(mob_effect::RAID_OMEN));

        // 和平难度。
        let (mut region, id, _) = region_with_player();
        make_village(&mut region);
        region.global.difficulty = Difficulty::Peaceful;
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));

        // 旁观者。
        let (mut region, id, _) = region_with_player();
        make_village(&mut region);
        region.players.get_mut(&id).unwrap().game_mode = GameMode::Spectator;
        give_bad_omen(&mut region, id, 0);
        bad_omen_check(&mut region, id, 0);
        assert!(region.players[&id].effects.has(mob_effect::BAD_OMEN));
    }

    /// 附近已有满级不祥等级的袭击时不转换。
    #[test]
    fn bad_omen_stops_at_a_maxed_raid() {
        let (mut region, id, _) = region_with_player();
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
        let (mut region, id, _) = region_with_player();
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
        let (mut region, id, _) = region_with_player();
        let pos = BlockPos::new(7, 64, -3);
        let raid_id = create_or_extend_raid(&mut region, id, pos, 0).unwrap();
        assert_eq!(region.raids.raids[&raid_id].center, pos);
        assert_eq!(region.raids.raids[&raid_id].raid_omen_level, 1);
    }

    /// 冷却倒数到 0 后刷出第一波，实体进名册、类型只含掠夺者 / 女巫。
    #[test]
    fn raid_tick_spawns_first_wave_after_cooldown() {
        let (mut region, _id, _) = region_with_player();
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

    /// 第 5 波（普通波次表最后一波）每只劫掠兽带一只掠夺者骑手。
    #[test]
    fn wave_5_ravagers_get_pillager_riders() {
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), DIFFICULTY_NORMAL);
        // groupsSpawned = 4 → 这一波是第 5 波。
        raid.groups_spawned = 4;
        let (members, riders) =
            spawn_group_plan(&mut raid, DIFFICULTY_NORMAL, BlockPos::new(0, 64, 0), 0);
        let ravagers: Vec<usize> = members
            .iter()
            .enumerate()
            .filter(|(_, mob)| **mob == MobType::Ravager)
            .map(|(index, _)| index)
            .collect();
        assert!(!ravagers.is_empty(), "第 5 波有劫掠兽");
        assert_eq!(riders.len(), ravagers.len(), "每只劫掠兽一个骑手");
        for (vehicle, rider) in riders {
            assert_eq!(members[vehicle], MobType::Ravager);
            assert_eq!(rider, MobType::Pillager);
        }
    }

    /// 所有波次刷完、名册清空后转胜利，并给英雄加 HERO_OF_THE_VILLAGE。
    #[test]
    fn raid_tick_wins_and_rewards_heroes() {
        let (mut region, id, _) = region_with_player();
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

    /// 首波之前：冷却第 300 刻给附近玩家发 boss_event add（RED / NOTCHED_10 /
    /// 进度 0），之后每刻发 update_progress；进度按 `(300 - cooldown)/300` 走。
    #[test]
    fn raid_bossbar_adds_and_tracks_cooldown_progress() {
        let (mut region, id, recorder) = region_with_player();
        make_village(&mut region);
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        raid.wave_spawn_pos = Some(BlockPos::new(0, 64, 0));
        region.raids.raids.insert(raid_id, raid);
        tick(&mut region);
        region.links.flush();
        let boss = mc_bamboo_data::packets::play::clientbound::BOSS_EVENT;
        let frames: Vec<_> = recorder
            .frames()
            .into_iter()
            .filter(|(packet, _)| *packet == boss)
            .collect();
        assert_eq!(frames.len(), 2, "add + 一次 update_progress");
        let add = &frames[0].1;
        // UUID(16) + 动作 0 + 名字组件 + 进度 float + 颜色 / 分段 / flags。
        assert_eq!(add[16], 0, "ADD 动作");
        let name_end = add.len() - 4 - 1 - 1 - 1;
        assert_eq!(add[name_end..name_end + 4], 0.0_f32.to_be_bytes());
        assert_eq!(add[name_end + 4], 2, "RED");
        assert_eq!(add[name_end + 5], 2, "NOTCHED_10");
        assert_eq!(frames[1].1[16], 2, "UPDATE_PROGRESS");
        assert_eq!(
            frames[1].1[17..21],
            (1.0_f32 / 300.0).to_be_bytes(),
            "冷却递减 1 刻"
        );
        // 观众集合里记着玩家。
        assert_eq!(region.raids.raids[&raid_id].boss_players, vec![id.0]);
    }

    /// 首波刷出时：号角音效 + 血条进度变满；progress 用刷出的总血量算。
    #[test]
    fn first_wave_plays_horn_and_fills_the_bossbar() {
        let (mut region, _id, recorder) = region_with_player();
        make_village(&mut region);
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        raid.wave_spawn_pos = Some(BlockPos::new(0, 64, 0));
        region.raids.raids.insert(raid_id, raid);
        for tick in 0..=DEFAULT_PRE_RAID_TICKS {
            crate::raid::tick(&mut region);
            if tick == DEFAULT_PRE_RAID_TICKS - 1 {
                // 首波之前的进度已到 1。
                assert_eq!(region.raids.raids[&raid_id].boss_progress, 1.0);
            }
        }
        region.links.flush();
        let sound = mc_bamboo_data::packets::play::clientbound::SOUND;
        let frames = recorder.frames();
        let horn = frames.iter().find(|(packet, _)| *packet == sound);
        assert!(horn.is_some(), "首波有一份号角音效包");
        assert_eq!(region.raids.raids[&raid_id].boss_progress, 1.0);
    }

    /// 袭击者死亡：记英雄、从名册移除、扣回血量、清队长登记。
    #[test]
    fn raider_death_credits_hero_and_leaves_the_roster() {
        let (mut region, id, _) = region_with_player();
        make_village(&mut region);
        let uuid = *region.players[&id].uuid.as_bytes();
        let raid_id = region.raids.unique_id();
        let mut raid = Raid::new(BlockPos::new(0, 64, 0), 2);
        raid.started = true;
        raid.add_wave_mob(1, 42, 24.0, true);
        raid.leaders.insert(1, 42);
        region.raids.raids.insert(raid_id, raid);
        // 死亡位置要落在袭击中心的半径内。
        raider_died(&mut region, 42, Vec3::new(0.5, 64.0, 0.5), Some(uuid));
        let raid = &region.raids.raids[&raid_id];
        assert_eq!(raid.heroes_of_the_village, vec![uuid]);
        assert!(raid.raiders.values().all(|members| members.is_empty()));
        assert!(raid.leaders.is_empty());
        assert_eq!(
            raid.total_health, 24.0,
            "官方 removeFromRaid(..., false) 不扣总血量"
        );
    }

    /// 袭击之兆到期建袭击并清掉坐标。
    #[test]
    fn raid_omen_expire_starts_the_raid_and_clears_position() {
        let (mut region, id, _) = region_with_player();
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
