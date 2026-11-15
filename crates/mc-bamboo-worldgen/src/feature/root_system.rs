//! `RootSystemFeature`：杜鹃树根系 / 硫磺泉根系这类「先往上找位置长一株树，再从原点
//! 往上铺一条根柱」的地物。
//!
//! 官方 `place` 只在 `origin` 是空气时继续：`placeDirtAndTree` 沿竖直方向最多
//! `root_column_max_height` 格逐格试，命中 `allowed_tree_position` 且竖直空间够时调
//! 子 `PlacedFeature.place` 放树；树放成功了才 `placeDirt`（沿根柱逐格 `placeRootedDirt`）
//! 并 `placeRoots`（悬挂根）。三个循环的随机数消耗顺序必须和官方一致：
//! `placeRootedDirt` 每次尝试抽两次 `nextInt(rootRadius)` 定 x、两次定 z，
//! `placeRoots` 每次尝试抽六次（x、y、z 各两次）。
//!
//! `RootSystemFeature`: features that grow one tree above the origin and then lay a
//! column of roots from the origin upward. Official `place` bails unless the origin is
//! air; `placeDirtAndTree` walks up to `root_column_max_height` cells, and on the first
//! cell that satisfies `allowed_tree_position` with enough vertical space it places the
//! child `PlacedFeature`. Only when that tree was placed does it run `placeDirt` (walking
//! the root column) and `placeRoots` (hanging roots). The random draw order must match
//! the official code: `placeRootedDirt` draws two `nextInt(rootRadius)` for x and two for
//! z per attempt, and `placeRoots` draws six (two per axis).

use serde_json::Value;

use mc_bamboo_data::{block_id_of_state, block_tag, facts, registries, state_facts, state_fluid};

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
    composition::{Child, place_child},
    placement::{PlaceOutcome, PlacementEnv},
    registry::Registry,
    state_provider::StateProvider,
    survival,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// 一个已解析的 `minecraft:root_system`。
