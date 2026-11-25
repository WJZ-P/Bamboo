//! 生物的效果实例和每刻推进。官方 `LivingEntity.tickEffects` /
//! `MobEffectInstance.tickServer` / `MobEffects` 静态表。
//!
//! 状态只存相对剩余刻数（`duration` 是倒计时，`INFINITE` 表示无限），不存岛时钟的
//! 绝对时刻。这里是纯函数：调用方（牛的 `living::base_tick`、玩家的每刻阶段）按
//! [`EffectEvents`] 去改属性、发包和施加治疗 / 伤害。玩家和牛共用这一份表，所以
//! 拆岛 / 并岛时整行跟着实体或玩家搬，不需要平移时钟。
//!
//! Mob effect instances and their per-tick advance, following vanilla
//! `LivingEntity.tickEffects` / `MobEffectInstance.tickServer` / `MobEffects`.
//! Durations are relative countdowns, never absolute island-clock instants.
//! Everything here is pure; the caller applies the returned events.

use std::collections::BTreeMap;

use mc_bamboo_data::registries::{attribute, mob_effect};

use super::attributes::{Modifier, Operation};

/// 官方 `MobEffectInstance.INFINITE_DURATION`。时长为 `-1` 时不减。
///
/// Vanilla `MobEffectInstance.INFINITE_DURATION`; a `-1` duration never ticks down.
pub const INFINITE: i32 = -1;

/// `tickEffects` 里每 600 刻补一次 `onEffectUpdated` 的周期。
///
/// The 600-tick re-sync interval in `tickEffects`.
pub const RESYNC_INTERVAL: i32 = 600;

/// `MobEffectInstance.tickServer` 的一刻结果。`Ended` 表示
/// `applyEffectTick` 返回 false（目前只有吸收血量用完）：官方在那一刻直接
/// 返回 false，既不减时长也不降级，`tickEffects` 立刻移除这一项。
/// 吸收血量由调用方通过 `state` 传入。
///
/// One `MobEffectInstance.tickServer` result. `Ended` means `applyEffectTick`
/// returned false (only absorption running out): vanilla returns immediately
/// without ticking the duration down, and `tickEffects` removes the entry.
/// The caller supplies its current absorption in `state`.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectTickOutcome {
    Continues(Vec<EffectAction>),
    Ended,
}

/// `tickServer` 要读的实体状态。纯函数不直接碰实体。
///
/// Entity state `tickServer` reads; pure functions never touch the entity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EffectState {
    pub health: f32,
    pub max_health: f32,
    pub absorption: f32,
}

/// 官方 `MobEffectInstance` 的六个字段加隐藏效果。`effect` 是注册表 ID。
///
/// The six fields of vanilla `MobEffectInstance` plus its hidden effect.
/// `effect` is a registry id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInstance {
    pub effect: i32,
    /// 剩余刻数；`INFINITE` 表示无限，0 表示这一刻结束。
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    /// `isVisible()`，官方 `show_particles`。
    pub visible: bool,
    pub show_icon: bool,
    /// `hiddenEffect`：叠加时藏在下层的低等级 / 短时长效果。
    pub hidden: Option<Box<EffectInstance>>,
}

impl EffectInstance {
    /// `MobEffectInstance(Holder, duration)`：等级 0、非环境、可见、显示图标。
    ///
    /// `MobEffectInstance(Holder, duration)`: amplifier 0, not ambient, visible, icon on.
    pub fn new(effect: i32, duration: i32) -> Self {
        Self {
            effect,
            duration,
            amplifier: 0,
            ambient: false,
            visible: true,
            show_icon: true,
            hidden: None,
        }
    }

    /// 构造时把等级夹到官方的 0..255，客户端 / 命令传来的越界值不会 panic。
    ///
    /// Clamps the amplifier to vanilla's 0..255 so untrusted input cannot panic.
    pub fn with_amplifier(effect: i32, duration: i32, amplifier: i32) -> Self {
        let mut result = Self::new(effect, duration);
        result.amplifier = amplifier.clamp(0, 255);
        result
    }

    /// 官方五参数构造：`ambient` / `visible` 由调用方给，
    /// `showIcon` 按官方缺省等于 `visible`。
    ///
    /// The five-argument vanilla constructor; `showIcon` defaults to `visible`.
    pub fn with_flags(
        effect: i32,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
    ) -> Self {
        let mut result = Self::with_amplifier(effect, duration, amplifier);
        result.ambient = ambient;
        result.visible = visible;
        result.show_icon = visible;
        result
    }

    pub fn is_infinite(&self) -> bool {
        self.duration == INFINITE
    }

    /// `tickServer` 只在无限或 `duration > 0` 时继续。
    fn has_remaining_duration(&self) -> bool {
        self.is_infinite() || self.duration > 0
    }

    /// `isShorterDurationThan`：无限时长永远比任何有限时长长。
    fn is_shorter_than(&self, other: &Self) -> bool {
        !self.is_infinite() && (self.duration < other.duration || other.is_infinite())
    }

