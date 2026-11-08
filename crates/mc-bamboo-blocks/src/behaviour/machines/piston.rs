//! `PistonBaseBlock`、`PistonHeadBlock`、`MovingPistonBlock`。
//!
//! 活塞要信号才动：`checkIfExtend` 只发方块事件，事件在下一刻由 `runBlockEvents` 执行，
//! 这时才真的推方块。要移动的方块换成 `moving_piston` 加一个方块实体，两刻后放回原处，
//! 见 [`piston_entity`](super::piston_entity)。世界里没有实体和声音，推实体和播放音效的两段没写。

mod delete_after_move;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, PushReaction, blocks};

use self::delete_after_move::DeleteAfterMove;
use super::{
    facing_the_view, piston_entity::PistonMoving, piston_structure::PistonStructureResolver,
};
use crate::{
    behaviour::{Behaviour, Plain},
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    shape::Boxes,
    state::{BlockState, EXTENDED, FACING, PISTON_TYPE, PistonType},
    world::{
        UPDATE_CLIENTS, UPDATE_INVISIBLE, UPDATE_KNOWN_SHAPE, UPDATE_LIMIT, UPDATE_MOVE_BY_PISTON,
        UPDATE_NEIGHBORS, UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS, World,
    },
};

/// `PistonBaseBlock`。
pub(super) struct PistonBase;

impl Behaviour for PistonBase {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx).with(EXTENDED, false))
    }

    /// 放下时如果已经通电，立刻准备伸出。
    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        check_if_extend(world, pos, state);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_if_extend(world, pos, state);
    }

    /// `onPlace`：换成活塞时检查，指令放的已经有了方块实体就不检查。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) && !world.has_block_entity(pos) {
            check_if_extend(world, pos, state);
        }
    }

    /// `triggerEvent`：伸出、收回真的在这里发生。
    fn trigger_event(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        let direction = state.get(FACING);
        let extended_state = state.with(EXTENDED, true);
        let extend = get_neighbor_signal(world, pos, direction);
        if extend && (param_a == 1 || param_a == 2) {
            world.set_block(pos, extended_state, UPDATE_CLIENTS);
            return false;
        }
        if !extend && param_a == 0 {
            return false;
        }

        match param_a {
            0 => {
                if !move_blocks(world, pos, direction, true, is_sticky(state)) {
                    return false;
                }
                world.set_block(pos, extended_state, EXTENDED_FLAGS);
            }
            1 | 2 => {
                world.with_block_entity::<PistonMoving, _>(
                    pos.relative(direction),
                    |entity, world| {
                        entity.final_tick(world);
                    },
                );

                let sticky = is_sticky(state);
                let moving_state = moving_piston_state(direction, sticky);
                world.set_block(pos, moving_state, RETRACT_FLAGS);
                world.set_block_entity(Box::new(PistonMoving::new(
                    pos,
                    state
                        .default_state()
                        .with(FACING, Direction::from_id(param_b & 7)),
                    direction,
                    false,
                    true,
                )));
                world.update_neighbors_at(pos, moving_state.block());
                world.update_neighbour_shapes(moving_state, pos, UPDATE_CLIENTS, UPDATE_LIMIT);
                if sticky {
                    retract_sticky_piston(world, pos, direction, param_a);
                } else {
                    world.remove_block(pos.relative(direction));
                }
            }
            _ => {}
        }
        true
    }
}

/// 方块换成 `moving_piston` 用的 324：可见性、移动方块、不跑方块实体的移除副作用。
const MOVING_FLAGS: u32 =
    UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | UPDATE_MOVE_BY_PISTON | UPDATE_INVISIBLE;
/// 收回时写活塞自己那一格、以及清掉旧活塞头用的 276。
const RETRACT_FLAGS: u32 =
    UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | UPDATE_KNOWN_SHAPE | UPDATE_INVISIBLE;
/// 伸出时写活塞自己的 67：更新邻居和客户端，算作被活塞移动。
const EXTENDED_FLAGS: u32 = UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_MOVE_BY_PISTON;
/// 打掉 `Popped` 方块用的 18：通知客户端，跳过形状更新。
const POPPED_FLAGS: u32 = UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE;
/// 原来要推的方块最后剩下的格子写成空气用的 82。
const LEFTOVER_FLAGS: u32 = UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE | UPDATE_MOVE_BY_PISTON;

/// 这个状态是粘性活塞。
fn is_sticky(state: BlockState) -> bool {
    state.is(blocks::STICKY_PISTON)
}

