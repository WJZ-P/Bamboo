//! 潜影盒、雕刻南瓜和凋灵骷髅头。
//!
//! 别的头颅官方不放方块，走可装备分支。南瓜和凋灵头放不下去时只试穿装备，不抛。
//! 傀儡和凋灵实体不生成。南瓜放下后 `onPlace` 会清掉图案。凋灵在 `checkSpawn` 里清，
//! 这里在完整图案对上之后照样清方块，不把凋灵放进世界。
//!
//! Shulker boxes, carved pumpkins and wither skeleton skulls.
//!
//! Other skulls are not placed. Official sends them through the equippable branch.
//! A pumpkin or wither skull that cannot be placed only tries equipment. It does not throw.
//! Golem and wither entities are not spawned. Placing the pumpkin runs `onPlace`, which
//! clears the pattern. The wither clear lives in `checkSpawn`. After the full pattern matches,
//! the blocks are cleared here without adding the wither to the world.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{blocks, items, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{Shot, equipment};
use crate::{
    behaviour::{common::item_of, machines::can_spawn_golem},
    dispenser::without_one,
    level::Effect,
    placement::place_from_dispenser,
    state::{BlockState, ROTATION_16},
    world::{UPDATE_CLIENTS, World},
};

pub(super) fn handles(item: i32) -> bool {
    item == items::CARVED_PUMPKIN || item == items::WITHER_SKELETON_SKULL || is_shulker(item)
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    if is_shulker(stack.item) {
        return shulker(world, pos, facing, stack);
    }
    if stack.item == items::CARVED_PUMPKIN {
        return pumpkin(world, pos, facing, stack);
    }
    wither_skull(world, pos, facing, stack)
}

fn is_shulker(item: i32) -> bool {
    item_of(item)
        .and_then(|item| item.block)
        .is_some_and(|block| {
            let state = BlockState(crate::behaviour::common::block_by_id(block).default_state);
            state.is_a(mc_bamboo_data::BlockAncestor::ShulkerBoxBlock)
        })
}

/// `ShulkerBoxDispenseBehavior`。下面一格是空的时，点的面是发射器的朝向，否则是上。
///
/// `ShulkerBoxDispenseBehavior`. The clicked face is the dispenser's facing when the block
/// below the target is empty, and up otherwise.
fn shulker(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let clicked = if world.is_empty_block(target.below()) {
        facing
    } else {
        Direction::Up
    };
    if place_from_dispenser(world, stack.item, target, clicked) {
        Shot::optional(without_one(&stack), true)
    } else {
        Shot::optional(stack, false)
    }
}

/// 前方是空的，并且 `canSpawnGolem` 为真，才放默认状态的南瓜（朝北）。
///
/// Place the default pumpkin, facing north, only when the front cell is empty and
/// `canSpawnGolem` is true.
fn pumpkin(world: &mut World, pos: BlockPos, facing: Direction, mut stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    if world.is_empty_block(target) && can_spawn_golem(world, target) {
        world.set_block_and_update(target, BlockState(blocks::CARVED_PUMPKIN));
        Shot::optional(without_one(&stack), true)
    } else {
        let equipped = equipment::equip(world, pos, facing, &mut stack);
        Shot::optional(stack, equipped)
    }
}

/// 前方是空的，并且 `canSpawnMob` 为真，才放带旋转的凋灵头。
///
/// Place a rotated wither skull only when the front cell is empty and `canSpawnMob` is true.
fn wither_skull(world: &mut World, pos: BlockPos, facing: Direction, mut stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    if world.is_empty_block(target) && can_spawn_wither(world, target) {
        let skull =
            BlockState(blocks::WITHER_SKELETON_SKULL).with(ROTATION_16, skull_rotation(facing));
        world.set_block_and_update(target, skull);
        // `checkSpawn`：实体 `create` 成功才清图案。凋灵不生成，方块照清。
        //
        // `checkSpawn` clears the pattern only after `create` succeeds. The wither is not
        // spawned. The blocks are still cleared.
        clear_wither(world, target);
        Shot::optional(without_one(&stack), true)
    } else {
        let equipped = equipment::equip(world, pos, facing, &mut stack);
        Shot::optional(stack, equipped)
    }
}

/// `RotationSegment.convertToSegment(Direction)`。竖直是 0，水平是 `get2DDataValue << 2`。
///
/// `RotationSegment.convertToSegment(Direction)`. Vertical is 0. Horizontal is
/// `get2DDataValue << 2`.
fn skull_rotation(facing: Direction) -> i32 {
    let data2d = match facing {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
        Direction::Down | Direction::Up => return 0,
    };
    data2d << 2
}

