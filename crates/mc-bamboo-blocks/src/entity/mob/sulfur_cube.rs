//! 硫磺方块怪：官方 `AbstractCubeMob` → `SulfurCube`。
//!
//! 尺寸、跳跃移动和分裂沿用 [`super::slime`] 的 `AbstractCubeMob` 建模；这里放
//! 原型表、吞下的方块、导火索和「装了方块就不再跳/不再转」的控制差异。
//!
//! Sulfur cubes: vanilla `AbstractCubeMob` → `SulfurCube`. Dimensions, hopping and
//! splitting reuse the slime-side `AbstractCubeMob` model; this module holds the
//! archetype table, the swallowed block, the fuse and the controls that stop
//! jumping/turning once a block is equipped.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{entity_types, registries::attribute, tags::item_tags};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Attributes, Mob, MobType,
    attributes::Attribute,
    equine::Voice,
    goals::{Goal, GoalSelector, WrappedGoal},
    navigation::Navigation,
    slime,
};
use crate::entity::Entity;

/// `SulfurCube.MIN_SIZE` / `MAX_SIZE` / `SPLIT_COUNT` / `PICKUP_TIMER_DURATION`。
pub const MIN_SIZE: i32 = 1;
pub const MAX_SIZE: i32 = 2;
pub const SPLIT_COUNT: i32 = 2;
pub const PICKUP_TIMER_DURATION: i32 = 100;

/// `SulfurCubeArchetypes` 在 `bootstrap` 里注册的 12 个 key，按注册表名字排序。
/// 官方是 `Registries.SULFUR_CUBE_ARCHETYPE` 的一张同步注册表；数据包的覆盖不在
/// 服务端建模，所以这里按字节码写死它的缺省内容。
///
/// The twelve keys registered by `SulfurCubeArchetypes.bootstrap`, sorted by registry
/// name. Vanilla keeps them in the synced `sulfur_cube_archetype` registry; datapack
/// overrides are not modelled, so the built-in contents are hardcoded here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Archetype {
    Bouncy,
    Explosive,
    FastFlat,
    FastSliding,
    HighResistance,
    Hot,
    Light,
    Regular,
    SlowBouncy,
    SlowFlat,
    SlowSliding,
    Sticky,
}

impl Archetype {
    /// 注册表名字（不带命名空间）。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Bouncy => "bouncy",
            Self::Explosive => "explosive",
            Self::FastFlat => "fast_flat",
            Self::FastSliding => "fast_sliding",
            Self::HighResistance => "high_resistance",
            Self::Hot => "hot",
            Self::Light => "light",
            Self::Regular => "regular",
            Self::SlowBouncy => "slow_bouncy",
            Self::SlowFlat => "slow_flat",
            Self::SlowSliding => "slow_sliding",
            Self::Sticky => "sticky",
        }
    }

    /// `register` 传进去的 `items` 标签（`ItemTags.SULFUR_CUBE_ARCHETYPE_*`）。
    ///
    /// The `items` tag passed to `register`.
    pub const fn item_tag(self) -> mc_bamboo_data::ItemTag {
        use mc_bamboo_data::tags::item_tags as tags;
        match self {
            Self::Bouncy => tags::SULFUR_CUBE_ARCHETYPE_BOUNCY,
            Self::Explosive => tags::SULFUR_CUBE_ARCHETYPE_EXPLOSIVE,
            Self::FastFlat => tags::SULFUR_CUBE_ARCHETYPE_FAST_FLAT,
            Self::FastSliding => tags::SULFUR_CUBE_ARCHETYPE_FAST_SLIDING,
            Self::HighResistance => tags::SULFUR_CUBE_ARCHETYPE_HIGH_RESISTANCE,
            Self::Hot => tags::SULFUR_CUBE_ARCHETYPE_HOT,
            Self::Light => tags::SULFUR_CUBE_ARCHETYPE_LIGHT,
            Self::Regular => tags::SULFUR_CUBE_ARCHETYPE_REGULAR,
            Self::SlowBouncy => tags::SULFUR_CUBE_ARCHETYPE_SLOW_BOUNCY,
            Self::SlowFlat => tags::SULFUR_CUBE_ARCHETYPE_SLOW_FLAT,
            Self::SlowSliding => tags::SULFUR_CUBE_ARCHETYPE_SLOW_SLIDING,
            Self::Sticky => tags::SULFUR_CUBE_ARCHETYPE_STICKY,
        }
    }
}

pub const ARCHETYPES: [Archetype; 12] = [
    Archetype::Bouncy,
    Archetype::Explosive,
    Archetype::FastFlat,
    Archetype::FastSliding,
    Archetype::HighResistance,
    Archetype::Hot,
    Archetype::Light,
    Archetype::Regular,
    Archetype::SlowBouncy,
    Archetype::SlowFlat,
    Archetype::SlowSliding,
    Archetype::Sticky,
];

/// `archetype(kr, bounciness, friction, airDrag)` 的四个数。`kr` 以「减去的
/// 击退抗性」形式存：官方 `add(KNOCKBACK_RESISTANCE, -kr)`。
///
/// The four numbers of `archetype(kr, bounciness, friction, airDrag)`. `kr` is
/// stored as the subtracted knockback resistance: vanilla adds `-kr`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArchetypeFacts {
    pub knockback_resistance: f64,
    pub bounciness: f64,
    pub friction: f64,
    pub air_drag: f64,
    pub buoyant: bool,
    /// `ExplosionData(power, causesFire, fuse)`；只有 explosive 原型有。
    pub explosion: Option<ExplosionData>,
    /// `ContactDamage`；只有 hot 原型有。
    pub contact_damage: Option<ContactDamage>,
    pub knockback_modifiers: KnockbackModifiers,
    pub sound_settings: SoundSettings,
}

/// `SulfurCubeArchetype.ExplosionData`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExplosionData {
    pub power: f32,
    pub causes_fire: bool,
    pub fuse: i32,
}

