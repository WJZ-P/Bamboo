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
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue},
    item::ItemStack,
    packet::play::GameMode,
};
use mc_bamboo_world::{MIN_Y, flat, poi::PoiStore, store::ChunkStore};

use super::{BlockChanges, MAX_Y};
use crate::{
    container::{ContainerEntry, ContainerKind},
    inbound::PlayerId,
    player::Player,
    tick_levels::TickLevels,
};

/// 换一个新的方块变更版本戳（`wrapping_add` 保证永不溢出 panic）。
///
/// Bump the block-change stamp (`wrapping_add` can never panic on overflow).
fn bump_block_change_stamp(stamp: &std::cell::Cell<u64>) {
    stamp.set(stamp.get().wrapping_add(1));
}

/// 一次方块操作借用的 [`Region`](crate::region::Region) 字段。
pub(crate) struct RegionLevel<'a> {
    pub(crate) chunks: &'a mut ChunkStore,
    /// POI 表和 `chunks` 平级：方块写入时同刻登记 / 注销；查询走这一份。
    pub(crate) poi: &'a mut PoiStore,
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
    /// `Level.getSkyDarken()`：这一刻由全局快照算好的天色变暗值。
    pub(crate) sky_darken: i32,
    /// 村民 / 幼年村民日程活动快照，和 `sky_darken` 一同从 `GlobalView` 读入。
    pub(crate) villager_activity: mc_bamboo_blocks::entity::mob::brain::Activity,
    pub(crate) baby_villager_activity: mc_bamboo_blocks::entity::mob::brain::Activity,
    /// `getOverworldClockTime()`：主世界时钟的整数刻，`MONSTERS_BURN` 和区域难度读它。
    pub(crate) clock_ticks: i64,
    /// `WorldGenLevel.getSeed()`：世界种子，史莱姆区块读它。
    ///
    /// `WorldGenLevel.getSeed()`: the world seed, read for slime chunks.
    pub(crate) world_seed: i64,
    /// 一局的世界生成参数，结构刷怪覆盖按种子重算结构起点时用。
    ///
    /// This session's world-generation parameters, used to recompute structure starts
    /// for the structure spawn override.
    pub(crate) world_gen: &'a crate::world_options::WorldGenConfig,
    /// 按岛的结构刷怪索引，`Level::structure_spawn_override` 只有 `&self`，
    /// 所以用 `RefCell` 在第一次查询时填进这一区块的拍平行。
    ///
    /// Per-island structure spawn index; `Level::structure_spawn_override` only
    /// gets `&self`, so a `RefCell` fills the chunk's flattened rows on first query.
    pub(crate) structure_spawn_index:
        &'a std::cell::RefCell<crate::structure_spawn::StructureSpawnIndex>,
    /// 这一岛的方块变更版本戳，写方块 / 改方块实体时加一。
    ///
    /// This island's block-change stamp, bumped on block and block-entity writes.
    pub(crate) block_change_stamp: &'a std::cell::Cell<u64>,
    /// `Level.getSeaLevel()`：这一维度的海平面。超平坦是 -63，其余是 63。
    ///
    /// `Level.getSeaLevel()` for this dimension. -63 for superflat and 63 otherwise.
    pub(crate) sea_level: i32,
    /// 计分板：玩家和生物的队伍（`TargetGoal` 的同队判断）。
    pub(crate) scoreboard: &'a crate::scoreboard::Scoreboard,
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
    /// 精确驱动的实体循环额外借出的玩家那一半；有它时生物打玩家当场落地，
    /// 没有时（方块逻辑、并行驱动的落地阶段）记成延后的效果。
    pub(crate) player_side: Option<PlayerSide<'a>>,
}

/// [`RegionLevel::player_side`]：玩家受伤链路在玩家表之外要用的连接、音效种子和规则。
pub(crate) struct PlayerSide<'a> {
    pub(crate) links: &'a mut crate::links::Links,
    pub(crate) sound_seeds: &'a LevelRandom,
    pub(crate) rules: &'a crate::gamerules::GameRuleSet,
    /// 精确驱动的实体循环把受伤产生的进度事件也攒回区域队列。
    pub(crate) pending_triggers: &'a mut Vec<crate::advancement_triggers::PendingEvent>,
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

