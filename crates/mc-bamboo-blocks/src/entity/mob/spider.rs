//! 蜘蛛和洞穴蜘蛛：官方 `Spider` / `CaveSpider`。
//!
//! 攀爬是 `DATA_FLAGS_ID` 第 0 位和 `onClimbable()` 的改写：`Spider.tick` 在
//! `super.tick()` 之后把这一刻的 `horizontalCollision` 写进标志，下一 tick 的
//! `travel` 因此在贴墙时每刻上升 0.2。导航换成 `WallClimberNavigation`，路径走完
//! 之后直接朝目标方块的三维坐标推。
//!
//! 蜘蛛在亮处不会锁定目标；困难模式生成时还有 10% × 难度系数的概率带一个随机
//! 药水效果，同一组蜘蛛共享这个结果。洞穴蜘蛛命中时按难度给目标中毒。
//!
//! Spiders and cave spiders: vanilla `Spider` / `CaveSpider`. Climbing is bit 0 of
//! `DATA_FLAGS_ID` with an overridden `onClimbable()`, so the travel code lifts the
//! spider 0.2 per tick while it pushes into a wall. Navigation is
//! `WallClimberNavigation`. Spiders do not lock on in bright light; hard-mode spawns
//! may carry a random potion effect shared by the group. Cave spiders poison on hit.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::{attribute, mob_effect};

use super::{
    Mob, MobType,
    attributes::{Attributes, Modifier, Operation},
    effects::{EffectInstance, INFINITE},
    goals::{Goal, GoalSelector, TargetType, WrappedGoal},
    navigation::Navigation,
    spawn::{RANDOM_SPAWN_BONUS, SpawnGroup, SpawnReason, SpiderGroupData},
    zombie_spawn::special_multiplier,
};
use crate::{
    entity::{Entity, ctx::BlockView},
    random::RandomSource,
    world::World,
};

/// `Spider.SPIDER_SPECIAL_EFFECT_CHANCE`。
const SPECIAL_EFFECT_CHANCE: f32 = 0.1;

