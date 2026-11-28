//! 线程和生命周期复查的回归测试。断言按「BUG 不存在」写。
//!
//! Regression tests from the threading and lifecycle review. Assertions state
//! that the bugs are absent, so a regression fails the build.

use std::{collections::BTreeSet, time::Duration};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_world::director::{HOME_REGION, Owner, TicketOp, owner_is_held};

use super::{
    island_hybrid_tests::{block_on_disk, bounded, island_of_group, scene},
    island_split_tests::with_stack,
    tests::Harness,
};
use crate::island_partition::{SectionPos, TopologyOp};

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

/// 关服时还有一条照做不了的拆分排着（玩家的视野跨着切口，拆分一直推迟）：卷进去的岛
/// 仍然要收回、存下全部区块。现在宿主扣着这些岛的信，关服的收回信没人收，它们上面
/// 上次匀速存盘以后的改动丢了。
///
/// 修法在 `director_host.rs` / `World::shut_down`（别的队员在改，这里只留复现）：关服时
/// 还排着的拆合要么强制照做（玩家马上都被踢，视野和配对不用再守），要么让关服的收回信
/// 按 Director 记的归属（区块实际所在的岛）投、并且不再扣信。
///
/// Shutdown with a split still queued that cannot be applied (a player's view
/// straddles the cut, so it keeps being postponed): the islands it names must
/// still be reclaimed and saved in full. Today the host keeps holding their
/// mail, nobody takes the shutdown reclaims, and their changes since the last
/// paced save are lost. The fix belongs in `director_host.rs` /
/// `World::shut_down` (being changed by others; this only reproduces it):
/// either force the queued changes at shutdown (every player is about to be
/// kicked, so views and pairings need no protection), or post the shutdown
/// reclaims by the Director's recorded owner and stop holding mail.
#[test]
#[ignore = "known bug, see the review report: shutdown skips islands held by a deferred split"]
fn shutdown_with_a_deferred_split_still_saves_every_chunk() {
    fn run(deferred_split: bool) -> Option<u32> {
        let root = std::env::temp_dir().join(format!(
            "bamboo-review-shutdown-split-{}-{deferred_split}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let spot = BlockPos::new(4, -60, 4);
        {
            let mut server = Harness::new_islands_with_save(2, &root, 0);
            let _client = server.join_and_settle("Alice");
            for _ in 0..3 {
                server.tick();
            }
            let world = &mut server.world;
            if deferred_split {
                // 玩家站在区块 (0, 0)、视距 2：只拆 section (0, 0) 的话他跟踪的区块跨着
                // 切口，拆分一直推迟。
                // The player stands on chunk (0, 0) with view distance 2:
                // splitting off section (0, 0) alone cuts through the chunks
                // they track, so the split keeps being postponed.
                world
                    .master
                    .host
                    .queue_topology_for_test(vec![TopologyOp::Split {
                        from: HOME_REGION,
                        to: 91,
                        sections: BTreeSet::from([SectionPos::of(ChunkPos::new(0, 0))]),
                    }]);
                world.settle_islands();
                assert!(world.master.host.has_topology(), "拆分推迟了，还排着");
            }
            let region = &mut world.islands.region;
            region
                .chunks
                .set_block(spot, mc_bamboo_data::blocks::GOLD_BLOCK);
            region.touch(spot);
            region.chunks.finish_light_updates();
            server.shut_down();
        }
        let block = block_on_disk(&root, spot);
        let _ = std::fs::remove_dir_all(&root);
        block
    }
    bounded(|| {
        let gold = Some(mc_bamboo_data::blocks::GOLD_BLOCK);
        assert_eq!(run(false), gold, "对照：没有排着的拆分时关服存下了改动");
        assert_eq!(run(true), gold, "有推迟的拆分排着时关服也要存下改动");
    });
}

/// 岛卡死期间投给它、它没取走的盒子，又因为票撤了被要求收回（交付信和收回信都还在主控
/// 的邮筒里）：判死以后这个区块不能变成没人管的孤儿。现在判死把它当成「在死岛手里」，
/// 删掉 Director 的记录和收回信，却留下交付信；同编号的新岛建出来以后把这个盒子收进
/// `Region`，Director 里没有它的记录，从此不收回、不存盘，一直按当时的档位 tick。
///
/// 修法在 `world/island_death.rs` 的 `chunks_of_dead` 和 `director_host.rs` 的
/// `lose_chunks` / `posted_reclaims`（别的队员在改，这里只留复现）：邮筒里交付信还没取走
/// 的位置不算死岛拿着的（盒子在主控手里），交付信和收回信都留着，同编号的新岛收信时
/// 把盒子原样退回（`return_pending_deliveries`），和平常「盒子还没交付就要收回」一样。
///
/// A box posted to an island while it was stuck and never taken, then asked
/// back because its ticket went away (both the delivery and the reclaim are
/// still in the master's postbox): after the death this chunk must not become
/// an orphan. Today the verdict treats it as held by the dead island, removes
/// the Director's record and the reclaim, but keeps the delivery; the new
/// island with the same id then takes the box into its `Region` with no
/// record in the Director, so it is never reclaimed or saved and keeps
/// ticking at its old level. The fix belongs in `chunks_of_dead`
/// (`world/island_death.rs`) and `lose_chunks` / `posted_reclaims`
/// (`director_host.rs`), which others are changing; this only reproduces it:
/// a position whose delivery is still posted is not in the dead island's
/// hands, so both letters should stay and the new island hands the box back
/// (`return_pending_deliveries`) like any box reclaimed before delivery.
#[test]
#[ignore = "known bug, see the review report: a posted box becomes an orphan chunk after an island death"]
fn a_box_still_posted_to_a_dead_island_does_not_become_an_orphan_chunk() {
    bounded(|| {
        let mut scene = scene(331, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        let victim = island_of_group(&mut scene, 1);
        assert_ne!(victim, HOME_REGION, "第 1 组不在家岛上");
        let center = scene.centers[1];
        scene.server.world.master.host.set_reclaim_delay(2);
        scene.server.world.stall_island(victim, 1, u64::MAX);
        scene.server.tick();
        assert!(!scene.server.world.islands.is_parked(victim), "卡住了");

        // 它卡着的时候，它的 section 旁边多了一张强加载票：盒子投进邮筒，没人取。
        // While it is stuck a forced ticket appears next to its sections: the
        // box is posted and nobody takes it.
        // 位置要同时满足两条：离所有的票够远（票撤掉以后等级掉出加载带，逗留到期就收回），
        // 它的 5 × 5 加载范围又挨着这个岛的 section（分区器把它归给这个岛）。
        // The position must be far enough from every ticket (once its own
        // ticket goes, its level leaves the loading band and it is reclaimed
        // after the delay), while its 5 x 5 loaded area still touches this
        // island's sections (so the partitioner gives it to this island).
        let extra = {
            let world = &scene.server.world;
            let partition = world.master.host.islands().expect("多岛开着");
            let size = crate::island_partition::SECTION_SIZE;
            let joins_victim = |at: ChunkPos| {
                (-2..=2).any(|dx| {
                    (-2..=2).any(|dz| {
                        let section = SectionPos::of(ChunkPos::new(at.x + dx, at.z + dz));
                        (-2..=2).any(|sx| {
                            (-2..=2).any(|sz| {
                                let near =
                                    ChunkPos::new((section.x + sx) * size, (section.z + sz) * size);
                                partition.island_of_chunk(near) == Some(victim)
                            })
                        })
                    })
                })
            };
            (10..=24)
                .flat_map(|distance| {
                    [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dz)| {
                        ChunkPos::new(center.x + dx * distance, center.z + dz * distance)
                    })
                })
                .find(|&at| {
                    world.tickets().loading_level(at) > mc_bamboo_world::ticket::MAX_LEVEL
                        && world
                            .master
                            .host
                            .director()
                            .owner_at(at)
                            .is_none_or(|owner| !owner_is_held(owner))
                        && joins_victim(at)
                })
                .expect("有一个离票够远、又挨着这个岛的位置")
        };
        scene.server.apply_ticket_for_test(TicketOp::Forced {
            pos: extra,
            forced: true,
        });
        for _ in 0..60 {
            if scene.server.world.master.host.has_posted_delivery(extra) {
                break;
            }
            scene.server.tick();
        }
        let host = &mut scene.server.world.master.host;
        assert!(host.has_posted_delivery(extra), "盒子投进了邮筒");
        assert_eq!(host.island_of(extra), Some(victim), "投给了卡住的岛");

        // 票撤了：收回信也投进同一个邮筒，交付信还在。
        // The ticket goes away: the reclaim is posted too, the delivery still is.
        scene.server.apply_ticket_for_test(TicketOp::Forced {
            pos: extra,
            forced: false,
        });
        for _ in 0..60 {
            let host = &scene.server.world.master.host;
            if host.posted_reclaims(victim).contains(&extra) {
                break;
            }
            scene.server.tick();
        }
        let host = &scene.server.world.master.host;
        assert!(host.posted_reclaims(victim).contains(&extra), "收回信投了");
        assert!(host.has_posted_delivery(extra), "交付信还没被取走");

        // 判死。
        // The verdict.
        let by = scene.server.world.master.island_dead_timeout + Duration::from_secs(1);
        assert!(scene.server.world.age_island(victim, by), "它在线程上跑着");
        for _ in 0..6 {
            scene.server.tick();
        }
        let world = &scene.server.world;
        assert_eq!(world.master.hybrid.death.deaths.len(), 1, "判死了");
        // 这个盒子要么没进任何岛，要么进了岛并且 Director 记着它在那个岛上。
        // The box is either on no island, or on one the Director records it on.
        let holder = world.islands.island_of_chunk(extra);
        let owner = world.master.host.director().owner_at(extra);
        assert!(
            holder.is_none() || matches!(owner, Some(Owner::Region(id)) if Some(id) == holder),
            "区块 {extra:?} 加载在岛 {holder:?} 上，Director 记的归属却是 {owner:?}"
        );
        scene.server.world.release_stalls_for_test();
    });
}
