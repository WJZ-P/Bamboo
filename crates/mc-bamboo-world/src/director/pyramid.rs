//! 生成阶段的依赖表，照 26.3 `ChunkPyramid.GENERATION_PYRAMID` 的字节码。
//!
//! 每一步声明「需要哪些邻居到了哪个阶段」和「这一步会写到离中心多远的区块」。
//! 写范围不相交的任务可以同时跑；官方 `features` 的 `blockStateWriteRadius` 是 1。
//! 现在还没有真正的世界生成，超平坦把整条流水线合成一步；这张表和调度器仍然
//! 按完整形状写好，M4 接生成阶段函数时只需要换 [`PipelineKind`]。

use crate::ticket::ChunkStatus;

/// 一个生成步骤。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// 这一步做完之后中心到的阶段。
    pub stage: ChunkStatus,
    /// `(邻居必须至少到了的阶段, 切比雪夫半径)`。
    pub requires: &'static [(ChunkStatus, i32)],
    /// `ChunkStep.blockStateWriteRadius`：这一步会写到邻居区块的距离。
    pub write_radius: i32,
}

const S: ChunkStatus = ChunkStatus::StructureStarts;
const B: ChunkStatus = ChunkStatus::Biomes;
const T: ChunkStatus = ChunkStatus::Terrain;

/// `GENERATION_PYRAMID`：从空区块生成到完工的十步。
///
/// 出处：`ChunkPyramid.<clinit>` 里每个 `ChunkStep.Builder` 的 `addRequirement` 和
/// `blockStateWriteRadius`。`structure_references` / `biomes` 要半径 8 的
/// `structure_starts`；`terrain` 还要半径 1 的 `biomes`；`features` 要半径 1 的
/// `terrain` 并且会写到半径 1；`light` 要半径 1 的 `initialize_light`；
/// `spawn` 要半径 1 的 `biomes`。
pub const GENERATION: [Step; 10] = [
    Step {
        stage: ChunkStatus::Empty,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::StructureStarts,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::StructureReferences,
        requires: &[(S, 8)],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Biomes,
        requires: &[(S, 8)],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Terrain,
        requires: &[(S, 8), (B, 1)],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Features,
        requires: &[(S, 8), (T, 1)],
        write_radius: 1,
    },
    Step {
        stage: ChunkStatus::InitializeLight,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Light,
        requires: &[(ChunkStatus::InitializeLight, 1)],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Spawn,
        requires: &[(B, 1)],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Full,
        requires: &[],
        write_radius: 0,
    },
];

/// 从磁盘读的 `LOADING_PYRAMID`：每一步都没有依赖，磁盘上 `Status = full`
/// 的区块直接跳到组装。
pub const LOADING: [Step; 10] = [
    Step {
        stage: ChunkStatus::Empty,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::StructureStarts,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::StructureReferences,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Biomes,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Terrain,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Features,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::InitializeLight,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Light,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Spawn,
        requires: &[],
        write_radius: 0,
    },
    Step {
        stage: ChunkStatus::Full,
        requires: &[],
        write_radius: 0,
    },
];

/// 流水线要走哪些阶段。
///
/// `Flat` 是现在唯一的生成器：超平坦模板与坐标无关、光照已经算好，
/// 十步合成一个 `Full` 阶段。`Staged` 走完整的 `GENERATION` 表，
/// 给依赖图的测试用，也是 M4 接真生成时用的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PipelineKind {
    #[default]
    Flat,
    Staged,
}

impl PipelineKind {
    /// 这条流水线的阶段序列，`[0]` 是初始状态。
    pub fn stages(self) -> &'static [ChunkStatus] {
        match self {
            Self::Flat => &[ChunkStatus::Empty, ChunkStatus::Full],
            Self::Staged => &ChunkStatus::ALL,
        }
    }

    /// 某一步的依赖和写范围。`Flat` 只用合成的那一步，没有依赖。
    pub fn step(self, stage: ChunkStatus) -> Step {
        match self {
            Self::Flat => Step {
                stage: ChunkStatus::Full,
                requires: &[],
                write_radius: 0,
            },
            Self::Staged => GENERATION[stage_index(stage)],
        }
    }

    /// 这一步要写到离中心多远的区块。
    pub fn write_radius(self, stage: ChunkStatus) -> i32 {
        self.step(stage).write_radius
    }
}

/// `ChunkStatus` 在 `ALL` 里的下标。
pub const fn stage_index(stage: ChunkStatus) -> usize {
    stage as usize
}

/// 磁盘上的完工区块走 `LOADING_PYRAMID`：全部阶段都是空操作，直接组装。
pub fn loading_step(stage: ChunkStatus) -> Step {
    LOADING[stage_index(stage)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_table_matches_the_pyramid_bytecode() {
        assert_eq!(GENERATION[2].requires, &[(S, 8)]);
        assert_eq!(GENERATION[3].requires, &[(S, 8)]);
        assert_eq!(GENERATION[4].requires, &[(S, 8), (B, 1)]);
        assert_eq!(GENERATION[4].write_radius, 0);
        assert_eq!(GENERATION[5].requires, &[(S, 8), (T, 1)]);
        assert_eq!(GENERATION[5].write_radius, 1);
        assert_eq!(GENERATION[7].requires, &[(ChunkStatus::InitializeLight, 1)]);
        assert_eq!(GENERATION[8].requires, &[(B, 1)]);
        assert!(LOADING.iter().all(|step| step.requires.is_empty()));
    }

    #[test]
    fn stages_are_in_official_order() {
        let stages = PipelineKind::Staged.stages();
        assert_eq!(stages, &ChunkStatus::ALL);
        for (index, stage) in stages.iter().enumerate() {
            assert_eq!(stage_index(*stage), index);
        }
    }
}
