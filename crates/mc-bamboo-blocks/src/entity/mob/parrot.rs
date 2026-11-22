//! 鹦鹉：可驯服（复用 [`super::tame`]）、五种颜色、飞行移动与飞行寻路、模仿附近敌对
//! 生物的叫声、停在主人肩膀上（未做，见 README）、吃曲奇中毒死亡。
//!
//! Parrots: tamable (reusing [`super::tame`]), five variants, flying movement and
//! pathfinding, imitating nearby hostile mobs, landing on the owner's shoulder (not
//! modelled yet, see the README) and dying from cookies.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    BlockAncestor, blocks, entity_types,
    registries::attribute,
    tags::{block_tags, item_tags},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    goals::{
        Flag, Goal, GoalSelector, air_and_water_random_pos, hover_random_pos, land_random_pos,
        reduced_tick_delay,
    },
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    tame::{self, TameGoal},
};
use crate::{
    InteractionResult,
    entity::{
        Entity,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
    state::item_is_tagged,
};
/// `Parrot.Variant` 的注册名，下标就是 `DATA_VARIANT_ID` 的值。
///
/// The `Parrot.Variant` names; the index is the `DATA_VARIANT_ID` value.
pub const VARIANTS: [&str; 5] = ["red_blue", "blue", "green", "yellow_blue", "gray"];

/// `Parrot.Variant.DEFAULT`。
pub const DEFAULT_VARIANT: i32 = 0;

/// `Parrot.MOB_SOUND_MAP` 的键，按官方的 `HashMap` 迭代顺序排不出来；这里按注册名排序
/// 存放，`getAmbient` 取第 `nextInt(size)` 个时和官方的顺序可能不同（见汇报）。
///
/// The keys of `Parrot.MOB_SOUND_MAP`. Vanilla iterates a `HashMap`, whose order is not
/// the insertion order; this table is sorted by name and may pick a different entry
/// (see the report).
pub const MOB_SOUNDS: [(&str, &str); 42] = [
    ("minecraft:blaze", "minecraft:entity.parrot.imitate.blaze"),
    ("minecraft:bogged", "minecraft:entity.parrot.imitate.bogged"),
    ("minecraft:breeze", "minecraft:entity.parrot.imitate.breeze"),
    (
        "minecraft:camel_husk",
        "minecraft:entity.parrot.imitate.camel_husk",
    ),
    (
        "minecraft:cave_spider",
        "minecraft:entity.parrot.imitate.spider",
    ),
    (
        "minecraft:creaking",
        "minecraft:entity.parrot.imitate.creaking",
    ),
    (
        "minecraft:creeper",
        "minecraft:entity.parrot.imitate.creeper",
    ),
    (
        "minecraft:drowned",
        "minecraft:entity.parrot.imitate.drowned",
    ),
    (
        "minecraft:elder_guardian",
        "minecraft:entity.parrot.imitate.elder_guardian",
    ),
    (
        "minecraft:ender_dragon",
        "minecraft:entity.parrot.imitate.ender_dragon",
    ),
    (
        "minecraft:endermite",
        "minecraft:entity.parrot.imitate.endermite",
    ),
    ("minecraft:evoker", "minecraft:entity.parrot.imitate.evoker"),
    ("minecraft:ghast", "minecraft:entity.parrot.imitate.ghast"),
    (
        "minecraft:guardian",
        "minecraft:entity.parrot.imitate.guardian",
    ),
    (
        "minecraft:happy_ghast",
        "minecraft:entity.generic.extinguish_fire",
    ),
    ("minecraft:hoglin", "minecraft:entity.parrot.imitate.hoglin"),
    ("minecraft:husk", "minecraft:entity.parrot.imitate.husk"),
    (
        "minecraft:illusioner",
        "minecraft:entity.parrot.imitate.illusioner",
    ),
    (
        "minecraft:magma_cube",
        "minecraft:entity.parrot.imitate.magma_cube",
    ),
    (
        "minecraft:parched",
        "minecraft:entity.parrot.imitate.parched",
    ),
    (
        "minecraft:phantom",
        "minecraft:entity.parrot.imitate.phantom",
    ),
    ("minecraft:piglin", "minecraft:entity.parrot.imitate.piglin"),
    (
        "minecraft:piglin_brute",
        "minecraft:entity.parrot.imitate.piglin_brute",
    ),
    (
        "minecraft:pillager",
        "minecraft:entity.parrot.imitate.pillager",
    ),
    (
        "minecraft:ravager",
        "minecraft:entity.parrot.imitate.ravager",
    ),
    (
        "minecraft:shulker",
        "minecraft:entity.parrot.imitate.shulker",
    ),
    (
        "minecraft:silverfish",
        "minecraft:entity.parrot.imitate.silverfish",
    ),
    (
        "minecraft:skeleton",
        "minecraft:entity.parrot.imitate.skeleton",
    ),
    ("minecraft:slime", "minecraft:entity.parrot.imitate.slime"),
    ("minecraft:spider", "minecraft:entity.parrot.imitate.spider"),
    ("minecraft:stray", "minecraft:entity.parrot.imitate.stray"),
    ("minecraft:vex", "minecraft:entity.parrot.imitate.vex"),
    (
        "minecraft:vindicator",
        "minecraft:entity.parrot.imitate.vindicator",
    ),
    ("minecraft:warden", "minecraft:entity.parrot.imitate.warden"),
    ("minecraft:witch", "minecraft:entity.parrot.imitate.witch"),
    ("minecraft:wither", "minecraft:entity.parrot.imitate.wither"),
    (
        "minecraft:wither_skeleton",
        "minecraft:entity.parrot.imitate.wither_skeleton",
    ),
    ("minecraft:zoglin", "minecraft:entity.parrot.imitate.zoglin"),
    ("minecraft:zombie", "minecraft:entity.parrot.imitate.zombie"),
    (
        "minecraft:zombie_horse",
        "minecraft:entity.parrot.imitate.zombie_horse",
    ),
    (
        "minecraft:zombie_nautilus",
        "minecraft:entity.parrot.imitate.zombie_nautilus",
    ),
    (
        "minecraft:zombie_villager",
        "minecraft:entity.parrot.imitate.zombie_villager",
    ),
];

/// 鹦鹉自己的状态。
///
/// Parrot-only state.
#[derive(Debug, Clone, PartialEq)]
pub struct Parrot {
    /// `DATA_VARIANT_ID`：0..=4。
    ///
    /// `DATA_VARIANT_ID`: 0..=4.
    pub variant: i32,
    /// `ShoulderRidingEntity.rideCooldownCounter`：上肩的冷却，不存盘。
    ///
    /// `ShoulderRidingEntity.rideCooldownCounter`: the shoulder cooldown, not saved.
    pub ride_cooldown: i32,
    /// `flapSpeed`：翅膀音效的间隔用得上，不存盘。
    ///
    /// `flapSpeed`: used for the wing-sound interval, not saved.
    pub flap_speed: f32,
    /// `flapping`：扇翅幅度。
    ///
    /// `flapping`: the wing animation amount.
    pub flapping: f32,
    /// `flyDist + 1.0F` 的起点（`nextFlap = 1.0F`）。
    ///
    /// The starting `nextFlap` (`1.0F`).
    pub next_flap: f32,
    /// `partyParrot`：唱片机在旁边时为真，只影响客户端。
    ///
    /// `partyParrot`: true next to a playing jukebox; client-only.
    pub party: bool,
    /// `jukebox`：正在放音乐的唱片机位置。
    ///
    /// `jukebox`: the playing jukebox position.
    pub jukebox: Option<BlockPos>,
}

impl Default for Parrot {
    fn default() -> Self {
        Self {
            variant: DEFAULT_VARIANT,
            ride_cooldown: 0,
            flap_speed: 0.0,
            flapping: 1.0,
            next_flap: 1.0,
            party: false,
            jukebox: None,
        }
    }
}

impl Parrot {
    /// 品种的注册名。
    ///
    /// Registered name of this variant.
    pub fn variant_name(&self) -> &'static str {
        usize::try_from(self.variant)
            .ok()
            .and_then(|id| VARIANTS.get(id))
            .copied()
            .unwrap_or(VARIANTS[DEFAULT_VARIANT as usize])
    }
}

