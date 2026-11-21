//! 美西螈：官方 `Axolotl`（`Animal` 一支，`Bucketable`）。
//!
//! 美西螈用 Brain（`AxolotlAi`）驱动，没有 `goalSelector`。本轮把 Brain 相关的
//! 行为（传感器、记忆、活动、近战、装死以外的移动）留作已知限制，只实现玩家能直接
//! 感知的部分：五个花色及生成时的抽取、装桶标记、在水里/离水的氧气与干死、
//! 装死状态、水里外不同的叫声、两栖寻路起点和 `travelInWater`。
//!
//! The axolotl is Brain-driven (`AxolotlAi`) and has no `goalSelector`. This round
//! leaves the Brain behaviours (sensors, memories, activities, melee, play-dead-driven
//! movement) as a documented limitation and models only what players directly see:
//! the five variants and their spawn rolls, the bucket flag, in/out-of-water air and
//! drying out, the playing-dead flag, water/land ambience, amphibious pathfinding and
//! `travelInWater`.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::Attributes,
    goals::GoalSelector,
    living, motion,
    path::Malus,
    spawn::{SpawnGroup, SpawnReason},
    water,
};
use crate::{
    entity::{Entity, ctx::EntityCtx},
    world::World,
};

/// `Axolotl.Variant`：0 lucy、1 wild、2 gold、3 cyan、4 blue（后者是稀有）。
///
/// `Axolotl.Variant`: 0 lucy, 1 wild, 2 gold, 3 cyan, 4 blue (the rare one).
pub(super) const COMMON_VARIANTS: [i32; 4] = [0, 1, 2, 3];
pub(super) const DEFAULT_VARIANT: i32 = 0;

/// 美西螈的专属状态：花色、装死和装桶标记。
///
/// Axolotl-only state: variant, playing dead and from-bucket flags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxolotlState {
    pub variant: i32,
    pub playing_dead: bool,
    pub from_bucket: bool,
    /// `MemoryModuleType.PLAY_DEAD_TICKS`（Brain 记忆，装死倒计时）。
    ///
    /// `MemoryModuleType.PLAY_DEAD_TICKS` (Brain memory, the play-dead countdown).
    pub play_dead_ticks: i32,
}

impl AxolotlState {
    pub const fn new() -> Self {
        Self {
            variant: DEFAULT_VARIANT,
            playing_dead: false,
            from_bucket: false,
            play_dead_ticks: 0,
        }
    }
}

impl Default for AxolotlState {
    fn default() -> Self {
        Self::new()
    }
}

/// 成年 0.75×0.42、眼高 0.2751；幼年 `BABY_DIMENSIONS` = 0.375×0.21、眼高 0.09375。
///
/// Adult 0.75×0.42, eye 0.2751; baby `BABY_DIMENSIONS` 0.375×0.21, eye 0.09375.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Axolotl.entity_type() as usize];
    if baby {
        (0.375, 0.21, 0.09375)
    } else {
        (facts.width, facts.height, 0.2751)
    }
}

/// `Axolotl.createAttributes`：`Animal.createAnimalAttributes()` 加 14 血、1.0 移速、
/// 2 攻击、1.0 台阶高度。
///
/// `Axolotl.createAttributes`: `Animal.createAnimalAttributes()` plus 14 health,
/// 1.0 speed, 2 attack and 1.0 step height.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 14.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, 1.0);
    attributes.set_base(attribute::STEP_HEIGHT, 1.0);
    attributes.entries.push(super::attributes::Attribute::new(
        attribute::ATTACK_DAMAGE,
        2.0,
    ));
    attributes
}

/// 构造函数 `setPathfindingMalus(PathType.WATER, 0.0F)`。
///
/// The constructor's `setPathfindingMalus(PathType.WATER, 0.0F)`.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(mc_bamboo_data::PathType::Water, 0.0);
    malus
}

