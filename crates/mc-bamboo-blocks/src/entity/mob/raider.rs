//! 官方 `PatrollingMonster` → `Raider` 这两层的公共部分：巡逻状态、存档键、
//! `LongDistancePatrolGoal`、`Raider.HoldGroundAttackGoal`、看别的生物的
//! `LookAtPlayerGoal(Mob.class)`，以及队长头上的灾厄旗帜。女巫和掠夺者共用。
//!
//! Bamboo 没有袭击系统：`Wave` / `CanJoinRaid` / `RaidId` 只当存档数据保存，
//! `getCurrentRaid()` 恒为空，`hasActiveRaid()` 恒为假。
//!
//! The shared part of vanilla `PatrollingMonster` → `Raider`: patrol state, save keys,
//! `LongDistancePatrolGoal`, `Raider.HoldGroundAttackGoal`, the mob-watching
//! `LookAtPlayerGoal(Mob.class)` and the ominous banner on a leader's head. Witches and
//! pillagers share it. Bamboo has no raids: `Wave` / `CanJoinRaid` / `RaidId` are stored
//! data only, `getCurrentRaid()` is always null and `hasActiveRaid()` always false.

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};
use mc_bamboo_data::{items, registries::banner_pattern, tags::block_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    goals::{self, Flag},
    living,
};
use crate::{
    World,
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{BlockView, Command, EntityCtx},
    },
};

/// `PatrollingMonster` 和 `Raider` 的字段。
///
/// The fields of `PatrollingMonster` and `Raider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Raider {
    /// `patrolTarget`。
    pub patrol_target: Option<BlockPos>,
    /// `patrolLeader`。
    pub patrol_leader: bool,
    /// `patrolling`。
    pub patrolling: bool,
    /// `Raider.wave`：只当存档数据。
    ///
    /// `Raider.wave`: stored data only.
    pub wave: i32,
    /// `Raider.canJoinRaid`：只当存档数据。
    ///
    /// `Raider.canJoinRaid`: stored data only.
    pub can_join_raid: bool,
    /// 存档里的 `RaidId`：官方读盘时拿它找袭击；这里原样保存。
    ///
    /// The saved `RaidId`: vanilla resolves the raid from it on load; kept verbatim here.
    pub raid_id: Option<i32>,
}

impl Raider {
    /// `setPatrolTarget`：设目标并开始巡逻。
    ///
    /// `setPatrolTarget`: set the target and start patrolling.
    pub(crate) const fn set_patrol_target(&mut self, target: BlockPos) {
        self.patrol_target = Some(target);
        self.patrolling = true;
    }

    /// `PatrollingMonster.addAdditionalSaveData` 加 `Raider.addAdditionalSaveData`。
    ///
    /// `PatrollingMonster.addAdditionalSaveData` plus `Raider.addAdditionalSaveData`.
    pub(super) fn save(&self, out: &mut Vec<(String, Tag)>) {
        if let Some(pos) = self.patrol_target {
            out.push((
                "patrol_target".to_owned(),
                Tag::IntArray(vec![pos.x, pos.y, pos.z]),
            ));
        }
        out.push((
            "PatrolLeader".to_owned(),
            Tag::Byte(i8::from(self.patrol_leader)),
        ));
        out.push((
            "Patrolling".to_owned(),
            Tag::Byte(i8::from(self.patrolling)),
        ));
        out.push(("Wave".to_owned(), Tag::Int(self.wave)));
        out.push((
            "CanJoinRaid".to_owned(),
            Tag::Byte(i8::from(self.can_join_raid)),
        ));
        if let Some(id) = self.raid_id {
            out.push(("RaidId".to_owned(), Tag::Int(id)));
        }
    }

