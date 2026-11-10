//! 这个 crate 和世界存储的边界。

use std::collections::{HashMap, HashSet};

use mc_bamboo_core::{
    BlockPos, ChunkPos, Hand, LightLayer,
    sign::{SignContent, SignTextSlot},
};
use mc_bamboo_data::SoundSource;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::{BlockEntities, BlockEntity, BlockEvent, BlockEvents},
    container::Container,
    entity::Entities,
    gamerules::BlockGameRules,
    hopper::HopperEntity,
    random::LevelRandom,
    shape::Aabb,
    state::{BlockState, NoteBlockInstrument},
};

/// 发射器前方一格里、可能被穿上装备的生物。
///
/// A living entity in front of a dispenser that might receive equipment.
///
/// `equipment` 的下标是组件里的 `equipment_slot`：0 主手、1 脚、2 腿、3 胸、4 头、5 副手、
/// 6 身体、7 鞍。`None` 表示这个实体没有这一格，`Some` 空物品表示格子是空的。
///
/// `equipment` is indexed by the component `equipment_slot`: 0 mainhand, 1 feet, 2 legs,
/// 3 chest, 4 head, 5 offhand, 6 body, 7 saddle. `None` means the entity has no such slot.
/// `Some` empty stack means the slot exists and is empty.
#[derive(Clone, Debug)]
pub struct EquipTarget {
    pub entity_id: i32,
    pub bounds: Aabb,
    /// `isSpectator`。
    pub spectator: bool,
    /// `minecraft:entity_type` 的注册表 ID。
    pub entity_type: i32,
    pub equipment: [Option<ItemStack>; 8],
}

/// 方块状态装不下、跟着位置走的世界数据：方块实体、方块事件、红石记忆。
///
/// 每个方法是一次具体的读或写，位置当参数传进来。方块逻辑只能通过这些方法碰这三样东西：
/// 以后它们按区块存放，或者外面套一层记录读写的实现时，这里的签名都不用改。
///
/// 用整张表存这三样东西的世界实现 [`LevelTables`] 就自动得到这个 trait；按区块存、
/// 或者要录制读写的实现者直接实现这个 trait，不必交出一整张表的引用。
///
/// World data that block states cannot hold: block entities, block events and redstone
/// memory. Every method is one concrete read or write, with the position passed in.
/// A world backed by whole tables implements [`LevelTables`]; an implementation that
/// splits state by chunk, or records reads and writes, implements this trait directly.
pub trait LevelSideState {
    // 红石记忆：每个方法对应 `RedstoneMemory` 上的一个操作。
    fn comparator_output(&self, pos: BlockPos) -> i32;
    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32);
    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize>;
    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize);
    fn is_jukebox_playing(&self, pos: BlockPos) -> bool;
    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool);
    fn crafter_disabled(&self, pos: BlockPos) -> u16;
    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16);
    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32);
    fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)>;
    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>);
    fn forget_block_entity_memory(&mut self, pos: BlockPos);
    fn set_pending_lectern(&mut self, facts: LecternFacts);
    fn take_pending_lectern(&mut self) -> Option<LecternFacts>;
    fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts);
    fn lectern(&self, pos: BlockPos) -> Option<LecternFacts>;
    fn clear_lectern(&mut self, pos: BlockPos);
    fn forget_torch_toggles_before(&mut self, when: i64);
    fn record_torch_toggle(&mut self, pos: BlockPos, when: i64);
    fn torch_toggle_count(&self, pos: BlockPos) -> usize;

    // 方块事件。
    fn push_block_event(&mut self, event: BlockEvent);
    fn pop_block_event(&mut self) -> Option<BlockEvent>;

    // 方块实体和它们的 ticker。
    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity>;
    fn has_block_entity(&self, pos: BlockPos) -> bool;
    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>);
    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>>;
    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)>;
    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>>;
    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>);
    fn is_block_entity_taken(&self, pos: BlockPos) -> bool;
    fn begin_block_entity_tick(&mut self);
    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)>;
    fn end_block_entity_tick(&mut self);
}

/// 用一整张表存方块实体、方块事件和红石记忆的世界。
///
/// 实现它就自动得到 [`LevelSideState`]。这五个方法只有存储实现者能碰到：
/// [`Level`] 不把它当父 trait，方块逻辑拿到的 `&mut dyn Level` 够不着整张表。
///
/// A world that stores these three kinds of state in whole tables. Implementing this
/// trait provides [`LevelSideState`]. [`Level`] does not require it, so block logic
/// holding a `&mut dyn Level` cannot reach the tables.
pub trait LevelTables {
    fn block_entities_table(&self) -> &BlockEntities;
    fn block_entities_table_mut(&mut self) -> &mut BlockEntities;
    fn block_events_table_mut(&mut self) -> &mut BlockEvents;
    fn redstone_table(&self) -> &RedstoneMemory;
    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory;
}

