//! 不祥物品生成器，官方 `OminousItemSpawner`。
//!
//! 不祥试炼刷怪笼在玩家或已记录生物的上方生成它；到点掉下一件物品，或者朝下射出一枚
//! 投射物。`item` 是同步的 `DATA_ITEM`（ITEM_STACK，index 8），`spawn_item_after_ticks`
//! 进存档。客户端的粒子只在客户端跑，这里不做。
//!
//! The ominous item spawner, vanilla `OminousItemSpawner`. An ominous trial
//! spawner places one above a player or a tracked mob; when its timer expires it
//! drops an item or shoots a projectile downwards. `item` is the synced
//! `DATA_ITEM` slot, and `spawn_item_after_ticks` is saved. Client particles are
//! not part of the server side.

use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::item::ItemStack;

use super::{Entity, EntityKind};

/// `SPAWN_ITEM_DELAY_MIN` / `SPAWN_ITEM_DELAY_MAX`：`create` 里的闭区间。
pub const SPAWN_ITEM_DELAY_MIN: i32 = 60;
pub const SPAWN_ITEM_DELAY_MAX: i32 = 120;
/// `TICKS_BEFORE_ABOUT_TO_SPAWN_SOUND`：到点前 36 刻放预警音。
pub const TICKS_BEFORE_ABOUT_TO_SPAWN_SOUND: i64 = 36;
/// `spawnItem` 里的世界事件号。
pub const SPAWN_ITEM_LEVEL_EVENT: i32 = 3021;

/// `OminousItemSpawner` 自己的状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OminousItemSpawner {
    /// `DATA_ITEM`；掉完东西后清成空堆。
    pub item: ItemStack,
    /// `spawnItemAfterTicks`，进存档。
    pub spawn_item_after_ticks: i64,
}

impl OminousItemSpawner {
    /// `create(level, stack)`：`level.random.nextIntBetweenInclusive(60, 120)`。
    /// 世界随机源由调用方传进来（官方用的是 `level.getRandom()`）。
    pub fn new(stack: ItemStack, level_random: &crate::LegacyRandom) -> Self {
        Self {
            item: stack,
            spawn_item_after_ticks: i64::from(
                level_random.next_int_between_inclusive(SPAWN_ITEM_DELAY_MIN, SPAWN_ITEM_DELAY_MAX),
            ),
        }
    }
}

/// 造一个生成器实体；`spawn_item_after_ticks` 由调用方按世界随机源填好。
pub fn entity(position: Vec3, state: OminousItemSpawner) -> Entity {
    Entity::new(EntityKind::OminousItemSpawner(Box::new(state)), position)
}

/// `tick()` 的服务端分支。第一小步只放骨架：计时、声音、掉物和世界事件在下一步接上。
pub fn tick(_me: &mut Entity, _ctx: &mut dyn super::ctx::EntityCtx) {}
