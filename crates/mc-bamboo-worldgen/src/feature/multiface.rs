//! `MultifaceGrowthFeature`：发光地衣、幽匿脉络这类多面生长方块。
//!
//! 官方先洗牌可放置的面（`Util.shuffledCopy`，Fisher-Yates，每步一次 `nextInt`），
//! 在原点试着贴一片；失败后按洗好的面顺序向外扫描 `search_range` 格，每格再洗一次
//! 「除反方向外的面」。贴成功后按 `chance_of_spreading` 判一次 `nextFloat`，命中就调
//! `MultifaceSpreader.spreadFromFaceTowardRandomDirection`，那里又会洗一次六个方向。
//! 随机数的取数次数和顺序必须照抄。
//!
//! `MultifaceGrowthFeature` for glow lichen and sculk veins. The official code shuffles
//! the placeable faces (`Util.shuffledCopy`, Fisher-Yates with one `nextInt` per step),
//! tries the origin, then scans outward `search_range` blocks re-shuffling the faces
//! minus the opposite direction at every step. A successful placement rolls one
//! `nextFloat` for `chance_of_spreading` and, on a hit, calls
//! `MultifaceSpreader.spreadFromFaceTowardRandomDirection`, which shuffles all six
//! directions again. The draw count and order are part of the contract.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{
    block_by_name, block_of_state, collision_shape, facts, registries, state_facts, state_fluid,
    support_shape,
};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{state::with_property, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `Direction.values()` 的顺序（`Direction::ALL`）。
/// `Direction.values()` order, i.e. [`Direction::ALL`].
const DIRECTIONS: [Direction; 6] = Direction::ALL;

/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`：同一格、同一平面、绕过去。
/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`: same position, same plane, wrap around.
const SPREAD_TYPES: [SpreadType; 3] = [
    SpreadType::SamePosition,
    SpreadType::SamePlane,
    SpreadType::WrapAround,
];

/// `BlockPos.relative(direction)` 的位移。
/// The block offset of `BlockPos.relative(direction)`.
fn offset(direction: Direction) -> (i32, i32, i32) {
    match direction {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// `MultifaceBlock.getFaceProperty(direction)` 的属性名。
/// Property name of `MultifaceBlock.getFaceProperty(direction)`.
fn face_property(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

/// `MultifaceBlock.hasFace(state, face)`。
/// `MultifaceBlock.hasFace(state, face)`.
fn has_face(state: u32, face: Direction) -> bool {
    with_property(state, face_property(face), "false").is_some_and(|without| without != state)
}

/// `MultifaceBlock.canAttachTo`：邻居朝回来那一面的支撑形状或碰撞形状是满的。
/// `MultifaceBlock.canAttachTo`: the neighbour's support or collision shape is full
/// toward the opposite face.
fn can_attach_to(state: u32, face: Direction) -> bool {
    let toward = face.opposite();
    mc_bamboo_blocks::shape::is_face_full(support_shape(state), toward)
        || mc_bamboo_blocks::shape::is_face_full(collision_shape(state), toward)
}

/// `BlockState.is(Blocks.WATER)`：任意含水量的水方块。
/// `BlockState.is(Blocks.WATER)`: a water block at any level.
fn is_water_block(state: u32) -> bool {
    block_of_state(state).is_some_and(|block| block.name == "minecraft:water")
}

/// `FluidState.isSourceOfType(Fluids.WATER)`：流体是水且是源。
/// `FluidState.isSourceOfType(Fluids.WATER)`: the fluid is water and a source.
fn is_water_source(state: u32) -> bool {
    let fluid = state_fluid(state);
    fluid.source && registries::fluid::WATER == fluid.fluid
}

/// `isAirOrWater`。
/// `isAirOrWater`.
fn is_air_or_water(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0 || is_water_block(state)
}

/// `MultifaceSpreader.SpreadType`。
/// `MultifaceSpreader.SpreadType`.
#[derive(Clone, Copy)]
enum SpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

impl SpreadType {
    /// `SpreadType.getSpreadPos(pos, spreadDirection, fromFace)`。
    /// `SpreadType.getSpreadPos(pos, spreadDirection, fromFace)`.
    fn spread_pos(self, pos: Pos, spread: Direction, from_face: Direction) -> (Pos, Direction) {
        let (sx, sy, sz) = offset(spread);
        match self {
            Self::SamePosition => (pos, spread),
            Self::SamePlane => ((pos.0 + sx, pos.1 + sy, pos.2 + sz), from_face),
            Self::WrapAround => {
                let (fx, fy, fz) = offset(from_face);
                (
                    (pos.0 + sx + fx, pos.1 + sy + fy, pos.2 + sz + fz),
                    spread.opposite(),
                )
            }
        }
    }
}

/// 一个已解析的 `minecraft:multiface_growth`。
/// A parsed `minecraft:multiface_growth`.
pub struct MultifaceGrowthFeature {
    place_block: u32,
    place_block_id: usize,
    search_range: i32,
    can_place_on_floor: bool,
    can_place_on_ceiling: bool,
    can_place_on_wall: bool,
    chance_of_spreading: f32,
    can_be_placed_on: Vec<i32>,
}

impl MultifaceGrowthFeature {
    /// 按官方 codec 解析；缺省值和 `MapCodec` 里的一致。
    ///
    /// Parse by the official codec; defaults match the `MapCodec`.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let block = block_by_name(object.get("block")?.as_str()?)?;
        let can_be_placed_on = object
            .get("can_be_placed_on")?
            .as_array()?
            .iter()
            .map(|value| {
                let name = value.as_str()?;
                let block = block_by_name(name)?;
                Some(mc_bamboo_data::block_id_of_state(block.default_state)? as i32)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            place_block: block.default_state,
            place_block_id: mc_bamboo_data::block_id_of_state(block.default_state)?,
            search_range: object
                .get("search_range")
                .and_then(Value::as_i64)
                .unwrap_or(10) as i32,
            can_place_on_floor: object
                .get("can_place_on_floor")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            can_place_on_ceiling: object
                .get("can_place_on_ceiling")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            can_place_on_wall: object
                .get("can_place_on_wall")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            chance_of_spreading: object
                .get("chance_of_spreading")
                .and_then(Value::as_f64)
                .unwrap_or(0.5) as f32,
            can_be_placed_on,
        })
    }

    /// `Util.shuffledCopy(list, random)`：Fisher-Yates，`i` 从 size 递减到 2，
    /// 每次 `nextInt(i)`；大小 0 或 1 时不消费随机数。
    ///
    /// `Util.shuffledCopy(list, random)`: Fisher-Yates from size down to 2 with one
    /// `nextInt(i)` each; sizes 0 and 1 consume nothing.
    fn shuffled(list: &[Direction], random: &WorldgenRandom) -> Vec<Direction> {
        let mut out = list.to_vec();
        for i in (2..=out.len()).rev() {
            let swap_to = random.next_int(i as i32) as usize;
            out.swap(i - 1, swap_to);
        }
        out
    }

    /// `validDirections()`：天花板、地板、四个墙面，按官方加入顺序。
    /// `validDirections()`: ceiling, floor, then the four walls, in official add order.
    fn valid_directions(&self) -> Vec<Direction> {
        let mut out = Vec::with_capacity(6);
        if self.can_place_on_ceiling {
            out.push(Direction::Up);
        }
        if self.can_place_on_floor {
            out.push(Direction::Down);
        }
        if self.can_place_on_wall {
            out.extend(Direction::HORIZONTAL);
        }
        out
    }

    /// `getShuffledDirectionsExcept(random, exclude)`。
    /// `getShuffledDirectionsExcept(random, exclude)`.
    fn shuffled_except(&self, random: &WorldgenRandom, exclude: Direction) -> Vec<Direction> {
        let filtered: Vec<Direction> = self
            .valid_directions()
            .into_iter()
            .filter(|&direction| direction != exclude)
            .collect();
        Self::shuffled(&filtered, random)
    }

    /// 状态是否属于 `place_block` 那个方块。
    /// Whether the state belongs to the `place_block` block.
    fn is_own_block(&self, state: u32) -> bool {
        mc_bamboo_data::block_id_of_state(state) == Some(self.place_block_id)
    }

    /// 一个候选位置，返回官方 `placed`。
    ///
    /// One candidate; returns the official `placed` flag.
    pub fn place(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, pos: Pos) -> bool {
        if !is_air_or_water(world.get_block(pos.0, pos.1, pos.2)) {
            return false;
        }
        let directions = Self::shuffled(&self.valid_directions(), random);
        let old = world.get_block(pos.0, pos.1, pos.2);
        if self.place_if_possible(world, pos, old, random, &directions) {
            return true;
        }

        for search in directions {
            // `placeGrowthIfPossible` 的候选方向只算一次，内层循环复用同一份。
            // The placement directions are shuffled once per search direction.
            let placement = self.shuffled_except(random, search.opposite());
            let (dx, dy, dz) = offset(search);
            let probe = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
            for _ in 0..self.search_range {
                let state = world.get_block(probe.0, probe.1, probe.2);
                // `!isAirOrWater(state) && !state.is(this.placeBlock)` 就停。
                // Stop when the probe is neither air/water nor the growth block itself.
                if !is_air_or_water(state) && !self.is_own_block(state) {
                    break;
                }
                if self.place_if_possible(world, probe, state, random, &placement) {
                    return true;
                }
                // 官方每轮都 `pos.setWithOffset(origin, searchDirection)`，探测位置恒为
                // 「原点 + 一步」，`search_range` 只是在同一格重复尝试。
                // The official loop re-applies `setWithOffset(origin, direction)` every
                // iteration, so the probe is always origin + one step; `search_range`
                // only repeats attempts on that same block.
            }
        }
        false
    }

    /// `placeGrowthIfPossible`。注意官方在 `getStateForPlacement` 返回 null 时**直接
    /// 返回 false**，不再试下一个方向。
    ///
    /// `placeGrowthIfPossible`. Note the official code returns false outright when
    /// `getStateForPlacement` yields null instead of trying the next direction.
    fn place_if_possible(
        &self,
        world: &mut FeatureWorld<'_>,
        pos: Pos,
        old: u32,
        random: &WorldgenRandom,
        directions: &[Direction],
    ) -> bool {
        for &face in directions {
            let (dx, dy, dz) = offset(face);
            let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
            let Some(block_id) = mc_bamboo_data::block_id_of_state(neighbour) else {
                continue;
            };
            if !self.can_be_placed_on.contains(&(block_id as i32)) {
                continue;
            }
            let Some(state) = self.state_for_placement(world, old, pos, face) else {
                return false;
            };
            // `setBlockAndUpdate` == `setBlock(pos, state, 3)`，走 `ChunkAccess.setBlockState`。
            // `setBlockAndUpdate` is `setBlock(pos, state, 3)`, i.e. `ChunkAccess.setBlockState`.
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
            if random.next_float() < self.chance_of_spreading {
                self.spread(world, pos, state, face, random);
            }
            return true;
        }
        false
    }

    /// `MultifaceBlock.getStateForPlacement(oldState, level, pos, direction)`。
    /// `MultifaceBlock.getStateForPlacement(oldState, level, pos, direction)`.
    fn state_for_placement(
        &self,
        world: &FeatureWorld<'_>,
        old: u32,
        pos: Pos,
        face: Direction,
    ) -> Option<u32> {
        if !self.is_valid_state_for_placement(world, old, pos, face) {
            return None;
        }
        let base = if self.is_own_block(old) {
            old
        } else if is_water_source(old) {
            with_property(self.place_block, "waterlogged", "true").unwrap_or(self.place_block)
        } else {
            self.place_block
        };
        with_property(base, face_property(face), "true")
    }

    /// `MultifaceBlock.isValidStateForPlacement`：这个面支持、旧状态还没有这个面、
    /// 且邻居能贴住。
    ///
    /// `MultifaceBlock.isValidStateForPlacement`: the face is supported, the old state
    /// lacks it, and the neighbour can hold it.
    fn is_valid_state_for_placement(
        &self,
        world: &FeatureWorld<'_>,
        old: u32,
        pos: Pos,
        face: Direction,
    ) -> bool {
        if self.is_own_block(old) && has_face(old, face) {
            return false;
        }
        let (dx, dy, dz) = offset(face);
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        can_attach_to(neighbour, face)
    }

    /// `DefaultSpreaderConfig.stateCanBeReplaced`：空气、同类方块，
    /// 或水方块且流体是源。
    /// `DefaultSpreaderConfig.stateCanBeReplaced`: air, the same block, or a water block
    /// whose fluid is a source.
    fn state_can_be_replaced(&self, state: u32) -> bool {
        state_facts(state) & facts::AIR != 0
            || self.is_own_block(state)
            || (is_water_block(state) && is_water_source(state))
    }

    /// `DefaultSpreaderConfig.canSpreadInto`。
    /// `DefaultSpreaderConfig.canSpreadInto`.
    fn can_spread_into(&self, world: &FeatureWorld<'_>, spread: (Pos, Direction)) -> bool {
        let (pos, face) = spread;
        let existing = world.get_block(pos.0, pos.1, pos.2);
        self.state_can_be_replaced(existing)
            && self.is_valid_state_for_placement(world, existing, pos, face)
    }

    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection`：按洗好的六方向顺序找
    /// 第一个能铺的位置，铺一次就返回。
    ///
    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection`: use the shuffled six
    /// directions to find the first acceptable spread and place at most one block.
    fn spread(
        &self,
        world: &mut FeatureWorld<'_>,
        pos: Pos,
        state: u32,
        from_face: Direction,
        random: &WorldgenRandom,
    ) {
        // 不管后面选中哪个方向，`Direction.allShuffled` 都先消费 5 次 nextInt。
        // `Direction.allShuffled` always consumes five nextInt draws first.
        let order = Self::shuffled(&DIRECTIONS, random);
        for spread_direction in order {
            // `spreadDirection.getAxis() == startingFace.getAxis()` 直接跳过。
            // Skip a spread direction on the same axis as the starting face.
            if spread_direction.axis() == from_face.axis() {
                continue;
            }
            // `config.hasFace(state, startingFace) && !config.hasFace(state, spreadDirection)`。
            // Require the starting face and the absence of the spread face.
            if !has_face(state, from_face) || has_face(state, spread_direction) {
                continue;
            }
            let Some((target_pos, target_face)) =
                self.spread_target(world, pos, spread_direction, from_face)
            else {
                continue;
            };
            let old = world.get_block(target_pos.0, target_pos.1, target_pos.2);
            if let Some(new_state) = self.state_for_placement(world, old, target_pos, target_face) {
                world.set_block_with_heightmaps(
                    target_pos.0,
                    target_pos.1,
                    target_pos.2,
                    new_state,
                );
            }
            return;
        }
    }

    /// `getSpreadFromFaceTowardDirection` 里按 `getSpreadTypes()` 找第一个能铺的位置。
    /// The first spread position accepted by `getSpreadTypes()` inside
    /// `getSpreadFromFaceTowardDirection`.
    fn spread_target(
        &self,
        world: &FeatureWorld<'_>,
        pos: Pos,
        spread: Direction,
        from_face: Direction,
    ) -> Option<(Pos, Direction)> {
        for kind in SPREAD_TYPES {
            let candidate = kind.spread_pos(pos, spread, from_face);
            if self.can_spread_into(world, candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn glow_lichen() -> Value {
        json!({
            "block": "minecraft:glow_lichen",
            "can_be_placed_on": ["minecraft:stone", "minecraft:deepslate"],
            "can_place_on_ceiling": true,
            "can_place_on_wall": true,
            "search_range": 20,
            "type": "minecraft:multiface_growth",
        })
    }

    #[test]
    fn parses_glow_lichen_and_its_direction_order() {
        let feature = MultifaceGrowthFeature::parse(&glow_lichen()).unwrap();
        assert_eq!(feature.search_range, 20);
        assert!(feature.can_place_on_ceiling);
        assert!(feature.can_place_on_wall);
        assert!(!feature.can_place_on_floor);
        assert_eq!(feature.chance_of_spreading, 0.5);
        // 天花板 + 四个墙面，顺序是 UP 后接 NORTH/EAST/SOUTH/WEST。
        // Ceiling plus four walls: UP followed by NORTH/EAST/SOUTH/WEST.
        assert_eq!(
            feature.valid_directions(),
            vec![
                Direction::Up,
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West
            ]
        );
    }

    #[test]
    fn sculk_vein_adds_the_floor_and_full_spreading() {
        // sculk_vein 有 `can_place_on_floor` 和 `chance_of_spreading: 1`。
        // sculk_vein sets `can_place_on_floor` and `chance_of_spreading: 1`.
        let config: Value = json!({
            "block": "minecraft:sculk_vein",
            "can_be_placed_on": ["minecraft:stone"],
            "can_place_on_ceiling": true,
            "can_place_on_floor": true,
            "can_place_on_wall": true,
            "chance_of_spreading": 1,
            "search_range": 20,
            "type": "minecraft:multiface_growth",
        });
        let feature = MultifaceGrowthFeature::parse(&config).unwrap();
        assert_eq!(feature.valid_directions().len(), 6);
        assert_eq!(feature.chance_of_spreading, 1.0);
    }

    #[test]
    fn face_properties_round_trip() {
        let state =
            crate::feature::state::parse_state(&json!({"id": "minecraft:glow_lichen"})).unwrap();
        assert!(!has_face(state, Direction::Up));
        let up = with_property(state, "up", "true").unwrap();
        assert!(has_face(up, Direction::Up));
        assert!(!has_face(up, Direction::Down));
    }
}
