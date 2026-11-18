//! 压测场景的接线：只给 `mc-bamboo-loadtest serve` 用，正式服务端不设这些钩子。
//!
//! 钩子挂在 `WorldConfig` 上，默认 `None`。`setup` 在世界线程每刻开头调一次，
//! 返回 `true` 表示区块交付完毕、场景已经摆好；之后每刻调 `tick` 驱动周期性输入。
//! 两个回调都跑在收包阶段，等价于一名管理员在世界线程上操作，不进三个方块侧阶段。
//!
//! Load-test wiring: only used by `mc-bamboo-loadtest serve`. The hooks live on
//! `WorldConfig` and default to `None`. `setup` runs at the start of each tick
//! until it reports that the chunks were delivered; `tick` then drives periodic
//! inputs. Both run in the inbox phase, like an operator's action on the world
//! thread, and never enter the three block-side phases.

use std::sync::OnceLock;

use mc_bamboo_blocks::{
    BlockEntity, BlockState, TickPriority, World,
    entity::{Entity, EntityKind},
    world::UPDATE_CLIENTS,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{container::ContainerKind, world::World as GameWorld};

/// 一个进程只装一套钩子。`serve` 在起服之前调 [`install`]；正式服务端从不调。
static HOOKS: OnceLock<BenchHooks> = OnceLock::new();

/// 装好这一进程的压测钩子。重复调用时第一次生效。
#[doc(hidden)]
pub fn install(hooks: BenchHooks) {
    let _ = HOOKS.set(hooks);
}

/// `Server` 建 `WorldConfig` 时读一次；没装钩子时是 `None`。
#[doc(hidden)]
pub fn installed() -> Option<BenchHooks> {
    HOOKS.get().copied()
}

/// 两个压测回调。`fn` 指针让 `WorldConfig` 仍然能派生 `Clone` / `PartialEq`；
/// 场景自己的「摆过没有」状态由调用方用进程内的静态量记。
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct BenchHooks {
    /// 每刻开头调用，直到返回 `true`。返回 `true` 那一刻之后不再调。
    pub setup: fn(&mut BenchWorld<'_>) -> bool,
    /// `setup` 成功之后的每一刻调用一次。
    pub tick: fn(&mut BenchWorld<'_>),
}

impl PartialEq for BenchHooks {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.setup, other.setup) && std::ptr::fn_addr_eq(self.tick, other.tick)
    }
}

impl Eq for BenchHooks {}

/// 压测回调能碰的世界。只暴露搭机器需要的几样东西，不公开 `Region` 的字段。
#[doc(hidden)]
pub struct BenchWorld<'a> {
    world: &'a mut GameWorld,
}

impl<'a> BenchWorld<'a> {
    pub(crate) fn new(world: &'a mut GameWorld) -> Self {
        Self { world }
    }

    /// 现在的岛内刻号（岛的时钟），驱动周期性输入用。精确模式下等于世界时间。
    ///
    /// The island's tick number, for periodic inputs; equal to the world time
    /// in exact mode.
    pub fn game_time(&self) -> i64 {
        self.world.islands.region.tick.game_time()
    }

    /// 世界出生点所在的区块。真实地形场景按它铺开预生成区域。
    pub fn spawn_chunk(&self) -> ChunkPos {
        self.world.master.global.state().spawn.pos.chunk()
    }

    /// 这些区块是不是都已交付。
    pub fn all_loaded(&self, chunks: &[ChunkPos]) -> bool {
        chunks
            .iter()
            .all(|&pos| self.world.islands.region.chunks.is_loaded(pos))
    }

    /// 正式的 `/forceload` 票：让这一片保持在已加载模式里并参与方块 tick。
    pub fn force_chunks(&mut self, chunks: &[ChunkPos]) {
        use mc_bamboo_world::director::TicketOp;
        self.world.apply_ticket_ops(
            chunks
                .iter()
                .map(|&pos| TicketOp::Forced { pos, forced: true }),
        );
    }

    /// 直接摆一个方块状态，不触发邻居更新。搭场景用，等价于读存档得到的方块。
    pub fn set_block(&mut self, pos: BlockPos, state: u32) {
        let _ = self.world.islands.region.chunks.set_block(pos, state);
        self.world.islands.region.touch(pos);
    }

