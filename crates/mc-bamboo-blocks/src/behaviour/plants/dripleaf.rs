//! `BigDripleafBlock`、`BigDripleafStemBlock`。`SmallDripleafBlock` 是高植物，
//! 在 [`double_plant`](super::double_plant) 里。
//!
//! 叶子只有实体和弹射物才能压倒，这里没有实体，`entityInside` 不用移植。
//! 倾倒之后的几步靠计划刻走，有信号就立刻复位，这两条和实体无关，都移植了。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, registries, sounds::sound_event, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, FluidState, HORIZONTAL_FACING, TILT, Tilt, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// 小型垂滴叶用骨粉长成的大型垂滴叶有 2 到 5 格高。
const MIN_GROWN_HEIGHT: i32 = 2;
const MAX_GROWN_HEIGHT: i32 = 5;

/// `NO_TICK`：这个倾斜状态之后不再排计划刻。
const NO_TICK: i32 = -1;

/// `DELAY_UNTIL_NEXT_TILT_STATE`。
fn delay_until_next_tilt(tilt: Tilt) -> i32 {
    match tilt {
        Tilt::Unstable | Tilt::Partial => 10,
        Tilt::Full => 100,
        Tilt::None => NO_TICK,
    }
}

/// `setTilt`。
fn set_tilt(state: BlockState, world: &mut World, pos: BlockPos, tilt: Tilt) {
    world.set_block(pos, state.with(TILT, tilt), UPDATE_CLIENTS);
}

/// `playTiltSound`：音高由 `Mth.randomBetween` 从世界随机源取，取的这个数后面的随机刻和计划刻都要用。
fn play_tilt_sound(world: &mut World, pos: BlockPos, sound: &str) {
    // `Mth.randomBetween` 用的是 `max - min`，和手写的 0.4 差最后一位。
    let pitch = world.random().next_float() * (1.2 - 0.8) + 0.8;
    world.play_block_sound(pos, sound_event(sound), 1.0, pitch);
}

/// `setTiltAndScheduleTick`：倾倒和复位都会响一声，先响再排刻。
fn set_tilt_and_schedule_tick(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    tilt: Tilt,
    sound: &str,
) {
    set_tilt(state, world, pos, tilt);
    play_tilt_sound(world, pos, sound);
    let delay = delay_until_next_tilt(tilt);
    if delay != NO_TICK {
        world.schedule_tick(pos, state.block(), delay);
    }
}

/// `resetTilt`：原来是倾斜的才响，已经水平的不响。
fn reset_tilt(state: BlockState, world: &mut World, pos: BlockPos) {
    let previous = state.get(TILT);
    set_tilt(state, world, pos, Tilt::None);
    if previous != Tilt::None {
        play_tilt_sound(world, pos, "minecraft:block.big_dripleaf.tilt_up");
    }
}

/// `BigDripleafBlock`。
pub(super) struct BigDripleaf;

impl Behaviour for BigDripleaf {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let below = world.block_state(pos.below());
        let on_dripleaf = below.is(blocks::BIG_DRIPLEAF) || below.is(blocks::BIG_DRIPLEAF_STEM);
        let facing = if on_dripleaf {
            below.get(HORIZONTAL_FACING)
        } else {
            ctx.horizontal_direction().opposite()
        };
        let state = default_state(block)
            .with(WATERLOGGED, is_water_source(world, pos))
            .with(HORIZONTAL_FACING, facing);
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        below.is_same_block(state)
            || below.is(blocks::BIG_DRIPLEAF_STEM)
            || below.is_tagged(block_tags::SUPPORTS_BIG_DRIPLEAF)
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
        if direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        tick_water_if_waterlogged(state, world, pos);
        if direction == Direction::Up && neighbor.is_same_block(state) {
            return state.copied_onto(BlockState(blocks::BIG_DRIPLEAF_STEM).block());
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_grow_into(world, pos.above())
    }

