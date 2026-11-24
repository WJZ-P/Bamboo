//! 玩家的生命、受伤冷却和饥饿状态，以及和存档字段的双向转换。
//!
//! 官方 `Player.hurtServer` 的状态都在 `Player` / `LivingEntity` 上。这一期
//! 只存相对倒计时（`hurt_time`、`damage_cooldown`、`hurt_duration`、
//! `invulnerable_time`），不存岛时钟的绝对时刻，拆岛 / 并岛时不用平移。
//! 死亡状态由 `player_death` / `player_respawn` 驱动，这里只管数值。
//!
//! Player health, hurt cooldowns and hunger state, plus the save-data mapping.
//! Only relative countdowns are stored; the death and respawn flow lives in
//! `player_death` / `player_respawn`.

use mc_bamboo_blocks::entity::mob::damage::{
    self as rules, ArmorValues, DifficultyId, HurtOutcome,
};
use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::attribute;
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};
use mc_bamboo_storage::player_data::PlayerData;

/// 官方 `FoodData` 的四个字段。`tick_timer` 按 `FoodData.tick` 推进并存读档。
///
/// The four `FoodData` fields; `tick_timer` follows `FoodData.tick`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FoodData {
    pub food_level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub tick_timer: i32,
}

impl Default for FoodData {
    fn default() -> Self {
        Self {
            food_level: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            tick_timer: 0,
        }
    }
}

/// 官方 `Player` / `LivingEntity` 里受伤链路用到的全部字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlayerHealth {
    pub health: f32,
    pub absorption: f32,
    /// `hurtTime`：受伤动画倒计时，每刻减 1。
    pub hurt_time: i32,
    /// `hurtDuration`：本次受伤动画的总时长。
    pub hurt_duration: i32,
    /// `damageCooldownTime`：无敌帧，完整受击后 20，每刻减 1。
    pub damage_cooldown: i32,
    /// `lastHurt`：上一次伤害的原始量，冷却期内只吃多出来的那部分。
    pub last_hurt: f32,
    /// `invulnerableTime`：临时的实体无敌，不是受伤冷却。
    pub invulnerable_time: i32,
    /// `deathTime`：死亡动画已经推进的刻数，重生时归零。
    pub death_time: i32,
    pub food: FoodData,
    /// 上次受伤的伤害种类（不含攻击者），`lastDamageSource`。
    pub last_damage_source: Option<mc_bamboo_blocks::entity::mob::DamageKind>,
    /// 上次受伤的方向，用于受伤动画的 `yaw`。
    pub hurt_dir: f32,
}

impl Default for PlayerHealth {
    fn default() -> Self {
        Self {
            health: rules::MAX_HEALTH,
            absorption: 0.0,
            hurt_time: 0,
            hurt_duration: 0,
            damage_cooldown: 0,
            last_hurt: 0.0,
            invulnerable_time: 0,
            death_time: 0,
            food: FoodData::default(),
            last_damage_source: None,
            hurt_dir: 0.0,
        }
    }
}

/// `FoodData.tick` 一刻度后果，调用方按它改血量 / 发包 / 生成饥饿伤害。
///
/// One tick of `FoodData.tick`; the caller applies the heal or starve damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FoodTick {
    /// 什么也没发生。
    Idle,
    /// 回 `amount` 点血，`FoodData.tick` 里的 `heal()`。
    Heal(f32),
    /// 对玩家造成一点饥饿伤害，官方 `damageSources().starve()`。
    Starve,
}

/// 官方 `ServerPlayer.tickRegeneration` 在和平难度下的回血 / 回饥饿。
/// 只在 `natural_health_regeneration` 打开时调用；两处计时都取
/// `tickCount` 的模，Bamboo 存的是相对倒计时，拆岛不需平移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PeacefulRegen {
    pub heal: bool,
    pub saturation_up: bool,
    pub food_up: bool,
}

impl PlayerHealth {
    /// 官方 `LivingEntity.isDeadOrDying()`：血量不大于 0 或已经死了。
    pub fn is_dead_or_dying(self) -> bool {
        self.health <= 0.0 || self.death_time > 0
    }

    /// `ServerPlayer.doTick` 里三个倒计时的推进。
    pub fn tick_cooldowns(&mut self) {
        let (cooldown, hurt, invulnerable) =
            rules::tick_cooldowns(self.damage_cooldown, self.hurt_time, self.invulnerable_time);
        self.damage_cooldown = cooldown;
        self.hurt_time = hurt;
        self.invulnerable_time = invulnerable;
    }

