//! 区块平面的最底层：ticket 和区块等级。
//!
//! `TicketTable` 是一个纯数据结构：给它票的增减、玩家的移动和每刻的推进，它算出每个区块的
//! **加载等级**和**模拟等级**，把等级变了的区块报给调用方。它不认识线程、区块内容和 IO，
//! 也不做加载/生成/卸载 —— 那是以后 Director 的事。
//!
//! 语义照官方 26.3：`TicketType` 的超时和标志、等级到 `FullChunkStatus` / `ChunkStatus` 的
//! 换算、两套 8 邻 +1 取最小的传播、超时票的清理、玩家票、强制加载和存盘标志。事实的出处、
//! 与官方有意不同的地方、oracle 和基准见 `docs/chunk-tickets.md`。
//!
//! 内部用哈希表存票和等级。**任何时候都不遍历哈希表来产生输出**：两平面的变化列表各自
//! 按 (x, z) 排序后线性归并，`persistent_tickets` / `forced_chunks` / `tickets_at` 都排序。
//! 同样的操作序列一定得到逐字节相同的输出。

mod level;
#[doc(hidden)]
pub mod profile;
mod spread;
mod table;

#[cfg(test)]
mod tests;

pub use level::{
    BLOCK_TICKING_LEVEL, ChunkStatus, DEFAULT_SIMULATION_DISTANCE, ENTITY_TICKING_LEVEL,
    FULL_CHUNK_LEVEL, FullChunkStatus, MAX_LEVEL, MAX_VIEW_DISTANCE, MIN_VIEW_DISTANCE,
    PLAYER_TICKET_LEVEL, RADIUS_AROUND_FULL_CHUNK, SIMULATION_MAX_LEVEL, TicketType,
    UNLOADED_LEVEL, flag, full_status, generation_status, is_block_ticking, is_entity_ticking,
    is_loaded,
};
pub use table::{ChunkHasher, ChunkHasherBuilder, LevelChange, SavedTicket, Ticket, TicketTable};
