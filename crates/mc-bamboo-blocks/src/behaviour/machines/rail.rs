//! `BaseRailBlock` 和它的子类 `RailBlock`、`PoweredRailBlock`、`DetectorRailBlock`，还有 `RailState`。
//!
//! 世界里没有矿车：探测铁轨不会被压下，所以它的 `powered` 只在被指令改时才变。
//! 动力铁轨和激活铁轨看真实信号，也顺着铁轨往两头找通电的同类铁轨。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, BlockClass, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            can_support_rigid_block, default_state, field_bool, is_water_source,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BlockState, POWERED, RAIL_SHAPE, RailShape, WATERLOGGED},
    world::World,
};

/// `BaseRailBlock`。三个子类的差别只有两处，在 [`neighbor_update`] 和 `on_place` 里分支。
pub(super) struct Rail;

/// `BaseRailBlock.isRail`。
fn is_rail(state: BlockState) -> bool {
    state.is_tagged(block_tags::RAILS) && state.is_a(BlockAncestor::BaseRailBlock)
}

/// `isStraight`：不能转弯的铁轨。
fn is_straight(state: BlockState) -> bool {
    field_bool(state.block(), "isStraight")
}

/// `RailShape.isSlope`。
const fn is_slope(shape: RailShape) -> bool {
    matches!(
        shape,
        RailShape::AscendingNorth
            | RailShape::AscendingEast
            | RailShape::AscendingSouth
            | RailShape::AscendingWest
    )
}

/// `shouldBeRemoved`：脚下要有支撑，斜坡高的那一头也要有。
fn should_be_removed(world: &World, pos: BlockPos, shape: RailShape) -> bool {
    if !can_support_rigid_block(world, pos.below()) {
        return true;
    }
    let high_side = match shape {
        RailShape::AscendingEast => Direction::East,
        RailShape::AscendingWest => Direction::West,
        RailShape::AscendingNorth => Direction::North,
        RailShape::AscendingSouth => Direction::South,
        _ => return false,
    };
    !can_support_rigid_block(world, pos.relative(high_side))
}

/// `updateDir`。
fn update_dir(state: BlockState, world: &mut World, pos: BlockPos, first: bool) -> BlockState {
    let current = state.get(RAIL_SHAPE);
    let has_signal = world.has_neighbor_signal(pos);
    RailState::new(pos, state).place(world, has_signal, first, current)
}

/// `updateState(state, level, pos, movedByPiston)`：刚放下时和周围的铁轨连上。
fn connect_placed_rail(state: BlockState, world: &mut World, pos: BlockPos) -> BlockState {
    let updated = update_dir(state, world, pos, true);
    if is_straight(state) {
        world.neighbor_changed_with_state(updated, pos, state.block());
    }
    updated
}

/// `updateState(state, level, pos, block)`：`neighborChanged` 里每个子类不同的那一段。
fn neighbor_update(state: BlockState, world: &mut World, pos: BlockPos, source: &'static Block) {
    match state.class() {
        BlockClass::RailBlock => update_junction(state, world, pos, source),
        BlockClass::PoweredRailBlock => update_powered_rail(state, world, pos),
        _ => {}
    }
}

/// `RailBlock.updateState`：丁字路口旁边的信号源变了时，重新选一次方向。
fn update_junction(state: BlockState, world: &mut World, pos: BlockPos, source: &'static Block) {
    let source_is_signal = default_state(source).is_signal_source();
    if source_is_signal && RailState::new(pos, state).count_potential_connections(world) == 3 {
        update_dir(state, world, pos, false);
    }
}

/// `findPoweredRailSignal` 的搜索深度上限。
const MAX_RAIL_SEARCH_DEPTH: i32 = 8;

/// `findPoweredRailSignal` 里按形状走出的一步：偏移、折算成直线的形状、要不要再看低一格。
struct RailStep {
    offset: (i32, i32, i32),
    shape: RailShape,
    check_below: bool,
}