    /// `MobEffectInstance.update(other)`：合并同一种效果，返回有没有变化。
    ///
    /// `MobEffectInstance.update(other)`: merges another instance of the same
    /// effect and reports whether anything changed.
    pub fn update(&mut self, other: &Self) -> bool {
        let mut changed = false;
        // 官方 `MobEffectInstance.update(other)` 两处 `isShorterDurationThan`
        // 的方向不同（探针验证）：
        // - 更高等级分支：`other.isShorterDurationThan(this)` —— 新的更短时把
        //   当前实例存进隐藏层；
        // - 同级 / 更低分支：`this.isShorterDurationThan(other)` —— 新的更长时
        //   才刷新，新的更短保持不变。第二处方向写反会让重复挂同等级更长效果
        //   时反而缩短，例如远古守卫者给剩余 1199 刻的玩家重挂 6000 刻挖掘疲劳。
        //
        // Vanilla `MobEffectInstance.update(other)` uses opposite directions in
        // the two `isShorterDurationThan` checks (verified with a Java probe):
        // higher amplifier keeps the current instance as a hidden effect when the
        // new one is shorter; same/lower amplifier refreshes only when the new
        // one is longer. Reversing the second check shortens longer re-applies.
        if other.amplifier > self.amplifier {
            if other.is_shorter_than(self) {
                // 官方把当前实例复制成隐藏效果，并把旧链挂在新副本下面。
                // Vanilla copies the current instance as the hidden effect and
                // chains the old hidden effect below the copy.
                let old = self.hidden.take();
                let mut copy = self.clone();
                copy.hidden = old;
                self.hidden = Some(Box::new(copy));
            }
            self.amplifier = other.amplifier;
            self.duration = other.duration;
            changed = true;
        } else if self.is_shorter_than(other) {
            if other.amplifier == self.amplifier {
                self.duration = other.duration;
                changed = true;
            } else if let Some(hidden) = &mut self.hidden {
                changed |= hidden.update(other);
            } else {
                self.hidden = Some(Box::new(other.clone()));
            }
        }
        if other.ambient || (self.ambient && changed) {
            self.ambient = other.ambient;
            changed = true;
        }
        if other.visible != self.visible {
            self.visible = other.visible;
            changed = true;
        }
        if other.show_icon != self.show_icon {
            self.show_icon = other.show_icon;
            changed = true;
        }
        changed
    }

    /// `tickDownDuration`：隐藏效果和自己一起减，无限 / 0 不变。
    fn tick_down_duration(&mut self) {
        if let Some(hidden) = &mut self.hidden {
            hidden.tick_down_duration();
        }
        if self.is_infinite() || self.duration == 0 {
            return;
        }
        self.duration -= 1;
    }

    /// `downgradeToHiddenEffect`：当前层归零时把隐藏层的详情提上来。
    fn downgrade_to_hidden(&mut self) -> bool {
        if self.duration != 0 {
            return false;
        }
        let Some(hidden) = self.hidden.take() else {
            return false;
        };
        self.duration = hidden.duration;
        self.amplifier = hidden.amplifier;
        self.ambient = hidden.ambient;
        self.visible = hidden.visible;
        self.show_icon = hidden.show_icon;
        self.hidden = hidden.hidden;
        true
    }

    /// 官方 `MobEffectInstance.tickServer` 的一刻。
    /// `entity_tick_count` 是无限时长效果用的 `LivingEntity.tickCount`；
    /// 有限效果不读它，所以拆岛平移不会改变行为。
    /// `Ended` 表示 `applyEffectTick` 返回 false，时长没有减。
    ///
    /// One tick of vanilla `MobEffectInstance.tickServer`. `entity_tick_count`
    /// is `LivingEntity.tickCount`, used only by infinite effects; finite ones
    /// never read it, so island clock rebasing cannot change their behaviour.
    /// `Ended` means `applyEffectTick` returned false and the duration did not tick.
    pub fn tick_server(&mut self, entity_tick_count: i32, state: EffectState) -> EffectTickOutcome {
        if !self.has_remaining_duration() {
            return EffectTickOutcome::Ended;
        }
        let duration = if self.is_infinite() {
            entity_tick_count
        } else {
            self.duration
        };
        // `PoisonMobEffect` 只在血量高于 1 时调 `hurtServer`，但仍然返回 true；
        // 条件放在动作里，由调用方施加。
        let mut actions = Vec::new();
        if should_apply_this_tick(self.effect, duration, self.amplifier) {
            if self.effect == mob_effect::ABSORPTION && state.absorption <= 0.0 {
                return EffectTickOutcome::Ended;
            }
            actions = effect_tick_actions(self.effect, self.amplifier);
        }
        self.tick_down_duration();
        if self.downgrade_to_hidden() {
            // 隐藏效果提上来时等级可能变了，属性由 `tick` 的返回值处理。
        }
        EffectTickOutcome::Continues(actions)
    }

    /// 存档 codec 的 `show_icon` 缺省值由调用方决定；这里只提供字段。
    pub fn has_remaining(&self) -> bool {
        self.has_remaining_duration()
    }
}

/// `shouldApplyEffectTickThisTick(int duration, int amplifier)`。
///
/// 再生 `50 >> amp`、中毒 `25 >> amp`、凋零 `40 >> amp`；商为 0 时每刻都跑。
/// 饥饿效果每刻跑；瞬间效果只在 `duration >= 1` 时跑。不祥之兆每刻跑
/// （进村判定在调用方），袭击之兆只在倒计时最后一刻 `duration == 1` 跑。
/// 其余效果不跑（返回空动作）。
///
/// `shouldApplyEffectTickThisTick`: regeneration 50>>amp, poison 25>>amp,
/// wither 40>>amp; a zero interval runs every tick. Hunger runs every tick and
/// instantaneous effects run when duration >= 1. Bad omen runs every tick (the
/// village check lives in the caller), raid omen only on its last tick
/// (`duration == 1`). Others do nothing.
pub fn should_apply_this_tick(effect: i32, duration: i32, amplifier: i32) -> bool {
    match effect {
        mob_effect::REGENERATION => interval_tick(duration, 50 >> amplifier.min(30)),
        mob_effect::POISON => interval_tick(duration, 25 >> amplifier.min(30)),
        mob_effect::WITHER => interval_tick(duration, 40 >> amplifier.min(30)),
        mob_effect::HUNGER => true,
        // `BadOmenMobEffect.shouldApplyEffectTickThisTick` 恒真：每一刻都要判定
        // 玩家是否进了村庄。转换 / 移除效果这些副作用在调用方落地。
        //
        // Vanilla `BadOmenMobEffect` runs every tick so the caller can test for
        // village entry; the conversion and removal are the caller's side effects.
        mob_effect::BAD_OMEN => true,
        // `RaidOmenMobEffect` 只在倒计时最后一刻触发 `createOrExtendRaid`。
        // Vanilla `RaidOmenMobEffect` fires only on the final tick.
        mob_effect::RAID_OMEN => duration == 1,
        mob_effect::INSTANT_HEALTH
        | mob_effect::INSTANT_DAMAGE
        | mob_effect::SATURATION
        | mob_effect::ABSORPTION => duration >= 1,
        _ => false,
    }
}

