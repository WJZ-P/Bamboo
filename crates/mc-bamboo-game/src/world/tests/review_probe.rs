//! 第三轮独立复查的回归测试：失败重试、只读快照无副作用、save-off 语义、
//! 关服补写、背压只挡收回。断言按「BUG 不存在」写。
//!
//! Regression tests from the third-round independent review. Assertions state
//! that the bugs are absent, so a regression fails the build.

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionFile},
    world_dir::WorldDir,
};
use mc_bamboo_world::{column::ChunkColumn, store::ChunkStore};

use crate::{
    chunk_codec::{ChunkCodec, InlineCodec, PackedChunk},
    save_session::SaveCommand,
    world::tests::Harness,
};

struct TempWorld(std::path::PathBuf);

impl TempWorld {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bamboo-review-{tag}-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时世界目录");
        Self(path)
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn disk_has(dir: &WorldDir, pos: ChunkPos) -> bool {
    let region = RegionCompression::Zlib;
    let _ = region;
    RegionFile::open(
        dir.region_dir()
            .join(format!("r.{}.{}.mca", pos.x >> 5, pos.z >> 5)),
        dir.region_dir(),
        RegionCompression::Zlib,
    )
    .map(|f| f.has_chunk(pos))
    .unwrap_or(false)
}

fn break_disk(dir: &WorldDir) {
    for directory in [dir.region_dir(), dir.entities_dir()] {
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::write(&directory, b"not a directory").unwrap();
    }
}

fn fix_disk(dir: &WorldDir) {
    for directory in [dir.region_dir(), dir.entities_dir()] {
        let _ = std::fs::remove_file(&directory);
        std::fs::create_dir_all(&directory).unwrap();
    }
}

/// 只读快照：连续两次 NBT 一致，版本号和段状态不变，光照算完后与卸载存盘逐字节一致。
#[test]
fn repeated_read_only_snapshots_are_stable_and_side_effect_free() {
    let mut server = Harness::new_loaded(2);
    let pos = ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    let edit = BlockPos::new(3, -60, 3);
    assert!(server.region().chunks.set_block(edit, blocks::GLOWSTONE));
    let region = server.region();
    let revision = region.chunks.revision(pos);
    let states = region.chunks.light().section_states(pos);

    let first: PackedChunk = region.encode_loaded_column(pos).unwrap().unwrap();
    let second: PackedChunk = region.encode_loaded_column(pos).unwrap().unwrap();
    assert_eq!(first, second);
    assert_eq!(region.chunks.revision(pos), revision);
    assert_eq!(region.chunks.light().section_states(pos), states);

    region.chunks.finish_light_updates();
    let after_light: PackedChunk = region.encode_loaded_column(pos).unwrap().unwrap();
    let boxes = region.take_columns(&[pos].into_iter().collect()).unwrap();
    let unload = InlineCodec.encode(pos, &boxes[&pos]).unwrap();
    println!(
        "SNAPSHOT_BYTES first_eq_unload={} settled_eq_unload={}",
        first == unload,
        after_light == unload
    );
    assert_eq!(after_light, unload);
}

/// 只读快照不重排计划刻、实体和方块实体的运行顺序。
#[test]
fn read_only_snapshot_keeps_tick_and_block_entity_order() {
    use mc_bamboo_blocks::TickPriority;

    let mut server = Harness::new_loaded(2);
    let pos = ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    let region = server.region();
    for (delay, x) in [(3_i32, 2), (1, 3), (2, 4)] {
        region.block_ticks.schedule(
            BlockPos::new(x, -60, x),
            blocks::STONE as usize,
            delay,
            TickPriority::Normal,
        );
    }
    let before_ticks: Vec<_> = region
        .block_ticks
        .pending()
        .map(|t| (t.pos, t.trigger_tick, t.priority))
        .collect();
    let before_entities: Vec<_> = region.entities.iter().map(|e| e.block_position()).collect();
    let mut before_be = Vec::new();
    region.block_entities.visit(|at, _| before_be.push(at));
    let _ = region.encode_loaded_column(pos).expect("快照可用");
    let after_ticks: Vec<_> = region
        .block_ticks
        .pending()
        .map(|t| (t.pos, t.trigger_tick, t.priority))
        .collect();
    let after_entities: Vec<_> = region.entities.iter().map(|e| e.block_position()).collect();
    assert_eq!(before_ticks, after_ticks);
    assert_eq!(before_entities, after_entities);
    let mut after_be = Vec::new();
    region.block_entities.visit(|at, _| after_be.push(at));
    assert_eq!(before_be, after_be);
}

/// 写盘失败后磁盘恢复，只靠运行中的自动保存（不发任何命令）也要把改动补写下去。
#[test]
fn a_failed_write_is_retried_by_autosave_after_the_disk_recovers() {
    let temp = TempWorld::new("revision");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &temp.0);
    let pos = ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    server.world.region.chunks.finish_light_updates();

