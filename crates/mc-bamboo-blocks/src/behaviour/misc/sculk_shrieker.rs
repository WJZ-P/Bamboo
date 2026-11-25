//! `SculkShriekerBlockEntity`：`VibrationSystem.User`，只听 `#shrieker_can_listen`。
//!
//! 幽匿尖啸体自己不直接听脚步：它听感测体激活时发出的 `sculk_sensor_tendrils_clicking`，
//! 源是玩家时 `tryShriek`。能响应（`can_summon` 且非和平且 `spawn_wardens` 开）时先给
//! 玩家加一级警告，加上了才尖叫；加不上（冷却）就静默。90 刻后计划刻复位尖叫并
//! `tryRespond`，按警告等级给附近玩家黑暗效果。召唤监守者这一步这轮留接口。

use std::any::Any;

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;
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
    state::{CAN_SUMMON, SHRIEKING},
    world::World,
};

/// `SculkShriekerBlock.SHRIEKING_TICKS`：尖叫后复位前的计划刻延迟。
const SHRIEKING_TICKS: i32 = 90;
/// `Warden.applyDarknessAround` 的黑暗时长与半径。
const DARKNESS_TICKS: i32 = 260;
const DARKNESS_RADIUS: i32 = 40;

/// 一个会听 `sculk_sensor_tendrils_clicking` 的尖啸体方块实体。
#[derive(Debug)]
pub struct SculkShriekerEntity {
    pos: BlockPos,
    /// 上一次成功警告后的等级（`tryToWarn` 记下，`tryRespond` 读它决定要不要召唤）。
    warning_level: i32,
    pub(crate) listener: VibrationData,
    /// 振动到达时要执行的 `tryShriek` 动作，由 `tick` 统一落地。
    shriek: Option<Option<crate::entity::EntityUid>>,
}

impl SculkShriekerEntity {
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            warning_level: 0,
            listener: VibrationData::new(),
            shriek: None,
        }
    }

    pub fn read(pos: BlockPos, tag: &Tag) -> Result<Self, Error> {
        let mut entity = Self::new(pos);
        entity.warning_level = tag::int_or(tag, "warning_level", 0);
        let listener = tag::field(tag, "listener");
        let starter = listener
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Vec::new()));
        entity.listener = crate::archive::passive::read_listener_for_vibration(&starter);
        Ok(entity)
    }

    pub fn additional(&self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        put(&mut out, "warning_level", Tag::Int(self.warning_level));
        put(
            &mut out,
            "listener",
            Tag::Compound(self.listener.write_codec()),
        );
        out
    }

    /// `VibrationSystem.Listener.handleGameEvent`。
    fn handle_game_event(
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
        };
        vibration::add_candidate(&mut self.listener, info, game_time, &mut || {});
    }

    /// `tryShriek`：能响应时先给玩家加警告，加上了才尖叫；加不上（冷却）就静默。
    /// 不能响应（和平 / `can_summon` 关）时照常尖叫。
    fn try_shriek(&mut self, world: &mut World<'_>, player: Option<crate::entity::EntityUid>) {
        let Some(player) = player else {
            return;
        };
        let state = world.block_state(self.pos);
        if state.get(SHRIEKING) {
            return;
        }
        self.warning_level = 0;
        if self.can_respond(world) {
            let Some(level) = crate::behaviour::misc::warden::warn_player(world, player) else {
                return;
            };
            self.warning_level = level;
        }
        self.shriek(world);
    }

    /// `shriek`：置 `SHRIEKING`、90 刻后复位，发 3007 事件和 `shriek` 游戏事件。
    fn shriek(&mut self, world: &mut World<'_>) {
        let state = world.block_state(self.pos);
        world.set_block(self.pos, state.with(SHRIEKING, true), 2);
        world.schedule_tick(self.pos, state.block(), SHRIEKING_TICKS);
        world.level_event(3007, self.pos, 0);
        let center = Vec3::new(
            f64::from(self.pos.x) + 0.5,
            f64::from(self.pos.y) + 0.5,
            f64::from(self.pos.z) + 0.5,
        );
        world.game_event(
            GameEvent::Shriek,
            center,
            crate::game_event::dispatch::EventContext::default(),
        );
    }

    /// `tryRespond`：计划刻里 `warning_level > 0` 时尝试召唤监守者（留接口），
    /// 并给附近玩家上黑暗。
    pub(crate) fn try_respond(&mut self, world: &mut World<'_>) {
        if self.warning_level <= 0 {
            return;
        }
        // 监守者本体这一轮还没有，召唤这一步先留成空接口（见 README 已知限制）。
        let center = Vec3::new(
            f64::from(self.pos.x) + 0.5,
            f64::from(self.pos.y) + 0.5,
            f64::from(self.pos.z) + 0.5,
        );
        world.darkness_around(center, DARKNESS_RADIUS, DARKNESS_TICKS);
    }

    /// `canRespond`：`can_summon` 且非和平且 `spawn_wardens` 游戏规则开。
    fn can_respond(&self, world: &World<'_>) -> bool {
        world.block_state(self.pos).get(CAN_SUMMON)
            && world.level().difficulty_id() != 0
            && world.level().block_game_rules().spawn_wardens
    }

    fn take_shriek(&mut self) -> Option<Option<crate::entity::EntityUid>> {
        self.shriek.take()
    }
}

