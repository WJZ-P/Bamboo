//! 单个 placed feature 的叶子分派：`FeaturePlacer` 在最后一个 modifier 给出的每个候选
//! 位置调用一次 `Feature.place`。这里把「已移植的类型」解析成 [`Leaf`]，其余返回 `None`
//! 让主循环标 unsupported。组合类地物（`composition.rs`）递归复用同一随机源。
//!
//! Leaf dispatch for one placed feature: `FeaturePlacer` calls `Feature.place` once per
//! candidate from the last modifier. Ported types parse into a [`Leaf`]; others return `None`
//! so the main loop marks the chunk unsupported. Composition features recurse here,
//! sharing the same random source.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    blue_ice::{self, BlueIceFeature},
    disk::{self, DiskFeature},
    large_dripstone::{self, LargeDripstoneFeature},
    ores,
    placement::OreFeature,
    simple_block::SimpleBlockFeature,
    speleothem::{self, SpeleothemFeature},
    speleothem_cluster::{self, SpeleothemClusterFeature},
    spike::{self, SpikeFeature},
    spring::{self, SpringFeature},
    underwater_magma::{self, UnderwaterMagmaFeature},
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已移植、`place` 不再需要注册表解析的叶子地物，按类型 ID 字母序排列。
///
/// A ported leaf feature that needs no more registry parsing, in alphabetical
/// order of type ID.
pub enum Leaf {
    BlueIce(BlueIceFeature),
    Disk(DiskFeature),
    LargeDripstone(LargeDripstoneFeature),
    Ore(OreFeature),
    SimpleBlock(SimpleBlockFeature),
    Speleothem(SpeleothemFeature),
    SpeleothemCluster(SpeleothemClusterFeature),
    Spike(SpikeFeature),
    Spring(SpringFeature),
    UnderwaterMagma(UnderwaterMagmaFeature),
}

impl Leaf {
    /// 配置里嵌套的谓词 / 状态提供者是否都已移植。
    ///
    /// Whether every nested predicate / state provider in the config is ported.
    fn is_supported(&self) -> bool {
        match self {
            Self::Disk(disk) => disk.supported(),
            Self::Spike(spike) => spike.supported(),
            _ => true,
        }
    }
}

/// 按 `Feature` 类型 ID 解析配置；未移植、解析不了或嵌套谓词未移植的类型返回 `None`。
///
/// Parse a config by `Feature` type ID; return `None` for unported / unparsable types
/// or when a nested predicate is not ported.
pub fn parse_leaf(kind: &str, config: &Value) -> Option<Leaf> {
    let leaf = match kind {
        "minecraft:blue_ice" => Leaf::BlueIce(BlueIceFeature::parse(config).ok()?),
        "minecraft:disk" => Leaf::Disk(DiskFeature::parse(config).ok()?),
        "minecraft:large_dripstone" => {
            Leaf::LargeDripstone(LargeDripstoneFeature::parse(config).ok()?)
        }
        "minecraft:ore" => Leaf::Ore(OreFeature::parse(config).ok()?),
        "minecraft:simple_block" => Leaf::SimpleBlock(SimpleBlockFeature::parse(config)?),
        "minecraft:speleothem" => Leaf::Speleothem(SpeleothemFeature::parse(config).ok()?),
        "minecraft:speleothem_cluster" => {
            Leaf::SpeleothemCluster(SpeleothemClusterFeature::parse(config).ok()?)
        }
        "minecraft:spike" => Leaf::Spike(SpikeFeature::parse(config).ok()?),
        "minecraft:spring_feature" => Leaf::Spring(SpringFeature::parse(config).ok()?),
        "minecraft:underwater_magma" => {
            Leaf::UnderwaterMagma(UnderwaterMagmaFeature::parse(config).ok()?)
        }
        _ => return None,
    };
    leaf.is_supported().then_some(leaf)
}

/// `Feature.place` 的一个候选位置，返回是否放了方块（官方 `placedAny`）。
///
/// One candidate of `Feature.place`; returns whether a block was placed (`placedAny`).
pub fn place_leaf(
    leaf: &Leaf,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> bool {
    match leaf {
        Leaf::BlueIce(feature) => blue_ice::place(feature, world, random, pos),
        Leaf::Disk(disk) => disk::place(disk, world, random, pos),
        Leaf::LargeDripstone(feature) => large_dripstone::place(feature, world, random, pos),
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, pos),
        Leaf::Speleothem(feature) => speleothem::place(feature, world, random, pos),
        Leaf::SpeleothemCluster(feature) => speleothem_cluster::place(feature, world, random, pos),
        Leaf::Spike(spike) => spike::place(spike, world, random, pos),
        Leaf::Spring(spring) => spring::place(spring, world, pos),
        Leaf::UnderwaterMagma(feature) => underwater_magma::place(feature, world, random, pos),
    }
}