impl<T: LevelTables + ?Sized> LevelSideState for T {
    fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.redstone_table().comparator_output(pos)
    }

    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.redstone_table_mut().set_comparator_output(pos, signal);
    }

    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.redstone_table().last_bookshelf_slot(pos)
    }

    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.redstone_table_mut().set_last_bookshelf_slot(pos, slot);
    }

    fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.redstone_table().is_jukebox_playing(pos)
    }

    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        self.redstone_table_mut().set_jukebox_playing(pos, playing);
    }

    fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.redstone_table().crafter_disabled(pos)
    }

    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        self.redstone_table_mut().set_crafter_disabled(pos, mask);
    }

    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        self.redstone_table_mut()
            .set_crafter_crafting(pos, remaining);
    }

    fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)> {
        self.redstone_table_mut().take_crafter_crafting()
    }

    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>) {
        self.redstone_table_mut().restore_crafter_crafting(entries);
    }

    fn forget_block_entity_memory(&mut self, pos: BlockPos) {
        self.redstone_table_mut().forget_block_entity(pos);
    }

    fn set_pending_lectern(&mut self, facts: LecternFacts) {
        self.redstone_table_mut().set_pending_lectern(facts);
    }

    fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        self.redstone_table_mut().take_pending_lectern()
    }

    fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        self.redstone_table_mut().set_lectern(pos, facts);
    }

    fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        self.redstone_table().lectern(pos)
    }

    fn clear_lectern(&mut self, pos: BlockPos) {
        self.redstone_table_mut().clear_lectern(pos);
    }

    fn forget_torch_toggles_before(&mut self, when: i64) {
        self.redstone_table_mut().forget_torch_toggles_before(when);
    }

    fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.redstone_table_mut().record_torch_toggle(pos, when);
    }

    fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.redstone_table().torch_toggle_count(pos)
    }

    fn push_block_event(&mut self, event: BlockEvent) {
        self.block_events_table_mut().push(event);
    }

    fn pop_block_event(&mut self) -> Option<BlockEvent> {
        self.block_events_table_mut().pop()
    }

    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity> {
        self.block_entities_table().get_dyn(pos)
    }

    fn has_block_entity(&self, pos: BlockPos) -> bool {
        self.block_entities_table().contains(pos)
    }

    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        self.block_entities_table_mut().insert(entity);
    }

    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
        self.block_entities_table_mut().remove(pos)
    }

    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
        self.block_entities_table_mut().take_at(pos)
    }

    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>> {
        self.block_entities_table_mut().take(pos, id)
    }

    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>) {
        self.block_entities_table_mut().put(pos, id, entity);
    }

    fn is_block_entity_taken(&self, pos: BlockPos) -> bool {
        self.block_entities_table().is_taken(pos)
    }

    fn begin_block_entity_tick(&mut self) {
        self.block_entities_table_mut().begin_tick();
    }

    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
        self.block_entities_table_mut().next_ticker()
    }

    fn end_block_entity_tick(&mut self) {
        self.block_entities_table_mut().end_tick();
    }
}

/// 世界的存储和世界外的一切。`mc-bamboo-game` 和 oracle 各有一个实现。
pub trait Level: LevelSideState {
    /// 世界外返回 `void_air`。
    fn block_state(&self, pos: BlockPos) -> BlockState;

    /// 写入一格，不做任何连锁。高度超出世界时什么也不做，返回 `false`。
    ///
    /// `flags` 是 [`set_block`](crate::World::set_block) 的那一组，存储只需要看
    /// [`UPDATE_CLIENTS`](crate::world::UPDATE_CLIENTS)：官方只在带这个标记时才把改动发给客户端。
    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool;

    /// 这些盒子（世界坐标）和所有挡路的实体都没有正体积重叠。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool;

    /// 世界最低的一层，`getMinY`。
    fn min_y(&self) -> i32;

    /// 世界最高的一层（含），`getMaxY`。
    fn max_y(&self) -> i32;

    /// `Level.random` 和 `randValue`，方块逻辑的随机数都从这里取。
    fn random(&self) -> &LevelRandom;

    /// `LevelChunkSection.isRandomlyTicking`：这个区块里纵坐标是 `section_y`（方块的 `y >> 4`）的区块段里
    /// 有会随机刻的方块或者流体，也就是有状态的 `is_randomly_ticking` 或者 `is_fluid_randomly_ticking` 为真。
    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool;

    /// `ChunkMap.anyPlayerCloseEnoughTo`：有不是旁观者的玩家，脚到这一格最小的角不到 `max_distance` 格。
    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool;

    /// `getBrightness(layer, pos)`：这一格的天空光或者方块光，0 到 15。
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32;

    /// `getBlockEntity(pos) instanceof SignBlockEntity` 之后读到的内容。
    ///
    /// 还没写过字的告示牌返回 [`SignContent::BLANK`]，官方也是按状态新建一个空的方块实体。
    /// 不是告示牌方块时返回 `None`。
    fn sign(&self, pos: BlockPos) -> Option<SignContent>;

