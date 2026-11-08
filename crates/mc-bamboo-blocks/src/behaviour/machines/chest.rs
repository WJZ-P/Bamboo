//! `ChestBlock`、`TrappedChestBlock`、`CopperChestBlock`、`WeatheringCopperChestBlock`、
//! `EnderChestBlock`。
//!
//! 箱子物品和开合动画都在方块实体里，这里只有放置和合并成大箱子。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, block_maps, blocks, tags::block_tags};

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            block_by_id, default_state, facing_the_player, field_bool, field_name, is_water_source,
            tick_water_if_waterlogged, update_neighbours_after_destroy,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CHEST_TYPE, ChestType, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

/// `CopperChestBlock.COPPER_TO_COPPER_CHEST_MAPPING`：铜块对应的铜箱子。
const COPPER_TO_CHEST: [(u32, u32); 8] = [
    (blocks::COPPER_BLOCK, blocks::COPPER_CHEST),
    (blocks::EXPOSED_COPPER, blocks::EXPOSED_COPPER_CHEST),
    (blocks::WEATHERED_COPPER, blocks::WEATHERED_COPPER_CHEST),
    (blocks::OXIDIZED_COPPER, blocks::OXIDIZED_COPPER_CHEST),
    (blocks::WAXED_COPPER_BLOCK, blocks::WAXED_COPPER_CHEST),
    (
        blocks::WAXED_EXPOSED_COPPER,
        blocks::WAXED_EXPOSED_COPPER_CHEST,
    ),
    (
        blocks::WAXED_WEATHERED_COPPER,
        blocks::WAXED_WEATHERED_COPPER_CHEST,
    ),
    (
        blocks::WAXED_OXIDIZED_COPPER,
        blocks::WAXED_OXIDIZED_COPPER_CHEST,
    ),
];

/// `ChestBlock`、`TrappedChestBlock`，后者只多了信号。
pub(super) struct Chest;

/// `getConnectedDirection`：另一半在哪个方向。单个的箱子也有结果，官方靠它判定邻居变化。
fn connected_direction(state: BlockState) -> Direction {
    let facing = state.get(HORIZONTAL_FACING);
    if state.get(CHEST_TYPE) == ChestType::Left {
        facing.clockwise()
    } else {
        facing.counter_clockwise()
    }
}

/// `ChestType.getOpposite`。
const fn opposite_type(chest_type: ChestType) -> ChestType {
    match chest_type {
        ChestType::Single => ChestType::Single,
        ChestType::Left => ChestType::Right,
        ChestType::Right => ChestType::Left,
    }
}

/// `chestCanConnectTo`：普通箱子只和同一种箱子合并，铜箱子不管氧化程度和涂蜡。
fn can_connect_to(chest: BlockState, other: BlockState) -> bool {
    if chest.is_a(BlockAncestor::CopperChestBlock) {
        other.is_tagged(block_tags::COPPER_CHESTS) && other.has(CHEST_TYPE)
    } else {
        other.is_same_block(chest)
    }
}

/// `candidatePartnerFacing`：这个方向上有还没合并的箱子时，返回它的朝向。
fn candidate_partner_facing(
    chest: BlockState,
    world: &World,
    pos: BlockPos,
    direction: Direction,
) -> Option<Direction> {
    let neighbour = world.block_state(pos.relative(direction));
    let free = can_connect_to(chest, neighbour) && neighbour.get(CHEST_TYPE) == ChestType::Single;
    free.then(|| neighbour.get(HORIZONTAL_FACING))
}

/// `getChestType`：朝向相同的单个箱子在哪一边。
fn chest_type_beside(
    chest: BlockState,
    world: &World,
    pos: BlockPos,
    facing: Direction,
) -> ChestType {
    if candidate_partner_facing(chest, world, pos, facing.clockwise()) == Some(facing) {
        ChestType::Left
    } else if candidate_partner_facing(chest, world, pos, facing.counter_clockwise())
        == Some(facing)
    {
        ChestType::Right
    } else {
        ChestType::Single
    }
}

/// 蹲着点旁边箱子的侧面时，直接和点中的箱子合并，不管玩家的朝向。
fn partner_of_clicked_side(
    chest: BlockState,
    ctx: &PlaceContext,
    world: &World,
) -> Option<(Direction, ChestType)> {
    let clicked_face = ctx.clicked_face();
    if !clicked_face.axis().is_horizontal() || !ctx.is_secondary_use_active() {
        return None;
    }
    let toward_clicked = clicked_face.opposite();
    let facing = candidate_partner_facing(chest, world, ctx.clicked_pos(), toward_clicked)
        .filter(|facing| facing.axis() != clicked_face.axis())?;
    let chest_type = if facing.counter_clockwise() == toward_clicked {
        ChestType::Right
    } else {
        ChestType::Left
    };
    Some((facing, chest_type))
}

