//! 用 `mc-bamboo-blocks` 重放一道题。

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    slice,
};

use mc_bamboo_blocks::{
    Aabb, Actor, BlockEntities, BlockEvents, BlockGameRules, BlockHit, BlockState, BlockTicks,
    Combined, Container, Effect, EntityStore, EntityTables, HopperEntity, InteractionResult, Level,
    LevelRandom, LevelTables, Outcome, RedstoneMemory, ShulkerSlots, Slots, TickPriority,
    apply_text, destroy_block,
    entity::Entities,
    level::EquipTarget,
    random_tick, redstone_signal, run_action_block_ticks, run_scheduled_ticks,
    run_scheduled_ticks_with_cache,
    shape::{self, BooleanOp},
    state::{
        CHEST_TYPE, ChestType, FACING, HAS_RECORD, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED,
        SLOT_2_OCCUPIED, SLOT_3_OCCUPIED, SLOT_4_OCCUPIED, SLOT_5_OCCUPIED,
    },
    tick_chunk, use_item, use_item_on, use_item_on_with_stack,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{BlockAncestor, blocks, items, registries::entity_type, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Names,
    lines::{
        Action, Case, Cell, ContainerStack, Destroyed, Double, Float, Header, Kind, LevelPlay,
        SoundPlay,
    },
};

/// `Player.DEFAULT_EYE_HEIGHT`。
const STANDING_EYE_HEIGHT: f32 = 1.62;
/// `Player.CROUCH_DIMENSIONS` 的眼睛高度。
const CROUCHING_EYE_HEIGHT: f32 = 1.27;
/// 创造模式的 `blockInteractionRange`。
const CREATIVE_INTERACTION_RANGE: f64 = 5.0;
/// `BlockOracle.java` 假世界的亮度，两种光都是这个值。
const FULL_BRIGHTNESS: i32 = 15;
/// `BlockOracle.java` 假世界的海平面，官方 `Biome.getTemperature` 用它。
const SEA_LEVEL: i32 = 63;
/// 流体的注册表名，下标是 [`mc_bamboo_data::registries::fluid`] 里的 ID。
pub const FLUID_NAMES: [&str; 5] = [
    "minecraft:empty",
    "minecraft:flowing_water",
    "minecraft:water",
    "minecraft:flowing_lava",
    "minecraft:lava",
];

/// 这个世界在 Rust 这边的答案。
#[derive(Debug)]
pub struct Replayed {
    pub result: &'static str,
    /// 和场景不同的格子。
    pub changes: BTreeMap<BlockPos, u32>,
    pub destroyed: Vec<Destroyed>,
    /// 全部世界事件，包括 2001，按发生顺序。
    pub level_events: Vec<LevelPlay>,
    /// 全部音效，按发生顺序。种子不比较。
    pub sounds: Vec<SoundPlay>,
    /// 结束时所有已建模容器的非空格子，按坐标和槽位排序。
    pub contents: Vec<ContainerStack>,
    /// 走完 `settle` 之后还排着的计划刻，方块的在前，流体的在后，各自按执行顺序，
    /// 延迟相对结束的时刻。
    pub ticks: Vec<Scheduled>,
    /// 物品栏里新出现的物品。
    pub gained: Vec<String>,
    /// 读写过的方块的注册表 ID。结果不对时看它们里面有没有还没移植的类。
    pub touched: BTreeSet<usize>,
    /// 跑完之后 `level.random` 的内部种子，发射器和合成器题与官方逐位比较。
    ///
    /// The internal seed of `level.random` after the case, compared bit for bit with vanilla
    /// on dispenser and crafter cases.
    pub random_seed: i64,
}

/// 排着队的计划刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    pub pos: BlockPos,
    /// 流体的计划刻，否则是方块的。
    pub fluid: bool,
    /// 方块或者流体的注册表名。
    pub id: &'static str,
    pub delay: i32,
    pub priority: i32,
}

/// 假世界：方块在表里，世界外是 `void_air`，实体只有场景里的玩家。随机源和官方用同一个种子。
struct OracleLevel {
    blocks: HashMap<BlockPos, BlockState>,
    /// 容器方块的槽位表，官方的 `Container` 方块实体。双箱两半各一张，引用时按
    /// `CompoundContainer` 拼起来。末影箱的槽位在玩家身上，不是容器，不在这里。
    containers: BTreeMap<BlockPos, Vec<ItemStack>>,
    entities: Vec<Aabb>,
    /// 下落方块和 TNT。方块 oracle 不 tick 它们，只是让 `addFreshEntity` 有地方放。
    ///
    /// Falling blocks and primed TNT. The block oracle does not tick them; the store
    /// only gives `addFreshEntity` somewhere to land.
    world_entities: Entities,
    min_y: i32,
    max_y: i32,
    random: LevelRandom,
    touched: RefCell<BTreeSet<usize>>,
    /// 每道题一份，和假世界里每道题新建的方块实体一样。
    redstone: RedstoneMemory,
    /// 活塞这类会动的方块实体，见 `BlockOracle.java` 的方块事件和 ticker。
    block_entities: BlockEntities,
    block_events: BlockEvents,
    /// 假玩家。先是操作的那一个，再是 `others` 里的旁观者。
    ///
    /// The fake players. The actor comes first, then the bystanders in `others`.
    living: Vec<EquipTarget>,
    /// 这道题的游戏规则。默认值和官方 `new GameRules(DEFAULT_FLAGS)` 一样。
    ///
    /// Game rules for this case. Defaults match `new GameRules(DEFAULT_FLAGS)`.
    block_rules: BlockGameRules,
    /// 题面里的天气快照。
    block_weather: mc_bamboo_blocks::BlockWeather,
    /// 假世界的降水条件：平原、海平面 63、按位置算真实官方温度。
    precipitation: mc_bamboo_worldgen::biome_temperature::BiomeClimate,
}

/// 已建模的容器，槽位数和官方一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Chest,
    Barrel,
    ShulkerBox,
    Hopper,
    Dispenser,
    Dropper,
    ChiseledBookshelf,
    Jukebox,
    DecoratedPot,
    Crafter,
    Furnace,
    Brewing,
    Shelf,
}