    break_disk(&dir);
    server
        .world
        .global
        .save_requests
        .push(SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    for _ in 0..5 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    assert!(server.world.host.io_failures() > 0, "写盘确实失败");
    fix_disk(&dir);
    for _ in 0..400 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    assert!(disk_has(&dir, pos), "磁盘恢复后自动保存必须把失败的写补上");
}

/// 反复 `/save-all` 解除退避后，失败的已加载脏区块要重新拍快照写盘。
#[test]
fn repeated_save_all_after_a_failed_write_recovers_the_chunk() {
    let temp = TempWorld::new("saveall-repeat");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &temp.0);
    let pos = ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    server.world.region.chunks.finish_light_updates();

    break_disk(&dir);
    server
        .world
        .global
        .save_requests
        .push(SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    for _ in 0..5 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    assert!(server.world.host.io_failures() > 0);
    fix_disk(&dir);
    for _ in 0..20 {
        server
            .world
            .global
            .save_requests
            .push(SaveCommand::SaveAll {
                flush: false,
                source: crate::inbound::PlayerId(1),
            });
        server.tick();
        server.world.host.flush_io();
        server.tick();
    }
    assert!(disk_has(&dir, pos), "反复 save-all 必须能重写失败的区块");
}

/// 快照已经交给 IO、队列里没有残留时，失败后重拍也不该依赖旧的 saved_revisions。
#[test]
fn a_failed_in_flight_write_with_no_queue_residue_is_retried() {
    let temp = TempWorld::new("no-residue");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &temp.0);
    let pos = ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    server.world.region.chunks.finish_light_updates();

    break_disk(&dir);
    server
        .world
        .global
        .save_requests
        .push(SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    for _ in 0..5 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    assert!(server.world.host.io_failures() > 0);
    #[cfg(test)]
    server.world.host.save_queue.clear();
    fix_disk(&dir);
    for _ in 0..30 {
        server
            .world
            .global
            .save_requests
            .push(SaveCommand::SaveAll {
                flush: false,
                source: crate::inbound::PlayerId(1),
            });
        server.tick();
        #[cfg(test)]
        server.world.host.save_queue.clear();
        server.world.host.flush_io();
        server.tick();
        #[cfg(test)]
        server.world.host.save_queue.clear();
    }
    assert!(disk_has(&dir, pos), "清掉队列残留之后也必须能重写");
}

/// 关服时 parking 里写失败的盒子要重新编码提交，磁盘到关服还在失败时照常退出。
#[test]
fn shutdown_retries_a_failed_write_parked_before_shutdown() {
    let temp = TempWorld::new("shutdown");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &temp.0);
    let pos = ChunkPos::new(40, 40);
    server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced { pos, forced: true });
    for _ in 0..64 {
        if server.world.region.chunks.is_loaded(pos) {
            break;
        }
        server.tick();
    }
    assert!(server.world.region.chunks.is_loaded(pos));
    server.world.region.chunks.set_block(
        BlockPos::new(pos.x * 16 + 3, -60, pos.z * 16 + 3),
        blocks::GLOWSTONE,
    );
    server
        .apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced { pos, forced: false });
    break_disk(&dir);
    for _ in 0..400 {
        server.tick();
        if !server.world.region.chunks.is_loaded(pos) && server.world.host.parked_count() > 0 {
            break;
        }
    }
    server.world.host.flush_io();
    server.tick();
    fix_disk(&dir);
    #[cfg(test)]
    server.world.host.save_queue.clear();
    assert!(server.world.shut_down().is_break());
    println!("AFTER_SHUTDOWN disk_has={}", disk_has(&dir, pos));
    assert!(disk_has(&dir, pos), "关服必须重写 parking 里没写成功的盒子");
}

/// save-off 期间区块照官方 `noSave` 不卸载；save-on 之后匀速保存照常写盘。
#[test]
fn save_off_keeps_chunks_loaded_like_official_no_save() {
    let temp = TempWorld::new("saveoff");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &temp.0);
    let pos = ChunkPos::new(40, 40);
    server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced { pos, forced: true });
    for _ in 0..64 {
        if server.world.region.chunks.is_loaded(pos) {
            break;
        }
        server.tick();
    }
    assert!(server.world.region.chunks.is_loaded(pos));
    server.world.region.chunks.set_block(
        BlockPos::new(pos.x * 16 + 3, -60, pos.z * 16 + 3),
        blocks::GLOWSTONE,
    );
    server.world.global.save_requests.push(SaveCommand::SaveOff);
    server.tick();
    server
        .apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced { pos, forced: false });
    for _ in 0..400 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    println!(
        "SAVE_OFF_UNLOAD loaded={} disk_has={}",
        server.world.region.chunks.is_loaded(pos),
        disk_has(&dir, pos)
    );
    assert!(
        server.world.region.chunks.is_loaded(pos),
        "save-off 照官方不收回区块"
    );
    server.world.global.save_requests.push(SaveCommand::SaveOn);
    for _ in 0..200 {
        server.tick();
    }
    server.world.host.flush_io();
    server.tick();
    assert!(server.world.host.io().unwrap().has_chunk(pos));
}

