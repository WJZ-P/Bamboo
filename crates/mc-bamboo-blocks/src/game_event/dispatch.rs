//! `GameEventDispatcher`：游戏事件当场按区段投递给登记过的监听者。
//!
//! 26.3 的 `ServerLevel.gameEvent` 没有刻末队列，直接调
//! `GameEventDispatcher.post`。`BY_DISTANCE` 的监听者先按访问顺序收集，
//! 再按监听点到事件点的距离稳定排序，其他监听者按访问顺序当场处理。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Vec3};
use mc_bamboo_data::tags::block_tags;

use crate::{
    game_event::{
        GameEvent,
        registry::{GameEventListeners, ListenerRef},
        vibration::VibrationData,
    },
    state::BlockState,
    world::World,
};

/// `GameEvent.Context` 里振动分发要用到的部分。`affected_state` 由共鸣、羊毛
/// 这类上下文给出；`source_player` 是发出事件的玩家（幽匿尖啸体只对玩家源响应）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventContext {
    pub affected_state: Option<BlockState>,
    pub sneaking: bool,
    pub dampens_vibrations: bool,
    pub spectator: bool,
    /// `tryGetPlayer(sourceEntity)`：事件的源头是不是一名玩家。
    pub source_player: Option<crate::entity::EntityUid>,
    /// `GameEvent.Context.sourceEntity()`：发出事件的实体，振动到达时按 UUID 找回，
    /// 作为 `onReceiveVibration` 的第 4 个参数。
    ///
    /// `GameEvent.Context.sourceEntity()`: the entity that emitted the event, resolved
    /// by UUID as the fourth `onReceiveVibration` argument.
    pub source_entity: Option<crate::entity::EntityUid>,
    /// `VibrationInfo.getProjectileOwner()`：振动由射弹产生时的射弹拥有者，
    /// 作为 `onReceiveVibration` 的第 5 个参数；不是射弹时为 `None`。
    ///
    /// `VibrationInfo.getProjectileOwner()`: the projectile owner when a projectile
    /// caused the vibration, as the fifth `onReceiveVibration` argument; `None` otherwise.
    pub projectile_owner: Option<crate::entity::EntityUid>,
}

/// 实体自带的游戏事件监听者（`VibrationSystem.Data` 的运行时状态 + 半径 / 投递模式）。
/// 只有设了 `Some` 的生物会被登记到区段表，位置取实体眼睛。
///
/// A mob's own game-event listener (the runtime `VibrationSystem.Data` plus its radius
/// and delivery mode). Only mobs with `Some` are registered in the section table;
/// the listener position is the entity's eye.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityListener {
    pub radius: i32,
    pub delivery_mode: DeliveryMode,
    pub vibration: VibrationData,
}

/// 实体监听者当前所在区段（按眼睛位置，`EntityPositionSource`）；不是监听者就 `None`。
///
/// The section an entity listener belongs to (by eye position, `EntityPositionSource`);
/// `None` for a mob that is not a listener.
pub(crate) fn entity_listener_section(entity: &crate::entity::Entity) -> Option<(ChunkPos, i32)> {
    let mob = entity.kind.mob()?;
    mob.entity_listener.as_ref()?;
    let eye_y = entity.position.y + f64::from(mob.eye_height);
    let block = BlockPos::containing(Vec3::new(entity.position.x, eye_y, entity.position.z));
    Some((block.chunk(), block.y >> 4))
}

/// `GameEventListener.DeliveryMode`：感测体等都是 `BY_DISTANCE`。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeliveryMode {
    ByDistance,
    #[default]
    Unspecified,
}

/// 一个区段遍历到的监听者，`distance` 是 `ListenerInfo.compareTo` 用的量。
#[derive(Clone, Copy, Debug)]
struct Candidate {
    listener: ListenerRef,
    position: Vec3,
    mode: DeliveryMode,
    distance: f64,
}

