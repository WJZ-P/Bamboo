//! 鱼类：官方 `WaterAnimal` → `AbstractFish` 一支的鳕鱼、鲑鱼、热带鱼和河豚。
//!
//! 鱼不走 `AgeableMob`：没有年龄、求爱和繁殖。水面下用 `WaterBoundPathNavigation`
//! （`SwimNodeEvaluator`），`MoveControl` 换成 `FishMoveControl`，`travelInWater` 只加
//! 0.01 的推进、乘 0.9 阻力、没有目标时每刻下沉 0.005。离水落在方块上会扑腾一下。
//!
//! Fish: vanilla `WaterAnimal` → `AbstractFish` (cod, salmon, tropical fish and
//! pufferfish). They are not `AgeableMob`s: no age, love or breeding. They path-find with
//! `WaterBoundPathNavigation` and `SwimNodeEvaluator`, use `FishMoveControl`, and
//! `travelInWater` adds 0.01 thrust, scales by 0.9 and sinks 0.005 without a target.
//! Beached on a block they flop once.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{
    component_enums::{counts, sparse},
    entity_types::{self, TROPICAL_FISH_COMMON_VARIANTS},
    registries::attribute,
    tags::TAG_GROUPS,
};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::Attributes,
    goals::reduced_tick_delay,
    goals::{Goal, GoalSelector, WrappedGoal},
    living,
    spawn::SpawnGroup,
};
use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{Command, EntityCtx},
    },
    random::RandomSource,
    world::World,
};

/// 鱼的专属状态：桶标记、花纹、河豚的膨胀，以及鱼群的跟随关系。
///
/// Fish-only state: the bucket flag, the variant, the pufferfish inflation and the
/// school follower link.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FishState {
    /// `AbstractFish.FROM_BUCKET`。
    pub from_bucket: bool,
    /// `Salmon.DATA_TYPE` / `TropicalFish.DATA_ID_TYPE_VARIANT`，别的鱼恒 0。
    ///
    /// `Salmon.DATA_TYPE` / `TropicalFish.DATA_ID_TYPE_VARIANT`; zero for the others.
    pub variant: i32,
    /// `Pufferfish.PUFF_STATE`。
    pub puff_state: i32,
    /// `Pufferfish.inflateCounter` / `deflateTimer`。
    pub inflate_counter: i32,
    pub deflate_timer: i32,
    /// `AbstractSchoolingFish.leader` / `schoolSize`。
    pub leader: Option<EntityUid>,
    pub school_size: i32,
    /// `TropicalFish.isSchool`。
    pub is_school: bool,
}

impl FishState {
    /// 构造函数里的缺省值：`schoolSize = 1`、热带鱼 `isSchool = true`、河豚 `0`。
    ///
    /// Constructor defaults: `schoolSize = 1`, tropical fish `isSchool = true`,
    /// pufferfish state zero.
    pub const fn new() -> Self {
        Self {
            from_bucket: false,
            variant: 0,
            puff_state: 0,
            inflate_counter: 0,
            deflate_timer: 0,
            leader: None,
            school_size: 1,
            is_school: true,
        }
    }
}

/// 是不是这一支的鱼。
///
/// Whether this is one of the fish.
pub(crate) const fn is_fish(kind: MobType) -> bool {
    matches!(
        kind,
        MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish
    )
}

/// 会不会成群（`AbstractSchoolingFish`）：河豚不在这一支。
///
/// Whether it is on the `AbstractSchoolingFish` branch; the pufferfish is not.
pub(crate) const fn is_schooling(kind: MobType) -> bool {
    matches!(kind, MobType::Cod | MobType::Salmon | MobType::TropicalFish)
}

/// `EntityType.Builder` 的眼高，生成器只导出宽高。和鱿鱼一样是手写常量。
///
/// Eye heights from the `EntityType.Builder`; the generator only exports width and
/// height. Hand-written constants like the squid ones.
const fn eye_height(kind: MobType) -> f32 {
    match kind {
        MobType::Cod => 0.195,
        MobType::Salmon | MobType::TropicalFish => 0.26,
        _ => 0.455,
    }
}

/// 成年尺寸。鱼没有幼年；鲑鱼按花纹、河豚按膨胀程度再缩放。
///
/// Adult size. Fish have no baby; salmon scale by variant and pufferfish by puff state.
pub fn dimensions(kind: MobType, variant: i32, puff_state: i32) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    let scale = match kind {
        MobType::Salmon => salmon_scale(variant),
        MobType::Pufferfish => puff_scale(puff_state),
        _ => 1.0,
    };
    (
        facts.width * scale,
        facts.height * scale,
        eye_height(kind) * scale,
    )
}