/// `EntityTypes.PARROT`：`sized(0.5F, 0.9F)`，眼高走默认的 `height * 0.85`。
///
/// `EntityTypes.PARROT`: `sized(0.5F, 0.9F)` with the default `height * 0.85` eye height.
pub(super) const fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Parrot.entity_type() as usize];
    (facts.width, facts.height, facts.height * 0.85)
}

/// `Parrot.createAttributes`：`MAX_HEALTH` 6、`FLYING_SPEED` 0.4、`MOVEMENT_SPEED` 0.2、
/// `ATTACK_DAMAGE` 3。
///
/// `Parrot.createAttributes`: `MAX_HEALTH` 6, `FLYING_SPEED` 0.4, `MOVEMENT_SPEED` 0.2
/// and `ATTACK_DAMAGE` 3.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 6.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.2_f32));
    table
        .entries
        .push(Attribute::new(attribute::FLYING_SPEED, f64::from(0.4_f32)));
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 3.0));
    table
}

/// `Parrot` 构造函数和 `createNavigation`：飞行导航、能浮水、打不开门，着火、着火邻格和
/// 可可豆的寻路代价都是 -1。
///
/// The `Parrot` constructor and `createNavigation`: flying navigation, floats, cannot open
/// doors, and fire, fire-in-neighbour and cocoa all cost -1.
pub(super) fn configure(mob: &mut Mob) {
    mob.navigation.flags.fly = true;
    mob.navigation.flags.can_float = true;
    mob.navigation.flags.can_open_doors = false;
    mob.malus
        .set(mc_bamboo_data::PathType::FireInNeighbor, -1.0);
    mob.malus.set(mc_bamboo_data::PathType::Fire, -1.0);
    mob.malus.set(mc_bamboo_data::PathType::Cocoa, -1.0);
}

