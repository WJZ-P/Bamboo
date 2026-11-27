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
    CollisionContext, EntityContext, World,
    entity::{
        JavaRandom,
        arrow::{clip_entity, clip_including_border},
        mob::{DamageSource, effects::EffectInstance},
    },
};
use mc_bamboo_core::{Aabb, BlockPos, Hand, Vec3, mth};
use mc_bamboo_data::{
    SOUND_EVENTS, gamerules, items,
    registries::{attribute, data_component_type as kinds, mob_effect, particle_type},
    sounds,
    tags::item_tags,
};
use mc_bamboo_protocol::{
    codec::write_varint,
    components::{
        ComponentPatch, ComponentValue, cursor::Holder, defaults, value::KineticCondition,
    },
    item::ItemStack,
    packet::play::{
        Animate, EntityEvent, Explode, GameMode, LevelParticles, Randomization, SetEntityMotion,
        SwingAnimationType,
    },
};

use crate::{
    durability, entity_sync, inbound::PlayerId, interaction, item_use_stats, links::encode_frames,
    player::Player, player_exhaustion, player_hurt, region::Region,
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
/// `handleAttack` 传给 `isWithinAttackRange(mainHandItem, targetBounds, 3.0)` 的缓冲区。
///
/// The buffer `handleAttack` passes to
/// `isWithinAttackRange(mainHandItem, targetBounds, 3.0)`.
const ATTACK_RANGE_BUFFER: f64 = 3.0;

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

/// `getCurrentItemAttackStrengthDelay()` = `(float)(1.0 / getAttributeValue(ATTACK_SPEED) × 20)`。
///
/// `getCurrentItemAttackStrengthDelay()` = `(float)(1.0 / getAttributeValue(ATTACK_SPEED) × 20)`.
fn strength_delay(player: &Player) -> f32 {
    (1.0 / player.attributes.value(attribute::ATTACK_SPEED) * 20.0) as f32
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
    mc_bamboo_data::in_tag("minecraft:entity_type", name, entity_type)
}

/// `EnchantmentHelper.modifyDamage`：锋利无条件，亡灵杀手 / 节肢杀手 / 穿刺看
/// 目标类型。返回加在 `baseDamage` 上的总和，即
/// `getEnchantedDamage − baseDamage`。目标不是 `LivingEntity` 时（例如船）依然
/// 按目标实体类型过一遍条件，官方 `Player.attack` 不会因为目标不是生物而跳过
/// `magicBoost`。
///
/// `EnchantmentHelper.modifyDamage`: Sharpness is unconditional; Smite, Bane of
/// Arthropods and Impaling depend on the target type. Returns the sum added to
/// `baseDamage`. A non-living target such as a boat still goes through the same
/// target-type conditions: vanilla `Player.attack` does not skip `magicBoost`
/// for entities that are not `LivingEntity`.
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
    // 26.3 `Enchantments.bootstrap` 给穿刺注册了 `DAMAGE` 组件，条件是
    // `THIS_ENTITY` 在 `sensitive_to_impaling` 里，线性每级 2.5。船不在这个标签里。
    //
    // 26.3 `Enchantments.bootstrap` registers Impaling's `DAMAGE` effect with a
    // `THIS_ENTITY` predicate on `sensitive_to_impaling`, 2.5 per level.
    if entity_in_tag("minecraft:sensitive_to_impaling", entity_type) {
        let impaling = enchantment_level(held, "minecraft:impaling");
        if impaling > 0 {
            bonus += 2.5 * impaling as f32;
        }
    }
    bonus
}

/// `EnchantmentHelper.modifyKnockback` 的加成 + `(float) getAttributeValue(ATTACK_KNOCKBACK)`，再 `/ 2`。
///
/// `EnchantmentHelper.modifyKnockback` bonus plus `(float) getAttributeValue(ATTACK_KNOCKBACK)`, then `/ 2`.
fn attack_knockback_amount(player: &Player, held: &ItemStack) -> f32 {
    let knockback = enchantment_level(held, "minecraft:knockback") as f32;
    (player.attributes.value(attribute::ATTACK_KNOCKBACK) as f32 + knockback) / 2.0
}

/// `(float) getAttributeValue(SWEEPING_DAMAGE_RATIO)`（`sweeping_edge` 附魔的
/// `Fraction(L, L+1)` 修饰符已经由属性表提供）。
///
/// `(float) getAttributeValue(SWEEPING_DAMAGE_RATIO)`; `sweeping_edge`'s
/// `Fraction(L, L+1)` modifier lives in the player attribute map.
fn sweeping_ratio(player: &Player) -> f32 {
    player.attributes.value(attribute::SWEEPING_DAMAGE_RATIO) as f32
}

/// `Player.canCriticalAttack`：下落中、不着地、不攀爬、不在水里、没有失明、
/// 没骑乘、非疾跑，且目标是生物。
///
/// `Player.canCriticalAttack`: falling, not on ground / climbable / in water /
/// blinded / riding / sprinting, and the target is living.
fn can_critical(
    player: &Player,
    blocks: &crate::player_move::PlayerBlocks<'_>,
    target_is_living: bool,
) -> bool {
    player.motion.fall_distance > 0.0
        && !player.on_ground
        && !crate::player_glide::on_climbable(player, blocks)
        // `Entity.isInWater()` 就是 `wasTouchingWater`：碰到水的流体高度。
        // `Entity.isInWater()` is `wasTouchingWater`: touching water fluid.
        && !crate::player_move::touches_fluid(
            player,
            blocks,
            mc_bamboo_data::tags::fluid_tags::WATER,
        )
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
    // 官方 `Player.getSpeed()` 读 `(float) getAttributeValue(MOVEMENT_SPEED)`，
    // 先 `f2d` 再乘 2.5；`Mth.square(D)` 和 `Vec3.horizontalDistanceSqr()` 都是 double。
    //
    // Vanilla's `Player.getSpeed()` reads `(float) getAttributeValue(MOVEMENT_SPEED)`,
    // widened with `f2d` before the ×2.5; both `Mth.square(D)` and
    // `Vec3.horizontalDistanceSqr()` are doubles.
    let speed = f64::from(player.attributes.value(attribute::MOVEMENT_SPEED) as f32);
    let max = speed * 2.5;
    let motion = player.motion.known_movement;
    let horizontal = motion.x * motion.x + motion.z * motion.z;
    if horizontal >= max * max {
        return false;
    }
    mc_bamboo_blocks::state::item_is_tagged(held.item, item_tags::SWORDS)
}

/// `MaceItem.SMASH_ATTACK_FALL_THRESHOLD`：摔落超过 1.5 格才算下砸。
///
/// `MaceItem.SMASH_ATTACK_FALL_THRESHOLD`: over 1.5 blocks of fall is a smash.
const SMASH_FALL_THRESHOLD: f64 = 1.5;
/// `MaceItem.SMASH_ATTACK_HEAVY_THRESHOLD`：重落地音和范围击退倍率的分界。
///
/// `MaceItem.SMASH_ATTACK_HEAVY_THRESHOLD`: heavy landing sound / knockback multiplier.
const SMASH_HEAVY_THRESHOLD: f64 = 5.0;
/// `MaceItem.SMASH_ATTACK_KNOCKBACK_RADIUS`。
const SMASH_KNOCKBACK_RADIUS: f64 = 3.5;
/// `MaceItem.SMASH_ATTACK_KNOCKBACK_POWER` = `0.7F`。字节码里的
/// `0.699999988079071d` 是 float 0.7 宽化成的 double，不是 double 0.7。
///
/// `MaceItem.SMASH_ATTACK_KNOCKBACK_POWER` = `0.7F`. The bytecode's
/// `0.699999988079071d` is the float 0.7 widened to double, not double 0.7.
const SMASH_KNOCKBACK_POWER: f64 = 0.699_999_988_079_071;
/// `MaceItem.hurtEnemy` 里 `setDeltaMovement(delta.with(Y, 0.009999999776482582))` 的
/// 下砸竖直速度。
///
/// The `0.009999999776482582` vertical velocity `MaceItem.hurtEnemy` sets.
const SMASH_IMPULSE_Y: f64 = 0.009_999_999_776_482_582;

/// `MaceItem.canSmashAttack`：摔落超过 1.5 格且没有滑翔。
/// `MaceItem.canSmashAttack`: falling more than 1.5 blocks without gliding.
fn can_smash_attack(player: &Player) -> bool {
    player.motion.fall_distance > SMASH_FALL_THRESHOLD && !player.sync.is_fall_flying()
}

/// 风爆 `wind_burst` 的击退倍率：`knockback_multiplier` lookup `[1.2, 1.75, 2.2]`，
/// 超出等级走 linear `base 1.5 / +0.35`。没有风爆时返回 `None`。
///
/// The wind burst knockback multiplier: the `[1.2, 1.75, 2.2]` lookup with a
/// `base 1.5 / +0.35` linear fallback; `None` without the enchantment.
fn wind_burst_multiplier(held: &ItemStack) -> Option<f32> {
    match enchantment_level(held, "minecraft:wind_burst").max(0) {
        0 => None,
        1 => Some(1.2),
        2 => Some(1.75),
        3 => Some(2.2),
        level => Some(1.5 + 0.35 * (level - 1) as f32),
    }
}

/// `ServerLevel.explode` 给攻击者自己发一份 `ClientboundExplodePacket`：风爆的
/// gust 大粒子和 `wind_burst` 音效，`block_particles` 空（不破坏方块）。
///
/// `ServerLevel.explode` sends the attacker its own `ClientboundExplodePacket`
/// with the gust particle and `wind_burst` sound; no block particles.
fn send_wind_burst_explode(
    region: &mut Region,
    id: PlayerId,
    center: Vec3,
    radius: f32,
    block_count: i32,
    knock: Option<Vec3>,
) {
    let packet = Explode {
        center,
        radius,
        block_count,
        player_knockback: knock,
        explosion_particle: particle_type::GUST_EMITTER_LARGE,
        explosion_sound: i32::from(sounds::sound_event(
            "minecraft:entity.wind_charge.wind_burst",
        )),
        block_particles: &[],
        play_sound: true,
    };
    region.links.send_frames(id, &encode_frames(&packet));
}

/// `MaceItem.getAttackDamageBonus` 的分段摔落加伤（不含致密）：
/// `fall<=3 → 4·fall`，`3<fall<=8 → 12 + 2·(fall−3)`，`fall>8 → 22 + fall − 8`。
///
/// The piecewise fall bonus of `MaceItem.getAttackDamageBonus`, without Density:
/// `fall<=3 → 4·fall`, `3<fall<=8 → 12 + 2·(fall−3)`, `fall>8 → 22 + fall − 8`.
fn mace_smash_damage(fall: f64) -> f64 {
    if fall <= 3.0 {
        4.0 * fall
    } else if fall <= 8.0 {
        12.0 + 2.0 * (fall - 3.0)
    } else {
        22.0 + fall - 8.0
    }
}

/// 致密 `smash_damage_per_fallen_block`：`add linear base 0.5 / +0.5`，即每级 0.5。
///
/// Density's `smash_damage_per_fallen_block` is `add linear base 0.5 / +0.5`,
/// so 0.5 per level.
fn density_bonus(held: &ItemStack) -> f32 {
    0.5 * enchantment_level(held, "minecraft:density").max(0) as f32
}

/// `MaceItem.getAttackDamageBonus`：下砸分段加伤 + 致密 × 摔落距离。
/// 非重锤、或没在下砸时返回 0。
///
/// `MaceItem.getAttackDamageBonus`: the smash bonus plus Density × fall distance.
/// Returns 0 for a non-mace, or when not smashing.
fn mace_attack_damage_bonus(held: &ItemStack, fall: f64) -> f32 {
    if held.item != items::MACE || fall <= SMASH_FALL_THRESHOLD {
        return 0.0;
    }
    let density = f64::from(density_bonus(held));
    (mace_smash_damage(fall) + density * fall) as f32
}

/// 重锤范围击退的一个候选：主目标碰撞盒 inflate(3.5) 内的其它生物或玩家。
///
/// One mace area-knockback candidate: another mob or player inside the main
/// target's box inflated by 3.5.
struct MaceTarget {
    id: i32,
    position: Vec3,
    knockback_resistance: f64,
    is_player: bool,
}

/// `MaceItem.knockbackPredicate` 里的候选收集：主目标碰撞盒 inflate(3.5) 与
/// 候选碰撞盒相交、距主目标 ≤ 3.5 的其它 `LivingEntity`（玩家也算，攻击者和
/// 主目标不算）。和攻击者同队的、攻击者自己驯服的、以及主目标驯服的宠物都不
/// 算（`isAlliedTo` 和 `isOwnedBy(target)`）；标记盔甲架 / 旁观者 / 飞行中的
/// 创造玩家同样不在候选里（旁观者和创造飞行玩家由攻击路径排除，标记盔甲架已建模并排除）。
/// 玩家排在生物之后（玩家不在实体表里）。
///
/// The `MaceItem.knockbackPredicate` candidates: other `LivingEntity`s whose box
/// intersects the target's inflated by 3.5 and that are within 3.5 of the target
/// (players included, attacker and main target excluded). Teammates, the attacker's
/// own pets and pets owned by the main target are excluded (`isAlliedTo` and
/// `isOwnedBy(target)`); spectators, marker armour stands and creative flying
/// players are excluded too.
/// 候选生物的有效队伍下标：先看自己所在的队；`TamableAnimal.getTeam()` 覆写——
/// 驯服动物自己没队时返回主人的队，这里也回退到主人（玩家）的队伍。
///
/// The candidate mob's effective team index: its own team first; `TamableAnimal.getTeam()`
/// falls back to the root owner's team when a tamed mob has none, mirrored here.
fn mob_team_index(
    region: &Region,
    mob: &mc_bamboo_blocks::entity::mob::MobPublic,
    tame: bool,
) -> Option<u32> {
    if let Some(team) = region.global.scoreboard.entity_team_index(&mob.uuid) {
        return Some(team);
    }
    if !tame {
        return None;
    }
    let owner = mob.owner?;
    region
        .players
        .values()
        .find(|player| *player.uuid.as_bytes() == owner)
        .and_then(|player| region.global.scoreboard.team_index_of(&player.name))
}

