//! 单元测试：官方常量、传播不动点、票叠加、超时、玩家移动、确定性和 Send。

use std::collections::{BTreeMap, BTreeSet};

type Sources = Vec<(ChunkPos, i32)>;

use mc_bamboo_core::ChunkPos;

use super::spread::{
    PEAK_MARGIN, PEAK_WINDOW, SCRATCH_POOL_CAP, SCRATCH_POOL_MIN, dense_enabled, dense_hits,
};
use super::{
    ChunkStatus, FULL_CHUNK_LEVEL, LevelChange, MAX_LEVEL, MIN_VIEW_DISTANCE, SIMULATION_MAX_LEVEL,
    SavedTicket, TicketTable, TicketType, UNLOADED_LEVEL, full_status, generation_status,
    is_loaded, level::FullChunkStatus, table::Ticket,
};

fn p(x: i32, z: i32) -> ChunkPos {
    ChunkPos::new(x, z)
}

fn player_loading_positions(table: &TicketTable) -> BTreeSet<ChunkPos> {
    table
        .ticket_positions()
        .into_iter()
        .filter(|&pos| {
            table
                .tickets_at(pos)
                .iter()
                .any(|t| t.ty == TicketType::PlayerLoading)
        })
        .collect()
}

/// 直接按定义算不动点，用来检查增量传播。
fn brute_force(sources: &[(ChunkPos, i32)], limit: i32) -> BTreeMap<ChunkPos, i32> {
    let mut map: BTreeMap<ChunkPos, i32> = BTreeMap::new();
    for &(pos, level) in sources {
        let entry = map.entry(pos).or_insert(limit);
        *entry = (*entry).min(level);
    }
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot = map.clone();
        for (&pos, &level) in &snapshot {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let neighbor = p(pos.x + dx, pos.z + dz);
                    let candidate = (level + 1).min(limit);
                    let entry = map.entry(neighbor).or_insert(limit);
                    if candidate < *entry {
                        *entry = candidate;
                        changed = true;
                    }
                }
            }
        }
    }
    map.retain(|_, level| *level < limit);
    map
}

#[test]
fn brute_force_reference_is_the_same_fixed_point() {
    let sources = [(p(0, 0), 31), (p(3, 2), 33), (p(-4, 1), 40)];
    let map = brute_force(&sources, UNLOADED_LEVEL);
    for x in -8..=8 {
        for z in -8..=8 {
            let expected = sources
                .iter()
                .map(|&(s, l)| l + (s.x - x).abs().max((s.z - z).abs()))
                .min()
                .unwrap()
                .min(UNLOADED_LEVEL);
            assert_eq!(*map.get(&p(x, z)).unwrap_or(&UNLOADED_LEVEL), expected);
        }
    }
}

/// 用一批随机操作喂 TicketTable，每一步都把增量结果和暴力不动点对照。
fn check_against_brute_force(seed: u64, steps: usize, with_players: bool) {
    let mut state = seed;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut table = TicketTable::new();

    for step in 0..steps {
        match rand() % 10 {
            0..=4 => {
                let ty = TicketType::ALL[(rand() % 9) as usize];
                let pos = p((rand() % 15) as i32 - 7, (rand() % 15) as i32 - 7);
                let level = (rand() % 40) as i32;
                if rand() % 3 == 0 {
                    table.remove(ty, pos, level);
                } else {
                    table.add(ty, pos, level);
                }
            }
            5 if with_players => {
                let id = rand() % 4;
                if rand() % 4 == 0 {
                    table.set_player(id, None);
                } else {
                    table.set_player(
                        id,
                        Some(p((rand() % 11) as i32 - 5, (rand() % 11) as i32 - 5)),
                    );
                }
            }
            6 if with_players => table.set_view_distance((rand() % 8) as i32 + 2),
            7 if with_players => table.set_simulation_distance((rand() % 12) as i32),
            8 => table.set_forced(
                p((rand() % 9) as i32 - 4, (rand() % 9) as i32 - 4),
                rand() % 2 == 0,
            ),
            _ => table.tick_idle(),
        }

        let changes = table.run_updates().to_vec();
        // 变化列表有序、无重复、每一项都真的变了。
        assert!(
            changes.windows(2).all(|w| w[0].pos < w[1].pos),
            "第 {step} 步输出没排序"
        );
        for change in &changes {
            assert!(
                change.loading.0 != change.loading.1 || change.simulation.0 != change.simulation.1
            );
            assert_eq!(table.loading_level(change.pos), change.loading.1);
            assert_eq!(table.simulation_level(change.pos), change.simulation.1);
        }

        // 直接从表里把全部票读出来，按官方 getLowestTicket 的规则拆成两个平面的源。
        let (loading_sources, simulation_sources) = table_sources(&table);
        for x in -20..=20 {
            for z in -20..=20 {
                let pos = p(x, z);
                let expected = loading_sources
                    .iter()
                    .map(|&(s, level)| {
                        level
                            + (s.x as i64 - x as i64)
                                .abs()
                                .max((s.z as i64 - z as i64).abs())
                                as i32
                    })
                    .min()
                    .unwrap_or(UNLOADED_LEVEL)
                    .min(UNLOADED_LEVEL);
                assert_eq!(
                    table.loading_level(pos),
                    expected,
                    "第 {step} 步 {pos:?} 加载等级"
                );
                let expected = simulation_sources
                    .iter()
                    .map(|&(s, level)| {
                        level
                            + (s.x as i64 - x as i64)
                                .abs()
                                .max((s.z as i64 - z as i64).abs())
                                as i32
                    })
                    .min()
                    .unwrap_or(SIMULATION_MAX_LEVEL)
                    .min(SIMULATION_MAX_LEVEL);
                assert_eq!(
                    table.simulation_level(pos),
                    expected,
                    "第 {step} 步 {pos:?} 模拟等级"
                );
            }
        }
    }
}

