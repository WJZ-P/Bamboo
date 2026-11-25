//! 生物的装备：`LivingEntity.equipment`（`EntityEquipment`）、`Mob.dropChances`
//! （`DropChances`）和 `LivingEntity.lastEquipmentItems`（`detectEquipmentUpdates` 的比较基准）。
//!
//! 八个槽都建模（主手、副手、脚、腿、胸、头、身体、鞍），顺序是官方 `EquipmentSlot` 的
//! ordinal，和协议里的 [`EquipmentSlot`] 一致。身体和鞍是马科用的。
//!
//! Mob equipment: all eight slots in vanilla `EquipmentSlot` ordinal order (body
//! and saddle are for equines), the drop chances and the last-sent copies
//! `detectEquipmentUpdates` compares against.

use mc_bamboo_data::{
    component_enums,
    registries::{SYNCED_REGISTRIES, attribute, data_component_type as kinds},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::{
    Mob, MobType,
    attributes::{Attributes, Modifier, Operation},
};

/// [`MobEquipment::detect_updates`] 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EquipmentUpdate {
    /// 主副手正好互换：官方发实体事件 55，不发这两个槽的装备。
    pub hand_swap: bool,
    /// 要发 `set_equipment` 的槽和新物品，按 ordinal。
    pub changes: Vec<(EquipmentSlot, ItemStack)>,
}

/// `LivingEntity.stopLocationBasedEffects` 的属性部分：摘掉这件物品在这个槽给的修饰符。
pub fn remove_stack_modifiers(stack: &ItemStack, slot: EquipmentSlot, attributes: &mut Attributes) {
    for (attribute, modifier) in stack_modifiers(stack, slot) {
        if let Some(instance) = attributes.instance(attribute) {
            instance.remove_modifier(&modifier.id);
        }
    }
}

/// `EquipmentSlotGroup.test`：组 ID 是 `minecraft:equipment_slot_group` 的顺序
/// （any、mainhand、offhand、hand、feet、legs、chest、head、armor、body、saddle）。
pub const fn slot_group_applies(group: i32, slot: EquipmentSlot) -> bool {
    match group {
        0 => true,
        1 => matches!(slot, EquipmentSlot::MainHand),
        2 => matches!(slot, EquipmentSlot::OffHand),
        3 => matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand),
        4 => matches!(slot, EquipmentSlot::Feet),
        5 => matches!(slot, EquipmentSlot::Legs),
        6 => matches!(slot, EquipmentSlot::Chest),
        7 => matches!(slot, EquipmentSlot::Head),
        8 => matches!(
            slot,
            EquipmentSlot::Feet
                | EquipmentSlot::Legs
                | EquipmentSlot::Chest
                | EquipmentSlot::Head
                | EquipmentSlot::Body
        ),
        9 => matches!(slot, EquipmentSlot::Body),
        10 => matches!(slot, EquipmentSlot::Saddle),
        _ => false,
    }
}

/// `net.minecraft.world.item.enchantment.LevelBasedValue` 里八条
/// `minecraft:attributes` 附魔会用到的三种形态。`calculate(int)` 返回 `float`。
///
/// The three `LevelBasedValue` shapes used by the attribute enchantments;
/// `calculate(int)` returns a `float`.
#[derive(Debug, Clone, Copy)]
enum LevelBasedValue {
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    LevelsSquared {
        added: f32,
    },
    Fraction {
        numerator: &'static LevelBasedValue,
        denominator: &'static LevelBasedValue,
    },
}

impl LevelBasedValue {
    /// `LevelBasedValue.calculate(int)`：linear / levels_squared / fraction。
    /// int 的减法和乘法按官方 `isub` / `Mth.square` 回绕，越界等级不 panic。
    ///
    /// `LevelBasedValue.calculate(int)`. The int subtraction/multiplication wrap
    /// like vanilla `isub` / `Mth.square`, so an out-of-range level cannot panic.
    fn calculate(self, level: i32) -> f32 {
        match self {
            Self::Linear {
                base,
                per_level_above_first,
            } => base + per_level_above_first * (level.wrapping_sub(1) as f32),
            Self::LevelsSquared { added } => (level.wrapping_mul(level) as f32) + added,
            Self::Fraction {
                numerator,
                denominator,
            } => {
                let den = denominator.calculate(level);
                if den == 0.0 {
                    0.0
                } else {
                    numerator.calculate(level) / den
                }
            }
        }
    }
}

/// 一条 `EnchantmentAttributeEffect`：`minecraft:attributes` 附魔效果。
struct EnchantmentAttributeEffect {
    /// 附魔注册名（`minecraft:enchantment` 注册表里的名字）。
    enchantment: &'static str,
    /// JSON 的 `id`；`getModifier` 用 `withSuffix("/" + slot)` 拼修饰符 ID。
    id: &'static str,
    attribute: i32,
    amount: LevelBasedValue,
    operation: Operation,
    /// 附魔定义的 `slots`（`EquipmentSlotGroup` 名字），`matchingSlot` 用。
    groups: &'static [&'static str],
}

// 横扫之刃的分子 / 分母是两条 linear，单独作为静态节点让 `Fraction` 引用。
// Sweeping Edge's numerator / denominator are two linear nodes.
static SWEEPING_NUMERATOR: LevelBasedValue = LevelBasedValue::Linear {
    base: 1.0,
    per_level_above_first: 1.0,
};
static SWEEPING_DENOMINATOR: LevelBasedValue = LevelBasedValue::Linear {
    base: 2.0,
    per_level_above_first: 1.0,
};