/// 顺着形状往 `forward` 那一头走一格。爬上斜坡的那一头不看低一格。
fn rail_step(shape: RailShape, forward: bool) -> RailStep {
    let flat = |offset, shape| RailStep {
        offset,
        shape,
        check_below: true,
    };
    let up = |offset, shape| RailStep {
        offset,
        shape,
        check_below: false,
    };
    match (shape, forward) {
        (RailShape::NorthSouth, true) => flat((0, 0, 1), RailShape::NorthSouth),
        (RailShape::NorthSouth, false) => flat((0, 0, -1), RailShape::NorthSouth),
        (RailShape::EastWest, true) => flat((-1, 0, 0), RailShape::EastWest),
        (RailShape::EastWest, false) => flat((1, 0, 0), RailShape::EastWest),
        (RailShape::AscendingEast, true) => flat((-1, 0, 0), RailShape::EastWest),
        (RailShape::AscendingEast, false) => up((1, 1, 0), RailShape::EastWest),
        (RailShape::AscendingWest, true) => up((-1, 1, 0), RailShape::EastWest),
        (RailShape::AscendingWest, false) => flat((1, 0, 0), RailShape::EastWest),
        (RailShape::AscendingNorth, true) => flat((0, 0, 1), RailShape::NorthSouth),
        (RailShape::AscendingNorth, false) => up((0, 1, -1), RailShape::NorthSouth),
        (RailShape::AscendingSouth, true) => up((0, 1, 1), RailShape::NorthSouth),
        (RailShape::AscendingSouth, false) => flat((0, 0, -1), RailShape::NorthSouth),
        // 动力铁轨只有直的形状，官方的 switch 在这里什么都不改，于是原地查一次。
        (curve, _) => flat((0, 0, 0), curve),
    }
}

/// `isSameRailWithPower` 里那两串条件：两段铁轨横着交叉就不算连着。
const fn crosses(dir: RailShape, shape: RailShape) -> bool {
    match dir {
        RailShape::EastWest => matches!(
            shape,
            RailShape::NorthSouth | RailShape::AscendingNorth | RailShape::AscendingSouth
        ),
        RailShape::NorthSouth => matches!(
            shape,
            RailShape::EastWest | RailShape::AscendingEast | RailShape::AscendingWest
        ),
        _ => false,
    }
}

/// `isSameRailWithPower`：同一种铁轨、方向对得上、通着电，还得自己有信号或者再往前找得到。
fn is_same_rail_with_power(
    world: &World,
    pos: BlockPos,
    block: &'static Block,
    forward: bool,
    depth: i32,
    dir: RailShape,
) -> bool {
    let state = world.block_state(pos);
    if !state.is_same_block(default_state(block)) {
        return false;
    }
    if crosses(dir, state.get(RAIL_SHAPE)) || !state.get(POWERED) {
        return false;
    }
    world.has_neighbor_signal(pos)
        || find_powered_rail_signal(world, pos, state, forward, depth + 1)
}

/// `findPoweredRailSignal`：顺着铁轨往一头找有信号的同类铁轨。
fn find_powered_rail_signal(
    world: &World,
    pos: BlockPos,
    state: BlockState,
    forward: bool,
    depth: i32,
) -> bool {
    if depth >= MAX_RAIL_SEARCH_DEPTH {
        return false;
    }
    let step = rail_step(state.get(RAIL_SHAPE), forward);
    let (dx, dy, dz) = step.offset;
    let next = pos.offset(dx, dy, dz);
    let block = state.block();
    is_same_rail_with_power(world, next, block, forward, depth, step.shape)
        || step.check_below
            && is_same_rail_with_power(world, next.below(), block, forward, depth, step.shape)
}

/// `PoweredRailBlock.updateState`。
fn update_powered_rail(state: BlockState, world: &mut World, pos: BlockPos) {
    let should_power = world.has_neighbor_signal(pos)
        || find_powered_rail_signal(world, pos, state, true, 0)
        || find_powered_rail_signal(world, pos, state, false, 0);
    if should_power == state.get(POWERED) {
        return;
    }
    world.set_block_and_update(pos, state.with(POWERED, should_power));
    world.update_neighbors_at(pos.below(), state.block());
    if is_slope(state.get(RAIL_SHAPE)) {
        world.update_neighbors_at(pos.above(), state.block());
    }
}

/// `DetectorRailBlock.checkPressed`：没有矿车，刚放下的铁轨也没有被压着，只剩通知比较器。
fn check_pressed(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.can_survive(world, pos) && world.block_state(pos).is_same_block(state) {
        world.update_neighbour_for_output_signal(pos, state.block());
    }
}

impl Behaviour for Rail {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let shape = if ctx.horizontal_direction().axis() == Axis::X {
            RailShape::EastWest
        } else {
            RailShape::NorthSouth
        };
        let water = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(RAIL_SHAPE, shape)
                .with(WATERLOGGED, water),
        )
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_support_rigid_block(world, pos.below())
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

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if old.is_same_block(state) {
            return;
        }
        let updated = connect_placed_rail(state, world, pos);
        if state.class() == BlockClass::DetectorRailBlock {
            check_pressed(updated, world, pos);
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_slope(state.get(RAIL_SHAPE)) {
            world.update_neighbors_at(pos.above(), state.block());
        }
        if is_straight(state) {
            world.update_neighbors_at(pos, state.block());
            world.update_neighbors_at(pos.below(), state.block());
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        if !world.block_state(pos).is_same_block(state) {
            return;
        }
        if should_be_removed(world, pos, state.get(RAIL_SHAPE)) {
            world.remove_block(pos);
        } else {
            neighbor_update(state, world, pos, source);
        }
    }

    /// `DetectorRailBlock.ownSignal`。另外两种铁轨不是信号源，连 `POWERED` 都没有。
    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        if state.class() == BlockClass::DetectorRailBlock && state.get(POWERED) {
            15
        } else {
            0
        }
    }

    /// `DetectorRailBlock.getDirectSignal`：只强充能上面那一格。
    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        if direction == Direction::Up {
            state.own_signal(world, pos)
        } else {
            0
        }
    }
}