    /// 两层的 `readAdditionalSaveData`。
    ///
    /// The two `readAdditionalSaveData` layers.
    pub(super) fn load(tag: &Tag) -> Self {
        use crate::archive::tag;
        let target = tag::int_array(tag, "patrol_target");
        Self {
            patrol_target: (target.len() == 3)
                .then(|| BlockPos::new(target[0], target[1], target[2])),
            patrol_leader: tag::bool_or(tag, "PatrolLeader", false),
            patrolling: tag::bool_or(tag, "Patrolling", false),
            wave: tag::int_or(tag, "Wave", 0),
            can_join_raid: tag::bool_or(tag, "CanJoinRaid", false),
            raid_id: match tag::field(tag, "RaidId") {
                Some(Tag::Int(id)) => Some(*id),
                _ => None,
            },
        }
    }
}

/// `PatrollingMonster.removeWhenFarAway`：没在巡逻、或离最近玩家超过 128 格才会消失。
///
/// `PatrollingMonster.removeWhenFarAway`: only when not patrolling, or more than 128
/// blocks from the nearest player.
pub(super) fn removes_when_far_away(mob: &Mob, distance_sqr: f64) -> bool {
    !mob.raider.patrolling || distance_sqr > 16384.0
}

/// `PatrollingMonster.findPatrolTarget`：自己随机源取两次 `nextInt(1000)`。
///
/// `PatrollingMonster.findPatrolTarget`: two `nextInt(1000)` draws from the mob's random.
pub(crate) fn find_patrol_target(me: &mut Entity, mob: &mut Mob) {
    let at = me.block_position();
    let x = -500 + me.random.next_int_bounded(1000);
    let z = -500 + me.random.next_int_bounded(1000);
    mob.raider
        .set_patrol_target(BlockPos::new(at.x + x, at.y, at.z + z));
}

/// `PatrollingMonster.finalizeSpawn` 在 `Mob.finalizeSpawn` 之前的那一段：6% 当队长
/// （巡逻、事件、结构生成不抽），队长戴灾厄旗帜且必掉，巡逻生成的开始巡逻。
///
/// The part of `PatrollingMonster.finalizeSpawn` before `Mob.finalizeSpawn`: a 6% leader
/// roll (skipped for patrol, event and structure spawns), the always-dropped ominous
/// banner on a leader, and patrolling for patrol spawns.
pub(super) fn finalize_patrol(
    mob: &mut Mob,
    random: &dyn crate::random::RandomSource,
    patrol_spawn: bool,
    can_be_leader: bool,
) {
    if !patrol_spawn && random.next_float() < 0.06 && can_be_leader {
        mob.raider.patrol_leader = true;
    }
    if mob.raider.patrol_leader {
        mob.equipment.set(EquipmentSlot::Head, ominous_banner());
        mob.equipment.set_drop_chance(EquipmentSlot::Head, 2.0);
    }
    if patrol_spawn {
        mob.raider.patrolling = true;
    }
}

/// `Raider.isCaptain()`：是巡逻队长，并且头上那件和灾厄旗帜 `ItemStack.matches`。
///
/// `Raider.isCaptain()`: a patrol leader whose head item `ItemStack.matches` the ominous
/// banner.
pub(crate) fn is_captain(mob: &Mob) -> bool {
    mob.raider.patrol_leader
        && mob
            .equipment
            .get(EquipmentSlot::Head)
            .matches(&ominous_banner())
}

