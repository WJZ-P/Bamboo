//! `MultifaceGrowthFeature`：发光地衣、幽匿脉络这类「多面生长」方块。
//!
//! 官方 `MultifaceGrowthFeature.place`：原点必须是空气或水；先按随机顺序尝试六个方向
//! 直接放；失败后沿每个搜索方向走 `search_range` 步，每步用「除反方向外」的随机方向表
//! 再试一次。放置成功后有 `chance_of_spreading` 的概率触发 `MultifaceSpreader`，
//! 按 `SAME_POSITION` / `SAME_PLANE` / `WRAP_AROUND` 三种扩散类型往邻近的面上再长一格。
//!
//! Official `MultifaceGrowthFeature.place`: the origin must be air or water. It first
//! tries the six directions in shuffled order; on failure it walks up to `search_range`
//! blocks along each search direction, retrying with the other directions shuffled. A
//! successful placement spreads with probability `chance_of_spreading` via
//! `MultifaceSpreader` (`SAME_POSITION` / `SAME_PLANE` / `WRAP_AROUND`).

use serde_json::Value;

use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_tag, collision_shape, state_facts, state_fluid,
    support_shape,
};

use crate::random::WorldgenRandom;

use super::{state::with_property, vines::face_is_full, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `Direction.values()` 的六个方向，顺序即 ordinal。
///
/// The six `Direction.values()` entries in ordinal order.
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// 已解析的 `MultifaceGrowthFeature` 配置。
pub struct MultifaceGrowthFeature {
    block: u32,
    search_range: i32,
    can_place_on_floor: bool,
    can_place_on_ceiling: bool,
    can_place_on_wall: bool,
    chance_of_spreading: f32,
    can_be_placed_on: Vec<i32>,
}

impl MultifaceGrowthFeature {
    /// 解析配置；所有可放置方块都必须是纯数据（标签已展开成 ID）。
    ///
    /// Parse the config; the acceptable-block set must be plain block IDs.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let block = block_by_name(object.get("block")?.as_str()?)?.default_state;
        let search_range = object
            .get("search_range")
            .and_then(Value::as_i64)
            .unwrap_or(10) as i32;
        let can_be_placed_on = parse_blocks(object.get("can_be_placed_on")?)?;
        Some(Self {
            block,
            search_range,
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

    /// 官方 `MultifaceGrowthFeature.place`：一个候选位置。
    ///
    /// Official `MultifaceGrowthFeature.place` at one candidate.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        let origin_state = world.get_block(origin.0, origin.1, origin.2);
        if !self.is_air_or_water(origin_state) {
            return false;
        }
        // `getShuffledDirections`：`Util.shuffledCopy(validDirections(), random)`。
        // `getShuffledDirections`: `Util.shuffledCopy(validDirections(), random)`.
        let search_directions = shuffle(&self.valid_directions(), random);
        if self.place_growth_if_possible(world, origin, origin_state, random, &search_directions) {
            return true;
        }
        for search in &search_directions {
            let placement_directions = shuffle(&self.directions_except(opposite(search)), random);
            for _ in 0..self.search_range {
                // 官方循环里每次都是 `pos.setWithOffset(origin, searchDirection)`，会重置到
                // `origin + searchDirection`（不是累加）。这是官方的实际行为，照抄。
                // The official loop calls `pos.setWithOffset(origin, searchDirection)` every
                // iteration, which resets to `origin + searchDirection` instead of advancing.
                // That is the official behaviour; copy it.
                let pos = offset(origin, search);
                let state = world.get_block(pos.0, pos.1, pos.2);
                // `!isAirOrWater(state) && !state.is(placeBlock)` 时停止这一方向。
                // Stop this direction when the block is neither air/water nor the growth.
                if !self.is_air_or_water(state) && !is_same_block(state, self.block) {
                    break;
                }
                if self.place_growth_if_possible(world, pos, state, random, &placement_directions) {
                    return true;
                }
            }
        }
        false
    }

    /// 官方 `placeGrowthIfPossible`：按给定方向表逐个看邻居是否可放置。
    ///
    /// Official `placeGrowthIfPossible`: walk the direction list and place on the first
    /// acceptable neighbour.
    fn place_growth_if_possible(
        &self,
        world: &mut FeatureWorld<'_>,
        pos: Pos,
        old_state: u32,
        random: &WorldgenRandom,
        directions: &[&'static str],
    ) -> bool {
        for &direction in directions {
            let neighbour = offset(pos, direction);
            let neighbour_state = world.get_block(neighbour.0, neighbour.1, neighbour.2);
            if !self
                .can_be_placed_on
                .contains(&block_id_of(neighbour_state))
            {
                continue;
            }
            let Some(new_state) = self.state_for_placement(old_state, world, pos, direction) else {
                return false;
            };
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, new_state);
            if random.next_float() < self.chance_of_spreading {
                self.spread(world, pos, new_state, direction, random);
            }
            return true;
        }
        false
    }

    /// `MultifaceBlock.getStateForPlacement(oldState, level, placementPos, placementDirection)`：
    /// 该面必须可支撑且当前没被占；oldState 已经是这种方块时沿用，是水源时带上
    /// `waterlogged=true`，否则用默认状态；最后把那一面设为 true。
    ///
    /// `MultifaceBlock.getStateForPlacement`: the face must be supported and not already
    /// present; keep `oldState` when it already is this block, mark waterlogged for a water
    /// source, else start from the default state, then set the face property.
    fn state_for_placement(
        &self,
        old_state: u32,
        world: &FeatureWorld<'_>,
        placement_pos: Pos,
        direction: &str,
    ) -> Option<u32> {
        if !self.is_valid_state_for_placement(world, old_state, placement_pos, direction) {
            return None;
        }
        let base = if is_same_block(old_state, self.block) {
            old_state
        } else if is_water_source(old_state) {
            with_property(self.block, "waterlogged", "true")?
        } else {
            self.block
        };
        with_property(base, direction, "true")
    }

    /// `MultifaceBlock.isValidStateForPlacement`：`isFaceSupported(direction)`、当前那面
    /// 还没被占，且邻居在反方向上是满面。
    ///
    /// `MultifaceBlock.isValidStateForPlacement`: the face is supported, not yet present,
    /// and the neighbour is full on the opposite face.
    fn is_valid_state_for_placement(
        &self,
        world: &FeatureWorld<'_>,
        old_state: u32,
        placement_pos: Pos,
        direction: &str,
    ) -> bool {
        if is_same_block(old_state, self.block)
            && with_property(old_state, direction, "true") == Some(old_state)
        {
            return false;
        }
        let neighbour = offset(placement_pos, direction);
        let neighbour_state = world.get_block(neighbour.0, neighbour.1, neighbour.2);
        can_attach_to(neighbour_state, opposite(direction))
    }

    /// `DefaultSpreaderConfig.canSpreadInto`：目标格可以替换，且新状态合法。
    ///
    /// `DefaultSpreaderConfig.canSpreadInto`: the target is replaceable and the new state is
    /// valid.
    fn can_spread_into(&self, world: &FeatureWorld<'_>, spread: SpreadPos) -> bool {
        let existing = world.get_block(spread.pos.0, spread.pos.1, spread.pos.2);
        let replaceable = state_facts(existing) & facts_air() != 0
            || is_same_block(existing, self.block)
            || is_water_source(existing);
        replaceable
            && self
                .state_for_placement(existing, world, spread.pos, spread.face)
                .is_some()
    }

    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection`：起始面是**刚放置用的那个
    /// 面**（`placeGrowthIfPossible` 传进来的 `placementDirection`），只把六个**扩散方向**
    /// 洗一次牌，逐个 `spreadFromFaceTowardDirection`，第一个成功的就返回。
    ///
    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection`: the starting face is the
    /// face just placed on (`placementDirection` from `placeGrowthIfPossible`); only the six
    /// **spread directions** are shuffled, and the first success returns.
    fn spread(
        &self,
        world: &mut FeatureWorld<'_>,
        pos: Pos,
        state: u32,
        starting_face: &'static str,
        random: &WorldgenRandom,
    ) {
        for spread_direction in shuffle(&DIRECTIONS, random) {
            if self.spread_from_face_toward_direction(
                world,
                pos,
                state,
                starting_face,
                spread_direction,
            ) {
                return;
            }
        }
    }

    /// `MultifaceSpreader.spreadFromFaceTowardDirection` +
    /// `getSpreadFromFaceTowardDirection`：扩散方向不能和起始面同轴；起始面必须在已放状态上
    /// （且目标面还没被占），然后按 `SAME_POSITION` / `SAME_PLANE` / `WRAP_AROUND` 顺序找
    /// 第一个可以放的位置。
    ///
    /// `MultifaceSpreader.spreadFromFaceTowardDirection` + `getSpreadFromFaceTowardDirection`:
    /// the spread axis must differ from the starting face's axis, the starting face must be
    /// present (and the target face absent), then try `SAME_POSITION`, `SAME_PLANE` and
    /// `WRAP_AROUND` in order.
    fn spread_from_face_toward_direction(
        &self,
        world: &mut FeatureWorld<'_>,
        pos: Pos,
        state: u32,
        starting_face: &'static str,
        spread_direction: &'static str,
    ) -> bool {
        if axis(spread_direction) == axis(starting_face) {
            return false;
        }
        // `hasFace(state, startingFace) && !hasFace(state, spreadDirection)`。
        // `hasFace(state, startingFace) && !hasFace(state, spreadDirection)`.
        let has_start = with_property(state, starting_face, "true") == Some(state);
        let has_spread = with_property(state, spread_direction, "true") == Some(state);
        if !has_start || has_spread {
            return false;
        }
        for spread_type in SPREAD_ORDER {
            let spread = match spread_type {
                SpreadType::SamePosition => SpreadPos {
                    pos,
                    face: spread_direction,
                },
                SpreadType::SamePlane => SpreadPos {
                    pos: offset(pos, spread_direction),
                    face: starting_face,
                },
                SpreadType::WrapAround => SpreadPos {
                    pos: offset(offset(pos, spread_direction), starting_face),
                    face: opposite(spread_direction),
                },
            };
            if !self.can_spread_into(world, spread) {
                continue;
            }
            // 官方这里已经选定第一个通过 `canSpreadInto` 的位置，随后只尝试放一次；
            // 放不下（`getStateForPlacement` 返回 null）就整条方向失败，不再试后面的
            // `SpreadType`。
            // The official code commits to the first position passing `canSpreadInto` and
            // then places once; a null `getStateForPlacement` fails the whole direction
            // instead of falling through to the next `SpreadType`.
            let existing = world.get_block(spread.pos.0, spread.pos.1, spread.pos.2);
            let Some(new_state) =
                self.state_for_placement(existing, world, spread.pos, spread.face)
            else {
                return false;
            };
            world.set_block_with_heightmaps(spread.pos.0, spread.pos.1, spread.pos.2, new_state);
            return true;
        }
        false
    }

    /// 官方 `validDirections`：按 ceiling / floor / wall 的开关收集方向。
    ///
    /// Official `validDirections`: collect directions per the ceiling/floor/wall switches.
    fn valid_directions(&self) -> Vec<&'static str> {
        let mut directions = Vec::with_capacity(6);
        if self.can_place_on_ceiling {
            directions.push("up");
        }
        if self.can_place_on_floor {
            directions.push("down");
        }
        if self.can_place_on_wall {
            // `Direction.Plane.HORIZONTAL` 的顺序是 NORTH, EAST, SOUTH, WEST。
            // `Direction.Plane.HORIZONTAL` is ordered NORTH, EAST, SOUTH, WEST.
            directions.extend(["north", "east", "south", "west"]);
        }
        directions
    }

    /// `validDirections()` 去掉一个方向，用于搜索时的「除反方向外」。
    ///
    /// `validDirections()` minus one entry, used for the search phase.
    fn directions_except(&self, excluded: &str) -> Vec<&'static str> {
        self.valid_directions()
            .into_iter()
            .filter(|direction| *direction != excluded)
            .collect()
    }

    /// `isAirOrWater(state)`：空气或水方块。
    ///
    /// `isAirOrWater(state)`: air or water.
    fn is_air_or_water(&self, state: u32) -> bool {
        state_facts(state) & facts_air() != 0 || block_id_of(state) == water_block_id()
    }
}

