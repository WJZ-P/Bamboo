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
///
/// 多岛时按位置找岛（岛的实施计划第 2 节第 34 行）：每个按位置的操作落到已加载这个
/// 区块的、停放着的岛上；哪个岛都没加载、或者那个岛正在 tick 线程上跑（混合驱动下落后
/// 了）时什么都不做。只有一个岛时就是原来的样子。
///
/// The world the load-test hooks may touch. With several islands every
/// positional operation goes to the parked island that has the chunk loaded
/// (island plan section 2, row 34); when none has it, or that island is
/// running on a tick thread (lagging under the hybrid drive), it does nothing.
/// With one island this is the old behaviour.
#[doc(hidden)]
pub struct BenchWorld<'a> {
    world: &'a mut GameWorld,
}

impl<'a> BenchWorld<'a> {
    pub(crate) fn new(world: &'a mut GameWorld) -> Self {
        Self { world }
    }

    /// 已加载 `pos` 所在区块的停放着的岛的 `Region`。
    fn region_at(&mut self, pos: BlockPos) -> Option<&mut crate::region::Region> {
        let id = self.world.islands.island_of_chunk(pos.chunk())?;
        Some(&mut self.world.islands.get_mut(id)?.region)
    }

    /// 现在的岛内刻号（家岛的时钟），驱动周期性输入用。精确模式下等于世界时间。
    ///
    /// The island's tick number (the home island's clock), for periodic
    /// inputs; equal to the world time in exact mode.
    pub fn game_time(&self) -> i64 {
        if self
            .world
            .islands
            .is_parked(mc_bamboo_world::director::HOME_REGION)
        {
            self.world.islands.region.tick.game_time()
        } else {
            self.world.master.global.time().0
        }
    }

    /// 已加载 `pos` 的停放着的岛的时钟；多岛时各岛的时钟可以不同（落后的岛慢）。没有这样
    /// 的岛时是 `None`。
    ///
    /// The clock of the parked island that has `pos` loaded; with several
    /// islands clocks may differ (a lagging island is behind). `None` when no
    /// such island exists.
    pub fn game_time_at(&self, pos: BlockPos) -> Option<i64> {
        let id = self.world.islands.island_of_chunk(pos.chunk())?;
        Some(self.world.islands.get(id)?.region.tick.game_time())
    }

    /// 世界出生点所在的区块。真实地形场景按它铺开预生成区域。
    pub fn spawn_chunk(&self) -> ChunkPos {
        self.world.master.global.state().spawn.pos.chunk()
    }

    /// 这些区块是不是都已交付，并且全部岛都停放着（之后搭场景的每一步都落得下去）。
    ///
    /// Whether these chunks are all delivered and every island is parked (so
    /// every step of building the scene lands).
    pub fn all_loaded(&self, chunks: &[ChunkPos]) -> bool {
        self.world.islands.all_parked()
            && chunks
                .iter()
                .all(|&pos| self.world.islands.island_of_chunk(pos).is_some())
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
        let Some(region) = self.region_at(pos) else {
            return;
        };
        let old = region.chunks.block(pos);
        if region.chunks.set_block(pos, state) {
            let old = old.unwrap_or(mc_bamboo_data::blocks::VOID_AIR);
            region.poi.on_block_state_change(pos, old, state);
        }
        region.touch(pos);
    }

    /// 像玩家拉杆那样换一格并通知邻居。周期性输入走这里。
    pub fn set_block_and_update(&mut self, pos: BlockPos, state: u32) {
        let Some(region) = self.region_at(pos) else {
            return;
        };
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.set_block_and_update(pos, BlockState(state));
        let effects = world.into_effects();
        crate::interaction::send_effects(region, None, &effects);
        region.touch(pos);
    }

    /// 写一张容器槽位表；`kind` 决定格数。
    pub fn set_container(&mut self, pos: BlockPos, kind: BenchContainer, slots: Vec<ItemStack>) {
        let Some(region) = self.region_at(pos) else {
            return;
        };
        let kind = kind.into_game();
        let entry = crate::container::entry_in(&mut region.containers, pos, kind);
        entry.slots = slots;
        region.touch(pos);
    }

