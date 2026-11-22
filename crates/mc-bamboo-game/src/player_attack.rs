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
use mc_bamboo_core::{Aabb, BlockPos, Hand, Vec3, mth};
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
/// `handleAttack` 传给 `isWithinAttackRange(mainHandItem, targetBounds, 3.0)` 的缓冲区。
///
/// The buffer `handleAttack` passes to
/// `isWithinAttackRange(mainHandItem, targetBounds, 3.0)`.
const ATTACK_RANGE_BUFFER: f64 = 3.0;
/// `Player.CREATIVE_ENTITY_INTERACTION_RANGE_MODIFIER_VALUE`：创造模式的
/// `ServerPlayer` 给 `ENTITY_INTERACTION_RANGE` 加的临时修饰符。
///
/// `Player.CREATIVE_ENTITY_INTERACTION_RANGE_MODIFIER_VALUE`: the transient
/// modifier creative `ServerPlayer`s add to `ENTITY_INTERACTION_RANGE`.
const CREATIVE_ENTITY_INTERACTION_RANGE: f32 = 2.0;

/// `Player.aiStep`：每刻把 `attackStrengthTicker` 加一；主手物品有任何变化
/// （数量 / 组件）就刷新缓存，但只有换成另一种物品（官方 `isSameItem`，只比
/// 物品 ID）才 `resetAttackStrengthTicker`。攻击命中扣耐久改的是组件，不该清冷却。
///
/// `Player.aiStep`: increment `attackStrengthTicker` each tick and refresh the
/// cache on any main-hand change (count / components), but call
/// `resetAttackStrengthTicker` only when the item itself changes (vanilla
/// `isSameItem`, item id only). A durability hit only changes components and
/// must not clear the cooldown.
pub(crate) fn tick(region: &mut Region) {
    for player in region.players.values_mut() {
        player.attack_strength_ticker = player.attack_strength_ticker.wrapping_add(1);
        let held = player.inventory.held(Hand::Main).clone();
        if held != player.last_main_hand_item {
            // 外层的 `!=` 检测物品堆的任意变化以刷新缓存；内层 `isSameItem` 只比
            // 物品 ID，决定是否重置冷却。组件 / 耐久 / 数量变化走到这里都不重置。
            //
            // The outer `!=` detects any stack change to refresh the cache; the
            // inner `isSameItem` compares the item id and decides whether to
            // reset. Component / durability / count changes land here without
            // resetting the cooldown.
            if !held.is_same_item(&player.last_main_hand_item) {
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
    /// `causeExtraKnockback` 的力道：`getKnockback + (疾跑 ? 0.5 : 0)`。
    /// 大于 0 时才推目标、衰减攻击者速度并取消疾跑。
    ///
    /// `causeExtraKnockback`'s strength: `getKnockback + (sprint ? 0.5 : 0)`.
    /// Only positive values push the target, damp the attacker and stop sprint.
    extra_knockback: f32,
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
    let game_mode = player.game_mode;
    let eye = Vec3::new(position.x, position.y + eye_height, position.z);
    let Some(target) = resolve_target(region, id, entity_id, eye, game_mode, &held) else {
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

/// `handleAttack` 的目标解析：世界边界、够距、可攻击、PVP 开关和自己不能打自己。
/// 非法目标（掉落物 / 经验球 / 不可攻击的箭）官方断开连接，这里只忽略。
///
/// `handleAttack` target resolution: world border, reach, attackable, PVP rule,
/// no self-hits. Vanilla disconnects on invalid targets; we ignore them.
fn resolve_target(
    region: &Region,
    attacker: PlayerId,
    entity_id: i32,
    eye: Vec3,
    game_mode: GameMode,
    held: &ItemStack,
) -> Option<TargetInfo> {
    let range = attack_range(held, game_mode);
    let creative = game_mode == GameMode::Creative;
    if let Some(public) = region.entities.public_at(entity_id) {
        if public.removed || !is_attackable(&public.kind) {
            return None;
        }
        // `level.getWorldBorder().isWithinBounds(target.blockPosition())`。
        //
        // `level.getWorldBorder().isWithinBounds(target.blockPosition())`.
        if !region
            .global
            .world_border
            .is_within_bounds_pos(BlockPos::containing(public.position))
        {
            return None;
        }
        let bounds = entity_bounds(public.position, public.kind.dimensions());
        return range
            .is_in_range(eye, bounds, ATTACK_RANGE_BUFFER, creative)
            .then(|| mob_target(public));
    }
    let other = region.players.get(&PlayerId(entity_id))?;
    // `ServerPlayer.canHarmPlayer`：PVP 规则 + 队伍，Bamboo 没有队伍。
    //
    // `ServerPlayer.canHarmPlayer`: the PVP rule plus teams; Bamboo has no teams.
    if entity_id == attacker.0 || !region.global.rules.bool(gamerules::PVP) {
        return None;
    }
    if !region
        .global
        .world_border
        .is_within_bounds_pos(BlockPos::containing(other.position()))
    {
        return None;
    }
    range
        .is_in_range(eye, other.bounding_box(), ATTACK_RANGE_BUFFER, creative)
        .then(|| TargetInfo {
            position: other.position(),
            entity_type: mc_bamboo_data::registries::entity_type::PLAYER,
            is_living: true,
            health: other.health.health,
            dimensions: (0.6, 1.8),
        })
}

/// 官方 `AttackRange` 里玩家攻击者读得到的几项。`mob_factor` 只对非玩家
/// 攻击者生效，玩家攻击永远走 `min/minCreative` / `max/maxCreative`。
///
/// The `AttackRange` fields a player attacker reads. `mob_factor` only applies
/// to non-player attackers, so players always use `min/minCreative` and
/// `max/maxCreative`.
#[derive(Debug, Clone, Copy)]
struct AttackRange {
    min_reach: f32,
    max_reach: f32,
    min_creative_reach: f32,
    max_creative_reach: f32,
    hitbox_margin: f32,
}

impl AttackRange {
    /// `AttackRange.defaultFor(livingEntity)`：`maxReach` 和 `maxCreativeReach`
    /// 取 `ENTITY_INTERACTION_RANGE`，其余全 0。
    ///
    /// `AttackRange.defaultFor(livingEntity)`: `maxReach` and `maxCreativeReach`
    /// take `ENTITY_INTERACTION_RANGE`, everything else is 0.
    fn default_for(entity_interaction_range: f32) -> Self {
        Self {
            min_reach: 0.0,
            max_reach: entity_interaction_range,
            min_creative_reach: 0.0,
            max_creative_reach: entity_interaction_range,
            hitbox_margin: 0.0,
        }
    }

    /// `AttackRange.isInRange(attacker, AABB, extraBuffer)`：眼睛到目标碰撞盒的
    /// 距离落在 `[min − margin − buffer, max + margin + buffer]` 闭区间内。
    ///
    /// `AttackRange.isInRange(attacker, AABB, extraBuffer)`: the eye-to-box
    /// distance lies in the closed `[min − margin − buffer, max + margin + buffer]`.
    fn is_in_range(self, eye: Vec3, bounds: Aabb, buffer: f64, creative: bool) -> bool {
        let (min, max) = if creative {
            (self.min_creative_reach, self.max_creative_reach)
        } else {
            (self.min_reach, self.max_reach)
        };
        let distance = bounds.distance_to_sqr(eye).sqrt();
        let margin = f64::from(self.hitbox_margin);
        distance >= f64::from(min) - margin - buffer && distance <= f64::from(max) + margin + buffer
    }
}

/// `LivingEntity.getAttackRangeWith(weaponItem)`：武器带 `ATTACK_RANGE` 组件
/// 就用它，否则 `AttackRange.defaultFor`。
///
/// `LivingEntity.getAttackRangeWith(weaponItem)`: use the weapon's `ATTACK_RANGE`
/// component when present, otherwise `AttackRange.defaultFor`.
fn attack_range(held: &ItemStack, game_mode: GameMode) -> AttackRange {
    if !held.is_empty()
        && let Ok(patch) = ComponentPatch::from_opaque(held)
        && let Some(ComponentValue::AttackRange {
            min_reach,
            max_reach,
            min_creative_reach,
            max_creative_reach,
            hitbox_margin,
            ..
        }) = defaults::resolve(held.item, &patch, kinds::ATTACK_RANGE)
    {
        return AttackRange {
            min_reach: *min_reach,
            max_reach: *max_reach,
            min_creative_reach: *min_creative_reach,
            max_creative_reach: *max_creative_reach,
            hitbox_margin: *hitbox_margin,
        };
    }
    AttackRange::default_for(entity_interaction_range(game_mode))
}

/// `getAttributeValue(Attributes.ENTITY_INTERACTION_RANGE)`：注册表缺省 3.0，
/// 创造模式的 `ServerPlayer` 再叠加 2.0 的临时修饰符。玩家的这个属性没有
/// 来自物品的修饰符。
///
/// `getAttributeValue(Attributes.ENTITY_INTERACTION_RANGE)`: registry default
/// 3.0 plus the 2.0 creative transient modifier. No item adds to it for players.
fn entity_interaction_range(game_mode: GameMode) -> f32 {
    let base = attribute_defaults::ENTITY_INTERACTION_RANGE as f32;
    if game_mode == GameMode::Creative {
        base + CREATIVE_ENTITY_INTERACTION_RANGE
    } else {
        base
    }
}

/// 目标实体的碰撞盒，官方 `Entity.makeBoundingBox(position)`：以脚底位置为中心、
/// 宽 `width` 高 `height`。
///
/// The target's collision box, vanilla `Entity.makeBoundingBox(position)`: a
/// `width × height` box centred on the feet position.
fn entity_bounds(position: Vec3, dimensions: (f32, f32)) -> Aabb {
    let half = f64::from(dimensions.0 / 2.0);
    let height = f64::from(dimensions.1);
    Aabb::new(
        position.x - half,
        position.y,
        position.z - half,
        position.x + half,
        position.y + height,
        position.z + half,
    )
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
    // `getKnockback(target, damageSource) + (knockbackAttack ? 0.5 : 0)`：
    // `Player.attack` 把它传给 `causeExtraKnockback`，这里先算好给落地阶段用。
    //
    // `getKnockback(target, damageSource) + (knockbackAttack ? 0.5 : 0)`:
    // `Player.attack` passes it to `causeExtraKnockback`; computed here for the
    // landing phase.
    let extra_knockback = attack_knockback_amount(held) + if knockback_attack { 0.5 } else { 0.0 };
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
        extra_knockback,
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
            apply_extra_knockback(&mut world, entity_id, plan.extra_knockback, plan.yaw);
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

/// 命中之后：衰减攻击者速度 / 取消疾跑、暴击 / 附魔暴击动画、横扫 / 伤害指示
/// 粒子、武器耐久、`lastHurtMob`、饥饿消耗和挥手。
///
/// After a hit: attacker velocity damp / sprint cancel, crit / magic-crit
/// animations, sweep / damage-indicator particles, weapon durability,
/// `lastHurtMob`, exhaustion and the swing.
fn finish_hit(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    target: &TargetInfo,
    plan: &AttackPlan,
    actual_damage: f32,
) {
    // `Player.causeExtraKnockback` 在命中之后衰减攻击者速度并取消疾跑，
    // 力道大于 0 才做；它和伤害、横扫、动画的先后顺序不影响后面读到的状态。
    //
    // `Player.causeExtraKnockback` damps the attacker and stops sprint after a
    // landed hit, only when its strength is positive; its exact position among
    // damage, sweep and animations does not change any state read afterwards.
    if plan.extra_knockback > 0.0 {
        damp_attacker(region, id);
    }
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

/// `Player.causeExtraKnockback` 对生物目标的那一下 `LivingEntity.knockback`：
/// 默认受伤击退之外的第二下，用攻击者朝向，力道是已经算好的
/// `getKnockback + (疾跑 ? 0.5 : 0)`。攻击者自己的速度衰减和取消疾跑见
/// [`damp_attacker`]，在命中之后单独落地。
///
/// The `LivingEntity.knockback` that `Player.causeExtraKnockback` applies to a
/// mob target: a second push along the attacker's facing with the precomputed
/// `getKnockback + (sprint ? 0.5 : 0)`. The attacker's own velocity damp and
/// sprint cancellation land separately in [`damp_attacker`] after the hit.
fn apply_extra_knockback(world: &mut World<'_>, entity_id: i32, amount: f32, yaw: f32) {
    if amount <= 0.0 {
        return;
    }
    let radians = f64::from(yaw * (std::f32::consts::PI / 180.0));
    let dx = mth::sin(radians);
    let dz = -mth::cos(radians);
    world.knockback_mob(entity_id, f64::from(amount), f64::from(dx), f64::from(dz));
}

/// `Player.causeExtraKnockback` 的收尾：`setDeltaMovement(×0.6, 1.0, ×0.6)`
/// 和 `setSprinting(false)`，只在 `knockbackAmount > 0` 的命中之后做。
///
/// The tail of `Player.causeExtraKnockback`: `setDeltaMovement(×0.6, 1.0, ×0.6)`
/// and `setSprinting(false)`, done only after a hit whose knockback is positive.
fn damp_attacker(region: &mut Region, id: PlayerId) {
    entity_sync::set_sprinting(region, id, false);
    if let Some(player) = region.players.get_mut(&id) {
        player.motion.delta = player.motion.delta.multiply(0.6, 1.0, 0.6);
    }
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

/// `Player.doSweepAttack`：给候选生物各打 `getEnchantedDamage(1 + ratio ×
/// baseDamage) × s` 点，命中后按攻击者朝向追加 0.4 击退，再补一次攻击后附魔
/// 效果（火焰附加 / 节肢杀手）。
///
/// `Player.doSweepAttack`: hit each candidate with
/// `getEnchantedDamage(1 + ratio × baseDamage) × s`, knock back 0.4 along the
/// attacker's facing, then apply the post-attack enchantment effects
/// (Fire Aspect / Bane of Arthropods) to that target too.
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
            // 官方 `doSweepAttack` 对每个打中的 nearby 也调一次 `doPostAttackEffects`。
            // Vanilla `doSweepAttack` also calls `doPostAttackEffects` on each hurt nearby.
            post_attack_effects(world, id, spec.held, entity_type);
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

    /// 官方 `Player.aiStep` 的内层判据是 `ItemStack.isSameItem`，只比物品 ID：
    /// 攻击扣耐久（写 `DAMAGE` 组件）、附魔组件变化都只刷新缓存，不清冷却；
    /// 换成另一种物品才清。这条测试锁住「同物品不同组件」不重置冷却，
    /// 否则每次攻击扣完耐久，下一刻冷却就被自己清掉。
    ///
    /// Vanilla `Player.aiStep`'s inner check is `ItemStack.isSameItem`, item id
    /// only: a durability hit (writing the `DAMAGE` component) or an enchantment
    /// change only refreshes the cache, while swapping the item resets the
    /// cooldown. This guards that a component-only change keeps the cooldown,
    /// so an attack cannot clear its own charge on the next tick.
    #[test]
    fn component_only_changes_keep_the_attack_cooldown() {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            uuid::Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        let slot = region.players[&id].inventory.hand_slot(Hand::Main);

        // 主手放剑、缓存对齐到同一把剑，模拟 `aiStep` 已经同步过；ticker 给 7。
        // Put a sword in hand and align the cache to the same stack, as if
        // `aiStep` already synced it; start the ticker at 7.
        let bare = stack(mc_bamboo_data::items::DIAMOND_SWORD);
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = bare.clone();
        region.players.get_mut(&id).unwrap().last_main_hand_item = bare.clone();
        region.players.get_mut(&id).unwrap().attack_strength_ticker = 7;

        // 耐久：攻击扣 5 点耐久写入 `DAMAGE` 组件，物品 ID 不变。
        // Durability: a 5-point hit writes the `DAMAGE` component; item id unchanged.
        let mut damaged = bare.clone();
        mc_bamboo_blocks::durability::set_damage_for_oracle(&mut damaged, 5);
        assert!(damaged.is_same_item(&bare), "same sword id");
        assert_ne!(damaged, bare, "a durability hit changes the components");
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = damaged.clone();
        tick(&mut region);
        assert_eq!(region.players[&id].attack_strength_ticker, 8);

        // 附魔：换成带锋利的同一把剑，仍是同物品不同组件。
        // Enchantment: the same sword with Sharpness, still same item, new components.
        let sharp = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:sharpness",
            1,
        );
        assert!(sharp.is_same_item(&damaged));
        assert_ne!(sharp, damaged);
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = sharp;
        tick(&mut region);
        assert_eq!(region.players[&id].attack_strength_ticker, 9);

        // 换成另一种物品才真的重置冷却。
        // Only swapping to a different item resets the cooldown.
        region.players.get_mut(&id).unwrap().inventory.slots[slot] =
            stack(mc_bamboo_data::items::DIAMOND_AXE);
        tick(&mut region);
        assert_eq!(region.players[&id].attack_strength_ticker, 0);
    }

    /// 官方 `Player.causeExtraKnockback` 在 `knockbackAmount > 0` 的命中之后
    /// 把攻击者水平速度 ×0.6 并 `setSprinting(false)`。
    ///
    /// Vanilla `Player.causeExtraKnockback` damps the attacker's horizontal
    /// velocity by 0.6 and calls `setSprinting(false)` after a hit whose
    /// `knockbackAmount > 0`.
    #[test]
    fn extra_knockback_damps_the_attacker_and_stops_sprint() {
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
        player.motion.delta = Vec3::new(1.0, 0.0, -0.5);
        region.insert_player(player);
        entity_sync::set_sprinting(&mut region, id, true);
        assert!(region.players[&id].sync.is_sprinting());

        damp_attacker(&mut region, id);

        let player = &region.players[&id];
        assert!(!player.sync.is_sprinting(), "sprint must be cancelled");
        assert!((player.motion.delta.x - 0.6).abs() < 1.0e-9);
        assert_eq!(player.motion.delta.y, 0.0);
        assert!((player.motion.delta.z + 0.3).abs() < 1.0e-9);
    }

    /// `getAttributeValue(ENTITY_INTERACTION_RANGE)`：生存 3.0，创造再 +2.0。
    /// 攻击距离用它算，不是方块交互的 4.5。
    ///
    /// `getAttributeValue(ENTITY_INTERACTION_RANGE)`: 3.0 in survival and 3.0 +
    /// 2.0 in creative. Attack reach uses it, not the 4.5 block-interaction one.
    #[test]
    fn entity_interaction_range_follows_the_creative_modifier() {
        assert_eq!(entity_interaction_range(GameMode::Survival), 3.0);
        assert_eq!(entity_interaction_range(GameMode::Creative), 5.0);
        assert_eq!(entity_interaction_range(GameMode::Adventure), 3.0);
    }

    /// 没有 `ATTACK_RANGE` 组件的武器走 `AttackRange.defaultFor`：上限是实体
    /// 交互距离、缓冲 3.0，闭区间；创造模式用 `maxCreativeReach`。
    ///
    /// A weapon without an `ATTACK_RANGE` component uses
    /// `AttackRange.defaultFor`: the eye-to-box distance must lie in the closed
    /// `[−3.0, max + 3.0]` window, and creative uses `maxCreativeReach`.
    #[test]
    fn default_attack_range_uses_the_entity_interaction_range() {
        let empty = ItemStack::empty();
        let range = attack_range(&empty, GameMode::Survival);
        assert_eq!(range.max_reach, 3.0);
        assert_eq!(range.max_creative_reach, 3.0);
        assert_eq!(range.hitbox_margin, 0.0);

        // 眼睛高度 1.62、牛盒 0.9×1.4：D=6.4 时距离约 5.96（命中），
        // D=6.5 时约 6.06（超出 6.0）。
        //
        // Eye at 1.62 and a 0.9×1.4 cow box: D=6.4 gives about 5.96 (hit),
        // D=6.5 gives about 6.06 (past the 6.0 limit).
        let eye = Vec3::new(0.5, 65.62, 0.5);
        let near = entity_bounds(Vec3::new(6.9, 64.0, 0.5), (0.9, 1.4));
        let far = entity_bounds(Vec3::new(7.0, 64.0, 0.5), (0.9, 1.4));
        assert!(range.is_in_range(eye, near, ATTACK_RANGE_BUFFER, false));
        assert!(!range.is_in_range(eye, far, ATTACK_RANGE_BUFFER, false));

        // 创造模式上限 8.0：同一头牛在生存打不到，创造打得到。
        //
        // Creative max is 8.0, so the same cow is out of survival reach but
        // inside creative reach.
        let creative_range = attack_range(&empty, GameMode::Creative);
        assert!(creative_range.is_in_range(eye, far, ATTACK_RANGE_BUFFER, true));
    }

    /// 边界是闭区间：距离正好等于上限时仍然算命中（`isInRange` 用 `<=`）。
    ///
    /// The bounds are closed: a distance exactly at the limit still hits
    /// (`isInRange` uses `<=`).
    #[test]
    fn attack_range_bounds_are_inclusive() {
        let range = AttackRange {
            min_reach: 0.0,
            max_reach: 6.0,
            min_creative_reach: 0.0,
            max_creative_reach: 6.0,
            hitbox_margin: 0.0,
        };
        let eye = Vec3::new(0.5, 64.0, 0.5);
        // 盒子从 x=6.5 起，眼睛到盒子的距离正好 6.0。
        // The box starts at x=6.5, so the eye-to-box distance is exactly 6.0.
        let bounds = Aabb::new(6.5, 63.5, 0.0, 7.0, 65.0, 1.0);
        assert!(range.is_in_range(eye, bounds, 0.0, false));
        let past = Aabb::new(6.500_001, 63.5, 0.0, 7.0, 65.0, 1.0);
        assert!(!range.is_in_range(eye, past, 0.0, false));
    }
}
