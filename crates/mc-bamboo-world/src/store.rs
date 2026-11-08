//! 区块表：所有区块读写的唯一入口。
//!
//! 超平坦世界的区块内容都相同，所以只存一个模板，第一次改动一个区块时才给它一份自己的拷贝。
//! 光照也跟着区块走：改方块时记下光照要检查的地方，[`run_light_updates`](ChunkStore::run_light_updates) 时一起算。
//! 以后加 ticket 和卸载时，也只需要改这一个地方。

use std::collections::{BTreeMap, HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::{facts, state_facts};

use crate::{
    MIN_Y, WORLD_HEIGHT,
    chunk::{Chunk, Section},
    flat,
    light::{Chunks, LevelLight, LightChanges, SectionPos, has_different_light_properties},
};

/// 世界里的全部区块。
///
/// 改过的区块一直留在内存里，直到服务端关闭。现在没有存档，卸载就等于丢掉玩家的改动。
#[derive(Debug, Clone)]
pub struct ChunkStore {
    template: Chunk,
    edited: HashMap<ChunkPos, Chunk>,
    light: LevelLight,
}

impl ChunkStore {
    /// 用 `template` 作为所有没改过的区块的内容，并算好它的光照。
    pub fn new(mut template: Chunk) -> Self {
        template.initialize_light_sources();
        let light = LevelLight::for_template(&template);
        Self {
            template,
            edited: HashMap::new(),
            light,
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

    /// 一个区块现在的内容。
    pub fn chunk(&self, pos: ChunkPos) -> &Chunk {
        self.edited.get(&pos).unwrap_or(&self.template)
    }

    /// 这个区块的方块和光照都和模板一样，可以共用模板的编码。
    pub fn is_template(&self, pos: ChunkPos) -> bool {
        !self.edited.contains_key(&pos) && !self.light.is_materialized(pos)
    }

    /// 读取方块状态。高度超出世界时返回 `None`，坐标可以直接来自客户端。
    pub fn block(&self, pos: BlockPos) -> Option<u32> {
        if !is_inside_world(pos.y) {
            return None;
        }
        Some(
            self.chunk(pos.chunk())
                .block(local(pos.x), pos.y, local(pos.z)),
        )
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
    ///
    /// 和官方的 `LevelChunk.setBlockState` 一样：区块段变空或者变满时登记一下，
    /// 光照性质变了时更新这一列的天空光源，再记下这一格要检查。
    pub fn set_block(&mut self, pos: BlockPos, state: u32) -> bool {
        let Some(previous) = self.block(pos) else {
            return false;
        };
        let was_empty = self.chunk(pos.chunk()).is_section_empty(pos.y);
        if previous == state || (was_empty && is_air(state)) {
            return false;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        let template = &self.template;
        let chunk = self
            .edited
            .entry(pos.chunk())
            .or_insert_with(|| template.clone());
        chunk.set_block(x, pos.y, z, state);
        let is_empty = chunk.is_section_empty(pos.y);
        let light_changed = has_different_light_properties(previous, state);
        if light_changed {
            chunk.update_sky_light_sources(x, pos.y, z);
        }

        let chunks = Chunks::new(&self.template, &self.edited);
        if was_empty != is_empty {
            self.light
                .update_section_status(chunks, SectionPos::of(pos), is_empty);
        }
        if light_changed {
            self.light.check_block(chunks, pos);
        }
        true
    }

    /// 把攒着的光照更新算完。官方在两个 tick 之间做这件事。
    pub fn run_light_updates(&mut self) {
        self.light
            .run_updates(Chunks::new(&self.template, &self.edited));
    }

    /// 上次取走以后光照变了的区块和段，发 `light_update` 用。
    pub fn take_light_changes(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        self.light.take_changes()
    }

    /// 世界的光照。
    pub fn light(&self) -> &LevelLight {
        &self.light
    }

    /// `getBrightness`：一格的天空光或者方块光。
    pub fn light_value(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        self.light.light_value(layer, pos)
    }
}

const fn is_inside_world(y: i32) -> bool {
    y >= MIN_Y && y < MIN_Y + WORLD_HEIGHT
}

/// `isAir`。
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
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
    fn air_into_an_empty_section_is_a_no_op() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, 100, 3);

        assert!(!store.set_block(pos, blocks::CAVE_AIR));
        assert!(store.is_template(pos.chunk()));
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
