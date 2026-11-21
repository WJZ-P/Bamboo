//! 苦力怕：官方 `Creeper` 和 `SwellGoal`。
//!
//! 膨胀计时在 `Creeper.tick` 里、`super.tick()` 之前推进；`SwellGoal` 只决定方向。
//! 爆炸交给 [`Command::ExplodeMob`] 落地（方块、掉落、伤害都在那里），苦力怕自己当刻消失。
//! 摔落会让它提前膨胀，打火石和火焰弹能点燃它。
//!
//! Creepers: vanilla `Creeper` and `SwellGoal`. The swell counter advances in
//! `Creeper.tick` before `super.tick()`; `SwellGoal` only picks the direction. The
//! blast lands through [`Command::ExplodeMob`] and the creeper is discarded in the same
//! tick. Falling primes it early; flint and steel or a fire charge ignite it.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, items, registries::attribute, tags::item_tags};
use mc_bamboo_nbt::Tag;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attributes, Modifier, Operation},
    goals::{Goal, GoalSelector, TargetType, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    spawn::RANDOM_SPAWN_BONUS,
};
use crate::{
    InteractionResult,
    entity::{
        Entity, EntityKind,
        ctx::{Command, EntityCtx},
    },
    random::RandomSource,
    state::item_is_tagged,
};

/// `Creeper.DEFAULT_MAX_SWELL`。
const DEFAULT_MAX_SWELL: i32 = 30;
/// `Creeper.DEFAULT_EXPLOSION_RADIUS`。
const DEFAULT_EXPLOSION_RADIUS: i32 = 3;

/// 苦力怕自己的状态：`swell` / `maxSwell` / `explosionRadius` 三个字段和三项同步元数据。
///
/// Creeper state: the `swell`, `maxSwell` and `explosionRadius` fields plus the three
/// synced data items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreeperState {
    pub swell: i32,
    pub max_swell: i32,
    pub explosion_radius: i32,
    /// `DATA_SWELL_DIR`，缺省 -1。
    pub swell_dir: i32,
    /// `DATA_IS_POWERED`。
    pub powered: bool,
    /// `DATA_IS_IGNITED`。
    pub ignited: bool,
}

impl Default for CreeperState {
    fn default() -> Self {
        Self {
            swell: 0,
            max_swell: DEFAULT_MAX_SWELL,
            explosion_radius: DEFAULT_EXPLOSION_RADIUS,
            swell_dir: -1,
            powered: false,
            ignited: false,
        }
    }
}

/// （宽，高，眼高），来自生成的实体类型表。
///
/// (width, height, eye height) from the generated entity type table.
pub(super) const fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Creeper.entity_type() as usize];
    (facts.width, facts.height, entity_types::CREEPER_EYE_HEIGHT)
}