/// 每刻判脏的开销不能和已加载区块数成正比：2 万区块、2 千条火把记录，
/// 真实的 `dirty_loaded_chunks` 只碰改动集合，20 次的均摊耗时远小于 50 ms 预算。
#[test]
fn dirty_detection_cost_does_not_scale_with_loaded_chunks() {
    let mut store = ChunkStore::loaded();
    let template = ChunkColumn::from_template();
    for i in 0..20_000 {
        store.load(ChunkPos::new(i, 0), Box::new(template.clone()));
    }
    let mut region = crate::region::Region::new(2, 20, false);
    region.chunks = store;
    for i in 0..2_000 {
        region
            .redstone
            .record_torch_toggle(BlockPos::new((i % 20_000) * 16, -60, 0), i as i64);
    }
    let mut host = crate::director_host::DirectorHost::new(
        mc_bamboo_world::director::DirectorConfig::default(),
        crate::director_host::MailBudget::default(),
    );
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let dirty = host.dirty_loaded_chunks(&mut region, false);
        std::hint::black_box(dirty);
    }
    let per_tick_ms = start.elapsed().as_millis() / 20;
    println!("DIRTY_MS_PER_TICK large(20000,2000)={per_tick_ms}");
    assert!(
        per_tick_ms < 20,
        "每刻判脏 {per_tick_ms} ms，和已加载区块数成正比"
    );
}

/// 带存档重启：近处盘上有区块、远处盘上没有，两种来源都走异步读流水线，
/// 回执在同一刻开头收回，按距离排序交付。第一包必须是出生点，第一批 9 个
/// 必须正好是脚下 3×3（官方 `PlayerChunkSender` 的 `distanceSquared` 顺序）。
///
/// A world with a save: the near chunk is on disk, a far one is not. Both go
/// through the async read pipeline, replies arrive in the same tick, and delivery
/// is distance-ordered: the first packet is the spawn chunk and the first batch
/// is exactly the 3x3 around it.
#[test]
fn a_saved_world_delivers_the_nearest_chunk_first() {
    use mc_bamboo_data::packets::play::clientbound;
    use mc_bamboo_storage::chunk_io::Reply;
    use mc_bamboo_storage::chunk_nbt::{ChunkEncodeExtra, encode_bytes};
    use mc_bamboo_world::column::ChunkColumn;

    let temp = TempWorld::new("saved-order");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();

    // 先在一个临时世界目录里写出「近处有存档」：把 (0,0) 的官方格式 NBT 直接
    // 放进 region/r.0.0.mca，`ChunkIo::open` 的启动扫描会认出它。
    let near = ChunkPos::new(0, 0);
    let bytes = encode_bytes(
        &ChunkColumn::from_template(),
        &ChunkEncodeExtra {
            chunk_pos: near,
            ..ChunkEncodeExtra::default()
        },
    )
    .unwrap();
    let mut region_file = RegionFile::open(
        dir.region_dir().join("r.0.0.mca"),
        dir.region_dir(),
        RegionCompression::Zlib,
    )
    .unwrap();
    region_file.write(near, Some(&bytes), 1).unwrap();
    region_file.sync().unwrap();
    drop(region_file);

    let mut server = Harness::new_loaded_with_save(8, &temp.0);
    assert!(
        server.world.host.io().unwrap().has_chunk(near),
        "启动扫描必须认出盘上已有的区块"
    );
    let client = server.join_with("Kitten", crate::world::tests::client_requesting(8));
    server.confirm_teleport(&client);
    server.load_player(&client);
    // 第一刻只派读任务，不应该发出任何区块包：模板不能再抢先。
    server.tick();
    let first_tick = client.take_frames();
    assert!(
        crate::world::tests::received_chunks(&first_tick).is_empty(),
        "两种来源都还没回执时不发区块，远处的模板不能抢先"
    );
    // 第二刻开头收回所有读结果，一次按距离排序交付。
    server.tick();
    let frames = client.take_frames();
    let chunks = crate::world::tests::received_chunks(&frames);
    assert_eq!(chunks.first(), Some(&near), "第一包必须是出生点区块");
    let first_batch: std::collections::BTreeSet<ChunkPos> = chunks.into_iter().collect();
    let expected: std::collections::BTreeSet<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    assert_eq!(first_batch, expected, "第一批正好是脚下 3×3");
    assert_eq!(
        crate::world::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED).len(),
        1,
        "一批一个 ChunkBatchFinished"
    );
    // IO 信箱里不该有没收回的读回执。
    assert!(
        !matches!(
            server.world.host.io().unwrap().try_recv(),
            Some(Reply::Read { .. })
        ),
        "读结果已经全部收回"
    );
}
