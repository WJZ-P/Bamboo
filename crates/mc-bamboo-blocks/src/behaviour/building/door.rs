//! `DoorBlock`、`WeatheringCopperDoorBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor};

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_bool, field_sound, prevent_drop_from_bottom_part},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, DOOR_HINGE, DOUBLE_BLOCK_HALF, DoorHingeSide, DoubleBlockHalf,
        HORIZONTAL_FACING, OPEN, POWERED, SupportType,
    },
    world::{UPDATE_CLIENTS, UPDATE_IMMEDIATE, World},
};

/// `DoorBlock`。
pub(super) struct Door;

/// `level.hasNeighborSignal` 看的两格：门的上下两半。
fn has_signal(world: &World, pos: BlockPos, other_half: BlockPos) -> bool {
    world.has_neighbor_signal(pos) || world.has_neighbor_signal(other_half)
}

/// `pos` 和上面一格里，有几格是完整的碰撞方块。
fn full_blocks_at(world: &World, pos: BlockPos) -> i32 {
    let is_full = |pos| i32::from(world.block_state(pos).is_collision_shape_full_block());
    is_full(pos) + is_full(pos.above())
}

/// 这一格是门的下半。
fn is_lower_door(state: BlockState) -> bool {
    state.is_a(BlockAncestor::DoorBlock) && state.get(DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower
}

/// `DoorBlock.playSound`：开门、关门的声音，音高是 `getRandom().nextFloat() * 0.1F + 0.9F`。
fn play_sound(world: &mut World, pos: BlockPos, block: &Block, open: bool, except: bool) {
    let name = if open {
        "type.doorOpen"
    } else {
        "type.doorClose"
    };
    let sound = field_sound(block, name);
    let pitch = world.random().next_float() * 0.1 + 0.9;
    if except {
        world.play_block_sound_except(pos, sound, 1.0, pitch);
    } else {
        world.play_block_sound(pos, sound, 1.0, pitch);
    }
}

/// `getHinge`：先贴着另一扇门，再贴着实心方块，都一样时看点在门的哪一边。
fn hinge(ctx: &PlaceContext, world: &World) -> DoorHingeSide {
    let pos = ctx.clicked_pos();
    let facing = ctx.horizontal_direction();
    let left = pos.relative(facing.counter_clockwise());
    let right = pos.relative(facing.clockwise());
    let solid_balance = full_blocks_at(world, right) - full_blocks_at(world, left);
    let door_left = is_lower_door(world.block_state(left));
    let door_right = is_lower_door(world.block_state(right));
    if door_left && !door_right || solid_balance > 0 {
        DoorHingeSide::Right
    } else if door_right && !door_left || solid_balance < 0 {
        DoorHingeSide::Left
    } else {
        hinge_from_click(ctx)
    }
}

/// `getHinge` 的最后一步：门轴在点击点的另一边。
fn hinge_from_click(ctx: &PlaceContext) -> DoorHingeSide {
    let pos = ctx.clicked_pos();
    let (step_x, _, step_z) = ctx.horizontal_direction().normal();
    let click = ctx.click_location();
    let click_x = click.x - f64::from(pos.x);
    let click_z = click.z - f64::from(pos.z);
    let right = step_x < 0 && click_z < 0.5
        || step_x > 0 && click_z > 0.5
        || step_z < 0 && click_x > 0.5
        || step_z > 0 && click_x < 0.5;
    if right {
        DoorHingeSide::Right
    } else {
        DoorHingeSide::Left
    }
}

impl Behaviour for Door {
    /// `WeatheringCopperDoorBlock` 的下半在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower {
            change_over_time(state, world, pos);
        }
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        if pos.y >= world.max_y() || !world.block_state(pos.above()).can_be_replaced(ctx, world) {
            return None;
        }
        let powered = has_signal(world, pos, pos.above());
        let state = default_state(block)
            .with(HORIZONTAL_FACING, ctx.horizontal_direction())
            .with(DOOR_HINGE, hinge(ctx, world))
            .with(POWERED, powered)
            .with(OPEN, powered)
            .with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Lower);
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        if state.get(DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower {
            world.is_face_sturdy(below, Direction::Up, SupportType::Full)
        } else {
            world.block_state(below).is_same_block(state)
        }
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
        let half = state.get(DOUBLE_BLOCK_HALF);
        let lower = half == DoubleBlockHalf::Lower;
        let toward_other_half =
            direction.axis() == Axis::Y && lower == (direction == Direction::Up);
        if toward_other_half {
            // 这一半的状态全部跟着另一半走，这样开关门时只要改一半。
            let other_half =
                neighbor.is_a(BlockAncestor::DoorBlock) && neighbor.get(DOUBLE_BLOCK_HALF) != half;
            return if other_half {
                neighbor.with(DOUBLE_BLOCK_HALF, half)
            } else {
                BlockState::AIR
            };
        }
        if lower && direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        let lower = state.get(DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower;
        let other_half = if lower { pos.above() } else { pos.below() };
        let signal = has_signal(world, pos, other_half);
        if !state.is(source.default_state) && signal != state.get(POWERED) {
            if signal != state.get(OPEN) {
                play_sound(world, pos, state.block(), signal, false);
            }
            let updated = state.with(POWERED, signal).with(OPEN, signal);
            world.set_block(pos, updated, UPDATE_CLIENTS);
        }
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let upper = state.with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper);
        world.set_block_and_update(pos.above(), upper);
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        prevent_drop_from_bottom_part(world, pos, state);
        Plain.player_will_destroy(state, world, pos, player)
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
        let toggled = state.cycle(OPEN);
        world.set_block(
            ctx.clicked_pos(),
            toggled,
            UPDATE_CLIENTS | UPDATE_IMMEDIATE,
        );
        // `playSound` 在放下新状态之后取音高，后面的随机刻要接在这里。
        play_sound(
            world,
            ctx.clicked_pos(),
            state.block(),
            toggled.get(OPEN),
            true,
        );
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        Effect,
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, break_block, use_item},
    };

    const LOWER: BlockPos = FLOOR.above();
    const UPPER: BlockPos = LOWER.above();

    #[test]
    fn placing_a_door_builds_both_halves() {
        let mut level = level_with_floor();
        let outcome = use_item(&mut level, &actor_holding(items::OAK_DOOR), top_of(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        let lower = level.get(LOWER);
        let upper = level.get(UPPER);
        assert!(lower.is(blocks::OAK_DOOR));
        assert_eq!(lower.get(DOUBLE_BLOCK_HALF), DoubleBlockHalf::Lower);
        assert_eq!(upper, lower.with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper));
    }

    #[test]
    fn a_door_needs_a_free_block_above_it() {
        let mut level = level_with_floor();
        level.set(UPPER, BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::OAK_DOOR), top_of(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(LOWER).is_air());
    }

    #[test]
    fn opening_one_half_opens_the_other() {
        let mut level = level_with_floor();
        use_item(&mut level, &actor_holding(items::OAK_DOOR), top_of(FLOOR));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_of(UPPER));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(LOWER).get(OPEN));
        assert!(level.get(UPPER).get(OPEN));
    }

    #[test]
    fn iron_doors_ignore_the_hand() {
        let mut level = level_with_floor();
        use_item(&mut level, &actor_holding(items::IRON_DOOR), top_of(FLOOR));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_of(LOWER));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(!level.get(LOWER).get(OPEN));
    }

    /// 官方的顺序：拿走下半时上半先被形状更新破坏，然后才是发给别的玩家的两个事件。
    #[test]
    fn breaking_the_upper_half_takes_the_lower_half_first() {
        let mut level = level_with_floor();
        use_item(&mut level, &actor_holding(items::OAK_DOOR), top_of(FLOOR));
        let lower = level.get(LOWER);
        let upper = level.get(UPPER);
        let outcome = break_block(&mut level, UPPER);
        assert!(level.get(LOWER).is_air());
        assert!(level.get(UPPER).is_air());
        assert_eq!(
            outcome.effects,
            [
                Effect::Destroyed {
                    pos: UPPER,
                    state: upper,
                    except_initiator: false,
                },
                Effect::Destroyed {
                    pos: LOWER,
                    state: lower,
                    except_initiator: true,
                },
                Effect::Destroyed {
                    pos: UPPER,
                    state: upper,
                    except_initiator: true,
                },
            ]
        );
    }

    #[test]
    fn losing_the_floor_breaks_the_whole_door() {
        let mut level = level_with_floor();
        use_item(&mut level, &actor_holding(items::OAK_DOOR), top_of(FLOOR));
        break_block(&mut level, FLOOR);
        assert!(level.get(LOWER).is_air());
        assert!(level.get(UPPER).is_air());
    }

    #[test]
    fn a_door_hinges_away_from_the_door_beside_it() {
        let mut level = level_with_floor();
        let actor = actor_holding(items::OAK_DOOR);
        level.set(FLOOR.east(), BlockState(blocks::STONE));
        use_item(&mut level, &actor, top_of(FLOOR));
        use_item(&mut level, &actor, top_of(FLOOR.east()));
        // 玩家朝南，西边在他的右手边，第一扇门就在这一边。
        assert_eq!(level.get(LOWER.east()).get(DOOR_HINGE), DoorHingeSide::Left);
    }
}