    /// 像玩家拉杆那样换一格并通知邻居。周期性输入走这里。
    pub fn set_block_and_update(&mut self, pos: BlockPos, state: u32) {
        let (mut level, ticks) = self.world.islands.region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.set_block_and_update(pos, BlockState(state));
        let effects = world.into_effects();
        crate::interaction::send_effects(&mut self.world.islands.region, None, &effects);
        self.world.islands.region.touch(pos);
    }

    /// 写一张容器槽位表；`kind` 决定格数。
    pub fn set_container(&mut self, pos: BlockPos, kind: BenchContainer, slots: Vec<ItemStack>) {
        let kind = kind.into_game();
        let entry = crate::container::entry_in(&mut self.world.islands.region.containers, pos, kind);
        entry.slots = slots;
        self.world.islands.region.touch(pos);
    }

    /// 往源头箱子的第一个空格补一组物品，然后通知邻居。
    pub fn refill(&mut self, pos: BlockPos, stack: ItemStack) -> bool {
        let mut changed = false;
        let entry = self.world.islands.region.containers.get_mut(&pos);
        let Some(entry) = entry else {
            return false;
        };
        for slot in &mut entry.slots {
            if slot.is_empty() {
                *slot = stack;
                changed = true;
                break;
            }
        }
        if !changed {
            return false;
        }
        let state = self
            .world
            .islands
            .region
            .chunks
            .block(pos)
            .map(BlockState)
            .unwrap_or(BlockState::AIR);
        let (mut level, ticks) = self.world.islands.region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.set_block(pos, state, UPDATE_CLIENTS);
        world.update_neighbour_for_output_signal(pos, state.block());
        let effects = world.into_effects();
        crate::interaction::send_effects(&mut self.world.islands.region, None, &effects);
        self.world.islands.region.touch(pos);
        true
    }

    /// 登记一个会动的方块实体（漏斗、熔炉等）。
    pub fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        self.world.islands.region.block_entities.insert(entity);
    }

    /// 排一条方块计划刻。
    pub fn schedule_tick(
        &mut self,
        pos: BlockPos,
        block: usize,
        delay: i32,
        priority: TickPriority,
    ) {
        self.world
            .islands
            .region
            .block_ticks
            .schedule(pos, block, delay, priority);
    }

    /// 在这个位置放一个掉落物，和 `/loot spawn` 一样带默认拾取延迟。
    ///
    /// Drops an item entity here with the default pickup delay, like `/loot spawn`.
    pub fn spawn_item(&mut self, position: Vec3, stack: ItemStack) {
        let (mut level, ticks) = self.world.islands.region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut entity = Entity::ground_item(position, stack);
        if let EntityKind::Item(item) = &mut entity.kind {
            item.set_default_pickup_delay();
        }
        world.spawn_entity(entity);
    }

    /// 在线玩家的名字，按玩家 ID 排。
    ///
    /// Names of the online players, in player id order.
    pub fn player_names(&self) -> Vec<String> {
        self.world
            .islands
            .region
            .players
            .values()
            .map(|player| player.name.clone())
            .collect()
    }

    /// 用服务器控制台来源执行一条命令，等价于在控制台里敲。
    ///
    /// Runs one command with the server console as its source.
    pub fn console_command(&mut self, line: &str) {
        crate::command::console::on_console_command(
            &mut self.world.master.global,
            &mut self.world.islands.region,
            line,
        );
    }

    /// 当前方块状态，输入翻转时判断现在是红石块还是空气。
    pub fn block_state(&self, pos: BlockPos) -> u32 {
        self.world
            .islands
            .region
            .chunks
            .block(pos)
            .unwrap_or(mc_bamboo_data::blocks::VOID_AIR)
    }
}

/// 压测会用到的容器种类，避免把游戏内部的枚举公开。
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchContainer {
    Chest,
    Hopper,
}

impl BenchContainer {
    fn into_game(self) -> ContainerKind {
        match self {
            Self::Chest => ContainerKind::Chest,
            Self::Hopper => ContainerKind::Hopper,
        }
    }
}
