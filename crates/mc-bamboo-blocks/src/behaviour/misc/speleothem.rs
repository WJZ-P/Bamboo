//! `SpeleothemBlock` 和子类 `PointedDripstoneBlock`、`SulfurSpikeBlock`，还有炼药锅接水要用的
//! 查找函数。往下长和往炼药锅里滴水都在随机刻里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, FieldValue, blocks, registries::fluid, tags::block_tags};

use super::{cauldron::can_receive_drip, fall};
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, field_int, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    fluid::Fluid,
    shape::{self, Aabb, BooleanOp, CollisionContext},
    state::{
        BlockState, SPELEOTHEM_THICKNESS, SpeleothemThickness, SupportType, VERTICAL_DIRECTION,
        WATERLOGGED,
    },
    world::{UPDATE_LIMIT, World},
};

/// `SpeleothemBlock`。
pub(super) struct Speleothem;

/// 钟乳石失去支撑后等多久才掉下来，`DELAY_BEFORE_FALLING`。
const DELAY_BEFORE_FALLING: i32 = 2;
/// `MAX_SEARCH_LENGTH_BETWEEN_STALACTITE_TIP_AND_CAULDRON`，也是 `findRootBlock` 的上限。
const MAX_SEARCH_LENGTH: i32 = 11;
/// `GROWTH_PROBABILITY_PER_RANDOM_TICK`。
const GROWTH_CHANCE: f32 = 0.011_377_778;
/// `MAX_STALAGMITE_SEARCH_RANGE_WHEN_GROWING`。
const STALAGMITE_SEARCH_RANGE: i32 = 10;
/// `WATER_TRANSFER_PROBABILITY_PER_RANDOM_TICK`。
const WATER_TRANSFER_CHANCE: f32 = 0.175_781_25;
/// `LAVA_TRANSFER_PROBABILITY_PER_RANDOM_TICK`。
const LAVA_TRANSFER_CHANCE: f32 = 0.058_593_75;
/// 水滴落进炼药锅的计划刻延迟，再加上下落的格数。
const DRIP_DELAY: i32 = 50;
/// `REQUIRED_SPACE_TO_DRIP_THROUGH_NON_SOLID_BLOCK`：`Block.column(4.0, 0.0, 16.0)`。
const DRIP_SPACE: &[Aabb] = &[Aabb::new(
    6.0 / 16.0,
    0.0,
    6.0 / 16.0,
    10.0 / 16.0,
    1.0,
    10.0 / 16.0,
)];

/// `isSpeleothemWithDirection`。
fn points(state: BlockState, tip: Direction) -> bool {
    state.is_tagged(block_tags::SPELEOTHEMS) && state.get(VERTICAL_DIRECTION) == tip
}

/// `isStalactite`。
fn is_stalactite(state: BlockState) -> bool {
    points(state, Direction::Down)
}

/// `isStalagmite`。
fn is_stalagmite(state: BlockState) -> bool {
    points(state, Direction::Up)
}

/// `isTip`，`includeMergedTip` 是真。
fn is_tip_or_merged(state: BlockState) -> bool {
    state.is_tagged(block_tags::SPELEOTHEMS)
        && matches!(
            state.get(SPELEOTHEM_THICKNESS),
            SpeleothemThickness::Tip | SpeleothemThickness::TipMerge
        )
}

/// `isFreeHangingStalactite`。
fn is_free_hanging_stalactite(state: BlockState) -> bool {
    is_stalactite(state)
        && state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::Tip
        && !state.get(WATERLOGGED)
}

/// `isValidSpeleothemPlacement`：`own` 是这个方块的任意一个状态。
fn is_valid_placement(own: BlockState, world: &World, pos: BlockPos, tip: Direction) -> bool {
    let behind_pos = pos.relative(tip.opposite());
    let behind = world.block_state(behind_pos);
    world.is_face_sturdy(behind_pos, tip, SupportType::Full)
        || points(behind, tip) && behind.is_same_block(own)
}

