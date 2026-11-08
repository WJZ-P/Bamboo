//! 光照的网络一侧：在两个 tick 之间算光照，把变了的段发给需要的玩家。
//!
//! 官方的 `ChunkHolder.broadcastChanges` 先发这个区块的 `light_update`，再发它的方块变化。
//! `light_update` 只发给这个区块在跟踪范围边上的玩家：范围里面的区块，客户端收到方块变化后自己重算光照；
//! 边上的区块缺邻居，客户端算不准，才要服务端的结果。

use std::{collections::BTreeMap, mem};

use mc_bamboo_core::{ChunkPos, LightLayer};
use mc_bamboo_protocol::packet::play::LightUpdate;
use mc_bamboo_world::{light::LightChanges, store::ChunkStore};

use crate::{
    chunk_encoding::{LightSections, with_light_data},
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
};

/// 每 tick 给光照的工作量：任务数加上传播的步数。官方在光照线程上算，主线程从不等它；
/// 这里在世界线程上算，做不完的留到下一 tick：一次改很多方块时光照晚几 tick 出来，tick 不会被拖住。
/// release 构建每毫秒大约做 8000 步，8 万步约 10 ms。按步数而不是按时间算，同样的输入每次结果都一样。
const LIGHT_BUDGET: usize = 80_000;

/// tick 第 1 阶段之后：算排着的光照，记下变了的段等广播阶段发出去。
///
/// 官方在两个 tick 之间处理完玩家的包以后才排光照，所以这一 tick 处理过的请求在这里排成一轮；
/// 计划刻改的方块要等到下一 tick 开头才排，和官方一样晚一 tick 发光照。
///
/// 做到 [`LIGHT_BUDGET`] 就停，一轮可以停在半路。一轮算完才换上新的光照，
/// 方块规则读到的总是上一轮算完的结果。一轮读的是它排好那一刻的方块，那些方块最晚在那一 tick 广播出去，
/// 所以发给客户端的光照不会比它手上的方块新。
pub(crate) fn run_light_updates(region: &mut Region) {
    region.chunks.run_light_updates(LIGHT_BUDGET);
    merge_changes(
        &mut region.light_changes,
        region.chunks.take_light_changes(),
    );
}

fn merge_changes(
    pending: &mut BTreeMap<ChunkPos, LightChanges>,
    changes: BTreeMap<ChunkPos, LightChanges>,
) {
    for (pos, change) in changes {
        let entry = pending.entry(pos).or_default();
        entry.sky |= change.sky;
        entry.block |= change.block;
    }
}

/// tick 第 5 阶段，紧挨在方块变化前面：每个光照变了的区块发一个 `light_update`。
///
/// 官方按区块逐个发，先光照后方块；这里先发完所有区块的光照再发方块。官方遍历区块的顺序是哈希顺序，
/// 不同区块之间本来就没有约定的先后，同一个区块里光照仍然在方块前面，而且在同一 tick。
pub(crate) fn broadcast_light_changes(region: &mut Region) {
    for (pos, changes) in mem::take(&mut region.light_changes) {
        let receivers: Vec<PlayerId> = region
            .players
            .values()
            .filter(|player| player.chunks.is_chunk_on_tracked_border(pos))
            .map(|player| player.id)
            .collect();
        if receivers.is_empty() {
            continue;
        }
        let frames = encode_light_update(&region.chunks, pos, changes);
        for id in receivers {
            region.links.send_frames(id, &frames);
        }
    }
}