fn interval_tick(duration: i32, interval: i32) -> bool {
    interval <= 0 || duration % interval == 0
}

/// 一刻的持续效果动作。调用方按它改血量 / 饥饿 / 吸收。
///
/// One tick's continuous-effect actions. The caller applies them to health,
/// hunger and absorption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectAction {
    /// `RegenerationMobEffect`：血量低于上限时回 1 点。
    Heal(f32),
    /// `PoisonMobEffect`：血量高于 1 时 `magic` 1.0。血量条件由调用方判。
    MagicDamage(f32),
    /// `WitherMobEffect`：`wither` 1.0。伤害类型目前建模为 `Magic`，见差异文档。
    WitherDamage(f32),
    /// `HungerMobEffect`：`0.005 * (amplifier + 1)` 的饥饿消耗。
    Exhaustion(f32),
    /// `SaturationMobEffect`：`FoodData.eat(amplifier + 1, 1.0)`。
    Eat {
        nutrition: i32,
        saturation: f32,
    },
    /// `AbsorptionMobEffect.onEffectStarted`：把吸收血量抬到 `4 * (amplifier + 1)`。
    Absorption(f32),
    /// `AbsorptionMobEffect.applyEffectTick` 返回 false 时这一刻效果立刻结束。
    AbsorptionEnded,
    /// `HealOrHarmMobEffect`：`4 << amplifier` 治疗 / `6 << amplifier` magic 伤害。
    InstantHeal(f32),
    InstantDamage(f32),
}

/// `applyEffectTick` / `onEffectStarted` 的纯数值动作表。不祥之兆 / 袭击之兆
/// 的副作用（转换效果、`createOrExtendRaid`）要访问世界，不在这里，动作表为空。
///
/// The pure numeric action table of `applyEffectTick` / `onEffectStarted`. The
/// bad-omen / raid-omen side effects (converting the effect, starting a raid)
/// need world access and are not here, so their action list is empty.
pub fn effect_tick_actions(effect: i32, amplifier: i32) -> Vec<EffectAction> {
    let level = amplifier.max(0) as f32;
    match effect {
        mob_effect::REGENERATION => vec![EffectAction::Heal(1.0)],
        mob_effect::POISON => vec![EffectAction::MagicDamage(1.0)],
        mob_effect::WITHER => vec![EffectAction::WitherDamage(1.0)],
        mob_effect::HUNGER => vec![EffectAction::Exhaustion(0.005 * (level + 1.0))],
        mob_effect::SATURATION => vec![EffectAction::Eat {
            nutrition: amplifier + 1,
            saturation: 1.0,
        }],
        mob_effect::INSTANT_HEALTH => vec![EffectAction::InstantHeal(scaled(4.0, amplifier))],
        mob_effect::INSTANT_DAMAGE => vec![EffectAction::InstantDamage(scaled(6.0, amplifier))],
        _ => Vec::new(),
    }
}

/// Java `4 << amplifier` / `6 << amplifier`，越界时饱和到有限值，不 panic。
fn scaled(base: f32, amplifier: i32) -> f32 {
    let shift = amplifier.clamp(0, 24);
    base * 2.0_f32.powi(shift)
}

/// `AbsorptionMobEffect.onEffectStarted`：吸收效果刚施加时设一次吸收血量。
///
/// `AbsorptionMobEffect.onEffectStarted`: set absorption when the effect starts.
pub fn absorption_on_start(amplifier: i32) -> f32 {
    4.0 * (amplifier.clamp(0, 255) as f32 + 1.0)
}

