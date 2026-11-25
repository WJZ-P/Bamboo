//! `CreakingHeartBlockEntity`：昼夜状态、绑定的嘎枝、刷出与比较器信号。
//!
//! 心每 20~24 刻复查一次状态，夜里变 `awake`，白天变 `dormant`；两头缺了
//! 苍白橡木且没有绑定的嘎枝时变 `uprooted`。存档的 `creaking` 是官方的
//! int-array UUID。
//!
//! `CreakingHeartBlockEntity`: the day/night state, the bound creaking, its
//! spawn attempt and the comparator signal. The `creaking` key is the int-array
//! UUID.

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks, sounds, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    BlockEntity, BlockState, World,
    archive::tag::{self, Result},
    entity::{
        Entity,
        mob::{MobType, creaking as mob_creaking},
    },
    shape::{self, CollisionContext},
    state::{AXIS, CREAKING_HEART_STATE, CreakingHeartState},
};

/// `CreakingHeartBlockEntity` 存档里的 UUID（四个 int）。
type Uuid = [i32; 4];
/// `CreakingHeartBlockEntity.PLAYER_DETECTION_RANGE` 的平方。
const PLAYER_RANGE_SQ: f64 = 32.0 * 32.0;
/// `CreakingHeartBlockEntity` 判定嘎枝离心的距离。
const MAX_PROTECTOR_DISTANCE: f64 = 34.0;

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
    /// `emitter`：心受伤后剩余的连线 / 心跳效果刻数。
    emitter: i32,
    /// `emitterTarget`：连线效果的另一端。
    emitter_target: Option<Vec3>,
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
            emitter: 0,
            emitter_target: None,
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
            emitter: 0,
            emitter_target: None,
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
        self.emitter = 0;
        self.emitter_target = None;
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

    /// `getCreakingProtector`：按存档 UUID 在实体表里找回这一只嘎枝。
    fn protector(&self, world: &World) -> Option<(i32, Vec3, [u8; 16])> {
        let uuid = uuid_bytes(self.creaking?);
        for id in world.entity_ids() {
            let entity = world.entity(id)?;
            let Some(mob) = entity.kind.mob() else {
                continue;
            };
            if mob.mob_type == MobType::Creaking && mob.uuid == uuid && !entity.removed {
                return Some((id, entity.position, uuid));
            }
        }
        None
    }

    /// `CreakingHeartBlockEntity.serverTick`。
    pub fn server_tick(&mut self, world: &mut World) {
        self.ticks_existed = self.ticks_existed.saturating_add(1);
        self.resolve_creaking(world);
        if self.emitter > 0 {
            self.tick_emitter(world);
        }
        // 绑定的嘎枝每刻把距离交给心，比较器按 15 - floor(d/32*15) 读。
        let protector = self.protector(world);
        let distance = protector.map(|(_, position, _)| {
            position
                .distance_to_sqr(Vec3::new(
                    f64::from(self.pos.x) + 0.5,
                    f64::from(self.pos.y),
                    f64::from(self.pos.z) + 0.5,
                ))
                .sqrt()
        });
        self.set_protector_distance(if self.loading { None } else { distance });
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
        let active = world.creaking_active(self.pos);
        let next = self.update_state(world, state);
        if state.get(CREAKING_HEART_STATE) != next {
            world.set_block_and_update(self.pos, state.with(CREAKING_HEART_STATE, next));
        }
        if next == CreakingHeartState::Uprooted {
            return;
        }
        if self.creaking.is_some() {
            if let Some((id, _, _)) = protector
                && (!active || distance.is_some_and(|d| d > MAX_PROTECTOR_DISTANCE))
            {
                self.tear_down_creaking(world, id);
                self.clear_creaking_info();
            }
            return;
        }
        if next != CreakingHeartState::Awake {
            return;
        }
        let rules = world.block_game_rules();
        if !rules.spawn_mobs || !rules.spawn_monsters || world.difficulty_id() == 0 {
            return;
        }
        let heart = Vec3::new(
            f64::from(self.pos.x),
            f64::from(self.pos.y),
            f64::from(self.pos.z),
        );
        let player_near = world
            .player_views()
            .iter()
            .any(|player| player.position.distance_to_sqr(heart) <= PLAYER_RANGE_SQ);
        if !player_near {
            return;
        }
        if let Some((id, position, uuid)) = self.spawn_protector(world) {
            self.set_creaking_info(uuid_ints(&uuid), false);
            self.protector_distance = Some(
                position
                    .distance_to_sqr(Vec3::new(
                        f64::from(self.pos.x) + 0.5,
                        f64::from(self.pos.y),
                        f64::from(self.pos.z) + 0.5,
                    ))
                    .sqrt(),
            );
            world.play_sound(
                position,
                sounds::sound_event("minecraft:entity.creaking.spawn"),
                sounds::HOSTILE,
                1.0,
                1.0,
                false,
            );
            world.play_block_sound(
                self.pos,
                sounds::sound_event("minecraft:block.creaking_heart.spawn"),
                1.0,
                1.0,
            );
            // 官方广播 entityEvent 60；Bamboo 的实体事件表由游戏层同步。
            world.with_entity_mut(id, &mut |entity| {
                entity.with_mob(|_, mob| {
                    mob.events
                        .push(crate::entity::mob::MobEvent::EntityEvent { event: 60 });
                });
            });
        }
    }

    /// `spawnProtector`：`SpawnUtil.trySpawnMob` 的 5 次尝试和 16×8 取点。
    fn spawn_protector(&self, world: &mut World) -> Option<(i32, Vec3, [u8; 16])> {
        for _ in 0..5 {
            let dx = world.random().next_int(33) - 16;
            let dz = world.random().next_int(33) - 16;
            let mut at = self.pos.offset(dx, 8, dz);
            let mut below = world.block_state(at);
            for _ in 0..=16 {
                let above = at.above();
                let state = world.block_state(above);
                if can_spawn_on(world, above, state, at, below) {
                    at = above.above();
                    break;
                }
                at = at.below();
                below = state;
            }
            // 循环走完仍未 break 视为没有落点；下面再检查一次落脚格。
            if !can_hold_protector(world, at) {
                continue;
            }
            let position = Vec3::new(
                f64::from(at.x) + 0.5,
                f64::from(at.y),
                f64::from(at.z) + 0.5,
            );
            let mut entity = world.prepare_entity(Entity::by_mob_type(MobType::Creaking, position));
            entity.with_mob(|_, mob| mob_creaking::bind(mob, self.pos));
            let uuid = entity.kind.mob().map(|mob| mob.uuid).unwrap_or_default();
            let id = world.add_prepared_entity(entity);
            return Some((id, position, uuid));
        }
        None
    }

    /// `preRemoveSideEffects` / `playerWillDestroy`：心没了，绑定的嘎枝直接拆解。
    pub fn remove_protector(&mut self, world: &mut World) {
        if let Some((id, _, _)) = self.protector(world) {
            self.tear_down_creaking(world, id);
        }
        self.clear_creaking_info();
    }

    fn tear_down_creaking(&self, world: &mut World, id: i32) {
        world.with_entity_mut(id, &mut |entity| {
            entity.with_mob(|_, mob| {
                mob.creaking.tearing_down = true;
                mob.health = 0.0;
                mob.dead = true;
                mob.death_time = 0;
                mob.events
                    .push(crate::entity::mob::MobEvent::EntityEvent { event: 60 });
            });
        });
    }

    /// `creakingHurt`：不扣血，改放树脂团与心的连线效果。
    pub fn creaking_hurt(&mut self, world: &mut World) {
        if self.emitter > 0 {
            return;
        }
        self.emitter = 100;
        self.emitter_target = self.protector(world).map(|(_, position, _)| position);
        if world.block_state(self.pos).get(CREAKING_HEART_STATE) == CreakingHeartState::Awake {
            let count = world.random().next_int_between_inclusive(2, 3);
            for _ in 0..count {
                if let Some(at) = spread_resin(world, self.pos) {
                    world.play_block_sound(
                        at,
                        sounds::sound_event("minecraft:block.resin.place"),
                        1.0,
                        1.0,
                    );
                }
            }
        }
    }

    /// emitter>50 时每刻发两组连线粒子，每 10 刻沿心→嘎枝播一次心跳声。
    fn tick_emitter(&mut self, world: &mut World) {
        if self.emitter % 10 == 0
            && let Some(target) = self.emitter_target
        {
            let fraction = 0.2 + 0.8 * f64::from(100 - self.emitter) / 100.0;
            let center = Vec3::new(
                f64::from(self.pos.x) + 0.5,
                f64::from(self.pos.y) + 0.5,
                f64::from(self.pos.z) + 0.5,
            );
            let at = center.add(target.subtract(center).scale(fraction));
            let pos = BlockPos::containing(at);
            let volume = f64::from(self.emitter) / 200.0 + 0.5;
            world.play_block_sound(
                pos,
                sounds::sound_event("minecraft:block.creaking_heart.hurt"),
                volume as f32,
                1.0,
            );
        }
        self.emitter = self.emitter.saturating_sub(1);
    }

    /// `saveAdditional`：`creaking` 只在有绑定时写。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        match self.creaking {
            Some(uuid) => vec![("creaking".to_owned(), Tag::IntArray(uuid.to_vec()))],
            None => Vec::new(),
        }
    }
}

