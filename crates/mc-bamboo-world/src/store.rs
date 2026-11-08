//! 区块表：所有区块读写的唯一入口。
//!
//! 超平坦世界的区块内容都相同，所以只存一个模板，第一次改动一个区块时才给它一份自己的拷贝。
//! 以后加 ticket 和卸载时，也只需要改这一个地方。

use std::collections::HashMap;

use mc_bamboo_core::{BlockPos, ChunkPos};

use crate::{
    MIN_Y, WORLD_HEIGHT,
    chunk::{Chunk, Section},
    flat,
};

/// 世界里的全部区块。
///
/// 改过的区块一直留在内存里，直到服务端关闭。现在没有存档，卸载就等于丢掉玩家的改动。
#[derive(Debug, Clone)]
pub struct ChunkStore {
    template: Chunk,
    edited: HashMap<ChunkPos, Chunk>,
}

impl ChunkStore {
    /// 用 `template` 作为所有没改过的区块的内容。
    pub fn new(template: Chunk) -> Self {
        Self {
            template,
            edited: HashMap::new(),
        }
    }

    /// 超平坦世界。
    pub fn flat() -> Self {
        Self::new(flat::generate_chunk())
    }

    /// 所有没改过的区块共用的内容。
    pub fn template(&self) -> &Chunk {
        &self.template
    }

    /// 改过的区块。这个位置没改过时返回 `None`，内容就是 [`template`](Self::template)。
    pub fn edited(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.edited.get(&pos)
    }

    /// 读取方块状态。高度超出世界时返回 `None`，坐标可以直接来自客户端。
    pub fn block(&self, pos: BlockPos) -> Option<u32> {
        if !is_inside_world(pos.y) {
            return None;
        }
        let chunk = self.edited.get(&pos.chunk()).unwrap_or(&self.template);
        Some(chunk.block(local(pos.x), pos.y, local(pos.z)))
    }

    /// 区块 `pos` 里纵坐标是 `section_y`（方块的 `y >> 4`）的区块段有会随机刻的方块。
    /// 超出世界高度的区块段没有方块。
    pub fn is_randomly_ticking_section(&self, pos: ChunkPos, section_y: i32) -> bool {
        let chunk = self.edited.get(&pos).unwrap_or(&self.template);
        usize::try_from(section_y - (MIN_Y >> 4))
            .ok()
            .and_then(|index| chunk.sections().get(index))
            .is_some_and(Section::is_randomly_ticking)
    }

    /// 设置方块状态，返回方块是否真的变了。高度超出世界时什么也不做。
    pub fn set_block(&mut self, pos: BlockPos, state: u32) -> bool {
        if self.block(pos).is_none_or(|current| current == state) {
            return false;
        }
        let chunk = self
            .edited
            .entry(pos.chunk())
            .or_insert_with(|| self.template.clone());
        chunk.set_block(local(pos.x), pos.y, local(pos.z), state);
        true
    }
}

const fn is_inside_world(y: i32) -> bool {
    y >= MIN_Y && y < MIN_Y + WORLD_HEIGHT
}

/// 世界坐标在区块里的下标。`&` 对负数也是对的，`-1` 是区块里的第 15 格。
const fn local(coordinate: i32) -> usize {
    (coordinate & 15) as usize
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    #[test]
    fn untouched_chunks_read_the_template() {
        let store = ChunkStore::flat();

        assert_eq!(store.block(BlockPos::new(0, -64, 0)), Some(blocks::BEDROCK));
        assert_eq!(
            store.block(BlockPos::new(-1000, -61, 7777)),
            Some(blocks::GRASS_BLOCK)
        );
        assert_eq!(store.block(BlockPos::new(5, -60, 5)), Some(blocks::AIR));
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn the_grass_section_ticks_randomly() {
        let mut store = ChunkStore::flat();
        let chunk = ChunkPos::new(3, -2);
        assert!(store.is_randomly_ticking_section(chunk, -4));
        assert!(!store.is_randomly_ticking_section(chunk, -3));
        assert!(!store.is_randomly_ticking_section(chunk, -5));
        assert!(!store.is_randomly_ticking_section(chunk, 20));

        store.set_block(BlockPos::new(48, 0, -32), blocks::WHEAT);
        assert!(store.is_randomly_ticking_section(chunk, 0));
    }

    #[test]
    fn heights_outside_the_world_have_no_block() {
        let mut store = ChunkStore::flat();

        for y in [-65, 320, i32::MIN, i32::MAX] {
            let pos = BlockPos::new(0, y, 0);
            assert_eq!(store.block(pos), None);
            assert!(!store.set_block(pos, blocks::STONE));
        }
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
        assert_eq!(store.block(BlockPos::new(0, 319, 0)), Some(blocks::AIR));
    }

    #[test]
    fn first_write_copies_the_template_for_that_chunk_only() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(-1, -60, 16);

        assert!(store.set_block(pos, blocks::STONE));

        assert_eq!(store.block(pos), Some(blocks::STONE));
        let chunk = store.edited(ChunkPos::new(-1, 1)).expect("这个区块改过了");
        assert_eq!(chunk.block(15, -60, 0), blocks::STONE);
        // 拷贝保留模板的其余内容，模板本身不变。
        assert_eq!(chunk.block(0, -61, 0), blocks::GRASS_BLOCK);
        assert_eq!(store.template().block(15, -60, 0), blocks::AIR);
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn writing_the_same_state_is_a_no_op() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -61, 3);

        assert!(!store.set_block(pos, blocks::GRASS_BLOCK));
        assert!(store.edited(pos.chunk()).is_none());

        assert!(store.set_block(pos, blocks::STONE));
        assert!(!store.set_block(pos, blocks::STONE));
    }

    #[test]
    fn edited_chunks_keep_their_block_entities() {
        let mut store = ChunkStore::flat();
        store.set_block(BlockPos::new(19, -60, -11), blocks::CHEST);

        let chunk = store.edited(ChunkPos::new(1, -1)).expect("这个区块改过了");
        let entity = chunk.block_entities().next().expect("箱子有方块实体");
        assert_eq!((entity.x, entity.y, entity.z), (3, -60, 5));
    }
}
