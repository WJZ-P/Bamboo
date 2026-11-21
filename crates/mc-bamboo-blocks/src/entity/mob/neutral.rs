//! 中立生物的持久愤怒：官方 `NeutralMob` 接口里和种类无关的部分。
//!
//! 北极熊先用；狼、蜜蜂、末影人、僵尸猪灵这些实现 `NeutralMob` 的种类可以直接复用：
//! 状态放在 [`Mob::anger`]，每刻在自己的 `tick` 末尾调 [`update`]，存读盘调
//! [`write_nbt`] / [`read_nbt`]。`universalAnger` 规则没有建模（恒为假）。
//!
//! Persistent anger of neutral mobs: the species-independent part of vanilla
//! `NeutralMob`. Polar bears use it first; wolves, bees, endermen and other
//! `NeutralMob`s can reuse it. State lives in [`Mob::anger`]; call [`update`] at the end
//! of the mob's tick and [`write_nbt`] / [`read_nbt`] when saving. The `universalAnger`
//! rule is not modelled (always false).

use mc_bamboo_nbt::Tag;

use super::{Mob, goals, nbt};
use crate::entity::{
    Entity, EntityUid, KindPublic,
    ctx::{EntityCtx, PlayerView},
};

/// `NeutralMob.NO_ANGER_END_TIME`。
const NO_ANGER_END_TIME: i64 = -1;

/// `persistentAngerEndTime` 和 `persistentAngerTarget`。
///
/// `persistentAngerEndTime` and `persistentAngerTarget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anger {
    /// 愤怒结束的绝对时刻（`World::game_time` 的时基，岛合并时平移）。Java 字段的初值是 0。
    ///
    /// The absolute game time the anger ends at (the `World::game_time` base, shifted when
    /// islands merge). The Java field starts at 0.
    pub end_time: i64,
    /// 愤怒对象的 UUID（`EntityReference`）。
    ///
    /// The UUID of the anger target (an `EntityReference`).
    pub target: Option<[u8; 16]>,
}

impl Anger {
    /// `NeutralMob.isAngry`：结束时刻为正且还没到。
    ///
    /// `NeutralMob.isAngry`: the end time is positive and still ahead.
    pub const fn is_angry(&self, now: i64) -> bool {
        self.end_time > 0 && self.end_time - now > 0
    }

    /// 岛合并时平移结束时刻；0 和 -1 是「没有」的标记，不动。
    ///
    /// Shifts the end time when islands merge; 0 and -1 mean "none" and stay.
    pub(super) const fn rebase(&mut self, delta: i64) {
        if self.end_time > 0 {
            self.end_time = self.end_time.saturating_add(delta);
        }
    }
}

/// `Mob.target` 这一刻指向的活物：UUID、是不是玩家、是不是已经死了。查不到（离岛）时是 `None`。
///
/// The living thing `Mob.target` points at: UUID, whether it is a player and whether it is
/// dead. `None` when it cannot be found (off this island).
struct RawTarget {
    uuid: Option<[u8; 16]>,
    player: Option<PlayerView>,
    dead: bool,
}