    /// 改写告示牌的内容，对应 `SignBlockEntity` 里那几个字段。不是告示牌方块时返回 `false`。
    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool;

    /// `getBlockEntity(pos) instanceof SkullBlockEntity` 时它的 `getNoteBlockSound`。
    ///
    /// 官方把物品上的 `note_block_sound` 组件搬进头颅方块实体，音符盒响的时候按它找自定义
    /// 音效，见 [`Effect::NoteBlockPlayed`]。不是头颅方块实体、或者头颅没带音效时返回
    /// `None`，官方对应 `null`。
    fn note_block_sound(&self, pos: BlockPos) -> Option<String>;

    /// 这一格的方块实体这一刻要不要跑。默认要跑。
    ///
    /// 方块 oracle 的假世界只登记漏斗和移动中的活塞，熔炉和酿造台不跑。
    ///
    /// Whether this cell's block entity ticks. The default is yes.
    ///
    /// The block oracle's fake level only registers hoppers and moving pistons, so furnaces
    /// and brewing stands do not tick.
    fn runs_block_entity_tick(&self, _pos: BlockPos) -> bool {
        true
    }

    /// 这个世界里的实体。实体由世界线程拥有，见 `docs/folia.md`。
    ///
    /// Entities in this world. The world thread owns them; see `docs/folia.md`.
    fn entities(&self) -> &Entities;

    /// 见 [`Level::entities`]。
    ///
    /// See [`Level::entities`].
    fn entities_mut(&mut self) -> &mut Entities;

    /// 方块逻辑读到的游戏规则快照。默认是官方默认值。
    ///
    /// The game-rule snapshot block logic reads. The default is the vanilla defaults.
    fn block_game_rules(&self) -> BlockGameRules {
        BlockGameRules::vanilla()
    }

    /// 方块逻辑读到的天气快照。默认是晴天，权威副本在世界线程的 `Global` 上。
    ///
    /// The weather snapshot block logic reads. The default is clear weather; the
    /// authoritative copy lives on the world thread's `Global`.
    fn block_weather(&self) -> crate::weather::BlockWeather {
        crate::weather::BlockWeather::CLEAR
    }

