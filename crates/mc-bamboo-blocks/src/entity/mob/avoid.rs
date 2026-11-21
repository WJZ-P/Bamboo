//! `AvoidEntityGoal`：看见某类实体就沿反方向跑开。兔子、狐狸等共用。
//!
//! `AvoidEntityGoal`: run away from a class of entities in sight. Shared by
//! rabbits, foxes and later mobs.

use mc_bamboo_core::{Aabb, Vec3};

use super::{Mob, MobType, goals, living, path::Path};
use crate::entity::{
    Entity, EntityUid,
    ctx::{EntityCtx, PlayerView},
    store::KindPublic,
};

/// 要躲的实体类（`avoidClass`）。
///
/// The entity class to avoid (`avoidClass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvoidClass {
    Player,
    Wolf,
    Monster,
    PolarBear,
}

impl AvoidClass {
    fn matches(self, kind: MobType) -> bool {
        match self {
            Self::Player => false,
            Self::Wolf => kind == MobType::Wolf,
            Self::Monster => kind.is_monster(),
            // 北极熊还没实现。Polar bears are not modelled yet.
            Self::PolarBear => false,
        }
    }
}

/// 正在躲的那个实体：uid 和最近一次看到的位置。
///
/// The entity being avoided: its uid and last seen position.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Avoided {
    uid: EntityUid,
    position: Vec3,
}

/// `predicateOnAvoidEntity`：缺省排除创造和旁观玩家；狐狸给的三种。
///
/// `predicateOnAvoidEntity`: by default no creative or spectator players;
/// foxes pass three of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvoidFilter {
    NoCreativeOrSpectator,
    /// `AVOID_PLAYERS && !trusts && !isDefending`。
    FoxPlayer,
    /// `!wolf.isTame() && !isDefending`。
    FoxWolf,
    /// `!isDefending`。
    FoxDefending,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AvoidEntity {
    class: AvoidClass,
    max_dist: f32,
    walk_speed: f64,
    sprint_speed: f64,
    to_avoid: Option<Avoided>,
    path: Option<Path>,
    filter: AvoidFilter,
}

impl AvoidEntity {
    pub fn new(class: AvoidClass, max_dist: f32, walk_speed: f64, sprint_speed: f64) -> Self {
        Self {
            class,
            max_dist,
            walk_speed,
            sprint_speed,
            to_avoid: None,
            path: None,
            filter: AvoidFilter::NoCreativeOrSpectator,
        }
    }

    /// 换掉 `predicateOnAvoidEntity`（狐狸用自己的条件）。
    ///
    /// Replace `predicateOnAvoidEntity` (foxes pass their own).
    pub const fn filtered(mut self, filter: AvoidFilter) -> Self {
        self.filter = filter;
        self
    }

