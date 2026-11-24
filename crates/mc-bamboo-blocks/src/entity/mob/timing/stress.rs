//! 生物压测（设计稿 `docs/mob-entity-design.md` 第 5 节第 3 步）。
//!
//! `#[ignore]` 的手工基准，只在 `cfg(test)` 下编译，正式构建里不存在。一条命令复现：
//!
//! ```text
//! cargo test -p mc-bamboo-blocks --release --lib mob_stress -- --ignored --nocapture
//! ```
//!
//! 场景、口径和报告见 `docs/mob-entity-design.md` 5.22 和
//! `docs/performance-optimizations.md` 25、26。这里只测量：所有地形、玩家和生物都由
//! 基准自己搭，走的是生产代码的同一套实体 tick 和并行驱动；计时开关关掉时
//! 正式二进制里这些探针全是空宏。

use std::{collections::BTreeMap, time::Instant};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{blocks, registries, spawning};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_world::{
    light::SectionPos,
    poi::{Occupancy, PoiStore},
};

use super::imp::{self, PARTS};
use crate::{
    Aabb, BlockEntities, BlockEvents, BlockGameRules, BlockState, BlockTicks, BlockWeather,
    EntityTables, Level, LevelRandom, LevelTables, RedstoneMemory, World, WorldBorder,
    entity::{
        Entities, Entity, EntityStore,
        ctx::PlayerView,
        mob::{MobType, natural},
        parallel::{
            Executor, InlineExecutor, ScopedThreads, WorkerPool, tick_entities_parallel_timed,
        },
        view::{BlockSource, SourceView, ViewFacts},
    },
    level::EquipTarget,
    state::{BED_PART, BedPart},
};

/// 每个场景、每个线程数跑几轮；报告取每轮中位数再取三轮中位数。
const ROUNDS: usize = 3;
/// 预热刻数：让牛散开、僵尸找到目标、自然刷怪到上限。
const WARM: usize = 200;
/// 测量刻数：刻级 p50/p95/p99 的样本数。
const MEASURE: usize = 300;
/// 线程数，0 是精确串行，其余是并行驱动。
const THREADS: [usize; 5] = [0, 1, 2, 4, 8];

const PART_NAMES: [&str; PARTS] = [
    "other",
    "travel",
    "block-effects",
    "push",
    "targeting",
    "goals",
    "navigation",
    "path",
    "brain-sensors",
    "brain-behaviors",
    "poi",
];

/// 把 `Level` / `BlockSource` 的 `occupancy: u8` 字节映回 `PoiStore` 的 `Occupancy`。
fn poi_occupancy(code: u8) -> Option<Occupancy> {
    match code {
        0 => Some(Occupancy::HasSpace),
        1 => Some(Occupancy::IsOccupied),
        2 => Some(Occupancy::Any),
        _ => None,
    }
}

/// 一段平地 / 障碍地形：按格查表的方块源，带玩家名单、真实 `PoiStore` 和 O(1) 高度图。
///
/// 精确驱动读 `StressLevel`（同一份数据），并行驱动的算阶段读 `StressSource`。
/// 两者对 `block_state`、POI 表、玩家名单和游戏规则给出完全一样的答案。
#[derive(Clone)]
struct Terrain {
    /// 每格一个状态；只存非默认（地面、墙、坑）的格子，其余按 `default` 给。
    cells: BTreeMap<BlockPos, u32>,
    default: u32,
    /// 每根柱子的地面高度：`world_surface_y` = 最高非空气方块的上面一格。
    surface: BTreeMap<(i32, i32), i32>,
    /// 没登记格子的柱子是否按 `floor_y` 的平地算（真地形场景为 false）。
    implicit_floor: bool,
    /// 玩家可以踩的地面，围栏 / 墙用它搭。
    floor_y: i32,
    /// 区块 → quart 群系格（真地形场景用；其余场景为空，按平原查表）。
    biomes: BTreeMap<(i32, i32), Vec<u32>>,
    /// 真实 POI 存储：摆地形和写方块时按 `on_block_state_change` 同步登记。
    poi: PoiStore,
    /// 场景起始游戏刻（白天村庄 2000、夜间村庄 13000，既有场景 0）。
    base_clock: i64,
    /// 天色暗度（既有场景 4 让僵尸不烧；白天村庄 0、夜间村庄 4）。
    sky_darken: i32,
    /// 玩家表和并行只读视图共用一份；绕圈走时两边每一刻看到同一个位置。
    players: std::sync::Arc<std::sync::Mutex<Vec<PlayerView>>>,
    rules: BlockGameRules,
}

impl Terrain {
    fn new(floor_y: i32) -> Self {
        Self {
            cells: BTreeMap::new(),
            default: blocks::AIR,
            surface: BTreeMap::new(),
            biomes: BTreeMap::new(),
            poi: PoiStore::new(),
            base_clock: 0,
            sky_darken: 4,
            implicit_floor: true,
            floor_y,
            players: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            rules: BlockGameRules::vanilla(),
        }
    }

    fn set(&mut self, pos: BlockPos, state: u32) {
        let old = self.cells.insert(pos, state).unwrap_or(self.default);
        let _ = self.poi.on_block_state_change(pos, old, state);
        let key = (pos.x, pos.z);
        if state != blocks::AIR {
            let entry = self.surface.entry(key).or_insert(self.floor_y);
            *entry = (*entry).max(pos.y + 1);
        }
    }

    /// 放一张红床（脚部在 `foot`，头部在 `head`，头部带 `PART=HEAD` 以登记 `home` POI）。
    fn place_bed(&mut self, foot: BlockPos, head: BlockPos) {
        let bed_foot = BlockState(blocks::RED_BED).with(BED_PART, BedPart::Foot).0;
        let bed_head = BlockState(blocks::RED_BED).with(BED_PART, BedPart::Head).0;
        self.set(foot, bed_foot);
        self.set(head, bed_head);
    }

    /// 铺一块 `radius × radius` 的平地（`y = floor_y - 1` 草方块，`floor_y` 是站的高度）。
    fn flat(&mut self, radius: i32) {
        for x in -radius..=radius {
            for z in -radius..=radius {
                self.set(BlockPos::new(x, self.floor_y - 1, z), blocks::GRASS_BLOCK);
            }
        }
    }

    fn state_at(&self, pos: BlockPos) -> u32 {
        self.cells.get(&pos).copied().unwrap_or(self.default)
    }

    /// 高度图在摆地形时增量维护，查询是 O(1)；挖掉的坑按平地算。
    fn surface_at(&self, x: i32, z: i32) -> i32 {
        self.surface
            .get(&(x, z))
            .copied()
            .unwrap_or(if self.implicit_floor {
                self.floor_y
            } else {
                -64
            })
    }

    fn add_player(&mut self, uid: i32, position: Vec3) {
        self.players.lock().expect("player mutex").push(PlayerView {
            uuid: None,
            uid,
            yaw: 0.0,
            pitch: 0.0,
            position,
            delta: Vec3::default(),
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::default(),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            combat: Default::default(),
            effects: Default::default(),
        });
    }
}

/// 精确驱动的世界：和 `TestLevel` 同一套接口，但高度图 O(1)、带玩家和自然刷怪表。
struct StressLevel {
    terrain: Terrain,
    random: LevelRandom,
    entities: Entities,
    redstone: RedstoneMemory,
    block_entities: BlockEntities,
    block_events: BlockEvents,
}

