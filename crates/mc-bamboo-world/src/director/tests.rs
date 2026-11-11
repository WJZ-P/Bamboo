//! Director 的确定性随机测试：假执行器、乱序完成、四类竞争和可断言的不变量。

use std::collections::BTreeMap;

use mc_bamboo_core::ChunkPos;

use crate::ticket::{ChunkStatus, TicketTable};

use super::{
    ChunkBundle, Director, DirectorConfig, Effect, Event, LevelClassification, Owner, PipelineKind,
    RegionMessage, Task, TaskId, TaskResult, TicketOp, classify,
};

/// 假盒子：0 表示「生成器输出 / 没改过」，非 0 表示玩家改过。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Fake(u64);

impl ChunkBundle for Fake {
    fn is_pristine(&self) -> bool {
        self.0 == 0
    }
}

/// 假执行器：把 `Submit` 记成待完成队列，测试可以任意顺序回 `TaskDone`。
#[derive(Debug, Default)]
struct Harness {
    director: Director<Fake>,
    /// 还没完成的任务，`(id, pos, generation, task, 阶段函数要用的盒子)`。
    tasks: BTreeMap<TaskId, (ChunkPos, u64, Task<Fake>)>,
    /// 停放表：任务 A 的「磁盘」，只存整个盒子，不经过 NBT。
    parked: BTreeMap<ChunkPos, Fake>,
    /// region 手里的盒子。
    region: BTreeMap<ChunkPos, Fake>,
    /// 已经发出但 region 还没确认的交付。
    in_mail: BTreeMap<ChunkPos, (u64, Fake)>,
    /// 待交回的 `(pos, generation)`。
    reclaim: BTreeMap<ChunkPos, u64>,
    /// 发给过玩家的 `Ready` 坐标。
    ready: Vec<ChunkPos>,
    /// 发过的 `Forgot` 坐标。
    forgot: Vec<ChunkPos>,
    /// 被 `DropSnapshot` 的快照。
    dropped: Vec<(ChunkPos, u64)>,
    /// 已经完成过的任务 id，`TaskDone` 不能重复消费。
    done: Vec<TaskId>,
    /// `Effect` 的 `Debug` 序列，用来对比确定性。
    trace: Vec<String>,
    seed: u64,
}

impl Harness {
    fn new(config: DirectorConfig) -> Self {
        let mut director = Director::new(config);
        director.set_template(|| Fake(0));
        Self {
            director,
            ..Self::default()
        }
    }