/// `SulfurCubeArchetype.ContactDamage`。HOT 是 1 点 `sulfur_cube_hot`，
/// `attributeToSource = false`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactDamage {
    pub amount: f32,
    pub attribute_to_source: bool,
}

/// `SulfurCubeArchetype.KnockbackModifiers`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnockbackModifiers {
    pub horizontal_power: f32,
    pub vertical_power: f32,
}

/// `SulfurCubeArchetype.SoundSettings`。声音事件名按原型拼。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundSettings {
    pub hit_sound: &'static str,
    pub push_sound: &'static str,
    pub push_sound_impulse_threshold: f32,
    pub push_sound_cooldown: f32,
}

pub const DEFAULT_KNOCKBACK_MODIFIERS: KnockbackModifiers = KnockbackModifiers {
    horizontal_power: 0.33,
    vertical_power: 0.06,
};

pub const DEFAULT_SOUND_SETTINGS: SoundSettings = SoundSettings {
    hit_sound: "minecraft:entity.sulfur_cube.regular.hit",
    push_sound: "minecraft:entity.sulfur_cube.regular.push",
    push_sound_impulse_threshold: 0.2,
    push_sound_cooldown: 0.5,
};

/// `SulfurCubeArchetypes.bootstrap` 里每个 `register` 调用的全部数值。
/// `soundSettings` 的两个事件名按 `entity.sulfur_cube.<key>.hit/push` 拼，
/// 12 个原型无一例外。
///
/// Every `register` call of `SulfurCubeArchetypes.bootstrap`. The two sound names
/// follow `entity.sulfur_cube.<key>.hit/push` for all twelve archetypes.
pub const fn facts(kind: Archetype) -> ArchetypeFacts {
    use Archetype::*;
    let (kr, bouncy, friction, air_drag, buoyant) = match kind {
        Regular => (1.0, 0.5, 0.3, 0.1, true),
        Bouncy => (2.0, 0.9, 0.3, 0.01, true),
        SlowBouncy => (-0.4, 0.6, 0.3, 0.05, false),
        SlowFlat => (-0.5, 0.4, 0.4, 0.1, false),
        FastFlat => (1.0, 0.5, 0.2, 0.01, false),
        Light => (1.0, 1.0, 0.3, 1.8, true),
        FastSliding => (-0.5, 0.1, 0.05, 0.01, false),
        SlowSliding => (-0.8, 0.1, 0.05, 0.01, false),
        Sticky => (2.0, 0.0, 2.0, 0.01, false),
        HighResistance => (-0.7, 0.2, 1.0, 0.01, false),
        Explosive => (1.0, 0.5, 0.3, 0.3, true),
        Hot => (1.0, 0.5, 0.3, 0.1, true),
    };
    let (horizontal, vertical, impulse, cooldown) = match kind {
        Regular => (0.4125, 0.09, 0.2, 0.5),
        Bouncy => (0.4125, 0.105, 0.3, 0.7),
        SlowBouncy => (0.4125, 0.24, 0.05, 0.5),
        SlowFlat => (0.4125, 0.105, 0.03, 0.9),
        FastFlat => (0.9125, 0.09, 0.03, 0.9),
        Light => (0.4125, 0.18, 0.2, 0.7),
        FastSliding => (0.6625, 0.09, 0.05, 1.0),
        SlowSliding => (0.4125, 0.09, 0.02, 1.0),
        Sticky => (0.4125, 0.09, 0.05, 0.5),
        HighResistance => (0.4125, 0.09, 0.03, 0.7),
        Explosive => (0.4125, 0.09, 0.1, 0.7),
        Hot => (0.4125, 0.09, 0.2, 0.7),
    };
    let (hit, push) = sound_names(kind);
    ArchetypeFacts {
        knockback_resistance: kr,
        bounciness: bouncy,
        friction,
        air_drag,
        buoyant,
        explosion: match kind {
            Explosive => Some(ExplosionData {
                power: 3.0,
                causes_fire: false,
                fuse: 120,
            }),
            _ => None,
        },
        contact_damage: match kind {
            Hot => Some(ContactDamage {
                amount: 1.0,
                attribute_to_source: false,
            }),
            _ => None,
        },
        knockback_modifiers: KnockbackModifiers {
            horizontal_power: horizontal,
            vertical_power: vertical,
        },
        sound_settings: SoundSettings {
            hit_sound: hit,
            push_sound: push,
            push_sound_impulse_threshold: impulse,
            push_sound_cooldown: cooldown,
        },
    }
}

const fn sound_names(kind: Archetype) -> (&'static str, &'static str) {
    use Archetype::*;
    match kind {
        Regular => (
            "minecraft:entity.sulfur_cube.regular.hit",
            "minecraft:entity.sulfur_cube.regular.push",
        ),
        Bouncy => (
            "minecraft:entity.sulfur_cube.bouncy.hit",
            "minecraft:entity.sulfur_cube.bouncy.push",
        ),
        SlowBouncy => (
            "minecraft:entity.sulfur_cube.slow_bouncy.hit",
            "minecraft:entity.sulfur_cube.slow_bouncy.push",
        ),
        SlowFlat => (
            "minecraft:entity.sulfur_cube.slow_flat.hit",
            "minecraft:entity.sulfur_cube.slow_flat.push",
        ),
        FastFlat => (
            "minecraft:entity.sulfur_cube.fast_flat.hit",
            "minecraft:entity.sulfur_cube.fast_flat.push",
        ),
        Light => (
            "minecraft:entity.sulfur_cube.light.hit",
            "minecraft:entity.sulfur_cube.light.push",
        ),
        FastSliding => (
            "minecraft:entity.sulfur_cube.fast_sliding.hit",
            "minecraft:entity.sulfur_cube.fast_sliding.push",
        ),
        SlowSliding => (
            "minecraft:entity.sulfur_cube.slow_sliding.hit",
            "minecraft:entity.sulfur_cube.slow_sliding.push",
        ),
        Sticky => (
            "minecraft:entity.sulfur_cube.sticky.hit",
            "minecraft:entity.sulfur_cube.sticky.push",
        ),
        HighResistance => (
            "minecraft:entity.sulfur_cube.high_resistance.hit",
            "minecraft:entity.sulfur_cube.high_resistance.push",
        ),
        Explosive => (
            "minecraft:entity.sulfur_cube.explosive.hit",
            "minecraft:entity.sulfur_cube.explosive.push",
        ),
        Hot => (
            "minecraft:entity.sulfur_cube.hot.hit",
            "minecraft:entity.sulfur_cube.hot.push",
        ),
    }
}