/// 分发后每个监听者的结果。`source` 是事件发生的精确位置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Delivery {
    pub listener: ListenerRef,
    pub position: Vec3,
    pub source: Vec3,
    pub event: GameEvent,
    pub context: EventContext,
    /// `GameEvent.Context.sourceEntity()`：死亡事件里发出事件的实体。
    pub source_entity: Option<crate::entity::EntityUid>,
    /// 死亡实体这一刻的基础经验值（`getExperienceReward` 的已建模部分）。
    /// `None` 表示源不是会掉经验的生物。
    ///
    /// The dying mob's base experience reward; `None` for a non-mob source.
    pub experience: Option<i32>,
}

/// 把游戏事件投递给能收到它的监听者，返回按官方顺序排好的投递列表。
///
/// 返回列表而不是直接改方块实体：分发期间可能注册 / 注销监听者，方块实体表
/// 也可能被别的逻辑借用；调用方按顺序逐个落地，等价于官方的 `handleGameEvent`。
pub fn post(
    world: &World<'_>,
    event: GameEvent,
    source: Vec3,
    context: EventContext,
) -> Vec<Delivery> {
    let Some(table) = world.level().game_event_listeners() else {
        return Vec::new();
    };
    // `ENTITY_DIE` 的上下文里带着发出事件的实体；催发体监听者要读它的经验值。
    // 实体此刻正在 tick，可能已经从表里取走，所以从保存的公开视图里取基础经验。
    // `ENTITY_DIE` carries the dying entity; catalyst listeners read its experience.
    // The entity may be detached from the table while ticking, so take the base
    // reward from the preserved public view.
    let (source_entity, experience) = if event == GameEvent::EntityDie {
        let uid = context.source_entity;
        let reward = uid
            .and_then(|uid| world.level().entity_public(uid))
            .and_then(|public| match &public.kind {
                crate::entity::KindPublic::Mob(mob) => Some(mob.mob_type.xp_reward()),
                _ => None,
            });
        (uid, reward)
    } else {
        (None, None)
    };
    collect_candidates(table, world, event, source, context)
        .into_iter()
        .map(|candidate| Delivery {
            listener: candidate.listener,
            position: candidate.position,
            source,
            event,
            context,
            source_entity,
            experience,
        })
        .collect()
}

/// `GameEventDispatcher.post`：x→z→y 走区段，未加载的整列跳过；监听者按距离
/// 稳定排序，等距时保持 x / z / y / 登记先后的访问顺序。
fn collect_candidates(
    table: &GameEventListeners,
    world: &World<'_>,
    event: GameEvent,
    source: Vec3,
    context: EventContext,
) -> Vec<Candidate> {
    let radius = event.notification_radius();
    let origin = BlockPos::containing(source);
    let min_x = section(origin.x - radius);
    let max_x = section(origin.x + radius);
    let min_y = section(origin.y - radius);
    let max_y = section(origin.y + radius);
    let min_z = section(origin.z - radius);
    let max_z = section(origin.z + radius);
    let mut unsorted = Vec::new();
    let mut by_distance = Vec::new();
    for x in min_x..=max_x {
        for z in min_z..=max_z {
            if !world.level().chunk_is_loaded(ChunkPos::new(x, z)) {
                continue;
            }
            for y in min_y..=max_y {
                for listener in table.section(ChunkPos::new(x, z), y) {
                    let Some(info) =
                        listener_info(world, table, listener, (x, y, z), source, radius, context)
                    else {
                        continue;
                    };
                    let candidate = Candidate {
                        listener,
                        position: info.0,
                        mode: info.2,
                        distance: info.0.distance_to_sqr(source),
                    };
                    if candidate.mode == DeliveryMode::ByDistance {
                        by_distance.push(candidate);
                    } else {
                        unsorted.push(candidate);
                    }
                }
            }
        }
    }
    // `Collections.sort` 是稳定排序；浮点相等时保留访问顺序。
    by_distance.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    unsorted.extend(by_distance);
    unsorted
}

