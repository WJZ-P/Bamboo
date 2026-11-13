//! 生成阶段函数的接口：`(阶段, 位置, 中心半成品, 邻居只读视图) → 中心的新内容`
//! （设计稿 5.3）。
//!
//! 现在只有超平坦一个实现，十个阶段合成一步，产物是与坐标无关的模板。
//! 接口按多阶段写好：Director 每个阶段派一个 `Task::Generate`，宿主把它交给
//! 内联执行器或计算池，计算池的线程上调用这里的 [`ChunkGenerator::generate`]。
//! 实现必须是纯函数：结果只由阶段、位置、种子和输入的区块决定，与哪条线程算、
//! 什么时候算无关。
//!
//! Generation stage functions (design 5.3). Only the superflat generator exists;
//! it folds all stages into one. Implementations must be pure: the result depends
//! only on the stage, position, seed and inputs, never on the thread or timing.

use std::time::Duration;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::ticket::ChunkStatus;

use crate::region::RegionColumn;

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
