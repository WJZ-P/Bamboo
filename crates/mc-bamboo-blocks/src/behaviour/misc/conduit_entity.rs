//! 潮涌核心方块实体，对应官方 `ConduitBlockEntity`。
//!
//! 服务端每刻推进 `tickCount`；`gameTime % 40 == 0` 时按官方的 42 格坐标表重扫
//! 周围的 3×3×3 水和 5×5×5 外壳上的 `#conduit_effect_block` 框架，按框架数决定
//! 激活、效果半径和能否攻击。环境音每刻判定；满框架时随机挑一只水里的敌对生物
//! 造成 4 点魔法伤害，目标 UUID 写进 `Target` 存档键。客户端的眼睛开合和旋转
//! 由 `clientTick` 自己算，服务端只同步 `Target`。
//!
//! Conduit block entity, matching vanilla `ConduitBlockEntity`. Every 40 ticks it
//! rescans the 3×3×3 water shell and the 42 frame positions on the 5×5×5 shell;
//! active conduits apply `conduit_power` and, with a full frame, strike a random
//! hostile mob in water. `Target` is the only persisted key; eye/rotation animation
//! is client-side.

use std::any::Any;

use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{registries::mob_effect, sounds::sound_event, tags::block_tags};
use mc_bamboo_nbt::Tag;

use crate::{
    archive::tag,
    block_entity::BlockEntity,
    entity::{Entity, ctx::PlayerView},
    level::Effect,
    world::World,
};

/// 官方 `BLOCK_REFRESH_RATE`：每 40 刻重扫框架。
pub const BLOCK_REFRESH_RATE: i64 = 40;
/// 官方 `EFFECT_DURATION`：给玩家的潮涌能量时长。
pub const EFFECT_DURATION: i32 = 260;
/// 官方 `MIN_ACTIVE_SIZE`：至少 16 块框架方块才激活。
pub const MIN_ACTIVE_SIZE: usize = 16;
/// 官方 `MIN_KILL_SIZE`：42 块框架方块才能攻击。
pub const MIN_KILL_SIZE: usize = 42;
/// 官方 `KILL_RANGE`：攻击距离（方块坐标距离平方小于 8²）。
pub const KILL_RANGE: f64 = 8.0;
/// 官方 `updateAndAttackTarget` 里 `damageSources().magic()` 的伤害量。
const ATTACK_DAMAGE: f32 = 4.0;
/// 官方 `getActiveRotation` 的旋转速度；客户端自己算，这里只留常量备查。
pub const ROTATION_SPEED: f32 = -0.0375;

const ACTIVATE: &str = "minecraft:block.conduit.activate";
const DEACTIVATE: &str = "minecraft:block.conduit.deactivate";
const AMBIENT: &str = "minecraft:block.conduit.ambient";
const AMBIENT_SHORT: &str = "minecraft:block.conduit.ambient.short";
const ATTACK_TARGET: &str = "minecraft:block.conduit.attack.target";

/// 官方 `updateShape` 在 5×5×5 外壳上检查的 42 个偏移，按 `i/j/k` 递增的遍历序。
///
/// The 42 offsets of the 5×5×5 shell checked by vanilla `updateShape`, in scan order.
pub fn frame_offsets() -> Vec<(i32, i32, i32)> {
    let mut out = Vec::with_capacity(MIN_KILL_SIZE);
    for i in -2_i32..=2 {
        for j in -2_i32..=2 {
            for k in -2_i32..=2 {
                let (ax, ay, az): (i32, i32, i32) = (i.abs(), j.abs(), k.abs());
                if ax <= 1 && ay <= 1 && az <= 1 {
                    continue;
                }
                // 官方的三段条件：某个坐标为 0 时，另外两个里必须有绝对值 2 的坐标。
                let along_axis_plane = (i == 0 && (ay == 2 || az == 2))
                    || (j == 0 && (ax == 2 || az == 2))
                    || (k == 0 && (ax == 2 || ay == 2));
                if along_axis_plane {
                    out.push((i, j, k));
                }
            }
        }
    }
    out
}