/// `Creeper.createAttributes`：`Monster.createMonsterAttributes` 加速度 0.25，
/// 和骷髅的表逐项相同。
///
/// `Creeper.createAttributes`: `Monster.createMonsterAttributes` with speed 0.25,
/// entry for entry the skeleton table.
pub(super) fn attributes() -> Attributes {
    Attributes::skeleton()
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Creeper.registerGoals` 的 `goalSelector`，按注册顺序。躲豹猫和躲猫两项保留注册位：
/// 这两种实体还没有，`AvoidEntityGoal.canUse` 找不到对象时本来也不取随机数。
///
/// The `goalSelector` half of `Creeper.registerGoals`, in registration order. The two
/// avoid goals keep their slots: without ocelots and cats `AvoidEntityGoal.canUse`
/// finds nothing and draws no random numbers either.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    // `FloatGoal` 的构造函数打开导航的浮水。
    // The `FloatGoal` constructor enables floating on the navigation.
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(2, Goal::Swell { target: None }),
        wrap(3, Goal::Unavailable("AvoidEntityGoal")),
        wrap(3, Goal::Unavailable("AvoidEntityGoal")),
        wrap(4, Goal::melee(1.0, false)),
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

/// `Creeper.registerGoals` 的 `targetSelector`：先最近的玩家，再反击。
///
/// The `targetSelector` half: nearest player first, then retaliation.
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(1, Goal::nearest_target(TargetType::Player)),
        wrap(2, Goal::HurtByTarget { timestamp: -1 }),
    ])
}

/// `Creeper.tick` 在 `super.tick()` 之前的那一段：点燃后一直膨胀，开始膨胀时响引信声，
/// 膨胀到 `maxSwell` 就爆炸。
///
/// The part of `Creeper.tick` before `super.tick()`: an ignited creeper keeps swelling,
/// the fuse sound plays when swelling starts, and reaching `maxSwell` explodes.
pub(super) fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    if mob.creeper.ignited {
        mob.creeper.swell_dir = 1;
    }
    let direction = mob.creeper.swell_dir;
    if direction > 0 && mob.creeper.swell == 0 {
        living::play_sound(me, mob, "minecraft:entity.creeper.primed", 1.0, 0.5);
    }
    mob.creeper.swell = (mob.creeper.swell + direction).max(0);
    if mob.creeper.swell >= mob.creeper.max_swell {
        mob.creeper.swell = mob.creeper.max_swell;
        explode(me, mob, ctx);
    }
}

/// `Creeper.explodeCreeper`：带电时威力翻倍，置 `dead`，爆炸，然后自己消失。
/// 滞留药水云和死亡时触发的效果没有建模。
///
/// `Creeper.explodeCreeper`: doubled power when powered, set `dead`, explode, discard.
/// The lingering cloud and on-death effect triggers are not modelled.
fn explode(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let multiplier = if mob.creeper.powered { 2.0_f32 } else { 1.0 };
    mob.dead = true;
    let source = {
        let mut whole = me.clone();
        whole.kind = EntityKind::Mob(Box::new(mob.clone()));
        Box::new(whole.split().public)
    };
    ctx.emit(
        Command::ExplodeMob {
            center: me.position,
            radius: mob.creeper.explosion_radius as f32 * multiplier,
            source,
        },
        me,
    );
    me.discard();
}

/// `Creeper.causeFallDamage` 在 `super` 之后的部分：摔得越高膨胀越多，最多到 `maxSwell - 5`。
///
/// The tail of `Creeper.causeFallDamage`: a longer fall swells more, capped at
/// `maxSwell - 5`.
pub(super) fn after_fall(mob: &mut Mob, fall_distance: f64) {
    let state = &mut mob.creeper;
    state.swell += (fall_distance * 1.5) as i32;
    if state.swell > state.max_swell - 5 {
        state.swell = state.max_swell - 5;
    }
}

/// `Creeper.getMaxFallDistance`：没有目标时 3 格，有目标时肯摔到只剩 1 点血。
///
/// `Creeper.getMaxFallDistance`: 3 blocks without a target, otherwise down to one
/// health point.
pub(super) fn max_fall_distance(mob: &Mob) -> i32 {
    let allowed = if mob.target.is_some() {
        mob.health - 1.0
    } else {
        0.0
    };
    mc_bamboo_core::mth::floor_f32(allowed + 3.0)
}

/// `Creeper.mobInteract`：`#creeper_igniters`（打火石、火焰弹）点燃它。音高取苦力怕自己的
/// 随机源；可损坏的物品扣 1 点耐久，否则消耗一个。
///
/// `Creeper.mobInteract`: a `#creeper_igniters` item ignites it. The pitch comes from the
/// creeper's own random; damageable items lose one durability, others are consumed.
pub(super) fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if req.held.is_empty() || !item_is_tagged(req.held.item, item_tags::CREEPER_IGNITERS) {
        return MobInteraction::pass();
    }
    let sound = if req.held.item == items::FIRE_CHARGE {
        "minecraft:item.firecharge.use"
    } else {
        "minecraft:item.flintandsteel.use"
    };
    let pitch = me.random.next_float() * 0.4 + 0.8;
    // `level.playSound(player, x, y, z, sound, getSoundSource(), 1, pitch)`：
    // 音源类别是苦力怕的 HOSTILE，位置是苦力怕。
    // The category is the creeper's HOSTILE and the position is the creeper's.
    mob.events.push(MobEvent::Sound {
        sound,
        volume: 1.0,
        pitch,
        position: me.position,
        hostile: true,
        seed: None,
    });
    mob.creeper.ignited = true;
    let damageable = crate::durability::is_damageable_stack(&req.held);
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !damageable && !req.creative,
        replacement: None,
        shear: false,
        damage_held: damageable && !req.creative,
    }
}

/// `Monster` 没有自己的 `finalizeSpawn`，只有 `Mob.finalizeSpawn`：跟随距离的随机加成
/// 和左撇子，各取一次世界随机数。
///
/// `Monster` adds nothing to `Mob.finalizeSpawn`: the random follow-range bonus and
/// left-handedness, one level-random draw each.
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn RandomSource) {
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
}