impl ContainerKind {
    /// 方块属于哪种容器。顺序有讲究：投掷器也是发射器的子类，末影箱不是箱子。
    fn of(state: BlockState) -> Option<Self> {
        [
            (BlockAncestor::ShulkerBoxBlock, Self::ShulkerBox),
            (BlockAncestor::BarrelBlock, Self::Barrel),
            (BlockAncestor::HopperBlock, Self::Hopper),
            (BlockAncestor::DropperBlock, Self::Dropper),
            (BlockAncestor::DispenserBlock, Self::Dispenser),
            (BlockAncestor::ChestBlock, Self::Chest),
            (
                BlockAncestor::ChiseledBookShelfBlock,
                Self::ChiseledBookshelf,
            ),
            (BlockAncestor::JukeboxBlock, Self::Jukebox),
            (BlockAncestor::DecoratedPotBlock, Self::DecoratedPot),
            (BlockAncestor::CrafterBlock, Self::Crafter),
            (BlockAncestor::AbstractFurnaceBlock, Self::Furnace),
            (BlockAncestor::BrewingStandBlock, Self::Brewing),
            (BlockAncestor::ShelfBlock, Self::Shelf),
        ]
        .into_iter()
        .find(|(ancestor, _)| state.is_a(*ancestor))
        .map(|(_, kind)| kind)
    }

    const fn slots(self) -> usize {
        match self {
            Self::Hopper => 5,
            Self::Dispenser | Self::Dropper | Self::Crafter => 9,
            Self::ChiseledBookshelf => 6,
            Self::Jukebox | Self::DecoratedPot => 1,
            Self::Chest | Self::Barrel | Self::ShulkerBox => 27,
            Self::Furnace | Self::Shelf => 3,
            Self::Brewing => 5,
        }
    }
}

/// 一个双箱的两半，`first` 是主箱（`ChestType::Right` 那一半）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DoubleChest {
    first: BlockPos,
    second: BlockPos,
}

/// `FakeLevel.syncContents` 里直接写字段的那两种。不调用 `setItem`，所以书架的
/// `lastInteractedSlot` 仍是 -1，唱机也不开始放歌。
///
/// The two `FakeLevel.syncContents` cases that write fields directly. They do not call
/// `setItem`, so a bookshelf keeps `lastInteractedSlot` at -1 and a jukebox does not play.
fn seed_mirrored_contents(slots: &mut [ItemStack], state: BlockState) {
    if state.is_a(BlockAncestor::ChiseledBookShelfBlock) {
        let occupied = [
            SLOT_0_OCCUPIED,
            SLOT_1_OCCUPIED,
            SLOT_2_OCCUPIED,
            SLOT_3_OCCUPIED,
            SLOT_4_OCCUPIED,
            SLOT_5_OCCUPIED,
        ];
        for (slot, property) in occupied.into_iter().enumerate() {
            if state.get(property) {
                slots[slot] = ItemStack::new(items::BOOK, 1);
            }
        }
    }
    if state.is_a(BlockAncestor::JukeboxBlock) && state.get(HAS_RECORD) && !slots.is_empty() {
        slots[0] = ItemStack::new(items::MUSIC_DISC_13, 1);
    }
}

fn block_rules_of(header: &Header) -> Result<BlockGameRules, String> {
    let mut rules = BlockGameRules::vanilla();
    for (name, value) in &header.rules {
        apply_text(&mut rules, name, value)?;
    }
    Ok(rules)
}

/// 题面里的稳定天气：`rain`/`thunder` 的雨强是 1，`thunder` 的雷暴强度是 1。
fn weather_of(header: &Header) -> Result<mc_bamboo_blocks::BlockWeather, String> {
    Ok(match header.weather.as_str() {
        "clear" => mc_bamboo_blocks::BlockWeather::CLEAR,
        "rain" | "thunder" => mc_bamboo_blocks::BlockWeather {
            rain_level: 1.0,
            thunder_level: f32::from(u8::from(header.weather == "thunder")),
        },
        other => return Err(format!("unknown oracle weather {other}")),
    })
}

impl OracleLevel {
    /// 按场景建世界，再填容器内容。容器和漏斗方块实体的建立顺序和官方一样：
    /// 场景按 `BlockPos.asLong` 排（x、z、y），先建全部方块实体，再填内容。
    fn new(case: &Case, header: &Header, names: &Names) -> Result<Self, String> {
        let mut cells: Vec<(BlockPos, BlockState)> = case
            .scene
            .iter()
            .map(|cell| (cell.pos(), BlockState(cell.state())))
            .collect();
        cells.sort_by_key(|(pos, _)| pos.pack());

        let mut containers: BTreeMap<BlockPos, Vec<ItemStack>> = BTreeMap::new();
        let mut block_entities = BlockEntities::default();
        for &(pos, state) in &cells {
            if let Some(kind) = ContainerKind::of(state) {
                let mut slots = vec![ItemStack::empty(); kind.slots()];
                // `FakeLevel.syncContents`：书架的占用格补成书，带唱片的唱机补成 `music_disc_13` 且不放歌。
                // 内容表随后覆盖这些格子，仍然不走 `setItem`。
                //
                // `FakeLevel.syncContents`: occupied bookshelf slots become books, and a jukebox
                // with a record becomes `music_disc_13` without starting playback. Later contents
                // overwrite those slots and still do not call `setItem`.
                seed_mirrored_contents(&mut slots, state);
                containers.insert(pos, slots);
            }
            // 官方的 `LevelChunk` 在场景加载时给每个有方块实体的格子建实体，漏斗的
            // ticker 这时登记，tick 顺序就是这里插入的顺序。
            if state.is_a(BlockAncestor::HopperBlock) {
                block_entities.insert(Box::new(HopperEntity::new(pos)));
            }
        }
        for filled in &case.contents {
            let Some(slots) = containers.get_mut(&filled.pos()) else {
                // 场景里这一格不是已建模的容器，官方那边也读不到 `Container`。
                continue;
            };
            let Ok(slot) = usize::try_from(filled.slot()) else {
                continue;
            };
            if let Some(slots) = slots.get_mut(slot) {
                let (item, _) = names.item(filled.item())?;
                *slots = ItemStack::new(item, filled.count());
            }
        }

        Ok(Self {
            blocks: cells.into_iter().collect(),
            containers,
            entities: entity_boxes(&case.action),
            world_entities: Entities::default(),
            min_y: header.min_y,
            max_y: header.max_y,
            random: LevelRandom::new(case.seed),
            touched: RefCell::new(BTreeSet::new()),
            redstone: RedstoneMemory::default(),
            block_entities,
            block_events: BlockEvents::default(),
            living: equip_targets(&case.action, names)?,
            block_rules: block_rules_of(header)?,
            block_weather: weather_of(header)?,
            precipitation: mc_bamboo_worldgen::biome_temperature::BiomeClimate::of(
                mc_bamboo_data::registries::biome::PLAINS,
            ),
        })
    }