/// `Salmon.Variant.boundingBoxScale`。
///
/// `Salmon.Variant.boundingBoxScale`.
pub(crate) const fn salmon_scale(variant: i32) -> f32 {
    match variant {
        0 => 0.5,
        1 => 1.0,
        _ => 1.5,
    }
}

/// `Pufferfish.getScale`。
///
/// `Pufferfish.getScale`.
const fn puff_scale(state: i32) -> f32 {
    match state {
        0 => 0.5,
        1 => 0.7,
        _ => 1.0,
    }
}

/// `AbstractFish.createAttributes`：`Mob.createMobAttributes()` 加 3 点血。
///
/// `AbstractFish.createAttributes`: the mob defaults with three health.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::mob();
    attributes.set_base(attribute::MAX_HEALTH, 3.0);
    attributes
}

/// `WaterAnimal` 构造函数：`setPathfindingMalus(PathType.WATER, 0.0F)`。
///
/// The `WaterAnimal` constructor: `setPathfindingMalus(PathType.WATER, 0.0F)`.
pub(super) fn malus() -> super::path::Malus {
    let mut malus = super::path::Malus::monster();
    malus.set(mc_bamboo_data::PathType::Water, 0.0);
    malus
}

/// 目标选择器：0 惊慌、1 河豚鼓起来、4 随机游泳、5 跟群（成群的那几种）。
/// `AvoidEntityGoal`（优先级 2）这一轮还没建模，见 README 的已知差异。
///
/// The goal selectors: panic at 0, the pufferfish puff at 1, random swimming at 4 and
/// flock following at 5 for the schooling species. `AvoidEntityGoal` (priority 2) is
/// not modelled yet; see the known differences in the README.
pub(super) fn goals(kind: MobType) -> GoalSelector {
    let mut selector = GoalSelector::default();
    let mut goals = vec![
        WrappedGoal {
            priority: 0,
            running: false,
            goal: Goal::Panic {
                speed: 1.25,
                target: Vec3::default(),
            },
        },
        WrappedGoal {
            priority: 4,
            running: false,
            goal: Goal::Swim {
                speed: 1.0,
                interval: 40,
                wanted: Vec3::default(),
            },
        },
    ];
    if kind == MobType::Pufferfish {
        goals.push(WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::PufferfishPuff,
        });
    }
    if is_schooling(kind) {
        goals.push(WrappedGoal {
            priority: 5,
            running: false,
            goal: Goal::FollowFlock {
                // 官方在构造函数里从实体随机源抽 `200 + nextInt(200) % 20` 再折半；
                // 那一刻实体还没有自己的种子，这里用折半后的中位数 100。
                // Vanilla rolls `200 + nextInt(200) % 20` and halves it in the goal
                // constructor, before the entity has its seed; use the halved median.
                next_start: 100,
                time_to_recalc: 0,
            },
        });
    }
    goals.sort_by_key(|wrapped| wrapped.priority);
    selector.goals = goals;
    selector
}

/// `AbstractSchoolingFish.getMaxSchoolSize()`：鳕鱼 8（`AbstractFish` 的簇大小）、
/// 鲑鱼 5、热带鱼 8。
///
/// `AbstractSchoolingFish.getMaxSchoolSize()`: cod 8 (the `AbstractFish` cluster size),
/// salmon 5, tropical fish 8.
pub(super) const fn max_school_size(kind: MobType) -> i32 {
    match kind {
        MobType::Salmon => 5,
        _ => 8,
    }
}

/// `AbstractSchoolingFish.isFollower()`：领头的还在。
///
/// `AbstractSchoolingFish.isFollower()`: the leader is still around.
pub(super) fn is_follower(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let Some(leader) = mob.fish.leader.and_then(|uid| ctx.other(uid)) else {
        return false;
    };
    !leader.removed && matches!(leader.kind, crate::entity::store::KindPublic::Mob(_))
}

/// `AbstractSchoolingFish.hasFollowers()`，以及 `canBeFollowed()` 的上限判断。
///
/// `AbstractSchoolingFish.hasFollowers()` and the `canBeFollowed()` cap check.
pub(super) fn has_followers(mob: &Mob) -> bool {
    mob.fish.school_size > 1
}

/// `AbstractSchoolingFish.canBeFollowed()`。
///
/// `AbstractSchoolingFish.canBeFollowed()`.
pub(crate) fn can_be_followed(mob: &crate::entity::mob::MobPublic) -> bool {
    mob.school_size > 1 && mob.school_size < max_school_size(mob.mob_type)
}

