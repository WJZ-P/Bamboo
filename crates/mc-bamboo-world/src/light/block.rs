//! 方块光，官方的 `BlockLightEngine`：火把、萤石这些方块自己发光，往六个方向一格格变暗。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction};
use mc_bamboo_data::blocks;

use super::{
    Chunks, SectionPos,
    engine::{LightEngine, PULL_LIGHT_IN_ENTRY, entry},
    properties::{emission, is_empty_shape, opacity, shape_occludes},
};
use crate::{MIN_Y, SECTION_SIZE, chunk::Chunk};

impl LightEngine {
    /// `getEmission`：光照没打开的区块柱里方块不发光。
    fn block_emission(&self, node: BlockPos, state: u32) -> u8 {
        let emission = emission(state);
        if emission > 0 && self.storage.light_on(node.chunk()) {
            emission
        } else {
            0
        }
    }

    /// `BlockLightEngine.checkNode`。
    pub(super) fn check_block_node(&mut self, chunks: Chunks<'_>, node: BlockPos) {
        if !self.storage.storing_light_for_section(SectionPos::of(node)) {
            return;
        }
        let state = chunks.state(node);
        let emission = self.block_emission(node, state);
        let old = self.storage.stored_level(node);
        if emission < old {
            self.storage.set_stored_level(node, 0);
            self.enqueue_decrease(node, entry::decrease_all_directions(old));
        } else {
            self.enqueue_decrease(node, PULL_LIGHT_IN_ENTRY);
        }
        if emission > 0 {
            let data = entry::increase_light_from_emission(emission, is_empty_shape(state));
            self.enqueue_increase(node, data);
        }
    }

    /// `BlockLightEngine.propagateIncrease`。
    pub(super) fn propagate_block_increase(
        &mut self,
        chunks: Chunks<'_>,
        from: BlockPos,
        data: u64,
        level: u8,
    ) {
        let mut from_state = None;
        for direction in Direction::ALL {
            if !entry::should_propagate_in_direction(data, direction) {
                continue;
            }
            let to = from.relative(direction);
            let Some((new_level, to_state)) = self.brighter_level(chunks, to, level) else {
                continue;
            };
            let from_state =
                *from_state.get_or_insert_with(|| propagating_state(chunks, from, data));
            if !shape_occludes(from_state, to_state, direction) {
                self.storage.set_stored_level(to, new_level);
                if new_level > 1 {
                    let data = entry::increase_skip_one_direction(
                        new_level,
                        is_empty_shape(to_state),
                        direction.opposite(),
                    );
                    self.enqueue_increase(to, data);
                }
            }
        }
    }

    /// 从 `level` 照进 `to` 以后的等级和 `to` 的方块，比 `to` 现在亮的时候才有。`to` 不存光照时也没有。
    ///
    /// 官方先用 `level - 1` 粗筛一遍，过了才读方块。
    pub(super) fn brighter_level(
        &self,
        chunks: Chunks<'_>,
        to: BlockPos,
        level: u8,
    ) -> Option<(u8, u32)> {
        if !self.storage.storing_light_for_section(SectionPos::of(to)) {
            return None;
        }
        let current = self.storage.stored_level(to);
        if level.saturating_sub(1) <= current {
            return None;
        }
        let state = chunks.state(to);
        let new_level = level.saturating_sub(opacity(state));
        (new_level > current).then_some((new_level, state))
    }

    /// `BlockLightEngine.propagateDecrease`。
    pub(super) fn propagate_block_decrease(
        &mut self,
        chunks: Chunks<'_>,
        from: BlockPos,
        data: u64,
    ) {
        let old_level = entry::from_level(data);
        for direction in Direction::ALL {
            if !entry::should_propagate_in_direction(data, direction) {
                continue;
            }
            let to = from.relative(direction);
            if !self.storage.storing_light_for_section(SectionPos::of(to)) {
                continue;
            }
            let level = self.storage.stored_level(to);
            if level == 0 {
                continue;
            }
            if level < old_level {
                let state = chunks.state(to);
                let emission = self.block_emission(to, state);
                self.storage.set_stored_level(to, 0);
                if emission < level {
                    let data = entry::decrease_skip_one_direction(level, direction.opposite());
                    self.enqueue_decrease(to, data);
                }
                if emission > 0 {
                    let data = entry::increase_light_from_emission(emission, is_empty_shape(state));
                    self.enqueue_increase(to, data);
                }
            } else {
                let data = entry::increase_only_one_direction(level, false, direction.opposite());
                self.enqueue_increase(to, data);
            }
        }
    }

    /// `BlockLightEngine.propagateLightSources`：打开光照，从区块里每个发光的方块往外照。
    /// 区块段从下往上，段里 y、z、x 都是升序，和官方扫描的顺序一样。
    pub(super) fn propagate_block_light_sources(&mut self, chunks: Chunks<'_>, pos: ChunkPos) {
        self.storage.set_light_enabled(pos, true);
        let chunk = chunks.chunk(pos);
        for (index, section) in chunk.sections().iter().enumerate() {
            if !section.maybe_has(|state| emission(state) != 0) {
                continue;
            }
            let bottom = MIN_Y + index as i32 * SECTION_SIZE;
            for y in bottom..bottom + SECTION_SIZE {
                self.enqueue_emitters_in_layer(chunk, pos, y);
            }
        }
    }

    /// 一层 16×16 里发光的方块，z、x 升序。
    fn enqueue_emitters_in_layer(&mut self, chunk: &Chunk, pos: ChunkPos, y: i32) {
        for z in 0..16 {
            for x in 0..16 {
                let state = chunk.block(x, y, z);
                let emission = emission(state);
                if emission != 0 {
                    let node = BlockPos::new(pos.x * 16 + x as i32, y, pos.z * 16 + z as i32);
                    let data = entry::increase_light_from_emission(emission, is_empty_shape(state));
                    self.enqueue_increase(node, data);
                }
            }
        }
    }
}

/// 传播起点的方块。入队时标了“空形状”的话按空气算，省一次读取，结果相同。
pub(super) fn propagating_state(chunks: Chunks<'_>, from: BlockPos, data: u64) -> u32 {
    if entry::is_from_empty_shape(data) {
        blocks::AIR
    } else {
        chunks.state(from)
    }
}
