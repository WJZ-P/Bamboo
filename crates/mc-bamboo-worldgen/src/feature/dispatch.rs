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
    block_pile::{self, BlockPileFeature},
    blue_ice::{self, BlueIceFeature},
    disk::{self, DiskFeature},
    fossil::FossilFeature,
    geode::{self, GeodeFeature},
    huge_mushroom::HugeMushroomFeature,
    iceberg::{self, IcebergFeature},
    lake::{self, LakeFeature},
    large_dripstone::{self, LargeDripstoneFeature},
    monster_room::{self, MonsterRoomFeature},
    multiface::MultifaceGrowthFeature,
    ores,
    placement::{OreFeature, PlaceOutcome},
    sculk_patch::{self, SculkPatchFeature},
    simple_block::SimpleBlockFeature,
    speleothem::{self, SpeleothemFeature},
    speleothem_cluster::{self, SpeleothemClusterFeature},
    spike::{self, SpikeFeature},
    spring::{self, SpringFeature},
    template_feature::{self, TemplateFeature},
    tree::{TreeFeature, decorator::Recursion, fallen::FallenTreeFeature},
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
    BlockPile(BlockPileFeature),
    BlockColumn(BlockColumnFeature),
    BlueIce(BlueIceFeature),
    Disk(DiskFeature),
    FallenTree(Box<FallenTreeFeature>),
    /// `FossilFeature`：沙漠 / 沼泽地下的化石，基座与矿脉各放一次。
    ///
    /// `FossilFeature`: an underground fossil, placed twice (base then ore overlay).
    Fossil(Box<FossilFeature>),
    Geode(Box<GeodeFeature>),
    HugeMushroom(Box<HugeMushroomFeature>),
    Iceberg(IcebergFeature),
    Lake(Box<LakeFeature>),
    LargeDripstone(LargeDripstoneFeature),
    MonsterRoom(MonsterRoomFeature),
    Multiface(MultifaceGrowthFeature),
    Ore(OreFeature),
    /// `ScatteredOreFeature`：`origin` 周围最多 7 格的零散单点尝试。
    ///
    /// `ScatteredOreFeature`: scattered single-block tries within seven blocks.
    ScatteredOre(OreFeature),
    SculkPatch(SculkPatchFeature),
    SimpleBlock(SimpleBlockFeature),
    Speleothem(SpeleothemFeature),
    SpeleothemCluster(SpeleothemClusterFeature),
    Spike(SpikeFeature),
    Spring(SpringFeature),
    /// `TemplateFeature`：把一份结构模板直接放进世界。
    ///
    /// `TemplateFeature`: drop one structure template straight into the world.
    Template(Box<TemplateFeature>),
    Tree(Box<TreeFeature>),
    UnderwaterMagma(UnderwaterMagmaFeature),
    Vines(VinesFeature),
    VoidStartPlatform,
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
            Self::Template(feature) => feature.supported(),
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
        "minecraft:block_pile" => Leaf::BlockPile(BlockPileFeature::parse(config)?),
        "minecraft:block_column" => Leaf::BlockColumn(BlockColumnFeature::parse(config, lookup)?),
        "minecraft:blue_ice" => Leaf::BlueIce(BlueIceFeature::parse(config).ok()?),
        "minecraft:disk" => Leaf::Disk(DiskFeature::parse(config).ok()?),
        "minecraft:fallen_tree" => Leaf::FallenTree(Box::new(FallenTreeFeature::parse(config)?)),
        "minecraft:fossil" => Leaf::Fossil(Box::new(FossilFeature::parse(config)?)),
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
        // `ScatteredOreFeature` 复用 `AbstractOreFeature` 的 codec，配置结构相同。
        // `ScatteredOreFeature` shares the `AbstractOreFeature` codec, so the config
        // shape is identical.
        "minecraft:scattered_ore" => Leaf::ScatteredOre(OreFeature::parse(config).ok()?),
        "minecraft:sculk_patch" => Leaf::SculkPatch(SculkPatchFeature::parse(config)?),
        "minecraft:simple_block" => Leaf::SimpleBlock(SimpleBlockFeature::parse(config, lookup)?),
        "minecraft:speleothem" => Leaf::Speleothem(SpeleothemFeature::parse(config).ok()?),
        "minecraft:speleothem_cluster" => {
            Leaf::SpeleothemCluster(SpeleothemClusterFeature::parse(config).ok()?)
        }
        "minecraft:spike" => Leaf::Spike(SpikeFeature::parse(config).ok()?),
        "minecraft:spring_feature" => Leaf::Spring(SpringFeature::parse(config).ok()?),
        // `TemplateFeature` 的 processor 链只有 `desert_well` 用到的子集；别的
        // processor 类型会让 `supported()` 返回 false，整个 placed feature 标 unsupported。
        // `TemplateFeature` ports only the processor subset `desert_well` uses; any other
        // processor kind makes `supported()` false so the placed feature stays unsupported.
        "minecraft:template" => Leaf::Template(Box::new(TemplateFeature::parse(config).ok()?)),
        "minecraft:tree" => Leaf::Tree(Box::new(TreeFeature::parse(config)?)),
        "minecraft:underwater_magma" => {
            Leaf::UnderwaterMagma(UnderwaterMagmaFeature::parse(config).ok()?)
        }
        "minecraft:vines" => Leaf::Vines(VinesFeature::parse(config)?),
        "minecraft:void_start_platform" => Leaf::VoidStartPlatform,
        _ => return None,
    };
    leaf.is_supported().then_some(leaf)
}