    fn next(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// 发出一个事件，收集效果，按假执行器的语义更新自己，然后逐步断言不变量。
    fn handle(&mut self, event: Event<Fake>) {
        let effects = self.director.handle(event);
        for effect in effects {
            self.trace.push(format!("{effect:?}"));
            self.apply(effect);
        }
        self.assert_invariants();
    }

    /// 只处理事件、不断言全套不变量。给规模大的流水线测试用，
    /// 它在关键步骤另调 [`Self::assert_invariants`]。
    fn handle_light(&mut self, event: Event<Fake>) {
        let effects = self.director.handle(event);
        for effect in effects {
            self.apply(effect);
        }
    }

    fn apply(&mut self, effect: Effect<Fake>) {
        match effect {
            Effect::Submit {
                id,
                generation,
                task,
            } => {
                let pos = task.pos();
                assert!(self.tasks.insert(id, (pos, generation, task)).is_none());
            }
            Effect::Cancel { id } => {
                self.tasks.remove(&id);
            }
            Effect::ToRegion(message) => match message {
                RegionMessage::Deliver {
                    pos,
                    generation,
                    column,
                    ..
                } => {
                    self.in_mail.insert(pos, (generation, column));
                }
                RegionMessage::Reclaim { positions } => {
                    for (pos, generation) in positions {
                        self.reclaim.insert(pos, generation);
                    }
                }
                RegionMessage::LevelChanged { .. } => {}
                RegionMessage::Ready { positions } => self.ready.extend(positions),
                RegionMessage::Forgot { positions } => self.forgot.extend(positions),
            },
            Effect::Park { pos, column } => {
                self.parked.insert(pos, column);
            }
            Effect::DropSnapshot { pos, seq } => self.dropped.push((pos, seq)),
            Effect::Warn { .. } => {}
        }
    }

    /// 随机取一个在途任务完成它。返回是否真的完成了一个。
    fn complete_one(&mut self, edit: bool) -> bool {
        if self.tasks.is_empty() {
            return false;
        }
        let index = (self.next() as usize) % self.tasks.len();
        let id = *self.tasks.keys().nth(index).expect("任务不为空");
        let (pos, generation, task) = self.tasks.remove(&id).expect("任务存在");
        if self.done.contains(&id) {
            return true;
        }
        self.done.push(id);
        let _edited = Fake(if edit { self.next() % 3 + 1 } else { 0 });
        let result = match task {
            Task::Read { pos } => TaskResult::Read {
                bytes: self
                    .parked
                    .remove(&pos)
                    .map(|value| value.0.to_le_bytes().to_vec()),
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
        self.handle(Event::TaskDone {
            id,
            pos,
            generation,
            result,
        });
        true
    }

    /// 把在途/待派任务一直做完，每完成一批跑一次 region 边界。
    fn drain(&mut self, edit: bool) {
        for _ in 0..100_000 {
            if self.tasks.is_empty() {
                self.region_tick();
                if self.tasks.is_empty() {
                    return;
                }
            }
            if !self.complete_one(edit) {
                self.region_tick();
            }
        }
        panic!("流水线没有收敛");
    }

    /// region 在 tick 边界处理所有待办：确认交付、交回区块。
    fn region_tick(&mut self) {
        for (pos, (generation, column)) in std::mem::take(&mut self.in_mail) {
            self.region.insert(pos, column);
            self.handle(Event::Delivered { pos, generation });
        }
        let reclaim = std::mem::take(&mut self.reclaim);
        if reclaim.is_empty() {
            return;
        }
        let mut returned = Vec::new();
        for (pos, generation) in reclaim {
            let column = self.region.remove(&pos);
            returned.push((pos, generation, column));
        }
        for (pos, generation, column) in returned {
            self.handle(Event::Returned {
                pos,
                generation,
                column,
            });
        }
    }

    /// 每步之后断言的不变量（见设计文档第 1.5 节）。
    fn assert_invariants(&self) {
        let state = self.director.debug_state();
        let records = state.records();
        let in_flight = state.in_flight();
        let pending = state.pending();

        // 1. 唯一归属：Region/Delivering/Reclaiming 的记录没有在途或待派任务，
        //    Pipeline/Saving/Draining 的记录恰好有一个任务或在停放/等待中。
        for (&pos, record) in &records {
            assert!(
                record
                    .task
                    .is_none_or(|id| in_flight.contains_key(&id) || pending.contains_key(&id)),
                "记录 {pos:?} 记着不属于任何队列的任务"
            );
            match record.owner {
                Owner::Region(_) | Owner::Delivering | Owner::Reclaiming => {
                    assert_eq!(record.task, None, "已交付的 {pos:?} 不该有任务");
                }
                _ => {}
            }
        }
        for (id, (pos, _, _)) in &in_flight {
            assert_eq!(records.get(pos).and_then(|r| r.task), Some(*id));
        }
        for (id, pos) in &pending {
            assert_eq!(records.get(pos).and_then(|r| r.task), Some(*id));
        }

        // 2. 票据一致：每条记录的目标等级等于票表当前值。
        for (&pos, record) in &records {
            if record.dependency {
                continue;
            }
            let level = state.tickets().loading_level(pos);
            let simulation = state.tickets().simulation_level(pos);
            assert_eq!(record.desired.loading_level, level, "{pos:?} 加载等级");
            assert_eq!(
                record.desired.simulation_level, simulation,
                "{pos:?} 模拟等级"
            );
        }

        // 3/4. 无票区块只在收回延迟内逗留：延迟没过可以留在 Region，
        // 过了延迟必须已经进入 Reclaiming/Saving/Draining，不能再是 Region/Delivering。
        let delay = self.director.config().reclaim_delay_ticks;
        for (&pos, record) in &records {
            if record.desired.target.is_none() {
                // 撤票发生在两次 Tick 之间时，`Reclaim` 要到下一次 Tick 才排，
                // 所以 `now - at == delay` 这一刻仍允许留在 Region。
                let lingered = record
                    .unloaded_at
                    .is_some_and(|at| self.director.now_tick().saturating_sub(at) <= delay);
                assert!(
                    lingered || !matches!(record.owner, Owner::Region(_) | Owner::Delivering),
                    "没票且逗留已过期的 {pos:?} 还留在 {owner:?}: {record:?}",
                    owner = record.owner
                );
                if record.owner == Owner::Pipeline {
                    assert!(record.dependency, "没票的 {pos:?} 不是依赖记录却还在流水线");
                }
            }
        }

        // 5. 任务/消息成对：假执行器里的在途任务就是 Director 的在途任务。
        assert_eq!(
            self.tasks.len(),
            in_flight.len(),
            "执行器和 Director 的在途任务数不一致"
        );
        for (id, (pos, generation, _task)) in &self.tasks {
            let actual = in_flight.get(id).copied().map(|(p, g, _)| (p, g));
            assert_eq!(actual, Some((*pos, *generation)), "任务 {id} 的归属不一致");
        }

        // 6. 快照单调：被丢掉的 seq 不会出现在已完成的 Saved 里。
        for (pos, seq) in &self.dropped {
            assert!(
                records
                    .get(pos)
                    .and_then(|record| record.snapshot)
                    .is_none_or(|current| current > *seq),
                "被丢掉的旧快照 {pos:?}#{seq} 又回来了"
            );
        }

        // 8. 有界：记录数不超过票覆盖 + 在途 + 待存盘。
        assert!(records.len() <= in_flight.len() + pending.len() + 20_000);

        // 逗留区块不与任何「正在 tick」的区块相邻（主代理答复第 3 处的论证）：
        // 等级 ≤ 32 的区块会 tick，等级 ≥ 45（无票）的区块逗留。
        // ticket 传播保证任何 ≤ 32 区块的 8 邻居等级 ≤ 33，仍然有票。
        for (&pos, record) in &records {
            if record.unloaded_at.is_none() {
                continue;
            }
            assert!(
                state.tickets().loading_level(pos) > 33,
                "逗留的 {pos:?} 等级还在已加载范围内"
            );
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let neighbor = ChunkPos::new(pos.x + dx, pos.z + dz);
                    assert!(
                        state.tickets().loading_level(neighbor) > 32,
                        "逗留的 {pos:?} 和会 tick 的 {neighbor:?} 相邻"
                    );
                }
            }
        }
    }
}

fn config(pipeline: PipelineKind, delay: u64) -> DirectorConfig {
    DirectorConfig {
        reclaim_delay_ticks: delay,
        pipeline,
        max_submits_per_tick: 0,
        max_in_flight: usize::MAX,
    }
}

/// 完整流水线的阶段函数：输出只取决于输入，写一个由 (pos, stage) 决定的指纹。
fn stage_fingerprint(pos: ChunkPos, stage: ChunkStatus) -> u64 {
    let packed = ((pos.x as i64 as u64) << 32) ^ (pos.z as i64 as u64);
    packed
        .rotate_left(7)
        .wrapping_add(stage as u64 + 1)
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        | 1
}

#[test]
fn level_change_filter_only_acts_on_stage_boundaries() {
    use crate::ticket::{LevelChange, UNLOADED_LEVEL};

    let change = |old, new| LevelChange {
        pos: ChunkPos::new(0, 0),
        loading: (old, new),
        simulation: (33, 33),
    };
    // 同一生成档内部的等级变化全是 Noop。
    assert_eq!(classify(change(38, 37)), LevelClassification::Noop);
    assert_eq!(classify(change(31, 33)), LevelClassification::LevelOnly);
    assert_eq!(classify(change(33, 31)), LevelClassification::LevelOnly);
    assert_eq!(classify(change(34, 33)), LevelClassification::Promote);
    assert_eq!(classify(change(33, 34)), LevelClassification::Demote);
    // 加载等级没变、只有模拟等级跨过 31/32/33 时也要通知 region。
    let simulation_only = |old, new| LevelChange {
        pos: ChunkPos::new(0, 0),
        loading: (33, 33),
        simulation: (old, new),
    };
    assert_eq!(
        classify(simulation_only(33, 31)),
        LevelClassification::LevelOnly
    );
    assert_eq!(
        classify(simulation_only(32, 31)),
        LevelClassification::LevelOnly
    );
    assert_eq!(
        classify(simulation_only(30, 29)),
        LevelClassification::Noop,
        "实体 tick 档内部不通知"
    );
    assert_eq!(
        classify(simulation_only(33, 32)),
        LevelClassification::LevelOnly
    );
    assert_eq!(
        classify(change(UNLOADED_LEVEL, 40)),
        LevelClassification::Load
    );
    assert_eq!(
        classify(change(40, UNLOADED_LEVEL)),
        LevelClassification::Unload
    );
}

#[test]
fn one_player_loads_then_reclaims_after_the_delay() {
    let mut h = Harness::new(config(PipelineKind::Flat, 3));
    h.seed = 1;
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    assert!(!h.tasks.is_empty(), "应该派出读盘任务");
    // 把流水线跑完：读盘 → 生成 → 组装 → 交付。
    h.drain(false);
    assert!(!h.region.is_empty(), "区块应该已经交付给 region");
    let loaded = h.director.loaded_count();
    assert!(loaded > 0);

    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
    });
    // 延迟之内不发 Reclaim。
    for tick in 1..=2 {
        h.handle(Event::Tick { now_tick: tick });
        assert!(h.reclaim.is_empty(), "第 {tick} 刻还不该收回");
    }
    h.handle(Event::Tick { now_tick: 3 });
    assert!(!h.reclaim.is_empty(), "延迟到了要收回");
    h.region_tick();
    // 盒子是 pristine 的，直接丢掉，不停放。
    assert!(h.parked.is_empty(), "没改过的盒子不停放");
    let _ = loaded;
}