/// 蜘蛛自己的状态：`DATA_FLAGS_ID` 的攀爬位。
///
/// Spider state: the climbing bit of `DATA_FLAGS_ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpiderState {
    pub climbing: bool,
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Spider.createAttributes` / `CaveSpider.createCaveSpider`：`Monster` 的表加
/// 16（洞穴蜘蛛 12）点生命和 0.3 移速。
///
/// `Spider.createAttributes` / `CaveSpider.createCaveSpider`: the `Monster` table
/// with 16 (12 for cave spiders) health and 0.3 movement speed.
pub fn attributes(kind: MobType) -> Attributes {
    let mut table = Attributes::skeleton();
    let health = if kind == MobType::CaveSpider {
        12.0
    } else {
        16.0
    };
    table.set_base(attribute::MAX_HEALTH, health);
    table.set_base(attribute::MOVEMENT_SPEED, 0.3);
    table
}

/// `Spider.registerGoals` 的 `goalSelector`。`AvoidEntityGoal(Armadillo)` 保留注册位：
/// 犰狳还没有，`canUse` 找不到对象也不取随机数。
///
/// The `goalSelector` half of `Spider.registerGoals`. The armadillo avoid goal keeps
/// its slot: without armadillos `canUse` finds nothing and draws no random numbers.
pub fn goals(navigation: &mut Navigation) -> GoalSelector {
    // `FloatGoal` 的构造函数打开导航的浮水。
    // The `FloatGoal` constructor enables floating on the navigation.
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(2, Goal::Unavailable("AvoidEntityGoal")),
        wrap(3, Goal::LeapAtTarget { yd: 0.4 }),
        wrap(4, Goal::spider_melee(1.0)),
        wrap(
            5,
            Goal::Stroll {
                speed: 0.8,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            6,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            6,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `Spider.registerGoals` 的 `targetSelector`：反击，然后是只看暗处的玩家和铁傀儡。
///
/// The `targetSelector` half: retaliation, then the dark-only player and iron golem goals.
pub fn target_goals(_navigation: &mut Navigation) -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(1, Goal::HurtByTarget { timestamp: -1 }),
        wrap(2, Goal::spider_target(TargetType::Player)),
        wrap(3, Goal::spider_target(TargetType::IronGolem)),
    ])
}

/// `Spider.tick` 在 `super.tick()` 之后的那一句：`setClimbing(horizontalCollision)`。
///
/// The trailing statement of `Spider.tick`: `setClimbing(horizontalCollision)`.
pub fn tick(me: &Entity, mob: &mut Mob) {
    mob.spider.climbing = me.horizontal_collision;
}

/// `Spider.onClimbable()`：只看自己的攀爬标志。
///
/// `Spider.onClimbable()`: only the climbing flag counts.
pub fn on_climbable(mob: &Mob) -> bool {
    mob.spider.climbing
}

/// `Spider.canBeAffected`：中毒无效，其余照常。
///
/// `Spider.canBeAffected`: poison never applies, everything else does.
pub fn can_be_affected(mob: &Mob, effect: i32) -> bool {
    !(mob.mob_type.climbs_walls() && effect == mob_effect::POISON)
}

/// `CaveSpider.doHurtTarget` 的中毒时长：普通 7 秒，困难 15 秒，其余 0。
///
/// The poison duration of `CaveSpider.doHurtTarget`: 7 s on normal, 15 s on hard.
pub fn poison_seconds(difficulty: i32) -> i32 {
    match difficulty {
        2 => 7,
        3 => 15,
        _ => 0,
    }
}

/// `Spider.SpiderEffectsGroupData.setRandomEffect`：`nextInt(5)` 挑一个效果。
///
/// `Spider.SpiderEffectsGroupData.setRandomEffect`: one `nextInt(5)` pick.
fn random_effect(random: &dyn RandomSource) -> i32 {
    match random.next_int(5) {
        0 | 1 => mob_effect::SPEED,
        2 => mob_effect::STRENGTH,
        3 => mob_effect::REGENERATION,
        _ => mob_effect::INVISIBILITY,
    }
}

/// `Spider.finalizeSpawn`。
///
/// 先 `Mob.finalizeSpawn` 的两笔取数，骑士判定，再组数据。骑士（骷髅骑蜘蛛）还没有
/// 骑乘机制，`nextInt(100)` 仍然取，只是不生成。组数据为空时新建；困难难度下
/// `nextFloat() < 0.1F * 特殊系数` 时再 `nextInt(5)`。
///
/// `Spider.finalizeSpawn`: the two `Mob.finalizeSpawn` draws, the jockey roll, then the
/// group data. The skeleton jockey needs riding, which is not modelled yet; the
/// `nextInt(100)` still happens. A fresh group data rolls the hard-mode effect.
pub(super) fn finalize_spawn(
    world: &mut World<'_>,
    entity: &mut Entity,
    _reason: SpawnReason,
    group: &mut Option<SpawnGroup>,
) {
    let hard = BlockView::difficulty(world) == 3;
    let special = special_multiplier(BlockView::effective_difficulty(
        world,
        entity.block_position(),
    ));
    let random = world.random();
    // `super.finalizeSpawn`：`Mob.finalizeSpawn` 的跟随距离加成和左撇子先取。
    // `super.finalizeSpawn`: the `Mob.finalizeSpawn` follow-range bonus and left-handedness.
    entity.with_mob(|_, mob| {
        if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
            && range.modifier(RANDOM_SPAWN_BONUS).is_none()
        {
            range.modifiers.push(Modifier {
                id: RANDOM_SPAWN_BONUS.to_owned(),
                amount: random.triangle(0.0, 0.114_850_000_000_000_01),
                operation: Operation::AddMultipliedBase,
                permanent: true,
            });
        }
        mob.left_handed = random.next_float() < 0.05;
    });
    let fresh = !matches!(group, Some(SpawnGroup::Spider(_)));
    let mut effect = match group {
        Some(SpawnGroup::Spider(data)) => data.effect,
        _ => None,
    };
    // `random.nextInt(100) == 0`：骷髅骑士。骑乘未建模，只保留这次取数。
    // The skeleton jockey roll; riding is not modelled, only the draw is kept.
    let _jockey = random.next_int(100) == 0;
    if fresh {
        if hard && random.next_float() < SPECIAL_EFFECT_CHANCE * special {
            effect = Some(random_effect(random));
        }
        *group = Some(SpawnGroup::Spider(SpiderGroupData { effect }));
    }
    if let Some(effect) = effect {
        entity.with_mob(|_, mob| {
            super::living::add_effect(mob, EffectInstance::new(effect, INFINITE));
        });
    }
}

/// 洞穴蜘蛛命中时给目标挂的效果；不是洞穴蜘蛛或难度不够时是 `None`。
///
/// The poison instance a cave spider applies on a hit, if any.
pub fn poison_instance(mob: &Mob, difficulty: i32) -> Option<EffectInstance> {
    if mob.mob_type != MobType::CaveSpider {
        return None;
    }
    let seconds = poison_seconds(difficulty);
    (seconds > 0).then(|| EffectInstance::new(mob_effect::POISON, seconds * 20))
}

/// 对拍的状态列：攀爬标志。
///
/// The oracle's state column: the climbing flag.
pub fn describe(mob: &Mob) -> String {
    format!("climb={}", mob.spider.climbing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, World, state::BlockState, world::UPDATE_ALL};
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    /// 属性、尺寸、目标顺序、中毒免疫和中毒时长照官方 `Spider` / `CaveSpider`。
    ///
    /// Attributes, size, goal order, poison immunity and durations follow vanilla.
    #[test]
    fn spiders_have_the_official_attributes_and_goals() {
        let spider = Mob::new(MobType::Spider);
        assert_eq!(spider.attributes.value(attribute::MAX_HEALTH), 16.0);
        assert_eq!(spider.attributes.value(attribute::MOVEMENT_SPEED), 0.3);
        assert_eq!(spider.attributes.value(attribute::ATTACK_DAMAGE), 2.0);
        assert_eq!(spider.attributes.value(attribute::FOLLOW_RANGE), 16.0);
        assert_eq!(MobType::Spider.dimensions(false), (1.4, 0.9, 0.65));
        assert_eq!(MobType::CaveSpider.dimensions(false), (0.7, 0.5, 0.45));
        let priorities: Vec<i32> = spider
            .goals
            .goals
            .iter()
            .map(|goal| goal.priority)
            .collect();
        assert_eq!(priorities, [1, 2, 3, 4, 5, 6, 6]);
        let targets: Vec<&str> = spider
            .target_goals
            .goals
            .iter()
            .map(|goal| goal.goal.name())
            .collect();
        assert_eq!(
            targets,
            [
                "HurtByTargetGoal",
                "NearestAttackableTargetGoal",
                "NearestAttackableTargetGoal"
            ]
        );
        assert!(spider.navigation.wall_climber);
        assert!(spider.navigation.can_float());
        let cave = Mob::new(MobType::CaveSpider);
        assert_eq!(cave.attributes.value(attribute::MAX_HEALTH), 12.0);
        assert_eq!(poison_seconds(0), 0);
        assert_eq!(poison_seconds(1), 0);
        assert_eq!(poison_seconds(2), 7);
        assert_eq!(poison_seconds(3), 15);
    }

    /// 蜘蛛对中毒免疫，洞穴蜘蛛也一样；别的效果照挂。
    ///
    /// Spiders ignore poison, cave spiders included; other effects still apply.
    #[test]
    fn spiders_ignore_poison() {
        let mut spider = Mob::new(MobType::Spider);
        let poison = EffectInstance::new(mob_effect::POISON, 100);
        assert!(!super::super::living::add_effect(&mut spider, poison));
        assert!(!spider.effects.has(mob_effect::POISON));
        let speed = EffectInstance::new(mob_effect::SPEED, 100);
        assert!(super::super::living::add_effect(&mut spider, speed));
        assert!(spider.effects.has(mob_effect::SPEED));
    }

    /// 一面六格高的墙，目标在墙后上方：`WallClimberNavigation` 找不到路径时记下目标方块，
    /// 蜘蛛贴墙往上爬。
    ///
    /// A six-block wall with the goal above and behind it: without a path the
    /// `WallClimberNavigation` remembers the destination and the spider climbs.
    #[test]
    fn a_wall_climber_pushes_up_an_unreachable_wall() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        for x in -8..=8 {
            for z in -8..=8 {
                world.set_block(
                    BlockPos::new(x, 63, z),
                    BlockState(blocks::STONE),
                    UPDATE_ALL,
                );
            }
        }
        for z in -8..=8 {
            for y in 64..=69 {
                world.set_block(
                    BlockPos::new(2, y, z),
                    BlockState(blocks::STONE),
                    UPDATE_ALL,
                );
            }
        }
        let spider = world.spawn_entity(Entity::by_mob_type(
            MobType::Spider,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        let goal = Vec3::new(6.5, 72.0, 0.5);
        let mut highest = 64.0_f64;
        for _ in 0..120 {
            world.tick_entities();
            let mut entity = world.entity(spider).expect("spider alive");
            // 和 `MeleeAttackGoal.tick` 一样每次重算：没有路径时
            // `WallClimberNavigation.moveTo` 返回真并记下目标。
            // Like `MeleeAttackGoal.tick`: without a path the wall climber remembers
            // the destination and reports success.
            entity.with_mob(|me, mob| {
                let nav = super::super::living::nav_mob(me, mob, &world);
                // 返回假也算正常：路径存在但走完时官方同样返回假，
                // `createPath` 记下的目标方块由 `tick` 接手。
                // False is fine: vanilla returns false for a finished path too, and the
                // destination recorded by `createPath` is picked up by `tick`.
                let _ = mob.navigation.move_to_entity(&world, &nav, goal, 1.0, 0);
            });
            world.with_entity_mut(spider, &mut |stored| *stored = entity.clone());
            if let Some(entity) = world.entity(spider) {
                highest = highest.max(entity.position.y);
            }
        }
        assert!(highest > 65.0, "the spider climbed to {highest}");
    }

    /// 亮处不锁定玩家，暗处才锁。
    ///
    /// No player target in bright light; the target appears in the dark.
    #[test]
    fn bright_light_stops_the_spider_from_targeting() {
        let run = |sky_darken: i32| {
            let mut level = crate::testing::TestLevel::new();
            level.sky_darken = sky_darken;
            level.players.push(crate::entity::ctx::PlayerView {
                uid: 900,
                uuid: None,
                position: Vec3::new(0.5, 64.0, 5.5),
                eye_height: 1.62,
                look_direction: Vec3::default(),
                gaze_disguise: false,
                alive: true,
                spectator: false,
                creative: false,
                sneaking: false,
                invisible: false,
                armor_cover: 0.0,
                main_hand: 0,
                off_hand: 0,
                combat: Default::default(),
            });
            let mut ticks = BlockTicks::new();
            let mut world = World::new(&mut level, &mut ticks);
            for x in -8..=8 {
                for z in -8..=8 {
                    world.set_block(
                        BlockPos::new(x, 63, z),
                        BlockState(blocks::STONE),
                        UPDATE_ALL,
                    );
                }
            }
            let spider = world.spawn_entity(Entity::by_mob_type(
                MobType::Spider,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            let mut seen = false;
            for _ in 0..200 {
                world.tick_entities();
                if let Some(entity) = world.entity(spider)
                    && entity.kind.mob().is_some_and(|mob| mob.target.is_some())
                {
                    seen = true;
                    break;
                }
            }
            seen
        };
        assert!(!run(0), "bright light must not produce a target");
        assert!(run(15), "darkness must produce a target");
    }
}