/// `matchingArchetypes`：按注册表顺序挑 `items` 标签包含这个物品的原型。
/// 官方的 tag 是 HolderSet，这里直接查生成器导出的物品标签表。
///
/// `matchingArchetypes`: archetypes whose `items` holder set contains this item,
/// in registry order. The holder set comes from the generated item tag table.
pub fn matching_archetypes(stack: &ItemStack) -> Vec<Archetype> {
    if stack.is_empty() {
        return Vec::new();
    }
    ARCHETYPES
        .iter()
        .copied()
        .filter(|kind| {
            mc_bamboo_data::item_tag(kind.item_tag().name)
                .is_some_and(|ids| ids.contains(&stack.item))
        })
        .collect()
}

/// `SulfurCube` 的自有状态。吞下的方块在 BODY 装备槽里，这里只存原型派生值、
/// 导火索和几个计时器；都是相对量或原始物品，随实体行搬岛。
///
/// `SulfurCube`'s own state. The swallowed block lives in the BODY equipment slot;
/// this holds the derived archetype values, fuse and timers.
#[derive(Debug, Clone, PartialEq)]
pub struct SulfurCubeState {
    /// BODY 槽里的物品（一股脑存进装备表，这里留一份方便读）。
    pub body: ItemStack,
    /// `pickupTimer`：剪掉方块后 100 刻不再捡。
    pub pickup_timer: i32,
    /// `pushSoundCooldown`。
    pub push_sound_cooldown: i32,
    /// `fuse`，-1 表示没点燃。
    pub fuse: i32,
    /// `MAX_FUSE` 同步元数据。
    pub max_fuse: i32,
    /// `FROM_BUCKET` 同步元数据。
    pub from_bucket: bool,
    /// `floatsInLiquids`：匹配原型里任何一个 `buoyant` 就为真。
    pub floats_in_liquids: bool,
    pub explosion: Option<ExplosionData>,
    pub contact_damages: Vec<ContactDamage>,
    pub knockback_modifiers: KnockbackModifiers,
    pub sound_settings: SoundSettings,
    /// 当前生效的原型（供存档/对拍读）。
    pub archetypes: Vec<Archetype>,
}

impl Default for SulfurCubeState {
    fn default() -> Self {
        Self {
            body: ItemStack::empty(),
            pickup_timer: 0,
            push_sound_cooldown: 0,
            fuse: -1,
            max_fuse: -1,
            from_bucket: false,
            floats_in_liquids: false,
            explosion: None,
            contact_damages: Vec::new(),
            knockback_modifiers: DEFAULT_KNOCKBACK_MODIFIERS,
            sound_settings: DEFAULT_SOUND_SETTINGS,
            archetypes: Vec::new(),
        }
    }
}

/// `SulfurCube.createSulfurCubeAttributes()`：`Mob.createMobAttributes()` 加
/// `TEMPT_RANGE = 8.0`。
///
/// `SulfurCube.createSulfurCubeAttributes()`: `Mob.createMobAttributes()` plus
/// `TEMPT_RANGE = 8.0`.
pub fn attributes() -> Attributes {
    use mc_bamboo_data::registries::attribute_defaults as defaults;
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MOVEMENT_SPEED, defaults::MOVEMENT_SPEED);
    table.set_base(attribute::TEMPT_RANGE, 8.0);
    table
}

/// `AbstractCubeMob.getDefaultDimensions`：官方 `EntityTypes.SULFUR_CUBE`
/// 的 `sized(0.49, 0.49).eyeHeight(0.175)` 乘大小。
///
/// `AbstractCubeMob.getDefaultDimensions`: the `sized(0.49, 0.49).eyeHeight(0.175)`
/// entity type scaled by the size.
pub const fn dimensions(size: i32) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::SulfurCube.entity_type() as usize];
    let scale = size as f32;
    (facts.width * scale, facts.height * scale, 0.175 * scale)
}

/// `SulfurCube.setCubeMobHealth`：`MAX_HEALTH = 4 * size`。
pub fn set_cube_mob_health(mob: &mut Mob, size: i32) {
    mob.attributes
        .set_base(attribute::MAX_HEALTH, f64::from(size * 4));
}

/// `AbstractCubeMob.setSize` 在硫磺方块怪上的覆写：血量是 `4 * size`，
/// 移速和攻击力沿用父类公式。`SulfurCube.setSpawnSize` 用幼年 1、成年 2。
///
/// The sulfur-cube override of `AbstractCubeMob.setSize`: health is `4 * size`
/// while speed and attack damage keep the parent formulas.
pub fn set_size(me: &mut Entity, mob: &mut Mob, size: i32, update_health: bool) {
    let actual = size.clamp(MIN_SIZE, MAX_SIZE);
    let changed = mob.cube.size != actual;
    mob.cube.size = actual;
    let (width, height, eye) = dimensions(actual);
    me.dimensions = (width, height);
    mob.eye_height = eye;
    if changed {
        me.y_rot = mob.y_head_rot;
        mob.y_body_rot = mob.y_head_rot;
        if me.in_water {
            let _ = me.random.next_int_bounded(20);
        }
    }
    // 官方 `AbstractCubeMob.setSize` 走 `getAttribute(...)`，会懒创建这两个实例，
    // 存档时因此多写两条 `attributes`。
    mob.attributes.instance(attribute::MAX_HEALTH);
    mob.attributes.instance(attribute::MOVEMENT_SPEED);
    set_cube_mob_health(mob, actual);
    mob.attributes.set_base(
        attribute::MOVEMENT_SPEED,
        f64::from(0.2_f32 + 0.1_f32 * actual as f32),
    );
    if update_health {
        mob.health = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    }
    mob.attributes
        .set_base(attribute::ATTACK_DAMAGE, f64::from(actual));
}