impl LevelTables for StressLevel {
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

impl EntityTables for StressLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }
    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }
}

impl Level for StressLevel {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !(-64..=319).contains(&pos.y) {
            return BlockState(blocks::VOID_AIR);
        }
        BlockState(self.terrain.state_at(pos))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        if !(-64..=319).contains(&pos.y) {
            return false;
        }
        self.terrain.set(pos, state.0);
        true
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        true
    }
    fn min_y(&self) -> i32 {
        -64
    }
    fn max_y(&self) -> i32 {
        319
    }
    fn random(&self) -> &LevelRandom {
        &self.random
    }
    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }
    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        true
    }
    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => 0,
            LightLayer::Block => 0,
        }
    }
    fn sign(&self, _pos: BlockPos) -> Option<SignContent> {
        None
    }
    fn set_sign(&mut self, _pos: BlockPos, _content: SignContent) -> bool {
        false
    }
    fn note_block_sound(&self, _pos: BlockPos) -> Option<String> {
        None
    }
    fn block_game_rules(&self) -> BlockGameRules {
        self.terrain.rules
    }
    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        for player in self.terrain.players.lock().expect("player mutex").iter() {
            visit(player);
        }
    }
    fn block_weather(&self) -> BlockWeather {
        BlockWeather::CLEAR
    }
    fn world_border(&self) -> WorldBorder {
        WorldBorder::default()
    }
    fn can_see_sky(&self, _pos: BlockPos) -> bool {
        false
    }
    /// 天色暗度：既有场景 4（僵尸不烧），白天村庄 0、夜间村庄 4。
    fn sky_darken(&self) -> i32 {
        self.terrain.sky_darken
    }
    fn monsters_burn(&self) -> bool {
        false
    }
    fn poi_type_at(&self, pos: BlockPos) -> Option<u8> {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.get_type(pos)
    }
    fn poi_count_in_range(&self, center: BlockPos, radius: i32, kind: u8, occupancy: u8) -> usize {
        super::span!(Poi);
        super::record_poi_query!();
        let Some(occupancy) = poi_occupancy(occupancy) else {
            return 0;
        };
        self.terrain
            .poi
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
        super::span!(Poi);
        super::record_poi_query!();
        let occupancy = poi_occupancy(occupancy)?;
        self.terrain.poi.find_closest(
            |found| found == kind,
            position_filter,
            center,
            radius,
            occupancy,
        )
    }
    fn poi_sections_to_village(&self, pos: BlockPos) -> i32 {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.sections_to_village(SectionPos::of(pos))
    }
    fn poi_is_close_to_village(&self, pos: BlockPos, sections: i32) -> bool {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.is_close_to_village(pos, sections)
    }
    fn biome_id(&self, pos: BlockPos) -> i32 {
        let chunk = (pos.x >> 4, pos.z >> 4);
        self.terrain
            .biomes
            .get(&chunk)
            .and_then(|cells| {
                let x = (pos.x >> 2) & 3;
                let z = (pos.z >> 2) & 3;
                cells.get((x * 16 + z) as usize).copied()
            })
            .unwrap_or(registries::biome::PLAINS as u32) as i32
    }
    /// `ChunkGenerator.getMobsAt`：按这一格的真群系查官方刷怪表（未建模的种类
    /// 仍参加抽签，权重和取数顺序不变）。
    fn natural_spawn_table(
        &self,
        pos: BlockPos,
        category: spawning::MobCategory,
    ) -> &'static [spawning::MobSpawnerDef] {
        spawning::for_biome_id(self.biome_id(pos)).mobs_to_spawn(category)
    }
    fn world_surface_y(&self, pos: BlockPos) -> i32 {
        self.terrain.surface_at(pos.x, pos.z)
    }
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        self.terrain.surface_at(pos.x, pos.z)
    }
    fn sea_level(&self) -> i32 {
        63
    }
    fn mob_loot_drops(
        &mut self,
        _loot: crate::entity::mob::MobDeathLoot,
        _origin: Vec3,
    ) -> Vec<ItemStack> {
        Vec::new()
    }
    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        Vec::new()
    }
}

/// 并行算阶段的只读方块视图：和 [`StressLevel`] 共用一份地形数据。
/// `clock` 由压测每刻写入（原子），让 `SourceView::game_time()` 和精确驱动的
/// `World::game_time()` 一致；否则 `MeleeAttackGoal.canUse` 的 20 刻调度只在
/// 精确驱动里生效，两种线程数就量不到同一种寻路负载。
struct StressSource {
    terrain: Terrain,
    clock: std::sync::atomic::AtomicI64,
}

impl StressSource {
    fn new(terrain: Terrain) -> Self {
        Self {
            terrain,
            clock: std::sync::atomic::AtomicI64::new(0),
        }
    }
}

impl BlockSource for StressSource {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !(-64..=319).contains(&pos.y) {
            return BlockState(blocks::VOID_AIR);
        }
        BlockState(self.terrain.state_at(pos))
    }

    fn facts(&self) -> ViewFacts {
        let clock = self.clock.load(std::sync::atomic::Ordering::Relaxed);
        ViewFacts {
            min_y: -64,
            max_y: 319,
            rules: self.terrain.rules,
            sky_brightness: 0,
            block_brightness: 0,
            sky_darken: self.terrain.sky_darken,
            monsters_burn: false,
            game_time: clock,
            clock_ticks: self.terrain.base_clock + clock,
            ..ViewFacts::default()
        }
    }

    fn poi_type_at(&self, pos: BlockPos) -> Option<u8> {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.get_type(pos)
    }

    fn poi_count_in_range(&self, center: BlockPos, radius: i32, kind: u8, occupancy: u8) -> usize {
        super::span!(Poi);
        super::record_poi_query!();
        let Some(occupancy) = poi_occupancy(occupancy) else {
            return 0;
        };
        self.terrain
            .poi
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
        super::span!(Poi);
        super::record_poi_query!();
        let occupancy = poi_occupancy(occupancy)?;
        self.terrain.poi.find_closest(
            |found| found == kind,
            position_filter,
            center,
            radius,
            occupancy,
        )
    }

    fn poi_sections_to_village(&self, pos: BlockPos) -> i32 {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.sections_to_village(SectionPos::of(pos))
    }

    fn poi_is_close_to_village(&self, pos: BlockPos, sections: i32) -> bool {
        super::span!(Poi);
        super::record_poi_query!();
        self.terrain.poi.is_close_to_village(pos, sections)
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        for player in self.terrain.players.lock().expect("player mutex").iter() {
            visit(player);
        }
    }

    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        self.terrain.surface_at(pos.x, pos.z)
    }

    // 不覆盖 `section_has_collision`：让并行算阶段和精确驱动用同样的碰撞扫描，
    // 否则两种线程数看到的墙不一样，测出来的寻路负载也不可比。
}