/// `RailState`：一格铁轨和它连着的位置。
struct RailState {
    pos: BlockPos,
    state: BlockState,
    straight: bool,
    connections: Vec<BlockPos>,
}

/// 四个方向上有没有要连接的铁轨。
#[derive(Clone, Copy)]
struct Sides {
    north: bool,
    south: bool,
    west: bool,
    east: bool,
}

impl Sides {
    /// `connectTo` 和 `place` 都先看这四种弯道：正好两面有铁轨，而且不在一条线上。
    const fn curve(self) -> Option<RailShape> {
        match (self.north, self.south, self.west, self.east) {
            (false, true, false, true) => Some(RailShape::SouthEast),
            (false, true, true, false) => Some(RailShape::SouthWest),
            (true, false, true, false) => Some(RailShape::NorthWest),
            (true, false, false, true) => Some(RailShape::NorthEast),
            _ => None,
        }
    }

    /// 三面或者四面都有铁轨时选哪个弯：有信号时北边优先，没有信号时南边优先。
    fn junction_curve(self, has_signal: bool) -> Option<RailShape> {
        let curves = [
            (self.north && self.west, RailShape::NorthWest),
            (self.north && self.east, RailShape::NorthEast),
            (self.south && self.west, RailShape::SouthWest),
            (self.south && self.east, RailShape::SouthEast),
        ];
        let found = |&(present, shape): &(bool, RailShape)| present.then_some(shape);
        if has_signal {
            curves.iter().find_map(found)
        } else {
            curves.iter().rev().find_map(found)
        }
    }
}

/// `place` 和 `connectTo` 的最后一步：平路旁边高一格有铁轨时变成斜坡。
fn sloped(world: &World, pos: BlockPos, shape: Option<RailShape>) -> Option<RailShape> {
    let rail_above = |side: BlockPos| is_rail(world.block_state(side.above()));
    let mut shape = shape;
    if shape == Some(RailShape::NorthSouth) {
        if rail_above(pos.north()) {
            shape = Some(RailShape::AscendingNorth);
        }
        if rail_above(pos.south()) {
            shape = Some(RailShape::AscendingSouth);
        }
    }
    if shape == Some(RailShape::EastWest) {
        if rail_above(pos.east()) {
            shape = Some(RailShape::AscendingEast);
        }
        if rail_above(pos.west()) {
            shape = Some(RailShape::AscendingWest);
        }
    }
    shape
}

/// `getRail`：这一格、上面一格、下面一格里的铁轨。
fn rail_at(world: &World, pos: BlockPos) -> Option<RailState> {
    [pos, pos.above(), pos.below()].into_iter().find_map(|at| {
        let state = world.block_state(at);
        is_rail(state).then(|| RailState::new(at, state))
    })
}

impl RailState {
    fn new(pos: BlockPos, state: BlockState) -> Self {
        let mut rail = Self {
            pos,
            state,
            straight: is_straight(state),
            connections: Vec::with_capacity(4),
        };
        rail.update_connections(state.get(RAIL_SHAPE));
        rail
    }

    /// `updateConnections`。
    fn update_connections(&mut self, shape: RailShape) {
        let pos = self.pos;
        let ends = match shape {
            RailShape::NorthSouth => [pos.north(), pos.south()],
            RailShape::EastWest => [pos.west(), pos.east()],
            RailShape::AscendingEast => [pos.west(), pos.east().above()],
            RailShape::AscendingWest => [pos.west().above(), pos.east()],
            RailShape::AscendingNorth => [pos.north().above(), pos.south()],
            RailShape::AscendingSouth => [pos.north(), pos.south().above()],
            RailShape::SouthEast => [pos.east(), pos.south()],
            RailShape::SouthWest => [pos.west(), pos.south()],
            RailShape::NorthWest => [pos.west(), pos.north()],
            RailShape::NorthEast => [pos.east(), pos.north()],
        };
        self.connections.clear();
        self.connections.extend(ends);
    }