    fn contains(&self, pos: BlockPos) -> bool {
        (self.min_y..=self.max_y).contains(&pos.y)
    }

    /// 这一格现在的容器种类，只看已经写过的方块。
    fn kind(&self, pos: BlockPos) -> Option<ContainerKind> {
        self.blocks.get(&pos).copied().and_then(ContainerKind::of)
    }

    /// 取槽位表，没有时按种类建一张空的。方块刚放下时官方也在这里建方块实体。
    fn slots_mut(&mut self, pos: BlockPos, kind: ContainerKind) -> &mut Vec<ItemStack> {
        self.containers
            .entry(pos)
            .or_insert_with(|| vec![ItemStack::empty(); kind.slots()])
    }

    /// `ChestBlock.isChestBlockedAt` 里和方块有关的那一半：上面是红石导体。
    fn blocked(&self, pos: BlockPos) -> bool {
        self.block_state(pos.above()).is_redstone_conductor()
    }

    /// `ChestBlock.getConnectedDirection`。
    fn connected_direction(state: BlockState) -> Direction {
        let facing = state.get(FACING);
        if state.get(CHEST_TYPE) == ChestType::Left {
            facing.clockwise()
        } else {
            facing.counter_clockwise()
        }
    }

    /// `chestCanConnectTo`：普通箱子只和同一种箱子合并，铜箱子不管氧化程度和涂蜡。
    fn can_connect_to(state: BlockState, other: BlockState) -> bool {
        if state.is_a(BlockAncestor::CopperChestBlock) {
            other.is_tagged(block_tags::COPPER_CHESTS) && other.has(CHEST_TYPE)
        } else {
            other.is_same_block(state)
        }
    }

    /// `DoubleBlockCombiner` 对邻居的全部要求：能合并、不是单箱、类型不同、朝向相同。
    fn neighbour_combines(&self, pos: BlockPos, state: BlockState) -> bool {
        let Some(other) = self.blocks.get(&pos).copied() else {
            return false;
        };
        if !Self::can_connect_to(state, other) {
            return false;
        }
        let (Some(other_type), Some(other_facing)) =
            (other.try_get(CHEST_TYPE), other.try_get(FACING))
        else {
            return false;
        };
        other_type != ChestType::Single
            && other_type != state.get(CHEST_TYPE)
            && other_facing == state.get(FACING)
    }

    /// `ChestBlock.combine`。`Ok(None)` 是单箱，`Err(())` 是被压住读不到。
    fn chest_shape(&self, pos: BlockPos, ignore_blocked: bool) -> Result<Option<DoubleChest>, ()> {
        if !ignore_blocked && self.blocked(pos) {
            return Err(());
        }
        let Some(state) = self.blocks.get(&pos).copied() else {
            return Ok(None);
        };
        let chest_type = state.get(CHEST_TYPE);
        if chest_type == ChestType::Single {
            return Ok(None);
        }
        let first_is_here = chest_type == ChestType::Right;
        let neighbour = pos.relative(Self::connected_direction(state));
        if !self.neighbour_combines(neighbour, state) {
            return Ok(None);
        }
        if !ignore_blocked && self.blocked(neighbour) {
            return Err(());
        }
        Ok(Some(if first_is_here {
            DoubleChest {
                first: pos,
                second: neighbour,
            }
        } else {
            DoubleChest {
                first: neighbour,
                second: pos,
            }
        }))
    }

    fn touch(&self, state: BlockState) {
        self.touched.borrow_mut().insert(state.block_id());
    }

    /// 和 `scene` 不同的格子，变成空气的格子也算。
    fn changes(&self, scene: &[Cell]) -> BTreeMap<BlockPos, u32> {
        let before: HashMap<BlockPos, u32> = scene
            .iter()
            .map(|cell| (cell.pos(), cell.state()))
            .collect();
        let mut changes: BTreeMap<BlockPos, u32> = self
            .blocks
            .iter()
            .map(|(&pos, state)| (pos, state.0))
            .filter(|&(pos, state)| before.get(&pos).copied().unwrap_or(blocks::AIR) != state)
            .collect();
        changes.retain(|pos, _| self.contains(*pos));
        changes
    }
}

impl LevelTables for OracleLevel {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events_table(&self) -> &BlockEvents {
        &self.block_events
    }

    fn block_events_table_mut(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone_table(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }
}

impl EntityTables for OracleLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.world_entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.world_entities
    }
}