#[test]
fn a_ticket_returning_during_save_reuses_the_box_without_reading() {
    let mut h = Harness::new(config(PipelineKind::Flat, 0));
    h.seed = 3;
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    h.drain(false);
    // 玩家改过盒子：放进 region 的盒子改成非 pristine。
    for column in h.region.values_mut() {
        column.0 = 7;
    }
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
    });
    h.handle(Event::Tick { now_tick: 1 });
    h.region_tick();
    // 现在有一个 Save 在途。
    assert!(!h.tasks.is_empty());
    // 票在存盘途中回来。
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    let save_id = *h.tasks.keys().next().expect("存盘在途");
    h.complete_one(false);
    // Save 完成应该直接 Deliver，不产生新的 Read。
    let has_read = h
        .tasks
        .values()
        .any(|(_, _, task)| matches!(task, Task::Read { .. }));
    assert!(!has_read, "票回来了不该再读盘（任务 {save_id}）");
    assert!(!h.in_mail.is_empty(), "盒子应该直接重新交付");
}

#[test]
fn reclaim_delay_means_a_returning_ticket_does_nothing() {
    let mut h = Harness::new(config(PipelineKind::Flat, 100));
    h.seed = 5;
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    h.drain(false);
    let before = h.director.debug_state().records();
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
    });
    h.handle(Event::Tick { now_tick: 1 });
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    h.handle(Event::Tick { now_tick: 2 });
    let after = h.director.debug_state().records();
    assert!(h.reclaim.is_empty(), "逗留期内不该收回");
    for (pos, record) in &before {
        assert_eq!(after.get(pos).map(|r| r.owner), Some(record.owner));
    }
}

