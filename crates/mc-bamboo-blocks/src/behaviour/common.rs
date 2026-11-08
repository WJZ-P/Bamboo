//! 几个家族都要用的官方静态方法和父类。
//!
//! 只有一个家族用的东西放在家族自己的文件里。有官方方法对应的，名字是方法名的 Rust 写法，
//! 对照官方时直接搜方法名。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, BlockAncestor, BlockTag, FieldValue, Item, blocks, items, registries::fluid,
    tags::block_tags,
};

use super::{Behaviour, Plain};
use crate::{
    context::PlaceContext,
    level::Effect,
    shape,
    state::{
        ATTACH_FACE, AttachFace, BlockState, DOUBLE_BLOCK_HALF, DOWN, DoubleBlockHalf, EAST,
        FluidState, HORIZONTAL_FACING, NORTH, Property, SOUTH, STABILITY_DISTANCE, SupportType, UP,
        WATERLOGGED, WEST,
    },
    world::{UPDATE_ALL, UPDATE_LIMIT, UPDATE_SUPPRESS_DROPS, World},
};

/// `ScaffoldingBlock.STABILITY_MAX_DISTANCE`。
pub const SCAFFOLDING_MAX_DISTANCE: i32 = 7;
/// `FluidState.isFull` 要求的量。
const FULL_FLUID_AMOUNT: i32 = 8;

/// 方块的注册表 ID 对应的方块。
pub fn block_by_id(id: i32) -> &'static Block {
    &blocks::BLOCKS[id as usize]
}

/// 物品 ID 对应的物品。ID 来自客户端，不认识的 ID 是 `None`。
pub fn item_of(id: i32) -> Option<&'static Item> {
    items::ITEMS.get(usize::try_from(id).ok()?)
}

/// 方块的默认状态。
pub fn default_state(block: &Block) -> BlockState {
    BlockState(block.default_state)
}

/// 最常见的放置：水平地正对着玩家。
pub fn facing_the_player(block: &'static Block, ctx: &PlaceContext) -> BlockState {
    default_state(block).with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite())
}

/// `PipeBlock.PROPERTY_BY_DIRECTION`：每个方向一个布尔属性。
pub const fn pipe_property(direction: Direction) -> Property<bool> {
    match direction {
        Direction::North => NORTH,
        Direction::East => EAST,
        Direction::South => SOUTH,
        Direction::West => WEST,
        Direction::Up => UP,
        Direction::Down => DOWN,
    }
}

/// 官方类的方块字段，比如 `field_block(block, "deadBlock")`。字段不存在时 panic，这是移植写错了。
pub fn field_block(block: &Block, name: &str) -> &'static Block {
    match field(block, name) {
        FieldValue::Block(id) => block_by_id(id),
        other => panic!("field {name} of {} is {other:?}", block.name),
    }
}

/// 官方类的布尔字段，比如 `field_bool(block, "type.canOpenByHand")`。
pub fn field_bool(block: &Block, name: &str) -> bool {
    match field(block, name) {
        FieldValue::Bool(value) => value,
        other => panic!("field {name} of {} is {other:?}", block.name),
    }
}

/// 官方类的整数字段，比如 `field_int(block, "ticksToStayPressed")`。
pub fn field_int(block: &Block, name: &str) -> i32 {
    match field(block, name) {
        FieldValue::Int(value) => value,
        other => panic!("field {name} of {} is {other:?}", block.name),
    }
}

/// 官方类的方块标签字段，比如 `field_tag(block, "supportBlocks")`。
pub fn field_tag(block: &Block, name: &str) -> BlockTag {
    match field(block, name) {
        FieldValue::Tag(tag) => tag,
        other => panic!("field {name} of {} is {other:?}", block.name),
    }
}

/// 官方类的枚举字段的小写名字，比如 `field_name(block, "getAge()")`。
pub fn field_name(block: &Block, name: &str) -> &'static str {
    match field(block, name) {
        FieldValue::Name(value) => value,
        other => panic!("field {name} of {} is {other:?}", block.name),
    }
}

fn field(block: &Block, name: &str) -> FieldValue {
    block
        .field(name)
        .unwrap_or_else(|| panic!("{} has no field {name}", block.name))
}

/// `Block.canSupportRigidBlock`。
pub fn can_support_rigid_block(world: &World, below: BlockPos) -> bool {
    world.is_face_sturdy(below, Direction::Up, SupportType::Rigid)
}

