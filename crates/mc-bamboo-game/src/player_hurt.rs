//! 玩家受伤的落地：状态、出站包、击退、每刻倒计时。
//!
//! 官方 `Player.hurtServer` → `LivingEntity.hurtServer` → `Player.actuallyHurt`
//! 的一次调用。伤害计算是 [`mc_bamboo_blocks::entity::mob::damage`] 里的纯函数，
//! 这里只负责取全局难度、把改动写进 `Player`、发包和推人。
//!
//! 血量归零不触发死亡：掉落、死亡消息和重生留到 1b。
//!
//! Applies one player hurt: state, outbound packets, knockback and per-tick
//! countdowns. Health reaching zero does not trigger death yet (1b).

use mc_bamboo_blocks::entity::{JavaRandom, mob::DamageSource};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_protocol::{
    frame::packet_body,
    packet::play::{DamageEvent, EntityEvent, HurtAnimation, SetHealth},
};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// `LivingEntity.ENTITY_EVENT_HURT`：官方 `hurtServer` 之后由
/// `markHurt` 触发，发给正在跟踪这名玩家的人。
const ENTITY_EVENT_HURT: u8 = 2;

/// 官方 `ServerPlayer.doTick` 里 `set_health` 的上次值和三个判据。
/// 血量、饥饿或饱和度是否为 0 变了，才补发一包。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HealthSync {
    pub health: f32,
    pub food: i32,
    pub saturation_zero: bool,
}

impl HealthSync {
    pub fn of(player: &Player) -> Self {
        Self {
            health: player.health.health,
            food: player.health.food.food_level,
            saturation_zero: player.health.food.saturation == 0.0,
        }
    }

    pub const INITIAL: Self = Self {
        health: -1.0e8,
        food: -99_999_999,
        saturation_zero: true,
    };
}

/// `ServerPlayer.doTick`：三个倒计时的推进，然后按需补 `set_health`。
pub(crate) fn tick(region: &mut Region) {
    let mut pending = Vec::new();
    for (id, player) in &mut region.players {
        player.health.tick_cooldowns();
        let now = HealthSync::of(player);
        if now != player.health_sync {
            player.health_sync = now;
            pending.push((
                *id,
                SetHealth {
                    health: now.health,
                    food: now.food,
                    saturation: player.health.food.saturation,
                },
            ));
        }
    }
    for (id, packet) in pending {
        region.links.send(id, &packet);
    }
}

/// 打一名玩家。返回是否真的受伤（官方 `hurtServer` 的布尔）。
///
/// `source.position` 是 `getSourcePosition()`：击退方向和受伤动画的偏角都用它。
/// `random` 是这名玩家的实体随机源，击退方向退化成 0 时官方从它取数。
pub(crate) fn hurt_player(
    region: &mut Region,
    id: PlayerId,
    source: DamageSource,
    amount: f32,
    random: &mut Option<JavaRandom>,
) -> bool {
    let difficulty = region.global.difficulty as i32;
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    // 创造 / 旁观模式的能力无敌（官方 `Player.hurtServer` 的第二道判据）。
    // 这一期把创造模式按 `Abilities.invulnerable` 处理；旁观者直接不受伤。
    if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Spectator {
        return false;
    }
    if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative
        && !source.kind.bypasses_invulnerability()
    {
        return false;
    }
    let position = player.position();
    let armor = {
        let empty = mc_bamboo_protocol::item::ItemStack::empty();
        let equipment = player.equipment_stacks(&empty);
        crate::player_health::PlayerHealth::armor_values(&equipment)
    };
    let (outcome, hurt_yaw, sync, saturation) = {
        let outcome = player
            .health
            .hurt(amount, source, difficulty, armor, position);
        let Some(outcome) = outcome else {
            return false;
        };
        // `set_entity_data` 的 `Health` 项由下一刻的派生状态阶段发出。
        player.sync.metadata_dirty = true;
        (
            outcome,
            hurt_yaw(player),
            HealthSync::of(player),
            player.health.food.saturation,
        )
    };
    if outcome.full_hit {
        broadcast_damage_event(region, id, &source);
    }
    if outcome.full_hit && !source.kind.no_impact() {
        broadcast_hurt_event(region, id);
    }
    if outcome.knockback {
        let player = region.players.get_mut(&id).expect("玩家刚才还在");
        knockback(player, &source, position, random);
        broadcast_hurt_animation(region, id, hurt_yaw);
    }
    let player = region.players.get_mut(&id).expect("玩家刚才还在");
    if sync != player.health_sync {
        player.health_sync = sync;
        region.links.send(
            id,
            &SetHealth {
                health: sync.health,
                food: sync.food,
                saturation,
            },
        );
    }
    true
}

/// `ServerLevel.broadcastDamageEvent`：`damage_event` 发给跟踪者（含自己）。
fn broadcast_damage_event(region: &mut Region, id: PlayerId, source: &DamageSource) {
    let damage_type = mc_bamboo_data::damage_type_id(source.kind.name()).unwrap_or(0);
    let packet = frame_from_body(&packet_body(&DamageEvent {
        entity_id: id.0,
        damage_type,
        source_cause: source.attacker,
        source_direct: source.direct.or(source.attacker),
        position: source.position,
    }));
    send_to_tracking_and_self(region, id, &packet);
}

/// `LivingEntity.markHurt`：受伤实体事件发给跟踪者（含自己）。
fn broadcast_hurt_event(region: &mut Region, id: PlayerId) {
    let event = frame_from_body(&packet_body(&EntityEvent {
        entity_id: id.0,
        event: ENTITY_EVENT_HURT,
    }));
    send_to_tracking_and_self(region, id, &event);
}

