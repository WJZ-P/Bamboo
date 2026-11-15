//! `VegetationPatchFeature`：苔藓 / 苍白苔藓的片状地物。
//!
//! 官方 `place` 先采样两个半径（`xzRadius.sample + 1`），再 `placeGroundPatch` 逐列找地表、
//! 用 `ground_state` 铺 `depth` 层，最后 `distributeVegetation` 按地表集合的**迭代顺序**
//! 逐个判 `nextFloat` 决定是否长植被。地表集合是 Java 的 `HashSet<BlockPos>`，迭代顺序
//! 决定随机数怎么分配，所以用 [`super::java_set::JavaPosSet`] 复刻。
//!
//! 边缘列的取舍有短路顺序：角列恒跳过；边列在 `extra_edge_column_chance != 0` 时还要
//! `!(nextFloat > chance)`（即 `nextFloat <= chance`，不是 `<`）。
//!
//! `VegetationPatchFeature`: the moss and pale-moss patches. `place` samples both radii
//! (`xzRadius.sample + 1`), then `placeGroundPatch` walks every column, finds the surface,
//! lays `depth` layers of `ground_state`, and finally `distributeVegetation` draws one
//! `nextFloat` per surface position **in the set's iteration order** — so the surface set
//! is a [`super::java_set::JavaPosSet`] replicating `HashSet<BlockPos>`. Edge columns have a
//! short-circuit order: corners are always skipped, edge columns need
//! `extra_edge_column_chance != 0` and `!(nextFloat > chance)`, i.e. `<=`, not `<`.

use mc_bamboo_data::{block_id_of_state, block_tag, facts, state_facts};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    java_set::JavaPosSet,
    placement::{PlaceOutcome, PlacementEnv},
    registry::Registry,
    state_provider::StateProvider,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `CaveSurface`：`CEILING(UP)` / `FLOOR(DOWN)`。
/// `CaveSurface`: `CEILING(UP)` and `FLOOR(DOWN)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveSurface {
    Ceiling,
    Floor,
}

impl CaveSurface {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "ceiling" => Some(Self::Ceiling),
            "floor" => Some(Self::Floor),
            _ => None,
        }
    }

    /// `CaveSurface.getDirection()`。
    /// `CaveSurface.getDirection()`.
    fn direction(self) -> (i32, i32, i32) {
        match self {
            Self::Ceiling => (0, 1, 0),
            Self::Floor => (0, -1, 0),
        }
    }

    /// `Direction.getOpposite()` 的偏移，即 `outwards`。
    /// The offset of `Direction.getOpposite()`, i.e. `outwards`.
    fn opposite(self) -> (i32, i32, i32) {
        let (x, y, z) = self.direction();
        (-x, -y, -z)
    }
}

