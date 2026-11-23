//! 生物的属性，官方 `AttributeMap` / `AttributeInstance` 里牛用得到的部分。
//!
//! 每个属性一个基础值加修饰符表；取值按 `calculateValue` 的三步（加值、乘基础值、
//! 乘总值）再夹到属性的范围里。官方同一种运算的修饰符按哈希顺序遍历，牛每种运算
//! 最多只有一个修饰符，这里按加入顺序遍历，结果相同。
//!
//! Mob attributes: the parts of vanilla `AttributeMap` / `AttributeInstance` a cow uses.
//! Values follow `calculateValue` (add, multiply base, multiply total) and are clamped to
//! the attribute range. Vanilla iterates one operation's modifiers in hash order; a cow has
//! at most one per operation, so insertion order gives the same result.

use mc_bamboo_data::registries::attribute;

/// 修饰符的运算，`AttributeModifier.Operation`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

/// 一个修饰符。`id` 是官方 `Identifier` 的文本；存档里读回的修饰符名字任意，所以是 `String`。
///
/// A modifier. `id` is the text of the vanilla `Identifier`; names read back from
/// disk are arbitrary, hence a `String`.
#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    pub id: String,
    pub amount: f64,
    pub operation: Operation,
    /// 永久修饰符会存盘，临时的（细雪减速）不会。
    pub permanent: bool,
}

/// 一个属性实例。`instantiated` 记官方 `AttributeMap` 有没有懒创建过这个实例，
/// 存盘时只写创建过的（官方 `AttributeMap.save` 只遍历已有的实例）。
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub id: i32,
    pub base: f64,
    pub modifiers: Vec<Modifier>,
    pub instantiated: bool,
    /// 官方 `AttributeInstance.setDirty()` 的代数计数：即使改完后值与修饰符表和上次相同
    /// （例如疾跑每刻删了又加 `minecraft:sprinting`），官方仍会发 `update_attributes`。
    ///
    /// Generation counter for vanilla `AttributeInstance.setDirty()`: even when the final
    /// value and modifier list match the last sync (e.g. sprinting removes and re-adds
    /// `minecraft:sprinting` every tick), vanilla still emits `update_attributes`.
    pub sync_version: u64,
}

/// 属性的取值范围，`RangedAttribute` 的 `minValue` / `maxValue`。
fn range(id: i32) -> (f64, f64) {
    match id {
        attribute::AIR_DRAG_MODIFIER | attribute::FRICTION_MODIFIER => (0.0, 2048.0),
        attribute::ARMOR => (0.0, 30.0),
        attribute::ARMOR_TOUGHNESS => (0.0, 20.0),
        attribute::BOUNCINESS
        | attribute::MOVEMENT_EFFICIENCY
        | attribute::WATER_MOVEMENT_EFFICIENCY
        | attribute::EXPLOSION_KNOCKBACK_RESISTANCE => (0.0, 1.0),
        attribute::BURNING_TIME => (0.0, 1024.0),
        attribute::FALL_DAMAGE_MULTIPLIER => (0.0, 100.0),
        attribute::FOLLOW_RANGE | attribute::TEMPT_RANGE | attribute::MAX_ABSORPTION => {
            (0.0, 2048.0)
        }
        attribute::GRAVITY => (-1.0, 1.0),
        attribute::JUMP_STRENGTH => (0.0, 32.0),
        attribute::KNOCKBACK_RESISTANCE => (-2.0, 1.0),
        attribute::MAX_HEALTH => (1.0, 1024.0),
        attribute::MOVEMENT_SPEED | attribute::OXYGEN_BONUS => (0.0, 1024.0),
        attribute::SAFE_FALL_DISTANCE => (-1024.0, 1024.0),
        attribute::SCALE => (0.0625, 16.0),
        attribute::STEP_HEIGHT => (0.0, 10.0),
        _ => (f64::NEG_INFINITY, f64::INFINITY),
    }
}

impl Attribute {
    pub fn new(id: i32, base: f64) -> Self {
        Self {
            id,
            base,
            modifiers: Vec::new(),
            instantiated: false,
            sync_version: 0,
        }
    }

    /// `AttributeInstance.getValue`。
    pub fn value(&self) -> f64 {
        let mut base = self.base;
        for modifier in &self.modifiers {
            if modifier.operation == Operation::AddValue {
                base += modifier.amount;
            }
        }
        let mut value = base;
        for modifier in &self.modifiers {
            if modifier.operation == Operation::AddMultipliedBase {
                value += base * modifier.amount;
            }
        }
        for modifier in &self.modifiers {
            if modifier.operation == Operation::AddMultipliedTotal {
                value *= 1.0 + modifier.amount;
            }
        }
        let (min, max) = range(self.id);
        if value.is_nan() {
            min
        } else {
            mc_bamboo_core::mth::clamp(value, min, max)
        }
    }

