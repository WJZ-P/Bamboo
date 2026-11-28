//! 线程和生命周期复查的回归测试。断言按「BUG 不存在」写。
//!
//! Regression tests from the threading and lifecycle review. Assertions state
//! that the bugs are absent, so a regression fails the build.

use super::{island_split_tests::with_stack, tests::Harness};

/// 世界线程复查名单时拒绝的进服：断开原因发完以后，家岛的出站链路表里不留这名玩家的
/// 链路。以前这条关掉的链路一直留在表里（他不是岛上的玩家，`Leave` 到了也没人删），每被
/// 拒一次多一条，之后家岛每次广播都要扫过它们。
///
/// A join refused by the world thread's own list check leaves no link in the
/// home island's link table once the reason is sent. The closed link used to
/// stay forever (the player is on no island, so even the `Leave` removed
/// nothing): one more per refusal, walked by every later broadcast.
#[test]
fn a_refused_join_leaves_no_link_behind() {
    let mut server = Harness::new(2);
    server.world.master.authoritative_admin = true;
    server.world.master.global.admin.using_whitelist = true;
    let mut refused = Vec::new();
    for index in 0..3 {
        let client = server.join(&format!("stranger{index}"));
        server.tick();
        assert!(client.is_closed(), "不在白名单上，被拒了");
        refused.push(client);
    }
    let links = server.world.islands.region.links.players();
    for client in &refused {
        assert!(
            !links.contains(&client.id),
            "被拒的进服在家岛的链路表里留下了一条链路"
        );
    }
    for client in &refused {
        server.leave(client);
    }
    server.tick();
    assert!(
        server.world.islands.region.links.players().is_empty(),
        "离开以后链路表是空的"
    );
    assert!(server.world.master.refused_inboxes.is_empty());
}

/// 没有常驻实体工作池可用的线程（分离驱动下岛的 tick 线程：岛的一刻拿不到主控那份池）
/// 自己的工作池只建一次，之后每一刻复用。以前每一刻都新起、再收掉 `entity_threads - 1`
/// 条线程，起不来时这一刻直接 panic。
///
/// A thread with no shared entity worker pool at hand (an island tick thread
/// under the detached drive, whose tick cannot reach the master's pool)
/// builds its own pool once and reuses it every tick. It used to start and
/// join `entity_threads - 1` threads on every tick, panicking the tick when
/// a thread could not start.
#[test]
fn a_thread_without_the_shared_pool_builds_its_entity_workers_once() {
    with_stack(|| {
        let mut server = Harness::new(2);
        let _client = server.join_and_settle("Alice");
        let before = crate::interaction::local_entity_pools_built();
        for _ in 0..3 {
            crate::interaction::parallel_entity_ticks(3, server.region(), None);
        }
        assert_eq!(
            crate::interaction::local_entity_pools_built() - before,
            1,
            "三刻只建一次工作池"
        );
        // 换了线程数才重建。
        // A different thread count rebuilds it.
        crate::interaction::parallel_entity_ticks(2, server.region(), None);
        crate::interaction::parallel_entity_ticks(2, server.region(), None);
        assert_eq!(crate::interaction::local_entity_pools_built() - before, 2);
        // 线程结束之前收掉自己的工作池（岛的 tick 线程退出时这么做）；之后再用就重新建。
        // A thread tears its pool down before it ends (as an island tick
        // thread does on exit); using it again builds a new one.
        crate::interaction::release_local_entity_pool();
        crate::interaction::parallel_entity_ticks(2, server.region(), None);
        assert_eq!(crate::interaction::local_entity_pools_built() - before, 3);
        crate::interaction::release_local_entity_pool();
    });
}