    /// `removeSoftConnections`：只留下真的连着的铁轨，位置换成铁轨实际所在的高度。
    fn remove_soft_connections(&mut self, world: &World) {
        let mut index = 0;
        while index < self.connections.len() {
            match rail_at(world, self.connections[index]) {
                Some(rail) if rail.connects_to(self) => {
                    self.connections[index] = rail.pos;
                    index += 1;
                }
                _ => {
                    self.connections.remove(index);
                }
            }
        }
    }

    /// `connectsTo`。
    fn connects_to(&self, rail: &Self) -> bool {
        self.has_connection(rail.pos)
    }

    /// `hasConnection`：只比水平位置，斜坡两头的高度不同。
    fn has_connection(&self, pos: BlockPos) -> bool {
        self.connections
            .iter()
            .any(|connection| connection.x == pos.x && connection.z == pos.z)
    }

    /// `countPotentialConnections`。
    fn count_potential_connections(&self, world: &World) -> usize {
        let has_rail = |pos: BlockPos| {
            [pos, pos.above(), pos.below()]
                .into_iter()
                .any(|at| is_rail(world.block_state(at)))
        };
        Direction::HORIZONTAL
            .into_iter()
            .filter(|&direction| has_rail(self.pos.relative(direction)))
            .count()
    }

    /// `canConnectTo`。
    fn can_connect_to(&self, rail: &Self) -> bool {
        self.connects_to(rail) || self.connections.len() != 2
    }

    /// `connectTo`。
    fn connect_to(&mut self, world: &mut World, rail: &Self) {
        self.connections.push(rail.pos);
        let shape = self.shape_of_connections(world);
        self.state = self.state.with(RAIL_SHAPE, shape);
        world.set_block_and_update(self.pos, self.state);
    }

    fn shape_of_connections(&self, world: &World) -> RailShape {
        let pos = self.pos;
        let sides = Sides {
            north: self.has_connection(pos.north()),
            south: self.has_connection(pos.south()),
            west: self.has_connection(pos.west()),
            east: self.has_connection(pos.east()),
        };
        let mut shape = None;
        if sides.north || sides.south {
            shape = Some(RailShape::NorthSouth);
        }
        if sides.west || sides.east {
            shape = Some(RailShape::EastWest);
        }
        if !self.straight {
            shape = sides.curve().or(shape);
        }
        sloped(world, pos, shape).unwrap_or(RailShape::NorthSouth)
    }

    /// `hasNeighborRail`。
    fn has_neighbor_rail(&self, world: &World, pos: BlockPos) -> bool {
        let Some(mut neighbor) = rail_at(world, pos) else {
            return false;
        };
        neighbor.remove_soft_connections(world);
        neighbor.can_connect_to(self)
    }

    /// `place`：按周围的铁轨选形状，写进世界，再让连着的铁轨转过来。返回最后的状态。
    fn place(
        mut self,
        world: &mut World,
        has_signal: bool,
        first: bool,
        default_shape: RailShape,
    ) -> BlockState {
        let pos = self.pos;
        let sides = Sides {
            north: self.has_neighbor_rail(world, pos.north()),
            south: self.has_neighbor_rail(world, pos.south()),
            west: self.has_neighbor_rail(world, pos.west()),
            east: self.has_neighbor_rail(world, pos.east()),
        };
        let shape = self.shape_of_sides(sides, has_signal, default_shape);
        let shape = sloped(world, pos, shape).unwrap_or(default_shape);
        self.update_connections(shape);
        self.state = self.state.with(RAIL_SHAPE, shape);
        if first || world.block_state(pos) != self.state {
            world.set_block_and_update(pos, self.state);
            self.turn_connected_rails(world);
        }
        self.state
    }

    fn shape_of_sides(
        &self,
        sides: Sides,
        has_signal: bool,
        default_shape: RailShape,
    ) -> Option<RailShape> {
        let north_or_south = sides.north || sides.south;
        let west_or_east = sides.west || sides.east;
        let mut shape = None;
        if north_or_south && !west_or_east {
            shape = Some(RailShape::NorthSouth);
        }
        if west_or_east && !north_or_south {
            shape = Some(RailShape::EastWest);
        }
        if !self.straight {
            shape = sides.curve().or(shape);
        }
        if shape.is_some() {
            return shape;
        }
        if north_or_south && west_or_east {
            shape = Some(default_shape);
        }
        if !self.straight {
            shape = sides.junction_curve(has_signal).or(shape);
        }
        shape
    }

    fn turn_connected_rails(&self, world: &mut World) {
        for &connection in &self.connections {
            let Some(mut neighbor) = rail_at(world, connection) else {
                continue;
            };
            neighbor.remove_soft_connections(world);
            if neighbor.can_connect_to(self) {
                neighbor.connect_to(world, self);
            }
        }
    }
}