/// 把表里的票读出来当暴力不动点的输入。
fn table_sources(table: &TicketTable) -> (Sources, Sources) {
    let mut loading = Vec::new();
    let mut simulation = Vec::new();
    for pos in table.ticket_positions() {
        let mut l = UNLOADED_LEVEL;
        let mut s = SIMULATION_MAX_LEVEL;
        for ticket in table.tickets_at(pos) {
            if ticket.ty.loads() {
                l = l.min(ticket.level.max(0));
            }
            if ticket.ty.simulates() {
                s = s.min(ticket.level.max(0));
            }
        }
        if l < UNLOADED_LEVEL {
            loading.push((pos, l));
        }
        if s < SIMULATION_MAX_LEVEL {
            simulation.push((pos, s));
        }
    }
    (loading, simulation)
}

#[test]
fn empty_table_is_unloaded() {
    let mut table = TicketTable::new();
    for x in -3..=3 {
        for z in -3..=3 {
            assert_eq!(table.loading_level(p(x, z)), UNLOADED_LEVEL);
            assert_eq!(table.simulation_level(p(x, z)), SIMULATION_MAX_LEVEL);
        }
    }
    assert_eq!(table.level_count(), 0);
    assert!(!table.keep_dimension_active());
    assert!(table.run_updates().is_empty());
}

#[test]
fn propagation_matches_official_levels() {
    let mut table = TicketTable::new();
    table.add(TicketType::Forced, p(0, 0), 31);
    table.run_updates();
    // 官方视距 10 的一行：中心 31，向外每格 +1，到 45 为止（模拟平面到 33）。
    let loading: Vec<i32> = (-16..=16).map(|x| table.loading_level(p(x, 0))).collect();
    let expected: Vec<i32> = (-16..=16)
        .map(|x: i32| (31 + x.abs()).min(UNLOADED_LEVEL))
        .collect();
    assert_eq!(loading, expected);
    let simulation: Vec<i32> = (-16..=16)
        .map(|x| table.simulation_level(p(x, 0)))
        .collect();
    let expected: Vec<i32> = (-16..=16)
        .map(|x: i32| (31 + x.abs()).min(SIMULATION_MAX_LEVEL))
        .collect();
    assert_eq!(simulation, expected);
    // 切比雪夫距离：对角线和直线一样。
    assert_eq!(table.loading_level(p(3, 4)), 35);
    assert_eq!(
        table.simulation_level(p(3, 4)),
        35_i32.min(SIMULATION_MAX_LEVEL)
    );
    // 等级边界换算。
    assert_eq!(full_status(31), FullChunkStatus::EntityTicking);
    assert_eq!(generation_status(34), Some(ChunkStatus::InitializeLight));
    assert!(is_loaded(44) && !is_loaded(45));
    assert_eq!(MAX_LEVEL + 1, UNLOADED_LEVEL);
}

