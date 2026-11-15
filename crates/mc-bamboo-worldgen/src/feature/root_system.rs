//! `RootSystemFeature`：`rooted_azalea_tree` / `rooted_sulfur_spring`。
//!
//! 官方逻辑：先向上找一棵能放下的树（`placeDirtAndTree`），成功后沿 origin 到树根铺
//! 一层 `root_state_provider`（`placeDirt`，每层 `root_placement_attempts` 次随机偏移），
//! 最后在 origin 周围撒 `hanging_root_placement_attempts` 个悬挂根（`placeRoots`）。
//!
//! 随机数顺序必须照抄：`placeDirtAndTree` 每层的 `Heightmap` 查询不取数；
//! `treeFeature.place` 取数；`placeDirt` 每层每尝试两次 `nextInt(rootRadius)`；
//! `placeRoots` 每次尝试三次 `nextInt`（x / y / z 各两次相减）。
//!
//! `RootSystemFeature`. The official flow: walk up looking for a spot that fits a tree
//! (`placeDirtAndTree`), then lay `root_state_provider` along the origin-to-trunk column
//! (`placeDirt`, `root_placement_attempts` random offsets per layer), and finally scatter
//! hanging roots around the origin (`placeRoots`). The draw order is part of the contract.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{block_id_of_state, block_tag, facts, state_facts, state_fluid};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
    placement::{PlaceOutcome, PlacementEnv},
    provider::IntProvider,
    registry::Registry,
    state_provider::StateProvider,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// 一个已解析的 `minecraft:root_system`。
/// A parsed `minecraft:root_system`.
pub struct RootSystem {
    tree_feature: Value,
    required_vertical_space_for_tree: i32,
    level_test_distance: i32,
    max_level_deviation: i32,
    root_radius: i32,
    root_replaceable: Vec<i32>,
    root_state_provider: StateProvider,
    root_placement_attempts: i32,
    root_column_max_height: i32,
    hanging_root_radius: i32,
    hanging_roots_vertical_span: i32,
    hanging_root_state_provider: StateProvider,
    hanging_root_placement_attempts: i32,
    allowed_vertical_water_for_tree: i32,
    allowed_tree_position: BlockPredicate,
}

