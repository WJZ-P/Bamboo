//! [`mc_bamboo_blocks::Level`] 的实现：方块逻辑通过它读写区块、记下改动、看玩家挡不挡路。

use std::{collections::BTreeMap, slice};

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, Combined, Container, HopperEntity, Level,
    LevelRandom, RedstoneMemory, ShulkerSlots, Slots,
    container::redstone_signal,
    entity::Entities,
    shape::{self, BooleanOp},
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{
    block_of_state, blocks,
    registries::{block_entity_type as entities, data_component_type as kinds},
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue},
    item::ItemStack,
};
use mc_bamboo_world::{MIN_Y, store::ChunkStore};

use super::{BlockChanges, MAX_Y};
use crate::{
    container::{ContainerEntry, ContainerKind},
    inbound::PlayerId,
    player::Player,
};

/// 一次方块操作借用的 [`Region`](crate::region::Region) 字段。
pub(crate) struct RegionLevel<'a> {
    pub(crate) chunks: &'a mut ChunkStore,
    pub(crate) changes: &'a mut BlockChanges,
    /// 方块实体从物品带来的组件，换了方块就丢，见 [`crate::block_items`]。
    pub(crate) block_items: &'a mut BTreeMap<BlockPos, ComponentPatch>,
    pub(crate) players: &'a BTreeMap<PlayerId, Player>,
    pub(crate) random: &'a LevelRandom,
    pub(crate) redstone: &'a mut RedstoneMemory,
    pub(crate) block_entities: &'a mut BlockEntities,
    pub(crate) block_events: &'a mut BlockEvents,
    pub(crate) containers: &'a mut BTreeMap<BlockPos, ContainerEntry>,
    pub(crate) entities: &'a mut Entities,
}

