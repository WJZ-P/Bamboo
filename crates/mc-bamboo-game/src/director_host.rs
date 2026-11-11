//! Director 和世界线程之间的宿主：内联执行器、停放表、region 消息泵。
//!
//! 提交 5 里执行器是内联的：任务当场做完，结果立刻送回 Director，
//! 一刻之内把所有该交付的区块交付完。`chunk_tracking` 仍按视野直接编码，
//! 所以已加载集合必须覆盖每名玩家的视野（[`assert_view_is_loaded`]）。

use std::collections::BTreeMap;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::director::{
    Desired, Director, DirectorConfig, Effect, Event, RegionMessage, Task, TaskId, TaskResult,
    TicketOp,
};

use crate::region::{Region, RegionColumn, parking::Parking};

/// 已加载模式下 Director 需要的全部状态。
pub(crate) struct DirectorHost {
    director: Director<Box<RegionColumn>>,
    /// 无限超平坦模式：票表照常记，但不派任何交付/收回。
    infinite_flat: bool,
    parking: Parking,
    /// 发给 region 的信箱：待交付的盒子（带交付时的等级）、待收回的位置。
    deliveries: BTreeMap<ChunkPos, (u64, Desired, Box<RegionColumn>)>,
    reclaims: Vec<(ChunkPos, u64)>,
    /// 统计：本刻交付/收回/停放的区块数，给提交 8 的剖析器用。
    pub(crate) delivered: usize,
    pub(crate) reclaimed: usize,
    pub(crate) parked: usize,
}

impl DirectorHost {
    pub(crate) fn new(config: DirectorConfig) -> Self {
        let mut director = Director::new(config);
        director.set_template(|| Box::new(RegionColumn::from_template()));
        Self {
            infinite_flat: false,
            director,
            parking: Parking::default(),
            deliveries: BTreeMap::new(),
            reclaims: Vec::new(),
            delivered: 0,
            reclaimed: 0,
            parked: 0,
        }
    }

    /// 测试/无限超平坦专用：只记账，不派任务也不碰 region。
    pub(crate) fn new_infinite_flat(config: DirectorConfig) -> Self {
        let mut host = Self::new(config);
        host.infinite_flat = true;
        host
    }

    #[allow(dead_code)]
    pub(crate) fn director(&self) -> &Director<Box<RegionColumn>> {
        &self.director
    }

    pub(crate) fn director_mut(&mut self) -> &mut Director<Box<RegionColumn>> {
        &mut self.director
    }

    #[allow(dead_code)]
    pub(crate) fn counters(&self) -> (u64, u64) {
        self.director.counters()
    }

    #[allow(dead_code)]
    pub(crate) fn parked_count(&self) -> usize {
        self.parking.len()
    }

    /// 一批票的操作：喂给 Director，把效果跑到底。
    pub(crate) fn tickets(&mut self, region: &mut Region, ops: Vec<TicketOp>) {
        if self.infinite_flat {
            // 只把票喂给票表，不产生效果。
            for op in ops {
                self.apply_infinite_ticket(op);
            }
            let _ = self.director.tickets_mut().run_updates();
            return;
        }
        self.pump(region, Event::Tickets { ops });
    }

    fn apply_infinite_ticket(&mut self, op: TicketOp) {
        let table = self.director.tickets_mut();
        match op {
            TicketOp::PlayerMoved { id, to } => table.set_player(id, to),
            TicketOp::ViewDistance(v) => table.set_view_distance(v),
            TicketOp::SimulationDistance(d) => table.set_simulation_distance(d),
            TicketOp::Add { ty, pos, level } => table.add(ty, pos, level),
            TicketOp::Remove { ty, pos, level } => table.remove(ty, pos, level),
            TicketOp::Forced { pos, forced } => table.set_forced(pos, forced),
            TicketOp::WithRadius {
                ty,
                pos,
                radius,
                add,
            } => {
                if add {
                    table.add_with_radius(ty, pos, radius);
                } else {
                    table.remove_with_radius(ty, pos, radius);
                }
            }
        }
    }

    /// 每刻推进。
    pub(crate) fn tick(&mut self, region: &mut Region, now_tick: u64) {
        self.delivered = 0;
        self.reclaimed = 0;
        self.parked = 0;
        if self.infinite_flat {
            self.director.tickets_mut().tick(&mut |_| false);
            let _ = self.director.tickets_mut().run_updates();
            return;
        }
        self.pump(region, Event::Tick { now_tick });
    }

