//! `GrowingPlantBlock` 系列：海带、垂泪藤、缠怨藤、洞穴藤蔓的头和身体。
//!
//! 官方的头和身体各有一个父类，四种植物只在构造参数和几个小方法上不同。构造参数从生成的表里读，
//! 小方法在 [`state_for_placement`]、[`can_attach_to`]、[`can_grow_into`]、[`keep_berries`]、
//! [`pick_berries`] 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, BlockAncestor, BlockClass, FieldValue, blocks, registries::fluid, tags::block_tags,
};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block, field_bool, is_full_water},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{AGE_25, BERRIES, BlockState, FluidState, SupportType},
    world::{UPDATE_CLIENTS, World},
};

/// `GrowingPlantHeadBlock.MAX_AGE`：长到这里就不再自己长。
const MAX_AGE: i32 = 25;
/// 洞穴藤蔓新长的一格带浆果的概率。
const BERRY_CHANCE: f32 = 0.11;
/// `NetherVines.BONEMEAL_GROW_PROBABILITY_DECREASE_RATE`：骨粉每多长一格，再长一格的概率乘上它。
const NETHER_VINES_GROWTH_DECAY: f64 = 0.826;

/// 字段 `growPerTickProbability`：每次随机刻长一格的概率。
fn grow_per_tick_probability(block: &Block) -> f64 {
    match block.field("growPerTickProbability") {
        Some(FieldValue::Double(probability)) => probability,
        other => panic!("field growPerTickProbability of {} is {other:?}", block.name),
    }
}

/// `canGrowInto`：海带长进水里，别的长进空气里。
fn can_grow_into(plant: BlockState, target: BlockState) -> bool {
    if is_kelp(plant) {
        target.is(blocks::WATER)
    } else {
        target.is_air()
    }
}

fn is_kelp(plant: BlockState) -> bool {
    matches!(
        plant.class(),
        BlockClass::KelpBlock | BlockClass::KelpPlantBlock
    )
}

fn is_cave_vines(plant: BlockState) -> bool {
    plant.try_get(BERRIES).is_some()
}

/// `getStateForPlacement(random)`：新的头的 `age` 是随机的，身体没有 `age`。
fn new_plant(block: &'static Block, world: &World) -> BlockState {
    let state = default_state(block);
    if state.is_a(BlockAncestor::GrowingPlantHeadBlock) {
        state.with(AGE_25, world.random().next_int(MAX_AGE))
    } else {
        state
    }
}

/// `getGrowIntoState`：`age` 加一，洞穴藤蔓还随机带上浆果。
fn grow_into_state(head: BlockState, world: &World) -> BlockState {
    let grown = head.cycle(AGE_25);
    if is_cave_vines(head) {
        grown.with(BERRIES, world.random().next_float() < BERRY_CHANCE)
    } else {
        grown
    }
}

/// `getBlocksToGrowWhenBonemealed`：海带和洞穴藤蔓一格，下界的藤蔓每多一格概率乘 0.826。
fn blocks_to_grow(head: BlockState, world: &World) -> i32 {
    if !matches!(
        head.class(),
        BlockClass::TwistingVinesBlock | BlockClass::WeepingVinesBlock
    ) {
        return 1;
    }
    let mut probability = 1.0;
    let mut count = 0;
    while world.random().next_double() < probability {
        probability *= NETHER_VINES_GROWTH_DECAY;
        count += 1;
    }
    count
}

/// `BlockUtil.getTopConnectedBlock`：沿着生长方向走过身体，停在头上。走到别的方块时是 `None`。
fn head_pos(body: BlockState, world: &World, pos: BlockPos) -> Option<BlockPos> {
    let block = body.block();
    let growth = growth_direction(block);
    let mut forward = pos.relative(growth);
    while world.block_state(forward).is_same_block(body_of(block)) {
        forward = forward.relative(growth);
    }
    world
        .block_state(forward)
        .is_same_block(head_of(block))
        .then_some(forward)
}

/// 头的 `isValidBonemealTarget`：前面一格能长。
fn head_can_grow(head: BlockState, world: &World, pos: BlockPos) -> bool {
    let growth_pos = pos.relative(growth_direction(head.block()));
    can_grow_into(head, world.block_state(growth_pos)) && world.is_inside_build_height(growth_pos.y)
}

/// 头的 `performBonemeal`：往前长几格，每格的 `age` 比后一格大一。
fn grow_head(head: BlockState, world: &mut World, pos: BlockPos) {
    let growth = growth_direction(head.block());
    let mut forward = pos.relative(growth);
    let mut age = (head.get(AGE_25) + 1).min(MAX_AGE);
    let count = blocks_to_grow(head, world);
    for _ in 0..count {
        if !can_grow_into(head, world.block_state(forward))
            || !world.is_inside_build_height(forward.y)
        {
            break;
        }
        world.set_block_and_update(forward, head.with(AGE_25, age));
        forward = forward.relative(growth);
        age = (age + 1).min(MAX_AGE);
    }
}

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