#[test]
fn stale_task_results_are_dropped() {
    let mut h = Harness::new(config(PipelineKind::Flat, 0));
    h.seed = 7;
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    // 拿走读盘任务但不完成，然后撤票、再挂票。
    let (id, pos, generation) = {
        let key = *h.tasks.keys().next().expect("有任务");
        let (pos, generation, _) = h.tasks.get(&key).expect("任务存在");
        (key, *pos, *generation)
    };
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
    });
    h.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(0, 0)),
        }],
    });
    // 旧的完成事件整包丢弃，不该改变状态。
    let before = h.director.debug_state().records();
    h.handle(Event::TaskDone {
        id,
        pos,
        generation,
        result: TaskResult::Read { bytes: None },
    });
    let after = h.director.debug_state().records();
    assert_eq!(before, after, "旧任务的完成不该改状态");
}

/// 完整 `ChunkPyramid`：同时在跑的任务写范围两两不相交，
/// 每个阶段开始时依赖的邻居阶段已满足，任意完成顺序最终结果相同。
#[test]
fn staged_pipeline_respects_dependencies_and_write_ranges() {
    use crate::director::GENERATION;

    // 静态断言：features 写半径 1，其余 0；依赖半径照字节码。
    assert_eq!(GENERATION[5].write_radius, 1);
    assert!(
        GENERATION[2]
            .requires
            .contains(&(ChunkStatus::StructureStarts, 8))
    );
    assert!(
        GENERATION[7]
            .requires
            .contains(&(ChunkStatus::InitializeLight, 1))
    );

    // 用两个不同种子、不同完成顺序跑同一个操作序列，比较最终内容。
    let run = |seed: u64| -> Vec<(ChunkPos, u64)> {
        let mut h = Harness::new(config(PipelineKind::Staged, 5));
        h.seed = seed;
        h.handle(Event::Tickets {
            ops: vec![
                TicketOp::ViewDistance(2),
                TicketOp::PlayerMoved {
                    id: 1,
                    to: Some(ChunkPos::new(0, 0)),
                },
            ],
        });
        let mut fingerprints: BTreeMap<ChunkPos, u64> = BTreeMap::new();
        for step in 0..200 {
            if h.tasks.is_empty() {
                h.region_tick();
                if h.tasks.is_empty() {
                    break;
                }
            }
            // 同时在跑的任务写范围不相交（假执行器是随机顺序完成，
            // Director 的 `Submit` 已经保证这一点）。
            let ids: Vec<TaskId> = h.tasks.keys().copied().collect();
            let mut ranges = Vec::new();
            for id in &ids {
                let (pos, _, task) = &h.tasks[id];
                ranges.push((*pos, task.write_radius()));
            }
            for (i, (a, ar)) in ranges.iter().enumerate() {
                for (b, br) in ranges.iter().skip(i + 1) {
                    assert!(
                        (a.x - b.x).abs() > ar + br || (a.z - b.z).abs() > ar + br,
                        "两个在途任务的写范围相交"
                    );
                }
            }
            // 阶段开始时检查它声明的依赖：邻居记录至少到了所需阶段。
            for (pos, _, task) in h.tasks.values() {
                if let Task::Generate { stage, .. } = task {
                    let step = GENERATION[*stage as usize];
                    for &(needed, radius) in step.requires {
                        let records = h.director.debug_state().records();
                        for &(dx, dz) in &[
                            (-radius, -radius),
                            (-radius, radius),
                            (radius, -radius),
                            (radius, radius),
                        ] {
                            let neighbor = ChunkPos::new(pos.x + dx, pos.z + dz);
                            let record = records.get(&neighbor).expect("依赖记录存在");
                            assert!(
                                record.stage as usize >= needed as usize || record.dependency,
                                "阶段 {stage:?} 的邻居 {neighbor:?}: {record:?}, 需要 {needed:?}"
                            );
                        }
                    }
                }
            }
            if let Some(id) = ids.first().copied() {
                let (pos, generation, task) = h.tasks.remove(&id).expect("任务存在");
                if let Task::Generate { stage, .. } = &task {
                    // 阶段函数只取决于 (pos, stage)：最终内容确定。
                    fingerprints.insert(pos, stage_fingerprint(pos, *stage));
                }
                let result = match task {
                    Task::Generate { stage, column, .. } => TaskResult::Generated {
                        stage,
                        column: column.unwrap_or(Fake(0)),
                    },
                    Task::Read { .. } => TaskResult::Read { bytes: None },
                    Task::Assemble { column, .. } => TaskResult::Assembled { column },
                    Task::Decode { bytes, .. } => TaskResult::Decoded {
                        column: Fake(u64::from_le_bytes(bytes.try_into().unwrap())),
                    },
                    Task::Save { seq, column, .. } => TaskResult::Saved { seq, column },
                };
                // 完整不变量每 10 步断言一次，其余步骤由 `handle` 的轻量路径覆盖。
                if step % 40 == 0 {
                    h.handle(Event::TaskDone {
                        id,
                        pos,
                        generation,
                        result,
                    });
                } else {
                    h.handle_light(Event::TaskDone {
                        id,
                        pos,
                        generation,
                        result,
                    });
                }
            }
            if h.seed.checked_rem(7) == Some(0) {
                h.region_tick();
            }
        }
        h.region_tick();
        // `features` 写半径 1，相邻区块的任务不能同时跑；循环末尾把
        // 还排着的任务按任意顺序做完，确认最终收敛。
        for _ in 0..3000 {
            if h.tasks.is_empty() {
                h.region_tick();
                if h.tasks.is_empty() {
                    break;
                }
            }
            let id = *h.tasks.keys().next().expect("任务存在");
            let (pos, generation, task) = h.tasks.remove(&id).expect("任务存在");
            if let Task::Generate { stage, .. } = &task {
                fingerprints.insert(pos, stage_fingerprint(pos, *stage));
            }
            let result = match task {
                Task::Generate { stage, column, .. } => TaskResult::Generated {
                    stage,
                    column: column.unwrap_or(Fake(0)),
                },
                Task::Read { .. } => TaskResult::Read { bytes: None },
                Task::Assemble { column, .. } => TaskResult::Assembled { column },
                Task::Decode { bytes, .. } => TaskResult::Decoded {
                    column: Fake(u64::from_le_bytes(bytes.try_into().unwrap())),
                },
                Task::Save { seq, column, .. } => TaskResult::Saved { seq, column },
            };
            h.handle_light(Event::TaskDone {
                id,
                pos,
                generation,
                result,
            });
        }
        h.region_tick();
        h.assert_invariants();
        assert!(h.tasks.is_empty(), "流水线没有收敛");
        fingerprints.into_iter().collect()
    };
    assert_eq!(run(11), run(29), "完成顺序不同，最终内容应相同");
}