    /// `AvoidEntityGoal.canUse`。
    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        self.to_avoid = self.nearest(me, mob, ctx);
        let Some(avoided) = self.to_avoid else {
            return false;
        };
        let view = ctx.blocks();
        let Some(pos) = goals::default_random_pos_away(me, mob, view, 16, 7, avoided.position)
        else {
            return false;
        };
        if avoided.position.distance_to_sqr(pos) < avoided.position.distance_to_sqr(me.position) {
            return false;
        }
        let nav = living::nav_mob(me, mob, view);
        self.path =
            mob.navigation
                .create_path(view, &nav, mc_bamboo_core::BlockPos::containing(pos), 0);
        self.path.is_some()
    }

    /// `AvoidEntityGoal.canContinueToUse`。
    pub(super) fn can_continue(mob: &Mob) -> bool {
        !mob.navigation.is_done()
    }

    /// `AvoidEntityGoal.start`：`pathNav.moveTo(path, walkSpeedModifier)`。
    pub(super) fn start(&mut self, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        let view = ctx.blocks();
        let nav = living::nav_mob(me, mob, view);
        let path = self.path.clone();
        mob.navigation
            .move_to_path(view, &nav, path, self.walk_speed);
    }

    /// `AvoidEntityGoal.stop`：只清 `toAvoid`，路径留在导航里。
    ///
    /// `AvoidEntityGoal.stop`: only clears `toAvoid`; the path stays.
    pub(super) fn stop(&mut self) {
        self.to_avoid = None;
    }

    /// `AvoidEntityGoal.tick`：7 格内冲刺，否则步行速度。
    pub(super) fn tick(&mut self, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        let Some(avoided) = self.to_avoid.as_mut() else {
            return;
        };
        if let Some(position) = current_position(ctx, avoided.uid) {
            avoided.position = position;
        }
        mob.navigation.speed_modifier = if me.position.distance_to_sqr(avoided.position) < 49.0 {
            self.sprint_speed
        } else {
            self.walk_speed
        };
    }

    /// `ServerLevel.getNearestEntity(getEntitiesOfClass(...), avoidEntityTargeting, ...)`。
    fn nearest(&self, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<Avoided> {
        let max = f64::from(self.max_dist);
        let area = me.bounding_box().inflate(max, 3.0, max);
        let mut best = -1.0_f64;
        let mut found = None;
        for candidate in self.candidates(me, ctx, area) {
            if !self.test(me, mob, ctx, &candidate) {
                continue;
            }
            let distance = candidate.position.distance_to_sqr(me.position);
            if best == -1.0 || distance < best {
                best = distance;
                found = Some(Avoided {
                    uid: candidate.uid,
                    position: candidate.position,
                });
            }
        }
        found
    }

    fn candidates(&self, me: &Entity, ctx: &dyn EntityCtx, area: Aabb) -> Vec<CandidateView> {
        let mut out = Vec::new();
        if self.class == AvoidClass::Player {
            ctx.blocks().for_each_player(&mut |player| {
                if player_box(player).intersects(area) {
                    out.push(Candidate::Player(*player).into());
                }
            });
            return out;
        }
        ctx.for_each_other_in(area, &mut |other| {
            let KindPublic::Mob(public) = &other.kind else {
                return;
            };
            if other.uid == me.id || other.removed || !self.class.matches(public.mob_type) {
                return;
            }
            let (_, _, eye) = public.mob_type.dimensions(public.baby);
            out.push(
                Candidate::Mob {
                    uid: other.uid,
                    position: other.position,
                    eye,
                    alive: public.is_alive(),
                    tame: public.tame_flags & super::tame::FLAG_TAME != 0,
                }
                .into(),
            );
        });
        out
    }

    /// `TargetingConditions.forCombat().range(maxDist)` 加 `NO_CREATIVE_OR_SPECTATOR`。
    fn test(
        &self,
        me: &Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
        candidate: &CandidateView,
    ) -> bool {
        let view = ctx.blocks();
        match candidate.source {
            Candidate::Player(player) => {
                // `LivingEntity.canAttack`：和平难度不把玩家当敌人；创造模式的
                // `abilities.invulnerable` 让 `canBeSeenAsEnemy` 为假。
                // `LivingEntity.canAttack`: players are no enemies on peaceful,
                // and creative invulnerability fails `canBeSeenAsEnemy`.
                if player.creative || view.difficulty() == 0 {
                    return false;
                }
                // `predicateOnAvoidEntity`：狐狸不躲潜行的、信任的玩家，保护时谁都不躲。
                // Foxes ignore sneaking or trusted players and avoid nobody while defending.
                if self.filter == AvoidFilter::FoxPlayer
                    && (player.sneaking || mob.fox.trusts(player.uuid) || mob.fox.is_defending())
                {
                    return false;
                }
                goals::targeting_test(me, mob, view, &player, f64::from(self.max_dist), true)
            }
            Candidate::Mob {
                uid,
                position,
                eye,
                alive,
                tame,
            } => {
                if !alive {
                    return false;
                }
                // `FoxWolf`：狐狸不躲驯服的狼。
                // `FoxWolf`: foxes ignore tamed wolves.
                if self.filter == AvoidFilter::FoxWolf && tame {
                    return false;
                }
                if matches!(
                    self.filter,
                    AvoidFilter::FoxWolf | AvoidFilter::FoxDefending
                ) && mob.fox.is_defending()
                {
                    return false;
                }
                let max = mc_bamboo_core::mth::java_max(f64::from(self.max_dist), 2.0);
                if me.position.distance_to_sqr(position) > max * max {
                    return false;
                }
                let eye = Vec3::new(position.x, position.y + f64::from(eye), position.z);
                living::has_line_of_sight(me, mob, view, uid, eye)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Candidate {
    Player(PlayerView),
    Mob {
        uid: EntityUid,
        position: Vec3,
        eye: f32,
        alive: bool,
        tame: bool,
    },
}

/// 候选实体加上排序要用的 uid 和位置。
///
/// A candidate with the uid and position used for picking the nearest.
#[derive(Debug, Clone, Copy)]
struct CandidateView {
    uid: EntityUid,
    position: Vec3,
    source: Candidate,
}

impl From<Candidate> for CandidateView {
    fn from(source: Candidate) -> Self {
        let (uid, position) = match source {
            Candidate::Player(player) => (player.uid, player.position),
            Candidate::Mob { uid, position, .. } => (uid, position),
        };
        Self {
            uid,
            position,
            source,
        }
    }
}

/// 玩家的碰撞箱：宽 0.6，站立 1.8、潜行 1.5。
///
/// A player's bounding box: 0.6 wide, 1.8 tall standing and 1.5 sneaking.
pub(super) fn player_box(player: &PlayerView) -> Aabb {
    let height = if player.sneaking {
        f64::from(1.5_f32)
    } else {
        f64::from(1.8_f32)
    };
    let half = f64::from(0.6_f32) / 2.0;
    let feet = player.position;
    Aabb::new(
        feet.x - half,
        feet.y,
        feet.z - half,
        feet.x + half,
        feet.y + height,
        feet.z + half,
    )
}

fn current_position(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<Vec3> {
    if let Some(player) = goals::player_by_uid(ctx.blocks(), uid) {
        return Some(player.position);
    }
    ctx.other(uid).map(|other| other.position)
}
