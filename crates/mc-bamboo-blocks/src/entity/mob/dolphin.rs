//! 海豚：官方 `Dolphin`（`AgeableWaterCreature` 一支）。
//!
//! 海豚是 `AgeableMob`：有年龄、求爱和繁殖，但 `handleAirSupply` 覆写为空，
//! 氧气按 `LivingEntity.baseTick` 走（满值 4800、离水每刻减一）。`Dolphin.tick()`
//! 在 `super.tick()` 之后管湿度、干死和搁浅扑腾。移动是 `SmoothSwimmingMoveControl`
//! 加 `WaterBoundPathNavigation(allowBreaching = true)`。
//!
//! Dolphin: vanilla `Dolphin` on the `AgeableWaterCreature` branch. It is an
//! `AgeableMob` (age, love, breeding) but overrides `handleAirSupply` to a no-op, so
//! air follows `LivingEntity.baseTick` (max 4800, minus one per tick out of water).
//! `Dolphin.tick()` handles moistness, drying out and beached flopping after
//! `super.tick()`. Movement is `SmoothSwimmingMoveControl` with a
//! `WaterBoundPathNavigation(allowBreaching = true)`.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::Attributes,
    goals::{Goal, GoalSelector, WrappedGoal},
    living, motion,
    path::Malus,
    spawn::SpawnGroup,
    water,
};
use crate::{
    entity::{
        Entity,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
};

/// `Dolphin.TOTAL_MOISTNESS_LEVEL`。
pub(super) const TOTAL_MOISTNESS: i32 = 2400;

/// 海豚的专属状态：`GOT_FISH`、`MOISTNESS_LEVEL` 和找宝藏目标的进度。
///
/// Dolphin-only state: `GOT_FISH`, `MOISTNESS_LEVEL` and the treasure goal's fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DolphinState {
    pub got_fish: bool,
    pub moistness: i32,
    pub treasure_pos: Option<BlockPos>,
    pub stuck: bool,
}

impl DolphinState {
    pub const fn new() -> Self {
        Self {
            got_fish: false,
            moistness: TOTAL_MOISTNESS,
            treasure_pos: None,
            stuck: false,
        }
    }
}

impl Default for DolphinState {
    fn default() -> Self {
        Self::new()
    }
}

/// `EntityTypes.DOLPHIN` 的成年尺寸 0.9×0.6、眼高 0.3；幼年是
/// `BABY_DIMENSIONS`（成年尺寸 × 0.65，眼高固定 0.09375）。
///
/// Adult `EntityTypes.DOLPHIN` is 0.9×0.6 with eye height 0.3; babies use
/// `BABY_DIMENSIONS` (adult × 0.65, eye height fixed at 0.09375).
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Dolphin.entity_type() as usize];
    if baby {
        (facts.width * 0.65, facts.height * 0.65, 0.09375)
    } else {
        (facts.width, facts.height, 0.3)
    }
}

/// `Dolphin.createAttributes`：`Mob.createMobAttributes()` 加 10 血、1.2 移速、3 攻击。
///
/// `Dolphin.createAttributes`: the mob defaults plus 10 health, 1.2 speed, 3 attack.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::mob();
    attributes.set_base(attribute::MAX_HEALTH, 10.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, f64::from(1.2_f32));
    attributes.entries.push(super::attributes::Attribute::new(
        attribute::ATTACK_DAMAGE,
        3.0,
    ));
    attributes
}

/// `AgeableWaterCreature` 构造函数：`setPathfindingMalus(PathType.WATER, 0.0F)`。
///
/// The `AgeableWaterCreature` constructor: `setPathfindingMalus(PathType.WATER, 0.0F)`.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(mc_bamboo_data::PathType::Water, 0.0);
    malus
}

