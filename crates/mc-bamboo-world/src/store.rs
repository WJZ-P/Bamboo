//! 区块表：所有区块读写的唯一入口。
//!
//! 超平坦世界的区块内容都相同，所以只存一个模板，第一次改动一个区块时才给它一份自己的拷贝。
//! 光照也跟着区块走：改方块时记下光照要检查的地方，[`run_light_updates`](ChunkStore::run_light_updates) 时一轮轮地算。
//! 以后加 ticket 和卸载时，也只需要改这一个地方。

use std::collections::{BTreeMap, HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{block_of_state, facts, registries::block_entity_type, state_facts};

use crate::{
    MIN_Y, WORLD_HEIGHT,
    chunk::{Chunk, Section},
    flat,
    light::{Chunks, LevelLight, LightChanges, SectionPos, write_block},
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
    /// 光照性质变了时更新这一列的天空光源，再记下这一格要检查。光照要等 [`run_light_updates`](Self::run_light_updates) 才算。
    pub fn set_block(&mut self, pos: BlockPos, state: u32) -> bool {
        let Some(previous) = self.block(pos) else {
            return false;
        };
        let was_empty = self.chunk(pos.chunk()).is_section_empty(pos.y);
        if previous == state || (was_empty && is_air(state)) {
            return false;
        }
        let current = self.edited.get(&pos.chunk()).unwrap_or(&self.template);
        self.light.block_changing(current, pos, state);
        let template = &self.template;
        let chunk = self
            .edited
            .entry(pos.chunk())
            .or_insert_with(|| template.clone());
        let light_changed = write_block(chunk, pos, state);
        let is_empty = chunk.is_section_empty(pos.y);

        if was_empty != is_empty {
            self.light
                .update_section_status(SectionPos::of(pos), is_empty);
        }
        if light_changed {
            self.light.check_block(pos);
        }
        true
    }

    /// 这一格的告示牌内容，对应官方 `getBlockEntity` 拿到 `SignBlockEntity` 后读的字段。
    ///
    /// 告示牌方块还没写过字时返回空白内容：官方也是按状态临时建一个空的方块实体。
    /// 不是告示牌方块时返回 `None`。
    pub fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        if !is_inside_world(pos.y) {
            return None;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        let chunk = self.chunk(pos.chunk());
        match chunk.sign(x, pos.y, z) {
            Some(content) => Some(content.clone()),
            None => is_sign(chunk.block(x, pos.y, z)).then(SignContent::default),
        }
    }

    /// 改写告示牌内容。这一格不是告示牌方块时什么也不做，返回 `false`。
    pub fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        if !is_inside_world(pos.y) {
            return false;
        }
        let (x, z) = (local(pos.x), local(pos.z));
        if !is_sign(self.chunk(pos.chunk()).block(x, pos.y, z)) {
            return false;
        }
        let template = &self.template;
        let chunk = self
            .edited
            .entry(pos.chunk())
            .or_insert_with(|| template.clone());
        chunk.set_sign(x, pos.y, z, content);
        true
    }

    /// 正在被某名玩家编辑的告示牌和它们的编辑器。
    pub fn sign_editors(&self) -> impl Iterator<Item = (BlockPos, [u8; 16])> + '_ {
        self.edited.iter().flat_map(|(chunk_pos, chunk)| {
            chunk.signs().filter_map(move |(x, y, z, content)| {
                content.editor.map(|editor| {
                    let pos =
                        BlockPos::new(chunk_pos.x * 16 + x as i32, y, chunk_pos.z * 16 + z as i32);
                    (pos, editor)
                })
            })
        })
    }

    /// 算攒着的光照，官方的光照线程在两个 tick 之间做这件事：攒着的任务排成一轮，再一轮轮地算。
    ///
    /// 工作量（任务数加上传播的步数）到了 `budget` 就停，哪怕一轮算到一半，下次接着算。
    /// 一轮算完才换上新的光照，[`light_value`](Self::light_value) 读到的总是上一轮算完的结果。返回这次的工作量。
    pub fn run_light_updates(&mut self, budget: usize) -> usize {
        self.light
            .run_updates(Chunks::new(&self.template, &self.edited), budget)
    }

    /// 把攒着的光照全部算完。
    pub fn finish_light_updates(&mut self) {
        self.run_light_updates(usize::MAX);
    }

    /// 还有没算完的光照。
    pub fn has_pending_light_updates(&self) -> bool {
        self.light.has_pending_updates()
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

/// 这个方块的方块实体是告示牌，站着、挂着、贴在墙上的都算。
fn is_sign(state: u32) -> bool {
    matches!(
        block_of_state(state).and_then(|block| block.block_entity_type),
        Some(block_entity_type::SIGN | block_entity_type::HANGING_SIGN)
    )
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

    #[test]
    fn signs_start_blank_and_keep_what_is_written() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -60, 3);

        assert_eq!(store.sign(pos), None);

        store.set_block(pos, blocks::OAK_SIGN);
        assert_eq!(store.sign(pos), Some(SignContent::default()));

        let mut content = SignContent::default();
        content.front.messages[0] = "hello".to_owned();
        assert!(store.set_sign(pos, content.clone()));
        assert_eq!(store.sign(pos), Some(content));
    }

    #[test]
    fn sign_content_does_not_survive_a_different_block() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(3, -60, 3);
        store.set_block(pos, blocks::OAK_SIGN);
        let mut content = SignContent::default();
        content.front.messages[0] = "hello".to_owned();
        store.set_sign(pos, content);

        // 含水或者转向都还是同一个方块，内容要留着。
        store.set_block(pos, blocks::OAK_SIGN + 1);
        assert!(store.sign(pos).is_some_and(|sign| sign.front.has_message()));

        store.set_block(pos, blocks::STONE);
        assert_eq!(store.sign(pos), None);
    }

    #[test]
    fn sign_content_cannot_be_written_to_another_block() {
        let mut store = ChunkStore::flat();
        assert!(!store.set_sign(BlockPos::new(0, -61, 0), SignContent::default()));
        assert!(store.edited(ChunkPos::new(0, 0)).is_none());
    }

    #[test]
    fn editors_are_reported_with_their_world_position() {
        let mut store = ChunkStore::flat();
        let pos = BlockPos::new(19, -60, -11);
        store.set_block(pos, blocks::OAK_WALL_SIGN);
        let content = SignContent {
            editor: Some([7; 16]),
            ..SignContent::default()
        };
        store.set_sign(pos, content);

        assert_eq!(store.sign_editors().collect::<Vec<_>>(), [(pos, [7; 16])]);
    }
}