impl Level for RegionLevel<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool {
        let previous = self.chunks.block(pos).map(BlockState);
        let written = self.chunks.set_block(pos, state.0);
        // 官方 `LevelChunk.setBlockState`：换成别的方块时旧的方块实体就没了。
        // 区块里那份 `getUpdateTag` 由 `Chunk::set_block` 自己丢。
        if written && previous.is_some_and(|previous| !previous.is_same_block(state)) {
            self.block_items.remove(&pos);
        }
        // 官方 `Level.setBlock` 只在 `UPDATE_CLIENTS` 时才 `sendBlockUpdated`，
        // 活塞的移动方块用 `UPDATE_INVISIBLE` 写，客户端会自己播放动画。
        if written && flags & mc_bamboo_blocks::world::UPDATE_CLIENTS != 0 {
            self.changes.changed.push(pos);
        }
        written
    }

    /// 发起者自己也算：客户端预测失败时也会发这个包，不查的话方块会放进玩家身体里。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool {
        !self.players.values().any(|player| {
            let body = player.bounding_box();
            shape::join_is_not_empty(boxes, slice::from_ref(&body), BooleanOp::And)
        })
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MAX_Y
    }

    fn random(&self) -> &LevelRandom {
        self.random
    }

    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        self.chunks.is_randomly_ticking_section(chunk, section_y)
    }

    /// 这个世界只有创造模式，没有旁观者。
    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool {
        let corner = pos.corner();
        self.players.values().any(|player| {
            player.position().distance_to_sqr(corner).sqrt() < f64::from(max_distance)
        })
    }

    /// 读的是上一轮光照算完时的结果，和官方一样：这次操作改的方块要等到下一轮才影响光照。
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        i32::from(self.chunks.light_value(layer, pos))
    }

    fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        self.chunks.sign(pos)
    }

    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        self.chunks.set_sign(pos, content)
    }

    /// 头颅方块实体认的 `note_block_sound` 存在 [`Self::block_items`] 里，见
    /// [`crate::block_items`] 的 `apply`。
    fn note_block_sound(&self, pos: BlockPos) -> Option<String> {
        let skull =
            block_of_state(self.chunks.block(pos)?)?.block_entity_type == Some(entities::SKULL);
        if !skull {
            return None;
        }
        match self.block_items.get(&pos)?.get(kinds::NOTE_BLOCK_SOUND)? {
            ComponentValue::NoteBlockSound(sound) => Some(sound.clone()),
            _ => None,
        }
    }

    fn block_entities(&self) -> &BlockEntities {
        self.block_entities
    }

    fn block_entities_mut(&mut self) -> &mut BlockEntities {
        self.block_entities
    }

    fn block_events(&mut self) -> &mut BlockEvents {
        self.block_events
    }

    fn redstone(&self) -> &RedstoneMemory {
        self.redstone
    }

    fn redstone_mut(&mut self) -> &mut RedstoneMemory {
        self.redstone
    }

    fn entities(&self) -> &Entities {
        self.entities
    }

    fn entities_mut(&mut self) -> &mut Entities {
        self.entities
    }

    /// `getBlockContainer`：方块是容器时给它建一张槽位表。双箱按 `CompoundContainer`
    /// 的顺序合并，并且和漏斗一样忽略压住箱子的方块。
    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let Some(kind) = crate::container::kind_in(self.chunks, pos) else {
            return false;
        };
        // 官方的末影箱方块实体不是 `Container`，它的 27 格在玩家身上，漏斗拿不到。
        if kind == ContainerKind::EnderChest {
            return false;
        }
        if kind == ContainerKind::Chest
            && let Ok(Some(chest)) = crate::container::chest_shape_with(self.chunks, pos, true)
        {
            crate::container::entry_in(self.containers, chest.first, ContainerKind::Chest);
            crate::container::entry_in(self.containers, chest.second, ContainerKind::Chest);
            // 两半要同时可变借用，标准库没有稳定的一次借两格，先拿出来再放回去。
            let Some(mut first) = self.containers.remove(&chest.first) else {
                return false;
            };
            let Some(mut second) = self.containers.remove(&chest.second) else {
                self.containers.insert(chest.first, first);
                return false;
            };
            {
                let mut combined = Combined::new(&mut first.slots, &mut second.slots);
                f(&mut combined);
            }
            self.containers.insert(chest.first, first);
            self.containers.insert(chest.second, second);
            return true;
        }
        let entry = crate::container::entry_in(self.containers, pos, kind);
        // 潜影盒是 `WorldlyContainer`：整张表都能用，但不收潜影盒物品。
        if kind == ContainerKind::ShulkerBox {
            let mut shulker = ShulkerSlots::new(&mut entry.slots);
            f(&mut shulker);
        } else {
            let mut slots = Slots::new(&mut entry.slots);
            f(&mut slots);
        }
        true
    }

    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let Some(kind) = crate::container::kind_in(self.chunks, pos) else {
            return false;
        };
        if kind.slots() != len {
            return false;
        }
        let entry = crate::container::entry_in(self.containers, pos, kind);
        f(&mut entry.slots);
        true
    }

    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        self.containers
            .get(&pos)
            .map(|entry| entry.slots.clone())
            .unwrap_or_default()
    }

    fn clear_container(&mut self, pos: BlockPos) {
        self.containers.remove(&pos);
    }

    /// `BlockEntity.preRemoveSideEffects` 掉落的是这一格方块实体自己的槽位表，
    /// 双箱的另一半不算在内。
    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        self.containers
            .get(&pos)
            .map(|entry| entry.slots.iter().map(|stack| stack.count).collect())
            .unwrap_or_default()
    }

    /// `ChestBlock.getAnalogOutputSignal` 比较器读信号时不忽略压住箱子的方块，
    /// 双箱按合并后的 54 格算。
    fn container_signal(&self, pos: BlockPos) -> i32 {
        let kind = crate::container::kind_in(self.chunks, pos);
        if kind == Some(ContainerKind::Chest) {
            match crate::container::chest_shape(self.chunks, pos) {
                Err(()) => return 0,
                Ok(None) => {}
                Ok(Some(chest)) => {
                    let first = self
                        .containers
                        .get(&chest.first)
                        .map_or(&[][..], |entry| entry.slots.as_slice());
                    let second = self
                        .containers
                        .get(&chest.second)
                        .map_or(&[][..], |entry| entry.slots.as_slice());
                    return redstone_signal(
                        first.iter().chain(second.iter()),
                        first.len() + second.len(),
                    );
                }
            }
        }
        let Some(entry) = self.containers.get(&pos) else {
            return 0;
        };
        redstone_signal(entry.slots.iter(), entry.slots.len())
    }

    /// `CompoundContainer.setChanged`：双箱两半都通知。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        if crate::container::kind_in(self.chunks, pos) == Some(ContainerKind::Chest)
            && let Ok(Some(chest)) = crate::container::chest_shape_with(self.chunks, pos, true)
        {
            return vec![chest.first, chest.second];
        }
        vec![pos]
    }

    /// 拿漏斗方块实体。它正在 tick 时已经从表里拿出来了，这时返回 `false`。
    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let Some(hopper) = self.block_entities.get_mut(pos) else {
            return false;
        };
        f(hopper);
        true
    }
}