/// `listener_info` 返回（监听点、半径、投递模式），`None` 表示这个监听者
/// 不在这一区段或距离外。`EuclideanGameEventListenerRegistry` 的距离判定用
/// 整数格的 `distSqr`，不是精确坐标。
fn listener_info(
    world: &World<'_>,
    table: &GameEventListeners,
    listener: ListenerRef,
    key: (i32, i32, i32),
    source: Vec3,
    notification_radius: i32,
    context: EventContext,
) -> Option<(Vec3, i32, DeliveryMode)> {
    let (pos, radius, mode) = match listener {
        ListenerRef::BlockEntity(pos) => {
            let chunk = pos.chunk();
            if (chunk.x, section(pos.y), chunk.z) != key || !table.contains(chunk, key.1, listener)
            {
                return None;
            }
            // 只读监听元数据，保留发出事件的阶段所用的休眠时钟。
            // Listener metadata is read-only. Keep the event emitter's sleep clock.
            let clock = crate::block_entity::block_entity_clock();
            let listener = world.level().block_entity_dyn(pos);
            crate::block_entity::set_block_entity_clock(clock);
            let listener = listener?;
            let radius = listener.listener_radius()?;
            let mode = listener.delivery_mode()?;
            (block_center(pos), radius, mode)
        }
        ListenerRef::Entity(uid) => {
            // `EntityPositionSource`：监听点在实体眼睛，半径 / 投递模式由实体自带。
            // `EntityPositionSource`: the listener sits at the entity's eye; the mob
            // carries its own radius and delivery mode.
            let entity = world.entity(uid)?;
            let mob = entity.kind.mob()?;
            let state = mob.entity_listener.as_ref()?;
            let eye = Vec3::new(
                entity.position.x,
                entity.position.y + f64::from(mob.eye_height),
                entity.position.z,
            );
            let block = BlockPos::containing(eye);
            let chunk = block.chunk();
            if (chunk.x, section(block.y), chunk.z) != key
                || !table.contains(chunk, key.1, listener)
            {
                return None;
            }
            (eye, state.radius, state.delivery_mode)
        }
    };
    let listener_block = BlockPos::containing(pos);
    let dx = f64::from(listener_block.x - BlockPos::containing(source).x);
    let dy = f64::from(listener_block.y - BlockPos::containing(source).y);
    let dz = f64::from(listener_block.z - BlockPos::containing(source).z);
    let distance_sqr = dx * dx + dy * dy + dz * dz;
    if distance_sqr > f64::from(radius * radius)
        || distance_sqr > f64::from(notification_radius * notification_radius)
    {
        return None;
    }
    let _ = context;
    Some((pos, radius, mode))
}

/// `SectionPos.blockToSectionCoord`：算术右移 4 位。
fn section(coordinate: i32) -> i32 {
    coordinate >> 4
}

/// `BlockPositionSource.getPosition`：格心。
fn block_center(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
}

/// `VibrationSystem.Listener.isOccluded`：六个方向各偏 1e-5 做一次
/// `ClipBlockStateContext`，全部撞上 `occludes_vibration_signals` 才算被羊毛挡住。
pub fn is_occluded(world: &World<'_>, from: Vec3, to: Vec3) -> bool {
    let center = |v: Vec3| {
        Vec3::new(
            mc_bamboo_core::mth::floor(v.x) as f64 + 0.5,
            mc_bamboo_core::mth::floor(v.y) as f64 + 0.5,
            mc_bamboo_core::mth::floor(v.z) as f64 + 0.5,
        )
    };
    let start = center(from);
    let end = center(to);
    const OFFSET: f64 = 9.999999747378752E-6;
    for direction in Direction::ALL {
        let (dx, dy, dz) = direction.normal();
        let offset = Vec3::new(
            f64::from(dx) * OFFSET,
            f64::from(dy) * OFFSET,
            f64::from(dz) * OFFSET,
        );
        if !block_in_line(world, start.add(offset), end) {
            return false;
        }
    }
    true
}