/// `Raid.getOminousBannerInstance`：白旗加八层图案、隐藏图案提示、物品名和稀有度。
///
/// `Raid.getOminousBannerInstance`: a white banner with eight pattern layers, the hidden
/// pattern tooltip, the item name and the rarity.
pub fn ominous_banner() -> ItemStack {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{
        ComponentPatch, ComponentValue, PatchEntry, cursor::Holder, value::BannerLayer,
    };
    // `DyeColor` 的 ID：灰 7、淡灰 8、青 9、黑 15。
    // `DyeColor` ids: gray 7, light gray 8, cyan 9, black 15.
    const GRAY: i32 = 7;
    const LIGHT_GRAY: i32 = 8;
    const CYAN: i32 = 9;
    const BLACK: i32 = 15;
    let layers = [
        (banner_pattern::RHOMBUS, CYAN),
        (banner_pattern::STRIPE_BOTTOM, LIGHT_GRAY),
        (banner_pattern::STRIPE_CENTER, GRAY),
        (banner_pattern::BORDER, LIGHT_GRAY),
        (banner_pattern::STRIPE_MIDDLE, BLACK),
        (banner_pattern::HALF_HORIZONTAL, LIGHT_GRAY),
        (banner_pattern::CIRCLE, LIGHT_GRAY),
        (banner_pattern::BORDER, BLACK),
    ]
    .into_iter()
    .map(|(pattern, color)| BannerLayer {
        pattern: Holder::Id(pattern),
        color,
    })
    .collect();
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::BANNER_PATTERNS,
        PatchEntry::Set(ComponentValue::BannerPatterns(layers)),
    );
    patch.put(
        kinds::TOOLTIP_DISPLAY,
        PatchEntry::Set(ComponentValue::TooltipDisplay {
            hide_tooltip: false,
            hidden_components: vec![kinds::BANNER_PATTERNS],
        }),
    );
    patch.put(
        kinds::ITEM_NAME,
        PatchEntry::Set(ComponentValue::ItemName(
            mc_bamboo_protocol::text::Component::translate("block.minecraft.ominous_banner", []),
        )),
    );
    // `Rarity.UNCOMMON` 的 ID 是 1。
    // `Rarity.UNCOMMON` has id 1.
    patch.put(kinds::RARITY, PatchEntry::Set(ComponentValue::Rarity(1)));
    let (added, removed) = patch.to_opaque();
    ItemStack {
        added,
        removed,
        ..ItemStack::new(items::WHITE_BANNER, 1)
    }
}

/// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`：这一列最高的「挡运动或含
/// 流体、且不是树叶」的方块上面一格；整列都没有时是世界底部。逐格往下扫，和高度图的
/// 定义等价。
///
/// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`: one above the highest block
/// that blocks motion or holds fluid and is not leaves, or the world bottom for an empty
/// column. Scans downward, which is equivalent to the heightmap's definition.
pub(crate) fn no_leaves_height(
    state_at: impl Fn(BlockPos) -> crate::BlockState,
    min_y: i32,
    max_y: i32,
    x: i32,
    z: i32,
) -> i32 {
    for y in (min_y..=max_y).rev() {
        let state = state_at(BlockPos::new(x, y, z));
        // `MOTION_BLOCKING_NO_LEAVES` 的谓词就是这条标签加流体，标签里已经排除了树叶。
        // The `MOTION_BLOCKING_NO_LEAVES` predicate is this tag plus fluids; the tag
        // already excludes leaves.
        if state.is_tagged(block_tags::BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES)
            || !state.fluid().is_empty()
        {
            return y + 1;
        }
    }
    min_y
}

fn view_no_leaves_height(view: &dyn BlockView, x: i32, z: i32) -> i32 {
    no_leaves_height(
        |pos| view.block_state(pos),
        view.min_y(),
        view.max_y(),
        x,
        z,
    )
}

/// 掠夺者一支自己的目标。
///
/// The goals of the raider branch.
#[derive(Debug, Clone, PartialEq)]
pub enum RaiderGoal {
    /// `PatrollingMonster.LongDistancePatrolGoal(mob, speed, leaderSpeed)`。
    LongDistancePatrol {
        speed: f64,
        leader_speed: f64,
        cooldown_until: i64,
    },
    /// `Raider.HoldGroundAttackGoal(mob, hostileRadius)`。
    HoldGround { hostile_radius_sqr: f32 },
    /// `RangedCrossbowAttackGoal`。
    Crossbow(super::crossbow::CrossbowAttack),
    /// `LookAtPlayerGoal(mob, Mob.class, lookDistance)`：看最近的别的生物。
    ///
    /// `LookAtPlayerGoal(mob, Mob.class, lookDistance)`: watch the nearest other mob.
    LookAtMob {
        look_distance: f32,
        probability: f32,
        look_at: Option<EntityUid>,
        look_time: i32,
    },
}

