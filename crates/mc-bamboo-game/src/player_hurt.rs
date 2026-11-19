//! 玩家受伤的落地：状态、出站包、击退、每刻倒计时、饥饿与回血。
//!
//! 官方 `Player.hurtServer` → `LivingEntity.hurtServer` → `Player.actuallyHurt`
//! 的一次调用。伤害计算是 [`mc_bamboo_blocks::entity::mob::damage`] 里的纯函数，
//! 这里只负责取全局难度、把改动写进 `Player`、发包和推人。
//! 血量归零时接 [`crate::player_death::die`]。
//!
//! Applies one player hurt: state, packets, knockback and the per-tick
//! food / regeneration countdowns. Health reaching zero runs the death flow.

use mc_bamboo_blocks::entity::{JavaRandom, mob::DamageSource};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::sounds;
use mc_bamboo_protocol::{
    frame::packet_body,
    packet::play::{DamageEvent, HurtAnimation, PlayerCombatEnter, SetHealth, Sound},
};

use crate::{inbound::PlayerId, player::Player, region::Region};

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

/// `ServerPlayer.tick` / `doTick`：实体 tick 数递增、和平回血、三个倒计时的推进、
/// `FoodData.tick` 的回血 / 饥饿伤害，最后按需补 `set_health`。
/// 官方顺序是 `Player.aiStep`（tickRegeneration）→ `Player.tick` →
/// `ServerPlayer.doTick`（FoodData.tick）→ `set_health`；这些都在同一刻的
/// 玩家阶段里，包只在最后按快照发一次。
///
/// Per-tick player health: entity ticks, peaceful regeneration, hurt
/// countdowns, `FoodData.tick`, then one `set_health` if the snapshot moved.
pub(crate) fn tick(region: &mut Region) {
    let difficulty = region.global.difficulty as i32;
    let natural_regen = region
        .global
        .rules
        .bool(mc_bamboo_data::gamerules::NATURAL_HEALTH_REGENERATION);
    let mut pending = Vec::new();
    let mut starve = Vec::new();
    for (id, player) in &mut region.players {
        player.entity_ticks = player.entity_ticks.wrapping_add(1);
        // 官方 `ServerPlayer.tick` 在死亡后仍递减 damageCooldownTime，
        // `LivingEntity.baseTick` 也递减 hurtTime；只有 `isAlive()` 为真才
        // 跑 `tickRegeneration` / `FoodData.tick`。
        player.health.tick_cooldowns();
        // 等出生点区块就绪时不跑玩家 tick、回血和饥饿。
        // No player tick, regeneration or food while spawn chunks are pending.
        if player.dead || player.spawn_pending.is_some() {
            continue;
        }
        if difficulty == 0 && natural_regen {
            let _ = player.health.tick_peaceful(player.entity_ticks);
        }
        if player.health.death_time > 0 && player.health.death_time < 20 {
            // `LivingEntity.baseTick` 每刻推一次死亡动画，20 刻后才 discard。
            player.health.death_time += 1;
        }
        let food_tick = player.health.tick_food(difficulty, natural_regen);
        match food_tick {
            crate::player_health::FoodTick::Heal(amount) => player.health.heal(amount),
            crate::player_health::FoodTick::Starve => starve.push(*id),
            crate::player_health::FoodTick::Idle => {}
        }
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
    // 饥饿伤害是一次真正的 `hurtServer(starve, 1.0)`，走完整受伤链路。
    // `LivingEntity.baseTick` 的击杀归属过期 + `CombatTracker.recheckStatus`。
    crate::player_death::tick_combat(region);
    for id in starve {
        hurt_player(
            region,
            id,
            mc_bamboo_blocks::entity::mob::DamageSource::environment(
                mc_bamboo_blocks::entity::mob::DamageKind::Starve,
            )
            .with_non_player_cause(true),
            1.0,
            &mut None,
        );
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
    // `LivingEntity.hurtServer`：`#is_fire` 的伤害被抗火效果完全挡下。
    // `LivingEntity.hurtServer`: fire resistance blocks all `#is_fire` damage.
    if source.kind.is_fire()
        && player
            .effects
            .has(mc_bamboo_data::registries::mob_effect::FIRE_RESISTANCE)
    {
        return false;
    }
    let position = player.position();
    let armor = {
        let empty = mc_bamboo_protocol::item::ItemStack::empty();
        let equipment = player.equipment_stacks(&empty);
        let mut armor = crate::player_health::PlayerHealth::armor_values(&equipment);
        armor.resistance = player
            .effects
            .amplifier(mc_bamboo_data::registries::mob_effect::RESISTANCE);
        armor
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
        // 官方 `Player.actuallyHurt` 在吸收之后 `amount == 0` 就提前 return：
        // 没有 `CombatTracker.recordDamage`，也没有饥饿消耗。
        // 进入战斗包只在那条路径里发；伤害事件、音效和击退由外层骨架发。
        if outcome.damage > 0.0 {
            send_player_combat_enter(region, id, &source);
        }
        broadcast_damage_event(region, id, &source);
        // 官方 `dealDefaultKnockback` 里 `indicateDamage` 先发 hurt_animation，
        // 之后才播死亡 / 受伤音效。
        if outcome.knockback {
            let player = region.players.get_mut(&id).expect("玩家刚才还在");
            knockback(player, &source, position, random);
            region.links.send(
                id,
                &HurtAnimation {
                    entity_id: id.0,
                    yaw: hurt_yaw,
                },
            );
        }
        // `LivingEntity.hurtServer` 在 `isDeadOrDying()` 时播 `getDeathSound()`
        // （玩家的 `entity.player.death`），否则播受伤音效。
        let dying = region
            .players
            .get(&id)
            .is_some_and(|player| player.health.health <= 0.0);
        if dying {
            play_sound(region, id, "minecraft:entity.player.death", position);
        } else {
            play_hurt_sound(region, id, &source, position);
        }
    }
    // 官方 `Player.actuallyHurt` 对完整受击和冷却期内的部分受击都写
    // `CombatTracker.recordDamage`、设置击杀归属，只要真正扣了血。
    if outcome.damage > 0.0 {
        crate::player_death::record_combat(region, id, &source, outcome.damage);
    }
    let dying = region
        .players
        .get(&id)
        .is_some_and(|player| player.health.health <= 0.0 && !player.dead);
    // 官方 `ServerPlayer.doTick` 在 `die` 之后才补 `set_health`；
    // 死亡流程里的 player_combat_kill / system_chat / entity_event 必须排前面。
    let send_health = |region: &mut Region| {
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
    };
    // `LivingEntity.hurtServer` 在 `isDeadOrDying()` 时调 `die`：
    // 掉落、死亡消息、死亡界面和 `immediate_respawn` 都在这里接上。
    if dying {
        crate::player_death::die(region, id, source);
    }
    send_health(region);
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
        // 官方 `ClientboundDamageEventPacket` 读 `sourcePositionRaw()`：
        // 只有 `at <location>` 显式给的位置才发，攻击者位置不写进包里。
        position: source.raw_position,
    }));
    send_to_tracking_and_self(region, id, &packet);
}

