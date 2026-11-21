//! 狼专属的目标：`BegGoal`、带呼叫同伴的 `HurtByTargetGoal`、按 `isAngryAt` 选玩家的
//! `NearestAttackableTargetGoal`、只在野生时捕猎的 `NonTameRandomTargetGoal`、
//! 打骷髅，以及 `ResetUniversalAngerTargetGoal`。
//!
//! Wolf-only goals: begging, hurt-by with alerting, anger-driven player
//! targeting, wild-only hunting, skeleton hunting and the universal anger reset.

use mc_bamboo_core::{Aabb, BlockPos, mth};

use super::{
    Mob, MobType,
    goals::{self, Flag},
    living, wolf,
};
use crate::entity::{
    Entity, EntityUid,
    ctx::{Command, EntityCtx, PlayerView},
    store::KindPublic,
};

/// `NearestAttackableTargetGoal` 一族在狼身上的四种目标。
///
/// The four `NearestAttackableTargetGoal` flavours on a wolf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolfTarget {
    /// `(4) NearestAttackableTargetGoal(Player, 10, true, false, isAngryAt)`。
    AngryPlayer,
    /// `(5) NonTameRandomTargetGoal(Animal, false, PREY_SELECTOR)`：羊、兔子、狐狸。
    Prey,
    /// `(6) NonTameRandomTargetGoal(Turtle, false, BABY_ON_LAND_SELECTOR)`：海龟还没有。
    BabyTurtle,
    /// `(7) NearestAttackableTargetGoal(AbstractSkeleton, false)`。
    Skeleton,
}

impl WolfTarget {
    /// `NonTameRandomTargetGoal` 只给野狼用。
    const fn wild_only(self) -> bool {
        matches!(self, Self::Prey | Self::BabyTurtle)
    }

    /// `TargetGoal.mustSee`。
    const fn must_see(self) -> bool {
        matches!(self, Self::AngryPlayer)
    }
}

/// 狼专属的目标和它们自己的状态。
///
/// The wolf-only goals and their state.
#[derive(Debug, Clone, PartialEq)]
pub enum WolfGoal {
    /// `BegGoal(wolf, 8)`。
    Beg {
        player: Option<EntityUid>,
        look_time: i32,
    },
    /// `HurtByTargetGoal(wolf).setAlertOthers()`。
    HurtBy { timestamp: i32, unseen_ticks: i32 },
    /// `NearestAttackableTargetGoal` / `NonTameRandomTargetGoal`。
    Nearest {
        kind: WolfTarget,
        found: Option<EntityUid>,
        unseen_ticks: i32,
    },
    /// `ResetUniversalAngerTargetGoal(wolf, true)`：Bamboo 没有 `universalAnger` 规则，
    /// 按关闭算，`canUse` 恒假。
    ///
    /// The `universalAnger` rule does not exist in Bamboo and counts as off.
    ResetUniversalAnger,
}

/// `Goal.reducedTickDelay(10)`：`NearestAttackableTargetGoal.randomInterval`。
const RANDOM_INTERVAL: i32 = 5;
/// `BegGoal.lookDistance`。
const BEG_DISTANCE: f64 = 8.0;

impl WolfGoal {
    pub const fn beg() -> Self {
        Self::Beg {
            player: None,
            look_time: 0,
        }
    }

    pub const fn hurt_by() -> Self {
        Self::HurtBy {
            timestamp: 0,
            unseen_ticks: 0,
        }
    }