/// `FollowFlockLeaderGoal.canUse`：有跟随者就退出；自己是跟随者就直接跑；否则倒计时，
/// 到点时抽 `nextInt(200)` 重设定时器并在同种鱼里找一群。
///
/// `FollowFlockLeaderGoal.canUse`: bail out when followed, run when a follower, else
/// count down, roll `nextInt(200)` on expiry and pick a school among the same species.
pub(super) fn flock_can_use(
    next_start: &mut i32,
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) -> bool {
    if has_followers(mob) {
        return false;
    }
    if is_follower(mob, ctx) {
        return true;
    }
    if *next_start > 0 {
        *next_start -= 1;
        return false;
    }
    *next_start = reduced_tick_delay(200 + me.random.next_int_bounded(200) % 20);
    let area = me.bounding_box().inflate(8.0, 8.0, 8.0);
    let mut leaders = Vec::new();
    let mut followers = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if other.uid == me.id || other.removed {
            return;
        }
        let crate::entity::store::KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if public.mob_type != mob.mob_type || !other.bounding_box().intersects(area) {
            return;
        }
        if can_be_followed(public) {
            leaders.push(other.uid);
        }
        if public.fish_leader.is_none() {
            followers.push(other.uid);
        }
    });
    let leader = leaders.first().copied().unwrap_or(me.id);
    if leader == me.id {
        // `addFollowers`：最多补到 `maxSchoolSize`，跳过自己。
        let room = (max_school_size(mob.mob_type) - mob.fish.school_size).max(0) as usize;
        for follower in followers.into_iter().take(room) {
            ctx.emit(Command::SchoolFollow { leader, follower }, me);
        }
    }
    is_follower(mob, ctx)
}

/// `FollowFlockLeaderGoal.canContinueToUse`：还在跟着且离领头的不超过 11 格。
///
/// `FollowFlockLeaderGoal.canContinueToUse`: still following and within eleven blocks.
pub(super) fn flock_can_continue(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    if !is_follower(mob, ctx) {
        return false;
    }
    let Some(leader) = mob.fish.leader.and_then(|uid| ctx.other(uid)) else {
        return false;
    };
    me.position.distance_to_sqr(leader.position) <= 121.0
}

/// `FollowFlockLeaderGoal.tick`：每 `adjustedTickDelay(10)` 刻把路重新指向领头的。
///
/// `FollowFlockLeaderGoal.tick`: re-path to the leader every `adjustedTickDelay(10)`
/// ticks.
pub(super) fn flock_tick(
    time_to_recalc: &mut i32,
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    *time_to_recalc -= 1;
    if *time_to_recalc > 0 {
        return;
    }
    *time_to_recalc = super::goals::reduced_tick_delay(10);
    if !is_follower(mob, ctx) {
        return;
    }
    let Some(leader) = mob.fish.leader.and_then(|uid| ctx.other(uid)) else {
        return;
    };
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation
        .move_to_entity(ctx.blocks(), &nav, leader.position, 1.0, 1);
}

/// `FollowFlockLeaderGoal.stop`：`stopFollowing()`。
///
/// `FollowFlockLeaderGoal.stop`: `stopFollowing()`.
pub(super) fn flock_stop(mob: &mut Mob, ctx: &mut dyn EntityCtx, me: &mut Entity) {
    if let Some(leader) = mob.fish.leader.take() {
        ctx.emit(
            Command::SchoolLeave {
                leader,
                follower: me.id,
            },
            me,
        );
    }
}

/// `AbstractFish.aiStep`：离水、着地、竖直撞过时往上蹦一下，并放扑腾声。
///
/// `AbstractFish.aiStep`: out of water, on the ground and after a vertical collision the
/// fish hops up and plays its flop sound.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob) {
    if !me.in_water && me.on_ground && me.vertical_collision {
        let x = (me.random.next_float() * 2.0 - 1.0) * 0.05;
        let z = (me.random.next_float() * 2.0 - 1.0) * 0.05;
        let delta = me.delta;
        me.delta = Vec3::new(
            delta.x + f64::from(x),
            delta.y + f64::from(0.4_f32),
            delta.z + f64::from(z),
        );
        me.on_ground = false;
        // `needsSync`：这一刻的 `sendChanges` 不等更新周期就发速度和移动包。活着时氧气
        // 每刻变、数据脏，看不出差别；死后倒计时那 20 刻只有它能让包当刻发出去。
        // `needsSync`: this tick's `sendChanges` sends velocity and movement without
        // waiting for the update interval. While alive the air supply dirties the data
        // every tick anyway; during the 20 death ticks only this flag sends them on time.
        mob.needs_sync = true;
        let pitch = living::voice_pitch(me, mob);
        living::play_sound(me, mob, flop_sound(mob.mob_type), 1.0, pitch);
    }
}