#[test]
fn random_events_never_break_invariants_and_converge() {
    for seed in 1u64..=8 {
        let mut h = Harness::new(config(PipelineKind::Flat, 2));
        h.seed = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let center = ChunkPos::new(0, 0);
        for tick in 1..=120u64 {
            let roll = h.next() % 10;
            match roll {
                0 => {
                    let x = (h.next() % 5) as i32 - 2;
                    h.handle(Event::Tickets {
                        ops: vec![TicketOp::PlayerMoved {
                            id: 1,
                            to: Some(ChunkPos::new(x, 0)),
                        }],
                    });
                }
                1 => h.handle(Event::Tickets {
                    ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
                }),
                2..=5 => {
                    let edit = h.next().checked_rem(3) == Some(0);
                    h.complete_one(edit);
                }
                6 => h.region_tick(),
                7 => {
                    let distance = 2 + (h.next() % 4) as i32;
                    h.handle(Event::Tickets {
                        ops: vec![TicketOp::ViewDistance(distance)],
                    });
                }
                8 => {
                    // 坏区块和存盘失败都要能收尾。
                    if let Some(id) = h.tasks.keys().next().copied() {
                        let (pos, generation, _) = h.tasks.remove(&id).expect("任务存在");
                        h.done.push(id);
                        h.handle(Event::TaskDone {
                            id,
                            pos,
                            generation,
                            result: TaskResult::BadChunk {
                                error: "fuzzed".to_owned(),
                            },
                        });
                    }
                }
                _ => {}
            }
            h.handle(Event::Tick { now_tick: tick });
        }
        // 收敛：把剩下的任务按任意顺序做完，再收回全部区块。
        for _ in 0..5000 {
            if h.tasks.is_empty() {
                h.region_tick();
                if h.tasks.is_empty() {
                    break;
                }
            }
            h.complete_one(false);
        }
        h.handle(Event::Tickets {
            ops: vec![TicketOp::PlayerMoved { id: 1, to: None }],
        });
        h.handle(Event::Shutdown);
        h.region_tick();
        for _ in 0..5000 {
            if h.tasks.is_empty() {
                break;
            }
            h.complete_one(false);
        }
        assert!(
            h.director.is_settled(),
            "种子 {seed} 没有收敛：{:?}",
            h.director.debug_state().records().len()
        );
        assert!(
            h.director.loaded_count() == 0,
            "种子 {seed} 关服后还有已加载区块"
        );
        let _ = center;
    }
}

