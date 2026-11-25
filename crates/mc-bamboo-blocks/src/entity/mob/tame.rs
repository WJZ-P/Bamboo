//! 可驯服的动物：官方 `TamableAnimal` 的状态（坐姿位、驯服位、主人、`orderedToSit`）、
//! 瞬移到主人身边，以及四个公共目标 `SitWhenOrderedToGoal`、`FollowOwnerGoal`、
//! `OwnerHurtByTargetGoal`、`OwnerHurtTargetGoal`。狼先用，猫和鹦鹉以后直接复用。
//!
//! 主人只按玩家找（`getOwner()` 解析主人 UUID 时官方也认别的生物，但只有命令能造出来）。
//!
//! Tamable animals: vanilla `TamableAnimal` state (sitting and tame bits, the
//! owner, `orderedToSit`), teleporting to the owner, and the four shared goals.
//! Wolves use it first; cats and parrots can reuse it. Owners are resolved
//! among players only.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::PathType;

use super::{Mob, MobType, goals, living, path};
use crate::entity::{
    Entity, EntityUid,
    ctx::{BlockView, EntityCtx, PlayerView},
};

/// `TamableAnimal.DATA_FLAGS_ID` 的坐姿位。
pub const FLAG_SITTING: u8 = 1;
/// `TamableAnimal.DATA_FLAGS_ID` 的驯服位。
pub const FLAG_TAME: u8 = 4;

/// `TamableAnimal.TELEPORT_WHEN_DISTANCE_IS_SQ`。
const TELEPORT_WHEN_DISTANCE_IS_SQ: f64 = 144.0;

/// 驯服相关的状态。
///
/// The taming state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tamable {
    /// `DATA_FLAGS_ID`：坐姿位 1、驯服位 4。
    pub flags: u8,
    /// `DATA_OWNERUUID_ID`。
    pub owner: Option<[u8; 16]>,
    /// `orderedToSit`：玩家下的命令，存盘为 `Sitting`；坐姿位是目标跑起来时才设。
    ///
    /// The player's order, saved as `Sitting`; the pose bit follows the goal.
    pub ordered_to_sit: bool,
}

impl Tamable {
    pub const fn is_tame(&self) -> bool {
        self.flags & FLAG_TAME != 0
    }

    pub const fn in_sitting_pose(&self) -> bool {
        self.flags & FLAG_SITTING != 0
    }

    /// `setInSittingPose`。
    pub fn set_in_sitting_pose(&mut self, value: bool) {
        self.set_flag(FLAG_SITTING, value);
    }