/// `MultifaceSpreader.SpreadType` 的三种，顺序即 `DEFAULT_SPREAD_ORDER`。
#[derive(Debug, Clone, Copy)]
enum SpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

/// `DEFAULT_SPREAD_ORDER`。
const SPREAD_ORDER: [SpreadType; 3] = [
    SpreadType::SamePosition,
    SpreadType::SamePlane,
    SpreadType::WrapAround,
];

/// `MultifaceSpreader.SpreadPos`：目标位置加要占用的面。
#[derive(Debug, Clone, Copy)]
struct SpreadPos {
    pos: Pos,
    face: &'static str,
}

/// `facts::AIR`。
fn facts_air() -> u64 {
    mc_bamboo_data::facts::AIR
}

/// `Blocks.WATER` 的方块 ID。
fn water_block_id() -> i32 {
    block_by_name("minecraft:water").map_or(-1, |block| {
        block_id_of_state(block.default_state).map_or(-1, |id| id as i32)
    })
}

/// 状态所属方块的注册表 ID，范围外是 -1。
fn block_id_of(state: u32) -> i32 {
    block_id_of_state(state).map_or(-1, |id| id as i32)
}

/// 两个状态是否属于同一个方块。
///
/// Whether two states belong to the same block.
fn is_same_block(left: u32, block_state: u32) -> bool {
    block_id_of(left) == block_id_of(block_state)
}

