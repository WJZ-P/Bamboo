//! `BedBlock`、`StrawBedBlock`，官方的共同父类是 `AbstractBedBlock`。
//!
//! 主世界的 `BedRule.canSleep` 是 `when_dark`（官方 `Level.isDarkOutside` 读 `getSkyDarken()`），
//! 右键完整的床会走 `ServerPlayer.startSleepInBed` 的全部条件；成功后床的 `occupied` 置位，
//! 失败时按 `BedSleepingProblem` 给玩家发快捷栏提示。玩家睡姿、重生点、睡眠统计和醒来
//! 由游戏层处理，方块侧只把结果放进 `Effect::SleepInBed`。
//! 主世界里 `destroyOnUse` 总是假，`BedBlock.destroyOnUse`、`StrawBedBlock.destroyOnUse` 走不到。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    level::{BedProblem, Effect},
    state::{BED_PART, BedPart, BlockState, HORIZONTAL_FACING, OCCUPIED},
    world::{UPDATE_ALL, UPDATE_SUPPRESS_DROPS, World},
};

/// `AbstractBedBlock`。
pub(super) struct Bed;

/// `getNeighbourDirection`：另一半在哪个方向。
fn neighbour_direction(part: BedPart, facing: Direction) -> Direction {
    if part == BedPart::Foot {
        facing
    } else {
        facing.opposite()
    }
}

/// 官方 `ServerPlayer.isReachableBedBlock`：玩家离床（或另一半）水平 3 格、垂直 2 格以内。
///
/// Official `ServerPlayer.isReachableBedBlock`: within 3 blocks horizontally and
/// 2 blocks vertically of the bed (or its other half).
fn reachable(actor: &Actor, pos: BlockPos) -> bool {
    let center_x = f64::from(pos.x) + 0.5;
    let center_z = f64::from(pos.z) + 0.5;
    (actor.position.x - center_x).abs() <= 3.0
        && (actor.position.y - f64::from(pos.y)).abs() <= 2.0
        && (actor.position.z - center_z).abs() <= 3.0
}

/// `ServerPlayer.freeAt`：这一格不让人窒息就能站。
///
/// `ServerPlayer.freeAt`: a non-suffocating block.
fn free_at(world: &World, pos: BlockPos) -> bool {
    !world.block_state(pos).is_suffocating()
}

/// `ServerPlayer.bedBlocked`：床尾和床头正上方有一格被挡住。
///
/// `ServerPlayer.bedBlocked`: either the space above the foot or above the
/// head is blocked.
fn bed_blocked(world: &World, pos: BlockPos, facing: Direction) -> bool {
    let above = pos.above();
    !free_at(world, above) || !free_at(world, above.relative(facing.opposite()))
}

/// 官方 `STRawBedBlock` 用 `STRAW_BED_RULE`（`canSetSpawn = never`），普通床用
/// `BED_RULE`（`canSetSpawn = always`）。两者的 `destroyOnUse` 在主世界都是假。
///
/// Official `StrawBedBlock` uses `STRAW_BED_RULE` (`canSetSpawn = never`),
/// ordinary beds use `BED_RULE` (`canSetSpawn = always`). Neither destroys on
/// use in the overworld.
fn can_set_spawn(state: BlockState) -> bool {
    state.class() != BlockClass::StrawBedBlock
}

impl Behaviour for Bed {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let facing = ctx.horizontal_direction();
        let head = ctx.clicked_pos().relative(facing);
        let free = world.block_state(head).can_be_replaced(ctx, world)
            && world.is_within_world_border(head);
        free.then(|| default_state(block).with(HORIZONTAL_FACING, facing))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let part = state.get(BED_PART);
        if direction != neighbour_direction(part, state.get(HORIZONTAL_FACING)) {
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        if neighbor.is_same_block(state) && neighbor.get(BED_PART) != part {
            state.with(OCCUPIED, neighbor.get(OCCUPIED))
        } else {
            BlockState::AIR
        }
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let head = pos.relative(state.get(HORIZONTAL_FACING));
        world.set_block_and_update(head, state.with(BED_PART, BedPart::Head));
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        if state.get(BED_PART) == BedPart::Foot {
            remove_head_silently(state, world, pos);
        }
        Plain.player_will_destroy(state, world, pos, player)
    }