/// `Creeper.addAdditionalSaveData`：四个键总是写。
///
/// `Creeper.addAdditionalSaveData`: the four keys are always written.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    let state = &mob.creeper;
    let mut put = |key: &str, value| out.push((key.to_owned(), value));
    put("powered", Tag::Byte(i8::from(state.powered)));
    put("Fuse", Tag::Short(state.max_swell as i16));
    put("ExplosionRadius", Tag::Byte(state.explosion_radius as i8));
    put("ignited", Tag::Byte(i8::from(state.ignited)));
}

/// `Creeper.readAdditionalSaveData`：缺的键按官方缺省；`ignited` 为真才点燃。
///
/// `Creeper.readAdditionalSaveData`: missing keys take the vanilla defaults; only a
/// true `ignited` ignites.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    use crate::archive::tag;
    let state = &mut mob.creeper;
    state.powered = tag::bool_or(tag, "powered", false);
    state.max_swell = i32::from(tag::short_or(tag, "Fuse", DEFAULT_MAX_SWELL as i16));
    state.explosion_radius = i32::from(tag::byte_or(
        tag,
        "ExplosionRadius",
        DEFAULT_EXPLOSION_RADIUS as i8,
    ));
    if tag::bool_or(tag, "ignited", false) {
        state.ignited = true;
    }
}

