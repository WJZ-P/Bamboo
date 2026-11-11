//! 票的发出：进服、离开、跨区块、视距和模拟距离变化、超时递减。
//!
//! 提交 4 只把票接进世界线程，还不派区块任务；提交 5 接 Director 时
//! 这些票会变成加载/卸载动作。

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{
    director::TicketOp,
    ticket::{TicketType, UNLOADED_LEVEL},
};

use crate::world::tests::{Harness, SPAWN_X, SPAWN_Z};

const SPAWN_CHUNK: ChunkPos = ChunkPos::new(0, 0);

/// 进服挂出生点的玩家票，离开撤掉。
#[test]
fn joining_and_leaving_add_and_remove_player_tickets() {
    let mut server = Harness::new(2);
    assert_eq!(
        server.tickets().loading_level(SPAWN_CHUNK),
        UNLOADED_LEVEL,
        "没有玩家时没有票"
    );
    let client = server.join_and_settle("Kitten");
    assert_eq!(
        server.tickets().loading_level(SPAWN_CHUNK),
        31,
        "玩家所在区块有 player_loading 票"
    );
    assert_eq!(server.tickets().simulation_level(SPAWN_CHUNK), 21);

    server.leave(&client);
    server.tick();
    assert_eq!(
        server.tickets().loading_level(SPAWN_CHUNK),
        UNLOADED_LEVEL,
        "离开后票撤掉"
    );
}

/// 跨区块只改两圈：原来中心的等级降一层，新中心升一层。
#[test]
fn crossing_a_chunk_boundary_moves_the_player_ticket() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    assert_eq!(server.tickets().loading_level(SPAWN_CHUNK), 31);

    // 往东走到 (1, 0)。测试 harness 对长距离直接放置玩家，这里跨一格用移动包。
    // 视距 2：玩家票方形半径 2。(3, 0) 在旧视野之外（靠传播），
    // 到新中心 (1, 0) 后落进方形里。
    let outside = ChunkPos::new(3, 0);
    let before = server.tickets().loading_level(outside);
    server.move_to(&client, SPAWN_X + 20.0, SPAWN_Z);
    server.tick();
    assert_eq!(server.tickets().loading_level(ChunkPos::new(1, 0)), 31);
    assert!(
        server.tickets().loading_level(outside) < before,
        "新视野里这一格更强了"
    );
}

/// 全服视距变化改的是票的覆盖半径，不是客户端自己的 `T(vd)`。
#[test]
fn view_distance_change_resizes_the_ticket_square() {
    let mut server = Harness::new(2);
    let _client = server.join_and_settle("Kitten");
    // 视距 2 时 (3, 0) 在票的覆盖范围里（玩家票方形半径 2，再外传播到 44）。
    let far = ChunkPos::new(6, 0);
    let before = server.tickets().loading_level(far);
    server.apply_ticket_for_test(TicketOp::ViewDistance(8));
    assert!(
        server.tickets().loading_level(far) < before,
        "视距变大后远处的等级应该变小（更强）"
    );
}

/// 模拟距离变化改模拟等级，不改加载等级。
#[test]
fn simulation_distance_change_only_moves_simulation_levels() {
    let mut server = Harness::new(2);
    let _client = server.join_and_settle("Kitten");
    let pos = ChunkPos::new(2, 0);
    let loading = server.tickets().loading_level(pos);
    let simulation = server.tickets().simulation_level(pos);
    server.apply_ticket_for_test(TicketOp::SimulationDistance(3));
    assert_eq!(server.tickets().loading_level(pos), loading);
    assert_ne!(server.tickets().simulation_level(pos), simulation);
}

/// 带超时的票（用 `unknown` 的 1 刻）每刻递减并到期移除。
#[test]
fn timed_tickets_expire_on_tick() {
    let mut server = Harness::new(2);
    let _client = server.join_and_settle("Kitten");
    let pos = ChunkPos::new(5, 0);
    server.tickets_mut().add(TicketType::Unknown, pos, 40);
    assert!(server.tickets().loading_level(pos) <= 40);
    for _ in 0..4 {
        server.tick();
    }
    assert!(
        !server.tickets().ticket_positions().contains(&pos),
        "unknown 票 1 刻后到期"
    );
}

/// `/forceload` 类型的票走 `Forced` 入口，且写进可存盘列表。
#[test]
fn forced_tickets_are_persistent() {
    let mut server = Harness::new(2);
    let _client = server.join_and_settle("Kitten");
    let pos = ChunkPos::new(7, 0);
    server.apply_ticket_for_test(TicketOp::Forced { pos, forced: true });
    assert!(server.tickets().forced_chunks().contains(&pos));
    assert!(
        server
            .tickets()
            .persistent_tickets()
            .iter()
            .any(|ticket| ticket.pos == pos && ticket.ty == TicketType::Forced)
    );
    server.apply_ticket_for_test(TicketOp::Forced { pos, forced: false });
    assert!(!server.tickets().forced_chunks().contains(&pos));
}