impl RaiderGoal {
    /// `LongDistancePatrolGoal(mob, 0.7, 0.595)`，`PatrollingMonster.registerGoals` 注册的那个。
    ///
    /// `LongDistancePatrolGoal(mob, 0.7, 0.595)` as `PatrollingMonster.registerGoals` adds it.
    pub(super) const fn patrol() -> Self {
        Self::LongDistancePatrol {
            speed: 0.7,
            leader_speed: 0.595,
            cooldown_until: -1,
        }
    }

    pub(super) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::LongDistancePatrol { .. } => &[Flag::Move],
            Self::HoldGround { .. } | Self::Crossbow(_) => &[Flag::Move, Flag::Look],
            Self::LookAtMob { .. } => &[Flag::Look],
        }
    }

    /// `requiresUpdateEveryTick`：守阵和弩两个为真。
    ///
    /// `requiresUpdateEveryTick`: true for hold-ground and the crossbow goal.
    pub(super) const fn every_tick(&self) -> bool {
        matches!(self, Self::HoldGround { .. } | Self::Crossbow(_))
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::LongDistancePatrol { .. } => "LongDistancePatrolGoal",
            Self::HoldGround { .. } => "HoldGroundAttackGoal",
            Self::Crossbow(_) => "RangedCrossbowAttackGoal",
            Self::LookAtMob { .. } => "LookAtPlayerGoal",
        }
    }

    pub(super) fn can_use(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::LongDistancePatrol { cooldown_until, .. } => {
                let on_cooldown = ctx.blocks().game_time() < *cooldown_until;
                mob.raider.patrolling
                    && goals::current_combat_target(mob, ctx).is_none()
                    && mob.controlling_passenger().is_none()
                    && mob.raider.patrol_target.is_some()
                    && !on_cooldown
            }
            Self::HoldGround { .. } => hold_ground_can_use(mob, ctx),
            Self::Crossbow(_) => super::crossbow::CrossbowAttack::can_use(mob, ctx),
            Self::LookAtMob {
                look_distance,
                probability,
                look_at,
                ..
            } => {
                if me.random.next_float() >= *probability {
                    return false;
                }
                *look_at = nearest_mob(me, mob, ctx, f64::from(*look_distance));
                look_at.is_some()
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::LookAtMob {
                look_distance,
                look_at,
                look_time,
                ..
            } => {
                let Some((target, alive)) = look_at.and_then(|uid| goals::target_view(ctx, uid))
                else {
                    return false;
                };
                let limit = f64::from(*look_distance * *look_distance);
                alive && me.position.distance_to_sqr(target.position) <= limit && *look_time > 0
            }
            _ => self.can_use(me, mob, ctx),
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::LongDistancePatrol { .. } | Self::Crossbow(_) => {}
            Self::HoldGround { .. } => {
                mob.navigation.stop();
                shout(me, mob, ctx, false);
            }
            Self::LookAtMob { look_time, .. } => {
                *look_time = goals::reduced_tick_delay(40 + me.random.next_int_bounded(40));
            }
        }
    }

    pub(super) fn stop(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::LongDistancePatrol { .. } => {}
            Self::HoldGround { .. } => {
                if goals::current_combat_target(mob, ctx).is_some() {
                    shout(me, mob, ctx, true);
                    mob.aggressive = true;
                }
            }
            Self::Crossbow(goal) => goal.stop(mob),
            Self::LookAtMob { look_at, .. } => *look_at = None,
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::LongDistancePatrol {
                speed,
                leader_speed,
                cooldown_until,
            } => patrol_tick(me, mob, ctx, *speed, *leader_speed, cooldown_until),
            Self::HoldGround { hostile_radius_sqr } => {
                hold_ground_tick(me, mob, ctx, *hostile_radius_sqr);
            }
            Self::Crossbow(goal) => goal.tick(me, mob, ctx),
            Self::LookAtMob {
                look_at, look_time, ..
            } => {
                let Some((target, alive)) = look_at.and_then(|uid| goals::target_view(ctx, uid))
                else {
                    return;
                };
                if alive {
                    mob.look.set_look_at(
                        target.position.x,
                        target.eye_y(),
                        target.position.z,
                        super::control::HEAD_ROT_SPEED as f32,
                        goals::max_head_x_rot(mob),
                    );
                    *look_time -= 1;
                }
            }
        }
    }
}