fn collect_mace_targets(
    region: &Region,
    attacker: PlayerId,
    main_target: i32,
    target: &TargetInfo,
) -> Vec<MaceTarget> {
    let area = entity_bounds(target.position, target.dimensions).inflate(
        SMASH_KNOCKBACK_RADIUS,
        SMASH_KNOCKBACK_RADIUS,
        SMASH_KNOCKBACK_RADIUS,
    );
    let radius_sq = SMASH_KNOCKBACK_RADIUS * SMASH_KNOCKBACK_RADIUS;
    let within = |position: Vec3| position.distance_to_sqr(target.position) <= radius_sq;
    // `attacker.isAlliedTo(candidate)` 里的两份事实：攻击者的队伍和 UUID。
    //
    // The attacker's team and UUID, the two facts behind `attacker.isAlliedTo(candidate)`.
    let attacker_player = region.players.get(&attacker);
    let attacker_team =
        attacker_player.and_then(|player| region.global.scoreboard.team_index_of(&player.name));
    let attacker_uuid = attacker_player.map(|player| *player.uuid.as_bytes());
    let mut out: Vec<MaceTarget> = region
        .entities
        .iter_public()
        .filter(|public| {
            let bounds = entity_bounds(public.position, public.kind.dimensions());
            let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind else {
                return false;
            };
            if public.removed
                || public.uid == main_target
                || public.uid == attacker.0
                || !area.intersects_coords(
                    bounds.min_x,
                    bounds.min_y,
                    bounds.min_z,
                    bounds.max_x,
                    bounds.max_y,
                    bounds.max_z,
                )
                || !within(public.position)
            {
                return false;
            }
            // `TamableAnimal.isTame()`：驯服位。
            let tame = mob.tame_flags & mc_bamboo_blocks::entity::mob::tame::FLAG_TAME != 0;
            // `attacker.isAlliedTo(candidate)`：同队，或候选是攻击者驯服的宠物
            // （`TamableAnimal.considersEntityAsAlly` 认主人）。
            //
            // `attacker.isAlliedTo(candidate)`: same team, or a pet the attacker owns
            // (`TamableAnimal.considersEntityAsAlly` treats its owner as an ally).
            // 队伍继承 `TamableAnimal.getTeam()`：候选自己没队时，驯服动物用主人的队。
            let candidate_team = mob_team_index(region, mob, tame);
            let allied = (attacker_team.is_some() && candidate_team == attacker_team)
                || (tame && mob.owner == attacker_uuid);
            // `isOwnedBy(target)`：候选是主目标驯服的宠物。
            let owned_by_target = tame && mob.owner == target.uuid;
            !allied && !owned_by_target
        })
        .map(|public| {
            let resistance = match &public.kind {
                mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                    mob.attributes.value(attribute::KNOCKBACK_RESISTANCE)
                }
                _ => 0.0,
            };
            MaceTarget {
                id: public.uid,
                position: public.position,
                knockback_resistance: resistance,
                is_player: false,
            }
        })
        .collect();
    for player in region.players.values() {
        if player.id == attacker || player.id.0 == main_target {
            continue;
        }
        // `attacker.isAlliedTo(player)`：玩家不是宠物，只比队伍。
        let candidate_team = region.global.scoreboard.team_index_of(&player.name);
        if attacker_team.is_some() && candidate_team == attacker_team {
            continue;
        }
        let bounds = player.bounding_box();
        let position = player.position();
        if !area.intersects_coords(
            bounds.min_x,
            bounds.min_y,
            bounds.min_z,
            bounds.max_x,
            bounds.max_y,
            bounds.max_z,
        ) || !within(position)
        {
            continue;
        }
        out.push(MaceTarget {
            id: player.id.0,
            position,
            knockback_resistance: player.attributes.value(attribute::KNOCKBACK_RESISTANCE),
            is_player: true,
        });
    }
    out
}

/// `MaceItem.hurtEnemy` 的下砸收尾：范围击退和落地音效。在 `land_attack` 命中
/// 之后、`post_attack_effects` 之前调用，对应官方 `itemAttackInteraction` 里
/// `hurtEnemy` 的位置。
///
/// `MaceItem.hurtEnemy`'s smash tail: area knockback and the landing sound, called
/// after a landed hit but before `post_attack_effects`.
fn mace_smash_effects(world: &mut World<'_>, plan: &AttackPlan) {
    // `levelEvent(2013, target.getOnPos(), 750)`：地面撞击事件。
    let event_pos = BlockPos::containing(plan.target_position);
    world.level_event(2013, event_pos, 750);
    // 落地音效三档：目标着地且摔落 > 5 格重音，否则普通落地音；空中是挥空音。
    let fall = plan.smash_fall_distance;
    let name = if plan.target_on_ground {
        if fall > SMASH_HEAVY_THRESHOLD {
            "minecraft:item.mace.smash_ground_heavy"
        } else {
            "minecraft:item.mace.smash_ground"
        }
    } else {
        "minecraft:item.mace.smash_air"
    };
    // `ServerLevel.playSound(null, attacker.x/y/z, sound, attacker.getSoundSource(), 1, 1)`。
    push_sound(world, plan.position, name);
    // `MaceItem.getKnockbackPower`：`(3.5 − dist) · 0.7 · (fall>5 ? 2 : 1) · (1 − 击退抗性)`，
    // 其中 `dist` 是候选到**主目标**的距离（`candidate − target`），乘数是攻击者的摔落。
    let heavy = if fall > SMASH_HEAVY_THRESHOLD {
        2.0
    } else {
        1.0
    };
    for candidate in &plan.mace_targets {
        let vector = candidate.position.subtract(plan.target_position);
        let dist = vector.length();
        let power = (SMASH_KNOCKBACK_RADIUS - dist)
            * SMASH_KNOCKBACK_POWER
            * heavy
            * (1.0 - candidate.knockback_resistance);
        if power <= 0.0 {
            continue;
        }
        let normalized = vector.normalize();
        let push = Vec3::new(
            normalized.x * power,
            SMASH_KNOCKBACK_POWER,
            normalized.z * power,
        );
        if candidate.is_player {
            world.push_player(candidate.id, push);
        } else {
            world.push_mob(candidate.id, push);
        }
    }
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
    on_ground: bool,
    /// 目标的 UUID，重锤范围击退的 `isOwnedBy(target)` 用它比候选宠物。
    ///
    /// The target's UUID, compared by the mace area knockback's `isOwnedBy(target)`.
    uuid: Option<[u8; 16]>,
}

fn mob_target(public: &mc_bamboo_blocks::entity::EntityPublic) -> TargetInfo {
    let is_living = matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_));
    let health = match &public.kind {
        mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.health,
        _ => 0.0,
    };
    let uuid = match &public.kind {
        mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.uuid),
        _ => None,
    };
    TargetInfo {
        position: public.position,
        entity_type: public.kind.entity_type(),
        is_living,
        health,
        dimensions: public.kind.dimensions(),
        on_ground: public.on_ground,
        uuid,
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
    /// 自旋攻击用 `autoSpinAttackDmg` 覆盖属性攻击力；普通攻击为 `None`。
    ///
    /// An auto-spin attack overrides the attribute damage with
    /// `autoSpinAttackDmg`; `None` for an ordinary attack.
    base_damage_override: Option<f32>,
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
    sweeping_ratio: f32,
    source: DamageSource,
    knockback_sound: Option<&'static str>,
    sweep_targets: Vec<SweepTarget>,
    /// 重锤下砸：`canSmashAttack` 为真。
    smash: bool,
    smash_fall_distance: f64,
    /// 主目标这一刻是否着地（`MaceItem.hurtEnemy` 选落地音效读它）。
    target_on_ground: bool,
    /// 主目标位置，`levelEvent` 的 `getOnPos` 用它。
    target_position: Vec3,
    /// 范围击退的候选，见 [`collect_mace_targets`]。
    mace_targets: Vec<MaceTarget>,
}

/// `ServerGamePacketListenerImpl.handleAttack` + `Player.attack` 的入口。
///
/// Entry point for `ServerGamePacketListenerImpl.handleAttack` + `Player.attack`.
/// 客户端左键攻击实体：官方 `ServerGamePacketListenerImpl.handleAttack`。
///
/// A client left-click attack, vanilla `ServerGamePacketListenerImpl.handleAttack`.
/// `Player.attack` 对船（非生物目标）算出的 `f`，即 `getEnchantedDamage` 之后的
/// 真实攻击伤害：复用 `plan_attack` 的充能 / 武器 / 附魔 / 暴击计算，但不落地
/// Mob 命中。船和矿车这类载具的 `hurtServer` 用它累计 `damage += f*10`。
///
/// The real attack damage `Player.attack` deals to a vehicle (boat or minecart):
/// reuse plan_attack's charge / weapon / enchantment / crit math without a mob hurt.
pub(crate) fn vehicle_attack_damage(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
) -> Option<f32> {
    let player = region.players.get(&id)?;
    let req = AttackRequest {
        id,
        entity_id,
        held: player.inventory.held(Hand::Main).clone(),
        ticker: player.attack_strength_ticker,
        position: player.position(),
        yaw: player.yaw,
        base_damage_override: None,
    };
    let public = region.entities.public_at(entity_id)?;
    let target = mob_target(public);
    let plan = plan_attack(region, &target, &req)?;
    Some(plan.total_damage)
}

pub(crate) fn on_attack(region: &mut Region, id: PlayerId, entity_id: i32) {
    attack_inner(region, id, entity_id, None);
}

/// `Player.doAutoAttackOnTouch` -> `Player.attack`：自旋攻击时官方用
/// `autoSpinAttackDmg` 作基础伤害，其余（附魔、充能、暴击、击退、横扫、音效、
/// 统计）都走同一套 `Player.attack`。
///
/// `Player.doAutoAttackOnTouch` -> `Player.attack`: during an auto-spin attack
/// vanilla uses `autoSpinAttackDmg` as the base damage; everything else runs the
/// same `Player.attack` path.
pub(crate) fn on_attack_with_base(region: &mut Region, id: PlayerId, entity_id: i32, base: f32) {
    attack_inner(region, id, entity_id, Some(base));
}

fn attack_inner(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    base_damage_override: Option<f32>,
) {
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
    let entity_reach = player.attributes.value(attribute::ENTITY_INTERACTION_RANGE) as f32;
    let eye = Vec3::new(position.x, position.y + eye_height, position.z);
    if try_attack_hanging(region, id, entity_id, eye, game_mode, entity_reach, &held) {
        return;
    }
    let Some(target) = resolve_target(region, id, entity_id, eye, game_mode, entity_reach, &held)
    else {
        return;
    };
    let request = AttackRequest {
        id,
        entity_id,
        held,
        ticker,
        position,
        yaw,
        base_damage_override,
    };
    let Some(plan) = plan_attack(region, &target, &request) else {
        return;
    };
    let (was_hurt, actual_damage, wind_burst) = land_attack(region, id, entity_id, &target, &plan);
    if was_hurt {
        // 官方 `PlayerHurtEntityTrigger` / `KilledTrigger`（玩家攻击实体）。
        let kind = region
            .entities
            .public_at(entity_id)
            .map(|public| format!("{:?}", public.kind))
            .unwrap_or_default();
        let payload = serde_json::json!({
            "entity": {"type": kind},
            "damage": actual_damage,
        });
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:player_hurt_entity",
            payload.clone(),
        );
        crate::advancement_triggers::record(region, id, "minecraft:player_killed_entity", payload);
        finish_hit(
            region,
            id,
            entity_id,
            &target,
            &plan,
            actual_damage,
            wind_burst,
        );
    }
    // `PlayerInteractTrigger`：玩家右键实体在 `interaction::on_interact_entity` 里报，
    // 这里是左键攻击，报 `any_block_use` 之外的交互事件。
    crate::advancement_triggers::record(
        region,
        id,
        "minecraft:player_interacted_with_entity",
        serde_json::json!({"entity": {"type": ""}}),
    );
}

/// `BlockAttachedEntity.skipAttackInteraction`：玩家左键画 / 物品展示框时直接走
/// `hurtServer`，不触发武器横扫、暴击、耐久和饥饿消耗。返回 `true` 表示已由悬挂实体接住。
///
/// `BlockAttachedEntity.skipAttackInteraction`: left-clicking a painting or item frame
/// runs `hurtServer` directly without sweep, crits, durability loss or exhaustion.
fn try_attack_hanging(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    eye: Vec3,
    game_mode: GameMode,
    entity_reach: f32,
    held: &ItemStack,
) -> bool {
    let Some(public) = region.entities.public_at(entity_id) else {
        return false;
    };
    let attach_pos = match &public.kind {
        mc_bamboo_blocks::entity::KindPublic::Painting(p) => p.pos,
        mc_bamboo_blocks::entity::KindPublic::ItemFrame(f) => f.pos,
        _ => return false,
    };
    if public.removed {
        return true;
    }
    let creative = game_mode == GameMode::Creative;
    let range = attack_range(held, entity_reach);
    if !region
        .global
        .world_border
        .is_within_bounds_pos(BlockPos::containing(public.position))
        || !region.global.world_border.is_within_bounds_pos(attach_pos)
        || !range.is_in_range(eye, public.bounding_box(), ATTACK_RANGE_BUFFER, creative)
    {
        return true;
    }
    let (hurt, effects) = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let hurt = world.hurt_hanging(
            entity_id,
            mc_bamboo_blocks::entity::hanging::HangingHurt::player(creative),
        );
        (hurt, world.into_effects())
    };
    interaction::send_effects(region, Some(id), &effects);
    if hurt {
        entity_sync::broadcast_swing(region, id, Hand::Main);
    }
    true
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
    entity_reach: f32,
    held: &ItemStack,
) -> Option<TargetInfo> {
    let range = attack_range(held, entity_reach);
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
    let bounds = other.bounding_box();
    range
        .is_in_range(eye, bounds, ATTACK_RANGE_BUFFER, creative)
        .then(|| TargetInfo {
            position: other.position(),
            entity_type: mc_bamboo_data::registries::entity_type::PLAYER,
            is_living: true,
            health: other.health.health,
            // 蹲下时玩家只有 1.5 高，横扫盒和伤害指示粒子都读这个尺寸，
            // 所以从真正的包围盒取，不能写死站立尺寸。
            //
            // A crouching player is only 1.5 tall; both the sweep box and the
            // damage-indicator particle read this size, so take it from the real
            // bounding box instead of hard-coding the standing size.
            dimensions: (
                (bounds.max_x - bounds.min_x) as f32,
                (bounds.max_y - bounds.min_y) as f32,
            ),
            on_ground: other.on_ground,
            uuid: Some(*other.uuid.as_bytes()),
        })
}