    pub const fn nearest(kind: WolfTarget) -> Self {
        Self::Nearest {
            kind,
            found: None,
            unseen_ticks: 0,
        }
    }

    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Beg { .. } => &[Flag::Look],
            Self::HurtBy { .. } | Self::Nearest { .. } => &[Flag::Target],
            Self::ResetUniversalAnger => &[],
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Beg { .. } => "BegGoal",
            Self::HurtBy { .. } => "HurtByTargetGoal",
            Self::Nearest { kind, .. } if kind.wild_only() => "NonTameRandomTargetGoal",
            Self::Nearest { .. } => "NearestAttackableTargetGoal",
            Self::ResetUniversalAnger => "ResetUniversalAngerTargetGoal",
        }
    }

    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        match self {
            Self::Beg { player, .. } => {
                *player = nearest_beg_player(me, mob, ctx).map(|found| found.uid);
                player
                    .and_then(|uid| goals::player_by_uid(ctx.blocks(), uid))
                    .is_some_and(|found| holding_interesting(&found))
            }
            Self::HurtBy { timestamp, .. } => {
                mob.last_hurt_by_mob_timestamp != *timestamp
                    && mob
                        .last_hurt_by_mob
                        .is_some_and(|uid| hurt_by_can_attack(mob, ctx, uid))
            }
            Self::Nearest { kind, found, .. } => {
                if kind.wild_only() && mob.tame.is_tame() {
                    return false;
                }
                if me.random.next_int_bounded(RANDOM_INTERVAL) != 0 {
                    return false;
                }
                *found = find_target(me, mob, ctx, *kind);
                found.is_some()
            }
            Self::ResetUniversalAnger => false,
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Beg { player, look_time } => {
                let Some(found) = player.and_then(|uid| goals::player_by_uid(ctx.blocks(), uid))
                else {
                    return false;
                };
                if !found.alive {
                    return false;
                }
                if me.position.distance_to_sqr(found.position) > BEG_DISTANCE * BEG_DISTANCE {
                    return false;
                }
                *look_time > 0 && holding_interesting(&found)
            }
            Self::HurtBy { unseen_ticks, .. } => {
                target_goal_continue(me, mob, ctx, Some(unseen_ticks), 300)
            }
            Self::Nearest {
                kind,
                found,
                unseen_ticks,
            } => {
                if kind.wild_only() {
                    return found.is_some_and(|uid| prey_still_valid(me, mob, ctx, uid));
                }
                let seen = kind.must_see().then_some(unseen_ticks);
                target_goal_continue(me, mob, ctx, seen, 60)
            }
            Self::ResetUniversalAnger => false,
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::Beg { look_time, .. } => {
                mob.wolf.interested = true;
                *look_time = goals::reduced_tick_delay(40 + me.random.next_int_bounded(40));
            }
            Self::HurtBy {
                timestamp,
                unseen_ticks,
            } => {
                mob.target = mob.last_hurt_by_mob;
                *timestamp = mob.last_hurt_by_mob_timestamp;
                *unseen_ticks = 0;
                alert_others(me, mob, ctx);
            }
            Self::Nearest {
                found,
                unseen_ticks,
                ..
            } => {
                mob.target = *found;
                *unseen_ticks = 0;
            }
            Self::ResetUniversalAnger => {}
        }
    }

    pub(super) fn stop(&mut self, mob: &mut Mob) {
        match self {
            Self::Beg { player, .. } => {
                mob.wolf.interested = false;
                *player = None;
            }
            Self::HurtBy { .. } => mob.target = None,
            Self::Nearest { found, .. } => {
                mob.target = None;
                *found = None;
            }
            Self::ResetUniversalAnger => {}
        }
    }

    pub(super) fn tick(&mut self, mob: &mut Mob, ctx: &dyn EntityCtx) {
        if let Self::Beg { player, look_time } = self {
            if let Some(found) = player.and_then(|uid| goals::player_by_uid(ctx.blocks(), uid)) {
                mob.look.set_look_at(
                    found.position.x,
                    found.eye_y(),
                    found.position.z,
                    10.0,
                    wolf::max_head_x_rot(mob),
                );
            }
            *look_time -= 1;
        }
    }
}

/// `BegGoal.playerHoldingInteresting`：任一只手拿着骨头或狼的食物。
fn holding_interesting(player: &PlayerView) -> bool {
    [player.main_hand, player.off_hand].into_iter().any(|item| {
        item == mc_bamboo_data::items::BONE
            || (item != 0
                && crate::state::item_is_tagged(item, mc_bamboo_data::tags::item_tags::WOLF_FOOD))
    })
}

/// `level.getNearestPlayer(forNonCombat().range(8), wolf)`：从脚底量距离。
fn nearest_beg_player(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<PlayerView> {
    let view = ctx.blocks();
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        if !goals::targeting_test(me, mob, view, &player, BEG_DISTANCE, true) {
            continue;
        }
        let distance = player.position.distance_to_sqr(me.position);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(player);
        }
    }
    found
}

/// `HurtByTargetGoal.canUse` 里的 `canAttack(lastHurtByMob, HURT_BY_TARGETING)`：
/// 不看视线和隐身，不看距离。
///
/// `canAttack(lastHurtByMob, HURT_BY_TARGETING)`: no sight, invisibility or range.
fn hurt_by_can_attack(mob: &Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> bool {
    goals::attack_candidate(mob, ctx, uid)
        .is_some_and(|found| goals::within_home(mob, BlockPos::containing(found.position)))
}

/// `TargetGoal.canContinueToUse`；`unseen` 为 `Some` 时就是 `mustSee`。
///
/// `TargetGoal.canContinueToUse`; `unseen` present means `mustSee`.
fn target_goal_continue(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    unseen: Option<&mut i32>,
    memory: i32,
) -> bool {
    if !goals::target_goal_can_continue(me, mob, ctx) {
        return false;
    }
    let Some(unseen) = unseen else {
        return true;
    };
    let Some(target) = goals::combat_target(mob, ctx) else {
        return false;
    };
    if living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position()) {
        *unseen = 0;
    } else {
        *unseen += 1;
        if *unseen > goals::reduced_tick_delay(memory) {
            return false;
        }
    }
    true
}

