//! 袭击系统的世界交互层：官方 `Raids`、`BadOmenMobEffect`、`RaidOmenMobEffect`
//! 里需要读写世界的那部分。纯数据、波次表和不碰世界的状态判定在
//! `mc_bamboo_blocks::entity::raid`，这里只编排 `Region`。
//!
//! The world-interaction half of raids: the parts of vanilla `Raids`,
//! `BadOmenMobEffect` and `RaidOmenMobEffect` that touch the world. Pure data,
//! wave tables and world-free predicates live in `mc_bamboo_blocks::entity::raid`;
//! this module only drives a `Region`.

use mc_bamboo_blocks::entity::{
    mob::effects::EffectInstance,
    raid::{DIFFICULTY_PEACEFUL, MAX_RAID_OMEN_LEVEL, Raid, VALID_RAID_RADIUS_SQR},
};
use mc_bamboo_core::{BlockPos, Vec3};
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