/// `ClientboundLightUpdatePacket`：只带变了的段，读的是现在的光照。
fn encode_light_update(store: &ChunkStore, pos: ChunkPos, changes: LightChanges) -> Vec<u8> {
    let light = store.light();
    let sky = LightSections::collect(light, LightLayer::Sky, pos, changes.sky);
    let block = LightSections::collect(light, LightLayer::Block, pos, changes.block);
    with_light_data(&sky, &block, |light| {
        encode_frames(&LightUpdate {
            x: pos.x,
            z: pos.z,
            light: *light,
        })
    })
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, packets::play::clientbound};
    use mc_bamboo_protocol::codec::Reader;

    use super::*;
    use crate::world::tests::{Client, Frame, Harness};

    /// 视距 2 时出生点的玩家跟踪到 x = 3 为止，(3, 0) 在边上，(0, 0) 在里面。
    const BORDER: ChunkPos = ChunkPos::new(3, 0);

    fn settled() -> (Harness, Client) {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        (server, client)
    }

    /// 像方块逻辑那样改一格：写进区块，记下要广播的位置。
    fn change_block(server: &mut Harness, pos: BlockPos, state: u32) {
        let region = server.region();
        region.chunks.set_block(pos, state);
        region.block_changes.changed.push(pos);
    }

    fn light_update_chunk(frame: &Frame) -> ChunkPos {
        let mut reader = Reader::new(&frame.body);
        ChunkPos::new(reader.varint().unwrap(), reader.varint().unwrap())
    }

    #[test]
    fn border_chunks_get_their_light_before_their_blocks() {
        let (mut server, client) = settled();
        change_block(&mut server, BlockPos::new(56, -60, 8), blocks::GLOWSTONE);
        server.tick();

        let frames = client.take_frames();
        let position = |id| frames.iter().position(|frame| frame.id == id);
        let light = position(clientbound::LIGHT_UPDATE).expect("边上的区块要发光照");
        let block = position(clientbound::BLOCK_UPDATE).expect("方块变化照常发");
        assert!(light < block);
        let lit: Vec<ChunkPos> = frames
            .iter()
            .filter(|frame| frame.id == clientbound::LIGHT_UPDATE)
            .map(light_update_chunk)
            .collect();
        assert!(lit.contains(&BORDER));
        // (2, 0) 也亮了，但它不在边上，客户端自己算得出来。
        assert!(!lit.contains(&ChunkPos::new(2, 0)));
    }

    #[test]
    fn inner_chunks_get_no_light_update() {
        let (mut server, client) = settled();
        change_block(&mut server, BlockPos::new(8, -60, 3), blocks::GLOWSTONE);
        server.tick();

        let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
        assert!(ids.contains(&clientbound::BLOCK_UPDATE));
        assert!(!ids.contains(&clientbound::LIGHT_UPDATE));
    }

    /// 边上那个区块里一块 16×5×16 的石头，1280 个任务，至少要算两轮。
    fn stone_box() -> Vec<BlockPos> {
        let mut positions = Vec::new();
        for x in 48..64 {
            for y in -55..=-51 {
                for z in 0..16 {
                    positions.push(BlockPos::new(x, y, z));
                }
            }
        }
        positions
    }

    #[test]
    fn light_that_takes_several_rounds_still_arrives() {
        let (mut server, client) = settled();
        let stones = stone_box();
        for &pos in &stones {
            change_block(&mut server, pos, blocks::STONE);
        }

        // 一 tick 能算几轮看机器快慢，所以只看最后的结果。
        let mut lit = Vec::new();
        for _ in 0..100 {
            server.tick();
            let frames = client.take_frames();
            let updates = frames
                .iter()
                .filter(|frame| frame.id == clientbound::LIGHT_UPDATE);
            lit.extend(updates.map(light_update_chunk));
            if !server.region().chunks.has_pending_light_updates() {
                break;
            }
        }
        assert!(!server.region().chunks.has_pending_light_updates());
        assert!(lit.contains(&BORDER));

        // 分几 tick 算完的光照和一口气算完的一样。
        let mut reference = ChunkStore::flat();
        for &pos in &stones {
            reference.set_block(pos, blocks::STONE);
        }
        reference.finish_light_updates();
        let chunks = &server.region().chunks;
        for x in 32..80 {
            for y in -61..-45 {
                for z in -16..32 {
                    let pos = BlockPos::new(x, y, z);
                    for layer in [LightLayer::Sky, LightLayer::Block] {
                        assert_eq!(
                            chunks.light_value(layer, pos),
                            reference.light_value(layer, pos)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn changes_from_several_runs_add_up() {
        let pos = ChunkPos::new(2, -3);
        let mut pending = BTreeMap::from([(
            pos,
            LightChanges {
                sky: 0b01,
                block: 0,
            },
        )]);
        merge_changes(
            &mut pending,
            BTreeMap::from([(
                pos,
                LightChanges {
                    sky: 0b10,
                    block: 0b100,
                },
            )]),
        );
        assert_eq!(
            pending[&pos],
            LightChanges {
                sky: 0b11,
                block: 0b100
            }
        );
    }
}
