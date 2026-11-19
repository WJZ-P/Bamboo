//! `TreeFeature`：照官方 26.3 的 `TreeFeature.place` / `doPlace` 逐行翻译。
//!
//! 一次放置的流程：取树高、树叶高度和半径 → 检查高度范围和可用空间 → 放树根 →
//! 放树干（得到若干树叶挂点）→ 每个挂点放树叶 → 装饰器 → `updateLeaves` 把树叶的
//! `distance` 写好。树根、树干、树叶、装饰的位置各记在一个和 Java `HashSet` 迭代顺序
//! 一致的集合里，见 [`java_set`]。
//!
//! `TreeFeature`, translated line by line from official 26.3 `TreeFeature.place` /
//! `doPlace`.
//!
//! One placement: pick the tree height, foliage height and radius → check the height
//! range and free space → place roots → place the trunk (yielding foliage attachments)
//! → place foliage at each attachment → decorators → `updateLeaves` writes the leaves'
//! `distance`. Root, trunk, foliage and decoration positions are each recorded in a set
//! that iterates like Java's `HashSet`, see [`java_set`].
//!

pub mod config;
pub mod decorator;
pub mod fallen;
pub mod foliage;
pub mod java_set;
pub mod leaves;
pub mod root;
pub mod trunk;

use std::sync::OnceLock;

use serde_json::Value;

use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_of_state, block_tag, facts, state_facts,
};

use crate::random::WorldgenRandom;

use crate::feature::{
    placement::PlaceOutcome, shape_update::update_shape_at_edge, world::FeatureWorld,
};

use config::TreeConfig;
use decorator::Recursion;
use java_set::JavaHashSet;

pub(crate) type Pos = (i32, i32, i32);

/// `FoliagePlacer.FoliageAttachment`（26.3 的五个字段）。
///
/// Official `FoliagePlacer.FoliageAttachment` (five fields in 26.3).
#[derive(Clone, Copy)]
pub(crate) struct FoliageAttachment {
    pub pos: Pos,
    pub radius_offset_xz: i32,
    pub foliage_height_offset: i32,
    pub size_x: i32,
    pub size_z: i32,
}

impl FoliageAttachment {
    /// `new FoliageAttachment(pos, radiusOffsetXZ, doubleTrunk)`。
    pub fn new(pos: Pos, radius_offset_xz: i32, double_trunk: bool) -> Self {
        let size = if double_trunk { 2 } else { 1 };
        Self {
            pos,
            radius_offset_xz,
            foliage_height_offset: 0,
            size_x: size,
            size_z: size,
        }
    }

    pub fn double_trunk(&self) -> bool {
        self.size_x == 2 && self.size_z == 2
    }
}

/// 一棵已解析的树。
///
/// A parsed tree.
pub struct TreeFeature {
    config: TreeConfig,
}

impl TreeFeature {
    /// 解析配置；含未移植的部分时返回 `None`。
    ///
    /// Parse the config; `None` when it uses an unported part.
    pub fn parse(config: &Value) -> Option<Self> {
        Some(Self {
            config: TreeConfig::parse(config)?,
        })
    }