    pub fn modifier(&self, id: &str) -> Option<&Modifier> {
        self.modifiers.iter().find(|modifier| modifier.id == id)
    }

    pub fn remove_modifier(&mut self, id: &str) -> bool {
        let before = self.modifiers.len();
        self.modifiers.retain(|modifier| modifier.id != id);
        self.modifiers.len() != before
    }

    /// 标记一次官方 `setDirty()`，让下一次属性同步发出 `update_attributes`。
    ///
    /// Record a vanilla `setDirty()` so the next attribute sync emits `update_attributes`.
    pub fn touch_sync(&mut self) {
        self.sync_version = self.sync_version.wrapping_add(1);
    }
}

/// 牛的属性表。只放官方 `AbstractCow.createAttributes` 给的那些。
#[derive(Debug, Clone, PartialEq)]
pub struct Attributes {
    pub entries: Vec<Attribute>,
}

impl Attributes {
    /// `AbstractCow.createAttributes()`：`createLivingAttributes` 的全部，
    /// 加上跟随距离 16、诱惑距离 10、生命 10、速度 `(double) 0.2F`。
    pub fn cow() -> Self {
        use mc_bamboo_data::registries::attribute_defaults as defaults;
        let living = [
            (attribute::MAX_HEALTH, 10.0),
            (
                attribute::KNOCKBACK_RESISTANCE,
                defaults::KNOCKBACK_RESISTANCE,
            ),
            (attribute::MOVEMENT_SPEED, f64::from(0.2_f32)),
            (attribute::ARMOR, defaults::ARMOR),
            (attribute::ARMOR_TOUGHNESS, defaults::ARMOR_TOUGHNESS),
            (attribute::MAX_ABSORPTION, defaults::MAX_ABSORPTION),
            (attribute::STEP_HEIGHT, defaults::STEP_HEIGHT),
            (attribute::SCALE, defaults::SCALE),
            (attribute::GRAVITY, defaults::GRAVITY),
            (attribute::SAFE_FALL_DISTANCE, defaults::SAFE_FALL_DISTANCE),
            (
                attribute::FALL_DAMAGE_MULTIPLIER,
                defaults::FALL_DAMAGE_MULTIPLIER,
            ),
            (attribute::JUMP_STRENGTH, defaults::JUMP_STRENGTH),
            (
                attribute::ENTITY_INTERACTION_RANGE,
                defaults::ENTITY_INTERACTION_RANGE,
            ),
            (attribute::OXYGEN_BONUS, defaults::OXYGEN_BONUS),
            (attribute::BURNING_TIME, defaults::BURNING_TIME),
            (
                attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
                defaults::EXPLOSION_KNOCKBACK_RESISTANCE,
            ),
            (
                attribute::WATER_MOVEMENT_EFFICIENCY,
                defaults::WATER_MOVEMENT_EFFICIENCY,
            ),
            (
                attribute::MOVEMENT_EFFICIENCY,
                defaults::MOVEMENT_EFFICIENCY,
            ),
            (attribute::ATTACK_KNOCKBACK, defaults::ATTACK_KNOCKBACK),
            (attribute::CAMERA_DISTANCE, defaults::CAMERA_DISTANCE),
            (
                attribute::WAYPOINT_TRANSMIT_RANGE,
                defaults::WAYPOINT_TRANSMIT_RANGE,
            ),
            (attribute::BOUNCINESS, defaults::BOUNCINESS),
            (attribute::AIR_DRAG_MODIFIER, defaults::AIR_DRAG_MODIFIER),
            (attribute::FRICTION_MODIFIER, defaults::FRICTION_MODIFIER),
            (attribute::NAME_TAG_DISTANCE, defaults::NAME_TAG_DISTANCE),
            (
                attribute::BELOW_NAME_DISTANCE,
                defaults::BELOW_NAME_DISTANCE,
            ),
            (attribute::FOLLOW_RANGE, 16.0),
            (attribute::TEMPT_RANGE, 10.0),
        ];
        Self {
            entries: living
                .into_iter()
                .map(|(id, base)| Attribute::new(id, base))
                .collect(),
        }
    }