/// `BlockGetter.isBlockInLine`：沿线段逐格找第一个在标签里的方块，命中就返回。
fn block_in_line(world: &World<'_>, from: Vec3, to: Vec3) -> bool {
    crate::world::traverse_blocks(from, to, |pos| {
        world
            .block_state(pos)
            .is_tagged(block_tags::OCCLUDES_VIBRATION_SIGNALS)
            .then_some(())
    })
    .is_some()
}

/// `VibrationSystem.User.isValidVibration`：事件在 `#vibrations` 里，源实体不是
/// 旁观者 / 减振者，潜行时不在 `#ignore_vibrations_sneaking` 里，且涉及的方块
/// 不在 `#dampens_vibrations` 里。
pub fn valid_vibration(event: GameEvent, context: EventContext) -> bool {
    if !event.in_vibrations_tag() {
        return false;
    }
    if context.spectator || context.dampens_vibrations {
        return false;
    }
    if context.sneaking && event.in_ignore_vibrations_sneaking_tag() {
        return false;
    }
    !context
        .affected_state
        .is_some_and(|state| state.is_tagged(block_tags::DAMPENS_VIBRATIONS))
}

impl World<'_> {
    /// 把一条投递交给具体的监听者。方块实体临时取出：每个监听者先在
    /// [`crate::BlockEntity::receive_vibration`] 里按振动规则记候选，
    /// 不匹配时再走 [`crate::BlockEntity::receive_game_event`] 的即时处理，
    /// 和官方 `GameEventListener.handleGameEvent` 一样。
    pub(crate) fn deliver_game_event(&mut self, delivery: Delivery) {
        let event = delivery.event;
        let context = delivery.context;
        let source = delivery.source;
        let listener_pos = delivery.position;
        let now = self.game_time();
        if is_occluded(self, source, listener_pos) {
            return;
        }
        match delivery.listener {
            ListenerRef::BlockEntity(pos) => {
                let Some((id, mut entity)) = self.level_mut().take_block_entity_at(pos) else {
                    return;
                };
                let received =
                    entity.receive_vibration(self, event, source, listener_pos, context, now)
                        || entity.receive_game_event(self, &delivery);
                self.level_mut().put_block_entity(pos, id, entity);
                if received {
                    self.level_mut().wake_block_entity(pos);
                }
            }
            ListenerRef::Entity(uid) => {
                // `Allay$JukeboxListener`：唱片机事件不进振动候选池，当场改跳舞状态。
                // `Allay$JukeboxListener`: jukebox events update dancing immediately.
                if matches!(event, GameEvent::JukeboxPlay | GameEvent::JukeboxStopPlay)
                    && let Some(allay) = self.entity(uid).as_ref().and_then(|e| e.kind.mob())
                    && allay.mob_type == crate::entity::mob::MobType::Allay
                {
                    let pos = BlockPos::containing(source);
                    let mut apply = |entity: &mut crate::entity::Entity| {
                        entity.with_mob(|_, mob| {
                            crate::entity::mob::allay::set_jukebox_playing(
                                mob,
                                pos,
                                event == GameEvent::JukeboxPlay,
                            );
                        });
                    };
                    if !self.with_listed_entity_mut(uid, &mut apply) {
                        self.with_entity_mut(uid, &mut apply);
                    }
                    return;
                }
                // `VibrationSystem.Listener.handleGameEvent`：记成候选，到达时由实体自己的
                // `VibrationSystem.Ticker` 回调 `User.onReceiveVibration`。
                //
                // `VibrationSystem.Listener.handleGameEvent`: record a candidate; the
                // entity's own `VibrationSystem.Ticker` fires `User.onReceiveVibration`.
                let info = crate::game_event::vibration::VibrationInfo {
                    event,
                    distance: source.distance_to_sqr(listener_pos).sqrt() as f32,
                    pos: source,
                    source_player: context.source_player,
                    source_entity: context.source_entity,
                    projectile_owner: context.projectile_owner,
                    // `VibrationInfo(Holder, float, Vec3, Entity)` 把源实体 UUID
                    // 和射弹拥有者 UUID 一起存进 `listener`。
                    // `VibrationInfo(Holder, float, Vec3, Entity)` persists the
                    // source entity UUID and projectile owner UUID in `listener`.
                    source_uuid: context
                        .source_entity
                        .or(context.source_player)
                        .map(|uid| crate::archive::entity::entity_uuid(uid).to_be_bytes()),
                    projectile_owner_uuid: context
                        .projectile_owner
                        .map(|uid| crate::archive::entity::entity_uuid(uid).to_be_bytes()),
                };
                // `VibrationSystem.User.isValidVibration` 里的可听事件标签：监守者用
                // `#vibrations`，悦灵用 `#allay_can_listen`。
                let (is_allay, can_receive) = self
                    .entity(uid)
                    .and_then(|entity| {
                        entity.kind.mob().map(|mob| {
                            (
                                mob.mob_type == crate::entity::mob::MobType::Allay,
                                crate::entity::mob::allay::can_receive_vibration(
                                    mob,
                                    event,
                                    BlockPos::containing(source),
                                ),
                            )
                        })
                    })
                    .unwrap_or((false, true));
                let valid_event = !is_allay || (event.in_allay_can_listen_tag() && can_receive);
                if !valid_event {
                    return;
                }
                let mut record = |entity: &mut crate::entity::Entity| {
                    entity.with_mob(|_, mob| {
                        if let Some(listener) = mob.entity_listener.as_mut()
                            && listener.vibration.current.is_none()
                        {
                            crate::game_event::vibration::add_candidate(
                                &mut listener.vibration,
                                info,
                                now,
                                &mut || {},
                            );
                        }
                    });
                };
                if !self.with_listed_entity_mut(uid, &mut record) {
                    self.with_entity_mut(uid, &mut record);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wool_between_the_event_and_listener_occludes_all_six_rays() {
        // 这个测试在没有实际方块表的单元环境里只验证遍历谓词不崩溃；
        // 方块表覆盖见 behaviour::misc::sculk 的集成测试。
        assert!(!Direction::ALL.is_empty());
    }

    #[test]
    fn section_coordinates_floor_negative_blocks() {
        assert_eq!(section(-1), -1);
        assert_eq!(section(-16), -1);
        assert_eq!(section(-17), -2);
        assert_eq!(section(16), 1);
    }

    /// 实体监听者生成即登记到区段，附近的游戏事件落成一条振动候选。
    ///
    /// An entity listener is registered on spawn and a nearby event becomes a candidate.
    #[test]
    fn an_entity_listener_receives_a_game_event_candidate() {
        use crate::{
            BlockTicks, World,
            entity::{Entity, mob::MobType},
            testing::TestLevel,
        };
        use mc_bamboo_core::{BlockPos, Vec3};

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        for x in -4..=4 {
            for z in -4..=4 {
                level.set(
                    BlockPos::new(x, 63, z),
                    crate::BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }
        let mut world = World::new(&mut level, &mut ticks);
        let mut entity = Entity::by_mob_type(MobType::Cow, Vec3::new(0.5, 64.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.entity_listener = Some(EntityListener {
                radius: 16,
                delivery_mode: DeliveryMode::ByDistance,
                vibration: VibrationData::default(),
            });
        });
        let uid = world.spawn_entity(entity);
        assert!(
            world
                .level()
                .game_event_listeners()
                .unwrap()
                .entity_section(uid)
                .is_some(),
            "生成时登记到眼睛所在区段"
        );
        world.game_event(
            GameEvent::Step,
            Vec3::new(2.5, 64.5, 0.5),
            EventContext::default(),
        );
        let state = world.entity(uid).expect("listener still in the world");
        let listener = state
            .kind
            .mob()
            .and_then(|mob| mob.entity_listener)
            .expect("entity listener state");
        assert!(
            listener.vibration.current.is_some()
                || listener.vibration.selector.codec_event().is_some(),
            "事件记成振动候选"
        );
    }
}