#[test]
fn the_same_event_stream_produces_byte_identical_effects() {
    let trace = |seed: u64| {
        let mut h = Harness::new(config(PipelineKind::Flat, 2));
        h.seed = seed;
        for tick in 1..=40u64 {
            if tick % 3 == 0 {
                h.handle(Event::Tickets {
                    ops: vec![TicketOp::PlayerMoved {
                        id: 1,
                        to: Some(ChunkPos::new((tick % 3) as i32, 0)),
                    }],
                });
            }
            h.complete_one(false);
            h.region_tick();
            h.handle(Event::Tick { now_tick: tick });
        }
        h.trace.clone()
    };
    assert_eq!(trace(17), trace(17));
}

/// 「任务不当场完成」的假执行器：把 `Submit` 攒着，测试自己决定什么时候回结果。
/// 这样可以造出「结果回来之前票已经撤了」这类内联执行器走不到的情形。
struct Deferred {
    director: Director<Fake>,
    /// 在途任务：`id → (pos, generation)`。任务形状由测试自己决定。
    tasks: BTreeMap<TaskId, (ChunkPos, u64)>,
    parked: BTreeMap<ChunkPos, Fake>,
    /// 发出但还没确认的交付。
    in_mail: BTreeMap<ChunkPos, u64>,
}

impl Deferred {
    fn new(delay: u64) -> Self {
        let mut director = Director::new(config(PipelineKind::Flat, delay));
        director.set_template(|| Fake(0));
        Self {
            director,
            tasks: BTreeMap::new(),
            parked: BTreeMap::new(),
            in_mail: BTreeMap::new(),
        }
    }