impl Level for OracleLevel {
    fn runs_block_entity_tick(&self, pos: BlockPos) -> bool {
        // 方块 oracle 不跑熔炉和酿造台。漏斗、移动活塞和潜影盒仍按原来的 ticker 跑。
        //
        // The block oracle does not tick furnaces or brewing stands. Hoppers, moving pistons
        // and shulkers keep the ticker they already had.
        let state = self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR);
        !state.is_a(BlockAncestor::AbstractFurnaceBlock)
            && !state.is_a(BlockAncestor::BrewingStandBlock)
    }

    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !self.contains(pos) {
            return BlockState(blocks::VOID_AIR);
        }
        let state = self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR);
        self.touch(state);
        state
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        if !self.contains(pos) {
            return false;
        }
        self.touch(state);
        // 换成别的方块时方块实体一起换掉，假世界的掉落表是空的。同一格还是同一种容器时保留内容。
        let old = self.kind(pos);
        let new = ContainerKind::of(state);
        if old != new {
            self.containers.remove(&pos);
            if let Some(kind) = new {
                self.containers
                    .insert(pos, vec![ItemStack::empty(); kind.slots()]);
            }
        }
        self.blocks.insert(pos, state);
        true
    }

    /// `EntityGetter.isUnobstructed`：先用外框筛一遍实体，再做精确的形状运算。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool {
        let bounds = bounds_of(boxes);
        !self.entities.iter().any(|entity| {
            intersects(bounds, *entity)
                && shape::join_is_not_empty(boxes, slice::from_ref(entity), BooleanOp::And)
        })
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.max_y
    }

    /// 假世界里 `getBrightness` 恒为 15，`getSkyDarken` 是 0。
    fn brightness(&self, _layer: LightLayer, _pos: BlockPos) -> i32 {
        FULL_BRIGHTNESS
    }

    /// 官方的假世界每次 `getBlockEntity` 都按状态新建一个空的告示牌实体，
    /// 所以这里看到的永远是空白告示牌。
    fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        let state = self.block_state(pos);
        state
            .is_a(BlockAncestor::SignBlock)
            .then(SignContent::default)
    }

    /// 写进来的内容不留下，下一次读还是那个新建的空实体。
    fn set_sign(&mut self, pos: BlockPos, _content: SignContent) -> bool {
        self.block_state(pos).is_a(BlockAncestor::SignBlock)
    }

    /// 官方的假世界不会给方块实体放组件，`getNoteBlockSound` 永远是 `null`，
    /// 音符盒的 `getCustomSoundId` 拿不到音效，`triggerEvent` 直接返回 `false`。
    fn note_block_sound(&self, _pos: BlockPos) -> Option<String> {
        None
    }

    fn block_game_rules(&self) -> BlockGameRules {
        self.block_rules
    }

    fn block_weather(&self) -> mc_bamboo_blocks::BlockWeather {
        self.block_weather
    }

    /// `FakeChunk.getHeight(MOTION_BLOCKING, x, z)`：从最高层往下扫到第一个
    /// `BLOCKS_MOTION_IN_HEIGHTMAP` 方块或流体，`Level.getHeightmapPos` 再 +1。
    /// 整列没有时是 `minY`。
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        let found = (self.min_y..=self.max_y).rev().find(|&y| {
            let state = self
                .blocks
                .get(&BlockPos::new(pos.x, y, pos.z))
                .copied()
                .unwrap_or(BlockState::AIR);
            state.is_tagged(block_tags::BLOCKS_MOTION_IN_HEIGHTMAP) || !state.fluid().is_empty()
        });
        found.map_or(self.min_y, |y| y + 1)
    }

    /// 假世界永远是平原，海平面 63。温度按官方 `Biome.getTemperature` 算。
    fn precipitation_facts(&self, pos: BlockPos) -> mc_bamboo_blocks::PrecipitationFacts {
        use mc_bamboo_worldgen::biome_temperature as biome;
        let precipitation =
            biome::precipitation_at(self.precipitation, pos.x, pos.y, pos.z, SEA_LEVEL);
        mc_bamboo_blocks::PrecipitationFacts {
            precipitation: match precipitation {
                biome::Precipitation::None => mc_bamboo_blocks::Precipitation::None,
                biome::Precipitation::Rain => mc_bamboo_blocks::Precipitation::Rain,
                biome::Precipitation::Snow => mc_bamboo_blocks::Precipitation::Snow,
            },
            warm_enough_to_rain: self
                .precipitation
                .warm_enough_to_rain(pos.x, pos.y, pos.z, SEA_LEVEL),
        }
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    /// 官方 `FakePlayer` 先加进世界，再加旁观者。格子都是空的，主副手用题目里的物品。
    ///
    /// Official adds the `FakePlayer` first, then bystanders. Armour slots start empty.
    /// The hands use the items from the case.
    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        self.living.clone()
    }

    fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        let target = self
            .living
            .iter_mut()
            .find(|target| target.entity_id == entity_id)?;
        let index = usize::try_from(component_slot).ok()?;
        let stored = target.equipment.get_mut(index)?.as_mut()?;
        *stored = stack;
        u16::try_from(component_slot).ok()
    }

    fn replace_block_entity_for_cache(&mut self, entity: Box<dyn mc_bamboo_blocks::BlockEntity>) {
        self.block_entities.replace_in_place(entity);
    }

    /// 打开缓存时守卫要读整段哈希：oracle 的假世界是稀疏表，按空气补齐整段。
    fn section_hash(&self, chunk: ChunkPos, section_y: i32) -> Option<u128> {
        Some(mc_bamboo_core::section_hash::section_hash(|x, y, z| {
            let pos = BlockPos::new(
                chunk.x * 16 + x as i32,
                section_y * 16 + y as i32,
                chunk.z * 16 + z as i32,
            );
            self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR).0
        }))
    }

    /// 官方假世界的区块段每次都扫一遍场景。
    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        self.blocks.iter().any(|(pos, state)| {
            pos.chunk() == chunk
                && pos.y >> 4 == section_y
                && (state.is_randomly_ticking() || state.is_fluid_randomly_ticking())
        })
    }

    /// 官方假世界的 `canSpreadFireAround` 永远是真：场景里的玩家总在目标附近。
    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        true
    }

    /// `getBlockContainer`：漏斗取物品时会忽略压住箱子的方块，双箱按 `CompoundContainer` 合并。
    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let Some(kind) = self.kind(pos) else {
            return false;
        };
        if kind.slots() != len {
            return false;
        }
        f(self.slots_mut(pos, kind));
        true
    }

    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        self.containers.get(&pos).cloned().unwrap_or_default()
    }

    fn clear_container(&mut self, pos: BlockPos) {
        self.containers.remove(&pos);
    }

    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let Some(kind) = self.kind(pos) else {
            return false;
        };
        if kind == ContainerKind::Chest
            && let Ok(Some(chest)) = self.chest_shape(pos, true)
        {
            self.slots_mut(chest.first, ContainerKind::Chest);
            self.slots_mut(chest.second, ContainerKind::Chest);
            let Some(mut first) = self.containers.remove(&chest.first) else {
                return false;
            };
            let Some(mut second) = self.containers.remove(&chest.second) else {
                self.containers.insert(chest.first, first);
                return false;
            };
            {
                let mut combined = Combined::new(&mut first, &mut second);
                f(&mut combined);
            }
            self.containers.insert(chest.first, first);
            self.containers.insert(chest.second, second);
            return true;
        }

        let slots = self.slots_mut(pos, kind);
        if kind == ContainerKind::ShulkerBox {
            let mut shulker = ShulkerSlots::new(slots);
            f(&mut shulker);
        } else {
            let mut normal = Slots::new(slots);
            f(&mut normal);
        }
        true
    }

    /// `BlockEntity.preRemoveSideEffects` 掉落的是这一格自己的槽位表，双箱的另一半不算。
    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        self.containers
            .get(&pos)
            .map(|slots| slots.iter().map(|stack| stack.count).collect())
            .unwrap_or_default()
    }

    /// `ChestBlock.getAnalogOutputSignal` 读信号时不忽略压住箱子的方块，双箱按 54 格算。
    fn container_signal(&self, pos: BlockPos) -> i32 {
        if self.kind(pos) == Some(ContainerKind::Chest) {
            match self.chest_shape(pos, false) {
                Err(()) => return 0,
                Ok(None) => {}
                Ok(Some(chest)) => {
                    let first = self
                        .containers
                        .get(&chest.first)
                        .map_or(&[][..], |slots| slots.as_slice());
                    let second = self
                        .containers
                        .get(&chest.second)
                        .map_or(&[][..], |slots| slots.as_slice());
                    return redstone_signal(
                        first.iter().chain(second.iter()),
                        first.len() + second.len(),
                    );
                }
            }
        }
        let Some(slots) = self.containers.get(&pos) else {
            return 0;
        };
        redstone_signal(slots.iter(), slots.len())
    }

    /// `CompoundContainer.setChanged`：双箱两半都通知。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        if self.kind(pos) == Some(ContainerKind::Chest)
            && let Ok(Some(chest)) = self.chest_shape(pos, true)
        {
            return vec![chest.first, chest.second];
        }
        vec![pos]
    }

    /// 漏斗的 ticker 在 [`Level::block_entities`] 里，这里直接取出来。
    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let Some(hopper) = self.block_entities.get_mut(pos) else {
            return false;
        };
        f(hopper);
        true
    }
}

