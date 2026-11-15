//! 单个 placed feature 的叶子分派：`FeaturePlacer` 在最后一个 modifier 给出的每个候选
//! 位置调用一次 `Feature.place`。这里把「已移植的类型」解析成 [`Leaf`]，其余返回 `None`
//! 让主循环标 unsupported。组合类地物（random_selector 等）会递归回到同一随机源。
//!
//! Leaf dispatch for one placed feature: `FeaturePlacer` calls `Feature.place` once per
//! candidate from the last modifier. Ported types parse into a [`Leaf`]; others return `None`
//! so the main loop marks the chunk unsupported. Composition features recurse here with the
//! same random source.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_blob::{self, BlockBlobFeature},
    blue_ice::{self, BlueIceFeature},
    disk::{self, DiskFeature},
    freeze_top_layer::{self, FreezeTopLayerFeature},
    geode::{self, GeodeFeature},
    huge_mushroom::HugeMushroomFeature,
    iceberg::{self, IcebergFeature},
    lake::{self, LakeFeature},
    large_dripstone::{self, LargeDripstoneFeature},
    monster_room::{self, MonsterRoomFeature},
    ores,
    placement::OreFeature,
    simple_block::SimpleBlockFeature,
    speleothem_cluster::SpeleothemClusterFeature,
    spike::{self, SpikeFeatureFeature},
    spring::{self, SpringFeature},
    tree::{TreeFeature, fallen::FallenTreeFeature},
    underwater_magma::{self, UnderwaterMagmaFeature},
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已移植、`place` 不再需要注册表解析的叶子地物；按类型 ID 字母序排列。
///
/// A ported leaf feature that `place` can run without further registry lookups, in
/// alphabetical order of type ID.
pub enum Leaf {
    BlockBlob(BlockBlobFeature),
    BlueIce(BlueIceFeature),
    FreezeTopLayer(FreezeTopLayerFeature),
    Disk(DiskFeature),
    Geode(Box<GeodeFeature>),
    Iceberg(IcebergFeature),
    Lake(Box<LakeFeature>),
    LargeDripstone(Box<LargeDripstoneFeature>),
    MonsterRoom(MonsterRoomFeature),
    Ore(OreFeature),
    SimpleBlock(SimpleBlockFeature),
    SpeleothemCluster(Box<SpeleothemClusterFeature>),
    Spike(SpikeFeatureFeature),
    Spring(SpringFeature),
    Tree(Box<TreeFeature>),
    FallenTree(Box<FallenTreeFeature>),
    HugeMushroom(Box<HugeMushroomFeature>),
    UnderwaterMagma(UnderwaterMagmaFeature),
}

/// 按 `Feature` 类型 ID 解析配置；未移植或解析不了的类型返回 `None`。
///
/// Parse a config by `Feature` type ID; return `None` for unported / unparsable types.
pub fn parse_leaf(kind: &str, config: &Value) -> Option<Leaf> {
    let leaf = match kind {
        "minecraft:block_blob" => Leaf::BlockBlob(BlockBlobFeature::parse(config).ok()?),
        "minecraft:blue_ice" => Leaf::BlueIce(BlueIceFeature::parse(config).ok()?),
        "minecraft:disk" => Leaf::Disk(DiskFeature::parse(config).ok()?),
        "minecraft:freeze_top_layer" => {
            Leaf::FreezeTopLayer(FreezeTopLayerFeature::parse(config).ok()?)
        }
        "minecraft:geode" => Leaf::Geode(Box::new(GeodeFeature::parse(config).ok()?)),
        "minecraft:iceberg" => Leaf::Iceberg(IcebergFeature::parse(config).ok()?),
        "minecraft:lake" => Leaf::Lake(Box::new(LakeFeature::parse(config).ok()?)),
        "minecraft:large_dripstone" => {
            Leaf::LargeDripstone(Box::new(LargeDripstoneFeature::parse(config).ok()?))
        }
        "minecraft:monster_room" => Leaf::MonsterRoom(MonsterRoomFeature::parse(config).ok()?),
        "minecraft:ore" => Leaf::Ore(OreFeature::parse(config).ok()?),
        "minecraft:simple_block" => Leaf::SimpleBlock(SimpleBlockFeature::parse(config)?),
        "minecraft:speleothem_cluster" => {
            Leaf::SpeleothemCluster(Box::new(SpeleothemClusterFeature::parse(config).ok()?))
        }
        "minecraft:spike" => Leaf::Spike(SpikeFeatureFeature::parse(config).ok()?),
        "minecraft:spring_feature" => Leaf::Spring(SpringFeature::parse(config).ok()?),
        "minecraft:tree" => Leaf::Tree(Box::new(TreeFeature::parse(config)?)),
        "minecraft:fallen_tree" => Leaf::FallenTree(Box::new(FallenTreeFeature::parse(config)?)),
        "minecraft:underwater_magma" => {
            Leaf::UnderwaterMagma(UnderwaterMagmaFeature::parse(config).ok()?)
        }
        "minecraft:huge_brown_mushroom" | "minecraft:huge_red_mushroom" => {
            Leaf::HugeMushroom(Box::new(HugeMushroomFeature::parse(kind, config)?))
        }
        _ => return None,
    };
    // 嵌套谓词 / 状态提供者没全移植时，同样按未移植处理。
    // Not fully ported nested predicates or state providers count as unported too.
    if is_supported(&leaf) {
        Some(leaf)
    } else {
        None
    }
}

/// 配置里嵌套的谓词 / 状态提供者是否都已移植。
///
/// Whether every nested predicate and state provider is ported.
fn is_supported(leaf: &Leaf) -> bool {
    match leaf {
        Leaf::BlockBlob(blob) => blob.supported(),
        Leaf::Disk(disk) => disk.supported(),
        Leaf::Geode(geode) => geode.supported(),
        Leaf::Lake(lake) => lake.supported(),
        Leaf::LargeDripstone(feature) => feature.supported(),
        Leaf::Spike(spike) => spike.supported(),
        _ => true,
    }
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
        Leaf::BlockBlob(blob) => block_blob::place(blob, world, random, pos),
        Leaf::BlueIce(feature) => blue_ice::place(feature, world, random, pos),
        Leaf::Disk(disk) => disk::place(disk, world, random, pos),
        Leaf::FreezeTopLayer(feature) => freeze_top_layer::place(feature, world, random, pos),
        Leaf::Geode(geode) => geode::place(geode, world, random, pos),
        Leaf::Iceberg(iceberg) => iceberg::place(iceberg, world, random, pos),
        Leaf::MonsterRoom(room) => monster_room::place(room, world, random, pos),
        Leaf::Lake(lake) => lake::place(lake, world, random, pos),
        Leaf::LargeDripstone(feature) => large_dripstone::place(feature, world, random, pos),
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, pos),
        Leaf::SpeleothemCluster(feature) => feature.place(world, random, pos),
        Leaf::Spike(spike) => spike::place(spike, world, random, pos),
        Leaf::Spring(spring) => spring::place(spring, world, pos),
        Leaf::Tree(tree) => tree.place(world, random, pos),
        Leaf::FallenTree(tree) => tree.place(world, random, pos),
        Leaf::HugeMushroom(mushroom) => mushroom.place(world, random, pos),
        Leaf::UnderwaterMagma(feature) => underwater_magma::place(feature, world, random, pos),
    }
}
