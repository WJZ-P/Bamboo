//! 蠹虫：`Silverfish`（`Monster` 一支）的两个专属目标和受伤唤醒。
//!
//! `SilverfishWakeUpFriendsGoal` 受伤后倒数 10 刻，然后扫 11×21×21 格里的虫蚀方块：
//! `mobGriefing` 开着就破坏那一格（连带掉落和召唤同伴），否则只换成宿主方块。
//! `SilverfishMergeWithStoneGoal` 是 `RandomStrollGoal(1.0, 10)` 的子类，偶尔把自己
//! 旁边的宿主方块变成虫蚀方块，然后消失。
//!
//! Silverfish: the two dedicated `Silverfish` goals and hurt-triggered wake-up.
//! `SilverfishWakeUpFriendsGoal` counts down ten ticks after a hit, then scans an
//! 11×21×21 box for infested blocks. `SilverfishMergeWithStoneGoal` extends the
//! plain random stroll and occasionally turns a neighbouring host block into its
//! infested state before discarding the silverfish.

use mc_bamboo_core::{Direction, Vec3};
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    DamageSource, Mob,
    attributes::Attributes,
    goals::{self, Goal, GoalSelector, WrappedGoal, reduced_tick_delay},
    infested,
    navigation::Navigation,
};
use crate::entity::{
    Entity,
    ctx::{Command, EntityCtx},
};

/// `Silverfish.createAttributes`：怪物属性上的 8 血 / 0.25 速 / 1 攻。
///
/// `Silverfish.createAttributes`: monster attributes with eight health, 0.25 speed
/// and one attack damage.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::monster();
    attributes.set_base(attribute::MAX_HEALTH, entity_types::SILVERFISH_MAX_HEALTH);
    attributes.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::SILVERFISH_MOVEMENT_SPEED,
    );
    attributes.set_base(
        attribute::ATTACK_DAMAGE,
        entity_types::SILVERFISH_ATTACK_DAMAGE,
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

/// `Silverfish.registerGoals`：细雪的那条目标只占注册位，不取随机数。
///
/// `Silverfish.registerGoals`: the powder-snow goal keeps its slot but draws nothing.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(1, Goal::Unavailable("ClimbOnTopOfPowderSnowGoal")),
        wrap(
            3,
            Goal::SilverfishWakeFriends {
                look_for_friends: 0,
            },
        ),
        wrap(4, Goal::melee(1.0, false)),
        wrap(
            5,
            Goal::SilverfishMergeWithStone {
                direction: Direction::Up,
                do_merge: false,
                speed: 1.0,
                interval: 10,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
    ])
}

/// `Silverfish.registerGoals` 的目标选择器：受伤反击（不叫别的蠹虫）、打玩家。
///
/// The silverfish target selector: hurt-by without alerting others, then players.
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

/// `Silverfish.hurtServer`：无敌判定之后、父类之前，有来源实体或伤害类型在
/// `#always_triggers_silverfish` 里就唤醒同伴。标签 ID 由生成器导出。
///
/// `Silverfish.hurtServer`: after the invulnerability check but before the parent,
/// a source entity or an `always_triggers_silverfish` damage type wakes the friends.
pub(super) fn notify_hurt(mob: &mut Mob, source: &DamageSource) {
    let has_entity = source.attacker.is_some() || source.direct.is_some();
    if !has_entity && !damage_always_triggers(source.kind) {
        return;
    }
    if let Some(Goal::SilverfishWakeFriends { look_for_friends }) = mob
        .goals
        .goals
        .iter_mut()
        .map(|wrapped| &mut wrapped.goal)
        .find(|goal| matches!(goal, Goal::SilverfishWakeFriends { .. }))
        && *look_for_friends == 0
    {
        *look_for_friends = reduced_tick_delay(20);
    }
}

/// 有来源实体的伤害也要按官方标签判定：`DamageSource.is(ALWAYS_TRIGGERS_SILVERFISH)`
/// 在此处等价于检查 `generic_kill`（26.3 标签里只有这一项）。
///
/// Damage with a source entity skips the tag check entirely; the tag's only member
/// in 26.3 is `generic_kill`.
fn damage_always_triggers(kind: super::DamageKind) -> bool {
    infested::damage_always_triggers_silverfish(kind.name())
}

/// `SilverfishWakeUpFriendsGoal` 的一刻：扫到虫蚀方块时按 `mobGriefing` 分派，
/// 每处理一格取一次 `nextBoolean`，为真就停。扫描顺序按官方的三个嵌套循环。
///
/// One tick of `SilverfishWakeUpFriendsGoal`: every infested block is handled under
/// `mobGriefing`, then one `nextBoolean` decides whether to stop early.
pub(crate) fn wake_tick(me: &mut Entity, _mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let origin = me.block_position();
    let griefing = ctx.blocks().block_game_rules().mob_griefing;
    for y in axis(5) {
        for x in axis(10) {
            for z in axis(10) {
                let pos = origin.offset(x, y, z);
                let state = ctx.blocks().block_state(pos);
                if !infested::is_infested(state) {
                    continue;
                }
                let command = if griefing {
                    Command::SilverfishBreak { pos }
                } else {
                    Command::SilverfishReveal {
                        pos,
                        host: infested::host_state_by_infested(state).unwrap_or(state),
                    }
                };
                ctx.emit(command, me);
                if me.random.next_boolean() {
                    return;
                }
            }
        }
    }
}

/// 官方 `BlockPos.MutableBlockPos.set(k, j, i)` 的循环顺序：0,1,-1,2,-2,... 一直到 ±limit。
///
/// The vanilla scan order: 0, 1, -1, 2, -2, ... up to ±limit.
fn axis(limit: i32) -> impl Iterator<Item = i32> {
    let bound = limit.unsigned_abs();
    (0..=bound)
        .flat_map(|magnitude| {
            if magnitude == 0 {
                [Some(0), None]
            } else {
                [Some(magnitude as i32), Some(-(magnitude as i32))]
            }
        })
        .flatten()
}
