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
    huge_mushroom::HugeMushroomFeature,
    ores,
    placement::OreFeature,
    simple_block::SimpleBlockFeature,
    tree::{TreeFeature, fallen::FallenTreeFeature},
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已移植、`place` 不再需要注册表解析的叶子地物。
pub enum Leaf {
    Ore(OreFeature),
    SimpleBlock(SimpleBlockFeature),
    Tree(Box<TreeFeature>),
    FallenTree(Box<FallenTreeFeature>),
    HugeMushroom(Box<HugeMushroomFeature>),
}

/// 按 `Feature` 类型 ID 解析配置；未移植或解析不了的类型返回 `None`。
///
/// Parse a config by `Feature` type ID; return `None` for unported / unparsable types.
pub fn parse_leaf(kind: &str, config: &Value) -> Option<Leaf> {
    match kind {
        "minecraft:ore" => Some(Leaf::Ore(OreFeature::parse(config).ok()?)),
        "minecraft:simple_block" => Some(Leaf::SimpleBlock(SimpleBlockFeature::parse(config)?)),
        "minecraft:tree" => TreeFeature::parse(config).map(|tree| Leaf::Tree(Box::new(tree))),
        "minecraft:fallen_tree" => {
            FallenTreeFeature::parse(config).map(|tree| Leaf::FallenTree(Box::new(tree)))
        }
        "minecraft:huge_brown_mushroom" | "minecraft:huge_red_mushroom" => {
            HugeMushroomFeature::parse(kind, config)
                .map(|mushroom| Leaf::HugeMushroom(Box::new(mushroom)))
        }
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
        Leaf::Ore(ore) => ores::place(ore, world, random, pos),
        Leaf::SimpleBlock(feature) => feature.place(world, random, pos),
        Leaf::Tree(tree) => tree.place(world, random, pos),
        Leaf::FallenTree(tree) => tree.place(world, random, pos),
        Leaf::HugeMushroom(mushroom) => mushroom.place(world, random, pos),
    }
}