/// `AbstractFish.travelInWater`：推进 0.01、移动、乘 0.9；没有目标时每刻下沉 0.005。
///
/// `AbstractFish.travelInWater`: 0.01 thrust, move, scale by 0.9, and sink by 0.005
/// each tick without a target.
pub(super) fn travel_in_water(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
) {
    living::move_relative(me, 0.01, input);
    let movement = me.delta;
    super::motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
    if mob.target.is_none() {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y - 0.005, delta.z);
    }
}

/// `Pufferfish.tick` 在 `super.tick()` 之前的膨胀 / 收缩，以及 `Pufferfish.aiStep` 之后
/// 对附近生物的蜇刺。必须在 `baseTick` 之前跑。
///
/// `Pufferfish.tick` runs the inflate/deflate state machine before `super.tick()`, and
/// the sting of nearby mobs after `aiStep`. It has to run before `baseTick`.
pub(super) fn pre_tick(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.mob_type != MobType::Pufferfish {
        return;
    }
    if !living::is_alive(me, mob) || mob.no_ai {
        return;
    }
    if mob.fish.inflate_counter > 0 {
        if mob.fish.puff_state == 0 {
            let pitch = living::voice_pitch(me, mob);
            living::play_sound(me, mob, "minecraft:entity.puffer_fish.blow_up", 1.0, pitch);
            mob.fish.puff_state = 1;
            resize_puffer(me, mob, ctx);
        } else if mob.fish.inflate_counter > 40 && mob.fish.puff_state == 1 {
            let pitch = living::voice_pitch(me, mob);
            living::play_sound(me, mob, "minecraft:entity.puffer_fish.blow_up", 1.0, pitch);
            mob.fish.puff_state = 2;
            resize_puffer(me, mob, ctx);
        }
        mob.fish.inflate_counter += 1;
    } else if mob.fish.puff_state != 0 {
        if mob.fish.deflate_timer > 60 && mob.fish.puff_state == 2 {
            let pitch = living::voice_pitch(me, mob);
            living::play_sound(me, mob, "minecraft:entity.puffer_fish.blow_out", 1.0, pitch);
            mob.fish.puff_state = 1;
            resize_puffer(me, mob, ctx);
        } else if mob.fish.deflate_timer > 100 && mob.fish.puff_state == 1 {
            let pitch = living::voice_pitch(me, mob);
            living::play_sound(me, mob, "minecraft:entity.puffer_fish.blow_out", 1.0, pitch);
            mob.fish.puff_state = 0;
            resize_puffer(me, mob, ctx);
        }
        mob.fish.deflate_timer += 1;
    }
}

fn resize_puffer(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let old = me.dimensions;
    refresh_dimensions(me, mob);
    if !me.first_tick
        && (me.dimensions.0 > old.0 || me.dimensions.1 > old.1)
        && let Some(position) = crate::entity::resize::fudge_position(ctx.blocks(), me, old)
    {
        me.position = position;
    }
}

/// `onSyncedDataUpdated` 里 `refreshDimensions()` 的等价物：按当前花纹 / 膨胀程度换尺寸。
///
/// The `refreshDimensions()` of `onSyncedDataUpdated`: resize for the current variant or
/// puff state.
pub fn refresh_dimensions(me: &mut Entity, mob: &mut Mob) {
    let (width, height, eye) = dimensions(mob.mob_type, mob.fish.variant, mob.fish.puff_state);
    me.dimensions = (width, height);
    mob.eye_height = eye;
}

/// `Pufferfish.aiStep` 尾部：膨胀状态大于 0 时，`getEntitiesOfClass(Mob, inflate(0.3))`
/// 里通过条件的每只生物都蜇一下。
///
/// The tail of `Pufferfish.aiStep`: with any inflation, every mob in the `inflate(0.3)`
/// box that passes the targeting conditions is stung.
pub(super) fn puffer_touch(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.mob_type != MobType::Pufferfish || mob.fish.puff_state <= 0 {
        return;
    }
    if !living::is_alive(me, mob) {
        return;
    }
    let area = me.bounding_box().inflate(0.3, 0.3, 0.3);
    let mut targets = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if let crate::entity::store::KindPublic::Mob(public) = &other.kind
            && other.uid != me.id
            && !other.removed
            && public.health > 0.0
            && scary_for_pufferfish(public.mob_type)
            && other.bounding_box().intersects(area)
        {
            targets.push(other.uid);
        }
    });
    for target in targets {
        let amount = 1.0 + mob.fish.puff_state as f32;
        let mut source =
            DamageSource::environment(DamageKind::MobAttack).with_non_player_cause(true);
        source.attacker = Some(me.id);
        source.direct = Some(me.id);
        source.attacker_uuid = Some(mob.uuid);
        source.position = Some(me.position);
        let seconds = 60 * mob.fish.puff_state;
        ctx.emit(
            Command::PufferSting {
                target,
                source,
                amount,
                seconds,
                position: me.position,
            },
            me,
        );
    }
}