/// 场景名和规模。`mixed` 跑自然刷怪，稳定后量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    Pasture500,
    Pasture2000,
    Chase,
    ChaseObstacles,
    /// 400 只僵尸追 4 名分散的玩家：寻路请求多、目标多，测去重和优先级。
    ChaseCrowd,
    Tower1x1,
    Tower3x3,
    Mixed,
    /// 玩家附近少、远处多：2000 头牛铺在半径 128 格内，一名玩家在中心。
    Distant2000,
    /// 40 只村民的常规村庄（床、工作站、钟、1 名玩家，白天）。
    Village40,
    /// 200 只村民的大型村庄（床、工作站、4 口钟、1 名玩家，白天）。
    Village200,
    /// 100 只村民的 1×1 交易大厅（双排隔间 + 工作站 + 床 + 1 名玩家，白天）。
    Hall100,
    /// 可调规模的 1×1 交易大厅（`MOB_STRESS_HALL_N`，缺省 250 只）。
    HallN,
    /// 100 只村民挤在 5×5 石墙繁殖舱内（周边摆床，关挤压伤害以测稳定负载）。
    Pod100,
    /// 60 只失业村民争夺 12 个工作站（60 张床 + 1 口钟 + 12 个工作站）。
    Unemployed60,
    /// 100 只村民的夜间村庄（`base_clock=13000`, `sky_darken=4`, 110 张床 + 工作站 + 钟）。
    NightVillage100,
}