/// `WitherSkullBlock.canSpawnMob`。难度不是和平，高度至少比世界底部高两格。
///
/// `WitherSkullBlock.canSpawnMob`. Difficulty is not peaceful, and the cell is at least two
/// above the world's minimum Y.
fn can_spawn_wither(world: &World, pos: BlockPos) -> bool {
    // `Difficulty.PEACEFUL` 是 0。`World::difficulty_id` 固定是普通。
    //
    // `Difficulty.PEACEFUL` is 0. `World::difficulty_id` is fixed at normal.
    pos.y >= world.min_y() + 2
        && world.difficulty_id() != 0
        && find(world, WITHER_BASE, pos).is_some()
}

fn clear_wither(world: &mut World, pos: BlockPos) {
    let Some(cells) = find(world, WITHER_FULL, pos) else {
        return;
    };
    for &(cell, state) in &cells {
        world.set_block(cell, BlockState::AIR, UPDATE_CLIENTS);
        world.push_effect(Effect::Destroyed {
            pos: cell,
            state,
            except_initiator: false,
        });
    }
    for &(cell, _) in &cells {
        world.update_neighbors_at(cell, BlockState::AIR.block());
    }
}

/// 图案里的一格。`' '` 匹配任何方块。
#[derive(Clone, Copy)]
enum Cell {
    Any,
    Air,
    Base,
    Skull,
}

impl Cell {
    fn matches(self, state: BlockState) -> bool {
        match self {
            Self::Any => true,
            Self::Air => state.is_air(),
            Self::Base => state.is_tagged(block_tags::WITHER_SUMMON_BASE_BLOCKS),
            Self::Skull => {
                state.is(blocks::WITHER_SKELETON_SKULL)
                    || state.is(blocks::WITHER_SKELETON_WALL_SKULL)
            }
        }
    }
}

/// `getOrCreateWitherBase`：`"   ", "###", "~#~"`。
const WITHER_BASE: &[&[Cell]] = &[
    &[Cell::Any, Cell::Any, Cell::Any],
    &[Cell::Base, Cell::Base, Cell::Base],
    &[Cell::Air, Cell::Base, Cell::Air],
];

/// `getOrCreateWitherFull`：`"^^^", "###", "~#~"`。
const WITHER_FULL: &[&[Cell]] = &[
    &[Cell::Skull, Cell::Skull, Cell::Skull],
    &[Cell::Base, Cell::Base, Cell::Base],
    &[Cell::Air, Cell::Base, Cell::Air],
];

type PatternMatch = Vec<(BlockPos, BlockState)>;

fn find(world: &World, pattern: &[&[Cell]], origin: BlockPos) -> Option<PatternMatch> {
    let reach = pattern.len().max(pattern[0].len()) as i32;
    for z in 0..reach {
        for y in 0..reach {
            for x in 0..reach {
                if let Some(found) = find_at(world, pattern, origin.offset(x, y, z)) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_at(world: &World, pattern: &[&[Cell]], origin: BlockPos) -> Option<PatternMatch> {
    for forwards in Direction::ALL {
        for up in Direction::ALL {
            if up.axis() == forwards.axis() {
                continue;
            }
            if let Some(found) = match_at(world, pattern, origin, forwards, up) {
                return Some(found);
            }
        }
    }
    None
}

fn match_at(
    world: &World,
    pattern: &[&[Cell]],
    origin: BlockPos,
    forwards: Direction,
    up: Direction,
) -> Option<PatternMatch> {
    let mut cells = Vec::new();
    for right in 0..pattern[0].len() {
        for (down, row) in pattern.iter().enumerate() {
            let pos = translate(origin, forwards, up, right as i32, down as i32);
            let state = world.block_state(pos);
            if !row[right].matches(state) {
                return None;
            }
            cells.push((pos, state));
        }
    }
    Some(cells)
}

/// `BlockPattern.translateAndRotate`。图案只有一层，`forwards` 不参与位移。
fn translate(
    origin: BlockPos,
    forwards: Direction,
    up: Direction,
    right: i32,
    down: i32,
) -> BlockPos {
    let (fx, fy, fz) = forwards.normal();
    let (ux, uy, uz) = up.normal();
    let (rx, ry, rz) = (fy * uz - fz * uy, fz * ux - fx * uz, fx * uy - fy * ux);
    origin.offset(
        ux * -down + rx * right,
        uy * -down + ry * right,
        uz * -down + rz * right,
    )
}