/// 美西螈是 Brain 驱动的，`Mob.registerGoals()` 保持空实现。
///
/// The axolotl is Brain-driven, so `Mob.registerGoals()` stays empty.
pub(super) fn goals() -> GoalSelector {
    GoalSelector::default()
}

/// `Axolotl.finalizeSpawn`：`BUCKET` 原因直接返回；否则从世界随机源抽两个常见花色
/// 组成组数据，再抽一个作为这只的花色；一组第三只起是幼崽。
/// `AgeableMob.finalizeSpawn` 的 `shouldSpawnBaby` 为假（不抽幼崽概率），
/// `Mob.finalizeSpawn` 的跟随距离加成和左撇子照旧。
///
/// `Axolotl.finalizeSpawn`: `BUCKET` returns immediately; otherwise draw two common
/// variants into the group data, draw one as this mob's variant, and make the third and
/// later members of a group babies. `AgeableMob.finalizeSpawn` has `shouldSpawnBaby`
/// false (no baby roll); `Mob.finalizeSpawn` still adds the follow-range bonus and the
/// left-handed roll.
pub(super) fn finalize_spawn(
    world: &mut World,
    entity: &mut Entity,
    reason: SpawnReason,
    group: &mut Option<SpawnGroup>,
) {
    if reason == SpawnReason::Bucket {
        return;
    }
    let random = world.random();
    let (types, is_baby, existed) = match group {
        Some(SpawnGroup::Axolotl { types, group_size }) => (*types, *group_size >= 2, true),
        _ => {
            let a = COMMON_VARIANTS[random.next_int(4) as usize];
            let b = COMMON_VARIANTS[random.next_int(4) as usize];
            ([a, b], false, false)
        }
    };
    let variant = types[random.next_int(2) as usize];
    entity.with_mob(|me, mob| {
        mob.axolotl.variant = variant;
        if is_baby {
            living::set_age(me, mob, -24000);
        }
        // `AgeableMob.finalizeSpawn`：`shouldSpawnBaby` 为假，不取随机数；`group_size++`。
        // `Mob.finalizeSpawn`：triangle 和 nextFloat。
        // `AgeableMob.finalizeSpawn`: no roll (shouldSpawnBaby false); group_size++.
        // `Mob.finalizeSpawn`: triangle and nextFloat.
        water::finalize_mob(mob, random);
    });
    if existed {
        if let Some(SpawnGroup::Axolotl { group_size, .. }) = group {
            *group_size += 1;
        }
    } else {
        *group = Some(SpawnGroup::Axolotl {
            types,
            group_size: 1,
        });
    }
}

/// `Axolotl.getAmbientSound`：`idle_water` / `idle_air`；装死时不叫
/// （`Axolotl.playAmbientSound` 直接不调 `super`）。
///
/// `Axolotl.getAmbientSound`: `idle_water` / `idle_air`; silent while playing dead
/// (`Axolotl.playAmbientSound` skips `super`).
pub(super) const fn ambient_voice(playing_dead: bool, in_water: bool) -> Option<&'static str> {
    if playing_dead {
        return None;
    }
    Some(if in_water {
        "minecraft:entity.axolotl.idle_water"
    } else {
        "minecraft:entity.axolotl.idle_air"
    })
}

/// `Axolotl.handleAirSupply`（在 `super.baseTick()` 之后，用这一刻开始时的氧气算）：
/// 在水里或雨里回满 6000；否则 `preTick - 1`，到 `shouldTakeDrowningDamage`
/// （`air <= -20`）归零并受 2 点 `dry_out` 伤害。
///
/// `Axolotl.handleAirSupply` (after `super.baseTick()`, with the tick-start air): in
/// water or rain refill to 6000; otherwise `preTick - 1`, and at
/// `shouldTakeDrowningDamage` (`air <= -20`) zero it and take two `dry_out` damage.
pub(super) fn handle_air_supply(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    pre_tick_air: i32,
) {
    if living::is_alive(me, mob) && !in_water_or_rain(me, ctx) {
        mob.air_supply = pre_tick_air - 1;
        if mob.air_supply <= -20 {
            mob.air_supply = 0;
            living::hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::DryOut),
                2.0,
            );
        }
    } else {
        mob.air_supply = MobType::Axolotl.max_air_supply();
    }
}