    /// `BlockAndLightGetter.canSeeSky(pos)`：天空光是 15 就看得见天。
    ///
    /// `isRainingAt` 用得到。默认实现读上一轮算好的天空光，和官方一致。
    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.brightness(LightLayer::Sky, pos) >= 15
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`：这一列挡运动的最高方块上面一格。
    /// 整列都是空气时是世界底部下面一格。
    ///
    /// `isRainingAt` 和 `tickPrecipitation` 用得到。有真实高度图的实现应该覆盖它。
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        for y in (self.min_y()..=self.max_y()).rev() {
            let state = self.block_state(BlockPos::new(pos.x, y, pos.z));
            if state.is_tagged(mc_bamboo_data::tags::block_tags::BLOCKS_MOTION_IN_HEIGHTMAP)
                || !state.fluid().is_empty()
            {
                return y + 1;
            }
        }
        self.min_y() - 1
    }

    /// 这一格所在群系的降水条件，`Biome.getPrecipitationAt(pos, seaLevel)` 加上
    /// `Biome.warmEnoughToRain(pos, seaLevel)`。默认平原（测试用），真实实现读区块的
    /// 群系调色板再按 `mc-bamboo-worldgen` 算温度。
    fn precipitation_facts(&self, _pos: BlockPos) -> crate::weather::PrecipitationFacts {
        crate::weather::PrecipitationFacts::PLAINS
    }

    /// 这一格的物品栏，对应官方的 `getBlockContainer`（摆在方块里的那个）。
    ///
    /// 双箱已经按官方 `CompoundContainer` 的顺序合并成一张表。这一格不是容器方块时
    /// 不调用 `f`，返回 `false`。
    /// 漏斗取放物品时官方会忽略压住箱子的方块（`getContainerAt` 传 `ignoreBeingBlocked=true`），
    /// 所以这里的容器是能取到的那个，和比较器看到的可能不同。
    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 这一格自己那张物品栏里每格有多少个物品，双箱只算这一半。
    ///
    /// 官方的 `BlockEntity.preRemoveSideEffects` 掉落的是方块实体自己的 `getItems`，
    /// 所以它和 [`Level::with_container`] 不同：不合并双箱，也不管压住箱子的方块。
    /// 没有物品表时返回空表，调用方按空槽处理。
    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        let _ = pos;
        Vec::new()
    }

    /// 比较器从这一格的容器读到的模拟信号，官方 `getRedstoneSignalFromBlockEntity`。
    ///
    /// 双箱按合并后的 54 格算，和被压住读不到时是 0，见 `ChestBlock.getAnalogOutputSignal`。
    fn container_signal(&self, pos: BlockPos) -> i32 {
        let _ = pos;
        0
    }

    /// 这一格容器背后的方块位置：单箱是这一格，双箱是两半，见 `CompoundContainer.setChanged`。
    ///
    /// 和 [`Level::with_container`] 一样按能取到的那条规则算，被压住的箱子也算能取到。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        vec![pos]
    }

    /// 带着这一格的漏斗方块实体运行一段代码。这一格没有漏斗实体时不调用 `f`，返回 `false`。
    ///
    /// 实体正在 tick 时已经从表里拿出来了，这里取不到。漏斗的物品在槽位表，不在这个实体上。
    ///
    /// While the entity is ticking it is out of the map, so this cannot see it.
    /// Hopper items live in the slot table, not on the entity.
    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 雕纹书架、唱片机、饰纹陶罐和合成器的槽位表。没有这张表时不调用 `f`。
    ///
    /// Slot list for a chiseled bookshelf, jukebox, decorated pot or crafter. `f` is not called
    /// when the list is absent.
    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let _ = (pos, len, f);
        false
    }

    /// 这一格容器现在的槽位。没有容器时是空的。
    ///
    /// The container slots at this position, or an empty list when there is no container.
    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        let _ = pos;
        Vec::new()
    }

    /// 删掉这一格的容器槽位。破坏唱片机时 `setTheItem(EMPTY)` 在通知邻居之前清空。
    ///
    /// Drop the container slots at this position. Breaking a jukebox runs `setTheItem(EMPTY)`
    /// before neighbors are told.
    fn clear_container(&mut self, pos: BlockPos) {
        let _ = pos;
    }

    /// `BlockBehaviour.onExplosionHit` 里的 `state.getDrops`。
    ///
    /// TNT 的 `tntExplosionDropDecay` 默认是假，所以没有 `EXPLOSION_RADIUS`，`explosion_decay`
    /// 不取数。假世界没有战利品表时返回空，这样不会多取 `level.random`。
    ///
    /// `state.getDrops` inside `BlockBehaviour.onExplosionHit`.
    ///
    /// `tntExplosionDropDecay` defaults to false, so there is no `EXPLOSION_RADIUS` and
    /// `explosion_decay` draws nothing. A world without loot tables returns nothing, and does
    /// not draw extra `level.random` values.
    fn explosion_block_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let _ = (state, pos);
        Vec::new()
    }

    /// `Block.getDrops`，`destroyBlock` 在移除方块之前按战利品表算出的每一组。
    ///
    /// 空堆也交给调用方 `popResource`：官方对每一组都取三次位置偏移。
    /// 方块 oracle 把表换成空表，这里默认返回空，不额外取数。
    ///
    /// `Block.getDrops`, the stacks `destroyBlock` computes before removing the block.
    /// Even an empty stack reaches `popResource` in vanilla, which draws three position
    /// offsets. The block oracle empties those tables, so the default here draws nothing.
    fn block_loot_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let _ = (state, pos);
        Vec::new()
    }

    /// 官方 `RandomizableContainer.unpackLootTable` 的调用点。有战利品表的容器在
    /// 第一次被读或写之前先填好内容。默认空实现用于没有战利品表的假世界。
    ///
    /// The hook for vanilla `RandomizableContainer.unpackLootTable`. A container with a
    /// loot table is filled before its first read or write. Fake worlds leave this empty.
    fn unpack_loot_table(&mut self, _pos: BlockPos) {}

    /// 官方 `ShulkerBoxBlock.playerWillDestroy` 的创造模式分支：非空潜影盒直接
    /// 掉出带 `BlockEntity.collectComponents()` 的方块物品。默认没有这类物品。
    ///
    /// Vanilla `ShulkerBoxBlock.playerWillDestroy` in creative mode: a non-empty
    /// shulker box drops the block item with `BlockEntity.collectComponents()`.
    fn creative_container_drop(&mut self, _pos: BlockPos, _item: i32) -> Option<ItemStack> {
        None
    }

    /// `BlockState.spawnAfterBreak` 里 `tryDropExperience` 和虫蚀方块生成蠹虫
    /// 可能要取的 `level.random` 数。没有经验球和生物时实体不生成，取数仍要照官方。
    ///
    /// `DropExperienceBlock` 的 `xpRange.sample(level.random)` 和 `ExperienceOrb.award`
    /// 的拆分次数都在这里；虫蚀方块不取 `level.random`。默认没有方块需要取数。
    ///
    /// The `level.random` draws inside `BlockState.spawnAfterBreak`: ore experience and
    /// infested-block spawns. Experience orbs and silverfish are not spawned, but the
    /// draws still follow vanilla. `DropExperienceBlock` samples `xpRange` and splits
    /// the amount in `ExperienceOrb.award`; infested blocks draw none. The default draws
    /// nothing.
    fn spawn_after_break_random(&mut self, state: BlockState, pos: BlockPos) {
        let _ = (state, pos);
    }
    /// `getEntitiesOfClass(LivingEntity, box)` 能看见的生物。默认没有。
    ///
    /// Living entities visible to `getEntitiesOfClass`. None by default.
    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        Vec::new()
    }

    /// `LivingEntity.setItemSlot`。写不进这一格时返回 `None`。
    ///
    /// 返回值是玩家物品栏菜单槽，调用方用它发 `container_set_slot`。不是玩家时可以是任意非空值。
    ///
    /// `LivingEntity.setItemSlot`. `None` when the slot cannot be stored.
    ///
    /// The value is the player's inventory-menu slot, used to send `container_set_slot`.
    /// A non-player may return any `Some`.
    fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        let _ = (entity_id, component_slot, stack);
        None
    }
}

/// 红石用到的、方块状态装不下的世界数据，活得比一次操作长。
///
/// 官方把这些放在方块实体（比较器的输出、雕纹书架最近动过的格子、唱片机在不在放）
/// 和按世界分组的静态表（火把最近的开关）里。
#[derive(Debug, Default)]
pub struct RedstoneMemory {
    /// `ComparatorBlockEntity.outputSignal`。没有记录的格子是 0，和空的方块实体一样。
    comparator_outputs: HashMap<BlockPos, i32>,
    /// `ChiseledBookShelfBlockEntity.lastInteractedSlot`。没有记录的格子是 -1，也就是没动过。
    bookshelf_slots: HashMap<BlockPos, usize>,
    /// `JukeboxBlockEntity` 的 `songPlayer.isPlaying()`。唱片在容器槽位里。
    /// 场景里直接摆下的 `has_record=true` 唱机没有经过 `setTheItem`，所以没在放。
    ///
    /// `JukeboxSongPlayer.isPlaying`. The disc lives in the container slot.
    /// A scene jukebox placed with `has_record=true` never went through `setTheItem`, so it is
    /// not playing.
    jukebox_playing: HashSet<BlockPos>,
    /// `CrafterBlockEntity` 的禁用槽。位 `n` 为 1 表示第 `n` 格禁用。没有记录时全是 0。
    ///
    /// Disabled slots of a `CrafterBlockEntity`. Bit `n` set means slot `n` is disabled.
    /// Missing positions are all enabled.
    crafter_disabled: HashMap<BlockPos, u16>,
    /// `CrafterBlockEntity.craftingTicksRemaining`。只在大于 0 时记。
    ///
    /// `CrafterBlockEntity.craftingTicksRemaining`. Only recorded while positive.
    crafter_crafting: HashMap<BlockPos, i32>,
    /// `RedstoneTorchBlock.RECENT_TOGGLES` 里属于这个世界的那一项，按时间排好。
    torch_toggles: Vec<(BlockPos, i64)>,
    /// `LecternBlockEntity` 的页码。比较器只读得了这三项，书本身在游戏层。
    ///
    /// Page, page count, and whether the book has writable or written content.
    /// The item stack itself lives in the game layer.
    lecterns: HashMap<BlockPos, LecternFacts>,
    /// 这次右键即将放上的书。方块逻辑看不见物品组件，由调用方先放进来。
    ///
    /// The book about to be placed. Block logic cannot see item components,
    /// so the caller deposits the facts before the use.
    pending_lectern: Option<LecternFacts>,
}

/// 讲台比较器要的三项，对应 `LecternBlockEntity` 的 `page`、`pageCount` 和 `hasBook()`。
///
/// The three facts a lectern comparator reads: `page`, `pageCount`, and `hasBook()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LecternFacts {
    pub page: i32,
    pub page_count: i32,
    pub has_content: bool,
}

impl LecternFacts {
    /// `LecternBlockEntity.getRedstoneSignal`。
    ///
    /// `pageCount > 1` 时进度是 `page / (pageCount - 1)`，否则是 1。
    /// `Mth.floor` 先把 float 提升成 double。
    ///
    /// Progress is `page / (pageCount - 1)` when `pageCount > 1`, otherwise 1.
    /// `Mth.floor` widens the float to double before truncating.
    pub fn signal(self) -> i32 {
        let progress = if self.page_count > 1 {
            self.page as f32 / (self.page_count as f32 - 1.0_f32)
        } else {
            1.0_f32
        };
        mc_bamboo_core::mth::floor(f64::from(progress * 14.0_f32)) + i32::from(self.has_content)
    }
}

impl RedstoneMemory {
    /// `ComparatorBlockEntity.getOutputSignal`。
    pub fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.comparator_outputs.get(&pos).copied().unwrap_or(0)
    }

