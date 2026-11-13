//! 差分测试：自动保存对正在运行的游戏不可见。
//!
//! 同一场景跑上千刻，一份每刻触发自动保存、一份从不保存，逐刻比较对游戏
//! 可见的东西（方块、各张表的内容、出站帧），必须完全相同。只读快照不改
//! 任何表的顺序、不换版本号、不重排 ticker 和计划刻、不让区块包缓存失效。
//!
//! Differential test: saving is invisible to the running game. One world
//! autosaves every tick, the other never saves; compare visible state and
//! outbound frames tick by tick. Internal monotonic counters are not compared.

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use super::tests::{Client, Harness};

const ORIGIN: ChunkPos = ChunkPos::new(0, 0);
const EDIT: BlockPos = BlockPos::new(2, -60, 2);
const TICKS: usize = 1000;

/// 逐刻状态指纹：方块、计划刻、实体数、玩家位置、容器内容和出站帧。
/// 不比较 `ChunkRevision` 之类内部计数器的绝对值——它们对游戏和客户端不可见。
fn fingerprint(region: &crate::region::Region, client: &Client) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&region.chunks.block(EDIT).unwrap_or_default().to_le_bytes());
    for tick in region.block_ticks.pending() {
        out.extend_from_slice(&tick.trigger_tick.to_le_bytes());
        out.extend_from_slice(&tick.pos.x.to_le_bytes());
        out.extend_from_slice(&tick.pos.y.to_le_bytes());
        out.extend_from_slice(&tick.pos.z.to_le_bytes());
        out.extend_from_slice(&(tick.block as u64).to_le_bytes());
    }
    out.extend_from_slice(&(region.entities.len() as u64).to_le_bytes());
    out.extend_from_slice(&(region.containers.len() as u64).to_le_bytes());
    out.extend_from_slice(&(region.block_items.len() as u64).to_le_bytes());
    out.extend_from_slice(&(region.command_blocks.len() as u64).to_le_bytes());
    out.extend_from_slice(&(region.lecterns.books.len() as u64).to_le_bytes());
    let position = region.players[&client.id].position();
    out.extend_from_slice(&position.x.to_le_bytes());
    out.extend_from_slice(&position.y.to_le_bytes());
    out.extend_from_slice(&position.z.to_le_bytes());
    // 出站帧逐字节比较：快照不能让区块包缓存失效、多发一次区块或多发一条变化。
    for frame in client.take_frames() {
        out.extend_from_slice(&(frame.id as u32).to_le_bytes());
        out.extend_from_slice(&(frame.body.len() as u64).to_le_bytes());
        out.extend_from_slice(&frame.body);
    }
    out
}

/// 两份世界都不接磁盘：没有异步读盘，启动完全对称，出站帧的时序才可比。
/// 每刻只调一次 `save_loaded_dirty`（`io` 为 `None` 时它是纯记账），等价于
/// 自动保存那一侧「每刻都拍快照」的世界线程路径。
#[test]
fn autosaving_every_tick_is_invisible_to_the_running_world() {
    let mut saved = Harness::new(2);
    let mut control = Harness::new(2);
    let saved_client = saved.join_and_settle("Alice");
    let control_client = control.join_and_settle("Alice");
    for server in [&mut saved, &mut control] {
        server.ensure_loaded(&[ORIGIN]);
        server.set_block(EDIT, blocks::STONE);
        server.region().chunks.finish_light_updates();
        let player = server.region().players.values_mut().next().unwrap();
        player.inventory.slots[36] = ItemStack::new(items::STONE, 3);
    }
    for tick in 0..TICKS {
        if tick % 100 == 0 {
            for server in [&mut saved, &mut control] {
                let state = if (tick / 100) % 2 == 0 {
                    blocks::STONE
                } else {
                    blocks::DIRT
                };
                server.set_block(EDIT, state);
                server.region().touch(EDIT);
                server.region().block_ticks.schedule(
                    EDIT,
                    blocks::STONE as usize,
                    40,
                    mc_bamboo_blocks::TickPriority::Normal,
                );
                server.region().touch(EDIT);
            }
        }
        // 每刻都走一次自动保存路径（没有 IO 线程时只做只读编码与记账）。
        saved
            .world
            .host
            .save_loaded_dirty(&mut saved.world.region, tick as u64 + 1, true);
        saved.tick();
        control.tick();
        let a = fingerprint(&saved.world.region, &saved_client);
        let b = fingerprint(&control.world.region, &control_client);
        assert_eq!(a, b, "第 {tick} 刻自动保存改变了世界状态或发出的包");
    }
}
