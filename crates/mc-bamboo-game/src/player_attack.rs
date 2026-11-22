//! 玩家左键近战攻击：官方 `ServerGamePacketListenerImpl.handleAttack` →
//! `Player.attack` → `ServerPlayer.crit` / `magicCrit` / `getEnchantedDamage`。
//!
//! 伤害、充能、暴击、横扫、击退、附魔加成、粒子、音效、动画、耐久和饥饿消耗
//! 都按官方 26.3 的顺序算；改动玩家状态的部分在 [`crate::player_hurt`] 的
//! 公共接缝里，发包走 [`crate::interaction::send_effects`] 和 `links`。
//!
//! Vanilla melee attack: `ServerGamePacketListenerImpl.handleAttack` →
//! `Player.attack` → `ServerPlayer.crit` / `magicCrit` / `getEnchantedDamage`.
//! Damage, charge, criticals, sweeping, knockback, enchantment bonuses, particles,
//! sounds, animations, durability and exhaustion follow vanilla 26.3's order.

use mc_bamboo_blocks::{
    World,
    entity::mob::{DamageSource, effects::EffectInstance},
};
use mc_bamboo_core::{BlockPos, Hand, Vec3, mth};
use mc_bamboo_data::{
    gamerules,
    registries::{
        attribute, attribute_defaults, data_component_type as kinds, mob_effect, particle_type,
    },
    sounds,
    tags::{self, item_tags},
};
use mc_bamboo_protocol::{
    codec::write_varint,
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::{Animate, GameMode, LevelParticles, Randomization},
};

use crate::{
    durability, entity_sync, inbound::PlayerId, interaction, links::encode_frames, player::Player,
    player_exhaustion, player_health, player_hurt, region::Region,
};

/// `getAttackStrengthScale(0.5F)` 的容差，官方 `Player.attack` 里恒为 0.5。
///
/// The tolerance of `getAttackStrengthScale(0.5F)`, always 0.5 in `Player.attack`.
const PARTIAL_TICK: f32 = 0.5;
/// `baseDamageScaleFactor()` = `0.2 + s² × 0.8`。
///
/// `baseDamageScaleFactor()` = `0.2 + s² × 0.8`.
const BASE_SCALE_MIN: f32 = 0.2;
const BASE_SCALE_SPAN: f32 = 0.8;
/// `attackStrengthScale > 0.9F` 才算满充能。
///
/// `attackStrengthScale > 0.9F` counts as a full-strength attack.
const FULL_STRENGTH: f32 = 0.9;
const CRIT_MULTIPLIER: f32 = 1.5;
/// `Player.attack` 打中之后的 `causeFoodExhaustion(0.1F)`。
///
/// `causeFoodExhaustion(0.1F)` after a landed hit in `Player.attack`.
const ATTACK_EXHAUSTION: f32 = 0.1;
/// `cannotAttackWithItem(mainHandItem, 5)` 的容差。
///
/// The tolerance of `cannotAttackWithItem(mainHandItem, 5)`.
const CHARGE_TOLERANCE: f32 = 5.0;
/// `Abilities.getWalkingSpeed()` 的缺省值，也就是玩家 `MOVEMENT_SPEED` 的基值。
/// 注册表通用缺省 0.7 是给其它生物用的，玩家这里被 `Player` 覆盖成 0.1。
///
/// `Abilities.getWalkingSpeed()` default, i.e. the player's `MOVEMENT_SPEED`
/// base. The registry default 0.7 is for other mobs; `Player` overrides it to 0.1.
const PLAYER_WALKING_SPEED: f32 = 0.1;

/// `Player.aiStep`：每刻把 `attackStrengthTicker` 加一；主手物品换了（且物品
/// 本身或组件不同）就 `resetAttackStrengthTicker`。
///
/// `Player.aiStep`: increment `attackStrengthTicker` each tick, and reset it when
/// the main-hand item changes (item or components differ).
pub(crate) fn tick(region: &mut Region) {
    for player in region.players.values_mut() {
        player.attack_strength_ticker = player.attack_strength_ticker.wrapping_add(1);
        let held = player.inventory.held(Hand::Main).clone();
        if held != player.last_main_hand_item {
            if !same_item(&held, &player.last_main_hand_item) {
                player.attack_strength_ticker = 0;
            }
            player.last_main_hand_item = held;
        }
    }
}

/// `ServerGamePacketListenerImpl.handlePunch`：`resetAttackStrengthTicker()`，
/// 挥空也重置攻击冷却。
///
/// `ServerGamePacketListenerImpl.handlePunch`: `resetAttackStrengthTicker()`,
/// so swinging at air also resets the cooldown.
pub(crate) fn on_punch(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        player.attack_strength_ticker = 0;
    }
}

/// `ItemStack.isSameItem`：物品 ID 加组件相同（数量不算）。
///
/// `ItemStack.isSameItem`: same item id and components, ignoring count.
fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.added == b.added && a.removed == b.removed
}

/// `getAttributeValue(Attributes.ATTACK_SPEED)`：缺省值加主手武器的
/// `attribute_modifiers`（`ADD_VALUE`，主手槽）。
///
/// `getAttributeValue(Attributes.ATTACK_SPEED)`: the default plus the main-hand
/// weapon's `attribute_modifiers` (`ADD_VALUE`, main-hand slots).
fn attack_speed(held: &ItemStack) -> f64 {
    attribute_value(
        held,
        attribute::ATTACK_SPEED,
        attribute_defaults::ATTACK_SPEED,
    )
}

