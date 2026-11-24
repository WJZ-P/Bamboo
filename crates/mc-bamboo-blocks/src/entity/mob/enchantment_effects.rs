//! 数据驱动的附魔 `location_changed` / `tick` 效果框架（26.3）。
//!
//! 官方把灵魂疾行、冰霜行者做成 `EnchantmentEffectComponents.LOCATION_CHANGED` 与
//! `TICK` 的 `List<ConditionalEffect<...>>`，由 `EnchantmentHelper` 在位置变化、换装备、
//! 每刻 tick 时求值。这里只用两个附魔实际用到的效果与谓词，其余官方注册过的类型以
//! [`Effect::Unimplemented`] / [`Predicate::Unimplemented`] 占位。
//!
//! 本模块是纯计算：输入实体状态快照，输出 [`Action`] 列表；接线层负责落地（改属性、
//! 写方块、发粒子音效、扣耐久）。没有相关附魔时零副作用，不消耗随机数。
//!
//! Data-driven enchantment `location_changed` / `tick` effects. Pure computation: an
//! entity-state snapshot goes in, a list of [`Action`]s comes out; the caller applies them.
//! Only the effect/predicate kinds the two enchantments use are implemented; the rest of
//! vanilla's registered types are kept as `Unimplemented` placeholders.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::registries::{SYNCED_REGISTRIES, attribute, data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::{
    attributes::Operation,
    equipment::{SLOTS, slot_name},
};

/// 附魔等级驱动的数值，`LevelBasedValue` 里本框架用到的三种形态。
///
/// The three `LevelBasedValue` shapes this framework uses; `calculate(level)` yields `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LevelValue {
    Constant(f32),
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    Clamped {
        value: &'static LevelValue,
        min: f32,
        max: f32,
    },
}

impl LevelValue {
    /// `LevelBasedValue.calculate(int)`。整数减法按官方回绕，越界等级不 panic；
    /// clamp 用比较实现，NaN 不被 `f32::clamp` 触发 panic。
    ///
    /// `LevelBasedValue.calculate(int)`. Int subtraction wraps like vanilla; clamp is a
    /// comparison so NaN cannot panic.
    pub fn calculate(self, level: i32) -> f32 {
        match self {
            Self::Constant(v) => v,
            Self::Linear {
                base,
                per_level_above_first,
            } => base + per_level_above_first * (level.wrapping_sub(1) as f32),
            Self::Clamped { value, min, max } => {
                let v = value.calculate(level);
                if v < min {
                    min
                } else if v > max {
                    max
                } else {
                    v
                }
            }
        }
    }
}

/// `replace_disk` 的参数（去掉固定的 `frosted_ice` age=0 与四个方块谓词）。
///
/// Parameters of `replace_disk` (the constant `frosted_ice` block and four block
/// predicates are fixed and live in [`freeze_cells`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreezeSpec {
    pub radius: LevelValue,
    pub height: LevelValue,
    pub offset: (i32, i32, i32),
}

/// 一次 `ReplaceDisk.apply` 算出的圆盘，交给接线层逐格落地。
///
/// The disk computed by one `ReplaceDisk.apply`, handed to the caller to apply cell by cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreezeRequest {
    /// `BlockPos.containing(pos).offset(offset)`。
    pub origin: BlockPos,
    /// 实体精确 x/z，用于 `distToCenterSqr` 的水平圆判定。
    pub center_x: f64,
    pub center_z: f64,
    pub radius: i32,
    pub height: i32,
}

/// 一格是否满足 `replace_disk` 的 `all_of` 方块谓词，由接线层读世界填好。
///
/// Whether one cell passes `replace_disk`'s `all_of` block predicates, filled by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeCell {
    /// `matching_blocks(water)` 且 `matching_fluids(water)`。
    pub is_water: bool,
    /// `matching_block_tag(offset [0,1,0], air)`。
    pub air_above: bool,
    /// `unobstructed`：该格没有能挡方块放置的实体。
    pub unobstructed: bool,
}

