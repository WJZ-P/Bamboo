//! `CreakingHeartBlockEntity`：昼夜状态、绑定的嘎枝 UUID 和比较器信号。
//!
//! 心每 20~24 刻复查一次状态，夜里变 `awake`，白天变 `dormant`；两头缺了
//! 苍白橡木且没有绑定的嘎枝时变 `uprooted`。存档的 `creaking` 是官方的
//! int-array UUID。
//!
//! `CreakingHeartBlockEntity`: the day/night state, the bound creaking UUID and
//! the comparator signal. The `creaking` key is the int-array UUID.

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockAncestor, blocks, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    BlockEntity, BlockState, World,
    archive::tag::{self, Result},
    state::{AXIS, CREAKING_HEART_STATE, CreakingHeartState},
};

/// `CreakingHeartBlockEntity` 存档里的 UUID（四个 int）。
type Uuid = [i32; 4];

/// `hasRequiredLogs`：这条轴两头都是同一条轴的苍白橡木。
fn has_required_logs(world: &World, pos: BlockPos, state: BlockState) -> bool {
    let Some(axis) = state.try_get(AXIS) else {
        return false;
    };
    [axis.positive(), axis.negative()].into_iter().all(|side| {
        let neighbor = world.block_state(pos.relative(side));
        neighbor.is_tagged(block_tags::PALE_OAK_LOGS) && neighbor.get(AXIS) == axis
    })
}

/// 嘎枝之心方块实体。
#[derive(Debug)]
pub struct CreakingHeartEntity {
    pos: BlockPos,
    /// 绑定的嘎枝 UUID；读盘后用它重新接回实体。
    creaking: Option<Uuid>,
    /// 读盘后还没按 UUID 接回实体。
    loading: bool,
    /// 官方 `ticker`：第一次状态检查前的剩余刻数。
    ticker: i32,
    /// `ticksExisted`：只用来判定读盘后多久还找不到那只嘎枝。
    ticks_existed: i32,
    /// 比较器上一次对外报出的信号。
    output_signal: i32,
    /// 最近一次由实体层算出的嘎枝到心的距离；`None` 表示没接回实体。
    protector_distance: Option<f64>,
}

impl CreakingHeartEntity {
    /// `CreakingHeartBlockEntity(pos, state)`。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            creaking: None,
            loading: false,
            ticker: 20,
            ticks_existed: 0,
            output_signal: 0,
            protector_distance: None,
        }
    }

    /// 按官方 `saveAdditional` 解码。UUID 必须是四个 int。
    pub fn read(pos: BlockPos, tag: &Tag) -> Self {
        let raw = tag::int_array(tag, "creaking");
        let creaking = (raw.len() == 4).then(|| {
            let mut uuid = [0; 4];
            uuid.copy_from_slice(&raw[..4]);
            uuid
        });
        Self {
            pos,
            creaking,
            loading: creaking.is_some(),
            ticker: 20,
            ticks_existed: 0,
            output_signal: 0,
            protector_distance: None,
        }
    }

    /// 绑定（或读盘后重新找到）的那只嘎枝的 UUID。
    pub fn creaking_uuid(&self) -> Option<Uuid> {
        self.creaking
    }

    /// `setCreakingInfo(Creaking)` / `setCreakingInfo(UUID)`。
    pub fn set_creaking_info(&mut self, uuid: Uuid, loaded: bool) {
        self.creaking = Some(uuid);
        self.loading = loaded;
        self.ticks_existed = 0;
    }

    /// `clearCreakingInfo`。
    fn clear_creaking_info(&mut self) {
        self.creaking = None;
        self.loading = false;
        self.protector_distance = None;
    }

    /// 读盘后按 UUID 找实体；找不到且过了 30 刻就清掉绑定。
    fn resolve_creaking(&mut self, world: &World) {
        let Some(uuid) = self.creaking.filter(|_| self.loading) else {
            return;
        };
        if world.has_mob_uuid(&uuid_bytes(uuid)) {
            self.loading = false;
        } else if self.ticks_existed >= 30 {
            self.clear_creaking_info();
        }
    }

    /// `isProtector(Creaking)`：官方按引用比较；Bamboo 按 UUID 比较。
    pub fn is_protector(&self, uuid: Uuid) -> bool {
        self.creaking == Some(uuid) && !self.loading
    }

    /// `computeAnalogOutputSignal`：还没接回实体时是 0；接回后按距离衰减。
    pub fn compute_analog_output_signal(&self) -> i32 {
        let Some(distance) = self.protector_distance else {
            return 0;
        };
        let clamped = distance.clamp(0.0, 32.0);
        15 - (clamped / 32.0 * 15.0).floor() as i32
    }

    /// 方块侧读比较器用。
    pub fn analog_output_signal(&self) -> i32 {
        self.output_signal
    }

    /// 实体层每次刻算出距离后交给心，供比较器读取。
    pub fn set_protector_distance(&mut self, distance: Option<f64>) {
        self.protector_distance = distance;
    }

    /// `updateCreakingState`。
    fn update_state(&self, world: &World, state: BlockState) -> CreakingHeartState {
        if !has_required_logs(world, self.pos, state) && self.creaking.is_none() {
            CreakingHeartState::Uprooted
        } else if world.creaking_active(self.pos) {
            CreakingHeartState::Awake
        } else {
            CreakingHeartState::Dormant
        }
    }

    /// `CreakingHeartBlockEntity.serverTick`：状态复查安排在 20~24 刻之后。
    pub fn server_tick(&mut self, world: &mut World) {
        self.ticks_existed = self.ticks_existed.saturating_add(1);
        self.resolve_creaking(world);
        let signal = self.compute_analog_output_signal();
        if signal != self.output_signal {
            self.output_signal = signal;
            let block = world.block_state(self.pos).block();
            world.update_neighbour_for_output_signal(self.pos, block);
        }

        self.ticker -= 1;
        if self.ticker >= 0 {
            return;
        }
        self.ticker = 20 + world.random().next_int(5);

        let state = world.block_state(self.pos);
        if !state.is_a(BlockAncestor::CreakingHeartBlock) {
            return;
        }
        let next = self.update_state(world, state);
        if state.get(CREAKING_HEART_STATE) != next {
            world.set_block_and_update(self.pos, state.with(CREAKING_HEART_STATE, next));
        }
        if next == CreakingHeartState::Uprooted {
            return;
        }
        // 绑定 / 刷怪分支在嘎枝实体落地后接入。
        let _ = (blocks::CREAKING_HEART, world.game_time());
    }

    /// `saveAdditional`：`creaking` 只在有绑定时写。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        match self.creaking {
            Some(uuid) => vec![("creaking".to_owned(), Tag::IntArray(uuid.to_vec()))],
            None => Vec::new(),
        }
    }
}