    /// `Zombie.createAttributes`：`Monster.createMonsterAttributes` 的全部
    /// （`createMobAttributes` 加 `ATTACK_DAMAGE`），再覆盖跟随距离 35、
    /// 速度 0.23、攻击力 3、护甲 2，以及增援概率属性。
    ///
    /// `Zombie.createAttributes`: `Monster.createMonsterAttributes` plus
    /// follow range 35, speed 0.23, attack damage 3, armor 2 and the
    /// reinforcement-chance attribute.
    pub fn zombie() -> Self {
        use mc_bamboo_data::registries::attribute_defaults as defaults;
        let mut table = Self::cow();
        table.set_base(attribute::MAX_HEALTH, defaults::MAX_HEALTH);
        table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.23_f32));
        table.set_base(attribute::FOLLOW_RANGE, 35.0);
        table.set_base(attribute::TEMPT_RANGE, defaults::TEMPT_RANGE);
        table.set_base(attribute::ARMOR, 2.0);
        if let Some(entry) = table
            .entries
            .iter_mut()
            .find(|entry| entry.id == attribute::ATTACK_DAMAGE)
        {
            entry.base = 3.0;
        } else {
            table
                .entries
                .push(Attribute::new(attribute::ATTACK_DAMAGE, 3.0));
        }
        table.entries.push(Attribute::new(
            attribute::SPAWN_REINFORCEMENTS,
            defaults::SPAWN_REINFORCEMENTS,
        ));
        table
    }

    /// AbstractSkeleton.createAttributes：Monster 属性加 0.25 移速。
    ///
    /// AbstractSkeleton.createAttributes: Monster attributes with 0.25 movement speed.
    pub fn skeleton() -> Self {
        use mc_bamboo_data::registries::attribute_defaults as defaults;
        let mut table = Self::cow();
        table.set_base(attribute::MAX_HEALTH, defaults::MAX_HEALTH);
        table.set_base(attribute::MOVEMENT_SPEED, 0.25);
        table.set_base(attribute::TEMPT_RANGE, defaults::TEMPT_RANGE);
        if let Some(entry) = table
            .entries
            .iter_mut()
            .find(|entry| entry.id == attribute::ATTACK_DAMAGE)
        {
            entry.base = defaults::ATTACK_DAMAGE;
        } else {
            table.entries.push(Attribute::new(
                attribute::ATTACK_DAMAGE,
                defaults::ATTACK_DAMAGE,
            ));
        }
        table
    }

    /// 对拍的改坏确认用：直接改一个属性的基础值。
    pub fn set_base_for_oracle(&mut self, id: i32, base: f64) {
        self.set_base(id, base);
    }

    /// `Mob.createMobAttributes()`：生物的缺省属性表。没有 `Animal` 加的 `TEMPT_RANGE`，
    /// 血量和移动速度是注册表缺省值。
    ///
    /// `Mob.createMobAttributes()`: the plain mob defaults, without the `TEMPT_RANGE` that
    /// `Animal` adds and with registry-default health and movement speed.
    pub(super) fn mob() -> Self {
        use mc_bamboo_data::registries::attribute_defaults as defaults;
        let mut attributes = Self::cow();
        attributes
            .entries
            .retain(|entry| entry.id != attribute::TEMPT_RANGE);
        attributes.set_base(attribute::MAX_HEALTH, defaults::MAX_HEALTH);
        attributes.set_base(attribute::MOVEMENT_SPEED, defaults::MOVEMENT_SPEED);
        attributes
    }

    /// `Monster.createMonsterAttributes()`：`Mob` 的缺省属性加攻击力（注册表缺省值 2）。
    ///
    /// `Monster.createMonsterAttributes()`: the mob defaults plus attack damage at its
    /// registry default of 2.
    pub(super) fn monster() -> Self {
        use mc_bamboo_data::registries::attribute_defaults as defaults;
        let mut attributes = Self::mob();
        if attributes.get(attribute::ATTACK_DAMAGE).is_none() {
            attributes.entries.push(Attribute::new(
                attribute::ATTACK_DAMAGE,
                defaults::ATTACK_DAMAGE,
            ));
        }
        attributes.set_base(attribute::ATTACK_DAMAGE, defaults::ATTACK_DAMAGE);
        attributes
    }

    pub(super) fn set_base(&mut self, id: i32, base: f64) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.base = base;
        }
    }

    pub fn get(&self, id: i32) -> Option<&Attribute> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// `getAttribute(holder)`：官方在这里懒创建实例。
    pub fn instance(&mut self, id: i32) -> Option<&mut Attribute> {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id)?;
        entry.instantiated = true;
        Some(entry)
    }

    /// `getAttributeValue(holder)`。不创建实例。表里没有的属性官方会抛异常，这里给 0。
    pub fn value(&self, id: i32) -> f64 {
        self.get(id).map_or(0.0, Attribute::value)
    }

    /// `getAttributeBaseValue(holder)`。
    pub fn base(&self, id: i32) -> f64 {
        self.get(id).map_or(0.0, |entry| entry.base)
    }
}