/// 粘性活塞收回时，两格外的移动方块或者被粘回来的方块。
fn retract_sticky_piston(world: &mut World, pos: BlockPos, direction: Direction, param_a: i32) {
    let two_pos = pos.relative_by(direction, 2);
    let moving_state = world.block_state(two_pos);
    let piston_piece = moving_state.is(blocks::MOVING_PISTON)
        && world
            .block_entity::<PistonMoving>(two_pos)
            .is_some_and(|entity| entity.direction() == direction && entity.is_extending());
    if piston_piece {
        world.with_block_entity::<PistonMoving, _>(two_pos, |entity, world| {
            entity.final_tick(world);
        });
        return;
    }

    let can_pull = param_a == 1
        && !moving_state.is_air()
        && is_pushable(
            moving_state,
            world,
            two_pos,
            direction.opposite(),
            false,
            direction,
        )
        && (moving_state.push_reaction() == PushReaction::PushPull
            || moving_state.is(blocks::PISTON)
            || moving_state.is(blocks::STICKY_PISTON));
    if can_pull {
        move_blocks(world, pos, direction, false, true);
    } else {
        world.remove_block(pos.relative(direction));
    }
}

/// `checkIfExtend`：通电且没伸出就发伸出的方块事件，断电且伸出了就发收回的事件。
fn check_if_extend(world: &mut World, pos: BlockPos, state: BlockState) {
    let direction = state.get(FACING);
    let extend = get_neighbor_signal(world, pos, direction);
    if extend && !state.get(EXTENDED) {
        if PistonStructureResolver::new(pos, direction, true).resolve(world) {
            world.block_event(pos, state.block(), 0, direction.id());
        }
    } else if !extend && state.get(EXTENDED) {
        let pushed_pos = pos.relative_by(direction, 2);
        let pushed_state = world.block_state(pushed_pos);
        let mut event = 1;
        if pushed_state.is(blocks::MOVING_PISTON) && pushed_state.get(FACING) == direction {
            let moving = world.block_entity::<PistonMoving>(pushed_pos);
            if moving.is_some_and(|entity| {
                entity.is_extending()
                    && (entity.progress(0.0) < 0.5
                        || world.game_time() == entity.last_ticked()
                        || world.is_handling_tick())
            }) {
                event = 2;
            }
        }
        world.block_event(pos, state.block(), event, direction.id());
    }
}

/// `getNeighborSignal`：活塞方向和下面那一格不看，上面六格都要看。
fn get_neighbor_signal(world: &World, pos: BlockPos, push_direction: Direction) -> bool {
    for direction in Direction::ALL {
        if direction != push_direction && world.has_signal(pos.relative(direction), direction) {
            return true;
        }
    }
    if world.has_signal(pos, Direction::Down) {
        return true;
    }
    let above = pos.above();
    for direction in Direction::ALL {
        if direction != Direction::Down && world.has_signal(above.relative(direction), direction) {
            return true;
        }
    }
    false
}

/// `PistonBaseBlock.isPushable`：这一格的方块能不能被活塞带着走。
///
/// `allow_destroyable` 是推的一方能不能把 `Popped` 的方块打掉，`connection_direction` 是
/// 指定方块面的方向，`PushReaction.PUSH` 的方块只有面朝它时才跟着走。
pub(super) fn is_pushable(
    state: BlockState,
    world: &World,
    pos: BlockPos,
    direction: Direction,
    allow_destroyable: bool,
    connection_direction: Direction,
) -> bool {
    if !world.is_inside_build_height(pos.y) || !world.is_within_world_border(pos) {
        return false;
    }
    if state.is_air() {
        return true;
    }
    if direction == Direction::Down && pos.y == world.min_y() {
        return false;
    }
    if direction == Direction::Up && pos.y == world.max_y() {
        return false;
    }
    if !state.is(blocks::PISTON) && !state.is(blocks::STICKY_PISTON) {
        if state.is_unbreakable() {
            return false;
        }
        match state.push_reaction() {
            PushReaction::Immoveable => return false,
            PushReaction::Popped => return allow_destroyable,
            PushReaction::Push => return direction == connection_direction,
            PushReaction::PushPull | PushReaction::IgnoreEntity => {}
        }
    } else if state.get(EXTENDED) {
        return false;
    }
    !state.has_block_entity()
}