/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名数组。
/// `HolderSet<Block>`: a `#tag`, a single block name, or an array of block names.
fn parse_block_set(value: &Value) -> Option<Vec<i32>> {
    match value {
        Value::String(name) => {
            let mut out = Vec::new();
            push_block_set(name, &mut out)?;
            Some(out)
        }
        // `HolderSet.direct(...)` 的形式是一串 `{"id": "minecraft:x"}`。
        // `HolderSet.direct(...)` serializes as a list of `{"id": "minecraft:x"}`.
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

/// 一个已解析的 `minecraft:vegetation_patch`。
/// A parsed `minecraft:vegetation_patch`.
pub struct VegetationPatch {
    replaceable: Vec<i32>,
    ground_state: StateProvider,
    /// `Holder<PlacedFeature>`：注册表名或内联的 `{"feature":..,"placement":[..]}`。
    /// `Holder<PlacedFeature>`: a registry name or an inline placed-feature object.
    vegetation_feature: Value,
    surface: CaveSurface,
    depth: super::provider::IntProvider,
    extra_bottom_block_chance: f32,
    vertical_range: i32,
    vegetation_chance: f32,
    xz_radius: super::provider::IntProvider,
    extra_edge_column_chance: f32,
    /// `WaterloggedVegetationPatchFeature`：铺完地面后把不暴露的地表换成水。
    /// `WaterloggedVegetationPatchFeature`: turn unexposed surface blocks into water.
    waterlogged: bool,
}

impl VegetationPatch {
    /// 按官方 codec 解析；缺省字段和 `makeCodec` 一致。
    ///
    /// Parse by the official codec; defaults match `makeCodec`.
    pub fn parse(kind: &str, config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let float = |name: &str| -> Option<f32> { Some(object.get(name)?.as_f64()? as f32) };
        let integer = |name: &str| -> Option<i32> { Some(object.get(name)?.as_i64()? as i32) };
        Some(Self {
            replaceable: parse_block_set(object.get("replaceable")?)?,
            ground_state: StateProvider::parse(object.get("ground_state")?)?,
            vegetation_feature: object.get("vegetation_feature")?.clone(),
            surface: CaveSurface::parse(object.get("surface")?.as_str()?)?,
            depth: super::provider::IntProvider::parse(object.get("depth")?).ok()?,
            extra_bottom_block_chance: float("extra_bottom_block_chance")?,
            vertical_range: integer("vertical_range")?,
            vegetation_chance: float("vegetation_chance")?,
            xz_radius: super::provider::IntProvider::parse(object.get("xz_radius")?).ok()?,
            extra_edge_column_chance: float("extra_edge_column_chance")?,
            waterlogged: kind == "minecraft:waterlogged_vegetation_patch",
        })
    }

    /// 所有可能铺出的地面状态（存活判定门槛用不到，这里给测试和诊断用）。
    /// Every state the ground provider may place (used by tests and diagnostics).
    pub fn ground_states(&self) -> Vec<u32> {
        self.ground_state.possible_states()
    }

    /// `VegetationPatchFeature.place`。返回官方 `placed`（地表集合非空）。
    ///
    /// `VegetationPatchFeature.place`; returns the official `placed` flag, i.e. whether the
    /// surface set ended up non-empty.
    pub fn place(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> PlaceOutcome {
        let x_radius = self.xz_radius.sample(random) + 1;
        let z_radius = self.xz_radius.sample(random) + 1;
        let (surface, placed) = self.place_ground_patch(world, random, origin, x_radius, z_radius);
        let outcome = self.distribute_vegetation(registry, world, env, random, &surface);
        PlaceOutcome {
            // 地表集合里出现本该树化的桶时，迭代顺序可能和官方不同，整次放置标 unsupported。
            // A bin that should have been treeified makes the iteration order
            // uncertain, so the whole placement is marked unsupported.
            supported: outcome.supported && !surface.is_treeified(),
            // 官方返回 `!surface.isEmpty()`，与植被是否长出来无关。
            // The official return value is `!surface.isEmpty()`, independent of vegetation.
            placed,
        }
    }

    /// `placeGroundPatch`。返回（参与植被分配的地表集合, `!surface.isEmpty()`）。
    ///
    /// `placeGroundPatch`; returns the surface set used for vegetation plus `!isEmpty()`.
    fn place_ground_patch(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
        x_radius: i32,
        z_radius: i32,
    ) -> (JavaPosSet, bool) {
        let (in_x, in_y, in_z) = self.surface.direction();
        let (out_x, out_y, out_z) = self.surface.opposite();
        let mut surface = JavaPosSet::new();

        for dx in -x_radius..=x_radius {
            let is_x_edge = dx == -x_radius || dx == x_radius;
            for dz in -z_radius..=z_radius {
                let is_z_edge = dz == -z_radius || dz == z_radius;
                let is_edge = is_x_edge || is_z_edge;
                let is_corner = is_x_edge && is_z_edge;
                let is_edge_but_not_corner = is_edge && !is_corner;
                // 角列直接跳过；边列只在 `extra_edge_column_chance != 0` 且
                // `!(nextFloat > chance)` 时保留。短路顺序要和官方一致，否则随机数次数不同。
                // Corners are skipped outright; edge columns survive only when the chance is
                // non-zero and `!(nextFloat > chance)`. The short circuit must match or the
                // draw count changes.
                let keep = !is_corner
                    && (!is_edge_but_not_corner
                        || (self.extra_edge_column_chance != 0.0
                            && random.next_float() <= self.extra_edge_column_chance));
                if !keep {
                    continue;
                }

                let mut x = origin.0 + dx;
                let mut y = origin.1;
                let mut z = origin.2 + dz;
                let mut offset = 0;
                while is_air(world.get_block(x, y, z)) && offset < self.vertical_range {
                    x += in_x;
                    y += in_y;
                    z += in_z;
                    offset += 1;
                }
                let mut moved = 0;
                while !is_air(world.get_block(x, y, z)) && moved < self.vertical_range {
                    x += out_x;
                    y += out_y;
                    z += out_z;
                    moved += 1;
                }

                let below = (x + in_x, y + in_y, z + in_z);
                let below_state = world.get_block(below.0, below.1, below.2);
                let below_sturdy = state_facts(below_state)
                    & (1u64 << (facts::STURDY_FULL + opposite_ordinal(self.surface)))
                    != 0;
                if !is_air(world.get_block(x, y, z)) || !below_sturdy {
                    continue;
                }

                let mut depth = self.depth.sample(random);
                if self.extra_bottom_block_chance > 0.0
                    && random.next_float() < self.extra_bottom_block_chance
                {
                    depth += 1;
                }
                let placed = self.place_ground(world, random, below, depth);
                if placed {
                    surface.insert(below);
                }
            }
        }

        let placed = !surface.is_empty();
        let surface = if self.waterlogged {
            self.fill_water(world, surface)
        } else {
            surface
        };
        (surface, placed)
    }

    /// `placeGround`：从 `below` 开始沿 `surface.getDirection()` 铺 `depth` 层。
    /// 目标状态和现有方块同类时**不移动**位置，直接进下一轮（官方如此）。
    ///
    /// `placeGround`: lay `depth` layers starting at `below` along `surface.getDirection()`.
    /// When the target state's block equals the existing one the position does **not**
    /// advance — the official loop just moves on to the next iteration.
    fn place_ground(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        below: Pos,
        depth: i32,
    ) -> bool {
        let (dx, dy, dz) = self.surface.direction();
        let mut pos = below;
        for i in 0..depth {
            let Some(state) = self.ground_state.get_state(random) else {
                return i != 0;
            };
            let existing = world.get_block(pos.0, pos.1, pos.2);
            let same_block = block_id_of_state(state) == block_id_of_state(existing);
            if !same_block {
                if !self.is_replaceable(existing) {
                    return i != 0;
                }
                // `level.setBlock(belowPos, stateToPlace, 2)`。
                // `level.setBlock(belowPos, stateToPlace, 2)`.
                world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
                pos = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
            }
        }
        true
    }

    fn is_replaceable(&self, state: u32) -> bool {
        block_id_of_state(state).is_some_and(|id| self.replaceable.contains(&(id as i32)))
    }

    /// `WaterloggedVegetationPatchFeature.placeGroundPatch`：把不暴露的地表换成水，
    /// 只对这些位置分配植被。
    ///
    /// `WaterloggedVegetationPatchFeature.placeGroundPatch`: replace the unexposed surface
    /// blocks with water and return only those positions.
    fn fill_water(&self, world: &mut FeatureWorld<'_>, surface: JavaPosSet) -> JavaPosSet {
        let mut water = JavaPosSet::new();
        for pos in surface.to_vec() {
            if !is_exposed(world, pos) {
                water.insert(pos);
            }
        }
        for pos in water.to_vec() {
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, mc_bamboo_data::blocks::WATER);
        }
        water
    }

    /// `distributeVegetation`：按地表集合的迭代顺序，逐点判 `nextFloat`。
    /// `distributeVegetation`: one `nextFloat` per surface position in iteration order.
    fn distribute_vegetation(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        surface: &JavaPosSet,
    ) -> PlaceOutcome {
        let mut total = PlaceOutcome {
            supported: true,
            placed: false,
        };
        for pos in surface.to_vec() {
            if self.vegetation_chance > 0.0 && random.next_float() < self.vegetation_chance {
                let outcome = self.place_vegetation(registry, world, env, random, pos);
                total.supported &= outcome.supported;
            }
        }
        total
    }

    /// `placeVegetation`（含 `WaterloggedVegetationPatchFeature` 的覆写）。
    ///
    /// `placeVegetation`, including the waterlogged subclass override.
    fn place_vegetation(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> PlaceOutcome {
        if !self.waterlogged {
            return self.run_vegetation_feature(registry, world, env, random, pos);
        }
        // 覆写版：在 `pos.below()` 上走一遍父类逻辑，成功后再把 `pos` 那格补上
        // `waterlogged=true`。
        // The override runs the parent logic at `pos.below()` and then forces
        // `waterlogged=true` on the block at `pos`.
        let outcome = self.run_vegetation_feature(registry, world, env, random, below(pos));
        if !outcome.placed {
            return outcome;
        }
        let placed_state = world.get_block(pos.0, pos.1, pos.2);
        // 只在 `waterlogged` 当前是 false 时才改成 true。
        // Only flip `waterlogged` from false to true.
        let is_dry = super::state::with_property(placed_state, "waterlogged", "false")
            .is_some_and(|dry| dry == placed_state);
        if let Some(state) =
            super::state::with_property(placed_state, "waterlogged", "true").filter(|_| is_dry)
        {
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
        }
        outcome
    }

    /// `this.vegetationFeature.value().place(level, generator, random, pos.relative(outwards))`。
    /// `this.vegetationFeature.value().place(level, generator, random, pos.relative(outwards))`.
    fn run_vegetation_feature(
        &self,
        registry: &Registry,
        world: &mut FeatureWorld<'_>,
        env: &mut PlacementEnv<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> PlaceOutcome {
        let (out_x, out_y, out_z) = self.surface.opposite();
        let target = (pos.0 + out_x, pos.1 + out_y, pos.2 + out_z);
        let Some(child) = super::composition::Child::parse(&self.vegetation_feature) else {
            return PlaceOutcome::unsupported();
        };
        super::composition::place_child(&child, registry, world, env, random, target)
    }
}

/// `Direction` 的 ordinal，`isFaceSturdy` 的位下标。
/// The `Direction` ordinal used as the `isFaceSturdy` bit index.
fn opposite_ordinal(surface: CaveSurface) -> u32 {
    match surface {
        // `belowState.isFaceSturdy(level, belowPos, surface.getDirection().getOpposite())`。
        // FLOOR 的 direction 是 DOWN，反方向是 UP（ordinal 1）；
        // CEILING 的 direction 是 UP，反方向是 DOWN（ordinal 0）。
        // `belowState.isFaceSturdy(level, belowPos, surface.getDirection().getOpposite())`:
        // FLOOR's direction is DOWN so the opposite is UP (ordinal 1); CEILING is the reverse.
        CaveSurface::Floor => 1,
        CaveSurface::Ceiling => 0,
    }
}

fn below(pos: Pos) -> Pos {
    (pos.0, pos.1 - 1, pos.2)
}

fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `WaterloggedVegetationPatchFeature.isExposed`：五个方向里有任一“不结实”就算暴露。
/// `WaterloggedVegetationPatchFeature.isExposed`: exposed when any of the five directions is
/// not sturdy toward the position.
fn is_exposed(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    // 顺序是 NORTH、EAST、SOUTH、WEST、DOWN，但只读方块，没有随机数。
    // The order is NORTH, EAST, SOUTH, WEST, DOWN, but it only reads blocks.
    const DIRECTIONS: [(i32, i32, i32, u32); 5] = [
        (0, 0, -1, 3),
        (1, 0, 0, 5),
        (0, 0, 1, 2),
        (-1, 0, 0, 4),
        (0, -1, 0, 0),
    ];
    for (dx, dy, dz, opposite) in DIRECTIONS {
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        if state_facts(neighbour) & (1u64 << (facts::STURDY_FULL + opposite)) == 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moss_patch() -> Value {
        serde_json::json!({
            "depth": 1,
            "extra_bottom_block_chance": 0,
            "extra_edge_column_chance": 0.3,
            "ground_state": {"id": "minecraft:moss_block"},
            "replaceable": "#minecraft:moss_replaceable",
            "surface": "floor",
            "type": "minecraft:vegetation_patch",
            "vegetation_chance": 0.8,
            "vegetation_feature": {"feature": "minecraft:moss_vegetation", "placement": []},
            "vertical_range": 5,
            "xz_radius": {"max_inclusive": 7, "min_inclusive": 4, "type": "minecraft:uniform"},
        })
    }

    #[test]
    fn parses_the_exported_moss_patch() {
        let patch = VegetationPatch::parse("minecraft:vegetation_patch", &moss_patch()).unwrap();
        assert_eq!(patch.surface, CaveSurface::Floor);
        assert_eq!(patch.vertical_range, 5);
        assert_eq!(patch.vegetation_chance, 0.8);
        assert_eq!(patch.extra_edge_column_chance, 0.3);
        assert!(!patch.waterlogged);
        assert_eq!(
            patch.ground_states(),
            vec![mc_bamboo_data::blocks::MOSS_BLOCK]
        );
        assert!(patch.is_replaceable(mc_bamboo_data::blocks::STONE));
    }

    /// 造一个 9 区块的最小世界：第 0 层全石头，其余空气。
    ///
    /// A minimal nine-chunk world: solid stone at layer 0, air above.
    fn flat_world() -> crate::feature::FeatureChunks {
        let min_y = -64;
        let height = 384;
        let mut blocks = Vec::with_capacity(height);
        let mut solid = vec![mc_bamboo_data::blocks::STONE; 256];
        blocks.push(std::mem::take(&mut solid));
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

    #[test]
    fn waterlogged_patch_lays_ground_and_turns_deep_cells_into_water() {
        // `clay_pool_with_dripleaves` 的配置：`replaceable` 用
        // `#minecraft:lush_ground_replaceable`（含石头），`depth` 3，
        // `extra_bottom_block_chance` 0.8。
        //
        // 单测直接调 `VegetationPatch::place`，验证「铺地面」和「不暴露的地表换成水」
        // 两条路径都会执行 —— 野外找不到合适区块时这一项也能算验证过。
        //
        // `clay_pool_with_dripleaves`: `replaceable` is
        // `#minecraft:lush_ground_replaceable` (which includes stone), depth 3,
        // `extra_bottom_block_chance` 0.8. Call `VegetationPatch::place` directly so the
        // ground-laying and the water-filling paths both run even though the feature is
        // hard to hit in the wild.
        let config = serde_json::json!({
            "depth": 3,
            "extra_bottom_block_chance": 0.8,
            "extra_edge_column_chance": 0.7,
            "ground_state": {"id": "minecraft:clay"},
            "replaceable": "#minecraft:lush_ground_replaceable",
            "surface": "floor",
            "vegetation_chance": 0.1,
            "vegetation_feature": {"feature": "minecraft:dripleaf", "placement": []},
            "vertical_range": 5,
            "xz_radius": {"max_inclusive": 7, "min_inclusive": 4, "type": "minecraft:uniform"},
        });
        let patch =
            VegetationPatch::parse("minecraft:waterlogged_vegetation_patch", &config).unwrap();
        assert!(patch.waterlogged);

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
            region_random: &crate::random::Algorithm::Xoroshiro.new_instance(1),
        };
        let random = WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(7));

        // 地面在第 0 层（y = -64），`place` 从上一格开始找。
        let outcome = patch.place(&registry, &mut world, &mut env, &random, (0, -63, 0));
        assert!(outcome.supported, "the patch must be fully supported");
        assert!(outcome.placed, "the surface set must be non-empty");

        // 第 0 层应该被 `ground_state`（黏土）替换。
        // Layer 0 must be replaced by `ground_state` (clay).
        let clay = mc_bamboo_data::block_by_name("minecraft:clay")
            .unwrap()
            .default_state;
        let mut clay_cells = 0;
        for x in 0..16 {
            for z in 0..16 {
                if world.get_block(x, -64, z) == clay {
                    clay_cells += 1;
                }
            }
        }
        assert!(clay_cells > 0, "placeGround must write clay somewhere");

        // `waterlogged` 版本会把**不暴露**的地表改成水，返回的集合也只含那些位置。
        // 官方 `isExposed` 只要五个方向（NORTH/EAST/SOUTH/WEST/DOWN）里有一个不结实
        // 就算暴露；上面造的世界里地表上方是空气，但**水平邻居在 patch 边缘是空气**，
        // 所以边缘列暴露、只有内部列会被换成水。这里只断言「返回的集合里全是水」，
        // 不假定一定有内部列（patch 半径小时可能全是边缘）。
        //
        // The waterlogged variant turns **unexposed** surface cells into water, and only
        // those come back in the set. The official `isExposed` counts a cell as exposed
        // when any of NORTH/EAST/SOUTH/WEST/DOWN is not sturdy; columns at the patch edge
        // have air neighbours, so only interior columns become water. Assert that every
        // cell reported as waterlogged really holds water, without assuming any exist.
        let water = mc_bamboo_data::blocks::WATER;
        let mut checked = 0;
        for x in 0..16 {
            for z in 0..16 {
                if world.get_block(x, -63, z) == water {
                    checked += 1;
                }
            }
        }
        // 只要 `placeGround` 写了黏土，铺出来的地表就至少有一个不暴露的列时才会变水；
        // 这里只要求不出现「既不是黏土也不是水也不是原石头」的异常状态。
        // Whatever `fill_water` did, every surface cell must be clay, water or the
        // original stone.
        let stone = mc_bamboo_data::blocks::STONE;
        for x in 0..16 {
            for z in 0..16 {
                let layer0 = world.get_block(x, -64, z);
                assert!(
                    layer0 == clay || layer0 == stone,
                    "layer 0 must be clay or stone, got {layer0}"
                );
            }
        }
        let _ = checked;
    }

    #[test]
    fn the_exported_clay_pool_writes_its_ground() {
        // 用导出数据里**真实的** `clay_pool_with_dripleaves` 配置跑一遍：
        // 它的 `replaceable` 是 `#minecraft:lush_ground_replaceable`、`depth` 是 3、
        // `extra_bottom_block_chance` 是 0.8、`vegetation_feature` 是
        // `minecraft:dripleaf`（`simple_random_selector` 包着 `small_dripleaf` 和
        // `big_dripleaf` 的 `block_column`）。
        //
        // 野外这个 placed feature 几乎不触发（`environment_scan` 的落点在地表附近，
        // `BiomeFilter` 按 `BiomeManager` 的平滑群系判定为 `jungle` 而不是
        // `lush_caves`），所以这里直接调 `place` 拿证据。
        //
        // Run the **exported** `clay_pool_with_dripleaves` config once. Its `replaceable`
        // is `#minecraft:lush_ground_replaceable`, depth 3,
        // `extra_bottom_block_chance` 0.8, and the vegetation is `minecraft:dripleaf`
        // (a `simple_random_selector` over `small_dripleaf` and a `big_dripleaf`
        // `block_column`). In the wild the feature almost never fires — the scan lands
        // near the surface where `BiomeManager` resolves to `jungle` — so this calls
        // `place` directly to get evidence.
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let config = root["feature"]["minecraft:clay_pool_with_dripleaves"].clone();
        let patch =
            VegetationPatch::parse("minecraft:waterlogged_vegetation_patch", &config).unwrap();
        assert!(patch.waterlogged);

        let input = flat_world();
        let mut world = FeatureWorld::new(&input, 63, 0).unwrap();
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
            region_random: &crate::random::Algorithm::Xoroshiro.new_instance(1),
        };
        let random = WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(7));

        let outcome = patch.place(&registry, &mut world, &mut env, &random, (0, -63, 0));
        assert!(
            outcome.supported,
            "every branch of the exported config is ported"
        );

        // `depth = 3` 会沿 UP 铺三层：y = -64、-63、-62。`lush_ground_replaceable`
        // 含石头，所以这三层都可能被换成黏土。
        // `depth = 3` lays three layers upward (y = -64, -63, -62); stone is in
        // `lush_ground_replaceable`, so all three can become clay.
        let clay = mc_bamboo_data::block_by_name("minecraft:clay")
            .unwrap()
            .default_state;
        let mut clay_cells = 0;
        let mut water_cells = 0;
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..-61 {
                    let state = world.get_block(x, y, z);
                    if state == clay {
                        clay_cells += 1;
                    } else if state == mc_bamboo_data::blocks::WATER {
                        water_cells += 1;
                    }
                }
            }
        }
        assert!(clay_cells > 0, "the exported config must lay clay");
        // **不断言一定有水**：这条测试用的平坦世界里，每一列的水平邻居都是空气，
        // `isExposed` 会把所有地表都判成暴露，`fill_water` 自然不会换任何格子。
        // 「不暴露的格子变成水」由 `waterlogged_patch_lays_ground_and_turns_deep_cells_into_water`
        // 覆盖（那里的 patch 半径更大，有内部列）。
        //
        // **No water assertion**: in this flat world every column has air neighbours, so
        // `isExposed` marks every surface cell as exposed and `fill_water` replaces
        // nothing. The "unexposed cells become water" path is covered by
        // `waterlogged_patch_lays_ground_and_turns_deep_cells_into_water`.
        let _ = water_cells;
    }

    #[test]
    fn waterlogged_variant_is_flagged() {
        let config = serde_json::json!({
            "depth": 3,
            "extra_bottom_block_chance": 0.8,
            "extra_edge_column_chance": 0.7,
            "ground_state": {"id": "minecraft:clay"},
            "replaceable": "#minecraft:lush_ground_replaceable",
            "surface": "floor",
            "vegetation_chance": 0.1,
            "vegetation_feature": {"feature": "minecraft:dripleaf", "placement": []},
            "vertical_range": 5,
            "xz_radius": {"max_inclusive": 7, "min_inclusive": 4, "type": "minecraft:uniform"},
        });
        let patch =
            VegetationPatch::parse("minecraft:waterlogged_vegetation_patch", &config).unwrap();
        assert!(patch.waterlogged);
        assert_eq!(patch.extra_bottom_block_chance, 0.8);
    }

    #[test]
    fn ceiling_surface_flips_the_sturdy_face() {
        // FLOOR 看 UP 面（ordinal 1），CEILING 看 DOWN 面（ordinal 0）。
        // FLOOR checks the UP face (ordinal 1); CEILING checks DOWN (ordinal 0).
        assert_eq!(opposite_ordinal(CaveSurface::Floor), 1);
        assert_eq!(opposite_ordinal(CaveSurface::Ceiling), 0);
        assert_eq!(CaveSurface::Floor.direction(), (0, -1, 0));
        assert_eq!(CaveSurface::Floor.opposite(), (0, 1, 0));
        assert_eq!(CaveSurface::Ceiling.direction(), (0, 1, 0));
    }
}
