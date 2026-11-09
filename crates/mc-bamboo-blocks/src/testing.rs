//! 单元测试用的假世界。

use std::collections::HashMap;

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{BlockAncestor, blocks, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::block_entity::{BlockEntities, BlockEvents};

use crate::{
    Actor, BlockHit, BlockState, BlockTicks, Container, Level, LevelRandom, Outcome, Slots,
    container::redstone_signal,
    destroy_block,
    entity::Entities,
    hopper::HopperEntity,
    level::{EquipTarget, RedstoneMemory},
    shape::Aabb,
};

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

/// 方块存在表里，没写过的格子是空气，世界外是 `void_air`。随机数的种子固定是 0，玩家总在附近。
pub(crate) struct TestLevel {
    blocks: HashMap<BlockPos, BlockState>,
    /// 告示牌的内容。和区块存储一样，换成别的方块时丢掉。
    pub(crate) signs: HashMap<BlockPos, SignContent>,
    /// 头颅方块实体的 `note_block_sound`，只有那一格还是头颅时才读得到。
    pub(crate) note_block_sounds: HashMap<BlockPos, String>,
    /// 按写入顺序记下每一格写过什么，测试连锁顺序时用。
    pub(crate) writes: Vec<(BlockPos, BlockState)>,
    /// 假装有实体挡在任何有碰撞的方块上。
    pub(crate) obstructed: bool,
    pub(crate) random: LevelRandom,
    redstone: RedstoneMemory,
    /// 每一格的天空光和方块光。没设时是开阔的白天：天空光 15，方块光 0。
    pub(crate) light: Option<(i32, i32)>,
    /// 容器方块的槽位表。没有条目的容器方块什么也取不到，测试要显式放。
    pub(crate) containers: HashMap<BlockPos, Vec<ItemStack>>,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    entities: Entities,
    /// 发射器能穿装备的生物。
    pub(crate) living: Vec<EquipTarget>,
    /// 方块能看见的游戏规则。默认和官方一样，`tnt_explodes` 为真。
    ///
    /// Game rules block logic can see. Vanilla defaults, so `tnt_explodes` starts true.
    pub(crate) rules: crate::gamerules::BlockGameRules,
}

impl TestLevel {
    pub(crate) fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            signs: HashMap::new(),
            note_block_sounds: HashMap::new(),
            writes: Vec::new(),
            obstructed: false,
            random: LevelRandom::new(0),
            redstone: RedstoneMemory::default(),
            light: None,
            containers: HashMap::new(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::default(),
            living: Vec::new(),
            rules: crate::gamerules::BlockGameRules::vanilla(),
        }
    }

    /// 直接写一格，不做任何连锁。
    pub(crate) fn set(&mut self, pos: BlockPos, state: BlockState) {
        self.blocks.insert(pos, state);
    }

    pub(crate) fn get(&self, pos: BlockPos) -> BlockState {
        self.block_state(pos)
    }

    /// 给这一格放一张槽位表，模拟官方的容器方块实体。
    pub(crate) fn set_container(&mut self, pos: BlockPos, slots: Vec<ItemStack>) {
        self.containers.insert(pos, slots);
    }

    pub(crate) fn redstone_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }
}

impl Level for TestLevel {
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
            self.signs.remove(&pos);
            self.note_block_sounds.remove(&pos);
        }
        self.blocks.insert(pos, state);
        self.writes.push((pos, state));
        true
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        !self.obstructed
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

    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        self.blocks.iter().any(|(pos, state)| {
            pos.chunk() == chunk
                && pos.y >> 4 == section_y
                && (state.is_randomly_ticking() || state.is_fluid_randomly_ticking())
        })
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        true
    }

    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        let (sky, block) = self.light.unwrap_or((15, 0));
        match layer {
            LightLayer::Sky => sky,
            LightLayer::Block => block,
        }
    }

    fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        if !self.block_state(pos).is_a(BlockAncestor::SignBlock) {
            return None;
        }
        Some(self.signs.get(&pos).cloned().unwrap_or_default())
    }

    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        if !self.block_state(pos).is_a(BlockAncestor::SignBlock) {
            return false;
        }
        self.signs.insert(pos, content);
        true
    }

    /// 和官方的假世界不同，这里可以给头颅方块实体放一个 `note_block_sound`。
    /// 换成别的方块以后那个方块实体就没了，和 [`Level::sign`] 一样读不到。
    fn note_block_sound(&self, pos: BlockPos) -> Option<String> {
        if !self
            .block_state(pos)
            .is_a(BlockAncestor::AbstractSkullBlock)
        {
            return None;
        }
        self.note_block_sounds.get(&pos).cloned()
    }

    fn block_entities(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }

    fn entities(&self) -> &Entities {
        &self.entities
    }

    fn entities_mut(&mut self) -> &mut Entities {
        &mut self.entities
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
        redstone_signal(slots.iter(), slots.len())
    }

    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let Some(hopper) = self.block_entities.get_mut(pos) else {
            return false;
        };
        f(hopper);
        true
    }

    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        self.living.clone()
    }

    fn block_game_rules(&self) -> crate::gamerules::BlockGameRules {
        self.rules
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
}

/// 站在原点、看着南方的玩家，主手拿着 `item`。UUID 全是零，创造模式。
pub(crate) fn actor_holding(item: i32) -> Actor {
    Actor {
        uuid: [0; 16],
        position: Vec3::new(0.5, 70.0, 0.5),
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        secondary_use: false,
        main_hand: item,
        off_hand: items::AIR,
        eye_height: 1.62,
        block_interaction_range: 5.0,
        may_build: true,
        prevents_block_drops: true,
        can_use_game_master_blocks: false,
    }
}

/// 点在这一格顶面的中间。
pub(crate) fn top_hit(pos: BlockPos) -> BlockHit {
    BlockHit {
        pos,
        face: Direction::Up,
        location: Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 1.0,
            f64::from(pos.z) + 0.5,
        ),
        inside: false,
    }
}

/// 主手右键。
pub(crate) fn use_item(level: &mut TestLevel, actor: &Actor, hit: BlockHit) -> Outcome {
    crate::use_item_on(level, &mut BlockTicks::new(), actor, Hand::Main, &hit)
}

/// 主手对着空气右键。
pub(crate) fn use_in_air(level: &mut TestLevel, actor: &Actor) -> Outcome {
    crate::use_item(level, &mut BlockTicks::new(), actor, Hand::Main)
}

/// 空手的玩家破坏这一格。
pub(crate) fn break_block(level: &mut TestLevel, pos: BlockPos) -> Outcome {
    let actor = actor_holding(items::AIR);
    destroy_block(level, &mut BlockTicks::new(), &actor, pos)
}
