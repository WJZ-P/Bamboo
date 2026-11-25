//! `SculkSensorBlockEntity` / `CalibratedSculkSensorBlockEntity`：`VibrationSystem.User`。
//!
//! 感测体每刻跑一次 `VibrationSystem.Ticker.tick`，没有候选也没有飞行中的振动时
//! 睡到下一次事件；收到振动就激活方块、给红石强度和比较器频率。

use std::any::Any;

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, sounds::sound_event, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    BlockEntity, BlockState,
    archive::{
        Error,
        tag::{self, put},
    },
    game_event::{
        GameEvent,
        dispatch::DeliveryMode,
        registry::ListenerRef,
        vibration::{self, VibrationData, VibrationInfo, VibrationUser},
    },
    state::{HORIZONTAL_FACING, POWER, SCULK_SENSOR_PHASE, SculkSensorPhase, WATERLOGGED},
    world::World,
};

/// `RESONANCE_PITCH_BEND` 的半音表，音高换算见音符盒。
const RESONANCE_NOTES: [i32; 16] = [0, 0, 2, 4, 6, 7, 9, 10, 12, 14, 15, 18, 19, 21, 22, 24];

/// 一个会听振动的感测体方块实体。
#[derive(Debug)]
pub struct SculkSensorEntity {
    pos: BlockPos,
    calibrated: bool,
    pub(crate) listener: VibrationData,
    last_vibration_frequency: i32,
    /// `onReceiveVibration` 里落地时要用的一次性动作。
    activation: Option<Activation>,
}

/// 振动到达时方块逻辑要做的动作。方块实体不能直接借用 `World`，所以由 `tick`
/// 在 `VibrationUser::on_receive` 里记账、随后统一执行。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Activation {
    frequency: i32,
    power: i32,
    /// 发出这次振动的玩家（可能没有），`SCULK_SENSOR_TENDRILS_CLICKING` 要带给尖啸体。
    source_player: Option<crate::entity::EntityUid>,
}

impl SculkSensorEntity {
    /// 新建：`VibrationSelector` 空，比较器频率 0。
    pub fn new(pos: BlockPos, calibrated: bool) -> Self {
        Self {
            pos,
            calibrated,
            listener: VibrationData::new(),
            last_vibration_frequency: 0,
            activation: None,
        }
    }