#[test]
fn multiple_tickets_take_minimum_and_removal_recomputes() {
    let mut table = TicketTable::new();
    table.add(TicketType::Portal, p(0, 0), 33);
    table.add(TicketType::PlayerSpawn, p(0, 0), 31);
    table.add(TicketType::EnderPearl, p(3, 0), 31);
    let changes = table.run_updates().to_vec();
    assert!(!changes.is_empty());
    assert_eq!(table.loading_level(p(0, 0)), 31);
    assert_eq!(table.loading_level(p(1, 0)), 32);
    assert_eq!(table.loading_level(p(2, 0)), 32); // 被 (3,0) 的 31 拉到 32
    assert_eq!(table.loading_level(p(3, 0)), 31);

    // 撤掉最小的那张，等级由 portal 的 33 决定。
    table.remove(TicketType::PlayerSpawn, p(0, 0), 31);
    let changes = table.run_updates().to_vec();
    assert!(
        changes
            .iter()
            .any(|c| c.pos == p(0, 0) && c.loading.1 == 33)
    );
    assert_eq!(table.loading_level(p(0, 0)), 33);
    assert_eq!(table.loading_level(p(1, 0)), 33); // ender_pearl 在 (3,0) 给 33
    assert_eq!(table.simulation_level(p(0, 0)), 33); // ender_pearl 在 (3,0) 给 34，钳到 33

    // 撤光之后回到无票。
    table.remove(TicketType::Portal, p(0, 0), 33);
    table.remove(TicketType::EnderPearl, p(3, 0), 31);
    table.run_updates();
    assert_eq!(table.loading_level(p(0, 0)), UNLOADED_LEVEL);
    assert_eq!(table.loading_level(p(3, 0)), UNLOADED_LEVEL);
}

#[test]
fn duplicate_ticket_resets_timeout_without_duplicating() {
    let mut table = TicketTable::new();
    table.add(TicketType::Portal, p(1, 1), 31);
    assert_eq!(table.tickets_at(p(1, 1)).len(), 1);
    table.tick_idle();
    assert_eq!(table.tickets_at(p(1, 1))[0].ticks_left, 299);
    table.add(TicketType::Portal, p(1, 1), 31);
    assert_eq!(table.tickets_at(p(1, 1)).len(), 1);
    assert_eq!(table.tickets_at(p(1, 1))[0].ticks_left, 300);
}

#[test]
fn timeouts_expire_at_the_official_tick() {
    // spawn_search / unknown 超时 1：第 1 刻变成 0，第 2 刻变 -1 被删。
    let mut table = TicketTable::new();
    table.add(TicketType::SpawnSearch, p(0, 0), 31);
    table.run_updates();
    assert_eq!(table.loading_level(p(0, 0)), 31);
    table.tick_idle();
    assert_eq!(table.tickets_at(p(0, 0)).len(), 1);
    table.tick_idle();
    assert!(table.tickets_at(p(0, 0)).is_empty());
    let changes = table.run_updates();
    assert!(changes.iter().any(|c| c.pos == p(0, 0)));
    assert_eq!(table.loading_level(p(0, 0)), UNLOADED_LEVEL); // 唯一的源没了，邻居也回到无票
    table.tick_idle();
    table.run_updates();
    assert_eq!(table.loading_level(p(0, 0)), UNLOADED_LEVEL);

    // portal 300 刻：第 301 次 tick 才删。
    let mut table = TicketTable::new();
    table.add(TicketType::Portal, p(0, 0), 31);
    for _ in 0..300 {
        table.tick_idle();
        assert_eq!(table.tickets_at(p(0, 0)).len(), 1);
    }
    table.tick_idle();
    assert!(table.tickets_at(p(0, 0)).is_empty());
}

#[test]
fn busy_chunk_does_not_count_down_regular_tickets() {
    let mut table = TicketTable::new();
    table.add(TicketType::Portal, p(0, 0), 31);
    table.add(TicketType::Unknown, p(5, 5), 31);
    for _ in 0..5 {
        table.tick(&mut |_| true);
    }
    // portal 不递减；unknown 有 CAN_EXPIRE_IF_UNLOADED，照常在第 2 刻过期。
    let portal = table
        .tickets_at(p(0, 0))
        .into_iter()
        .find(|t| t.ty == TicketType::Portal)
        .expect("portal 票还在");
    assert_eq!(portal.ticks_left, 300);
    assert!(table.tickets_at(p(5, 5)).is_empty());
}

#[test]
fn player_tickets_cover_the_view_square_and_simulation_radius() {
    let mut table = TicketTable::new();
    table.set_player(1, Some(p(0, 0)));
    table.run_updates();
    // 视距 10：方形里每个区块都是 31，方形外第一圈是 32。
    for x in -10..=10 {
        for z in -10..=10 {
            assert_eq!(table.loading_level(p(x, z)), 31, "{x},{z}");
        }
    }
    assert_eq!(table.loading_level(p(11, 0)), 32);
    assert_eq!(table.loading_level(p(0, 11)), 32);
    assert_eq!(table.loading_level(p(11, 11)), 32);
    assert_eq!(table.loading_level(p(12, 0)), 33);
    // 模拟票只有玩家所在区块一张，等级 31-10=21。
    assert_eq!(table.tickets_at(p(0, 0)).len(), 2);
    assert_eq!(
        table.tickets_at(p(0, 0)),
        vec![
            Ticket::new(TicketType::PlayerLoading, 31),
            Ticket::new(TicketType::PlayerSimulation, 21),
        ]
    );
    assert_eq!(table.simulation_level(p(0, 0)), 21);
    assert_eq!(table.simulation_level(p(2, 0)), 23);
    assert_eq!(table.simulation_level(p(12, 0)), 33);
    assert_eq!(table.simulation_level(p(13, 0)), 33);
}

