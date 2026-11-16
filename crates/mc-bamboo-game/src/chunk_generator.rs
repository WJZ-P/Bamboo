//! 生成阶段函数的接口：`(阶段, 位置, 中心半成品, 邻居只读视图) → 中心的新内容`
//! （设计稿 5.3）。
//!
//! 两个实现：[`FlatGenerator`] 把超平坦的十个阶段合成一步、产物是模板；
//! [`OverworldGenerator`] 把 `worldgen` 的纯函数接进 `Staged` 多阶段流水线。
//! 接口按多阶段写好：Director 每个阶段派一个 `Task::Generate`，宿主把它交给
//! 内联执行器或计算池，计算池的线程上调用这里的 [`ChunkGenerator::generate`]。
//! 实现必须是纯函数：结果只由阶段、位置、种子和输入的区块决定，与哪条线程算、
//! 什么时候算无关。`features` 阶段读半径 1 的邻居只读快照、产出写邻居的增量，
//! 由世界线程按 `TaskId` 顺序合进各自的 `Arc` 快照（设计稿 5.3.1）。
//!
//! Generation stage functions (design 5.3). Two implementations: [`FlatGenerator`]
//! folds superflat's ten stages into one template-producing step;
//! [`OverworldGenerator`] wires `worldgen`'s pure functions into the `Staged`
//! pipeline. Implementations must be pure: the result depends only on the stage,
//! position, seed and inputs, never on the thread or timing. The `features`
//! stage reads radius-1 neighbor snapshots and emits neighbor deltas that the
//! world thread merges into `Arc` snapshots in `TaskId` order (design 5.3.1).

use std::{sync::Arc, time::Duration};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::ticket::ChunkStatus;
use tracing::error;

use crate::{
    region::RegionColumn,
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{
        GenerateOutput, SnapshotView, generate_features_stage, generate_structure_stage,
        generate_terrain_stage, passthrough_stage,
    },
    worldgen_light::generate_light_stage,
};

/// 邻居只读视图：`(位置, 不可变的 CARVERS 快照)`。`None` 表示计算线程现算。
///
/// Read-only neighbor view: `(pos, immutable CARVERS snapshot)`. `None` means
/// the compute thread regenerates it purely.
pub(crate) type NeighborView<'a> = SnapshotView<'a>;

/// 中心半成品的纯数据快照（CARVERS 地形），供 `features` 阶段拼 3×3 视图。
/// Clean CARVERS snapshot of the center, for the `features` 3×3 view.
pub(crate) type CenterSnapshot = Option<Arc<GeneratedChunk>>;

/// 一个生成器。实现要能在多条线程上同时调用。
pub(crate) trait ChunkGenerator: Send + Sync {
    /// 把中心推进到 `stage`。`center` 为 `None` 表示还没有半成品，从空区块开始。
    fn generate(
        &self,
        stage: ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<RegionColumn>>,
        center_snapshot: CenterSnapshot,
        neighbors: NeighborView<'_>,
    ) -> GenerateOutput;
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
        _center_snapshot: CenterSnapshot,
        _neighbors: NeighborView<'_>,
    ) -> GenerateOutput {
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        passthrough_stage(center)
    }
}

/// 主世界多阶段生成器：把 `worldgen` 的纯函数接进 Director 的 `Staged` 流水线。
///
/// `structure_starts` / `structure_references` 走纯函数重算并把 `structures` 标签
/// 交回世界线程；`biomes` 仍透传。`terrain`
/// 一步同时算出 quart 群系和跑到 CARVERS 的方块；`features` 读半径 1 的邻居
/// 快照、跑 `Generator::features()`，中心内容和写邻居的增量交回世界线程；
/// `light` 读同一份 3×3 邻居视图、算中心柱的生成期光照（天空光 + 方块光）交回；
/// `initialize_light` / `spawn` / `full` 原样透传。每一步都是
/// `(阶段, 位置, 配置, 输入) → 盒子 + 增量 + 光照` 的纯函数。
///
/// Overworld staged generator. The two structure stages recompute starts and
/// references and return the `structures` tag; `terrain` computes quart biomes
/// + CARVERS blocks;
/// `features` reads radius-1 neighbor snapshots, runs `Generator::features()`,
/// and returns the center content plus neighbor deltas; `light` reads the same
/// 3×3 view and computes the center column's generation-time sky + block light;
/// initialize_light/spawn/full pass through. Each step is pure.
#[derive(Debug, Clone)]
pub(crate) struct OverworldGenerator {
    config: WorldGenConfig,
}

