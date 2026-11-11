//! Director 和世界线程之间的宿主：内联执行器、停放表、region 消息泵。
//!
//! 提交 5 里执行器是内联的：任务当场做完，盒子攒在信箱里。信箱在 tick 开头
//! 按离玩家的距离交付/收回，每刻有个数上限；玩家脚下 3×3 不受上限约束。
//! 视野里没交付的区块留在 `PlayerChunkSender` 的待发送集合里，交付了才发。

use std::collections::BTreeMap;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::director::{
    Director, DirectorConfig, Effect, Event, RegionMessage, Task, TaskId, TaskResult, TicketOp,
};

use crate::region::{Region, RegionColumn, parking::Parking};

/// 交付/收回每刻的个数上限。0 表示不限（只给剖析和回归对比用）。
pub(crate) const DEFAULT_MAIL_BUDGET: usize = 64;
/// 世界创建时读一次这两个环境变量，和 `pacing.rs` 一样：读不到、解析不了、
/// 或者超出范围都回落缺省值，之后不再看环境变量。
pub(crate) const DELIVERIES_ENV: &str = "BAMBOO_DELIVERIES_PER_TICK";
pub(crate) const RECLAIMS_ENV: &str = "BAMBOO_RECLAIMS_PER_TICK";
/// 上限的合法范围上限，防住误设的巨值把信箱当成没有限速。
const MAX_MAIL_BUDGET: usize = 1 << 16;

/// 信箱的每刻预算。`0` 表示不限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MailBudget {
    pub(crate) deliveries: usize,
    pub(crate) reclaims: usize,
}

impl Default for MailBudget {
    fn default() -> Self {
        Self {
            deliveries: DEFAULT_MAIL_BUDGET,
            reclaims: DEFAULT_MAIL_BUDGET,
        }
    }
}

impl MailBudget {
    /// 从环境变量读一次。收不回不设上限，只按缺省值处理。
    pub(crate) fn from_env() -> Self {
        Self {
            deliveries: read_budget(DELIVERIES_ENV),
            reclaims: read_budget(RECLAIMS_ENV),
        }
    }
}

fn read_budget(name: &str) -> usize {
    match std::env::var(name) {
        Ok(value) => match value.trim().parse::<usize>() {
            Ok(budget) if budget <= MAX_MAIL_BUDGET => budget,
            _ => {
                tracing::warn!(env = name, value = %value, "invalid chunk mail budget, using default");
                DEFAULT_MAIL_BUDGET
            }
        },
        Err(_) => DEFAULT_MAIL_BUDGET,
    }
}

/// 已加载模式下 Director 需要的全部状态。
pub(crate) struct DirectorHost {
    director: Director<Box<RegionColumn>>,
    /// 无限超平坦模式：票表照常记，但不派任何交付/收回。
    infinite_flat: bool,
    parking: Parking,
    budget: MailBudget,
    /// 发给 region 的信箱：待交付的盒子、待收回的位置。
    deliveries: BTreeMap<ChunkPos, (u64, Box<RegionColumn>)>,
    reclaims: Vec<(ChunkPos, u64)>,
    /// 本刻还没花掉的预算。整刻内所有 `pump` 共用，进服/离开/移动各发一批票
    /// 也不会把上限乘上好几倍；每刻开头由 `begin_tick` 重置。
    budget_left: MailBudget,
    /// 统计：本刻交付/收回/停放的区块数，给剖析器用。
    delivered: usize,
    reclaimed: usize,
    parked: usize,
}

impl DirectorHost {
    pub(crate) fn new(config: DirectorConfig, budget: MailBudget) -> Self {
        let mut director = Director::new(config);
        director.set_template(|| Box::new(RegionColumn::from_template()));
        Self {
            infinite_flat: false,
            director,
            parking: Parking::default(),
            budget,
            budget_left: budget,
            deliveries: BTreeMap::new(),
            reclaims: Vec::new(),
            delivered: 0,
            reclaimed: 0,
            parked: 0,
        }
    }