/// 官方 26.3 里带 `minecraft:attributes` 的八个附魔，逐条从
/// `data/minecraft/enchantment/*.json` 抄出的 amount / attribute / operation / slots。
/// `amount` 的 `calculate(level)` 是 `float`，`getModifier` 再 `f2d` 成 `double`。
///
/// The eight 26.3 enchantments carrying `minecraft:attributes`, transcribed from
/// the official enchantment JSON. `amount.calculate(level)` is a `float` that
/// `getModifier` widens to `double`.
static ENCHANTMENT_ATTRIBUTE_EFFECTS: &[EnchantmentAttributeEffect] = &[
    EnchantmentAttributeEffect {
        enchantment: "minecraft:aqua_affinity",
        id: "minecraft:enchantment.aqua_affinity",
        attribute: attribute::SUBMERGED_MINING_SPEED,
        amount: LevelBasedValue::Linear {
            base: 4.0,
            per_level_above_first: 4.0,
        },
        operation: Operation::AddMultipliedTotal,
        groups: &["head"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:blast_protection",
        id: "minecraft:enchantment.blast_protection",
        attribute: attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
        amount: LevelBasedValue::Linear {
            base: 0.15,
            per_level_above_first: 0.15,
        },
        operation: Operation::AddValue,
        groups: &["armor"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:depth_strider",
        id: "minecraft:enchantment.depth_strider",
        attribute: attribute::WATER_MOVEMENT_EFFICIENCY,
        amount: LevelBasedValue::Linear {
            base: 0.33333334,
            per_level_above_first: 0.33333334,
        },
        operation: Operation::AddValue,
        groups: &["feet"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:efficiency",
        id: "minecraft:enchantment.efficiency",
        attribute: attribute::MINING_EFFICIENCY,
        amount: LevelBasedValue::LevelsSquared { added: 1.0 },
        operation: Operation::AddValue,
        groups: &["mainhand"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:fire_protection",
        id: "minecraft:enchantment.fire_protection",
        attribute: attribute::BURNING_TIME,
        amount: LevelBasedValue::Linear {
            base: -0.15,
            per_level_above_first: -0.15,
        },
        operation: Operation::AddMultipliedBase,
        groups: &["armor"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:respiration",
        id: "minecraft:enchantment.respiration",
        attribute: attribute::OXYGEN_BONUS,
        amount: LevelBasedValue::Linear {
            base: 1.0,
            per_level_above_first: 1.0,
        },
        operation: Operation::AddValue,
        groups: &["head"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:sweeping_edge",
        id: "minecraft:enchantment.sweeping_edge",
        attribute: attribute::SWEEPING_DAMAGE_RATIO,
        amount: LevelBasedValue::Fraction {
            numerator: &SWEEPING_NUMERATOR,
            denominator: &SWEEPING_DENOMINATOR,
        },
        operation: Operation::AddValue,
        groups: &["mainhand"],
    },
    EnchantmentAttributeEffect {
        enchantment: "minecraft:swift_sneak",
        id: "minecraft:enchantment.swift_sneak",
        attribute: attribute::SNEAKING_SPEED,
        amount: LevelBasedValue::Linear {
            base: 0.15,
            per_level_above_first: 0.15,
        },
        operation: Operation::AddValue,
        groups: &["legs"],
    },
];

/// `Enchantment.matchingSlot(slot)`：`definition.slots()` 里任一组 `test(slot)` 为真。
///
/// `Enchantment.matchingSlot(slot)`: any definition slot group tests this slot.
fn matching_slot(groups: &[&str], slot: EquipmentSlot) -> bool {
    groups.iter().any(|group| {
        component_enums::names::EQUIPMENT_SLOT_GROUP
            .iter()
            .position(|name| *name == *group)
            .is_some_and(|group_id| slot_group_applies(group_id as i32, slot))
    })
}

/// `EnchantmentHelper.forEachModifier(stack, slot, ...)` 的属性部分：逐条附魔和等级，
/// `matchingSlot` 过滤，再按 `getModifier(level, slot)` 拼带槽位后缀的临时修饰符。
///
/// The attribute half of `EnchantmentHelper.forEachModifier`. Each enchantment
/// level whose `matchingSlot` passes yields a transient modifier with a
/// slot-suffixed id via `getModifier(level, slot)`.
fn enchantment_attribute_modifiers(
    patch: &ComponentPatch,
    item: i32,
    slot: EquipmentSlot,
) -> Vec<(i32, Modifier)> {
    let Some(ComponentValue::Enchantments(levels)) =
        defaults::resolve(item, patch, kinds::ENCHANTMENTS)
    else {
        return Vec::new();
    };
    let Some(registry) = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &(enchant_id, level) in levels.iter() {
        let Some(name) = usize::try_from(enchant_id)
            .ok()
            .and_then(|index| registry.entries.get(index))
            .copied()
        else {
            continue;
        };
        for effect in ENCHANTMENT_ATTRIBUTE_EFFECTS
            .iter()
            .filter(|effect| effect.enchantment == name)
        {
            if !matching_slot(effect.groups, slot) {
                continue;
            }
            out.push((
                effect.attribute,
                Modifier {
                    // getModifier: id = effect.id.withSuffix("/" + slot.getSerializedName())。
                    id: format!("{}/{}", effect.id, slot_name(slot)),
                    amount: f64::from(effect.amount.calculate(level)),
                    operation: effect.operation,
                    permanent: false,
                },
            ));
        }
    }
    out
}

/// `ItemStack.forEachModifier(slot, ...)`：先 `attribute_modifiers` 组件里组匹配这个槽的
/// 条目（按组件顺序），再 `EnchantmentHelper.forEachModifier` 的附魔属性效果。
/// 两者挂的都是临时修饰符（`addTransientModifier`），不存盘。
///
/// `ItemStack.forEachModifier(slot, ...)`: the component's matching entries in
/// order, then the enchantment `minecraft:attributes` effects. Both are transient.
pub fn stack_modifiers(stack: &ItemStack, slot: EquipmentSlot) -> Vec<(i32, Modifier)> {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(ComponentValue::AttributeModifiers(entries)) =
        defaults::resolve(stack.item, &patch, kinds::ATTRIBUTE_MODIFIERS)
    {
        for entry in entries {
            if !slot_group_applies(entry.slot, slot) {
                continue;
            }
            let operation = match entry.modifier.operation {
                0 => Operation::AddValue,
                1 => Operation::AddMultipliedBase,
                _ => Operation::AddMultipliedTotal,
            };
            out.push((
                entry.attribute,
                Modifier {
                    id: entry.modifier.id.clone(),
                    amount: entry.modifier.amount,
                    operation,
                    permanent: false,
                },
            ));
        }
    }
    out.extend(enchantment_attribute_modifiers(&patch, stack.item, slot));
    out
}

/// 全部八个槽，按官方 ordinal（含身体和鞍）。
///
/// All eight slots in vanilla ordinal order, including body and saddle.
pub const SLOTS: [EquipmentSlot; 8] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
    EquipmentSlot::Body,
    EquipmentSlot::Saddle,
];

/// `DropChances.DEFAULT_EQUIPMENT_DROP_CHANCE`。
pub const DEFAULT_DROP_CHANCE: f32 = 0.085;

/// `DropChances.withGuaranteedDrop` 写进去的值：大于 1 就算「保存」（必掉、不损耐久）。
pub const GUARANTEED_DROP_CHANCE: f32 = 2.0;

/// `EquipmentSlot.getSerializedName()`，NBT 里 `equipment` / `drop_chances` 的键。
pub const fn slot_name(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::MainHand => "mainhand",
        EquipmentSlot::OffHand => "offhand",
        EquipmentSlot::Feet => "feet",
        EquipmentSlot::Legs => "legs",
        EquipmentSlot::Chest => "chest",
        EquipmentSlot::Head => "head",
        EquipmentSlot::Body => "body",
        EquipmentSlot::Saddle => "saddle",
    }
}

/// `LivingEntity.entityEventForEquipmentBreak`：装备坏掉时广播的实体事件。
pub const fn break_event(slot: EquipmentSlot) -> u8 {
    match slot {
        EquipmentSlot::MainHand => 47,
        EquipmentSlot::OffHand => 48,
        EquipmentSlot::Head => 49,
        EquipmentSlot::Chest => 50,
        EquipmentSlot::Feet => 52,
        EquipmentSlot::Legs => 51,
        EquipmentSlot::Body => 65,
        EquipmentSlot::Saddle => 68,
    }
}

fn index(slot: EquipmentSlot) -> Option<usize> {
    SLOTS.iter().position(|candidate| *candidate == slot)
}

/// 一个生物的装备、掉落几率和上次同步出去的装备。
#[derive(Debug, Clone, PartialEq)]
pub struct MobEquipment {
    items: [ItemStack; SLOTS.len()],
    drop_chances: [f32; SLOTS.len()],
    /// `lastEquipmentItems`：上次 `detectEquipmentUpdates` 记下的副本，初始全空。
    last_sent: [ItemStack; SLOTS.len()],
}

impl Default for MobEquipment {
    fn default() -> Self {
        Self {
            items: std::array::from_fn(|_| ItemStack::empty()),
            drop_chances: [DEFAULT_DROP_CHANCE; SLOTS.len()],
            last_sent: std::array::from_fn(|_| ItemStack::empty()),
        }
    }
}

impl MobEquipment {
    /// `getItemBySlot`。不建模的槽恒空。
    pub fn get(&self, slot: EquipmentSlot) -> &ItemStack {
        static EMPTY: std::sync::OnceLock<ItemStack> = std::sync::OnceLock::new();
        match index(slot) {
            Some(at) => &self.items[at],
            None => EMPTY.get_or_init(ItemStack::empty),
        }
    }

    /// `setItemSlot`（不含 `onEquipItem` 的音效；生成和读档时官方也不放音效）。
    pub fn set(&mut self, slot: EquipmentSlot, stack: ItemStack) {
        if let Some(at) = index(slot) {
            self.items[at] = stack;
        }
    }

    /// 八个槽和其中的物品，按官方 ordinal。
    ///
    /// The eight slots and their items, in vanilla ordinal order.
    pub fn iter(&self) -> impl Iterator<Item = (EquipmentSlot, &ItemStack)> {
        SLOTS.iter().copied().zip(self.items.iter())
    }

    /// `EntityEquipment.isEmpty`。
    pub fn is_empty(&self) -> bool {
        self.items.iter().all(ItemStack::is_empty)
    }

    /// `DropChances.byEquipment`。
    pub fn drop_chance(&self, slot: EquipmentSlot) -> f32 {
        index(slot).map_or(DEFAULT_DROP_CHANCE, |at| self.drop_chances[at])
    }

    /// `Mob.setDropChance`。负数和 NaN 按官方 `NON_NEGATIVE_FLOAT` 拒收（这里直接忽略）。
    pub fn set_drop_chance(&mut self, slot: EquipmentSlot, chance: f32) {
        if chance.is_nan() || chance < 0.0 {
            return;
        }
        if let Some(at) = index(slot) {
            self.drop_chances[at] = chance;
        }
    }

    /// `DropChances.isPreserved`：几率大于 1。
    pub fn is_preserved(&self, slot: EquipmentSlot) -> bool {
        self.drop_chance(slot) > 1.0
    }

    /// 掉落几率是不是全是缺省值（`DropChances.DEFAULT`），是就不写 `drop_chances`。
    pub fn default_drop_chances(&self) -> bool {
        self.drop_chances
            .iter()
            .all(|chance| chance.to_bits() == DEFAULT_DROP_CHANCE.to_bits())
    }

    /// 改一个槽里的物品（耐久之类）。不建模的槽什么也不做。
    pub fn edit(&mut self, slot: EquipmentSlot, f: impl FnOnce(&mut ItemStack)) {
        if let Some(at) = index(slot) {
            f(&mut self.items[at]);
        }
    }

    /// `LivingEntity.detectEquipmentUpdates`：和上次同步出去的不一样（`ItemStack.matches`，
    /// 含组件和数量）的槽按 ordinal 收集，同时
    /// - `collectEquipmentChanges`：旧物品的属性修饰先摘掉，新物品（非空、没坏）的再挂上；
    /// - `handleHandSwap`：只是主副手互换时不发装备，改发实体事件 55；
    /// - 收集到的槽都记成已同步（`lastEquipmentItems`）。
    pub fn detect_updates(&mut self, attributes: &mut Attributes) -> EquipmentUpdate {
        let changed: Vec<usize> = (0..SLOTS.len())
            .filter(|&at| !same_stack(&self.items[at], &self.last_sent[at]))
            .collect();
        if changed.is_empty() {
            return EquipmentUpdate::default();
        }
        for &at in &changed {
            let old = &self.last_sent[at];
            if !old.is_empty() {
                remove_stack_modifiers(old, SLOTS[at], attributes);
            }
        }
        for &at in &changed {
            let new = &self.items[at];
            if new.is_empty() || crate::durability::is_broken_stack(new) {
                continue;
            }
            for (attribute, modifier) in stack_modifiers(new, SLOTS[at]) {
                if let Some(instance) = attributes.instance(attribute) {
                    instance.remove_modifier(&modifier.id);
                    instance.modifiers.push(modifier);
                }
            }
        }
        let (main, off) = (0, 1);
        let swapped = changed.contains(&main)
            && changed.contains(&off)
            && same_stack(&self.items[main], &self.last_sent[off])
            && same_stack(&self.items[off], &self.last_sent[main]);
        let mut update = EquipmentUpdate {
            hand_swap: swapped,
            changes: Vec::new(),
        };
        for &at in &changed {
            self.last_sent[at] = self.items[at].clone();
            if swapped && (at == main || at == off) {
                continue;
            }
            update.changes.push((SLOTS[at], self.items[at].clone()));
        }
        update
    }

    /// `EntityEquipment.CODEC`：非空槽的 `槽名 → ItemStack.CODEC`。编不出来的物品跳过。
    pub fn equipment_tag(&self) -> Tag {
        let mut entries = Vec::new();
        for (slot, stack) in self.iter() {
            if stack.is_empty() {
                continue;
            }
            if let Ok(tag) = mc_bamboo_protocol::components::archive::slots::encode_item(stack) {
                entries.push((slot_name(slot).to_owned(), tag));
            }
        }
        Tag::Compound(entries)
    }

    /// `DropChances.CODEC`：只写不等于缺省值的槽。
    pub fn drop_chances_tag(&self) -> Tag {
        let mut entries = Vec::new();
        for (at, slot) in SLOTS.iter().enumerate() {
            let chance = self.drop_chances[at];
            if chance.to_bits() != DEFAULT_DROP_CHANCE.to_bits() {
                entries.push((slot_name(*slot).to_owned(), Tag::Float(chance)));
            }
        }
        Tag::Compound(entries)
    }

    /// 读 `equipment`：认识的槽名、解得出来的物品才收；坏的条目跳过，不 panic。
    pub fn read_equipment(&mut self, tag: Option<&Tag>) {
        let Some(Tag::Compound(entries)) = tag else {
            return;
        };
        for (key, value) in entries {
            let Some(slot) = SLOTS.iter().copied().find(|slot| slot_name(*slot) == key) else {
                continue;
            };
            if let Ok(stack) = mc_bamboo_protocol::components::archive::slots::decode_item(value) {
                self.set(slot, stack);
            }
        }
    }

    /// 读 `drop_chances`：数值标签按 float 收，负数跳过。
    pub fn read_drop_chances(&mut self, tag: Option<&Tag>) {
        let Some(Tag::Compound(entries)) = tag else {
            return;
        };
        for (key, value) in entries {
            let Some(slot) = SLOTS.iter().copied().find(|slot| slot_name(*slot) == key) else {
                continue;
            };
            let chance = match value {
                Tag::Float(value) => *value,
                Tag::Double(value) => *value as f32,
                Tag::Int(value) => *value as f32,
                Tag::Byte(value) => f32::from(*value),
                Tag::Short(value) => f32::from(*value),
                Tag::Long(value) => *value as f32,
                _ => continue,
            };
            self.set_drop_chance(slot, chance);
        }
    }
}

/// `EquipmentSlot.isArmor()`：humanoid 四件（脚、腿、胸、头）加动物护甲（body）。
pub const fn is_armor_slot(slot: EquipmentSlot) -> bool {
    matches!(
        slot,
        EquipmentSlot::Feet
            | EquipmentSlot::Legs
            | EquipmentSlot::Chest
            | EquipmentSlot::Head
            | EquipmentSlot::Body
    )
}

/// `EquipmentSlot.limit(stack)`：`countLimit > 0` 时把数量压到上限。主手 / 副手没有
/// 上限（`NO_COUNT_LIMIT`），其余槽上限 1。
fn slot_limit(slot: EquipmentSlot, mut stack: ItemStack) -> ItemStack {
    let limit = if matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand) {
        -1
    } else {
        1
    };
    if limit > 0 {
        stack.count = stack.count.min(limit);
    }
    stack
}

/// `minecraft:equipment_slot` 注册表 ID → 协议 `EquipmentSlot`（枚举值是 ordinal）。
/// 注册表顺序是官方 `EquipmentSlot.getId()`：mainhand、feet、legs、chest、head、
/// offhand、body、saddle。
const fn equipment_slot_from_registry(id: i32) -> EquipmentSlot {
    match id {
        0 => EquipmentSlot::MainHand,
        1 => EquipmentSlot::Feet,
        2 => EquipmentSlot::Legs,
        3 => EquipmentSlot::Chest,
        4 => EquipmentSlot::Head,
        5 => EquipmentSlot::OffHand,
        6 => EquipmentSlot::Body,
        _ => EquipmentSlot::Saddle,
    }
}

/// 协议 `EquipmentSlot` → `minecraft:equipment_slot` 注册表 ID。
const fn equipment_slot_to_registry(slot: EquipmentSlot) -> i32 {
    match slot {
        EquipmentSlot::MainHand => 0,
        EquipmentSlot::OffHand => 5,
        EquipmentSlot::Feet => 1,
        EquipmentSlot::Legs => 2,
        EquipmentSlot::Chest => 3,
        EquipmentSlot::Head => 4,
        EquipmentSlot::Body => 6,
        EquipmentSlot::Saddle => 7,
    }
}

/// `LivingEntity.getEquipmentSlotForItem`：`equippable` 组件给的槽，没有组件就是主手。
fn equippable_slot(stack: &ItemStack) -> EquipmentSlot {
    equipment_slot_from_registry(mc_bamboo_recipes::equipment_slot_for_item(stack))
}

/// `LivingEntity.isEquippableInSlot`（玩家版本）：`equippable` 组件的槽和这个槽一致。
/// `canBeEquippedBy` 的 `allowed_entities` 没复刻，见 README 已知限制。
fn equippable_in_slot(stack: &ItemStack, slot: EquipmentSlot) -> bool {
    mc_bamboo_recipes::is_equippable_in_slot(stack, equipment_slot_to_registry(slot))
}

/// `Mob.canHoldItem`：缺省恒真，狐狸按 `Fox.canHoldItem` 覆盖。
fn can_hold_item(mob: &Mob, stack: &ItemStack) -> bool {
    if mob.mob_type == MobType::Fox {
        super::fox::can_hold(mob, stack)
    } else {
        true
    }
}

/// `Mob.getPreferredWeaponType`：这些生物偏爱某类武器；其余是 `None`。
fn preferred_weapon(mob_type: MobType) -> Option<mc_bamboo_data::ItemTag> {
    use mc_bamboo_data::tags::item_tags;
    match mob_type {
        MobType::Skeleton | MobType::Stray | MobType::Bogged | MobType::Parched => {
            Some(item_tags::SKELETON_PREFERRED_WEAPONS)
        }
        MobType::Drowned => Some(item_tags::DROWNED_PREFERRED_WEAPONS),
        MobType::Pillager => Some(item_tags::PILLAGER_PREFERRED_WEAPONS),
        _ => None,
    }
}

/// `EnchantmentHelper.has(stack, PREVENT_ARMOR_CHANGE)`：26.3 里只有
/// `minecraft:binding_curse`（绑定诅咒）带这个效果。
fn has_prevent_armor_change(stack: &ItemStack) -> bool {
    crate::entity::arrow::enchantment_level(stack, "minecraft:binding_curse") > 0
}

/// `ItemStack.getOrDefault(ENCHANTMENTS, EMPTY).entrySet().size()`。
fn enchantment_count(stack: &ItemStack) -> usize {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return 0;
    };
    match defaults::resolve(stack.item, &patch, kinds::ENCHANTMENTS) {
        Some(ComponentValue::Enchantments(levels)) => levels.len(),
        _ => 0,
    }
}

/// `ItemStack.has(CUSTOM_NAME)`。
fn has_custom_name(stack: &ItemStack) -> bool {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    defaults::resolve(stack.item, &patch, kinds::CUSTOM_NAME).is_some()
}

/// `Mob.canReplaceEqualItem`：附魔多者优先；一样多时耐久高（伤害值低）者优先；
/// 再一样时新的有自定义名而旧的没有则换。
fn can_replace_equal_item(new: &ItemStack, current: &ItemStack) -> bool {
    let new_ench = enchantment_count(new);
    let cur_ench = enchantment_count(current);
    if new_ench != cur_ench {
        return new_ench > cur_ench;
    }
    let new_damage = crate::durability::damage_of(new).unwrap_or(0);
    let cur_damage = crate::durability::damage_of(current).unwrap_or(0);
    if new_damage != cur_damage {
        return new_damage < cur_damage;
    }
    has_custom_name(new) && !has_custom_name(current)
}

/// `Mob.getApproximateAttributeWith`：基础值加上物品在这个槽给的属性修饰符，
/// 单遍、按修饰符列表顺序。
fn approximate_attribute(mob: &Mob, stack: &ItemStack, attr: i32, slot: EquipmentSlot) -> f64 {
    let base = mob.attributes.base(attr);
    let mut value = base;
    for (attribute, modifier) in stack_modifiers(stack, slot) {
        if attribute != attr {
            continue;
        }
        match modifier.operation {
            Operation::AddValue => value += modifier.amount,
            Operation::AddMultipliedBase => value += modifier.amount * base,
            Operation::AddMultipliedTotal => value += modifier.amount * value,
        }
    }
    value
}

/// `Mob.compareArmor`：旧的带 `prevent_armor_change` 不换；否则比护甲值，再比韧性，
/// 都相等时走 [`can_replace_equal_item`]。
fn compare_armor(mob: &Mob, new: &ItemStack, current: &ItemStack, slot: EquipmentSlot) -> bool {
    if has_prevent_armor_change(current) {
        return false;
    }
    let new_armor = approximate_attribute(mob, new, attribute::ARMOR, slot);
    let cur_armor = approximate_attribute(mob, current, attribute::ARMOR, slot);
    if new_armor != cur_armor {
        return new_armor > cur_armor;
    }
    let new_toughness = approximate_attribute(mob, new, attribute::ARMOR_TOUGHNESS, slot);
    let cur_toughness = approximate_attribute(mob, current, attribute::ARMOR_TOUGHNESS, slot);
    if new_toughness != cur_toughness {
        return new_toughness > cur_toughness;
    }
    can_replace_equal_item(new, current)
}

/// `Mob.compareWeapons`：先按偏爱武器标签，再比攻击力，都相等时走
/// [`can_replace_equal_item`]。
fn compare_weapons(mob: &Mob, new: &ItemStack, current: &ItemStack, slot: EquipmentSlot) -> bool {
    if let Some(tag) = preferred_weapon(mob.mob_type) {
        let current_is = crate::state::item_is_tagged(current.item, tag);
        let new_is = crate::state::item_is_tagged(new.item, tag);
        if current_is && !new_is {
            return false;
        }
        if !current_is && new_is {
            return true;
        }
    }
    let new_damage = approximate_attribute(mob, new, attribute::ATTACK_DAMAGE, slot);
    let cur_damage = approximate_attribute(mob, current, attribute::ATTACK_DAMAGE, slot);
    if new_damage != cur_damage {
        return new_damage > cur_damage;
    }
    can_replace_equal_item(new, current)
}

/// `Mob.canReplaceCurrentItem`：槽是空的直接换；护甲槽走 [`compare_armor`]，主手走
/// [`compare_weapons`]，其余槽（副手、身体、鞍）不换。
fn can_replace_current_item(
    mob: &Mob,
    new: &ItemStack,
    current: &ItemStack,
    slot: EquipmentSlot,
) -> bool {
    if current.is_empty() {
        return true;
    }
    if is_armor_slot(slot) {
        return compare_armor(mob, new, current, slot);
    }
    if slot == EquipmentSlot::MainHand {
        return compare_weapons(mob, new, current, slot);
    }
    false
}

impl Mob {
    /// `Mob.equipItemIfPossible(level, stack)`：把一件物品套进合适的装备槽。
    ///
    /// 槽里已有装备且换上更差时（护甲 / 武器比较、绑定诅咒）套不上，返回空堆；
    /// 换上更好的会把旧的那件按掉落几率掉出来（返回的 `dropped` 交给调用方
    /// `spawnAtLocation`），新件按槽上限收进 `leftover`。套上后 `persistenceRequired`
    /// 置真、掉落几率设成必掉。`random` 是这只生物自己的随机源。
    ///
    /// `Mob.equipItemIfPossible(level, stack)`: put an item into its equipment slot.
    /// A worse replacement (armor / weapon comparison, binding curse) leaves the
    /// slot alone and returns an empty stack; a better one drops the old item by
    /// chance (the returned `dropped`, which the caller spawns) and keeps the new
    /// one limited to the slot cap in `leftover`. On success `persistenceRequired`
    /// is set and the drop chance becomes guaranteed. `random` is the mob's source.
    pub fn equip_item_if_possible(
        &mut self,
        random: &mut crate::entity::JavaRandom,
        stack: ItemStack,
    ) -> (ItemStack, Option<ItemStack>) {
        let slot = equippable_slot(&stack);
        if !equippable_in_slot(&stack, slot) {
            return (ItemStack::empty(), None);
        }
        let mut slot = slot;
        let mut current = self.equipment.get(slot).clone();
        let mut can_replace = can_replace_current_item(self, &stack, &current, slot);
        // `slot.isArmor() && !canReplace`：护甲换不上就改试主手。
        if is_armor_slot(slot) && !can_replace {
            slot = EquipmentSlot::MainHand;
            current = self.equipment.get(slot).clone();
            can_replace = current.is_empty();
        }
        if !can_replace || !can_hold_item(self, &stack) {
            return (ItemStack::empty(), None);
        }
        // 旧的那件按 `max(0, nextFloat() - 0.1) < dropChance` 掉出来。
        let dropped = if current.is_empty() {
            None
        } else {
            let roll = (random.next_float() - 0.1).max(0.0);
            (roll < self.equipment.drop_chance(slot)).then_some(current)
        };
        let limited = slot_limit(slot, stack);
        self.equipment.set(slot, limited.clone());
        self.equipment.set_drop_chance(slot, GUARANTEED_DROP_CHANCE);
        self.persistence_required = true;
        (limited, dropped)
    }
}

/// `ItemStack.matches`：两个都空，或者物品、数量和组件都一样。
fn same_stack(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() || b.is_empty() {
        return a.is_empty() && b.is_empty();
    }
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_reported_once_in_slot_order() {
        let mut equipment = MobEquipment::default();
        let mut attributes = Attributes::zombie();
        assert_eq!(
            equipment.detect_updates(&mut attributes),
            EquipmentUpdate::default()
        );
        equipment.set(
            EquipmentSlot::Head,
            ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1),
        );
        equipment.set(
            EquipmentSlot::MainHand,
            ItemStack::new(mc_bamboo_data::items::IRON_SWORD, 1),
        );
        let armor = attributes.value(mc_bamboo_data::registries::attribute::ARMOR);
        let update = equipment.detect_updates(&mut attributes);
        let slots: Vec<_> = update.changes.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, [EquipmentSlot::MainHand, EquipmentSlot::Head]);
        // 铁头盔 +2 护甲，摘下来又回到原值。
        let with_helmet = attributes.value(mc_bamboo_data::registries::attribute::ARMOR);
        assert_eq!(with_helmet, armor + 2.0);
        assert!(equipment.detect_updates(&mut attributes).changes.is_empty());
        equipment.set(EquipmentSlot::Head, ItemStack::empty());
        let update = equipment.detect_updates(&mut attributes);
        assert_eq!(update.changes.len(), 1);
        assert_eq!(
            attributes.value(mc_bamboo_data::registries::attribute::ARMOR),
            armor
        );
    }

    #[test]
    fn swapping_hands_is_an_entity_event_not_an_equipment_packet() {
        let mut equipment = MobEquipment::default();
        let mut attributes = Attributes::zombie();
        equipment.set(
            EquipmentSlot::MainHand,
            ItemStack::new(mc_bamboo_data::items::IRON_SWORD, 1),
        );
        equipment.detect_updates(&mut attributes);
        let sword = equipment.get(EquipmentSlot::MainHand).clone();
        equipment.set(EquipmentSlot::MainHand, ItemStack::empty());
        equipment.set(EquipmentSlot::OffHand, sword);
        let update = equipment.detect_updates(&mut attributes);
        assert!(update.hand_swap);
        assert!(update.changes.is_empty());
    }

    #[test]
    fn default_drop_chances_are_not_written() {
        let mut equipment = MobEquipment::default();
        assert!(equipment.default_drop_chances());
        assert_eq!(equipment.drop_chances_tag(), Tag::Compound(Vec::new()));
        equipment.set_drop_chance(EquipmentSlot::Head, GUARANTEED_DROP_CHANCE);
        equipment.set_drop_chance(EquipmentSlot::Feet, -1.0);
        assert!(!equipment.default_drop_chances());
        assert!(equipment.is_preserved(EquipmentSlot::Head));
        assert_eq!(
            equipment.drop_chances_tag(),
            Tag::Compound(vec![("head".to_owned(), Tag::Float(2.0))])
        );
    }

    #[test]
    fn broken_entries_in_the_archive_are_skipped() {
        let mut equipment = MobEquipment::default();
        equipment.read_equipment(Some(&Tag::Compound(vec![
            ("head".to_owned(), Tag::Int(3)),
            ("tail".to_owned(), Tag::Compound(Vec::new())),
        ])));
        assert!(equipment.is_empty());
        equipment.read_drop_chances(Some(&Tag::Compound(vec![(
            "legs".to_owned(),
            Tag::String("x".to_owned()),
        )])));
        assert!(equipment.default_drop_chances());
    }

    fn enchantment_id(name: &str) -> i32 {
        SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
            .expect("enchantment is registered") as i32
    }

    /// 一个挂了单条附魔的 `DIAMOND_PICKAXE`。
    fn enchanted(name: &str, level: i32) -> ItemStack {
        use mc_bamboo_data::registries::data_component_type::ENCHANTMENTS;
        use mc_bamboo_protocol::components::PatchEntry;

        let mut stack = ItemStack::new(mc_bamboo_data::items::DIAMOND_PICKAXE, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(
                enchantment_id(name),
                level,
            )])),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        stack
    }

    /// `stack_modifiers` 里只看附魔那部分（物品自己的 `attribute_modifiers` 也会在结果里）。
    fn enchant_modifiers(stack: &ItemStack, slot: EquipmentSlot) -> Vec<(i32, Modifier)> {
        stack_modifiers(stack, slot)
            .into_iter()
            .filter(|(_, modifier)| modifier.id.starts_with("minecraft:enchantment."))
            .collect()
    }

    #[test]
    fn level_based_value_matches_the_official_formulas() {
        // levels_squared: Mth.square(level) + added。效率 V = 25 + 1 = 26。
        assert_eq!(
            LevelBasedValue::LevelsSquared { added: 1.0 }.calculate(5),
            26.0
        );
        assert_eq!(
            LevelBasedValue::LevelsSquared { added: 1.0 }.calculate(1),
            2.0
        );
        // linear: base + per * (level - 1)。
        assert_eq!(
            LevelBasedValue::Linear {
                base: 1.0,
                per_level_above_first: 1.0
            }
            .calculate(3),
            3.0
        );
        // fraction: 分母为 0 时是 0，否则 numerator / denominator。
        assert_eq!(
            LevelBasedValue::Fraction {
                numerator: &SWEEPING_NUMERATOR,
                denominator: &SWEEPING_DENOMINATOR,
            }
            .calculate(1),
            0.5
        );
        assert_eq!(
            LevelBasedValue::Fraction {
                numerator: &SWEEPING_NUMERATOR,
                denominator: &LevelBasedValue::Linear {
                    base: 0.0,
                    per_level_above_first: 0.0,
                },
            }
            .calculate(3),
            0.0
        );
    }

    #[test]
    fn efficiency_v_adds_twenty_six_mining_efficiency_in_the_main_hand() {
        let mods = enchant_modifiers(
            &enchanted("minecraft:efficiency", 5),
            EquipmentSlot::MainHand,
        );
        assert_eq!(mods.len(), 1);
        let (attribute, modifier) = &mods[0];
        assert_eq!(
            *attribute,
            mc_bamboo_data::registries::attribute::MINING_EFFICIENCY
        );
        assert_eq!(modifier.id, "minecraft:enchantment.efficiency/mainhand");
        assert_eq!(modifier.amount, 26.0);
        assert_eq!(modifier.operation, Operation::AddValue);
        assert!(!modifier.permanent);
    }

    #[test]
    fn enchantment_modifiers_only_apply_in_matching_slots() {
        // efficiency 的 slots=[mainhand]：戴头上、放副手都不给修饰符。
        for slot in [
            EquipmentSlot::Head,
            EquipmentSlot::OffHand,
            EquipmentSlot::Feet,
        ] {
            assert!(
                enchant_modifiers(&enchanted("minecraft:efficiency", 5), slot).is_empty(),
                "efficiency must not apply in {slot:?}"
            );
        }
        // armor 组：胸甲有，主手没有。
        let blast = enchanted("minecraft:blast_protection", 2);
        assert_eq!(enchant_modifiers(&blast, EquipmentSlot::Chest).len(), 1);
        assert!(enchant_modifiers(&blast, EquipmentSlot::MainHand).is_empty());
    }

    #[test]
    fn every_official_attribute_enchantment_yields_its_effect() {
        // (name, level, slot, 期望属性, 期望 amount, 期望运算, 期望 id)。
        let cases: &[(&str, i32, EquipmentSlot, i32, f64, Operation, &str)] = &[
            (
                "minecraft:aqua_affinity",
                1,
                EquipmentSlot::Head,
                mc_bamboo_data::registries::attribute::SUBMERGED_MINING_SPEED,
                4.0,
                Operation::AddMultipliedTotal,
                "minecraft:enchantment.aqua_affinity/head",
            ),
            (
                "minecraft:blast_protection",
                1,
                EquipmentSlot::Chest,
                mc_bamboo_data::registries::attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
                f64::from(0.15_f32),
                Operation::AddValue,
                "minecraft:enchantment.blast_protection/chest",
            ),
            (
                "minecraft:depth_strider",
                1,
                EquipmentSlot::Feet,
                mc_bamboo_data::registries::attribute::WATER_MOVEMENT_EFFICIENCY,
                f64::from(0.33333334_f32),
                Operation::AddValue,
                "minecraft:enchantment.depth_strider/feet",
            ),
            (
                "minecraft:efficiency",
                3,
                EquipmentSlot::MainHand,
                mc_bamboo_data::registries::attribute::MINING_EFFICIENCY,
                10.0,
                Operation::AddValue,
                "minecraft:enchantment.efficiency/mainhand",
            ),
            (
                "minecraft:fire_protection",
                1,
                EquipmentSlot::Legs,
                mc_bamboo_data::registries::attribute::BURNING_TIME,
                f64::from(-0.15_f32),
                Operation::AddMultipliedBase,
                "minecraft:enchantment.fire_protection/legs",
            ),
            (
                "minecraft:respiration",
                3,
                EquipmentSlot::Head,
                mc_bamboo_data::registries::attribute::OXYGEN_BONUS,
                3.0,
                Operation::AddValue,
                "minecraft:enchantment.respiration/head",
            ),
            (
                "minecraft:sweeping_edge",
                3,
                EquipmentSlot::MainHand,
                mc_bamboo_data::registries::attribute::SWEEPING_DAMAGE_RATIO,
                0.75,
                Operation::AddValue,
                "minecraft:enchantment.sweeping_edge/mainhand",
            ),
            (
                "minecraft:swift_sneak",
                3,
                EquipmentSlot::Legs,
                mc_bamboo_data::registries::attribute::SNEAKING_SPEED,
                f64::from(0.15_f32 + 0.15_f32 * 2.0_f32),
                Operation::AddValue,
                "minecraft:enchantment.swift_sneak/legs",
            ),
        ];
        for &(name, level, slot, want_attribute, want_amount, want_operation, want_id) in cases {
            let mods = enchant_modifiers(&enchanted(name, level), slot);
            assert_eq!(mods.len(), 1, "{name} in {slot:?}");
            let (attribute, modifier) = &mods[0];
            assert_eq!(*attribute, want_attribute, "{name} attribute");
            assert_eq!(modifier.id, want_id, "{name} id");
            assert_eq!(modifier.amount, want_amount, "{name} amount");
            assert_eq!(modifier.operation, want_operation, "{name} operation");
        }
    }

    #[test]
    fn an_enchantment_with_no_attribute_effect_yields_nothing() {
        // 锋利没有 minecraft:attributes，不能凭空造出修饰符。
        assert!(
            enchant_modifiers(
                &enchanted("minecraft:sharpness", 5),
                EquipmentSlot::MainHand
            )
            .is_empty()
        );
    }

    #[test]
    fn extreme_enchantment_levels_do_not_panic() {
        // 客户端能传任意等级；linear 的 level-1、levels_squared 的乘法都要回绕不 panic。
        for level in [i32::MIN, -1, 0, 1, i32::MAX] {
            let _ = enchant_modifiers(
                &enchanted("minecraft:efficiency", level),
                EquipmentSlot::MainHand,
            );
            let _ = enchant_modifiers(
                &enchanted("minecraft:respiration", level),
                EquipmentSlot::Head,
            );
        }
    }

    /// 任意物品挂一条附魔。
    fn enchanted_item(item: i32, name: &str, level: i32) -> ItemStack {
        use mc_bamboo_data::registries::data_component_type::ENCHANTMENTS;
        use mc_bamboo_protocol::components::PatchEntry;

        let mut stack = ItemStack::new(item, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(
                enchantment_id(name),
                level,
            )])),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        stack
    }

    #[test]
    fn equip_item_if_possible_fills_an_empty_slot() {
        let mut mob = Mob::new(MobType::Zombie);
        let mut random = crate::entity::JavaRandom::new(0);
        let sword = ItemStack::new(mc_bamboo_data::items::IRON_SWORD, 1);
        let (leftover, dropped) = mob.equip_item_if_possible(&mut random, sword.clone());
        assert_eq!(leftover, sword);
        assert!(dropped.is_none());
        assert_eq!(mob.equipment.get(EquipmentSlot::MainHand), &sword);
        assert!(mob.persistence_required);
        assert!(mob.equipment.is_preserved(EquipmentSlot::MainHand));
    }

    #[test]
    fn equip_item_if_possible_replaces_worse_armor() {
        let mut mob = Mob::new(MobType::Zombie);
        mob.equipment.set_drop_chance(EquipmentSlot::Head, 0.0);
        mob.equipment.set(
            EquipmentSlot::Head,
            ItemStack::new(mc_bamboo_data::items::LEATHER_HELMET, 1),
        );
        let mut random = crate::entity::JavaRandom::new(0);
        let iron = ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1);
        let (leftover, dropped) = mob.equip_item_if_possible(&mut random, iron.clone());
        assert_eq!(leftover, iron);
        assert!(dropped.is_none(), "drop chance 0 不掉旧头盔");
        assert_eq!(mob.equipment.get(EquipmentSlot::Head), &iron);
    }

    #[test]
    fn worse_armor_falls_back_to_the_main_hand() {
        let mut mob = Mob::new(MobType::Zombie);
        mob.equipment.set(
            EquipmentSlot::Head,
            ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1),
        );
        let mut random = crate::entity::JavaRandom::new(0);
        let leather = ItemStack::new(mc_bamboo_data::items::LEATHER_HELMET, 1);
        let (leftover, _) = mob.equip_item_if_possible(&mut random, leather.clone());
        assert_eq!(leftover, leather, "皮头盔比铁头盔差，改放主手");
        assert_eq!(
            mob.equipment.get(EquipmentSlot::Head),
            &ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1)
        );
        assert_eq!(mob.equipment.get(EquipmentSlot::MainHand), &leather);
    }

    #[test]
    fn equip_item_if_possible_drops_the_replaced_item_by_chance() {
        let mut mob = Mob::new(MobType::Zombie);
        mob.equipment
            .set_drop_chance(EquipmentSlot::Head, GUARANTEED_DROP_CHANCE);
        mob.equipment.set(
            EquipmentSlot::Head,
            ItemStack::new(mc_bamboo_data::items::LEATHER_HELMET, 1),
        );
        let mut random = crate::entity::JavaRandom::new(0);
        let iron = ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1);
        let (leftover, dropped) = mob.equip_item_if_possible(&mut random, iron);
        assert_eq!(
            leftover,
            ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1)
        );
        assert_eq!(
            dropped,
            Some(ItemStack::new(mc_bamboo_data::items::LEATHER_HELMET, 1)),
            "必掉几率下旧头盔掉出来"
        );
    }

    #[test]
    fn equip_item_if_possible_keeps_binding_curse_armor() {
        let mut mob = Mob::new(MobType::Zombie);
        let cursed = enchanted_item(
            mc_bamboo_data::items::IRON_HELMET,
            "minecraft:binding_curse",
            1,
        );
        mob.equipment.set(EquipmentSlot::Head, cursed);
        let mut random = crate::entity::JavaRandom::new(0);
        let diamond = ItemStack::new(mc_bamboo_data::items::DIAMOND_HELMET, 1);
        let (leftover, _) = mob.equip_item_if_possible(&mut random, diamond.clone());
        assert_eq!(leftover, diamond, "绑定诅咒挡下头盔，钻石头盔改放主手");
        assert_eq!(
            mob.equipment.get(EquipmentSlot::Head).item,
            mc_bamboo_data::items::IRON_HELMET
        );
        assert_eq!(mob.equipment.get(EquipmentSlot::MainHand), &diamond);
    }
}