#[test]
fn player_move_only_touches_the_two_border_strips() {
    let mut table = TicketTable::new();
    table.set_player(1, Some(p(0, 0)));
    table.run_updates();
    let before = player_loading_positions(&table);
    table.set_player(1, Some(p(1, 0)));
    let after = player_loading_positions(&table);
    let entered: Vec<_> = after.difference(&before).copied().collect();
    let left: Vec<_> = before.difference(&after).copied().collect();
    // 视距 10 跨一格：一排进、一排出，各 21 个区块。
    assert_eq!(entered.len(), 21, "{entered:?}");
    assert_eq!(left.len(), 21, "{left:?}");
    assert!(entered.iter().all(|pos| pos.x == 11));
    assert!(left.iter().all(|pos| pos.x == -10));
    let changes = table.run_updates().to_vec();
    assert!(
        changes
            .iter()
            .any(|c| c.pos == p(11, 0) && c.loading.1 == 31)
    );
    assert_eq!(table.loading_level(p(11, 0)), 31);
    assert_eq!(table.loading_level(p(-10, 0)), 32);
    assert_eq!(table.simulation_level(p(1, 0)), 21);
    assert_eq!(table.simulation_level(p(0, 0)), 22);
}

#[test]
fn players_overlap_with_reference_counting() {
    let mut table = TicketTable::new();
    table.set_player(1, Some(p(0, 0)));
    table.set_player(2, Some(p(2, 0)));
    table.run_updates();
    assert_eq!(table.loading_level(p(10, 0)), 31);
    assert_eq!(table.loading_level(p(12, 0)), 31);
    table.set_player(1, None);
    table.run_updates();
    // 2 号玩家还在，重叠部分不能撤票。
    assert_eq!(table.loading_level(p(2, 0)), 31);
    assert_eq!(table.loading_level(p(10, 0)), 31); // 仍在 2 号玩家的方形里
    assert_eq!(table.loading_level(p(12, 0)), 31);
    assert_eq!(table.simulation_level(p(0, 0)), 23); // 2 号玩家的模拟源在 (2,0)
    table.set_player(2, None);
    table.run_updates();
    assert_eq!(table.loading_level(p(0, 0)), UNLOADED_LEVEL);
    assert_eq!(
        table.level_count(),
        0,
        "残留：{:?}",
        table.ticket_positions()
    );
}

#[test]
fn view_and_simulation_distance_changes() {
    let mut table = TicketTable::new();
    table.set_player(7, Some(p(0, 0)));
    table.set_view_distance(4);
    table.run_updates();
    assert_eq!(table.view_distance(), 4);
    for x in -4..=4 {
        assert_eq!(table.loading_level(p(x, 0)), 31);
    }
    assert_eq!(table.loading_level(p(5, 0)), 32);
    // 越界输入夹到 2..=32，不 panic。
    table.set_view_distance(i32::MIN);
    assert_eq!(table.view_distance(), MIN_VIEW_DISTANCE);
    table.set_view_distance(i32::MAX);
    assert_eq!(table.view_distance(), 32);
    table.set_view_distance(4);
    table.run_updates();

    table.set_simulation_distance(6);
    table.run_updates();
    assert_eq!(table.simulation_distance(), 6);
    assert_eq!(table.tickets_at(p(0, 0))[1].level, 25);
    assert_eq!(table.simulation_level(p(0, 0)), 25);
    assert_eq!(table.simulation_level(p(8, 0)), 33);
    table.set_simulation_distance(0);
    table.run_updates();
    assert_eq!(table.simulation_level(p(0, 0)), 31);
    assert_eq!(table.simulation_level(p(1, 0)), 32);
    assert_eq!(table.simulation_level(p(2, 0)), 33);
}

