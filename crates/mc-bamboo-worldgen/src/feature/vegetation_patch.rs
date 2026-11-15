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
            supported: outcome.supported,
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
