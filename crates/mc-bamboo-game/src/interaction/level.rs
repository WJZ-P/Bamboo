//! [`mc_bamboo_blocks::Level`] 的实现：方块逻辑通过它读写区块、记下改动、看玩家挡不挡路。

use std::{collections::BTreeMap, slice};

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockGameRules, BlockState, Combined, Container, EntityStore,
    EntityTables, HopperEntity, Level, LevelRandom, LevelTables, RedstoneMemory, ShulkerSlots,
    Slots, WorldBorder,
    container::redstone_signal,
    entity::Entities,
    level::EquipTarget,
    shape::{self, BooleanOp},
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{
    block_of_state, blocks,
    registries::{block_entity_type as entities, data_component_type as kinds, entity_type},
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue},
    item::ItemStack,
    packet::play::GameMode,
};
use mc_bamboo_world::{MIN_Y, flat, store::ChunkStore};

use super::{BlockChanges, MAX_Y};
use crate::{
    container::{ContainerEntry, ContainerKind},
    inbound::PlayerId,
    player::Player,
    tick_levels::TickLevels,
};

/// 一次方块操作借用的 [`Region`](crate::region::Region) 字段。
pub(crate) struct RegionLevel<'a> {
    pub(crate) chunks: &'a mut ChunkStore,
    pub(crate) changes: &'a mut BlockChanges,
    /// 方块实体从物品带来的组件，换了方块就丢，见 [`crate::block_items`]。
    pub(crate) block_items: &'a mut BTreeMap<BlockPos, ComponentPatch>,
    pub(crate) players: &'a mut BTreeMap<PlayerId, Player>,
    pub(crate) random: &'a LevelRandom,
    pub(crate) redstone: &'a mut RedstoneMemory,
    pub(crate) block_entities: &'a mut BlockEntities,
    pub(crate) block_events: &'a mut BlockEvents,
    pub(crate) containers: &'a mut BTreeMap<BlockPos, ContainerEntry>,
    pub(crate) entities: &'a mut Entities,
    pub(crate) loot_sequences: &'a mc_bamboo_loot::RandomSequences,
    pub(crate) block_rules: BlockGameRules,
    /// 方块逻辑读到的天气快照。权威副本在 `Global`，改完立刻写过来。
    pub(crate) block_weather: mc_bamboo_blocks::BlockWeather,
    /// 这一维度的世界边界快照，`Level.getWorldBorder()`。
    ///
    /// This dimension's world-border snapshot, `Level.getWorldBorder()`.
    pub(crate) world_border: WorldBorder,
    pub(crate) command_blocks: &'a mut BTreeMap<BlockPos, crate::command_block::CommandBlockData>,
    /// 这一刻哪些区块参与各阶段。无限超平坦模式下恒为真。
    pub(crate) tick_levels: &'a TickLevels,
}

impl LevelTables for RegionLevel<'_> {
    fn block_entities_table(&self) -> &BlockEntities {
        self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        self.block_entities
    }

    fn block_events_table_mut(&mut self) -> &mut BlockEvents {
        self.block_events
    }

    fn redstone_table(&self) -> &RedstoneMemory {
        self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory {
        self.redstone
    }
}

impl EntityTables for RegionLevel<'_> {
    fn entities_table(&self) -> &EntityStore {
        self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        self.entities
    }
}

