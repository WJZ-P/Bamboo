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
use mc_bamboo_core::Vec3;
use mc_bamboo_data::sounds;
use mc_bamboo_protocol::{
    frame::packet_body,
    packet::play::{
        DamageEvent, HurtAnimation, PlayerCombatEnter, SetEntityMotion, SetHealth, Sound,
    },
};

use std::collections::BTreeMap;

use mc_bamboo_blocks::{LevelRandom, PlayerDeathDrops, entity::Entities};

use crate::{
    gamerules::GameRuleSet, inbound::PlayerId, links::Links, player::Player, region::Region,
};

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
        if player.spawn_pending.is_some() {
            continue;
        }
        if !player.dead {
            let max_health = crate::player_effects::max_health(player);
            if difficulty == 0 && natural_regen {
                let _ = player.health.tick_peaceful(player.entity_ticks, max_health);
            }
            if player.health.death_time > 0 && player.health.death_time < 20 {
                // `LivingEntity.baseTick` 每刻推一次死亡动画，20 刻后才 discard。
                player.health.death_time += 1;
            }
            let food_tick = player
                .health
                .tick_food(difficulty, natural_regen, max_health);
            match food_tick {
                crate::player_health::FoodTick::Heal(amount) => {
                    player.health.heal(amount, max_health)
                }
                crate::player_health::FoodTick::Starve => starve.push(*id),
                crate::player_health::FoodTick::Idle => {}
            }
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

/// 受伤链路要用到的那一部分区域状态：玩家表、连接、音效种子、实体表（读攻击者名字）
/// 和规则。精确驱动的实体循环里整个 `Region` 已经拆给了方块世界，生物打玩家时
/// 从 `RegionLevel` 拼出同一组借用，和区域入口走同一份代码。
///
/// The slice of region state the hurt path needs, so a mob's hit can run it from
/// inside the entity loop where the region is already split up.
pub(crate) struct HurtParts<'a> {
    pub players: &'a mut BTreeMap<PlayerId, Player>,
    pub links: &'a mut Links,
    pub sound_seeds: &'a LevelRandom,
    pub entities: &'a mut Entities,
    pub difficulty: i32,
    pub rules: &'a GameRuleSet,
    /// 正在 tick、不在实体表里的攻击者（精确驱动里打人的生物）。
    pub attacker: Option<&'a mc_bamboo_blocks::entity::EntityPublic>,
    /// 活着的玩家这次不当场补 `set_health`，留给同一刻随后的玩家阶段
    /// （官方 `ServerPlayer.doTick`）。生物打人时为真：效果包（例如尸壳给的饥饿）
    /// 必须排在 `set_health` 前面。
    pub defer_health: bool,
    /// 借用被拆开时把这一刻的触发器事件攒到 `Region::pending_triggers`。
    /// 精确驱动的实体循环没有这个借用，事件在落地阶段按效果补发。
    ///
    /// When the region is split, this borrows the queued advancement events.
    pub pending_triggers: Option<&'a mut Vec<crate::advancement_triggers::PendingEvent>>,
}

impl Region {
    /// 拆出受伤链路用的借用。
    pub(crate) fn hurt_parts(&mut self) -> HurtParts<'_> {
        HurtParts {
            players: &mut self.players,
            links: &mut self.links,
            sound_seeds: &self.sound_seed_generator,
            entities: &mut self.entities,
            difficulty: self.global.difficulty as i32,
            rules: &self.global.rules,
            attacker: None,
            defer_health: false,
            pending_triggers: Some(&mut self.pending_triggers),
        }
    }
}