impl Behaviour for Chest {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let chest = default_state(block);
        let pos = ctx.clicked_pos();
        let (facing, chest_type) =
            partner_of_clicked_side(chest, ctx, world).unwrap_or_else(|| {
                let facing = ctx.horizontal_direction().opposite();
                let chest_type = if ctx.is_secondary_use_active() {
                    ChestType::Single
                } else {
                    chest_type_beside(chest, world, pos, facing)
                };
                (facing, chest_type)
            });
        let state = chest
            .with(HORIZONTAL_FACING, facing)
            .with(CHEST_TYPE, chest_type)
            .with(WATERLOGGED, is_water_source(world, pos));
        Some(state)
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
        if can_connect_to(state, neighbor) && direction.axis().is_horizontal() {
            let neighbor_type = neighbor.get(CHEST_TYPE);
            if state.get(CHEST_TYPE) == ChestType::Single
                && neighbor_type != ChestType::Single
                && state.get(HORIZONTAL_FACING) == neighbor.get(HORIZONTAL_FACING)
                && connected_direction(neighbor) == direction.opposite()
            {
                return state.with(CHEST_TYPE, opposite_type(neighbor_type));
            }
        } else if connected_direction(state) == direction {
            return state.with(CHEST_TYPE, ChestType::Single);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// `ChestBlockEntity.triggerEvent`：事件 1 是开盖人数，客户端自己播盖子的动画。
    ///
    /// 箱子内容在 `mc-bamboo-game` 里，方块层没有箱子实体，所以不能像官方那样查实体在不在。
    fn trigger_event(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        param_a: i32,
        _param_b: i32,
    ) -> bool {
        param_a == 1
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `ChestBlock.getAnalogOutputSignal`：`getContainer(..., false)`，压住时读不到，
    /// 双箱按合并后的 54 格算。位置上的规则在 `Level::container_signal` 里。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world.container_signal(pos)
    }
}

/// `CopperChestBlock`、`WeatheringCopperChestBlock`：合并时两半要变成同一种铜箱子。
pub(super) struct CopperChest;

/// `WeatherState.ordinal`：氧化到哪一步。
fn weather_age(state: BlockState) -> usize {
    match field_name(state.block(), "weatherState") {
        "unaffected" => 0,
        "exposed" => 1,
        "weathered" => 2,
        "oxidized" => 3,
        other => panic!("unknown weather state {other}"),
    }
}

fn is_waxed(state: BlockState) -> bool {
    field_bool(state.block(), "isWaxed()")
}

/// `unwaxBlock`：擦掉蜡之后的箱子，查不到对应关系时是 `None`。
fn unwaxed(state: BlockState) -> Option<BlockState> {
    if !is_waxed(state) {
        return Some(state);
    }
    let id = state.block_id() as i32;
    block_maps::WAXABLES
        .iter()
        .find(|&&(_, waxed)| waxed == id)
        .map(|&(plain, _)| state.copied_onto(block_by_id(plain)))
}

/// `getLeastOxidizedChestOfConnectedBlocks`：大箱子取两半里氧化最少的一种，有一半没涂蜡就都不涂蜡。
fn least_oxidized_of_connected(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let connected = world.block_state(pos.relative(connected_direction(state)));
    if state.get(CHEST_TYPE) == ChestType::Single
        || !connected.is_a(BlockAncestor::CopperChestBlock)
    {
        return state;
    }
    let (mine, theirs) = if is_waxed(state) == is_waxed(connected) {
        (state, connected)
    } else {
        (
            unwaxed(state).unwrap_or(state),
            unwaxed(connected).unwrap_or(connected),
        )
    };
    let least_oxidized = if weather_age(state) <= weather_age(connected) {
        mine
    } else {
        theirs
    };
    mine.copied_onto(least_oxidized.block())
}

/// `CopperChestBlock.getFromCopperBlock`：铜傀儡留下的箱子。
pub(super) fn chest_from_copper_block(
    copper: BlockState,
    facing: Direction,
    world: &World,
    pos: BlockPos,
) -> BlockState {
    let chest = COPPER_TO_CHEST
        .iter()
        .find(|&&(block, _)| copper.is(block))
        .map_or(blocks::COPPER_CHEST, |&(_, chest)| chest);
    let chest = BlockState(chest).default_state();
    let state = chest
        .with(HORIZONTAL_FACING, facing)
        .with(CHEST_TYPE, chest_type_beside(chest, world, pos, facing));
    least_oxidized_of_connected(state, world, pos)
}

impl Behaviour for CopperChest {
    /// `WeatheringCopperChestBlock`：右半边不氧化，交给左半边。没有人打开箱子，世界里没有实体。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(CHEST_TYPE) != ChestType::Right {
            change_over_time(state, world, pos);
        }
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = Chest.state_for_placement(block, ctx, world)?;
        Some(least_oxidized_of_connected(state, world, ctx.clicked_pos()))
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
        let updated = Chest.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        if can_connect_to(state, neighbor)
            && updated.get(CHEST_TYPE) != ChestType::Single
            && connected_direction(updated) == direction
        {
            return updated.copied_onto(neighbor.block());
        }
        updated
    }

    fn trigger_event(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        Chest.trigger_event(state, world, pos, param_a, param_b)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        Chest.affect_neighbors_after_removal(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Chest.use_without_item(state, world, ctx)
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        Chest.analog_output_signal(state, world, pos, direction)
    }
}

/// `EnderChestBlock`。
pub(super) struct EnderChest;

impl Behaviour for EnderChest {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let water = is_water_source(world, ctx.clicked_pos());
        Some(facing_the_player(block, ctx).with(WATERLOGGED, water))
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

    /// `EnderChestBlockEntity.triggerEvent`，和箱子一样只认事件 1。
    fn trigger_event(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        param_a: i32,
        _param_b: i32,
    ) -> bool {
        param_a == 1
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}
