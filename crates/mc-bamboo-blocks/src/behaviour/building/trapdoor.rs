//! `TrapDoorBlock`、`WeatheringCopperTrapDoorBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::clicked_upper_half;
use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, field_bool, field_sound, is_water_source, tick_water_if_waterlogged,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HALF, HORIZONTAL_FACING, Half, OPEN, POWERED, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `TrapDoorBlock`。
pub(super) struct TrapDoor;

/// `TrapDoorBlock.playSound`：开、关活板门的声音，音高是 `getRandom().nextFloat() * 0.1F + 0.9F`。
fn play_sound(world: &mut World, pos: BlockPos, block: &Block, opening: bool, except: bool) {
    let name = if opening {
        "type.trapdoorOpen"
    } else {
        "type.trapdoorClose"
    };
    let sound = field_sound(block, name);
    let pitch = world.random().next_float() * 0.1 + 0.9;
    if except {
        world.play_block_sound_except(pos, sound, 1.0, pitch);
    } else {
        world.play_block_sound(pos, sound, 1.0, pitch);
    }
}

/// 点在侧面时贴着点中的方块，上下看点击点；别的情况朝着玩家，上下看点的是顶面还是底面。
fn attachment(ctx: &PlaceContext) -> (Direction, Half) {
    let face = ctx.clicked_face();
    if !ctx.replacing_clicked_on_block() && face.axis().is_horizontal() {
        let half = if clicked_upper_half(ctx) {
            Half::Top
        } else {
            Half::Bottom
        };
        return (face, half);
    }
    let half = if face == Direction::Up {
        Half::Bottom
    } else {
        Half::Top
    };
    (ctx.horizontal_direction().opposite(), half)
}

impl Behaviour for TrapDoor {
    /// `WeatheringCopperTrapDoorBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let (facing, half) = attachment(ctx);
        let mut state = default_state(block)
            .with(HORIZONTAL_FACING, facing)
            .with(HALF, half);
        if world.has_neighbor_signal(pos) {
            state = state.with(OPEN, true).with(POWERED, true);
        }
        Some(state.with(WATERLOGGED, is_water_source(world, pos)))
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
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let signal = world.has_neighbor_signal(pos);
        if signal != state.get(POWERED) {
            if state.get(OPEN) != signal {
                play_sound(world, pos, state.block(), signal, false);
            }
            let updated = state.with(OPEN, signal).with(POWERED, signal);
            world.set_block(pos, updated, UPDATE_CLIENTS);
            tick_water_if_waterlogged(state, world, pos);
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !field_bool(state.block(), "type.canOpenByHand") {
            return InteractionResult::Pass;
        }
        let pos = ctx.clicked_pos();
        let toggled = state.cycle(OPEN);
        world.set_block(pos, toggled, UPDATE_CLIENTS);
        tick_water_if_waterlogged(toggled, world, pos);
        // `playSound` 在计划流体刻之后取音高。
        play_sound(world, pos, state.block(), toggled.get(OPEN), true);
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, side_of, top_of},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn a_trapdoor_on_a_wall_hangs_from_the_clicked_face() {
        let mut level = level_with_floor();
        let click = side_of(FLOOR, Direction::East, 0.75);
        use_item(&mut level, &actor_holding(items::OAK_TRAPDOOR), click);
        let placed = level.get(FLOOR.east());
        assert_eq!(placed.get(HORIZONTAL_FACING), Direction::East);
        assert_eq!(placed.get(HALF), Half::Top);
    }

    #[test]
    fn a_trapdoor_on_the_floor_faces_the_player() {
        let mut level = level_with_floor();
        let actor = actor_holding(items::OAK_TRAPDOOR);
        use_item(&mut level, &actor, top_of(FLOOR));
        let placed = level.get(FLOOR.above());
        assert_eq!(placed.get(HORIZONTAL_FACING), Direction::North);
        assert_eq!(placed.get(HALF), Half::Bottom);
    }

    #[test]
    fn the_hand_opens_wooden_trapdoors_only() {
        let hand = actor_holding(items::AIR);
        let trapdoor = FLOOR.above();

        let mut level = level_with_floor();
        level.set(trapdoor, BlockState(blocks::OAK_TRAPDOOR));
        let outcome = use_item(&mut level, &hand, top_of(trapdoor));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(trapdoor).get(OPEN));

        let mut level = level_with_floor();
        level.set(trapdoor, BlockState(blocks::IRON_TRAPDOOR));
        let outcome = use_item(&mut level, &hand, top_of(trapdoor));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(!level.get(trapdoor).get(OPEN));
    }
}
