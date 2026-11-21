//! 水生生物共用的部分：官方 `AgeableWaterCreature` 的氧气处理和水面生成规则，
//! `SpawnPlacementTypes.IN_WATER`，以及不带花色的 `AgeableMob.finalizeSpawn`。
//! 鱿鱼和发光鱿鱼先用，以后的海豚、鱼类接着用。
//!
//! Shared aquatic pieces: vanilla `AgeableWaterCreature` air handling and surface spawn
//! rules, `SpawnPlacementTypes.IN_WATER`, and the variant-free `AgeableMob.finalizeSpawn`.
//! Squid and glow squid use them first; dolphins and fish can follow.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, registries::attribute, tags::fluid_tags};

use super::{
    DamageKind, DamageSource, Mob,
    attributes::{Modifier, Operation},
    living,
    spawn::{AgeableGroupData, RANDOM_SPAWN_BONUS, SpawnGroup},
};
use crate::{
    entity::{
        Entity,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
    world::World,
};

/// `AgeableMob.getBabyStartAge()`。
const BABY_START_AGE: i32 = -24000;

/// `AgeableWaterCreature.handleAirSupply`：在 `super.baseTick()` 之后用这一刻开始时的氧气算。
/// 离水每刻减 1，到 -20 归零并受 2 点溺水伤害；在水里（或已死）直接回满。
///
/// `AgeableWaterCreature.handleAirSupply`, run after `super.baseTick()` with the air the
/// tick started with: out of water it drops by one and deals two drowning damage at -20;
/// in water (or once dead) it refills.
pub(super) fn handle_air_supply(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    pre_tick_air: i32,
) {
    if living::is_alive(me, mob) && !me.in_water {
        mob.air_supply = pre_tick_air - 1;
        if mob.air_supply <= -20 {
            mob.air_supply = 0;
            let source = DamageSource::environment(DamageKind::Drown);
            living::hurt_server(me, mob, ctx, source, 2.0);
        }
    } else {
        mob.air_supply = 300;
    }
}

/// `AgeableWaterCreature.checkSurfaceAgeableWaterCreatureSpawnRules`：海平面下 13 格以内，
/// 下面是水（流体标签），上面是水方块。
///
/// `checkSurfaceAgeableWaterCreatureSpawnRules`: within thirteen blocks below sea level,
/// water fluid below and a water block above.
pub(super) fn surface_spawn_rules(view: &dyn BlockView, sea_level: i32, pos: BlockPos) -> bool {
    pos.y >= sea_level - 13
        && pos.y <= sea_level
        && view.fluid_state(pos.below()).is_tagged(fluid_tags::WATER)
        && view.block_state(pos.above()).is(blocks::WATER)
}

/// `SpawnPlacementTypes.IN_WATER.isSpawnPositionOk`：边界以内，这一格是水，上一格不是
/// 红石导体。
///
/// `SpawnPlacementTypes.IN_WATER.isSpawnPositionOk`: inside the border, water at the
/// position and no redstone conductor above.
pub(super) fn in_water_position_ok(world: &World, pos: BlockPos) -> bool {
    world.world_border().is_within_bounds_pos(pos)
        && world.fluid_state(pos).is_tagged(fluid_tags::WATER)
        && !world.block_state(pos.above()).is_redstone_conductor()
}

/// `AgeableMob.finalizeSpawn` 加 `Mob.finalizeSpawn`，没有物种自己的花色那一层。
/// `baby_chance` 是组数据为空时新建的 `AgeableMobGroupData` 的幼年概率
/// （鱿鱼传 0.05）。
///
/// `AgeableMob.finalizeSpawn` plus `Mob.finalizeSpawn` without a species layer.
/// `baby_chance` seeds a fresh `AgeableMobGroupData` when there is no group yet
/// (squid pass 0.05).
pub(super) fn finalize_ageable(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
    baby_chance: f32,
) {
    if !matches!(group, Some(SpawnGroup::Ageable(_))) {
        *group = Some(SpawnGroup::Ageable(AgeableGroupData {
            group_size: 0,
            should_spawn_baby: true,
            baby_spawn_chance: baby_chance,
        }));
    }
    let Some(SpawnGroup::Ageable(data)) = group else {
        unreachable!("the ageable group data was just stored");
    };
    let baby = data.should_spawn_baby
        && data.group_size > 0
        && random.next_float() <= data.baby_spawn_chance;
    data.group_size += 1;
    entity.with_mob(|me, mob| {
        if baby {
            living::set_age(me, mob, BABY_START_AGE);
        }
        finalize_mob(mob, random);
    });
}

/// `Mob.finalizeSpawn`：跟随距离的随机加成（没有时才加），再抽左撇子。
///
/// `Mob.finalizeSpawn`: the random follow-range bonus (only when absent), then the
/// left-handed roll.
pub(super) fn finalize_mob(mob: &mut Mob, random: &dyn RandomSource) {
    if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
        && range.modifier(RANDOM_SPAWN_BONUS).is_none()
    {
        let amount = random.triangle(0.0, 0.114_850_000_000_000_01);
        range.modifiers.push(Modifier {
            id: RANDOM_SPAWN_BONUS.to_owned(),
            amount,
            operation: Operation::AddMultipliedBase,
            permanent: true,
        });
    }
    mob.left_handed = random.next_float() < 0.05;
}