/// `moveBlocks`：把结构里的方块换成移动方块，收回时还要先清掉活塞头。
fn move_blocks(
    world: &mut World,
    piston_pos: BlockPos,
    direction: Direction,
    extending: bool,
    sticky: bool,
) -> bool {
    let arm_pos = piston_pos.relative(direction);
    if !extending && world.block_state(arm_pos).is(blocks::PISTON_HEAD) {
        world.set_block(arm_pos, BlockState::AIR, RETRACT_FLAGS);
    }

    let mut resolver = PistonStructureResolver::new(piston_pos, direction, extending);
    if !resolver.resolve(world) {
        return false;
    }
    let to_push = resolver.to_push().to_vec();
    let to_destroy = resolver.to_destroy().to_vec();
    let push_direction = resolver.push_direction();

    // 官方先用 `deleteAfterMove` 记下这些格子原来的样子，换完方块后剩下的写成空气。
    // 遍历顺序是 `HashMap` 的桶顺序，见 [`DeleteAfterMove`]。
    let mut delete_after_move = DeleteAfterMove::default();
    let mut to_push_shapes = Vec::with_capacity(to_push.len());
    for &pos in &to_push {
        let state = world.block_state(pos);
        delete_after_move.put(pos, state);
        to_push_shapes.push(state);
    }

    let mut to_update = Vec::with_capacity(to_push.len() + to_destroy.len());

    // `Popped` 的方块先打掉。官方还会掉落物品和播放破坏效果，这个世界没有掉落物。
    for &pos in to_destroy.iter().rev() {
        let state = world.block_state(pos);
        world.set_block(pos, BlockState::AIR, POPPED_FLAGS);
        to_update.push(state);
    }

    for i in (0..to_push.len()).rev() {
        let pos = to_push[i];
        let block_state = world.block_state(pos);
        let moved = pos.relative(push_direction);
        delete_after_move.remove(moved);
        // 被推的方块只记朝向，`TYPE` 留在默认的 normal，官方的 `moveBlocks` 就是这样。
        let actual = BlockState(blocks::MOVING_PISTON).with(FACING, direction);
        world.set_block(moved, actual, MOVING_FLAGS);
        world.set_block_entity(Box::new(PistonMoving::new(
            moved,
            to_push_shapes[i],
            direction,
            extending,
            false,
        )));
        to_update.push(block_state);
    }

    if extending {
        let piston_type = if sticky {
            PistonType::Sticky
        } else {
            PistonType::Normal
        };
        let head = BlockState(blocks::PISTON_HEAD)
            .with(FACING, direction)
            .with(PISTON_TYPE, piston_type);
        let moving = moving_piston_state(direction, sticky);
        delete_after_move.remove(arm_pos);
        world.set_block(arm_pos, moving, MOVING_FLAGS);
        world.set_block_entity(Box::new(PistonMoving::new(
            arm_pos, head, direction, true, true,
        )));
    }

    let air = BlockState::AIR;
    for (pos, _) in delete_after_move.iter() {
        world.set_block(pos, air, LEFTOVER_FLAGS);
    }
    for (pos, old_state) in delete_after_move.iter() {
        old_state.update_indirect_neighbour_shapes(world, pos, UPDATE_CLIENTS, UPDATE_LIMIT);
        world.update_neighbour_shapes(air, pos, UPDATE_CLIENTS, UPDATE_LIMIT);
        air.update_indirect_neighbour_shapes(world, pos, UPDATE_CLIENTS, UPDATE_LIMIT);
    }

    let mut update_index = 0;
    for &pos in to_destroy.iter().rev() {
        let state = to_update[update_index];
        update_index += 1;
        state.affect_neighbors_after_removal(world, pos);
        state.update_indirect_neighbour_shapes(world, pos, UPDATE_CLIENTS, UPDATE_LIMIT);
        world.update_neighbors_at(pos, state.block());
    }
    for &pos in to_push.iter().rev() {
        let state = to_update[update_index];
        update_index += 1;
        world.update_neighbors_at(pos, state.block());
    }
    if extending {
        world.update_neighbors_at(arm_pos, BlockState(blocks::PISTON_HEAD).block());
    }
    true
}

/// `MovingPistonBlock` 的默认状态加上朝向和类型。收回时活塞自己那一格和活塞头用这个。
fn moving_piston_state(direction: Direction, sticky: bool) -> BlockState {
    let piston_type = if sticky {
        PistonType::Sticky
    } else {
        PistonType::Normal
    };
    BlockState(blocks::MOVING_PISTON)
        .with(FACING, direction)
        .with(PISTON_TYPE, piston_type)
}

/// `PistonHeadBlock`。
pub(super) struct PistonHead;