/// `calculateTipDirection`：优先朝着玩家看的反方向。
fn tip_direction(
    own: BlockState,
    world: &World,
    pos: BlockPos,
    preferred: Direction,
) -> Option<Direction> {
    [preferred, preferred.opposite()]
        .into_iter()
        .find(|&tip| is_valid_placement(own, world, pos, tip))
}

/// `calculateSpeleothemThickness`。
fn thickness(
    own: BlockState,
    world: &World,
    pos: BlockPos,
    tip: Direction,
    merge_opposing_tips: bool,
) -> SpeleothemThickness {
    let base = tip.opposite();
    let in_front = world.block_state(pos.relative(tip));
    if points(in_front, base) && in_front.is_same_block(own) {
        let merged = in_front.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::TipMerge;
        return if merge_opposing_tips || merged {
            SpeleothemThickness::TipMerge
        } else {
            SpeleothemThickness::Tip
        };
    }
    if !points(in_front, tip) {
        return SpeleothemThickness::Tip;
    }
    match in_front.get(SPELEOTHEM_THICKNESS) {
        SpeleothemThickness::Tip | SpeleothemThickness::TipMerge => SpeleothemThickness::Frustum,
        _ if points(world.block_state(pos.relative(base)), tip) => SpeleothemThickness::Middle,
        _ => SpeleothemThickness::Base,
    }
}

/// `spawnFallingStalactite`：从 `pos` 往下到尖端的整根都掉下来。
fn drop_stalactite(state: BlockState, world: &mut World, pos: BlockPos) {
    let mut fall_pos = pos;
    let mut fall_state = state;
    while is_stalactite(fall_state) {
        let id = fall(world, fall_pos, fall_state);
        if is_tip_or_merged(fall_state) {
            // `SpeleothemBlock.spawnFallingStalactite`：尖端 `setHurtsEntities(1 * size, 40)`。
            // 砸到实体的伤害不做，但铁砧裂开用的是同一个标记，这里也要设上。
            //
            // The tip calls `setHurtsEntities`. Entity damage is skipped, but the flag
            // is what an anvil reads before it rolls to crack, and dripstone sets it too.
            let size = (1 + pos.y - fall_pos.y).max(6);
            if let Some(entity) = world.entity_mut(id) {
                entity.set_hurts_entities(size as f32, 40);
            }
            break;
        }
        fall_pos = fall_pos.below();
        fall_state = world.block_state(fall_pos);
    }
}

/// `findBlockVertical`。
fn find_block_vertical(
    world: &World,
    pos: BlockPos,
    direction: Direction,
    path: impl Fn(BlockPos, BlockState) -> bool,
    target: impl Fn(BlockState) -> bool,
) -> Option<BlockPos> {
    let mut current = pos;
    for _ in 1..MAX_SEARCH_LENGTH {
        current = current.relative(direction);
        let state = world.block_state(current);
        if target(state) {
            return Some(current);
        }
        if !world.is_inside_build_height(current.y) || !path(current, state) {
            return None;
        }
    }
    None
}

/// `canDripThrough`：水滴能穿过这一格。
fn can_drip_through(world: &World, pos: BlockPos, state: BlockState) -> bool {
    if state.is_air() {
        return true;
    }
    if state.is_solid_render() || !state.fluid().is_empty() {
        return false;
    }
    let collision = state.collision_boxes(world, pos, CollisionContext::Empty);
    !shape::join_is_not_empty(DRIP_SPACE, &collision, BooleanOp::And)
}

/// `findStalactiteTipAboveCauldron`。
pub(super) fn stalactite_tip_above(world: &World, cauldron: BlockPos) -> Option<BlockPos> {
    find_block_vertical(
        world,
        cauldron,
        Direction::Up,
        |pos, state| can_drip_through(world, pos, state),
        is_free_hanging_stalactite,
    )
}

