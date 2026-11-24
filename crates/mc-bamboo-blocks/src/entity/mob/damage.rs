//! 玩家受伤的减伤公式，官方 `Player.hurtServer` / `CombatRules` / `LivingEntity`。
//!
//! 这里只放不碰世界的纯函数：难度缩放、护甲 / 韧性、保护附魔、吸收血量、
//! 无敌帧和冷却的推进。状态本身挂在游戏层的 `Player` 上，这样精确驱动、
//! 并行驱动和对拍假世界用的是同一份算式。
//!
//! 选项 A 的范围：护甲值 / 韧性取自六个装备槽的 `attribute_modifiers` 组件，
//! 保护附魔等级取自 `enchantments` 组件。武器破甲、抗性效果、格挡和不死图腾
//! 是已知差异，这一期不做。
//!
//! Pure damage-reduction maths for player hurt. World state lives in the game
//! layer; exact, parallel and oracle drives all use these functions.

use mc_bamboo_core::mth;
use mc_bamboo_data::registries::{attribute, data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
};

use super::DamageKind;

/// 官方 `Difficulty.getId()`，也是网络包里的字节。
///
/// Official `Difficulty.getId()`, also the byte on the wire.
pub type DifficultyId = i32;

/// 官方 `PLAYER_HEALTH` / `LivingEntity.MAX_HEALTH` 的缺省上限。
pub const MAX_HEALTH: f32 = 20.0;
/// 官方 `Attributes.MAX_ABSORPTION` 的基础值是 0；没有状态效果时吸收血量恒为 0。
/// 存档里读回的值也夹到这个上限，见 `LivingEntity.readAdditionalSaveData`。
///
/// Vanilla `Attributes.MAX_ABSORPTION` starts at 0, so without effects the
/// absorption amount is always zero.
pub const MAX_ABSORPTION: f32 = 0.0;
/// 官方 `FoodData.addExhaustion` 的上限。
pub const MAX_EXHAUSTION: f32 = 40.0;
/// `hurtServer` 完整受击后的伤害冷却，`LivingEntity.hurtServer`。
pub const HURT_COOLDOWN: i32 = 20;
/// `hurtServer` 完整受击后的受伤动画时长。
pub const HURT_DURATION: i32 = 10;

/// 官方 `Player.hurtServer` 的难度缩放：`scalesWithDifficulty()` 为真时
/// 和平 0、简单 `min(a/2+1, a)`、困难 `a*3/2`、普通原值。
///
/// `Player.hurtServer`'s difficulty scaling when `scalesWithDifficulty()` is
/// true: peaceful 0, easy `min(a/2+1, a)`, hard `a*3/2`, normal unchanged.
pub fn scale_by_difficulty(amount: f32, difficulty: DifficultyId, scales: bool) -> f32 {
    if !scales {
        return amount;
    }
    match difficulty {
        0 => 0.0,
        1 => mth::java_min_f32(amount / 2.0 + 1.0, amount),
        3 => amount * 3.0 / 2.0,
        _ => amount,
    }
}

/// 官方 `CombatRules.getDamageAfterAbsorb` 在没有武器破甲效果时的式子。
/// `armor` 是 `getArmorValue()` 取整后的结果，`toughness` 是 `ARMOR_TOUGHNESS`。
///
/// `CombatRules.getDamageAfterAbsorb` without a weapon's armor-effectiveness
/// modifier: `factor = 2 + toughness/4`, `effective = clamp(armor - amount/factor,
/// armor*0.2, 20)`, then `amount *= 1 - effective/25`.
pub fn damage_after_absorb(amount: f32, armor: f32, toughness: f32) -> f32 {
    let factor = 2.0 + toughness / 4.0;
    let effective = mth::clamp_f32(armor - amount / factor, armor * 0.2, 20.0);
    amount * (1.0 - effective / 25.0)
}