/// `Feature.place` 的一个候选位置，返回是否放了方块（官方 `placedAny`）。
///
/// One candidate of `Feature.place`; returns whether a block was placed (`placedAny`).
pub fn place_leaf<'r>(
    leaf: &Leaf,
    world: &mut FeatureWorld<'_>,
    random: &'r WorldgenRandom,
    // `level.getRandom()`，只有 `MossyCarpetBlock.placeAt` 这类调用用它。
    // `level.getRandom()`, used only by calls such as `MossyCarpetBlock.placeAt`.
    region_random: &crate::random::Random,
    pos: Pos,
    biome_at: &'r mut dyn FnMut(i32, i32, i32) -> u32,
    recursion: Option<&'r Recursion<'r>>,
) -> PlaceOutcome {
    let placed = match leaf {
        Leaf::Bamboo(feature) => feature.place(world, random, pos),
        Leaf::BlockBlob(blob) => block_blob::place(blob, world, random, pos),
        Leaf::BlockPile(feature) => block_pile::place(feature, world, random, pos),
        Leaf::BlockColumn(feature) => feature.place(world, random, pos),
        Leaf::BlueIce(feature) => blue_ice::place(feature, world, random, pos),
        Leaf::Disk(disk) => disk::place(disk, world, random, pos),
        Leaf::FallenTree(tree) => tree.place(world, random, pos),
        Leaf::Fossil(fossil) => fossil.place(world, random, pos),
        Leaf::Geode(geode) => geode::place(geode, world, random, pos),
        Leaf::HugeMushroom(mushroom) => mushroom.place(world, random, pos),
        Leaf::Iceberg(iceberg) => iceberg::place(iceberg, world, random, pos),
        Leaf::Lake(lake) => lake::place(lake, world, random, pos),
        Leaf::LargeDripstone(feature) => large_dripstone::place(feature, world, random, pos),
        Leaf::MonsterRoom(room) => monster_room::place(room, world, random, pos),
        Leaf::Multiface(feature) => feature.place(world, random, pos),
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::ScatteredOre(ore) => ores::place_scattered(ore, world, random, pos),
        Leaf::SculkPatch(feature) => sculk_patch::place(feature, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, region_random, pos),
        Leaf::Speleothem(feature) => speleothem::place(feature, world, random, pos),
        Leaf::SpeleothemCluster(feature) => speleothem_cluster::place(feature, world, random, pos),
        Leaf::Spike(spike) => spike::place(spike, world, random, pos),
        Leaf::Spring(spring) => spring::place(spring, world, pos),
        Leaf::Template(feature) => template_feature::place(feature, world, random, pos),
        Leaf::Tree(tree) => {
            return tree.place(world, random, pos, biome_at, recursion);
        }
        Leaf::UnderwaterMagma(feature) => underwater_magma::place(feature, world, random, pos),
        Leaf::Vines(feature) => feature.place(world, random, pos),
        Leaf::VoidStartPlatform => super::flat::place_void_platform(world, pos),
    };
    PlaceOutcome {
        supported: true,
        placed,
    }
}