/// `ServerPlayer.indicateDamage`：`hurt_animation` 发给跟踪者（含自己）。
fn broadcast_hurt_animation(region: &mut Region, id: PlayerId, hurt_yaw: f32) {
    let animation = frame_from_body(&packet_body(&HurtAnimation {
        entity_id: id.0,
        yaw: hurt_yaw,
    }));
    send_to_tracking_and_self(region, id, &animation);
}

/// 把已经编码好的「ID + 包体」加上帧长头。
fn frame_from_body(body: &[u8]) -> Vec<u8> {
    let header = mc_bamboo_protocol::codec::encode_varint(body.len() as i32);
    let mut frames = Vec::with_capacity(body.len() + 3);
    frames.extend_from_slice(&header.0[..header.1]);
    frames.extend_from_slice(body);
    frames
}

/// 官方 `ChunkMap.sendToTrackingPlayersAndSelf`：观察者是 `seen_by` 加自己。
fn send_to_tracking_and_self(region: &mut Region, id: PlayerId, packet: &[u8]) {
    let viewers: Vec<PlayerId> = region
        .players
        .get(&id)
        .map(|player| player.sync.seen_by().iter().copied().collect())
        .unwrap_or_default();
    for viewer in viewers {
        region.links.send_frames(viewer, packet);
    }
    region.links.send_frames(id, packet);
}

/// 把官方 `getHurtDir()` 的偏角写进 `HurtAnimation` 包。
/// 官方在 `ServerPlayer.indicateDamage` 里把 `atan2(dz, dx)` 的角度减去 `yRot`，
/// `Player.hurt` 已经存好这个值。
pub(crate) fn hurt_yaw(player: &Player) -> f32 {
    player.health.hurt_dir - player.yaw
}

/// `LivingEntity.knockback`：0.4 强度，方向是来源位置减玩家位置。
/// 方向退化成 0 时从玩家随机源取两对数，和官方一致。
fn knockback(
    player: &mut Player,
    source: &DamageSource,
    position: Vec3,
    random: &mut Option<JavaRandom>,
) {
    let (mut dx, mut dz) = source
        .position
        .map_or((0.0, 0.0), |at| (at.x - position.x, at.z - position.z));
    if let Some(random) = random.as_mut()
        && dx * dx + dz * dz < 1.0e-5
    {
        dx = (random.next_double() - random.next_double()) * 0.01;
        dz = (random.next_double() - random.next_double()) * 0.01;
    }
    if dx * dx + dz * dz < 1.0e-5 {
        return;
    }
    let strength = 0.4_f64;
    let length = (dx * dx + dz * dz).sqrt();
    let push_x = dx / length * strength;
    let push_z = dz / length * strength;
    let delta = player.motion.delta;
    // `Math.min(0.4, dy/2 + strength)` 只在着地时用。
    let dy = if player.on_ground {
        (0.4_f64).min(delta.y / 2.0 + strength)
    } else {
        delta.y
    };
    player.motion.delta = Vec3::new(delta.x / 2.0 - push_x, dy, delta.z / 2.0 - push_z);
}

/// 官方 `ServerGamePacketListenerImpl.handleAttack` 的最小版本：
/// 旁观者、够不到、目标不是生物时忽略；打玩家按官方 `Player.canHarmPlayer`
/// 的 PVP 缺失处理，明确忽略并记一条日志（1b 之后再做完整 PVP）。
pub(crate) fn on_attack(region: &mut Region, id: PlayerId, entity_id: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Spectator {
        return;
    }
    let Some(target) = region.entities.public_at(entity_id) else {
        return;
    };
    if target.removed || !matches!(target.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_)) {
        return;
    }
    // 够距：和右键交互同一套判定，官方 `isWithinAttackRange` 的简化版。
    let position = player.position();
    let eye_height = player.eye_height();
    let reach = crate::interaction::interaction_range(player.game_mode);
    if !crate::interaction::within_block_interaction_range(
        position,
        eye_height,
        reach,
        mc_bamboo_core::BlockPos::containing(target.position),
        3.0,
    ) {
        return;
    }
    let held = player.inventory.held(Hand::Main).clone();
    let amount = crate::player_health::PlayerHealth::attack_damage(&held);
    let mut source = DamageSource::player(id.0, position, false);
    if !held.is_empty()
        && let Ok(weapon) = u16::try_from(held.item)
    {
        source = source.with_weapon(weapon);
    }
    let ok = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        let ok = world.hurt_mob(entity_id, source, amount);
        let effects = world.into_effects();
        crate::interaction::send_effects(region, Some(id), &effects);
        ok
    };
    if ok {
        // 挥手包：官方打中实体时给观察者发主手挥手。
        crate::entity_sync::broadcast_swing(region, id, Hand::Main);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::entity::mob::DamageKind;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    fn player() -> Player {
        Player::new(
            PlayerId(7),
            uuid::Uuid::nil(),
            "Kitten".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::default(),
        )
    }

    #[test]
    fn initial_health_sync_differs_from_the_first_snapshot() {
        let player = player();
        assert_ne!(HealthSync::INITIAL, HealthSync::of(&player));
    }

    #[test]
    fn knockback_uses_the_attacker_position() {
        let mut player = player();
        player.motion.delta = Vec3::new(0.0, 0.0, 0.0);
        let source = DamageSource::environment(DamageKind::MobAttack)
            .with_position(Vec3::new(2.0, 0.0, 0.0));
        knockback(&mut player, &source, Vec3::default(), &mut None);
        assert_eq!(player.motion.delta.x, -0.4);
        assert_eq!(player.motion.delta.z, 0.0);
    }
}