/// 官方 `CombatRules.getDamageAfterMagicAbsorb`：`clamp(protection, 0, 20)`
/// 再乘 `1 - protection/25`。`getDamageProtection` 返回 0 时官方不调用它。
///
/// `CombatRules.getDamageAfterMagicAbsorb`: clamp protection to 0..20 and
/// multiply by `1 - protection/25`.
pub fn damage_after_magic_absorb(amount: f32, protection: f32) -> f32 {
    let protection = mth::clamp_f32(protection, 0.0, 20.0);
    amount * (1.0 - protection / 25.0)
}

/// 护甲值 / 韧性 / 保护附魔等级的合计。四个盔甲槽各算一次，主手和副手
/// 只用于 `any` / `hand` 组（官方 `EquipmentSlotGroup` 的匹配）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ArmorValues {
    pub armor: f32,
    pub toughness: f32,
    pub protection: f32,
    /// 抗性提升效果的等级；`None` 表示没有。`#bypasses_resistance` 的伤害不吃它。
    ///
    /// The resistance amplifier; `None` when absent. `#bypasses_resistance`
    /// damage ignores it.
    pub resistance: Option<i32>,
}

impl ArmorValues {
    /// 无武器破甲时扣掉护甲、保护附魔和抗性效果的那一份伤害。
    ///
    /// `LivingEntity.getDamageAfterMagicAbsorb`: resistance multiplies by
    /// `max(0, 25 - 5*(amplifier+1)) / 25`.
    pub fn reduce(self, amount: f32, kind: DamageKind) -> f32 {
        let mut amount = amount;
        if !kind.bypasses_armor() {
            amount = damage_after_absorb(amount, self.armor, self.toughness);
        }
        if !kind.bypasses_effects() {
            if !kind.bypasses_enchantments() && amount > 0.0 && self.protection > 0.0 {
                amount = damage_after_magic_absorb(amount, self.protection);
            }
            if let Some(amplifier) = self.resistance
                && !kind.bypasses_resistance()
            {
                let factor = (25 - 5 * (amplifier.clamp(0, 4) + 1)).max(0) as f32 / 25.0;
                amount *= factor;
            }
        }
        amount
    }
}

/// 六个装备槽按官方 `EquipmentSlotGroup` 的 ID：主手、副手、靴子、护腿、胸甲、头盔。
/// 和 [`crate::entity::mob::damage::ArmorValues`] 的调用方约定一致。
const ARMOR_SLOT_GROUPS: [i32; 6] = [
    1, // mainhand
    2, // offhand
    4, // feet
    5, // legs
    6, // chest
    7, // head
];

/// 槽位组 `group` 是否覆盖装备槽 `slot`。`any` 和 `armor` 也都算。
fn group_applies(group: i32, slot: i32) -> bool {
    match group {
        0 | 8 => true,  // any / armor
        3 => slot <= 1, // hand
        _ => group == slot,
    }
}

/// 从六个装备槽的组件求护甲值、韧性和保护附魔等级。
/// 物品的默认组件由 [`defaults::resolve`] 提供，客户端传来的补丁优先。
///
/// Sums armor, toughness and protection from six equipment slots, including
/// each item's default components.
pub fn armor_values(equipment: &[&ItemStack; 6]) -> ArmorValues {
    let mut values = ArmorValues::default();
    for (slot, stack) in equipment.iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        let Ok(patch) = ComponentPatch::from_opaque(stack) else {
            continue;
        };
        let slot = ARMOR_SLOT_GROUPS[slot];
        if let Some(ComponentValue::AttributeModifiers(entries)) =
            defaults::resolve(stack.item, &patch, kinds::ATTRIBUTE_MODIFIERS)
        {
            for entry in entries {
                if !group_applies(entry.slot, slot) || entry.modifier.operation != 0 {
                    continue;
                }
                let amount = entry.modifier.amount as f32;
                match entry.attribute {
                    attribute::ARMOR => values.armor += amount,
                    attribute::ARMOR_TOUGHNESS => values.toughness += amount,
                    _ => {}
                }
            }
        }
        if let Some(ComponentValue::Enchantments(levels)) =
            defaults::resolve(stack.item, &patch, kinds::ENCHANTMENTS)
        {
            for (enchantment, level) in levels {
                if *enchantment == protection_enchantment_id() {
                    values.protection += *level as f32;
                }
            }
        }
    }
    // 官方 `getArmorValue()` 返回 int（Mth.floor），护甲和韧性还要夹到属性范围。
    values.armor = (mth::floor_f32(mth::clamp_f32(values.armor, 0.0, 30.0))) as f32;
    values.toughness = mth::clamp_f32(values.toughness, 0.0, 20.0);
    values
}