/// `MobEffect.addAttributeModifiers` 的属性修饰表。`AttributeTemplate.create`
/// 用 `amount * (amplifier + 1)`，ID 用官方的 `effect.*` 文本。
///
/// The attribute modifiers from `MobEffect.addAttributeModifiers`.
/// `AttributeTemplate.create` multiplies the amount by `(amplifier + 1)`.
pub fn attribute_modifiers(effect: i32, amplifier: i32) -> Vec<(i32, Modifier)> {
    let (attr, id, amount, operation) = match effect {
        mob_effect::SPEED => (
            attribute::MOVEMENT_SPEED,
            "minecraft:effect.speed",
            f64::from(0.2_f32),
            Operation::AddMultipliedTotal,
        ),
        mob_effect::SLOWNESS => (
            attribute::MOVEMENT_SPEED,
            "minecraft:effect.slowness",
            f64::from(-0.15_f32),
            Operation::AddMultipliedTotal,
        ),
        mob_effect::HASTE => (
            attribute::ATTACK_SPEED,
            "minecraft:effect.haste",
            f64::from(0.1_f32),
            Operation::AddMultipliedTotal,
        ),
        mob_effect::MINING_FATIGUE => (
            attribute::ATTACK_SPEED,
            "minecraft:effect.mining_fatigue",
            f64::from(-0.1_f32),
            Operation::AddMultipliedTotal,
        ),
        mob_effect::STRENGTH => (
            attribute::ATTACK_DAMAGE,
            "minecraft:effect.strength",
            3.0,
            Operation::AddValue,
        ),
        mob_effect::JUMP_BOOST => (
            attribute::SAFE_FALL_DISTANCE,
            "minecraft:effect.jump_boost",
            1.0,
            Operation::AddValue,
        ),
        mob_effect::WEAKNESS => (
            attribute::ATTACK_DAMAGE,
            "minecraft:effect.weakness",
            -4.0,
            Operation::AddValue,
        ),
        mob_effect::HEALTH_BOOST => (
            attribute::MAX_HEALTH,
            "minecraft:effect.health_boost",
            4.0,
            Operation::AddValue,
        ),
        mob_effect::ABSORPTION => (
            attribute::MAX_ABSORPTION,
            "minecraft:effect.absorption",
            4.0,
            Operation::AddValue,
        ),
        mob_effect::LUCK => (
            attribute::LUCK,
            "minecraft:effect.luck",
            1.0,
            Operation::AddValue,
        ),
        mob_effect::UNLUCK => (
            attribute::LUCK,
            "minecraft:effect.unluck",
            -1.0,
            Operation::AddValue,
        ),
        _ => return Vec::new(),
    };
    vec![(
        attr,
        Modifier {
            id: id.to_owned(),
            amount: amount * f64::from(amplifier.clamp(0, 255) + 1),
            operation,
            permanent: false,
        },
    )]
}

/// 官方 `MobEffect.isInstantaneous()`：`instant_health` / `instant_damage` /
/// `saturation`。
///
/// Vanilla `MobEffect.isInstantaneous()`.
pub fn is_instantaneous(effect: i32) -> bool {
    matches!(
        effect,
        mob_effect::INSTANT_HEALTH | mob_effect::INSTANT_DAMAGE | mob_effect::SATURATION
    )
}

/// 效果是不是有害的（官方 `MobEffectCategory.HARMFUL`），牛奶只清有害效果。
///
/// Whether the effect is harmful (`MobEffectCategory.HARMFUL`); milk only
/// removes harmful effects.
pub fn is_beneficial(effect: i32) -> bool {
    matches!(
        effect,
        mob_effect::SPEED
            | mob_effect::HASTE
            | mob_effect::STRENGTH
            | mob_effect::INSTANT_HEALTH
            | mob_effect::JUMP_BOOST
            | mob_effect::REGENERATION
            | mob_effect::RESISTANCE
            | mob_effect::FIRE_RESISTANCE
            | mob_effect::WATER_BREATHING
            | mob_effect::INVISIBILITY
            | mob_effect::NIGHT_VISION
            | mob_effect::HEALTH_BOOST
            | mob_effect::ABSORPTION
            | mob_effect::SATURATION
            | mob_effect::LUCK
            | mob_effect::SLOW_FALLING
            | mob_effect::CONDUIT_POWER
            | mob_effect::DOLPHINS_GRACE
            | mob_effect::HERO_OF_THE_VILLAGE
    )
}

/// `minecraft:mob_effect` 注册表里的颜色，实体数据的效果粒子列表用它。
/// 表来自 `MobEffects` 静态初始化的 `(category, color)`。
///
/// The effect colour from `MobEffects`, used by the entity-data particle list.
pub fn color(effect: i32) -> i32 {
    match effect {
        mob_effect::SPEED => 3_402_751,
        mob_effect::SLOWNESS => 9_154_528,
        mob_effect::HASTE => 14_270_531,
        mob_effect::MINING_FATIGUE => 4_866_583,
        mob_effect::STRENGTH => 16_762_624,
        mob_effect::INSTANT_HEALTH | mob_effect::SATURATION => 16_262_179,
        mob_effect::INSTANT_DAMAGE => 11_101_546,
        mob_effect::JUMP_BOOST => 16_646_020,
        mob_effect::NAUSEA => 5_578_058,
        mob_effect::REGENERATION => 13_458_603,
        mob_effect::RESISTANCE => 9_520_880,
        mob_effect::FIRE_RESISTANCE => 14_981_690,
        mob_effect::WATER_BREATHING => 3_032_581,
        mob_effect::INVISIBILITY => 8_356_758,
        mob_effect::BLINDNESS => 2_030_407,
        mob_effect::NIGHT_VISION => 2_030_713,
        mob_effect::HUNGER => 5_799_459,
        mob_effect::WEAKNESS => 4_736_728,
        mob_effect::POISON => 5_144_489,
        mob_effect::WITHER => 3_488_199,
        mob_effect::HEALTH_BOOST => 16_284_963,
        mob_effect::ABSORPTION => 2_445_989,
        mob_effect::GLOWING => 9_740_385,
        mob_effect::LEVITATION => 13_565_951,
        mob_effect::LUCK => 5_882_118,
        mob_effect::UNLUCK => 3_097_253,
        mob_effect::SLOW_FALLING => 16_773_079,
        mob_effect::CONDUIT_POWER => 195_041,
        mob_effect::DOLPHINS_GRACE => 1_908_845,
        mob_effect::BAD_OMEN => 7_457_848,
        mob_effect::HERO_OF_THE_VILLAGE => 4_547_555,
        mob_effect::DARKNESS => 2_696_993,
        _ => 0,
    }
}