/// `Dolphin.registerGoals` 的十四个目标：优先级、标志位和注册次序照官方。
/// 通用目标直接复用 `goals.rs` 的变体，海豚专属的放进 `dolphin_goals` 模块。
/// 结构定位、船 / 鹦鹉螺、掉落物、玩家效果这些仓库没接的设施，`canUse` 照官方
/// 保住取数次数与顺序、缺少设施时走到返回的那一支（见 `dolphin_goals` 的说明
/// 和 README 已知差异）。
///
/// `Dolphin.registerGoals`'s fourteen goals: priorities, flags and registration order
/// follow vanilla. Shared goals reuse the variants in `goals.rs`; the dolphin-specific
/// ones live in the `dolphin_goals` module. For structures, boats / nautiluses, item
/// entities and player effects that are not wired here, `canUse` keeps vanilla's random
/// draw count and order and takes the return-false branch (see `dolphin_goals` and the
/// README known differences).
pub(super) fn goals() -> GoalSelector {
    use super::dolphin_goals::DolphinGoal as D;
    let registered = vec![
        (0, Goal::Dolphin(D::BreathAir)),
        (0, Goal::Dolphin(D::TryFindLiquid)),
        (1, Goal::Dolphin(D::SwimToTreasure { stuck: false })),
        (2, Goal::Dolphin(D::SwimWithPlayer { player: None })),
        (
            4,
            Goal::Swim {
                speed: 1.0,
                interval: 10,
                wanted: Vec3::default(),
            },
        ),
        (
            4,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
        (
            5,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (5, Goal::Dolphin(D::Jump { breached: false })),
        (6, super::goals::melee_attack(1.2, true, "MeleeAttackGoal")),
        (7, Goal::Dolphin(D::MoveToItem { cooldown: 0 })),
        (8, Goal::Dolphin(D::PlayWithItems)),
        (8, Goal::Dolphin(D::FollowRidden)),
        (8, Goal::Dolphin(D::FollowRidden)),
        // `AvoidEntityGoal(Guardian.class, 8.0F, 1.0D, 1.0D)`：守卫者已在模型里，
        // 这一条走通用的 `AvoidEntity` 真行为，不再是保住取数的占位。
        // `AvoidEntityGoal(Guardian.class, 8.0F, 1.0D, 1.0D)`: guardians are
        // modelled now, so this runs the shared `AvoidEntity` implementation
        // instead of the stub that only kept the draw count.
        (
            9,
            Goal::Avoid(super::avoid::AvoidEntity::new(
                super::avoid::AvoidClass::Guardian,
                8.0,
                1.0,
                1.0,
            )),
        ),
    ];
    let mut selector = GoalSelector::default();
    selector.goals = registered
        .into_iter()
        .map(|(priority, goal)| WrappedGoal {
            priority,
            running: false,
            goal,
        })
        .collect();
    selector
}

/// `Dolphin.registerGoals` 的 `targetSelector`：`HurtByTargetGoal(this,
/// Guardian.class).setAlertOthers()`，优先级 1。夹具里没有伤害，`canUse`
/// 直接短路、不取随机数；`setAlertOthers` 只在挨打时起作用。
///
/// The `targetSelector` of `Dolphin.registerGoals`:
/// `HurtByTargetGoal(this, Guardian.class).setAlertOthers()` at priority 1. With
/// no hurt in the fixture, `canUse` short-circuits without drawing; `setAlertOthers`
/// only matters when the dolphin is hurt.
pub(super) fn target_goals() -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = vec![WrappedGoal {
        priority: 1,
        running: false,
        goal: Goal::HurtByTarget {
            timestamp: -1,
            unseen_ticks: 0,
        },
    }];
    selector
}

/// `Dolphin.finalizeSpawn`：满氧气、抬头角清零，没有组数据时新建
/// `AgeableMobGroupData(0.1F)`，再走 `AgeableMob` / `Mob` 两层。
///
/// `Dolphin.finalizeSpawn`: full air, level head, a fresh `AgeableMobGroupData(0.1F)`
/// when there is no group, then the `AgeableMob` / `Mob` layers.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    entity.with_mob(|_, mob| {
        mob.air_supply = MobType::Dolphin.max_air_supply();
        mob.x_rot = 0.0;
    });
    water::finalize_ageable(entity, random, group, 0.1);
}

/// `SpawnPlacements` 给海豚登记的是 `checkSurfaceAgeableWaterCreatureSpawnRules`，
/// 和鱿鱼一样（海平面下 13 格以内、下面是水、上面是水方块）。
///
/// `SpawnPlacements` registers dolphins with
/// `checkSurfaceAgeableWaterCreatureSpawnRules`, like squid (within thirteen blocks
/// below sea level, water fluid below, a water block above).
pub(super) fn check_spawn_rules(view: &dyn BlockView, sea_level: i32, pos: BlockPos) -> bool {
    water::surface_spawn_rules(view, sea_level, pos)
}

/// `Dolphin.getAmbientSound`：在水里用 `ambient_water`，离水用 `ambient`。
///
/// `Dolphin.getAmbientSound`: `ambient_water` in water, `ambient` out of water.
pub(super) const fn ambient_voice(in_water: bool) -> &'static str {
    if in_water {
        "minecraft:entity.dolphin.ambient_water"
    } else {
        "minecraft:entity.dolphin.ambient"
    }
}