    /// 官方 `TreeFeature.place`，返回是否放了方块。
    ///
    /// Official `TreeFeature.place`; returns whether anything was placed.
    pub fn place<'r, 'a, 'w>(
        &self,
        world: &'w mut FeatureWorld<'a>,
        random: &'r WorldgenRandom,
        pos: Pos,
        biome_at: &'r mut dyn FnMut(i32, i32, i32) -> u32,
        recursion: Option<&'r Recursion<'r>>,
    ) -> PlaceOutcome {
        let mut place = TreePlace::new(world);
        if !self.do_place(&mut place, random, pos) {
            return PlaceOutcome {
                supported: true,
                placed: false,
            };
        }
        if place.trunks.is_empty() && place.foliage.is_empty() {
            return PlaceOutcome {
                supported: true,
                placed: false,
            };
        }
        let mut outcome = PlaceOutcome {
            supported: true,
            placed: true,
        };
        for decorator in &self.config.decorators {
            let placed = decorator.place(&mut place, random, &mut *biome_at, recursion);
            outcome.supported &= placed.supported;
        }
        // `StructureTemplate.updateShapeAtEdge(level, 3, shape, ...)`：树的外表面两侧各做
        // 一次 `updateShape`，比如失去支撑的藤蔓在这里被移除。
        // `StructureTemplate.updateShapeAtEdge(level, 3, shape, ...)`: both sides of the
        // tree's outer faces run `updateShape`; unsupported vines are removed here.
        if let Some((shape, bounds)) = leaves::update_leaves(&mut place) {
            let _ = update_shape_at_edge(place.world, &shape, bounds.min, bounds.max);
        }
        outcome
    }

    /// 官方 `TreeFeature.doPlace`。
    ///
    /// Official `TreeFeature.doPlace`.
    fn do_place(
        &self,
        place: &mut TreePlace<'_, '_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        let config = &self.config;
        let tree_height = config.trunk.tree_height(random);
        let foliage_height = config.foliage.foliage_height(random, tree_height);
        let trunk_height = tree_height - foliage_height;
        let leaf_radius = config.foliage.foliage_radius(random, trunk_height);
        let trunk_origin = match &config.root {
            Some(root) => root.trunk_origin(origin, random),
            None => origin,
        };
        let min_y = origin.1.min(trunk_origin.1);
        let max_y = origin.1.max(trunk_origin.1) + tree_height + 1;
        // `level.getMaxY() + 1` 就是 `minY + height`。
        // `level.getMaxY() + 1` equals `minY + height`.
        let world_top = place.world.min_y + place.world.height;
        if min_y < place.world.min_y + 1 || max_y > world_top {
            return false;
        }
        let clipped = self.max_free_tree_height(place, tree_height, trunk_origin);
        let tall_enough = clipped >= tree_height
            || config
                .size
                .min_clipped_height
                .is_some_and(|min| clipped >= min);
        if !tall_enough {
            return false;
        }
        if let Some(root) = &config.root
            && !root.place_roots(place, random, origin, trunk_origin)
        {
            return false;
        }
        let attachments = config
            .trunk
            .place_trunk(place, config, random, clipped, trunk_origin);
        for attachment in &attachments {
            config.foliage.create_foliage(
                place,
                config,
                random,
                clipped,
                attachment,
                foliage_height,
                leaf_radius,
            );
        }
        true
    }

    /// 官方 `TreeFeature.getMaxFreeTreeHeight`。
    ///
    /// Official `TreeFeature.getMaxFreeTreeHeight`.
    fn max_free_tree_height(&self, place: &TreePlace<'_, '_>, max_height: i32, pos: Pos) -> i32 {
        for y in 0..=max_height + 1 {
            let r = self.config.size.size_at_height(max_height, y);
            for x in -r..=r {
                for z in -r..=r {
                    let at = (pos.0 + x, pos.1 + y, pos.2 + z);
                    let blocked = !self.config.trunk.is_free(place.world, at)
                        || (!self.config.ignore_vines && is_vine(place.world, at));
                    if blocked {
                        return y - 2;
                    }
                }
            }
        }
        max_height
    }
}

/// 一次放置里官方那四个 `HashSet<BlockPos>` 和世界。
///
/// The world plus the four `HashSet<BlockPos>` of one official placement.
pub(crate) struct TreePlace<'a, 'w> {
    pub world: &'w mut FeatureWorld<'a>,
    pub roots: JavaHashSet,
    pub trunks: JavaHashSet,
    pub foliage: JavaHashSet,
    pub decorations: JavaHashSet,
}

impl<'a, 'w> TreePlace<'a, 'w> {
    fn new(world: &'w mut FeatureWorld<'a>) -> Self {
        Self {
            world,
            roots: JavaHashSet::new(),
            trunks: JavaHashSet::new(),
            foliage: JavaHashSet::new(),
            decorations: JavaHashSet::new(),
        }
    }

    pub fn get_block(&self, pos: Pos) -> u32 {
        self.world.get_block(pos.0, pos.1, pos.2)
    }

    /// 官方 `trunkSetter`：记下位置，再 `level.setBlock(pos, state, 19)`。
    ///
    /// Official `trunkSetter`: record the position, then `level.setBlock(pos, state, 19)`.
    pub fn set_trunk(&mut self, pos: Pos, state: u32) {
        self.trunks.insert(pos);
        self.world.set_tree_block(pos.0, pos.1, pos.2, state);
    }

    /// 官方 `rootSetter`。
    ///
    /// Official `rootSetter`.
    pub fn set_root(&mut self, pos: Pos, state: u32) {
        self.roots.insert(pos);
        self.world.set_tree_block(pos.0, pos.1, pos.2, state);
    }

    /// 官方 `FoliageSetter.set`。
    ///
    /// Official `FoliageSetter.set`.
    pub fn set_foliage(&mut self, pos: Pos, state: u32) {
        self.foliage.insert(pos);
        self.world.set_tree_block(pos.0, pos.1, pos.2, state);
    }
}