/// `FluidState.isSourceOfType(Fluids.WATER)`。
fn is_water_source(state: u32) -> bool {
    let fluid = state_fluid(state);
    fluid.fluid == mc_bamboo_data::registries::fluid::WATER && fluid.source
}

/// `MultifaceBlock.canAttachTo(level, direction, neighbourPos, neighbourState)`：
/// 邻居的支撑形状或碰撞形状在 `direction.getOpposite()` 上是满的。
///
/// `MultifaceBlock.canAttachTo`: the neighbour's support or collision shape is full on the
/// opposite face.
fn can_attach_to(neighbour_state: u32, face: &str) -> bool {
    face_is_full(support_shape(neighbour_state), face)
        || face_is_full(collision_shape(neighbour_state), face)
}

/// `Direction.getOpposite()`。
fn opposite(direction: &str) -> &'static str {
    match direction {
        "up" => "down",
        "down" => "up",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    }
}

/// `Direction.getAxis()`。
fn axis(direction: &str) -> u8 {
    match direction {
        "up" | "down" => 1,
        "north" | "south" => 2,
        _ => 0,
    }
}

/// 方块坐标偏移。
fn offset(pos: Pos, direction: &str) -> Pos {
    match direction {
        "down" => (pos.0, pos.1 - 1, pos.2),
        "up" => (pos.0, pos.1 + 1, pos.2),
        "north" => (pos.0, pos.1, pos.2 - 1),
        "south" => (pos.0, pos.1, pos.2 + 1),
        "west" => (pos.0 - 1, pos.1, pos.2),
        _ => (pos.0 + 1, pos.1, pos.2),
    }
}