/// `NearestAttackableTargetGoal.findTarget` 在狼身上。
fn find_target(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    kind: WolfTarget,
) -> Option<EntityUid> {
    match kind {
        WolfTarget::AngryPlayer => nearest_angry_player(me, mob, ctx),
        WolfTarget::Prey => goals::nearest_mob_target(me, mob, ctx, is_prey),
        WolfTarget::Skeleton => {
            goals::nearest_mob_target(me, mob, ctx, |kind| kind == MobType::Skeleton)
        }
        WolfTarget::BabyTurtle => None,
    }
}

/// `Wolf.PREY_SELECTOR`：羊、兔子、狐狸（羊在 cx-farm）。
fn is_prey(kind: MobType) -> bool {
    matches!(kind, MobType::Rabbit | MobType::Fox)
}

/// `level.getNearestPlayer(conditions.selector(isAngryAt), mob, x, eyeY, z)`。
fn nearest_angry_player(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let view = ctx.blocks();
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let eye = mc_bamboo_core::Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        if goals::attack_candidate(mob, ctx, player.uid).is_none()
            || !wolf::is_angry_at(mob, ctx, player.uid)
            || !goals::targeting_test(me, mob, view, &player, range, true)
        {
            continue;
        }
        let distance = player.position.distance_to_sqr(eye);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `NonTameRandomTargetGoal.canContinueToUse`：用找目标时的条件重测原来那一个
/// （活着、能打、跟随距离内、看得见）。
///
/// Re-test the found target against the finding conditions.
fn prey_still_valid(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> bool {
    let Some(other) = ctx.other(uid) else {
        return false;
    };
    let KindPublic::Mob(public) = &other.kind else {
        return false;
    };
    if other.removed || !public.is_alive() || !is_prey(public.mob_type) {
        return false;
    }
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let limit = mth::java_max(range, 2.0);
    if me.position.distance_to_sqr(other.position) > limit * limit {
        return false;
    }
    let (_, _, eye) = public.mob_type.dimensions(public.baby);
    let eye = mc_bamboo_core::Vec3::new(
        other.position.x,
        other.position.y + f64::from(eye),
        other.position.z,
    );
    living::has_line_of_sight(me, mob, ctx.blocks(), uid, eye)
}

/// `HurtByTargetGoal.alertOthers`：跟随距离（竖直 10 格）内、没有目标、主人相同、
/// 不和攻击者结盟的别的狼，都把攻击者设成目标。
///
/// `HurtByTargetGoal.alertOthers` for wolves.
fn alert_others(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let Some(attacker) = mob.last_hurt_by_mob else {
        return;
    };
    let within = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let corner = me.position;
    let area = Aabb::new(
        corner.x,
        corner.y,
        corner.z,
        corner.x + 1.0,
        corner.y + 1.0,
        corner.z + 1.0,
    )
    .inflate(within, 10.0, within);
    let attacker_uuid = mob.last_hurt_by_mob_ref;
    let mut others = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if other.uid == me.id || other.removed || public.mob_type != MobType::Wolf {
            return;
        }
        if public.has_target || public.owner != mob.tame.owner {
            return;
        }
        // `isAlliedTo`：驯服的狼和它的主人结盟。
        // `isAlliedTo`: a tamed wolf is allied with its owner.
        let tame = public.tame_flags & super::tame::FLAG_TAME != 0;
        if tame && attacker_uuid.is_some() && public.owner == attacker_uuid {
            return;
        }
        others.push(other.uid);
    });
    if !others.is_empty() {
        ctx.emit(
            Command::WolfAlert {
                others,
                target: attacker,
            },
            me,
        );
    }
}

/// [`Command::WolfAlert`] 的落地：`alertOther` → `setTarget(hurtByMob)`。
pub(crate) fn apply_alert(
    world: &mut crate::world::World,
    others: &[EntityUid],
    target: EntityUid,
) {
    for &other in others {
        world.with_entity_mut(other, &mut |entity| {
            entity.with_mob(|_, mob| mob.target = Some(target));
        });
    }
}
