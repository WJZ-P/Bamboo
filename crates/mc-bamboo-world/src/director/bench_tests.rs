//! Director 的手工基准：进服、跨区块、空闲刻。
//!
//! `cargo test --release -p mc-bamboo-world --lib director_cost -- --ignored --nocapture`
//! 用一个当场完成任务的假执行器，只量 Director 自己的记账开销，不做任何 IO。

use std::{collections::BTreeMap, time::Instant};

use mc_bamboo_core::ChunkPos;

use crate::ticket::TicketTable;

use super::{
    ChunkBundle, Director, DirectorConfig, Effect, Event, PipelineKind, RegionMessage, Task,
    TaskId, TaskResult, TicketOp,
};

/// 假盒子：0 是模板，非 0 表示玩家改过。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Fake(u64);

impl ChunkBundle for Fake {
    fn is_pristine(&self) -> bool {
        self.0 == 0
    }
}

/// 当场把每个任务做完的执行器，逻辑和 `director/tests.rs` 的假执行器一致，
/// 但不做逐步不变量断言（那本身就是 O(记录数)）。
#[derive(Debug, Default)]
struct Inline {
    director: Director<Fake>,
    /// 放着盒子的「停放表」。
    parked: BTreeMap<ChunkPos, Fake>,
    /// region 手里收到的盒子，按位置记代数。
    region: BTreeMap<ChunkPos, (u64, Fake)>,
    /// 这一轮信箱里待确认的交付。
    in_mail: BTreeMap<ChunkPos, (u64, Fake)>,
    /// 待交回的收回请求。
    reclaim: Vec<(ChunkPos, u64)>,
    events: usize,
}

impl Inline {
    fn new() -> Self {
        let mut director = Director::new(DirectorConfig {
            pipeline: PipelineKind::Flat,
            ..DirectorConfig::default()
        });
        director.set_template(|| Fake(0));
        Self {
            director,
            ..Self::default()
        }
    }

    /// 处理一个事件，把它引发的全部任务结果和 region 回执跑到底。
    fn handle(&mut self, event: Event<Fake>) {
        let mut events = vec![event];
        let mut rounds = 0usize;
        loop {
            while let Some(event) = events.pop() {
                rounds += 1;
                assert!(rounds < 10_000_000, "事件循环没有收敛");
                self.events += 1;
                for effect in self.director.handle(event) {
                    if let Some(next) = self.apply(effect) {
                        events.push(next);
                    }
                }
            }
            events = self.region_tick();
            if events.is_empty() {
                return;
            }
        }
    }

    fn apply(&mut self, effect: Effect<Fake>) -> Option<Event<Fake>> {
        match effect {
            Effect::Submit { id, task } => Some(self.run_task(id, task)),
            Effect::ToRegion(message) => {
                match message {
                    RegionMessage::Deliver {
                        pos,
                        generation,
                        column,
                        ..
                    } => {
                        self.in_mail.insert(pos, (generation, column));
                    }
                    RegionMessage::Reclaim { positions } => self.reclaim.extend(positions),
                    RegionMessage::LevelChanged { .. }
                    | RegionMessage::Ready { .. }
                    | RegionMessage::Forgot { .. } => {}
                }
                None
            }
            Effect::Park { pos, column } => {
                self.parked.insert(pos, column);
                None
            }
            Effect::Cancel { .. } | Effect::DropSnapshot { .. } | Effect::Warn { .. } => None,
        }
    }

    fn run_task(&mut self, id: TaskId, task: Task<Fake>) -> Event<Fake> {
        let pos = task.pos();
        let generation = self
            .director
            .debug_state()
            .in_flight()
            .get(&id)
            .map_or(0, |(_, generation, _)| *generation);
        let result = match task {
            Task::Read { pos } => match self.parked.remove(&pos) {
                Some(column) => TaskResult::Decoded { column },
                None => TaskResult::Read { bytes: None },
            },
            Task::Decode { bytes, .. } => {
                let value = u64::from_le_bytes(bytes.try_into().expect("8 字节"));
                TaskResult::Decoded {
                    column: Fake(value),
                }
            }
            Task::Generate { stage, column, .. } => TaskResult::Generated {
                stage,
                column: column.unwrap_or(Fake(0)),
            },
            Task::Assemble { column, .. } => TaskResult::Assembled { column },
            Task::Save { seq, column, .. } => TaskResult::Saved { seq, column },
        };
        Event::TaskDone {
            id,
            pos,
            generation,
            result,
        }
    }