/// `HoldGroundAttackGoal.canUse`：没在袭击里、在巡逻、有目标、还没进攻击状态，并且上次打它的
/// 不是玩家。
///
/// `HoldGroundAttackGoal.canUse`: no raid, patrolling, a target, not yet aggressive, and
/// not last hurt by a player.
fn hold_ground_can_use(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    mob.raider.patrolling
        && goals::current_combat_target(mob, ctx).is_some()
        && !mob.aggressive
        && mob
            .last_hurt_by_mob
            .is_none_or(|uid| goals::player_by_uid(ctx.blocks(), uid).is_none())
}

/// `HoldGroundAttackGoal.tick`：目标在敌对半径外就盯着它、五十分之一叫一声；进了半径就进攻。
///
/// `HoldGroundAttackGoal.tick`: outside the hostile radius stare at the target and grunt
/// one tick in fifty; inside it turn aggressive.
fn hold_ground_tick(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx, radius_sqr: f32) {
    let Some(target) = goals::current_combat_target(mob, ctx) else {
        return;
    };
    if me.position.distance_to_sqr(target.position) > f64::from(radius_sqr) {
        mob.look.set_look_at(
            target.position.x,
            target.eye_y(),
            target.position.z,
            30.0,
            30.0,
        );
        if me.random.next_int_bounded(50) == 0 {
            living::play_ambient_sound(me, mob);
        }
    } else {
        mob.aggressive = true;
    }
}

/// `HoldGroundAttackGoal.start` / `stop` 里的喊话：`shoutTargeting`（非战斗、8 格、不看视线和
/// 隐身）选中的同伴拿到自己的目标，`stop` 时同伴还进攻击状态。
///
/// The shout of `HoldGroundAttackGoal.start` / `stop`: raiders picked by `shoutTargeting`
/// (non-combat, 8 blocks, no sight or invisibility test) take this mob's target, and on
/// `stop` they also turn aggressive.
fn shout(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx, aggressive: bool) {
    let Some(target) = mob.target else {
        return;
    };
    let mut others = Vec::new();
    ctx.for_each_other_in(me.bounding_box().inflate(8.0, 8.0, 8.0), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type.is_raider()
            && !other.removed
            && public.is_alive()
            && me.position.distance_to_sqr(other.position) <= 64.0
        {
            others.push(other.uid);
        }
    });
    if !others.is_empty() {
        ctx.emit(
            Command::RaiderShout {
                others,
                target,
                aggressive,
            },
            me,
        );
    }
}

/// 落地 [`Command::RaiderShout`]：`entity.setTarget(target)`，`stop` 那次再
/// `setAggressive(true)`。
///
/// Applies [`Command::RaiderShout`]: `entity.setTarget(target)`, plus
/// `setAggressive(true)` for the `stop` call.
pub(crate) fn apply_shout(
    world: &mut World,
    others: &[EntityUid],
    target: EntityUid,
    aggressive: bool,
) {
    for &other in others {
        world.with_entity_mut(other, &mut |entity| {
            if let Some(mob) = entity.kind.mob_mut() {
                mob.target = Some(target);
                if aggressive {
                    mob.aggressive = true;
                }
            }
        });
    }
}

