//! `TreeFeature`：26.3 的橡树放置骨架，逐行照 `TreeFeature.doPlace` / `place` 翻译。
//!
//! 本轮只走橡树需要的分支：无 root placer、无 decorator、straight trunk + blob foliage。
//! 命中未移植的 placer / decorator 时在 [`TreeConfig::parse`] 返回 `None`，由分发把
//! 整个 placed feature 标成 unsupported。
//!
//! `TreeFeature`: the oak placement skeleton, translated line by line from official
//! `TreeFeature.doPlace` / `place`. Only oak's branches are ported (no root placer,
//! no decorator, straight trunk + blob foliage). Unported placers / decorators make
//! [`TreeConfig::parse`] return `None` so the placed feature is marked unsupported.

pub mod config;
pub mod foliage;
pub mod leaves;
pub mod trunk;

use serde_json::Value;

use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_of_state, block_tag, facts, registries, state_facts,
    state_fluid,
};

use crate::random::WorldgenRandom;

use crate::feature::{state::with_property, world::FeatureWorld};

use config::TreeConfig;

pub(crate) type Pos = (i32, i32, i32);

/// `FoliagePlacer.FoliageAttachment` 的五个字段（26.3）。
///
/// The five fields of official `FoliagePlacer.FoliageAttachment` (26.3).
pub(crate) struct FoliageAttachment {
    pub pos: Pos,
    pub radius_offset_xz: i32,
    pub foliage_height_offset: i32,
    pub double_trunk: bool,
}

/// 一棵已解析的橡树。
///
/// A parsed oak tree.
pub struct TreeFeature {
    config: TreeConfig,
}

impl TreeFeature {
    /// 解析配置；只接受橡树这一组已移植的分支，其余返回 `None`。
    ///
    /// Parse the config; `None` for anything outside the ported oak branches.
    pub fn parse(config: &Value) -> Option<Self> {
        Some(Self {
            config: TreeConfig::parse(config)?,
        })
    }

    /// `TreeFeature.place` 的一个候选位置，返回是否放了方块。
    ///
    /// One candidate of official `TreeFeature.place`; returns whether anything was placed.
    pub fn place(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, pos: Pos) -> bool {
        let mut place = TreePlace::new(world);
        if !self.do_place(&mut place, random, pos) {
            return false;
        }
        if place.trunks.is_empty() && place.foliage.is_empty() {
            return false;
        }
        // decorators 为空，官方这里直接跑 updateLeaves。
        // decorators are empty, so official code goes straight to updateLeaves.
        leaves::update_leaves(&mut place);
        true
    }

    /// 官方 `TreeFeature.doPlace`（89–125 行）。
    ///
    /// Official `TreeFeature.doPlace` (lines 89–125).
    fn do_place(
        &self,
        place: &mut TreePlace<'_, '_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        let tree_height = self.config.trunk.tree_height(random);
        let foliage_height = self.config.foliage.foliage_height();
        let leaf_radius = self.config.foliage.foliage_radius(random);
        // 无 root placer：trunkOrigin == origin。
        // No root placer: trunkOrigin == origin.
        let min_y = origin.1;
        let max_y = origin.1 + tree_height + 1;
        let world_max = place.world.min_y + place.world.height;
        if min_y > place.world.min_y && max_y <= world_max {
            let clipped = self.get_max_free_tree_height(place, tree_height, origin);
            let tall_enough = clipped >= tree_height
                || self
                    .config
                    .min_clipped_height
                    .is_some_and(|min| clipped >= min);
            if tall_enough {
                let attachments =
                    self.config
                        .trunk
                        .place_trunk(place, &self.config, random, clipped, origin);
                for attachment in &attachments {
                    self.config.foliage.create_foliage(
                        place,
                        &self.config,
                        random,
                        attachment,
                        foliage_height,
                        leaf_radius,
                    );
                }
                return true;
            }
        }
        false
    }