/// `VoxelShape.bounds`。
fn bounds_of(boxes: &[Aabb]) -> Aabb {
    let fold = |pick: fn(&Aabb) -> f64, merge: fn(f64, f64) -> f64| {
        boxes
            .iter()
            .map(pick)
            .reduce(merge)
            .expect("空形状不用查实体")
    };
    Aabb::new(
        fold(|bounds| bounds.min_x, f64::min),
        fold(|bounds| bounds.min_y, f64::min),
        fold(|bounds| bounds.min_z, f64::min),
        fold(|bounds| bounds.max_x, f64::max),
        fold(|bounds| bounds.max_y, f64::max),
        fold(|bounds| bounds.max_z, f64::max),
    )
}

/// `AABB.intersects`。
fn intersects(first: Aabb, second: Aabb) -> bool {
    first.min_x < second.max_x
        && first.max_x > second.min_x
        && first.min_y < second.max_y
        && first.max_y > second.min_y
        && first.min_z < second.max_z
        && first.max_z > second.min_z
}

/// 发射器能穿上的假玩家。创造模式，不是旁观者，八个槽都在。
///
/// Fake players a dispenser can equip. Creative, not spectating, and all eight slots exist.
fn equip_targets(action: &Action, names: &Names) -> Result<Vec<EquipTarget>, String> {
    let mut targets = vec![player_target(
        1,
        &action.actor_box,
        &action.main,
        &action.off,
        names,
    )?];
    for (index, bounds) in action.others.iter().enumerate() {
        let id = i32::try_from(index).unwrap_or(0).saturating_add(2);
        targets.push(player_target(
            id,
            bounds,
            "minecraft:air",
            "minecraft:air",
            names,
        )?);
    }
    Ok(targets)
}

fn player_target(
    entity_id: i32,
    bounds: &[Double; 6],
    main: &str,
    off: &str,
    names: &Names,
) -> Result<EquipTarget, String> {
    let empty = Some(ItemStack::empty());
    Ok(EquipTarget {
        entity_id,
        bounds: aabb(bounds),
        spectator: false,
        entity_type: entity_type::PLAYER,
        equipment: [
            Some(held_stack(names, main)?),
            empty.clone(),
            empty.clone(),
            empty.clone(),
            empty.clone(),
            Some(held_stack(names, off)?),
            empty.clone(),
            empty,
        ],
    })
}

fn held_stack(names: &Names, name: &str) -> Result<ItemStack, String> {
    let (id, _) = names.item(name)?;
    if id == items::AIR {
        Ok(ItemStack::empty())
    } else {
        Ok(ItemStack::new(id, 1))
    }
}

fn entity_boxes(action: &Action) -> Vec<Aabb> {
    std::iter::once(&action.actor_box)
        .chain(&action.others)
        .map(aabb)
        .collect()
}

fn aabb(bits: &[Double; 6]) -> Aabb {
    let [min_x, min_y, min_z, max_x, max_y, max_z] = bits.map(|value| value.0);
    Aabb::new(min_x, min_y, min_z, max_x, max_y, max_z)
}

fn actor_of(action: &Action, names: &Names) -> Result<Actor, String> {
    let [x, y, z] = action.actor.map(|value| value.0);
    let eye_height = if action.sneak {
        CROUCHING_EYE_HEIGHT
    } else {
        STANDING_EYE_HEIGHT
    };
    Ok(Actor {
        // 假世界每次 `getBlockEntity` 都新建空告示牌，编辑器登记不上，UUID 看不出来。
        uuid: [0; 16],
        position: Vec3::new(x, y, z),
        yaw: action.yaw.0,
        // 官方的题里头和身体的朝向相同。
        head_yaw: action.yaw.0,
        pitch: action.pitch.0,
        secondary_use: action.sneak,
        main_hand: names.item(&action.main)?.0,
        off_hand: names.item(&action.off)?.0,
        eye_height: f64::from(eye_height),
        block_interaction_range: CREATIVE_INTERACTION_RANGE,
        // 题目的玩家一直是创造模式。
        may_build: true,
        prevents_block_drops: true,
        // 假玩家是创造模式，但没有任何权限。`canUseGameMasterBlocks` 是假。
        //
        // The fake player is creative and has no permissions, so `canUseGameMasterBlocks` is false.
        can_use_game_master_blocks: false,
    })
}

