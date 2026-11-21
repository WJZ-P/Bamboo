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
use crate::{InteractionResult, entity::Entity, random::RandomSource};

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
            },
        },
    );
    selector
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

    pub(super) fn pick(random: &dyn RandomSource) -> Self {
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
        shear: false,
    }
}