/// `LongDistancePatrolGoal.findPatrolCompanions`：16 格盒子里别的 `PatrollingMonster`
/// （`canJoinPatrol()` 没有袭击时恒真）。
///
/// `LongDistancePatrolGoal.findPatrolCompanions`: the other `PatrollingMonster`s in the
/// 16-block box (`canJoinPatrol()` is always true without raids).
fn patrol_companions(me: &Entity, ctx: &dyn EntityCtx) -> Vec<EntityUid> {
    let mut others = Vec::new();
    ctx.for_each_other_in(me.bounding_box().inflate(16.0, 16.0, 16.0), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type.is_raider()
            && !other.removed
        {
            others.push(other.uid);
        }
    });
    others
}

/// `LongDistancePatrolGoal.tick`。
fn patrol_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    speed: f64,
    leader_speed: f64,
    cooldown_until: &mut i64,
) {
    if !mob.navigation.is_done() {
        return;
    }
    let leader = mob.raider.patrol_leader;
    let companions = patrol_companions(me, ctx);
    let Some(target) = mob.raider.patrol_target else {
        return;
    };
    if mob.raider.patrolling && companions.is_empty() {
        mob.raider.patrolling = false;
    } else if leader && closer_to_center_than(target, me.position, 10.0) {
        find_patrol_target(me, mob);
    } else {
        let path_target = patrol_step(ctx.blocks(), me.position, target);
        let nav = living::nav_mob(me, mob, ctx.blocks());
        let at = Vec3::new(
            f64::from(path_target.x),
            f64::from(path_target.y),
            f64::from(path_target.z),
        );
        let moved = mob.navigation.move_to(
            ctx.blocks(),
            &nav,
            at,
            if leader { leader_speed } else { speed },
        );
        if !moved {
            move_randomly(me, mob, ctx.blocks(), speed);
            *cooldown_until = ctx.blocks().game_time() + 200;
        } else if leader {
            ctx.emit(
                Command::SetPatrolTarget {
                    others: companions,
                    target: path_target,
                },
                me,
            );
        }
    }
}

/// `Vec3i.closerToCenterThan`：到方块中心（三个轴都加 0.5）的距离。
///
/// `Vec3i.closerToCenterThan`: the distance to the block centre (0.5 on all three axes).
fn closer_to_center_than(pos: BlockPos, at: Vec3, distance: f64) -> bool {
    let dx = f64::from(pos.x) + 0.5 - at.x;
    let dy = f64::from(pos.y) + 0.5 - at.y;
    let dz = f64::from(pos.z) + 0.5 - at.z;
    dx * dx + dy * dy + dz * dz < distance * distance
}

/// `LongDistancePatrolGoal.tick` 里算下一段路的终点：把「自己到远处目标」的向量按
/// `yRot(90.0F)` 转一下（官方传的是 90 弧度）、缩到 0.4 加回目标，再朝它走 10 格，
/// 落到 `MOTION_BLOCKING_NO_LEAVES` 高度图上。
///
/// The next leg of `LongDistancePatrolGoal.tick`: rotate the self-to-target vector with
/// `yRot(90.0F)` (vanilla passes 90 radians), scale by 0.4, add the target back, walk 10
/// blocks towards that and snap to the `MOTION_BLOCKING_NO_LEAVES` heightmap.
fn patrol_step(view: &dyn BlockView, position: Vec3, target: BlockPos) -> BlockPos {
    let far = Vec3::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y),
        f64::from(target.z) + 0.5,
    );
    let distance = position.subtract(far);
    let cos = f64::from(mth::cos(f64::from(90.0_f32)));
    let sin = f64::from(mth::sin(f64::from(90.0_f32)));
    let rotated = Vec3::new(
        distance.x * cos + distance.z * sin,
        distance.y,
        distance.z * cos - distance.x * sin,
    );
    let far = rotated.scale(0.4).add(far);
    let step = far.subtract(position).normalize().scale(10.0).add(position);
    let at = BlockPos::containing(step);
    BlockPos::new(at.x, view_no_leaves_height(view, at.x, at.z), at.z)
}

