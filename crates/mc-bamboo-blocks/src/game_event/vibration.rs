//! `VibrationSystem` / `VibrationSelector` / `VibrationInfo` 的运行时数据。
//!
//! 这里只管一个监听者收到候选之后的选择、飞行和存档字段；谁监听、怎么分发在
//! [`super::dispatch`] 和 [`super::registry`]。方块实体把自己的 `VibrationData`
//! 每刻交给 [`tick`]，到达时回调 `User::on_receive`。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::Tag;

use crate::game_event::{GameEvent, redstone_strength_for_distance};

/// `VibrationInfo`：一次事件的频率由 `game_event` 查表得到。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VibrationInfo {
    pub event: GameEvent,
    /// `Vec3.distanceTo` 的 `double` 再收窄成 `float`。
    pub distance: f32,
    pub pos: Vec3,
    /// 发出事件的玩家（`tryGetPlayer`）；存档里不写，运行时传给尖啸体。
    pub source_player: Option<crate::entity::EntityUid>,
    /// `VibrationInfo.getEntity()`：发出事件的源实体；存档里不写。
    ///
    /// `VibrationInfo.getEntity()`: the source entity; not written to disk.
    pub source_entity: Option<crate::entity::EntityUid>,
    /// `VibrationInfo.getProjectileOwner()`：射弹的拥有者；存档里不写。
    ///
    /// `VibrationInfo.getProjectileOwner()`: the projectile owner; not written to disk.
    pub projectile_owner: Option<crate::entity::EntityUid>,
    /// `VibrationInfo.CODEC` 的 `source`：`UUIDUtil.CODEC` 四元 int 数组。
    ///
    /// `VibrationInfo.CODEC`'s `source`: a four-word UUID int array.
    pub source_uuid: Option<[u8; 16]>,
    /// `VibrationInfo.CODEC` 的 `projectile_owner`。
    ///
    /// `VibrationInfo.CODEC`'s `projectile_owner`.
    pub projectile_owner_uuid: Option<[u8; 16]>,
}

/// `VibrationSelector`：同一刻来的候选只留距离最近、等距时频率最高的一个。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VibrationSelector {
    candidate: Option<(VibrationInfo, i64)>,
}

impl VibrationSelector {
    /// `addCandidate`：候选的刻与已有候选不同时保留先来的。
    pub fn add_candidate(&mut self, info: VibrationInfo, game_time: i64) {
        let replace = match self.candidate {
            None => true,
            Some((old, tick)) => {
                tick == game_time
                    && (info.distance < old.distance
                        || (info.distance == old.distance
                            && info.event.vibration_frequency() > old.event.vibration_frequency()))
            }
        };
        if replace {
            self.candidate = Some((info, game_time));
        }
    }

    /// `chosenCandidate`：候选必须在下一刻之后才选中。
    pub fn chosen(&self, game_time: i64) -> Option<VibrationInfo> {
        self.candidate
            .filter(|(_, tick)| *tick < game_time)
            .map(|(info, _)| info)
    }

    /// `startOver`。
    pub fn start_over(&mut self) {
        self.candidate = None;
    }

    /// `VibrationSelector.CODEC` 的 `tick`：没有候选时恒为 `-1`。
    pub fn codec_tick(self) -> i64 {
        self.candidate.map_or(-1, |(_, tick)| tick)
    }

    /// 存档里的候选（codec 的 `event` 字段）。
    pub fn codec_event(self) -> Option<VibrationInfo> {
        self.candidate.map(|(info, _)| info)
    }
}

/// `VibrationSystem.Data`：正在飞的事件、选择器和剩余刻数。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VibrationData {
    pub current: Option<VibrationInfo>,
    pub selector: VibrationSelector,
    pub travel: i32,
    /// 读档后要补发一次飞行粒子。
    pub reload_particle: bool,
}

impl VibrationData {
    /// `new Data()`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按 `VibrationSystem.Data.CODEC` 读出的三个字段还原。
    pub fn from_codec(
        current: Option<VibrationInfo>,
        selector: VibrationSelector,
        travel: i32,
    ) -> Self {
        Self {
            current,
            selector,
            travel: travel.max(0),
            reload_particle: true,
        }
    }

    /// `decrementTravelTime`：`Math.max(0, event_delay - 1)`。
    pub fn decrement_travel(&mut self) {
        self.travel = self.travel.saturating_sub(1);
    }