#[test]
fn forced_chunks_and_persistence() {
    let mut table = TicketTable::new();
    table.set_forced(p(3, 4), true);
    table.set_forced(p(-3, -4), true);
    table.add(TicketType::Portal, p(8, 8), 31);
    table.add(TicketType::PlayerSpawn, p(9, 9), 31);
    table.run_updates();
    assert_eq!(table.forced_chunks(), vec![p(-3, -4), p(3, 4)]);
    assert_eq!(table.loading_level(p(3, 4)), 31);
    assert_eq!(table.simulation_level(p(3, 4)), 31);
    assert!(table.keep_dimension_active());
    let saved = table.persistent_tickets();
    assert_eq!(
        saved,
        vec![
            SavedTicket {
                pos: p(-3, -4),
                ty: TicketType::Forced,
                level: 31,
                ticks_left: 0
            },
            SavedTicket {
                pos: p(3, 4),
                ty: TicketType::Forced,
                level: 31,
                ticks_left: 0
            },
            SavedTicket {
                pos: p(8, 8),
                ty: TicketType::Portal,
                level: 31,
                ticks_left: 300
            },
        ]
    );
    let mut restored = TicketTable::new();
    restored.restore_tickets(saved.clone());
    // 非存盘票装不进去。
    restored.restore_tickets(vec![SavedTicket {
        pos: p(9, 9),
        ty: TicketType::PlayerSpawn,
        level: 31,
        ticks_left: 20,
    }]);
    restored.run_updates();
    assert_eq!(restored.persistent_tickets(), saved);
    assert_eq!(
        restored.loading_level(p(3, 4)),
        table.loading_level(p(3, 4))
    );
    assert!(restored.tickets_at(p(9, 9)).is_empty());
    table.set_forced(p(3, 4), false);
    assert_eq!(table.forced_chunks(), vec![p(-3, -4)]);
}

#[test]
fn add_with_radius_matches_official_formula() {
    let mut table = TicketTable::new();
    table.add_with_radius(TicketType::Forced, p(0, 0), 2);
    assert_eq!(table.tickets_at(p(0, 0))[0].level, FULL_CHUNK_LEVEL - 2);
    table.remove_with_radius(TicketType::Forced, p(0, 0), 2);
    assert!(table.tickets_at(p(0, 0)).is_empty());
}

#[test]
fn run_updates_is_deterministic_and_exact() {
    fn scenario() -> Vec<Vec<LevelChange>> {
        let mut table = TicketTable::new();
        let mut out = Vec::new();
        for i in 0..30_i32 {
            let x = i - 15;
            table.add(TicketType::Portal, p(x, x % 5), 20 + (x % 15));
            table.add(TicketType::EnderPearl, p(x / 2, -x / 3), 31);
            table.set_player(1, Some(p(x / 4, 0)));
            if i % 7 == 0 {
                table.set_view_distance(3 + i % 9);
                table.set_simulation_distance(4 + i % 11);
            }
            if i % 5 == 0 {
                table.remove(
                    TicketType::Portal,
                    p(x - 2, (x - 2) % 5),
                    20 + ((x - 2) % 15),
                );
            }
            table.tick_idle();
            out.push(table.run_updates().to_vec());
        }
        out
    }
    let a = scenario();
    let b = scenario();
    assert_eq!(a, b, "同样的操作序列必须逐字节相同");
    assert!(a.iter().any(|step| !step.is_empty()));
}

#[test]
fn incremental_propagation_matches_brute_force() {
    check_against_brute_force(0x5eed_1234_5678_9abc, 90, false);
    check_against_brute_force(0xdead_beef_cafe_0001, 60, true);
    check_against_brute_force(0x0123_4567_89ab_cdef, 60, true);
}

/// 窗口边界、负坐标、跨 64 块、多源重叠和撤源：强制走快路径逐步对暴力不动点。
#[test]
fn dense_window_handles_boundaries_and_source_removal() {
    let _guard = TicketTable::force_dense(true);
    let mut table = TicketTable::new();
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for step in 0..120 {
        // 坐标特意跨 64 的块边界和正负号，等级覆盖 0、31、44、45 几个临界值。
        let x = match step % 4 {
            0 => (rand() % 9) as i32 - 68,
            1 => (rand() % 9) as i32 + 60,
            2 => (rand() % 141) as i32 - 70,
            _ => -((rand() % 141) as i32),
        };
        let z = (rand() % 141) as i32 - 70;
        let level = [0, 1, 30, 31, 32, 33, 34, 44, 45][(rand() % 9) as usize];
        let ty = TicketType::ALL[(rand() % 9) as usize];
        if rand() % 2 == 0 {
            table.add(ty, p(x, z), level);
        } else {
            table.remove(ty, p(x, z), level);
        }
        if step % 11 == 0 {
            table.set_player(7, Some(p((rand() % 9) as i32 - 68, 63)));
        }
        if step % 13 == 0 {
            table.set_view_distance((rand() % 12) as i32 - 1);
        }
        let changes = table.run_updates().to_vec();
        assert!(changes.windows(2).all(|w| w[0].pos < w[1].pos));
        let (loading_sources, simulation_sources) = table_sources(&table);
        for bx in -80..=80 {
            for bz in -80..=80 {
                let pos = p(bx, bz);
                assert_eq!(
                    table.loading_level(pos),
                    min_distance_level(&loading_sources, pos, UNLOADED_LEVEL),
                    "第 {step} 步 {pos:?} 加载等级"
                );
                assert_eq!(
                    table.simulation_level(pos),
                    min_distance_level(&simulation_sources, pos, SIMULATION_MAX_LEVEL),
                    "第 {step} 步 {pos:?} 模拟等级"
                );
            }
        }
    }
}