    /// 往源头箱子的第一个空格补一组物品，然后通知邻居。
    pub fn refill(&mut self, pos: BlockPos, stack: ItemStack) -> bool {
        let Some(region) = self.region_at(pos) else {
            return false;
        };
        let mut changed = false;
        let Some(entry) = region.containers.get_mut(&pos) else {
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
        let state = region
            .chunks
            .block(pos)
            .map(BlockState)
            .unwrap_or(BlockState::AIR);
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.set_block(pos, state, UPDATE_CLIENTS);
        world.update_neighbour_for_output_signal(pos, state.block());
        let effects = world.into_effects();
        crate::interaction::send_effects(region, None, &effects);
        region.touch(pos);
        true
    }

    /// 登记一个会动的方块实体（漏斗、熔炉等）。
    pub fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        if let Some(region) = self.region_at(entity.pos()) {
            region.block_entities.insert(entity);
        }
    }

    /// 排一条方块计划刻。
    pub fn schedule_tick(
        &mut self,
        pos: BlockPos,
        block: usize,
        delay: i32,
        priority: TickPriority,
    ) {
        if let Some(region) = self.region_at(pos) {
            region.block_ticks.schedule(pos, block, delay, priority);
        }
    }

    /// 在这个位置放一个掉落物，和 `/loot spawn` 一样带默认拾取延迟。
    ///
    /// Drops an item entity here with the default pickup delay, like `/loot spawn`.
    pub fn spawn_item(&mut self, position: Vec3, stack: ItemStack) {
        let pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        let Some(region) = self.region_at(pos) else {
            return;
        };
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut entity = Entity::ground_item(position, stack);
        if let EntityKind::Item(item) = &mut entity.kind {
            item.set_default_pickup_delay();
        }
        world.spawn_entity(entity);
    }

    /// 停放着的岛上的在线玩家的名字，按玩家 ID 排。
    ///
    /// Names of the online players on parked islands, in player id order.
    pub fn player_names(&self) -> Vec<String> {
        let mut players: Vec<(crate::inbound::PlayerId, String)> = self
            .world
            .islands
            .parked()
            .flat_map(|island| {
                island
                    .region
                    .players
                    .iter()
                    .map(|(&id, player)| (id, player.name.clone()))
            })
            .collect();
        players.sort_unstable_by_key(|&(id, _)| id);
        players.into_iter().map(|(_, name)| name).collect()
    }

    /// 把在线玩家 `name` 送到 `at`（和 `/tp` 同一段移动）。他所在的岛这一刻不在主控手里、
    /// 或者没有这名玩家时返回 `false`，下一刻再试。多岛时他落到别的岛的区块上，之后照常
    /// 由原岛清掉视野再搬过去（和命令送人一样）。
    ///
    /// Send online player `name` to `at` (the same move as `/tp`). Returns
    /// `false` when their island is away this tick or there is no such
    /// player; try again next tick. With several islands they land on another
    /// island's chunk and are moved over afterwards as with a command.
    pub fn teleport_player(&mut self, name: &str, at: Vec3) -> bool {
        let found = self.world.islands.parked().find_map(|island| {
            island
                .region
                .players
                .iter()
                .find(|(_, player)| player.name == name)
                .map(|(&id, _)| (island.id, id))
        });
        let Some((island, id)) = found else {
            return false;
        };
        let Some(island) = self.world.islands.get_mut(island) else {
            return false;
        };
        crate::command::place_player(&mut island.region, id, at);
        true
    }

    /// 用服务器控制台来源执行一条命令，等价于在控制台里敲：和连接层送来的控制台命令走
    /// 同一条路（多岛时借全部玩家或者把全部岛拼起来，混合驱动下有岛不在就排队）。
    ///
    /// Runs one command with the server console as its source, exactly like a
    /// line typed at the console: the same path as console lines from the
    /// connection layer (borrowing every player or joining every island with
    /// several, queued under the hybrid drive while some island is away).
    pub fn console_command(&mut self, line: &str) {
        self.world
            .accept_message(crate::inbound::Inbound::ConsoleCommand(line.to_owned()));
    }

    /// 当前方块状态，输入翻转时判断现在是红石块还是空气。
    pub fn block_state(&self, pos: BlockPos) -> u32 {
        self.world
            .islands
            .island_of_chunk(pos.chunk())
            .and_then(|id| self.world.islands.get(id))
            .and_then(|island| island.region.chunks.block(pos))
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