/// `getStateForPlacement`：生长方向上已经有这种植物时，新的一格是身体，否则是随机 `age` 的头。
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
        new_plant(block, world)
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
        world.schedule_fluid_tick(pos, fluid::WATER);
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

    /// 没长满时按 `growPerTickProbability` 的概率往前长一格。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let block = state.block();
        if state.get(AGE_25) >= MAX_AGE
            || world.random().next_double() >= grow_per_tick_probability(block)
        {
            return;
        }
        let growth_pos = pos.relative(growth_direction(block));
        if can_grow_into(state, world.block_state(growth_pos)) {
            let grown = grow_into_state(state, world);
            world.set_block_and_update(growth_pos, grown);
        }
    }

    /// 洞穴藤蔓的骨粉只长浆果，见 `CaveVinesBlock`。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        if is_cave_vines(state) {
            !state.get(BERRIES)
        } else {
            head_can_grow(state, world, pos)
        }
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_cave_vines(state) {
            world.set_block(pos, state.with(BERRIES, true), UPDATE_CLIENTS);
        } else {
            grow_head(state, world, pos);
        }
    }

    /// 四种植物里只有 `KelpBlock` 是 `LiquidBlockContainer`，它本来就泡在水里。
    fn can_place_liquid(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _fluid: i32,
        _by_player: bool,
    ) -> bool {
        false
    }

    fn place_liquid(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _fluid: FluidState,
    ) -> bool {
        false
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
            let head = new_plant(field_block(block, "getHeadBlock()"), world);
            return keep_berries(state, head);
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

    /// 骨粉催的是最前面的头，见 `GrowingPlantBodyBlock`。洞穴藤蔓的身体只长浆果。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        if is_cave_vines(state) {
            return !state.get(BERRIES);
        }
        head_pos(state, world, pos).is_some_and(|head| {
            let head_state = world.block_state(head);
            head_can_grow(head_state, world, head)
        })
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_cave_vines(state) {
            world.set_block(pos, state.with(BERRIES, true), UPDATE_CLIENTS);
        } else if let Some(head) = head_pos(state, world, pos) {
            let head_state = world.block_state(head);
            head_state.perform_bonemeal(world, head);
        }
    }

    /// 四种植物里只有 `KelpPlantBlock` 是 `LiquidBlockContainer`，它本来就泡在水里。
    fn can_place_liquid(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _fluid: i32,
        _by_player: bool,
    ) -> bool {
        false
    }

    fn place_liquid(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _fluid: FluidState,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        LevelRandom,
        testing::{TestLevel, actor_holding, break_block, top_hit, use_item},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);
    const ABOVE_FLOOR: BlockPos = FLOOR.above();

    /// 放下时的 `age` 是世界随机源取的第一个数。
    #[test]
    fn kelp_placed_in_water_gets_a_random_age() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(ABOVE_FLOOR, BlockState::WATER);
        let expected = LevelRandom::new(0).source().next_int(MAX_AGE);
        let outcome = use_item(&mut level, &actor_holding(items::KELP), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(ABOVE_FLOOR).is(blocks::KELP));
        assert_eq!(level.get(ABOVE_FLOOR).get(AGE_25), expected);
    }

    #[test]
    fn kelp_grows_into_the_water_above() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(ABOVE_FLOOR, BlockState(blocks::KELP).with(AGE_25, 3));
        level.set(ABOVE_FLOOR.above(), BlockState::WATER);
        // 种子 0 的第一个 `nextDouble` 比 0.14 大，要先跳过几次。
        let mut grown = false;
        for _ in 0..50 {
            crate::random_tick(&mut level, &mut crate::BlockTicks::new(), ABOVE_FLOOR);
            if level.get(ABOVE_FLOOR.above()).is(blocks::KELP) {
                grown = true;
                break;
            }
        }
        assert!(grown);
        assert_eq!(level.get(ABOVE_FLOOR.above()).get(AGE_25), 4);
        assert!(level.get(ABOVE_FLOOR).is(blocks::KELP_PLANT));
    }

    #[test]
    fn a_body_that_loses_its_head_grows_a_new_head() {
        let mut level = TestLevel::new();
        let head = ABOVE_FLOOR.above();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(ABOVE_FLOOR, BlockState(blocks::KELP_PLANT));
        level.set(head, BlockState(blocks::KELP).with(AGE_25, 7));
        break_block(&mut level, head);
        assert!(level.get(ABOVE_FLOOR).is(blocks::KELP));
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
        assert!(level.get(body).is(blocks::CAVE_VINES));
        assert!(level.get(body).get(BERRIES));
    }
}
