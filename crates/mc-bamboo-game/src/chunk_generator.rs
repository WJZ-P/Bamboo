//! 生成阶段函数的接口：`(阶段, 位置, 中心半成品, 邻居只读视图) → 中心的新内容`
//! （设计稿 5.3）。
//!
//! 两个实现：[`FlatGenerator`] 把超平坦的十个阶段合成一步、产物是模板；
//! [`OverworldGenerator`] 把 `worldgen` 的纯函数接进 `Staged` 多阶段流水线。
//! 接口按多阶段写好：Director 每个阶段派一个 `Task::Generate`，宿主把它交给
//! 内联执行器或计算池，计算池的线程上调用这里的 [`ChunkGenerator::generate`]。
//! 实现必须是纯函数：结果只由阶段、位置、种子和输入的区块决定，与哪条线程算、
//! 什么时候算无关。
//!
//! Generation stage functions (design 5.3). Two implementations: [`FlatGenerator`]
//! folds superflat's ten stages into one template-producing step;
//! [`OverworldGenerator`] wires `worldgen`'s pure functions into the `Staged`
//! pipeline. Implementations must be pure: the result depends only on the stage,
//! position, seed and inputs, never on the thread or timing.

use std::time::Duration;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::ticket::ChunkStatus;
use tracing::error;

use crate::{
    region::RegionColumn,
    world_options::WorldGenConfig,
    worldgen::{build_region_column, generate_biomes, generate_chunk},
};

/// 邻居只读视图：`(位置, 它已经到的阶段, 盒子)`。
///
/// 现在 Director 派的 `Task::Generate` 不带邻居（超平坦不需要），这里总是空的。
/// M4 接真生成时，Director 里的半成品改存成不可变快照（`Arc`），视图只加引用计数、
/// 不复制；中心推进时写时复制。见 `docs/world-model-design.md` 5.3。
pub(crate) type NeighborView<'a> = &'a [(ChunkPos, ChunkStatus, Box<RegionColumn>)];

/// 一个生成器。实现要能在多条线程上同时调用。
pub(crate) trait ChunkGenerator: Send + Sync {
    /// 把中心推进到 `stage`。`center` 为 `None` 表示还没有半成品，从空区块开始。
    fn generate(
        &self,
        stage: ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<RegionColumn>>,
        neighbors: NeighborView<'_>,
    ) -> Box<RegionColumn>;
}

/// 超平坦：任何位置都是同一份模板，光照已经算好。
///
/// `delay` 只给压测用（`BAMBOO_FAKE_GEN_DELAY_US`）：每个区块额外睡这么久，
/// 模拟一个慢的真生成器，看计算池能不能把它移出世界线程。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct FlatGenerator {
    pub(crate) delay: Duration,
}

/// 调试开关：每个超平坦区块额外花多少微秒。只在压测里用，不进配置文件。
const FAKE_DELAY_ENV: &str = "BAMBOO_FAKE_GEN_DELAY_US";
/// 调试开关的上限，防止手误让每个区块睡上几分钟。
const MAX_FAKE_DELAY_US: u64 = 1_000_000;

impl FlatGenerator {
    /// 读调试环境变量；没设或值不合法时不加延迟。
    pub(crate) fn from_env() -> Self {
        let micros = std::env::var(FAKE_DELAY_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0)
            .min(MAX_FAKE_DELAY_US);
        Self {
            delay: Duration::from_micros(micros),
        }
    }
}

impl ChunkGenerator for FlatGenerator {
    fn generate(
        &self,
        _stage: ChunkStatus,
        _pos: ChunkPos,
        center: Option<Box<RegionColumn>>,
        _neighbors: NeighborView<'_>,
    ) -> Box<RegionColumn> {
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        center.unwrap_or_else(|| Box::new(RegionColumn::from_template()))
    }
}