// ---- 目标 ----

/// 鹦鹉自己的目标：`Parrot.ParrotWanderGoal`（`WaterAvoidingRandomFlyingGoal` 的子类）。
///
/// The parrot's own goal: `Parrot.ParrotWanderGoal`, a `WaterAvoidingRandomFlyingGoal`.
#[derive(Debug, Clone, PartialEq)]
pub enum ParrotGoal {
    /// `WaterAvoidingRandomFlyingGoal` 加 `ParrotWanderGoal.getPosition`。
    ///
    /// `WaterAvoidingRandomFlyingGoal` with `ParrotWanderGoal.getPosition`.
    Wander {
        speed: f64,
        interval: i32,
        probability: f32,
        wanted: Vec3,
        force_trigger: bool,
    },
    /// `LandOnOwnersShoulderGoal`：目标本身会跑，但上肩的落地还没做（见 README）。
    ///
    /// `LandOnOwnersShoulderGoal`: the goal itself runs, but landing on the shoulder is
    /// not implemented yet (see the README).
    LandOnShoulder { sitting_on_shoulder: bool },
}

impl ParrotGoal {
    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Wander { .. } => &[Flag::Move],
            Self::LandOnShoulder { .. } => &[],
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Wander { .. } => "ParrotWanderGoal",
            Self::LandOnShoulder { .. } => "LandOnOwnersShoulderGoal",
        }
    }

    pub const fn every_tick(&self) -> bool {
        false
    }

    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        let view = ctx.blocks();
        match self {
            Self::Wander {
                interval,
                wanted,
                force_trigger,
                ..
            } => {
                if !mob.passengers.is_empty() {
                    return false;
                }
                if !*force_trigger {
                    if mob.no_action_time >= 100 {
                        return false;
                    }
                    if me.random.next_int_bounded(reduced_tick_delay(*interval)) != 0 {
                        return false;
                    }
                }
                let Some(found) = wander_position(me, mob, view) else {
                    return false;
                };
                *wanted = found;
                *force_trigger = false;
                true
            }
            Self::LandOnShoulder { .. } => {
                if mob.tame.ordered_to_sit || mob.parrot.ride_cooldown <= 100 {
                    return false;
                }
                let Some(owner) = tame::owner(mob, view) else {
                    return false;
                };
                // 玩家的飞行、水里和细雪状态还没进 `PlayerView`，这里只按已知的三项过滤。
                // Player flying / water / powder-snow state is not in `PlayerView` yet, so
                // only the three known conditions are checked.
                !owner.spectator && !me.in_water && !mob.in_powder_snow
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        _me: &mut Entity,
        mob: &mut Mob,
        _ctx: &dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Wander { .. } => !mob.navigation.is_done() && mob.passengers.is_empty(),
            Self::LandOnShoulder { .. } => true,
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::Wander { speed, wanted, .. } => {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
            }
            Self::LandOnShoulder {
                sitting_on_shoulder,
            } => *sitting_on_shoulder = false,
        }
    }

    pub(super) fn stop(&mut self, mob: &mut Mob) {
        match self {
            Self::Wander { .. } => mob.navigation.stop(),
            Self::LandOnShoulder { .. } => {}
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::Wander { .. } => {}
            Self::LandOnShoulder {
                sitting_on_shoulder,
            } => {
                if *sitting_on_shoulder || mob.tame.in_sitting_pose() || mob.tame.ordered_to_sit {
                    return;
                }
                let Some(owner) = tame::owner(mob, ctx.blocks()) else {
                    return;
                };
                if !me.bounding_box().intersects(owner_box(&owner)) {
                    return;
                }
                // 官方 `setEntityOnShoulder` 把鹦鹉存进玩家的 `ShoulderEntityLeft` / `Right`
                // 并 `discard()`。Bamboo 还没有玩家肩膀存档和同步，先不下车，避免鹦鹉凭空
                // 消失（见 README 的已知限制）。
                // Vanilla `setEntityOnShoulder` stores the parrot in the player's
                // `ShoulderEntityLeft` / `Right` and `discard()`s it. Bamboo has no player
                // shoulder storage or sync yet, so the parrot stays instead of vanishing
                // (see the README's known limits).
                *sitting_on_shoulder = false;
            }
        }
    }
}

