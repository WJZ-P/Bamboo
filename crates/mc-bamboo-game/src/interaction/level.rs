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
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{
    block_of_state, blocks,
    registries::{block_entity_type as entities, data_component_type as kinds, entity_type},
};
use mc_bamboo_loot::block_experience_set;
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
    pub(crate) block_items: &'a mut crate::pos_map::PosMap<ComponentPatch>,
    /// 这次挖方块的主手物品，读 `processBlockExperience`。
    /// 只在 [`RegionLevel::set_break_tool`] 里由挖矿路径设置，其余路径是空手。
    pub(crate) break_tool: mc_bamboo_protocol::item::ItemStack,
    pub(crate) players: &'a mut BTreeMap<PlayerId, Player>,
    pub(crate) random: &'a LevelRandom,
    pub(crate) redstone: &'a mut RedstoneMemory,
    pub(crate) block_entities: &'a mut BlockEntities,
    pub(crate) block_events: &'a mut BlockEvents,
    pub(crate) containers: &'a mut crate::pos_map::PosMap<ContainerEntry>,
    pub(crate) entities: &'a mut Entities,
    pub(crate) loot_sequences: &'a mc_bamboo_loot::RandomSequences,
    pub(crate) block_rules: BlockGameRules,
    /// 方块逻辑读到的天气快照。权威副本在 `Global`，改完立刻写过来。
    pub(crate) block_weather: mc_bamboo_blocks::BlockWeather,
    /// 这一刻的难度 ID，权威副本在 `GlobalView`。
    pub(crate) difficulty: crate::world_state::Difficulty,
    /// 这一维度的世界边界快照，`Level.getWorldBorder()`。
    ///
    /// This dimension's world-border snapshot, `Level.getWorldBorder()`.
    pub(crate) world_border: WorldBorder,
    pub(crate) command_blocks:
        &'a mut crate::pos_map::PosMap<crate::command_block::CommandBlockData>,
    /// 这一刻哪些区块参与各阶段。无限超平坦模式下恒为真。
    pub(crate) tick_levels: &'a TickLevels,
    /// 「改过没存」的标记，方块逻辑的每一次写都在这里登记。
    pub(crate) dirty: &'a mut crate::region::dirty::Dirty,
    /// 按区块统计三个方块侧阶段时用的剖析器。默认关闭，`None` 时 `observe` 直接返回。
    pub(crate) chunk_profiler: Option<&'a crate::profiler::Profiler>,
    /// 第 2 步的划分 / 越界统计。默认关闭，两个开关都关时 `observe` 只看两个
    /// `Option`，方块逻辑的读写完全不受影响；`World::new` 也只问一次布尔值。
    pub(crate) partition_profiler: Option<&'a crate::profiler::Profiler>,
    /// `halo = 1 / 2` 两套划分，观察点据此判断触发碰到的区块在不在范围内。
    pub(crate) partitions: &'a [crate::region::partition::Partition],
}

impl LevelTables for RegionLevel<'_> {
    fn block_entities_table(&self) -> &BlockEntities {
        self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        self.block_entities
    }

    fn block_events_table(&self) -> &BlockEvents {
        self.block_events
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

    fn mark_chunk_dirty(&mut self, pos: ChunkPos) {
        self.dirty.mark(pos);
    }
}

impl EntityTables for RegionLevel<'_> {
    fn entities_table(&self) -> &EntityStore {
        self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        self.entities
    }

    fn mark_chunk_dirty(&mut self, pos: ChunkPos) {
        self.dirty.mark(pos);
    }
}

