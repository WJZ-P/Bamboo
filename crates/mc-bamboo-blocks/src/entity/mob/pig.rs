//! 猪的目标、叫声和上鞍。
//!
//! Pig-specific goals, sounds, and saddle interaction.

use mc_bamboo_data::{
    component_registries::registry_names, entity_types, items, registries::attribute,
};
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::{
    Mob, MobType,
    attributes::Attributes,
    equipment::GUARANTEED_DROP_CHANCE,
    goals::{Goal, GoalSelector, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
};
use crate::{InteractionResult, entity::Entity};

pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.45, 0.45, 0.40625)
    } else {
        let facts = &entity_types::FACTS[MobType::Pig.entity_type() as usize];
        (facts.width, facts.height, facts.height * 0.85)
    }
}

pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MOVEMENT_SPEED, 0.25);
    attributes
}

pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut selector = GoalSelector::cow(navigation);
    for wrapped in &mut selector.goals {
        if wrapped.priority >= 2 {
            wrapped.priority += 1;
        }
        match &mut wrapped.goal {
            Goal::Panic { speed, .. } => *speed = 1.25,
            Goal::Tempt { speed, .. } => *speed = 1.2,
            Goal::FollowParent { speed, .. } => *speed = 1.1,
            _ => {}
        }
    }
    // 两个诱惑目标独立冷却；原版先注册胡萝卜钓竿，再注册食物。
    // The temptation goals cool down independently; vanilla registers the rod first.
    selector.goals.insert(
        3,
        WrappedGoal {
            priority: 4,
            running: false,
            goal: Goal::Tempt {
                speed: 1.2,
                steering_item: true,
                player: None,
                calm_down: 0,
                scare: None,
            },
        },
    );
    selector
}

/// `ItemBasedSteering`：胡萝卜钓竿加速的状态。`boost_time_total` 就是同步给客户端的
/// `DATA_BOOST_TIME`。
///
/// `ItemBasedSteering`: the carrot-on-a-stick boost state. `boost_time_total` is
/// the synced `DATA_BOOST_TIME`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Steering {
    pub boosting: bool,
    pub boost_time: i32,
    pub boost_time_total: i32,
}

impl Steering {
    /// `ItemBasedSteering.boost`：已在加速时返回假；否则开始加速，总时长 `nextInt(841) + 140`。
    ///
    /// `ItemBasedSteering.boost`: false while boosting, otherwise start a boost of
    /// `nextInt(841) + 140` ticks.
    pub fn boost(&mut self, random: &dyn crate::random::RandomSource) -> bool {
        if self.boosting {
            return false;
        }
        self.boosting = true;
        self.boost_time = 0;
        self.boost_time_total = random.next_int(841) + 140;
        true
    }

    /// `ItemBasedSteering.tickBoost`：比较的是自增前的值。
    ///
    /// `ItemBasedSteering.tickBoost`: compares the value before the increment.
    pub fn tick_boost(&mut self) {
        if self.boosting {
            let old = self.boost_time;
            self.boost_time += 1;
            if old > self.boost_time_total {
                self.boosting = false;
            }
        }
    }

    /// `ItemBasedSteering.boostFactor`。
    ///
    /// `ItemBasedSteering.boostFactor`.
    pub fn boost_factor(&self) -> f32 {
        if self.boosting && self.boost_time_total > 0 {
            let t = self.boost_time as f32 / self.boost_time_total as f32;
            1.0 + 1.15 * mc_bamboo_core::mth::sin(f64::from(t * std::f32::consts::PI))
        } else {
            1.0
        }
    }
}

/// `Pig.getRiddenSpeed`：`MOVEMENT_SPEED * 0.225 * boostFactor`。服务端上玩家控制的
/// 载具不能自己模拟运动，速度只由客户端用来算 `move_vehicle` 报上来的位置。
///
/// `Pig.getRiddenSpeed`: `MOVEMENT_SPEED * 0.225 * boostFactor`. On the server a
/// player-controlled vehicle cannot simulate its own movement; the client alone
/// uses the speed to compute the positions sent in `move_vehicle`.
pub fn ridden_speed(movement_speed: f64, steering: &Steering) -> f32 {
    (movement_speed * 0.225 * f64::from(steering.boost_factor())) as f32
}