/// 玩家碰撞箱：宽 0.6、高 1.8。
///
/// The player bounding box: 0.6 wide and 1.8 tall.
fn owner_box(owner: &crate::entity::ctx::PlayerView) -> mc_bamboo_core::Aabb {
    mc_bamboo_core::Aabb::new(
        owner.position.x - 0.3,
        owner.position.y,
        owner.position.z - 0.3,
        owner.position.x + 0.3,
        owner.position.y + 1.8,
        owner.position.z + 0.3,
    )
}

fn wrap(priority: i32, goal: Goal) -> super::goals::WrappedGoal {
    super::goals::WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Parrot.registerGoals`。
///
/// `Parrot.registerGoals`.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.fly = true;
    GoalSelector::from_goals(vec![
        wrap(
            0,
            Goal::Panic {
                speed: 1.25,
                target: Vec3::default(),
            },
        ),
        wrap(0, Goal::Float),
        wrap(
            1,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(2, Goal::Tame(TameGoal::Sit)),
        wrap(2, Goal::Tame(TameGoal::follow_owner(1.0, 5.0, 1.0))),
        wrap(
            2,
            Goal::Parrot(ParrotGoal::Wander {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            }),
        ),
        wrap(
            3,
            Goal::Parrot(ParrotGoal::LandOnShoulder {
                sitting_on_shoulder: false,
            }),
        ),
        wrap(3, Goal::Unavailable("FollowMobGoal")),
    ])
}

/// `ParrotWanderGoal.getPosition`，找不到时退回
/// `WaterAvoidingRandomFlyingGoal.getPosition`。
///
/// `ParrotWanderGoal.getPosition`, falling back to
/// `WaterAvoidingRandomFlyingGoal.getPosition`.
fn wander_position(me: &mut Entity, mob: &Mob, view: &dyn BlockView) -> Option<Vec3> {
    let mut pos = None;
    if me.in_water {
        pos = land_random_pos(me, mob, view, 15, 15);
    }
    if me.random.next_float() >= 0.001 {
        pos = tree_pos(me, mob, view);
    }
    pos.or_else(|| flying_wander_position(me, mob, view))
}
/// `WaterAvoidingRandomFlyingGoal.getPosition`：先试悬停点，再试空中 / 水里。
///
/// `WaterAvoidingRandomFlyingGoal.getPosition`: hover first, then air or water.
fn flying_wander_position(me: &mut Entity, mob: &Mob, view: &dyn BlockView) -> Option<Vec3> {
    // `Vec3.directionFromRotation(xRot, yRot)`，全程 f32。
    // `Vec3.directionFromRotation(xRot, yRot)`, all in f32.
    let scale = std::f32::consts::PI / 180.0;
    let yaw = -mob.y_rot_o * scale - std::f32::consts::PI;
    let pitch = -mob.x_rot_o * scale;
    let f = mth::cos(f64::from(yaw));
    let f1 = mth::sin(f64::from(yaw));
    let f2 = -mth::cos(f64::from(pitch));
    let f3 = mth::sin(f64::from(pitch));
    let dir = Vec3::new(f64::from(f1 * f2), f64::from(f3), f64::from(f * f2));
    let hover_only = hover_random_pos(
        me,
        mob,
        view,
        8,
        7,
        dir.x,
        dir.z,
        f64::from(std::f32::consts::FRAC_PI_2),
        3,
        1,
    );
    hover_only.or_else(|| {
        air_and_water_random_pos(
            me,
            mob,
            view,
            8,
            4,
            -2,
            dir.x,
            dir.z,
            f64::from(std::f32::consts::FRAC_PI_2),
        )
    })
}

/// `ParrotWanderGoal.getTreePos`：在周围 ±3 / ±6 / ±3 的格子里找树叶或原木上方的空位。
/// 官方的 `betweenClosed` 以 x 最快、y 次之、z 最慢的顺序遍历。
///
/// `ParrotWanderGoal.getTreePos`: an empty cell above leaves or logs within ±3 / ±6 / ±3.
/// Vanilla's `betweenClosed` walks x fastest, then y, then z.
fn tree_pos(me: &Entity, mob: &Mob, view: &dyn BlockView) -> Option<Vec3> {
    let mob_pos = me.block_position();
    let (min_x, max_x) = (
        mth::floor(me.position.x - 3.0),
        mth::floor(me.position.x + 3.0),
    );
    let (min_y, max_y) = (
        mth::floor(me.position.y - 6.0),
        mth::floor(me.position.y + 6.0),
    );
    let (min_z, max_z) = (
        mth::floor(me.position.z - 3.0),
        mth::floor(me.position.z + 3.0),
    );
    let _ = mob;
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let pos = BlockPos::new(x, y, z);
                if mob_pos == pos {
                    continue;
                }
                let below = view.block_state(pos.below());
                let can_sit_on =
                    below.is_a(BlockAncestor::LeavesBlock) || below.is_tagged(block_tags::LOGS);
                if can_sit_on
                    && view.block_state(pos).is_air()
                    && view.block_state(pos.above()).is_air()
                {
                    return Some(Vec3::new(
                        f64::from(x) + 0.5,
                        f64::from(y),
                        f64::from(z) + 0.5,
                    ));
                }
            }
        }
    }
    None
}

// ---- 飞行的移动控制 ----

/// `FlyingMoveControl(this, 10, false)`。
///
/// `FlyingMoveControl(this, 10, false)`.
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if mob.mover.operation == super::control::Operation::MoveTo {
        mob.mover.operation = super::control::Operation::Wait;
        mob.no_gravity = true;
        let dx = mob.mover.wanted_x - me.position.x;
        let dz = mob.mover.wanted_z - me.position.z;
        let dy = mob.mover.wanted_y - me.position.y;
        let distance = dx * dx + dy * dy + dz * dz;
        if distance < 2.500_000_3e-7 {
            mob.yya = 0.0;
            mob.zza = 0.0;
            return;
        }
        let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
        me.y_rot = super::control::move_rotlerp(me.y_rot, yaw, 90.0);
        let speed = if me.on_ground {
            (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32
        } else {
            (mob.mover.speed_modifier * mob.attributes.value(attribute::FLYING_SPEED)) as f32
        };
        living::set_speed(mob, speed);
        let horizontal = (dx * dx + dz * dz).sqrt();
        if dy.abs() > f64::from(1.0e-5_f32) || horizontal.abs() > f64::from(1.0e-5_f32) {
            let pitch = (-(mth::atan2(dy, horizontal) * 57.295_776_367_187_5)) as f32;
            mob.x_rot = super::control::move_rotlerp(mob.x_rot, pitch, 10.0);
            mob.yya = if dy > 0.0 { speed } else { -speed };
        }
    } else {
        mob.no_gravity = false;
        mob.yya = 0.0;
        mob.zza = 0.0;
    }
}

// ---- aiStep ----

/// `Parrot.aiStep` 在 `super.aiStep` 之前的部分：唱片机和模仿附近生物。
///
/// The part of `Parrot.aiStep` before `super.aiStep`: the jukebox and imitating nearby mobs.
pub(super) fn before_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    let keep = mob.parrot.jukebox.is_some_and(|pos| {
        let centre = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        centre.distance_to_sqr(me.position) < 3.46 * 3.46
            && view.block_state(pos).is(blocks::JUKEBOX)
    });
    if !keep {
        mob.parrot.party = false;
        mob.parrot.jukebox = None;
    }
    if view.level_random_int(400) == 0 {
        imitate_nearby_mobs(me, mob, ctx);
    }
}

/// `Parrot.aiStep` 在 `super.aiStep` 之后的 `calculateFlapping`。
///
/// `calculateFlapping` at the end of `Parrot.aiStep`.
pub(super) fn after_ai_step(me: &mut Entity, mob: &mut Mob) {
    let airborne = !me.on_ground && mob.passengers.is_empty();
    let parrot = &mut mob.parrot;
    parrot.flap_speed += if airborne { 4.0 } else { -1.0 } * 0.3;
    parrot.flap_speed = mth::clamp_f32(parrot.flap_speed, 0.0, 1.0);
    if airborne && parrot.flapping < 1.0 {
        parrot.flapping = 1.0;
    }
    parrot.flapping *= 0.9;
    if airborne && me.delta.y < 0.0 {
        let delta = me.delta;
        me.delta = delta.multiply(1.0, 0.6, 1.0);
    }
}

/// `Parrot.imitateNearbyMobs`：`nextInt(2)` 后取一只 20 格内的生物，照它的类型放模仿音。
///
/// `Parrot.imitateNearbyMobs`: after a `nextInt(2)` roll, pick a mob within twenty blocks
/// and play the sound it imitates.
fn imitate_nearby_mobs(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.dead || mob.silent || ctx.blocks().level_random_int(2) != 0 {
        return;
    }
    let area = me.bounding_box().inflate(20.0, 20.0, 20.0);
    let mut candidates: Vec<(MobType, bool)> = Vec::new();
    // 官方的 `getEntitiesOfClass(Mob.class, ...)` 会连鹦鹉自己也收进来，但谓词要求
    // 类型在表里；鹦鹉不在表里，所以自然被排除。
    // `getEntitiesOfClass` includes the parrot itself, but the predicate requires a
    // type in the table, so parrots never qualify.
    ctx.for_each_other_in(area, &mut |public| {
        if let crate::entity::KindPublic::Mob(public_mob) = &public.kind
            && MOB_SOUNDS
                .iter()
                .any(|(name, _)| *name == public_mob.mob_type.name())
        {
            candidates.push((public_mob.mob_type, public_mob.silent));
        }
    });
    if candidates.is_empty() {
        return;
    }
    let index = ctx.blocks().level_random_int(candidates.len() as i32) as usize;
    let (kind, silent) = candidates[index];
    if silent {
        return;
    }
    let sound = imitated_sound(kind);
    let first = ctx.blocks().level_random_float();
    let second = ctx.blocks().level_random_float();
    living::play_sound(me, mob, sound, 0.7, pitch_of(first, second));
}

/// `Parrot.getImitatedSound`。
///
/// `Parrot.getImitatedSound`.
fn imitated_sound(kind: MobType) -> &'static str {
    MOB_SOUNDS
        .iter()
        .find(|(name, _)| *name == kind.name())
        .map_or("minecraft:entity.parrot.ambient", |(_, sound)| *sound)
}

/// `Parrot.getPitch`。
///
/// `Parrot.getPitch`.
fn pitch_of(first: f32, second: f32) -> f32 {
    (first - second) * 0.2 + 1.0
}

// ---- 叫声 ----

/// `Parrot.getAmbientSound`：和平难度以外，1000 次里有一次从表里挑一个模仿音。
/// 这里的随机源是**世界**随机源（`level.getRandom()`）。
///
/// `Parrot.getAmbientSound`: outside peaceful, one roll in a thousand picks an imitated
/// sound from the table. The random source is the **level** random.
pub(super) fn ambient_sound(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    let sound = if view.difficulty() != 0 && view.level_random_int(1000) == 0 {
        let index = view.level_random_int(MOB_SOUNDS.len() as i32) as usize;
        MOB_SOUNDS[index].1
    } else {
        "minecraft:entity.parrot.ambient"
    };
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(me, mob, sound, mob.mob_type.sound_volume(), pitch);
}

// ---- 生成 ----

/// `Parrot.checkParrotSpawnRules`：脚下在 `#parrots_spawnable_on` 里且亮度足够。
///
/// `Parrot.checkParrotSpawnRules`: the block below is in `#parrots_spawnable_on` and the
/// light is bright enough.
pub(super) fn spawn_rules(world: &crate::world::World, pos: BlockPos) -> bool {
    super::natural::bright_spawn_rules(world, pos, block_tags::PARROTS_SPAWNABLE_ON)
}

/// `Parrot.finalizeSpawn`：先抽品种，再用 `AgeableMobGroupData(false)` 建组（不抽幼崽），
/// 最后走 `Mob.finalizeSpawn` 的跟随距离和左撇子。
///
/// `Parrot.finalizeSpawn`: the variant first, then an `AgeableMobGroupData(false)` group
/// (no baby roll), then `Mob.finalizeSpawn`'s follow range and left-handedness.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<super::spawn::SpawnGroup>,
) {
    let variant = random.next_int(VARIANTS.len() as i32);
    if !matches!(group, Some(super::spawn::SpawnGroup::Ageable(_))) {
        *group = Some(super::spawn::SpawnGroup::Ageable(
            super::spawn::AgeableGroupData {
                group_size: 0,
                should_spawn_baby: false,
                baby_spawn_chance: 0.0,
            },
        ));
    }
    let Some(super::spawn::SpawnGroup::Ageable(data)) = group else {
        unreachable!("parrot spawn group was just initialized");
    };
    // `AgeableMobGroupData(false)`：`shouldSpawnBaby` 为假，不抽幼崽，只把组大小加一。
    // `AgeableMobGroupData(false)`: no baby roll, only the group size goes up.
    data.group_size += 1;
    entity.with_mob(|_, mob| {
        mob.parrot.variant = variant;
        super::spawn::finalize_mob(random, mob);
    });
}