/// 同一轮里两个相距很远的源必须回退，回退路径仍然算出同一个不动点。
#[test]
fn sparse_updates_fall_back_to_two_phase() {
    let mut table = TicketTable::new();
    // 两个变化落在同一个 run_updates 里，包围盒宽高都远超单窗口的格数硬上限。
    let before = dense_hits();
    table.add(TicketType::Forced, p(-300_000, -300_000), 0);
    table.add(TicketType::Forced, p(300_000, 300_000), 0);
    let changes = table.run_updates().to_vec();
    assert_eq!(dense_hits(), before, "超出窗口硬上限时不该走稠密窗口");
    assert!(changes.windows(2).all(|w| w[0].pos < w[1].pos));
    let (loading_sources, simulation_sources) = table_sources(&table);
    for &(x, z) in &[
        (-300_000, -300_000),
        (300_000, 300_000),
        (0, 0),
        (-300_001, -299_999),
        (300_044, 300_044),
    ] {
        let pos = p(x, z);
        assert_eq!(
            table.loading_level(pos),
            min_distance_level(&loading_sources, pos, UNLOADED_LEVEL)
        );
        assert_eq!(
            table.simulation_level(pos),
            min_distance_level(&simulation_sources, pos, SIMULATION_MAX_LEVEL)
        );
    }
}

/// 单个玩家跨一格时窗口很小，正常判据下也必须命中快路径（不是悄悄回退）。
#[test]
fn small_move_takes_the_dense_window_path() {
    if !dense_enabled() {
        // 环境变量把快路径整条关掉时，这一测的是回退路径；其余测试仍照常覆盖。
        return;
    }
    let mut table = TicketTable::new();
    table.set_player(1, Some(p(0, 0)));
    table.run_updates();
    let before = dense_hits();
    for x in 1..=8 {
        table.set_player(1, Some(p(x, 0)));
        table.run_updates();
    }
    let hits = dense_hits() - before;
    assert!(hits >= 8, "两次平面各命中一次，8 步至少 16 次，实际 {hits}");
}

/// 稠密窗口快路径和回退路径必须算出同一个不动点：强制走快路径再跑一遍随机序列。
#[test]
fn dense_window_matches_brute_force_when_forced() {
    let _guard = TicketTable::force_dense(true);
    check_against_brute_force(0x5eed_1234_5678_9abc, 90, false);
    check_against_brute_force(0xdead_beef_cafe_0001, 60, true);
    check_against_brute_force(0x0123_4567_89ab_cdef, 60, true);
}

/// 更长的随机序列：每步把两张平面的全部等级和 `run_updates` 的输出一起对一遍。
///
/// 传播改法只要让某一步的某个区块差一级、或者变化列表多/少一项，这里就会停在那一步。
/// 视距和模拟距离也随机变，覆盖方环、`replaceTicketLevelOfType` 和多玩家引用计数。
/// 长随机参照测试用的种子；强制走快路径的一版用同一组种子，方便逐位对照。
const LONG_RANDOM_SEEDS: &[u64] = &[
    0x5150_0a5a_1234_5678,
    0xfeed_face_dead_beef,
    0x9e37_79b9_7f4a_7c15,
    0x0bad_c0de_0000_0026u64,
    0xaaaa_bbbb_cccc_dddd,
    0x1111_2222_3333_4444,
    0xdead_0000_beef_ffff,
    0x0000_0000_0000_0001,
];

#[test]
fn long_random_sequence_matches_full_fixed_point() {
    long_random_sequence_with_seeds(LONG_RANDOM_SEEDS);
}

/// 强制稠密窗口快路径下的长随机序列：每步把全部等级和变化列表对一遍暴力不动点。
#[test]
fn dense_window_long_random_matches_full_fixed_point() {
    let _guard = TicketTable::force_dense(true);
    // 视距和模拟距离随机变化会撑出大窗口，这里覆盖到窗口边界、负坐标和多源重叠。
    long_random_sequence_with_seeds(LONG_RANDOM_SEEDS);
}