/// 属性表里要挂原型修饰的 5 个属性。官方 `Mob.createMobAttributes` 没实例化
/// 其中的 `BOUNCINESS` / `FRICTION_MODIFIER` / `AIR_DRAG_MODIFIER` 等，
/// `getAttribute` 会懒创建；这里按需要补进去。
fn ensure_archetype_attributes(table: &mut Attributes) {
    for id in [
        attribute::KNOCKBACK_RESISTANCE,
        attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
        attribute::BOUNCINESS,
        attribute::FRICTION_MODIFIER,
        attribute::AIR_DRAG_MODIFIER,
    ] {
        if table.get(id).is_none() {
            let base = match id {
                attribute::AIR_DRAG_MODIFIER | attribute::FRICTION_MODIFIER => 1.0,
                _ => 0.0,
            };
            table.entries.push(Attribute::new(id, base));
        }
    }
}

/// `AttributeEntry.add` / `multiply` 生成的临时修饰符 ID。官方的 ID 是
/// 原型注册键（`minecraft:<key>`），所有 5 个修饰共用它；这里按属性分开存，
/// 移除时按同一组 ID 精确匹配。
fn modifier_id(kind: Archetype, attr: i32) -> String {
    format!("minecraft:{}/{attr}", kind.key())
}

/// `collectEquipmentChanges` 的属性部分：先移除旧原型的修饰，再逐个加上新的。
/// `multiply` 在官方里是 `ADD_MULTIPLIED_TOTAL`（`AttributeModifier.Operation` 的
/// `add / addMultipliedBase / multiply` 分别对应前三种）。
pub fn apply_modifiers(mob: &mut Mob, old: &[Archetype], new: &[Archetype]) {
    ensure_archetype_attributes(&mut mob.attributes);
    for kind in old {
        let facts = facts(*kind);
        let mods = [
            (attribute::KNOCKBACK_RESISTANCE, -facts.knockback_resistance),
            (
                attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
                -facts.knockback_resistance,
            ),
            (attribute::BOUNCINESS, facts.bounciness),
            (attribute::FRICTION_MODIFIER, facts.friction),
            (attribute::AIR_DRAG_MODIFIER, facts.air_drag),
        ];
        for (attr, _) in mods {
            if let Some(instance) = mob.attributes.instance(attr) {
                instance.remove_modifier(&modifier_id(*kind, attr));
            }
        }
    }
    for kind in new {
        let facts = facts(*kind);
        let mods = [
            (
                attribute::KNOCKBACK_RESISTANCE,
                -facts.knockback_resistance,
                super::attributes::Operation::AddValue,
            ),
            (
                attribute::EXPLOSION_KNOCKBACK_RESISTANCE,
                -facts.knockback_resistance,
                super::attributes::Operation::AddValue,
            ),
            (
                attribute::BOUNCINESS,
                facts.bounciness,
                super::attributes::Operation::AddValue,
            ),
            (
                attribute::FRICTION_MODIFIER,
                facts.friction - 1.0,
                super::attributes::Operation::AddMultipliedTotal,
            ),
            (
                attribute::AIR_DRAG_MODIFIER,
                facts.air_drag - 1.0,
                super::attributes::Operation::AddMultipliedTotal,
            ),
        ];
        for (attr, amount, operation) in mods {
            if let Some(instance) = mob.attributes.instance(attr) {
                let id = modifier_id(*kind, attr);
                instance.remove_modifier(&id);
                instance.modifiers.push(super::attributes::Modifier {
                    id,
                    amount,
                    operation,
                    permanent: false,
                });
                instance.touch_sync();
            }
        }
    }
}

/// `AbstractCubeMob.registerGoals` 加 `SulfurCube.addBehaviourGoals`。
/// `SulfurCube` 覆写了 `canDealDamage()` 为假、`addTargetingGoals()` 为空，
/// 所以没有 `CubeAttack`、也没有目标选择器；`Tempt` 和 `SearchForItems`
/// 是新目标，其余三个沿用 `AbstractCubeMob`。
///
/// `AbstractCubeMob.registerGoals` plus `SulfurCube.addBehaviourGoals`. Sulfur cubes
/// override `canDealDamage()` to false and leave `addTargetingGoals()` empty, so
/// there is no `CubeAttack` and no target selector; Tempt and SearchForItems are new.
pub fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    let mut selector = GoalSelector::default();
    selector.goals = vec![
        WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::CubeFloat,
        },
        WrappedGoal {
            priority: 2,
            running: false,
            goal: Goal::SulfurCubeTempt { player: None },
        },
        WrappedGoal {
            priority: 3,
            running: false,
            goal: Goal::SulfurCubeSearchForItems { target: None },
        },
        WrappedGoal {
            priority: 4,
            running: false,
            goal: Goal::CubeRandomDirection {
                chosen_degrees: 0.0,
                next_randomize_time: 0,
            },
        },
        WrappedGoal {
            priority: 5,
            running: false,
            goal: Goal::CubeKeepJumping,
        },
    ];
    selector
}

/// `SulfurCubeTemptGoal.stopNavigation`：`CubeMobMoveControl.setWantedMovement(0)`。
pub fn stop_tempt(mob: &mut Mob) {
    slime::set_wanted_movement(mob, 0.0);
}