/// 打一名玩家。返回是否真的受伤（官方 `hurtServer` 的布尔）。
///
/// `source.position` 是 `getSourcePosition()`：击退方向和受伤动画的偏角都用它。
/// `random` 是这名玩家的实体随机源，击退方向退化成 0 时官方从它取数。
/// 打死时的掉落在死亡流程的包发完之后生成：生成本身不发包，只取世界随机数，
/// 死亡流程里没有别的世界随机数，所以和官方「先掉落再发后面的包」等价。
pub(crate) fn hurt_player(
    region: &mut Region,
    id: PlayerId,
    source: DamageSource,
    amount: f32,
    random: &mut Option<JavaRandom>,
) -> bool {
    let (hurt, drops) = hurt_player_in(&mut region.hurt_parts(), id, source, amount, random);
    if let Some(drops) = drops {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_player_death_drops(&drops);
    }
    hurt
}

/// 生物打玩家（`Mob.doHurtTarget` 里的 `hurtServer`）：玩家自己的实体随机源
/// （击退方向退化时取数）按值拷出、打中后写回。精确驱动的钩子和效果阶段都走这里。
pub(crate) fn hurt_player_by_mob(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    source: DamageSource,
    amount: f32,
) -> (bool, Option<PlayerDeathDrops>) {
    let mut random = parts.players.get(&id).map(|player| player.random);
    let defer = std::mem::replace(&mut parts.defer_health, true);
    let (hurt, drops) = hurt_player_in(parts, id, source, amount, &mut random);
    parts.defer_health = defer;
    if hurt && let (Some(state), Some(player)) = (random, parts.players.get_mut(&id)) {
        player.random = state;
    }
    if hurt && source.arrow_punch > 0 {
        apply_arrow_push(parts, id, &source);
    }
    (hurt, drops)
}

/// 玩家的击退抗性：直接从属性表读 `Attributes.KNOCKBACK_RESISTANCE`。
///
/// The player's knockback resistance from `Attributes.KNOCKBACK_RESISTANCE`.
fn knockback_resistance(player: &Player) -> f64 {
    player
        .attributes
        .value(mc_bamboo_data::registries::attribute::KNOCKBACK_RESISTANCE)
}

/// 山羊冲撞的 `LivingEntity.knockback(strength, dx, dz)`：不管这一下打没打中都推，
/// 方向和力道由山羊给；推完把速度发给看得见的人和自己。返回这一下发出去的速度，
/// 没推（力道归零）时是 `None`。
///
/// A goat ram's `LivingEntity.knockback(strength, dx, dz)`, applied whether or not the
/// hit landed, with the goat's direction and strength; the new velocity goes to the
/// trackers and the player. Returns the velocity sent, or `None` when nothing was pushed.
pub(crate) fn knock_back_player(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    strength: f64,
    (mut dx, mut dz): (f64, f64),
) -> Option<Vec3> {
    let movement = {
        let player = parts.players.get_mut(&id)?;
        let strength = strength * (1.0 - knockback_resistance(player));
        if strength <= 0.0 {
            return None;
        }
        while dx * dx + dz * dz < f64::from(1.0e-5_f32) {
            dx = (player.random.next_double() - player.random.next_double()) * 0.01;
            dz = (player.random.next_double() - player.random.next_double()) * 0.01;
        }
        let length = (dx * dx + dz * dz).sqrt();
        let delta = player.motion.delta;
        let dy = if player.on_ground {
            0.4_f64.min(delta.y / 2.0 + strength)
        } else {
            delta.y
        };
        player.motion.delta = Vec3::new(
            delta.x / 2.0 - dx / length * strength,
            dy,
            delta.z / 2.0 - dz / length * strength,
        );
        player.motion.delta
    };
    let packet = crate::links::encode_frames(&SetEntityMotion {
        entity_id: id.0,
        movement,
    });
    send_to_tracking_and_self(parts, id, &packet);
    Some(movement)
}