impl RootSystem {
    /// 按官方 codec 解析；`root_state_provider` / `hanging_root_state_provider` 可以是
    /// 内联 bare state，也可以是字符串 holder（用 `lookup` 查）。
    ///
    /// Parse by the official codec; the two state providers may be inline bare states or
    /// string holders resolved through `lookup`.
    pub fn parse<'a>(
        config: &'a Value,
        lookup: &dyn Fn(&str) -> Option<&'a Value>,
    ) -> Option<Self> {
        let object = config.as_object()?;
        let integer = |name: &str| -> Option<i32> { Some(object.get(name)?.as_i64()? as i32) };
        let allowed_tree_position = BlockPredicate::parse(object.get("allowed_tree_position")?);
        if !allowed_tree_position.is_supported() {
            return None;
        }
        Some(Self {
            tree_feature: object.get("feature")?.clone(),
            required_vertical_space_for_tree: integer("required_vertical_space_for_tree")?,
            level_test_distance: integer("level_test_distance")?,
            max_level_deviation: integer("max_level_deviation")?,
            root_radius: integer("root_radius")?,
            root_replaceable: parse_block_set(object.get("root_replaceable")?)?,
            root_state_provider: StateProvider::parse_with(
                object.get("root_state_provider")?,
                lookup,
            )?,
            root_placement_attempts: integer("root_placement_attempts")?,
            root_column_max_height: integer("root_column_max_height")?,
            hanging_root_radius: integer("hanging_root_radius")?,
            hanging_roots_vertical_span: integer("hanging_roots_vertical_span")?,
            hanging_root_state_provider: StateProvider::parse_with(
                object.get("hanging_root_state_provider")?,
                lookup,
            )?,
            hanging_root_placement_attempts: integer("hanging_root_placement_attempts")?,
            allowed_vertical_water_for_tree: integer("allowed_vertical_water_for_tree")?,
            allowed_tree_position,
        })
    }

    /// `RootSystemFeature.place`。
    /// `RootSystemFeature.place`.
    pub fn place(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> PlaceOutcome {
        if !world.is_air(origin.0, origin.1, origin.2) {
            return PlaceOutcome {
                supported: true,
                placed: false,
            };
        }
        let working = self.place_dirt_and_tree(registry, world, env, random, origin);
        let mut supported = working.is_some();
        if let Some(working) = working {
            supported &= self.place_roots(world, random, origin, working);
        }
        PlaceOutcome {
            supported,
            // 官方这里恒返回 true（只要 origin 是空气）。
            // The official code returns true whenever the origin is air.
            placed: true,
        }
    }

    /// `placeDirtAndTree`：向上逐格找能放树的位置，返回成功时的树根位置（`workingPos`）。
    ///
    /// `placeDirtAndTree`: walk upward looking for a spot that fits a tree; returns the
    /// trunk position on success.
    fn place_dirt_and_tree(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> Option<Pos> {
        let mut working = origin;
        for y in 0..self.root_column_max_height {
            working = (working.0, working.1 + 1, working.2);
            let surface = world.get_height(HeightmapKind::WorldSurface, working.0, working.2);
            if surface < working.1 {
                return None;
            }
            if !self.allowed_tree_position.test(world, working) {
                continue;
            }
            if !self.space_for_tree(world, working) {
                continue;
            }
            let below = (working.0, working.1 - 1, working.2);
            let below_state = world.get_block(below.0, below.1, below.2);
            if in_fluid_tag(below_state, "minecraft:lava")
                || state_facts(below_state) & facts::SOLID == 0
            {
                return None;
            }
            let outcome = self.place_tree(registry, world, env, random, working);
            if !outcome.supported {
                return None;
            }
            if outcome.placed {
                let target = origin.1 + y;
                self.place_dirt(world, random, origin, target);
                return Some(working);
            }
        }
        None
    }

    /// `this.treeFeature.value().place(level, generator, random, workingPos)`。
    /// `this.treeFeature.value().place(level, generator, random, workingPos)`.
    fn place_tree(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> PlaceOutcome {
        let Some(child) = super::composition::Child::parse(&self.tree_feature) else {
            return PlaceOutcome::unsupported();
        };
        super::composition::place_child(&child, registry, world, env, random, pos)
    }

    /// `spaceForTree`：上方 `required_vertical_space_for_tree` 格都要能放树（空气，
    /// 或允许高度内的水）；`level_test_distance > 0` 时还要检查四个角。
    ///
    /// `spaceForTree`: every cell above must allow a tree (air, or water within the
    /// allowed height); with `level_test_distance > 0` the four corners are checked too.
    fn space_for_tree(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        for i in 1..=self.required_vertical_space_for_tree {
            let state = world.get_block(pos.0, pos.1 + i, pos.2);
            if !is_allowed_tree_space(state, i, self.allowed_vertical_water_for_tree) {
                return false;
            }
        }
        if self.level_test_distance > 0 {
            // `Direction.from2DDataValue(i)` 依次是 SOUTH、WEST、NORTH、EAST。
            // `Direction.from2DDataValue(i)` yields SOUTH, WEST, NORTH, EAST.
            for direction in [
                Direction::South,
                Direction::West,
                Direction::North,
                Direction::East,
            ] {
                let (dx, _, dz) = offset(direction);
                let corner_x = pos.0 + dx * self.level_test_distance;
                let corner_z = pos.2 + dz * self.level_test_distance;
                let below = world.get_block(corner_x, pos.1 - self.max_level_deviation, corner_z);
                let above = world.get_block(corner_x, pos.1 + self.max_level_deviation, corner_z);
                if is_air(below) || !is_air(above) {
                    return false;
                }
            }
        }
        true
    }

    /// `placeDirt`：从 `origin.y` 到 `target_height` 逐层铺根。
    /// `placeDirt`: lay roots layer by layer from `origin.y` to `target_height`.
    fn place_dirt(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
        target_height: i32,
    ) {
        let mut y = origin.1;
        while y < target_height {
            self.place_rooted_dirt(world, random, origin.0, origin.2, y);
            y += 1;
        }
    }

    /// `placeRootedDirt`：`root_placement_attempts` 次随机水平偏移。
    /// `placeRootedDirt`: `root_placement_attempts` random horizontal offsets.
    fn place_rooted_dirt(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin_x: i32,
        origin_z: i32,
        y: i32,
    ) {
        for _ in 0..self.root_placement_attempts {
            let dx = random.next_int(self.root_radius) - random.next_int(self.root_radius);
            let dz = random.next_int(self.root_radius) - random.next_int(self.root_radius);
            let x = origin_x + dx;
            let z = origin_z + dz;
            let existing = world.get_block(x, y, z);
            if block_id_of_state(existing)
                .is_some_and(|id| self.root_replaceable.contains(&(id as i32)))
            {
                let Some(state) = self.root_state_provider.get_state(random) else {
                    continue;
                };
                world.set_block_with_heightmaps(x, y, z, state);
            }
        }
    }

    /// `placeRoots`：在 origin 周围撒悬挂根，要求目标格是空气、状态能存活、
    /// 且上面那格朝下的面结实。
    ///
    /// `placeRoots`: scatter hanging roots around the origin; the target must be air, the
    /// state must survive, and the block above must be sturdy toward DOWN.
    fn place_roots(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: Pos,
        _working: Pos,
    ) -> bool {
        let mut supported = true;
        for _ in 0..self.hanging_root_placement_attempts {
            let dx = random.next_int(self.hanging_root_radius)
                - random.next_int(self.hanging_root_radius);
            let dy = random.next_int(self.hanging_roots_vertical_span)
                - random.next_int(self.hanging_roots_vertical_span);
            let dz = random.next_int(self.hanging_root_radius)
                - random.next_int(self.hanging_root_radius);
            let target = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
            if !world.is_air(target.0, target.1, target.2) {
                continue;
            }
            let Some(state) = self.hanging_root_state_provider.get_state(random) else {
                supported = false;
                continue;
            };
            if !super::survival::can_survive(state, world, target) {
                continue;
            }
            let above = world.get_block(target.0, target.1 + 1, target.2);
            if !survival_sturdy_down(state, world, target, above) {
                continue;
            }
            world.set_block_with_heightmaps(target.0, target.1, target.2, state);
        }
        supported
    }
}

/// `HangingRootsBlock.canSurvive`：上面那格 `isFaceSturdy(DOWN)`（FULL）。
/// `HangingRootsBlock.canSurvive`: the block above is `isFaceSturdy(DOWN)` (FULL).
fn survival_sturdy_down(_state: u32, _world: &FeatureWorld<'_>, _pos: Pos, above: u32) -> bool {
    // `Direction.DOWN` 的 ordinal 是 0。
    // `Direction.DOWN` has ordinal 0.
    state_facts(above) & (1u64 << facts::STURDY_FULL) != 0
}

/// `isAllowedTreeSpace`：空气，或允许高度内的水。
/// `isAllowedTreeSpace`: air, or water within the allowed height.
fn is_allowed_tree_space(
    state: u32,
    blocks_above_origin: i32,
    allowed_vertical_water: i32,
) -> bool {
    if is_air(state) {
        return true;
    }
    let blocks_above_ground = blocks_above_origin + 1;
    blocks_above_ground <= allowed_vertical_water && in_fluid_tag(state, "minecraft:water")
}

fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

fn in_fluid_tag(state: u32, tag: &str) -> bool {
    let fluid = state_fluid(state).fluid;
    let group = &mc_bamboo_data::tags::TAG_GROUPS[mc_bamboo_data::tags::fluid_tags::GROUP];
    group
        .tags
        .iter()
        .find(|entry| entry.name == tag)
        .is_some_and(|entry| entry.ids.contains(&fluid))
}

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

/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名数组。
/// `HolderSet<Block>`: a `#tag`, one block name, or an array of names.
fn parse_block_set(value: &Value) -> Option<Vec<i32>> {
    match value {
        Value::String(name) => {
            let mut out = Vec::new();
            push_block_set(name, &mut out)?;
            Some(out)
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                let name = match item {
                    Value::String(name) => name.as_str(),
                    Value::Object(object) => object.get("id")?.as_str()?,
                    _ => return None,
                };
                push_block_set(name, &mut out)?;
            }
            Some(out)
        }
        _ => None,
    }
}

fn push_block_set(name: &str, out: &mut Vec<i32>) -> Option<()> {
    if let Some(tag) = name.strip_prefix('#') {
        out.extend_from_slice(block_tag(tag)?);
    } else {
        let block = mc_bamboo_data::block_by_name(name)?;
        out.push(block_id_of_state(block.default_state)? as i32);
    }
    Some(())
}

/// 让 `IntProvider` 的引用在这个模块里可见（`RootSystem` 的字段目前都用 `i32`，
/// 这里只是避免 unused import 告警时误删）。
/// Keeps the `IntProvider` import meaningful while every field is a plain `i32`.
#[allow(dead_code)]
type UnusedIntProvider = IntProvider;

#[cfg(test)]
mod tests {
    use super::*;

    fn rooted_azalea_tree() -> Value {
        serde_json::json!({
            "allowed_tree_position": {
                "predicates": [
                    {"predicates": [
                        {"tag": "minecraft:air", "type": "minecraft:matching_block_tag"},
                        {"tag": "minecraft:replaceable_by_trees", "type": "minecraft:matching_block_tag"}
                    ], "type": "minecraft:any_of"},
                    {"offset": [0, -1, 0], "tag": "minecraft:azalea_grows_on", "type": "minecraft:matching_block_tag"}
                ], "type": "minecraft:all_of"
            },
            "allowed_vertical_water_for_tree": 2,
            "feature": {"feature": "minecraft:azalea_tree", "placement": []},
            "hanging_root_placement_attempts": 20,
            "hanging_root_radius": 3,
            "hanging_root_state_provider": {"id": "minecraft:hanging_roots", "properties": {"waterlogged": "false"}},
            "hanging_roots_vertical_span": 2,
            "level_test_distance": 0,
            "max_level_deviation": 0,
            "required_vertical_space_for_tree": 3,
            "root_column_max_height": 100,
            "root_placement_attempts": 20,
            "root_radius": 3,
            "root_replaceable": "#minecraft:azalea_root_replaceable",
            "root_state_provider": {"id": "minecraft:rooted_dirt"},
            "type": "minecraft:root_system"
        })
    }

    #[test]
    fn parses_the_exported_rooted_azalea_tree() {
        let system = RootSystem::parse(&rooted_azalea_tree(), &|_| None).unwrap();
        assert_eq!(system.required_vertical_space_for_tree, 3);
        assert_eq!(system.root_radius, 3);
        assert_eq!(system.root_placement_attempts, 20);
        assert_eq!(system.root_column_max_height, 100);
        assert_eq!(system.hanging_root_radius, 3);
        assert_eq!(system.hanging_roots_vertical_span, 2);
        assert_eq!(system.allowed_vertical_water_for_tree, 2);
        assert!(system.allowed_tree_position.is_supported());
        assert!(!system.root_replaceable.is_empty());
    }

    #[test]
    fn allowed_tree_space_accepts_air_and_shallow_water_only() {
        let air = mc_bamboo_data::blocks::AIR;
        assert!(is_allowed_tree_space(air, 1, 0));
        let water = mc_bamboo_data::block_by_name("minecraft:water")
            .unwrap()
            .default_state;
        // 允许高度 2：`blocksAboveGround = blocksAboveOrigin + 1`，所以第 1 格
        // （`blocksAboveOrigin=1` → 2）可以，第 2 格（→ 3）不行。
        // With allowed height 2, `blocksAboveGround = blocksAboveOrigin + 1`: cell 1
        // (`blocksAboveOrigin=1` → 2) passes, cell 2 (→ 3) does not.
        assert!(is_allowed_tree_space(water, 1, 2));
        assert!(!is_allowed_tree_space(water, 2, 2));
        assert!(!is_allowed_tree_space(water, 3, 2));
        assert!(!is_allowed_tree_space(water, 1, 0));
        let stone = mc_bamboo_data::blocks::STONE;
        assert!(!is_allowed_tree_space(stone, 1, 64));
    }
}