/// 官方 `AttackRange` 里玩家攻击者读得到的几项。`mob_factor` 只对非玩家
/// 攻击者生效，玩家攻击永远走 `min/minCreative` / `max/maxCreative`。
///
/// The `AttackRange` fields a player attacker reads. `mob_factor` only applies
/// to non-player attackers, so players always use `min/minCreative` and
/// `max/maxCreative`.
#[derive(Debug, Clone, Copy, PartialEq)]
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
fn attack_range(held: &ItemStack, entity_reach: f32) -> AttackRange {
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
    AttackRange::default_for(entity_reach)
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
    let (id, entity_id, held, ticker, position, yaw, base_damage_override) = (
        req.id,
        req.entity_id,
        &req.held,
        req.ticker,
        req.position,
        req.yaw,
        req.base_damage_override,
    );
    let delay = strength_delay(&region.players[&id]);
    if has_piercing_weapon(held) || cannot_attack_with_item(ticker, held, delay) {
        return None;
    }
    let s = strength_scale(ticker, delay);
    // `Item.getAttackDamageBonus` 只有重锤覆写（下砸加伤），其余物品（含剑）默认
    // 返回 0。它在 `baseDamageScaleFactor` 之后、暴击之前加，不乘 `s`。
    //
    // Only `MaceItem` overrides `Item.getAttackDamageBonus` (smash damage);
    // every other item (swords included) returns 0. It is added after
    // `baseDamageScaleFactor` and before the crit, and is not scaled by `s`.
    let base_damage_0 = base_damage_override.unwrap_or_else(|| {
        region.players[&id]
            .attributes
            .value(attribute::ATTACK_DAMAGE) as f32
    });
    let magic_boost = s * damage_bonus(held, target.entity_type);
    let mut base_damage = base_damage_0 * (BASE_SCALE_MIN + s * s * BASE_SCALE_SPAN);
    // `onAttack()` → `resetOnlyAttackStrengthTicker()`。
    if let Some(player) = region.players.get_mut(&id) {
        player.attack_strength_ticker = 0;
    }
    // `Player.attack` 在 `baseDamage > 0 || magicBoost > 0` 之外整段跳过：不 hurt、
    // 不发包，连 `nodamage` 音都不播。正常路径 `ATTACK_DAMAGE` 基值 1.0 让
    // `baseDamage >= 0.2`，这道门槛只在伤害被减到 0 时生效。
    //
    // `Player.attack` skips everything outside `baseDamage > 0 || magicBoost > 0`:
    // no hurt, no packets, not even the `nodamage` sound. The base
    // `ATTACK_DAMAGE` of 1.0 keeps `baseDamage >= 0.2`, so this only matters when
    // damage is reduced to zero.
    if base_damage <= 0.0 && magic_boost <= 0.0 {
        return None;
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
    let extra_knockback =
        attack_knockback_amount(player, held) + if knockback_attack { 0.5 } else { 0.0 };
    // `weapon.getItem().getAttackDamageBonus(target, baseDamage, damageSource)`：
    // 重锤下砸加伤（含致密），在暴击前加。
    let smash = can_smash_attack(player);
    let smash_fall_distance = player.motion.fall_distance;
    if smash {
        base_damage += mace_attack_damage_bonus(held, smash_fall_distance);
    }
    let critical = full && {
        let blocks = crate::player_move::PlayerBlocks::new(&region.chunks, &region.block_entities);
        can_critical(player, &blocks, target.is_living)
    };
    if critical {
        base_damage *= CRIT_MULTIPLIER;
    }
    let total_damage = base_damage + magic_boost;
    let sweep = is_sweep_attack(player, held, full, critical, knockback_attack);
    let sweep_ratio = sweeping_ratio(player);
    // 横扫的候选来自实体表，要在借用 `block_level()` 之前取好。
    //
    // Sweep candidates come from the entity store, collected before `block_level()`.
    let sweep_targets = if sweep {
        collect_sweep_targets(region, id, entity_id, target, position)
    } else {
        Vec::new()
    };
    // 重锤范围击退候选同样要在借用 `block_level()` 之前取好。
    let mace_targets = if smash {
        collect_mace_targets(region, id, entity_id, target)
    } else {
        Vec::new()
    };
    let creative = player_is_creative(region, id);
    let mut source = DamageSource::player(id.0, position, creative);
    if !held.is_empty()
        && let Ok(weapon) = u16::try_from(held.item)
    {
        source = source.with_weapon(weapon);
    }
    // `MaceItem.getItemDamageSource`：下砸时伤害类型换成 `minecraft:mace_smash`。
    if smash {
        source.kind = mc_bamboo_blocks::entity::mob::DamageKind::Mace;
    }
    // 破甲附魔：`armor_effectiveness` 每级 -0.15，受击路径把它加进护甲减伤比例。
    let breach = enchantment_level(held, "minecraft:breach").max(0) as f32;
    if breach > 0.0 {
        source = source.with_armor_effectiveness(-0.15 * breach);
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
        sweeping_ratio: sweep_ratio,
        source,
        knockback_sound,
        sweep_targets,
        smash,
        smash_fall_distance,
        target_on_ground: target.on_ground,
        target_position: target.position,
        mace_targets,
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
) -> (bool, f32, Option<(Vec3, i32)>) {
    let hits_player = player_target(region, entity_id);
    // 官方在 `hurtOrSimulate` 之前记下目标的旧速度，最后用
    // `ServerPlayer.syncVelocity` 那一支把服务端速度改回去，只把新速度发给客户端。
    //
    // Vanilla captures the target's old velocity before `hurtOrSimulate` and,
    // through the `ServerPlayer.syncVelocity` branch, puts the server-side
    // velocity back afterwards; only the client is told the new one.
    let old_target_movement = hits_player
        .then(|| region.players.get(&PlayerId(entity_id)))
        .flatten()
        .map(|player| player.motion.delta);
    // 风爆把攻击者自己弹起来的那一下速度和爆炸方块数，先在这个块里算出来。
    let mut wind_burst_knock = None;
    let mut wind_burst_block_count = 0;
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
                plan.extra_knockback,
                plan.yaw,
                hits_player,
            );
            if plan.sweep {
                let spec = SweepSpec {
                    held: &plan.held,
                    base_damage: plan.base_damage,
                    sweeping_ratio: plan.sweeping_ratio,
                    s: plan.s,
                    player_position: plan.position,
                    creative: plan.source.creative_player,
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
            // `itemAttackInteraction` 里的 `hurtEnemy`：重锤下砸的范围击退和落地音。
            if plan.smash {
                mace_smash_effects(&mut world, plan);
            }
            // 风爆 `wind_burst`：post_attack 的 explode，对攻击者生效。需求是
            // `direct_attacker !is_flying && fall_distance >= 1.5`（下砸才有）。
            if plan.smash
                && let Some(multiplier) = wind_burst_multiplier(&plan.held)
            {
                let (block_count, hit_players) =
                    world.explode_wind_burst(plan.position, 3.5, multiplier);
                wind_burst_block_count = block_count;
                wind_burst_knock = hit_players
                    .iter()
                    .find(|(uid, _)| *uid == id.0)
                    .map(|(_, knock)| *knock);
            }
            post_attack_effects(
                &mut world,
                entity_id,
                &plan.held,
                target.entity_type,
                hits_player,
            );
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
    if hit
        && let Some(old) = old_target_movement
        && let Some(player) = region.players.get_mut(&PlayerId(entity_id))
    {
        player.motion.delta = old;
    }
    // 风爆把攻击者自己弹起来的那一下速度和爆炸包，交给 `finish_hit` 在
    // 重锤下砸冲量（`set_entity_motion`）之后发。
    (
        hit,
        damage_taken(region, entity_id, target.health),
        wind_burst_knock.map(|knock| (knock, wind_burst_block_count)),
    )
}

/// 命中之后：衰减攻击者速度 / 取消疾跑、暴击 / 附魔暴击动画、横扫 / 伤害指示
/// 粒子、`DAMAGE_DEALT` 统计、武器耐久、`lastHurtMob`、饥饿消耗和挥手。
///
/// After a hit: attacker velocity damp / sprint cancel, crit / magic-crit
/// animations, sweep / damage-indicator particles, the `DAMAGE_DEALT` stat,
/// weapon durability, `lastHurtMob`, exhaustion and the swing.
fn finish_hit(
    region: &mut Region,
    id: PlayerId,
    entity_id: i32,
    target: &TargetInfo,
    plan: &AttackPlan,
    actual_damage: f32,
    wind_burst: Option<(Vec3, i32)>,
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
    // `Player.damageStatsAndHearts`：先 `awardStat(DAMAGE_DEALT, round(实际伤害 × 10))`，
    // 再按 `actualDamage > 2` 发伤害指示粒子。统计只在会话内累计（不写 stats 文件）。
    //
    // `Player.damageStatsAndHearts`: first `awardStat(DAMAGE_DEALT, round(actual
    // damage × 10))`, then the damage-indicator particles when `actualDamage > 2`.
    // The stat accumulates for this session only (no stats file).
    item_use_stats::award_damage_dealt(region, id, (actual_damage * 10.0).round() as i32);
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
    // `itemAttackInteraction` → `ItemStack.hurtEnemy`：带 `WEAPON` 组件的
    // 武器除了扣耐久，还给攻击者记一次 `Stats.ITEM_USED.get(item)`。
    //
    // `itemAttackInteraction` → `ItemStack.hurtEnemy`: a weapon with the
    // `WEAPON` component also awards one `Stats.ITEM_USED.get(item)`.
    if weapon_component(&plan.held) {
        item_use_stats::award(region, id, plan.held.item);
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
    // `MaceItem.hurtEnemy` 的下砸竖直速度和 `postHurtEnemy` 的摔落清零：
    // 命中之后把攻击者的竖直速度设成下砸冲量、摔落距离归零，并像官方那样给
    // 攻击者自己补一个 `set_entity_motion`（`ServerPlayer` 的 `connection.send`）。
    if plan.smash
        && let Some(player) = region.players.get_mut(&id)
    {
        player.motion.delta.y = SMASH_IMPULSE_Y;
        player.motion.fall_distance = 0.0;
        let movement = player.motion.delta;
        let packet = encode_frames(&SetEntityMotion {
            entity_id: id.0,
            movement,
        });
        region.links.send_frames(id, &packet);
    }
    // 风爆 `wind_burst` 的 `pushFromExplosion` 在 `hurtEnemy` 之后：爆炸包
    // `connection.send` 给自己，然后只改服务端速度（客户端由爆炸包拿击退），
    // 所以 `set_entity_motion` 里还是下砸冲量。
    if let Some((knock, block_count)) = wind_burst {
        send_wind_burst_explode(region, id, plan.position, 3.5, block_count, Some(knock));
        if let Some(player) = region.players.get_mut(&id) {
            player.motion.delta = player.motion.delta.add(knock);
        }
    }
    player_exhaustion::add_in(region, id, ATTACK_EXHAUSTION);
    entity_sync::broadcast_swing(region, id, Hand::Main);
}

/// `ItemStack.has(DataComponents.WEAPON)`：官方 `hurtEnemy` 只有带这个组件的
/// 武器才记 `ITEM_USED`，空手 / 其它物品不记。
///
/// `ItemStack.has(DataComponents.WEAPON)`: vanilla `hurtEnemy` awards
/// `ITEM_USED` only for stacks with this component.
fn weapon_component(held: &ItemStack) -> bool {
    !held.is_empty() && defaults::resolve(held.item, &held.patch(), kinds::WEAPON).is_some()
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
    // `Entity.getY(0.5)` = `position.y + getBbHeight() * 0.5`：`getBbHeight` 是
    // float，这一点 `1.8F * 0.5` 的舍入必须保留，否则高度少一位、粒子位置对不上。
    //
    // `Entity.getY(0.5)` is `position.y + getBbHeight() * 0.5`; `getBbHeight` is
    // a float, so the rounding of `1.8F * 0.5` must be kept or the particle
    // position differs in the last bits.
    let box3d = player.bounding_box();
    let height = (box3d.max_y - box3d.min_y) as f32;
    player.position().y + f64::from(height * 0.5)
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
fn apply_extra_knockback(
    world: &mut World<'_>,
    entity_id: i32,
    amount: f32,
    yaw: f32,
    is_player: bool,
) {
    if amount <= 0.0 {
        return;
    }
    let radians = f64::from(yaw * (std::f32::consts::PI / 180.0));
    let dx = mth::sin(radians);
    let dz = -mth::cos(radians);
    if is_player {
        world.knock_back_player(entity_id, f64::from(amount), (f64::from(dx), f64::from(dz)));
    } else {
        world.knockback_mob(entity_id, f64::from(amount), f64::from(dx), f64::from(dz));
    }
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

/// 横扫的一个候选。
///
/// One sweeping candidate.
struct SweepTarget {
    id: i32,
    entity_type: i32,
    is_player: bool,
}

/// `Player.doSweepAttack` 的候选：主目标的整个碰撞盒 inflate(1, 0.25, 1) 与
/// 候选自己的碰撞盒相交、距玩家小于 3 格的其它 `LivingEntity`（玩家也算，
/// 攻击者和主目标不算）。玩家排在生物的后面，因为玩家不在实体表里。
///
/// `Player.doSweepAttack` candidates: other `LivingEntity`s whose own collision
/// box intersects the main target's box inflated by (1, 0.25, 1) and that are
/// within 3 blocks of the player (players included, the attacker and the main
/// target excluded). Players come after mobs because they are not in the entity
/// store.
fn collect_sweep_targets(
    region: &Region,
    attacker: PlayerId,
    main_target: i32,
    target: &TargetInfo,
    player_position: Vec3,
) -> Vec<SweepTarget> {
    let area = entity_bounds(target.position, target.dimensions).inflate(1.0, 0.25, 1.0);
    let within_reach = |position: Vec3| {
        (position.x - player_position.x).powi(2)
            + (position.y - player_position.y).powi(2)
            + (position.z - player_position.z).powi(2)
            < 9.0
    };
    let mut out: Vec<SweepTarget> = region
        .entities
        .iter_public()
        .filter(|public| {
            let bounds = entity_bounds(public.position, public.kind.dimensions());
            matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_))
                && !public.removed
                && public.uid != main_target
                && public.uid != attacker.0
                && area.intersects_coords(
                    bounds.min_x,
                    bounds.min_y,
                    bounds.min_z,
                    bounds.max_x,
                    bounds.max_y,
                    bounds.max_z,
                )
                && within_reach(public.position)
        })
        .map(|public| SweepTarget {
            id: public.uid,
            entity_type: public.kind.entity_type(),
            is_player: false,
        })
        .collect();
    // `getEntitiesOfClass(LivingEntity.class, ...)` 也包含玩家。官方没有队伍，
    // 所以除了攻击者本人没有别的排除。
    //
    // `getEntitiesOfClass(LivingEntity.class, ...)` includes players too. There
    // are no teams, so nothing but the attacker itself is excluded.
    for player in region.players.values() {
        let bounds = player.bounding_box();
        let position = player.position();
        if player.id == attacker
            || player.id.0 == main_target
            || !area.intersects_coords(
                bounds.min_x,
                bounds.min_y,
                bounds.min_z,
                bounds.max_x,
                bounds.max_y,
                bounds.max_z,
            )
            || !within_reach(position)
        {
            continue;
        }
        out.push(SweepTarget {
            id: player.id.0,
            entity_type: mc_bamboo_data::registries::entity_type::PLAYER,
            is_player: true,
        });
    }
    out
}

/// `do_sweep` 的固定参数。
///
/// The fixed parameters of `do_sweep`.
struct SweepSpec<'a> {
    held: &'a ItemStack,
    base_damage: f32,
    sweeping_ratio: f32,
    s: f32,
    player_position: Vec3,
    /// 官方 `doSweepAttack` 复用 `Player.attack` 的同一个 `damageSource`，
    /// 其 `isCreativePlayer()` 动态取攻击者能力；这里把主攻击那一份的
    /// `creative_player` 透传过来，创造模式攻击者才能绕过目标的受伤无敌帧。
    ///
    /// Vanilla `doSweepAttack` reuses the same `damageSource` from `Player.attack`,
    /// whose `isCreativePlayer()` reads the attacker's abilities dynamically; pass
    /// the main attack's `creative_player` through so a creative attacker bypasses
    /// the target's temporary invulnerability here too.
    creative: bool,
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
    nearby: &[SweepTarget],
    spec: &SweepSpec<'_>,
) {
    let sweep_base = 1.0 + spec.sweeping_ratio * spec.base_damage;
    let radians = f64::from(spec.yaw * (std::f32::consts::PI / 180.0));
    let dx = mth::sin(radians);
    let dz = -mth::cos(radians);
    for target in nearby {
        let enchanted = (sweep_base + damage_bonus(spec.held, target.entity_type)) * spec.s;
        let mut source = DamageSource::player(attacker.0, spec.player_position, spec.creative);
        if !spec.held.is_empty()
            && let Ok(weapon) = u16::try_from(spec.held.item)
        {
            source = source.with_weapon(weapon);
        }
        let hurt = if target.is_player {
            world.hurt_player_from_mob(target.id, source, enchanted, None)
        } else {
            world.hurt_mob(target.id, source, enchanted)
        };
        if hurt {
            if target.is_player {
                world.knock_back_player(
                    target.id,
                    f64::from(0.4_f32),
                    (f64::from(dx), f64::from(dz)),
                );
            } else {
                world.knockback_mob(target.id, f64::from(0.4_f32), f64::from(dx), f64::from(dz));
            }
            // 官方 `doSweepAttack` 对每个打中的 nearby 也调一次 `doPostAttackEffects`。
            // Vanilla `doSweepAttack` also calls `doPostAttackEffects` on each hurt nearby.
            post_attack_effects(
                world,
                target.id,
                spec.held,
                target.entity_type,
                target.is_player,
            );
        }
    }
}

/// `EnchantmentHelper.doPostAttackEffectsWithItemSource`：火焰附加点燃目标，
/// 节肢杀手给节肢目标挂缓慢 IV。
///
/// `EnchantmentHelper.doPostAttackEffectsWithItemSource`: Fire Aspect ignites
/// the target and Bane of Arthropods applies Slowness IV to arthropods.
fn post_attack_effects(
    world: &mut World<'_>,
    entity_id: i32,
    held: &ItemStack,
    entity_type: i32,
    is_player: bool,
) {
    let fire = enchantment_level(held, "minecraft:fire_aspect");
    if fire > 0 {
        // `Ignite(LevelBasedValue.perLevel(4.0F))` → 4×等级 秒 → ×20 刻。
        if is_player {
            world.ignite_player_from_mob(entity_id, fire * 80);
        } else {
            world.ignite_mob(entity_id, fire * 80);
        }
    }
    let bane = enchantment_level(held, "minecraft:bane_of_arthropods");
    if bane > 0 && entity_in_tag("minecraft:sensitive_to_bane_of_arthropods", entity_type) {
        let drawn = world.with_mob_random(entity_id, |random| bane_slowness(random, bane));
        if let Some((ticks, amplifier)) = drawn {
            world.add_mob_effect(
                entity_id,
                EffectInstance::with_amplifier(mob_effect::SLOWNESS, ticks, amplifier),
            );
        }
    }
}

/// `ApplyMobEffect` 给节肢杀手挂的缓慢 IV：三次取数都用受害者自己的随机源，
/// 顺序是 `getRandomElement`、时长、等级。单元素 `HolderSet` 也消费一次
/// `nextInt(1)`，已由 `HolderSet.Direct.getRandomElement` → `Util.getRandomSafe`
/// → `Util.getRandom`（`list.get(random.nextInt(list.size()))`）确认，不是推断；
/// 时长、等级各取一次 `Mth.randomBetween` 的 `nextFloat`。时长
/// `[1.5, 1.5 + 0.5×(L−1)]` 秒 ×20 取整，等级 `[3, 3]`。
///
/// The Slowness IV `ApplyMobEffect` applies for Bane of Arthropods: three draws
/// from the victim's own random, in vanilla order — `getRandomElement`, duration,
/// amplifier. A single-element `HolderSet` still consumes `nextInt(1)`, confirmed
/// by `HolderSet.Direct.getRandomElement` → `Util.getRandomSafe` →
/// `Util.getRandom` (`list.get(random.nextInt(list.size()))`), not inferred; the
/// duration and amplifier each take one `Mth.randomBetween` `nextFloat`. Duration
/// is `[1.5, 1.5 + 0.5×(L−1)]` seconds ×20, rounded; amplifier is 3.
fn bane_slowness(random: &mut JavaRandom, level: i32) -> (i32, i32) {
    let min_seconds = 1.5_f32;
    let max_seconds = 1.5 + 0.5 * (level - 1) as f32;
    let min_amplifier = 3.0_f32;
    let max_amplifier = 3.0_f32;
    let _ = random.next_int_bounded(1);
    let duration = random.next_float() * (max_seconds - min_seconds) + min_seconds;
    let amplifier = random.next_float() * (max_amplifier - min_amplifier) + min_amplifier;
    (
        (duration * 20.0).round() as i32,
        amplifier.round().max(0.0) as i32,
    )
}

// ===== 长矛穿刺（PiercingWeapon） =====

/// `piercing_weapon` 组件里读出来的几样。
struct PiercingSpec {
    deals_knockback: bool,
    _dismounts: bool,
    sound: Option<String>,
    hit_sound: Option<String>,
}

/// 一次穿刺命中的目标。
#[derive(Clone, Copy)]
struct SpearHit {
    id: i32,
    entity_type: i32,
    is_player: bool,
    /// 命中前的血量，`damageStatsAndHearts` 用它算 `DAMAGE_DEALT` 统计。
    health: f32,
}

/// 把 `sound` / `hit_sound` 的 `Holder` 换成注册表名字。
fn spear_sound_name(
    sound: &Option<Holder<mc_bamboo_protocol::components::value::SoundEvent>>,
) -> Option<String> {
    match sound {
        Some(Holder::Id(id)) => Some(SOUND_EVENTS[*id as usize].name.to_string()),
        Some(Holder::Inline(sound)) => Some(sound.location.clone()),
        None => None,
    }
}

/// `ItemStack.get(PIERCING_WEAPON)`。
fn piercing_spec(held: &ItemStack) -> Option<PiercingSpec> {
    if held.is_empty() {
        return None;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return None;
    };
    match defaults::resolve(held.item, &patch, kinds::PIERCING_WEAPON) {
        Some(ComponentValue::PiercingWeapon {
            deals_knockback,
            dismounts,
            sound,
            hit_sound,
        }) => Some(PiercingSpec {
            deals_knockback: *deals_knockback,
            _dismounts: *dismounts,
            sound: spear_sound_name(sound),
            hit_sound: spear_sound_name(hit_sound),
        }),
        _ => None,
    }
}

/// 长矛一次刺击的射线参数，`on_stab` 开头读玩家算好。
struct StabSetup {
    held: ItemStack,
    spec: PiercingSpec,
    position: Vec3,
    eye: Vec3,
    look: Vec3,
    min_point: Vec3,
    max_point: Vec3,
    margin: f32,
    creative: bool,
    yaw: f32,
    base_damage: f32,
    /// `attack_knockback_amount(player, held)`，第二个 `causeExtraKnockback` 的力道。
    knockback: f32,
    s: f32,
    crouching: bool,
    fall_distance: f64,
}

/// `PlayerActionKind::Stab` 的分发：`handlePlayerAction` 的 STAB 分支 →
/// `PiercingWeapon.attack(player, MAINHAND)`。
///
/// The `PlayerActionKind::Stab` dispatch: the STAB arm of `handlePlayerAction`
/// → `PiercingWeapon.attack(player, MAINHAND)`.
pub(crate) fn on_stab(region: &mut Region, id: PlayerId) {
    let Some(setup) = stab_setup(region, id) else {
        return;
    };
    let hits = spear_hits(region, &setup);
    let mut hit_targets = Vec::new();
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        for target in &hits {
            if stab_hit(&mut world, id, &setup, target).is_some() {
                hit_targets.push(*target);
            }
        }
        // `makeHitSound` / `makeSound`：命中才发 `hit_sound`，`sound` 恒发。
        if !hit_targets.is_empty()
            && let Some(name) = &setup.spec.hit_sound
        {
            push_sound(&mut world, setup.position, name);
        }
        if let Some(name) = &setup.spec.sound {
            push_sound(&mut world, setup.position, name);
        }
        world.drain_effects()
    };
    interaction::send_effects(region, Some(id), &effects);
    // `onAttack()`：重置攻击冷却。
    if let Some(player) = region.players.get_mut(&id) {
        player.attack_strength_ticker = 0;
    }
    // `itemAttackInteraction` 的耐久 / 统计 / `lastHurtMob` / 饥饿。
    finish_stab(region, id, &setup, &hit_targets);
    // `postPiercingAttack()`：突进附魔 `lunge` 的前冲、耐久、饥饿和音效。
    post_piercing_attack(region, id, &setup);
    // `swingAndResetAttackStrength(MAIN_HAND, swingAnim, false)`：`swingAnim` 是
    // 武器 `attack_animation` 组件给的动画（长矛是 `stab` + 对应时长），不是缺省挥手。
    let (animation, duration) = attack_swing(&setup.held);
    entity_sync::broadcast_swing_with(region, id, Hand::Main, animation, duration);
}

/// `ItemStack.getAttackAnimation()`：`attack_animation` 组件，没有时是
/// `SwingAnimation.DEFAULT`（`whack`、时长 6）。
///
/// `ItemStack.getAttackAnimation()`: the `attack_animation` component, else
/// `SwingAnimation.DEFAULT` (`whack`, duration 6).
fn attack_swing(held: &ItemStack) -> (SwingAnimationType, i32) {
    if !held.is_empty()
        && let Ok(patch) = ComponentPatch::from_opaque(held)
        && let Some(ComponentValue::AttackAnimation(swing)) =
            defaults::resolve(held.item, &patch, kinds::ATTACK_ANIMATION)
    {
        let animation = match swing.kind {
            2 => SwingAnimationType::Stab,
            _ => SwingAnimationType::Whack,
        };
        return (animation, swing.duration);
    }
    (SwingAnimationType::Whack, 6)
}

/// `handlePlayerAction` STAB 分支的读玩家部分，也把 `PiercingWeapon.attack`
/// 的 `baseDamage`、`range` 和射线参数算好。
fn stab_setup(region: &Region, id: PlayerId) -> Option<StabSetup> {
    let player = region.players.get(&id)?;
    if player.game_mode == GameMode::Spectator || player.spawn_pending.is_some() {
        return None;
    }
    let held = player.inventory.held(Hand::Main).clone();
    let delay = strength_delay(player);
    if !has_piercing_weapon(&held)
        || cannot_attack_with_item(player.attack_strength_ticker, &held, delay)
    {
        return None;
    }
    let spec = piercing_spec(&held)?;
    let position = player.position();
    let eye_height = player.eye_height();
    let eye = Vec3::new(position.x, position.y + eye_height, position.z);
    let look = Vec3::view_vector(player.yaw, player.pitch);
    let entity_reach = player.attributes.value(attribute::ENTITY_INTERACTION_RANGE) as f32;
    let range = attack_range(&held, entity_reach);
    let creative = player.game_mode == GameMode::Creative;
    let (min_reach, max_reach) = if creative {
        (range.min_creative_reach, range.max_creative_reach)
    } else {
        (range.min_reach, range.max_reach)
    };
    // `getHitEntitiesAlong` 的两端：`min_point` 是实体射线的终点，`max_point`
    // 是方块裁剪的终点（`effectiveMaxRange + max(0, look · knownMovement)`）。
    let movement = player.motion.known_movement;
    let speed = look.x * movement.x + look.y * movement.y + look.z * movement.z;
    let min_point = eye.add(look.scale(f64::from(min_reach)));
    let max_point = eye.add(look.scale(f64::from(max_reach) + speed.max(0.0)));
    let knockback = attack_knockback_amount(player, &held);
    Some(StabSetup {
        held,
        spec,
        position,
        eye,
        look,
        min_point,
        max_point,
        margin: range.hitbox_margin,
        creative,
        yaw: player.yaw,
        base_damage: player.attributes.value(attribute::ATTACK_DAMAGE) as f32,
        knockback,
        s: strength_scale(player.attack_strength_ticker, delay),
        crouching: player.sync.is_crouching(),
        fall_distance: player.motion.fall_distance,
    })
}

/// 长矛攻击者的 `CollisionContext.of(entity)`。
fn spear_collision_context(setup: &StabSetup) -> CollisionContext {
    CollisionContext::Entity(EntityContext {
        fall_distance: setup.fall_distance,
        bottom: setup.position.y,
        falling_block: false,
        descending: setup.crouching,
        placement: false,
        powder_snow_walkable: false,
    })
}

/// `ProjectileUtil.getManyEntityHitResult` 对单个候选包围盒的命中判定（含
/// `b8` 的「眼在盒内」和 `hitbox_margin` 的膨胀盒擦边分支）。
fn spear_clip_hit(
    view: &dyn mc_bamboo_blocks::entity::ctx::BlockView,
    bb: Aabb,
    from: Vec3,
    to: Vec3,
    context: CollisionContext,
    margin: f64,
) -> bool {
    if bb.contains(from.x, from.y, from.z) {
        return true;
    }
    if clip_entity(bb, from, to).is_some() {
        return true;
    }
    if margin <= 0.0 {
        return false;
    }
    let Some(outside) = clip_entity(bb.inflated(margin), from, to) else {
        return false;
    };
    let center = bb.center();
    let inside =
        clip_including_border(view, outside, center, context).map_or(center, |hit| hit.location);
    clip_entity(bb, outside, inside).is_some()
}

/// `ProjectileUtil.getHitEntitiesAlong`：先方块裁剪，再在 `min_point` 起、
/// `hitbox_margin` 厚、向 `max_point` 展开的搜索盒里收命中目标。候选在
/// `block_level` 借期之间收集，逐目标遮挡判定在借期里做。
fn spear_hits(region: &mut Region, setup: &StabSetup) -> Vec<SpearHit> {
    let margin = f64::from(setup.margin);
    // 方块裁剪：`to2` 是最远点，被方块截断时提前。
    let mut to2 = setup.max_point;
    {
        let context = spear_collision_context(setup);
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        if let Some(hit) = clip_including_border(&world, setup.eye, to2, context) {
            to2 = hit.location;
            if setup.eye.distance_to_sqr(to2) < setup.eye.distance_to_sqr(setup.min_point) {
                return Vec::new();
            }
        }
    }
    // 搜索盒：`AABB.ofSize(min_point, margin).expandTowards(to2 - min_point).inflate(1)`。
    let half = margin / 2.0;
    let area = Aabb::new(
        setup.min_point.x - half,
        setup.min_point.y - half,
        setup.min_point.z - half,
        setup.min_point.x + half,
        setup.min_point.y + half,
        setup.min_point.z + half,
    );
    let expand = to2.subtract(setup.min_point);
    let area = area
        .expanded_towards(expand.x, expand.y, expand.z)
        .inflated(1.0);
    // 候选收集（不借 `block_level`）。
    let mut candidates: Vec<(i32, Aabb, i32, bool, f32)> = Vec::new();
    for public in region.entities.iter_public() {
        let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind else {
            continue;
        };
        if public.removed || mob.health <= 0.0 {
            continue;
        }
        let bb = entity_bounds(public.position, public.kind.dimensions());
        if bb.intersects(area) {
            candidates.push((public.uid, bb, public.kind.entity_type(), false, mob.health));
        }
    }
    for player in region.players.values() {
        if player.id.0 == 0
            || player.game_mode == GameMode::Spectator
            || player.health.health <= 0.0
        {
            continue;
        }
        let bb = player.bounding_box();
        if bb.intersects(area) {
            candidates.push((
                player.id.0,
                bb,
                mc_bamboo_data::registries::entity_type::PLAYER,
                true,
                player.health.health,
            ));
        }
    }
    // 逐目标命中判定（借 `block_level` 做遮挡）。
    let context = spear_collision_context(setup);
    let mut hits = Vec::new();
    let (mut level, ticks) = region.block_level();
    let world = World::new(&mut level, ticks);
    for (uid, bb, entity_type, is_player, health) in candidates {
        if spear_clip_hit(&world, bb, setup.eye, setup.min_point, context, margin) {
            hits.push(SpearHit {
                id: uid,
                entity_type,
                is_player,
                health,
            });
        }
    }
    hits
}

/// 单目标 `Player.stabAttack`：伤害、两下击退和攻击后附魔效果。命中后的动画、
/// 耐久、统计和饥饿在 [`finish_stab`] 里统一收尾。
fn stab_hit(
    world: &mut World<'_>,
    attacker: PlayerId,
    setup: &StabSetup,
    target: &SpearHit,
) -> Option<f32> {
    let magic = damage_bonus(&setup.held, target.entity_type);
    // 长矛挥击不是「正在使用」，所以 `magicBoost *= s`、`base *= baseDamageScaleFactor`。
    let magic_boost = magic * setup.s;
    let base = setup.base_damage * (BASE_SCALE_MIN + setup.s * setup.s * BASE_SCALE_SPAN);
    let total = base + magic_boost;
    let mut source = DamageSource::player(attacker.0, setup.position, setup.creative);
    source.kind = mc_bamboo_blocks::entity::mob::DamageKind::Spear;
    if let Ok(weapon) = u16::try_from(setup.held.item) {
        source = source.with_weapon(weapon);
    }
    let breach = enchantment_level(&setup.held, "minecraft:breach").max(0) as f32;
    if breach > 0.0 {
        source = source.with_armor_effectiveness(-0.15 * breach);
    }
    let hit = if target.is_player {
        hurt_player_target(world, target.id, source, total)
    } else {
        world.hurt_mob(target.id, source, total)
    };
    if hit {
        if setup.spec.deals_knockback {
            // `causeExtraKnockback(0.4)` 再 `causeExtraKnockback(getKnockback)`。
            apply_extra_knockback(world, target.id, 0.4, setup.yaw, target.is_player);
            apply_extra_knockback(
                world,
                target.id,
                setup.knockback,
                setup.yaw,
                target.is_player,
            );
        }
        post_attack_effects(
            world,
            target.id,
            &setup.held,
            target.entity_type,
            target.is_player,
        );
        Some(magic_boost)
    } else {
        None
    }
}

/// `Player.stabAttack` 命中之后的收尾：附魔暴击动画、伤害统计、耐久、`lastHurtMob`
/// 和饥饿。每个命中目标各做一遍。
fn finish_stab(region: &mut Region, id: PlayerId, setup: &StabSetup, hits: &[SpearHit]) {
    for target in hits {
        let magic = damage_bonus(&setup.held, target.entity_type) * setup.s;
        if magic > 0.0 {
            send_animate(region, id, target.id, Animate::MAGIC_CRITICAL_HIT);
        }
        // `damageStatsAndHearts`：`DAMAGE_DEALT` 统计和伤害指示粒子。
        let actual_damage = damage_taken(region, target.id, target.health);
        item_use_stats::award_damage_dealt(region, id, (actual_damage * 10.0).round() as i32);
        // 耐久、`ITEM_USED` 统计、`lastHurtMob` 和饥饿按官方顺序落到主手武器上。
        if weapon_component(&setup.held) {
            item_use_stats::award(region, id, setup.held.item);
        }
        durability::run(region, id, Hand::Main, |region| {
            let (mut level, ticks) = region.block_level();
            let world = World::new(&mut level, ticks);
            mc_bamboo_blocks::durability::hurt_hand(&world, Hand::Main, 1);
        });
        if let Some(player) = region.players.get_mut(&id) {
            let now = player.entity_ticks;
            player.combat_memory.record_attack(target.id, now);
            player.health_sync = player_hurt::HealthSync::of(player);
        }
        player_exhaustion::add_in(region, id, ATTACK_EXHAUSTION);
    }
}

/// `LivingEntity.postPiercingAttack` → `EnchantmentHelper.doPostPiercingAttackEffects`：
/// 突进附魔 `lunge` 的 `post_piercing_attack` 效果（前冲、扣耐久、饥饿和音效）。
fn post_piercing_attack(region: &mut Region, id: PlayerId, setup: &StabSetup) {
    let lunge = enchantment_level(&setup.held, "minecraft:lunge").max(0);
    if lunge == 0 {
        return;
    }
    let mut random = {
        let Some(player) = region.players.get(&id) else {
            return;
        };
        // 突进要求没有骑乘、滑翔或浸水，且生存模式下饥饿至少为 7。
        // Lunge requires no mount, gliding or water, and at least 7 food in survival.
        let blocks = crate::player_move::PlayerBlocks::new(&region.chunks, &region.block_entities);
        if player.riding.vehicle.is_some()
            || player.sync.is_fall_flying()
            || crate::player_move::touches_fluid(
                player,
                &blocks,
                mc_bamboo_data::tags::fluid_tags::WATER,
            )
        {
            return;
        }
        if player.game_mode != GameMode::Creative && player.health.food.food_level < 7 {
            return;
        }
        player.random
    };
    // `play_sound`：[lunge_1, lunge_2, lunge_3] 三选一，`getRandomElement` 取一次随机。
    let sound_index = random.next_int_bounded(3);
    // `apply_impulse`：方向 `[0,0,1]` 经视线四元数旋转后 y 归零（`coordinate_scale`），
    // 量级 `linear base 0.458 / +0.458`。
    let magnitude = 0.458_f64 * lunge as f64;
    let impulse = Vec3::new(setup.look.x * magnitude, 0.0, setup.look.z * magnitude);
    let movement = {
        let player = region.players.get_mut(&id).expect("玩家");
        player.random = random;
        player.motion.delta = player.motion.delta.add(impulse);
        player.motion.delta
    };
    // `ServerPlayer` 补发 `set_entity_motion`（`ApplyEntityImpulse` 同款）。
    let packet = encode_frames(&SetEntityMotion {
        entity_id: id.0,
        movement,
    });
    region.links.send_frames(id, &packet);
    // `change_item_damage`（1 点）和 `apply_exhaustion`（linear 4.0 / +4.0）。
    durability::run(region, id, Hand::Main, |region| {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::durability::hurt_hand(&world, Hand::Main, 1);
    });
    player_exhaustion::add_in(region, id, 4.0 * lunge as f32);
    let name = [
        "minecraft:item.spear.lunge_1",
        "minecraft:item.spear.lunge_2",
        "minecraft:item.spear.lunge_3",
    ][sound_index as usize];
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        push_sound(&mut world, setup.position, name);
        world.drain_effects()
    };
    interaction::send_effects(region, Some(id), &effects);
}

// ===== 长矛蓄力冲刺（KineticWeapon） =====

/// `kinetic_weapon` 组件里读出来的几样。
struct KineticSpec {
    contact_cooldown_ticks: i32,
    delay_ticks: i32,
    dismount: Option<KineticCondition>,
    knockback: Option<KineticCondition>,
    damage: Option<KineticCondition>,
    damage_multiplier: f32,
    sound: Option<String>,
}

/// `mainHandItem.has(DataComponents.KINETIC_WEAPON)`：长矛蓄力走使用物品这一条。
pub(crate) fn is_kinetic_weapon(held: &ItemStack) -> bool {
    kinetic_spec(held).is_some()
}

/// `Item.use` 对 kinetic_weapon 的 `makeSound`：开始蓄力时播的 charge 音效。
pub(crate) fn kinetic_start_sound(held: &ItemStack) -> Option<String> {
    kinetic_spec(held).and_then(|spec| spec.sound)
}

/// `ItemStack.get(KINETIC_WEAPON)`。
fn kinetic_spec(held: &ItemStack) -> Option<KineticSpec> {
    if held.is_empty() {
        return None;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return None;
    };
    match defaults::resolve(held.item, &patch, kinds::KINETIC_WEAPON) {
        Some(ComponentValue::KineticWeapon {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            damage_multiplier,
            sound,
            ..
        }) => Some(KineticSpec {
            contact_cooldown_ticks: *contact_cooldown_ticks,
            delay_ticks: *delay_ticks,
            dismount: *dismount_conditions,
            knockback: *knockback_conditions,
            damage: *damage_conditions,
            damage_multiplier: *damage_multiplier,
            sound: spear_sound_name(sound),
        }),
        _ => None,
    }
}

/// `KineticWeapon.Condition.test(dur, speed, rel, mult)`。
fn kinetic_condition(cond: &KineticCondition, dur: i32, speed: f64, rel: f64, mult: f64) -> bool {
    dur <= cond.max_duration_ticks
        && speed >= f64::from(cond.min_speed) * mult
        && rel >= f64::from(cond.min_relative_speed) * mult
}

/// `KineticWeapon.damageEntities` 的一次逐刻调用。`time_held` 是已用刻数
/// （`getUseDuration - useTicks`，kinetic_weapon 的 useDuration 恒 72000）。
///
/// One per-tick `KineticWeapon.damageEntities` call. `time_held` is the elapsed
/// use ticks (`getUseDuration - useTicks`; a kinetic weapon's use duration is
/// always 72000).
pub(crate) fn kinetic_tick(region: &mut Region, id: PlayerId, time_held: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == GameMode::Spectator || player.spawn_pending.is_some() {
        return;
    }
    let held = player.inventory.held(Hand::Main).clone();
    let Some(spec) = kinetic_spec(&held) else {
        return;
    };
    // `dur = getUseDuration - useTicks - delayTicks`；小于 delayTicks 直接返回。
    let dur = time_held - spec.delay_ticks;
    if dur < 0 {
        return;
    }
    // `look = getLookAngle()`；`speed = look · getMotion(attacker)`，其中
    // `getMotion` 对玩家是 `lastKnownClientMovement × 20`（方块 / 秒）。
    let position = player.position();
    let eye = Vec3::new(position.x, position.y + player.eye_height(), position.z);
    let look = Vec3::view_vector(player.yaw, player.pitch);
    let movement = player.motion.known_movement;
    let speed = (look.x * movement.x + look.y * movement.y + look.z * movement.z) * 20.0;
    let base = player.attributes.base(attribute::ATTACK_DAMAGE);
    let knockback = attack_knockback_amount(player, &held);
    let entity_reach = player.attributes.value(attribute::ENTITY_INTERACTION_RANGE) as f32;
    let range = attack_range(&held, entity_reach);
    let creative = player.game_mode == GameMode::Creative;
    let (min_reach, max_reach) = if creative {
        (range.min_creative_reach, range.max_creative_reach)
    } else {
        (range.min_reach, range.max_reach)
    };
    // 射线两端和穿刺那一条完全一样（`ProjectileUtil.getHitEntitiesAlong`）。
    let ray_speed = look.x * movement.x + look.y * movement.y + look.z * movement.z;
    let min_point = eye.add(look.scale(f64::from(min_reach)));
    let max_point = eye.add(look.scale(f64::from(max_reach) + ray_speed.max(0.0)));
    // 原版长矛总是同时带穿刺组件；命令给出的自定义物品可能只有 `kinetic_weapon`，
    // 这时不打，避免让服务器崩溃。
    // Vanilla spears always carry the piercing component; a custom item from a
    // command may only have `kinetic_weapon`, so skip it instead of panicking.
    let Some(piercing) = piercing_spec(&held) else {
        return;
    };
    let setup = StabSetup {
        held: held.clone(),
        spec: piercing,
        position,
        eye,
        look,
        min_point,
        max_point,
        margin: range.hitbox_margin,
        creative,
        yaw: player.yaw,
        base_damage: base as f32,
        knockback,
        s: 1.0,
        crouching: player.sync.is_crouching(),
        fall_distance: player.motion.fall_distance,
    };
    let hits = spear_hits(region, &setup);
    // 逐实体：`wasRecentlyStabbed` 跳过、记下、算 `rel`、三组条件，全假跳过。
    let now = region.players[&id].entity_ticks;
    let mut to_hit: Vec<(SpearHit, KineticHitPlan)> = Vec::new();
    for hit in hits {
        let delta = kinetic_entity_delta(region, &hit);
        let tspeed = look.x * delta.x + look.y * delta.y + look.z * delta.z;
        let rel = (speed - tspeed).max(0.0);
        let dism = spec
            .dismount
            .is_some_and(|cond| kinetic_condition(&cond, dur, speed, rel, 1.0));
        let knock = spec
            .knockback
            .is_some_and(|cond| kinetic_condition(&cond, dur, speed, rel, 1.0));
        let dmg = spec
            .damage
            .is_some_and(|cond| kinetic_condition(&cond, dur, speed, rel, 1.0));
        if !dism && !knock && !dmg {
            continue;
        }
        if region.players[&id]
            .recent_kinetic_enemies
            .get(&hit.id)
            .is_some_and(|stamp| now - *stamp < spec.contact_cooldown_ticks)
        {
            continue;
        }
        let plan = KineticHitPlan {
            dmg_amt: base as f32 + mth::floor(rel * f64::from(spec.damage_multiplier)) as f32,
            damage: dmg,
            knockback: knock,
        };
        to_hit.push((hit, plan));
    }
    if to_hit.is_empty() {
        return;
    }
    // `rememberStabbedEntity`。
    if let Some(player) = region.players.get_mut(&id) {
        for (hit, _) in &to_hit {
            player.recent_kinetic_enemies.insert(hit.id, now);
        }
    }
    let mut hit_targets: Vec<(SpearHit, f32)> = Vec::new();
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        for (hit, plan) in &to_hit {
            if let Some(magic) = kinetic_stab_hit(&mut world, id, &setup, hit, plan) {
                hit_targets.push((*hit, magic));
            }
        }
        world.drain_effects()
    };
    interaction::send_effects(region, Some(id), &effects);
    let hit_any = !hit_targets.is_empty();
    // `Player.stabAttack` 命中后的收尾：附魔暴击动画、伤害统计、耐久、`lastHurtMob`
    // 和饥饿（每命中目标一遍）。
    for (target, magic) in &hit_targets {
        if *magic > 0.0 {
            send_animate(region, id, target.id, Animate::MAGIC_CRITICAL_HIT);
        }
        let actual_damage = damage_taken(region, target.id, target.health);
        item_use_stats::award_damage_dealt(region, id, (actual_damage * 10.0).round() as i32);
        // `damageStatsAndHearts`：`actualDamage > 2` 时发伤害指示粒子。
        if actual_damage > 2.0
            && let Some((position, height)) = kinetic_target_center(region, target)
        {
            send_particles(
                region,
                ParticleSpec {
                    particle: particle_type::DAMAGE_INDICATOR,
                    position: Vec3::new(
                        position.x,
                        position.y + f64::from(height) * 0.5,
                        position.z,
                    ),
                    spread: (0.1, 0.0, 0.1),
                    speed: 0.2,
                    count: (actual_damage * 0.5) as i32,
                },
            );
        }
        if weapon_component(&setup.held) {
            item_use_stats::award(region, id, setup.held.item);
        }
        durability::run(region, id, Hand::Main, |region| {
            let (mut level, ticks) = region.block_level();
            let world = World::new(&mut level, ticks);
            mc_bamboo_blocks::durability::hurt_hand(&world, Hand::Main, 1);
        });
        if let Some(player) = region.players.get_mut(&id) {
            let now = player.entity_ticks;
            player.combat_memory.record_attack(target.id, now);
            player.health_sync = player_hurt::HealthSync::of(player);
        }
        player_exhaustion::add_in(region, id, ATTACK_EXHAUSTION);
    }
    if hit_any {
        // `broadcastEntityEvent(attacker, 2)`：HIT_FEEDBACK_TICKS，客户端据此播
        // 命中音效（`onKineticHit` → `makeLocalHitSound`）。对拍里 `seen_by` 不建模，
        // 走 `links.broadcast` 和官方 `FakeLevel` 的单流收集一致。
        let frames = encode_frames(&EntityEvent {
            entity_id: id.0,
            event: 2,
        });
        region.links.broadcast(&frames);
        // `SPEAR_MOBS_TRIGGER.trigger(player, stabbedEntities(isAlive))`。
        let stabbed = region.players[&id]
            .recent_kinetic_enemies
            .keys()
            .filter(|&&uid| {
                region
                    .entities
                    .public_at(uid)
                    .is_some_and(|public| !public.removed)
            })
            .count() as i32;
        if stabbed > 0 {
            crate::advancement_triggers::record(
                region,
                id,
                "minecraft:spear_mobs",
                serde_json::json!({ "count": stabbed }),
            );
        }
    }
}

