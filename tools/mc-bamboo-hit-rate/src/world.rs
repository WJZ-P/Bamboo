//! 实验用的假世界和驱动。
//!
//! 和 `mc-bamboo-blocks` 的测试假世界同一套思路：稀疏的方块表、容器槽位表、
//! 方块实体和计划刻。逐刻比较时把同一台机器造两份，一份套录制包装、一份不套。

use std::collections::BTreeMap;

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, BlockTicks, Container, EntityTables,
    HopperEntity, Level, LevelRandom, LevelTables, RedstoneMemory, Slots,
    entity::EntityStore,
    level::EquipTarget,
    recording::{Record, Recording},
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, section_hash, sign::SignContent};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::item::ItemStack;

pub const MIN_Y: i32 = -64;
pub const MAX_Y: i32 = 319;

/// 一台机器的世界。方块表里没写的格子是空气。
pub struct BenchLevel {
    pub blocks: BTreeMap<BlockPos, BlockState>,
    pub containers: BTreeMap<BlockPos, Vec<ItemStack>>,
    pub random: LevelRandom,
    pub block_entities: BlockEntities,
    pub block_events: BlockEvents,
    pub redstone: RedstoneMemory,
    pub entities: EntityStore,
}

impl Default for BenchLevel {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchLevel {
    pub fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
            containers: BTreeMap::new(),
            random: LevelRandom::new(0),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            redstone: RedstoneMemory::default(),
            entities: EntityStore::default(),
        }
    }

    /// 直接摆一格，不做任何连锁。
    pub fn set(&mut self, pos: BlockPos, state: BlockState) {
        self.blocks.insert(pos, state);
    }

    /// 放一张槽位表。
    pub fn set_container(&mut self, pos: BlockPos, slots: Vec<ItemStack>) {
        self.containers.insert(pos, slots);
    }

    /// 逐刻比较用的世界快照：方块、容器、红石记忆、随机数种子、计划刻。
    pub fn snapshot(&self, ticks: &BlockTicks) -> Vec<String> {
        let mut out = Vec::new();
        for (pos, state) in &self.blocks {
            out.push(format!("b{pos:?}:{}", state.0));
        }
        for (pos, slots) in &self.containers {
            let mut text = String::new();
            for stack in slots {
                text.push_str(&format!(
                    "{}:{}:{}:",
                    stack.item,
                    stack.count,
                    stack.added.len()
                ));
            }
            out.push(format!("c{pos:?}:{text}"));
        }
        for pos in self.blocks.keys() {
            out.push(format!(
                "r{pos:?}:{}:{}:{}:{}:{}:{}",
                self.redstone.comparator_output(*pos),
                self.redstone
                    .last_bookshelf_slot(*pos)
                    .unwrap_or(usize::MAX),
                self.redstone.is_jukebox_playing(*pos),
                self.redstone.crafter_disabled(*pos),
                self.redstone.lectern(*pos).is_some(),
                self.redstone.torch_toggle_count(*pos),
            ));
        }
        for tick in ticks.pending() {
            out.push(format!(
                "t{:?}:{}:{}:{:?}",
                tick.pos, tick.block, tick.trigger_tick, tick.priority
            ));
        }
        out.push(format!("seed:{}", self.random.source().seed()));
        out
    }
}

impl EntityTables for BenchLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }
}

impl LevelTables for BenchLevel {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
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

impl Level for BenchLevel {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !(MIN_Y..=MAX_Y).contains(&pos.y) {
            return BlockState(blocks::VOID_AIR);
        }
        self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR)
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        if !(MIN_Y..=MAX_Y).contains(&pos.y) {
            return false;
        }
        if !self.block_state(pos).is_same_block(state) {
            self.containers.remove(&pos);
        }
        self.blocks.insert(pos, state);
        true
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MAX_Y
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }

    fn section_hash(&self, chunk: ChunkPos, section_y: i32) -> Option<u128> {
        Some(section_hash::section_hash(|x, y, z| {
            let pos = BlockPos::new(
                chunk.x * 16 + x as i32,
                section_y * 16 + y as i32,
                chunk.z * 16 + z as i32,
            );
            self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR).0
        }))
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        false
    }

    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => 15,
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

    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let Some(slots) = self.containers.get_mut(&pos) else {
            return false;
        };
        let mut container = Slots::new(slots);
        f(&mut container);
        true
    }

    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let slots = self
            .containers
            .entry(pos)
            .or_insert_with(|| vec![ItemStack::empty(); len]);
        if slots.len() != len {
            *slots = vec![ItemStack::empty(); len];
        }
        f(slots);
        true
    }

    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        self.containers.get(&pos).cloned().unwrap_or_default()
    }

    fn clear_container(&mut self, pos: BlockPos) {
        self.containers.remove(&pos);
    }

    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        self.containers
            .get(&pos)
            .map(|slots| slots.iter().map(|stack| stack.count).collect())
            .unwrap_or_default()
    }

    fn container_signal(&self, pos: BlockPos) -> i32 {
        let Some(slots) = self.containers.get(&pos) else {
            return 0;
        };
        mc_bamboo_blocks::redstone_signal(slots.iter(), slots.len())
    }

    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let Some(hopper) = self.block_entities.get_mut(pos) else {
            return false;
        };
        f(hopper);
        true
    }

    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        Vec::new()
    }

    fn set_dispenser_equipment(
        &mut self,
        _entity_id: i32,
        _component_slot: i32,
        _stack: ItemStack,
    ) -> Option<u16> {
        None
    }
}