/// `Pufferfish.SCARY_MOB`：使用官方实体类型标签；创造模式玩家由调用方排除。
///
/// `Pufferfish.SCARY_MOB`: use the vanilla entity-type tag; the caller excludes
/// creative players separately.
fn scary_for_pufferfish(kind: MobType) -> bool {
    static EXEMPT: std::sync::OnceLock<&[i32]> = std::sync::OnceLock::new();
    !EXEMPT
        .get_or_init(|| {
            TAG_GROUPS
                .iter()
                .find(|group| group.registry == "minecraft:entity_type")
                .and_then(|group| {
                    group
                        .tags
                        .iter()
                        .find(|tag| tag.name == "minecraft:not_scary_for_pufferfish")
                })
                .map_or(&[], |tag| tag.ids)
        })
        .contains(&kind.entity_type())
}

/// `PufferfishPuffGoal.canUse`：2 格内有没有可瞄准的生物；`start` 把计数器置 1。
///
/// `PufferfishPuffGoal.canUse`: whether any targetable entity is within two blocks;
/// `start` sets the counter to one.
pub(super) fn puff_can_use(me: &Entity, ctx: &dyn EntityCtx) -> bool {
    let area = me.bounding_box().inflate(2.0, 2.0, 2.0);
    let mut found = false;
    ctx.for_each_other_in(area, &mut |other| {
        if let crate::entity::store::KindPublic::Mob(public) = &other.kind
            && other.uid != me.id
            && !other.removed
            && public.health > 0.0
            && scary_for_pufferfish(public.mob_type)
            && other.bounding_box().intersects(area)
        {
            found = true;
        }
    });
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive
            && !player.spectator
            && !player.creative
            && crate::entity::arrow::player_box(player).intersects(area)
        {
            found = true;
        }
    });
    found
}

/// `PufferfishPuffGoal.start`。
///
/// `PufferfishPuffGoal.start`.
pub(super) fn puff_start(mob: &mut Mob) {
    mob.fish.inflate_counter = 1;
    mob.fish.deflate_timer = 0;
}

/// `PufferfishPuffGoal.stop`。
///
/// `PufferfishPuffGoal.stop`.
pub(super) fn puff_stop(mob: &mut Mob) {
    mob.fish.inflate_counter = 0;
}

/// 各物种的四种音效。
///
/// The four sounds of each species.
pub(super) const fn ambient_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::Cod => "minecraft:entity.cod.ambient",
        MobType::Salmon => "minecraft:entity.salmon.ambient",
        MobType::TropicalFish => "minecraft:entity.tropical_fish.ambient",
        _ => "",
    }
}

pub(super) const fn hurt_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::Cod => "minecraft:entity.cod.hurt",
        MobType::Salmon => "minecraft:entity.salmon.hurt",
        MobType::TropicalFish => "minecraft:entity.tropical_fish.hurt",
        _ => "minecraft:entity.puffer_fish.hurt",
    }
}

pub(super) const fn death_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::Cod => "minecraft:entity.cod.death",
        MobType::Salmon => "minecraft:entity.salmon.death",
        MobType::TropicalFish => "minecraft:entity.tropical_fish.death",
        _ => "minecraft:entity.puffer_fish.death",
    }
}

const fn flop_sound(kind: MobType) -> &'static str {
    match kind {
        MobType::Cod => "minecraft:entity.cod.flop",
        MobType::Salmon => "minecraft:entity.salmon.flop",
        MobType::TropicalFish => "minecraft:entity.tropical_fish.flop",
        _ => "minecraft:entity.puffer_fish.flop",
    }
}

/// `AbstractFish.getSwimSound`：所有鱼用同一条。
///
/// `AbstractFish.getSwimSound`: one sound for every fish.
pub(super) const SWIM_SOUND: &str = "minecraft:entity.fish.swim";

