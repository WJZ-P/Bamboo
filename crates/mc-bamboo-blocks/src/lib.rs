//! 方块的行为：放置、形状更新、破坏、右键和计划刻，是官方服务端逻辑的 Rust 移植。
//! 全部官方方块类都在 [`behaviour`] 里，`mc-bamboo-vanilla` 的 `oracle` 命令用官方代码检查它们。
//!
//! 这是纯逻辑：给一个能读写方块的 [`Level`]，算出世界的变化。`mc-bamboo-game` 给它做适配，
//! 够距检查、序列号和发包都在调用方。入口在 [`use_item_on`]、[`use_item`]、[`destroy_block`]
//! 和 [`run_scheduled_ticks`]。
//!
//! 红石信号是真的，见 [`World::signal`] 和 [`World::has_neighbor_signal`]。
//! 世界里没有流体流动和光照引擎，见 [`World::schedule_fluid_tick`] 和 [`World::raw_brightness`]。

pub mod behaviour;
pub mod context;
pub mod interact;
pub mod level;
pub mod placement;
pub mod shape;
pub mod state;
pub mod ticks;
pub mod world;

#[cfg(test)]
mod testing;

pub use behaviour::Behaviour;
pub use context::{Actor, BlockHit, PlaceContext, UseContext};
pub use interact::{
    InteractionResult, Outcome, destroy_block, run_scheduled_ticks, use_item, use_item_on,
};
pub use level::{Effect, Level, RedstoneMemory};
pub use shape::{Aabb, Boxes, CollisionContext};
pub use state::{BlockState, FluidState, Property, PropertyValue, SupportType};
pub use ticks::{BlockTicks, ScheduledTick, TickPriority};
pub use world::World;