/// `LongDistancePatrolGoal.moveRandomly`：自己随机源取两次 `nextInt(16)`。
///
/// `LongDistancePatrolGoal.moveRandomly`: two `nextInt(16)` draws from the mob's random.
fn move_randomly(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView, speed: f64) -> bool {
    let at = me.block_position();
    let x = at.x - 8 + me.random.next_int_bounded(16);
    let z = at.z - 8 + me.random.next_int_bounded(16);
    let y = view_no_leaves_height(view, x, z);
    let nav = living::nav_mob(me, mob, view);
    mob.navigation.move_to(
        view,
        &nav,
        Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
        speed,
    )
}

/// 落地 [`Command::SetPatrolTarget`]：队长给同伴 `setPatrolTarget(pathTarget)`。
///
/// Applies [`Command::SetPatrolTarget`]: the leader's `setPatrolTarget(pathTarget)` on
/// its companions.
pub(crate) fn apply_patrol_target(world: &mut World, others: &[EntityUid], target: BlockPos) {
    for &other in others {
        world.with_entity_mut(other, &mut |entity| {
            if let Some(mob) = entity.kind.mob_mut()
                && mob.mob_type.is_raider()
            {
                mob.raider.set_patrol_target(target);
            }
        });
    }
}

/// `LookAtPlayerGoal.canUse` 的非玩家分支：`getEntitiesOfClass(Mob.class,
/// bb.inflate(d, 3, d))` 里过了 `lookAtContext`（非战斗、`d` 格、看得见）的最近一只，
/// 距离从它的脚量到自己的眼睛。目标身上的隐身效果没有建模。
///
/// The non-player branch of `LookAtPlayerGoal.canUse`: the nearest of
/// `getEntitiesOfClass(Mob.class, bb.inflate(d, 3, d))` passing `lookAtContext`
/// (non-combat, `d` blocks, in sight), measured from its feet to this mob's eyes. A
/// target's invisibility effect is not modelled.
fn nearest_mob(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx, range: f64) -> Option<EntityUid> {
    let eye = Vec3::new(me.position.x, living::eye_y(me, mob), me.position.z);
    let mut candidates: Vec<(EntityUid, Vec3)> = Vec::new();
    ctx.for_each_other_in(me.bounding_box().inflate(range, 3.0, range), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && !other.removed
            && public.is_alive()
        {
            candidates.push((other.uid, other.position));
        }
    });
    let limit = mth::java_max(range, 2.0);
    let mut best = -1.0_f64;
    let mut found = None;
    for (uid, position) in candidates {
        if me.position.distance_to_sqr(position) > limit * limit {
            continue;
        }
        let Some((target, _)) = goals::target_view(ctx, uid) else {
            continue;
        };
        if !living::has_line_of_sight(me, mob, ctx.blocks(), uid, target.eye_position()) {
            continue;
        }
        let distance = position.distance_to_sqr(eye);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(uid);
        }
    }
    found
}

/// 搜索盒：`AABB.unitCubeFromLowerCorner(position).inflate(within, 10, within)`。
///
/// The search box `AABB.unitCubeFromLowerCorner(position).inflate(within, 10, within)`.
fn alert_area(at: Vec3, within: f64) -> Aabb {
    Aabb::new(at.x, at.y, at.z, at.x + 1.0, at.y + 1.0, at.z + 1.0).inflate(within, 10.0, within)
}