fn hit_of(action: &Action) -> Result<BlockHit, String> {
    let [x, y, z] = action.loc.ok_or("place or use case without a click")?;
    Ok(BlockHit {
        pos: action.target(),
        face: action.face.ok_or("place or use case without a face")?,
        location: Vec3::new(x.0, y.0, z.0),
        inside: action.inside.ok_or("place or use case without inside")?,
    })
}

/// 跑一道题。题目本身有问题时返回错误，`mc-bamboo-blocks` 的 panic 由调用方接住。
pub fn replay(
    case: &Case,
    header: &Header,
    names: &Names,
    redstone_cache: Option<&mut mc_bamboo_blocks::cache::RedstoneCache>,
) -> Result<Replayed, String> {
    let action = &case.action;
    let mut level = OracleLevel::new(case, header, names)?;
    let mut ticks = BlockTicks::new();
    // 官方的假世界直接从 `gameTime` 开始，没有先跑一刻，这里也只拨时钟：
    // 跑一刻会把场景里的漏斗方块实体先 tick 一次，动作之后的刻数就对不上了。
    ticks.set_time(header.game_time);

    let actor = actor_of(action, names)?;
    let target = action.target();
    let now = header.game_time;
    let mut outcome = match action.kind {
        Kind::Place | Kind::Use => {
            let hit = hit_of(action)?;
            let held = match action.hand {
                Hand::Main => &action.main,
                Hand::Off => &action.off,
            };
            // 点到炼药锅时给手里的物品挂上和 Java 同一套组件，两边才能比到水瓶、染料和旗帜。
            // A cauldron click gets the same components Java attaches, so water, dye and banners compare.
            match super::cauldron_hand::stack(
                level.block_state(hit.pos),
                names.item(held)?.0,
                action.sneak,
            ) {
                Some(stack) => use_item_on_with_stack(
                    &mut level,
                    &mut ticks,
                    &actor,
                    action.hand,
                    &hit,
                    &stack,
                ),
                None => use_item_on(&mut level, &mut ticks, &actor, action.hand, &hit),
            }
        }
        Kind::Destroy => destroy_block(&mut level, &mut ticks, &actor, target),
        Kind::Tick => run_one_tick(&mut level, &mut ticks, target, now),
        Kind::Flow => run_flow(&mut level, &mut ticks, target, now, action.steps),
        Kind::Item => use_item(&mut level, &mut ticks, &actor, action.hand),
        Kind::Random => random_tick(&mut level, &mut ticks, target),
        Kind::Chunk => tick_chunk(&mut level, &mut ticks, target.chunk(), action.speed),
    };
    // flow 题在动作里已经走到 `game_time + steps`，后面的刻从那里接着数。
    let after_action = match action.kind {
        Kind::Flow => now + i64::from(action.steps),
        _ => now,
    };
    let end = after_action + i64::from(action.settle);
    settle(
        &mut level,
        &mut ticks,
        after_action,
        end,
        &mut outcome,
        redstone_cache,
    );

    let (level_events, sounds) = events_of(&outcome);
    Ok(Replayed {
        result: result_name(action.kind, outcome.result),
        changes: level.changes(&case.scene),
        destroyed: destroyed_of(&outcome),
        level_events,
        sounds,
        contents: contents_of(&level),
        ticks: scheduled_of(&ticks, end),
        gained: gained_of(&outcome, &actor),
        touched: level.touched.into_inner(),
        random_seed: level.random.source().seed(),
    })
}

/// 动作之后一直走到 `end`：每一刻和世界线程一样，时间加一再跑到期的计划刻。破坏事件接在动作的后面。
fn settle(
    level: &mut OracleLevel,
    ticks: &mut BlockTicks,
    start: i64,
    end: i64,
    outcome: &mut Outcome,
    mut redstone_cache: Option<&mut mc_bamboo_blocks::cache::RedstoneCache>,
) {
    for now in start + 1..=end {
        let settled = if let Some(cache) = redstone_cache.as_deref_mut() {
            run_scheduled_ticks_with_cache(level, ticks, now, Some(cache))
        } else {
            run_scheduled_ticks(level, ticks, now)
        };
        outcome.effects.extend(settled.effects);
    }
}

/// 官方的题直接调用 `state.tick`，这里排一个立刻到期的刻，只跑到期的方块刻。
fn run_one_tick(
    level: &mut OracleLevel,
    ticks: &mut BlockTicks,
    pos: BlockPos,
    now: i64,
) -> Outcome {
    let block = level.block_state(pos).block_id();
    ticks.schedule(pos, block, 0, TickPriority::Normal);
    run_action_block_ticks(level, ticks, now)
}

/// 官方的题直接调用这一格的 `FluidState.tick`，再一刻一刻往后跑。这里同样排一个立刻到期的流体刻。
fn run_flow(
    level: &mut OracleLevel,
    ticks: &mut BlockTicks,
    pos: BlockPos,
    now: i64,
    steps: u32,
) -> Outcome {
    let fluid = level.block_state(pos).fluid().fluid();
    ticks.schedule_fluid(pos, fluid, 0, TickPriority::Normal);
    let mut outcome = run_scheduled_ticks(level, ticks, now);
    for step in 1..=i64::from(steps) {
        let later = run_scheduled_ticks(level, ticks, now + step);
        outcome.effects.extend(later.effects);
    }
    outcome
}