    /// 官方 `FoodData.tick(ServerPlayer)` 的完整分支。
    ///
    /// `exhaustion > 4` 先消耗 4 点：饱和度够就扣饱和度，否则非和平扣饥饿。
    /// 之后按 `natural_health_regeneration` 走「满饥饿快速回血 / 半饱慢回血 /
    /// 饥饿掉血」三段；没落在任何一段里 `tickTimer` 归零。
    /// `isHurt()` 是 `getHealth() > 0 && getHealth() < getMaxHealth()`。
    pub fn tick_food(&mut self, difficulty: DifficultyId, natural_regen: bool) -> FoodTick {
        let food = &mut self.food;
        if food.exhaustion > 4.0 {
            food.exhaustion -= 4.0;
            if food.saturation > 0.0 {
                food.saturation = (food.saturation - 1.0).max(0.0);
            } else if difficulty != 0 {
                food.food_level = (food.food_level - 1).max(0);
            }
        }
        let is_hurt = self.health > 0.0 && self.health < rules::MAX_HEALTH;
        if natural_regen && food.saturation > 0.0 && is_hurt && food.food_level >= 20 {
            food.tick_timer += 1;
            if food.tick_timer >= 10 {
                let amount = food.saturation.min(6.0);
                food.tick_timer = 0;
                food.exhaustion = (food.exhaustion + amount).min(rules::MAX_EXHAUSTION);
                return FoodTick::Heal(amount / 6.0);
            }
        } else if natural_regen && food.food_level >= 18 && is_hurt {
            food.tick_timer += 1;
            if food.tick_timer >= 80 {
                food.tick_timer = 0;
                food.exhaustion = (food.exhaustion + 6.0).min(rules::MAX_EXHAUSTION);
                return FoodTick::Heal(1.0);
            }
        } else if food.food_level == 0 {
            food.tick_timer += 1;
            if food.tick_timer >= 80 {
                food.tick_timer = 0;
                // 官方：HARD 或 hp<=10 或 (hp>1 且 NORMAL) 时才真的 hurtServer。
                if difficulty == 3 || self.health <= 10.0 || (difficulty == 2 && self.health > 1.0)
                {
                    return FoodTick::Starve;
                }
            }
        } else {
            food.tick_timer = 0;
        }
        FoodTick::Idle
    }

    /// 官方 `ServerPlayer.tickRegeneration`：和平 + 自然回血时每 20 刻回 1 血
    /// 并加 1 饱和度（上限 20），每 10 刻加 1 饥饿（上限 20）。
    /// `tick` 是玩家的实体 tick 数，调用方存相对计数。
    pub fn tick_peaceful(&mut self, tick: i32) -> PeacefulRegen {
        let mut result = PeacefulRegen {
            heal: false,
            saturation_up: false,
            food_up: false,
        };
        if tick % 20 == 0 {
            if self.health > 0.0 && self.health < rules::MAX_HEALTH {
                self.health = (self.health + 1.0).min(rules::MAX_HEALTH);
                result.heal = true;
            }
            if self.food.saturation < 20.0 {
                self.food.saturation = (self.food.saturation + 1.0).min(20.0);
                result.saturation_up = true;
            }
        }
        if tick % 10 == 0 && self.food.food_level < 20 {
            self.food.food_level += 1;
            result.food_up = true;
        }
        result
    }