    /// 关服：收回全部区块、停放完所有快照。
    // 任务 B 接真实 IO 后由 `World::shut_down` 调用：关服要等全部写入落盘。
    #[allow(dead_code)]
    pub(crate) fn shutdown(&mut self, _region: &mut Region) {
        if self.infinite_flat {
            return;
        }
        // 任务 A 没有真实磁盘：停放的盒子只活在内存表里，关服后进程就结束了。
        // 逐柱收回几千个区块只会拖住关服，没有可观察的收益，所以这里只让
        // Director 的状态机走完 Shutdown，不执行它派出的 Reclaim/Save。
        // 任务 B 接真实 IO 时要把这里换成「等全部写入落盘」。
        let mut events = vec![Event::Shutdown];
        let mut rounds = 0;
        while let Some(event) = events.pop() {
            rounds += 1;
            assert!(rounds < 100_000, "Director 的关服循环没有收敛");
            for effect in self.director.handle(event) {
                if let Effect::Submit {
                    id,
                    generation,
                    task,
                } = effect
                {
                    events.push(self.run_task(id, generation, task));
                }
            }
        }
        self.deliveries.clear();
        self.reclaims.clear();
        self.flush_region_mail(_region);
    }

    /// 处理一个事件和它引发的全部任务，直到没有新效果。
    /// 内联执行器不设每刻上限（提交 5/6 的要求）。
    fn pump(&mut self, region: &mut Region, event: Event<Box<RegionColumn>>) {
        let mut events = vec![event];
        let mut rounds = 0;
        loop {
            // 先把队列里的事件和它们引发的任务结果全部跑完。
            while let Some(event) = events.pop() {
                rounds += 1;
                assert!(rounds < 1_000_000, "Director 的事件循环没有收敛");
                for effect in self.director.handle(event) {
                    if let Some(next) = self.apply(region, effect) {
                        events.push(next);
                    }
                }
            }
            // 再把 region 信箱里的交付/收回变成 Delivered / Returned 事件。
            // 没有新消息就收敛了。
            events = self.flush_region_mail(region);
            if events.is_empty() {
                return;
            }
        }
    }

    /// 执行一个效果。返回需要立刻回给 Director 的事件（内联任务的结果）。
    fn apply(
        &mut self,
        region: &mut Region,
        effect: Effect<Box<RegionColumn>>,
    ) -> Option<Event<Box<RegionColumn>>> {
        match effect {
            Effect::Submit {
                id,
                generation,
                task,
            } => Some(self.run_task(id, generation, task)),
            Effect::Cancel { .. } => None,
            Effect::ToRegion(message) => {
                self.region_message(region, message);
                None
            }
            Effect::Park { pos, column } => {
                self.parking.park(pos, column);
                self.parked += 1;
                None
            }
            Effect::DropSnapshot { .. } => None,
            Effect::Warn { message } => {
                tracing::warn!(%message, "chunk director");
                None
            }
        }
    }

    /// 内联执行器。任务全部是纯函数或内存表操作，不碰文件、不起线程。
    fn run_task(
        &mut self,
        id: TaskId,
        generation: u64,
        task: Task<Box<RegionColumn>>,
    ) -> Event<Box<RegionColumn>> {
        let pos = task.pos();
        let result = match task {
            // 停放表里是整个盒子（不经过 NBT）。命中就直接当成解码结果交还，
            // 没命中才让 Director 走生成流水线。
            Task::Read { pos } => match self.parking.take(pos) {
                Some(column) => TaskResult::Decoded { column },
                None => TaskResult::Read { bytes: None },
            },
            Task::Decode { .. } => TaskResult::Decoded {
                column: Box::new(RegionColumn::from_template()),
            },
            Task::Generate { stage, column, .. } => TaskResult::Generated {
                stage,
                column: column.unwrap_or_else(|| Box::new(RegionColumn::from_template())),
            },
            Task::Assemble { column, .. } => TaskResult::Assembled { column },
            Task::Save { seq, column, .. } => {
                // 盒子随完成事件交还 Director；有票就直接重新交付，
                // 没票才由 `Effect::Park` 放进停放表。
                TaskResult::Saved { seq, column }
            }
        };
        Event::TaskDone {
            id,
            pos,
            generation,
            result,
        }
    }

    fn region_message(&mut self, region: &mut Region, message: RegionMessage<Box<RegionColumn>>) {
        match message {
            RegionMessage::Deliver {
                pos,
                generation,
                column,
                desired,
            } => {
                self.deliveries.insert(pos, (generation, desired, column));
            }
            RegionMessage::Reclaim { positions } => self.reclaims.extend(positions),
            RegionMessage::LevelChanged { changes } => {
                // 同一个 pump 里可能先收到档位变化、后 flush 交付：信箱里那份档位
                // 也要跟着变，否则新交付的区块会带着旧档位进 region。尚未交付的
                // 位置在 region 里没有记录，`level_changed` 只更新已交付的那些。
                for change in &changes {
                    if let Some(entry) = self.deliveries.get_mut(&change.pos) {
                        entry.1 = Desired::new(change.loading.1, change.simulation.1);
                    }
                }
                region.tick_levels.level_changed(&changes);
            }
            RegionMessage::Ready { .. } | RegionMessage::Forgot { .. } => {}
        }
    }