    /// 按官方 codec 从存档里还原。
    pub fn read(pos: BlockPos, calibrated: bool, tag: &Tag) -> std::result::Result<Self, Error> {
        let mut entity = Self::new(pos, calibrated);
        entity.last_vibration_frequency = tag::int_or(tag, "last_vibration_frequency", 0);
        let listener = tag::field(tag, "listener");
        let starter = listener
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Vec::new()));
        entity.listener = crate::archive::passive::read_listener_for_vibration(&starter);
        Ok(entity)
    }

    /// 官方 `saveAdditional` 的键序。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        put(
            &mut out,
            "last_vibration_frequency",
            Tag::Int(self.last_vibration_frequency),
        );
        put(
            &mut out,
            "listener",
            Tag::Compound(self.listener.write_codec()),
        );
        out
    }

    /// 半径：普通 8，校频 16。
    pub fn radius(&self) -> i32 {
        if self.calibrated { 16 } else { 8 }
    }

    /// `lastVibrationFrequency`：ACTIVE 时比较器读它。
    pub fn last_frequency(&self) -> i32 {
        self.last_vibration_frequency
    }

    /// 振动到达后要激活的红石强度；测试用它观察一次到达。
    pub fn last_activation(&self) -> Option<(i32, i32)> {
        self.activation.map(|a| (a.frequency, a.power))
    }

    /// 完成本次到达后的方块动作，清掉一次性记账。
    fn take_activation(&mut self) -> Option<Activation> {
        self.activation.take()
    }

    /// `canReceiveVibration` 的方块实体部分（校频体还要读背面红石信号）。
    pub(crate) fn can_receive_vibration(
        &self,
        world: &World<'_>,
        event: GameEvent,
        source: BlockPos,
    ) -> bool {
        if source == self.pos && matches!(event, GameEvent::BlockDestroy | GameEvent::BlockPlace) {
            return false;
        }
        if event.vibration_frequency() == 0 {
            return false;
        }
        let Some(state) = world.block_state_if_valid(self.pos) else {
            return false;
        };
        if state.get(SCULK_SENSOR_PHASE) != SculkSensorPhase::Inactive {
            return false;
        }
        if self.calibrated {
            let facing = state.get(HORIZONTAL_FACING);
            let back = world.signal(self.pos.relative(facing.opposite()), facing.opposite());
            if back != 0 && back != event.vibration_frequency() {
                return false;
            }
        }
        true
    }

    /// `SculkSensorBlock.activate`：激活方块并做共鸣。
    fn activate(&self, world: &mut World<'_>, activation: Activation) {
        let state = world.block_state(self.pos);
        let active = state
            .with(SCULK_SENSOR_PHASE, SculkSensorPhase::Active)
            .with(POWER, activation.power);
        world.set_block_and_update(self.pos, active);
        let active_ticks = if self.calibrated { 10 } else { 30 };
        world.schedule_tick(self.pos, state.block(), active_ticks);
        crate::behaviour::misc::sculk::update_neighbours(state, world, self.pos);
        self.try_resonate(world, activation.frequency);
        let source = Vec3::new(
            f64::from(self.pos.x) + 0.5,
            f64::from(self.pos.y) + 0.5,
            f64::from(self.pos.z) + 0.5,
        );
        world.game_event(
            GameEvent::SculkSensorTendrilsClicking,
            source,
            crate::game_event::dispatch::EventContext {
                source_player: activation.source_player,
                ..Default::default()
            },
        );
        if !state.get(WATERLOGGED) {
            let pitch = world.random().next_float() * 0.2 + 0.8;
            world.play_block_sound(
                self.pos,
                sound_event("minecraft:block.sculk_sensor.clicking"),
                1.0,
                pitch,
            );
        }
    }

    /// `tryResonateVibration`：六个方向上的紫水晶块接收共鸣事件并响一声。
    fn try_resonate(&self, world: &mut World<'_>, frequency: i32) {
        let Some(event) = GameEvent::resonate(frequency) else {
            return;
        };
        let note = RESONANCE_NOTES
            .get(usize::try_from(frequency).unwrap_or(0))
            .copied()
            .unwrap_or(0);
        let pitch = crate::behaviour::machines::note_block::pitch_from_note(note);
        for direction in mc_bamboo_core::Direction::ALL {
            let neighbor = self.pos.relative(direction);
            if world
                .block_state(neighbor)
                .is_tagged(block_tags::VIBRATION_RESONATORS)
            {
                let center = Vec3::new(
                    f64::from(neighbor.x) + 0.5,
                    f64::from(neighbor.y) + 0.5,
                    f64::from(neighbor.z) + 0.5,
                );
                world.game_event(
                    event,
                    center,
                    crate::game_event::dispatch::EventContext {
                        affected_state: Some(world.block_state(neighbor)),
                        ..Default::default()
                    },
                );
                world.play_block_sound(
                    neighbor,
                    sound_event("minecraft:block.amethyst_block.resonate"),
                    1.0,
                    pitch,
                );
            }
        }
    }

    /// `VibrationSystem.Listener.handleGameEvent`：记一个候选并叫醒自己。
    pub fn handle_game_event(
        &mut self,
        event: GameEvent,
        source: Vec3,
        listener_pos: Vec3,
        source_player: Option<crate::entity::EntityUid>,
        game_time: i64,
    ) {
        if self.listener.current.is_some() {
            return;
        }
        let info = VibrationInfo {
            event,
            distance: source.distance_to_sqr(listener_pos).sqrt() as f32,
            pos: source,
            source_player,
            source_entity: None,
            projectile_owner: None,
        };
        vibration::add_candidate(&mut self.listener, info, game_time, &mut || {});
    }

    /// 缓存守卫用：候选和飞行状态都能观察到。
    pub fn observed_facts(&self) -> (i32, i32, i32, Option<GameEvent>) {
        (
            self.listener.travel,
            self.listener
                .selector
                .codec_tick()
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            self.last_vibration_frequency,
            self.listener.current.map(|info| info.event),
        )
    }
}