    /// `VibrationSystem.Data.CODEC` 写出的键序：`event`、`selector`、`event_delay`。
    pub fn write_codec(self) -> Vec<(String, Tag)> {
        let mut out = Vec::new();
        if let Some(info) = self.current {
            put(&mut out, "event", info.write_codec());
        }
        let mut selector = Vec::new();
        if let Some(info) = self.selector.codec_event() {
            put(&mut selector, "event", info.write_codec());
        }
        put(&mut selector, "tick", Tag::Long(self.selector.codec_tick()));
        put(&mut out, "selector", Tag::Compound(selector));
        put(&mut out, "event_delay", Tag::Int(self.travel));
        out
    }
}

impl VibrationInfo {
    /// `VibrationInfo.CODEC`。`source` / `projectile_owner` 只有玩家实体才有，
    /// 运行时不保存实体 UUID，存档里按官方的「可省字段」省略。
    pub fn write_codec(self) -> Tag {
        let pos = |value: f64| Tag::Double(value);
        let mut out = vec![
            ("game_event".to_owned(), Tag::String(self.event.full_name())),
            ("distance".to_owned(), Tag::Float(self.distance)),
            (
                "pos".to_owned(),
                Tag::List(vec![pos(self.pos.x), pos(self.pos.y), pos(self.pos.z)]),
            ),
        ];
        if let Some(uuid) = self.source_uuid {
            put(&mut out, "source", uuid_tag(&uuid));
        }
        if let Some(uuid) = self.projectile_owner_uuid {
            put(&mut out, "projectile_owner", uuid_tag(&uuid));
        }
        Tag::Compound(out)
    }

    /// `VibrationSelector` / `Data` codec 的坏数据都回到默认值，这里只认
    /// 注册表里的名字和三个有限坐标，客户端 / 存档数据不会触发 panic。
    pub fn read_codec(tag: &Tag) -> Option<Self> {
        let Tag::Compound(values) = tag else {
            return None;
        };
        let event = values
            .iter()
            .rev()
            .find(|(key, _)| key == "game_event")
            .and_then(|(_, value)| match value {
                Tag::String(name) => GameEvent::from_name(name),
                _ => None,
            })?;
        let distance = values
            .iter()
            .rev()
            .find(|(key, _)| key == "distance")
            .and_then(|(_, value)| crate::archive::tag::java_f32(value).ok())?;
        if !distance.is_finite() || distance < 0.0 {
            return None;
        }
        let pos =
            values.iter().rev().find(|(key, _)| key == "pos").and_then(
                |(_, value)| match value {
                    Tag::List(coords) if coords.len() == 3 => Some(coords),
                    _ => None,
                },
            )?;
        let mut xyz = [0.0; 3];
        for (i, value) in pos.iter().enumerate() {
            xyz[i] = crate::archive::tag::java_f64(value).ok()?;
            if !xyz[i].is_finite() {
                return None;
            }
        }
        Some(Self {
            event,
            distance,
            pos: Vec3::new(xyz[0], xyz[1], xyz[2]),
            source_player: None,
            source_entity: None,
            projectile_owner: None,
            source_uuid: values
                .iter()
                .rev()
                .find(|(key, _)| key == "source")
                .and_then(|(_, value)| uuid_bytes(value)),
            projectile_owner_uuid: values
                .iter()
                .rev()
                .find(|(key, _)| key == "projectile_owner")
                .and_then(|(_, value)| uuid_bytes(value)),
        })
    }
}

/// `UUIDUtil.CODEC` 的 `int[4]`。
fn uuid_tag(bytes: &[u8; 16]) -> Tag {
    let mut words = [0i32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        let mut value = 0u32;
        for &byte in &bytes[index * 4..index * 4 + 4] {
            value = value << 8 | u32::from(byte);
        }
        *word = value as i32;
    }
    Tag::IntArray(words.to_vec())
}

/// 读 `UUIDUtil.CODEC` 的四元 int 数组；长度不对就当作缺省字段。
fn uuid_bytes(tag: &Tag) -> Option<[u8; 16]> {
    let Tag::IntArray(words) = tag else {
        return None;
    };
    if words.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, word) in words.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
    }
    Some(bytes)
}

/// 同名键取最后一个，写档时按 codec 的键序覆盖旧值。
fn put(out: &mut Vec<(String, Tag)>, key: &str, value: Tag) {
    crate::archive::tag::put(out, key, value);
}

