//! 岛的两种驱动（岛的实施计划第 6 节第 6 步）：齐步在原来的时点当场收信，
//! 分离在岛下一刻开头收信；存盘是「主控出名单、岛编码交回」。
//!
//! The island's two drives (island plan, section 6, step 6): lockstep takes
//! the mail on the spot, detached at the start of the island's next tick;
//! saving is "the master picks the list, the island encodes it".

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;

use super::tests::{D6TempDir, Harness, received_chunks, written_chunks};
use crate::island::{IslandDrive, IslandState};

const SPAWN: ChunkPos = ChunkPos::new(0, 0);

/// 进服那一刻派出的交付：齐步当刻进岛，分离留在主控的邮筒里，岛下一刻开头收下；
/// 两种驱动在刻与刻之间岛都是停放的。
///
/// The join's deliveries: lockstep puts them in the island that tick; detached
/// keeps them in the master's postbox until the island's next tick. Between
/// ticks the island is parked under both drives.
#[test]
fn detached_mail_waits_for_the_islands_next_tick() {
    for drive in [IslandDrive::Lockstep, IslandDrive::Detached] {
        let mut server = Harness::new_loaded_with_drive(2, drive);
        let client = server.join("Kitten");
        server.tick();
        assert_eq!(server.world.island.state, IslandState::Parked);
        let first = received_chunks(&client.take_frames());
        match drive {
            IslandDrive::Lockstep => {
                assert!(server.world_region().chunks.is_loaded(SPAWN));
                assert_eq!(server.world.master.host.posted_len(), 0);
                assert_eq!(first.first(), Some(&SPAWN), "齐步：进服这一刻就发");
            }
            IslandDrive::Detached => {
                assert!(!server.world_region().chunks.is_loaded(SPAWN));
                assert!(server.world.master.host.posted_len() > 0, "信在邮筒里");
                assert!(first.is_empty(), "分离：进服这一刻还没有区块可发");
                assert!(
                    !server
                        .world
                        .master
                        .host
                        .director()
                        .debug_state()
                        .records()
                        .get(&SPAWN)
                        .is_some_and(|record| matches!(
                            record.owner,
                            mc_bamboo_world::director::Owner::Region(_)
                        )),
                    "岛还没收下，Director 不会记成已交付"
                );
                server.tick();
                assert!(server.world_region().chunks.is_loaded(SPAWN));
                let second = received_chunks(&client.take_frames());
                assert_eq!(second.first(), Some(&SPAWN), "分离：晚一刻、顺序不变");
            }
        }
        // 交回之后 Director 记下交付，归岛 0 管。
        assert!(
            server
                .world
                .master
                .host
                .director()
                .debug_state()
                .records()
                .get(&SPAWN)
                .is_some_and(|record| record.owner
                    == mc_bamboo_world::director::Owner::Region(
                        mc_bamboo_world::director::HOME_REGION
                    )),
            "{drive:?}：交回之后 Director 记成归岛 0"
        );
    }
}

/// 分离驱动下存盘照「主控出名单、岛编码交回」走：改过的区块在下一刻被判脏、
/// 再下一刻由岛编码交回写盘；区块始终留在岛上。
///
/// Detached saving follows "master lists, island encodes": an edited chunk is
/// judged dirty on the next tick and encoded by the island the tick after.
#[test]
fn detached_saves_encode_the_masters_list_one_tick_later() {
    let tmp = D6TempDir::new("detached-list");
    let mut server =
        Harness::new_loaded_with_save_threads_and_drive(2, &tmp.0, 0, IslandDrive::Detached);
    server.ensure_loaded(&[SPAWN]);
    // 再跑几刻，让开局的读和存盘都安定下来。
    for _ in 0..3 {
        server.tick();
    }
    server.world.master.host.flush_io();
    let _ = written_chunks(&server.world.master.host);
    let edit = BlockPos::new(3, -60, 3);
    server.set_block(edit, blocks::GLOWSTONE);
    server.world.island.region.touch(edit);
    server.world.island.region.chunks.finish_light_updates();

    server.tick();
    server.world.master.host.flush_io();
    assert!(
        !written_chunks(&server.world.master.host).contains(&SPAWN),
        "这一刻岛只交回改动，主控判脏、出名单"
    );
    assert!(
        server.world.island.save_plan.encode.contains(&SPAWN),
        "名单随交回给了岛"
    );
    server.tick();
    server.world.master.host.flush_io();
    assert!(
        written_chunks(&server.world.master.host).contains(&SPAWN),
        "下一刻末尾岛照名单编码交回，主控写盘"
    );
    assert!(server.world_region().chunks.is_loaded(SPAWN));
}

/// 分离驱动下 `/save-all` 等岛交回再过栅栏，名单里还没编码的区块也一并写下。
///
/// Under the detached drive `/save-all` waits for the hand-back and also writes
/// the chunks on a list not yet encoded.
#[test]
fn detached_save_all_folds_in_the_pending_list() {
    let tmp = D6TempDir::new("detached-save-all");
    let mut server =
        Harness::new_loaded_with_save_threads_and_drive(2, &tmp.0, 0, IslandDrive::Detached);
    server.ensure_loaded(&[SPAWN]);
    for _ in 0..3 {
        server.tick();
    }
    let edit = BlockPos::new(5, -60, 5);
    server.set_block(edit, blocks::GLOWSTONE);
    server.world.island.region.touch(edit);
    server.world.island.region.chunks.finish_light_updates();
    // 这一刻判脏、出名单，还没编码。
    server.tick();
    assert!(server.world.island.save_plan.encode.contains(&SPAWN));
    server.world.master.host.flush_io();
    let _ = written_chunks(&server.world.master.host);

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    server.tick();
    server.world.master.host.flush_io();
    assert!(
        written_chunks(&server.world.master.host).contains(&SPAWN),
        "save-all 要把名单里还没编码的区块也写下"
    );
    assert!(
        server.world.island.save_plan.encode.is_empty(),
        "名单已经并进 save-all"
    );
}