    fn set_flag(&mut self, flag: u8, value: bool) {
        if value {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    /// `isOwnedBy`：`entity == getOwner()`，按 UUID 比。
    ///
    /// `isOwnedBy`, compared by UUID.
    pub fn is_owned_by(&self, uuid: Option<[u8; 16]>) -> bool {
        self.owner.is_some() && self.owner == uuid
    }
}

/// 这个种类是不是 `TamableAnimal`。
///
/// Whether this kind is a `TamableAnimal`.
pub const fn is_tamable(kind: MobType) -> bool {
    matches!(
        kind,
        MobType::Wolf
            | MobType::Cat
            | MobType::Parrot
            | MobType::Nautilus
            | MobType::ZombieNautilus
    )
}

/// `TamableAnimal.setTame(isTame, includeSideEffects)`。
pub fn set_tame(mob: &mut Mob, tame: bool, side_effects: bool) {
    mob.tame.set_flag(FLAG_TAME, tame);
    if side_effects {
        apply_taming_side_effects(mob);
    }
    // `Cat.setTame` 覆写 `setTame`，无论 `includeSideEffects` 都重算躲玩家目标。
    // `Cat.setTame` overrides `setTame` to reassess its avoid-players goal regardless of `includeSideEffects`.
    if mob.mob_type == MobType::Cat {
        super::cat::reassess_tame_goals(mob);
    }
}

/// `applyTamingSideEffects`：按种类分派（现在只有狼）。
fn apply_taming_side_effects(mob: &mut Mob) {
    if mob.mob_type == MobType::Wolf {
        super::wolf::apply_taming_side_effects(mob);
    }
}

/// `TamableAnimal.tame(player)`：驯服（带副作用）并记下主人。
/// `TAME_ANIMAL` 进度触发器不建模。
///
/// `TamableAnimal.tame(player)`; the advancement trigger is not modelled.
pub fn tame(mob: &mut Mob, player: [u8; 16]) {
    set_tame(mob, true, true);
    mob.tame.owner = Some(player);
}

/// `getOwner()`：主人在本岛的玩家视图。
///
/// `getOwner()`: the owner's player view on this island.
pub fn owner(mob: &Mob, view: &dyn BlockView) -> Option<PlayerView> {
    let uuid = mob.tame.owner?;
    let mut found = None;
    view.for_each_player(&mut |player| {
        if found.is_none() && player.uuid == Some(uuid) {
            found = Some(*player);
        }
    });
    found
}

/// `TamableAnimal.canAttack` 的额外一条：不打主人。
///
/// The extra rule of `TamableAnimal.canAttack`: never the owner.
pub fn refuses_target(mob: &Mob, target_uuid: Option<[u8; 16]>) -> bool {
    is_tamable(mob.mob_type) && mob.tame.is_owned_by(target_uuid)
}

/// `unableToMoveToOwner`：坐着、骑着、拴着（`isPassenger` 还没建模）或主人是旁观者。
///
/// `unableToMoveToOwner`: sitting, a passenger, leashed or the owner is a spectator.
pub fn unable_to_move_to_owner(mob: &Mob, view: &dyn BlockView) -> bool {
    mob.tame.ordered_to_sit
        || super::leash::may_be_leashed(mob)
        || owner(mob, view).is_some_and(|owner| owner.spectator)
}

/// `shouldTryTeleportToOwner`：主人离开 12 格。
pub fn should_try_teleport_to_owner(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    owner(mob, view).is_some_and(|owner| {
        me.position.distance_to_sqr(owner.position) >= TELEPORT_WHEN_DISTANCE_IS_SQ
    })
}

/// `tryToTeleportToOwner` → `teleportToAroundBlockPos`：10 次随机落点，水平至少离开 2 格。
///
/// `tryToTeleportToOwner`: ten random spots at least two blocks away.
pub fn try_to_teleport_to_owner(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    let Some(owner) = owner(mob, view) else {
        return;
    };
    let target = BlockPos::containing(owner.position);
    for _ in 0..10 {
        let xd = between_inclusive(me, -3, 3);
        let zd = between_inclusive(me, -3, 3);
        if xd.abs() < 2 && zd.abs() < 2 {
            continue;
        }
        let yd = between_inclusive(me, -1, 1);
        if maybe_teleport_to(me, mob, view, target.offset(xd, yd, zd)) {
            return;
        }
    }
}

/// `RandomSource.nextIntBetweenInclusive`。
fn between_inclusive(me: &mut Entity, min: i32, max: i32) -> i32 {
    me.random.next_int_bounded(max - min + 1) + min
}

/// `maybeTeleportTo`：能落脚就 `snapTo` 到方块中心并停下导航。
fn maybe_teleport_to(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView, pos: BlockPos) -> bool {
    if !can_teleport_to(me, mob, view, pos) {
        return false;
    }
    let at = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    // `snapTo(x, y, z, yRot, xRot)`：位置和上一刻的位置、朝向一起设。
    // `snapTo` sets the position, the previous position and the rotations.
    me.position = at;
    me.old_position = at;
    mob.y_rot_o = me.y_rot;
    mob.x_rot_o = mob.x_rot;
    mob.navigation.stop();
    true
}

/// `canTeleportTo`：路径类型可走、脚下不是树叶、移过去不撞方块。
fn can_teleport_to(me: &Entity, mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> bool {
    if path::static_path_type(view, pos) != PathType::Walkable {
        return false;
    }
    if view
        .block_state(pos.below())
        .is_tagged(mc_bamboo_data::tags::block_tags::LEAVES)
    {
        return false;
    }
    let here = me.block_position();
    let moved = me.bounding_box().moved(
        f64::from(pos.x - here.x),
        f64::from(pos.y - here.y),
        f64::from(pos.z - here.z),
    );
    !path::has_block_collision(view, moved, me.mob_collision_context(mob), |at| {
        view.block_state(at)
    })
}

/// 公共的驯服目标。
///
/// The shared taming goals.
#[derive(Debug, Clone, PartialEq)]
pub enum TameGoal {
    /// `SitWhenOrderedToGoal`。
    Sit,
    /// `FollowOwnerGoal(speed, startDistance, stopDistance)`。
    FollowOwner {
        speed: f64,
        start_distance: f32,
        stop_distance: f32,
        owner: Option<EntityUid>,
        time_to_recalc_path: i32,
        old_water_cost: f32,
    },
    /// `OwnerHurtByTargetGoal`。
    OwnerHurtBy {
        attacker: Option<EntityUid>,
        timestamp: i32,
    },
    /// `OwnerHurtTargetGoal`。
    OwnerHurt {
        victim: Option<EntityUid>,
        timestamp: i32,
    },
}

impl TameGoal {
    /// `FollowOwnerGoal(mob, speed, startDistance, stopDistance)`。
    pub const fn follow_owner(speed: f64, start_distance: f32, stop_distance: f32) -> Self {
        Self::FollowOwner {
            speed,
            start_distance,
            stop_distance,
            owner: None,
            time_to_recalc_path: 0,
            old_water_cost: 0.0,
        }
    }

    pub const fn flags(&self) -> &'static [goals::Flag] {
        use goals::Flag;
        match self {
            Self::Sit => &[Flag::Jump, Flag::Move],
            Self::FollowOwner { .. } => &[Flag::Move, Flag::Look],
            Self::OwnerHurtBy { .. } | Self::OwnerHurt { .. } => &[Flag::Target],
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Sit => "SitWhenOrderedToGoal",
            Self::FollowOwner { .. } => "FollowOwnerGoal",
            Self::OwnerHurtBy { .. } => "OwnerHurtByTargetGoal",
            Self::OwnerHurt { .. } => "OwnerHurtTargetGoal",
        }
    }

    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        let view = ctx.blocks();
        match self {
            Self::Sit => sit_can_use(me, mob, view),
            Self::FollowOwner {
                start_distance,
                owner: found,
                ..
            } => {
                let Some(owner) = owner(mob, view) else {
                    return false;
                };
                if unable_to_move_to_owner(mob, view) {
                    return false;
                }
                let start = f64::from(*start_distance * *start_distance);
                if me.position.distance_to_sqr(owner.position) < start {
                    return false;
                }
                *found = Some(owner.uid);
                true
            }
            Self::OwnerHurtBy {
                attacker,
                timestamp,
            } => {
                let Some(owner) = guarding_owner(mob, view) else {
                    return false;
                };
                if !owner.combat.retaliation_damage {
                    return false;
                }
                *attacker = owner.combat.last_hurt_by_mob;
                owner.combat.last_hurt_by_mob_timestamp != *timestamp
                    && attacker.is_some_and(|uid| defends_against(me, mob, ctx, uid, &owner))
            }
            Self::OwnerHurt { victim, timestamp } => {
                let Some(owner) = guarding_owner(mob, view) else {
                    return false;
                };
                *victim = owner.combat.last_hurt_mob;
                owner.combat.last_hurt_mob_timestamp != *timestamp
                    && victim.is_some_and(|uid| defends_against(me, mob, ctx, uid, &owner))
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
    ) -> bool {
        let view = ctx.blocks();
        match self {
            Self::Sit => mob.tame.ordered_to_sit,
            Self::FollowOwner {
                stop_distance,
                owner: found,
                ..
            } => {
                if mob.navigation.is_done() || unable_to_move_to_owner(mob, view) {
                    return false;
                }
                let Some(owner) = found.and_then(|uid| goals::player_by_uid(view, uid)) else {
                    return false;
                };
                let stop = f64::from(*stop_distance * *stop_distance);
                me.position.distance_to_sqr(owner.position) > stop
            }
            Self::OwnerHurtBy { .. } | Self::OwnerHurt { .. } => {
                goals::target_goal_can_continue(me, mob, ctx)
            }
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        let view = ctx.blocks();
        match self {
            Self::Sit => {
                mob.navigation.stop();
                mob.tame.set_in_sitting_pose(true);
            }
            Self::FollowOwner {
                time_to_recalc_path,
                old_water_cost,
                ..
            } => {
                *time_to_recalc_path = 0;
                *old_water_cost = mob.malus.get(PathType::Water);
                mob.malus.set(PathType::Water, 0.0);
            }
            Self::OwnerHurtBy {
                attacker,
                timestamp,
            } => {
                mob.target = *attacker;
                if let Some(owner) = owner(mob, view) {
                    *timestamp = owner.combat.last_hurt_by_mob_timestamp;
                }
            }
            Self::OwnerHurt { victim, timestamp } => {
                mob.target = *victim;
                if let Some(owner) = owner(mob, view) {
                    *timestamp = owner.combat.last_hurt_mob_timestamp;
                }
            }
        }
        let _ = me;
    }

    pub(super) fn stop(&mut self, mob: &mut Mob) {
        match self {
            Self::Sit => mob.tame.set_in_sitting_pose(false),
            Self::FollowOwner {
                owner,
                old_water_cost,
                ..
            } => {
                *owner = None;
                mob.navigation.stop();
                mob.malus.set(PathType::Water, *old_water_cost);
            }
            // `TargetGoal.stop`：`setTarget(null)`。
            Self::OwnerHurtBy { .. } | Self::OwnerHurt { .. } => mob.target = None,
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        let Self::FollowOwner {
            speed,
            owner: found,
            time_to_recalc_path,
            ..
        } = self
        else {
            return;
        };
        let view = ctx.blocks();
        let Some(owner) = found.and_then(|uid| goals::player_by_uid(view, uid)) else {
            return;
        };
        let far_away = should_try_teleport_to_owner(me, mob, view);
        if !far_away {
            mob.look.set_look_at(
                owner.position.x,
                owner.eye_y(),
                owner.position.z,
                10.0,
                goals::max_head_x_rot(mob),
            );
        }
        *time_to_recalc_path -= 1;
        if *time_to_recalc_path > 0 {
            return;
        }
        // `adjustedTickDelay(10)`：这个目标不是每刻更新，折成 5。
        // `adjustedTickDelay(10)`: not an every-tick goal, so 5.
        *time_to_recalc_path = goals::reduced_tick_delay(10);
        if far_away {
            try_to_teleport_to_owner(me, mob, view);
        } else {
            let nav = living::nav_mob(me, mob, view);
            mob.navigation
                .move_to_entity(view, &nav, owner.position, *speed, 1);
        }
    }
}

/// `SitWhenOrderedToGoal.canUse`。
fn sit_can_use(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    let ordered = mob.tame.ordered_to_sit;
    if !ordered && !mob.tame.is_tame() {
        return false;
    }
    if me.in_water || !me.on_ground {
        return false;
    }
    let Some(owner) = owner(mob, view) else {
        return true;
    };
    if me.position.distance_to_sqr(owner.position) < 144.0
        && owner.combat.last_hurt_by_mob.is_some()
    {
        return false;
    }
    ordered
}

/// 两个保护主人目标的前置条件：驯服了、没坐、主人在。
///
/// The shared guard of the two owner goals: tame, not sitting, owner present.
fn guarding_owner(mob: &Mob, view: &dyn BlockView) -> Option<PlayerView> {
    if !mob.tame.is_tame() || mob.tame.ordered_to_sit {
        return None;
    }
    owner(mob, view)
}

/// `TargetGoal.canAttack(target, TargetingConditions.DEFAULT)` 加 `wantsToAttack`。
///
/// `TargetGoal.canAttack(target, DEFAULT)` plus `wantsToAttack`.
fn defends_against(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    target: EntityUid,
    owner: &PlayerView,
) -> bool {
    if target == me.id || target == owner.uid {
        return false;
    }
    let Some(found) = goals::attack_candidate(mob, ctx, target) else {
        return false;
    };
    if !goals::within_home(mob, BlockPos::containing(found.position)) {
        return false;
    }
    if !living::has_line_of_sight(me, mob, ctx.blocks(), target, found.eye_position()) {
        return false;
    }
    match mob.mob_type {
        MobType::Wolf => super::wolf::wants_to_attack(ctx, target, owner),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sitting_and_tame_bits_are_independent() {
        let mut tame = Tamable::default();
        tame.set_in_sitting_pose(true);
        assert!(tame.in_sitting_pose());
        assert!(!tame.is_tame());
        tame.set_flag(FLAG_TAME, true);
        tame.set_in_sitting_pose(false);
        assert_eq!(tame.flags, FLAG_TAME);
    }

    #[test]
    fn only_the_recorded_owner_owns_the_animal() {
        let tame = Tamable {
            owner: Some([3; 16]),
            ..Tamable::default()
        };
        assert!(tame.is_owned_by(Some([3; 16])));
        assert!(!tame.is_owned_by(Some([4; 16])));
        assert!(!Tamable::default().is_owned_by(None));
    }
}