/// 活塞头背后的那一格。
fn base_pos(arm: BlockState, pos: BlockPos) -> BlockPos {
    pos.relative(arm.get(FACING).opposite())
}

/// `isFittingBase`。
fn is_fitting_base(arm: BlockState, base: BlockState) -> bool {
    let base_block = if arm.get(PISTON_TYPE) == PistonType::Normal {
        blocks::PISTON
    } else {
        blocks::STICKY_PISTON
    };
    base.is(base_block) && base.get(EXTENDED) && base.get(FACING) == arm.get(FACING)
}

impl Behaviour for PistonHead {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let base = world.block_state(base_pos(state, pos));
        is_fitting_base(state, base)
            || base.is(blocks::MOVING_PISTON) && base.get(FACING) == state.get(FACING)
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
        if direction.opposite() == state.get(FACING) && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let base = base_pos(state, pos);
        if is_fitting_base(state, world.block_state(base)) {
            world.destroy_block(base, UPDATE_LIMIT);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        if state.can_survive(world, pos) {
            world.neighbor_changed(base_pos(state, pos), source);
        }
    }

    /// 官方只在玩家不掉落方块（创造模式的 `preventsBlockDrops`）时在这里拆底座，
    /// 生存模式改由 [`PistonHead::affect_neighbors_after_removal`] 拆。
    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        if player.prevents_block_drops {
            let base = base_pos(state, pos);
            if is_fitting_base(state, world.block_state(base)) {
                world.destroy_block(base, UPDATE_LIMIT);
            }
        }
        Plain.player_will_destroy(state, world, pos, player)
    }
}

/// `MovingPistonBlock`。方块实体还在时右键不吃点击，动画结束后那一格才会被拿走。
pub(super) struct MovingPiston;

impl Behaviour for MovingPiston {
    /// 没有方块实体的移动方块是残留，右键清掉它。
    fn use_without_item(
        &self,
        _state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if world.has_block_entity(ctx.clicked_pos()) {
            return InteractionResult::Pass;
        }
        world.remove_block(ctx.clicked_pos());
        InteractionResult::Consume
    }

    /// `destroy`：活塞头外面那一格换成空气时，把背后的活塞底座一起拿掉。
    fn destroy(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let relative = pos.relative(state.get(FACING).opposite());
        let base = world.block_state(relative);
        if (base.is(blocks::PISTON) || base.is(blocks::STICKY_PISTON)) && base.get(EXTENDED) {
            world.remove_block(relative);
        }
    }

    /// `getCollisionShape`：形状在方块实体里，跟着动画走。
    fn collision_boxes(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _context: crate::shape::CollisionContext,
    ) -> Boxes {
        match world.block_entity::<PistonMoving>(pos) {
            Some(entity) => entity.collision_boxes(),
            None => std::borrow::Cow::Owned(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        BlockTicks,
        testing::{TestLevel, actor_holding},
    };

    const BASE: BlockPos = BlockPos::new(0, 64, 0);
    const HEAD: BlockPos = BlockPos::new(0, 65, 0);

    /// 朝上伸出的活塞和它的活塞头。
    fn extended_piston() -> TestLevel {
        let mut level = TestLevel::new();
        let base = BlockState(blocks::PISTON)
            .with(FACING, Direction::Up)
            .with(EXTENDED, true);
        let head = BlockState(blocks::PISTON_HEAD)
            .with(FACING, Direction::Up)
            .with(PISTON_TYPE, PistonType::Normal);
        level.set(BASE, base);
        level.set(HEAD, head);
        level
    }

    fn break_head(level: &mut TestLevel, prevents_block_drops: bool) {
        let mut actor = actor_holding(items::AIR);
        actor.prevents_block_drops = prevents_block_drops;
        crate::destroy_block(level, &mut BlockTicks::new(), &actor, HEAD);
    }

    /// 创造模式在 `player_will_destroy` 里先拆底座，生存模式留到头被移除时的形状更新。
    /// 写方块的先后能看出走的是哪条路，最后底座都没了。
    #[test]
    fn creative_players_break_the_base_first() {
        let mut creative = extended_piston();
        break_head(&mut creative, true);
        assert_eq!(creative.writes[0].0, BASE);
        assert_eq!(creative.get(BASE), BlockState::AIR);

        let mut survival = extended_piston();
        break_head(&mut survival, false);
        assert_eq!(survival.writes[0].0, HEAD);
        assert_eq!(survival.get(BASE), BlockState::AIR);
    }
}