/// `getAttributeValue(Attributes.ATTACK_KNOCKBACK)`。
///
/// `getAttributeValue(Attributes.ATTACK_KNOCKBACK)`.
fn attack_knockback(held: &ItemStack) -> f64 {
    attribute_value(
        held,
        attribute::ATTACK_KNOCKBACK,
        attribute_defaults::ATTACK_KNOCKBACK,
    )
}

fn attribute_value(held: &ItemStack, wanted: i32, default: f64) -> f64 {
    let mut value = default;
    if held.is_empty() {
        return value;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return value;
    };
    if let Some(ComponentValue::AttributeModifiers(entries)) =
        defaults::resolve(held.item, &patch, kinds::ATTRIBUTE_MODIFIERS)
    {
        for entry in entries {
            if entry.attribute == wanted
                && entry.modifier.operation == 0
                && (entry.slot == 0 || entry.slot == 1 || entry.slot == 3)
            {
                value += entry.modifier.amount;
            }
        }
    }
    value
}

/// `getCurrentItemAttackStrengthDelay()` = `(float)(1.0 / ATTACK_SPEED × 20)`。
///
/// `getCurrentItemAttackStrengthDelay()` = `(float)(1.0 / ATTACK_SPEED × 20)`.
fn strength_delay(held: &ItemStack) -> f32 {
    (1.0 / attack_speed(held) * 20.0) as f32
}

/// `getAttackStrengthScale(0.5F)` = `clamp((ticker + 0.5) / delay, 0, 1)`。
///
/// `getAttackStrengthScale(0.5F)` = `clamp((ticker + 0.5) / delay, 0, 1)`.
fn strength_scale(ticker: i32, delay: f32) -> f32 {
    ((ticker as f32 + PARTIAL_TICK) / delay).clamp(0.0, 1.0)
}

/// `ItemStack.getOrDefault(MINIMUM_ATTACK_CHARGE, 0)`。
///
/// `ItemStack.getOrDefault(MINIMUM_ATTACK_CHARGE, 0)`.
fn minimum_charge(held: &ItemStack) -> f32 {
    if held.is_empty() {
        return 0.0;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return 0.0;
    };
    match defaults::resolve(held.item, &patch, kinds::MINIMUM_ATTACK_CHARGE) {
        Some(ComponentValue::MinimumAttackCharge(value)) => *value,
        _ => 0.0,
    }
}

/// `cannotAttackWithItem(mainHandItem, 5)`。
///
/// `cannotAttackWithItem(mainHandItem, 5)`.
fn cannot_attack_with_item(ticker: i32, held: &ItemStack, delay: f32) -> bool {
    let required = minimum_charge(held);
    required > 0.0 && (ticker as f32 + CHARGE_TOLERANCE) / delay < required
}

/// `mainHandItem.has(DataComponents.PIERCING_WEAPON)`：长矛走另一条攻击路径。
///
/// `mainHandItem.has(DataComponents.PIERCING_WEAPON)`: spears take another path.
fn has_piercing_weapon(held: &ItemStack) -> bool {
    if held.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return false;
    };
    defaults::resolve(held.item, &patch, kinds::PIERCING_WEAPON).is_some()
}

/// 武器上某个附魔的等级（沿用箭矢那边的 `EnchantmentHelper` 读法）。
///
/// One enchantment's level on the weapon, via the arrow module's reader.
fn enchantment_level(held: &ItemStack, name: &str) -> i32 {
    mc_bamboo_blocks::entity::arrow::enchantment_level(held, name)
}

/// `EntityTypePredicate.of(entityTypes, tag)`：目标实体类型是否在标签里。
///
/// `EntityTypePredicate.of(entityTypes, tag)`: whether the target type is in a tag.
fn entity_in_tag(name: &str, entity_type: i32) -> bool {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:entity_type")
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .is_some_and(|tag| tag.ids.contains(&entity_type))
}

/// `EnchantmentHelper.modifyDamage`：锋利无条件，亡灵杀手 / 节肢杀手看目标类型。
/// 返回加在 `baseDamage` 上的总和，即 `getEnchantedDamage − baseDamage`。
///
/// `EnchantmentHelper.modifyDamage`: Sharpness is unconditional, Smite / Bane of
/// Arthropods depend on the target type. Returns the sum added to `baseDamage`.
fn damage_bonus(held: &ItemStack, entity_type: i32) -> f32 {
    let mut bonus = 0.0;
    let sharpness = enchantment_level(held, "minecraft:sharpness");
    if sharpness > 0 {
        // `LevelBasedValue.Linear(1.0, 0.5)`。
        bonus += 1.0 + 0.5 * (sharpness - 1) as f32;
    }
    if entity_in_tag("minecraft:sensitive_to_smite", entity_type) {
        let smite = enchantment_level(held, "minecraft:smite");
        if smite > 0 {
            bonus += 2.5 * smite as f32;
        }
    }
    if entity_in_tag("minecraft:sensitive_to_bane_of_arthropods", entity_type) {
        let bane = enchantment_level(held, "minecraft:bane_of_arthropods");
        if bane > 0 {
            bonus += 2.5 * bane as f32;
        }
    }
    bonus
}

