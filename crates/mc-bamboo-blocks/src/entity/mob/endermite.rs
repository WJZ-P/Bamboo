//! 末影螨：`Endermite`（`Monster` 一支）的寿命和存档状态。
//!
//! 官方 `Endermite.aiStep` 在服务端只做两件事：`!isPersistenceRequired` 时 `life++`，
//! `life >= MAX_LIFE` 时 `discard()`。客户端那两个 `PORTAL` 粒子用的是客户端随机源，
//! 服务端不取数也不发包；`tick` 前置把 `yBodyRot` 设成 `yRot`。
//!
//! Endermite: lifetime and saved state. In vanilla's `Endermite.aiStep` the server
//! only increments `life` when the mob is not persistent and discards it at
//! `MAX_LIFE`; the two `PORTAL` particles are client-only and draw nothing here.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    Mob,
    attributes::Attributes,
    goals::{self, Goal, GoalSelector, WrappedGoal},
    navigation::Navigation,
};
use crate::entity::Entity;

/// `Endermite.MAX_LIFE`，由生成器导出。
///
/// `Endermite.MAX_LIFE`, exported by the generator.
pub const MAX_LIFE: i32 = entity_types::ENDERMITE_MAX_LIFE;

/// `Endermite` 的状态：只有一个存盘的 `Lifetime`。
///
/// Endermite state: just the saved `Lifetime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndermiteState {
    pub life: i32,
}

/// `Endermite.createAttributes`：怪物属性上的 8 血 / 0.25 速 / 2 攻。
///
/// `Endermite.createAttributes`: monster attributes with eight health, 0.25 speed
/// and two attack damage.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::monster();
    attributes.set_base(attribute::MAX_HEALTH, entity_types::ENDERMITE_MAX_HEALTH);
    attributes.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::ENDERMITE_MOVEMENT_SPEED,
    );
    attributes.set_base(
        attribute::ATTACK_DAMAGE,
        entity_types::ENDERMITE_ATTACK_DAMAGE,
    );
    attributes
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        goal,
        running: false,
    }
}

/// `Endermite.registerGoals`：细雪目标只占注册位，不取随机数。
///
/// `Endermite.registerGoals`: the powder-snow goal keeps its slot but draws nothing.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(1, Goal::Unavailable("ClimbOnTopOfPowderSnowGoal")),
        wrap(2, Goal::melee(1.0, false)),
        wrap(
            3,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            7,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            8,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `Endermite.registerGoals` 的目标选择器：受伤反击、打玩家。
///
/// The endermite target selector: hurt-by, then players.
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        wrap(2, Goal::nearest_target(goals::TargetType::Player)),
    ])
}

/// `Endermite.aiStep` 的服务端部分。`isPersistenceRequired()` 为真时不加寿命，
/// 但仍然每刻检查是否已经到 2400。
///
/// The server part of `Endermite.aiStep`: a persistent endermite does not age but
/// still checks the `MAX_LIFE` cap each tick.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob) {
    if !mob.persistence_required {
        mob.endermite.life = mob.endermite.life.saturating_add(1);
    }
    if mob.endermite.life >= MAX_LIFE {
        me.discard();
    }
}

/// 逐刻对拍用的状态行。
///
/// A description for the tick-by-tick oracle.
impl Mob {
    pub fn describe_endermite(&self) -> String {
        format!("life={}", self.endermite.life)
    }
}

/// 客户端 `PORTAL` 粒子的注册表 ID；服务端不发包，用在这里只是让对拍能确认粒子种类。
///
/// The `PORTAL` particle type id; no packet is sent server-side.
pub const PORTAL_PARTICLE: i32 = mc_bamboo_data::registries::particle_type::PORTAL;