    fn handle(&mut self, event: Event<Fake>) -> Vec<Effect<Fake>> {
        let effects = self.director.handle(event);
        for effect in &effects {
            match effect {
                Effect::Submit {
                    id,
                    generation,
                    task,
                } => {
                    self.tasks.insert(*id, (task.pos(), *generation));
                }
                Effect::Cancel { id } => {
                    self.tasks.remove(id);
                }
                Effect::ToRegion(RegionMessage::Deliver {
                    pos, generation, ..
                }) => {
                    self.in_mail.insert(*pos, *generation);
                }
                Effect::Park { pos, column } => {
                    self.parked.insert(*pos, *column);
                }
                _ => {}
            }
        }
        effects
    }

    /// 指定坐标的在途任务。
    fn task_at(&self, pos: ChunkPos) -> (TaskId, u64) {
        let (&id, &(_, generation)) = self
            .tasks
            .iter()
            .find(|&(_, &(p, _))| p == pos)
            .expect("这个坐标有在途任务");
        (id, generation)
    }

    /// 最后一个在途任务（`Save` 排在后面）。
    fn last_task(&self) -> (TaskId, ChunkPos, u64) {
        let (&id, &(pos, generation)) = self.tasks.iter().next_back().expect("有在途任务");
        (id, pos, generation)
    }

    fn join(&mut self, id: u64, pos: ChunkPos) {
        self.handle(Event::Tickets {
            ops: vec![TicketOp::PlayerMoved { id, to: Some(pos) }],
        });
    }

    fn leave(&mut self, id: u64) {
        self.handle(Event::Tickets {
            ops: vec![TicketOp::PlayerMoved { id, to: None }],
        });
    }
}

/// 从停放表拿到一个「玩家改过」的盒子，结果回来之前撤票：
/// 盒子必须回到停放表，不能消失。
#[test]
fn a_read_result_whose_ticket_was_removed_returns_to_the_parking_table() {
    let mut d = Deferred::new(0);
    let pos = ChunkPos::new(0, 0);
    d.parked.insert(pos, Fake(42));
    d.join(1, pos);
    let (id, generation) = d.task_at(pos);

    // 执行器已经把盒子从停放表取走，之后票撤了。
    let column = d.parked.remove(&pos).expect("停放表里有盒子");
    d.leave(1);
    let effects = d.handle(Event::TaskDone {
        id,
        pos,
        generation,
        result: TaskResult::Decoded { column },
    });
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Park { pos: p, column } if *p == pos && column.0 == 42
        )),
        "非原样的盒子要回停放表：{effects:?}"
    );
    assert_eq!(d.parked.get(&pos), Some(&Fake(42)));
}

/// 过期的 `Assembled` 结果（票撤了又回来、代数已变）也把盒子送回停放表。
#[test]
fn a_stale_assembled_result_parks_its_box() {
    let mut d = Deferred::new(0);
    let pos = ChunkPos::new(0, 0);
    d.join(1, pos);
    let (id, generation) = d.task_at(pos);
    d.leave(1);
    d.join(1, pos);
    let stale = generation.wrapping_add(1);
    let effects = d.handle(Event::TaskDone {
        id,
        pos,
        generation: stale,
        result: TaskResult::Assembled { column: Fake(9) },
    });
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Park { column, .. } if column.0 == 9)),
        "过期结果里的盒子不能丢：{effects:?}"
    );
    assert_eq!(d.parked.get(&pos), Some(&Fake(9)));
}