    /// `AbstractBedBlock.useWithoutItem` → `ServerPlayer.startSleepInBed`。
    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let part = state.get(BED_PART);
        let facing = state.get(HORIZONTAL_FACING);
        let direction = neighbour_direction(part, facing);
        let other = world.block_state(ctx.clicked_pos().relative(direction));
        // 不是一整张床：官方 `CONSUME`，不进玩家的睡觉流程。
        if !other.is_same_block(state) || other.get(BED_PART) == part {
            return InteractionResult::Consume;
        }
        // 官方 `AbstractBedBlock.useWithoutItem`：点到床尾时 `pos = pos.relative(facing)`
        // 换到床头，点到床头时不动；`getBedRule` / `startSleepInBed` 都用这一格，
        // `LivingEntity.startSleeping` 也把 `occupied` 置在床头。
        //
        // Official `AbstractBedBlock.useWithoutItem`: clicking the foot moves
        // `pos` to the head (`pos.relative(facing)`); clicking the head leaves
        // it. `getBedRule`, `startSleepInBed` and `LivingEntity.startSleeping`
        // all use that head position, including its `occupied` flag.
        let head = if part == BedPart::Foot {
            ctx.clicked_pos().relative(facing)
        } else {
            ctx.clicked_pos()
        };
        let head_state = world.block_state(head);
        if head_state.get(OCCUPIED) {
            // 已经有人睡：官方先试着踢村民，没有村民就发 `block.minecraft.bed.occupied`。
            world.push_effect(Effect::Overlay {
                key: "block.minecraft.bed.occupied",
                first: None,
                second: None,
            });
            return InteractionResult::SuccessServer;
        }
        let set_spawn = can_set_spawn(head_state);
        let problem = sleep_problem(world, ctx, head, facing, set_spawn);
        if problem.is_none() {
            // `LivingEntity.startSleeping`：`Level.setBlockAndUpdate(pos, state.setValue(OCCUPIED, true))`。
            // 形状更新会把 `occupied` 传给另一半，也会让旁边多余的床脚变空气。
            world.set_block_and_update(head, head_state.with(OCCUPIED, true));
        }
        world.push_effect(Effect::SleepInBed {
            pos: head,
            set_spawn,
            problem,
        });
        InteractionResult::SuccessServer
    }

    /// `StrawBedBlock` 放的是 2014 号事件，不是破坏效果。
    fn spawn_destroy_particles(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.class() == BlockClass::StrawBedBlock {
            world.level_event(2014, pos, state.0 as i32);
        } else {
            Plain.spawn_destroy_particles(state, world, pos);
        }
    }
}

/// `ServerPlayer.startSleepInBed` 的各个失败条件，按字节码顺序。
///
/// The failure branches of `ServerPlayer.startSleepInBed`, in bytecode order.
fn sleep_problem(
    world: &World,
    ctx: &UseContext,
    foot: BlockPos,
    facing: Direction,
    set_spawn: bool,
) -> Option<BedProblem> {
    let actor = ctx.actor;
    // `canSleep(level)` / `canSetSpawn(level)`：两种床都是 `when_dark` / 固定值，
    // 只有 `canSleep` 依赖天色。
    let can_sleep = world.is_dark_outside();
    if !reachable(actor, foot) && !reachable(actor, foot.relative(facing.opposite())) {
        return Some(BedProblem::TooFarAway);
    }
    if bed_blocked(world, foot, facing) {
        return Some(BedProblem::Obstructed);
    }
    // `canSetSpawn` 为真时官方先设重生点，再检查 `canSleep`；
    // 重生点由游戏层的 `Effect::SleepInBed` 处理。
    let _ = set_spawn;
    if !can_sleep {
        return Some(BedProblem::NotPossibleNow);
    }
    // 创造模式跳过怪物检查；世界里还没有怪物，但按官方逻辑保留这个入口，
    // 让僵尸分支合并之后自然生效。
    if !actor.prevents_block_drops && world.monster_prevents_rest(foot) {
        return Some(BedProblem::NotSafe);
    }
    // `Player.startSleepInBed` 里 `startSleeping(pos)` 还会检查床的形状非空：
    // 这里两半都在，`getSleepHeight` 的 VoxelShape 一定非空。
    None
}