/// `EnchantmentHelper.modifyKnockback` 的加成 + `ATTACK_KNOCKBACK`，再 `/ 2`。
///
/// `EnchantmentHelper.modifyKnockback` bonus plus `ATTACK_KNOCKBACK`, then `/ 2`.
fn attack_knockback_amount(held: &ItemStack) -> f32 {
    let knockback = enchantment_level(held, "minecraft:knockback") as f32;
    (attack_knockback(held) as f32 + knockback) / 2.0
}

/// `sweeping_edge` 给 `SWEEPING_DAMAGE_RATIO` 的 `Fraction(L, L+1)`。
///
/// `sweeping_edge`'s `Fraction(L, L+1)` for `SWEEPING_DAMAGE_RATIO`.
fn sweeping_ratio(held: &ItemStack) -> f32 {
    let level = enchantment_level(held, "minecraft:sweeping_edge") as f32;
    if level > 0.0 {
        level / (level + 1.0)
    } else {
        0.0
    }
}

/// `Player.canCriticalAttack`：下落中、不着地、不攀爬、不在水里、没有失明、
/// 没骑乘、非疾跑，且目标是生物。
///
/// `Player.canCriticalAttack`: falling, not on ground / climbable / in water /
/// blinded / riding / sprinting, and the target is living.
fn can_critical(player: &Player, target_is_living: bool) -> bool {
    player.motion.fall_distance > 0.0
        && !player.on_ground
        && !player.effects.has(mob_effect::BLINDNESS)
        && !player.sync.is_sprinting()
        && player.riding.vehicle.is_none()
        && target_is_living
}

/// `Player.isSweepAttack`：满充能、非暴击、非疾跑、着地、移动够慢、拿剑。
///
/// `Player.isSweepAttack`: full strength, non-critical, non-sprint, on ground,
/// slow enough, and holding a sword.
fn is_sweep_attack(
    player: &Player,
    held: &ItemStack,
    full: bool,
    crit: bool,
    sprint: bool,
) -> bool {
    if !full || crit || sprint || !player.on_ground {
        return false;
    }
    let speed = PLAYER_WALKING_SPEED;
    let max = speed * 2.5;
    let motion = player.motion.known_movement;
    let horizontal = (motion.x * motion.x + motion.z * motion.z) as f32;
    if horizontal >= max * max {
        return false;
    }
    mc_bamboo_blocks::state::item_is_tagged(held.item, item_tags::SWORDS)
}

/// `level().playSound(null, getX(), getY(), getZ(), sound, PLAYERS, 1, 1)`。
/// 用同一个临时 `World` 把声音记进效果列表，后面统一 `send_effects` 发出去，
/// 否则玩家听不到（弓箭曾经踩过这个坑）。
///
/// `level().playSound(null, ...)` recorded into the shared `World`'s effect list,
/// then drained and delivered with `send_effects`. Without that drain no client
/// hears it (the bow path hit this bug before).
fn push_sound(world: &mut World<'_>, position: Vec3, name: &str) {
    world.play_sound(
        position,
        sounds::sound_event(name),
        sounds::PLAYERS,
        1.0,
        1.0,
        false,
    );
}

/// `ServerPlayer.crit` / `magicCrit`：`ChunkSource.sendToTrackingPlayersAndSelf(this, …)`。
/// 包里的实体是要播放动画的**被打目标**，收包的人是跟踪攻击者的人加攻击者自己。
///
/// `ServerPlayer.crit` / `magicCrit`: `sendToTrackingPlayersAndSelf(this, …)`.
/// The packet names the hit target, while recipients track the attacker.
fn send_animate(region: &mut Region, attacker: PlayerId, target: i32, action: u8) {
    let frames = encode_frames(&Animate {
        entity_id: target,
        action,
    });
    if let Some(player) = region.players.get(&attacker) {
        player.sync.send_to_viewers(&mut region.links, &frames);
    }
    region.links.send_frames(attacker, &frames);
}

/// 一次 `ServerLevel.sendParticles` 的参数。
///
/// One `ServerLevel.sendParticles` request.
struct ParticleSpec {
    particle: i32,
    position: Vec3,
    spread: (f32, f32, f32),
    speed: f32,
    count: i32,
}

/// `ServerLevel.sendParticles`：32 格内（按玩家所在方块中心算）每个玩家一个包。
///
/// `ServerLevel.sendParticles`: one packet per player within 32 blocks, measured
/// from the centre of the player's block.
fn send_particles(region: &mut Region, spec: ParticleSpec) {
    let mut options = Vec::new();
    write_varint(&mut options, spec.particle);
    let packet = LevelParticles {
        options: &options,
        override_limiter: false,
        always_show: false,
        x: spec.position.x,
        y: spec.position.y,
        z: spec.position.z,
        x_dist: spec.spread.0,
        y_dist: spec.spread.1,
        z_dist: spec.spread.2,
        max_speed: spec.speed,
        count: spec.count,
        randomization: Randomization::Default,
    };
    let viewers: Vec<PlayerId> = region
        .players
        .values()
        .filter(|player| {
            let block = BlockPos::containing(player.position());
            let center = Vec3::new(
                f64::from(block.x) + 0.5,
                f64::from(block.y) + 0.5,
                f64::from(block.z) + 0.5,
            );
            let dx = center.x - spec.position.x;
            let dy = center.y - spec.position.y;
            let dz = center.z - spec.position.z;
            dx * dx + dy * dy + dz * dz < 32.0 * 32.0
        })
        .map(|player| player.id)
        .collect();
    for viewer in viewers {
        region.links.send(viewer, &packet);
    }
}