    /// 测试/无限超平坦专用：只记账，不派任务也不碰 region。
    pub(crate) fn new_infinite_flat(config: DirectorConfig) -> Self {
        let mut host = Self::new(config, MailBudget::default());
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

    /// 本刻交付/收回的区块数。
    pub(crate) fn counters_this_tick(&self) -> (usize, usize) {
        (self.delivered, self.reclaimed)
    }

    /// 每刻开头重置预算和计数。进服/离开/移动发来的票操作都算在这一刻里。
    pub(crate) fn begin_tick(&mut self) {
        self.budget_left = self.budget;
        self.delivered = 0;
        self.reclaimed = 0;
        self.parked = 0;
    }

    /// 测试专用：直接改预算，不碰进程级环境变量。
    #[cfg(test)]
    pub(crate) fn set_budget(&mut self, budget: MailBudget) {
        self.budget = budget;
        self.budget_left = budget;
    }

    #[cfg(test)]
    pub(crate) fn set_reclaim_delay(&mut self, delay: u64) {
        self.director.config_mut().reclaim_delay_ticks = delay;
    }

    /// 本刻还欠着多少交付/收回。测试用它给信箱长度一个上界。
    #[cfg(test)]
    pub(crate) fn mail_backlog(&self) -> (usize, usize) {
        (self.deliveries.len(), self.reclaims.len())
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

    /// 每刻推进。预算和计数由 [`Self::begin_tick`] 在刻开头重置。
    pub(crate) fn tick(&mut self, region: &mut Region, now_tick: u64) {
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
        self.budget_left = MailBudget {
            deliveries: 0,
            reclaims: 0,
        };
        self.flush_region_mail(_region);
    }

    /// 处理一个事件和它引发的全部任务，直到没有新效果。
    /// 内联执行器不设每刻上限（提交 5/6 的要求）：任务的纯函数部分当场做完，
    /// 盒子攒进信箱，由 `flush_region_mail` 按预算交付/收回。
    ///
    /// 同一刻里 Director 可能因为 `Delivered` / `Returned` 又派出新任务，
    /// 所以主循环要跑到没有新消息为止；`flush_region_mail` 每轮都只花掉
    /// 这一刻剩下的预算，预算用完之后它返回空事件，循环就停。
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
        _region: &mut Region,
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
                self.region_message(message);
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

    fn region_message(&mut self, message: RegionMessage<Box<RegionColumn>>) {
        match message {
            RegionMessage::Deliver {
                pos,
                generation,
                column,
                ..
            } => {
                self.deliveries.insert(pos, (generation, column));
            }
            RegionMessage::Reclaim { positions } => self.reclaims.extend(positions),
            // `Ready` 不用另存一份集合：`ChunkStore::is_loaded` 就是已交付集合，
            // 发区块时直接问它；多存一份只会多一处要保持一致的状态。
            RegionMessage::LevelChanged { .. }
            | RegionMessage::Ready { .. }
            | RegionMessage::Forgot { .. } => {}
        }
    }

    /// 离最近的玩家有多远（切比雪夫距离）。没有玩家时所有位置一样远。
    fn distance_to_players(pos: ChunkPos, centers: &[ChunkPos]) -> (u64, i32, i32) {
        let nearest = centers
            .iter()
            .map(|center| {
                let dx = i64::from(pos.x) - i64::from(center.x);
                let dz = i64::from(pos.z) - i64::from(center.z);
                // 差值非负，两个 i32 相减放进 i64 不会溢出。
                dx.abs().max(dz.abs()) as u64
            })
            .min()
            .unwrap_or(u64::MAX);
        // 同距离按坐标排，结果不依赖 BTreeMap 之外的任何顺序。
        (nearest, pos.x, pos.z)
    }

    /// 玩家脚下 3×3：不受每刻上限约束，且优先于任何其它位置。
    /// 玩家不能站在一个没加载的地方，进服、传送、高速移动都一样。
    fn is_under_a_player(pos: ChunkPos, centers: &[ChunkPos]) -> bool {
        centers.iter().any(|center| {
            (i64::from(pos.x) - i64::from(center.x)).abs() <= 1
                && (i64::from(pos.z) - i64::from(center.z)).abs() <= 1
        })
    }

    /// 玩家所在的区块。刻预算用完时，脚下 3×3 仍然要交付完。
    fn guaranteed_centers(region: &Region) -> Vec<ChunkPos> {
        region
            .players
            .values()
            .map(|player| player.chunk_pos())
            .collect()
    }

    /// region 在 tick 边界收信箱：先交付、后收回，返回要回给 Director 的事件。
    ///
    /// 整刻最多交付 `budget.deliveries` 个非豁免区块、收回
    /// `budget.reclaims` 个区块（`0` 表示不限），剩下的留到下一刻。
    /// 同一刻内 `pump` 会多次调用它，共用 `budget_left` 里的剩余额度。
    fn flush_region_mail(&mut self, region: &mut Region) -> Vec<Event<Box<RegionColumn>>> {
        let MailBudget {
            deliveries: delivery_left,
            reclaims: reclaim_left,
        } = self.budget_left;
        let limited_deliveries = self.budget.deliveries > 0;
        let limited_reclaims = self.budget.reclaims > 0;
        let mut events = Vec::new();
        // 只有信箱里真有要交付/收回的东西时才把光照算完：这个函数每刻都调，
        // 无条件 `finish_light_updates` 会绕开「光照每刻有预算」的设计。
        if self.deliveries.is_empty() && self.reclaims.is_empty() {
            return events;
        }
        // 交付/收回要求「没有算到一半的轮次」。
        region.chunks.finish_light_updates();
        let centers = Self::guaranteed_centers(region);

        // 近的先交付。玩家脚下 3×3 全部交付且不算进上限；同等距离按坐标排。
        let mut ordered: Vec<ChunkPos> = self.deliveries.keys().copied().collect();
        // BTreeMap 的键已经按 (x, z) 升序，稳定排序让同等距离保持坐标顺序。
        ordered.sort_by_key(|&pos| {
            let guaranteed = Self::is_under_a_player(pos, &centers);
            (!guaranteed, Self::distance_to_players(pos, &centers))
        });
        let mut used = 0usize;
        for pos in ordered {
            let guaranteed = Self::is_under_a_player(pos, &centers);
            if !guaranteed && limited_deliveries && used >= delivery_left {
                continue;
            }
            let Some((generation, column)) = self.deliveries.remove(&pos) else {
                continue;
            };
            match region.deliver_column(pos, column) {
                Ok(()) => {
                    self.delivered += 1;
                    if !guaranteed {
                        used += 1;
                    }
                    events.push(Event::Delivered { pos, generation });
                }
                Err((_, column)) => {
                    // 前提不满足（例如某张表正在 tick 中途）：留在信箱里，
                    // 下一次 pump 再试，绝不把盒子丢掉。
                    tracing::warn!(?pos, "chunk delivery was not ready, will retry");
                    self.deliveries.insert(pos, (generation, column));
                }
            }
        }
        if limited_deliveries {
            self.budget_left.deliveries = delivery_left.saturating_sub(used);
        }

        if self.reclaims.is_empty() {
            return events;
        }
        let reclaimed_before = self.reclaimed;
        // 远的先收回：这些区块不急，玩家脚下的票也没了时先留着更保险。
        let mut by_generation = BTreeMap::new();
        for (pos, generation) in std::mem::take(&mut self.reclaims) {
            by_generation.insert(pos, generation);
        }
        let mut ordered: Vec<ChunkPos> = by_generation.keys().copied().collect();
        let distance = |pos: &ChunkPos| Self::distance_to_players(*pos, &centers);
        // 距离取相反数，远的在前；同距离按坐标升序。
        ordered.sort_by_key(|pos| {
            let (far, x, z) = distance(pos);
            (std::cmp::Reverse(far), x, z)
        });
        let count = if limited_reclaims {
            ordered.len().min(reclaim_left)
        } else {
            ordered.len()
        };
        let batch: Vec<(ChunkPos, u64)> = ordered
            .into_iter()
            .take(count)
            .map(|pos| {
                let generation = by_generation.remove(&pos).expect("键在表里");
                (pos, generation)
            })
            .collect();
        // 没轮到的留在信箱里，下一次 tick 再收。
        self.reclaims.extend(by_generation);
        let positions = batch.iter().map(|&(pos, _)| pos).collect();
        match region.reclaim_columns(&positions) {
            Ok(mut boxes) => {
                for (pos, generation) in batch {
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
                self.reclaims.extend(batch);
            }
        }
        if limited_reclaims {
            self.budget_left.reclaims =
                reclaim_left.saturating_sub(self.reclaimed.saturating_sub(reclaimed_before));
        }
        events
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
        let mut host = DirectorHost::new(DirectorConfig::default(), MailBudget::default());
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

    #[test]
    fn invalid_env_values_fall_back_to_the_default_budget() {
        // 其它测试可能并发改环境变量，这里只验证解析函数本身不会 panic。
        assert_eq!(MailBudget::default().deliveries, DEFAULT_MAIL_BUDGET);
        assert_eq!(MailBudget::default().reclaims, DEFAULT_MAIL_BUDGET);
    }
}