/// `Entity.push(dx, dy, dz)` 给玩家：音波命中这类「直接加冲量」的击退。
/// 不按 `knockback` 公式，直接把向量加到速度上，再广播运动包。返回加完的速度。
///
/// `Entity.push(dx, dy, dz)` for a player: an impulse knockback such as a landed
/// sonic boom. It adds the vector to the velocity directly and broadcasts motion.
pub(crate) fn push_player(parts: &mut HurtParts<'_>, id: PlayerId, push: Vec3) -> Option<Vec3> {
    let movement = {
        let player = parts.players.get_mut(&id)?;
        player.motion.delta = player.motion.delta.add(push);
        player.motion.delta
    };
    let packet = crate::links::encode_frames(&SetEntityMotion {
        entity_id: id.0,
        movement,
    });
    send_to_tracking_and_self(parts, id, &packet);
    Some(movement)
}

fn apply_arrow_push(parts: &mut HurtParts<'_>, id: PlayerId, source: &DamageSource) {
    let Some(player) = parts.players.get_mut(&id) else {
        return;
    };
    let resistance = knockback_resistance(player);
    if let Some(push) = source.arrow_push(resistance) {
        player.motion.delta = player.motion.delta.add(push);
        let packet = crate::links::encode_frames(&SetEntityMotion {
            entity_id: id.0,
            movement: player.motion.delta,
        });
        send_to_tracking_and_self(parts, id, &packet);
    }
}

/// [`hurt_player`] 的本体：返回 `wasHurt` 和打死时要在世界里生成的掉落。
pub(crate) fn hurt_player_in(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    source: DamageSource,
    amount: f32,
    random: &mut Option<JavaRandom>,
) -> (bool, Option<PlayerDeathDrops>) {
    // 火矢先点燃，再伤害；拒绝伤害时只恢复火焰刻数，不恢复冰冻。
    // Flaming arrows ignite before damage; rejection restores only the fire timer, not freezing.
    let previous_fire = if source.arrow_ignite_ticks > 0 {
        parts.players.get_mut(&id).map(|player| {
            let previous = player.environment.remaining_fire_ticks;
            crate::player_environment::ignite_player(player, source.arrow_ignite_ticks);
            previous
        })
    } else {
        None
    };
    let outcome = hurt_player_damage(parts, id, source, amount, random);
    if !outcome.0
        && let Some(previous) = previous_fire
        && let Some(player) = parts.players.get_mut(&id)
    {
        crate::player_environment::set_player_fire_ticks(player, previous);
    }
    outcome
}