    /// 官方 `TreeFeature.getMaxFreeTreeHeight`（127–144 行）。
    ///
    /// Official `TreeFeature.getMaxFreeTreeHeight` (lines 127–144).
    fn get_max_free_tree_height(
        &self,
        place: &TreePlace<'_, '_>,
        max_height: i32,
        pos: Pos,
    ) -> i32 {
        for y in 0..max_height + 2 {
            let r = self.config.size_at_height(max_height, y);
            for x in -r..=r {
                for z in -r..=r {
                    let at = (pos.0 + x, pos.1 + y, pos.2 + z);
                    let blocked = !is_free(place.world, at)
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

/// 一次放置收集到的方块：树干（`trunks`）和树叶（`foliage`），供 `updateLeaves` 用。
///
/// Blocks collected during one placement: `trunks` and `foliage`, for `updateLeaves`.
pub(crate) struct TreePlace<'a, 'w> {
    pub world: &'w mut FeatureWorld<'a>,
    pub trunks: Vec<Pos>,
    pub foliage: Vec<Pos>,
}

impl<'a, 'w> TreePlace<'a, 'w> {
    fn new(world: &'w mut FeatureWorld<'a>) -> Self {
        Self {
            world,
            trunks: Vec::new(),
            foliage: Vec::new(),
        }
    }

    pub fn get_block(&self, pos: Pos) -> u32 {
        self.world.get_block(pos.0, pos.1, pos.2)
    }

    pub fn set_trunk(&mut self, pos: Pos, state: u32) {
        self.world
            .set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
        self.trunks.push(pos);
    }

    pub fn set_foliage(&mut self, pos: Pos, state: u32) {
        self.world
            .set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
        self.foliage.push(pos);
    }
}

/// `TreeFeature.validTreePos`：空气或 `replaceable_by_trees`。
///
/// Official `TreeFeature.validTreePos`: air or `replaceable_by_trees`.
pub(crate) fn valid_tree_pos(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    state_facts(state) & facts::AIR != 0 || in_tag("minecraft:replaceable_by_trees", state)
}

/// `TrunkPlacer.isFree`：validTreePos 或 `LOGS`。
///
/// Official `TrunkPlacer.isFree`: validTreePos or `LOGS`.
fn is_free(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    valid_tree_pos(world, pos) || in_tag("minecraft:logs", state)
}

/// `TreeFeature.isVine`：`minecraft:vine`。
///
/// Official `TreeFeature.isVine`: `minecraft:vine`.
fn is_vine(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    let Some(vine) = block_by_name("minecraft:vine") else {
        return false;
    };
    block_id_of_state(state)
        .is_some_and(|id| id == block_id_of_state(vine.default_state).unwrap_or(usize::MAX))
}

/// `TrunkPlacer.placeLog` 的 setter（simple provider 不取随机数）。
///
/// The setter of official `TrunkPlacer.placeLog` (simple provider takes no random).
pub(crate) fn place_log(place: &mut TreePlace<'_, '_>, state: u32, pos: Pos) {
    place.set_trunk(pos, state);
}

/// `FoliagePlacer.tryPlaceLeaf`（169–186 行）。
///
/// Official `FoliagePlacer.tryPlaceLeaf` (lines 169–186).
pub(crate) fn try_place_leaf(
    place: &mut TreePlace<'_, '_>,
    config: &TreeConfig,
    _random: &WorldgenRandom,
    pos: Pos,
) {
    let current = place.get_block(pos);
    let persistent = property_value(current, "persistent") == Some("true");
    if persistent || !valid_tree_pos(place.world, pos) {
        return;
    }
    let mut state = config.foliage_state;
    if has_property(state, "waterlogged") {
        let fluid = state_fluid(place.get_block(pos)).fluid;
        let water = fluid == registries::fluid::by_name("minecraft:water").unwrap_or(-1);
        state = with_property(state, "waterlogged", if water { "true" } else { "false" })
            .unwrap_or(state);
    }
    place.set_foliage(pos, state);
}

/// `BlockState.hasProperty` / `getValueOrElse` 的属性读取。
///
/// Reading a property for `BlockState.hasProperty` / `getValueOrElse`.
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

fn in_tag(name: &str, state: u32) -> bool {
    block_id_of_state(state)
        .is_some_and(|id| block_tag(name).is_some_and(|ids| ids.contains(&(id as i32))))
}

/// `soil_beneath_tree` 规则的 `cannot_replace_below_tree_trunk` 判定。
///
/// The `cannot_replace_below_tree_trunk` test of the `soil_beneath_tree` rule.
pub(crate) fn in_cannot_replace_tag(state: u32) -> bool {
    in_tag("minecraft:cannot_replace_below_tree_trunk", state)
}