/// `Util.shuffle`：Fisher-Yates，从尾部往前，每步 `nextInt(i)`。
///
/// `Util.shuffle`: Fisher-Yates from the end, `nextInt(i)` each step.
fn shuffle<T: Clone>(items: &[T], random: &WorldgenRandom) -> Vec<T> {
    let mut list = items.to_vec();
    for i in (2..=list.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        list.swap(i - 1, swap_to);
    }
    list
}

/// `HolderSet<Block>`：方块名或数组，展开成方块注册表 ID。
///
/// `HolderSet<Block>`: a block name or an array, expanded to block registry IDs.
fn parse_blocks(value: &Value) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    match value {
        Value::String(name) => push_block(name, &mut out)?,
        Value::Array(items) => {
            for item in items {
                push_block(item.as_str()?, &mut out)?;
            }
        }
        _ => return None,
    }
    Some(out)
}

fn push_block(name: &str, out: &mut Vec<i32>) -> Option<()> {
    if let Some(tag) = name.strip_prefix('#') {
        out.extend_from_slice(block_tag(tag)?);
    } else {
        let block = block_by_name(name)?;
        out.push(block_id_of_state(block.default_state)? as i32);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_the_exported_glow_lichen_config() {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let config = &root["feature"]["minecraft:glow_lichen"];
        let feature = MultifaceGrowthFeature::parse(config).unwrap();
        assert_eq!(feature.search_range, 20);
        assert!(feature.can_place_on_ceiling);
        assert!(feature.can_place_on_wall);
        assert!(!feature.can_place_on_floor);
        assert_eq!(feature.can_be_placed_on.len(), 10);
        // 天花板 + 四面墙 = 5 个有效方向。
        // Ceiling plus four walls = five valid directions.
        assert_eq!(feature.valid_directions().len(), 5);
        assert_eq!(feature.directions_except("north").len(), 4);
    }

    #[test]
    fn shuffle_consumes_one_random_per_swap() {
        let random = WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(7));
        let shuffled = shuffle(&DIRECTIONS, &random);
        assert_eq!(shuffled.len(), 6);
        // 洗牌只换位置，集合不变。
        // Shuffling only reorders, the set stays the same.
        let mut sorted = shuffled.clone();
        sorted.sort_unstable();
        let mut expected = DIRECTIONS.to_vec();
        expected.sort_unstable();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn opposite_and_axis_match_the_official_tables() {
        assert_eq!(opposite("up"), "down");
        assert_eq!(opposite("east"), "west");
        assert_eq!(axis("up"), 1);
        assert_eq!(axis("north"), 2);
        assert_eq!(axis("east"), 0);
    }

    #[test]
    fn face_presence_matches_the_property_value() {
        // `hasFace(state, face)` 必须等价于「该面属性是 true」。
        // `hasFace(state, face)` must equal "the face property is true".
        use crate::feature::tree::property_value;
        let block = *mc_bamboo_data::block_by_name("minecraft:glow_lichen").unwrap();
        let faces = ["down", "east", "north", "south", "up", "west"];
        for bits in 0u32..(1 << 6) {
            let mut state = block.first_state;
            for (index, face) in faces.iter().enumerate() {
                let value = if bits & (1 << index) != 0 {
                    "true"
                } else {
                    "false"
                };
                state = with_property(state, face, value).unwrap();
            }
            for face in faces {
                let expected = property_value(state, face) == Some("true");
                let got = with_property(state, face, "true") == Some(state);
                assert_eq!(expected, got, "state {state} face {face}");
            }
        }
    }

    #[test]
    fn parses_block_name_arrays() {
        let value: Value = json!(["minecraft:stone", "minecraft:deepslate"]);
        assert_eq!(parse_blocks(&value).unwrap().len(), 2);
    }
}