/// `VibrationSystem.User`：每个监听者给出半径、位置和到达后的动作。
pub trait VibrationUser {
    /// `getListenerRadius`。
    fn radius(&self) -> i32;
    /// `getPositionSource().getPosition(level)`：方块实体就是格心。
    fn position(&self) -> Option<Vec3>;
    /// `canReceiveVibration`，在分发时当场判断。
    fn can_receive(&self, event: GameEvent, source: BlockPos) -> bool;
    /// `onReceiveVibration`，`distance` 是两个整数格之间的欧氏距离。
    fn on_receive(
        &mut self,
        event: GameEvent,
        source: BlockPos,
        source_player: Option<crate::entity::EntityUid>,
        distance: f32,
    );
    /// `VibrationSystem.Ticker` 的完整回调：额外带源实体和射弹拥有者。默认实现
    /// 只转发旧签名，实体监听者可以覆写拿到这两个实体。
    ///
    /// The full `VibrationSystem.Ticker` callback with the source entity and
    /// projectile owner. The default forwards to [`Self::on_receive`]; entity
    /// listeners may override it to use both entities.
    fn on_receive_full(
        &mut self,
        event: GameEvent,
        source: BlockPos,
        source_player: Option<crate::entity::EntityUid>,
        source_entity: Option<crate::entity::EntityUid>,
        projectile_owner: Option<crate::entity::EntityUid>,
        distance: f32,
    ) {
        let _ = (source_entity, projectile_owner);
        self.on_receive(event, source, source_player, distance);
    }
    /// `calculateTravelTimeInTicks` 的默认值 `Mth.floor(distance)`。
    fn travel_ticks(&self, distance: f32) -> i32 {
        mc_bamboo_core::mth::floor_f32(distance)
    }
    /// `requiresAdjacentChunksToBeTicking`。
    fn requires_adjacent_chunks(&self) -> bool {
        false
    }
    /// `onDataChanged`：方块实体在这里标脏。
    fn on_data_changed(&mut self) {}
    /// `sendParticles`：返回发给了几个玩家；返回 0 时补发粒子的标记不清除。
    fn send_particles(&mut self, from: Vec3, to: Vec3, travel: i32) -> usize;
}

/// 新事件到达候选池时叫醒方块实体；分发层用 `wake` 把这件事交给表。
pub fn add_candidate(
    data: &mut VibrationData,
    info: VibrationInfo,
    game_time: i64,
    wake: &mut impl FnMut(),
) {
    let before = data.selector.codec_event();
    data.selector.add_candidate(info, game_time);
    if data.selector.codec_event() != before {
        wake();
    }
}

/// `VibrationSystem.Ticker.tick` 的一次执行。`listener_pos` 是当前的监听点，
/// `adjacent_chunks_ticking` 由世界查好后传进来。返回是否改了持久数据。
pub fn tick<U: VibrationUser>(
    data: &mut VibrationData,
    user: &mut U,
    game_time: i64,
    listener_pos: Option<Vec3>,
    adjacent_chunks_ticking: bool,
) -> bool {
    let mut changed = data.travel > 0;
    if data.current.is_none() {
        if let Some(info) = data.selector.chosen(game_time) {
            data.current = Some(info);
            data.travel = user.travel_ticks(info.distance);
            user.send_particles(info.pos, info.pos, data.travel);
            user.on_data_changed();
            data.selector.start_over();
            changed = true;
        } else {
            return false;
        }
    }

    let Some(info) = data.current else {
        return false;
    };
    let target = listener_pos.unwrap_or(info.pos);
    if data.travel > 0 {
        try_reload_particle(data, user, info, target);
    }
    data.decrement_travel();
    if data.travel <= 0 {
        let arrived = if user.requires_adjacent_chunks() && !adjacent_chunks_ticking {
            false
        } else {
            let source = BlockPos::containing(info.pos);
            let listener = BlockPos::containing(target);
            user.on_receive_full(
                info.event,
                source,
                info.source_player,
                info.source_entity,
                info.projectile_owner,
                distance_between_in_blocks(source, listener),
            );
            data.current = None;
            true
        };
        changed |= arrived;
    }
    if changed {
        user.on_data_changed();
    }
    changed
}

/// `tryReloadVibrationParticle`：读档后补一次飞行中的粒子。
fn try_reload_particle<U: VibrationUser>(
    data: &mut VibrationData,
    user: &mut U,
    info: VibrationInfo,
    target: Vec3,
) {
    if !data.reload_particle {
        return;
    }
    if data.current.is_none() {
        data.reload_particle = false;
        return;
    }
    let total = user.travel_ticks(info.distance);
    let progress = if total > 0 {
        1.0 - f64::from(data.travel) / f64::from(total)
    } else {
        1.0
    };
    let position = lerp(info.pos, target, progress);
    if user.send_particles(position, position, data.travel) > 0 {
        data.reload_particle = false;
    }
}

/// `Mth.lerp(double, Vec3, Vec3)`。
fn lerp(from: Vec3, to: Vec3, progress: f64) -> Vec3 {
    Vec3::new(
        from.x + (to.x - from.x) * progress,
        from.y + (to.y - from.y) * progress,
        from.z + (to.z - from.z) * progress,
    )
}