impl Scene {
    const ALL: [Self; 16] = [
        Self::Pasture500,
        Self::Pasture2000,
        Self::Chase,
        Self::ChaseObstacles,
        Self::ChaseCrowd,
        Self::Tower1x1,
        Self::Tower3x3,
        Self::Mixed,
        Self::Distant2000,
        Self::Village40,
        Self::Village200,
        Self::Hall100,
        Self::HallN,
        Self::Pod100,
        Self::Unemployed60,
        Self::NightVillage100,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Pasture500 => "pasture-500",
            Self::Pasture2000 => "pasture-2000",
            Self::Chase => "chase-flat",
            Self::ChaseObstacles => "chase-obstacles",
            Self::ChaseCrowd => "chase-crowd",
            Self::Tower1x1 => "tower-1x1",
            Self::Tower3x3 => "tower-3x3",
            Self::Mixed => "mixed-natural",
            Self::Distant2000 => "distant-2000",
            Self::Village40 => "village-40",
            Self::Village200 => "village-200",
            Self::Hall100 => "hall-100",
            Self::HallN => "hall-n",
            Self::Pod100 => "pod-100",
            Self::Unemployed60 => "unemployed-60",
            Self::NightVillage100 => "night-village-100",
        }
    }

    const WORKSTATIONS: [u32; 13] = [
        blocks::COMPOSTER,
        blocks::LECTERN,
        blocks::BARREL,
        blocks::BLAST_FURNACE,
        blocks::SMOKER,
        blocks::CARTOGRAPHY_TABLE,
        blocks::BREWING_STAND,
        blocks::FLETCHING_TABLE,
        blocks::CAULDRON,
        blocks::STONECUTTER,
        blocks::LOOM,
        blocks::SMITHING_TABLE,
        blocks::GRINDSTONE,
    ];

    /// 村庄通用搭法：`count` 只村民、中心钟、若干张床和工作站、1 名玩家。
    fn village(count: usize, workstations: usize, night: bool) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.base_clock = if night { 13_000 } else { 2_000 };
        terrain.sky_darken = if night { 4 } else { 0 };
        let half = if count >= 150 { 80 } else { 48 };
        terrain.flat(half);
        // 中心广场放钟，大型村庄四象限各加一口钟。
        terrain.set(BlockPos::new(0, floor, 0), blocks::BELL);
        if count >= 150 {
            for (bx, bz) in [(-28, -28), (28, -28), (-28, 28), (28, 28)] {
                terrain.set(BlockPos::new(bx, floor, bz), blocks::BELL);
            }
        }
        // 环形屋区摆床（略多于村民数）和工作站。
        let beds = count + (count / 10).max(4);
        let bed_cols = (beds as f64).sqrt().ceil() as i32;
        for i in 0..beds {
            let gx = (i as i32 % bed_cols) - bed_cols / 2;
            let gz = (i as i32 / bed_cols) - bed_cols / 2;
            let x = gx * 5 + 3;
            let z = gz * 5 + 3;
            terrain.place_bed(BlockPos::new(x, floor, z), BlockPos::new(x, floor, z + 1));
        }
        let ws_cols = (workstations.max(1) as f64).sqrt().ceil() as i32;
        for i in 0..workstations {
            let gx = (i as i32 % ws_cols) - ws_cols / 2;
            let gz = (i as i32 / ws_cols) - ws_cols / 2;
            let x = gx * 5 + 1;
            let z = gz * 5 + 1;
            if x == 0 && z == 0 {
                continue;
            }
            terrain.set(
                BlockPos::new(x, floor, z),
                Self::WORKSTATIONS[i % Self::WORKSTATIONS.len()],
            );
        }
        terrain.add_player(1, Vec3::new(0.5, f64::from(floor), 6.5));
        let side = (count as f64).sqrt().ceil() as i32;
        let mut mobs = Vec::with_capacity(count);
        for i in 0..count {
            let gx = (i as i32 % side) - side / 2;
            let gz = (i as i32 / side) - side / 2;
            mobs.push((
                MobType::Villager,
                Vec3::new(
                    f64::from(gx * 4) + 2.5,
                    f64::from(floor),
                    f64::from(gz * 4) + 2.5,
                ),
            ));
        }
        (terrain, mobs)
    }

    /// 交易大厅：两排 `count/2` 个 1×1 石墙隔间，面前 1 个工作站、身后 1 张床，玩家在过道里。
    fn hall(count: usize) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.base_clock = 2_000;
        terrain.sky_darken = 0;
        let per_row = count.div_ceil(2) as i32;
        let half = (per_row + 16).max(32);
        terrain.flat(half);
        terrain.set(BlockPos::new(0, floor, 0), blocks::BELL);
        let mut mobs = Vec::with_capacity(count);
        for i in 0..count {
            let row = (i % 2) as i32;
            let col = (i / 2) as i32 - per_row / 2;
            let x = col * 2;
            let stall_z = if row == 0 { -3 } else { 3 };
            let ws_z = if row == 0 { -2 } else { 2 };
            let bed_z = if row == 0 { -5 } else { 5 };
            // 隔间左右和后方砌两格高石墙，把村民关在 1×1 格里。
            for dx in [-1, 1] {
                for y in floor..floor + 2 {
                    terrain.set(BlockPos::new(x + dx, y, stall_z), blocks::STONE);
                }
            }
            for y in floor..floor + 2 {
                terrain.set(
                    BlockPos::new(x, y, stall_z + if row == 0 { -1 } else { 1 }),
                    blocks::STONE,
                );
            }
            terrain.set(
                BlockPos::new(x, floor, ws_z),
                Self::WORKSTATIONS[i % Self::WORKSTATIONS.len()],
            );
            terrain.place_bed(
                BlockPos::new(x, floor, bed_z),
                BlockPos::new(x, floor, bed_z + if row == 0 { -1 } else { 1 }),
            );
            mobs.push((
                MobType::Villager,
                Vec3::new(
                    f64::from(x) + 0.5,
                    f64::from(floor),
                    f64::from(stall_z) + 0.5,
                ),
            ));
        }
        terrain.add_player(1, Vec3::new(0.5, f64::from(floor), 0.5));
        (terrain, mobs)
    }

    /// 繁殖舱 / 刷铁机密闭舱：100 只村民挤在 5×5 石墙内，四周摆 100 张床，关挤压伤害保数量稳定。
    fn pod(count: usize) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.base_clock = 2_000;
        terrain.sky_darken = 0;
        terrain.flat(32);
        terrain.rules.max_entity_cramming = 0;
        let side = 5;
        for x in -1..=side {
            for z in [-1, side] {
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(x, y, z), blocks::STONE);
                }
            }
        }
        for z in -1..=side {
            for x in [-1, side] {
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(x, y, z), blocks::STONE);
                }
            }
        }
        terrain.set(BlockPos::new(side + 3, floor, 2), blocks::BELL);
        for i in 0..count {
            let bx = (i as i32 % 10) * 2 - 10;
            let bz = (i as i32 / 10) * 3 + side + 4;
            terrain.place_bed(
                BlockPos::new(bx, floor, bz),
                BlockPos::new(bx, floor, bz + 1),
            );
        }
        terrain.add_player(1, Vec3::new(f64::from(side) + 4.5, f64::from(floor), 2.5));
        let mut mobs = Vec::with_capacity(count);
        for i in 0..count {
            let x = (i as i32) % side;
            let z = (i as i32 / side) % side;
            mobs.push((
                MobType::Villager,
                Vec3::new(f64::from(x) + 0.5, f64::from(floor), f64::from(z) + 0.5),
            ));
        }
        (terrain, mobs)
    }

    /// 牧场四种动物混养，围栏里按网格摆开；玩家站在旁边。
    fn pasture(count: usize) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        let side = (count as f64).sqrt().ceil() as i32;
        let half = side + 8;
        terrain.flat(half);
        // 围栏：一圈两层高的橡木栅栏，把动物圈在里面（不改移动逻辑，只给碰撞）。
        for x in -half..=half {
            for z in [-half, half] {
                for y in [floor, floor + 1] {
                    terrain.set(BlockPos::new(x, y, z), blocks::OAK_FENCE);
                }
            }
        }
        for z in -half..=half {
            for x in [-half, half] {
                for y in [floor, floor + 1] {
                    terrain.set(BlockPos::new(x, y, z), blocks::OAK_FENCE);
                }
            }
        }
        terrain.add_player(1, Vec3::new(0.5, f64::from(floor), 0.5));
        let kinds = [MobType::Cow, MobType::Sheep, MobType::Pig, MobType::Chicken];
        let mut mobs = Vec::with_capacity(count);
        for i in 0..count {
            let x = (i as i32 % side) - side / 2;
            let z = (i as i32 / side) - side / 2;
            mobs.push((
                kinds[i % kinds.len()],
                Vec3::new(f64::from(x) + 0.5, f64::from(floor), f64::from(z) + 0.5),
            ));
        }
        (terrain, mobs)
    }

    /// 200 只僵尸追一个绕圈走的玩家；`obstacles` 加墙和坑让 A\* 真有活干。
    fn chase(obstacles: bool) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        // 平地要盖住最远的僵尸（半径 80），否则它们掉出世界，预热里就少一批。
        let half = 96;
        terrain.flat(half);
        terrain.add_player(1, Vec3::new(0.5, f64::from(floor), 0.5));
        if obstacles {
            // 四道三格高的石墙，中间留缺口；A\* 要绕过它们。
            for x in -30_i32..=30 {
                if x.abs() < 6 {
                    continue;
                }
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(x, y, -16), blocks::STONE);
                    terrain.set(BlockPos::new(x, y, 16), blocks::STONE);
                }
            }
            for z in -30_i32..=30 {
                if z.abs() < 6 {
                    continue;
                }
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(-16, y, z), blocks::STONE);
                    terrain.set(BlockPos::new(16, y, z), blocks::STONE);
                }
            }
            // 几个 2×2 的坑，把地面挖掉，考验落地判定。
            for (x, z) in [(24, 0), (-24, 0), (0, 24), (0, -24)] {
                for dx in 0..2 {
                    for dz in 0..2 {
                        terrain.set(BlockPos::new(x + dx, floor - 1, z + dz), blocks::AIR);
                    }
                }
            }
        }
        let mut mobs = Vec::with_capacity(200);
        for i in 0..200 {
            // 环形铺开，玩家在中心；一半在 32 格内、一半在 64 格外，三档都有样本。
            let angle = (i as f64) * std::f64::consts::TAU / 200.0;
            // 半径 8..80：32 格内、32–64、64–80 三档都有样本，且不超过
            // 官方 `despawnDistance` 128 格，僵尸不会在预热里消失。
            let radius = 8.0 + (i % 9) as f64 * 9.0;
            mobs.push((
                MobType::Zombie,
                Vec3::new(
                    angle.cos() * radius + 0.5,
                    f64::from(floor),
                    angle.sin() * radius + 0.5,
                ),
            ));
        }
        (terrain, mobs)
    }

    /// 400 只僵尸追 4 名分散的玩家：每只僵尸按 10 格网格铺在 400×400 的平地上，
    /// 四名玩家各站一角，僵尸各自挑最近的一名。
    fn crowd() -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.flat(200);
        for (index, (x, z)) in [(-36.5, -36.5), (40.5, -36.5), (-36.5, 40.5), (40.5, 40.5)]
            .into_iter()
            .enumerate()
        {
            terrain.add_player(index as i32 + 1, Vec3::new(x, f64::from(floor), z));
        }
        let mut mobs = Vec::with_capacity(400);
        for i in 0..400 {
            let x = (i % 20) * 5 - 50;
            let z = (i / 20) * 5 - 50;
            mobs.push((
                MobType::Zombie,
                Vec3::new(f64::from(x) + 0.5, f64::from(floor), f64::from(z) + 0.5),
            ));
        }
        (terrain, mobs)
    }

    /// 玩家附近少、远处多：2000 头牛按网格铺在半径 128 格内，三档都有大量样本。
    /// 没有围栏，牛会缓慢散开；玩家站在出生点不绕圈。
    fn distant(count: usize) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.flat(140);
        terrain.add_player(1, Vec3::new(0.5, f64::from(floor), 0.5));
        let side = (count as f64).sqrt().ceil() as i32;
        let step = 256 / side;
        let mut mobs = Vec::with_capacity(count);
        for i in 0..count {
            let x = (i as i32 % side) * step - 128;
            let z = (i as i32 / side) * step - 128;
            mobs.push((
                MobType::Cow,
                Vec3::new(f64::from(x) + 0.5, f64::from(floor), f64::from(z) + 0.5),
            ));
        }
        (terrain, mobs)
    }

    /// 刷怪塔：200–500 只生物挤在 `side × side` 格里，四周是墙。
    fn tower(side: i32) -> (Terrain, Vec<(MobType, Vec3)>) {
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        terrain.flat(24);
        // 挤压伤害会把塔里的生物一批批打死，测不到稳定的推挤成本。关掉规则
        // `maxEntityCramming`（设为 0 = 不限），单独在「挤压上限触发次数」里记
        // 默认规则下同一场景的数字，见报告 5.9。
        terrain.rules.max_entity_cramming = 0;
        for x in -1..=side {
            for z in [-1, side] {
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(x, y, z), blocks::STONE);
                }
            }
        }
        for z in -1..=side {
            for x in [-1, side] {
                for y in floor..floor + 3 {
                    terrain.set(BlockPos::new(x, y, z), blocks::STONE);
                }
            }
        }
        terrain.add_player(1, Vec3::new(8.5, f64::from(floor), 8.5));
        let mut mobs = Vec::with_capacity(350);
        for i in 0..350 {
            let x = i % side;
            let z = i / side % side;
            mobs.push((
                MobType::Cow,
                Vec3::new(f64::from(x) + 0.5, f64::from(floor), f64::from(z) + 0.5),
            ));
        }
        (terrain, mobs)
    }

    /// 混合：两名玩家、自然刷怪开、真的噪声主世界地形（`minecraft:overworld`，
    /// 种子 20261116，跑 `mc_bamboo_worldgen::chunk::Generator` 到 CARVERS）。
    /// 刷怪走 `natural::spawn_for_chunk` 的真实路径；地形只在搭场景时生成一次，
    /// 不进每刻计时。
    fn mixed() -> (Terrain, Vec<(MobType, Vec3)>) {
        use mc_bamboo_worldgen::{Worldgen, chunk};
        let floor = 64;
        let mut terrain = Terrain::new(floor);
        // 9×9 个区块（±72 格），覆盖两名玩家 8 区块的刷怪方形。
        let worldgen = Worldgen::shared().expect("worldgen data");
        let generator = chunk::Generator::new(worldgen, 20_261_116).expect("generator");
        for cx in -5..=5 {
            for cz in -5..=5 {
                let region = generator.biome_region(cx, cz).expect("biomes");
                let out = generator
                    .terrain_with_region_and_post_processing(
                        cx,
                        cz,
                        chunk::Stage::Carvers,
                        None,
                        &region,
                    )
                    .expect("terrain");
                // `Terrain` 只存非默认格子：把地表上下的实心方块存进去，
                // 空气格交给 `state_at` 的缺省值，避免 9 个区块 × 384 层的全量拷贝。
                // 群系：区块每个 section 64 个 quart 格；刷怪用 y=64 附近的 section。
                let quart_y =
                    ((64 - (-64)) >> 2).clamp(0, region.center().sections().len() as i32 * 4 - 1);
                let section = (quart_y / 4) as usize;
                terrain
                    .biomes
                    .insert((cx, cz), region.center().sections()[section].clone());
                for (layer, states) in out.blocks.iter().enumerate() {
                    let y = -64 + layer as i32;
                    for (index, &state) in states.iter().enumerate() {
                        if state == blocks::AIR {
                            continue;
                        }
                        let x = cx * 16 + (index % 16) as i32;
                        let z = cz * 16 + (index / 16) as i32;
                        terrain.set(BlockPos::new(x, y, z), state);
                    }
                }
            }
        }
        // 玩家落在出生点附近地表高度上；`StressLevel::world_surface_y` 由地形表算。
        terrain.implicit_floor = false;
        let y1 = terrain.surface_at(-24, 0);
        let y2 = terrain.surface_at(24, 0);
        terrain.add_player(1, Vec3::new(-24.5, f64::from(y1), 0.5));
        terrain.add_player(2, Vec3::new(24.5, f64::from(y2), 0.5));
        (terrain, Vec::new())
    }

    fn build(self) -> (Terrain, Vec<(MobType, Vec3)>, bool) {
        match self {
            Self::Pasture500 => {
                let (terrain, mobs) = Self::pasture(500);
                (terrain, mobs, false)
            }
            Self::Pasture2000 => {
                let (terrain, mobs) = Self::pasture(2000);
                (terrain, mobs, false)
            }
            Self::Chase => {
                let (terrain, mobs) = Self::chase(false);
                (terrain, mobs, false)
            }
            Self::ChaseObstacles => {
                let (terrain, mobs) = Self::chase(true);
                (terrain, mobs, false)
            }
            Self::ChaseCrowd => {
                let (terrain, mobs) = Self::crowd();
                (terrain, mobs, false)
            }
            Self::Tower1x1 => {
                let (terrain, mobs) = Self::tower(1);
                (terrain, mobs, false)
            }
            Self::Tower3x3 => {
                let (terrain, mobs) = Self::tower(3);
                (terrain, mobs, false)
            }
            Self::Mixed => {
                let (terrain, mobs) = Self::mixed();
                (terrain, mobs, true)
            }
            Self::Distant2000 => {
                let (terrain, mobs) = Self::distant(2000);
                (terrain, mobs, false)
            }
            Self::Village40 => {
                let (terrain, mobs) = Self::village(40, 40, false);
                (terrain, mobs, false)
            }
            Self::Village200 => {
                let (terrain, mobs) = Self::village(200, 200, false);
                (terrain, mobs, false)
            }
            Self::Hall100 => {
                let (terrain, mobs) = Self::hall(100);
                (terrain, mobs, false)
            }
            Self::HallN => {
                let n = std::env::var("MOB_STRESS_HALL_N")
                    .ok()
                    .and_then(|raw| raw.parse::<usize>().ok())
                    .filter(|&n| n > 0)
                    .unwrap_or(250);
                let (terrain, mobs) = Self::hall(n);
                (terrain, mobs, false)
            }
            Self::Pod100 => {
                let (terrain, mobs) = Self::pod(100);
                (terrain, mobs, false)
            }
            Self::Unemployed60 => {
                let (terrain, mobs) = Self::village(60, 12, false);
                (terrain, mobs, false)
            }
            Self::NightVillage100 => {
                let (terrain, mobs) = Self::village(100, 100, true);
                (terrain, mobs, false)
            }
        }
    }
}

