//! 鸡：属性、目标、叫声变种、下蛋计时和缓慢下落。
//!
//! Chickens: attributes, goals, sound variants, the egg timer and slow falling.

use mc_bamboo_data::{
    PathType, component_registries::registry_names, entity_types, registries::attribute,
    tags::item_tags,
};

use super::{
    Mob, MobType,
    attributes::Attributes,
    goals::{Goal, GoalSelector},
    path::Malus,
};
use crate::{
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
    random::RandomSource,
    state::item_is_tagged,
};

/// 成年和幼年的（宽，高，眼高）。成年眼高是 `EntityTypes.CHICKEN` 的 `eyeHeight(0.644F)`，
/// 幼年是 `Chicken.BABY_DIMENSIONS`。
///
/// Adult and baby (width, height, eye height): `eyeHeight(0.644F)` on the entity type and
/// `Chicken.BABY_DIMENSIONS`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.3, 0.4, 0.281_25)
    } else {
        let facts = &entity_types::FACTS[MobType::Chicken.entity_type() as usize];
        (facts.width, facts.height, 0.644)
    }
}

/// `Chicken.createAttributes`：生命 4，速度 0.25。
///
/// `Chicken.createAttributes`: 4 health and a 0.25 movement speed.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 4.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, 0.25);
    attributes
}

/// `Chicken.registerGoals`：和牛同样的布局，换恐慌 1.4、诱惑 1.0、跟随 1.1。
///
/// `Chicken.registerGoals`: the cow layout with panic 1.4, tempt 1.0 and follow 1.1.
pub(super) fn goals(navigation: &mut super::navigation::Navigation) -> GoalSelector {
    let mut selector = GoalSelector::cow(navigation);
    for wrapped in &mut selector.goals {
        match &mut wrapped.goal {
            Goal::Panic { speed, .. } => *speed = 1.4,
            Goal::Tempt { speed, .. } => *speed = 1.0,
            Goal::FollowParent { speed, .. } => *speed = 1.1,
            _ => {}
        }
    }
    selector
}

/// 构造函数里的 `setPathfindingMalus(PathType.WATER, 0.0F)`。
///
/// The constructor's `setPathfindingMalus(PathType.WATER, 0.0F)`.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::animal();
    malus.set(PathType::Water, 0.0);
    malus
}

/// `Chicken.isFood`。
pub(super) fn is_food(item: i32) -> bool {
    item_is_tagged(item, item_tags::CHICKEN_FOOD)
}

/// `Chicken.aiStep` 在 `super.aiStep()` 之后的服务端部分：空中下落时纵向速度乘 0.6；
/// 活着的成年鸡 `eggTime` 减到 0 时按 `CHICKEN_LAY` 礼物表下蛋、放下蛋声，再重抽计时。
///
/// The server part of `Chicken.aiStep` after `super.aiStep()`: vertical speed times 0.6
/// while falling; a living adult lays from the `CHICKEN_LAY` gift table when `eggTime` runs
/// out, plays the egg sound and rerolls the timer.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !me.on_ground && me.delta.y < 0.0 {
        me.delta.y *= 0.6;
    }
    if !super::living::is_alive(me, mob) || mob.is_baby() {
        return;
    }
    mob.egg_time -= 1;
    if mob.egg_time > 0 {
        return;
    }
    let request = super::EntityLootRequest::new("minecraft:gameplay/chicken_lay", me, mob, None);
    ctx.emit(Command::DropEntityLoot(request), me);
    let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
    super::living::play_sound(me, mob, "minecraft:entity.chicken.egg", 1.0, pitch);
    mob.egg_time = me.random.next_int_bounded(6000) + 6000;
}

/// 鸡的叫声变种，`minecraft:chicken_sound_variant` 注册表。
///
/// Chicken sound variants from the `minecraft:chicken_sound_variant` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundVariant {
    Classic,
    Picky,
}

impl SoundVariant {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.strip_prefix("minecraft:").unwrap_or(name) {
            "classic" => Some(Self::Classic),
            "picky" => Some(Self::Picky),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:classic",
            Self::Picky => "minecraft:picky",
        }
    }

    pub fn registry_id(self) -> i32 {
        registry_names::CHICKEN_SOUND_VARIANT
            .iter()
            .position(|name| *name == self.name())
            .expect("chicken sound variant in the registry") as i32
    }

    /// `ChickenSoundVariants.pickRandomSoundVariant`：注册表里 `nextInt(size)`。
    ///
    /// `ChickenSoundVariants.pickRandomSoundVariant`: `nextInt(size)` over the registry.
    pub(super) fn pick(random: &dyn RandomSource) -> Self {
        let names = registry_names::CHICKEN_SOUND_VARIANT;
        Self::from_name(names[random.next_int(names.len() as i32) as usize])
            .expect("known chicken sound variant")
    }

    /// 叫声、受伤、死亡、脚步；幼年统一用 `baby_chicken`。
    ///
    /// Ambient, hurt, death and step; babies share the `baby_chicken` set.
    fn sounds(self, baby: bool) -> [&'static str; 4] {
        if baby {
            return [
                "minecraft:entity.baby_chicken.ambient",
                "minecraft:entity.baby_chicken.hurt",
                "minecraft:entity.baby_chicken.death",
                "minecraft:entity.baby_chicken.step",
            ];
        }
        match self {
            Self::Classic => [
                "minecraft:entity.chicken.ambient",
                "minecraft:entity.chicken.hurt",
                "minecraft:entity.chicken.death",
                "minecraft:entity.chicken.step",
            ],
            Self::Picky => [
                "minecraft:entity.chicken_picky.ambient",
                "minecraft:entity.chicken_picky.hurt",
                "minecraft:entity.chicken_picky.death",
                "minecraft:entity.chicken.step",
            ],
        }
    }
}

/// 这只鸡此刻的四个音效。
///
/// This chicken's four sounds right now.
pub(super) fn sounds(mob: &Mob) -> [&'static str; 4] {
    mob.chicken_sound_variant.sounds(mob.is_baby())
}
