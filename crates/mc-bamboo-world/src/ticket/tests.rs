//! 单元测试：官方常量、传播不动点、票叠加、超时、玩家移动、确定性和 Send。

use std::collections::{BTreeMap, BTreeSet};

type Sources = Vec<(ChunkPos, i32)>;

use mc_bamboo_core::ChunkPos;

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