/// 主世界多阶段生成器：把 `worldgen` 的纯函数接进 Director 的 `Staged` 流水线。
///
/// `structure_starts` / `structure_references` 先接成空实现（结构队员填）；
/// `biomes` 写 quart 群系；`terrain` 写跑到 CARVERS 的方块并重算高度图；
/// `features` / `initialize_light` / `light` / `spawn` / `full` 本轮先原样透传
/// （邻居只读快照、增量合并、生成时光照还没接，见 README 已知限制）。
/// 每一步都是 `(阶段, 位置, 配置) → 盒子` 的纯函数，与线程、调度无关。
///
/// Overworld staged generator: wires `worldgen`'s pure functions into the
/// Director's `Staged` pipeline. `structure_starts` / `structure_references` are
/// empty stubs (for the structures teammate); `biomes` writes quart biomes;
/// `terrain` writes blocks through CARVERS and recomputes heightmaps;
/// `features` / `initialize_light` / `light` / `spawn` / `full` pass through for
/// now (neighbor snapshots, delta merge and in-pipeline light are not wired;
/// see README known limitations). Each step is a pure
/// `(stage, pos, config) → box` function, independent of thread/timing.
#[derive(Debug, Clone)]
pub(crate) struct OverworldGenerator {
    config: WorldGenConfig,
}

impl OverworldGenerator {
    pub(crate) fn new(config: WorldGenConfig) -> Self {
        Self { config }
    }
}

/// 没有半成品时先造一个空模板盒子，后面的阶段再替换内容。
/// Make an empty template box when there is no half-built column yet.
fn empty_center(center: Option<Box<RegionColumn>>) -> Box<RegionColumn> {
    center.unwrap_or_else(|| Box::new(RegionColumn::from_template()))
}

impl ChunkGenerator for OverworldGenerator {
    fn generate(
        &self,
        stage: ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<RegionColumn>>,
        _neighbors: NeighborView<'_>,
    ) -> Box<RegionColumn> {
        let result = match stage {
            // `ChunkStatus.BIOMES`：只写 quart 群系，方块仍是空气。
            // `ChunkStatus.BIOMES`: write quart biomes only, blocks remain air.
            ChunkStatus::Biomes => generate_biomes(&self.config, pos.x, pos.z)
                .and_then(|chunk| build_region_column(&chunk)),
            // `ChunkStatus.TERRAIN`：一步跑 doFill → buildSurface → generateCarvers。
            // `ChunkStatus.TERRAIN`: run doFill → buildSurface → generateCarvers.
            ChunkStatus::Terrain => generate_chunk(&self.config, pos.x, pos.z)
                .and_then(|chunk| build_region_column(&chunk)),
            _ => return empty_center(center),
        };
        match result {
            Ok(column) => column,
            Err(error) => {
                error!(%error, x = pos.x, z = pos.z, ?stage, "world generation failed; keeping previous column");
                empty_center(center)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::registries;
    use mc_bamboo_world::{MIN_Y, ticket::ChunkStatus};

    use super::*;
    use crate::world_options::LevelType;

    /// 按 `Staged` 流水线的十个阶段跑一遍 `OverworldGenerator`，确认最后交回的盒子
    /// 是真正的噪声地形（有非空气方块、有群系），而不是超平坦模板。
    ///
    /// Run `OverworldGenerator` through all ten `Staged` stages and check the
    /// final box is real noise terrain (non-air blocks, real biomes), not the
    /// superflat template.
    #[test]
    fn staged_pipeline_produces_terrain_instead_of_the_flat_template() {
        let config = WorldGenConfig {
            seed: 1234,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let generator = OverworldGenerator::new(config);
        let pos = ChunkPos::new(2, -3);
        let mut column: Option<Box<RegionColumn>> = None;
        for stage in ChunkStatus::ALL {
            column = Some(generator.generate(stage, pos, column, &[]));
        }
        let column = column.expect("pipeline produced a final column");
        let world = column.parts().column.chunk();
        // 噪声地形从世界底部一路填到地表，跨好几个段；超平坦模板只有最底一个段
        // 有四层方块。多于一个非空段就说明是真正的地形而不是模板。
        // Noise terrain fills from the world bottom up through several sections;
        // the flat template has four layers in only the bottom section. More than
        // one non-empty section means real terrain, not the template.
        let non_empty = world
            .sections()
            .iter()
            .filter(|section| !section.is_empty())
            .count();
        assert!(
            non_empty > 1,
            "expected many filled sections, got {non_empty}"
        );
        let biome = world.biome_at(0, MIN_Y, 0);
        assert!(biome < registries::biome::TOTAL);
        // 生成的区块不是「原样」（会存档），模板才是原样。
        // Generated columns are not pristine (they get saved); templates are.
        assert!(!column.parts().column.untouched());
    }
}
