//! 美西螈：官方 `Axolotl`（`Animal` 一支，`Bucketable`）。
//!
//! 美西螈用 Brain（`AxolotlAi`）驱动，没有 `goalSelector`。Brain 的传感器、记忆、
//! 活动、近战和装死以外的移动仍留作已知限制；玩家能直接感知的部分照官方实现：
//! 五个花色及生成时的抽取、装桶标记、在水里/离水的氧气与干死、装死状态
//! （`ValidatePlayDead` 逐刻递减、`PlayDead` 的 200 刻 `REGENERATION`）、
//! 水里外不同的叫声、两栖寻路起点和 `travelInWater`。
//!
//! The axolotl is Brain-driven (`AxolotlAi`) and has no `goalSelector`. Brain
//! sensors, memories, activities, melee and play-dead-driven movement remain a
//! documented limitation; what players directly see follows vanilla: the five
//! variants and their spawn rolls, the bucket flag, in/out-of-water air and drying
//! out, playing dead (the `ValidatePlayDead` countdown and `PlayDead`'s 200-tick
//! `REGENERATION`), water/land ambience, amphibious pathfinding and `travelInWater`.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, registries::attribute};

mod ai;

pub use ai::Behavior;
pub(super) use ai::{custom_server_ai_step, sense_attackables};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::Attributes,
    brain::{Brain, memory::MemoryType, memory::MemoryValue},
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
const RARE_VARIANT: i32 = 4;
/// `Axolotl.RARE_VARIANT_CHANCE`。
///
/// `Axolotl.RARE_VARIANT_CHANCE`.
const RARE_VARIANT_CHANCE: i32 = 1200;

/// 美西螈的专属状态：花色和装桶标记。装死倒计时是大脑记忆 `PLAY_DEAD_TICKS`。
///
/// Axolotl-only state: variant and the from-bucket flag. The play-dead countdown is the
/// `PLAY_DEAD_TICKS` brain memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxolotlState {
    pub variant: i32,
    pub playing_dead: bool,
    pub from_bucket: bool,
}

