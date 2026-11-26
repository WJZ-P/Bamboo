//! `RunAroundLikeCrazyGoal`：没驯服的马科被人骑着时朝随机方向乱跑，偶尔抽签把人甩下来，
//! 抽中了就 `tameWithName`。
//!
//! `RunAroundLikeCrazyGoal`: an untamed equine with a rider bolts towards a random
//! spot and now and then rolls to throw the rider off; a success tames it.

use mc_bamboo_core::Vec3;

use super::{EquineState, Mob, life};
use crate::entity::{
    Entity,
    ctx::EntityCtx,
    mob::{
        MobEvent,
        goals::{default_random_pos, reduced_tick_delay},
        living,
    },
};

fn state(mob: &mut Mob) -> &mut EquineState {
    mob.equine.as_mut().expect("equine state")
}

/// `AbstractHorse.isTamed()`。
///
/// `AbstractHorse.isTamed()`.
fn is_tamed(mob: &Mob) -> bool {
    mob.equine.as_ref().is_some_and(|state| state.tame)
}

/// `RunAroundLikeCrazyGoal.canUse`：没被生物控制、没驯服、有人骑着，再取一个随机点。
/// Bamboo 没有 `isMobControlled()` 为真的马科，所以第一位恒真。
///
/// `RunAroundLikeCrazyGoal.canUse`: not mob controlled, not tamed, ridden, then a
/// random spot. No Bamboo equine is mob controlled, so the first test is always true.
pub(in crate::entity::mob) fn can_use(
    me: &mut Entity,
    mob: &Mob,
    ctx: &mut dyn EntityCtx,
    wanted: &mut Vec3,
) -> bool {
    if is_tamed(mob) || !mob.is_vehicle() {
        return false;
    }
    match default_random_pos(me, mob, ctx.blocks(), 5, 4) {
        Some(pos) => {
            *wanted = pos;
            true
        }
        None => false,
    }
}

/// `RunAroundLikeCrazyGoal.start`：以 1.2 的倍速走向那个点。
///
/// `RunAroundLikeCrazyGoal.start`: walk to the spot at 1.2 times the speed.
pub(in crate::entity::mob) fn start(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    wanted: Vec3,
    speed: f64,
) {
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation.move_to(ctx.blocks(), &nav, wanted, speed);
}

/// `RunAroundLikeCrazyGoal.canContinueToUse`：还没驯服、路没走完、还有人在上面。
///
/// `RunAroundLikeCrazyGoal.canContinueToUse`: still untamed, the path is not done
/// and someone is still riding.
pub(in crate::entity::mob) fn can_continue(mob: &Mob) -> bool {
    !is_tamed(mob) && !mob.navigation.is_done() && mob.is_vehicle()
}

/// `RunAroundLikeCrazyGoal.tick`：按 `nextInt(adjustedTickDelay(50))` 抽签；抽中就
/// `nextInt(maxTemper) < temper` 决定驯服，否则加 5 点脾气，再甩人、扬蹄、发事件 6。
///
/// `RunAroundLikeCrazyGoal.tick`: draw `nextInt(adjustedTickDelay(50))`; on a hit,
/// `nextInt(maxTemper) < temper` tames and keeps the rider. On failure, temper
/// grows by 5, the rider is thrown off, the horse rears and entity event 6 goes out.
pub(in crate::entity::mob) fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if is_tamed(mob) {
        return;
    }
    // `requiresUpdateEveryTick()` 为假，目标两刻才 tick 一次，所以间隔折半。
    //
    // `requiresUpdateEveryTick()` is false, so the goal ticks every other tick and
    // the interval is halved.
    if me.random.next_int_bounded(reduced_tick_delay(50)) != 0 {
        return;
    }
    let Some(rider) = mob.first_passenger() else {
        return;
    };
    if let Some(player) = ctx.player(rider)
        && let Some(uuid) = player.uuid
    {
        let max_temper = super::interact::max_temper(mob.mob_type);
        let temper = state(mob).temper;
        if max_temper > 0 && me.random.next_int_bounded(max_temper) < temper {
            tame_with_name(mob, Some(uuid));
            // 马科的驯服发生在 AI tick 里，不在交互结果上；用实体事件把
            // 玩家 UUID 带回游戏层，让进度触发器也能补上这一条。
            mob.events
                .push(crate::entity::mob::MobEvent::Tamed { player: uuid });
            return;
        }
        modify_temper(mob, 5);
    }
    // `ejectPassengers`：玩家表里的骑乘关系由游戏层按乘客名单摘掉。
    // 记下这一刻刚甩掉的骑手，避免同一刻后面的 `push_entities` 把还没挪走的骑手当成推挤来源。
    //
    // `ejectPassengers`: the game layer drops the riding relation when the
    // passenger list no longer holds the rider. Remember who was just ejected so
    // `push_entities` later in this tick does not push against the unmoved rider.
    mob.ejected_passenger = Some(rider);
    mob.passengers.clear();
    life::make_mad(me, mob);
    mob.events.push(MobEvent::EntityEvent { event: 6 });
}