impl BlockEntity for SculkSensorEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        let now = world.game_time();
        let listener_pos = Some(Vec3::new(
            f64::from(self.pos.x) + 0.5,
            f64::from(self.pos.y) + 0.5,
            f64::from(self.pos.z) + 0.5,
        ));
        let adjacent = adjacent_chunks_ticking(world, self.pos);
        let activation = {
            let mut user = Adapter {
                entity: self,
                world,
            };
            let mut data = user.entity.listener;
            vibration::tick(&mut data, &mut user, now, listener_pos, adjacent);
            user.entity.listener = data;
            user.entity.take_activation()
        };
        if let Some(activation) = activation {
            self.activate(world, activation);
        }
    }

    /// 没有候选、没有飞行中的振动时一直睡到外部叫醒。
    fn sleep_from(&mut self, _now: i64) -> Option<i64> {
        (self.listener.current.is_none() && self.listener.selector.codec_event().is_none())
            .then_some(i64::MAX)
    }

    /// 睡眠被外部改动打断时推进飞行中的振动，不发效果、不取随机数。
    fn materialize(&mut self, _now: i64) {
        // 飞行时间本来就是相对刻数，装盒 / 叫醒都不改它；不补发粒子。
        self.listener.reload_particle = false;
    }

    /// 岛合并时把候选的绝对刻平移。
    fn rebase(&mut self, delta: i64) {
        if let Some((info, tick)) = self
            .listener
            .selector
            .codec_event()
            .map(|info| (info, self.listener.selector.codec_tick()))
        {
            self.listener.selector.start_over();
            self.listener
                .selector
                .add_candidate(info, tick.saturating_add(delta));
        }
    }

    fn listener_radius(&self) -> Option<i32> {
        Some(self.radius())
    }

    fn delivery_mode(&self) -> Option<DeliveryMode> {
        Some(DeliveryMode::ByDistance)
    }

    fn receive_vibration(
        &mut self,
        world: &mut World,
        event: GameEvent,
        source: Vec3,
        listener_pos: Vec3,
        context: crate::game_event::dispatch::EventContext,
        now: i64,
    ) -> bool {
        if !crate::game_event::dispatch::valid_vibration(event, context) {
            return false;
        }
        let source_block = BlockPos::containing(source);
        if self.listener.current.is_some()
            || !self.can_receive_vibration(world, event, source_block)
        {
            return false;
        }
        self.handle_game_event(event, source, listener_pos, context.source_player, now);
        true
    }

    fn pre_remove_side_effects(&mut self, world: &mut World, pos: BlockPos, _state: BlockState) {
        let chunk = pos.chunk();
        if let Some(table) = world.level_mut().game_event_listeners_mut() {
            table.unregister(chunk, pos.y >> 4, ListenerRef::BlockEntity(pos));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> std::result::Result<Vec<(String, Tag)>, Error> {
        Ok(self.additional())
    }
}

/// 把方块实体和 `World` 临时拼成 `VibrationUser`：`on_receive` 只记账，
/// 方块写入统一在 `tick` 里做，避免 `VibrationUser` 与 `World` 相互借用。
struct Adapter<'a, 'w> {
    entity: &'a mut SculkSensorEntity,
    world: &'a mut World<'w>,
}

impl VibrationUser for Adapter<'_, '_> {
    fn radius(&self) -> i32 {
        self.entity.radius()
    }

    fn position(&self) -> Option<Vec3> {
        Some(Vec3::new(
            f64::from(self.entity.pos.x) + 0.5,
            f64::from(self.entity.pos.y) + 0.5,
            f64::from(self.entity.pos.z) + 0.5,
        ))
    }

    fn can_receive(&self, event: GameEvent, source: BlockPos) -> bool {
        self.entity.can_receive_vibration(self.world, event, source)
    }

    fn on_receive(
        &mut self,
        event: GameEvent,
        _source: BlockPos,
        source_player: Option<crate::entity::EntityUid>,
        distance: f32,
    ) {
        let frequency = event.vibration_frequency();
        self.entity.last_vibration_frequency = frequency;
        self.entity.activation = Some(Activation {
            frequency,
            power: vibration::redstone_strength(distance, self.entity.radius()),
            source_player,
        });
    }

    fn requires_adjacent_chunks(&self) -> bool {
        true
    }

    fn send_particles(&mut self, from: Vec3, _to: Vec3, travel: i32) -> usize {
        self.world
            .push_effect(crate::level::Effect::VibrationParticle {
                pos: self.entity.pos,
                x: from.x,
                y: from.y,
                z: from.z,
                travel,
            });
        1
    }
}

/// `areAdjacentChunksTicking`：3×3 区块都在 tick。
pub(crate) fn adjacent_chunks_ticking(world: &World<'_>, pos: BlockPos) -> bool {
    let center = pos.chunk();
    for x in -1..=1 {
        for z in -1..=1 {
            let chunk = mc_bamboo_core::ChunkPos::new(center.x + x, center.z + z);
            if !world.level().chunk_ticks_blocks(chunk) || !world.level().chunk_is_loaded(chunk) {
                return false;
            }
        }
    }
    true
}

/// 这一格是会发出游戏事件的感测体方块（缓存守卫用）。
pub const fn is_vibration_listener_block(kind: usize) -> bool {
    kind == blocks::SCULK_SENSOR as usize || kind == blocks::CALIBRATED_SCULK_SENSOR as usize
}

/// 新建感测体方块实体，供 `on_place` 和读档共用。
pub fn create_block_entity(pos: BlockPos, calibrated: bool) -> Box<dyn BlockEntity> {
    Box::new(SculkSensorEntity::new(pos, calibrated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, testing::TestLevel, world::World};
    use mc_bamboo_data::blocks;

    const SENSOR: BlockPos = BlockPos::new(0, 64, 0);

    fn world_with_sensor(calibrated: bool) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        let block = if calibrated {
            blocks::CALIBRATED_SCULK_SENSOR
        } else {
            blocks::SCULK_SENSOR
        };
        level.set(SENSOR, BlockState(block));
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block_entity(create_block_entity(SENSOR, calibrated));
            world.tick_block_entities();
        }
        (level, ticks)
    }

    #[test]
    fn step_vibration_activates_the_sensor_after_travelling() {
        let (mut level, mut ticks) = world_with_sensor(false);
        let mut world = World::new(&mut level, &mut ticks);
        let source = Vec3::new(4.5, 64.5, 0.5);
        world.game_event(GameEvent::Step, source, Default::default());
        // 距离 4，候选入选后还要 4 刻才到达。
        for t in 1..=5 {
            world.ticks_mut().set_time(t);
            world.tick_block_entities();
        }
        let state = world.block_state(SENSOR);
        assert_eq!(state.get(SCULK_SENSOR_PHASE), SculkSensorPhase::Active);
        // max(1, 15 - floor(15/8 * 4)) = 8。
        assert_eq!(state.get(POWER), 8);
    }

    #[test]
    fn wool_between_the_source_and_sensor_blocks_the_vibration() {
        let (mut level, mut ticks) = world_with_sensor(false);
        level.set(BlockPos::new(2, 64, 0), BlockState(blocks::WHITE_WOOL));
        let mut world = World::new(&mut level, &mut ticks);
        world.game_event(
            GameEvent::Step,
            Vec3::new(4.5, 64.5, 0.5),
            Default::default(),
        );
        for t in 1..=5 {
            world.ticks_mut().set_time(t);
            world.tick_block_entities();
        }
        assert_eq!(
            world.block_state(SENSOR).get(SCULK_SENSOR_PHASE),
            SculkSensorPhase::Inactive
        );
    }

    #[test]
    fn calibrated_sensor_ignores_a_frequency_that_does_not_match_its_back_signal() {
        let (mut level, mut ticks) = world_with_sensor(true);
        // 校频体默认朝北，背面是 SOUTH；那一格给强度 15，STEP 频率 1。
        level.set(BlockPos::new(0, 64, 1), BlockState(blocks::REDSTONE_BLOCK));
        let mut world = World::new(&mut level, &mut ticks);
        world.game_event(
            GameEvent::Step,
            Vec3::new(4.5, 64.5, 0.5),
            Default::default(),
        );
        for t in 1..=5 {
            world.ticks_mut().set_time(t);
            world.tick_block_entities();
        }
        assert_eq!(
            world.block_state(SENSOR).get(SCULK_SENSOR_PHASE),
            SculkSensorPhase::Inactive
        );
    }
}
