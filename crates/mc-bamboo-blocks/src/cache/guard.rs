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

/// 一次读的类别和参数。
///
/// 白名单里的每个实验读项都要对应一个变体；`guard_value` 按变体重算**同一形状**的
/// 值再哈希。少了哪个变体，`finish_record` 的守卫数量核对会把整条记录作废。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadMethod {
    /// `Level::block_state`：整段哈希已经覆盖，守卫只需确认触发点这一格。
    Block,
    MinY,
    MaxY,
    /// `LevelSideState::comparator_output`。
    Comparator(BlockPos),
    /// `has_scheduled_tick` / `will_tick_this_tick`，队列在 `BlockTicks` 里。
    TickQuery(BlockPos, usize, bool),
    /// 触发本身：这一格现在还是不是记录里的方块类型。
    Trigger,
    /// 排计划刻之前这一格这一种刻是不是已经排着。
    Dedup(i64, bool),
    /// 排方块事件之前它是不是已经在队列里。
    BlockEventDedup(crate::block_entity::BlockEvent),
    /// 方块实体表的一次读：`tag` 是 `read_entity` 的调用点编号，
    /// `sub` 区分 `has_block_entity`(0) / `is_taken`(1) 这类不带实体的读。
    /// 重算时按 `(tag, sub, entity_facts, entity_time_facts)` 哈希。
    BlockEntity(BlockPos, u8, u8),
    /// 方块实体里由绝对时刻派生的字段（现在活塞已相对化，恒为 0 或漏斗的差值）。
    EntityTime(BlockPos),
    /// `slots_at` / `with_slots` / `with_container` 的一次槽位读。
    Slots(BlockPos, u8),
    /// `unpack_loot_table` 之前这一格有没有表可解包。
    LootUnpack(BlockPos),
    /// `next_block_entity_ticker`(0) / `runs_block_entity_tick`(1) 的驱动读。
    /// sub=0 会推进队列，守卫阶段不能重查：只确认录下的位置就是当前触发的位置。
    Ticker(u8, BlockPos),
    /// `WorldBorder` 的 12 元组快照。
    WorldBorder,
    /// `K_REDSTONE` 的子标签（0..=11），重算时按 `(sub, 原始值)` 哈希。
    Redstone(Option<BlockPos>, u8),
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