/// `findRootBlock`：这根钟乳石上面第一个不是它的方块。
fn root_above(world: &World, pos: BlockPos, stalactite: BlockState) -> Option<BlockPos> {
    let tip = stalactite.get(VERTICAL_DIRECTION);
    let same_stalactite = |state: BlockState| state.is_same_block(stalactite);
    find_block_vertical(
        world,
        pos,
        tip.opposite(),
        |_, state| same_stalactite(state) && state.get(VERTICAL_DIRECTION) == tip,
        |state| !same_stalactite(state),
    )
}

/// `getCauldronFillFluidType`：这根钟乳石能往下滴的流体，主世界的泥巴也算水。
pub(super) fn cauldron_fill_fluid(world: &World, stalactite: BlockPos) -> Option<Fluid> {
    let state = world.block_state(stalactite);
    if !is_stalactite(state) {
        return None;
    }
    let above = root_above(world, stalactite, state)?.above();
    let fluid_above = world.fluid_state(above);
    if world.block_state(above).is(blocks::MUD) || fluid_above.is(fluid::WATER) {
        Some(Fluid::Water)
    } else if fluid_above.is(fluid::LAVA) {
        Some(Fluid::Lava)
    } else {
        None
    }
}

impl Behaviour for Speleothem {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let own = default_state(block);
        let preferred = ctx.nearest_looking_vertical_direction().opposite();
        let tip = tip_direction(own, world, pos, preferred)?;
        let thickness = thickness(own, world, pos, tip, !ctx.is_secondary_use_active());
        Some(
            own.with(VERTICAL_DIRECTION, tip)
                .with(SPELEOTHEM_THICKNESS, thickness)
                .with(WATERLOGGED, is_water_source(world, pos)),
        )
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        is_valid_placement(state, world, pos, state.get(VERTICAL_DIRECTION))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        if direction.axis().is_horizontal() {
            return state;
        }
        let tip = state.get(VERTICAL_DIRECTION);
        if tip == Direction::Down && world.has_scheduled_tick(pos, state.block()) {
            return state;
        }
        if direction == tip.opposite() && !state.can_survive(world, pos) {
            let delay = if tip == Direction::Down {
                DELAY_BEFORE_FALLING
            } else {
                1
            };
            world.schedule_tick(pos, state.block(), delay);
            return state;
        }
        let merge = state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::TipMerge;
        state.with(
            SPELEOTHEM_THICKNESS,
            thickness(state, world, pos, tip, merge),
        )
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_stalagmite(state) && !state.can_survive(world, pos) {
            world.destroy_block(pos, true, UPDATE_LIMIT);
        } else {
            drop_stalactite(state, world, pos);
        }
    }

    /// 滴水石锥先看要不要往下滴水，两种钟乳石都偶尔往下长或者在下面长出石笋。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.is(blocks::POINTED_DRIPSTONE) {
            let value = world.random().next_float();
            transfer_fluid(state, world, pos, value);
        }
        if world.random().next_float() < GROWTH_CHANCE && is_stalactite_start(state, world, pos) {
            grow_if_possible(state, world, pos);
        }
    }
}

/// `isStalactiteStartPos`：钟乳石最上面的一节。
fn is_stalactite_start(state: BlockState, world: &World, pos: BlockPos) -> bool {
    is_stalactite(state) && !world.block_state(pos.above()).is_same_block(state)
}

/// `isTip(state, false)`：不算合并的尖端。
fn is_unmerged_tip(state: BlockState) -> bool {
    state.is_tagged(block_tags::SPELEOTHEMS)
        && state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::Tip
}