/// `Block.canSupportCenter`。
pub fn can_support_center(world: &World, pos: BlockPos, direction: Direction) -> bool {
    let state = world.block_state(pos);
    if direction == Direction::Down && state.is_tagged(block_tags::UNSTABLE_BOTTOM_CENTER) {
        return false;
    }
    world.is_face_sturdy(pos, direction, SupportType::Center)
}

/// `Block.isExceptionForConnection`：这些方块的面是完整的，栅栏和玻璃板却不连上去。
pub fn is_exception_for_connection(state: BlockState) -> bool {
    state.is_a(BlockAncestor::LeavesBlock)
        || state.is(blocks::BARRIER)
        || state.is(blocks::CARVED_PUMPKIN)
        || state.is(blocks::JACK_O_LANTERN)
        || state.is(blocks::MELON)
        || state.is(blocks::PUMPKIN)
        || state.is_tagged(block_tags::SHULKER_BOXES)
}

/// `Block.isFaceFull(state.getCollisionShape(level, pos), direction)`。
pub fn is_collision_face_full(world: &World, pos: BlockPos, direction: Direction) -> bool {
    let state = world.block_state(pos);
    let boxes = state.collision_boxes(world, pos, shape::CollisionContext::Empty);
    shape::is_face_full(&boxes, direction)
}

/// `MultifaceBlock.canAttachTo(level, pos, direction)`：这一格朝 `direction` 的邻居能贴。
pub fn multiface_can_attach_at(world: &World, pos: BlockPos, direction: Direction) -> bool {
    let neighbor_pos = pos.relative(direction);
    let neighbor = world.block_state(neighbor_pos);
    multiface_can_attach_to(world, direction, neighbor_pos, neighbor)
}

/// `MultifaceBlock.canAttachTo(level, direction, neighbourPos, neighbourState)`：
/// `direction` 方向上的邻居朝向这里的面是完整的。
pub fn multiface_can_attach_to(
    world: &World,
    direction: Direction,
    neighbor_pos: BlockPos,
    neighbor: BlockState,
) -> bool {
    let face = direction.opposite();
    let collision = neighbor.collision_boxes(world, neighbor_pos, shape::CollisionContext::Empty);
    shape::is_face_full(&shape::support_boxes(neighbor, neighbor_pos), face)
        || shape::is_face_full(&collision, face)
}

/// `level.getFluidState(pos).is(Fluids.WATER)`：含水方块放置时看的是不是水源。
pub fn is_water_source(world: &World, pos: BlockPos) -> bool {
    world.fluid_state(pos).is(fluid::WATER)
}

/// `is(FluidTags.WATER) && isFull()`：水源，或者往下落的满格水。
pub fn is_full_water(fluid: FluidState) -> bool {
    fluid.is_water() && fluid.amount() == FULL_FLUID_AMOUNT
}

/// 含水方块 `updateShape` 开头的那一段：含水时给水排一个刻。
pub fn tick_water_if_waterlogged(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.get(WATERLOGGED) {
        world.schedule_fluid_tick(pos, fluid::WATER);
    }
}

/// `SimpleWaterloggedBlock.placeLiquid`：不含水时放进水源，给水排一个刻。
pub fn waterlog(state: BlockState, world: &mut World, pos: BlockPos, liquid: FluidState) -> bool {
    if state.get(WATERLOGGED) || !liquid.is(fluid::WATER) {
        return false;
    }
    world.set_block_and_update(pos, state.with(WATERLOGGED, true));
    world.schedule_fluid_tick(pos, liquid.fluid());
    true
}

/// `SimpleWaterloggedBlock.pickupBlock`：舀走水，没了水活不下去的方块跟着碎掉。返回水桶，没有水时是空气。
pub fn drain_waterlogged(state: BlockState, world: &mut World, pos: BlockPos) -> i32 {
    if !state.get(WATERLOGGED) {
        return items::AIR;
    }
    world.set_block_and_update(pos, state.with(WATERLOGGED, false));
    // 官方用的是舀水之前的状态。
    if !state.can_survive(world, pos) {
        world.destroy_block(pos, UPDATE_LIMIT);
    }
    items::WATER_BUCKET
}

/// `Containers.updateNeighboursAfterDestroy`：告诉比较器这里的内容没了。
pub fn update_neighbours_after_destroy(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbour_for_output_signal(pos, state.block());
}