/// 一次效果表变化的类型，`onEffectAdded` / `onEffectUpdated` / `onEffectsRemoved`。
///
/// One effect-map change, mirroring `onEffectAdded` / `onEffectUpdated` /
/// `onEffectsRemoved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectChange {
    /// 新增（发包时 `blend = true`）。
    Added(i32),
    /// 更新或 600 刻补发包（发包时 `blend = false`）。
    Updated(i32),
    /// 移除。
    Removed(i32),
}

/// `tickEffects` 一刻的结果：要发的包、要施加的动作、属性是否变了。
///
/// The result of one `tickEffects` call: packets to send, actions to apply,
/// and whether attributes changed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectEvents {
    pub changes: Vec<EffectChange>,
    pub actions: Vec<EffectTick>,
    /// `onEffectAdded` / `onEffectUpdated(true)` / `onEffectsRemoved` 会刷新属性。
    pub attributes_changed: bool,
    /// `effectsDirty`：隐身 / 发光 / 粒子元数据要重算。
    pub metadata_dirty: bool,
}

/// 带来源的一刻动作，调用方按效果 ID 决定伤害种类等。
///
/// One action with its source, so the caller can choose the damage type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectTick {
    pub effect: i32,
    pub action: EffectAction,
}

/// 官方 `LivingEntity.activeEffects` 表。按效果 ID 升序，遍历顺序确定，
/// 和线程数 / 岛数无关。
///
/// Vanilla `LivingEntity.activeEffects`, keyed by effect id. A `BTreeMap`
/// gives a deterministic iteration order independent of threads or islands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EffectMap {
    entries: BTreeMap<i32, EffectInstance>,
}

