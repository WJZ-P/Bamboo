//! 区块变化的广播，官方的 `ChunkHolder.broadcastChanges`。
//!
//! 有变化的区块逐个处理：先把 `light_update` 发给区块在跟踪范围边上的玩家，再从下往上逐段把方块变化
//! 发给 `is_chunk_tracked` 的玩家。这一 tick 只改了一格的段发 `block_update`，改了几格的段发一个
//! `section_blocks_update`，后面都跟着这些格子里方块实体的 `block_entity_data`。
//! 区块还在 `pending` 里的玩家不用发，它们收到的区块就是最新内容。
//!
//! 官方按 `ReferenceOpenHashSet` 的顺序遍历区块，没有约定的先后，这里按区块坐标排序。
//! 同一个区块里的顺序和官方相同，段里格子的先后见 [`changed_blocks`]。

mod changed_blocks;

use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::block_of_state;
use mc_bamboo_protocol::packet::play::{
    BlockEntityData, BlockUpdate, SectionBlock, SectionBlocksUpdate,
};

use self::changed_blocks::ChangedBlocks;
use crate::{inbound::PlayerId, light_updates, links::encode_frames, region::Region, sign};

/// 一个区块里改过的段，键是段的纵坐标（方块 `y >> 4`），从下往上。
type ChangedSections = BTreeMap<i32, ChangedBlocks>;

/// tick 第 5 阶段：逐个区块发光照和方块的变化，发完清空。
pub(crate) fn broadcast_changes(region: &mut Region) {
    let light = mem::take(&mut region.light_changes);
    let blocks = group_by_section(&mem::take(&mut region.block_changes.changed));
    let chunks: BTreeSet<ChunkPos> = light.keys().chain(blocks.keys()).copied().collect();
    for chunk in chunks {
        if let Some(&changes) = light.get(&chunk) {
            light_updates::broadcast_chunk_light(region, chunk, changes);
        }
        if let Some(sections) = blocks.get(&chunk) {
            broadcast_sections(region, chunk, sections);
        }
    }
}

/// 按区块和段分好，格子按改的先后记进各段，官方的 `ChunkHolder.blockChanged`。
fn group_by_section(changed: &[BlockPos]) -> BTreeMap<ChunkPos, ChangedSections> {
    let mut chunks: BTreeMap<ChunkPos, ChangedSections> = BTreeMap::new();
    for &pos in changed {
        chunks
            .entry(pos.chunk())
            .or_default()
            .entry(pos.y >> 4)
            .or_insert_with(ChangedBlocks::new)
            .insert(local_of(pos));
    }
    chunks
}

/// 一个区块的方块变化。收包的玩家每个区块只挑一次，没人收时也算发过了。
fn broadcast_sections(region: &mut Region, chunk: ChunkPos, sections: &ChangedSections) {
    let receivers: Vec<PlayerId> = region
        .players
        .values()
        .filter(|player| player.chunks.is_chunk_tracked(chunk))
        .map(|player| player.id)
        .collect();
    if receivers.is_empty() {
        return;
    }
    for (&section_y, blocks) in sections {
        let frames = encode_section(region, chunk, section_y, blocks);
        for &id in &receivers {
            region.links.send_frames(id, &frames);
        }
    }
}

/// 一段的方块变化，读的是现在的状态，后面跟着方块实体的数据。
fn encode_section(
    region: &Region,
    chunk: ChunkPos,
    section_y: i32,
    blocks: &ChangedBlocks,
) -> Vec<u8> {
    // 一段要么整个在世界里，要么整个在外面，世界外的段读不到方块，什么也不发。
    let changes: Vec<(BlockPos, u32)> = blocks
        .iter()
        .map(|local| block_pos(chunk, section_y, local))
        .filter_map(|pos| Some((pos, region.chunks.block(pos)?)))
        .collect();
    let mut frames = match changes[..] {
        [] => return Vec::new(),
        [(pos, state)] => encode_frames(&BlockUpdate {
            pos,
            state: state as i32,
        }),
        _ => encode_section_update(chunk, section_y, &changes),
    };
    // `broadcastBlockEntityIfNeeded`，和包里的格子一个顺序。
    for &(pos, state) in &changes {
        if let Some(data) = block_entity_data_frames(region, pos, state) {
            frames.extend_from_slice(&data);
        }
    }
    frames
}

fn encode_section_update(chunk: ChunkPos, section_y: i32, changes: &[(BlockPos, u32)]) -> Vec<u8> {
    let blocks: Vec<SectionBlock> = changes
        .iter()
        .map(|&(pos, state)| SectionBlock {
            local: local_of(pos),
            state: state as i32,
        })
        .collect();
    encode_frames(&SectionBlocksUpdate {
        x: chunk.x,
        y: section_y,
        z: chunk.z,
        blocks: &blocks,
    })
}

/// 这一格的方块实体有内容时，广播还要带上官方 `getUpdateTag` 的结果。
///
/// 告示牌的内容在区块的告示牌表里，旗帜、头颅和陶罐的在 [`crate::block_items`] 编好的那份
/// NBT 里。剩下的方块实体官方发的是空标签，客户端按方块状态自己建，所以只发方块更新。
/// 和官方一样先看方块有没有方块实体，一段改满时大多数格子在这一步就结束了。
fn block_entity_data_frames(region: &Region, pos: BlockPos, state: u32) -> Option<Vec<u8>> {
    let kind = block_of_state(state)?.block_entity_type?;
    let data = match region.chunks.sign(pos) {
        Some(content) => sign::update_tag(&content),
        None => region.chunks.update_tag(pos)?.to_vec(),
    };
    Some(encode_frames(&BlockEntityData {
        pos,
        kind,
        data: &data,
    }))
}

/// 段内坐标，官方的 `SectionPos.sectionRelativePos`：`x << 8 | z << 4 | y`，各取低 4 位。
fn local_of(pos: BlockPos) -> u16 {
    (((pos.x & 15) << 8) | ((pos.z & 15) << 4) | (pos.y & 15)) as u16
}

/// [`local_of`] 的逆运算，官方的 `SectionPos.relativeToBlockPos`。
fn block_pos(chunk: ChunkPos, section_y: i32, local: u16) -> BlockPos {
    let local = i32::from(local);
    BlockPos::new(
        (chunk.x << 4) | (local >> 8),
        (section_y << 4) | (local & 15),
        (chunk.z << 4) | ((local >> 4) & 15),
    )
}

#[cfg(test)]
mod tests;
