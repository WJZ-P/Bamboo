//! 玩家的生命、受伤冷却和饥饿状态，以及和存档字段的双向转换。
//!
//! 官方 `Player.hurtServer` 的状态都在 `Player` / `LivingEntity` 上。这一期
//! 只存相对倒计时（`hurt_time`、`damage_cooldown`、`hurt_duration`、
//! `invulnerable_time`），不存岛时钟的绝对时刻，拆岛 / 并岛时不用平移。
//!
//! 血量降到 0 不触发死亡：掉落、死亡消息和重生留到 1b。
//!
//! Player health, hurt cooldowns and hunger state, plus the save-data mapping.
//! Only relative countdowns are stored; health reaching zero does not trigger
//! death (drops, messages and respawn belong to 1b).

use mc_bamboo_blocks::entity::mob::damage::{
    self as rules, ArmorValues, DifficultyId, HurtOutcome,
};
use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::attribute;
use mc_bamboo_protocol::{components::ComponentValue, item::ItemStack};
use mc_bamboo_storage::player_data::PlayerData;

/// 官方 `FoodData` 的四个字段。`tick_timer` 这一期不推进，只存读档。
///
/// The four `FoodData` fields. `tick_timer` is persisted but not ticked yet.
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
    /// `deathTime`：这一期恒为 0，死亡流程留给 1b。
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
            hurt_duration: 10,
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

    /// 官方 `Player.attack` 里 `ATTACK_DAMAGE` 加上主手武器的 `attribute_modifiers`。
    /// 这一期没有冷却 / 暴击 / 附魔加成，只做一次 `player_attack` 伤害。
    pub fn attack_damage(held: &ItemStack) -> f32 {
        let mut damage = mc_bamboo_data::registries::attribute_defaults::ATTACK_DAMAGE as f32;
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
            && let Some(at) = source.position
        {
            // `ServerPlayer.indicateDamage`: atan2(dz, dx) 转角度再减玩家朝向。
            let dx = at.x - position.x;
            let dz = at.z - position.z;
            self.hurt_dir = mc_bamboo_core::mth::atan2(dz, dx).to_degrees() as f32;
        }
        Some(outcome)
    }

    /// 把官方存盘字段写进运行时。
    pub fn restore(&mut self, data: &PlayerData) {
        self.health = data.health.clamp(0.0, rules::MAX_HEALTH);
        self.absorption = data.absorption.clamp(0.0, rules::MAX_ABSORPTION);
        self.hurt_time = i32::from(data.hurt_time).max(0);
        self.hurt_duration = self.hurt_time.max(10);
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
        restored.restore(&data);
        assert_eq!(restored.health, 7.5);
        assert_eq!(restored.absorption, 3.0);
        assert_eq!(restored.hurt_time, 6);
        assert_eq!(restored.food, state.food);
    }
}