/// `AbstractHorse.tameWithName`：记下主人、置驯服、广播事件 7。进度触发器
/// `TAME_ANIMAL` 在 Bamboo 里没有对应系统。
///
/// `AbstractHorse.tameWithName`: remember the owner, mark it tamed and broadcast
/// entity event 7. The `TAME_ANIMAL` criterion has no counterpart in Bamboo.
fn tame_with_name(mob: &mut Mob, owner: Option<[u8; 16]>) {
    let state = state(mob);
    state.owner = owner;
    state.tame = true;
    mob.events.push(MobEvent::EntityEvent { event: 7 });
}

/// `AbstractHorse.modifyTemper`：夹到 `[0, getMaxTemper()]`。
///
/// `AbstractHorse.modifyTemper`: clamp to `[0, getMaxTemper()]`.
fn modify_temper(mob: &mut Mob, amount: i32) {
    let max = super::interact::max_temper(mob.mob_type);
    let state = state(mob);
    state.temper = state.temper.wrapping_add(amount).clamp(0, max);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Vec3};

    use super::*;
    use crate::{
        BlockState, BlockTicks, World,
        entity::{Entity, EntityUid, JavaRandom, ctx::PreciseCtx, mob::MobType},
        testing::TestLevel,
    };

    const RIDER: EntityUid = 7;
    const RIDER_UUID: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    // 第一次 `nextInt(25)` 就命中。
    // The first `nextInt(25)` hits.
    const FIRST_ROLL_HIT: i64 = 18;

    fn rider() -> crate::entity::ctx::PlayerView {
        crate::entity::ctx::PlayerView {
            main_arm_right: true,
            fall_flying: false,
            shoulder_left: None,
            shoulder_right: None,
            uid: RIDER,
            uuid: Some(RIDER_UUID),
            position: Vec3::new(1.0, 64.0, 0.5),
            delta: Vec3::default(),
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::default(),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            yaw: 0.0,
            pitch: 0.0,
            combat: Default::default(),
            effects: Default::default(),
        }
    }

    /// 一块平地和一头被人骑着的马科，脾气按参数。
    ///
    /// A flat floor and a ridden equine with the given temper.
    fn ridden(kind: MobType, temper: i32, tame: bool) -> (TestLevel, Entity) {
        let mut level = TestLevel::new();
        for x in -10..=10 {
            for z in -10..=10 {
                level.set(
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }
        level.set(
            BlockPos::new(0, 63, 0),
            BlockState(mc_bamboo_data::blocks::GRASS_BLOCK),
        );
        level.players.push(rider());
        let mut horse = Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5));
        horse.on_ground = true;
        horse.with_mob(|_, mob| {
            let state = mob.equine.as_mut().expect("equine state");
            state.temper = temper;
            state.tame = tame;
            mob.passengers.push(RIDER);
        });
        (level, horse)
    }

    /// 抽中了就驯服：把脾气顶满以后 `nextInt(maxTemper) < temper` 恒真，
    /// 第一次抽中就是 `tameWithName`，骑手留着。
    ///
    /// A full temper makes `nextInt(maxTemper) < temper` always true, so the first
    /// hit tames and the rider stays on.
    #[test]
    fn a_full_temper_tames_the_rider_on_the_first_hit() {
        let (mut level, mut horse) = ridden(MobType::Horse, 100, false);
        horse.random = JavaRandom::new(FIRST_ROLL_HIT);
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let outcome = horse
            .with_mob(|me, mob| {
                let mut ctx = PreciseCtx::new(&mut world, me.id);
                tick(me, mob, &mut ctx);
                let state = mob.equine.as_ref().expect("equine state");
                (
                    state.tame,
                    state.owner,
                    mob.passengers.clone(),
                    mob.events.clone(),
                )
            })
            .expect("equine row");
        assert!(outcome.0, "the horse should be tamed");
        assert_eq!(outcome.1, Some(RIDER_UUID));
        assert_eq!(outcome.2, [RIDER]);
        assert!(
            outcome
                .3
                .iter()
                .any(|event| matches!(event, MobEvent::EntityEvent { event: 7 }))
        );
    }

    /// 脾气不够就甩人：乘客名单清空、脾气加 5、发事件 6，马还没驯服。
    ///
    /// A low temper throws the rider off: the passenger list empties, the temper
    /// grows by 5, entity event 6 goes out and the horse stays untamed.
    #[test]
    fn a_low_temper_throws_the_rider_and_grows_by_five() {
        let (mut level, mut horse) = ridden(MobType::Donkey, 0, false);
        horse.random = JavaRandom::new(FIRST_ROLL_HIT);
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let outcome = horse
            .with_mob(|me, mob| {
                let mut ctx = PreciseCtx::new(&mut world, me.id);
                tick(me, mob, &mut ctx);
                let state = mob.equine.as_ref().expect("equine state");
                (
                    state.tame,
                    state.temper,
                    mob.passengers.clone(),
                    mob.events.clone(),
                )
            })
            .expect("equine row");
        assert!(!outcome.0);
        assert_eq!(outcome.1, 5);
        assert!(outcome.2.is_empty());
        assert!(
            outcome
                .3
                .iter()
                .any(|event| matches!(event, MobEvent::EntityEvent { event: 6 }))
        );
    }

    /// 骑手不被当成注视目标（`EntitySelector.notRiding`），乱跑目标照常启动。
    ///
    /// The rider is not a look target (`EntitySelector.notRiding`) while the
    /// bolting goal still starts.
    #[test]
    fn the_rider_is_not_a_look_target() {
        let (mut level, mut horse) = ridden(MobType::Horse, 0, false);
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let seen = horse
            .with_mob(|me, mob| {
                let mut seen = Vec::new();
                for tick in 1..=400 {
                    if !mob.is_vehicle() {
                        break;
                    }
                    me.tick_count = tick;
                    let mut goals = std::mem::take(&mut mob.goals);
                    {
                        let mut ctx = PreciseCtx::new(&mut world, me.id);
                        goals.tick(me, mob, &mut ctx);
                    }
                    mob.goals = goals;
                    for name in mob.goals.running_names(mob.mob_type) {
                        if !seen.contains(&name) {
                            seen.push(name);
                        }
                    }
                }
                seen
            })
            .expect("equine row");
        assert!(seen.contains(&"RunAroundLikeCrazyGoal"), "{seen:?}");
        assert!(!seen.contains(&"LookAtPlayerGoal"), "{seen:?}");
    }

    /// `canUse` 的三个条件：没驯服、有人骑着、找得到一个随机点。
    ///
    /// The three `canUse` conditions: untamed, ridden, and a random spot exists.
    #[test]
    fn only_an_untamed_ridden_equine_starts_the_goal() {
        for (kind, tame) in [
            (MobType::Horse, false),
            (MobType::Llama, false),
            (MobType::Horse, true),
        ] {
            let (mut level, mut horse) = ridden(kind, 0, tame);
            let mut ticks = BlockTicks::default();
            let mut world = World::new(&mut level, &mut ticks);
            let started = horse
                .with_mob(|me, mob| {
                    let mut ctx = PreciseCtx::new(&mut world, me.id);
                    let mut wanted = Vec3::default();
                    can_use(me, mob, &mut ctx, &mut wanted)
                })
                .expect("equine row");
            assert_eq!(started, !tame, "{kind:?} tame={tame}");
        }
        let (mut level, mut horse) = ridden(MobType::Horse, 0, false);
        horse.with_mob(|_, mob| mob.passengers.clear());
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let started = horse
            .with_mob(|me, mob| {
                let mut ctx = PreciseCtx::new(&mut world, me.id);
                let mut wanted = Vec3::default();
                can_use(me, mob, &mut ctx, &mut wanted)
            })
            .expect("equine row");
        assert!(!started, "no rider means no goal");
    }
}