fn result_name(kind: Kind, result: InteractionResult) -> &'static str {
    match kind {
        Kind::Tick | Kind::Flow | Kind::Random | Kind::Chunk => "ticked",
        Kind::Destroy if result == InteractionResult::Fail => "refused",
        Kind::Destroy => "destroyed",
        Kind::Place | Kind::Use | Kind::Item => match result {
            InteractionResult::Success => "success",
            InteractionResult::SuccessServer => "success_server",
            InteractionResult::Consume => "consume",
            InteractionResult::Fail => "fail",
            InteractionResult::Pass => "pass",
            InteractionResult::TryEmptyHandInteraction => "try_empty_hand",
        },
    }
}

/// 结束时所有已建模容器的非空格子。顺序和官方一样：先按 `BlockPos.asLong`（x、z、y），再按槽位。
fn contents_of(level: &OracleLevel) -> Vec<ContainerStack> {
    // 场景里的熔炉和酿造台在槽位表里。玩家这题放下的那一台物品在方块实体上，实体盖过空表。
    //
    // Scene furnaces and brewing stands live in the slot table. One placed during the case
    // keeps items on its block entity, and that entity replaces the empty table.
    let mut lists: BTreeMap<i64, (BlockPos, Vec<ItemStack>)> = BTreeMap::new();
    for (pos, slots) in &level.containers {
        let state = level.blocks.get(pos).copied().unwrap_or(BlockState::AIR);
        // 雕纹书架的内容是状态的镜像，官方 `collectContents` 不把它写进内容表。
        //
        // A chiseled bookshelf mirrors its contents in the block state. Vanilla
        // `collectContents` leaves it out of the contents list.
        if state.is_a(BlockAncestor::ChiseledBookShelfBlock) {
            continue;
        }
        lists.insert(pos.pack(), (*pos, slots.clone()));
    }
    level.block_entities.visit(|pos, entity| {
        let Some(items) = mc_bamboo_blocks::entity_container_items(entity) else {
            return;
        };
        let state = level.blocks.get(&pos).copied().unwrap_or(BlockState::AIR);
        if state.is_a(BlockAncestor::AbstractFurnaceBlock)
            || state.is_a(BlockAncestor::BrewingStandBlock)
        {
            lists.insert(pos.pack(), (pos, items));
        }
    });
    let mut contents = Vec::new();
    for (_, (pos, slots)) in lists {
        for (slot, stack) in slots.iter().enumerate() {
            if stack.is_empty() {
                continue;
            }
            let item = usize::try_from(stack.item).expect("物品 ID 不会是负数");
            let slot = u32::try_from(slot).expect("槽位放得进 u32");
            contents.push(ContainerStack(
                pos.x,
                pos.y,
                pos.z,
                slot,
                items::ITEMS[item].name.to_owned(),
                stack.count,
            ));
        }
    }
    contents
}

fn destroyed_of(outcome: &Outcome) -> Vec<Destroyed> {
    outcome
        .effects
        .iter()
        .filter_map(|effect| match *effect {
            Effect::Destroyed {
                pos,
                state,
                except_initiator,
            } => Some(Destroyed(pos.x, pos.y, pos.z, state.0, except_initiator)),
            // 火把烧坏只放烟，题里不比较。
            Effect::TorchBurnout { .. } => None,
            Effect::BoneMealUsed { .. }
            | Effect::FilledBucket { .. }
            | Effect::EmptiedBucket { .. }
            | Effect::SignChanged { .. }
            | Effect::SignWaxed { .. }
            | Effect::OpenSignEditor { .. }
            | Effect::Sound { .. }
            | Effect::LevelEvent { .. }
            | Effect::NoteBlockPlayed { .. }
            // 方块事件只发给客户端播放动画，假世界没有客户端。
            | Effect::BlockEvent { .. }
            // 堆肥的粒子和声音，题里不比较。
            | Effect::ComposterFill { .. }
            // 讲台翻页的声音，题里不比较。
            | Effect::PageTurned { .. }
            | Effect::DispenserEquipped { .. }
            // 营火食物的方块实体包，题里不比较。
            | Effect::CampfireChanged { .. }
            // 快捷栏提示和命令方块编辑器不进方块题的比较。
            //
            // Action-bar text and the command-block editor are not part of a block case.
            | Effect::Overlay { .. }
            | Effect::OpenCommandBlock { .. } => None,
        })
        .collect()
}

/// 把这一题留下的效应拆成世界事件和音效两个列表，各自按发生顺序。
fn events_of(outcome: &Outcome) -> (Vec<LevelPlay>, Vec<SoundPlay>) {
    let mut events = Vec::new();
    let mut sounds = Vec::new();
    for effect in &outcome.effects {
        match *effect {
            Effect::Destroyed {
                pos,
                state,
                except_initiator,
            } => events.push(LevelPlay(
                2001,
                pos.x,
                pos.y,
                pos.z,
                state.0 as i32,
                except_initiator,
            )),
            Effect::TorchBurnout { pos } => {
                events.push(LevelPlay(1502, pos.x, pos.y, pos.z, 0, false));
            }
            Effect::BoneMealUsed { pos } => {
                events.push(LevelPlay(1505, pos.x, pos.y, pos.z, 15, false));
            }
            Effect::SignWaxed { pos } => {
                events.push(LevelPlay(3003, pos.x, pos.y, pos.z, 0, false));
            }
            Effect::LevelEvent {
                event,
                pos,
                data,
                except_initiator,
            } => events.push(LevelPlay(
                event,
                pos.x,
                pos.y,
                pos.z,
                data,
                except_initiator,
            )),
            // `ComposterBlock` 的 `levelEvent(1500)`。官方 `except` 是 null。
            Effect::ComposterFill { pos, success } => events.push(LevelPlay(
                1500,
                pos.x,
                pos.y,
                pos.z,
                i32::from(success),
                false,
            )),
            Effect::Sound {
                sound,
                source,
                x,
                y,
                z,
                volume,
                pitch,
                except_initiator,
                ..
            } => sounds.push(SoundPlay(
                sound,
                Double(x),
                Double(y),
                Double(z),
                source.id(),
                Float(volume),
                Float(pitch),
                except_initiator,
            )),
            // `NoteBlock.triggerEvent` 的 `playSeededSound`。自定义头颅是直接 Holder，注册表 ID 是 -1。
            Effect::NoteBlockPlayed {
                pos,
                instrument,
                pitch,
                custom,
                ..
            } => {
                let id = if custom.is_some() {
                    -1
                } else {
                    i32::from(instrument.sound())
                };
                sounds.push(SoundPlay(
                    id,
                    Double(f64::from(pos.x) + 0.5),
                    Double(f64::from(pos.y) + 0.5),
                    Double(f64::from(pos.z) + 0.5),
                    mc_bamboo_data::sounds::RECORDS.id(),
                    Float(3.0),
                    Float(pitch),
                    false,
                ));
            }
            Effect::FilledBucket { .. }
            | Effect::EmptiedBucket { .. }
            | Effect::SignChanged { .. }
            | Effect::OpenSignEditor { .. }
            | Effect::BlockEvent { .. }
            // 讲台翻页是菜单里的 `levelEvent(1043)`，方块题不比较。游戏层会把它发出去。
            // A lectern page turn is menu `levelEvent(1043)`. Block cases do not compare it.
            // The game layer still sends it.
            | Effect::PageTurned { .. }
            | Effect::DispenserEquipped { .. }
            // 营火食物的方块实体包，方块题不比较。游戏层会把它发出去。
            // The campfire food block-entity packet is not compared. The game layer still sends it.
            | Effect::CampfireChanged { .. }
            | Effect::Overlay { .. }
            | Effect::OpenCommandBlock { .. } => {}
        }
    }
    (events, sounds)
}