// ---- 交互 ----

/// `Parrot.mobInteract`：种子有 1/10 概率驯服，曲奇中毒并可能直接死亡，驯服后主人切换
/// 坐下。鹦鹉没有 `BreedGoal`，`canMate` 与 `isFood` 恒假。
///
/// `Parrot.mobInteract`: seeds tame with a 1-in-10 chance, cookies poison and may kill,
/// and a tamed parrot's owner toggles sitting. Parrots have no `BreedGoal`; `canMate`
/// and `isFood` are always false.
pub(super) fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    let held = &req.held;
    let consume = !req.creative;
    if !mob.tame.is_tame() && is_tagged(held, item_tags::PARROT_FOOD) {
        if !mob.silent {
            // `getPitch(this.random)`：实体的随机源取两次 `nextFloat`。
            let first = me.random.next_float();
            let second = me.random.next_float();
            living::play_sound(
                me,
                mob,
                "minecraft:entity.parrot.eat",
                1.0,
                pitch_of(first, second),
            );
        }
        if me.random.next_int_bounded(10) == 0 {
            tame::tame(mob, req.player_uuid);
            mob.events.push(MobEvent::EntityEvent { event: 7 });
        } else {
            mob.events.push(MobEvent::EntityEvent { event: 6 });
        }
        return outcome(InteractionResult::Success, consume);
    }
    if is_tagged(held, item_tags::PARROT_POISONOUS_FOOD) {
        // `addEffect(POISON, 900)` 然后 `hurt(playerAttack, Float.MAX_VALUE)`；伤害由调用方
        // 在把实体放回世界之后按 [`MobInteraction::self_hurt`] 结算。
        // `addEffect(POISON, 900)` then `hurt(playerAttack, Float.MAX_VALUE)`; the caller
        // resolves the damage from [`MobInteraction::self_hurt`] once the mob is back.
        living::add_effect(
            mob,
            super::effects::EffectInstance::new(
                mc_bamboo_data::registries::mob_effect::POISON,
                900,
            ),
        );
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: consume,
            self_hurt: Some(f32::MAX),
            ..MobInteraction::pass()
        };
    }
    if me.on_ground && mob.tame.is_tame() && mob.tame.is_owned_by(Some(req.player_uuid)) {
        mob.tame.ordered_to_sit = !mob.tame.ordered_to_sit;
        return outcome(InteractionResult::Success, false);
    }
    super::interact::animal_interact(me, mob, req)
}