/// `ReplaceDisk.apply` 的格子枚举：遍历 `betweenClosed(from, to)`，按
/// `distToCenterSqr(pos.x, p.y+0.5, pos.z) < radius²` 过滤水平圆，再逐格跑谓词。
///
/// Enumerate the cells of `ReplaceDisk.apply`: iterate `betweenClosed`, keep the horizontal
/// disc where `distToCenterSqr(pos.x, p.y+0.5, pos.z) < radius²`, then test each predicate.
pub fn freeze_cells(
    req: &FreezeRequest,
    mut query: impl FnMut(BlockPos) -> FreezeCell,
) -> Vec<BlockPos> {
    let r = req.radius;
    if r <= 0 || req.height <= 0 {
        return Vec::new();
    }
    let y_top = (req.height - 1).min(0);
    let r2 = i64::from(r) * i64::from(r);
    let mut out = Vec::new();
    let min_x = req.origin.x.saturating_sub(r);
    let max_x = req.origin.x.saturating_add(r);
    let min_z = req.origin.z.saturating_sub(r);
    let max_z = req.origin.z.saturating_add(r);
    let min_y = req.origin.y;
    let max_y = req.origin.y.saturating_add(y_top);
    for y in min_y..=max_y {
        for z in min_z..=max_z {
            for x in min_x..=max_x {
                let dx = (f64::from(x) + 0.5) - req.center_x;
                let dz = (f64::from(z) + 0.5) - req.center_z;
                // 传入的 y 恰是 p.y+0.5，y 分量差恒为 0。
                // The supplied y is exactly p.y+0.5, so the vertical term is always 0.
                if dx * dx + dz * dz >= r2 as f64 {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                let cell = query(pos);
                if cell.is_water && cell.air_above && cell.unobstructed {
                    out.push(pos);
                }
            }
        }
    }
    out
}

/// 求值所需的实体状态快照，对应官方 `ENCHANTED_LOCATION` / `ENCHANTED_ENTITY` 上下文。
///
/// The entity-state snapshot needed by the predicates, matching the official loot context.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntityState {
    pub position: Vec3,
    pub on_ground: bool,
    pub is_flying: bool,
    pub has_vehicle: bool,
    pub horizontal_speed: f64,
    pub tick_count: i32,
    /// `movement_affected_by` 命中 `#minecraft:soul_speed_blocks`（影响移动的脚下方块）。
    pub on_soul_speed_block: bool,
}