    /// region 在 tick 边界收信箱：先交付、后收回，返回要回给 Director 的事件。
    fn flush_region_mail(&mut self, region: &mut Region) -> Vec<Event<Box<RegionColumn>>> {
        let mut events = Vec::new();
        // 只有信箱里真有要交付/收回的东西时才把光照算完：这个函数每刻都调，
        // 无条件 `finish_light_updates` 会绕开「光照每刻有预算」的设计。
        if self.deliveries.is_empty() && self.reclaims.is_empty() {
            return events;
        }
        // 交付/收回要求「没有算到一半的轮次」。
        region.chunks.finish_light_updates();
        let deliveries = std::mem::take(&mut self.deliveries);
        let mut delivered_levels = Vec::with_capacity(deliveries.len());
        for (pos, (generation, desired, column)) in deliveries {
            match region.deliver_column(pos, column) {
                Ok(()) => {
                    self.delivered += 1;
                    delivered_levels.push((pos, desired));
                    events.push(Event::Delivered { pos, generation });
                }
                Err((_, column)) => {
                    // 前提不满足（例如某张表正在 tick 中途）：留在信箱里，
                    // 下一次 pump 再试，绝不把盒子丢掉。
                    tracing::warn!(?pos, "chunk delivery was not ready, will retry");
                    self.deliveries.insert(pos, (generation, desired, column));
                }
            }
        }
        // 一批交付只重算一次受影响的邻域，不逐块重算。
        region.tick_levels.deliver(delivered_levels);
        let reclaims = std::mem::take(&mut self.reclaims);
        if reclaims.is_empty() {
            return events;
        }
        let mut by_generation = BTreeMap::new();
        for (pos, generation) in reclaims {
            by_generation.insert(pos, generation);
        }
        let positions = by_generation.keys().copied().collect();
        match region.reclaim_columns(&positions) {
            Ok(mut boxes) => {
                region.tick_levels.reclaim(positions.iter());
                for (pos, generation) in by_generation {
                    self.reclaimed += 1;
                    events.push(Event::Returned {
                        pos,
                        generation,
                        column: boxes.remove(&pos),
                    });
                }
            }
            Err(_) => {
                // 失败时整批放回信箱，下次 tick 边界再试：Director 还以为这些
                // 区块「正在收回」，丢了这条列表它就永远等不到回执。
                tracing::warn!("chunk reclaim was not ready, will retry");
                self.reclaims.extend(by_generation);
            }
        }
        events
    }
}

/// 已加载模式下，每名玩家视野里的区块必须都已经交付。
/// 发区块的那一步调用它；调试构建下不满足就 panic，
/// 发布构建由 `chunk_frame` 返回的 `NotLoaded` 错误兜住。
pub(crate) fn assert_view_is_loaded(store: &mc_bamboo_world::store::ChunkStore, pos: ChunkPos) {
    if store.is_loaded_mode() {
        debug_assert!(
            store.is_loaded(pos),
            "区块 ({}, {}) 还没交付就要发给客户端",
            pos.x,
            pos.z
        );
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, PrimedTnt};
    use mc_bamboo_core::{ChunkPos, Vec3};

    use super::*;
    use crate::world::tests::Harness;

    const ORIGIN: ChunkPos = ChunkPos::new(0, 0);

    /// 造一个「收回失败」的前提：实体表里有一行被借出去还没放回，
    /// `EntityStore::take_chunk` 会因此整批拒绝。
    fn borrowed_entity(region: &mut crate::region::Region) -> Entity {
        let entity = Entity::new(
            EntityKind::Tnt(PrimedTnt::new()),
            Vec3::new(2.0, -59.0, 2.0),
        );
        let uid = region.entities.spawn(entity);
        let entity = region.entities.take(uid).expect("刚放进去的实体");
        assert!(
            !region.ready_to_move_columns(),
            "有借出去的实体时不该能收回"
        );
        entity
    }

    #[test]
    fn a_failed_reclaim_is_put_back_and_retried() {
        let mut host = DirectorHost::new(DirectorConfig::default());
        let mut server = Harness::new(2);
        let region = server.region();
        host.reclaims.push((ORIGIN, 1));
        let entity = borrowed_entity(region);
        let first = host.flush_region_mail(region);
        assert!(
            first.is_empty(),
            "前提不满足时不产生 Returned，Director 还在等"
        );
        assert_eq!(host.reclaims, vec![(ORIGIN, 1)], "失败的收回要放回信箱");

        region.entities.put(entity);
        let second = host.flush_region_mail(region);
        assert!(host.reclaims.is_empty(), "重试成功之后信箱清空");
        assert_eq!(second.len(), 1, "这一刻要给 Director 回 Returned");
        assert!(matches!(second[0], Event::Returned { pos, .. } if pos == ORIGIN));
    }
}