fn hurt_player_damage(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    source: DamageSource,
    amount: f32,
    random: &mut Option<JavaRandom>,
) -> (bool, Option<PlayerDeathDrops>) {
    let difficulty = parts.difficulty;
    let Some(player) = parts.players.get_mut(&id) else {
        return (false, None);
    };
    // 创造 / 旁观模式的能力无敌（官方 `Player.hurtServer` 的第二道判据）。
    // 这一期把创造模式按 `Abilities.invulnerable` 处理；旁观者直接不受伤。
    if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Spectator {
        return (false, None);
    }
    if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative
        && !source.kind.bypasses_invulnerability()
    {
        return (false, None);
    }
    // 官方 `Player.hurtServer` 在这两道判据之后、结算伤害之前无条件
    // `removeEntitiesOnShoulder()`：受伤立刻把肩上的鹦鹉放回世界。
    // Vanilla `Player.hurtServer` calls `removeEntitiesOnShoulder()` here,
    // before resolving damage: being hurt drops shoulder parrots at once.
    crate::player_shoulder::remove_on_hurt(player, parts.entities);
    // `LivingEntity.hurtServer`：`#is_fire` 的伤害被抗火效果完全挡下。
    // `LivingEntity.hurtServer`: fire resistance blocks all `#is_fire` damage.
    if source.kind.is_fire()
        && player
            .effects
            .has(mc_bamboo_data::registries::mob_effect::FIRE_RESISTANCE)
    {
        return (false, None);
    }
    let position = player.position();
    let max_health = crate::player_effects::max_health(player);
    let armor = {
        let empty = mc_bamboo_protocol::item::ItemStack::empty();
        let equipment = player.equipment_stacks(&empty);
        let mut armor = crate::player_health::PlayerHealth::armor_values(&equipment);
        // 官方 `LivingEntity.getDamageAfterArmorAbsorb`：`getArmorValue()` 取
        // `Mth.floor(getAttributeValue(ARMOR))`，韧性取 `(float) getAttributeValue(ARMOR_TOUGHNESS)`。
        // 保护附魔等级和抗性提升效果仍从装备附魔与效果表读。
        //
        // Vanilla `getDamageAfterArmorAbsorb`: armor is `floor(ARMOR)`, toughness
        // is `(float) ARMOR_TOUGHNESS`; protection and resistance come from
        // enchantments and effects.
        armor.armor = mc_bamboo_core::mth::floor(
            player
                .attributes
                .value(mc_bamboo_data::registries::attribute::ARMOR),
        ) as f32;
        armor.toughness = player
            .attributes
            .value(mc_bamboo_data::registries::attribute::ARMOR_TOUGHNESS)
            as f32;
        armor.resistance = player
            .effects
            .amplifier(mc_bamboo_data::registries::mob_effect::RESISTANCE);
        armor
    };
    let (outcome, hurt_yaw, sync, saturation) = {
        let outcome = player
            .health
            .hurt(amount, source, difficulty, armor, position, max_health);
        let Some(outcome) = outcome else {
            return (false, None);
        };
        // `resolveMobResponsibleForDamage` 和 `lastDamageSource`：狼保护主人时读。
        // `resolveMobResponsibleForDamage` and `lastDamageSource`, read by wolves.
        let now = player.entity_ticks;
        player.combat_memory.record_hurt(&source, now);
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
            send_player_combat_enter(parts, id, &source);
        }
        broadcast_damage_event(parts, id, &source);
        // 官方 `dealDefaultKnockback` 里 `indicateDamage` 先发 hurt_animation，
        // 之后才播死亡 / 受伤音效。
        if outcome.knockback {
            let player = parts.players.get_mut(&id).expect("玩家刚才还在");
            knockback(player, &source, position, random);
            parts.links.send(
                id,
                &HurtAnimation {
                    entity_id: id.0,
                    yaw: hurt_yaw,
                },
            );
        }
        // `LivingEntity.hurtServer` 在 `isDeadOrDying()` 时播 `getDeathSound()`
        // （玩家的 `entity.player.death`），否则播受伤音效。
        let dying = parts
            .players
            .get(&id)
            .is_some_and(|player| player.health.health <= 0.0);
        if dying {
            play_sound(parts, id, "minecraft:entity.player.death", position);
        } else {
            play_hurt_sound(parts, id, &source, position);
        }
    }
    // 官方 `Player.actuallyHurt` 对完整受击和冷却期内的部分受击都写
    // `CombatTracker.recordDamage`、设置击杀归属，只要真正扣了血。
    if outcome.damage > 0.0 {
        crate::player_death::record_combat(parts, id, &source, outcome.damage);
        // `LivingEntity.hurtServer` 里玩家实际受伤时触发 ENTITY_HURT_PLAYER。
        let attack_type = source.kind.name();
        let projectile = matches!(
            source.kind,
            mc_bamboo_blocks::entity::mob::DamageKind::Arrow
        );
        if let Some(events) = parts.pending_triggers.as_deref_mut() {
            events.push(crate::advancement_triggers::PendingEvent {
                player: id,
                trigger: "minecraft:entity_hurt_player",
                payload: serde_json::json!({
                    "damage": {
                        "blocked": !outcome.full_hit,
                        "projectile": projectile,
                        "type": attack_type,
                        "dealt": amount,
                        "taken": outcome.damage,
                    },
                }),
            });
        }
    }
    let dying = parts
        .players
        .get(&id)
        .is_some_and(|player| player.health.health <= 0.0 && !player.dead);
    // 官方 `ServerPlayer.doTick` 在 `die` 之后才补 `set_health`；
    // 死亡流程里的 player_combat_kill / system_chat / entity_event 必须排前面。
    let send_health = |parts: &mut HurtParts<'_>| {
        let player = parts.players.get_mut(&id).expect("玩家刚才还在");
        if sync != player.health_sync {
            player.health_sync = sync;
            parts.links.send(
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
    // 掉落、死亡消息和死亡界面都在这里接上；重生等客户端的
    // `PERFORM_RESPAWN`，不受 `immediate_respawn` 影响。
    let drops = if dying {
        crate::player_death::die_in(parts, id, source)
    } else {
        None
    };
    // 官方 `ServerPlayer.doTick` 在 `die` 之后才补 `set_health`，死亡流程里的
    // player_combat_kill / system_chat / entity_event 以及同一刻更晚的爆炸包都要排前面，
    // 所以死了也走玩家阶段的血量同步，不在这里当场补。
    //
    // Vanilla `doTick` supplements `set_health` after `die`: the death flow's
    // combat_kill / system_chat / entity_event and the same-tick blast packet must
    // come first, so a dying player also syncs in the player phase.
    if !parts.defer_health {
        send_health(parts);
    }
    (true, drops)
}

/// `ServerLevel.broadcastDamageEvent`：`damage_event` 发给跟踪者（含自己）。
fn broadcast_damage_event(parts: &mut HurtParts<'_>, id: PlayerId, source: &DamageSource) {
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
    send_to_tracking_and_self(parts, id, &packet);
}

/// `CombatTracker.recordDamage`：造成者是非玩家生物时，`ServerPlayer.onEnterCombat`
/// 给玩家自己发一个没有包体的 `player_combat_enter`。
fn send_player_combat_enter(parts: &mut HurtParts<'_>, id: PlayerId, source: &DamageSource) {
    let living_source = source.attacker.is_some()
        && (source.kind != mc_bamboo_blocks::entity::mob::DamageKind::Arrow
            || source.attacker_is_player
            || source.scale_when_non_player);
    if living_source && parts.players.get(&id).is_some_and(|p| !p.in_combat) {
        if let Some(player) = parts.players.get_mut(&id) {
            player.in_combat = true;
        }
        parts.links.send(id, &PlayerCombatEnter);
    }
}

/// `LivingEntity.playHurtSound`：官方 `Level.playSound` 取一次世界随机源当种子，
/// 广播给范围内除自己以外的玩家。音效由 `DamageType.effects` 决定。
/// `LivingEntity.causeFallDamage` 的 `playSound(getFallDamageSound(i))`：
/// 伤害 > 4 用 `big_fall`，否则 `small_fall`，音源 PLAYERS。
pub(crate) fn play_fall_sound(region: &mut Region, id: PlayerId, damage: i32, position: Vec3) {
    let name = if damage > 4 {
        "minecraft:entity.player.big_fall"
    } else {
        "minecraft:entity.player.small_fall"
    };
    // 官方 `Entity.playSound` 走 `ServerLevel.playSound` →
    // `sendToTrackingPlayersAndSelf`，玩家自己也收到这一包，且排在伤害事件之前。
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
    let frames = crate::links::encode_frames(&sound);
    send_to_tracking_and_self(&mut region.hurt_parts(), id, &frames);
}

fn play_hurt_sound(parts: &mut HurtParts<'_>, id: PlayerId, source: &DamageSource, position: Vec3) {
    let name = hurt_sound_name(source);
    play_sound(parts, id, name, position);
    // `LivingEntity.playSecondaryHurtSound`：`thorns` 类型再多一声。
    if source.kind == mc_bamboo_blocks::entity::mob::DamageKind::Thorns {
        let seed = parts.sound_seeds.source().next_long();
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
        crate::interaction::send_sound_to(parts.players, parts.links, Some(id), &secondary);
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
fn play_sound(parts: &mut HurtParts<'_>, id: PlayerId, name: &str, position: Vec3) {
    let seed = parts.sound_seeds.source().next_long();
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
    crate::interaction::send_sound_to(parts.players, parts.links, Some(id), &sound);
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
fn send_to_tracking_and_self(parts: &mut HurtParts<'_>, id: PlayerId, packet: &[u8]) {
    let viewers: Vec<PlayerId> = parts
        .players
        .get(&id)
        .map(|player| player.sync.seen_by().iter().copied().collect())
        .unwrap_or_default();
    for viewer in viewers {
        parts.links.send_frames(viewer, packet);
    }
    parts.links.send_frames(id, packet);
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
    let (mut dx, mut dz) = source.knockback_direction(position).unwrap_or((0.0, 0.0));
    if let Some(random) = random.as_mut()
        && dx * dx + dz * dz < 1.0e-5
    {
        dx = (random.next_double() - random.next_double()) * 0.01;
        dz = (random.next_double() - random.next_double()) * 0.01;
    }
    if dx * dx + dz * dz < 1.0e-5 {
        return;
    }
    // 官方 `dealDefaultKnockback` 传的是 float 0.4f，转成 double 后再乘 `1 - KNOCKBACK_RESISTANCE`。
    let strength = f64::from(0.4_f32) * (1.0 - knockback_resistance(player));
    if strength <= 0.0 {
        return;
    }
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

/// 官方 `ServerGamePacketListenerImpl.handleAttack` → `Player.attack` 的入口。
/// 完整逻辑（冷却、暴击、横扫、击退、附魔、粒子、音效、动画、PVP）在
/// [`crate::player_attack`]，这里只保留原来的公共接缝。
///
/// Entry point for `ServerGamePacketListenerImpl.handleAttack` → `Player.attack`.
/// The full logic (cooldown, crit, sweep, knockback, enchantments, particles,
/// sounds, animations, PVP) lives in [`crate::player_attack`]; this is the seam.
pub(crate) fn on_attack(region: &mut Region, id: PlayerId, entity_id: i32) {
    crate::player_attack::on_attack(region, id, entity_id);
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

#[cfg(test)]
mod mob_hit_tests {
    use mc_bamboo_blocks::entity::mob::{DamageKind, DamageSource};
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    fn region_with_player(health: f32) -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        region.global.difficulty = crate::world_state::Difficulty::Normal;
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        player.health.health = health;
        player.inventory.slots[36] =
            mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::STONE, 3);
        region.insert_player(player);
        (region, id)
    }

    fn zombie_hit() -> DamageSource {
        let mut source =
            DamageSource::environment(DamageKind::MobAttack).with_non_player_cause(true);
        source.attacker = Some(1 << 20);
        source.direct = Some(1 << 20);
        source.position = Some(Vec3::new(1.5, 64.0, 0.5));
        source
    }

    #[test]
    fn flaming_arrow_restores_only_fire_after_creative_rejection() {
        let (mut region, id) = region_with_player(20.0);
        let player = region.players.get_mut(&id).unwrap();
        player.game_mode = GameMode::Creative;
        player.environment.remaining_fire_ticks = 37;
        player.environment.ticks_frozen = 80;
        let mut source = DamageSource::environment(DamageKind::Arrow);
        source.arrow_ignite_ticks = 100;
        {
            let (mut level, ticks) = region.entity_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            assert!(!world.hurt_player_from_mob(id.0, source, 3.0, None));
        }
        let player = &region.players[&id];
        assert_eq!(player.health.health, 20.0);
        assert_eq!(player.environment.remaining_fire_ticks, 1);
        assert_eq!(player.environment.ticks_frozen, 0);
    }

    #[test]
    fn flaming_arrow_restores_the_uncapped_fire_timer_in_survival() {
        let (mut region, id) = region_with_player(20.0);
        let player = region.players.get_mut(&id).unwrap();
        player.environment.remaining_fire_ticks = 37;
        player.environment.ticks_frozen = 80;
        let mut source = DamageSource::environment(DamageKind::Arrow);
        source.arrow_ignite_ticks = 100;
        {
            let (mut level, ticks) = region.entity_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            assert!(world.hurt_player_from_mob(id.0, zombie_hit(), 3.0, None));
            assert!(!world.hurt_player_from_mob(id.0, source, 2.0, None));
        }
        let player = &region.players[&id];
        assert_eq!(player.health.health, 17.0);
        assert_eq!(player.environment.remaining_fire_ticks, 37);
        assert_eq!(player.environment.ticks_frozen, 0);
    }

    /// 精确驱动的世界当场落地：第一下打中，同一刻的第二下落在无敌帧里返回假，
    /// 两下都不留延后的效果。
    #[test]
    fn the_entity_level_applies_a_mob_hit_at_once() {
        let (mut region, id) = region_with_player(20.0);
        let (mut level, ticks) = region.entity_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        assert!(world.hurt_player_from_mob(id.0, zombie_hit(), 3.0, None));
        assert!(!world.hurt_player_from_mob(id.0, zombie_hit(), 3.0, None));
        assert!(world.drain_effects().is_empty());
        drop(world);
        drop(level);
        let player = &region.players[&id];
        assert_eq!(player.health.health, 17.0);
        assert_eq!(player.health.damage_cooldown, 20);
    }

    /// 鸡蛋打中玩家是 0 点伤害：官方 `Player.hurtServer` 末尾 `damage == 0.0F`
    /// 直接返回假，不扣血、不进无敌帧、不击退，也不发任何包。
    ///
    /// A thrown egg deals 0 damage to a player: vanilla `Player.hurtServer` ends
    /// with `damage == 0.0F ? false`, so there is no health loss, no cooldown, no
    /// knockback and no packet.
    #[test]
    fn a_thrown_egg_does_not_hurt_a_player() {
        let (mut region, id) = region_with_player(20.0);
        let mut source =
            DamageSource::environment(DamageKind::Thrown).with_position(Vec3::new(1.5, 64.0, 0.5));
        source.direct = Some(1 << 20);
        source.projectile_motion = Some(Vec3::new(-1.0, 0.0, 0.0));
        {
            let (mut level, ticks) = region.entity_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            assert!(!world.hurt_player_from_mob(id.0, source, 0.0, None));
            assert!(world.drain_effects().is_empty());
        }
        let player = &region.players[&id];
        assert_eq!(player.health.health, 20.0);
        assert_eq!(player.health.damage_cooldown, 0);
        assert_eq!(player.health.hurt_time, 0);
        assert_eq!(player.motion.delta, Vec3::default());
    }

    /// 方块逻辑和并行驱动的落地阶段没有玩家那一半：记成延后的效果，按打中算。
    #[test]
    fn the_block_level_defers_a_mob_hit() {
        let (mut region, id) = region_with_player(20.0);
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        assert!(world.hurt_player_from_mob(id.0, zombie_hit(), 3.0, None));
        let effects = world.drain_effects();
        assert!(matches!(
            effects.as_slice(),
            [mc_bamboo_blocks::Effect::HurtPlayer { .. }]
        ));
        drop(world);
        drop(level);
        assert_eq!(region.players[&id].health.health, 20.0);
    }

    /// 打死玩家时死亡流程当场跑完，背包里的东西在同一刻掉进世界。
    #[test]
    fn a_killing_mob_hit_runs_the_death_flow_on_the_spot() {
        let (mut region, id) = region_with_player(2.0);
        {
            let (mut level, ticks) = region.entity_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            assert!(world.hurt_player_from_mob(id.0, zombie_hit(), 3.0, None));
        }
        let player = &region.players[&id];
        assert!(player.dead);
        assert!(player.inventory.slots[36].is_empty());
        let items = region
            .entities
            .iter_public()
            .filter(|public| {
                matches!(
                    public.kind,
                    mc_bamboo_blocks::entity::KindPublic::Item { .. }
                )
            })
            .count();
        assert_eq!(items, 1);
    }
}