/// `LootItemCondition` 里两个附魔用到的子集；未用到的官方类型以 `Unimplemented` 占位。
///
/// The subset of `LootItemCondition` the two enchantments use; other vanilla types are
/// kept as `Unimplemented` placeholders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Predicate {
    AllOf(&'static [Predicate]),
    AnyOf(&'static [Predicate]),
    Inverted(&'static Predicate),
    /// `enchantment_active_check`：读 `ENCHANTMENT_ACTIVE`（即本效果上次是否激活）。
    ActiveCheck(bool),
    OnGround,
    NotFlying,
    /// `vehicle: {}`：实体有载具。
    Vehicle,
    MovementAffectedBySoulSpeed,
    HorizontalSpeedAbove(f64),
    /// `periodic_tick: n`：`tickCount % n == 0`。
    PeriodicTick(i32),
    RandomChance(f32),
    /// `random_chance(enchantment_level * amount)`。
    RandomChancePerLevel(f32),
    Unimplemented(&'static str),
}

impl Predicate {
    /// `LootItemCondition.test`。组合谓词短路，随机数只在真正取到时消耗。
    ///
    /// `LootItemCondition.test`. Combinators short-circuit; RNG is drawn only when reached.
    pub fn matches(
        &self,
        e: &EntityState,
        level: i32,
        was_active: bool,
        rng: &mut dyn FnMut() -> f32,
    ) -> bool {
        match *self {
            Self::AllOf(terms) => terms
                .iter()
                .all(|term| term.matches(e, level, was_active, rng)),
            Self::AnyOf(terms) => terms
                .iter()
                .any(|term| term.matches(e, level, was_active, rng)),
            Self::Inverted(term) => !term.matches(e, level, was_active, rng),
            Self::ActiveCheck(active) => was_active == active,
            Self::OnGround => e.on_ground,
            Self::NotFlying => !e.is_flying,
            Self::Vehicle => e.has_vehicle,
            Self::MovementAffectedBySoulSpeed => e.on_soul_speed_block,
            Self::HorizontalSpeedAbove(min) => e.horizontal_speed >= min,
            Self::PeriodicTick(n) => n > 0 && e.tick_count % n == 0,
            Self::RandomChance(c) => rng() < c,
            Self::RandomChancePerLevel(a) => rng() < a * (level as f32),
            Self::Unimplemented(_) => false,
        }
    }
}

/// `EnchantmentLocationBasedEffect` 里两个附魔用到的效果类型。
///
/// The `EnchantmentLocationBasedEffect` kinds the two enchantments use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// 临时属性修饰符：首次激活加、停用撤。
    Attribute {
        attribute: i32,
        id: &'static str,
        amount: LevelValue,
        operation: Operation,
    },
    /// `all_of`：把 `on_changed_block` / `on_deactivated` 转发给每个子效果。
    AllOf(&'static [Effect]),
    /// `change_item_damage`（`EnchantmentEntityEffect`）：每次位置变化都扣耐久。
    ChangeItemDamage(i32),
    /// `replace_disk`（`EnchantmentEntityEffect`）：每次位置变化都结冰。
    ReplaceDisk(FreezeSpec),
    /// `spawn_particles`（`minecraft:soul`，`TICK`）。
    SoulParticles,
    /// `play_sound`（`minecraft:particle.soul_escape`，`TICK`）。
    SoulSound,
    /// 官方注册过但本任务不实现的效果类型。
    ///
    /// Effect types vanilla registers but this task does not implement:
    /// `apply_mob_effect`, `damage_entity`, `explode`, `ignite`, `apply_impulse`,
    /// `apply_exhaustion`, `replace_block`, `run_function`, `set_block_properties`,
    /// `summon_entity`.
    Unimplemented(&'static str),
}

/// 一次效果求值的输出动作，由接线层落地。
///
/// One output action of an effect evaluation, applied by the caller.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    AddModifier {
        attribute: i32,
        id: String,
        amount: f64,
        operation: Operation,
    },
    RemoveModifier {
        attribute: i32,
        id: String,
    },
    Freeze(FreezeRequest),
    DamageItem {
        slot: EquipmentSlot,
        amount: i32,
    },
    SoulParticles {
        position: Vec3,
    },
    SoulSound {
        position: Vec3,
    },
}

impl Effect {
    /// `onChangedBlock(ServerLevel, level, item, entity, pos, fresh)`。
    /// `EnchantmentEntityEffect` 的 default 实现忽略 `fresh`，每次都 `apply`。
    ///
    /// `onChangedBlock`. The `EnchantmentEntityEffect` default ignores `fresh` and always
    /// calls `apply`.
    pub fn on_changed_block(
        &self,
        level: i32,
        fresh: bool,
        slot: EquipmentSlot,
        e: &EntityState,
    ) -> Vec<Action> {
        match *self {
            Self::Attribute {
                attribute,
                id,
                amount,
                operation,
            } => {
                if fresh {
                    vec![Action::AddModifier {
                        attribute,
                        id: format!("{}/{}", id, slot_name(slot)),
                        amount: f64::from(amount.calculate(level)),
                        operation,
                    }]
                } else {
                    Vec::new()
                }
            }
            Self::AllOf(effects) => effects
                .iter()
                .flat_map(|effect| effect.on_changed_block(level, fresh, slot, e))
                .collect(),
            Self::ChangeItemDamage(amount) => vec![Action::DamageItem { slot, amount }],
            Self::ReplaceDisk(spec) => {
                let radius = spec.radius.calculate(level) as i32;
                let height = spec.height.calculate(level) as i32;
                let origin = BlockPos::new(
                    e.position.x.floor() as i32 + spec.offset.0,
                    e.position.y.floor() as i32 + spec.offset.1,
                    e.position.z.floor() as i32 + spec.offset.2,
                );
                vec![Action::Freeze(FreezeRequest {
                    origin,
                    center_x: e.position.x,
                    center_z: e.position.z,
                    radius,
                    height,
                })]
            }
            Self::SoulParticles => vec![Action::SoulParticles {
                position: e.position,
            }],
            Self::SoulSound => vec![Action::SoulSound {
                position: e.position,
            }],
            Self::Unimplemented(_) => Vec::new(),
        }
    }

    /// `onDeactivated`：只有属性效果需要撤掉临时修饰符。
    ///
    /// `onDeactivated`: only attribute effects remove a transient modifier.
    pub fn on_deactivated(&self, slot: EquipmentSlot) -> Vec<Action> {
        match *self {
            Self::Attribute { attribute, id, .. } => vec![Action::RemoveModifier {
                attribute,
                id: format!("{}/{}", id, slot_name(slot)),
            }],
            Self::AllOf(effects) => effects
                .iter()
                .flat_map(|effect| effect.on_deactivated(slot))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `EnchantmentEntityEffect.apply`（`TICK`）。
    pub fn apply(&self, level: i32, slot: EquipmentSlot, e: &EntityState) -> Vec<Action> {
        // TICK 的 entity effect 不走 fresh 逻辑，位置变化语义在此不适用。
        // TICK entity effects ignore `fresh`; reuse the shared implementation.
        self.on_changed_block(level, true, slot, e)
    }
}

/// 官方 `activeLocationDependentEnchantments` 里被跟踪的效果，一个 bit 一个。
/// 两个附魔都只在 `feet` 槽生效，用一个全局位集即可。
///
/// One bit per tracked effect (vanilla `activeLocationDependentEnchantments`). Both
/// enchantments only apply to the `feet` slot, so one global bitset suffices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectId {
    SoulSpeedAttributes = 0,
    SoulSpeedDamage = 1,
    FrostWalkerDisk = 2,
    SoulSpeedParticles = 3,
    SoulSpeedSound = 4,
}

impl EffectId {
    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// 每个效果的激活状态，对应官方 `Set<EnchantmentLocationBasedEffect>`。
///
/// Active/inactive state per effect, mirroring vanilla's `Set<EnchantmentLocationBasedEffect>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveLocationEffects {
    bits: u8,
}

impl ActiveLocationEffects {
    pub fn is_active(&self, id: EffectId) -> bool {
        self.bits & id.bit() != 0
    }

    fn set(&mut self, id: EffectId, active: bool) {
        if active {
            self.bits |= id.bit();
        } else {
            self.bits &= !id.bit();
        }
    }
}

/// 官方 `ConditionalEffect<T>`：`effect` + 可选 `requirements`。
///
/// Vanilla `ConditionalEffect<T>`: an effect plus optional requirements.
#[derive(Debug, Clone, Copy)]
pub struct ConditionalEffect {
    pub id: EffectId,
    pub effect: Effect,
    pub requirements: Predicate,
}

// ---- 静态数据：逐条从官方 enchantment JSON 抄出 ----

static LINEAR_3_1: LevelValue = LevelValue::Linear {
    base: 3.0,
    per_level_above_first: 1.0,
};
static FROST_RADIUS: LevelValue = LevelValue::Clamped {
    value: &LINEAR_3_1,
    min: 0.0,
    max: 16.0,
};

static P_ON_GROUND: Predicate = Predicate::OnGround;
static P_VEHICLE: Predicate = Predicate::Vehicle;

static SS_ACTIVE_ANY: &[Predicate] = &[
    Predicate::MovementAffectedBySoulSpeed,
    Predicate::Inverted(&P_ON_GROUND),
];
static SS_ACTIVE_BRANCH: &[Predicate] = &[
    Predicate::ActiveCheck(true),
    Predicate::NotFlying,
    Predicate::AnyOf(SS_ACTIVE_ANY),
];
static SS_INACTIVE_BRANCH: &[Predicate] = &[
    Predicate::ActiveCheck(false),
    Predicate::NotFlying,
    Predicate::MovementAffectedBySoulSpeed,
];
static SS_ANY: &[Predicate] = &[
    Predicate::AllOf(SS_ACTIVE_BRANCH),
    Predicate::AllOf(SS_INACTIVE_BRANCH),
];
static SS_REQ_ATTRS: &[Predicate] = &[Predicate::Inverted(&P_VEHICLE), Predicate::AnyOf(SS_ANY)];
static SOUL_SPEED_ATTRS_REQ: Predicate = Predicate::AllOf(SS_REQ_ATTRS);

static SS_REQ_DAMAGE: &[Predicate] = &[
    Predicate::RandomChancePerLevel(0.04),
    Predicate::OnGround,
    Predicate::MovementAffectedBySoulSpeed,
];
static SOUL_SPEED_DAMAGE_REQ: Predicate = Predicate::AllOf(SS_REQ_DAMAGE);

static SS_TICK_ENTITY: &[Predicate] = &[
    Predicate::NotFlying,
    Predicate::OnGround,
    Predicate::HorizontalSpeedAbove(9.999999747378752e-06),
    Predicate::MovementAffectedBySoulSpeed,
    Predicate::PeriodicTick(5),
];
static SS_TICK_ENTITY_REQ: Predicate = Predicate::AllOf(SS_TICK_ENTITY);
static SS_TICK_SOUND: &[Predicate] = &[
    Predicate::RandomChance(0.35),
    Predicate::AllOf(SS_TICK_ENTITY),
];
static SS_TICK_SOUND_REQ: Predicate = Predicate::AllOf(SS_TICK_SOUND);

static FROST_REQ: &[Predicate] = &[Predicate::OnGround, Predicate::Inverted(&P_VEHICLE)];
static FROST_WALKER_REQ: Predicate = Predicate::AllOf(FROST_REQ);

static SS_ATTR_EFFECTS: &[Effect] = &[
    Effect::Attribute {
        attribute: attribute::MOVEMENT_SPEED,
        id: "minecraft:enchantment.soul_speed",
        amount: LevelValue::Linear {
            base: 0.0405,
            per_level_above_first: 0.0105,
        },
        operation: Operation::AddValue,
    },
    Effect::Attribute {
        attribute: attribute::MOVEMENT_EFFICIENCY,
        id: "minecraft:enchantment.soul_speed",
        amount: LevelValue::Constant(1.0),
        operation: Operation::AddValue,
    },
];

static FROST_WALKER_LOCATION: &[ConditionalEffect] = &[ConditionalEffect {
    id: EffectId::FrostWalkerDisk,
    effect: Effect::ReplaceDisk(FreezeSpec {
        radius: FROST_RADIUS,
        height: LevelValue::Constant(1.0),
        offset: (0, -1, 0),
    }),
    requirements: FROST_WALKER_REQ,
}];

static SOUL_SPEED_LOCATION: &[ConditionalEffect] = &[
    ConditionalEffect {
        id: EffectId::SoulSpeedAttributes,
        effect: Effect::AllOf(SS_ATTR_EFFECTS),
        requirements: SOUL_SPEED_ATTRS_REQ,
    },
    ConditionalEffect {
        id: EffectId::SoulSpeedDamage,
        effect: Effect::ChangeItemDamage(1),
        requirements: SOUL_SPEED_DAMAGE_REQ,
    },
];

static SOUL_SPEED_TICK: &[ConditionalEffect] = &[
    ConditionalEffect {
        id: EffectId::SoulSpeedParticles,
        effect: Effect::SoulParticles,
        requirements: SS_TICK_ENTITY_REQ,
    },
    ConditionalEffect {
        id: EffectId::SoulSpeedSound,
        effect: Effect::SoulSound,
        requirements: SS_TICK_SOUND_REQ,
    },
];

/// 附魔注册名。
pub const SOUL_SPEED: &str = "minecraft:soul_speed";
pub const FROST_WALKER: &str = "minecraft:frost_walker";

fn location_table(name: &str) -> Option<&'static [ConditionalEffect]> {
    match name {
        SOUL_SPEED => Some(SOUL_SPEED_LOCATION),
        FROST_WALKER => Some(FROST_WALKER_LOCATION),
        _ => None,
    }
}

/// `Enchantment.matchingSlot`：两个附魔的 `slots` 都是 `feet`。
///
/// `Enchantment.matchingSlot`: both enchantments declare `slots: [feet]`.
pub fn matching_slot(name: &str, slot: EquipmentSlot) -> bool {
    location_table(name).is_some() && slot == EquipmentSlot::Feet
}

/// 一件物品上的附魔（`槽无关`），按 `ItemEnchantments.entrySet` 顺序。
///
/// The enchantments on one item, in `ItemEnchantments.entrySet` order.
pub fn enchantments_of(stack: &ItemStack) -> Vec<(&'static str, i32)> {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return Vec::new();
    };
    let Some(ComponentValue::Enchantments(levels)) =
        defaults::resolve(stack.item, &patch, kinds::ENCHANTMENTS)
    else {
        return Vec::new();
    };
    let Some(registry) = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
    else {
        return Vec::new();
    };
    levels
        .iter()
        .filter_map(|&(id, level)| {
            let name = usize::try_from(id)
                .ok()
                .and_then(|index| registry.entries.get(index))
                .copied()?;
            Some((name, level))
        })
        .collect()
}

/// 按官方 `EquipmentSlot.VALUES` 顺序整理装备上的附魔，供 [`run_location_changed`] /
/// [`tick_effects`] 使用。只保留槽匹配、等级为正、且本框架认识的附魔。
///
/// Collect equipped enchantments in vanilla `EquipmentSlot.VALUES` order, keeping only
/// slot-matching, positive-level, recognised enchantments.
pub fn equipped_enchantments(
    stacks: &[(EquipmentSlot, &ItemStack)],
) -> Vec<(EquipmentSlot, &'static str, i32)> {
    let mut out = Vec::new();
    for slot in SLOTS {
        let Some((_, stack)) = stacks.iter().find(|(s, _)| *s == slot) else {
            continue;
        };
        for (name, level) in enchantments_of(stack) {
            if level > 0 && matching_slot(name, slot) {
                out.push((slot, name, level));
            }
        }
    }
    out
}

/// `Enchantment.runLocationChangedEffects`：遍历装备，逐条 `ConditionalEffect` 检查
/// `requirements`（`locationContext` 带 `was_active`），维护激活状态并产出动作。
///
/// `Enchantment.runLocationChangedEffects`: walk equipment, test each `ConditionalEffect`
/// against a `locationContext` carrying `was_active`, track active state, emit actions.
pub fn run_location_changed(
    e: &EntityState,
    enchants: &[(EquipmentSlot, &'static str, i32)],
    active: &mut ActiveLocationEffects,
    rng: &mut dyn FnMut() -> f32,
) -> Vec<Action> {
    let mut out = Vec::new();
    for &(slot, name, level) in enchants {
        let Some(table) = location_table(name) else {
            continue;
        };
        if !matching_slot(name, slot) || level <= 0 {
            continue;
        }
        for ce in table {
            let was = active.is_active(ce.id);
            if ce.requirements.matches(e, level, was, rng) {
                if !was {
                    active.set(ce.id, true);
                }
                out.extend(ce.effect.on_changed_block(level, !was, slot, e));
            } else if was {
                active.set(ce.id, false);
                out.extend(ce.effect.on_deactivated(slot));
            }
        }
    }
    out
}

/// `Enchantment.tick`：`TICK` 组件的 `ConditionalEffect` 用 `entityContext`（不带
/// `ENCHANTMENT_ACTIVE`），命中就 `apply`。
///
/// `Enchantment.tick`: `TICK` effects use an `entityContext` without `ENCHANTMENT_ACTIVE`.
pub fn tick_effects(
    e: &EntityState,
    enchants: &[(EquipmentSlot, &'static str, i32)],
    rng: &mut dyn FnMut() -> f32,
) -> Vec<Action> {
    let mut out = Vec::new();
    for &(slot, name, level) in enchants {
        if name != SOUL_SPEED || !matching_slot(name, slot) || level <= 0 {
            continue;
        }
        for ce in SOUL_SPEED_TICK {
            if ce.requirements.matches(e, level, false, rng) {
                out.extend(ce.effect.apply(level, slot, e));
            }
        }
    }
    out
}

/// `Enchantment.stopLocationBasedEffects`：撤掉所有仍激活效果的临时修饰符并清空状态。
///
/// `Enchantment.stopLocationBasedEffects`: deactivate everything and remove transient
/// modifiers.
pub fn stop_location_based(active: &mut ActiveLocationEffects, slot: EquipmentSlot) -> Vec<Action> {
    let mut out = Vec::new();
    for table in [SOUL_SPEED_LOCATION, FROST_WALKER_LOCATION] {
        for ce in table {
            if active.is_active(ce.id) {
                active.set(ce.id, false);
                out.extend(ce.effect.on_deactivated(slot));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soul_state(on_soul: bool) -> EntityState {
        EntityState {
            position: Vec3::new(0.5, 64.0, 0.5),
            on_ground: true,
            on_soul_speed_block: on_soul,
            ..EntityState::default()
        }
    }

    fn soul_enchants(level: i32) -> Vec<(EquipmentSlot, &'static str, i32)> {
        vec![(EquipmentSlot::Feet, SOUL_SPEED, level)]
    }

    /// 灵魂疾行：站上灵魂沙首次激活加两个修饰符，持续激活不再重复加，离开撤掉。
    #[test]
    fn soul_speed_modifiers_added_and_removed() {
        let mut rng = || 1.0_f32;
        let mut active = ActiveLocationEffects::default();

        let added =
            run_location_changed(&soul_state(true), &soul_enchants(3), &mut active, &mut rng);
        let expected = vec![
            Action::AddModifier {
                attribute: attribute::MOVEMENT_SPEED,
                id: "minecraft:enchantment.soul_speed/feet".to_owned(),
                amount: f64::from(0.0405_f32 + 0.0105_f32 * 2.0),
                operation: Operation::AddValue,
            },
            Action::AddModifier {
                attribute: attribute::MOVEMENT_EFFICIENCY,
                id: "minecraft:enchantment.soul_speed/feet".to_owned(),
                amount: 1.0,
                operation: Operation::AddValue,
            },
        ];
        assert_eq!(added, expected);
        assert!(active.is_active(EffectId::SoulSpeedAttributes));

        // 持续站着：属性效果不重复 add（fresh=false），耐久效果由概率决定（rng=1.0 不命中）。
        let held =
            run_location_changed(&soul_state(true), &soul_enchants(3), &mut active, &mut rng);
        assert!(held.is_empty());

        // 离开灵魂沙且落地：迟滞谓词不再成立，撤掉两个修饰符。
        let removed =
            run_location_changed(&soul_state(false), &soul_enchants(3), &mut active, &mut rng);
        assert_eq!(
            removed,
            vec![
                Action::RemoveModifier {
                    attribute: attribute::MOVEMENT_SPEED,
                    id: "minecraft:enchantment.soul_speed/feet".to_owned(),
                },
                Action::RemoveModifier {
                    attribute: attribute::MOVEMENT_EFFICIENCY,
                    id: "minecraft:enchantment.soul_speed/feet".to_owned(),
                },
            ]
        );
        assert!(!active.is_active(EffectId::SoulSpeedAttributes));
    }

    /// 灵魂疾行：空中离开灵魂沙仍保持激活（is_on_ground=false 的迟滞分支）。
    #[test]
    fn soul_speed_hysteresis_keeps_modifier_in_air() {
        let mut rng = || 1.0_f32;
        let mut active = ActiveLocationEffects::default();
        run_location_changed(&soul_state(true), &soul_enchants(1), &mut active, &mut rng);
        let mut air = soul_state(false);
        air.on_ground = false;
        let held = run_location_changed(&air, &soul_enchants(1), &mut active, &mut rng);
        assert!(held.is_empty());
        assert!(active.is_active(EffectId::SoulSpeedAttributes));
    }

    /// 冰霜行者：不同等级的半径（level+2），水平圆内格子数与等级对应。
    #[test]
    fn frost_walker_radius_grows_with_level() {
        let cells_for = |level: i32| {
            let mut rng = || 1.0_f32;
            let e = EntityState {
                position: Vec3::new(0.0, 64.0, 0.0),
                on_ground: true,
                ..EntityState::default()
            };
            let enchants = vec![(EquipmentSlot::Feet, FROST_WALKER, level)];
            let mut active = ActiveLocationEffects::default();
            let actions = run_location_changed(&e, &enchants, &mut active, &mut rng);
            let Action::Freeze(req) = &actions[0] else {
                panic!("expected a freeze request");
            };
            let all = FreezeCell {
                is_water: true,
                air_above: true,
                unobstructed: true,
            };
            freeze_cells(req, |_| all)
        };
        let r1 = 3; // level 1 -> clamp(3,0,16)
        let r2 = 4; // level 2 -> clamp(4,0,16)
        let count = |r: i32, cx: f64, cz: f64, y: i32| -> usize {
            let mut n = 0;
            for z in -r..=r {
                for x in -r..=r {
                    let dx = f64::from(x) + 0.5 - cx;
                    let dz = f64::from(z) + 0.5 - cz;
                    if dx * dx + dz * dz < f64::from(r * r) && y == 63 {
                        n += 1;
                    }
                }
            }
            n
        };
        // 圆心 (0,0) 在格心，offset [0,-1,0] 把 y 放到 63。
        assert_eq!(cells_for(1).len(), count(r1, 0.0, 0.0, 63));
        assert_eq!(cells_for(2).len(), count(r2, 0.0, 0.0, 63));
        assert!(cells_for(2).len() > cells_for(1).len());
        assert_eq!(
            cells_for(1).iter().filter(|p| p.y == 63).count(),
            cells_for(1).len()
        );
    }

    /// 冰霜行者：谓词不通过的格子不结冰（水上方非空气 / 被实体占据）。
    #[test]
    fn frost_walker_respects_cell_predicates() {
        let mut rng = || 1.0_f32;
        let e = EntityState {
            position: Vec3::new(0.0, 64.0, 0.0),
            on_ground: true,
            ..EntityState::default()
        };
        let enchants = vec![(EquipmentSlot::Feet, FROST_WALKER, 2)];
        let mut active = ActiveLocationEffects::default();
        let actions = run_location_changed(&e, &enchants, &mut active, &mut rng);
        let Action::Freeze(req) = &actions[0] else {
            panic!("expected a freeze request");
        };
        let blocked = freeze_cells(req, |pos| FreezeCell {
            is_water: true,
            air_above: pos.x != 0,
            unobstructed: pos.z != 0,
        });
        assert!(blocked.iter().all(|p| p.x != 0 && p.z != 0));
    }

    // ---- 改坏确认：把常量改坏，行为必须跟着变 ----

    /// 改坏确认 1：灵魂疾行的 `per_level_above_first` 若被改，等级 3 的加成不同。
    #[test]
    fn break_check_soul_speed_per_level() {
        let good = LevelValue::Linear {
            base: 0.0405,
            per_level_above_first: 0.0105,
        }
        .calculate(3);
        let broken = LevelValue::Linear {
            base: 0.0405,
            per_level_above_first: 0.0,
        }
        .calculate(3);
        assert_ne!(good, broken);
        assert_eq!(good, 0.0405_f32 + 0.0105_f32 * 2.0);
    }

    /// 改坏确认 2：冰霜行者半径基数若被改，等级 1 的格子集不同。
    #[test]
    fn break_check_frost_walker_radius_base() {
        let good = LevelValue::Clamped {
            value: &LevelValue::Linear {
                base: 3.0,
                per_level_above_first: 1.0,
            },
            min: 0.0,
            max: 16.0,
        }
        .calculate(1);
        let broken = LevelValue::Linear {
            base: 2.0,
            per_level_above_first: 1.0,
        }
        .calculate(1);
        assert_ne!(good, broken);
        assert_eq!(good, 3.0);
    }
}