/// 目标的公共信息，够距判定和附魔条件都读它。
///
/// Public target info; reach and enchantment conditions read it.
struct TargetInfo {
    position: Vec3,
    entity_type: i32,
    is_living: bool,
    health: f32,
    dimensions: (f32, f32),
}

fn mob_target(public: &mc_bamboo_blocks::entity::EntityPublic) -> TargetInfo {
    let is_living = matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_));
    let health = match &public.kind {
        mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.health,
        _ => 0.0,
    };
    TargetInfo {
        position: public.position,
        entity_type: public.kind.entity_type(),
        is_living,
        health,
        dimensions: public.kind.dimensions(),
    }
}

/// `handleAttack` 交给 `plan_attack` 的原始输入。
///
/// The raw inputs `handleAttack` hands to `plan_attack`.
struct AttackRequest {
    id: PlayerId,
    entity_id: i32,
    held: ItemStack,
    ticker: i32,
    position: Vec3,
    yaw: f32,
}

/// 一次攻击算好的所有量，交给落地和发包两个阶段用。
///
/// Everything computed for one attack, shared by the landing and delivery phases.
struct AttackPlan {
    held: ItemStack,
    position: Vec3,
    yaw: f32,
    s: f32,
    base_damage: f32,
    magic_boost: f32,
    total_damage: f32,
    full: bool,
    critical: bool,
    knockback_attack: bool,
    sweep: bool,
    source: DamageSource,
    knockback_sound: Option<&'static str>,
    sweep_targets: Vec<(i32, i32)>,
}

/// `ServerGamePacketListenerImpl.handleAttack` + `Player.attack` 的入口。
///
/// Entry point for `ServerGamePacketListenerImpl.handleAttack` + `Player.attack`.
pub(crate) fn on_attack(region: &mut Region, id: PlayerId, entity_id: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == GameMode::Spectator || player.spawn_pending.is_some() {
        return;
    }
    let position = player.position();
    let eye_height = player.eye_height();
    let yaw = player.yaw;
    let held = player.inventory.held(Hand::Main).clone();
    let ticker = player.attack_strength_ticker;
    let reach = interaction::interaction_range(player.game_mode);
    let Some(target) = resolve_target(region, id, entity_id, position, eye_height, reach) else {
        return;
    };
    let request = AttackRequest {
        id,
        entity_id,
        held,
        ticker,
        position,
        yaw,
    };
    let Some(plan) = plan_attack(region, &target, &request) else {
        return;
    };
    let (was_hurt, actual_damage) = land_attack(region, id, entity_id, &target, &plan);
    if was_hurt {
        finish_hit(region, id, entity_id, &target, &plan, actual_damage);
    }
}

/// `handleAttack` 的目标解析：够距、可攻击、PVP 开关和自己不能打自己。
/// 非法目标（掉落物 / 经验球 / 不可攻击的箭）官方断开连接，这里只忽略。
///
/// `handleAttack` target resolution: reach, attackable, PVP rule, no self-hits.
/// Vanilla disconnects on invalid targets; we ignore them.
fn resolve_target(
    region: &Region,
    attacker: PlayerId,
    entity_id: i32,
    position: Vec3,
    eye_height: f64,
    reach: f64,
) -> Option<TargetInfo> {
    if let Some(public) = region.entities.public_at(entity_id) {
        if public.removed || !is_attackable(&public.kind) {
            return None;
        }
        let in_range = interaction::within_block_interaction_range(
            position,
            eye_height,
            reach,
            BlockPos::containing(public.position),
            3.0,
        );
        return in_range.then(|| mob_target(public));
    }
    let other = region.players.get(&PlayerId(entity_id))?;
    // `ServerPlayer.canHarmPlayer`：PVP 规则 + 队伍，Bamboo 没有队伍。
    //
    // `ServerPlayer.canHarmPlayer`: the PVP rule plus teams; Bamboo has no teams.
    if entity_id == attacker.0 || !region.global.rules.bool(gamerules::PVP) {
        return None;
    }
    let in_range = interaction::within_block_interaction_range(
        position,
        eye_height,
        reach,
        BlockPos::containing(other.position()),
        3.0,
    );
    in_range.then(|| TargetInfo {
        position: other.position(),
        entity_type: mc_bamboo_data::registries::entity_type::PLAYER,
        is_living: true,
        health: other.health.health,
        dimensions: (0.6, 1.8),
    })
}