impl BlockEntity for SculkShriekerEntity {
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
        let adjacent =
            crate::behaviour::misc::sculk_entity::adjacent_chunks_ticking(world, self.pos);
        let shriek = {
            let mut user = Adapter { entity: self };
            let mut data = user.entity.listener;
            vibration::tick(&mut data, &mut user, now, listener_pos, adjacent);
            user.entity.listener = data;
            user.entity.take_shriek()
        };
        if let Some(player) = shriek {
            self.try_shriek(world, player);
        }
    }

    fn sleep_from(&mut self, _now: i64) -> Option<i64> {
        (self.listener.current.is_none() && self.listener.selector.codec_event().is_none())
            .then_some(i64::MAX)
    }

    fn materialize(&mut self, _now: i64) {
        self.listener.reload_particle = false;
    }

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
        Some(8)
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
        if !event.in_shrieker_can_listen_tag() {
            return false;
        }
        if world.block_state(self.pos).get(SHRIEKING) {
            return false;
        }
        let Some(player) = context.source_player else {
            return false;
        };
        if self.listener.current.is_some() {
            return false;
        }
        self.handle_game_event(event, source, listener_pos, Some(player), now);
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

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, Error> {
        Ok(self.additional())
    }
}

/// 把实体临时当成 `VibrationUser`，`on_receive` 只记账，方块写入统一在 `tick` 做。
struct Adapter<'a> {
    entity: &'a mut SculkShriekerEntity,
}

impl VibrationUser for Adapter<'_> {
    fn radius(&self) -> i32 {
        8
    }

    fn position(&self) -> Option<Vec3> {
        Some(Vec3::new(
            f64::from(self.entity.pos.x) + 0.5,
            f64::from(self.entity.pos.y) + 0.5,
            f64::from(self.entity.pos.z) + 0.5,
        ))
    }

    fn can_receive(&self, _event: GameEvent, _source: BlockPos) -> bool {
        true
    }

    fn on_receive(
        &mut self,
        _event: GameEvent,
        _source: BlockPos,
        source_player: Option<crate::entity::EntityUid>,
        _distance: f32,
    ) {
        self.entity.shriek = Some(source_player);
    }

    fn requires_adjacent_chunks(&self) -> bool {
        true
    }

    fn send_particles(&mut self, _from: Vec3, _to: Vec3, _travel: i32) -> usize {
        0
    }
}

/// 这一格是会发游戏事件的尖啸体方块（缓存守卫用）。
pub const fn is_vibration_listener_block(kind: usize) -> bool {
    kind == blocks::SCULK_SHRIEKER as usize
}

/// 新建尖啸体方块实体，供 `on_place` 和读档共用。
pub fn create_block_entity(pos: BlockPos) -> Box<dyn BlockEntity> {
    Box::new(SculkShriekerEntity::new(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, testing::TestLevel};
    use mc_bamboo_data::blocks;

    const SHRIEKER: BlockPos = BlockPos::new(0, 64, 0);

    #[test]
    fn a_player_sensor_click_makes_the_shrieker_shriek() {
        let mut level = TestLevel::new();
        // 天然生成的尖啸体 `can_summon=true`，才能警告玩家。
        level.set(
            SHRIEKER,
            BlockState(blocks::SCULK_SHRIEKER).with(CAN_SUMMON, true),
        );
        // 玩家在 1 格内，能收到警告；uid 1。
        level.players.push(crate::entity::ctx::PlayerView {
            uid: 1,
            position: mc_bamboo_core::Vec3::new(1.5, 64.0, 0.5),
            ..crate::entity::mob::throttle::test_player(
                1,
                mc_bamboo_core::Vec3::new(1.5, 64.0, 0.5),
            )
        });
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block_entity(create_block_entity(SHRIEKER));
        }
        // 感测体激活后发出的 `sculk_sensor_tendrils_clicking`，源是玩家。
        let mut world = World::new(&mut level, &mut ticks);
        world.game_event(
            GameEvent::SculkSensorTendrilsClicking,
            mc_bamboo_core::Vec3::new(4.5, 64.5, 0.5),
            crate::game_event::dispatch::EventContext {
                source_player: Some(1),
                ..Default::default()
            },
        );
        for t in 1..=5 {
            world.ticks_mut().set_time(t);
            world.tick_block_entities();
        }
        assert!(
            world.block_state(SHRIEKER).get(SHRIEKING),
            "尖啸体要进入尖叫状态"
        );
        assert_eq!(
            world
                .block_entity::<SculkShriekerEntity>(SHRIEKER)
                .map(|e| e.warning_level),
            Some(1),
            "警告等级加到 1"
        );
    }
}