///
/// A parsed `minecraft:root_system`.
pub struct RootSystemFeature {
    /// `Holder<PlacedFeature>`：注册表名或内联的 `{"feature":..,"placement":[..]}`。
    ///
    /// `Holder<PlacedFeature>`: a registry name or an inline placed-feature object.
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

impl RootSystemFeature {
    /// 按官方 codec 解析。`allowed_tree_position` 或两个状态提供者里有未移植分支时返回
    /// `None`，让调用方把整个 placed feature 标 unsupported。
    ///
    /// Parse by the official codec. Return `None` when `allowed_tree_position` or either
    /// state provider contains an unported branch, so the caller can mark the whole
    /// placed feature unsupported.
    pub fn parse<'a>(
        config: &'a Value,
        lookup: &dyn Fn(&str) -> Option<&'a Value>,
    ) -> Option<Self> {
        let object = config.as_object()?;
        let integer = |name: &str| -> Option<i32> { Some(object.get(name)?.as_i64()? as i32) };
        let root_state_provider =
            StateProvider::parse_with(object.get("root_state_provider")?, lookup)?;
        let hanging_root_state_provider =
            StateProvider::parse_with(object.get("hanging_root_state_provider")?, lookup)?;
        // 两个 provider 的状态都走 `setBlock`，悬挂根还要过 `canSurvive`；任一分支未移植
        // 就不接入。
        // Both providers feed `setBlock`, and the hanging one also goes through
        // `canSurvive`, so an unported branch blocks the port.
        for provider in [&root_state_provider, &hanging_root_state_provider] {
            if !provider.supported() {
                return None;
            }
            if !provider
                .possible_states()
                .iter()
                .all(|&state| survival::supports(state))
            {
                return None;
            }
        }
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
            root_state_provider,
            root_placement_attempts: integer("root_placement_attempts")?,
            root_column_max_height: integer("root_column_max_height")?,
            hanging_root_radius: integer("hanging_root_radius")?,
            hanging_roots_vertical_span: integer("hanging_roots_vertical_span")?,
            hanging_root_state_provider,
            hanging_root_placement_attempts: integer("hanging_root_placement_attempts")?,
            allowed_vertical_water_for_tree: integer("allowed_vertical_water_for_tree")?,
            allowed_tree_position,
        })
    }

    /// 官方 `RootSystemFeature.place`：`origin` 不是空气返回 false；之后无论有没有种成树
    /// 都返回 true。
    ///
    /// Official `RootSystemFeature.place`: false only when the origin is not air;
    /// afterwards it returns true whether or not a tree was placed.
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
        let Some((outcome, hit_y)) = self.place_dirt_and_tree(registry, world, env, random, origin)
        else {
            return PlaceOutcome {
                supported: true,
                placed: true,
            };
        };
        if !outcome.supported {
            return outcome;
        }
        if outcome.placed {
            // 官方 `placeDirt(pos, pos.getY() + y, ...)`，`y` 是命中的循环下标
            // （`workingPos.y = origin.y + y + 1`）。
            // The official call passes `origin.y + y` where `y` is the loop index.
            self.place_dirt(world, random, origin, origin.1 + hit_y);
            self.place_roots(world, random, origin);
        }
        PlaceOutcome {
            supported: true,
            placed: true,
        }
    }

    /// `placeDirtAndTree`。种成树返回 `Some((子地物结果, 命中的循环下标 y))`，
    /// 循环走完或撞到「下面是岩浆 / 不是实心方块」返回 `None`。
    ///
    /// `placeDirtAndTree`. `Some((child outcome, loop index y))` when a tree was placed;
    /// `None` when the loop finishes or hits a lava / non-solid block below.
    fn place_dirt_and_tree(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> Option<(PlaceOutcome, i32)> {
        for y in 0..self.root_column_max_height {
            let pos = (origin.0, origin.1 + y + 1, origin.2);
            // `level.getHeight(WORLD_SURFACE, workingPos) < workingPos.getY()`。
            // `level.getHeight(WORLD_SURFACE, workingPos) < workingPos.getY()`.
            if world.get_height(HeightmapKind::WorldSurface, pos.0, pos.2) < pos.1 {
                return None;
            }
            if !self.allowed_tree_position.test(world, pos) || !self.space_for_tree(world, pos) {
                continue;
            }
            let below = world.get_block(pos.0, pos.1 - 1, pos.2);
            if state_fluid(below).fluid == registries::fluid::LAVA
                || state_facts(below) & facts::SOLID == 0
            {
                return None;
            }
            let Some(child) = Child::parse(&self.tree_feature) else {
                return Some((PlaceOutcome::unsupported(), y));
            };
            let outcome = place_child(&child, registry, world, env, random, pos);
            if !outcome.supported || outcome.placed {
                return Some((outcome, y));
            }
        }
        None
    }

    /// `spaceForTree`：上面 `required_vertical_space_for_tree` 格都得是空气或允许高度内的
    /// 水，再按 `level_test_distance` 抽查四个水平方向的地面落差。
    ///
    /// `spaceForTree`: the cells above must be air or allowed water, then the four
    /// horizontal directions are checked for a level jump.
    fn space_for_tree(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        for i in 1..=self.required_vertical_space_for_tree {
            let state = world.get_block(pos.0, pos.1 + i, pos.2);
            if !is_allowed_tree_space(state, i, self.allowed_vertical_water_for_tree) {
                return false;
            }
        }
        if self.level_test_distance > 0 {
            // `Direction.from2DDataValue(i)` 的 `BY_2D_DATA` 按 `data2d` 升序排，
            // 顺序是 SOUTH、WEST、NORTH、EAST，不是枚举顺序。
            // `BY_2D_DATA` is sorted by `data2d`: SOUTH, WEST, NORTH, EAST.
            const HORIZONTAL_BY_2D: [(i32, i32); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];
            for (dx, dz) in HORIZONTAL_BY_2D {
                let x = pos.0 + dx * self.level_test_distance;
                let z = pos.2 + dz * self.level_test_distance;
                let below = world.get_block(x, pos.1 - self.max_level_deviation, z);
                let above = world.get_block(x, pos.1 + self.max_level_deviation, z);
                if state_facts(below) & facts::AIR != 0 || state_facts(above) & facts::AIR == 0 {
                    return false;
                }
            }
        }
        true
    }

    /// `placeDirt`：从 `origin.y` 到 `target_y`（不含）沿同一列逐格铺根。
    ///
    /// `placeDirt`: walk the column from `origin.y` to `target_y` exclusive.
    fn place_dirt(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
        target_y: i32,
    ) {
        for y in origin.1..target_y {
            self.place_rooted_dirt(world, random, origin.0, origin.2, y);
        }
    }

    /// `placeRootedDirt`：每轮相对原点偏移 `nextInt(radius) - nextInt(radius)`（x、z 各一组），
    /// 命中 `root_replaceable` 才写 provider 的状态。随机数每轮必抽满四次。
    ///
    /// `placeRootedDirt`: each attempt offsets from the origin by
    /// `nextInt(radius) - nextInt(radius)` per axis, then writes the provider state when
    /// the target is `root_replaceable`. Four draws happen every attempt.
    fn place_rooted_dirt(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin_x: i32,
        origin_z: i32,
        y: i32,
    ) {
        for _ in 0..self.root_placement_attempts {
            let x =
                origin_x + random.next_int(self.root_radius) - random.next_int(self.root_radius);
            let z =
                origin_z + random.next_int(self.root_radius) - random.next_int(self.root_radius);
            let state = world.get_block(x, y, z);
            let replaceable = block_id_of_state(state)
                .is_some_and(|id| self.root_replaceable.contains(&(id as i32)));
            if replaceable
                && let Some(new_state) =
                    self.root_state_provider
                        .get_optional_state(world, random, (x, y, z))
            {
                world.set_block_with_heightmaps(x, y, z, new_state);
            }
        }
    }

    /// `placeRoots`：围绕 `origin` 在悬挂根半径与竖直跨度内试放悬挂根，要求空位、
    /// 能存活，且上面那格朝下结实。
    ///
    /// `placeRoots`: try hanging roots around the origin, requiring an air cell that
    /// survives and a sturdy block face above it.
    fn place_roots(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, origin: Pos) {
        for _ in 0..self.hanging_root_placement_attempts {
            // 官方六抽：x、y、z 各两次，顺序固定。
            // Six draws in a fixed order: two each for x, y and z.
            let x = origin.0 + random.next_int(self.hanging_root_radius)
                - random.next_int(self.hanging_root_radius);
            let y = origin.1 + random.next_int(self.hanging_roots_vertical_span)
                - random.next_int(self.hanging_roots_vertical_span);
            let z = origin.2 + random.next_int(self.hanging_root_radius)
                - random.next_int(self.hanging_root_radius);
            if !world.is_air(x, y, z) {
                continue;
            }
            let Some(state) =
                self.hanging_root_state_provider
                    .get_optional_state(world, random, (x, y, z))
            else {
                continue;
            };
            if !survival::can_survive(state, world, (x, y, z)) {
                continue;
            }
            // `getBlockState(workingPos.above()).isFaceSturdy(level, workingPos, DOWN)`：
            // 判定的是上面那格朝下的面，即 `STURDY_FULL + DOWN(0)`。
            // The face of the cell above toward DOWN is tested: `STURDY_FULL + 0`.
            let above = world.get_block(x, y + 1, z);
            if state_facts(above) & (1u64 << facts::STURDY_FULL) == 0 {
                continue;
            }
            world.set_block_with_heightmaps(x, y, z, state);
        }
    }
}