/// `Player.attack` 的伤害算式：`s`、`baseDamage`、`magicBoost`、暴击 / 横扫 /
/// 疾跑判定，以及 `onAttack()` 重置冷却。`PIERCING_WEAPON` 和
/// `MINIMUM_ATTACK_CHARGE` 也在这一层挡下。
///
/// `Player.attack`'s damage maths: `s`, `baseDamage`, `magicBoost`, the crit /
/// sweep / sprint decisions and `onAttack()` resetting the cooldown.
/// `PIERCING_WEAPON` and `MINIMUM_ATTACK_CHARGE` gate here too.
fn plan_attack(
    region: &mut Region,
    target: &TargetInfo,
    req: &AttackRequest,
) -> Option<AttackPlan> {
    let (id, entity_id, held, ticker, position, yaw) = (
        req.id,
        req.entity_id,
        &req.held,
        req.ticker,
        req.position,
        req.yaw,
    );
    let delay = strength_delay(held);
    if has_piercing_weapon(held) || cannot_attack_with_item(ticker, held, delay) {
        return None;
    }
    let s = strength_scale(ticker, delay);
    // 长矛 / 重锤的 `Item.getAttackDamageBonus` 在 26.3 里默认返回 0，
    // 其余物品（含剑）都不加，这里按官方缺省跳过。
    //
    // `Item.getAttackDamageBonus` defaults to 0 in 26.3; swords add nothing.
    let base_damage_0 = player_health::PlayerHealth::attack_damage(held);
    let magic_boost = s * damage_bonus(held, target.entity_type);
    let mut base_damage = base_damage_0 * (BASE_SCALE_MIN + s * s * BASE_SCALE_SPAN);
    // `onAttack()` → `resetOnlyAttackStrengthTicker()`。
    if let Some(player) = region.players.get_mut(&id) {
        player.attack_strength_ticker = 0;
    }
    let player = &region.players[&id];
    let full = s > FULL_STRENGTH;
    let sprinting = player.sync.is_sprinting();
    let knockback_attack = sprinting && full;
    // 疾跑满充能的 `PLAYER_ATTACK_KNOCKBACK` 音效在伤害之前播。
    //
    // The full-strength sprint `PLAYER_ATTACK_KNOCKBACK` sound precedes damage.
    let knockback_sound = knockback_attack.then_some("minecraft:entity.player.attack.knockback");
    let critical = full && can_critical(player, target.is_living);
    if critical {
        base_damage *= CRIT_MULTIPLIER;
    }
    let total_damage = base_damage + magic_boost;
    let sweep = is_sweep_attack(player, held, full, critical, knockback_attack);
    // 横扫的候选来自实体表，要在借用 `block_level()` 之前取好。
    //
    // Sweep candidates come from the entity store, collected before `block_level()`.
    let sweep_targets = if sweep {
        collect_sweep_targets(region, id, entity_id, target, position)
    } else {
        Vec::new()
    };
    let mut source = DamageSource::player(id.0, position, player_is_creative(region, id));
    if !held.is_empty()
        && let Ok(weapon) = u16::try_from(held.item)
    {
        source = source.with_weapon(weapon);
    }
    Some(AttackPlan {
        held: held.clone(),
        position,
        yaw,
        s,
        base_damage,
        magic_boost,
        total_damage,
        full,
        critical,
        knockback_attack,
        sweep,
        source,
        knockback_sound,
        sweep_targets,
    })
}

/// 一次 `World` 里按官方顺序做完 `hurtOrSimulate`、额外击退、横扫、攻击后效果，
/// 再把所有效果（含攻击音效）一次 `send_effects` 发出去。
///
/// One `World` runs `hurtOrSimulate`, extra knockback, sweeping and post-attack
/// effects in vanilla order; all effects (attack sounds included) are then
/// delivered by a single `send_effects`.
fn land_attack(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    target: &TargetInfo,
    plan: &AttackPlan,
) -> (bool, f32) {
    let hits_player = player_target(region, entity_id);
    let hit = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        if let Some(name) = plan.knockback_sound {
            push_sound(&mut world, plan.position, name);
        }
        let hit = if hits_player {
            hurt_player_target(&mut world, entity_id, plan.source, plan.total_damage)
        } else {
            world.hurt_mob(entity_id, plan.source, plan.total_damage)
        };
        if hit {
            apply_extra_knockback(
                &mut world,
                entity_id,
                &plan.held,
                plan.knockback_attack,
                plan.yaw,
            );
            if plan.sweep {
                let spec = SweepSpec {
                    held: &plan.held,
                    base_damage: plan.base_damage,
                    s: plan.s,
                    player_position: plan.position,
                    yaw: plan.yaw,
                };
                do_sweep(&mut world, id, &plan.sweep_targets, &spec);
            }
            // `attackVisualEffects`：暴击音 + CRITICAL_HIT 动画，或 strong/weak 音；
            // 附魔加成 > 0 时再发 MAGIC_CRITICAL_HIT 动画。
            //
            // `attackVisualEffects`: crit sound + animation, or strong/weak sound;
            // a positive magic boost also sends the MAGIC_CRITICAL_HIT animation.
            if plan.critical {
                push_sound(
                    &mut world,
                    plan.position,
                    "minecraft:entity.player.attack.crit",
                );
            } else if !plan.sweep {
                push_sound(
                    &mut world,
                    plan.position,
                    if plan.full {
                        "minecraft:entity.player.attack.strong"
                    } else {
                        "minecraft:entity.player.attack.weak"
                    },
                );
            }
            if plan.sweep {
                push_sound(
                    &mut world,
                    plan.position,
                    "minecraft:entity.player.attack.sweep",
                );
            }
            post_attack_effects(&mut world, entity_id, &plan.held, target.entity_type);
        } else {
            push_sound(
                &mut world,
                plan.position,
                "minecraft:entity.player.attack.nodamage",
            );
        }
        let effects = world.drain_effects();
        interaction::send_effects(region, Some(id), &effects);
        hit
    };
    (hit, damage_taken(region, entity_id, target.health))
}