/// 舀起来的东西。假玩家的物品栏里只有两只手上的物品，已经有同样的物品时官方不再放。
fn gained_of(outcome: &Outcome, actor: &Actor) -> Vec<String> {
    let mut held = vec![actor.main_hand, actor.off_hand];
    let mut gained = Vec::new();
    for effect in &outcome.effects {
        if let Effect::FilledBucket { item, .. } = *effect
            && !held.contains(&item)
        {
            held.push(item);
            gained.push(items::ITEMS[item as usize].name.to_owned());
        }
    }
    gained
}

fn scheduled_of(ticks: &BlockTicks, now: i64) -> Vec<Scheduled> {
    let delay =
        |trigger_tick: i64| i32::try_from(trigger_tick - now).expect("计划刻的延迟放得进 i32");
    let blocks = ticks.pending().map(|tick| Scheduled {
        pos: tick.pos,
        fluid: false,
        id: blocks::BLOCKS[tick.block].name,
        delay: delay(tick.trigger_tick),
        priority: tick.priority as i32,
    });
    let fluids = ticks.pending_fluids().map(|tick| Scheduled {
        pos: tick.pos,
        fluid: true,
        id: FLUID_NAMES[tick.fluid as usize],
        delay: delay(tick.trigger_tick),
        priority: tick.priority as i32,
    });
    blocks.chain(fluids).collect()
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    fn level_with_player(player: Aabb) -> OracleLevel {
        OracleLevel {
            blocks: HashMap::new(),
            containers: BTreeMap::new(),
            entities: vec![player],
            world_entities: Entities::default(),
            min_y: -64,
            max_y: 319,
            random: LevelRandom::new(0),
            touched: RefCell::new(BTreeSet::new()),
            redstone: RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            living: Vec::new(),
            block_rules: BlockGameRules::vanilla(),
            block_weather: mc_bamboo_blocks::BlockWeather::CLEAR,
            precipitation: mc_bamboo_worldgen::biome_temperature::BiomeClimate::of(
                mc_bamboo_data::registries::biome::PLAINS,
            ),
        }
    }

    #[test]
    fn fluid_names_follow_the_registry() {
        use mc_bamboo_data::registries::fluid;

        assert_eq!(FLUID_NAMES.len(), fluid::TOTAL as usize);
        assert_eq!(FLUID_NAMES[fluid::EMPTY as usize], "minecraft:empty");
        assert_eq!(FLUID_NAMES[fluid::WATER as usize], "minecraft:water");
        assert_eq!(
            FLUID_NAMES[fluid::FLOWING_WATER as usize],
            "minecraft:flowing_water"
        );
        assert_eq!(FLUID_NAMES[fluid::LAVA as usize], "minecraft:lava");
        assert_eq!(
            FLUID_NAMES[fluid::FLOWING_LAVA as usize],
            "minecraft:flowing_lava"
        );
    }

    #[test]
    fn outside_the_world_is_void_air_and_cannot_be_written() {
        let mut level = level_with_player(Aabb::new(0.0, 0.0, 0.0, 0.1, 0.1, 0.1));
        let above = BlockPos::new(0, 320, 0);
        assert_eq!(level.block_state(above), BlockState(blocks::VOID_AIR));
        assert!(!level.write_block(above, BlockState(blocks::STONE), 0));
        assert!(level.touched.borrow().is_empty());
    }

    #[test]
    fn changes_ignore_cells_written_back_to_the_scene() {
        let mut level = level_with_player(Aabb::new(0.0, 0.0, 0.0, 0.1, 0.1, 0.1));
        let kept = BlockPos::new(0, 64, 0);
        let removed = BlockPos::new(0, 65, 0);
        let scene = [Cell(0, 64, 0, blocks::STONE), Cell(0, 65, 0, blocks::STONE)];
        level.write_block(kept, BlockState(blocks::STONE), 0);
        level.write_block(removed, BlockState::AIR, 0);
        let air_written = BlockPos::new(0, 66, 0);
        level.write_block(air_written, BlockState::AIR, 0);
        assert_eq!(
            level.changes(&scene),
            BTreeMap::from([(removed, blocks::AIR)])
        );
    }

    #[test]
    fn touching_a_player_is_not_being_in_the_way() {
        let block = [Aabb::new(3.0, 64.0, 3.0, 4.0, 65.0, 4.0)];
        // 贴着方块的东面，浮点误差只有一位。
        let touching = level_with_player(Aabb::new(
            3.999_999_999_999_999_6,
            64.0,
            3.2,
            4.6,
            65.8,
            3.8,
        ));
        assert!(touching.is_unobstructed(&block));
        let overlapping = level_with_player(Aabb::new(3.9, 64.0, 3.2, 4.5, 65.8, 3.8));
        assert!(!overlapping.is_unobstructed(&block));
    }
}
