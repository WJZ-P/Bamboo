//! `PistonStructureResolver`：活塞会推动哪些方块。
//!
//! 从活塞口沿着方向找一条线，最多 12 格；线上的粘液块和蜂蜜块把垂直方向的方块也粘进来。
//! 粘液块和蜂蜜块互相不粘，推不动的方块会让整个结构推不动，`Popped` 的方块直接掉下来。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{PushReaction, blocks};

use crate::{state::BlockState, world::World};

use super::piston::is_pushable;

/// `MAX_PUSH_DEPTH`。
pub(super) const MAX_PUSH_DEPTH: usize = 12;

/// 一次推动的结构。
pub(super) struct PistonStructureResolver {
    piston_pos: BlockPos,
    extending: bool,
    start_pos: BlockPos,
    push_direction: Direction,
    piston_direction: Direction,
    to_push: Vec<BlockPos>,
    to_destroy: Vec<BlockPos>,
}

impl PistonStructureResolver {
    /// `PistonStructureResolver(level, pistonPos, direction, extending)`。
    pub(super) fn new(piston_pos: BlockPos, direction: Direction, extending: bool) -> Self {
        let (push_direction, start_pos) = if extending {
            (direction, piston_pos.relative(direction))
        } else {
            (direction.opposite(), piston_pos.relative_by(direction, 2))
        };
        Self {
            piston_pos,
            extending,
            start_pos,
            push_direction,
            piston_direction: direction,
            to_push: Vec::new(),
            to_destroy: Vec::new(),
        }
    }

    /// `resolve`：能不能推。
    pub(super) fn resolve(&mut self, world: &World) -> bool {
        self.to_push.clear();
        self.to_destroy.clear();
        let next_state = world.block_state(self.start_pos);
        if !is_pushable(
            next_state,
            world,
            self.start_pos,
            self.push_direction,
            false,
            self.piston_direction,
        ) {
            if self.extending && next_state.push_reaction() == PushReaction::Popped {
                self.to_destroy.push(self.start_pos);
                return true;
            }
            return false;
        }
        if !self.add_block_line(world, self.start_pos, self.push_direction) {
            return false;
        }
        let mut i = 0;
        while i < self.to_push.len() {
            let pos = self.to_push[i];
            if is_sticky(world.block_state(pos)) && !self.add_branching_blocks(world, pos) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// `getPushDirection`。
    pub(super) const fn push_direction(&self) -> Direction {
        self.push_direction
    }

    /// `getToPush`。
    pub(super) fn to_push(&self) -> &[BlockPos] {
        &self.to_push
    }

    /// `getToDestroy`。
    pub(super) fn to_destroy(&self) -> &[BlockPos] {
        &self.to_destroy
    }

    /// `addBlockLine`：从 `start` 往里收一条线，粘液块往回多收几格。
    fn add_block_line(&mut self, world: &World, start: BlockPos, direction: Direction) -> bool {
        let mut next_state = world.block_state(start);
        if next_state.is_air() {
            return true;
        }
        if !is_pushable(
            next_state,
            world,
            start,
            self.push_direction,
            false,
            direction,
        ) {
            return true;
        }
        if start == self.piston_pos || self.to_push.contains(&start) {
            return true;
        }
        let mut block_count = 1;
        if block_count + self.to_push.len() > MAX_PUSH_DEPTH {
            return false;
        }
        // 粘液块会粘住后面同样的方块，一起往后收。
        while is_sticky(next_state) {
            let pos = start.relative_by(self.push_direction.opposite(), block_count as i32);
            let previous_state = next_state;
            next_state = world.block_state(pos);
            if next_state.is_air()
                || !can_stick_to_each_other(previous_state, next_state)
                || !is_pushable(
                    next_state,
                    world,
                    pos,
                    self.push_direction,
                    false,
                    self.push_direction.opposite(),
                )
                || pos == self.piston_pos
            {
                break;
            }
            block_count += 1;
            if block_count + self.to_push.len() > MAX_PUSH_DEPTH {
                return false;
            }
        }

        let mut blocks_added = 0;
        for i in (0..block_count).rev() {
            self.to_push
                .push(start.relative_by(self.push_direction.opposite(), i as i32));
            blocks_added += 1;
        }

        let mut i = 1;
        loop {
            let pos = start.relative_by(self.push_direction, i);
            if let Some(collision_pos) = self.to_push.iter().position(|&pushed| pushed == pos) {
                self.reorder_list_at_collision(blocks_added, collision_pos);
                for j in 0..=collision_pos + blocks_added {
                    let block_pos = self.to_push[j];
                    if is_sticky(world.block_state(block_pos))
                        && !self.add_branching_blocks(world, block_pos)
                    {
                        return false;
                    }
                }
                return true;
            }

            next_state = world.block_state(pos);
            if next_state.is_air() {
                return true;
            }
            if !is_pushable(
                next_state,
                world,
                pos,
                self.push_direction,
                true,
                self.push_direction,
            ) || pos == self.piston_pos
            {
                return false;
            }
            if next_state.push_reaction() == PushReaction::Popped {
                self.to_destroy.push(pos);
                return true;
            }
            if self.to_push.len() >= MAX_PUSH_DEPTH {
                return false;
            }
            self.to_push.push(pos);
            blocks_added += 1;
            i += 1;
        }
    }

    /// `reorderListAtCollision`：拐弯的线和原来的线交错时，按官方的顺序重新排。
    fn reorder_list_at_collision(&mut self, blocks_added: usize, collision_pos: usize) {
        let mut head = self.to_push[..collision_pos].to_vec();
        let mut last_line_added = self.to_push[self.to_push.len() - blocks_added..].to_vec();
        let mut collision_to_line =
            self.to_push[collision_pos..self.to_push.len() - blocks_added].to_vec();
        self.to_push.clear();
        self.to_push.append(&mut head);
        self.to_push.append(&mut last_line_added);
        self.to_push.append(&mut collision_to_line);
    }

    /// `addBranchingBlocks`：和这一格粘着的垂直方向方块也排进来。
    fn add_branching_blocks(&mut self, world: &World, from_pos: BlockPos) -> bool {
        let from_state = world.block_state(from_pos);
        for direction in Direction::ALL {
            if direction.axis() != self.push_direction.axis() {
                let neighbour_pos = from_pos.relative(direction);
                let neighbour_state = world.block_state(neighbour_pos);
                if can_stick_to_each_other(neighbour_state, from_state)
                    && !self.add_block_line(world, neighbour_pos, direction)
                {
                    return false;
                }
            }
        }
        true
    }
}

/// `isSticky`。
fn is_sticky(state: BlockState) -> bool {
    state.is(blocks::SLIME_BLOCK) || state.is(blocks::HONEY_BLOCK)
}

/// `canStickToEachOther`：粘液块和蜂蜜块不互相粘。
fn can_stick_to_each_other(first: BlockState, second: BlockState) -> bool {
    if first.is(blocks::HONEY_BLOCK) && second.is(blocks::SLIME_BLOCK) {
        return false;
    }
    if first.is(blocks::SLIME_BLOCK) && second.is(blocks::HONEY_BLOCK) {
        return false;
    }
    is_sticky(first) || is_sticky(second)
}