mod farm_loot;

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

    fn chunk_is_loaded(&self, chunk: ChunkPos) -> bool {
        self.chunks.is_loaded(chunk)
    }

    /// 官方 `PoiManager.getInRange(#bee_home, center, radius, ANY)`：`beehive` /
    /// `bee_nest` 两类 POI 的全部位置，保持官方遍历序。精确驱动读的是本岛
    /// `PoiStore` 的当前状态（放 / 挖蜂巢时同刻登记）。
    ///
    /// Vanilla `PoiManager.getInRange(#bee_home, center, radius, ANY)`: all
    /// `beehive` / `bee_nest` POIs in official traversal order, read from the
    /// island's live `PoiStore`.
    fn beehives_near(&self, center: BlockPos, radius: i32) -> Vec<BlockPos> {
        crate::poi_query::bee_homes_in_range(&*self.poi, center, radius)
            .into_iter()
            .map(|(_, pos)| pos)
            .collect()
    }

    /// 本岛的玩家。官方 `ServerLevel.players()` 不过滤旁观者和死亡状态，
    /// 具体调用方各自用 `NO_SPECTATORS` / `TargetingConditions` 过滤。
    ///
    /// This island's players. `ServerLevel.players()` does not filter
    /// spectators or dead players; each caller applies its own predicates.
    fn for_each_player(&self, visit: &mut dyn FnMut(&mc_bamboo_blocks::entity::ctx::PlayerView)) {
        for (id, player) in self.players.iter() {
            visit(&player.view_in(*id, self.scoreboard));
        }
    }

    /// `LivingEntity.isSleeping()`：玩家 `sleep` 非空就是在床上。
    fn player_sleeping(&self, uid: mc_bamboo_blocks::entity::EntityUid) -> bool {
        self.players
            .get(&PlayerId(uid))
            .is_some_and(|player| player.sleep.is_some())
    }

    /// `Mob.equip(EquipmentTable)`：按装备表给一只还没进世界的生物套上装备。
    ///
    /// 官方先给八个槽设掉落几率（`slot_drop_chances`，缺省 0，也接受一个浮点值表示
    /// 所有槽同一个值），再滚战利品表，每件按 `equippable` 组件的槽位走
    /// `equipItemIfPossible`：槽位能穿才套，套上后 `setGuaranteedDrop` 把掉落几率设成
    /// 2.0（必掉）。新生的生物槽位是空的，`canReplaceCurrentItem` 的盔甲 / 武器比较和
    /// 「掉出已有装备」的分支走不到。
    fn equip_mob(&mut self, entity: &mut mc_bamboo_blocks::entity::Entity, equipment: &Tag) {
        use mc_bamboo_blocks::entity::mob::equipment::{GUARANTEED_DROP_CHANCE, SLOTS, slot_name};
        use mc_bamboo_protocol::packet::play::EquipmentSlot;

        let Tag::Compound(values) = equipment else {
            return;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
        };
        let Some(Tag::String(loot_table)) = get("loot_table") else {
            return;
        };
        let mut chances = std::collections::BTreeMap::new();
        match get("slot_drop_chances") {
            Some(Tag::Compound(entries)) => {
                for (name, value) in entries {
                    if let Ok(chance) = mc_bamboo_blocks::archive::tag::java_f32(value) {
                        chances.insert(name.clone(), chance);
                    }
                }
            }
            // `DROP_CHANCES_CODEC` 也接受一个浮点：所有槽同一个值。
            Some(value) => {
                if let Ok(chance) = mc_bamboo_blocks::archive::tag::java_f32(value) {
                    for slot in SLOTS {
                        chances.insert(slot_name(slot).to_owned(), chance);
                    }
                }
            }
            None => {}
        }
        let Some(mob_type) = entity.kind.mob().map(|mob| mob.mob_type) else {
            return;
        };
        let origin =
            mc_bamboo_loot::Position::new(entity.position.x, entity.position.y, entity.position.z);
        let mob = entity.kind.mob_mut().expect("mob type checked above");
        for slot in SLOTS {
            mob.equipment
                .set_drop_chance(slot, chances.get(slot_name(slot)).copied().unwrap_or(0.0));
        }
        let Ok(table) = mc_bamboo_loot::LootTable::from_name(loot_table) else {
            return;
        };
        let mut params = mc_bamboo_loot::LootParams::new(0.0);
        params.this_entity = Some(mc_bamboo_loot::EntityView::new(mob_type.entity_type()));
        params.origin = Some(origin);
        let Ok(stacks) = table.get_random_items(&params, self.random.source()) else {
            return;
        };
        for stack in stacks {
            let stack = crate::loot_stacks::loot_stack(&stack);
            let slot_id = mc_bamboo_recipes::equipment_slot_for_item(&stack);
            if !mc_bamboo_recipes::is_equippable_in_slot(&stack, slot_id) {
                continue;
            }
            let slot = SLOTS
                .get(slot_id as usize)
                .copied()
                .unwrap_or(EquipmentSlot::MainHand);
            mob.equipment.set(slot, stack);
            // `setItemSlotAndDropWhenKilled` 里的 `setGuaranteedDrop`。
            mob.equipment.set_drop_chance(slot, GUARANTEED_DROP_CHANCE);
        }
    }

    fn trial_spawner_players(
        &self,
        pos: BlockPos,
        range: f64,
    ) -> Vec<mc_bamboo_blocks::level::TrialSpawnerPlayer> {
        use mc_bamboo_data::registries::mob_effect;
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        let squared = range * range;
        let mut out = Vec::new();
        for player in self.players.values() {
            let position = player.position();
            if position.distance_to_sqr(center) > squared {
                continue;
            }
            let eye = Vec3::new(position.x, position.y + player.eye_height(), position.z);
            out.push(mc_bamboo_blocks::level::TrialSpawnerPlayer {
                uuid: *player.uuid.as_bytes(),
                trial_omen: player.effects.has(mob_effect::TRIAL_OMEN),
                bad_omen_amplifier: player.effects.amplifier(mob_effect::BAD_OMEN),
                eye_position: eye,
            });
        }
        out
    }

    fn trial_spawner_mob_untracked(&self, uuid: [u8; 16], pos: BlockPos) -> bool {
        use mc_bamboo_blocks::entity::KindPublic;
        let Some(public) = self
            .entities
            .iter_public()
            .find(|public| match &public.kind {
                KindPublic::Mob(mob) => mob.uuid == uuid,
                _ => false,
            })
        else {
            return true;
        };
        if public.removed {
            return true;
        }
        // `blockPosition().distSqr(pos) > MAX_MOB_TRACKING_DISTANCE_SQR`（47²）。
        let mob = public.block_position();
        let dx = i64::from(mob.x - pos.x);
        let dy = i64::from(mob.y - pos.y);
        let dz = i64::from(mob.z - pos.z);
        dx * dx + dy * dy + dz * dz > 2_209
    }

    fn trial_spawner_discard_mobs(&mut self, mobs: &[[u8; 16]]) {
        use mc_bamboo_blocks::entity::KindPublic;
        let doomed: Vec<mc_bamboo_blocks::entity::EntityUid> = self
            .entities
            .iter_public()
            .filter(|public| match &public.kind {
                KindPublic::Mob(mob) => mobs.contains(&mob.uuid),
                _ => false,
            })
            .map(|public| public.uid)
            .collect();
        for uid in doomed {
            let _ = mc_bamboo_blocks::level::LevelEntities::discard_entity(self, uid);
        }
    }

    fn trial_spawner_eject_reward(&mut self, pos: BlockPos, loot_table: &str) -> bool {
        let Ok(table) = mc_bamboo_loot::LootTable::from_name(loot_table) else {
            return false;
        };
        let params = mc_bamboo_loot::LootParams::new(0.0);
        let Ok(stacks) = table.get_random_items(&params, self.random.source()) else {
            return false;
        };
        if stacks.is_empty() {
            return false;
        }
        for stack in stacks {
            let stack = crate::loot_stacks::loot_stack(&stack);
            // `DefaultDispenseItemBehavior.spawnItem(level, stack, 2, UP, atBottomCenterOf(pos).relative(UP, 1.2))`。
            let position = Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 1.2 - 0.125,
                f64::from(pos.z) + 0.5,
            );
            let random = self.random.source();
            // `spawnItem` 先取 `g = nextDouble * 0.1 + 0.2`，UP 方向 stepX/stepZ 是 0，
            // 这个值用不上但取数仍要发生。
            let _g = random.next_double() * 0.1 + 0.2;
            let spread = 0.017_227_5 * 2.0;
            let velocity = Vec3::new(
                random.triangle(0.0, spread),
                random.triangle(0.2, spread),
                random.triangle(0.0, spread),
            );
            let uid = mc_bamboo_blocks::level::LevelEntities::spawn_entity(
                self,
                mc_bamboo_blocks::entity::Entity::ground_item(position, stack),
            );
            mc_bamboo_blocks::level::LevelEntities::with_just_spawned(self, uid, &mut |entity| {
                entity.delta = velocity;
            });
        }
        true
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
        if written {
            // `ServerLevel.updatePOIOnBlockStateChange`：旧 / 新状态对应的 POI 类型
            // 相等就什么都不做，否则 remove + add。官方丢进主线程队列异步执行，
            // Bamboo 单世界线程、多岛并行，这里同刻顺序应用（文档里记的已知差异）。
            let old = previous.map_or(mc_bamboo_data::blocks::VOID_AIR, |old| old.0);
            self.poi.on_block_state_change(pos, old, state.0);
        }
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
        if written {
            bump_block_change_stamp(self.block_change_stamp);
        }
        written
    }

    fn block_change_stamp(&self) -> u64 {
        // 方块写和方块实体形状变化各有一份版本戳，加在一起就是岛级的戳。
        // Block writes and block-entity shape changes each have a stamp; their
        // sum is the island-level stamp.
        self.block_change_stamp
            .get()
            .wrapping_add(self.block_entities.shape_stamp())
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

    fn sea_level(&self) -> i32 {
        self.sea_level
    }

    fn enchant_mob_spawn_equipment(&self, stack: &mut ItemStack, special: f32) {
        self.enchant_mob_spawn_equipment_from(stack, special, self.random().source());
    }

    fn enchant_mob_spawn_equipment_from(
        &self,
        stack: &mut ItemStack,
        special: f32,
        random: &dyn mc_bamboo_blocks::random::RandomSource,
    ) {
        use mc_bamboo_protocol::components::{ComponentValue, PatchEntry};
        let levels =
            mc_bamboo_loot::mob_spawn_enchantments(&game_stack_to_loot(stack), special, random);
        if !levels.is_empty() {
            let mut patch = stack.patch();
            patch.put(
                kinds::ENCHANTMENTS,
                PatchEntry::Set(ComponentValue::Enchantments(levels)),
            );
            (stack.added, stack.removed) = patch.to_opaque();
        }
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

    /// `getSkyDarken()`：权威副本在 `GlobalView`，精确模式的 `World` 和
    /// 并行的 `SourceView` 都读这一份。
    fn sky_darken(&self) -> i32 {
        self.sky_darken
    }

    fn villager_activity(&self) -> mc_bamboo_blocks::entity::mob::brain::Activity {
        self.villager_activity
    }

    fn baby_villager_activity(&self) -> mc_bamboo_blocks::entity::mob::brain::Activity {
        self.baby_villager_activity
    }

    /// `getDifficulty().getId()`：权威副本在 `GlobalView`，这里读每刻发布的快照。
    fn difficulty_id(&self) -> i32 {
        self.difficulty as i32
    }

    fn clock_ticks(&self) -> i64 {
        self.clock_ticks
    }

    fn world_seed(&self) -> i64 {
        self.world_seed
    }

    /// `ChunkGenerator.getMobsAt` 的结构分支：位置落在结构覆盖盒里就用结构的表。
    ///
    /// The structure branch of `ChunkGenerator.getMobsAt`: use the structure's list
    /// inside an override box.
    fn structure_spawn_override(
        &self,
        pos: BlockPos,
        category: mc_bamboo_data::spawning::MobCategory,
    ) -> Option<&'static [mc_bamboo_data::spawning::MobSpawnerDef]> {
        crate::structure_spawn::override_at_indexed(
            self.world_gen,
            self.structure_spawn_index,
            pos,
            category,
        )
    }

    /// `EnvironmentAttributes.CAN_PILLAGER_PATROL_SPAWN`：群系属性（蘑菇岛关掉）叠加
    /// `EARLY_GAME` 时间线（主世界时钟 120000 刻之前不刷巡逻队）。
    ///
    /// `EnvironmentAttributes.CAN_PILLAGER_PATROL_SPAWN`: the biome attribute (mushroom
    /// fields disables it) combined with the `EARLY_GAME` timeline (no patrols before
    /// overworld-clock tick 120000).
    fn can_pillager_patrol_spawn(&self, pos: BlockPos) -> bool {
        let biome = self.chunks.biome_at(pos).unwrap_or(0) as i32;
        mc_bamboo_blocks::entity::mob::patrol::can_patrol_spawn_at(self.clock_ticks, biome)
    }

    /// `ServerLevel.hasChunksAt(x - margin, z - margin, x + margin, z + margin)`。
    ///
    /// `ServerLevel.hasChunksAt(...)` over the patrol spawn margin.
    fn patrol_chunks_loaded(&self, center: BlockPos, margin: i32) -> bool {
        let min_x = (center.x - margin) >> 4;
        let max_x = (center.x + margin) >> 4;
        let min_z = (center.z - margin) >> 4;
        let max_z = (center.z + margin) >> 4;
        (min_x..=max_x).all(|x| {
            (min_z..=max_z).all(|z| self.chunks.is_loaded(mc_bamboo_core::ChunkPos::new(x, z)))
        })
    }

    fn entity_team(&self, uuid: &[u8; 16]) -> Option<u32> {
        self.scoreboard.entity_team_index(uuid)
    }

    fn inhabited_time(&self, chunk: mc_bamboo_core::ChunkPos) -> i64 {
        self.chunks.inhabited_time(chunk)
    }

    fn poi_type_at(&self, pos: BlockPos) -> Option<u8> {
        self.poi.get_type(pos)
    }

    fn poi_count_in_range(&self, center: BlockPos, radius: i32, kind: u8, occupancy: u8) -> usize {
        let Some(occupancy) = crate::poi_query::occupancy(occupancy) else {
            return 0;
        };
        self.poi
            .get_count_in_range(|found| found == kind, center, radius, occupancy)
    }

    fn poi_find_closest(
        &self,
        center: BlockPos,
        radius: i32,
        kind: u8,
        occupancy: u8,
        position_filter: &dyn Fn(BlockPos) -> bool,
    ) -> Option<BlockPos> {
        let occupancy = crate::poi_query::occupancy(occupancy)?;
        self.poi.find_closest(
            |found| found == kind,
            position_filter,
            center,
            radius,
            occupancy,
        )
    }

    fn poi_sections_to_village(&self, pos: BlockPos) -> i32 {
        self.poi
            .sections_to_village(mc_bamboo_world::light::SectionPos::of(pos))
    }

    fn poi_is_close_to_village(&self, pos: BlockPos, sections: i32) -> bool {
        self.poi.is_close_to_village(pos, sections)
    }

    /// 精确驱动里生物打玩家：拼出和区域入口同一组借用，当场走玩家受伤的正式入口。
    fn hurt_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        source: mc_bamboo_blocks::entity::mob::DamageSource,
        amount: f32,
        attacker: Option<&mc_bamboo_blocks::entity::EntityPublic>,
    ) -> Option<mc_bamboo_blocks::PlayerHurtNow> {
        let side = self.player_side.as_mut()?;
        let mut parts = crate::player_hurt::HurtParts {
            players: &mut *self.players,
            links: &mut *side.links,
            sound_seeds: side.sound_seeds,
            entities: &*self.entities,
            difficulty: self.difficulty as i32,
            rules: side.rules,
            attacker,
            defer_health: true,
            pending_triggers: Some(side.pending_triggers),
        };
        let (hurt, death_drops) =
            crate::player_hurt::hurt_player_by_mob(&mut parts, PlayerId(target), source, amount);
        Some(mc_bamboo_blocks::PlayerHurtNow { hurt, death_drops })
    }

    /// 精确驱动里山羊把玩家撞开：和受伤入口同一组借用，当场改速度、发速度包。
    ///
    /// A goat ramming a player in the precise driver: same borrows as the hurt hook, the
    /// velocity changes and is sent at once.
    fn knock_back_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        strength: f64,
        direction: (f64, f64),
    ) -> bool {
        let Some(side) = self.player_side.as_mut() else {
            return false;
        };
        let mut parts = crate::player_hurt::HurtParts {
            players: &mut *self.players,
            links: &mut *side.links,
            sound_seeds: side.sound_seeds,
            entities: &*self.entities,
            difficulty: self.difficulty as i32,
            rules: side.rules,
            attacker: None,
            defer_health: true,
            pending_triggers: Some(side.pending_triggers),
        };
        crate::player_hurt::knock_back_player(&mut parts, PlayerId(target), strength, direction);
        true
    }

    fn ignite_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        ticks: i32,
    ) -> bool {
        if self.player_side.is_none() {
            return false;
        }
        if let Some(player) = self.players.get_mut(&PlayerId(target)) {
            crate::player_environment::ignite_player(player, ticks);
        }
        true
    }

    fn give_player_effect_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        instance: &mc_bamboo_blocks::entity::mob::effects::EffectInstance,
    ) -> bool {
        let Some(side) = self.player_side.as_mut() else {
            return false;
        };
        if mc_bamboo_blocks::entity::mob::effects::is_instantaneous(instance.effect) {
            return false;
        }
        crate::player_effects::give_in(
            self.players,
            side.links,
            PlayerId(target),
            instance.clone(),
        );
        true
    }

    fn heal_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        amount: f32,
    ) -> bool {
        if self.player_side.is_none() {
            return false;
        }
        if let Some(player) = self.players.get_mut(&PlayerId(target))
            && player.health.health > 0.0
        {
            let max = crate::player_effects::max_health(player);
            player.health.health = (player.health.health + amount).min(max);
        }
        true
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`：读区块里算好的真实高度图。
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        self.chunks
            .heightmap_y(pos)
            .unwrap_or(mc_bamboo_world::MIN_Y - 1)
    }

    /// `getHeight(WORLD_SURFACE, pos)`：读区块里算好的世界表面高度图。
    ///
    /// `getHeight(WORLD_SURFACE, pos)`: the chunk's `WORLD_SURFACE` heightmap.
    fn world_surface_y(&self, pos: BlockPos) -> i32 {
        self.chunks
            .world_surface_y(pos)
            .unwrap_or(mc_bamboo_world::MIN_Y)
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
            crate::block_drops::ExplodedBlock {
                state,
                pos,
                block_entity,
                explosion_radius: None,
            },
        )
    }

    /// 生物爆炸在 `mobExplosionDropDecay` 下的 `getDrops`：带爆炸半径。
    ///
    /// `getDrops` for a mob explosion under `mobExplosionDropDecay`, with the radius.
    fn explosion_block_drops_decayed(
        &mut self,
        state: BlockState,
        pos: BlockPos,
        radius: f32,
    ) -> Vec<ItemStack> {
        let block_entity = crate::block_drops::collect_block_entity(self.containers, pos);
        crate::block_drops::explosion_loot(
            self.chunks,
            self.loot_sequences,
            self.random.source(),
            self.block_weather,
            crate::block_drops::ExplodedBlock {
                state,
                pos,
                block_entity,
                explosion_radius: Some(radius),
            },
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
            crate::block_drops::ExplodedBlock {
                state,
                pos,
                block_entity,
                explosion_radius: None,
            },
        )
    }

    /// `LivingEntity.dropFromLootTable`：牛死时的战利品表。
    /// `on_fire` 决定牛肉是不是熟的，攻击者的主手附魔决定抢夺和熔炼加成。
    ///
    /// `LivingEntity.dropFromLootTable` distinguishes the fatal damage source
    /// from the remembered player who supplies kill credit.
    fn mob_loot_drops(
        &mut self,
        loot: mc_bamboo_blocks::entity::mob::MobDeathLoot,
        origin: Vec3,
    ) -> Vec<ItemStack> {
        use mc_bamboo_blocks::entity::KindPublic;
        let source = loot.source;
        let actor = |uid| {
            if let Some(player) = self.players.get(&PlayerId(uid)) {
                return Some(death_loot_player(player));
            }
            if let Some(entity) = self.entities.public_at(uid) {
                let mut view = mc_bamboo_loot::EntityView::new(entity.kind.entity_type());
                view.position = Some(loot_position(entity.position));
                if let KindPublic::Mob(mob) = &entity.kind {
                    view.is_living = true;
                    view.equipment[mc_bamboo_loot::EquipmentSlot::Mainhand.index()] = mob
                        .equipment
                        .iter()
                        .find(|(slot, _)| {
                            *slot == mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand
                        })
                        .map(|(_, stack)| game_stack_to_loot(stack));
                }
                return Some(view);
            }
            // 命中的箭正在落地命令中，暂时离开实体表。
            // An impacting arrow can be temporarily absent from the entity store.
            (source.direct == Some(uid) && source.projectile_motion.is_some())
                .then(|| mc_bamboo_loot::EntityView::new(entity_type::ARROW))
        };
        // 打人的生物正在 tick 时不在实体表里，按伤害来源带的类型补一个。
        // A mob attacking mid-tick is not in the table; rebuild it from the source's type.
        let typed = |found: Option<mc_bamboo_loot::EntityView>| {
            found.or_else(|| {
                source.attacker_type.map(|kind| {
                    let mut view = mc_bamboo_loot::EntityView::new(kind);
                    view.is_living = true;
                    view
                })
            })
        };
        let attacker = typed(source.attacker.and_then(actor));
        let direct = if source.direct == source.attacker {
            typed(source.direct.and_then(actor))
        } else {
            source.direct.and_then(actor)
        };
        let last_player = match loot.recent_player {
            Some(uuid) => self
                .players
                .values()
                .find(|player| player.uuid.as_bytes() == &uuid),
            None => loot
                .recent_player_id
                .and_then(|uid| self.players.get(&PlayerId(uid))),
        }
        .map(death_loot_player);
        let mut victim = mc_bamboo_loot::EntityView::new(loot.victim.entity_type());
        victim.is_living = true;
        victim.is_on_fire = loot.on_fire;
        victim.position = Some(loot_position(origin));
        farm_loot::describe(&mut victim, loot.victim, loot.wool, None);
        if loot.victim == mc_bamboo_blocks::entity::mob::MobType::Slime {
            victim.cube_mob_size = Some(loot.cube_size);
        }
        // `type_specific/raider`：没有袭击，`has_raid` 恒假；队长由生物一侧算好带过来。
        // `type_specific/raider`: no raids, so `has_raid` stays false; the mob side
        // supplies the captain flag.
        victim.is_raider = loot.victim.is_raider();
        victim.is_captain = loot.captain;
        let view = crate::block_drops::ChunkLevel {
            chunks: self.chunks,
            weather: self.block_weather,
        };
        let params = mc_bamboo_loot::EntityDeathParams {
            entity: victim,
            origin: loot_position(origin),
            damage_source: mc_bamboo_loot::DamageSourceView {
                damage_type: mc_bamboo_data::damage_type_id(source.kind.name()).unwrap_or_default(),
                direct_entity: direct.clone().map(Box::new),
                source_entity: attacker.clone().map(Box::new),
                is_direct: source.direct == source.attacker,
            },
            attacking_entity: attacker,
            direct_attacking_entity: direct,
            last_damage_player: last_player,
            luck: 0.0,
            level: Some(&view),
        };
        let mut stacks = match mc_bamboo_loot::entity_death_drops(
            loot.victim.loot_table(),
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
        };
        // `Enderman.dropCustomDeathLoot`：搬着的方块用一把带精准采集的假钻石斧查方块表。
        // `Enderman.dropCustomDeathLoot`: the carried block's table is rolled with a fake
        // silk-touch diamond axe.
        if let Some(carried) = loot.carried {
            let mut tool = mc_bamboo_loot::LootStack::new(mc_bamboo_data::items::DIAMOND_AXE, 1);
            if let Some(silk_touch) = mc_bamboo_loot::enchantment_id("minecraft:silk_touch") {
                tool.enchant(silk_touch, 1);
            }
            let at = mc_bamboo_core::BlockPos::containing(origin);
            let view = crate::block_drops::ChunkLevel {
                chunks: self.chunks,
                weather: self.block_weather,
            };
            let block = mc_bamboo_loot::BlockDropParams {
                state: carried,
                pos: (at.x, at.y, at.z),
                level: &view,
                block_entity: None,
                breaker: None,
                tool: Some(tool),
                explosion_radius: None,
                luck: 0.0,
            };
            match mc_bamboo_loot::block_drops(
                &block,
                Some(self.loot_sequences),
                self.random.source(),
            ) {
                Ok(drops) => stacks.extend(drops.iter().map(crate::loot_stacks::loot_stack)),
                Err(error) => {
                    tracing::debug!(%error, "skipping the carried block's drops");
                }
            }
        }
        stacks
    }

    fn entity_loot_drops(
        &mut self,
        request: mc_bamboo_blocks::entity::mob::EntityLootRequest,
    ) -> Vec<ItemStack> {
        farm_loot::entity_loot_drops(self, request)
    }

    fn is_halloween(&self) -> bool {
        use chrono::Datelike;
        let date = chrono::Local::now().date_naive();
        date.month() == 10 && date.day() == 31
    }

    fn player_mob_attack(
        &self,
        uid: i32,
    ) -> Option<mc_bamboo_blocks::entity::mob::PlayerMobAttack> {
        let player = self.players.get(&PlayerId(uid))?;
        let held = player.inventory.held(mc_bamboo_core::Hand::Main);
        let patch = held.patch();
        let enchantments = mc_bamboo_protocol::components::defaults::resolve(
            held.item,
            &patch,
            kinds::ENCHANTMENTS,
        );
        let looting = match (
            mc_bamboo_loot::enchantment_id("minecraft:looting"),
            enchantments,
        ) {
            (Some(id), Some(ComponentValue::Enchantments(levels))) => levels
                .iter()
                .find_map(|&(kind, level)| (kind == id).then_some(level))
                .unwrap_or(0),
            _ => 0,
        };
        Some(mc_bamboo_blocks::entity::mob::PlayerMobAttack {
            uuid: *player.uuid.as_bytes(),
            looting,
        })
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
fn loot_position(pos: Vec3) -> mc_bamboo_loot::Position {
    mc_bamboo_loot::Position::new(pos.x, pos.y, pos.z)
}

fn death_loot_player(player: &Player) -> mc_bamboo_loot::EntityView {
    let mut view = mc_bamboo_loot::EntityView::new(entity_type::PLAYER);
    view.is_living = true;
    view.position = Some(loot_position(player.position()));
    view.equipment[mc_bamboo_loot::EquipmentSlot::Mainhand.index()] = Some(game_stack_to_loot(
        player.inventory.held(mc_bamboo_core::Hand::Main),
    ));
    view
}

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

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::{
        World,
        entity::{
            Entity, KindPublic,
            mob::{DamageKind, DamageSource, MobType},
        },
    };
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

    use crate::region::Region;
    use mc_bamboo_core::Vec3;

    #[test]
    fn indirect_skeleton_death_uses_the_causing_players_current_looting() {
        use crate::{inbound::PlayerId, player::Player};
        use mc_bamboo_core::Hand;
        use mc_bamboo_protocol::{
            components::{ComponentValue, PatchEntry},
            packet::play::{ClientInformation, GameMode},
        };
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1025);
        let uuid = uuid::Uuid::from_u128(53);
        let position = Vec3::new(2.5, 64.0, 0.5);
        let mut player = Player::new(
            id,
            uuid,
            "Archer".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            position,
        );
        let mut sword = ItemStack::new(items::DIAMOND_SWORD, 1);
        let mut patch = sword.patch();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(
                mc_bamboo_loot::enchantment_id("minecraft:looting").unwrap(),
                3,
            )])),
        );
        (sword.added, sword.removed) = patch.to_opaque();
        let hand = player.inventory.hand_slot(Hand::Main);
        player.inventory.slots[hand] = sword;
        region.insert_player(player);
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let mut skeleton = world.prepare_entity(Entity::by_mob_type(
            MobType::Skeleton,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        skeleton.random = mc_bamboo_blocks::entity::JavaRandom::new(20261173);
        skeleton.with_mob(|_, mob| {
            mob.equipment
                .set(EquipmentSlot::MainHand, ItemStack::new(items::BOW, 1))
        });
        let uid = world.add_prepared_entity(skeleton);
        let mut source = DamageSource::player(id.0, position, false);
        source.kind = DamageKind::Arrow;
        source.direct = Some(2000);
        source.projectile_motion = Some(Vec3::new(1.0, 0.0, 0.0));
        assert!(world.hurt_mob(uid, source, 100.0));
        let entity = world.entity(uid).unwrap();
        let mob = entity.kind.mob().unwrap();
        assert_eq!(mob.last_hurt_by_player_ref, Some(*uuid.as_bytes()));
        assert!(mob.equipment.get(EquipmentSlot::MainHand).is_empty());
    }

    #[test]
    fn skeleton_death_uses_the_region_loot_table_and_drops_preserved_gear() {
        let mut region = Region::new(2, 20, false);
        let mut skeleton = Entity::by_mob_type(MobType::Skeleton, Vec3::new(0.5, 64.0, 0.5));
        skeleton.with_mob(|_, mob| {
            mob.equipment
                .set(EquipmentSlot::MainHand, ItemStack::new(items::BOW, 1));
            mob.equipment.set_drop_chance(EquipmentSlot::MainHand, 2.0);
        });
        // 世界种子 0 的首组战利品可以为空；连续击杀推进同一条表随机序列。
        // Seed zero can produce empty initial loot; repeated kills advance the same table stream.
        let mut saw_loot = false;
        for _ in 0..4 {
            {
                let (mut level, ticks) = region.block_level();
                let mut world = World::new(&mut level, ticks);
                let uid = world.spawn_entity(skeleton.clone());
                assert!(world.hurt_mob(uid, DamageSource::environment(DamageKind::Fall), 100.0));
            }
            let spawned = region.entities.take_spawn_snapshots();
            let items: Vec<_> = spawned
                .iter()
                .filter_map(|entity| match &entity.kind {
                    KindPublic::Item { stack, .. } => Some(stack.item),
                    KindPublic::ExperienceOrb { .. } => panic!("environmental death awarded XP"),
                    _ => None,
                })
                .collect();
            let (last, loot) = items.split_last().expect("preserved bow drop");
            assert_eq!(*last, items::BOW);
            assert!(
                loot.iter()
                    .all(|&item| matches!(item, items::ARROW | items::BONE))
            );
            saw_loot |= !loot.is_empty();
        }
        assert!(saw_loot);
    }
}