    /// tick 边界：确认交付、交回收回的盒子。
    fn region_tick(&mut self) -> Vec<Event<Fake>> {
        let mut events = Vec::new();
        // 只有这一轮信箱里新到的交付才回执；已确认过的不再重复发。
        for (pos, (generation, column)) in std::mem::take(&mut self.in_mail) {
            events.push(Event::Delivered { pos, generation });
            self.region.insert(pos, (generation, column));
        }
        for (pos, generation) in std::mem::take(&mut self.reclaim) {
            let column = self.region.remove(&pos).map(|(_, column)| column);
            events.push(Event::Returned {
                pos,
                generation,
                column,
            });
        }
        events
    }

    fn join(&mut self, id: u64, at: ChunkPos) {
        self.handle(Event::Tickets {
            ops: vec![TicketOp::PlayerMoved { id, to: Some(at) }],
        });
    }

    fn move_to(&mut self, id: u64, at: ChunkPos) {
        self.handle(Event::Tickets {
            ops: vec![TicketOp::PlayerMoved { id, to: Some(at) }],
        });
    }
}

/// 200 名互相离得很远的玩家：各自的等级圈互不相交。
fn far_players(count: u64) -> Vec<(u64, ChunkPos)> {
    (0..count)
        .map(|id| (id + 1, ChunkPos::new(id as i32 * 100, 0)))
        .collect()
}

#[test]
#[ignore = "手工基准，不进常规测试"]
fn director_cost() {
    // 一名玩家视距 10 进服，到全部交付完。
    let mut one = Inline::new();
    let started = Instant::now();
    one.join(1, ChunkPos::new(0, 0));
    let join = started.elapsed();
    let join_events = one.events;
    assert!(one.region.len() > 400, "视距 10 的区块都该交付完");

    // 这名玩家跨一个区块边界。票表自己的等级传播约 0.33 ms，不在要修的范围里：
    // 在另一张同样的票表上单独跑一次，得到这次操作里 `run_updates()` 的耗时。
    let mut propagation = TicketTable::new();
    propagation.set_player(1, Some(ChunkPos::new(0, 0)));
    let _ = propagation.run_updates();
    propagation.set_player(1, Some(ChunkPos::new(1, 0)));
    let updates_started = Instant::now();
    let changes = propagation.run_updates().len();
    let propagation_cost = updates_started.elapsed();

    let started = Instant::now();
    one.move_to(1, ChunkPos::new(1, 0));
    let crossing = started.elapsed();
    let crossing_events = one.events - join_events;

    // 空闲刻：先量一名玩家，再量 200 名分散玩家。
    let idle = |players: &[(u64, ChunkPos)]| {
        let mut host = Inline::new();
        for &(id, at) in players {
            host.join(id, at);
        }
        let mut samples = Vec::new();
        for tick in 1..=5u64 {
            let started = Instant::now();
            host.handle(Event::Tick { now_tick: tick });
            samples.push(started.elapsed());
        }
        samples.sort();
        samples[samples.len() / 2]
    };
    let idle_one = idle(&[(1, ChunkPos::new(0, 0))]);
    let idle_many = idle(&far_players(200));

    eprintln!(
        "进服：{join:?}，{join_events} 个事件；已交付 {} 个区块",
        one.region.len()
    );
    eprintln!(
        "跨一次边界：{crossing:?}（其中 run_updates() 约 {propagation_cost:?}，{changes} 条变化），{crossing_events} 个事件"
    );
    eprintln!("空闲刻：1 名玩家 {idle_one:?}，200 名分散玩家 {idle_many:?}");
}