    /// `ComparatorBlockEntity.setOutputSignal`。
    pub fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.comparator_outputs.insert(pos, signal);
    }

    /// `ChiseledBookShelfBlockEntity.getLastInteractedSlot`，没动过时是 `None`。
    pub fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.bookshelf_slots.get(&pos).copied()
    }

    /// 放进或者拿出一本书时记下是哪一格。
    pub fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.bookshelf_slots.insert(pos, slot);
    }

    /// `JukeboxSongPlayer.isPlaying`。
    pub fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.jukebox_playing.contains(&pos)
    }

    /// `JukeboxSongPlayer.play`、`stop` 之后记下这一格在不在放。
    pub fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        if playing {
            self.jukebox_playing.insert(pos);
        } else {
            self.jukebox_playing.remove(&pos);
        }
    }

    /// `CrafterBlockEntity.isSlotDisabled` 的九个位。
    ///
    /// The nine bits behind `CrafterBlockEntity.isSlotDisabled`.
    pub fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.crafter_disabled.get(&pos).copied().unwrap_or(0)
    }

    /// 写下合成器的禁用槽。全是 0 时不留记录。
    ///
    /// Store a crafter's disabled slots. An all-zero mask is not recorded.
    pub fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        if mask == 0 {
            self.crafter_disabled.remove(&pos);
        } else {
            self.crafter_disabled.insert(pos, mask);
        }
    }

    /// `CrafterBlockEntity.setCraftingTicksRemaining`：亮起 `crafting` 时记下还剩几刻。
    ///
    /// `CrafterBlockEntity.setCraftingTicksRemaining`: record the countdown when `crafting` turns on.
    pub fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        if remaining > 0 {
            self.crafter_crafting.insert(pos, remaining);
        } else {
            self.crafter_crafting.remove(&pos);
        }
    }

    /// 取走全部倒计时，供一刻末尾统一递减。
    ///
    /// Take every countdown so the end of a tick can decrement them together.
    pub fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)> {
        std::mem::take(&mut self.crafter_crafting)
            .into_iter()
            .collect()
    }

    /// 把还没到期的倒计时放回去。
    ///
    /// Put the countdowns that have not expired back.
    pub fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>) {
        for (pos, remaining) in entries {
            if remaining > 0 {
                self.crafter_crafting.insert(pos, remaining);
            }
        }
    }

    /// 这一格换了方块，官方把方块实体删掉，下次新建的是空的。
    pub fn forget_block_entity(&mut self, pos: BlockPos) {
        self.comparator_outputs.remove(&pos);
        self.bookshelf_slots.remove(&pos);
        self.jukebox_playing.remove(&pos);
        self.crafter_disabled.remove(&pos);
        self.crafter_crafting.remove(&pos);
        self.lecterns.remove(&pos);
    }

    /// 这次交互要放上的书。方块行为在 `placeBook` 里取走。
    ///
    /// Facts for the book the current interaction is placing. `placeBook` takes them.
    pub fn set_pending_lectern(&mut self, facts: LecternFacts) {
        self.pending_lectern = Some(facts);
    }

    /// 取走上一段放进来的书。没有就返回 `None`。
    ///
    /// Takes the facts deposited for this interaction, if any.
    pub fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        self.pending_lectern.take()
    }

    /// 记下这一格讲台的页码。
    ///
    /// Stores the lectern facts at `pos`.
    pub fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        self.lecterns.insert(pos, facts);
    }

    /// 这一格讲台的页码。没有记录时是 `None`。
    ///
    /// Lectern facts at `pos`, if a book was recorded there.
    pub fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        self.lecterns.get(&pos).copied()
    }

    /// 取书或拆掉讲台时丢掉页码。
    ///
    /// Drops the lectern facts when the book is removed or the block goes away.
    pub fn clear_lectern(&mut self, pos: BlockPos) {
        self.lecterns.remove(&pos);
    }

    /// 扔掉 `when` 之前的开关记录，官方在红石火把的计划刻开头做这件事。
    pub fn forget_torch_toggles_before(&mut self, when: i64) {
        let keep = self
            .torch_toggles
            .iter()
            .position(|&(_, toggled_at)| toggled_at >= when)
            .unwrap_or(self.torch_toggles.len());
        self.torch_toggles.drain(..keep);
    }

    /// 记下一次开关。
    pub fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.torch_toggles.push((pos, when));
    }

    /// 这一格还留着几次开关记录。
    pub fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.torch_toggles
            .iter()
            .filter(|&&(toggled, _)| toggled == pos)
            .count()
    }
}