/// `SulfurCubeLookControl.tick`：装了方块时 `yRot -= wrapDegrees90(yRot)`，
/// `yHeadRot = yRot`；没装时走通用 `LookControl`。
///
/// `SulfurCubeLookControl.tick`: with a body item, snap the yaw to the nearest
/// right angle and copy it to the head; otherwise use the common look control.
pub fn look_control_tick(me: &mut Entity, mob: &mut Mob) {
    if has_body_item(mob) {
        let wrapped = wrap_degrees_90(me.y_rot);
        me.y_rot -= wrapped;
        mob.y_head_rot = me.y_rot;
    } else {
        super::living::look_control_tick_public(me, mob);
    }
}

/// `Mth.wrapDegrees90`：`value % 90` 折回 `[-45, 45)`。
///
/// `Mth.wrapDegrees90`: `value % 90` folded into `[-45, 45)`.
fn wrap_degrees_90(value: f32) -> f32 {
    let mut wrapped = value % 90.0;
    if wrapped >= 45.0 {
        wrapped -= 90.0;
    }
    if wrapped < -45.0 {
        wrapped += 90.0;
    }
    wrapped
}

/// `SulfurCubeMobMoveControl.tick`：装了方块时什么都不做。
pub fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if !has_body_item(mob) {
        slime::move_control_tick(me, mob);
    }
}

/// `SulfurCube.hasBodyItem()`：BODY 槽非空。
pub fn has_body_item(mob: &Mob) -> bool {
    !mob.equipment.get(EquipmentSlot::Body).is_empty()
}

/// `has_body_item` 的 const 版本，只给声音分派用。
const fn has_body_item_const(mob: &Mob) -> bool {
    !mob.sulfur_cube.body.is_empty()
}

/// `SulfurCube.isSwallowableItem()`：`#minecraft:sulfur_cube_swallowable`。
pub fn is_swallowable(stack: &ItemStack) -> bool {
    !stack.is_empty()
        && mc_bamboo_data::item_tag(item_tags::SULFUR_CUBE_SWALLOWABLE.name)
            .is_some_and(|ids| ids.contains(&stack.item))
}

/// `collectEquipmentChanges`：BODY 物品变化时重算原型派生值。
/// 官方先移除旧的属性修饰、清空效果，再套上所有新匹配原型的修饰（后匹配者
/// 覆盖单值字段）。这里只更新派生值：属性修饰涉及注册表 ID，由 [`apply_modifiers`]
/// 单独处理。
///
/// `collectEquipmentChanges`: recompute the derived archetype values when the body
/// item changes. Vanilla removes the old modifiers, clears the effects, then adds
/// every newly matching archetype's modifiers (later matches overwrite singles).
pub fn collect_equipment_changes(mob: &mut Mob, new_body: ItemStack) {
    let old_archetypes = std::mem::take(&mut mob.sulfur_cube.archetypes);
    let state = &mut mob.sulfur_cube;
    state.body = new_body.clone();
    mob.equipment.set(EquipmentSlot::Body, new_body.clone());
    let matching = matching_archetypes(&new_body);
    // `collectEquipmentChanges`：装了方块就 `removeAllGoals(goal -> true)` 并把
    // speed 清零；卸下时 `registerGoals()` 重建目标表。
    if !new_body.is_empty() {
        mob.goals.clear_running();
        mob.speed = 0.0;
    } else {
        let mut navigation = mob.navigation.clone();
        mob.goals = goals(&mut navigation);
    }
    state.floats_in_liquids = false;
    state.explosion = None;
    state.contact_damages.clear();
    state.knockback_modifiers = DEFAULT_KNOCKBACK_MODIFIERS;
    state.sound_settings = DEFAULT_SOUND_SETTINGS;
    for kind in &matching {
        let facts = facts(*kind);
        if facts.buoyant {
            state.floats_in_liquids = true;
        }
        if let Some(explosion) = facts.explosion {
            state.explosion = Some(explosion);
        }
        if let Some(damage) = facts.contact_damage {
            state.contact_damages.push(damage);
        }
        state.knockback_modifiers = facts.knockback_modifiers;
        state.sound_settings = facts.sound_settings;
    }
    apply_modifiers(mob, &old_archetypes, &matching);
    mob.sulfur_cube.archetypes = matching;
}

/// `equipItem`：成年、且还没吞过同种物品时把 1 个放进 BODY。
/// 官方在替换时先把旧物品以 `spawnAtLocation` 吐出来；这里只记装备变化，
/// 掉落由游戏层的 [`collect_equipment_changes`] 调用方处理。
///
/// `equipItem`: put one item in BODY when adult and not already carrying that
/// item. Vanilla spawns the old item when replacing; the caller handles the drop.
pub fn equip_item(mob: &mut Mob, stack: &ItemStack) -> bool {
    if mob.age < 0 {
        return false;
    }
    if has_body_item(mob) && mob.equipment.get(EquipmentSlot::Body).item == stack.item {
        return false;
    }
    let mut one = stack.clone();
    one.count = 1;
    collect_equipment_changes(mob, one);
    true
}

/// 四种叫声按官方类分派：`isTiny()` 用幼年音，落地声在装了方块时换成 bounce。
/// 硫磺方块怪没有环境音，这里 `Voice::Ambient` 只作兜底。
///
/// The four sounds by vanilla class: tiny cubes use the small variants and the
/// landing sound becomes `bounce` with a body item. There is no ambient sound.
pub(super) fn voice(mob: &Mob, voice: Voice) -> &'static str {
    let tiny = slime::is_tiny(mob);
    match voice {
        Voice::Hurt => {
            if tiny {
                "minecraft:entity.small_sulfur_cube.hurt"
            } else {
                "minecraft:entity.sulfur_cube.hurt"
            }
        }
        Voice::Death => {
            if tiny {
                "minecraft:entity.small_sulfur_cube.death"
            } else {
                "minecraft:entity.sulfur_cube.death"
            }
        }
        _ => "minecraft:entity.sulfur_cube.squish",
    }
}