/// 树要查的几个方块标签，各只查一次。
///
/// The few block tags trees test, each looked up once.
fn cached_tag(cell: &'static OnceLock<&'static [i32]>, name: &str) -> &'static [i32] {
    cell.get_or_init(|| block_tag(name).unwrap_or(&[]))
}

fn state_in(ids: &[i32], state: u32) -> bool {
    block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

pub(crate) fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

pub(crate) fn is_log(state: u32) -> bool {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    state_in(cached_tag(&TAG, "minecraft:logs"), state)
}

pub(crate) fn is_leaves(state: u32) -> bool {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    state_in(cached_tag(&TAG, "minecraft:leaves"), state)
}

/// `TreeFeature.validTreePos`：空气或 `replaceable_by_trees`。
///
/// Official `TreeFeature.validTreePos`: air or `replaceable_by_trees`.
pub(crate) fn valid_tree_pos(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    let state = world.get_block(pos.0, pos.1, pos.2);
    is_air(state) || state_in(cached_tag(&TAG, "minecraft:replaceable_by_trees"), state)
}

/// `TreeFeature.isAirOrLeaves`。
pub(crate) fn is_air_or_leaves(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    is_air(state) || is_leaves(state)
}

/// `TreeFeature.isVine`：`minecraft:vine`。
///
/// Official `TreeFeature.isVine`: `minecraft:vine`.
fn is_vine(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    static VINE: OnceLock<Option<usize>> = OnceLock::new();
    let vine = *VINE.get_or_init(|| {
        block_by_name("minecraft:vine").and_then(|block| block_id_of_state(block.default_state))
    });
    let state = world.get_block(pos.0, pos.1, pos.2);
    vine.is_some() && block_id_of_state(state) == vine
}

/// `soil_beneath_tree` 规则里的 `cannot_replace_below_tree_trunk` 标签。
///
/// The `cannot_replace_below_tree_trunk` tag of the `soil_beneath_tree` rule.
pub(crate) fn cannot_replace_below_trunk(state: u32) -> bool {
    static TAG: OnceLock<&'static [i32]> = OnceLock::new();
    state_in(
        cached_tag(&TAG, "minecraft:cannot_replace_below_tree_trunk"),
        state,
    )
}

/// `BlockState.getValue` 的属性读取；方块没有这个属性时返回 `None`。
///
/// Read a property like `BlockState.getValue`; `None` when the block lacks it.
pub(crate) fn property_value(state: u32, name: &str) -> Option<&'static str> {
    let block = block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        if property.name == name {
            let index = (state - block.first_state) / stride % count;
            return Some(property.values[index as usize]);
        }
        stride *= count;
    }
    None
}

pub(crate) fn has_property(state: u32, name: &str) -> bool {
    block_of_state(state).is_some_and(|b| b.properties.iter().any(|p| p.name == name))
}

/// `Mth.floor(float)`。
pub(crate) fn floor_f32(value: f32) -> i32 {
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated - 1
    } else {
        truncated
    }
}

/// `Mth.floor(double)`。
pub(crate) fn floor_f64(value: f64) -> i32 {
    let truncated = value as i32;
    if value < f64::from(truncated) {
        truncated - 1
    } else {
        truncated
    }
}

/// `Direction.Plane.HORIZONTAL` 的顺序：NORTH、EAST、SOUTH、WEST，值是 `(stepX, stepZ)`。
///
/// The order of `Direction.Plane.HORIZONTAL`: NORTH, EAST, SOUTH, WEST as `(stepX, stepZ)`.
pub(crate) const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// `Direction.Plane.HORIZONTAL.getRandomDirection`：`faces[random.nextInt(4)]`，
/// 返回它在 [`HORIZONTAL`] 里的下标。
///
/// `Direction.Plane.HORIZONTAL.getRandomDirection`: `faces[random.nextInt(4)]`,
/// returned as an index into [`HORIZONTAL`].
pub(crate) fn random_horizontal(random: &WorldgenRandom) -> usize {
    random.next_int(4) as usize
}

/// 水平方向所在的轴名（`Direction.getAxis`）。
///
/// The axis name of a horizontal direction (`Direction.getAxis`).
pub(crate) fn axis_of(direction: usize) -> &'static str {
    if HORIZONTAL[direction].0 != 0 {
        "x"
    } else {
        "z"
    }
}