impl AxolotlState {
    pub const fn new() -> Self {
        Self {
            variant: DEFAULT_VARIANT,
            playing_dead: false,
            from_bucket: false,
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

/// `Axolotl.getBreedOffspring` 的花色（幼崽构造之后）：先 `nextInt(1200)` 赌稀有的
/// 蓝色，命中时 `getRareSpawnVariant` 再从只含蓝色的名单里抽一次（`nextInt(1)`）；
/// 没命中就用 `nextBoolean` 取父亲或母亲的花色。两条路径都恰好取两次随机数。
///
/// The variant part of `Axolotl.getBreedOffspring` (after the baby is constructed):
/// `nextInt(1200)` rolls for the rare blue, whose `getRareSpawnVariant` draws once more
/// from the one-entry rare list (`nextInt(1)`); otherwise one `nextBoolean` picks a
/// parent's variant. Both paths draw exactly twice.
pub fn offspring_variant(random: &mut crate::entity::JavaRandom, mine: i32, partner: i32) -> i32 {
    if random.next_int_bounded(RARE_VARIANT_CHANCE) == 0 {
        // `Util.getRandom` 对单元素数组仍走 `nextInt(length)`。
        // `Util.getRandom` still calls `nextInt(length)` for a one-entry array.
        let _ = random.next_int_bounded(1);
        RARE_VARIANT
    } else if random.next_boolean() {
        mine
    } else {
        partner
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

/// `Axolotl.AxolotlMoveControl`：`SmoothSwimmingMoveControl(this, 85, 10, 0.1F, 0.5F, false)`，
/// 装死时整个跳过（`AxolotlMoveControl.tick` 的覆写）。
///
/// `Axolotl.AxolotlMoveControl`: `SmoothSwimmingMoveControl(this, 85, 10, 0.1F, 0.5F,
/// false)`, skipped entirely while playing dead (the `AxolotlMoveControl.tick` override).
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if mob.axolotl.playing_dead {
        return;
    }
    super::smooth_swim::move_tick(me, mob, &super::smooth_swim::AXOLOTL);
}

/// `Axolotl.AxolotlLookControl`：`SmoothSwimmingLookControl(this, 20)`，装死时不转视角。
///
/// `Axolotl.AxolotlLookControl`: `SmoothSwimmingLookControl(this, 20)`, which does not
/// look around while playing dead.
pub(super) fn look_control_tick(me: &Entity, mob: &mut Mob) {
    if mob.axolotl.playing_dead {
        return;
    }
    super::smooth_swim::look_tick(me, mob, 20.0);
}

/// `Axolotl.hurtServer` 的装死判定（在 `super.hurtServer` 之前）：
/// 用实体随机源做短路求值，命中就把 `PLAY_DEAD_TICKS` 记忆设成 200。
///
/// The play-dead check of `Axolotl.hurtServer` (before `super.hurtServer`): a
/// short-circuit roll on the entity random; on a hit it sets the `PLAY_DEAD_TICKS`
/// memory to 200.
pub(super) fn maybe_play_dead(me: &mut Entity, mob: &mut Mob, damage: f32, has_source: bool) {
    if mob.no_ai {
        return;
    }
    if me.random.next_int_bounded(3) != 0 {
        return;
    }
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    let low = (me.random.next_int_bounded(3) as f32) < damage || mob.health / max < 0.5;
    if low
        && damage < mob.health
        && me.in_water
        && has_source
        && !mob.axolotl.playing_dead
        && let Some(brain) = mob.brain.as_mut()
    {
        brain
            .memories
            .set(MemoryType::PlayDeadTicks, MemoryValue::Int(200));
    }
}

/// `Axolotl.customServerAiStep` 尾部：把 `PLAY_DEAD_TICKS` 记忆映射成元数据
/// `playingDead`（`playDeadTicks.isPresent() && playDeadTicks.get() > 0`）。
///
/// The tail of `Axolotl.customServerAiStep`: map the `PLAY_DEAD_TICKS` memory to the
/// `playingDead` metadata (`playDeadTicks.isPresent() && playDeadTicks.get() > 0`).
pub(super) fn sync_playing_dead(mob: &mut Mob) {
    if mob.no_ai {
        return;
    }
    let ticks = mob
        .brain
        .as_ref()
        .and_then(|brain| brain.memories.get(MemoryType::PlayDeadTicks))
        .and_then(|value| match value {
            MemoryValue::Int(ticks) => Some(*ticks),
            _ => None,
        })
        .unwrap_or(0);
    mob.axolotl.playing_dead = ticks > 0;
}

/// `Axolotl.BRAIN_PROVIDER`。
///
/// `Axolotl.BRAIN_PROVIDER`.
pub(super) fn brain() -> Brain {
    ai::brain()
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

    #[test]
    fn playing_dead_metadata_follows_the_brain_memory() {
        let mut mob = Mob::new(MobType::Axolotl);
        let brain = mob.brain.as_mut().expect("an axolotl has a brain");
        brain
            .memories
            .set(MemoryType::PlayDeadTicks, MemoryValue::Int(200));
        // 记忆大于 0 时元数据是装死；`ValidatePlayDead` 减到 0 之后复位。
        // The metadata reads playing-dead while the memory is above zero and clears once
        // `ValidatePlayDead` brings it to zero.
        sync_playing_dead(&mut mob);
        assert!(mob.axolotl.playing_dead);
        mob.brain
            .as_mut()
            .expect("a brain")
            .memories
            .set(MemoryType::PlayDeadTicks, MemoryValue::Int(0));
        sync_playing_dead(&mut mob);
        assert!(!mob.axolotl.playing_dead);
        // NoAI 的对拍夹具不动元数据。
        // NoAI oracle fixtures leave the metadata alone.
        let mut frozen = Mob::new(MobType::Axolotl);
        frozen.no_ai = true;
        frozen
            .brain
            .as_mut()
            .expect("a brain")
            .memories
            .set(MemoryType::PlayDeadTicks, MemoryValue::Int(200));
        sync_playing_dead(&mut frozen);
        assert!(!frozen.axolotl.playing_dead);
    }

    /// `PLAY_DEAD_TICKS`（`Codec.INT`）和 `HAS_HUNTING_COOLDOWN`（`Codec.BOOL`）进存档，
    /// `ATTACK_COOLING_DOWN` 没有编解码器、不写。
    ///
    /// `PLAY_DEAD_TICKS` (`Codec.INT`) and `HAS_HUNTING_COOLDOWN` (`Codec.BOOL`) are saved;
    /// `ATTACK_COOLING_DOWN` has no codec and is skipped.
    #[test]
    fn play_dead_and_hunting_memories_round_trip() {
        let mut brain = super::brain();
        brain
            .memories
            .set(MemoryType::PlayDeadTicks, MemoryValue::Int(150));
        brain
            .memories
            .set_with_ttl(MemoryType::HuntingCooldown, MemoryValue::Bool(true), 2400);
        brain
            .memories
            .set_with_ttl(MemoryType::AttackCoolingDown, MemoryValue::Bool(true), 20);
        let saved = crate::entity::mob::brain::nbt::write(Some(&brain));
        let mut loaded = super::brain();
        crate::entity::mob::brain::nbt::read(&mut loaded, &saved);
        assert_eq!(
            loaded.memories.get(MemoryType::PlayDeadTicks),
            Some(&MemoryValue::Int(150))
        );
        assert_eq!(
            loaded.memories.get(MemoryType::HuntingCooldown),
            Some(&MemoryValue::Bool(true))
        );
        assert_eq!(loaded.memories.get(MemoryType::AttackCoolingDown), None);
    }

    #[test]
    fn a_bucketed_or_named_axolotl_never_despawns() {
        // `Axolotl.removeWhenFarAway` / `requiresCustomPersistence`。
        // `Axolotl.removeWhenFarAway` / `requiresCustomPersistence`.
        let mut mob = Mob::new(MobType::Axolotl);
        assert!(mob.removes_when_far_away(0));
        assert!(!mob.requires_custom_persistence());
        mob.axolotl.from_bucket = true;
        assert!(!mob.removes_when_far_away(0));
        assert!(mob.requires_custom_persistence());
        let mut named = Mob::new(MobType::Axolotl);
        named.custom_name = Some(mc_bamboo_nbt::Tag::String("Axel".to_owned()));
        assert!(!named.removes_when_far_away(0));
    }

    #[test]
    fn offspring_variant_draws_once_for_the_roll_and_once_for_the_parent() {
        // 种子挑一个 `nextInt(1200)` 不为 0 的，走普通路径。
        // A seed whose `nextInt(1200)` is not zero takes the common path.
        let seed = 0x5EED_5EED;
        let mut random = crate::entity::JavaRandom::new(seed);
        let mut reference = crate::entity::JavaRandom::new(seed);
        let variant = offspring_variant(&mut random, 1, 3);
        let roll = reference.next_int_bounded(RARE_VARIANT_CHANCE);
        assert_ne!(roll, 0);
        let expected = if reference.next_boolean() { 1 } else { 3 };
        assert_eq!(variant, expected);
        assert_eq!(random.state(), reference.state(), "exactly two draws");
    }

    #[test]
    fn a_rare_roll_takes_the_one_entry_rare_list() {
        // 找一个 `nextInt(1200) == 0` 的种子。
        // Find a seed whose `nextInt(1200)` returns zero.
        let seed = (0..100_000)
            .find(|seed| {
                let mut random = crate::entity::JavaRandom::new(*seed);
                random.next_int_bounded(RARE_VARIANT_CHANCE) == 0
            })
            .expect("a rare roll exists in the first 100000 seeds");
        let mut random = crate::entity::JavaRandom::new(seed);
        let mut reference = crate::entity::JavaRandom::new(seed);
        assert_eq!(offspring_variant(&mut random, 1, 3), RARE_VARIANT);
        assert_eq!(reference.next_int_bounded(RARE_VARIANT_CHANCE), 0);
        // `Variant.getRareSpawnVariant` 从单元素名单里再抽一次。
        // `Variant.getRareSpawnVariant` draws once from the one-entry list.
        assert_eq!(reference.next_int_bounded(1), 0);
        assert_eq!(random.state(), reference.state(), "exactly two draws");
    }
}