/// 真噪声地形生成一次要几十秒，缓存起来给每个场景 / 线程 / 轮次复用。
static MIXED_TERRAIN: std::sync::OnceLock<Terrain> = std::sync::OnceLock::new();

/// 一次测量的结果。
struct Sample {
    /// 每刻墙上时间（纳秒），`MEASURE` 个样本。
    ticks_ns: Vec<u64>,
    /// 分段自身时间（纳秒），按 [`PARTS`]。
    parts: [u64; PARTS],
    /// 并行驱动的四段（复制 / 算 / 发布 / 落地，含 `EntityStore` 框架开销）。
    framework: [u64; 4],
    counters: imp::Snapshot,
    entities: usize,
}

/// 一个场景 × 一个线程数的一轮：搭世界、预热、量 `MEASURE` 刻。
fn run_one(scene: Scene, threads: usize, natural: bool, seed: u64, scoped: bool) -> Sample {
    let (mut terrain, mobs, _) = if scene == Scene::Mixed {
        (
            MIXED_TERRAIN.get_or_init(|| Scene::mixed().0).clone(),
            Vec::new(),
            true,
        )
    } else {
        scene.build()
    };
    // A/B 用：打开挤压上限（Paper 的缺省值 8）。
    if let Ok(raw) = std::env::var("MOB_STRESS_MAX_COLLISIONS") {
        terrain.rules.max_entity_collisions = raw.parse().unwrap_or(8);
    }
    // A/B 用：`MOB_STRESS_THROTTLE=1` 打开三档降频（推荐值 32 / 64）。
    if std::env::var_os("MOB_STRESS_THROTTLE").is_some() {
        terrain.rules.mob_throttle_distance = 32;
        terrain.rules.mob_sleep_distance = 64;
        terrain.rules.mob_wakeups_scale = 1;
    }
    // A/B 用：`MOB_STRESS_PATHFINDING=<预算>` 打开独立寻路阶段（0 = 缺省预算）。
    if let Ok(raw) = std::env::var("MOB_STRESS_PATHFINDING") {
        terrain.rules.mob_pathfinding_phase = true;
        terrain.rules.mob_pathfinding_budget = raw.parse().unwrap_or(0);
    }
    let mut level = StressLevel {
        terrain: terrain.clone(),
        random: LevelRandom::new(seed as i64),
        entities: Entities::with_next_id(2),
        redstone: RedstoneMemory::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
    };
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for (kind, position) in &mobs {
            let uid = world.spawn_entity(Entity::by_mob_type(*kind, *position));
            // 追玩家场景：把玩家设成每只僵尸的攻击目标，近战目标每刻才会发寻路请求。
            // `NearestAttackableTarget` 自己也会挑到他，但那样受 10 刻调度和视线限制；
            // 显式设目标等价于「已经看见玩家」，让 A* 稳定有活干。
            if matches!(scene, Scene::Chase | Scene::ChaseObstacles) {
                world.with_entity_mut(uid, &mut |entity| {
                    entity.with_mob(|_, mob| mob.target = Some(1));
                });
            } else if scene == Scene::ChaseCrowd {
                let nearest =
                    if position.x < 4.0 { 1 } else { 2 } + if position.z > 4.0 { 2 } else { 0 };
                world.with_entity_mut(uid, &mut |entity| {
                    entity.with_mob(|_, mob| mob.target = Some(nearest));
                });
            }
        }
    }
    let source = SourceView::new(StressSource::new(terrain));
    let executor: Box<dyn Executor> = match (threads, scoped) {
        (0 | 1, _) => Box::new(InlineExecutor),
        (_, true) => Box::new(ScopedThreads { threads }),
        (_, false) => Box::new(WorkerPool::new("mob-stress", threads).expect("起实体线程池")),
    };
    // 混合场景的自然刷怪：每刻对每名玩家周围 8 区块的方形各跑一次
    // `natural::spawn_for_chunk`，直到刷怪上限稳定。
    let spawn_chunks: Vec<ChunkPos> = if natural {
        let mut set = std::collections::BTreeSet::new();
        for player in level.terrain.players.lock().expect("player mutex").iter() {
            let center = ChunkPos::containing(player.position.x, player.position.z);
            for dx in -8..=8 {
                for dz in -8..=8 {
                    set.insert(ChunkPos::new(center.x + dx, center.z + dz));
                }
            }
        }
        set.into_iter().collect()
    } else {
        Vec::new()
    };

    let tick = |level: &mut StressLevel,
                ticks: &mut BlockTicks,
                source: &SourceView<StressSource>,
                executor: &dyn Executor,
                game_time: i64| {
        // `MeleeAttackGoal.canUse` 按 `gameTime - lastCanUseCheck >= 20` 调度；
        // 时钟不走的话近战目标永远不启动，寻路就没活干。和真实服务端一样每刻推进。
        ticks.set_time(game_time);
        source
            .0
            .clock
            .store(game_time, std::sync::atomic::Ordering::Relaxed);
        if natural {
            let mut world = World::new(&mut *level, &mut *ticks);
            let params = natural::SpawnParams {
                spawnable_chunk_count: spawn_chunks.len() as i32,
                spawn_enemies: true,
                per_player_cap: true,
                respawn_pos: None,
                sea_level: 63,
            };
            let mut state = natural::SpawnState::create(&world, spawn_chunks.len() as i32);
            let categories = state.filtered_categories(true, game_time % 400 == 0);
            let mut chunks = spawn_chunks.clone();
            crate::shuffle(world.random(), &mut chunks);
            for chunk in chunks {
                natural::spawn_for_chunk(&mut world, chunk, &mut state, &params, &categories);
            }
        }
        if threads == 0 {
            let mut world = World::new(&mut *level, &mut *ticks);
            world.tick_entities();
            [0; 4]
        } else {
            let mut world = World::new(&mut *level, &mut *ticks);
            let times = tick_entities_parallel_timed(source, &mut world, executor);
            [
                times.clone.as_nanos() as u64,
                times.compute.as_nanos() as u64,
                // `publish` 里含 `begin_entity_tick` 和只读名单，是框架开销的一部分。
                times.publish.as_nanos() as u64,
                times.apply.as_nanos() as u64,
            ]
        }
    };

    // 预热。玩家绕圈走，僵尸的目标跟着动；每 20 刻换一个位置。
    let move_players = |level: &mut StressLevel, i: usize| {
        let angle = (i as f64) * std::f64::consts::TAU / 120.0;
        let mut players = level.terrain.players.lock().expect("player mutex");
        if natural || matches!(scene, Scene::Chase | Scene::ChaseObstacles) {
            if let Some(player) = players.first_mut() {
                player.position = Vec3::new(angle.cos() * 10.0, 64.0, angle.sin() * 10.0);
            }
        } else if scene == Scene::ChaseCrowd {
            // 四名玩家各自绕自己的角点转，僵尸的目标一直动，寻路请求持续有活干。
            let corners = [(-36.5, -36.5), (40.5, -36.5), (-36.5, 40.5), (40.5, 40.5)];
            for (player, (x, z)) in players.iter_mut().zip(corners) {
                player.position = Vec3::new(x + angle.cos() * 8.0, 64.0, z + angle.sin() * 8.0);
            }
        }
    };
    for i in 0..WARM {
        move_players(&mut level, i);
        tick(&mut level, &mut ticks, &source, executor.as_ref(), i as i64);
    }
    // 清掉预热期间的计数。
    imp::take();
    let _ = imp::take_all_counters();

    let mut ticks_ns = Vec::with_capacity(MEASURE);
    let mut framework = [0_u64; 4];
    for i in 0..MEASURE {
        move_players(&mut level, WARM + i);
        let start = Instant::now();
        let times = tick(
            &mut level,
            &mut ticks,
            &source,
            executor.as_ref(),
            (WARM + i) as i64,
        );
        ticks_ns.push(start.elapsed().as_nanos() as u64);
        for (to, from) in framework.iter_mut().zip(times) {
            *to += from;
        }
    }
    let parts = imp::take();
    let counters = imp::take_all_counters();
    let entities = level.entities.len();
    Sample {
        ticks_ns,
        parts,
        framework,
        counters,
        entities,
    }
}