/// `Pig.tickRidden`：朝向跟骑手（俯仰取一半），旧朝向、身体、头对齐，再走 `tickBoost`。
///
/// `Pig.tickRidden`: face the rider (half pitch), snap previous/body/head yaw, then
/// `tickBoost`.
pub fn tick_ridden(me: &mut Entity, mob: &mut Mob, yaw: f32, pitch: f32) {
    let yaw = if yaw.is_finite() { yaw } else { me.y_rot };
    let pitch = if pitch.is_finite() { pitch } else { 0.0 };
    me.y_rot = yaw;
    mob.x_rot = super::control::clamp_x_rot(pitch * 0.5, mob.x_rot);
    mob.y_rot_o = yaw;
    mob.y_body_rot = yaw;
    mob.y_head_rot = yaw;
    mob.pig_steering.tick_boost();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundVariant {
    Big,
    Classic,
    Mini,
}

impl SoundVariant {
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        match name {
            "big" => Some(Self::Big),
            "classic" => Some(Self::Classic),
            "mini" => Some(Self::Mini),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Big => "minecraft:big",
            Self::Classic => "minecraft:classic",
            Self::Mini => "minecraft:mini",
        }
    }

    pub fn registry_id(self) -> i32 {
        registry_names::PIG_SOUND_VARIANT
            .iter()
            .position(|name| *name == self.name())
            .expect("pig sound variant in the registry") as i32
    }

    pub(super) fn pick(random: &dyn crate::random::RandomSource) -> Self {
        let names = registry_names::PIG_SOUND_VARIANT;
        Self::from_name(names[random.next_int(names.len() as i32) as usize])
            .expect("known pig sound variant")
    }

    fn sounds(self, baby: bool) -> [&'static str; 4] {
        macro_rules! sounds {
            ($prefix:literal) => {
                [
                    concat!($prefix, ".ambient"),
                    concat!($prefix, ".hurt"),
                    concat!($prefix, ".death"),
                    concat!($prefix, ".eat"),
                ]
            };
        }
        if baby {
            return sounds!("minecraft:entity.baby_pig");
        }
        match self {
            Self::Big => sounds!("minecraft:entity.pig_big"),
            Self::Classic => sounds!("minecraft:entity.pig"),
            Self::Mini => sounds!("minecraft:entity.pig_mini"),
        }
    }

    pub(super) fn ambient(self, baby: bool) -> &'static str {
        self.sounds(baby)[0]
    }
    pub(super) fn hurt(self, baby: bool) -> &'static str {
        self.sounds(baby)[1]
    }
    pub(super) fn death(self, baby: bool) -> &'static str {
        self.sounds(baby)[2]
    }
}

pub(super) fn play_eating_sound(me: &mut Entity, mob: &mut Mob) {
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(
        me,
        mob,
        mob.pig_sound_variant.sounds(mob.is_baby())[3],
        1.0,
        pitch,
    );
}

pub(super) fn step_sound(baby: bool) -> &'static str {
    if baby {
        "minecraft:entity.baby_pig.step"
    } else {
        "minecraft:entity.pig.step"
    }
}

/// `Pig.mobInteract` 手里不是食物时：上了鞍、没人在骑、没潜行就骑上去（SUCCESS），
/// 否则走装鞍分支。
///
/// `Pig.mobInteract` with a non-food item: a saddled, unridden pig is mounted when
/// the player is not sneaking (SUCCESS); otherwise fall through to saddling.
pub(super) fn interact_not_food(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    let saddled = !mob.equipment.get(EquipmentSlot::Saddle).is_empty();
    if saddled && mob.passengers.is_empty() && !req.sneaking {
        return MobInteraction {
            result: InteractionResult::Success,
            ride: true,
            ..MobInteraction::pass()
        };
    }
    equip_saddle(me, mob, req)
}

/// `ItemStack.interactLivingEntity` → `Equippable.equipOnTarget` 的鞍分支：成年（`Pig.canUseSlot`
/// 对鞍槽要求 `!isBaby()`）、活着、鞍槽空才装上，声音来自 `LivingEntity.onEquipItem`。
///
/// The saddle branch of `ItemStack.interactLivingEntity` → `Equippable.equipOnTarget`: equip an
/// empty saddle slot only on a live adult (`Pig.canUseSlot` requires `!isBaby()` for the saddle
/// slot); the sound comes from `LivingEntity.onEquipItem`.
pub(super) fn equip_saddle(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    let slot = EquipmentSlot::Saddle;
    if req.held.is_empty()
        || req.held.item != items::SADDLE
        || mob.is_baby()
        || mob.is_dead_or_dying()
        || !mob.equipment.get(slot).is_empty()
    {
        return MobInteraction::pass();
    }
    let mut saddle = req.held.clone();
    saddle.count = 1;
    mob.equipment.set(slot, saddle);
    mob.equipment.set_drop_chance(slot, GUARANTEED_DROP_CHANCE);
    if me.tick_count > 0 && !mob.silent {
        mob.events.push(super::MobEvent::Sound {
            sound: "minecraft:entity.pig.saddle",
            volume: 1.0,
            pitch: 1.0,
            position: me.position,
            hostile: false,
            seed: Some(me.random.next_long()),
        });
    }
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !req.creative,
        replacement: None,
        ..MobInteraction::pass()
    }
}