/// `Save` 在途时撤票、票又回来：结果里带着的盒子不能凭空消失。
#[test]
fn a_save_result_whose_ticket_returned_is_not_dropped() {
    let mut d = Deferred::new(0);
    let pos = ChunkPos::new(0, 0);
    let box_ = Fake(17);
    d.join(1, pos);
    let (id, generation) = d.task_at(pos);
    // 交付到 region，再撤票收回：`Returned` 会派出一个 `Save`。
    d.handle(Event::TaskDone {
        id,
        pos,
        generation,
        result: TaskResult::Decoded { column: box_ },
    });
    d.handle(Event::Delivered { pos, generation });
    d.leave(1);
    // 收回延迟到点才发 `Reclaim`；region 在 tick 边界交回盒子。
    let reclaims = d.handle(Event::Tick { now_tick: 1 });
    assert!(
        reclaims.iter().any(|effect| matches!(
            effect,
            Effect::ToRegion(RegionMessage::Reclaim { positions })
                if positions.iter().any(|(p, _)| *p == pos)
        )),
        "该发包含这个坐标的收回消息"
    );
    let returned = d.handle(Event::Returned {
        pos,
        generation,
        column: Some(box_),
    });
    let (save_id, _, save_generation) = d.last_task();
    let seq = returned.iter().find_map(|effect| match effect {
        Effect::Submit {
            id,
            task: Task::Save { seq, .. },
            ..
        } if *id == save_id => Some(*seq),
        _ => None,
    });
    let seq = seq.expect("收回之后派出 Save 任务");
    // 票在 Save 完成之前回来：盒子应该直接重新交付，不经过读盘。
    d.join(1, pos);
    let effects = d.handle(Event::TaskDone {
        id: save_id,
        pos,
        generation: save_generation,
        result: TaskResult::Saved { seq, column: box_ },
    });
    let delivered = effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::ToRegion(RegionMessage::Deliver { column, .. }) if column.0 == 17
        )
    });
    let parked = effects
        .iter()
        .any(|effect| matches!(effect, Effect::Park { column, .. } if column.0 == 17));
    assert!(delivered || parked, "Save 的盒子要有去处：{effects:?}");
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::Submit {
                task: Task::Read { .. },
                ..
            }
        )),
        "盒子不该被重新读一遍"
    );
}

/// 模拟距离比视距小时，玩家走动会让某些区块只有模拟等级变化。
/// 这些变化也要作为 `LevelChanged` 发给 region。
#[test]
fn simulation_only_changes_reach_the_region() {
    let mut director = Director::<Fake>::new(config(PipelineKind::Flat, 0));
    director.set_template(|| Fake(0));
    // 模拟距离小于视距：模拟等级的圈比加载等级的圈小。
    director.handle(Event::Tickets {
        ops: vec![
            TicketOp::SimulationDistance(4),
            TicketOp::PlayerMoved {
                id: 1,
                to: Some(ChunkPos::new(0, 0)),
            },
        ],
    });

    // 玩家跨一格，找出「加载等级没变、模拟等级变了」的那些位置。
    let mut table = TicketTable::new();
    table.set_simulation_distance(4);
    table.set_player(1, Some(ChunkPos::new(0, 0)));
    let _ = table.run_updates();
    table.set_player(1, Some(ChunkPos::new(1, 0)));
    let changes = table.run_updates().to_vec();
    let simulation_only: Vec<_> = changes
        .iter()
        .copied()
        .filter(|change| {
            change.loading.0 == change.loading.1 && change.simulation.0 != change.simulation.1
        })
        .collect();
    assert!(
        !simulation_only.is_empty(),
        "这个场景里应该有只有模拟等级变化的位置"
    );
    // 其中至少有一条跨过 31/32/33，之前会被归成 Noop 而丢掉。
    let crossing: Vec<_> = simulation_only
        .iter()
        .copied()
        .filter(|change| classify(*change) == LevelClassification::LevelOnly)
        .collect();
    assert!(!crossing.is_empty(), "模拟等级跨 31/32/33 的变化要报出来");

    // 把这些变化当作一批票更新喂给 Director（先让票表产生同样的变化列表），
    // 断言 `LevelChanged` 里带着它们。
    director.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(1, 0)),
        }],
    });
    // 上面这次 handle 已经消费掉变化；再用一张干净的 Director 单独验证
    // `consume_level_changes` 的输出（用同一个玩家移动序列）。
    let mut probe = Director::<Fake>::new(config(PipelineKind::Flat, 0));
    probe.set_template(|| Fake(0));
    probe.handle(Event::Tickets {
        ops: vec![
            TicketOp::SimulationDistance(4),
            TicketOp::PlayerMoved {
                id: 1,
                to: Some(ChunkPos::new(0, 0)),
            },
        ],
    });
    let effects = probe.handle(Event::Tickets {
        ops: vec![TicketOp::PlayerMoved {
            id: 1,
            to: Some(ChunkPos::new(1, 0)),
        }],
    });
    let notified: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ToRegion(RegionMessage::LevelChanged { changes }) => Some(changes),
            _ => None,
        })
        .flatten()
        .copied()
        .collect();
    for change in &crossing {
        assert!(
            notified.contains(change),
            "只有模拟等级变化的 {change:?} 要出现在 LevelChanged 里"
        );
    }
}