/// `findTip(state, level, pos, maxSearchLength, false)`：顺着尖端的方向找到尖端。
fn find_tip(state: BlockState, world: &World, pos: BlockPos, max_length: i32) -> Option<BlockPos> {
    if is_unmerged_tip(state) {
        return Some(pos);
    }
    let tip = state.get(VERTICAL_DIRECTION);
    let mut current = pos;
    for _ in 1..max_length {
        current = current.relative(tip);
        let found = world.block_state(current);
        if is_unmerged_tip(found) {
            return Some(current);
        }
        let same = found.is_same_block(state) && found.get(VERTICAL_DIRECTION) == tip;
        if !world.is_inside_build_height(current.y) || !same {
            return None;
        }
    }
    None
}

/// `PointedDripstoneBlock.maybeTransferFluid`：根上面是水或者岩浆时，按概率滴进下面的炼药锅，
/// 根上面是泥巴时把它变成黏土。`value` 是官方先取好的那个 `nextFloat`。
fn transfer_fluid(state: BlockState, world: &mut World, pos: BlockPos, value: f32) {
    if value > WATER_TRANSFER_CHANCE || !is_stalactite_start(state, world, pos) {
        return;
    }
    let Some(root) = root_above(world, pos, state) else {
        return;
    };
    let source_pos = root.above();
    let source = world.block_state(source_pos);
    let fluid_above = world.fluid_state(source_pos);
    let (fluid, chance) = if source.is(blocks::MUD) || fluid_above.is(fluid::WATER) {
        (Fluid::Water, WATER_TRANSFER_CHANCE)
    } else if fluid_above.is(fluid::LAVA) {
        (Fluid::Lava, LAVA_TRANSFER_CHANCE)
    } else {
        return;
    };
    if value >= chance {
        return;
    }
    let Some(tip) = find_tip(state, world, pos, MAX_SEARCH_LENGTH) else {
        return;
    };
    if source.is(blocks::MUD) {
        world.set_block_and_update(source_pos, BlockState(blocks::CLAY));
        // 泥巴变成黏土之后，在尖端那里放滴落的粒子。
        world.level_event(1504, tip, 0);
    } else if let Some(cauldron) = fillable_cauldron_below(world, tip, fluid) {
        // 官方在排炼药锅的刻之前发这个事件，炼药锅那一声是 `1046`/`1047`。
        world.level_event(1504, tip, 0);
        let delay = DRIP_DELAY + tip.y - cauldron.y;
        let cauldron_block = world.block_state(cauldron).block();
        world.schedule_tick(cauldron, cauldron_block, delay);
    }
}

/// `findFillableCauldronBelowStalactiteTip`。
fn fillable_cauldron_below(world: &World, tip: BlockPos, fluid: Fluid) -> Option<BlockPos> {
    find_block_vertical(
        world,
        tip,
        Direction::Down,
        |pos, state| can_drip_through(world, pos, state),
        |state| can_receive_drip(state, fluid),
    )
}

/// `growStalactiteOrStalagmiteIfPossible`：钟乳石的尖端能长时，一半机会往下长，一半机会在下面长石笋。
fn grow_if_possible(state: BlockState, world: &mut World, pos: BlockPos) {
    if !can_grow(state, world, pos) {
        return;
    }
    let max_length = field_int(state.block(), "getMaxGrowthLength()");
    let Some(tip) = find_tip(state, world, pos, max_length) else {
        return;
    };
    let tip_state = world.block_state(tip);
    if is_free_hanging_stalactite(tip_state) && can_tip_grow(state, tip_state, world, tip) {
        if world.random().next_boolean() {
            grow(state, world, tip, Direction::Down);
        } else {
            grow_stalagmite_below(state, world, tip);
        }
    }
}