/// `UUIDUtil.CODEC`：`int[4]`（UUID 的高 64 位两字、低 64 位两字）。
fn uuid_tag(uuid: [u8; 16]) -> Tag {
    let mut words = [0i32; 4];
    for (word, chunk) in words.iter_mut().zip(uuid.as_chunks::<4>().0) {
        *word = i32::from_be_bytes(*chunk);
    }
    Tag::IntArray(words.to_vec())
}

/// `EntityReference.read` 的宽松解码：只收长度正好 4 的 `IntArray`。
fn read_uuid(raw: &Tag) -> Option<[u8; 16]> {
    let Tag::IntArray(words) = tag::field(raw, "Target")? else {
        return None;
    };
    let [a, b, c, d] = words.as_slice() else {
        return None;
    };
    let mut uuid = [0u8; 16];
    for (chunk, word) in uuid.as_chunks_mut::<4>().0.iter_mut().zip([a, b, c, d]) {
        *chunk = word.to_be_bytes();
    }
    Some(uuid)
}

/// 官方 `ConduitBlockEntity` 的服务端状态。
///
/// `effectBlocks` 和 `nextAmbientSoundActivation` 只在服务端用；客户端的形状、
/// 旋转和粒子全部由客户端自己的 `clientTick` 计算，不靠这里逐刻发包。
///
/// Server-side state of vanilla `ConduitBlockEntity`. Clients compute the shape,
/// rotation and particles locally in `clientTick`; no per-tick data is sent.
#[derive(Debug, Clone)]
pub struct ConduitBlockEntity {
    pos: BlockPos,
    pub tick_count: i32,
    active: bool,
    hunting: bool,
    effect_blocks: Vec<BlockPos>,
    /// `destroyTarget`，按 UUID 引用一只活着的生物。
    target: Option<[u8; 16]>,
    next_ambient_sound_activation: i64,
}

impl ConduitBlockEntity {
    /// 新建一个初始态的潮涌核心方块实体。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            tick_count: 0,
            active: false,
            hunting: false,
            effect_blocks: Vec::new(),
            target: None,
            next_ambient_sound_activation: 0,
        }
    }

    /// 官方 `loadAdditional`：只读 `Target`，坏值按 `Optional.orElse(null)` 丢弃。
    pub fn read(pos: BlockPos, raw: &Tag) -> Self {
        let mut entity = Self::new(pos);
        entity.target = read_uuid(raw);
        entity
    }

    /// 官方 `saveAdditional` / `getUpdateTag`（`saveCustomOnly`）：有目标才写 `Target`。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        self.target
            .map(|uuid| vec![("Target".to_owned(), uuid_tag(uuid))])
            .unwrap_or_default()
    }

    /// `isActive()`。
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// `isHunting()`。
    pub const fn is_hunting(&self) -> bool {
        self.hunting
    }

    /// `effectBlocks` 的个数，也就是官方 `List.size()`。
    pub fn effect_block_count(&self) -> usize {
        self.effect_blocks.len()
    }

    /// `effectBlocks` 列表，按官方 `updateShape` 的遍历序。
    pub fn effect_blocks(&self) -> &[BlockPos] {
        &self.effect_blocks
    }

    /// `destroyTarget` 的 UUID。
    pub const fn target(&self) -> Option<[u8; 16]> {
        self.target
    }

    /// 测试 / 夹具直接注入一次目标引用。
    pub fn set_target(&mut self, target: Option<[u8; 16]>) {
        self.target = target;
    }

    /// 官方 `ConduitBlockEntity.serverTick(Level, BlockPos, BlockState, ConduitBlockEntity)`。
    pub fn server_tick(&mut self, world: &mut World) {
        self.tick_count = self.tick_count.wrapping_add(1);
        let game_time = world.game_time();
        if game_time % BLOCK_REFRESH_RATE == 0 {
            let was_active = self.active;
            let active = update_shape(world, self.pos, &mut self.effect_blocks);
            if active != was_active {
                let sound = if active { ACTIVATE } else { DEACTIVATE };
                play_block_sound(world, self.pos, sound);
            }
            self.active = active;
            self.hunting = self.effect_blocks.len() >= MIN_KILL_SIZE;
            if active {
                apply_effects(world, self.pos, &self.effect_blocks);
                update_and_attack_target(
                    world,
                    self.pos,
                    self.effect_blocks.len() >= MIN_KILL_SIZE,
                    &mut self.target,
                );
            }
        }
        if self.active {
            if game_time % 80 == 0 {
                play_block_sound(world, self.pos, AMBIENT);
            }
            if game_time > self.next_ambient_sound_activation {
                let delay = i64::from(world.random().next_int(40));
                self.next_ambient_sound_activation =
                    game_time.saturating_add(60).saturating_add(delay);
                play_block_sound(world, self.pos, AMBIENT_SHORT);
            }
        }
    }

    /// 官方 `getActiveRotation(partialTick)`：客户端按 `activeRotation` 自算，
    /// 这里给只读查询用（服务端不保存旋转量，恒按 0 计）。
    pub const fn active_rotation(&self, partial_tick: f32) -> f32 {
        partial_tick * ROTATION_SPEED
    }
}