/// 一次操作留下的、方块以外的结果，由调用方发给玩家。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// `levelEvent(2001, pos, state)`。`except_initiator` 表示不用发给发起者，它自己已经预测过了。
    Destroyed {
        pos: BlockPos,
        state: BlockState,
        except_initiator: bool,
    },
    /// `levelEvent(1502, pos, 0)`：红石火把开关得太频繁，烧坏时冒的那股烟。
    TorchBurnout { pos: BlockPos },
    /// 空桶舀起了东西。游戏层按 `ItemUtils.createFilledResult` 把 `item` 交还到 `hand`。
    ///
    /// An empty bucket picked something up. The game layer runs `ItemUtils.createFilledResult`
    /// and puts `item` back into `hand`.
    FilledBucket { item: i32, hand: Hand },
    /// 满桶倒成功了。生存模式交还空桶，创造模式交还手里原来的那一堆。
    ///
    /// A full bucket emptied successfully. Survival hands back an empty bucket. Creative hands
    /// back the stack that was already in the hand.
    EmptiedBucket { hand: Hand },
    /// `levelEvent(1505, pos, 15)`：骨粉起作用的粒子，发给附近所有人。
    BoneMealUsed { pos: BlockPos },
    /// 告示牌的内容变了，`SignBlockEntity.markUpdated` 里的 `sendBlockUpdated`：
    /// 方块状态没变，但要把方块和方块实体数据重新发给追踪这个区块的玩家。
    SignChanged { pos: BlockPos },
    /// 上蜡成功的光照效果，`levelEvent(null, 3003, pos, 0)`。
    SignWaxed { pos: BlockPos },
    /// `SignBlock.openTextEdit`：给这名玩家打开编辑器，`player.openTextEdit(sign, slot)`。
    OpenSignEditor { pos: BlockPos, slot: SignTextSlot },
    /// `playSound` / `playSeededSound`：坐标乘 8 截断后发给范围内的玩家。
    Sound {
        /// `sound_event` 注册表 ID。
        sound: i32,
        source: SoundSource,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        /// 官方传的 `except` 是发起这次操作的玩家。
        except_initiator: bool,
        /// 显式给的种子。音符盒用 `Level.random.nextLong()`，其余由网络层另取。
        seed: Option<i64>,
    },
    /// `levelEvent`：客户端自己放粒子和声音。传播范围是 64 格，坐标是方块整数坐标。
    LevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
        except_initiator: bool,
    },
    /// 音符盒响了，`NoteBlock.triggerEvent`。`pitch` 是官方的 `getPitchFromNote(note)`，
    /// `seed` 是官方传给 `playSeededSound` 的那个随机数。
    ///
    /// 上面是带 `note_block_sound` 的头颅时，官方播放的是那个音效，不按乐器查表。
    /// 名字在这一次操作的自定义音效表里，[`NoteBlockPlayed::custom`] 是下标。
    NoteBlockPlayed {
        pos: BlockPos,
        instrument: NoteBlockInstrument,
        note: i32,
        pitch: f32,
        seed: i64,
        /// 自定义音效在 [`intern_custom_sound`] 那张表里的下标。`None` 用乐器表。
        custom: Option<u32>,
    },
    /// `ServerLevel.runBlockEvents` 里的 `ClientboundBlockEventPacket`：方块事件处理成功了，
    /// 把这一项发给附近 64 格内的玩家。`block` 是方块的注册表 ID，官方包里的第三个字段。
    BlockEvent {
        pos: BlockPos,
        block: usize,
        param_a: i32,
        param_b: i32,
    },
    /// `levelEvent(1500, pos, data)`：堆肥桶收进一个能堆肥的物品。
    /// `success` 为真时数据是 1（等级升了），否则是 0。客户端自己播粒子和声音。
    ComposterFill { pos: BlockPos, success: bool },
    /// 讲台翻页，`levelEvent(1043, pos, 0)`。客户端自己播翻页的声音。
    ///
    /// `LecternBlock.signalPageChange` emits `levelEvent(1043, pos, 0)`.
    /// The client plays the page-turn sound. Nearby players, including the one
    /// who turned the page, all receive it.
    PageTurned { pos: BlockPos },
    /// 发射器把一件装备写进了这名玩家的物品栏。菜单槽已经改过，调用方发 `container_set_slot`。
    ///
    /// A dispenser wrote equipment into this player's inventory. The menu slot is already
    /// updated. The caller sends `container_set_slot`.
    DispenserEquipped { entity_id: i32, menu_slot: u16 },
    /// 营火上的食物变了，`CampfireBlockEntity.getUpdateTag` 要重新发给客户端。
    ///
    /// Food on a campfire changed. `CampfireBlockEntity.getUpdateTag` has to be sent again.
    CampfireChanged { pos: BlockPos },
    /// 快捷栏上方的提示。`key` 是翻译键，参数是已经序列化好的文本。
    ///
    /// An action-bar message. `key` is the translation key and the arguments are already text.
    Overlay {
        key: &'static str,
        first: Option<&'static str>,
        second: Option<&'static str>,
    },
    /// 有权限的玩家右键了命令方块。客户端自己打开编辑界面，服务端补一份方块实体数据。
    ///
    /// A permitted player used a command block. The client opens the editor; the server sends
    /// the block-entity data.
    OpenCommandBlock { pos: BlockPos },
}