/// `isAllowedTreeSpace`：空气恒允许；非空气只有当它在允许高度内的水里才行。
///
/// `isAllowedTreeSpace`: air always passes; otherwise only water within the allowed
/// height passes.
fn is_allowed_tree_space(
    state: u32,
    blocks_above_origin: i32,
    allowed_vertical_water: i32,
) -> bool {
    if state_facts(state) & facts::AIR != 0 {
        return true;
    }
    let blocks_above_ground = blocks_above_origin + 1;
    blocks_above_ground <= allowed_vertical_water
        && state_fluid(state).fluid == registries::fluid::WATER
}

/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名数组（`HolderSet.direct` 的形式）。
///
/// `HolderSet<Block>`: a `#tag`, a single block name, or an array of block names.
fn parse_block_set(value: &Value) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    match value {
        Value::String(name) => push_block_set(name, &mut out)?,
        Value::Array(items) => {
            for item in items {
                let name = match item {
                    Value::String(name) => name.as_str(),
                    Value::Object(object) => object.get("id")?.as_str()?,
                    _ => return None,
                };
                push_block_set(name, &mut out)?;
            }
        }
        _ => return None,
    }
    Some(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn exported(name: &str) -> Value {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        root["feature"][name].clone()
    }

    #[test]
    fn parses_the_exported_root_systems() {
        let feature =
            RootSystemFeature::parse(&exported("minecraft:rooted_azalea_tree"), &|_| None)
                .expect("rooted_azalea_tree");
        assert_eq!(feature.required_vertical_space_for_tree, 3);
        assert_eq!(feature.root_column_max_height, 100);
        assert_eq!(feature.root_radius, 3);
        assert_eq!(feature.hanging_root_placement_attempts, 20);
        let grass = mc_bamboo_data::block_by_name("minecraft:grass_block")
            .unwrap()
            .default_state;
        assert!(
            feature
                .root_replaceable
                .contains(&(block_id_of_state(grass).unwrap() as i32))
        );
        let sulfur =
            RootSystemFeature::parse(&exported("minecraft:rooted_sulfur_spring"), &|_| None)
                .expect("rooted_sulfur_spring");
        assert_eq!(sulfur.level_test_distance, 8);
        assert_eq!(sulfur.max_level_deviation, 2);
        assert_eq!(sulfur.allowed_vertical_water_for_tree, 1);
    }

    /// 造一个 9 区块的最小世界：第 0 层（y = -64）全石头，其余空气。
    ///
    /// A minimal nine-chunk world: solid stone at layer 0 (y = -64), air above.
    fn flat_world() -> crate::feature::FeatureChunks {
        let min_y = -64;
        let height = 384;
        let mut blocks = Vec::with_capacity(height);
        blocks.push(vec![mc_bamboo_data::blocks::STONE; 256]);
        for _ in 1..height {
            blocks.push(vec![mc_bamboo_data::blocks::AIR; 256]);
        }
        let chunk_blocks = || blocks.clone();
        let sections = || {
            let mut out = Vec::new();
            for _ in 0..(height / 16) {
                out.push(vec![0_u32; 64]);
            }
            out
        };
        crate::feature::FeatureChunks {
            center_x: 0,
            center_z: 0,
            min_y,
            height: height as i32,
            blocks: std::array::from_fn(|_| chunk_blocks()),
            biomes: std::array::from_fn(|_| sections()),
            heights: std::array::from_fn(|_| crate::feature::Heightmaps::new(min_y)),
        }
    }

    /// 在假世界里调用 `RootSystemFeature::place`，确认它对空气 origin 返回
    /// `supported && placed`。
    ///
    /// Call `RootSystemFeature::place` in a synthetic world and check that an air origin
    /// yields `supported && placed`.
    #[test]
    fn root_system_place_returns_supported_for_an_air_origin() {
        // 子 feature 用 `simple_block` 放一格 `oak_leaves`：只要能放成功，
        // `placeDirtAndTree` 就会走 `placeDirt`。
        //
        // The child is a `simple_block` placing one `oak_leaves`; as soon as it succeeds
        // `placeDirtAndTree` runs `placeDirt`.
        let config = serde_json::json!({
            "allowed_tree_position": {"tag": "minecraft:air", "type": "minecraft:matching_block_tag"},
            "allowed_vertical_water_for_tree": 2,
            "feature": {
                "feature": {"to_place": {"id": "minecraft:oak_leaves"}, "type": "minecraft:simple_block"},
                "placement": []
            },
            "hanging_root_placement_attempts": 40,
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
            "type": "minecraft:root_system",
        });
        let feature = RootSystemFeature::parse(&config, &|_| None).unwrap();

        let input = flat_world();
        let mut world = FeatureWorld::new(&input, 63, 0).unwrap();
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        let mut biome_at = |_: i32, _: i32, _: i32| 0_u32;
        let mut env = PlacementEnv {
            generation: crate::feature::provider::WorldGenerationContext {
                min_y: -64,
                height: 384,
                sea_level: 63,
            },
            biome_at: &mut biome_at,
            registry: &registry,
            top_feature: None,
            region_random: &WorldgenRandom::new(
                crate::random::Algorithm::Xoroshiro.new_instance(1),
            ),
        };
        let random = WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(7));

        // `placeDirtAndTree` 第一次就 `move(UP)`，所以 origin 放在石头层上方的空气格。
        // `placeDirtAndTree` moves UP first, so the origin sits just above the stone.
        let outcome = feature.place(&registry, &mut world, &mut env, &random, (0, -63, 0));
        assert!(outcome.supported, "the root system must be fully supported");
        assert!(
            outcome.placed,
            "the official place() returns true for an air origin"
        );

        // **这个测试只覆盖「解析 + 顶层 `place` 的返回值语义」**：在石头层上方的空气格
        // 调用 `place`，确认它返回 `supported && placed`（官方对空气 origin 恒返回 true）。
        //
        // 子 feature 与 `placeDirt` 的具体写入路径在这里**没有被触发**（诊断显示叶子没放
        // 到 y = -62），原因未定位 —— 野外也一直没找到能放成树的坐标。所以 `root_system`
        // 整体仍算「已写未验证」，这个测试不改变那个结论。
        //
        // This test only covers parsing and the top-level `place` return contract: calling
        // `place` on an air cell above the stone layer must yield `supported && placed`
        // (the official code returns true for any air origin). The child feature and the
        // `placeDirt` write path are **not** exercised here (diagnostics showed the leaves
        // never landed at y = -62), and no wild chunk has been found that grows a tree, so
        // `root_system` stays **written but unverified**.
        assert!(outcome.supported, "the root system must be fully supported");
        assert!(
            outcome.placed,
            "the official place() returns true for an air origin"
        );
    }

    #[test]
    fn air_passes_tree_space_and_water_is_height_limited() {
        assert!(is_allowed_tree_space(mc_bamboo_data::blocks::AIR, 1, 2));
        // `blocksAboveGround = blocksAboveOrigin + 1`：第 1 格在允许量 2 内，第 3 格不在。
        // `blocksAboveGround` is `blocksAboveOrigin + 1`: cell 1 fits under 2, cell 3 does not.
        assert!(is_allowed_tree_space(mc_bamboo_data::blocks::WATER, 1, 2));
        assert!(!is_allowed_tree_space(mc_bamboo_data::blocks::WATER, 3, 2));
        let stone = mc_bamboo_data::block_by_name("minecraft:stone")
            .unwrap()
            .default_state;
        assert!(!is_allowed_tree_space(stone, 1, 2));
    }
}