    /// 叶子变成茎，上面长一片新叶子。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let above = pos.above();
        if can_grow_into(world, above) {
            let facing = state.get(HORIZONTAL_FACING);
            place_part(world, blocks::BIG_DRIPLEAF_STEM, pos, state.fluid(), facing);
            let above_fluid = world.fluid_state(above);
            place_part(world, blocks::BIG_DRIPLEAF, above, above_fluid, facing);
        }
    }

    /// 有信号就复位，没有就倒下去一格。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.has_neighbor_signal(pos) {
            reset_tilt(state, world, pos);
            return;
        }
        match state.get(TILT) {
            Tilt::Unstable => set_tilt_and_schedule_tick(
                state,
                world,
                pos,
                Tilt::Partial,
                "minecraft:block.big_dripleaf.tilt_down",
            ),
            Tilt::Partial => set_tilt_and_schedule_tick(
                state,
                world,
                pos,
                Tilt::Full,
                "minecraft:block.big_dripleaf.tilt_down",
            ),
            Tilt::Full => reset_tilt(state, world, pos),
            Tilt::None => {}
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        if world.has_neighbor_signal(pos) {
            reset_tilt(state, world, pos);
        }
    }
}

/// `BigDripleafBlock.canGrowInto`：世界里的空气、水或者小型垂滴叶。
fn can_grow_into(world: &World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    world.is_inside_build_height(pos.y)
        && (state.is_air() || state.is(blocks::WATER) || state.is(blocks::SMALL_DRIPLEAF))
}

/// `BigDripleafBlock.place` 和 `BigDripleafStemBlock.place`：`fluid` 是水源时含水。
fn place_part(world: &mut World, block: u32, pos: BlockPos, fluid: FluidState, facing: Direction) {
    let waterlogged = fluid.is(registries::fluid::WATER);
    let part = BlockState(block)
        .with(WATERLOGGED, waterlogged)
        .with(HORIZONTAL_FACING, facing);
    world.set_block_and_update(pos, part);
}

/// `BigDripleafBlock.placeWithRandomHeight`：从 `bottom` 往上长出一株 2 到 5 格高的大型垂滴叶，
/// 长不了这么高时能长多高长多高。
pub(super) fn place_with_random_height(world: &mut World, bottom: BlockPos, facing: Direction) {
    let desired = world
        .random()
        .mth_next_int(MIN_GROWN_HEIGHT, MAX_GROWN_HEIGHT);
    let mut height = 0;
    while height < desired && can_grow_into(world, bottom.relative_by(Direction::Up, height)) {
        height += 1;
    }
    let leaf_y = bottom.y + height - 1;
    let mut pos = bottom;
    while pos.y < leaf_y {
        let fluid = world.fluid_state(pos);
        place_part(world, blocks::BIG_DRIPLEAF_STEM, pos, fluid, facing);
        pos = pos.above();
    }
    let fluid = world.fluid_state(pos);
    place_part(world, blocks::BIG_DRIPLEAF, pos, fluid, facing);
}

/// `BlockUtil.getTopConnectedBlock`：沿着茎往上，停在叶子上。
fn leaf_above_stem(stem: BlockState, world: &World, pos: BlockPos) -> Option<BlockPos> {
    let mut forward = pos.above();
    while world.block_state(forward).is_same_block(stem) {
        forward = forward.above();
    }
    world
        .block_state(forward)
        .is(blocks::BIG_DRIPLEAF)
        .then_some(forward)
}

/// `BigDripleafStemBlock`。
pub(super) struct BigDripleafStem;

impl Behaviour for BigDripleafStem {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        let above = world.block_state(pos.above());
        (below.is_same_block(state) || below.is_tagged(block_tags::SUPPORTS_BIG_DRIPLEAF))
            && (above.is_same_block(state) || above.is(blocks::BIG_DRIPLEAF))
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
        if direction.axis().is_vertical() {
            schedule_break_if_unsupported(state, world, pos);
        }
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        leaf_above_stem(state, world, pos).is_some_and(|leaf| can_grow_into(world, leaf.above()))
    }

    /// 最上面的叶子变成茎，再往上长一片叶子，朝向用点中的这节茎的。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if let Some(leaf) = leaf_above_stem(state, world, pos) {
            let facing = state.get(HORIZONTAL_FACING);
            let leaf_fluid = world.fluid_state(leaf);
            place_part(world, blocks::BIG_DRIPLEAF_STEM, leaf, leaf_fluid, facing);
            let above = leaf.above();
            let above_fluid = world.fluid_state(above);
            place_part(world, blocks::BIG_DRIPLEAF, above, above_fluid, facing);
        }
    }
}