fn raw_target(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<RawTarget> {
    if let Some(player) = goals::player_by_uid(ctx.blocks(), uid) {
        return Some(RawTarget {
            uuid: player.uuid,
            dead: !player.alive,
            player: Some(player),
        });
    }
    let other = ctx.other(uid)?;
    let KindPublic::Mob(public) = &other.kind else {
        return None;
    };
    Some(RawTarget {
        uuid: Some(public.uuid),
        player: None,
        dead: !public.is_alive(),
    })
}

/// `NeutralMob.isValidPlayerTarget`：非创造、非旁观的玩家，且不是和平难度。
///
/// `NeutralMob.isValidPlayerTarget`: a non-creative, non-spectator player outside peaceful.
fn valid_player_target(player: &PlayerView, difficulty: i32) -> bool {
    !player.creative && !player.spectator && difficulty != 0
}

/// `NeutralMob.stopBeingAngry`：忘掉打过自己的生物、愤怒对象和当前目标。
///
/// `NeutralMob.stopBeingAngry`: forget the last attacker, the anger target and the target.
pub(super) fn stop_being_angry(mob: &mut Mob) {
    mob.last_hurt_by_mob = None;
    mob.last_hurt_by_mob_ref = None;
    mob.anger.target = None;
    mob.target = None;
    mob.anger.end_time = NO_ANGER_END_TIME;
}

/// `NeutralMob.updatePersistentAnger(level, stayAngryIfTargetPresent)`。`remaining` 是
/// `startPersistentAngerTimer` 抽的刻数（`PERSISTENT_ANGER_TIME.sample(random)`），由种类给。
///
/// `NeutralMob.updatePersistentAnger(level, stayAngryIfTargetPresent)`. `remaining` draws
/// the species' `PERSISTENT_ANGER_TIME.sample(random)` for `startPersistentAngerTimer`.
pub(super) fn update(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    stay_angry_if_target_present: bool,
    remaining: fn(&mut Entity) -> i64,
) {
    let now = ctx.blocks().game_time();
    let difficulty = ctx.blocks().difficulty();
    let previous = mob.target.and_then(|uid| raw_target(ctx, uid));
    let anger_target = mob.anger.target;
    // 目标是别的生物、已经死了、又正好是愤怒对象：消气。
    // A dead mob target that is also the anger target ends the anger.
    if let Some(previous) = &previous
        && previous.dead
        && previous.player.is_none()
        && anger_target.is_some()
        && anger_target == previous.uuid
    {
        stop_being_angry(mob);
        return;
    }
    // `getTarget()`：打不了的目标读成 null。
    // `getTarget()`: a target that cannot be attacked reads as null.
    let target = previous.filter(|_| goals::combat_target(mob, ctx).is_some());
    if let Some(target) = &target {
        let new_target = anger_target.is_none() || anger_target != target.uuid;
        if new_target {
            mob.anger.target = target.uuid;
        }
        if new_target || stay_angry_if_target_present {
            mob.anger.end_time = now + remaining(me);
        }
    }
    let valid_player = target
        .as_ref()
        .and_then(|target| target.player.as_ref())
        .is_some_and(|player| valid_player_target(player, difficulty));
    if anger_target.is_some()
        && !mob.anger.is_angry(now)
        && (!valid_player || !stay_angry_if_target_present)
    {
        stop_being_angry(mob);
    }
    // 愤怒对象是创造 / 旁观的玩家，或者难度是和平：消气。官方这里用的是进函数时读到的对象。
    // A creative or spectator anger target, or peaceful difficulty, ends the anger. Vanilla
    // tests the reference it read on entry.
    if let Some(uuid) = anger_target
        && let Some(player) = player_by_uuid(ctx, uuid)
        && (player.creative || player.spectator || difficulty == 0)
    {
        stop_being_angry(mob);
    }
}

fn player_by_uuid(ctx: &dyn EntityCtx, uuid: [u8; 16]) -> Option<PlayerView> {
    let mut found = None;
    ctx.blocks().for_each_player(&mut |player| {
        if player.uuid == Some(uuid) {
            found = Some(*player);
        }
    });
    found
}

/// `NeutralMob.isAngryAt` 在玩家身上的结果：能打（`canAttack`），并且愤怒对象就是他。
/// `isAngryAtAllPlayers` 要 `universalAnger` 规则，没有建模。
///
/// `NeutralMob.isAngryAt` for a player: attackable (`canAttack`) and the anger target.
/// `isAngryAtAllPlayers` needs the unmodelled `universalAnger` rule.
pub(super) fn is_angry_at_player(mob: &Mob, player: &PlayerView, difficulty: i32) -> bool {
    difficulty != 0
        && player.alive
        && !player.spectator
        && !player.creative
        && mob.anger.target.is_some()
        && mob.anger.target == player.uuid
}

/// `NeutralMob.addPersistentAngerSaveData`：`anger_end_time` 一定写，`angry_at` 有才写。
///
/// `NeutralMob.addPersistentAngerSaveData`: `anger_end_time` always, `angry_at` when set.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push(("anger_end_time".to_owned(), Tag::Long(mob.anger.end_time)));
    if let Some(uuid) = &mob.anger.target {
        out.push(("angry_at".to_owned(), nbt::uuid_tag(uuid)));
    }
}

/// `NeutralMob.readPersistentAngerSaveData`：没有 `anger_end_time` 时是 -1。旧存档的
/// `AngerTime`（剩余刻数）没有读，见 README 已知限制；`setTarget` 那一步也不做，
/// 目标由 `NearestAttackableTargetGoal(isAngryAt)` 重新找回来。
///
/// `NeutralMob.readPersistentAngerSaveData`: -1 without `anger_end_time`. The legacy
/// `AngerTime` (remaining ticks) is not read (see the README limits), nor is the target
/// restored here; `NearestAttackableTargetGoal(isAngryAt)` finds it again.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    mob.anger.end_time = crate::archive::tag::long_or(tag, "anger_end_time", NO_ANGER_END_TIME);
    mob.anger.target = nbt::uuid_field(tag, "angry_at");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::MobType;

    #[test]
    fn anger_lasts_until_the_end_time() {
        let anger = Anger {
            end_time: 100,
            target: None,
        };
        assert!(anger.is_angry(99));
        assert!(!anger.is_angry(100));
        // Java 字段的初值 0 和 -1 都不算生气。
        // Neither the Java default 0 nor -1 counts as angry.
        assert!(!Anger::default().is_angry(-5));
    }

    #[test]
    fn rebase_moves_only_real_end_times() {
        let mut anger = Anger {
            end_time: 100,
            target: None,
        };
        anger.rebase(50);
        assert_eq!(anger.end_time, 150);
        let mut none = Anger {
            end_time: NO_ANGER_END_TIME,
            target: None,
        };
        none.rebase(50);
        assert_eq!(none.end_time, NO_ANGER_END_TIME);
    }

    #[test]
    fn save_data_round_trips() {
        let mut mob = Mob::new(MobType::PolarBear);
        mob.anger = Anger {
            end_time: 1234,
            target: Some([7; 16]),
        };
        let mut entries = Vec::new();
        write_nbt(&mob, &mut entries);
        let mut loaded = Mob::new(MobType::PolarBear);
        read_nbt(&mut loaded, &Tag::Compound(entries));
        assert_eq!(loaded.anger, mob.anger);
        // 没有键时结束时刻是 -1。
        // Without the key the end time is -1.
        read_nbt(&mut loaded, &Tag::Compound(Vec::new()));
        assert_eq!(loaded.anger.end_time, NO_ANGER_END_TIME);
        assert_eq!(loaded.anger.target, None);
    }
}