/// 机器每隔若干刻需要的外部输入。
#[derive(Clone, Debug)]
pub enum Input {
    /// 没有外部输入。
    None,
    /// 周期性翻转一个红石块（满信号源）：写成红石块或空气，走 `set_block_and_update`
    /// 通知邻居。这等价于一个由实验程序驱动的开关，方块行为和 Bamboo 的信号查询一致。
    Clock { pos: BlockPos, period: i64 },
    /// 几个周期不同的独立时钟：`(位置, 周期)`，各自按 `now % period == 0` 翻转。
    Clocks(Vec<(BlockPos, i64)>),
    /// 往源头箱子补物品，驱动漏斗机器。
    Refill {
        pos: BlockPos,
        stack: ItemStack,
        period: i64,
    },
}

/// 一台机器。
pub struct Machine {
    pub name: &'static str,
    pub level: BenchLevel,
    pub ticks: BlockTicks,
    pub input: Input,
}

/// 翻转一格周期输入：红石块（满信号）<-> 空气，并通知邻居。
fn toggle_source(level: &mut BenchLevel, ticks: &mut mc_bamboo_blocks::BlockTicks, pos: BlockPos) {
    use mc_bamboo_blocks::World;
    let on = level.block_state(pos).is(blocks::REDSTONE_BLOCK);
    let next = if on {
        BlockState::AIR
    } else {
        BlockState(blocks::REDSTONE_BLOCK)
    };
    let mut world = World::new(level, ticks);
    world.set_block_and_update(pos, next);
}

impl Machine {
    /// 在机器里放一个漏斗方块实体并登记槽位表。
    pub fn place_hopper(&mut self, pos: BlockPos, facing: Direction) {
        use mc_bamboo_blocks::{World, state::FACING_HOPPER};
        self.level
            .set(pos, BlockState(blocks::HOPPER).with(FACING_HOPPER, facing));
        self.level.set_container(pos, vec![ItemStack::empty(); 5]);
        let mut world = World::new(&mut self.level, &mut self.ticks);
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
    }

    /// 这一刻的外部输入（如果有）。
    fn apply_input(&mut self, now: i64) {
        use mc_bamboo_blocks::{World, world::UPDATE_CLIENTS};
        match self.input.clone() {
            Input::None => {}
            Input::Clock { pos, period } => {
                if now % period == 0 {
                    toggle_source(&mut self.level, &mut self.ticks, pos);
                }
            }
            Input::Clocks(clocks) => {
                for (pos, period) in clocks {
                    if now % period == 0 {
                        toggle_source(&mut self.level, &mut self.ticks, pos);
                    }
                }
            }
            Input::Refill { pos, stack, period } if now % period == 0 => {
                let mut changed = false;
                let ok = self.level.with_container(pos, &mut |container| {
                    for slot in 0..container.size() {
                        if container.get(slot).is_empty() {
                            container.set(slot, stack.clone());
                            changed = true;
                            break;
                        }
                    }
                });
                if ok && changed {
                    let mut world = World::new(&mut self.level, &mut self.ticks);
                    let state = world.block_state(pos);
                    world.set_block(pos, state, UPDATE_CLIENTS);
                    world.update_neighbour_for_output_signal(pos, state.block());
                }
            }
            _ => {}
        }
    }

    /// 不套录制包装地跑一个世界刻，供钩子开销基准使用。
    pub fn plain_step(&mut self, now: i64) {
        self.apply_input(now);
        mc_bamboo_blocks::run_scheduled_ticks(&mut self.level, &mut self.ticks, now);
    }

    /// 跑一个世界刻，返回这一刻收集到的录制记录。`recording` 为假时不套包装。
    pub fn step(&mut self, now: i64, recording: bool) -> Vec<Record> {
        self.apply_input(now);
        if !recording {
            mc_bamboo_blocks::run_scheduled_ticks(&mut self.level, &mut self.ticks, now);
            return Vec::new();
        }
        let mut recorder = Recording::new(&mut self.level);
        recorder.set_tick(now);
        mc_bamboo_blocks::run_scheduled_ticks(&mut recorder, &mut self.ticks, now);
        recorder.take_records()
    }

    /// 两份同样的机器，一份开录制、一份不开，逐刻比世界状态。
    pub fn compare(build: impl Fn() -> Machine, ticks: i64) -> (Vec<Record>, usize) {
        let mut recorded = build();
        let mut plain = build();
        let mut records = Vec::new();
        let mut mismatches = 0;
        for now in 1..=ticks {
            records.extend(recorded.step(now, true));
            plain.step(now, false);
            if recorded.level.snapshot(&recorded.ticks) != plain.level.snapshot(&plain.ticks) {
                mismatches += 1;
            }
        }
        (records, mismatches)
    }
}