/// `minecraft:protection` 的附魔注册表 ID。
fn protection_enchantment_id() -> i32 {
    const ENCHANTMENTS: &str = "minecraft:enchantment";
    const PROTECTION: &str = "minecraft:protection";
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == ENCHANTMENTS)
        .and_then(|registry| registry.entries.iter().position(|name| *name == PROTECTION))
        .and_then(|id| i32::try_from(id).ok())
        .unwrap_or(-1)
}

/// 一次受伤后的状态变化。返回值里所有数都已经夹到官方范围。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurtOutcome {
    /// 这次真正扣掉的生命（护甲、附魔、吸收之后）。
    pub damage: f32,
    /// 新的血量。
    pub health: f32,
    /// 新的吸收血量。
    pub absorption: f32,
    /// 新的饥饿消耗，按 `FoodData.addExhaustion` 夹到 40。
    pub exhaustion: f32,
    /// 是否完整受击（发伤害事件、受伤动画、击退）。
    pub full_hit: bool,
    /// 新的伤害冷却。
    pub damage_cooldown: i32,
    /// 新的受伤动画倒计时。
    pub hurt_time: i32,
    /// 新的 `lastHurt`。
    pub last_hurt: f32,
    /// 新的 `hurtDuration`。
    pub hurt_duration: i32,
    /// 击退方向（相对目标的 dx、dz），需要击退时由调用方用源位置算。
    pub knockback: bool,
}

/// 官方 `Player.hurtServer` → `LivingEntity.hurtServer` → `Player.actuallyHurt`
/// 的完整一次受伤。`damage_cooldown` 每刻由 [`tick_cooldowns`] 递减。
///
/// 不触发死亡：血量可以降到 0，但不掉落、不重生，留给 1b。
///
/// One full hurt. Death is not triggered: health may reach zero without drops
/// or respawn, which belongs to 1b.
#[allow(
    clippy::too_many_arguments,
    reason = "the vanilla hurt path takes all these fields"
)]
pub fn hurt(
    amount: f32,
    kind: DamageKind,
    difficulty: DifficultyId,
    scales: bool,
    health: f32,
    absorption: f32,
    exhaustion: f32,
    damage_cooldown: i32,
    last_hurt: f32,
    hurt_time: i32,
    hurt_duration: i32,
    armor: ArmorValues,
) -> Option<HurtOutcome> {
    hurt_with_max_health(
        amount,
        kind,
        difficulty,
        scales,
        health,
        absorption,
        exhaustion,
        damage_cooldown,
        last_hurt,
        hurt_time,
        hurt_duration,
        armor,
        MAX_HEALTH,
    )
}