impl EffectMap {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, effect: i32) -> Option<&EffectInstance> {
        self.entries.get(&effect)
    }

    pub fn has(&self, effect: i32) -> bool {
        self.entries.contains_key(&effect)
    }

    /// 抗性效果的等级，减伤用；没有时是 `None`。
    ///
    /// The resistance amplifier, if present, for damage reduction.
    pub fn amplifier(&self, effect: i32) -> Option<i32> {
        self.entries.get(&effect).map(|entry| entry.amplifier)
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectInstance> {
        self.entries.values()
    }

    #[cfg(test)]
    pub(crate) fn get_mut_for_test(&mut self, effect: i32) -> &mut EffectInstance {
        self.entries.get_mut(&effect).unwrap()
    }

    /// `LivingEntity.addEffect`：`canBeAffected` 的实体类型过滤留给调用方；
    /// 返回这一刻的事件（新增 / 更新都会刷新属性和元数据）。
    ///
    /// `LivingEntity.addEffect`; entity-type immunity filtering belongs to the
    /// caller. Returns the events (adds and updates refresh attributes).
    pub fn add(&mut self, instance: EffectInstance) -> EffectEvents {
        let effect = instance.effect;
        let mut events = EffectEvents::default();
        if let Some(current) = self.entries.get_mut(&effect) {
            if current.update(&instance) {
                events.changes.push(EffectChange::Updated(effect));
                events.attributes_changed = true;
                events.metadata_dirty = true;
            }
        } else {
            self.entries.insert(effect, instance.clone());
            events.changes.push(EffectChange::Added(effect));
            events.attributes_changed = true;
            events.metadata_dirty = true;
            if effect == mob_effect::ABSORPTION {
                events.actions.push(EffectTick {
                    effect,
                    action: EffectAction::Absorption(absorption_on_start(instance.amplifier)),
                });
            }
        }
        events
    }

    /// `MobEffect.applyInstantaneousEffect`：药水 / `consume_effects` 直接施加瞬间
    /// 效果，不进 `activeEffects`（官方 `PotionContents.applyToLivingEntity`）。
    /// 命令 `/effect give` 走的是 `addEffect`，会在下一刻的 `tickEffects` 里结算。
    ///
    /// `MobEffect.applyInstantaneousEffect`: potions and consume effects apply
    /// instantaneous effects without putting them into `activeEffects`. The
    /// `/effect give` command uses `addEffect` and settles on the next tick.
    pub fn apply_instantaneous(&mut self, effect: i32, amplifier: i32) -> EffectEvents {
        let mut events = EffectEvents::default();
        for action in effect_tick_actions(effect, amplifier) {
            events.actions.push(EffectTick { effect, action });
        }
        events
    }

    /// `removeEffect` / `removeEffectNoUpdate`。返回有没有真的移除。
    ///
    /// `removeEffect` / `removeEffectNoUpdate`; reports whether it was present.
    pub fn remove(&mut self, effect: i32) -> EffectEvents {
        let mut events = EffectEvents::default();
        if self.entries.remove(&effect).is_some() {
            events.changes.push(EffectChange::Removed(effect));
            events.attributes_changed = true;
            events.metadata_dirty = true;
        }
        events
    }

    /// `removeAllEffects`。官方按 HashMap 顺序移除，这里按 ID 升序，包序列确定。
    ///
    /// `removeAllEffects`. Vanilla uses HashMap order; we use id order so the
    /// packet sequence is deterministic.
    pub fn remove_all(&mut self) -> EffectEvents {
        let ids: Vec<i32> = self.entries.keys().copied().collect();
        let mut events = EffectEvents::default();
        for id in ids {
            if self.entries.remove(&id).is_some() {
                events.changes.push(EffectChange::Removed(id));
            }
        }
        if !events.changes.is_empty() {
            events.attributes_changed = true;
            events.metadata_dirty = true;
        }
        events
    }

    /// `ClearAllStatusEffectsConsumeEffect` 的牛奶分支：只清有害效果。
    ///
    /// Milk's `ClearAllStatusEffectsConsumeEffect`: harmful effects only.
    pub fn remove_harmful(&mut self) -> EffectEvents {
        let ids: Vec<i32> = self
            .entries
            .keys()
            .copied()
            .filter(|&id| !is_beneficial(id))
            .collect();
        let mut events = EffectEvents::default();
        for id in ids {
            if self.entries.remove(&id).is_some() {
                events.changes.push(EffectChange::Removed(id));
            }
        }
        if !events.changes.is_empty() {
            events.attributes_changed = true;
            events.metadata_dirty = true;
        }
        events
    }

    /// `LivingEntity.tickEffects` 的一刻。`entity_tick_count` 只给无限时长效果用，
    /// `state` 给 `applyEffectTick` 读血量 / 吸收。
    ///
    /// One `LivingEntity.tickEffects` tick. `entity_tick_count` is used only
    /// by infinite effects; `state` supplies health and absorption.
    pub fn tick(&mut self, entity_tick_count: i32, state: EffectState) -> EffectEvents {
        let mut events = EffectEvents::default();
        let ids: Vec<i32> = self.entries.keys().copied().collect();
        for id in ids {
            let Some(instance) = self.entries.get_mut(&id) else {
                continue;
            };
            let before = instance.amplifier;
            let outcome = instance.tick_server(entity_tick_count, state);
            let EffectTickOutcome::Continues(actions) = outcome else {
                // `applyEffectTick` 返回 false：官方不减时长，直接移除并走
                // `onEffectsRemoved`（属性 / 元数据脏、发移除包）。
                self.entries.remove(&id);
                events.changes.push(EffectChange::Removed(id));
                events.attributes_changed = true;
                events.metadata_dirty = true;
                continue;
            };
            // 官方 `tickServer` 在减时长之前先跑 `applyEffectTick`；到期那一刻
            // 的动作照样生效，之后 `tickEffects` 才移除这一项。
            for action in actions {
                events.actions.push(EffectTick { effect: id, action });
            }
            if !instance.has_remaining() {
                self.entries.remove(&id);
                events.changes.push(EffectChange::Removed(id));
                events.attributes_changed = true;
                events.metadata_dirty = true;
                continue;
            }
            // `downgradeToHiddenEffect` 会改等级，属性要重刷并发一次更新包。
            let downgraded = instance.amplifier != before;
            if downgraded {
                events.attributes_changed = true;
                events.metadata_dirty = true;
                events.changes.push(EffectChange::Updated(id));
            } else if instance.duration > 0 && instance.duration % RESYNC_INTERVAL == 0 {
                // 官方每 600 刻补一次 `onEffectUpdated(false, null)`；属性不变，
                // 但仍要发一包 `update_mob_effect`。
                events.changes.push(EffectChange::Updated(id));
            }
        }
        events
    }

    /// 重建属性表里的效果修饰。`refresh_attributes` 由调用方在
    /// [`EffectEvents::attributes_changed`] 为真时调。
    ///
    /// Rebuilds the effect modifiers in an attribute map. The caller invokes
    /// this when [`EffectEvents::attributes_changed`] is set.
    pub fn rebuild_attributes(&self, attributes: &mut super::attributes::Attributes) {
        for entry in attributes.entries.iter_mut() {
            entry
                .modifiers
                .retain(|modifier| !modifier.id.starts_with("minecraft:effect."));
        }
        for instance in self.entries.values() {
            for (id, modifier) in attribute_modifiers(instance.effect, instance.amplifier) {
                if let Some(attribute) = attributes.instance(id) {
                    attribute.remove_modifier(&modifier.id);
                    attribute.modifiers.push(modifier);
                }
            }
        }
    }

    /// 存档 / 元数据用：按 ID 升序的全部效果。
    ///
    /// All effects in id order, for saving and metadata.
    pub fn entries(&self) -> Vec<EffectInstance> {
        self.entries.values().cloned().collect()
    }

    /// `readAdditionalSaveData`：直接放进表里，不触发 `onEffectAdded` 的包。
    ///
    /// `readAdditionalSaveData`: insert without emitting add packets.
    pub fn load(&mut self, instances: Vec<EffectInstance>) {
        self.entries.clear();
        for instance in instances {
            self.entries.insert(instance.effect, instance);
        }
    }

    /// 实体数据 `DATA_EFFECT_PARTICLES`：可见效果的颜色列表（按 ID 升序）。
    ///
    /// `DATA_EFFECT_PARTICLES`: colours of visible effects, in id order.
    pub fn particle_colors(&self) -> Vec<i32> {
        self.entries
            .values()
            .filter(|instance| instance.visible)
            .map(|instance| color(instance.effect))
            .collect()
    }

    /// 实体数据 `DATA_EFFECT_AMBIENCE_ID`：`areAllEffectsAmbient`。
    ///
    /// `DATA_EFFECT_AMBIENCE_ID`: `areAllEffectsAmbient`.
    pub fn all_ambient(&self) -> bool {
        !self.entries.is_empty() && self.entries.values().all(|instance| instance.ambient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn give(map: &mut EffectMap, effect: i32, duration: i32, amplifier: i32) -> EffectEvents {
        map.add(EffectInstance::with_amplifier(effect, duration, amplifier))
    }

    #[test]
    fn amplifier_is_clamped_like_the_vanilla_constructor() {
        let low = EffectInstance::with_amplifier(mob_effect::SPEED, 100, -7);
        let high = EffectInstance::with_amplifier(mob_effect::SPEED, 100, 999);
        assert_eq!(low.amplifier, 0);
        assert_eq!(high.amplifier, 255);
    }

    #[test]
    fn adding_updates_and_respects_the_hidden_effect_chain() {
        let mut map = EffectMap::new();
        assert_eq!(
            give(&mut map, mob_effect::SPEED, 600, 0).changes,
            vec![EffectChange::Added(mob_effect::SPEED)]
        );
        // 更高等级、更短时长：旧效果进隐藏层，等级和时长换成新的。
        let events = give(&mut map, mob_effect::SPEED, 200, 2);
        assert_eq!(
            events.changes,
            vec![EffectChange::Updated(mob_effect::SPEED)]
        );
        let instance = map.get(mob_effect::SPEED).unwrap();
        assert_eq!((instance.duration, instance.amplifier), (200, 2));
        assert_eq!(instance.hidden.as_ref().unwrap().amplifier, 0);
        assert_eq!(instance.hidden.as_ref().unwrap().duration, 600);
        // 官方 `MobEffectInstance.update`：同级 / 更低等级时，只有新的时长
        // 更长才刷新；新的更短保持不变（探针验证）。
        let shorter = give(&mut map, mob_effect::SPEED, 50, 2);
        assert!(shorter.changes.is_empty(), "同级更短不应刷新");
        assert_eq!(map.get(mob_effect::SPEED).unwrap().duration, 200);
        // 同级更长才刷新（远古守卫者给 1199 刻玩家重挂 6000 刻疲劳）。
        let longer = give(&mut map, mob_effect::SPEED, 6000, 2);
        assert_eq!(
            longer.changes,
            vec![EffectChange::Updated(mob_effect::SPEED)]
        );
        assert_eq!(map.get(mob_effect::SPEED).unwrap().duration, 6000);
    }

    #[test]
    fn tick_downgrades_to_the_hidden_effect() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::SPEED, 20, 0);
        let _ = give(&mut map, mob_effect::SPEED, 2, 1);
        for _ in 0..2 {
            let _ = map.tick(0, EffectState::default());
        }
        let instance = map.get(mob_effect::SPEED).unwrap();
        assert_eq!(instance.amplifier, 0);
        assert_eq!(instance.duration, 18);
        assert!(instance.hidden.is_none());
        // 改坏确认：如果隐藏层没有提前减时长，它应该是 20。
        assert_ne!(instance.duration, 20);
    }

    #[test]
    fn regeneration_runs_on_the_official_interval() {
        assert!(should_apply_this_tick(mob_effect::REGENERATION, 50, 0));
        assert!(!should_apply_this_tick(mob_effect::REGENERATION, 49, 0));
        assert!(should_apply_this_tick(mob_effect::REGENERATION, 25, 1));
        assert!(should_apply_this_tick(mob_effect::POISON, 25, 0));
        assert!(should_apply_this_tick(mob_effect::WITHER, 40, 0));
        // 改坏确认：把再生的间隔改成 25，49 刻会错误地触发。
        assert!(!interval_tick(49, 25));
    }

    /// 不祥之兆每刻判定、袭击之兆只在最后一刻判定；两者没有纯数值动作。
    #[test]
    fn omen_effects_tick_on_the_official_schedule() {
        // `BadOmenMobEffect.shouldApplyEffectTickThisTick` 恒真。
        assert!(should_apply_this_tick(mob_effect::BAD_OMEN, 120000, 0));
        assert!(should_apply_this_tick(mob_effect::BAD_OMEN, 1, 3));
        assert!(should_apply_this_tick(mob_effect::BAD_OMEN, 7, 0));
        // `RaidOmenMobEffect` 只在 `duration == 1` 触发。
        assert!(should_apply_this_tick(mob_effect::RAID_OMEN, 1, 0));
        assert!(!should_apply_this_tick(mob_effect::RAID_OMEN, 2, 0));
        assert!(!should_apply_this_tick(mob_effect::RAID_OMEN, 600, 2));
        assert!(!should_apply_this_tick(mob_effect::RAID_OMEN, 0, 0));
        // 改坏确认：把袭击之兆当成每刻触发、或不祥之兆当成不触发都会翻。
        assert!(!should_apply_this_tick(mob_effect::RAID_OMEN, 5, 0));
        assert_ne!(
            should_apply_this_tick(mob_effect::BAD_OMEN, 7, 0),
            should_apply_this_tick(mob_effect::RAID_OMEN, 7, 0)
        );
        // 副作用在调用方，纯动作表为空。
        assert!(effect_tick_actions(mob_effect::BAD_OMEN, 0).is_empty());
        assert!(effect_tick_actions(mob_effect::RAID_OMEN, 2).is_empty());
    }

    #[test]
    fn expiry_removes_the_effect_and_raises_the_removal_event() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::BLINDNESS, 2, 0);
        assert!(map.tick(0, EffectState::default()).changes.is_empty());
        let events = map.tick(0, EffectState::default());
        assert_eq!(
            events.changes,
            vec![EffectChange::Removed(mob_effect::BLINDNESS)]
        );
        assert!(events.attributes_changed);
        assert!(map.is_empty());
    }

    #[test]
    fn infinite_duration_never_expires_and_a_resync_is_sent_every_600_ticks() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::SPEED, INFINITE, 0);
        for tick in 0..1200 {
            assert!(
                map.tick(tick, EffectState::default()).changes.is_empty(),
                "无限时长不补发包"
            );
            assert!(map.has(mob_effect::SPEED), "tick {tick}");
        }
        assert_eq!(map.get(mob_effect::SPEED).unwrap().duration, INFINITE);
        // 有限时长 1300 刻：剩余 1200 和 600 时各补一次 `onEffectUpdated`。
        let mut finite = EffectMap::new();
        let _ = give(&mut finite, mob_effect::SPEED, 1300, 0);
        let mut resync = 0;
        for tick in 1..=1300 {
            if finite
                .tick(tick, EffectState::default())
                .changes
                .contains(&EffectChange::Updated(mob_effect::SPEED))
            {
                resync += 1;
            }
        }
        assert_eq!(resync, 2, "剩余 1200 和 600 刻各一次");
    }

    #[test]
    fn instant_effects_from_a_potion_apply_at_once_but_commands_tick_them() {
        let mut potion = EffectMap::new();
        let events = potion.apply_instantaneous(mob_effect::INSTANT_HEALTH, 1);
        assert!(potion.is_empty());
        assert_eq!(
            events.actions,
            vec![EffectTick {
                effect: mob_effect::INSTANT_HEALTH,
                action: EffectAction::InstantHeal(8.0),
            }]
        );
        // `/effect give` 走 `addEffect`：进表、下一刻结算、结算完移除。
        let mut command = EffectMap::new();
        let added = give(&mut command, mob_effect::INSTANT_DAMAGE, 1, 0);
        assert_eq!(
            added.changes,
            vec![EffectChange::Added(mob_effect::INSTANT_DAMAGE)]
        );
        assert!(command.has(mob_effect::INSTANT_DAMAGE));
        let tick = command.tick(1, EffectState::default());
        assert_eq!(
            tick.actions,
            vec![EffectTick {
                effect: mob_effect::INSTANT_DAMAGE,
                action: EffectAction::InstantDamage(6.0),
            }]
        );
        assert_eq!(
            tick.changes,
            vec![EffectChange::Removed(mob_effect::INSTANT_DAMAGE)]
        );
        assert!(command.is_empty());
    }

    #[test]
    fn absorption_sets_its_amount_when_the_effect_starts() {
        let mut map = EffectMap::new();
        let events = give(&mut map, mob_effect::ABSORPTION, 100, 1);
        assert_eq!(
            events.actions,
            vec![EffectTick {
                effect: mob_effect::ABSORPTION,
                action: EffectAction::Absorption(8.0),
            }]
        );
    }

    #[test]
    fn absorption_ends_without_ticking_when_the_pool_is_empty() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::ABSORPTION, 100, 1);
        let events = map.tick(
            1,
            EffectState {
                health: 10.0,
                max_health: 20.0,
                absorption: 0.0,
            },
        );
        assert_eq!(
            events.changes,
            vec![EffectChange::Removed(mob_effect::ABSORPTION)]
        );
        assert!(map.is_empty());
        // 改坏确认：吸收还有剩时效果继续，时长减 1。
        let mut alive = EffectMap::new();
        let _ = give(&mut alive, mob_effect::ABSORPTION, 100, 1);
        let events = alive.tick(
            1,
            EffectState {
                health: 10.0,
                max_health: 20.0,
                absorption: 8.0,
            },
        );
        assert!(events.changes.is_empty());
        assert_eq!(alive.get(mob_effect::ABSORPTION).unwrap().duration, 99);
    }

    #[test]
    fn attribute_modifiers_follow_the_official_amount_and_operation() {
        let modifiers = attribute_modifiers(mob_effect::SPEED, 1);
        assert_eq!(modifiers.len(), 1);
        let (attribute, modifier) = &modifiers[0];
        assert_eq!(
            *attribute,
            mc_bamboo_data::registries::attribute::MOVEMENT_SPEED
        );
        assert_eq!(modifier.amount, f64::from(0.2_f32) * 2.0);
        assert_eq!(modifier.operation, Operation::AddMultipliedTotal);
        assert_eq!(modifier.id, "minecraft:effect.speed");
        // 改坏确认：等级 1 的力量应该是 +6，不是 +3。
        let (_, strength) = &attribute_modifiers(mob_effect::STRENGTH, 1)[0];
        assert_eq!(strength.amount, 6.0);
        assert_ne!(strength.amount, 3.0);
        assert!(attribute_modifiers(mob_effect::BLINDNESS, 0).is_empty());
    }

    #[test]
    fn rebuilding_attributes_replaces_effect_modifiers() {
        let mut attributes = super::super::attributes::Attributes::cow();
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::SPEED, 100, 2);
        map.rebuild_attributes(&mut attributes);
        let speed = attributes.value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED);
        assert_eq!(speed, f64::from(0.2_f32) * (1.0 + f64::from(0.2_f32) * 3.0));
        map.remove(mob_effect::SPEED);
        map.rebuild_attributes(&mut attributes);
        assert_eq!(
            attributes.value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED),
            f64::from(0.2_f32)
        );
    }

    #[test]
    fn milk_only_removes_harmful_effects() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::SPEED, 100, 0);
        let _ = give(&mut map, mob_effect::POISON, 100, 0);
        let events = map.remove_harmful();
        assert_eq!(
            events.changes,
            vec![EffectChange::Removed(mob_effect::POISON)]
        );
        assert!(map.has(mob_effect::SPEED));
        assert!(!map.has(mob_effect::POISON));
    }

    #[test]
    fn particles_and_ambience_follow_the_visible_flags() {
        let mut map = EffectMap::new();
        let _ = give(&mut map, mob_effect::SPEED, 100, 0);
        assert_eq!(map.particle_colors(), vec![color(mob_effect::SPEED)]);
        assert!(!map.all_ambient());
        assert_eq!(
            give(&mut map, mob_effect::POISON, 100, 0).changes,
            vec![EffectChange::Added(mob_effect::POISON)]
        );
        map.get_mut_for_test(mob_effect::POISON).visible = false;
        assert_eq!(map.particle_colors(), vec![color(mob_effect::SPEED)]);
    }
}