/// 自定义音效的名字。`Effect` 要能按值拷贝，名字放在这里，效果里只留下标。
///
/// 一次结算会把好几个世界的效果拼到一起，这张表不能挂在某一个世界上。
static CUSTOM_SOUNDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 记下一个 `SoundEvent.createVariableRangeEvent` 的标识符，返回下标。
pub fn intern_custom_sound(name: &str) -> u32 {
    let mut table = CUSTOM_SOUNDS.lock().expect("custom sound table");
    if let Some(index) = table.iter().position(|existing| existing == name) {
        return u32::try_from(index).expect("custom sound index");
    }
    let index = u32::try_from(table.len()).expect("custom sound index");
    table.push(name.to_owned());
    index
}

/// 按下标取回自定义音效的标识符。
pub fn custom_sound_name(index: u32) -> String {
    CUSTOM_SOUNDS
        .lock()
        .expect("custom sound table")
        .get(index as usize)
        .cloned()
        .expect("custom sound index")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不实现 `LevelTables`、直接实现 `LevelSideState` 也能当 trait 对象用：
    /// 按区块存或者录制读写的实现者走的就是这条路。
    struct NoTables;

    impl LevelSideState for NoTables {
        fn comparator_output(&self, _pos: BlockPos) -> i32 {
            unimplemented!()
        }
        fn set_comparator_output(&mut self, _pos: BlockPos, _signal: i32) {
            unimplemented!()
        }
        fn last_bookshelf_slot(&self, _pos: BlockPos) -> Option<usize> {
            unimplemented!()
        }
        fn set_last_bookshelf_slot(&mut self, _pos: BlockPos, _slot: usize) {
            unimplemented!()
        }
        fn is_jukebox_playing(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn set_jukebox_playing(&mut self, _pos: BlockPos, _playing: bool) {
            unimplemented!()
        }
        fn crafter_disabled(&self, _pos: BlockPos) -> u16 {
            unimplemented!()
        }
        fn set_crafter_disabled(&mut self, _pos: BlockPos, _mask: u16) {
            unimplemented!()
        }
        fn set_crafter_crafting(&mut self, _pos: BlockPos, _remaining: i32) {
            unimplemented!()
        }
        fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)> {
            unimplemented!()
        }
        fn restore_crafter_crafting(&mut self, _entries: Vec<(BlockPos, i32)>) {
            unimplemented!()
        }
        fn forget_block_entity_memory(&mut self, _pos: BlockPos) {
            unimplemented!()
        }
        fn set_pending_lectern(&mut self, _facts: LecternFacts) {
            unimplemented!()
        }
        fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
            unimplemented!()
        }
        fn set_lectern(&mut self, _pos: BlockPos, _facts: LecternFacts) {
            unimplemented!()
        }
        fn lectern(&self, _pos: BlockPos) -> Option<LecternFacts> {
            unimplemented!()
        }
        fn clear_lectern(&mut self, _pos: BlockPos) {
            unimplemented!()
        }
        fn forget_torch_toggles_before(&mut self, _when: i64) {
            unimplemented!()
        }
        fn record_torch_toggle(&mut self, _pos: BlockPos, _when: i64) {
            unimplemented!()
        }
        fn torch_toggle_count(&self, _pos: BlockPos) -> usize {
            unimplemented!()
        }
        fn push_block_event(&mut self, _event: BlockEvent) {
            unimplemented!()
        }
        fn pop_block_event(&mut self) -> Option<BlockEvent> {
            unimplemented!()
        }
        fn block_entity_dyn(&self, _pos: BlockPos) -> Option<&dyn BlockEntity> {
            unimplemented!()
        }
        fn has_block_entity(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn insert_block_entity(&mut self, _entity: Box<dyn BlockEntity>) {
            unimplemented!()
        }
        fn remove_block_entity(&mut self, _pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
            unimplemented!()
        }
        fn take_block_entity_at(&mut self, _pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
            unimplemented!()
        }
        fn take_block_entity(&mut self, _pos: BlockPos, _id: u64) -> Option<Box<dyn BlockEntity>> {
            unimplemented!()
        }
        fn put_block_entity(&mut self, _pos: BlockPos, _id: u64, _entity: Box<dyn BlockEntity>) {
            unimplemented!()
        }
        fn is_block_entity_taken(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn begin_block_entity_tick(&mut self) {
            unimplemented!()
        }
        fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
            unimplemented!()
        }
        fn end_block_entity_tick(&mut self) {
            unimplemented!()
        }
    }

    /// 两个 trait 都要 dyn 安全：`Level` 的调用方拿的是 trait 对象，
    /// 按区块存或录制读写的实现者可以只实现 `LevelSideState`。
    #[test]
    fn side_state_traits_are_object_safe() {
        fn assert_object_safe(_: &dyn LevelSideState, _: &dyn LevelTables) {}
        let state: &dyn LevelSideState = &NoTables;
        let _ = state as *const dyn LevelSideState;
        let _ = assert_object_safe;
    }
}