/// `getSquishSound` / `getJumpSound` / `getAbsorbSound` / `getEjectSound`。
pub const fn squish_sound(mob: &Mob) -> &'static str {
    if slime::is_tiny(mob) {
        "minecraft:entity.small_sulfur_cube.squish"
    } else if has_body_item_const(mob) {
        "minecraft:entity.sulfur_cube.bounce"
    } else {
        "minecraft:entity.sulfur_cube.squish"
    }
}

pub const fn jump_sound(mob: &Mob) -> &'static str {
    if slime::is_tiny(mob) {
        "minecraft:entity.small_sulfur_cube.jump"
    } else {
        "minecraft:entity.sulfur_cube.jump"
    }
}

pub const ABSORB_SOUND: &str = "minecraft:entity.sulfur_cube.absorb";
pub const EJECT_SOUND: &str = "minecraft:entity.sulfur_cube.eject";

/// `SulfurCube.isPrimed()`。
pub const fn is_primed(state: &SulfurCubeState) -> bool {
    state.fuse >= 0
}

/// `sulfur_cube_hot` 是 HOT 原型接触伤害用的伤害类型。
pub const SULFUR_CUBE_HOT: &str = "minecraft:sulfur_cube_hot";

/// `SulfurCubeContent` 组件的名字（`minecraft:sulfur_cube_content`）。
pub const CONTENT_COMPONENT: &str = "minecraft:sulfur_cube_content";

/// 推挤距离阈值，`SulfurCube.PUSH_DISTANCE_THRESHOLD`。
pub const PUSH_DISTANCE_THRESHOLD: f64 = 1.2999999523162842;

/// `SulfurCube.finalizeSpawn`：`AgeableMob` 那一层用 `AgeableMobGroupData(true)`
/// （组里第二只起按 0.05 概率成为幼崽），`Mob.finalizeSpawn` 给跟随距离加成
/// 和左撇子，最后 `setSpawnSize` 按幼年设 1、成年设 2 并回满血。
/// 与史莱姆不同，这里**不**抽 `nextInt(3)` 的大小档。
///
/// `SulfurCube.finalizeSpawn`: the `AgeableMob` layer uses
/// `AgeableMobGroupData(true)` (second and later group members are babies with 0.05
/// chance), `Mob.finalizeSpawn` adds the follow-range bonus and left-handedness,
/// then `setSpawnSize` picks 1 for babies / 2 for adults and refills health.
/// Unlike slimes there is **no** `nextInt(3)` size roll.
pub fn finalize_spawn(
    world: &mut crate::world::World<'_>,
    entity: &mut Entity,
    group: &mut Option<super::spawn::SpawnGroup>,
) {
    use super::spawn::{AgeableGroupData, SpawnGroup};
    let random = world.random();
    if !matches!(group, Some(SpawnGroup::Ageable(_))) {
        *group = Some(SpawnGroup::Ageable(AgeableGroupData::new(0.05)));
    }
    let baby = match group {
        Some(SpawnGroup::Ageable(data)) => data.roll_baby(random),
        _ => false,
    };
    entity.with_mob(|me, mob| {
        if baby {
            super::living::set_age(me, mob, super::spawn::BABY_START_AGE);
        }
        super::spawn::finalize_mob(random, mob);
        set_size(me, mob, if baby { 1 } else { 2 }, true);
    });
}

/// `AbstractCubeMob.tick` 在硫磺方块怪上的落地段：按 `sulfur_cube_goo`
/// 粒子的个数取两笔 `nextFloat`，再取两次定音高；声音按大小和 BODY 分派。
///
/// The sulfur cube's landing branch of `AbstractCubeMob.tick`.
pub fn after_tick(me: &mut Entity, mob: &mut Mob) {
    if me.on_ground && !mob.cube.was_on_ground {
        let count = me.dimensions.0 * 16.0;
        let mut index = 0;
        while (index as f32) < count {
            me.random.next_float();
            me.random.next_float();
            index += 1;
        }
        let first = me.random.next_float();
        let second = me.random.next_float();
        let pitch = ((first - second) * 0.2 + 1.0) * if slime::is_tiny(mob) { 1.4 } else { 0.8 };
        super::living::play_sound(me, mob, squish_sound(mob), slime::sound_volume(mob), pitch);
    }
    mob.cube.was_on_ground = me.on_ground;
}

/// `SulfurCubeContent`：把 BODY 槽里的物品包成一个模板组件，桶装时
/// `copyFrom` 到桶物品上。
///
/// `SulfurCubeContent`: wrap the BODY item into an `ItemStackTemplate` component.
pub fn content_of(mob: &Mob) -> Option<mc_bamboo_protocol::components::stack::ItemStackTemplate> {
    if mob.sulfur_cube.body.is_empty() {
        return None;
    }
    Some(mc_bamboo_protocol::components::stack::ItemStackTemplate {
        item: mob.sulfur_cube.body.item,
        count: 1,
        components: mob.sulfur_cube.body.patch(),
    })
}

/// `applyImplicitComponent(SULFUR_CUBE_CONTENT)`：把组件里的物品放回 BODY 槽。
pub fn set_content(
    mob: &mut Mob,
    template: &mc_bamboo_protocol::components::stack::ItemStackTemplate,
) {
    let mut stack = mc_bamboo_protocol::item::ItemStack::new(template.item, template.count.max(1));
    for (kind, entry) in template.components.entries() {
        if let mc_bamboo_protocol::components::PatchEntry::Set(value) = entry {
            let mut bytes = Vec::new();
            value.write(&mut bytes);
            stack.added.push((*kind, bytes));
        } else {
            stack.removed.push(*kind);
        }
    }
    collect_equipment_changes(mob, stack);
}