fn long_random_sequence_with_seeds(seeds: &[u64]) {
    for &seed in seeds {
        let mut state: u64 = seed;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut table = TicketTable::new();
        let mut players: [Option<ChunkPos>; 4] = [None; 4];

        for step in 0..200 {
            match rand() % 12 {
                0..=3 => {
                    let ty = TicketType::ALL[(rand() % 9) as usize];
                    let pos = p((rand() % 25) as i32 - 12, (rand() % 25) as i32 - 12);
                    let level = (rand() % 46) as i32;
                    if rand() % 3 == 0 {
                        table.remove(ty, pos, level);
                    } else {
                        table.add(ty, pos, level);
                    }
                }
                4 => {
                    let pos = p((rand() % 13) as i32 - 6, (rand() % 13) as i32 - 6);
                    table.set_forced(pos, rand() % 2 == 0);
                }
                5..=8 => {
                    let id = (rand() % 4) as usize;
                    let next = if rand() % 7 == 0 {
                        None
                    } else {
                        let previous = players[id].unwrap_or(p(0, 0));
                        // 每步只跨 0 或 1 格，让玩家票集中在一小片区域。
                        let step = |rand: &mut dyn FnMut() -> u64| (rand() % 3) as i32 - 1;
                        Some(p(
                            previous.x.saturating_add(step(&mut rand)),
                            previous.z.saturating_add(step(&mut rand)),
                        ))
                    };
                    players[id] = next;
                    table.set_player(id as u64, next);
                }
                9 => table.set_view_distance((rand() % 12) as i32 - 1),
                10 => table.set_simulation_distance((rand() % 35) as i32 - 1),
                _ => table.tick_idle(),
            }

            let changes = table.run_updates().to_vec();
            assert!(
                changes.windows(2).all(|w| w[0].pos < w[1].pos),
                "seed {seed:#x} 第 {step} 步输出没排序"
            );
            // 变化列表必须恰好是等级变了的区块：先按表里的旧值反推，再逐格核对。
            let before: BTreeMap<ChunkPos, (i32, i32)> = changes
                .iter()
                .map(|change| (change.pos, (change.loading.0, change.simulation.0)))
                .collect();
            let (loading_sources, simulation_sources) = table_sources(&table);
            let mut expected = BTreeMap::new();
            for x in -70..=70 {
                for z in -70..=70 {
                    let pos = p(x, z);
                    let loading = min_distance_level(&loading_sources, pos, UNLOADED_LEVEL);
                    let simulation =
                        min_distance_level(&simulation_sources, pos, SIMULATION_MAX_LEVEL);
                    assert_eq!(
                        table.loading_level(pos),
                        loading,
                        "seed {seed:#x} 第 {step} 步 {pos:?} 加载等级"
                    );
                    assert_eq!(
                        table.simulation_level(pos),
                        simulation,
                        "seed {seed:#x} 第 {step} 步 {pos:?} 模拟等级"
                    );
                    let old = before.get(&pos).copied().unwrap_or((loading, simulation));
                    if old.0 != loading || old.1 != simulation {
                        expected.insert(pos, (old, (loading, simulation)));
                    }
                }
            }
            assert_eq!(
                changes.len(),
                expected.len(),
                "seed {seed:#x} 第 {step} 步变化列表长度"
            );
            for change in &changes {
                assert_eq!(
                    change.loading.1,
                    table.loading_level(change.pos),
                    "seed {seed:#x} 第 {step} 步 {change:?} 加载新值"
                );
                assert_eq!(
                    change.simulation.1,
                    table.simulation_level(change.pos),
                    "seed {seed:#x} 第 {step} 步 {change:?} 模拟新值"
                );
                assert_eq!(
                    expected.get(&change.pos),
                    Some(&(
                        (change.loading.0, change.simulation.0),
                        (change.loading.1, change.simulation.1),
                    )),
                    "seed {seed:#x} 第 {step} 步多报了 {change:?}"
                );
            }
        }
    }
}

fn min_distance_level(sources: &Sources, pos: ChunkPos, limit: i32) -> i32 {
    sources
        .iter()
        .map(|&(source, level)| {
            level
                + (source.x as i64 - pos.x as i64)
                    .abs()
                    .max((source.z as i64 - pos.z as i64).abs()) as i32
        })
        .min()
        .unwrap_or(limit)
        .min(limit)
}

#[test]
fn all_public_types_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<TicketTable>();
    assert_send::<LevelChange>();
    assert_send::<TicketType>();
    assert_send::<Ticket>();
    assert_send::<SavedTicket>();
    assert_send::<ChunkStatus>();
    assert_send::<FullChunkStatus>();
}

#[test]
fn out_of_range_inputs_do_not_panic() {
    let mut table = TicketTable::new();
    table.set_view_distance(-1);
    table.set_view_distance(1000);
    table.set_simulation_distance(-100);
    table.set_simulation_distance(i32::MAX);
    table.add(TicketType::Forced, p(i32::MIN, i32::MAX), -5);
    table.add(TicketType::Portal, p(0, 0), i32::MAX);
    table.add_with_radius(TicketType::Unknown, p(0, 0), i32::MIN);
    table.set_player(1, Some(p(i32::MIN, 0)));
    table.set_player(1, Some(p(i32::MAX, 0)));
    table.set_player(1, None);
    for _ in 0..5 {
        table.tick_idle();
        table.run_updates();
    }
}