/// 命中之后：暴击 / 附魔暴击动画、横扫 / 伤害指示粒子、武器耐久、
/// `lastHurtMob`、饥饿消耗和挥手。
///
/// After a hit: crit / magic-crit animations, sweep / damage-indicator particles,
/// weapon durability, `lastHurtMob`, exhaustion and the swing.
fn finish_hit(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    target: &TargetInfo,
    plan: &AttackPlan,
    actual_damage: f32,
) {
    if plan.critical {
        send_animate(region, id, entity_id, Animate::CRITICAL_HIT);
    }
    if plan.magic_boost > 0.0 {
        send_animate(region, id, entity_id, Animate::MAGIC_CRITICAL_HIT);
    }
    let center_y = target.position.y + f64::from(target.dimensions.1) * 0.5;
    let indicator = Vec3::new(target.position.x, center_y, target.position.z);
    if plan.sweep {
        let radians = f64::from(plan.yaw * (std::f32::consts::PI / 180.0));
        let dx = -mth::sin(radians);
        let dz = mth::cos(radians);
        let player_center = player_center_y(region, id);
        send_particles(
            region,
            ParticleSpec {
                particle: particle_type::SWEEP_ATTACK,
                position: Vec3::new(
                    plan.position.x + f64::from(dx),
                    player_center,
                    plan.position.z + f64::from(dz),
                ),
                spread: (dx, 0.0, dz),
                speed: 0.0,
                count: 0,
            },
        );
    }
    if actual_damage > 2.0 {
        let count = (actual_damage * 0.5) as i32;
        send_particles(
            region,
            ParticleSpec {
                particle: particle_type::DAMAGE_INDICATOR,
                position: indicator,
                spread: (0.1, 0.0, 0.1),
                speed: 0.2,
                count,
            },
        );
    }
    // `itemAttackInteraction`：主手武器 `hurtEnemy` 扣 1 点耐久。
    //
    // `itemAttackInteraction`: `hurtEnemy` spends one durability point.
    durability::run(region, id, Hand::Main, |region| {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::durability::hurt_hand(&world, Hand::Main, 1);
    });
    // 打中之后记 `lastHurtMob`，加 0.1 饥饿消耗（pending，tick 末尾 flush），挥手。
    //
    // After a hit: record `lastHurtMob`, add 0.1 exhaustion (pending, flushed
    // at tick end) and swing.
    if let Some(player) = region.players.get_mut(&id) {
        let now = player.entity_ticks;
        player.combat_memory.record_attack(entity_id, now);
        player.health_sync = player_hurt::HealthSync::of(player);
    }
    player_exhaustion::add_in(region, id, ATTACK_EXHAUSTION);
    entity_sync::broadcast_swing(region, id, Hand::Main);
}

fn player_is_creative(region: &Region, id: PlayerId) -> bool {
    region
        .players
        .get(&id)
        .is_some_and(|player| player.game_mode == GameMode::Creative)
}

fn player_target(region: &Region, entity_id: i32) -> bool {
    region.players.contains_key(&PlayerId(entity_id))
}

/// 玩家打玩家走 `hurt_player_from_mob` 的同一条精确落地路径。
///
/// Player-on-player damage uses the same precise `hurt_player_from_mob` path.
fn hurt_player_target(
    world: &mut World<'_>,
    entity_id: i32,
    source: DamageSource,
    amount: f32,
) -> bool {
    world.hurt_player_from_mob(entity_id, source, amount, None)
}