/// `AbstractFish.FishMoveControl.tick`：在水里每刻上浮 0.005，走向目标点时把速度按
/// 0.125 插值、按高度差补一点竖直速度、水平朝向对准目标。
///
/// `AbstractFish.FishMoveControl.tick`: rise by 0.005 each tick in water, lerp the speed
/// by 0.125 towards the target, add a little vertical speed from the height difference
/// and turn the body towards the target.
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if super::living::eye_in_fluid(mob, mc_bamboo_data::tags::fluid_tags::WATER) {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y + 0.005, delta.z);
    }
    if mob.mover.operation == super::control::Operation::MoveTo && !mob.navigation.is_done() {
        let target_speed =
            (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
        let speed = mob.speed + (target_speed - mob.speed) * 0.125;
        living::set_speed(mob, speed);
        let xd = mob.mover.wanted_x - me.position.x;
        let yd = mob.mover.wanted_y - me.position.y;
        let zd = mob.mover.wanted_z - me.position.z;
        if yd != 0.0 {
            let dd = (xd * xd + yd * yd + zd * zd).sqrt();
            let delta = me.delta;
            me.delta = Vec3::new(
                delta.x,
                delta.y + f64::from(mob.speed) * (yd / dd) * 0.1,
                delta.z,
            );
        }
        if xd != 0.0 || zd != 0.0 {
            let yaw = (mc_bamboo_core::mth::atan2(zd, xd) * 57.295_776_367_187_5) as f32 - 90.0;
            me.y_rot = super::control::move_rotlerp(me.y_rot, yaw, 90.0);
            mob.y_body_rot = me.y_rot;
        }
    } else {
        living::set_speed(mob, 0.0);
    }
}

/// `BehaviorUtils.getRandomSwimmablePos`：先抽一个随机点，不是可寻路的水就重抽，
/// 最多重抽 10 次（一共 11 次）。
///
/// `BehaviorUtils.getRandomSwimmablePos`: draw a random spot, reroll while it is not
/// path-findable water, at most ten rerolls (eleven draws in all).
pub(super) fn random_swim_pos(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn crate::entity::ctx::BlockView,
) -> Option<Vec3> {
    let mut target = super::goals::default_random_pos(me, mob, view, 10, 7);
    let mut count = 0;
    while let Some(pos) = target {
        if view
            .block_state(mc_bamboo_core::BlockPos::containing(pos))
            .is_pathfindable_water()
        {
            break;
        }
        if count >= 10 {
            break;
        }
        count += 1;
        target = super::goals::default_random_pos(me, mob, view, 10, 7);
    }
    target
}

/// `Salmon.Variant` 的存档名（`StringRepresentable.fromEnum`），没有命名空间。
///
/// The save name of `Salmon.Variant` (`StringRepresentable.fromEnum`), no namespace.
pub(super) const fn salmon_variant_name(variant: i32) -> &'static str {
    match variant {
        0 => "small",
        2 => "large",
        _ => "medium",
    }
}

/// 反查 `Salmon.Variant`，认不出时返回 `None`。
///
/// Looks up `Salmon.Variant`; `None` when the name is unknown.
pub(super) fn salmon_variant_from_name(name: &str) -> Option<i32> {
    match name {
        "small" => Some(0),
        "medium" => Some(1),
        "large" => Some(2),
        _ => None,
    }
}

/// `Salmon.finalizeSpawn`：按权重 30 / 50 / 15 抽 SMALL / MEDIUM / LARGE，再走
/// `super.finalizeSpawn`。
///
/// `Salmon.finalizeSpawn`: weighted 30 / 50 / 15 for small / medium / large, then
/// `super.finalizeSpawn`.
fn salmon_variant(random: &mut crate::entity::random::JavaRandom) -> i32 {
    // `WeightedList.getRandom`：`nextInt(totalWeight)` 后按顺序减。
    // `WeightedList.getRandom`: one `nextInt(totalWeight)`, then subtract in order.
    let roll = random.next_int_bounded(95);
    if roll < 30 {
        0
    } else if roll < 80 {
        1
    } else {
        2
    }
}

/// `WaterAnimal.checkSurfaceWaterAnimalSpawnRules` 加
/// `TropicalFish.checkTropicalFishSpawnRules`：热带鱼允许在 `#allows_tropical_fish_spawns_at_any_height`
/// 的群系里任意高度生成。
///
/// `WaterAnimal.checkSurfaceWaterAnimalSpawnRules` plus
/// `TropicalFish.checkTropicalFishSpawnRules`; tropical fish may spawn at any height in
/// biomes tagged `#allows_tropical_fish_spawns_at_any_height`.
pub(super) fn check_spawn_rules(
    world: &World,
    kind: MobType,
    pos: mc_bamboo_core::BlockPos,
    sea_level: i32,
) -> bool {
    let water = world
        .fluid_state(pos.below())
        .is_tagged(mc_bamboo_data::tags::fluid_tags::WATER)
        && world
            .block_state(pos.above())
            .is(mc_bamboo_data::blocks::WATER);
    if kind == MobType::TropicalFish
        && super::natural::biome_tag_contains(
            "minecraft:allows_tropical_fish_spawns_at_any_height",
            world.biome_id(pos),
        )
    {
        return water;
    }
    super::water::surface_spawn_rules(world, sea_level, pos)
}

