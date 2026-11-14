//! 地物类型分派：把配置类型 ID 解析成已移植的地物，交给各自的 `place`。
//!
//! 主循环（`run.rs`）只负责按全局 index 排序、播种随机数、跑修饰器链；具体放什么方块
//! 由这里的 [`Feature::place`] 决定。未移植的类型返回 `None`，主循环据此标 `unsupported`。
//!
//! Feature-type dispatch: parse a config type ID into a ported feature and place it.
//! The main loop (`run.rs`) only orders global indices, seeds randomness and runs the
//! modifier chain; [`Feature::place`] decides the blocks. Unported types return `None`
//! so the main loop marks the chunk unsupported.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    blue_ice::{self, BlueIceFeature},
    disk::{self, DiskFeature},
    large_dripstone::{self, LargeDripstoneFeature},
    placement::OreFeature,
    speleothem_cluster::{self, SpeleothemClusterFeature},
    spike::{self, SpikeFeature},
    spring::{self, SpringFeature},
    underwater_magma::{self, UnderwaterMagmaFeature},
    world::FeatureWorld,
};

/// 已移植的地物类型，按类型 ID 字母序排列。
///
/// Ported feature types, in alphabetical order of type ID.
#[derive(Debug, Clone)]
pub enum Feature {
    BlueIce(BlueIceFeature),
    Disk(DiskFeature),
    LargeDripstone(LargeDripstoneFeature),
    Ore(OreFeature),
    SpeleothemCluster(SpeleothemClusterFeature),
    Spike(SpikeFeature),
    Spring(SpringFeature),
    UnderwaterMagma(UnderwaterMagmaFeature),
}

impl Feature {
    /// `None` 表示本轮未移植这个类型（主循环标 `unsupported`，不消费随机数）。
    ///
    /// `None` means this type is not ported this round (main loop marks unsupported,
    /// consuming no randomness).
    pub fn parse(kind: &str, config: &Value) -> Result<Option<Self>, String> {
        Ok(Some(match kind {
            "minecraft:blue_ice" => Self::BlueIce(BlueIceFeature::parse(config)?),
            "minecraft:disk" => Self::Disk(DiskFeature::parse(config)?),
            "minecraft:large_dripstone" => {
                Self::LargeDripstone(LargeDripstoneFeature::parse(config)?)
            }
            "minecraft:ore" => Self::Ore(OreFeature::parse(config)?),
            "minecraft:speleothem_cluster" => {
                Self::SpeleothemCluster(SpeleothemClusterFeature::parse(config)?)
            }
            "minecraft:spike" => Self::Spike(SpikeFeature::parse(config)?),
            "minecraft:spring_feature" => Self::Spring(SpringFeature::parse(config)?),
            "minecraft:underwater_magma" => {
                Self::UnderwaterMagma(UnderwaterMagmaFeature::parse(config)?)
            }
            _ => return Ok(None),
        }))
    }

    /// 配置里嵌套的谓词/状态提供者是否都已移植。
    ///
    /// Whether every nested predicate / state provider in the config is ported.
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Disk(disk) => disk.supported(),
            Self::Spike(spike) => spike.supported(),
            _ => true,
        }
    }

    /// `Feature.place(level, generator, random, pos)`：`generator` 只对个别类型有用，
    /// 这里按各自模块的纯函数签名调用。
    ///
    /// `Feature.place(...)`: calls each module's pure function with what it needs.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: (i32, i32, i32),
    ) -> bool {
        match self {
            Self::BlueIce(feature) => blue_ice::place(feature, world, random, pos),
            Self::Disk(disk) => disk::place(disk, world, random, pos),
            Self::LargeDripstone(feature) => large_dripstone::place(feature, world, random, pos),
            Self::Ore(ore) => super::ores::place(ore, world, random, pos),
            Self::SpeleothemCluster(feature) => {
                speleothem_cluster::place(feature, world, random, pos)
            }
            Self::Spike(spike) => spike::place(spike, world, random, pos),
            Self::Spring(spring) => spring::place(spring, world, pos),
            Self::UnderwaterMagma(feature) => underwater_magma::place(feature, world, random, pos),
        }
    }
}