fn damage_taken(region: &Region, entity_id: i32, old_health: f32) -> f32 {
    if player_target(region, entity_id) {
        return region
            .players
            .get(&PlayerId(entity_id))
            .map_or(0.0, |p| (old_health - p.health.health).max(0.0));
    }
    region
        .entities
        .public_at(entity_id)
        .map(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => (old_health - mob.health).max(0.0),
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

fn player_center_y(region: &Region, id: PlayerId) -> f64 {
    let Some(player) = region.players.get(&id) else {
        return 0.0;
    };
    let box3d = player.bounding_box();
    (box3d.min_y + box3d.max_y) * 0.5
}

/// `Entity.isAttackable()` 的简化版：只让生物和玩家挨打；掉落物、经验球、
/// 箭、矿车等都忽略（官方对其中一部分会断开连接）。
///
/// Simplified `Entity.isAttackable()`: only mobs and players; items, orbs,
/// arrows and vehicles are ignored (vanilla disconnects on some of them).
fn is_attackable(kind: &mc_bamboo_blocks::entity::KindPublic) -> bool {
    matches!(kind, mc_bamboo_blocks::entity::KindPublic::Mob(_))
}

/// `Player.causeExtraKnockback`：默认受伤击退之外的第二下击退，用攻击者朝向，
/// 力道是 `getKnockback + (疾跑 ? 0.5 : 0)`；顺带把攻击者水平速度 ×0.6、
/// 取消疾跑。
///
/// `Player.causeExtraKnockback`: the second knockback, along the attacker's
/// facing, with `getKnockback + (sprint ? 0.5 : 0)`; also damps the attacker's
/// horizontal velocity and stops sprinting.
fn apply_extra_knockback(
    world: &mut World<'_>,
    entity_id: i32,
    held: &ItemStack,
    sprint: bool,
    yaw: f32,
) {
    let amount = attack_knockback_amount(held) + if sprint { 0.5 } else { 0.0 };
    if amount <= 0.0 {
        return;
    }
    let radians = f64::from(yaw * (std::f32::consts::PI / 180.0));
    let dx = mth::sin(radians);
    let dz = -mth::cos(radians);
    world.knockback_mob(entity_id, f64::from(amount), f64::from(dx), f64::from(dz));
}

/// `Player.doSweepAttack` 的候选：主目标盒子 inflate(1, 0.25, 1) 里、
/// 距玩家小于 3 格的其它生物（玩家本人不算）。返回 (实体 ID, 实体类型)。
///
/// `Player.doSweepAttack` candidates: other mobs in the main target's
/// inflate(1, 0.25, 1) box and within 3 blocks of the player.
fn collect_sweep_targets(
    region: &Region,
    attacker: PlayerId,
    main_target: i32,
    target: &TargetInfo,
    player_position: Vec3,
) -> Vec<(i32, i32)> {
    let (width, height) = target.dimensions;
    let half = f64::from(width) * 0.5;
    region
        .entities
        .iter_public()
        .filter(|public| {
            matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_))
                && !public.removed
                && public.uid != main_target
                && public.uid != attacker.0
                && (public.position.x - target.position.x).abs() <= half + 1.0
                && (public.position.z - target.position.z).abs() <= half + 1.0
                && (public.position.y - target.position.y).abs() <= f64::from(height) + 0.25
                && (public.position.x - player_position.x).powi(2)
                    + (public.position.y - player_position.y).powi(2)
                    + (public.position.z - player_position.z).powi(2)
                    < 9.0
        })
        .map(|public| (public.uid, public.kind.entity_type()))
        .collect()
}

/// `Player.doSweepAttack`：给候选生物各打 `getEnchantedDamage(1 + ratio ×
/// baseDamage) × s` 点，命中后按攻击者朝向追加 0.4 击退。
///
/// `Player.doSweepAttack`: hit each candidate with
/// `getEnchantedDamage(1 + ratio × baseDamage) × s`, then knock back 0.4.
/// `do_sweep` 的固定参数。
///
/// The fixed parameters of `do_sweep`.
struct SweepSpec<'a> {
    held: &'a ItemStack,
    base_damage: f32,
    s: f32,
    player_position: Vec3,
    yaw: f32,
}

fn do_sweep(
    world: &mut World<'_>,
    attacker: PlayerId,
    nearby: &[(i32, i32)],
    spec: &SweepSpec<'_>,
) {
    let sweep_base = 1.0 + sweeping_ratio(spec.held) * spec.base_damage;
    let radians = f64::from(spec.yaw * (std::f32::consts::PI / 180.0));
    let dx = mth::sin(radians);
    let dz = -mth::cos(radians);
    for &(id, entity_type) in nearby {
        let enchanted = (sweep_base + damage_bonus(spec.held, entity_type)) * spec.s;
        let mut source = DamageSource::player(attacker.0, spec.player_position, false);
        if !spec.held.is_empty()
            && let Ok(weapon) = u16::try_from(spec.held.item)
        {
            source = source.with_weapon(weapon);
        }
        if world.hurt_mob(id, source, enchanted) {
            world.knockback_mob(id, f64::from(0.4_f32), f64::from(dx), f64::from(dz));
        }
    }
}

/// `EnchantmentHelper.doPostAttackEffectsWithItemSource`：火焰附加点燃目标，
/// 节肢杀手给节肢目标挂缓慢 IV。
///
/// `EnchantmentHelper.doPostAttackEffectsWithItemSource`: Fire Aspect ignites
/// the target and Bane of Arthropods applies Slowness IV to arthropods.
fn post_attack_effects(world: &mut World<'_>, entity_id: i32, held: &ItemStack, entity_type: i32) {
    let fire = enchantment_level(held, "minecraft:fire_aspect");
    if fire > 0 {
        // `Ignite(LevelBasedValue.perLevel(4.0F))` → 4×等级 秒 → ×20 刻。
        world.ignite_mob(entity_id, fire * 80);
    }
    let bane = enchantment_level(held, "minecraft:bane_of_arthropods");
    if bane > 0 && entity_in_tag("minecraft:sensitive_to_bane_of_arthropods", entity_type) {
        // `ApplyMobEffect(SLOWNESS, 1.5s, 1.5+0.5×(L−1)s, 3, 3)`：
        // 时长在 [30, 30+10×(L−1)] 刻之间取随机，等级恒为 3（缓慢 IV）。
        let max_ticks = 30 + 10 * (bane - 1);
        let ticks = sweep_slowness_ticks(world, entity_id, max_ticks);
        world.add_mob_effect(
            entity_id,
            EffectInstance::with_amplifier(mob_effect::SLOWNESS, ticks, 3),
        );
    }
}