impl BlockEntity for ConduitBlockEntity {
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

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        Ok(self.additional())
    }
}

/// 官方 `updateShape`：先查 3×3×3 的水，再扫 42 格框架，返回 `size >= 16`。
///
/// 水检查失败时官方立即返回，此时 `effectBlocks` 已经被清空。
fn update_shape(world: &World, pos: BlockPos, out: &mut Vec<BlockPos>) -> bool {
    out.clear();
    for i in -1..=1 {
        for j in -1..=1 {
            for k in -1..=1 {
                if !world.is_water_at(pos.offset(i, j, k)) {
                    return false;
                }
            }
        }
    }
    for (i, j, k) in frame_offsets() {
        let at = pos.offset(i, j, k);
        if world
            .block_state(at)
            .is_tagged(block_tags::CONDUIT_EFFECT_BLOCK)
        {
            out.push(at);
        }
    }
    out.len() >= MIN_ACTIVE_SIZE
}

/// 官方 `applyEffects`：半径 `size / 7 * 16`，范围内且在水里 / 淋雨的玩家拿潮涌能量。
fn apply_effects(world: &mut World, pos: BlockPos, blocks: &[BlockPos]) {
    let radius = (blocks.len() / 7 * 16) as f64;
    let world_height = f64::from(world.max_y() - world.min_y() + 1);
    // 官方 `AABB(pos..pos+1).inflate(radius).expandTowards(0, getHeight(), 0)`。
    let area = Aabb::new(
        f64::from(pos.x) - radius,
        f64::from(pos.y) - radius,
        f64::from(pos.z) - radius,
        f64::from(pos.x + 1) + radius,
        f64::from(pos.y + 1) + radius + world_height,
        f64::from(pos.z + 1) + radius,
    );
    let targets: Vec<i32> = world
        .player_views()
        .into_iter()
        .filter(|player| {
            player_bounds(player).intersects(area)
                && closer_than(pos, BlockPos::containing(player.position), radius)
                && world.player_in_water_or_rain(player)
        })
        .map(|player| player.uid)
        .collect();
    for target in targets {
        world.push_effect(Effect::PlayerEffect {
            target,
            effect: mob_effect::CONDUIT_POWER,
            duration: EFFECT_DURATION,
            amplifier: 0,
            ambient: true,
            visible: true,
            show_icon: true,
        });
    }
}

