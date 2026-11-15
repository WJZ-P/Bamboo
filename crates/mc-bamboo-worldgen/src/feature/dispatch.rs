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
    bamboo::BambooFeature,
    block_blob::{self, BlockBlobFeature},
    block_column::BlockColumnFeature,
    blue_ice::{self, BlueIceFeature},
    disk::{self, DiskFeature},
    geode::{self, GeodeFeature},
    huge_mushroom::HugeMushroomFeature,
    iceberg::{self, IcebergFeature},
    lake::{self, LakeFeature},
    large_dripstone::{self, LargeDripstoneFeature},
    monster_room::{self, MonsterRoomFeature},
    multiface::MultifaceGrowthFeature,
    ores,
    placement::OreFeature,
    sculk_patch::{self, SculkPatchFeature},
    simple_block::SimpleBlockFeature,
    speleothem::{self, SpeleothemFeature},
    speleothem_cluster::{self, SpeleothemClusterFeature},
    spike::{self, SpikeFeature},
    spring::{self, SpringFeature},
    tree::{TreeFeature, fallen::FallenTreeFeature},
    underwater_magma::{self, UnderwaterMagmaFeature},
    vines::VinesFeature,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已移植、`place` 不再需要注册表解析的叶子地物，按类型 ID 字母序排列。
///
/// A ported leaf feature that needs no more registry parsing, in alphabetical
/// order of type ID.
pub enum Leaf {
    Bamboo(BambooFeature),
    BlockBlob(BlockBlobFeature),
    BlockColumn(BlockColumnFeature),
    BlueIce(BlueIceFeature),
    Disk(DiskFeature),
    FallenTree(Box<FallenTreeFeature>),
    Geode(Box<GeodeFeature>),
    HugeMushroom(Box<HugeMushroomFeature>),
    Iceberg(IcebergFeature),
    Lake(Box<LakeFeature>),
    LargeDripstone(LargeDripstoneFeature),
    MonsterRoom(MonsterRoomFeature),
    Multiface(MultifaceGrowthFeature),
    Ore(OreFeature),
    SculkPatch(SculkPatchFeature),
    SimpleBlock(SimpleBlockFeature),
    Speleothem(SpeleothemFeature),
    SpeleothemCluster(SpeleothemClusterFeature),
    Spike(SpikeFeature),
    Spring(SpringFeature),
    Tree(Box<TreeFeature>),
    UnderwaterMagma(UnderwaterMagmaFeature),
    Vines(VinesFeature),
}

impl Leaf {
    /// 配置里嵌套的谓词 / 状态提供者是否都已移植。
    ///
    /// Whether every nested predicate / state provider in the config is ported.
    fn is_supported(&self) -> bool {
        match self {
            Self::BlockBlob(blob) => blob.supported(),
            Self::Disk(disk) => disk.supported(),
            Self::Geode(geode) => geode.supported(),
            Self::Lake(lake) => lake.supported(),
            Self::LargeDripstone(feature) => feature.supported(),
            Self::Spike(spike) => spike.supported(),
            _ => true,
        }
    }
}