/// `Mth.randomBetween(random, min, max) × 20` 的官方取整；随机源是目标生物自己的
/// `getRandom()`，这里按 `[30, max]` 均匀分布取一次 `nextFloat`。
///
/// `Mth.randomBetween(random, min, max) × 20`, drawn from the target's own
/// `getRandom()`; one `nextFloat` over `[30, max]`.
fn sweep_slowness_ticks(_world: &mut World<'_>, _entity_id: i32, max_ticks: i32) -> i32 {
    // 官方 `ApplyMobEffect` 先从 `HolderSet` 取一个元素，再各取一次随机定
    // 时长和等级。Bamboo 没有把生物随机源暴露到这里，用最短时长（30 刻）
    // 保守实现，见 README 已知限制。
    //
    // Vanilla draws from the mob's own random for duration and amplifier.
    // Bamboo does not expose that source here, so the minimum (30 ticks) is
    // used; see README known limits.
    let _ = max_ticks;
    30
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    fn stack(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    fn enchanted(item: i32, name: &str, level: i32) -> ItemStack {
        let id = mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
            .expect("the enchantment exists") as i32;
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(id, level)])),
        );
        let (added, removed) = patch.to_opaque();
        ItemStack {
            item,
            count: 1,
            added,
            removed,
        }
    }

    #[test]
    fn cooldown_scale_matches_vanilla() {
        // 空手 ATTACK_SPEED=4 → delay=5；ticker=0 时 s=0.1，ticker=5 时 s=1。
        let empty = ItemStack::empty();
        let delay = strength_delay(&empty);
        assert_eq!(delay, 5.0);
        assert_eq!(strength_scale(0, delay), 0.1);
        assert_eq!(strength_scale(4, delay), 0.9);
        assert_eq!(strength_scale(5, delay), 1.0);
        assert_eq!(BASE_SCALE_MIN + 1.0_f32 * 1.0 * BASE_SCALE_SPAN, 1.0);
        assert!((BASE_SCALE_MIN + 0.1_f32 * 0.1 * BASE_SCALE_SPAN - 0.208).abs() < 1.0e-6);
    }

    #[test]
    fn diamond_sword_slows_the_attack() {
        // 钻石剑 ATTACK_SPEED 修饰符 −2.4 → 1.6 → delay = 12.5。
        let sword = stack(mc_bamboo_data::items::DIAMOND_SWORD);
        assert!((strength_delay(&sword) - 12.5).abs() < 0.001);
        assert!((attack_speed(&sword) - 1.6).abs() < 0.001);
    }

    #[test]
    fn damage_enchantments_follow_the_generated_tables() {
        let cow = mc_bamboo_data::registries::entity_type::COW;
        let zombie = mc_bamboo_data::registries::entity_type::ZOMBIE;
        let spider = mc_bamboo_data::registries::entity_type::SPIDER;

        let sharp = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:sharpness",
            3,
        );
        assert_eq!(damage_bonus(&sharp, cow), 2.0);
        assert_eq!(damage_bonus(&sharp, zombie), 2.0);

        let smite = enchanted(mc_bamboo_data::items::DIAMOND_SWORD, "minecraft:smite", 2);
        assert_eq!(damage_bonus(&smite, cow), 0.0);
        assert_eq!(damage_bonus(&smite, zombie), 5.0);

        let bane = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:bane_of_arthropods",
            2,
        );
        assert_eq!(damage_bonus(&bane, spider), 5.0);
        assert_eq!(damage_bonus(&bane, zombie), 0.0);
    }

    #[test]
    fn knockback_and_sweeping_use_the_generated_formulas() {
        let knock = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:knockback",
            2,
        );
        assert_eq!(attack_knockback_amount(&knock), 1.0);
        let sweeping = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:sweeping_edge",
            3,
        );
        assert_eq!(sweeping_ratio(&sweeping), 0.75);
        assert_eq!(
            sweeping_ratio(&stack(mc_bamboo_data::items::DIAMOND_SWORD)),
            0.0
        );
    }

    #[test]
    fn strength_ticker_resets_on_item_swap_and_punch() {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            uuid::Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        player.attack_strength_ticker = 7;
        region.insert_player(player);
        // 主手没换，`aiStep` 只把计数加一。
        //
        // No item swap: `aiStep` only increments the ticker.
        tick(&mut region);
        assert_eq!(region.players[&id].attack_strength_ticker, 8);

        // 把主手换成另一种物品，下一刻重置冷却。
        //
        // Swapping the main-hand item resets the cooldown on the next tick.
        let slot = region.players[&id].inventory.hand_slot(Hand::Main);
        region.players.get_mut(&id).unwrap().inventory.slots[slot] =
            stack(mc_bamboo_data::items::DIAMOND_SWORD);
        tick(&mut region);
        assert_eq!(region.players[&id].attack_strength_ticker, 0);

        region.players.get_mut(&id).unwrap().attack_strength_ticker = 12;
        on_punch(&mut region, id);
        assert_eq!(region.players[&id].attack_strength_ticker, 0);
    }
}