/// `CombatTracker.recordDamage`：造成者是非玩家生物时，`ServerPlayer.onEnterCombat`
/// 给玩家自己发一个没有包体的 `player_combat_enter`。
fn send_player_combat_enter(region: &mut Region, id: PlayerId, source: &DamageSource) {
    if source.attacker.is_some() && region.players.get(&id).is_some_and(|p| !p.in_combat) {
        if let Some(player) = region.players.get_mut(&id) {
            player.in_combat = true;
        }
        region.links.send(id, &PlayerCombatEnter);
    }
}

/// `LivingEntity.playHurtSound`：官方 `Level.playSound` 取一次世界随机源当种子，
/// 广播给范围内除自己以外的玩家。音效由 `DamageType.effects` 决定。
fn play_hurt_sound(region: &mut Region, id: PlayerId, source: &DamageSource, position: Vec3) {
    let name = hurt_sound_name(source);
    play_sound(region, id, name, position);
    // `LivingEntity.playSecondaryHurtSound`：`thorns` 类型再多一声。
    if source.kind == mc_bamboo_blocks::entity::mob::DamageKind::Thorns {
        let seed = region.sound_seed_generator.source().next_long();
        let secondary = Sound {
            sound: i32::from(sounds::sound_event("minecraft:enchant.thorns.hit")),
            inline: None,
            source: sounds::PLAYERS,
            x: position.x,
            y: position.y,
            z: position.z,
            volume: 1.0,
            pitch: 1.0,
            seed,
        };
        crate::interaction::send_sound(region, Some(id), &secondary);
    }
}

