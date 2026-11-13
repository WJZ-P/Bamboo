//! 方块的行为：放置、形状更新、破坏、右键和计划刻，是官方服务端逻辑的 Rust 移植。
//! 全部官方方块类都在 [`behaviour`] 里，斧子、打火石这些非方块物品对方块的右键在 `item_use` 里，
//! 水和岩浆的流动、桶在 [`fluid`] 里，
//! `mc-bamboo-vanilla` 的 `oracle` 命令用官方代码检查它们。
//!
//! 这是纯逻辑：给一个能读写方块的 [`Level`]，算出世界的变化。`mc-bamboo-game` 给它做适配，
//! 够距检查、序列号和发包都在调用方。入口在 [`use_item_on`]、[`use_item`]、[`destroy_block`]、
//! [`run_scheduled_ticks`] 和随机刻的 [`tick_chunk`]。随机数和官方逐位相同，见 [`random`]。
//!
//! 红石信号是真的，见 [`World::signal`] 和 [`World::has_neighbor_signal`]；
//! 流体流动和光照引擎在别处算好，这里只读，见 [`World::schedule_fluid_tick`] 和
//! [`World::raw_brightness`]。

pub mod archive;
pub mod behaviour;
mod block_container;
pub mod block_entity;
mod composter;
pub mod container;
pub mod context;
pub mod crafter;
pub mod dispense;
pub mod dispenser;
pub mod durability;
pub mod entity;
pub mod fluid;
pub mod gamerules;
pub mod hopper;
pub mod interact;
mod item_exchange;
mod item_use;
pub mod level;
mod machine_container;
pub mod mining;
pub mod placement;
pub mod predicate;
pub mod random;
pub mod random_tick;
pub mod recording;
pub mod shape;
pub mod state;
pub mod ticks;
pub mod weather;
pub mod world;
pub mod world_border;

#[cfg(test)]
mod block_entity_sleep;

#[cfg(test)]
mod testing;

pub use behaviour::Behaviour;
pub use behaviour::machines::{
    CampfireEntity, PistonMoving, ShulkerBoxEntity, ShulkerStatus, open_lid_box, reset_lectern,
    signal_lectern_page,
};
pub use block_entity::{BlockEntities, BlockEntity, BlockEvent, BlockEvents, ChunkBlockEntities};
pub use container::{Combined, Container, ShulkerSlots, Slots, redstone_signal};
pub use context::{Actor, BlockHit, PlaceContext, UseContext};
pub use dispenser::{click_if_empty, dispense_from};
pub use entity::{
    ChunkEntities, EntityPublic, EntityRow, EntityState, EntityStore, EntityUid, KindPublic,
    KindState,
};
pub use gamerules::{BlockGameRules, apply_text, block_drops_allowed};
pub use hopper::HopperEntity;
pub use interact::{
    InteractionResult, Outcome, attack, destroy_block, run_action_block_ticks,
    run_block_entity_tick, run_block_event_phase, run_due_ticks, run_entity_tick,
    run_scheduled_ticks, use_item, use_item_on, use_item_on_checked,
    use_item_on_checked_with_stack, use_item_on_with_stack,
};
pub use item_exchange::{ItemExchange, wash_dyed_color};
pub use item_use::{StickSelect, debug_stick_select};
pub use level::{
    ArchiveRedstone, ChunkRedstone, Effect, EntityTables, LecternFacts, Level, LevelEntities,
    LevelSideState, LevelTables, Observation, RedstoneEntry, RedstoneMemory, Trigger,
    custom_sound_name, intern_custom_sound,
};
pub use machine_container::entity_container_items;
pub use random::{LegacyRandom, LevelRandom, RandomSource, Seed128, XoroshiroRandom, shuffle};
pub use random_tick::{DEFAULT_RANDOM_TICK_SPEED, random_tick, tick_chunk, tick_thunder};
pub use shape::{Aabb, Boxes, CollisionContext, EntityContext};
pub use state::{BlockState, FluidState, Property, PropertyValue, SupportType};
pub use ticks::{BlockTicks, Busy, ChunkTicks, ScheduledFluidTick, ScheduledTick, TickPriority};
pub use weather::{BlockWeather, Precipitation, PrecipitationFacts};
pub use world::World;
pub use world_border::{
    ABSOLUTE_MAX_SIZE, BorderEvent, BorderSettings, BorderStatus, MAX_CENTER_COORDINATE, MAX_SIZE,
    WorldBorder,
};