/// `finalizeSpawn`：鲑鱼抽花纹，热带鱼抽花纹和颜色，河豚只走 `Mob` 那层。
///
/// `finalizeSpawn`: salmon roll their variant, tropical fish their pattern and colours,
/// and the pufferfish only runs the `Mob` layer.
pub(super) fn finalize_spawn(
    world: &mut World,
    entity: &mut Entity,
    group: &mut Option<SpawnGroup>,
) {
    let kind = entity
        .kind
        .mob()
        .map(|mob| mob.mob_type)
        .unwrap_or_default();
    if kind == MobType::Salmon {
        let variant = salmon_variant(&mut entity.random);
        entity.with_mob(|me, mob| {
            mob.fish.variant = variant;
            refresh_dimensions(me, mob);
        });
    }
    let random = world.random();
    entity.with_mob(|_, mob| super::water::finalize_mob(mob, random));
    if is_schooling(kind) {
        finalize_school(world, entity, group);
    }
    if kind == MobType::TropicalFish {
        finalize_tropical(entity, world.random(), group);
    }
}

/// `AbstractSchoolingFish.finalizeSpawn`：把同批后续个体连到首条鱼上。
///
/// `AbstractSchoolingFish.finalizeSpawn`: link subsequent members to the first fish.
fn finalize_school(world: &mut World, entity: &mut Entity, group: &mut Option<SpawnGroup>) {
    if let Some(SpawnGroup::FishSchool { leader, .. }) = group {
        entity.with_mob(|_, mob| mob.fish.leader = Some(*leader));
        world.with_entity_mut(*leader, &mut |entity| {
            entity.with_mob(|_, mob| mob.fish.school_size += 1);
        });
    } else {
        *group = Some(SpawnGroup::FishSchool {
            leader: entity.id,
            variant: None,
        });
    }
}

/// `TropicalFish.finalizeSpawn` 的花纹那一层。
///
/// The tropical-fish variant layer of `TropicalFish.finalizeSpawn`.
fn finalize_tropical(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let Some(SpawnGroup::FishSchool {
        variant: shared, ..
    }) = group
    else {
        unreachable!("tropical fish have school spawn data");
    };
    let variant = if let Some(variant) = *shared {
        variant
    } else if f64::from(random.next_float()) < 0.9 {
        // `Util.getRandom(COMMON_VARIANTS, random)`。
        let index = random.next_int(TROPICAL_FISH_COMMON_VARIANTS.len() as i32) as usize;
        let variant = TROPICAL_FISH_COMMON_VARIANTS[index];
        *shared = Some(variant);
        variant
    } else {
        entity.with_mob(|_, mob| mob.fish.is_school = false);
        let pattern = random.next_int(sparse::TROPICAL_FISH_PATTERN.len() as i32);
        let base = random.next_int(counts::DYE_COLOR);
        let pattern_color = random.next_int(counts::DYE_COLOR);
        pack_variant(pattern, base, pattern_color)
    };
    entity.with_mob(|_, mob| mob.fish.variant = variant);
}

/// `TropicalFish.packVariant`：低 16 位是花纹打包 ID，16–23 位底色，24–31 位花纹色。
///
/// `TropicalFish.packVariant`: pattern id in the low 16 bits, base colour in 16–23 and
/// pattern colour in 24–31.
pub const fn pack_variant(pattern: i32, base_color: i32, pattern_color: i32) -> i32 {
    (pattern_packed_id(pattern) & 65535) | (base_color & 0xFF) << 16 | (pattern_color & 0xFF) << 24
}

/// `Pattern` 的打包 ID：`base.id | index << 8`。
///
/// The packed `Pattern` id: `base.id | index << 8`.
pub const fn pattern_packed_id(pattern: i32) -> i32 {
    if pattern < 0 || pattern as usize >= sparse::TROPICAL_FISH_PATTERN.len() {
        0
    } else {
        sparse::TROPICAL_FISH_PATTERN[pattern as usize]
    }
}