/// UUID 的四个 int 转成 Bamboo 的 `[u8; 16]`（大端，顺序与官方一致）。
fn uuid_bytes(uuid: Uuid) -> [u8; 16] {
    let mut out = [0; 16];
    for (index, value) in uuid.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

/// Bamboo 的 `[u8; 16]` 转成官方 int-array UUID。
pub fn uuid_ints(uuid: &[u8; 16]) -> Uuid {
    let mut out = [0; 4];
    for (index, value) in (0..4)
        .map(|index| {
            let offset = index * 4;
            i32::from_be_bytes([
                uuid[offset],
                uuid[offset + 1],
                uuid[offset + 2],
                uuid[offset + 3],
            ])
        })
        .enumerate()
    {
        out[index] = value;
    }
    out
}

impl BlockEntity for CreakingHeartEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>> {
        Ok(self.additional())
    }
}

/// `CreakingHeartBlock.newBlockEntity`：放置时挂一个空的心。
pub fn create_block_entity(pos: BlockPos) -> Box<dyn BlockEntity> {
    Box::new(CreakingHeartEntity::new(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        behaviour::misc::creaking_heart_entity as entity, testing::TestLevel, ticks::BlockTicks,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    /// 一条 Y 轴、两头都是苍白橡木原木的心。
    fn heart() -> BlockState {
        BlockState(blocks::CREAKING_HEART)
            .with(AXIS, mc_bamboo_core::Axis::Y)
            .with(CREAKING_HEART_STATE, CreakingHeartState::Dormant)
    }

    fn level(state: BlockState, ticks: i64) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        level.clock_ticks = ticks;
        level.set(POS, state);
        let log = BlockState(blocks::PALE_OAK_LOG).with(AXIS, mc_bamboo_core::Axis::Y);
        level.set(POS.above(), log);
        level.set(POS.below(), log);
        (level, BlockTicks::new())
    }

    #[test]
    fn the_state_follows_the_day_timeline() {
        let (mut night, mut night_ticks) = level(heart(), 12600);
        {
            let mut world = World::new(&mut night, &mut night_ticks);
            world.set_block_entity(entity::create_block_entity(POS));
            for _ in 0..21 {
                world.with_block_entity::<CreakingHeartEntity, _>(POS, |be, world| be.tick(world));
            }
            assert_eq!(
                world.block_state(POS).get(CREAKING_HEART_STATE),
                CreakingHeartState::Awake
            );
        }
        let (mut day, mut day_ticks) = level(heart(), 1000);
        let mut world = World::new(&mut day, &mut day_ticks);
        world.set_block_entity(entity::create_block_entity(POS));
        for _ in 0..25 {
            world.with_block_entity::<CreakingHeartEntity, _>(POS, |be, world| be.tick(world));
        }
        assert_eq!(
            world.block_state(POS).get(CREAKING_HEART_STATE),
            CreakingHeartState::Dormant
        );
    }

    #[test]
    fn a_heart_without_its_logs_is_uprooted_but_a_bound_heart_is_not() {
        let (mut level, mut ticks) = level(heart(), 1000);
        level.set(POS.above(), BlockState::AIR);
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block_entity(entity::create_block_entity(POS));
        for _ in 0..21 {
            world.with_block_entity::<CreakingHeartEntity, _>(POS, |be, world| be.tick(world));
        }
        assert_eq!(
            world.block_state(POS).get(CREAKING_HEART_STATE),
            CreakingHeartState::Uprooted
        );

        let mut bound = CreakingHeartEntity::new(POS);
        bound.set_creaking_info([1, 2, 3, 4], true);
        assert_eq!(
            bound.update_state(&world, heart()),
            CreakingHeartState::Dormant
        );
    }

    #[test]
    fn creaking_uuid_and_comparator_signal_round_trip() {
        let uuid = [7, 11, 13, 17];
        let mut heart = CreakingHeartEntity::new(POS);
        heart.set_creaking_info(uuid, false);
        heart.set_protector_distance(Some(16.0));
        assert!(heart.is_protector(uuid));
        assert_eq!(heart.compute_analog_output_signal(), 8);
        let tag = Tag::Compound(heart.additional());
        let rebuilt = CreakingHeartEntity::read(POS, &tag);
        assert_eq!(rebuilt.creaking_uuid(), Some(uuid));
        assert!(rebuilt.loading);
        assert!(!rebuilt.is_protector(uuid));
        assert_eq!(
            CreakingHeartEntity::read(POS, &tag).additional(),
            heart.additional()
        );
        assert_eq!(uuid_ints(&uuid_bytes(uuid)), uuid);
    }
}