    /// `LivingEntity.heal`：夹到最大血量。
    pub fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(rules::MAX_HEALTH);
    }

    /// 官方 `Player.getArmorCoverPercentage()`：四格盔甲里非空的比例。
    pub fn armor_cover(equipment: &[&ItemStack; 6]) -> f32 {
        let filled = equipment[2..]
            .iter()
            .filter(|stack| !stack.is_empty())
            .count();
        if equipment.len() < 6 {
            0.0
        } else {
            filled as f32 / 4.0
        }
    }

    /// 六个装备槽的护甲值、韧性和保护附魔等级。
    pub fn armor_values(equipment: &[&ItemStack; 6]) -> ArmorValues {
        rules::armor_values(equipment)
    }

    /// 官方 `Player.createAttributes()` 把玩家的 `ATTACK_DAMAGE` 基值设成 1.0，
    /// 不是属性的注册表缺省 2.0；再加主手武器的 `attribute_modifiers`。
    /// 冷却 / 暴击 / 附魔加成在 [`crate::player_attack`] 里接着算。
    ///
    /// `Player.createAttributes()` sets the player's `ATTACK_DAMAGE` base to 1.0,
    /// not the registry default 2.0; then add the main-hand weapon's
    /// `attribute_modifiers`. Cooldown / crit / enchantments are applied in
    /// [`crate::player_attack`].
    pub fn attack_damage(held: &ItemStack) -> f32 {
        let mut damage = 1.0_f32;
        if held.is_empty() {
            return damage;
        }
        let Ok(patch) = mc_bamboo_protocol::components::ComponentPatch::from_opaque(held) else {
            return damage;
        };
        if let Some(ComponentValue::AttributeModifiers(entries)) =
            mc_bamboo_protocol::components::defaults::resolve(
                held.item,
                &patch,
                mc_bamboo_data::registries::data_component_type::ATTRIBUTE_MODIFIERS,
            )
        {
            for entry in entries {
                if entry.attribute == attribute::ATTACK_DAMAGE
                    && entry.modifier.operation == 0
                    && (entry.slot == 0 || entry.slot == 1 || entry.slot == 3)
                {
                    damage += entry.modifier.amount as f32;
                }
            }
        }
        damage
    }

    /// 一次完整受伤。返回状态的增量，包和击退由调用方按 `HurtOutcome` 发。
    pub fn hurt(
        &mut self,
        amount: f32,
        source: mc_bamboo_blocks::entity::mob::DamageSource,
        difficulty: DifficultyId,
        armor: ArmorValues,
        position: Vec3,
    ) -> Option<HurtOutcome> {
        if self.is_dead_or_dying() || self.invulnerable_time > 0 {
            return None;
        }
        let outcome = rules::hurt(
            amount,
            source.kind,
            difficulty,
            source.scales_with_difficulty(),
            self.health,
            self.absorption,
            self.food.exhaustion,
            self.damage_cooldown,
            self.last_hurt,
            self.hurt_time,
            self.hurt_duration,
            armor,
        )?;
        self.health = outcome.health;
        self.absorption = outcome.absorption;
        self.food.exhaustion = outcome.exhaustion;
        self.damage_cooldown = outcome.damage_cooldown;
        self.last_hurt = outcome.last_hurt;
        self.hurt_time = outcome.hurt_time;
        self.hurt_duration = outcome.hurt_duration;
        self.last_damage_source = Some(source.kind);
        if outcome.full_hit
            && let Some((dx, dz)) = source.knockback_direction(position)
        {
            // ServerPlayer.indicateDamage：同一击退方向先取 atan2(dz, dx)，再减玩家朝向。
            // ServerPlayer.indicateDamage takes atan2(dz, dx) of the knockback direction.
            // 官方 `Mth.atan2` 再乘 `57.2957763671875d`（常量，不是 180/PI）。
            self.hurt_dir = (mc_bamboo_core::mth::atan2(dz, dx) * 57.295_776_367_187_5_f64) as f32;
        }
        Some(outcome)
    }

    /// 把官方存盘字段写进运行时。血量按当前 `MAX_HEALTH` 夹取（官方先读 `attributes`
    /// 再读 `Health`，所以高于 20 的最大生命不会被截断）。
    ///
    /// Restores the saved fields into runtime state, clamping health to the
    /// resolved `MAX_HEALTH` (vanilla reads `attributes` before `Health`).
    pub fn restore(&mut self, data: &PlayerData, max_health: f32) {
        self.health = data.health.clamp(0.0, max_health.max(1.0));
        // 官方 `LivingEntity.readAdditionalSaveData` 走 `internalSetAbsorptionAmount`，
        // 不夹到属性上限；`setAbsorptionAmount` 的夹取只发生在效果变化那一路上。
        self.absorption = data.absorption.max(0.0);
        self.hurt_time = i32::from(data.hurt_time).max(0);
        self.hurt_duration = 0;
        self.damage_cooldown = 0;
        self.last_hurt = 0.0;
        self.invulnerable_time = 0;
        self.death_time = i32::from(data.death_time).max(0);
        self.food = FoodData {
            food_level: data.food_level,
            saturation: data.food_saturation,
            exhaustion: data.food_exhaustion,
            tick_timer: data.food_tick_timer,
        };
        self.last_damage_source = None;
        self.hurt_dir = 0.0;
    }

    /// 写回官方存盘字段。
    pub fn save(&self, data: &mut PlayerData) {
        data.health = self.health;
        data.absorption = self.absorption;
        data.hurt_time = self.hurt_time.clamp(0, i16::MAX as i32) as i16;
        data.death_time = self.death_time.clamp(0, i16::MAX as i32) as i16;
        data.food_level = self.food.food_level;
        data.food_saturation = self.food.saturation;
        data.food_exhaustion = self.food.exhaustion;
        data.food_tick_timer = self.food.tick_timer;
    }
}

/// 标记：这一期不建新的 `IslandPart`，`PlayerHealth` 整行跟着 `Players` 搬。
/// 岛合并时所有字段都是相对倒计时，不需要时钟平移。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_official_new_player() {
        let state = PlayerHealth::default();
        assert_eq!(state.health, 20.0);
        assert_eq!(state.absorption, 0.0);
        assert_eq!(state.food.food_level, 20);
        assert_eq!(state.food.saturation, 5.0);
        assert_eq!(state.food.exhaustion, 0.0);
        assert_eq!(state.food.tick_timer, 0);
    }

    #[test]
    fn save_and_restore_round_trip_health_and_food() {
        let state = PlayerHealth {
            health: 7.5,
            absorption: 3.0,
            hurt_time: 6,
            food: FoodData {
                food_level: 13,
                saturation: 1.25,
                exhaustion: 3.75,
                tick_timer: 42,
            },
            ..PlayerHealth::default()
        };
        let mut data = PlayerData::default();
        state.save(&mut data);
        assert_eq!(data.health, 7.5);
        assert_eq!(data.absorption, 3.0);
        assert_eq!(data.food_tick_timer, 42);
        let mut restored = PlayerHealth::default();
        restored.restore(&data, rules::MAX_HEALTH);
        assert_eq!(restored.health, 7.5);
        assert_eq!(restored.absorption, 3.0);
        assert_eq!(restored.hurt_time, 6);
        assert_eq!(restored.food, state.food);
    }
}