/// `DoublePlantBlock.preventDropFromBottomPart`：门和高植物的上半被破坏时，
/// 下半先静默地变成空气或者水，这样不会多掉一份。
pub fn prevent_drop_from_bottom_part(world: &mut World, pos: BlockPos, state: BlockState) {
    if state.get(DOUBLE_BLOCK_HALF) != DoubleBlockHalf::Upper {
        return;
    }
    let bottom_pos = pos.below();
    let bottom = world.block_state(bottom_pos);
    if !bottom.is_same_block(state) || bottom.get(DOUBLE_BLOCK_HALF) != DoubleBlockHalf::Lower {
        return;
    }
    let replacement = if bottom.fluid().is(fluid::WATER) {
        BlockState::WATER
    } else {
        BlockState::AIR
    };
    world.set_block(bottom_pos, replacement, UPDATE_ALL | UPDATE_SUPPRESS_DROPS);
    world.push_effect(Effect::Destroyed {
        pos: bottom_pos,
        state: bottom,
        except_initiator: true,
    });
}

/// `RotationSegment.convertToSegment(degrees)`：告示牌、旗帜、头颅的 16 个朝向。
pub fn rotation_segment(degrees: f32) -> i32 {
    // `Math.round` 在 `float` 上的结果就是加 0.5 再向下取整，放到 `f64` 里算没有舍入误差。
    let turns = degrees * (16.0 / 360.0);
    (f64::from(turns) + 0.5).floor() as i32 & 15
}

/// `RotationSegment.convertToDegrees(segment)`：段换回角度，180 度以上的折成负角。
pub fn rotation_degrees(segment: i32) -> f32 {
    let degrees = (segment & 15) as f32 * 22.5;
    if degrees >= 180.0 {
        degrees - 360.0
    } else {
        degrees
    }
}

/// `FallingBlock.isFree`。
pub fn is_free(state: BlockState) -> bool {
    state.is_air()
        || state.is_tagged(block_tags::FIRE)
        || state.is_liquid()
        || state.is_replaceable()
}

/// `VegetationBlock.mayPlaceOn` 的默认实现。
pub fn may_place_vegetation_on(state: BlockState) -> bool {
    state.is_tagged(block_tags::SUPPORTS_VEGETATION)
}

/// `VegetationBlock.updateShape`：放不住就变成空气。
pub fn vegetation_update_shape(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    if state.can_survive(world, pos) {
        state
    } else {
        BlockState::AIR
    }
}

/// `ScaffoldingBlock.getDistance`：离支撑还有几格，7 表示放不住。
pub fn scaffolding_distance(world: &World, pos: BlockPos) -> i32 {
    let below_pos = pos.below();
    let below = world.block_state(below_pos);
    let mut distance = SCAFFOLDING_MAX_DISTANCE;
    if below.is(blocks::SCAFFOLDING) {
        distance = below.get(STABILITY_DISTANCE);
    } else if world.is_face_sturdy(below_pos, Direction::Up, SupportType::Full) {
        return 0;
    }
    for direction in Direction::HORIZONTAL {
        let beside = world.block_state(pos.relative(direction));
        if beside.is(blocks::SCAFFOLDING) {
            distance = distance.min(beside.get(STABILITY_DISTANCE) + 1);
            if distance == 1 {
                break;
            }
        }
    }
    distance
}

/// `BaseTorchBlock`：普通火把和红石火把的父类。
pub struct BaseTorch;

impl Behaviour for BaseTorch {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_support_center(world, pos.below(), Direction::Up)
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// 挂墙方块 `getStateForPlacement` 里的循环：按视线找第一个放得住的水平朝向，方块背对支撑。
pub fn wall_state_for_placement(
    default: BlockState,
    ctx: &PlaceContext,
    world: &World,
) -> Option<BlockState> {
    ctx.nearest_looking_directions()
        .into_iter()
        .filter(|direction| direction.axis().is_horizontal())
        .map(|direction| default.with(HORIZONTAL_FACING, direction.opposite()))
        .find(|state| state.can_survive(world, ctx.clicked_pos()))
}

/// 挂墙方块 `updateShape` 里的判定：支撑那一面变了，现在放不住了。
pub fn lost_wall_support(
    state: BlockState,
    world: &World,
    pos: BlockPos,
    direction: Direction,
) -> bool {
    direction.opposite() == state.get(HORIZONTAL_FACING) && !state.can_survive(world, pos)
}

/// `WallTorchBlock.canSurvive(level, pos, facing)`：背后那一格朝这边的面是完整的。
/// 墙上火把、梯子、绊线钩和墙上的珊瑚扇都这样挂着。
pub fn has_sturdy_wall(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let facing = state.get(HORIZONTAL_FACING);
    world.is_face_sturdy(pos.relative(facing.opposite()), facing, SupportType::Full)
}

/// `FaceAttachedHorizontalDirectionalBlock`：拉杆、按钮和砂轮的父类。
pub struct FaceAttached;

impl FaceAttached {
    /// `getConnectedDirection`：方块朝外的那个方向，支撑在它的反方向。
    pub fn connected_direction(state: BlockState) -> Direction {
        match state.get(ATTACH_FACE) {
            AttachFace::Ceiling => Direction::Down,
            AttachFace::Floor => Direction::Up,
            AttachFace::Wall => state.get(HORIZONTAL_FACING),
        }
    }