impl Level for RegionLevel<'_> {
    fn world_border(&self) -> WorldBorder {
        self.world_border.clone()
    }

    fn chunk_ticks_blocks(&self, chunk: ChunkPos) -> bool {
        self.tick_levels.chunk_ticks_blocks(chunk)
    }

    fn chunk_ticks_entities(&self, chunk: ChunkPos) -> bool {
        self.tick_levels.chunk_ticks_entities(chunk)
    }

    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool {
        let previous = self.chunks.block(pos).map(BlockState);
        // `/setblock` / `/fill` 没有玩家，破坏路径经过这里：官方的
        // `preRemoveSideEffects` 在读内容之前解包。
        // `/setblock` and `/fill` have no player; their destruction path passes here,
        // and vanilla's `preRemoveSideEffects` unpacks before reading the contents.
        if previous.is_some_and(|previous| !previous.is_same_block(state)) {
            self.unpack_loot_table(pos);
        }
        let written = self.chunks.set_block(pos, state.0);
        // 官方 `LevelChunk.setBlockState`：换成别的方块时旧的方块实体就没了。
        // 区块里那份 `getUpdateTag` 由 `Chunk::set_block` 自己丢。
        if written && previous.is_some_and(|previous| !previous.is_same_block(state)) {
            self.block_items.remove(&pos);
        }
        if written {
            crate::command_block::forget_if_replaced(self.command_blocks, pos, state);
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

    fn section_hash(&self, chunk: ChunkPos, section_y: i32) -> Option<u128> {
        self.chunks.section_hash(chunk, section_y)
    }

    fn randomly_ticking_sections(&self, chunk: ChunkPos) -> u32 {
        self.chunks.randomly_ticking_sections(chunk)
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

    fn block_game_rules(&self) -> BlockGameRules {
        self.block_rules
    }

    fn block_weather(&self) -> mc_bamboo_blocks::BlockWeather {
        self.block_weather
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`：读区块里算好的真实高度图。
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        self.chunks
            .heightmap_y(pos)
            .unwrap_or(mc_bamboo_world::MIN_Y - 1)
    }

    /// 群系注册表 ID 来自区块的调色板，温度按官方 `Biome.getTemperature` 算。
    fn precipitation_facts(&self, pos: BlockPos) -> mc_bamboo_blocks::PrecipitationFacts {
        use mc_bamboo_worldgen::biome_temperature as biome;
        let id = self.chunks.biome_at(pos).unwrap_or(0) as i32;
        let climate = biome::BiomeClimate::of(id);
        let warm = climate.warm_enough_to_rain(pos.x, pos.y, pos.z, flat::SEA_LEVEL);
        mc_bamboo_blocks::PrecipitationFacts {
            precipitation: match biome::precipitation_at(
                climate,
                pos.x,
                pos.y,
                pos.z,
                flat::SEA_LEVEL,
            ) {
                biome::Precipitation::None => mc_bamboo_blocks::Precipitation::None,
                biome::Precipitation::Rain => mc_bamboo_blocks::Precipitation::Rain,
                biome::Precipitation::Snow => mc_bamboo_blocks::Precipitation::Snow,
            },
            warm_enough_to_rain: warm,
        }
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

    /// `RandomizableContainer.unpackLootTable`：漏斗、发射器、陶罐和比较器读之前
    /// 先把这一格的战利品表填进槽位。
    ///
    /// `RandomizableContainer.unpackLootTable`: fill the table into the slots before
    /// a hopper, dispenser, decorated pot or comparator reads this cell.
    fn unpack_loot_table(&mut self, pos: BlockPos) {
        crate::container_loot::unpack(
            crate::container_loot::Unpack {
                chunks: self.chunks,
                containers: self.containers,
                sequences: self.loot_sequences,
                random: self.random,
                weather: self.block_weather,
            },
            pos,
        );
    }

    /// `ShulkerBoxBlock.playerWillDestroy` 的创造模式分支。空盒子返回 `None`。
    ///
    /// The creative branch of `ShulkerBoxBlock.playerWillDestroy`. An empty box returns `None`.
    fn creative_container_drop(&mut self, pos: BlockPos, item: i32) -> Option<ItemStack> {
        let entry = self.containers.get(&pos)?;
        if entry.slots.iter().all(ItemStack::is_empty) {
            return None;
        }
        let mut stack = ItemStack::new(item, 1);
        let patch =
            crate::block_items::collect_components_at(self.block_items, self.containers, pos);
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        Some(stack)
    }

    /// TNT 爆炸的 `getDrops`。没有爆炸半径，因为 `tntExplosionDropDecay` 默认是假。
    ///
    /// `getDrops` for a TNT explosion. There is no explosion radius because
    /// `tntExplosionDropDecay` defaults to false.
    fn explosion_block_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let block_entity = crate::block_drops::collect_block_entity(self.containers, pos);
        crate::block_drops::explosion_loot(
            self.chunks,
            self.loot_sequences,
            self.random.source(),
            self.block_weather,
            state,
            pos,
            block_entity,
        )
    }

    /// `destroyBlock` 的 `getDrops`。和爆炸一样没有半径，工具是空手。
    fn block_loot_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let block_entity = crate::block_drops::collect_block_entity(self.containers, pos);
        crate::block_drops::explosion_loot(
            self.chunks,
            self.loot_sequences,
            self.random.source(),
            self.block_weather,
            state,
            pos,
            block_entity,
        )
    }

    /// `getEntitiesOfClass(LivingEntity)`。玩家按 `PlayerId` 排序，和实体列表的顺序不一定相同。
    ///
    /// `getEntitiesOfClass(LivingEntity)`. Players are ordered by `PlayerId`, which is not
    /// necessarily the entity-list order.
    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        self.players.values().map(equip_target).collect()
    }

    /// 写进玩家物品栏。`Body` 和 `Saddle` 这套物品栏没有，返回 `None`。
    ///
    /// Write into the player inventory. This inventory has no `Body` or `Saddle` slot, so those
    /// return `None`.
    fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        let player = self
            .players
            .values_mut()
            .find(|player| player.id.0 == entity_id)?;
        let slot =
            crate::inventory::set_dispenser_slot(&mut player.inventory, component_slot, stack)?;
        u16::try_from(slot).ok()
    }
}

/// 一名玩家能被发射器看到的装备。身体和鞍是 `None`。
///
/// Equipment a dispenser can see on one player. Body and saddle are `None`.
fn equip_target(player: &Player) -> EquipTarget {
    let worn = |slot| player.inventory.equipment(slot).cloned();
    use mc_bamboo_protocol::packet::play::EquipmentSlot::{
        Body, Chest, Feet, Head, Legs, MainHand, OffHand, Saddle,
    };
    EquipTarget {
        entity_id: player.id.0,
        bounds: player.bounding_box(),
        spectator: player.game_mode == GameMode::Spectator,
        entity_type: entity_type::PLAYER,
        equipment: [
            worn(MainHand),
            worn(Feet),
            worn(Legs),
            worn(Chest),
            worn(Head),
            worn(OffHand),
            worn(Body),
            worn(Saddle),
        ],
    }
}