/// 命中目标的位置和高度，伤害指示粒子的中心用它。
fn kinetic_target_center(region: &Region, hit: &SpearHit) -> Option<(Vec3, f32)> {
    if hit.is_player {
        let player = region.players.get(&PlayerId(hit.id))?;
        let bounds = player.bounding_box();
        return Some((player.position(), (bounds.max_y - bounds.min_y) as f32));
    }
    let public = region.entities.public_at(hit.id)?;
    Some((public.position, public.kind.dimensions().1))
}

/// 长矛蓄力冲刺命中一个实体时，它的速度（`getKnownSpeed().scale(20)`）：玩家读
/// `lastKnownClientMovement`，生物读 `delta`。骑乘的载具回退没建模（对拍里都是静止牛）。
fn kinetic_entity_delta(region: &Region, hit: &SpearHit) -> Vec3 {
    if hit.is_player {
        return region
            .players
            .get(&PlayerId(hit.id))
            .map_or(Vec3::default(), |player| player.motion.delta);
    }
    region
        .entities
        .public_at(hit.id)
        .map_or(Vec3::default(), |public| public.delta)
}

/// 一次蓄力冲刺命中算好的量：`dmgAmt` 和伤害 / 击退两组条件的判定结果。
/// dismount 条件只参与「全假跳过」的判断，没有可观察的效果（目标不骑乘）。
struct KineticHitPlan {
    dmg_amt: f32,
    damage: bool,
    knockback: bool,
}