/// `SulfurCube.mobInteract`：幼年用 slime_ball 长大；点火 / 火焰弹点燃可爆炸原型；
/// 剪刀吐出吞下的方块并设 100 刻冷却；可吞物品进 BODY 并消费一个。
///
/// `SulfurCube.mobInteract`: a baby grows on slime balls; flint / fire charges prime
/// explosive archetypes; shears eject the swallowed block and set the 100-tick
/// pickup cooldown; swallowable items enter BODY and one is consumed.
pub fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &super::interact::MobInteractRequest,
) -> Option<super::interact::MobInteraction> {
    use crate::InteractionResult;
    use mc_bamboo_data::{items, tags::item_tags};
    use mc_bamboo_protocol::item::ItemStack;

    let held = &req.held;
    if mob.is_baby() {
        if crate::state::item_is_tagged(held.item, item_tags::SULFUR_CUBE_FOOD) {
            // `getSpeedUpSecondsWhenFeeding(-age)`：int 除法先算，再乘 0.1F 截成 int。
            let seconds = (((-mob.age) / 20) as f32 * 0.1_f32) as i32;
            super::interact::age_up(me, mob, seconds, true);
            crate::entity::mob::living::play_sound(
                me,
                mob,
                "minecraft:entity.small_sulfur_cube.eat",
                1.0,
                1.0,
            );
            return Some(super::interact::MobInteraction {
                result: InteractionResult::Success,
                shrink_held: !req.creative,
                ..super::interact::MobInteraction::pass()
            });
        }
        return Some(super::interact::MobInteraction::pass());
    }
    if is_primed(&mob.sulfur_cube) {
        return Some(super::interact::MobInteraction::pass());
    }
    if can_explode(mob) && (held.item == items::FLINT_AND_STEEL || held.item == items::FIRE_CHARGE)
    {
        prime(me, mob, false);
        return Some(super::interact::MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: held.item == items::FIRE_CHARGE && !req.creative,
            damage_held: held.item == items::FLINT_AND_STEEL,
            ..super::interact::MobInteraction::pass()
        });
    }
    if held.item == items::SHEARS && has_body_item(mob) {
        let old = mob.sulfur_cube.body.clone();
        mob.sulfur_cube.pickup_timer = PICKUP_TIMER_DURATION;
        collect_equipment_changes(mob, ItemStack::empty());
        crate::entity::mob::living::play_sound(me, mob, EJECT_SOUND, 1.0, 1.0);
        return Some(super::interact::MobInteraction {
            result: InteractionResult::Success,
            shear: true,
            damage_held: true,
            drop_stack: Some(old),
            ..super::interact::MobInteraction::pass()
        });
    }
    if is_swallowable(held) && equip_item(mob, held) {
        crate::entity::mob::living::play_sound(me, mob, ABSORB_SOUND, 1.0, 1.0);
        return Some(super::interact::MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            ..super::interact::MobInteraction::pass()
        });
    }
    None
}

/// `SulfurCube.tick`：先 `tickFuse()`，再 `primeWhenOnPoweredPosition()`，
/// 最后才是父类 tick（父类那一段已经在 `living::tick_mob` 里）。
/// `tickFuse` 把 fuse 从 120 减到 0，归零那一刻 `ExplosionInteraction.TNT`
/// 走 [`Command::ExplodeMob`] 落地，然后 `discard()`。
///
/// `SulfurCube.tick`: `tickFuse()` then `primeWhenOnPoweredPosition()` ahead of
/// the parent tick (which `living::tick_mob` runs). At fuse zero it emits a TNT
/// interaction explosion and discards itself.
pub fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn crate::entity::ctx::EntityCtx) {
    use crate::entity::{EntityKind, EntityPublic, ctx::Command};
    let state = &mut mob.sulfur_cube;
    if state.pickup_timer > 0 {
        state.pickup_timer -= 1;
    }
    if state.push_sound_cooldown > 0 {
        state.push_sound_cooldown -= 1;
    }
    if state.fuse > 0 {
        state.fuse -= 1;
    }
    let Some(data) = state.explosion else {
        return;
    };
    if state.fuse == 0 {
        let center = Vec3::new(me.position.x, me.y_at(0.0625), me.position.z);
        let radius = data.power;
        state.fuse = -1;
        mob.dead = true;
        let mut whole = me.clone();
        whole.kind = EntityKind::Mob(Box::new(mob.clone()));
        let source: Box<EntityPublic> = Box::new(whole.split().public);
        ctx.emit(
            Command::ExplodeMob {
                center,
                radius,
                source,
            },
            me,
        );
        me.discard();
        return;
    }
    // `primeWhenOnPoweredPosition`：能爆炸、活着、没点燃，脚下有红石信号就点燃。
    if can_explode(mob)
        && ctx
            .blocks()
            .best_own_or_neighbour_signal(me.block_position())
            != 0
    {
        prime(me, mob, false);
    }
}

/// `SulfurCube.canExplode()`：有爆炸数据、活着、还没点燃。
pub fn can_explode(mob: &Mob) -> bool {
    mob.sulfur_cube.explosion.is_some() && mob.health > 0.0 && !is_primed(&mob.sulfur_cube)
}

/// `SulfurCube.primeTime(shortFuse)`：gamerule `tntExplodes` 开着时把 fuse
/// 设成原型的 fuse（短引信按 `PrimedTnt.getRandomShortFuse` 抽），同步
/// `MAX_FUSE`、永久无敌并播 TNT_PRIMED。
///
/// `SulfurCube.primeTime(shortFuse)`: with `tntExplodes` on, set the fuse from
/// the archetype (short fuse uses `PrimedTnt.getRandomShortFuse`), sync MAX_FUSE,
/// make the cube permanently invulnerable and play TNT_PRIMED.
pub fn prime(me: &mut Entity, mob: &mut Mob, short_fuse: bool) {
    let Some(data) = mob.sulfur_cube.explosion else {
        return;
    };
    if mob.health <= 0.0 || is_primed(&mob.sulfur_cube) {
        return;
    }
    let fuse = data.fuse;
    let fuse = if short_fuse {
        // `PrimedTnt.getRandomShortFuse(fuse, random)`:
        // nextInt(max(1, fuse / 4)) + fuse / 8。
        let bound = (fuse / 4).max(1);
        me.random.next_int_bounded(bound) + fuse / 8
    } else {
        fuse
    };
    let state = &mut mob.sulfur_cube;
    state.fuse = fuse;
    state.max_fuse = fuse;
    mob.invulnerable = true;
    crate::entity::mob::living::play_sound(me, mob, "minecraft:entity.tnt.primed", 1.0, 1.0);
}