/// `HurtByTargetGoal.alertOthers`（`Pillager` 的 `setAlertOthers()`）：跟随距离内别的同类
/// 把攻击者当目标，已有目标的由落地函数跳过。
///
/// `HurtByTargetGoal.alertOthers` (`setAlertOthers()` on the pillager): the same kind
/// within follow range targets the attacker; the apply step skips those with a target.
pub(super) fn alert_same_kind(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let Some(enemy) = mob.last_hurt_by_mob else {
        return;
    };
    let within = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let kind = mob.mob_type;
    let mut others = Vec::new();
    ctx.for_each_other_in(alert_area(me.position, within), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type == kind
        {
            others.push(other.uid);
        }
    });
    if !others.is_empty() {
        ctx.emit(Command::AlertMobs { others, enemy }, me);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 存档键和官方两层的 `addAdditionalSaveData` 一致，缺省值读回来不变。
    ///
    /// The save keys match the two vanilla layers and defaults round-trip.
    #[test]
    fn save_keys_round_trip() {
        let state = Raider {
            patrol_target: Some(BlockPos::new(12, 70, -340)),
            patrol_leader: true,
            patrolling: true,
            wave: 3,
            can_join_raid: true,
            raid_id: Some(7),
        };
        let mut out = Vec::new();
        state.save(&mut out);
        let keys: Vec<&str> = out.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "patrol_target",
                "PatrolLeader",
                "Patrolling",
                "Wave",
                "CanJoinRaid",
                "RaidId"
            ]
        );
        assert_eq!(Raider::load(&Tag::Compound(out)), state);
        let mut out = Vec::new();
        Raider::default().save(&mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(Raider::load(&Tag::Compound(out)), Raider::default());
    }

    /// 灾厄旗帜是白旗，八层图案按官方顺序。
    ///
    /// The ominous banner is a white banner with the eight vanilla layers in order.
    #[test]
    fn ominous_banner_has_eight_layers() {
        use mc_bamboo_data::registries::data_component_type::BANNER_PATTERNS;
        use mc_bamboo_protocol::components::{ComponentValue, defaults};
        let banner = ominous_banner();
        assert_eq!(banner.item, items::WHITE_BANNER);
        let patch = banner.patch();
        let Some(ComponentValue::BannerPatterns(layers)) =
            defaults::resolve(banner.item, &patch, BANNER_PATTERNS)
        else {
            panic!("no banner patterns");
        };
        assert_eq!(layers.len(), 8);
        assert_eq!(layers[0].color, 9);
        assert_eq!(layers[7].color, 15);
    }

    /// 队长要同时是巡逻队长并且戴着灾厄旗帜；普通白旗不算。
    ///
    /// A captain is a patrol leader wearing the ominous banner; a plain white banner
    /// does not count.
    #[test]
    fn a_leader_with_the_banner_is_a_captain() {
        let mut mob = Mob::new(super::super::MobType::Pillager);
        assert!(!is_captain(&mob));
        mob.raider.patrol_leader = true;
        assert!(!is_captain(&mob));
        mob.equipment.set(EquipmentSlot::Head, ominous_banner());
        assert!(is_captain(&mob));
        mob.equipment
            .set(EquipmentSlot::Head, ItemStack::new(items::WHITE_BANNER, 1));
        assert!(!is_captain(&mob));
    }

    /// 巡逻中的生物在 128 格内不会按距离消失。
    ///
    /// A patrolling mob does not despawn by distance within 128 blocks.
    #[test]
    fn patrolling_mobs_stay_within_128_blocks() {
        let mut mob = Mob::new(super::super::MobType::Pillager);
        assert!(removes_when_far_away(&mob, 100.0));
        mob.raider.patrolling = true;
        assert!(!removes_when_far_away(&mob, 16384.0));
        assert!(removes_when_far_away(&mob, 16384.5));
    }

    /// 高度图跳过树叶，落在挡运动的方块上面。
    ///
    /// The heightmap skips leaves and rests on the motion-blocking block.
    #[test]
    fn no_leaves_height_skips_leaves() {
        use mc_bamboo_data::blocks;
        let state_at = |pos: BlockPos| {
            crate::BlockState(match pos.y {
                64 => blocks::STONE,
                66 => blocks::OAK_LEAVES,
                _ => blocks::AIR,
            })
        };
        assert_eq!(no_leaves_height(state_at, -64, 319, 0, 0), 65);
        let empty = |_: BlockPos| crate::BlockState(blocks::AIR);
        assert_eq!(no_leaves_height(empty, -64, 319, 0, 0), -64);
    }
}