/// 同 [`hurt`]，但血量上限由调用方从 `Attributes.MAX_HEALTH` 传入，
/// 避免生命提升或修改最大生命属性时受伤把血量截断回 20。
///
/// Same as [`hurt`], clamping the resulting health to the caller's resolved
/// `MAX_HEALTH` instead of the default 20.
#[allow(
    clippy::too_many_arguments,
    reason = "the vanilla hurt path takes all these fields"
)]
pub fn hurt_with_max_health(
    amount: f32,
    kind: DamageKind,
    difficulty: DifficultyId,
    scales: bool,
    health: f32,
    absorption: f32,
    exhaustion: f32,
    damage_cooldown: i32,
    last_hurt: f32,
    hurt_time: i32,
    hurt_duration: i32,
    armor: ArmorValues,
    max_health: f32,
) -> Option<HurtOutcome> {
    let scaled = scale_by_difficulty(amount, difficulty, scales);
    if scaled == 0.0 || health <= 0.0 {
        return None;
    }
    let mut amount = if amount < 0.0 { 0.0 } else { scaled };
    if amount.is_nan() || amount.is_infinite() {
        amount = f32::MAX;
    }
    let cooldown_hit = damage_cooldown > 10 && !kind.bypasses_cooldown();
    if cooldown_hit && amount <= last_hurt {
        return None;
    }
    let full_hit = !cooldown_hit;
    let applied = if full_hit { amount } else { amount - last_hurt };
    let reduced = armor.reduce(applied, kind).max(0.0);
    let (damage, new_absorption) = absorb(reduced, absorption);
    let new_exhaustion = if damage > 0.0 {
        (exhaustion + kind.exhaustion()).min(MAX_EXHAUSTION)
    } else {
        exhaustion
    };
    let new_health = mth::clamp_f32(health - damage, 0.0, max_health.max(1.0));
    Some(HurtOutcome {
        damage,
        health: new_health,
        absorption: new_absorption,
        exhaustion: new_exhaustion,
        full_hit,
        damage_cooldown: if full_hit {
            HURT_COOLDOWN
        } else {
            damage_cooldown
        },
        hurt_time: if full_hit { HURT_DURATION } else { hurt_time },
        last_hurt: amount,
        hurt_duration: if full_hit {
            HURT_DURATION
        } else {
            hurt_duration
        },
        knockback: full_hit && !kind.no_knockback(),
    })
}

/// `Player.actuallyHurt` 的吸收血量那一段：`Math.max(amount - absorption, 0)`，
/// 再把吸收设成 `absorption - (before - after)`。
fn absorb(amount: f32, absorption: f32) -> (f32, f32) {
    let before = amount;
    let after = mth::java_max_f32(before - absorption, 0.0);
    (after, (absorption - (before - after)).max(0.0))
}