/// `canGrow`：长在 `blockToGrowOn` 下面。滴水石锥还要再上面一格是水源。
fn can_grow(own: BlockState, world: &World, pos: BlockPos) -> bool {
    let block = own.block();
    let Some(FieldValue::State(grow_on)) = block.field("blockToGrowOn") else {
        panic!("field blockToGrowOn of {} is not a state", block.name);
    };
    let on_base = world
        .block_state(pos.above())
        .is_same_block(BlockState(grow_on));
    if !own.is(blocks::POINTED_DRIPSTONE) {
        return on_base;
    }
    on_base
        && world
            .block_state(pos.relative_by(Direction::Up, 2))
            .fluid()
            .is(fluid::WATER)
}

/// `isUnmergedTipWithDirection`：同种方块、朝 `tip` 的、没合并的尖端。
fn is_unmerged_tip_toward(own: BlockState, state: BlockState, tip: Direction) -> bool {
    is_unmerged_tip(state) && state.get(VERTICAL_DIRECTION) == tip && state.is_same_block(own)
}

/// `canTipGrow`：尖端前面是空气，或者是对着它的另一根的尖端。
fn can_tip_grow(own: BlockState, tip_state: BlockState, world: &World, tip: BlockPos) -> bool {
    let direction = tip_state.get(VERTICAL_DIRECTION);
    let ahead = world.block_state(tip.relative(direction));
    if !ahead.fluid().is_empty() {
        return false;
    }
    ahead.is_air() || is_unmerged_tip_toward(own, ahead, direction.opposite())
}

/// `grow`：往 `direction` 长一节，碰到对面的尖端就合并。
fn grow(own: BlockState, world: &mut World, from: BlockPos, direction: Direction) {
    let target = from.relative(direction);
    let existing = world.block_state(target);
    if is_unmerged_tip_toward(own, existing, direction.opposite()) {
        create_merged_tips(own, world, existing, target);
    } else if existing.is_air() || existing.is(blocks::WATER) {
        create_speleothem(own, world, target, direction, SpeleothemThickness::Tip);
    }
}

/// `createSpeleothem`。
fn create_speleothem(
    own: BlockState,
    world: &mut World,
    pos: BlockPos,
    direction: Direction,
    thickness: SpeleothemThickness,
) {
    let state = own
        .default_state()
        .with(VERTICAL_DIRECTION, direction)
        .with(SPELEOTHEM_THICKNESS, thickness)
        .with(WATERLOGGED, world.fluid_state(pos).is(fluid::WATER));
    world.set_block_and_update(pos, state);
}

/// `createMergedTips`：上下两个尖端都变成合并的尖端。
fn create_merged_tips(own: BlockState, world: &mut World, tip_state: BlockState, tip: BlockPos) {
    let (stalactite, stalagmite) = if tip_state.get(VERTICAL_DIRECTION) == Direction::Up {
        (tip.above(), tip)
    } else {
        (tip, tip.below())
    };
    create_speleothem(
        own,
        world,
        stalactite,
        Direction::Down,
        SpeleothemThickness::TipMerge,
    );
    create_speleothem(
        own,
        world,
        stalagmite,
        Direction::Up,
        SpeleothemThickness::TipMerge,
    );
}

/// `growStalagmiteBelow`：往下找最多 10 格，长高已有的石笋，或者在地上长一根新的。
fn grow_stalagmite_below(own: BlockState, world: &mut World, above: BlockPos) {
    let mut pos = above;
    for _ in 0..STALAGMITE_SEARCH_RANGE {
        pos = pos.below();
        let state = world.block_state(pos);
        if !state.fluid().is_empty() {
            return;
        }
        if is_unmerged_tip_toward(own, state, Direction::Up) && can_tip_grow(own, state, world, pos)
        {
            grow(own, world, pos, Direction::Up);
            return;
        }
        if is_valid_placement(own, world, pos, Direction::Up) && !world.is_water_at(pos.below()) {
            grow(own, world, pos.below(), Direction::Up);
            return;
        }
        // `blocksStalagmiteScan`：只有滴水石锥会被挡住。
        if own.is(blocks::POINTED_DRIPSTONE) && !can_drip_through(world, pos, state) {
            return;
        }
    }
}