/// 官方 `updateAndAttackTarget` + `updateDestroyTarget` + `selectNewTarget`。
fn update_and_attack_target(
    world: &mut World,
    pos: BlockPos,
    can_hunt: bool,
    target: &mut Option<[u8; 16]>,
) {
    let next = if !can_hunt {
        None
    } else if let Some(uuid) = *target {
        world
            .live_entities()
            .iter()
            .find(|entity| mob_uuid(entity) == Some(uuid))
            .filter(|entity| {
                mob_alive(entity) && closer_than(pos, entity.block_position(), KILL_RANGE)
            })
            .map(|entity| mob_uuid(entity).unwrap_or(uuid))
    } else {
        let area = Aabb::new(
            f64::from(pos.x) - KILL_RANGE,
            f64::from(pos.y) - KILL_RANGE,
            f64::from(pos.z) - KILL_RANGE,
            f64::from(pos.x) + KILL_RANGE,
            f64::from(pos.y) + KILL_RANGE,
            f64::from(pos.z) + KILL_RANGE,
        );
        let candidates: Vec<(i32, [u8; 16], Vec3)> = world
            .live_entities()
            .iter()
            .filter_map(|entity| {
                let uuid = mob_uuid(entity)?;
                let mob = entity.kind.mob()?;
                if !mob.mob_type.is_enemy()
                    || !in_water_or_rain(world, entity)
                    || !entity.bounding_box().intersects(area)
                {
                    return None;
                }
                Some((entity.id, uuid, entity.position))
            })
            .collect();
        if candidates.is_empty() {
            None
        } else {
            // `Util.getRandom(list, level.random)` = `list.get(random.nextInt(list.size()))`。
            let index = world.random().next_int(candidates.len() as i32) as usize;
            candidates.get(index).map(|(_, uuid, _)| *uuid)
        }
    };

    if let Some(uuid) = next
        && let Some((id, position)) = world
            .live_entities()
            .iter()
            .find(|entity| mob_uuid(entity) == Some(uuid))
            .map(|entity| (entity.id, entity.position))
    {
        // `ServerLevel.playSound(null, target.getX(), getY(), getZ(), ...)`。
        world.play_sound(
            position,
            sound_event(ATTACK_TARGET),
            mc_bamboo_data::sounds::BLOCKS,
            1.0,
            1.0,
            false,
        );
        world.hurt_mob(
            id,
            crate::entity::mob::DamageSource::environment(crate::entity::mob::DamageKind::Magic),
            ATTACK_DAMAGE,
        );
    }

    if next != *target {
        *target = next;
        // `sendBlockUpdated(pos, state, state, 2)`：方块状态没变，但要把
        // `getUpdateTag`（`Target`）重新发给追踪这个区块的客户端。
        world.push_effect(Effect::ConduitChanged { pos });
    }
}

fn mob_uuid(entity: &Entity) -> Option<[u8; 16]> {
    entity.kind.mob().map(|mob| mob.uuid)
}

fn mob_alive(entity: &Entity) -> bool {
    entity
        .kind
        .mob()
        .is_some_and(|mob| !entity.removed && mob.health > 0.0)
}

/// `Entity.isInWaterOrRain()`：身体碰水，或脚下 / 碰撞盒顶上那格在下雨。
fn in_water_or_rain(world: &World, entity: &Entity) -> bool {
    if entity.in_water {
        return true;
    }
    let feet = entity.block_position();
    let top = BlockPos::containing(Vec3::new(
        entity.position.x,
        entity.bounding_box().max_y,
        entity.position.z,
    ));
    world.is_raining_at(feet) || world.is_raining_at(top)
}

/// `EntityDimensions.makeBoundingBox(position)`：玩家的碰撞盒。
fn player_bounds(player: &PlayerView) -> Aabb {
    let half = f64::from(player.width) / 2.0;
    Aabb::new(
        player.position.x - half,
        player.position.y,
        player.position.z - half,
        player.position.x + half,
        player.position.y + f64::from(player.height),
        player.position.z + half,
    )
}

/// `Vec3i.closerThan(Vec3i, double)`：整格坐标的距离平方严格小于半径平方。
fn closer_than(from: BlockPos, to: BlockPos, radius: f64) -> bool {
    let dx = f64::from(from.x - to.x);
    let dy = f64::from(from.y - to.y);
    let dz = f64::from(from.z - to.z);
    dx * dx + dy * dy + dz * dz < radius * radius
}