/// 按 `Feature` 类型 ID 解析配置；未移植、解析不了或嵌套谓词未移植的类型返回 `None`。
/// `lookup` 用来解析 `Holder<BlockStateProvider>` 的字符串引用。
///
/// Parse a config by `Feature` type ID; return `None` for unported / unparsable types
/// or when a nested predicate is not ported. `lookup` resolves
/// `Holder<BlockStateProvider>` string references.
pub fn parse_leaf<'a>(
    kind: &str,
    config: &'a Value,
    lookup: &dyn Fn(&str) -> Option<&'a Value>,
) -> Option<Leaf> {
    let leaf = match kind {
        "minecraft:bamboo" => Leaf::Bamboo(BambooFeature::parse(config)?),
        "minecraft:block_blob" => Leaf::BlockBlob(BlockBlobFeature::parse(config).ok()?),
        "minecraft:block_column" => Leaf::BlockColumn(BlockColumnFeature::parse(config, lookup)?),
        "minecraft:blue_ice" => Leaf::BlueIce(BlueIceFeature::parse(config).ok()?),
        "minecraft:disk" => Leaf::Disk(DiskFeature::parse(config).ok()?),
        "minecraft:fallen_tree" => Leaf::FallenTree(Box::new(FallenTreeFeature::parse(config)?)),
        "minecraft:geode" => Leaf::Geode(Box::new(GeodeFeature::parse(config).ok()?)),
        "minecraft:huge_brown_mushroom" | "minecraft:huge_red_mushroom" => {
            Leaf::HugeMushroom(Box::new(HugeMushroomFeature::parse(kind, config)?))
        }
        "minecraft:iceberg" => Leaf::Iceberg(IcebergFeature::parse(config).ok()?),
        "minecraft:lake" => Leaf::Lake(Box::new(LakeFeature::parse(config).ok()?)),
        "minecraft:large_dripstone" => {
            Leaf::LargeDripstone(LargeDripstoneFeature::parse(config).ok()?)
        }
        "minecraft:monster_room" => Leaf::MonsterRoom(MonsterRoomFeature::parse(config).ok()?),
        "minecraft:multiface_growth" => Leaf::Multiface(MultifaceGrowthFeature::parse(config)?),
        "minecraft:ore" => Leaf::Ore(OreFeature::parse(config).ok()?),
        "minecraft:sculk_patch" => Leaf::SculkPatch(SculkPatchFeature::parse(config)?),
        "minecraft:simple_block" => Leaf::SimpleBlock(SimpleBlockFeature::parse(config, lookup)?),
        "minecraft:speleothem" => Leaf::Speleothem(SpeleothemFeature::parse(config).ok()?),
        "minecraft:speleothem_cluster" => {
            Leaf::SpeleothemCluster(SpeleothemClusterFeature::parse(config).ok()?)
        }
        "minecraft:spike" => Leaf::Spike(SpikeFeature::parse(config).ok()?),
        "minecraft:spring_feature" => Leaf::Spring(SpringFeature::parse(config).ok()?),
        "minecraft:tree" => Leaf::Tree(Box::new(TreeFeature::parse(config)?)),
        "minecraft:underwater_magma" => {
            Leaf::UnderwaterMagma(UnderwaterMagmaFeature::parse(config).ok()?)
        }
        "minecraft:vines" => Leaf::Vines(VinesFeature::parse(config)?),
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
        Leaf::Bamboo(feature) => feature.place(world, random, pos),
        Leaf::BlockBlob(blob) => block_blob::place(blob, world, random, pos),
        Leaf::BlockColumn(feature) => feature.place(world, random, pos),
        Leaf::BlueIce(feature) => blue_ice::place(feature, world, random, pos),
        Leaf::Disk(disk) => disk::place(disk, world, random, pos),
        Leaf::FallenTree(tree) => tree.place(world, random, pos),
        Leaf::Geode(geode) => geode::place(geode, world, random, pos),
        Leaf::HugeMushroom(mushroom) => mushroom.place(world, random, pos),
        Leaf::Iceberg(iceberg) => iceberg::place(iceberg, world, random, pos),
        Leaf::Lake(lake) => lake::place(lake, world, random, pos),
        Leaf::LargeDripstone(feature) => large_dripstone::place(feature, world, random, pos),
        Leaf::MonsterRoom(room) => monster_room::place(room, world, random, pos),
        Leaf::Multiface(feature) => feature.place(world, random, pos),
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::SculkPatch(feature) => sculk_patch::place(feature, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, pos),
        Leaf::Speleothem(feature) => speleothem::place(feature, world, random, pos),
        Leaf::SpeleothemCluster(feature) => speleothem_cluster::place(feature, world, random, pos),
        Leaf::Spike(spike) => spike::place(spike, world, random, pos),
        Leaf::Spring(spring) => spring::place(spring, world, pos),
        Leaf::Tree(tree) => tree.place(world, random, pos),
        Leaf::UnderwaterMagma(feature) => underwater_magma::place(feature, world, random, pos),
        Leaf::Vines(feature) => feature.place(world, random, pos),
    }
}