/// 打包 ID 反查 `Pattern`（`ByIdMap.sparse`，查不到回 KOB）。
///
/// `Pattern` from a packed id (`ByIdMap.sparse`, KOB when absent).
pub fn pattern_from_packed(packed: i32) -> i32 {
    sparse::TROPICAL_FISH_PATTERN
        .iter()
        .position(|&id| id == packed & 65535)
        .map_or(0, |index| index as i32)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::{archive::tag, entity::mob::nbt};

    fn saved(kind: MobType, variant: i32, puff_state: i32) -> mc_bamboo_nbt::Tag {
        let mut entity = Entity::by_mob_type(kind, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.fish.variant = variant;
            mob.fish.puff_state = puff_state;
        });
        nbt::write_mob(&entity.split()).expect("a fish saves")
    }

    fn loaded(kind: MobType, tag: &mc_bamboo_nbt::Tag) -> Mob {
        let row = nbt::read_mob(tag, kind.name(), 7)
            .expect("the tag loads")
            .expect("the fish is modelled");
        row.into_entity().kind.mob().expect("a mob").clone()
    }

    /// `AbstractFish.aiStep` 只属于鱼：站在平整石头上的羊不管闲不闲逛，脚都不该离地。
    /// 守卫丢了时羊落地后的下一刻就被加 0.4 的竖直速度（`sheep_eat` 和 `island_command`
    /// 的回归）。羊要带 AI：没有 AI 的生物不 `travel`，永远不着地，也就不会扑腾。
    ///
    /// `AbstractFish.aiStep` belongs to fish only: a sheep on flat stone must keep its feet
    /// on the ground whether it strolls or not. Without the guard it gains 0.4 upward speed
    /// the tick after landing (the `sheep_eat` and `island_command` regressions). The sheep
    /// needs its AI: a mob without AI never travels, never lands and so never flops.
    #[test]
    fn land_animals_on_the_ground_never_flop() {
        use mc_bamboo_core::BlockPos;

        use crate::{BlockTicks, state::BlockState, testing::TestLevel, world::World};

        let mut level = TestLevel::new();
        for x in -3..=3 {
            for z in -3..=3 {
                level.set(
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let sheep = Entity::by_mob_type(MobType::Sheep, Vec3::new(0.5, 64.0, 0.5));
        let id = world.spawn_entity(sheep);
        for tick in 0..20 {
            world.tick_entities();
            let mut y = None;
            world.with_entity_mut(id, &mut |entity| y = Some(entity.position.y));
            assert_eq!(y, Some(64.0), "tick {tick}: the sheep left the ground");
        }
    }

    #[test]
    fn salmon_variant_survives_a_save_and_load() {
        let tag = saved(MobType::Salmon, 2, 0);
        assert_eq!(tag::string_or(&tag, "type"), Some("large"));
        assert_eq!(loaded(MobType::Salmon, &tag).fish.variant, 2);
    }

    #[test]
    fn pufferfish_puff_state_is_clamped_and_saved() {
        let tag = saved(MobType::Pufferfish, 0, 2);
        assert_eq!(tag::int_or(&tag, "PuffState", 0), 2);
        assert_eq!(loaded(MobType::Pufferfish, &tag).fish.puff_state, 2);
        // `readAdditionalSaveData` 用 `Math.min(..., 2)` 夹。
        // `readAdditionalSaveData` clamps with `Math.min(..., 2)`.
        let mut entries = match tag {
            mc_bamboo_nbt::Tag::Compound(entries) => entries,
            _ => unreachable!("the save is a compound"),
        };
        for (key, value) in &mut entries {
            if key == "PuffState" {
                *value = mc_bamboo_nbt::Tag::Int(7);
            }
        }
        let clamped = loaded(MobType::Pufferfish, &mc_bamboo_nbt::Tag::Compound(entries));
        assert_eq!(clamped.fish.puff_state, 2);
    }

    #[test]
    fn fish_save_has_the_bucket_flag_but_no_age() {
        let tag = saved(MobType::Cod, 0, 0);
        assert!(!tag::bool_or(&tag, "FromBucket", true));
        assert!(tag::field(&tag, "Age").is_none());
        assert!(tag::field(&tag, "InLove").is_none());
    }

    #[test]
    fn tropical_patterns_pack_like_vanilla() {
        // `COMMON_VARIANTS[0]`：STRIPEY 橙 / 灰。
        // `COMMON_VARIANTS[0]`: STRIPEY orange / gray.
        assert_eq!(TROPICAL_FISH_COMMON_VARIANTS[0], pack_variant(7, 1, 7));
        assert_eq!(pattern_from_packed(pack_variant(9, 10, 4)), 9);
        assert_eq!(pattern_from_packed(12345), 0);
        // `packVariant` 的低 16 位是花纹，再往上是两个颜色字节。
        // `packVariant` keeps the pattern low and the two colours above.
        assert_eq!(TROPICAL_FISH_COMMON_VARIANTS[5], 1 << 16);
    }
}