/// `VibrationSystem.Listener.distanceBetweenInBlocks`。
pub fn distance_between_in_blocks(a: BlockPos, b: BlockPos) -> f32 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    let dz = f64::from(a.z - b.z);
    ((dx * dx + dy * dy + dz * dz).sqrt()) as f32
}

/// `VibrationSystem.getRedstoneStrengthForDistance` 的转发，方便方模块复用。
pub fn redstone_strength(distance: f32, radius: i32) -> i32 {
    redstone_strength_for_distance(distance, radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestUser {
        received: Vec<(GameEvent, f32)>,
        particles: Vec<(Vec3, i32)>,
    }

    impl VibrationUser for TestUser {
        fn radius(&self) -> i32 {
            8
        }
        fn position(&self) -> Option<Vec3> {
            Some(Vec3::new(8.5, 0.5, 0.5))
        }
        fn can_receive(&self, _event: GameEvent, _source: BlockPos) -> bool {
            true
        }
        fn on_receive(
            &mut self,
            event: GameEvent,
            _source: BlockPos,
            _source_player: Option<crate::entity::EntityUid>,
            distance: f32,
        ) {
            self.received.push((event, distance));
        }
        fn send_particles(&mut self, from: Vec3, _to: Vec3, travel: i32) -> usize {
            self.particles.push((from, travel));
            1
        }
    }

    fn info(event: GameEvent, distance: f32) -> VibrationInfo {
        VibrationInfo {
            event,
            distance,
            pos: Vec3::new(0.5, 0.5, 0.5),
            source_player: None,
            source_entity: None,
            projectile_owner: None,
            source_uuid: None,
            projectile_owner_uuid: None,
        }
    }

    #[test]
    fn selector_prefers_the_nearest_then_the_highest_frequency() {
        let mut selector = VibrationSelector::default();
        selector.add_candidate(info(GameEvent::Step, 5.0), 10);
        selector.add_candidate(info(GameEvent::Explode, 6.0), 10);
        assert_eq!(selector.codec_event().unwrap().distance, 5.0);
        selector.add_candidate(info(GameEvent::Explode, 5.0), 10);
        assert_eq!(selector.codec_event().unwrap().event, GameEvent::Explode);
        selector.add_candidate(info(GameEvent::Step, 1.0), 11);
        assert_eq!(selector.codec_event().unwrap().distance, 5.0);
        assert_eq!(selector.chosen(10), None);
        assert_eq!(selector.chosen(11).unwrap().event, GameEvent::Explode);
        selector.start_over();
        assert_eq!(selector.codec_tick(), -1);
    }

    #[test]
    fn vibration_travels_one_tick_per_block_before_arriving() {
        let mut data = VibrationData::new();
        data.selector.add_candidate(info(GameEvent::Step, 8.0), 0);
        let mut user = TestUser {
            received: Vec::new(),
            particles: Vec::new(),
        };
        for t in 1..8 {
            let listener_pos = user.position();
            tick(&mut data, &mut user, t, listener_pos, true);
        }
        assert!(user.received.is_empty());
        assert_eq!(user.particles.len(), 1);
        assert_eq!(user.particles[0].1, 8);
        let listener_pos = user.position();
        tick(&mut data, &mut user, 8, listener_pos, true);
        assert_eq!(user.received.len(), 1);
        assert_eq!(user.received[0].0, GameEvent::Step);
        assert_eq!(user.received[0].1, 8.0);
        assert!(data.current.is_none());
    }

    #[test]
    fn codec_round_trip_keeps_the_scheduled_event() {
        let mut data = VibrationData::new();
        data.current = Some(info(GameEvent::BlockPlace, 3.5));
        data.selector.add_candidate(info(GameEvent::Step, 2.0), 42);
        data.travel = 3;
        let entries = data.write_codec();
        let tag = Tag::Compound(entries.clone());
        let listener = crate::archive::passive::read_listener_for_vibration(&tag);
        assert_eq!(listener.current.unwrap().event, GameEvent::BlockPlace);
        assert_eq!(listener.selector.codec_tick(), 42);
        assert_eq!(listener.travel, 3);
        assert!(listener.reload_particle);
        let written = VibrationInfo::read_codec(
            &entries
                .iter()
                .rev()
                .find(|(key, _)| key == "event")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(written.event, GameEvent::BlockPlace);
        assert_eq!(written.distance, 3.5);
        assert_eq!(written.pos.x, 0.5);
        assert_eq!(entries.last().unwrap().1, Tag::Int(3));
    }
}
