//! 守卫读项：带真实值的方法枚举，重放前按它调同一个底层方法重算。

use std::hash::Hash;

use mc_bamboo_core::BlockPos;

use crate::{cache::hash_value, level::Trigger, recording::stable_hash};

/// 守卫要复核的一次读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GuardRead {
    pub method: ReadMethod,
    pub pos: Option<BlockPos>,
    /// 录制时按 `stable_hash(真实值)` 算出来的哈希。
    pub value: u64,
}

/// 第一版支持的读入口。白名单在 [`crate::cache::supported_read`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadMethod {
    /// `Level::block_state`：整段哈希已经覆盖，守卫只需确认触发点这一格。
    Block,
    MinY,
    MaxY,
    /// `LevelSideState::comparator_output`。
    Comparator(BlockPos),
    /// 红石记忆类读项，值形状由录制时的类别决定。
    /// `has_scheduled_tick` / `will_tick_this_tick`，队列在 `BlockTicks` 里。
    TickQuery(BlockPos, usize, bool),
    /// 触发本身：这一格现在还是不是记录里的方块类型。
    Trigger,
    /// 排计划刻之前这一格这一种刻是不是已经排着。
    Dedup(i64, bool),
    /// 排方块事件之前它是不是已经在队列里。
    BlockEventDedup(crate::block_entity::BlockEvent),
    /// 白名单之外的读，见到就退回解释。
    Unsupported(u8),
}

impl GuardRead {
    /// 录一次读，值用和实验录制层同一个稳定哈希。
    pub fn new(method: ReadMethod, pos: Option<BlockPos>, value: impl Hash) -> Self {
        Self {
            method,
            pos,
            value: stable_hash(&value),
        }
    }

    /// 触发本身的守卫：这一格的方块状态 ID 要和记录时相同。
    pub fn trigger(pos: BlockPos, state: u32) -> Self {
        Self::new(ReadMethod::Trigger, Some(pos), state)
    }

    /// 重放前重算一次哈希。`World` 层按 `method` 读对应的底层状态。
    pub fn recompute(&self, current: impl Hash) -> Option<u64> {
        (self.method != ReadMethod::Unsupported(0)).then(|| hash_value(&current))
    }
}

/// 段哈希守卫：存储层按段给一个 128 位值，录制时和重放前都要相等。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionGuard {
    pub chunk: mc_bamboo_core::ChunkPos,
    pub section_y: i32,
    pub hash: u128,
}

/// 触发点的方块状态读，录制时从 `Trigger` 拿到位置。
pub fn trigger_pos(trigger: Trigger) -> BlockPos {
    match trigger {
        Trigger::ScheduledTick { pos, .. }
        | Trigger::ScheduledFluidTick { pos, .. }
        | Trigger::BlockEvent { pos, .. }
        | Trigger::BlockEntityTick { pos, .. } => pos,
    }
}
