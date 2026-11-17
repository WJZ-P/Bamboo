//! 第 8 步：分区器接在 Director 的 `pump` 上之后，正式 `World::tick` 的
//! 增量输入和分区器内部状态逐刻一致。不碰入站消息、Global 或生成器。

use std::collections::BTreeSet;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{
    director::{TicketOp, owner_is_held},
    ticket::TicketType,
};

use crate::{island_partition::DESIRED_LEVEL, world::tests::Harness};

/// 测试专用：把分区器打开，并跑几刻让状态追上。
fn with_islands(view_distance: i32, verify: bool) -> Harness {
    let mut server = Harness::new_loaded(view_distance);
    server.world.master.host.enable_islands(verify);
    server
}

/// 当前 Director 里「票想加载」的区块集合。
fn desired(server: &Harness) -> BTreeSet<ChunkPos> {
    server
        .world
        .master
        .host
        .director()
        .tickets()
        .loading_chunks_at_most(DESIRED_LEVEL)
        .into_iter()
        .collect()
}

fn held(server: &Harness) -> BTreeSet<ChunkPos> {
    server
        .world
        .master
        .host
        .director()
        .owner_records()
        .into_iter()
        .filter(|record| owner_is_held(record.owner))
        .map(|record| record.pos)
        .collect()
}

/// 分区器开着时每刻都拿到和 Director 一样的依据集合、逐刻不变量成立，
/// 且增量划分等于全量重算。开关关着时运行时完全不建表。
#[test]
fn the_pump_feeds_the_partitioner_every_tick_without_changing_behaviour() {
    let mut server = with_islands(2, true);
    let client = server.join_and_settle("Kitten");
    // 随机挂 / 撤强加载票，同时让玩家来回走。
    let mut at = 0;
    for tick in 1..=60_u64 {
        if tick % 3 == 0 {
            let pos = ChunkPos::new((tick as i32 % 13) * 7 - 40, (tick as i32 % 7) * 9 - 30);
            server.apply_ticket_for_test(TicketOp::Forced { pos, forced: true });
        }
        if tick % 7 == 0 {
            let pos = ChunkPos::new(((tick / 7) as i32 % 9) * 5 - 20, 0);
            server.apply_ticket_for_test(TicketOp::Add {
                ty: TicketType::Forced,
                pos,
                level: 31,
            });
        }
        if tick % 5 == 0 {
            at = (at + 1) % 4;
            server.move_to(&client, 8.5 + 16.0 * f64::from(at), 8.5);
        }
        server.tick();
        let partition = server.world.master.host.islands().expect("分区器已打开");
        assert!(partition.invariants_hold(), "tick={tick}");
        assert_eq!(
            partition.snapshot(),
            partition.full_recompute(),
            "tick={tick}"
        );
        assert!(
            partition.chunk_invariants_hold(&desired(&server)),
            "票想加载的区块不变量：tick={tick}"
        );
        // 并集：还没收回的区块也在 section 计数里。
        let union: BTreeSet<ChunkPos> = desired(&server).union(&held(&server)).copied().collect();
        assert!(union.iter().all(|&pos| partition.island_of(pos).is_some()));
    }
    // 开关关着的世界不建表。
    let plain = Harness::new_loaded(2);
    assert!(plain.world.master.host.islands().is_none());
}

/// `=2` 模式的对照：pump 收敛后按 Director 的等级表和 owner 表全量对账，
/// 分区器每刻断言增量 == 全量，且票想加载的区块都在正确的岛里。
#[test]
fn the_full_recompute_reference_agrees_with_the_live_pump() {
    let mut server = with_islands(2, true);
    let client = server.join_and_settle("Kitten");
    for tick in 1..=30_u64 {
        if tick % 4 == 0 {
            let pos = ChunkPos::new(20 + tick as i32, -12);
            server.apply_ticket_for_test(TicketOp::Forced { pos, forced: true });
        }
        if tick % 6 == 0 {
            server.move_to(&client, 8.5 + 16.0 * (tick % 3) as f64, 8.5);
        }
        server.tick();
        let partition = server.world.master.host.islands().expect("分区器已打开");
        assert!(partition.invariants_hold(), "verify tick={tick}");
        assert!(
            partition.chunk_invariants_hold(&desired(&server)),
            "tick={tick}"
        );
        assert_eq!(
            partition.snapshot(),
            partition.full_recompute(),
            "tick={tick}"
        );
    }
}