/// 官方 `ServerPlayer.doTick` 每刻递减 `damageCooldownTime`，
/// `LivingEntity.baseTick` 递减 `hurtTime`，`invulnerableTime` 同样递减。
pub fn tick_cooldowns(
    damage_cooldown: i32,
    hurt_time: i32,
    invulnerable_time: i32,
) -> (i32, i32, i32) {
    (
        (damage_cooldown - 1).max(0),
        (hurt_time - 1).max(0),
        (invulnerable_time - 1).max(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_scaling_matches_the_official_branches() {
        assert_eq!(scale_by_difficulty(7.0, 0, true), 0.0);
        assert_eq!(scale_by_difficulty(7.0, 1, true), 4.5);
        assert_eq!(scale_by_difficulty(7.0, 2, true), 7.0);
        assert_eq!(scale_by_difficulty(7.0, 3, true), 10.5);
        assert_eq!(scale_by_difficulty(0.5, 1, true), 0.5);
        assert_eq!(scale_by_difficulty(7.0, 0, false), 7.0);
    }

    #[test]
    fn armor_and_protection_shrink_damage() {
        // 铁甲全套：armor=15、toughness=0；20 点伤害 -> 20*(1-(15-10)/25)=16。
        assert_eq!(damage_after_absorb(20.0, 15.0, 0.0), 16.0);
        // 保护 IV 四件共 16 级 -> * (1 - 16/25) = 0.36。
        assert_eq!(damage_after_magic_absorb(16.0, 16.0), 16.0 * 0.36);
        let values = ArmorValues {
            armor: 15.0,
            toughness: 0.0,
            protection: 16.0,
            resistance: None,
        };
        assert_eq!(values.reduce(20.0, DamageKind::MobAttack), 16.0 * 0.36);
        // 绕过护甲的种类仍吃保护附魔，只有 `bypasses_enchantments` 才跳过。
        assert_eq!(values.reduce(20.0, DamageKind::Generic), 20.0 * 0.36);
        assert_eq!(
            ArmorValues::default().reduce(20.0, DamageKind::Generic),
            20.0
        );
    }

    #[test]
    fn resistance_multiplier_follows_the_official_formula() {
        let resistance = |amplifier| ArmorValues {
            resistance: Some(amplifier),
            ..ArmorValues::default()
        };
        // `max(0, 25 - 5*(amp+1))/25`：0 级 20/25，1 级 15/25，4 级 0。
        assert_eq!(resistance(0).reduce(20.0, DamageKind::MobAttack), 16.0);
        assert_eq!(resistance(1).reduce(20.0, DamageKind::MobAttack), 12.0);
        assert_eq!(resistance(4).reduce(20.0, DamageKind::MobAttack), 0.0);
        // 改坏确认：等级 1 不是 20/25 的倍率。
        assert_ne!(resistance(1).reduce(20.0, DamageKind::MobAttack), 16.0);
        // 虚空的伤害绕过抗性。
        assert_eq!(resistance(2).reduce(20.0, DamageKind::OutOfWorld), 20.0);
    }

    #[test]
    fn absorption_is_consumed_before_health() {
        assert_eq!(absorb(5.0, 2.0), (3.0, 0.0));
        assert_eq!(absorb(1.0, 5.0), (0.0, 4.0));
        // `f32::MAX - 5` 在 IEEE 里仍是 `f32::MAX`，官方的减号也保留吸收值。
        assert_eq!(absorb(f32::MAX, 5.0), (f32::MAX, 5.0));
    }

    #[test]
    fn cooldown_only_takes_the_excess_of_a_stronger_hit() {
        let args = |amount: f32, cooldown: i32, last: f32| {
            hurt(
                amount,
                DamageKind::MobAttack,
                2,
                false,
                20.0,
                0.0,
                0.0,
                cooldown,
                last,
                0,
                10,
                ArmorValues::default(),
            )
        };
        let first = args(6.0, 0, 0.0).unwrap();
        assert_eq!(first.health, 14.0);
        assert_eq!(first.damage_cooldown, 20);
        assert_eq!(first.hurt_time, 10);
        assert!(first.full_hit);
        assert_eq!(args(4.0, 15, 6.0), None);
        let stronger = args(9.0, 15, 6.0).unwrap();
        assert!(!stronger.full_hit);
        assert_eq!(stronger.damage, 3.0);
        assert_eq!(stronger.health, 17.0);
        assert_eq!(stronger.damage_cooldown, 15);
        assert_eq!(stronger.last_hurt, 9.0);
    }

    #[test]
    fn exhaustion_accumulates_only_when_health_is_lost() {
        let args = |amount: f32, health: f32, kind: DamageKind| {
            hurt(
                amount,
                kind,
                2,
                false,
                health,
                0.0,
                3.0,
                0,
                0.0,
                0,
                10,
                ArmorValues::default(),
            )
        };
        assert_eq!(
            args(1.0, 20.0, DamageKind::PlayerAttack)
                .unwrap()
                .exhaustion,
            3.1
        );
        assert_eq!(
            args(1.0, 20.0, DamageKind::Generic).unwrap().exhaustion,
            3.0
        );
        assert_eq!(args(1.0, 0.0, DamageKind::PlayerAttack), None);
    }
}

#[cfg(test)]
mod debug_armor {
    use super::*;
    use mc_bamboo_data::items;

    #[test]
    fn iron_armor_defaults_sum_to_fifteen() {
        let empty = ItemStack::empty();
        let stacks: [&ItemStack; 6] = [
            &empty,
            &empty,
            &ItemStack::new(items::IRON_BOOTS, 1),
            &ItemStack::new(items::IRON_LEGGINGS, 1),
            &ItemStack::new(items::IRON_CHESTPLATE, 1),
            &ItemStack::new(items::IRON_HELMET, 1),
        ];
        let refs: Vec<&ItemStack> = stacks.to_vec();
        let array: [&ItemStack; 6] = [refs[0], refs[1], refs[2], refs[3], refs[4], refs[5]];
        assert_eq!(
            armor_values(&array).armor,
            15.0,
            "{:?}",
            armor_values(&array)
        );
    }
}