fn hurt_sound_name(source: &DamageSource) -> &'static str {
    match source.kind {
        mc_bamboo_blocks::entity::mob::DamageKind::Drown => "minecraft:entity.player.hurt_drown",
        mc_bamboo_blocks::entity::mob::DamageKind::Freeze => "minecraft:entity.player.hurt_freeze",
        mc_bamboo_blocks::entity::mob::DamageKind::OnFire
        | mc_bamboo_blocks::entity::mob::DamageKind::InFire
        | mc_bamboo_blocks::entity::mob::DamageKind::Lava
        | mc_bamboo_blocks::entity::mob::DamageKind::HotFloor => {
            "minecraft:entity.player.hurt_on_fire"
        }
        mc_bamboo_blocks::entity::mob::DamageKind::SweetBerryBush => {
            "minecraft:entity.player.hurt_sweet_berry_bush"
        }
        _ => "minecraft:entity.player.hurt",
    }
}

/// `Level.playSeededSound` 的玩家音效：种子取一次世界随机源，广播给范围内除自己以外的玩家。
fn play_sound(region: &mut Region, id: PlayerId, name: &str, position: Vec3) {
    let seed = region.sound_seed_generator.source().next_long();
    let sound = Sound {
        sound: i32::from(sounds::sound_event(name)),
        inline: None,
        source: sounds::PLAYERS,
        x: position.x,
        y: position.y,
        z: position.z,
        volume: 1.0,
        pitch: 1.0,
        seed,
    };
    crate::interaction::send_sound(region, Some(id), &sound);
}

/// 把已经编码好的「ID + 包体」加上帧长头。
fn frame_from_body(body: &[u8]) -> Vec<u8> {
    let mut frames = Vec::with_capacity(body.len() + 3);
    let header = mc_bamboo_protocol::codec::encode_varint(body.len() as i32);
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
    // 官方 `dealDefaultKnockback` 传的是 float 0.4f，转成 double 后是 0.4000000059604645。
    let strength = f64::from(0.4_f32);
    let length = (dx * dx + dz * dz).sqrt();
    let push_x = dx / length * strength;
    let push_z = dz / length * strength;
    let delta = player.motion.delta;
    // `Math.min(0.4, dy/2 + strength)` 只在着地时用。
    let dy = if player.on_ground {
        f64::from(0.4_f32).min(delta.y / 2.0 + strength)
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
        // 官方 `Player.attack` 打中之后 `causeFoodExhaustion(0.1f)`，
        // 挥手包由 `attackVisualEffects` 发给观察者。
        if let Some(player) = region.players.get_mut(&id) {
            let food = &mut player.health.food;
            food.exhaustion =
                (food.exhaustion + 0.1).min(mc_bamboo_blocks::entity::mob::damage::MAX_EXHAUSTION);
            player.health_sync = HealthSync::of(player);
        }
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
        assert_eq!(player.motion.delta.x, -f64::from(0.4_f32));
        assert_eq!(player.motion.delta.z, 0.0);
    }
}