/// `Dolphin.tick` 在 `super.tick()` 之后的部分（服务端）。
///
/// - `isNoAi()`：直接把氧气设满；
/// - 在水里或雨里：湿度回满 2400；
/// - 否则湿度每刻减一，到 0 受 1 点 `dry_out`；着地时像鱼一样扑腾并置
///   `needsSync`（两个 `nextFloat` 和一个 `yRot` 的 `nextFloat`）。
///
/// The part of `Dolphin.tick` after `super.tick()` (server):
/// - `isNoAi()` refills air to the max;
/// - in water or rain the moistness refills to 2400;
/// - otherwise it drops by one each tick and at zero takes one `dry_out` damage;
///   while on the ground it flops like a fish and sets `needsSync`.
pub(super) fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.no_ai {
        mob.air_supply = MobType::Dolphin.max_air_supply();
        return;
    }
    let in_water_or_rain = me.in_water || ctx.blocks().is_raining_at(me.block_position());
    if in_water_or_rain {
        mob.dolphin.moistness = TOTAL_MOISTNESS;
        return;
    }
    mob.dolphin.moistness -= 1;
    if mob.dolphin.moistness <= 0 {
        living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::DryOut),
            1.0,
        );
    }
    if me.on_ground {
        let dx = (me.random.next_float() * 2.0 - 1.0) * 0.2;
        let dz = (me.random.next_float() * 2.0 - 1.0) * 0.2;
        me.delta = me.delta.add(Vec3::new(f64::from(dx), 0.5, f64::from(dz)));
        me.y_rot = me.random.next_float() * 360.0;
        me.on_ground = false;
        mob.needs_sync = true;
    }
}

/// `Dolphin.travelInWater`：仅 `moveRelative` + `move(SELF)` + ×0.9，
/// 没有攻击目标时再加 -0.005 的重力项。
///
/// `Dolphin.travelInWater`: `moveRelative` + `move(SELF)` + ×0.9, plus a -0.005
/// gravity term when there is no attack target.
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
    if mob.target.is_none() {
        me.delta = me.delta.add(Vec3::new(0.0, -0.005, 0.0));
    }
}

/// 玩家拿鱼右键（`Dolphin.mobInteract`）：成年喂小鱼加速长大，否则设 `gotFish`。
/// 消费物品和音效由游戏层交互落地，这里只改生物状态。
///
/// Feeding a dolphin a fish (`Dolphin.mobInteract`): a baby ages up, otherwise
/// `gotFish` is set. Item consumption and the eat sound land in the game layer;
/// here only the mob state changes.
pub(super) fn feed(me: &mut Entity, mob: &mut Mob) {
    if mob.is_baby() {
        // `getSpeedUpSecondsWhenFeeding(-age)`：int 除法先算，再乘 0.1F 截成 int。
        // `getSpeedUpSecondsWhenFeeding(-age)`: integer division first, then × 0.1F.
        let seconds = (((-mob.age) / 20) as f32 * 0.1_f32) as i32;
        super::interact::age_up(me, mob, seconds, true);
    } else {
        mob.dolphin.got_fish = true;
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::{archive::tag, entity::mob::nbt};

    #[test]
    fn dolphin_state_defaults_and_save_round_trip() {
        let mut entity = Entity::by_mob_type(MobType::Dolphin, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.dolphin.got_fish = true;
            mob.dolphin.moistness = 1234;
        });
        let saved = nbt::write_mob(&entity.split()).expect("a dolphin saves");
        assert!(tag::bool_or(&saved, "GotFish", false));
        assert_eq!(tag::int_or(&saved, "Moistness", 0), 1234);
        let row = nbt::read_mob(&saved, "minecraft:dolphin", 7)
            .expect("the tag loads")
            .expect("dolphins are modelled");
        let loaded = row.into_entity();
        let mob = loaded.kind.mob().expect("a mob");
        assert!(mob.dolphin.got_fish);
        assert_eq!(mob.dolphin.moistness, 1234);
        assert_eq!(mob.air_supply, 4800);
        let _ = Tag::Byte(0);
    }

    #[test]
    fn dolphin_is_an_ageable_water_creature_with_4800_air() {
        let dolphin = Mob::new(MobType::Dolphin);
        assert_eq!(dolphin.health, 10.0);
        assert!(MobType::Dolphin.is_ageable_water_creature());
        assert!(!MobType::Dolphin.can_breathe_under_water());
        assert_eq!(MobType::Dolphin.max_air_supply(), 4800);
        assert_eq!(MobType::Dolphin.increase_air_supply(10), 4800);
        assert!(!MobType::Dolphin.is_animal());
        assert!(MobType::Dolphin.spawns_in_water());
    }
}