fn is_tagged(stack: &ItemStack, tag: mc_bamboo_data::ItemTag) -> bool {
    !stack.is_empty() && item_is_tagged(stack.item, tag)
}

fn outcome(result: InteractionResult, shrink_held: bool) -> MobInteraction {
    MobInteraction {
        result,
        shrink_held,
        ..MobInteraction::pass()
    }
}

/// `Parrot.addAdditionalSaveData`：写 `"Variant"`（旧版整数编码）。
///
/// `Parrot.addAdditionalSaveData`: writes `"Variant"` (the legacy integer codec).
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push(("Variant".to_owned(), Tag::Int(mob.parrot.variant)));
}

/// `Parrot.readAdditionalSaveData`：读 `"Variant"`，越界夹到最近的品种。
///
/// `Parrot.readAdditionalSaveData`: reads `"Variant"`, clamping out-of-range values.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    use crate::archive::tag;
    mob.parrot.variant = match tag::field(tag, "Variant") {
        Some(Tag::Int(id)) => (*id).clamp(0, VARIANTS.len() as i32 - 1),
        _ => DEFAULT_VARIANT,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_attributes_and_dimensions_match_vanilla() {
        assert_eq!(VARIANTS[0], "red_blue");
        assert_eq!(VARIANTS[4], "gray");
        assert_eq!(dimensions(), (0.5, 0.9, 0.9 * 0.85));
        let attrs = attributes();
        assert_eq!(attrs.value(attribute::MAX_HEALTH), 6.0);
        assert_eq!(attrs.value(attribute::FLYING_SPEED), f64::from(0.4_f32));
        assert_eq!(attrs.value(attribute::MOVEMENT_SPEED), f64::from(0.2_f32));
        assert_eq!(attrs.value(attribute::ATTACK_DAMAGE), 3.0);
        assert_eq!(Parrot::default().variant_name(), "red_blue");
    }

    #[test]
    fn save_data_round_trips_the_variant() {
        let mut mob = Mob::new(MobType::Parrot);
        mob.parrot.variant = 2;
        let mut entries = Vec::new();
        write_nbt(&mob, &mut entries);
        let mut loaded = Mob::new(MobType::Parrot);
        read_nbt(&mut loaded, &Tag::Compound(entries));
        assert_eq!(loaded.parrot.variant, 2);
        assert_eq!(loaded.parrot.variant_name(), "green");
    }
}