/// `SpawnUtil.Strategy.ON_TOP_OF_COLLIDER_NO_LEAVES`：上面一格空、脚下是完整顶面且不是树叶。
fn can_spawn_on(
    world: &World,
    _above: BlockPos,
    state: BlockState,
    at: BlockPos,
    below: BlockState,
) -> bool {
    if !state.is_air() {
        return false;
    }
    let support = shape::block_collision_boxes(
        below,
        at,
        CollisionContext::Empty,
        world.moving_piston_shape(at).as_deref(),
        world.shulker_box_shape(at),
    );
    !below.is_tagged(block_tags::LEAVES) && shape::is_face_full(&support, Direction::Up)
}

/// 嘎枝落脚格的 AABB 不能与方块碰撞；简化为落脚格与上一格都是空气 / 无碰撞。
fn can_hold_protector(world: &World, at: BlockPos) -> bool {
    [at, at.above()].into_iter().all(|pos| {
        let state = world.block_state(pos);
        state.is_air()
            || shape::block_collision_boxes(
                state,
                pos,
                CollisionContext::Empty,
                world.moving_piston_shape(pos).as_deref(),
                world.shulker_box_shape(pos),
            )
            .is_empty()
    })
}

/// `spreadResin`：从心沿苍白橡木 BFS 两层，找到能放树脂团的邻格。
fn spread_resin(world: &mut World, heart: BlockPos) -> Option<BlockPos> {
    let mut visited = vec![heart];
    let mut frontier = vec![heart];
    for _ in 0..2 {
        let mut next = Vec::new();
        for pos in frontier {
            for side in [
                Direction::Up,
                Direction::Down,
                Direction::North,
                Direction::South,
                Direction::East,
                Direction::West,
            ] {
                let neighbor = pos.relative(side);
                if visited.contains(&neighbor)
                    || !world.block_state(pos).is_tagged(block_tags::PALE_OAK_LOGS)
                {
                    continue;
                }
                visited.push(neighbor);
                let state = world.block_state(neighbor);
                let clump = BlockState(blocks::RESIN_CLUMP)
                    .default_state()
                    .with(AXIS, side.axis());
                if state.is_air() {
                    world.set_block_and_update(neighbor, clump);
                    return Some(neighbor);
                }
                if state.is(blocks::RESIN_CLUMP) {
                    world.set_block_and_update(neighbor, clump);
                    return Some(neighbor);
                }
                next.push(neighbor);
            }
        }
        frontier = next;
    }
    None
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

    /// `preRemoveSideEffects` → `removeProtector(null)`。
    fn pre_remove_side_effects(&mut self, world: &mut World, _pos: BlockPos, _state: BlockState) {
        self.remove_protector(world);
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

    #[test]
    fn a_heart_spawns_and_saves_one_creaking_at_night() {
        let (mut level, mut ticks) = level(heart(), 12600);
        level.players.push(player_view(Vec3::new(0.5, 64.0, 0.5)));
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block_entity(entity::create_block_entity(POS));
        for _ in 0..21 {
            world.with_block_entity::<CreakingHeartEntity, _>(POS, |be, world| be.tick(world));
        }
        let uuid = world
            .block_entity::<CreakingHeartEntity>(POS)
            .and_then(CreakingHeartEntity::creaking_uuid);
        assert!(uuid.is_some(), "夜里 32 格内有玩家时心应刷出嘎枝");
        assert!(world.has_mob_uuid(&uuid_bytes(uuid.unwrap())));
    }

    fn player_view(position: Vec3) -> crate::entity::ctx::PlayerView {
        crate::entity::ctx::PlayerView {
            uuid: None,
            uid: 1,
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
        }
    }
}
