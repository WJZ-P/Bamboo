//! `GrowingPlantBlock` 系列：海带、垂泪藤、缠怨藤、洞穴藤蔓的头和身体。
//!
//! 官方的头和身体各有一个父类，四种植物只在构造参数和几个小方法上不同。构造参数从生成的表里读，
//! 小方法在 [`state_for_placement`]、[`can_attach_to`]、[`keep_berries`]、[`pick_berries`] 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass, FieldValue, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block, field_bool, is_full_water},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BERRIES, BlockState, SupportType},
    world::{Fluid, UPDATE_CLIENTS, World},
};

/// 字段 `growthDirection`。
fn growth_direction(block: &Block) -> Direction {
    match block.field("growthDirection") {
        Some(FieldValue::Direction(id)) => Direction::from_id(id),
        other => panic!("field growthDirection of {} is {other:?}", block.name),
    }
}

/// `getHeadBlock` 的默认状态。
fn head_of(block: &Block) -> BlockState {
    default_state(field_block(block, "getHeadBlock()"))
}

/// `getBodyBlock` 的默认状态。
fn body_of(block: &Block) -> BlockState {
    default_state(field_block(block, "getBodyBlock()"))
}

/// `state` 是这种植物的头或者身体。
fn is_part_of(block: &Block, state: BlockState) -> bool {
    state.is_same_block(head_of(block)) || state.is_same_block(body_of(block))
}

/// `canAttachTo`：只有海带挑支撑方块。
fn can_attach_to(plant: BlockState, support: BlockState) -> bool {
    let kelp = matches!(
        plant.class(),
        BlockClass::KelpBlock | BlockClass::KelpPlantBlock
    );
    !kelp || !support.is_tagged(block_tags::CANNOT_SUPPORT_KELP)
}

/// `updateBodyAfterConvertedFromHead`、`updateHeadAfterConvertedFromBody`：
/// 只有洞穴藤蔓有 `berries`，头和身体互换时把它带过去。
fn keep_berries(from: BlockState, to: BlockState) -> BlockState {
    from.try_get(BERRIES)
        .map_or(to, |berries| to.with(BERRIES, berries))
}

/// `getStateForPlacement`：生长方向上已经有这种植物时，新的一格是身体。
///
/// 官方给新的头一个随机的 `age`，这里不生长，留默认值。
fn state_for_placement(
    block: &'static Block,
    ctx: &PlaceContext,
    world: &World,
) -> Option<BlockState> {
    let pos = ctx.clicked_pos();
    // `KelpBlock.getStateForPlacement` 多的条件。
    if block.class == BlockClass::KelpBlock && !is_full_water(world.fluid_state(pos)) {
        return None;
    }
    let ahead = world.block_state(pos.relative(growth_direction(block)));
    Some(if is_part_of(block, ahead) {
        body_of(block)
    } else {
        default_state(block)
    })
}

/// `canSurvive`。
fn can_survive(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let block = state.block();
    let growth = growth_direction(block);
    let support_pos = pos.relative(growth.opposite());
    let support = world.block_state(support_pos);
    can_attach_to(state, support)
        && (is_part_of(block, support)
            || world.is_face_sturdy(support_pos, growth, SupportType::Full))
}

/// 字段 `scheduleFluidTicks` 为真的植物泡在水里。
fn tick_water_if_submerged(block: &Block, world: &mut World, pos: BlockPos) {
    if field_bool(block, "scheduleFluidTicks") {
        world.schedule_fluid_tick(pos, Fluid::Water);
    }
}

/// `CaveVines.use`：摘下发光浆果。别的植物没有 `berries`，和官方的默认实现一样什么也不做。
fn pick_berries(state: BlockState, world: &mut World, pos: BlockPos) -> InteractionResult {
    if state.try_get(BERRIES) != Some(true) {
        return InteractionResult::Pass;
    }
    world.set_block(pos, state.with(BERRIES, false), UPDATE_CLIENTS);
    InteractionResult::Success
}

/// `GrowingPlantHeadBlock`：植物最前面的一格。
pub(super) struct GrowingPlantHead;

impl Behaviour for GrowingPlantHead {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        state_for_placement(block, ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_survive(state, world, pos)
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
        let block = state.block();
        let growth = growth_direction(block);
        if direction == growth.opposite() {
            if !state.can_survive(world, pos) {
                world.schedule_tick(pos, block, 1);
            } else if is_part_of(block, world.block_state(pos.relative(growth))) {
                return keep_berries(state, body_of(block));
            }
        }
        if direction == growth && is_part_of(block, neighbor) {
            return keep_berries(state, body_of(block));
        }
        tick_water_if_submerged(block, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        pick_berries(state, world, ctx.clicked_pos())
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}

/// `GrowingPlantBodyBlock`：头后面的每一格。
pub(super) struct GrowingPlantBody;

impl Behaviour for GrowingPlantBody {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        state_for_placement(block, ctx, world)
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let head_item = head_of(state.block()).block().item;
        Plain.can_be_replaced(state, ctx, world) && ctx.item != head_item
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_survive(state, world, pos)
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
        let block = state.block();
        let growth = growth_direction(block);
        if direction == growth.opposite() {
            schedule_break_if_unsupported(state, world, pos);
        }
        if direction == growth && !is_part_of(block, neighbor) {
            // 官方给新的头一个随机的 `age`，这里不生长，留默认值。
            return keep_berries(state, head_of(block));
        }
        tick_water_if_submerged(block, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        pick_berries(state, world, ctx.clicked_pos())
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        state::AGE_25,
        testing::{TestLevel, actor_holding, break_block, top_hit, use_item},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);
    const ABOVE_FLOOR: BlockPos = FLOOR.above();

    /// 官方放下时的 `age` 是随机的，这些题 oracle 全部排除了，这里把固定值定下来。
    #[test]
    fn kelp_placed_in_water_starts_at_age_zero() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(ABOVE_FLOOR, BlockState::WATER);
        let outcome = use_item(&mut level, &actor_holding(items::KELP), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(ABOVE_FLOOR), BlockState(blocks::KELP));
        assert_eq!(level.get(ABOVE_FLOOR).get(AGE_25), 0);
    }

    #[test]
    fn a_body_that_loses_its_head_grows_a_head_at_age_zero() {
        let mut level = TestLevel::new();
        let head = ABOVE_FLOOR.above();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(ABOVE_FLOOR, BlockState(blocks::KELP_PLANT));
        level.set(head, BlockState(blocks::KELP).with(AGE_25, 7));
        break_block(&mut level, head);
        assert_eq!(level.get(ABOVE_FLOOR), BlockState(blocks::KELP));
    }

    #[test]
    fn cave_vines_keep_their_berries_when_the_body_becomes_a_head() {
        let mut level = TestLevel::new();
        let ceiling = BlockPos::new(0, 70, 0);
        let body = ceiling.below();
        let head = body.below();
        level.set(ceiling, BlockState(blocks::STONE));
        level.set(
            body,
            BlockState(blocks::CAVE_VINES_PLANT).with(BERRIES, true),
        );
        level.set(head, BlockState(blocks::CAVE_VINES));
        break_block(&mut level, head);
        assert_eq!(
            level.get(body),
            BlockState(blocks::CAVE_VINES).with(BERRIES, true)
        );
    }
}