/// `Player.stabAttack` 的「正在使用」分支：`isUsingItem() && 同手` 时不乘 `s`、
/// 不乘 `baseDamageScaleFactor`，`dmgAmt` 已经是 `base + floor(rel × multiplier)`。
/// 命中后 `causeExtraKnockback` 两下（0.4 和 `getKnockback`）、攻击后附魔效果。
fn kinetic_stab_hit(
    world: &mut World<'_>,
    attacker: PlayerId,
    setup: &StabSetup,
    target: &SpearHit,
    plan: &KineticHitPlan,
) -> Option<f32> {
    let magic = damage_bonus(&setup.held, target.entity_type);
    let total = if plan.damage {
        plan.dmg_amt + magic
    } else {
        0.0
    };
    let mut source = DamageSource::player(attacker.0, setup.position, setup.creative);
    source.kind = mc_bamboo_blocks::entity::mob::DamageKind::Spear;
    if let Ok(weapon) = u16::try_from(setup.held.item) {
        source = source.with_weapon(weapon);
    }
    let breach = enchantment_level(&setup.held, "minecraft:breach").max(0) as f32;
    if breach > 0.0 {
        source = source.with_armor_effectiveness(-0.15 * breach);
    }
    let hurt = if target.is_player {
        world.hurt_player_from_mob(target.id, source, total, None)
    } else {
        world.hurt_mob(target.id, source, total)
    };
    if plan.knockback {
        apply_extra_knockback(world, target.id, 0.4, setup.yaw, target.is_player);
        apply_extra_knockback(
            world,
            target.id,
            setup.knockback,
            setup.yaw,
            target.is_player,
        );
    }
    if hurt || plan.knockback {
        if hurt {
            post_attack_effects(
                world,
                target.id,
                &setup.held,
                target.entity_type,
                target.is_player,
            );
        }
        return Some(magic);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    pub(crate) use crate::world::tests::frames_with_id;

    fn stack(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    /// `ItemStack.hurtEnemy` 的 `WEAPON` 组件判定：带它的武器记
    /// `ITEM_USED`，空手和不带该组件的物品不记。
    ///
    /// `ItemStack.hurtEnemy`'s `WEAPON` component gate: weapons carrying it
    /// award `ITEM_USED`, empty hands and other items do not.
    #[test]
    fn weapon_component_gates_the_extra_item_used_stat() {
        assert!(weapon_component(&stack(mc_bamboo_data::items::TRIDENT)));
        assert!(weapon_component(&stack(
            mc_bamboo_data::items::DIAMOND_SWORD
        )));
        assert!(!weapon_component(&ItemStack::empty()));
        assert!(!weapon_component(&stack(mc_bamboo_data::items::STICK)));
    }

    /// 重锤下砸分段加伤：`<=3 → 4·fall`、`(3,8] → 12+2·(fall−3)`、`>8 → 22+fall−8`。
    #[test]
    fn mace_smash_damage_follows_the_three_segments() {
        assert_eq!(mace_smash_damage(1.0), 4.0);
        assert_eq!(mace_smash_damage(3.0), 12.0);
        assert_eq!(mace_smash_damage(5.0), 16.0);
        assert_eq!(mace_smash_damage(8.0), 22.0);
        assert_eq!(mace_smash_damage(10.0), 24.0);
    }

    /// 重锤下砸阈值：摔落 ≤ 1.5 不加伤、非重锤不加伤；致密每级再加 0.5×摔落。
    #[test]
    fn mace_bonus_gates_on_fall_and_item() {
        let mace = stack(mc_bamboo_data::items::MACE);
        assert_eq!(mace_attack_damage_bonus(&mace, 1.5), 0.0);
        assert_eq!(mace_attack_damage_bonus(&mace, 2.0), 8.0);
        assert_eq!(
            mace_attack_damage_bonus(&stack(mc_bamboo_data::items::STICK), 2.0),
            0.0
        );
        // 致密 III：3×0.5=1.5，2 格 → 8 + 1.5×2 = 11。
        let density = enchanted(mc_bamboo_data::items::MACE, "minecraft:density", 3);
        assert_eq!(mace_attack_damage_bonus(&density, 2.0), 11.0);
    }

    #[test]
    fn fall_flying_disables_a_mace_smash() {
        let mut player = Player::new(
            PlayerId(1),
            uuid::Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        player.motion.fall_distance = 8.0;
        assert!(can_smash_attack(&player));
        player.sync.set_fall_flying(true);
        assert!(!can_smash_attack(&player));
    }

    /// 破甲偏移：每级 -0.15，夹到 [0,1] 后按比例穿透护甲（改坏确认用）。
    #[test]
    fn breach_offset_is_per_level_minus_point_fifteen() {
        let mace = stack(mc_bamboo_data::items::MACE);
        assert_eq!(enchantment_level(&mace, "minecraft:breach"), 0);
        let breach = enchanted(mc_bamboo_data::items::MACE, "minecraft:breach", 4);
        let offset = -0.15 * enchantment_level(&breach, "minecraft:breach").max(0) as f32;
        assert_eq!(offset, -0.6);
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
            bundle_selected: None,
            added,
            removed,
        }
    }

    /// 官方 `Player.canCriticalAttack` 的六个已建模前置：下落中、不着地、无失明
    /// （`isMobilityRestricted`）、非疾跑（`isSprinting`）、无载具（`isPassenger`）、
    /// 目标是生物。`onClimbable` / `isInWater` 是 README 已知限制里按缺省值处理的
    /// 两项也一起断言。
    ///
    /// Vanilla `Player.canCriticalAttack`'s six modelled preconditions: falling,
    /// off ground, no blindness (`isMobilityRestricted`), not sprinting
    /// (`isSprinting`), no vehicle (`isPassenger`), living target. `onClimbable` /
    /// `isInWater` are asserted too.
    #[test]
    fn can_critical_requires_every_modelled_precondition() {
        use mc_bamboo_blocks::entity::mob::effects::{EffectInstance, EffectMap};

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
        player.motion.fall_distance = 1.0;
        player.on_ground = false;
        region.insert_player(player);
        let crit = |region: &Region, living: bool| {
            let blocks =
                crate::player_move::PlayerBlocks::new(&region.chunks, &region.block_entities);
            can_critical(&region.players[&id], &blocks, living)
        };

        // 全部前置满足才算暴击；目标不是生物则否。
        // All preconditions met means crit; a non-living target cannot be crit.
        assert!(crit(&region, true));
        assert!(!crit(&region, false));

        // 着地（`onGround`）。
        // On ground.
        region.players.get_mut(&id).unwrap().on_ground = true;
        assert!(!crit(&region, true));
        region.players.get_mut(&id).unwrap().on_ground = false;

        // 没有下落（fallDistance）。
        // Not falling.
        region.players.get_mut(&id).unwrap().motion.fall_distance = 0.0;
        assert!(!crit(&region, true));
        region.players.get_mut(&id).unwrap().motion.fall_distance = 1.0;

        // 失明（hasEffect(BLINDNESS)）。
        // Blindness.
        region
            .players
            .get_mut(&id)
            .unwrap()
            .effects
            .add(EffectInstance::with_amplifier(
                mob_effect::BLINDNESS,
                600,
                0,
            ));
        assert!(!crit(&region, true));
        region.players.get_mut(&id).unwrap().effects = EffectMap::new();

        // 疾跑（isSprinting）。
        // Sprinting.
        entity_sync::set_sprinting(&mut region, id, true);
        assert!(!crit(&region, true));
        entity_sync::set_sprinting(&mut region, id, false);

        // 骑乘（isPassenger）。
        // Riding a vehicle.
        region.players.get_mut(&id).unwrap().riding.vehicle = Some(7);
        assert!(!crit(&region, true));
        region.players.get_mut(&id).unwrap().riding.vehicle = None;
        assert!(crit(&region, true));

        // 攀爬（`onClimbable`）：脚下是梯子。
        // Climbing (`onClimbable`): a ladder at the feet.
        let feet = mc_bamboo_core::BlockPos::new(0, 64, 0);
        region
            .chunks
            .set_block(feet, mc_bamboo_data::blocks::LADDER);
        assert!(!crit(&region, true));

        // 在水里（`isInWater`）。
        // In water (`isInWater`).
        region.chunks.set_block(feet, mc_bamboo_data::blocks::WATER);
        assert!(!crit(&region, true));
        region.chunks.set_block(feet, mc_bamboo_data::blocks::AIR);
        assert!(crit(&region, true));
    }

    /// 官方 `Player.isSweepAttack` 的前置：满充能、非暴击、非疾跑、着地、水平
    /// 移动够慢、拿剑。`getSpeed()` 这里按 README 已知限制写死 f32 基值 0.1；
    /// 阈值在 double 里算，约 0.0625000019。
    ///
    /// Vanilla `Player.isSweepAttack`'s gates: full strength, non-critical,
    /// non-sprint, on ground, slow horizontal movement, holding a sword.
    /// `getSpeed()` is the README-known-limit f32 0.1 base; the threshold is
    /// computed in double and is about 0.0625000019.
    #[test]
    fn is_sweep_attack_requires_full_charge_ground_and_a_sword() {
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
        player.on_ground = true;
        player.motion.known_movement = Vec3::new(0.0, 0.0, 0.0);
        region.insert_player(player);
        let sword = stack(mc_bamboo_data::items::DIAMOND_SWORD);
        let axe = stack(mc_bamboo_data::items::DIAMOND_AXE);
        let sweep = |region: &Region, held: &ItemStack, full, crit, sprint| {
            is_sweep_attack(&region.players[&id], held, full, crit, sprint)
        };

        // 满充能、着地、拿剑、静止 → 横扫。
        // Full strength, on ground, sword, still → sweep.
        assert!(sweep(&region, &sword, true, false, false));
        // 非满充能 / 暴击 / 疾跑 / 不是剑 → 不横扫。
        // Not full / critical / sprinting / not a sword → no sweep.
        assert!(!sweep(&region, &sword, false, false, false));
        assert!(!sweep(&region, &sword, true, true, false));
        assert!(!sweep(&region, &sword, true, false, true));
        assert!(!sweep(&region, &axe, true, false, false));

        // 离地 → 不横扫。
        // Off ground → no sweep.
        region.players.get_mut(&id).unwrap().on_ground = false;
        assert!(!sweep(&region, &sword, true, false, false));
        region.players.get_mut(&id).unwrap().on_ground = true;

        // 水平位移平方 ≥ (0.1F×2.5)² → 不横扫；慢于它 → 横扫。
        // Horizontal distance squared ≥ (0.1F×2.5)² → no sweep; slower → sweep.
        region.players.get_mut(&id).unwrap().motion.known_movement = Vec3::new(0.3, 0.0, 0.0);
        assert!(!sweep(&region, &sword, true, false, false));
        // 0.25² = 0.0625 仍略小于 double 里的阈值 0.0625000019，官方会横扫；
        // 用 f32 算阈值会把它错判成不横扫。
        // 0.25² = 0.0625 is still just under the double threshold 0.0625000019,
        // so vanilla sweeps; an f32 threshold would wrongly reject it.
        region.players.get_mut(&id).unwrap().motion.known_movement = Vec3::new(0.25, 0.0, 0.0);
        assert!(sweep(&region, &sword, true, false, false));
        // 阈值本身严格排除（`dcmpg >= 0` → false）。
        // The threshold itself is excluded (`dcmpg >= 0` → false).
        let max = f64::from(0.1_f32) * 2.5;
        region.players.get_mut(&id).unwrap().motion.known_movement = Vec3::new(max, 0.0, 0.0);
        assert!(!sweep(&region, &sword, true, false, false));
        region.players.get_mut(&id).unwrap().motion.known_movement = Vec3::new(0.2, 0.0, 0.0);
        assert!(sweep(&region, &sword, true, false, false));
    }

    /// `Player.attack` 在 `baseDamage > 0 || magicBoost > 0` 之外整段跳过：
    /// `onAttack()` 已经把冷却清零，但既不 hurt、也不发攻击音效。武器把
    /// `ATTACK_DAMAGE` 减到 0、又没有任何附魔加成时才会走到这里。
    ///
    /// Vanilla `Player.attack` skips everything outside
    /// `baseDamage > 0 || magicBoost > 0`: `onAttack()` has already reset the
    /// cooldown, but there is no hurt and no attack sound. A weapon that reduces
    /// `ATTACK_DAMAGE` to zero without an enchantment boost reaches this gate.
    #[test]
    fn a_zero_damage_attack_skips_the_hit_and_its_sound() {
        use mc_bamboo_data::{items, packets::play::clientbound, sounds};
        use mc_bamboo_protocol::{codec::Reader, packet::play::PlayRequest};

        use crate::world::tests::Harness;

        let mut server = Harness::new(8);
        let client = server.join("Kitten");
        server.tick();
        client.take_frames();
        let region = &mut server.world.islands.region;
        let position = region.players.get(&client.id).unwrap().position();
        let cow = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::cow(position));
        // 主手武器上一条 `ATTACK_DAMAGE = -1` 的 ADD_VALUE 修饰符，把玩家的
        // 基础 1.0 减到 0。官方 `Player.createAttributes()` 的基值就是 1.0。
        //
        // One `ATTACK_DAMAGE = -1` ADD_VALUE modifier on the main-hand weapon
        // cancels the player's base 1.0 from `Player.createAttributes()`.
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::ATTRIBUTE_MODIFIERS,
            PatchEntry::Set(ComponentValue::AttributeModifiers(vec![
                mc_bamboo_protocol::components::value::AttributeEntry {
                    attribute: mc_bamboo_data::registries::attribute::ATTACK_DAMAGE,
                    modifier: mc_bamboo_protocol::components::value::AttributeModifier {
                        id: "minecraft:base_attack_damage".to_owned(),
                        amount: -1.0,
                        operation: 0,
                    },
                    slot: 1,
                    display: mc_bamboo_protocol::components::value::AttributeDisplay::Default,
                },
            ])),
        );
        let (added, removed) = patch.to_opaque();
        {
            let player = region.players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = ItemStack {
                item: items::DIAMOND_SWORD,
                count: 1,
                bundle_selected: None,
                added,
                removed,
            };
            crate::player_attributes::rebuild_player(player, false);
            player.attack_strength_ticker = 100;
            player.on_ground = true;
            player.motion.fall_distance = 0.0;
        }
        let health_before = region
            .entities
            .get(cow)
            .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
            .expect("cow");

        server.request(&client, PlayRequest::Attack { entity_id: cow });
        server.tick();
        let frames = client.take_frames();
        let ids: Vec<i32> = frames_with_id(&frames, clientbound::SOUND)
            .iter()
            .filter_map(|frame| Reader::new(&frame.body).varint().ok())
            .map(|id| id - 1)
            .collect();
        let sound = |name| i32::from(sounds::sound_event(name));
        for absent in [
            "minecraft:entity.player.attack.weak",
            "minecraft:entity.player.attack.strong",
            "minecraft:entity.player.attack.crit",
            "minecraft:entity.player.attack.sweep",
            "minecraft:entity.player.attack.nodamage",
        ] {
            assert!(!ids.contains(&sound(absent)), "{absent} must not be sent");
        }
        let health_after = server
            .world
            .islands
            .region
            .entities
            .get(cow)
            .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
            .expect("cow");
        assert_eq!(health_after, health_before, "no hurt without damage");
        // `onAttack()` 仍然把冷却清零：这一刻开头的 +1 之后没有再涨。
        // `onAttack()` still resets the cooldown: after this tick's leading +1
        // the ticker stays at zero instead of continuing to charge.
        assert_eq!(
            server
                .world
                .islands
                .region
                .players
                .get(&client.id)
                .unwrap()
                .attack_strength_ticker,
            0,
            "the skipped attack still resets the cooldown"
        );
    }

    fn player_holding(mode: GameMode, held: ItemStack) -> Player {
        let mut p = Player::new(
            PlayerId(1),
            uuid::Uuid::nil(),
            "Test".to_owned(),
            mode,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        let slot = p.inventory.hand_slot(Hand::Main);
        p.inventory.slots[slot] = held;
        crate::player_attributes::rebuild_player(&mut p, false);
        p
    }

    #[test]
    fn cooldown_scale_matches_vanilla() {
        // 空手 ATTACK_SPEED=4 → delay=5；ticker=0 时 s=0.1，ticker=5 时 s=1。
        let player = player_holding(GameMode::Survival, ItemStack::empty());
        let delay = strength_delay(&player);
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
        let player = player_holding(
            GameMode::Survival,
            stack(mc_bamboo_data::items::DIAMOND_SWORD),
        );
        assert!((strength_delay(&player) - 12.5).abs() < 0.001);
        assert!((player.attributes.value(attribute::ATTACK_SPEED) - 1.6).abs() < 0.001);
    }

    #[test]
    fn damage_enchantments_follow_the_generated_tables() {
        let cow = mc_bamboo_data::registries::entity_type::COW;
        let zombie = mc_bamboo_data::registries::entity_type::ZOMBIE;
        let spider = mc_bamboo_data::registries::entity_type::SPIDER;
        let squid = mc_bamboo_data::registries::entity_type::SQUID;

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

        let impaling = enchanted(mc_bamboo_data::items::TRIDENT, "minecraft:impaling", 3);
        assert_eq!(damage_bonus(&impaling, squid), 7.5);
        assert_eq!(damage_bonus(&impaling, cow), 0.0);
    }

    /// `Player.attack` 对船这样的非生物目标照样把 `getEnchantedDamage` 的附魔
    /// 加成算进 `f`：锋利无条件，亡灵 / 节肢 / 穿刺按船的实体类型标签过滤。
    ///
    /// `Player.attack` includes the enchanted damage bonus even when the target
    /// is a non-living boat: Sharpness is unconditional, and Smite / Bane /
    /// Impaling are filtered by the boat's entity-type tags.
    #[test]
    fn boat_attack_keeps_the_enchanted_damage_bonus() {
        let boat = mc_bamboo_data::registries::entity_type::OAK_CHEST_BOAT;
        let sharp = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:sharpness",
            3,
        );
        let smite = enchanted(mc_bamboo_data::items::DIAMOND_SWORD, "minecraft:smite", 5);
        let bane = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:bane_of_arthropods",
            5,
        );
        let impaling = enchanted(mc_bamboo_data::items::TRIDENT, "minecraft:impaling", 5);
        assert_eq!(damage_bonus(&sharp, boat), 2.0);
        assert_eq!(damage_bonus(&smite, boat), 0.0);
        assert_eq!(damage_bonus(&bane, boat), 0.0);
        assert_eq!(damage_bonus(&impaling, boat), 0.0);

        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = player_holding(GameMode::Survival, sharp);
        // 满充能：`s = 1`，`baseDamage = ATTACK_DAMAGE`，`magicBoost = 2.0`。
        player.attack_strength_ticker = 20;
        let base = player.attributes.value(attribute::ATTACK_DAMAGE) as f32;
        region.insert_player(player);
        let boat_id = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::boat(
                boat,
                Vec3::new(1.5, 64.0, 1.5),
            ));
        let damage = vehicle_attack_damage(&mut region, id, boat_id).unwrap();
        assert_eq!(damage, base + 2.0);
    }

    /// 官方 `ApplyMobEffect` 对节肢杀手的三次取数全来自受害者自己的随机源，
    /// 顺序是 `nextInt(1)`、时长、等级；等级恒为 3，L=1 时时长恒 30 刻。
    ///
    /// Vanilla `ApplyMobEffect` for Bane of Arthropods makes all three draws from
    /// the victim's random in the order `nextInt(1)`, duration, amplifier. The
    /// amplifier is always 3 and L=1 always yields 30 ticks.
    #[test]
    fn bane_slowness_draws_three_times_from_the_mob_random() {
        let mut random = JavaRandom::new(12_345);
        let mut reference = JavaRandom::new(12_345);
        let (ticks, amplifier) = bane_slowness(&mut random, 1);
        let _ = reference.next_int_bounded(1);
        let _ = reference.next_float();
        let _ = reference.next_float();
        assert_eq!(ticks, 30, "L=1 duration is the constant 1.5s");
        assert_eq!(amplifier, 3);
        // 三次取数之后两个随机源的状态一致。
        // Both sources are in the same state after the three draws.
        assert_eq!(random.next_long(), reference.next_long());

        // L=3：时长落在 [30, 50) 刻。
        // L=3: the duration lands in [30, 50) ticks.
        let mut random = JavaRandom::new(7);
        let mut reference = JavaRandom::new(7);
        let (ticks, _) = bane_slowness(&mut random, 3);
        let _ = reference.next_int_bounded(1);
        let duration = reference.next_float() * (2.5 - 1.5) + 1.5;
        assert_eq!(ticks, (duration * 20.0).round() as i32);
        assert!((30..50).contains(&ticks), "{ticks}");
    }

    /// 攻击路径确实从**受害者**的随机源取数：打完蜘蛛后它的随机源正好前进三次，
    /// 缓慢时长与按同样三次取数算出来的一致。世界随机源不在这一步被消耗。
    ///
    /// The attack path really draws from the **victim's** random: after hitting a
    /// spider its random is exactly three draws ahead and the Slowness duration
    /// matches the same three draws. The world random is not touched here.
    #[test]
    fn bane_slowness_draws_from_the_victim_in_the_attack_path() {
        use mc_bamboo_blocks::entity::mob::MobType;

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
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:bane_of_arthropods",
            3,
        );
        crate::player_attributes::rebuild(&mut region);
        region.players.get_mut(&id).unwrap().attack_strength_ticker = 100;
        let spider = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::by_mob_type(
                MobType::Spider,
                Vec3::new(2.5, 64.0, 0.5),
            ));
        let before = region.entities.get(spider).expect("spider").random;
        let sword = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:bane_of_arthropods",
            3,
        );
        // 直接调 `land_attack` 命中后调的 `post_attack_effects`，跳过 `hurt_mob`
        // 里受伤音高的两次取数，才能精确锁住这三次。
        //
        // Call the `post_attack_effects` that `land_attack` runs on a hit,
        // skipping the two hurt-sound pitch draws inside `hurt_mob`, so the
        // three draws can be pinned exactly.
        let mut expected = before;
        let (ticks, amplifier) = bane_slowness(&mut expected, 3);
        {
            let (mut level, block_ticks) = region.block_level();
            let mut world = World::new(&mut level, block_ticks);
            post_attack_effects(
                &mut world,
                spider,
                &sword,
                mc_bamboo_data::registries::entity_type::SPIDER,
                false,
            );
        }

        let entity = region.entities.get(spider).expect("spider");
        let effect = entity
            .kind
            .mob()
            .and_then(|mob| mob.effects.get(mob_effect::SLOWNESS))
            .expect("Bane of Arthropods applies Slowness");
        assert_eq!(effect.duration, ticks);
        assert_eq!(effect.amplifier, amplifier);
        assert_eq!(
            entity.random.state(),
            expected.state(),
            "the victim random advanced by exactly the three ApplyMobEffect draws"
        );
    }

    /// 完整的 `on_attack` 走到节肢杀手：蜘蛛被挂上缓慢 IV（等级 3），时长落在
    /// `[30, 50)` 刻。这里不复述精确取数——`hurt_mob` 的受伤音高也会消耗两次
    /// 受害者随机数，精确值由上面那条直接调 `post_attack_effects` 的测试锁住。
    ///
    /// The full `on_attack` reaches Bane of Arthropods: the spider gets Slowness IV
    /// (amplifier 3) for `[30, 50)` ticks. This does not pin the exact draw because
    /// `hurt_mob`'s hurt-sound pitch also consumes two victim draws; the exact
    /// values are pinned by the `post_attack_effects` test above.
    #[test]
    fn attack_applies_bane_slowness_to_a_spider() {
        use mc_bamboo_blocks::entity::mob::MobType;

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
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:bane_of_arthropods",
            3,
        );
        crate::player_attributes::rebuild(&mut region);
        region.players.get_mut(&id).unwrap().attack_strength_ticker = 100;
        let spider = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::by_mob_type(
                MobType::Spider,
                Vec3::new(2.5, 64.0, 0.5),
            ));

        on_attack(&mut region, id, spider);

        let entity = region.entities.get(spider).expect("spider");
        let effect = entity
            .kind
            .mob()
            .and_then(|mob| mob.effects.get(mob_effect::SLOWNESS))
            .expect("Bane of Arthropods applies Slowness");
        assert_eq!(effect.amplifier, 3);
        assert!((30..50).contains(&effect.duration), "{}", effect.duration);
    }

    /// 创造模式攻击者横扫时，官方 `doSweepAttack` 复用的 `damageSource` 带着
    /// `isCreativePlayer()`，能绕过被横扫目标的临时无敌帧。横扫来源如果写死成
    /// 非创造，带无敌帧的第二头牛就漏了伤害。
    ///
    /// A creative attacker's sweep reuses the `damageSource` from `Player.attack`,
    /// whose `isCreativePlayer()` bypasses the swept target's temporary
    /// invulnerability. Hard-coding the sweep source as non-creative would drop
    /// the damage on an invulnerable second cow.
    #[test]
    fn creative_sweep_bypasses_swept_target_invulnerability() {
        use mc_bamboo_blocks::entity::Entity;

        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            uuid::Uuid::nil(),
            "Test".to_owned(),
            GameMode::Creative,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        player.on_ground = true;
        player.motion.fall_distance = 0.0;
        player.attack_strength_ticker = 100;
        region.insert_player(player);
        let sword = stack(mc_bamboo_data::items::DIAMOND_SWORD);
        let slot = region.players[&id].inventory.hand_slot(Hand::Main);
        region.players.get_mut(&id).unwrap().inventory.slots[slot] = sword.clone();
        region.players.get_mut(&id).unwrap().last_main_hand_item = sword;
        crate::player_attributes::rebuild(&mut region);
        let main = region
            .entities
            .spawn(Entity::cow(Vec3::new(2.5, 64.0, 0.5)));
        let swept = region
            .entities
            .spawn(Entity::cow(Vec3::new(3.0, 64.0, 0.5)));
        // 临时无敌帧：只有 `creative_player` 为真的伤害源能打进去。
        // Temporary invulnerability: only a `creative_player` source gets through.
        region.entities.with_mut(swept, |entity| {
            if let Some(mob) = entity.kind.mob_mut() {
                mob.invulnerable_time = 10;
            }
        });
        let cow_health = |region: &Region, uid: i32| {
            region
                .entities
                .get(uid)
                .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
                .unwrap_or(0.0)
        };
        let before = cow_health(&region, swept);

        on_attack(&mut region, id, main);

        let after = cow_health(&region, swept);
        assert!(
            after < before,
            "a creative sweep must bypass the swept target's invulnerability"
        );
    }

    #[test]
    fn knockback_and_sweeping_use_the_generated_formulas() {
        let knock = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:knockback",
            2,
        );
        let knock_player = player_holding(GameMode::Survival, knock.clone());
        assert_eq!(attack_knockback_amount(&knock_player, &knock), 1.0);
        let sweeping = enchanted(
            mc_bamboo_data::items::DIAMOND_SWORD,
            "minecraft:sweeping_edge",
            3,
        );
        let sweep_player = player_holding(GameMode::Survival, sweeping);
        assert_eq!(sweeping_ratio(&sweep_player), 0.75);
        let plain_player = player_holding(
            GameMode::Survival,
            stack(mc_bamboo_data::items::DIAMOND_SWORD),
        );
        assert_eq!(sweeping_ratio(&plain_player), 0.0);
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
        mc_bamboo_blocks::durability::set_damage_value(&mut damaged, 5);
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
        let reach = |mode| {
            player_holding(mode, ItemStack::empty())
                .attributes
                .value(attribute::ENTITY_INTERACTION_RANGE) as f32
        };
        assert_eq!(reach(GameMode::Survival), 3.0);
        assert_eq!(reach(GameMode::Creative), 5.0);
        assert_eq!(reach(GameMode::Adventure), 3.0);
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
        let range = attack_range(&empty, 3.0);
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
        let creative_range = attack_range(&empty, 5.0);
        assert!(creative_range.is_in_range(eye, far, ATTACK_RANGE_BUFFER, true));
    }

    /// 边界是闭区间：距离正好等于上限时仍然算命中（`isInRange` 用 `<=`）。
    ///
    /// The bounds are closed: a distance exactly at the limit still hits
    /// (`isInRange` uses `<=`).
    /// `LivingEntity.getAttackRangeWith(weaponItem)`：武器上的 `ATTACK_RANGE`
    /// 组件逐字段覆盖 `defaultFor`，玩家攻击者读 min/max（创造读 min/maxCreative），
    /// `hitbox_margin` 同时放宽下界和上界。`mob_factor` 只对非玩家生效。
    ///
    /// `LivingEntity.getAttackRangeWith(weaponItem)`: an `ATTACK_RANGE`
    /// component overrides `defaultFor` field by field. Player attackers read
    /// min/max (creative reads min/maxCreative); `hitbox_margin` widens both
    /// ends. `mob_factor` only applies to non-player attackers.
    #[test]
    fn a_weapon_attack_range_component_overrides_the_default() {
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::ATTACK_RANGE,
            PatchEntry::Set(ComponentValue::AttackRange {
                min_reach: 2.0,
                max_reach: 4.0,
                min_creative_reach: 1.0,
                max_creative_reach: 7.0,
                hitbox_margin: 0.5,
                mob_factor: 0.25,
            }),
        );
        let (added, removed) = patch.to_opaque();
        let weapon = ItemStack {
            item: mc_bamboo_data::items::DIAMOND_SWORD,
            count: 1,
            bundle_selected: None,
            added,
            removed,
        };
        assert_eq!(
            attack_range(&weapon, 3.0),
            AttackRange {
                min_reach: 2.0,
                max_reach: 4.0,
                min_creative_reach: 1.0,
                max_creative_reach: 7.0,
                hitbox_margin: 0.5,
            }
        );
        // `isInRange` 的窗口是 `[min − margin − buffer, max + margin + buffer]`：
        // 眼睛到盒子的距离正好 3.0 时，下界 2.0 − 0.5 − 3.0 = −1.5，
        // 上界 4.0 + 0.5 + 3.0 = 7.5，落在里面。创造模式的上界是 10.5。
        //
        // `isInRange` uses `[min − margin − buffer, max + margin + buffer]`.
        let eye = Vec3::new(0.5, 64.0, 0.5);
        let bounds = Aabb::new(3.5, 63.5, 0.0, 4.0, 65.0, 1.0);
        let range = attack_range(&weapon, 3.0);
        assert!(range.is_in_range(eye, bounds, ATTACK_RANGE_BUFFER, false));
        assert!(range.is_in_range(eye, bounds, ATTACK_RANGE_BUFFER, true));
        // 没有组件时同一个盒子也在范围内，但上界只有 6.0：更远的盒子只有带
        // 组件的武器（或创造模式）够得到。
        //
        // Without the component the same box is still in range, but the upper
        // limit is 6.0: a farther box needs the component or creative mode.
        let far = Aabb::new(7.9, 63.5, 0.0, 8.4, 65.0, 1.0);
        assert!(!attack_range(&ItemStack::empty(), 3.0).is_in_range(
            eye,
            far,
            ATTACK_RANGE_BUFFER,
            false
        ));
        assert!(range.is_in_range(eye, far, ATTACK_RANGE_BUFFER, false));
    }

    /// `resolve_target` 对玩家目标用 `Player.bounding_box()` 的真实尺寸：
    /// 蹲下时只有 1.5 高（站着是 1.8），横扫盒和伤害指示粒子都读这个尺寸。
    /// 写死站立尺寸会让蹲着的受害者被当成更高的盒子。
    ///
    /// `resolve_target` takes the player target's size from its real
    /// `bounding_box()`: crouching is 1.5 tall (standing 1.8), and both the
    /// sweep box and the damage-indicator particle read it. Hard-coding the
    /// standing height would model a crouching victim as a taller box.
    #[test]
    fn a_crouching_player_target_keeps_its_real_collision_box() {
        let mut region = Region::new(2, 20, false);
        let attacker = PlayerId(1);
        let victim = PlayerId(2);
        let mut a = Player::new(
            attacker,
            uuid::Uuid::nil(),
            "Attacker".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        a.game_mode = GameMode::Survival;
        region.insert_player(a);
        let mut v = Player::new(
            victim,
            uuid::Uuid::nil(),
            "Victim".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(2.5, 64.0, 0.5),
        );
        v.game_mode = GameMode::Survival;
        region.insert_player(v);
        let eye = Vec3::new(0.5, 65.62, 0.5);
        let held = ItemStack::empty();
        let standing = resolve_target(
            &region,
            attacker,
            victim.0,
            eye,
            GameMode::Survival,
            3.0,
            &held,
        )
        .expect("the standing victim is a valid target");
        assert_eq!(standing.dimensions, (0.6, 1.8));

        entity_sync::on_player_input(
            &mut region,
            victim,
            mc_bamboo_protocol::packet::play::INPUT_FLAG_SHIFT,
        );
        let box3d = region.players[&victim].bounding_box();
        assert!((box3d.max_y - box3d.min_y - 1.5).abs() < 1.0e-9);
        let crouching = resolve_target(
            &region,
            attacker,
            victim.0,
            eye,
            GameMode::Survival,
            3.0,
            &held,
        )
        .expect("the crouching victim is a valid target");
        assert_eq!(crouching.dimensions, (0.6, 1.5));
    }

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

    /// 长矛的 `attack_animation` 组件是刺击（kind 2、时长 19），别的武器是缺省挥手
    /// （whack、时长 6）。
    #[test]
    fn spear_attack_animation_is_a_stab() {
        let spear = stack(mc_bamboo_data::items::IRON_SPEAR);
        assert_eq!(attack_swing(&spear), (SwingAnimationType::Stab, 19));
        let mace = stack(mc_bamboo_data::items::MACE);
        assert_eq!(attack_swing(&mace), (SwingAnimationType::Whack, 6));
        assert_eq!(
            attack_swing(&ItemStack::empty()),
            (SwingAnimationType::Whack, 6)
        );
    }

    /// 铁长矛的 `kinetic_weapon` 组件按官方 26.3 的数值解析出来。
    #[test]
    fn iron_spear_kinetic_spec_matches_26_3() {
        let spec = kinetic_spec(&stack(mc_bamboo_data::items::IRON_SPEAR))
            .expect("iron spear has a kinetic component");
        assert_eq!(spec.contact_cooldown_ticks, 10);
        assert_eq!(spec.delay_ticks, 12);
        assert_eq!(spec.damage_multiplier, 0.95);
        let damage = spec.damage.expect("damage conditions");
        assert_eq!(damage.max_duration_ticks, 225);
        assert_eq!(damage.min_speed, 0.0);
        assert_eq!(damage.min_relative_speed, 4.6);
        let knockback = spec.knockback.expect("knockback conditions");
        assert_eq!(knockback.max_duration_ticks, 135);
        assert_eq!(knockback.min_speed, 5.1);
    }

    /// `KineticWeapon.Condition.test`：时长、绝对速度和相对速度三个门槛都过了才真。
    #[test]
    fn kinetic_condition_requires_all_three_gates() {
        let cond = KineticCondition {
            max_duration_ticks: 225,
            min_speed: 0.0,
            min_relative_speed: 4.6,
        };
        let mult = 1.0;
        assert!(kinetic_condition(&cond, 100, 6.0, 5.0, mult));
        // 时长超了。
        assert!(!kinetic_condition(&cond, 226, 6.0, 5.0, mult));
        // 相对速度不够。
        assert!(!kinetic_condition(&cond, 100, 6.0, 4.5, mult));
        // 绝对速度不够。
        let speed_gate = KineticCondition {
            max_duration_ticks: 50,
            min_speed: 11.0,
            min_relative_speed: 0.0,
        };
        assert!(!kinetic_condition(&speed_gate, 10, 10.0, 0.0, mult));
        assert!(kinetic_condition(&speed_gate, 10, 11.0, 0.0, mult));
    }

    /// `TamableAnimal.getTeam()` 覆写：驯服动物自己没队时返回主人的队。这里钉住
    /// `mob_team_index` 的回退：狼自己没队、主人（玩家）在 0 队时，有效队伍是 0。
    #[test]
    fn tamed_mob_inherits_its_owners_team() {
        use mc_bamboo_blocks::entity::{Entity, mob::MobType};
        use uuid::Uuid;

        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::from_u128(1),
            "Kitten".to_owned(),
            GameMode::Survival,
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        std::sync::Arc::make_mut(&mut region.global.scoreboard).insert_team("Kitten", 0);

        let uid = region.entities.spawn(Entity::by_mob_type(
            MobType::Wolf,
            Vec3::new(2.5, 64.0, 2.5),
        ));
        region.entities.with_mut(uid, |entity| {
            if let Some(mob) = entity.kind.mob_mut() {
                mob.tame.flags |= mc_bamboo_blocks::entity::mob::tame::FLAG_TAME;
                mob.tame.owner = Some(*Uuid::from_u128(1).as_bytes());
            }
        });
        let public = region.entities.public_at(uid).expect("the wolf exists");
        let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind else {
            panic!("expected a mob");
        };
        // 自己没队、驯服 → 回退到主人（Kitten）的 0 队。
        assert_eq!(mob_team_index(&region, mob, true), Some(0));
        // 没驯服（tame 假）→ 不回退。
        assert_eq!(mob_team_index(&region, mob, false), None);
    }
}