#[test]
fn scratch_blocks_stay_bounded_while_moving_away() {
    let mut table = TicketTable::new();
    table.set_player(1, Some(p(0, 0)));
    table.run_updates();
    let first = table.scratch_blocks();
    assert!(first > 0, "更新之后辅助结构应该用到过块");
    // 先走过一段有代表性的距离，让池子达到稳态，再往后走几千格。
    for tick in 1..=256 {
        table.set_player(1, Some(p(tick, 0)));
        table.run_updates();
    }
    let steady = table.scratch_blocks();
    let mut peak = steady;
    for tick in 257..=4000 {
        table.set_player(1, Some(p(tick, 0)));
        table.run_updates();
        peak = peak.max(table.scratch_blocks());
    }
    // 上界和视距有关：两张平面各不超过 (2 * (视距 + 传播半径) / 64 + 2)^2 * 4 个块。
    let bound = 2 * 4 * ((2 * (table.view_distance() + 12) / 64) + 2).pow(2) as usize;
    assert!(
        peak <= bound,
        "辅助块数没有上界：起始 {first}，稳态 {steady}，峰值 {peak}，上界 {bound}"
    );
    assert_eq!(table.scratch_blocks(), steady, "稳态之后块数应该不再变化");
}

/// 快路径的稠密缓冲也按近期峰值回收：走过一大片之后峰值滑出窗口，缓冲缩回。
#[test]
fn dense_buffer_recycles_after_a_large_window() {
    if !dense_enabled() {
        return;
    }
    let mut table = TicketTable::new();
    // 一个玩家的小窗口稳态。
    table.set_player(1, Some(p(0, 0)));
    for x in 1..=PEAK_WINDOW as i32 {
        table.set_player(1, Some(p(x, 0)));
        table.run_updates();
    }
    let steady = table.scratch_blocks();
    assert!(steady > 0);
    // 一批玩家分散在一段仍有界、但比单个视野大得多的区域里，撑大窗口。
    for id in 0..40u64 {
        table.set_player(id, Some(p(id as i32 * 23, (id as i32 % 7) * 9)));
    }
    table.run_updates();
    let peak = table.scratch_blocks();
    assert!(peak > steady, "大窗口应该撑大辅助结构：{steady} → {peak}");
    assert!(dense_hits() > 0, "这个场景应该走快路径");
    // 全部离开，跑够一个完整的峰值窗口。
    for id in 0..40u64 {
        table.set_player(id, None);
    }
    for _ in 0..=PEAK_WINDOW * 2 {
        table.run_updates();
    }
    assert!(
        table.scratch_blocks() <= steady + 2 * (PEAK_MARGIN + SCRATCH_POOL_MIN),
        "峰值过去后辅助结构要缩回稳态附近：{} → {}",
        peak,
        table.scratch_blocks()
    );
}

#[test]
fn scratch_pool_recycles_after_two_hundred_players_move_apart() {
    let mut table = TicketTable::new();
    // 200 名玩家分散到视距之外，每人沿不同方向越走越远；近期峰值远高于
    // 单个玩家的一次更新，池子既要跟得上，也要在峰值过去之后缩回来。
    for id in 0..200u64 {
        let step = id as i32;
        table.set_player(id, Some(p(step * 97, step * 61)));
    }
    table.run_updates();
    let mut peak = table.scratch_blocks();
    for tick in 1..=20 {
        for id in 0..200u64 {
            let step = id as i32;
            table.set_player(
                id,
                Some(p(step * 97 + step.signum() * tick, step * 61 - tick)),
            );
        }
        table.run_updates();
        peak = peak.max(table.scratch_blocks());
        assert!(
            table.scratch_blocks() <= 2 * SCRATCH_POOL_CAP,
            "辅助块池超过硬上限：{} > {}",
            table.scratch_blocks(),
            2 * SCRATCH_POOL_CAP
        );
    }
    assert!(peak > 2 * SCRATCH_POOL_MIN, "分散场景确实撑大了池子");

    // 玩家全部离开；再跑够一个完整的峰值窗口，旧峰值逐条滑出窗口。
    for id in 0..200u64 {
        table.set_player(id, None);
    }
    for _ in 0..=PEAK_WINDOW * 2 {
        table.run_updates();
        assert!(
            table.scratch_blocks() <= 2 * SCRATCH_POOL_CAP,
            "回收途中也不该超过硬上限"
        );
    }
    assert!(
        table.scratch_blocks() <= 2 * (SCRATCH_POOL_MIN + PEAK_MARGIN),
        "峰值过去之后池子要缩回最小值附近：{}",
        table.scratch_blocks()
    );
}