impl RegionLevel<'_> {
    /// 官方 `Block.dropResources` 把挖矿者的主手物品一路传给
    /// `spawnAfterBreak` → `tryDropExperience`；这里在破坏之前把它挂到这次
    /// 的方块世界上。别的破坏路径不调用，看到的仍是空手。
    ///
    /// Official `Block.dropResources` passes the breaker's main-hand item to
    /// `spawnAfterBreak` → `tryDropExperience`; attach it to this block world
    /// before the break. Other break paths leave it empty.
    pub(crate) fn set_break_tool(&mut self, tool: mc_bamboo_protocol::item::ItemStack) {
        self.break_tool = tool;
    }

    /// 这一格（以及双箱另一半）的容器内容可能变了：叫醒关注它的睡眠方块实体。
    /// 和 `Region::touch` 是同一条规则，方块逻辑直接写上层表时走这里。
    fn notify_container(&mut self, pos: BlockPos) {
        self.block_entities.notify_cell(pos);
        for other in self.container_positions(pos) {
            if other != pos {
                self.block_entities.notify_cell(other);
            }
        }
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

    /// 本岛的玩家。官方 `ServerLevel.players()` 不过滤旁观者和死亡状态，
    /// 具体调用方各自用 `NO_SPECTATORS` / `TargetingConditions` 过滤。
    ///
    /// This island's players. `ServerLevel.players()` does not filter
    /// spectators or dead players; each caller applies its own predicates.
    fn for_each_player(&self, visit: &mut dyn FnMut(&mc_bamboo_blocks::entity::ctx::PlayerView)) {
        for (id, player) in self.players.iter() {
            visit(&player.view(*id));
        }
    }

    /// 官方 `Block.tryDropExperience` + `Block.popExperience`：
    /// `xpRange.sample(level.random)`（常数区间不取数），然后
    /// `ExperienceOrb.award(level, Vec3.atCenterOf(pos), amount)` 拆成球。
    /// `block_drops` 规则只挡生成球，不挡前面那次取数。
    ///
    /// Official `Block.tryDropExperience` + `Block.popExperience`: sample
    /// `xpRange` from `level.random` (constants draw nothing), then split it
    /// with `ExperienceOrb.award(level, Vec3.atCenterOf(pos), amount)`.
    /// The `block_drops` rule only suppresses the orbs, not the sample.
    fn spawn_after_break_random(&mut self, state: BlockState, pos: BlockPos) {
        let Some((min, max)) =
            mc_bamboo_blocks::block_experience::experience_range(state.block_id() as u32)
        else {
            return;
        };
        let mut amount = if min == max {
            min
        } else {
            min + self
                .random
                .source()
                .next_int(max.wrapping_sub(min).wrapping_add(1))
        };
        // 官方 `EnchantmentHelper.processBlockExperience`：附魔的
        // `minecraft:block_experience` 效果可以改掉采样值；26.3 里精准采集
        // 用 `set(0)` 让矿石不掉经验，时运没有这个效果所以值不变。
        // `MutableFloat` 按 float 累加，最后截断成 int。
        //
        // Official `EnchantmentHelper.processBlockExperience`: the enchantment's
        // `minecraft:block_experience` effects may change the sampled value.
        // In 26.3 silk touch sets it to zero; fortune has no such effect.
        let patch = self.break_tool.patch();
        if let Some(ComponentValue::Enchantments(levels)) = patch.get(kinds::ENCHANTMENTS) {
            let mut value = amount as f32;
            for (id, level) in levels {
                if *level > 0
                    && let Some(set) = block_experience_set(*id)
                {
                    value = set;
                }
            }
            amount = value as i32;
        }
        if amount <= 0 || !self.block_rules.block_drops {
            return;
        }
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        let mut left = amount;
        while left > 0 {
            let value = mc_bamboo_blocks::entity::experience_value(left);
            left -= value;
            // `ExperienceOrb.tryMergeToExisting` 的 `level.random.nextInt(40)`。
            let _ = self.random.source().next_int(40);
            mc_bamboo_blocks::level::LevelEntities::spawn_entity(
                self,
                mc_bamboo_blocks::entity::Entity::experience_orb(center, value),
            );
        }
    }

    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool {
        // 没加载的位置写不进去（`/fill` 的范围可以穿过没加载的区块）：不留待存标记、不叫醒
        // 关注者。那里的标记本来就是无效的（取走时按「已加载」滤掉），多岛时它还不属于
        // 任何一个岛，拆岛时没处放。
        // An unloaded position cannot be written (a `/fill` range may pass
        // through unloaded chunks): leave no save mark and wake nobody. Such
        // a mark is void anyway (dropped by the loaded filter when taken) and,
        // with several islands, would belong to no island on a split.
        if !self.chunks.is_loaded(pos.chunk()) {
            return false;
        }
        self.dirty.mark(pos.chunk());
        self.block_entities.notify_cell(pos);
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
        self.dirty.mark(pos.chunk());
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

    /// `getDifficulty().getId()`：权威副本在 `GlobalView`，这里读每刻发布的快照。
    fn difficulty_id(&self) -> i32 {
        self.difficulty as i32
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
        self.dirty.mark(pos.chunk());
        self.notify_container(pos);
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
        self.notify_container(pos);
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
        self.dirty.mark(pos.chunk());
        self.notify_container(pos);
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
        self.dirty.mark(pos.chunk());
        self.block_entities.notify_cell(pos);
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
    fn replace_block_entity_for_cache(&mut self, entity: Box<dyn mc_bamboo_blocks::BlockEntity>) {
        self.dirty.mark(entity.pos().chunk());
        self.block_entities.replace_in_place(entity);
    }

    fn unpack_loot_table(&mut self, pos: BlockPos) {
        self.dirty.mark(pos.chunk());
        if self.loot_table_pending(pos) {
            self.notify_container(pos);
        }
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

    fn loot_table_pending(&self, pos: BlockPos) -> bool {
        self.containers
            .get(&pos)
            .is_some_and(|entry| entry.loot_table.is_some())
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

    /// `LivingEntity.dropFromLootTable`：牛死时的战利品表。
    /// `on_fire` 决定牛肉是不是熟的，攻击者的主手附魔决定抢夺和熔炼加成。
    ///
    /// `LivingEntity.dropFromLootTable` for a dying cow. `on_fire` decides whether
    /// the beef is cooked; the killer's held item supplies the looting level.
    fn mob_loot_drops(
        &mut self,
        table: &str,
        on_fire: bool,
        killer: Option<mc_bamboo_blocks::EntityUid>,
    ) -> Vec<ItemStack> {
        let mut attacker = None;
        let mut direct = None;
        let mut last_player = None;
        if let Some(uid) = killer {
            let held = self
                .players
                .values()
                .find(|player| player.id.0 == uid)
                .map(|player| {
                    game_stack_to_loot(player.inventory.held(mc_bamboo_core::Hand::Main))
                });
            let mut view =
                mc_bamboo_loot::EntityView::new(mc_bamboo_data::registries::entity_type::PLAYER);
            view.is_living = true;
            if let Some(stack) = held {
                view.equipment[mc_bamboo_loot::EquipmentSlot::Mainhand.index()] = Some(stack);
            }
            let boxed = Box::new(view.clone());
            attacker = Some(view.clone());
            direct = Some(view);
            last_player = Some(*boxed);
        }
        let victim_type = match table {
            "minecraft:entities/zombie" => mc_bamboo_data::registries::entity_type::ZOMBIE,
            "minecraft:entities/husk" => mc_bamboo_data::registries::entity_type::HUSK,
            _ => mc_bamboo_data::registries::entity_type::COW,
        };
        let mut victim = mc_bamboo_loot::EntityView::new(victim_type);
        victim.is_living = true;
        victim.is_on_fire = on_fire;
        let view = crate::block_drops::ChunkLevel {
            chunks: self.chunks,
            weather: self.block_weather,
        };
        let params = mc_bamboo_loot::EntityDeathParams {
            entity: victim,
            origin: mc_bamboo_loot::Position::new(0.0, 0.0, 0.0),
            damage_source: mc_bamboo_loot::DamageSourceView {
                damage_type: 0,
                direct_entity: direct.clone().map(Box::new),
                source_entity: attacker.clone().map(Box::new),
                is_direct: true,
            },
            attacking_entity: attacker,
            direct_attacking_entity: direct,
            last_damage_player: last_player,
            luck: 0.0,
            level: Some(&view),
        };
        match mc_bamboo_loot::entity_death_drops(
            table,
            &params,
            0,
            Some(self.loot_sequences),
            self.random.source(),
        ) {
            Ok(stacks) => stacks.iter().map(crate::loot_stacks::loot_stack).collect(),
            Err(error) => {
                tracing::debug!(%error, "skipping mob drops: the loot table needs missing data");
                Vec::new()
            }
        }
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

    /// 按区块统计三个方块侧阶段的触发耗时；开关关着时 `chunk_profiler` 是 `None`，
    /// 这一步只有一次 `Option` 判断，和默认的空实现等价。
    fn observe(&self, event: mc_bamboo_blocks::Observation<'_>) {
        if let Some(profiler) = self.chunk_profiler {
            profiler.observe_trigger(event);
        }
        if let Some(profiler) = self.partition_profiler {
            profiler.observe_partition(event, self.partitions);
        }
    }

    fn observe_partition_enabled(&self) -> bool {
        self.partition_profiler
            .is_some_and(crate::profiler::Profiler::partition_enabled)
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

/// 把游戏里的物品堆换成战利品能读的一堆，保留 `minecraft:enchantments` 里的等级。
/// 官方 `LootItemFunctions` 的条件和 `enchanted_count_increase` 只问这个组件。
///
/// Converts a game item stack into a loot stack, carrying the enchantment levels
/// that loot conditions and `enchanted_count_increase` read.
fn game_stack_to_loot(stack: &ItemStack) -> mc_bamboo_loot::LootStack {
    let mut out = mc_bamboo_loot::LootStack::new(stack.item, stack.count);
    if let Some(mc_bamboo_protocol::components::ComponentValue::Enchantments(levels)) =
        mc_bamboo_protocol::components::defaults::resolve(
            stack.item,
            &stack.patch(),
            kinds::ENCHANTMENTS,
        )
    {
        let mut map = std::collections::BTreeMap::new();
        for &(id, level) in levels {
            if let Some(name) = mc_bamboo_loot::enchantment_name(id) {
                map.insert(name.to_owned(), level);
            }
        }
        out.set_enchantment_levels(mc_bamboo_loot::ENCHANTMENTS_COMPONENT, map);
    }
    out
}