/// `Entity.isInWaterOrRain()`：在水里或脚下在下雨。
///
/// `Entity.isInWaterOrRain()`: in water, or raining at the feet.
fn in_water_or_rain(me: &Entity, ctx: &dyn EntityCtx) -> bool {
    me.in_water || ctx.blocks().is_raining_at(me.block_position())
}

/// `Axolotl.travelInWater`：只有 `moveRelative` + `move(SELF)` + ×0.9，没有重力项。
///
/// `Axolotl.travelInWater`: just `moveRelative` + `move(SELF)` + ×0.9, no gravity.
pub(super) fn travel_in_water(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
) {
    living::move_relative(me, mob.speed, input);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
}

/// `Axolotl.hurtServer` 的装死判定（在 `super.hurtServer` 之前）：
/// 用实体随机源做短路求值，命中就把 `PLAY_DEAD_TICKS` 设成 200。
///
/// The play-dead check of `Axolotl.hurtServer` (before `super.hurtServer`): a
/// short-circuit roll on the entity random; on a hit it sets `PLAY_DEAD_TICKS` to 200.
pub(super) fn maybe_play_dead(me: &mut Entity, mob: &mut Mob, damage: f32, has_source: bool) {
    if mob.no_ai {
        return;
    }
    if me.random.next_int_bounded(3) != 0 {
        return;
    }
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    let low = (me.random.next_int_bounded(3) as f32) < damage || mob.health / max < 0.5;
    if low && damage < mob.health && me.in_water && has_source && !mob.axolotl.playing_dead {
        mob.axolotl.play_dead_ticks = 200;
    }
}

/// `Axolotl.customServerAiStep` 尾部：把 `PLAY_DEAD_TICKS` 映射成元数据 `playingDead`。
///
/// The tail of `Axolotl.customServerAiStep`: map `PLAY_DEAD_TICKS` to the `playingDead`
/// metadata.
pub(super) fn update_playing_dead(mob: &mut Mob) {
    if !mob.no_ai {
        mob.axolotl.playing_dead = mob.axolotl.play_dead_ticks > 0;
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::{archive::tag, entity::mob::nbt};

    #[test]
    fn axolotl_variant_and_bucket_flag_survive_a_save_and_load() {
        let mut entity = Entity::by_mob_type(MobType::Axolotl, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.axolotl.variant = 3;
            mob.axolotl.from_bucket = true;
        });
        let saved = nbt::write_mob(&entity.split()).expect("an axolotl saves");
        assert_eq!(tag::int_or(&saved, "Variant", -1), 3);
        assert!(tag::bool_or(&saved, "FromBucket", false));
        let row = nbt::read_mob(&saved, "minecraft:axolotl", 7)
            .expect("the tag loads")
            .expect("axolotls are modelled");
        let loaded = row.into_entity();
        let mob = loaded.kind.mob().expect("a mob");
        assert_eq!(mob.axolotl.variant, 3);
        assert!(mob.axolotl.from_bucket);
        assert_eq!(mob.air_supply, 6000);
    }

    #[test]
    fn axolotl_is_an_animal_that_breathes_under_water() {
        let axolotl = Mob::new(MobType::Axolotl);
        assert_eq!(axolotl.health, 14.0);
        assert!(MobType::Axolotl.can_breathe_under_water());
        assert_eq!(MobType::Axolotl.max_air_supply(), 6000);
        assert!(MobType::Axolotl.is_animal());
        assert_eq!(
            MobType::Axolotl.mob_category(),
            mc_bamboo_data::spawning::MobCategory::Axolotls
        );
        assert_eq!(MobType::Axolotl.increase_air_supply(10), 14);
    }
}