impl OverworldGenerator {
    pub(crate) fn new(config: WorldGenConfig) -> Self {
        Self { config }
    }
}

impl ChunkGenerator for OverworldGenerator {
    fn generate(
        &self,
        stage: ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<RegionColumn>>,
        center_snapshot: CenterSnapshot,
        neighbors: NeighborView<'_>,
    ) -> GenerateOutput {
        // `ChunkStatus.LIGHT`：把官方 `initializeLight` + `light` 合成一步纯函数，
        // 读 3×3 邻居视图算出中心柱的天空光 + 方块光，装进 `GenerateOutput.light`
        // 交回世界线程（完工组装时落进交付盒子）。中心盒子仍是透传，组装时重建。
        // `ChunkStatus.LIGHT`: fold vanilla `initializeLight` + `light` into one
        // pure step; compute the center's sky + block light from the 3×3 view and
        // put it in `GenerateOutput.light`. The center box is still a pass-through,
        // rebuilt at final assembly.
        if stage == ChunkStatus::Light {
            let light =
                generate_light_stage(&self.config, pos.x, pos.z, center_snapshot, neighbors);
            let mut output = passthrough_stage(center);
            let _ = &mut output;
            match light {
                Ok(light) => output.light = Some(light),
                Err(error) => {
                    error!(%error, x = pos.x, z = pos.z, ?stage, "generation light failed; keeping column without baked light");
                }
            }
            return output;
        }
        // 生成阶段失败时要保留输入盒子：只在结构阶段 `take()`，失败分支还能取回。
        // Keep the input box for the failure path: only the structure stage `take()`s
        // it, so the error branch can still recover it.
        let mut center = center;
        let result = match stage {
            // `ChunkStatus.STRUCTURE_STARTS` / `STRUCTURE_REFERENCES`：纯函数重算
            // 半径 8 的邻域起点并登记引用；引用阶段把 `structures` 标签交回。
            // `STRUCTURE_STARTS` / `STRUCTURE_REFERENCES`: recompute the radius-8
            // neighborhood and register references; the references stage hands back
            // the `structures` tag.
            ChunkStatus::StructureStarts | ChunkStatus::StructureReferences => {
                generate_structure_stage(&self.config, pos.x, pos.z, center.take())
            }
            // `ChunkStatus.TERRAIN`：算 quart 群系 + doFill → buildSurface →
            // generateCarvers 的方块，并把 CARVERS 纯数据作为邻居快照交回。
            // `ChunkStatus.TERRAIN`: quart biomes + blocks through doFill →
            // buildSurface → generateCarvers, plus the clean CARVERS data.
            ChunkStatus::Terrain => generate_terrain_stage(&self.config, pos.x, pos.z),
            // `ChunkStatus.FEATURES`：3×3 视图 + `Generator::features()`。
            // `ChunkStatus.FEATURES`: 3×3 view + `Generator::features()`.
            ChunkStatus::Features => {
                generate_features_stage(&self.config, pos.x, pos.z, center_snapshot, neighbors)
            }
            _ => return passthrough_stage(center),
        };
        match result {
            Ok(output) => output,
            Err(error) => {
                error!(%error, x = pos.x, z = pos.z, ?stage, "world generation failed; keeping previous column");
                passthrough_stage(center)
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

    /// 按 `Staged` 流水线的十个阶段跑一遍 `OverworldGenerator`（透传阶段带空
    /// 邻居视图），确认最后交回的盒子是真正的噪声地形而不是超平坦模板。
    ///
    /// Run `OverworldGenerator` through all ten `Staged` stages (pass-through
    /// stages get an empty neighbor view) and check the final box is real noise
    /// terrain, not the superflat template.
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
            let output = generator.generate(stage, pos, column, None, &[]);
            column = Some(output.column);
        }
        let column = column.expect("pipeline produced a final column");
        let world = column.parts().column.chunk();
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
        assert!(!column.parts().column.untouched());
    }
}