/// `playerWillDestroy` 里的那一段：破坏床尾时先静默地拿走床头，这样不会多掉一份。
fn remove_head_silently(foot: BlockState, world: &mut World, pos: BlockPos) {
    let head_pos = pos.relative(foot.get(HORIZONTAL_FACING));
    let head = world.block_state(head_pos);
    if !head.is_same_block(foot) || head.get(BED_PART) != BedPart::Head {
        return;
    }
    world.set_block(
        head_pos,
        BlockState::AIR,
        UPDATE_ALL | UPDATE_SUPPRESS_DROPS,
    );
    world.push_effect(Effect::Destroyed {
        pos: head_pos,
        state: head,
        except_initiator: true,
    });
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, break_block, use_item},
    };

    const FOOT: BlockPos = FLOOR.above();

    fn placed_bed() -> crate::testing::TestLevel {
        let mut level = level_with_floor();
        level.set(FLOOR.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_holding(items::RED_BED), top_of(FLOOR));
        level
    }

    /// `ServerPlayer.bedInRange`：站在床尾旁边、床面高度上。
    fn sleeper() -> crate::context::Actor {
        crate::context::Actor {
            position: mc_bamboo_core::Vec3::new(
                f64::from(FOOT.x) + 0.5,
                f64::from(FOOT.y),
                f64::from(FOOT.z) + 0.5,
            ),
            ..actor_holding(items::AIR)
        }
    }

    #[test]
    fn placing_a_bed_builds_both_halves_ahead_of_the_player() {
        let level = placed_bed();
        let foot = level.get(FOOT);
        let head = level.get(FOOT.south());
        assert!(foot.is(blocks::RED_BED));
        assert_eq!(foot.get(HORIZONTAL_FACING), Direction::South);
        assert_eq!(foot.get(BED_PART), BedPart::Foot);
        assert_eq!(head, foot.with(BED_PART, BedPart::Head));
    }

    /// oracle 的场景都在原点附近，碰不到世界边界。
    #[test]
    fn the_head_of_a_bed_stays_inside_the_world_border() {
        let mut actor = actor_holding(items::RED_BED);
        actor.yaw = 270.0;
        for (x, expected) in [
            (29_999_982, InteractionResult::Success),
            (29_999_983, InteractionResult::Fail),
        ] {
            let floor = BlockPos::new(x, 64, 0);
            let mut level = crate::testing::TestLevel::new();
            level.set(floor, BlockState(blocks::STONE));
            let outcome = use_item(&mut level, &actor, top_of(floor));
            assert_eq!(outcome.result, expected);
        }
    }

    /// 白天 `when_dark` 不让睡：床的 `occupied` 不变，只有一条提示。
    #[test]
    fn a_bright_day_refuses_to_sleep() {
        let mut level = placed_bed();
        let outcome = use_item(&mut level, &sleeper(), top_of(FOOT));
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(!level.get(FOOT).get(OCCUPIED));
        assert!(outcome.effects.iter().any(|effect| matches!(
            effect,
            Effect::SleepInBed {
                problem: Some(BedProblem::NotPossibleNow),
                ..
            }
        )));
    }

    /// 夜里能睡：床尾和床头的 `occupied` 都置位。
    #[test]
    fn a_dark_night_occupies_both_halves() {
        let mut level = placed_bed();
        level.sky_darken = 11;
        let outcome = use_item(&mut level, &sleeper(), top_of(FOOT));
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(level.get(FOOT).get(OCCUPIED));
        assert!(level.get(FOOT.south()).get(OCCUPIED));
        assert!(
            outcome
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::SleepInBed { problem: None, .. }))
        );
    }

    #[test]
    fn breaking_either_half_removes_the_bed() {
        for broken in [FOOT, FOOT.south()] {
            let mut level = placed_bed();
            break_block(&mut level, broken);
            assert!(level.get(FOOT).is_air(), "{broken:?} 的床尾应该没了");
            assert!(
                level.get(FOOT.south()).is_air(),
                "{broken:?} 的床头应该没了"
            );
        }
    }
}