    /// `canAttach`。
    pub fn can_attach(world: &World, pos: BlockPos, direction: Direction) -> bool {
        world.is_face_sturdy(
            pos.relative(direction),
            direction.opposite(),
            SupportType::Full,
        )
    }
}

impl Behaviour for FaceAttached {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let default = default_state(block);
        for direction in ctx.nearest_looking_directions() {
            let state = if direction.axis().is_vertical() {
                let face = if direction == Direction::Up {
                    AttachFace::Ceiling
                } else {
                    AttachFace::Floor
                };
                default
                    .with(ATTACH_FACE, face)
                    .with(HORIZONTAL_FACING, ctx.horizontal_direction())
            } else {
                default
                    .with(ATTACH_FACE, AttachFace::Wall)
                    .with(HORIZONTAL_FACING, direction.opposite())
            };
            if state.can_survive(world, ctx.clicked_pos()) {
                return Some(state);
            }
        }
        None
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Self::can_attach(world, pos, Self::connected_direction(state).opposite())
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
        if Self::connected_direction(state).opposite() == direction
            && !state.can_survive(world, pos)
        {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_segments_match_mth() {
        assert_eq!(rotation_segment(0.0), 0);
        assert_eq!(rotation_segment(22.5), 1);
        assert_eq!(rotation_segment(180.0), 8);
        assert_eq!(rotation_segment(-22.5), 15);
        assert_eq!(rotation_segment(359.0), 0);
        // 正好在两段中间时向正方向取整。
        assert_eq!(rotation_segment(11.25), 1);
        assert_eq!(rotation_segment(-11.25), 0);
    }

    #[test]
    fn typed_fields_read_constructor_parameters() {
        let iron_door = default_state(block_by_id(BlockState(blocks::IRON_DOOR).block_id() as i32));
        assert!(!field_bool(iron_door.block(), "type.canOpenByHand"));
        assert!(field_bool(
            BlockState(blocks::OAK_DOOR).block(),
            "type.canOpenByHand"
        ));
        let button = BlockState(blocks::STONE_BUTTON).block();
        assert_eq!(field_int(button, "ticksToStayPressed"), 20);
        let coral = BlockState(blocks::TUBE_CORAL).block();
        assert_eq!(
            field_block(coral, "deadBlock").default_state,
            BlockState(blocks::DEAD_TUBE_CORAL).0
        );
        let stem = BlockState(blocks::MELON_STEM).block();
        assert_eq!(
            field_tag(stem, "stemSupportBlocks"),
            block_tags::SUPPORTS_MELON_STEM
        );
        let copper = BlockState(blocks::COPPER_BLOCK).block();
        assert_eq!(field_name(copper, "weatherState"), "unaffected");
    }

    #[test]
    #[should_panic(expected = "has no field")]
    fn a_wrong_field_name_panics() {
        field_bool(BlockState(blocks::STONE).block(), "noSuchField");
    }

    #[test]
    fn free_blocks_are_the_ones_sand_falls_through() {
        assert!(is_free(BlockState::AIR));
        assert!(is_free(BlockState::WATER));
        assert!(is_free(BlockState(blocks::FIRE)));
        assert!(is_free(BlockState(blocks::SHORT_GRASS)));
        assert!(!is_free(BlockState(blocks::STONE)));
    }

    #[test]
    fn connection_exceptions_are_the_official_list() {
        assert!(is_exception_for_connection(BlockState(blocks::OAK_LEAVES)));
        assert!(is_exception_for_connection(BlockState(blocks::SHULKER_BOX)));
        assert!(is_exception_for_connection(BlockState(blocks::MELON)));
        assert!(!is_exception_for_connection(BlockState(blocks::STONE)));
    }
}