fn median(mut values: Vec<u64>) -> f64 {
    values.sort_unstable();
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        values[n / 2] as f64
    } else {
        (values[n / 2 - 1] + values[n / 2]) as f64 / 2.0
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * (sorted.len() - 1) as f64).ceil() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// 把三轮的分位数 / 分段 / 计数各自再取中位数。
fn summarize(scene: Scene, threads: usize, rounds: Vec<Sample>) -> String {
    let entities = rounds.iter().map(|s| s.entities as u64).collect::<Vec<_>>();
    let mut tick_stats = Vec::new();
    let mut part_millis: [Vec<f64>; PARTS] = std::array::from_fn(|_| Vec::new());
    let mut framework_millis: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
    let mut path_stats = Vec::new();
    let mut push = Vec::new();
    let mut cramming = Vec::new();
    let mut brain_ticks = Vec::new();
    let mut sensor_scans = Vec::new();
    let mut behavior_starts = Vec::new();
    let mut poi_queries = Vec::new();
    let mut band_ticks = Vec::new();
    let mut band_ms = Vec::new();
    let mut kinds: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
    for sample in &rounds {
        let mut sorted: Vec<f64> = sample.ticks_ns.iter().map(|&ns| ns as f64 / 1e6).collect();
        sorted.sort_by(f64::total_cmp);
        tick_stats.push([
            median(sample.ticks_ns.clone()) / 1e6,
            percentile(&sorted, 0.95),
            percentile(&sorted, 0.99),
        ]);
        for (i, part) in part_millis.iter_mut().enumerate() {
            part.push(sample.parts[i] as f64 / 1e6 / MEASURE as f64);
        }
        for (i, framework) in framework_millis.iter_mut().enumerate() {
            framework.push(sample.framework[i] as f64 / 1e6 / MEASURE as f64);
        }
        let p = &sample.counters.paths;
        path_stats.push([
            p.requests as f64 / MEASURE as f64,
            p.nodes as f64 / MEASURE as f64,
            p.succeeded as f64 / MEASURE as f64,
            p.failed as f64 / MEASURE as f64,
            p.duplicates as f64 / MEASURE as f64,
            p.succeeded as f64 / p.requests.max(1) as f64,
            p.global_duplicates as f64 / MEASURE as f64,
        ]);
        push.push(sample.counters.push_commands as f64 / MEASURE as f64);
        cramming.push(sample.counters.cramming_hits as f64 / MEASURE as f64);
        brain_ticks.push(sample.counters.brain_ticks as f64 / MEASURE as f64);
        sensor_scans.push(sample.counters.sensor_scans as f64 / MEASURE as f64);
        behavior_starts.push(sample.counters.behavior_starts as f64 / MEASURE as f64);
        poi_queries.push(sample.counters.poi_queries as f64 / MEASURE as f64);
        band_ticks.push(
            sample
                .counters
                .bands
                .map(|b| b.ticks as f64 / MEASURE as f64),
        );
        band_ms.push(
            sample
                .counters
                .bands
                .map(|b| b.parts.iter().sum::<u64>() as f64 / 1e6 / MEASURE as f64),
        );
        for (kind, stats) in sample.counters.kinds() {
            let total = stats.parts.iter().sum::<u64>() as f64 / 1e6 / MEASURE as f64;
            kinds.entry(*kind).or_default().push(total);
        }
    }
    let med = |values: &mut Vec<f64>| {
        values.sort_by(f64::total_cmp);
        let n = values.len();
        if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        }
    };
    let med_u64 = |values: &mut Vec<u64>| {
        values.sort_unstable();
        median(values.clone())
    };
    let _ = med_u64;
    let mut out = String::new();
    let line = |values: &mut Vec<f64>| format!("{:.1}", med(values));
    out += &format!("\n== {} threads={} ==\n", scene.name(), threads);
    out += &format!("entities={} ", median(entities.clone()) as usize);
    let mut p50 = tick_stats
        .iter()
        .map(|s| (s[0] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut p95 = tick_stats
        .iter()
        .map(|s| (s[1] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut p99 = tick_stats
        .iter()
        .map(|s| (s[2] * 1e3) as u64)
        .collect::<Vec<_>>();
    out += &format!(
        "tick-ms p50={:.3} p95={:.3} p99={:.3}\n",
        median(std::mem::take(&mut p50)) / 1e3,
        median(std::mem::take(&mut p95)) / 1e3,
        median(std::mem::take(&mut p99)) / 1e3
    );
    out += "parts-ms/tick: ";
    for i in 0..PARTS {
        out += &format!("{}={:.3} ", PART_NAMES[i], med(&mut part_millis[i]));
    }
    out += "\nframework-ms/tick: ";
    for (i, name) in ["clone", "compute", "publish", "apply"].iter().enumerate() {
        out += &format!("{name}={:.3} ", med(&mut framework_millis[i]));
    }
    out += "\n";
    let mut req = path_stats
        .iter()
        .map(|s| (s[0] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut nodes = path_stats
        .iter()
        .map(|s| (s[1] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut ok = path_stats
        .iter()
        .map(|s| (s[2] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut fail = path_stats
        .iter()
        .map(|s| (s[3] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut dup = path_stats
        .iter()
        .map(|s| (s[4] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut global_dup = path_stats
        .iter()
        .map(|s| (s[6] * 1e3) as u64)
        .collect::<Vec<_>>();
    let mut success_rate = path_stats
        .iter()
        .map(|s| (s[5] * 1e6) as u64)
        .collect::<Vec<_>>();
    out += &format!(
        "paths/tick requests={:.1} nodes={:.1} ok={:.1} fail={:.1} dedup-local={:.1} dedup-global={:.1} found={:.1}%\n",
        median(std::mem::take(&mut req)) / 1e3,
        median(std::mem::take(&mut nodes)) / 1e3,
        median(std::mem::take(&mut ok)) / 1e3,
        median(std::mem::take(&mut fail)) / 1e3,
        median(std::mem::take(&mut dup)) / 1e3,
        median(std::mem::take(&mut global_dup)) / 1e3,
        median(std::mem::take(&mut success_rate)) / 1e4,
    );
    out += &format!(
        "push-cmds/tick={} cramming-hits/tick={}\n",
        line(&mut push),
        line(&mut cramming)
    );
    out += &format!(
        "brain-poi/tick: brain-ticks={} sensor-scans={} behavior-starts={} poi-queries={}\n",
        line(&mut brain_ticks),
        line(&mut sensor_scans),
        line(&mut behavior_starts),
        line(&mut poi_queries),
    );
    for band in 0..3 {
        let mut count = band_ticks
            .iter()
            .map(|b| (b[band] * 1e3) as u64)
            .collect::<Vec<_>>();
        let mut ms = band_ms
            .iter()
            .map(|b| (b[band] * 1e3) as u64)
            .collect::<Vec<_>>();
        out += &format!(
            "band{}({}): mobs/tick={:.1} ms/tick={:.3}\n",
            band,
            ["0-32", "32-64", "64+"][band],
            median(std::mem::take(&mut count)) / 1e3,
            median(std::mem::take(&mut ms)) / 1e3,
        );
    }
    out += "kinds-ms/tick: ";
    for (kind, mut values) in kinds {
        let name = MobType::by_entity_type(kind)
            .map(MobType::name)
            .unwrap_or("?");
        out += &format!("{}={:.3} ", name, med(&mut values));
    }
    out += "\n";
    out
}

/// `mob_stress`：所有场景 × 五种线程数，每个数字跑 [`ROUNDS`] 轮取中位数。
///
/// ```text
/// cargo test -p mc-bamboo-blocks --release --lib mob_stress -- --ignored --nocapture
/// ```
#[test]
#[ignore = "timing run, not a check"]
fn mob_stress() {
    let start = Instant::now();
    // `MOB_STRESS_SCENE=<名字前缀>` 和 `MOB_STRESS_THREADS=0,1,2` 用来单独复现一个数字；
    // 不给就跑全部，报告里的表格就是这一次完整输出。
    let only = std::env::var("MOB_STRESS_SCENE").ok();
    // A/B 用：`MOB_STRESS_SCOPED=1` 回到每刻起线程的旧执行器。
    let scoped = std::env::var("MOB_STRESS_SCOPED").is_ok();
    let threads: Vec<usize> = std::env::var("MOB_STRESS_THREADS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect()
        })
        .unwrap_or_else(|| THREADS.to_vec());
    println!(
        "mob-stress: rounds={ROUNDS} warm={WARM} measure={MEASURE} scenes={} threads={:?} scoped={} max-collisions={} throttle={} pathfinding={}",
        only.as_deref().unwrap_or("all"),
        threads,
        scoped,
        std::env::var("MOB_STRESS_MAX_COLLISIONS").unwrap_or_else(|_| "off".to_owned()),
        std::env::var_os("MOB_STRESS_THROTTLE").is_some(),
        std::env::var("MOB_STRESS_PATHFINDING").unwrap_or_else(|_| "off".to_owned())
    );
    println!("uptime: {}", uptime());
    for scene in Scene::ALL {
        if only.as_deref().is_some_and(|raw| {
            !raw.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .any(|prefix| scene.name().starts_with(prefix))
        }) {
            continue;
        }
        let natural = scene == Scene::Mixed;
        for &count in &threads {
            let mut rounds = Vec::with_capacity(ROUNDS);
            for round in 0..ROUNDS {
                rounds.push(run_one(
                    scene,
                    count,
                    natural,
                    0x5eed + round as u64,
                    scoped,
                ));
            }
            print!("{}", summarize(scene, count, rounds));
        }
        if matches!(scene, Scene::Tower1x1 | Scene::Tower3x3) && threads.contains(&0) {
            print!("{}", measure_cramming(scene, 0, ROUNDS));
        }
    }
    println!(
        "uptime-end: {} elapsed={:.1}s",
        uptime(),
        start.elapsed().as_secs_f64()
    );
}

/// 挤压上限触发次数：用默认游戏规则（不限 = 0 关掉、默认 24）在塔里跑短测量。
/// 主塔场景把 `maxEntityCramming` 设 0 是为了不让生物被挤压伤害一批批打死，
/// 保持 350 只的稳定负载；这里单独量默认规则下真正触发了多少次伤害判定。
fn measure_cramming(scene: Scene, threads: usize, rounds: usize) -> String {
    let mut per_round = Vec::new();
    for round in 0..rounds {
        let (mut terrain, mobs, _) = scene.build();
        // A/B 用：打开挤压上限（8，Paper 的缺省值）。
        if std::env::var_os("MOB_STRESS_MAX_COLLISIONS").is_some() {
            terrain.rules.max_entity_collisions = 8;
        }
        if std::env::var_os("MOB_STRESS_THROTTLE").is_some() {
            terrain.rules.mob_throttle_distance = 32;
            terrain.rules.mob_sleep_distance = 64;
        }
        // 恢复官方默认（24），`Terrain::new` 给的就是 vanilla。
        terrain.rules.max_entity_cramming = BlockGameRules::vanilla().max_entity_cramming;
        let mut level = StressLevel {
            terrain: terrain.clone(),
            random: LevelRandom::new(0x5eed + round as i64),
            entities: Entities::with_next_id(2),
            redstone: RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
        };
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for (kind, position) in &mobs {
                world.spawn_entity(Entity::by_mob_type(*kind, *position));
            }
        }
        let source = SourceView::new(StressSource::new(terrain));
        let executor: Box<dyn Executor> = if threads <= 1 {
            Box::new(InlineExecutor)
        } else {
            Box::new(WorkerPool::new("mob-stress", threads).expect("起实体线程池"))
        };
        const TICKS: usize = 100;
        imp::take();
        let _ = imp::take_all_counters();
        for i in 0..TICKS {
            ticks.set_time(i as i64);
            source
                .0
                .clock
                .store(i as i64, std::sync::atomic::Ordering::Relaxed);
            if threads == 0 {
                World::new(&mut level, &mut ticks).tick_entities();
            } else {
                let mut world = World::new(&mut level, &mut ticks);
                let _ = tick_entities_parallel_timed(&source, &mut world, executor.as_ref());
            }
        }
        let counters = imp::take_all_counters();
        imp::take();
        per_round.push((
            counters.cramming_hits,
            counters.push_commands,
            level.entities.len(),
        ));
    }
    let hits = per_round
        .iter()
        .map(|&(hits, _, _)| hits as f64 / 100.0)
        .collect::<Vec<_>>();
    let mut out = format!(
        "== {} cramming-default threads={} ==\n",
        scene.name(),
        threads
    );
    out += &format!(
        "hits/tick={:.2} push-cmds/tick={:.1} alive@100t={}\n",
        median(hits.iter().map(|v| (v * 1e3) as u64).collect()) / 1e3,
        per_round[0].1 as f64 / 100.0,
        per_round[0].2,
    );
    out
}

/// macOS / Linux 的 1 分钟负载，写进报告说明测量当时的机器状态。
fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".to_owned())
}

/// 非 `#[ignore]` 冒烟：确认 7 个村民压测场景都接了真 `PoiStore`（床 / 钟 / 工作站
/// 登记正确、精确和并行只读视图读到同一份 POI 结果），并且精确 / 并行都能正常跑刻。
#[test]
fn villager_stress_scenarios_register_real_pois_and_tick_cleanly() {
    use crate::entity::ctx::BlockView;
    use mc_bamboo_data::poi::poi_type_id;

    let home = poi_type_id("minecraft:home").expect("home poi");
    let meeting = poi_type_id("minecraft:meeting").expect("meeting poi");
    let scenes = [
        Scene::Village40,
        Scene::Village200,
        Scene::Hall100,
        Scene::HallN,
        Scene::Pod100,
        Scene::Unemployed60,
        Scene::NightVillage100,
    ];
    for scene in scenes {
        let (terrain, mobs, _) = scene.build();
        assert!(!mobs.is_empty(), "{} must spawn villagers", scene.name());
        assert!(
            mobs.iter().all(|(kind, _)| *kind == MobType::Villager),
            "{} only spawns villagers",
            scene.name()
        );
        let center = BlockPos::new(0, 64, 0);
        let homes = terrain
            .poi
            .get_count_in_range(|k| k == home, center, 160, Occupancy::Any);
        let bells = terrain
            .poi
            .get_count_in_range(|k| k == meeting, center, 160, Occupancy::Any);
        assert!(
            homes >= 40,
            "{}: expected >=40 home POIs, got {homes}",
            scene.name()
        );
        assert!(
            bells >= 1,
            "{}: expected >=1 bell POI, got {bells}",
            scene.name()
        );

        let mut level = StressLevel {
            terrain: terrain.clone(),
            random: LevelRandom::new(7),
            entities: Entities::with_next_id(2),
            redstone: RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
        };
        let source = SourceView::new(StressSource::new(terrain));
        assert_eq!(
            level.poi_count_in_range(center, 160, home, 2),
            source.poi_count_in_range(center, 160, home, 2),
            "{}: StressLevel and StressSource must agree on POI counts",
            scene.name()
        );
        assert_eq!(
            level.poi_sections_to_village(center),
            source.poi_sections_to_village(center),
            "{}: StressLevel and StressSource must agree on village distance",
            scene.name()
        );
        // 精确 + 并行各跑 2 刻验证无 panic 且探针不崩。
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for (kind, pos) in mobs.iter().take(8) {
                world.spawn_entity(Entity::by_mob_type(*kind, *pos));
            }
        }
        for t in 0..2 {
            ticks.set_time(t);
            source
                .0
                .clock
                .store(t, std::sync::atomic::Ordering::Relaxed);
            let mut world = World::new(&mut level, &mut ticks);
            if t == 0 {
                world.tick_entities();
            } else {
                let _ = tick_entities_parallel_timed(&source, &mut world, &InlineExecutor);
            }
        }
    }
}
