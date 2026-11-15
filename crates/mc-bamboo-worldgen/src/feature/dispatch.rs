//! 单个 placed feature 的叶子分派：`FeaturePlacer` 在最后一个 modifier 给出的每个候选
//! 位置调用一次 `Feature.place`。这里把「已移植的类型」解析成 [`Leaf`]，其余返回 `None`
//! 让主循环标 unsupported。组合类地物（random_selector 等）以后在这里递归复用同一随机源。
//!
//! Leaf dispatch for one placed feature: `FeaturePlacer` calls `Feature.place` once per
//! candidate from the last modifier. Ported types parse into a [`Leaf`]; others return `None`
//! so the main loop marks the chunk unsupported. Composition features will recurse here
//! later, sharing the same random source.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    bamboo::BambooFeature,
    huge_mushroom::HugeMushroomFeature,
    multiface::MultifaceGrowthFeature,
    ores,
    placement::OreFeature,
    simple_block::SimpleBlockFeature,
    single_block_pillar::SingleBlockPillarFeature,
    tree::{TreeFeature, fallen::FallenTreeFeature},
    vines::VinesFeature,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已移植、`place` 不再需要注册表解析的叶子地物。
pub enum Leaf {
    Bamboo(Box<BambooFeature>),
    Ore(Box<OreFeature>),
    SimpleBlock(Box<SimpleBlockFeature>),
    Vines(Box<VinesFeature>),
    MultifaceGrowth(Box<MultifaceGrowthFeature>),
    Tree(Box<TreeFeature>),
    FallenTree(Box<FallenTreeFeature>),
    HugeMushroom(Box<HugeMushroomFeature>),
    /// `NoOpFeature`：什么都不做，但 `place` 返回 true。
    ///
    /// `NoOpFeature`: does nothing but returns true from `place`.
    NoOp,
    /// `SingleBlockPillarFeature`：沿竖直方向连续放方块。
    ///
    /// `SingleBlockPillarFeature`: stack one block along a vertical axis.
    SingleBlockPillar(Box<SingleBlockPillarFeature>),
}

/// 按 `Feature` 类型 ID 解析配置；未移植或解析不了的类型返回 `None`。
///
/// Parse a config by `Feature` type ID; return `None` for unported / unparsable types.
pub fn parse_leaf(kind: &str, config: &Value) -> Option<Leaf> {
    match kind {
        // 按 Feature 类型 ID 的字母序插入。
        // Inserted in Feature type ID order.
        "minecraft:bamboo" => Some(Leaf::Bamboo(Box::new(BambooFeature::parse(config)?))),
        "minecraft:fallen_tree" => {
            FallenTreeFeature::parse(config).map(|tree| Leaf::FallenTree(Box::new(tree)))
        }
        "minecraft:huge_brown_mushroom" | "minecraft:huge_red_mushroom" => {
            HugeMushroomFeature::parse(kind, config)
                .map(|mushroom| Leaf::HugeMushroom(Box::new(mushroom)))
        }
        "minecraft:multiface_growth" => Some(Leaf::MultifaceGrowth(Box::new(
            MultifaceGrowthFeature::parse(config)?,
        ))),
        // `MapCodec.unit(NoOpFeature::new)`：配置里没有字段，`place` 恒返回 true。
        // `MapCodec.unit(NoOpFeature::new)`: no fields, `place` always returns true.
        "minecraft:no_op" => Some(Leaf::NoOp),
        "minecraft:ore" => Some(Leaf::Ore(Box::new(OreFeature::parse(config).ok()?))),
        "minecraft:simple_block" => Some(Leaf::SimpleBlock(Box::new(SimpleBlockFeature::parse(
            config,
        )?))),
        "minecraft:single_block_pillar" => Some(Leaf::SingleBlockPillar(Box::new(
            SingleBlockPillarFeature::parse(config)?,
        ))),
        "minecraft:tree" => TreeFeature::parse(config).map(|tree| Leaf::Tree(Box::new(tree))),
        "minecraft:vines" => Some(Leaf::Vines(Box::new(VinesFeature::parse(config)?))),
        _ => None,
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
        Leaf::Bamboo(bamboo) => bamboo.place(world, random, pos),
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, pos),
        Leaf::Vines(vines) => vines.place(world, random, pos),
        Leaf::MultifaceGrowth(growth) => growth.place(world, random, pos),
        Leaf::Tree(tree) => tree.place(world, random, pos),
        Leaf::FallenTree(tree) => tree.place(world, random, pos),
        Leaf::HugeMushroom(mushroom) => mushroom.place(world, random, pos),
        Leaf::NoOp => true,
        Leaf::SingleBlockPillar(pillar) => pillar.place(world, random, pos),
    }
}
