//! 生物的装备：`LivingEntity.equipment`（`EntityEquipment`）、`Mob.dropChances`
//! （`DropChances`）和 `LivingEntity.lastEquipmentItems`（`detectEquipmentUpdates` 的比较基准）。
//!
//! 只建模六个人形槽（主手、副手、脚、腿、胸、头），顺序是官方 `EquipmentSlot` 的 ordinal，
//! 和协议里的 [`EquipmentSlot`] 一致。`BODY` / `SADDLE` 是马和狼的槽，这里的生物用不到。
//!
//! Mob equipment: the six humanoid slots in vanilla `EquipmentSlot` ordinal order, the
//! drop chances and the last-sent copies `detectEquipmentUpdates` compares against.

use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::attributes::{Attributes, Modifier, Operation};

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

/// `ItemStack.forEachModifier(slot, ...)` 的组件部分：`attribute_modifiers` 里组匹配这个槽的
/// 条目，按组件顺序。挂上去的都是临时修饰符（`addTransientModifier`），不存盘。
/// 附魔给的属性效果（`EnchantmentHelper.forEachModifier`）这一期不建模。
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
    out
}

/// 六个槽，按官方 ordinal。
pub const SLOTS: [EquipmentSlot; 6] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
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
    items: [ItemStack; 6],
    drop_chances: [f32; 6],
    /// `lastEquipmentItems`：上次 `detectEquipmentUpdates` 记下的副本，初始全空。
    last_sent: [ItemStack; 6],
}

impl Default for MobEquipment {
    fn default() -> Self {
        Self {
            items: std::array::from_fn(|_| ItemStack::empty()),
            drop_chances: [DEFAULT_DROP_CHANCE; 6],
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

    /// 六个槽和其中的物品，按官方 ordinal。
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
}