/// 对拍的状态列：膨胀计数、方向、带电、点燃。
///
/// The oracle's state column: swell counter, direction, powered, ignited.
pub fn describe(mob: &Mob) -> String {
    let state = &mob.creeper;
    format!(
        "swell={},dir={},powered={},ignited={}",
        state.swell, state.swell_dir, state.powered, state.ignited
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::nbt;

    /// 属性、尺寸和目标注册顺序照官方 `Creeper`。
    ///
    /// Attributes, size and goal registration order follow vanilla `Creeper`.
    #[test]
    fn a_creeper_has_the_official_attributes_and_goals() {
        let mob = Mob::new(MobType::Creeper);
        assert_eq!(mob.attributes.value(attribute::MAX_HEALTH), 20.0);
        assert_eq!(mob.attributes.value(attribute::MOVEMENT_SPEED), 0.25);
        assert_eq!(mob.attributes.value(attribute::FOLLOW_RANGE), 16.0);
        assert_eq!(MobType::Creeper.dimensions(false), (0.6, 1.7, 1.445));
        let priorities: Vec<i32> = mob.goals.goals.iter().map(|goal| goal.priority).collect();
        assert_eq!(priorities, [1, 2, 3, 3, 4, 5, 6, 6]);
        let targets: Vec<&str> = mob
            .target_goals
            .goals
            .iter()
            .map(|goal| goal.goal.name())
            .collect();
        assert_eq!(targets, ["NearestAttackableTargetGoal", "HurtByTargetGoal"]);
        assert!(mob.navigation.can_float());
        assert_eq!(mob.creeper, CreeperState::default());
    }

    /// 摔落让膨胀提前，但到不了自己炸的那一步。
    ///
    /// A fall swells the creeper early without reaching the explosion.
    #[test]
    fn a_fall_swells_up_to_five_below_the_fuse() {
        let mut mob = Mob::new(MobType::Creeper);
        after_fall(&mut mob, 4.9);
        assert_eq!(mob.creeper.swell, 7);
        after_fall(&mut mob, 100.0);
        assert_eq!(mob.creeper.swell, 25);
        assert_eq!(max_fall_distance(&mob), 3);
        mob.target = Some(7);
        assert_eq!(max_fall_distance(&mob), 22);
    }

    /// 存档往返保留四个键；`ignited` 读回后下一刻开始膨胀。
    ///
    /// The four save keys round-trip, and a loaded `ignited` starts the swell.
    #[test]
    fn creeper_nbt_round_trips_fuse_radius_powered_and_ignited() {
        let mut entity = Entity::by_mob_type(MobType::Creeper, Vec3::new(0.5, 64.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.creeper.powered = true;
            mob.creeper.max_swell = 12;
            mob.creeper.explosion_radius = 5;
            mob.creeper.ignited = true;
        });
        let tag = nbt::write_mob(&entity.split()).expect("write creeper");
        let Tag::Compound(entries) = &tag else {
            panic!("mob NBT is a compound");
        };
        let value = |key: &str| entries.iter().find(|(name, _)| name == key).map(|e| &e.1);
        assert_eq!(value("powered"), Some(&Tag::Byte(1)));
        assert_eq!(value("Fuse"), Some(&Tag::Short(12)));
        assert_eq!(value("ExplosionRadius"), Some(&Tag::Byte(5)));
        assert_eq!(value("ignited"), Some(&Tag::Byte(1)));
        assert!(value("Age").is_none());
        let mut loaded = nbt::read_mob(&tag, "minecraft:creeper", 9)
            .expect("read creeper")
            .expect("modelled")
            .into_entity();
        loaded.with_mob(|_, mob| {
            assert!(mob.creeper.powered && mob.creeper.ignited);
            assert_eq!(mob.creeper.max_swell, 12);
            assert_eq!(mob.creeper.explosion_radius, 5);
        });
    }

    /// 一只点燃的苦力怕旁边放一块石头和一头不动的牛，跑 30 刻：
    /// （石头还在不在，牛剩多少血，苦力怕还在不在）。
    ///
    /// An ignited creeper beside a stone block and a still cow, run for 30 ticks:
    /// (stone left, cow health, creeper left).
    fn ignited_blast(griefing: bool) -> (bool, f32, bool) {
        use crate::{BlockTicks, World, state::BlockState, world::UPDATE_ALL};
        use mc_bamboo_core::BlockPos;
        use mc_bamboo_data::blocks;

        let mut level = crate::testing::TestLevel::new();
        level.rules.mob_griefing = griefing;
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        for x in -6..=6 {
            for z in -6..=6 {
                let floor = BlockPos::new(x, 63, z);
                world.set_block(floor, BlockState(blocks::OBSIDIAN), UPDATE_ALL);
            }
        }
        let stone = BlockPos::new(1, 64, 0);
        world.set_block(stone, BlockState(blocks::STONE), UPDATE_ALL);
        let still = |kind, x: f64| {
            let mut entity = Entity::by_mob_type(kind, Vec3::new(x, 64.0, 0.5));
            entity.with_mob(|_, mob| mob.no_ai = true);
            entity
        };
        let mut creeper = still(MobType::Creeper, 0.5);
        creeper.with_mob(|_, mob| mob.creeper.ignited = true);
        let creeper = world.spawn_entity(creeper);
        let cow = world.spawn_entity(still(MobType::Cow, -4.5));
        for _ in 0..30 {
            world.tick_entities();
        }
        let health = world
            .entity(cow)
            .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
            .expect("the cow survives");
        (
            world.block_state(stone).is(blocks::STONE),
            health,
            world.entity(creeper).is_some(),
        )
    }

    /// 第 30 刻爆炸：5 格外的牛按官方公式掉 5.08 点血，石头只在 `mobGriefing` 开着时被炸掉。
    ///
    /// The blast comes on tick 30: a cow five blocks away loses 5.08 health by the vanilla
    /// formula, and the stone only breaks under `mobGriefing`.
    #[test]
    fn an_ignited_creeper_explodes_after_its_fuse() {
        let expected = 10.0 - ((1.0 / 36.0 + 1.0 / 6.0) / 2.0 * 7.0 * 6.0 + 1.0);
        let (stone, health, creeper) = ignited_blast(true);
        assert!(!stone && !creeper);
        assert!((f64::from(health) - expected).abs() < 1.0e-4, "{health}");
        let (stone, health, creeper) = ignited_blast(false);
        assert!(stone && !creeper);
        assert!((f64::from(health) - expected).abs() < 1.0e-4, "{health}");
    }

    /// 打火石点燃并扣耐久，火焰弹被消耗，别的物品不理。
    ///
    /// Flint and steel ignites and takes damage, a fire charge is consumed, anything
    /// else passes.
    #[test]
    fn igniters_light_the_fuse() {
        let request = |item| MobInteractRequest {
            player_uid: 1,
            player_uuid: [0; 16],
            hand: mc_bamboo_core::Hand::Main,
            held: mc_bamboo_protocol::item::ItemStack::new(item, 1),
            creative: false,
            position: Vec3::default(),
        };
        let mut entity = Entity::by_mob_type(MobType::Creeper, Vec3::default());
        entity.with_mob(|me, mob| {
            let pass = interact(me, mob, &request(items::STICK));
            assert_eq!(pass, MobInteraction::pass());
            assert!(!mob.creeper.ignited);
            let flint = interact(me, mob, &request(items::FLINT_AND_STEEL));
            assert!(flint.damage_held && !flint.shrink_held);
            assert!(mob.creeper.ignited);
            let charge = interact(me, mob, &request(items::FIRE_CHARGE));
            assert!(charge.shrink_held && !charge.damage_held);
        });
    }
}