/// `playSound(null, pos, sound, BLOCKS, 1, 1)`，坐标是方块中心。
fn play_block_sound(world: &mut World, pos: BlockPos, name: &str) {
    world.play_block_sound(pos, sound_event(name), 1.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockState, BlockTicks, testing::TestLevel};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn water(level: &mut TestLevel, pos: BlockPos) {
        for i in -2..=2 {
            for j in -2..=2 {
                for k in -2..=2 {
                    level.set(pos.offset(i, j, k), BlockState::WATER);
                }
            }
        }
        level.set(pos, BlockState(mc_bamboo_data::blocks::CONDUIT));
    }

    #[test]
    fn frame_offsets_are_the_official_42_cells() {
        let offsets = frame_offsets();
        assert_eq!(offsets.len(), 42);
        assert!(offsets.contains(&(2, 2, 0)));
        assert!(offsets.contains(&(0, -2, 2)));
        assert!(!offsets.contains(&(2, 2, 2)), "角上的格子不在坐标平面环里");
        assert!(!offsets.contains(&(1, 0, 0)), "3×3×3 内壳不算框架");
    }

    #[test]
    fn archive_round_trip_writes_target_only() {
        let uuid = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let mut entity = ConduitBlockEntity::new(POS);
        entity.set_target(Some(uuid));
        let fields = entity.additional();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "Target");
        let raw = Tag::Compound(fields);
        assert_eq!(ConduitBlockEntity::read(POS, &raw).target(), Some(uuid));
        assert!(
            ConduitBlockEntity::read(POS, &Tag::Compound(vec![]))
                .target()
                .is_none()
        );
        // 坏数据不 panic，也不带入目标。
        let bad = Tag::Compound(vec![("Target".to_owned(), Tag::IntArray(vec![1, 2, 3]))]);
        assert!(ConduitBlockEntity::read(POS, &bad).target().is_none());
    }

    #[test]
    fn water_gate_clears_the_frame_and_deactivates() {
        let mut level = TestLevel::new();
        water(&mut level, POS);
        for (i, j, k) in frame_offsets().into_iter().take(16) {
            level.set(
                POS.offset(i, j, k),
                BlockState(mc_bamboo_data::blocks::PRISMARINE),
            );
        }
        let mut ticks = BlockTicks::new();
        let mut entity = ConduitBlockEntity::new(POS);
        {
            let mut world = World::new(&mut level, &mut ticks);
            entity.server_tick(&mut world);
        }
        assert!(entity.is_active());
        assert_eq!(entity.effect_block_count(), 16);
        // 拿掉一格 3×3×3 的水：下一次重扫立刻清空框架并失效。
        level.set(POS.offset(1, 1, 1), BlockState::AIR);
        ticks.set_time(40);
        let effects = {
            let mut world = World::new(&mut level, &mut ticks);
            entity.server_tick(&mut world);
            world.drain_effects()
        };
        assert!(!entity.is_active());
        assert_eq!(entity.effect_block_count(), 0);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event(DEACTIVATE))
        )));
    }

    #[test]
    fn full_frame_hunts_and_plays_the_attack_sound() {
        let mut level = TestLevel::new();
        water(&mut level, POS);
        for (i, j, k) in frame_offsets() {
            level.set(
                POS.offset(i, j, k),
                BlockState(mc_bamboo_data::blocks::SEA_LANTERN),
            );
        }
        let mut ticks = BlockTicks::new();
        let mut entity = ConduitBlockEntity::new(POS);
        let mut world = World::new(&mut level, &mut ticks);
        entity.server_tick(&mut world);
        assert!(entity.is_active());
        assert!(entity.is_hunting());
        let effects = world.drain_effects();
        // 第 0 刻 `gameTime == nextAmbientSoundActivation`，官方的 `>` 不放短音。
        assert!(!effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event(AMBIENT_SHORT))
        )));
        ticks.set_time(1);
        let mut world = World::new(&mut level, &mut ticks);
        entity.server_tick(&mut world);
        let short_effects = world.drain_effects();
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event(ACTIVATE))
        )));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event(AMBIENT))
        )));
        assert!(short_effects.iter().any(|e| matches!(
            e,
            Effect::Sound { sound, .. } if *sound == i32::from(sound_event(AMBIENT_SHORT))
        )));
    }
}