/// `SulfurCube.hurtServer` 的点燃分支：装了方块且能爆炸、未点燃时，
/// 火焰 / 着火箭 -> 普通引信，爆炸伤害 -> 短引信。
pub fn prime_on_hurt(me: &mut Entity, mob: &mut Mob, source: &super::DamageSource) -> bool {
    if !has_body_item(mob) || !can_explode(mob) {
        return false;
    }
    if source.kind.is_fire()
        || source.kind == super::DamageKind::Arrow && source.arrow_ignite_ticks > 0
    {
        prime(me, mob, false);
        return true;
    }
    if source.kind.is_explosion() {
        prime(me, mob, true);
        return true;
    }
    false
}

/// `SulfurCube.applyContactDamage` 的玩家那一半：HOT 原型在真被推到时
/// 造成 1 点 `sulfur_cube_hot` 伤害。`attributeToSource=false` 时给玩家
/// 归因到这只立方体，其它实体不归因。
///
/// The player half of `SulfurCube.applyContactDamage`: the HOT archetype deals
/// one point of `sulfur_cube_hot` damage on contact.
pub fn contact_player_touch(
    world: &mut crate::world::World<'_>,
    uid: crate::entity::EntityUid,
    player: crate::entity::EntityUid,
) -> bool {
    use crate::entity::mob::{DamageKind, DamageSource};
    let Some(entity) = world.entity(uid) else {
        return false;
    };
    let Some(mob) = entity.kind.mob() else {
        return false;
    };
    if mob.mob_type != MobType::SulfurCube || !has_body_item(mob) {
        return false;
    }
    let damages: Vec<f32> = mob
        .sulfur_cube
        .contact_damages
        .iter()
        .map(|entry| entry.amount)
        .collect();
    if damages.is_empty() {
        return false;
    }
    let attacker = entity.clone().split().public;
    let mut source = DamageSource::environment(DamageKind::SulfurCubeHot);
    source.attacker = Some(uid);
    source.direct = Some(uid);
    source.attacker_uuid = Some(mob.uuid);
    source.position = Some(entity.position);
    let mut hurt = false;
    for amount in damages {
        hurt |= world.hurt_player_from_mob(player, source, amount, Some(&attacker));
    }
    hurt
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items;

    #[test]
    fn archetype_table_matches_the_bootstrap_constants() {
        assert_eq!(ARCHETYPES.len(), 12);
        assert_eq!(facts(Archetype::Regular).knockback_resistance, 1.0);
        assert_eq!(facts(Archetype::Regular).bounciness, 0.5);
        assert_eq!(facts(Archetype::Regular).friction, 0.3);
        assert_eq!(facts(Archetype::Regular).air_drag, 0.1);
        assert!(facts(Archetype::Regular).buoyant);
        assert_eq!(facts(Archetype::Light).air_drag, 1.8);
        assert!(!facts(Archetype::SlowBouncy).buoyant);
        assert_eq!(facts(Archetype::Explosive).explosion.unwrap().fuse, 120);
        assert_eq!(facts(Archetype::Hot).contact_damage.unwrap().amount, 1.0);
        assert!(facts(Archetype::Bouncy).explosion.is_none());
        assert_eq!(DEFAULT_KNOCKBACK_MODIFIERS.horizontal_power, 0.33);
        assert_eq!(DEFAULT_SOUND_SETTINGS.push_sound_impulse_threshold, 0.2);
    }

    #[test]
    fn item_tags_pick_the_expected_archetypes() {
        assert_eq!(
            matching_archetypes(&ItemStack::new(items::DIRT, 1)),
            vec![Archetype::Regular]
        );
        assert_eq!(
            matching_archetypes(&ItemStack::new(items::TNT, 1)),
            vec![Archetype::Explosive]
        );
        assert_eq!(
            matching_archetypes(&ItemStack::new(items::MAGMA_BLOCK, 1)),
            vec![Archetype::Hot]
        );
        assert!(matching_archetypes(&ItemStack::empty()).is_empty());
        assert!(is_swallowable(&ItemStack::new(items::DIRT, 1)));
        assert!(!is_swallowable(&ItemStack::new(items::DIAMOND, 1)));
    }

    #[test]
    fn equipping_a_block_recomputes_derived_values() {
        let mut mob = Mob::new(MobType::SulfurCube);
        assert!(!has_body_item(&mob));
        assert!(equip_item(&mut mob, &ItemStack::new(items::TNT, 1)));
        assert!(has_body_item(&mob));
        assert_eq!(mob.sulfur_cube.archetypes, vec![Archetype::Explosive]);
        assert_eq!(mob.sulfur_cube.explosion.unwrap().power, 3.0);
        // 同种物品拒绝，换成别的物品会重生原型。
        assert!(!equip_item(&mut mob, &ItemStack::new(items::TNT, 1)));
        assert!(equip_item(&mut mob, &ItemStack::new(items::MAGMA_BLOCK, 1)));
        assert_eq!(mob.sulfur_cube.archetypes, vec![Archetype::Hot]);
        assert_eq!(mob.sulfur_cube.contact_damages.len(), 1);
        assert!(mob.sulfur_cube.explosion.is_none());
    }
}
