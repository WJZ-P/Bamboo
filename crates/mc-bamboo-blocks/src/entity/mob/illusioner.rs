//! 幻术师：官方 `PatrollingMonster` → `Raider` → `AbstractIllager` →
//! `SpellcasterIllager` → `Illusioner`（还实现 `RangedAttackMob`）。
//!
//! 两个法术（分身 DISAPPEAR、致盲 BLINDNESS）、弓箭攻击和施法目标按 26.3 字节码。
//! 分身的位置只在客户端渲染，服务端只加 1200 刻隐身；不在袭击波次表，只能 `/summon`。
//!
//! Illusioner: vanilla `PatrollingMonster` → `Raider` → `AbstractIllager` →
//! `SpellcasterIllager` → `Illusioner` (also `RangedAttackMob`). The two spells
//! (mirror DISAPPEAR, blindness BLINDNESS), the bow and the casting goal follow the
//! 26.3 bytecode. The duplicate positions are client-rendered only; the server just
//! applies 1200 ticks of invisibility. Not in the raid waves, only `/summon`.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    attributes::Attributes,
    avoid::{AvoidClass, AvoidEntity},
    goals::{self, Flag, Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
    raider::RaiderGoal,
    skeleton::BowGoal,
    spellcaster::{IllagerSpell, SpellcasterUseSpell},
};
use crate::entity::{
    Entity, EntityUid,
    ctx::{Command, EntityCtx},
};

/// 幻术师没有自己独有的存档字段（施法状态在 `spellcaster`），留一个空的状态占位。
///
/// The illusioner has no state of its own (casting lives in `spellcaster`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IllusionerState {}

/// `Illusioner.createAttributes()`：`Monster` 的缺省加 0.5 移速、18 跟随距离、32 血。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.5_f32));
    table.set_base(attribute::FOLLOW_RANGE, 18.0);
    table.set_base(attribute::MAX_HEALTH, 32.0);
    table
}

/// `Illusioner` 自己的目标：施法、分身、致盲。
///
/// The illusioner's own goals: casting, mirror and blindness.
#[derive(Debug, Clone, PartialEq)]
pub enum IllusionerGoal {
    /// `SpellcasterCastingSpellGoal`（基类，没有唤魔者的 wololo 分支）。
    Casting,
    /// `IllusionerMirrorSpellGoal`：隐身 1200 刻（DISAPPEAR）。
    MirrorSpell { spell: SpellcasterUseSpell },
    /// `IllusionerBlindnessSpellGoal`：致盲目标 400 刻（BLINDNESS）。
    BlindnessSpell {
        spell: SpellcasterUseSpell,
        last_target_id: Option<EntityUid>,
    },
}

impl IllusionerGoal {
    pub(super) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Casting => &[Flag::Move, Flag::Look],
            Self::MirrorSpell { .. } | Self::BlindnessSpell { .. } => &[],
        }
    }

    pub(super) const fn every_tick(&self) -> bool {
        false
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Casting => "SpellcasterCastingSpellGoal",
            Self::MirrorSpell { .. } => "IllusionerMirrorSpellGoal",
            Self::BlindnessSpell { .. } => "IllusionerBlindnessSpellGoal",
        }
    }

    pub(super) fn can_use(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Casting => mob.spellcaster.spell_casting_tick_count > 0,
            Self::MirrorSpell { spell } => {
                use_spell_can_use(me, mob, spell)
                    && !mob
                        .effects
                        .has(mc_bamboo_data::registries::mob_effect::INVISIBILITY)
            }
            Self::BlindnessSpell {
                spell,
                last_target_id,
            } => {
                // 改坏确认 `illusioner_blindness`：致盲永不施放，施法状态必须报不一致。
                // Break check `illusioner_blindness`: never cast, the spell state must diverge.
                if super::spellcaster::oracle_tamper()
                    == super::spellcaster::Tamper::IllusionerBlindness
                {
                    return false;
                }
                if !use_spell_can_use(me, mob, spell) {
                    return false;
                }
                let Some(target) = mob.target else {
                    return false;
                };
                if Some(target) == *last_target_id {
                    return false;
                }
                // 只在局部难度高于普通（`Difficulty.NORMAL.ordinal() = 2`）时用致盲。
                // Only cast blindness when effective difficulty exceeds `Difficulty.NORMAL.ordinal()` (2.0).
                ctx.blocks().effective_difficulty(me.block_position()) > 2.0
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        _me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Casting => mob.spellcaster.spell_casting_tick_count > 0,
            Self::MirrorSpell { spell } | Self::BlindnessSpell { spell, .. } => {
                mob.target
                    .and_then(|uid| goals::target_view(ctx, uid))
                    .is_some_and(|(_, alive)| alive)
                    && spell.attack_warmup_delay > 0
            }
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::Casting => {
                mob.navigation.stop();
            }
            Self::MirrorSpell { spell } => {
                use_spell_start(
                    me,
                    mob,
                    spell,
                    20,
                    340,
                    "minecraft:entity.illusioner.prepare_mirror",
                    IllagerSpell::Disappear,
                );
            }
            Self::BlindnessSpell {
                spell,
                last_target_id,
            } => {
                use_spell_start(
                    me,
                    mob,
                    spell,
                    20,
                    180,
                    "minecraft:entity.illusioner.prepare_blindness",
                    IllagerSpell::Blindness,
                );
                *last_target_id = mob.target;
            }
        }
    }

    pub(super) fn stop(&mut self, _me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::Casting => {
                mob.spellcaster.set_spell(IllagerSpell::None);
            }
            Self::MirrorSpell { .. } | Self::BlindnessSpell { .. } => {}
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::Casting => {
                if let Some(target) = goals::combat_target(mob, ctx) {
                    mob.look.set_look_at(
                        target.position.x,
                        target.eye_y(),
                        target.position.z,
                        super::control::MAX_HEAD_Y_ROT as f32,
                        goals::max_head_x_rot(mob),
                    );
                }
            }
            Self::MirrorSpell { spell } => {
                spell.attack_warmup_delay -= 1;
                if spell.attack_warmup_delay == 0 {
                    living::add_effect(
                        mob,
                        super::effects::EffectInstance::new(
                            mc_bamboo_data::registries::mob_effect::INVISIBILITY,
                            1200,
                        ),
                    );
                    living::play_sound(me, mob, "minecraft:entity.illusioner.cast_spell", 1.0, 1.0);
                }
            }
            Self::BlindnessSpell { spell, .. } => {
                spell.attack_warmup_delay -= 1;
                if spell.attack_warmup_delay == 0 {
                    if let Some(target) = mob.target {
                        ctx.emit(
                            Command::BlindTarget {
                                target,
                                effect: mc_bamboo_data::registries::mob_effect::BLINDNESS,
                                duration: 400,
                            },
                            me,
                        );
                    }
                    living::play_sound(me, mob, "minecraft:entity.illusioner.cast_spell", 1.0, 1.0);
                }
            }
        }
    }
}

fn use_spell_can_use(me: &mut Entity, mob: &mut Mob, spell: &SpellcasterUseSpell) -> bool {
    mob.target.is_some()
        && !mob.spellcaster.is_casting_spell()
        && me.tick_count >= spell.next_attack_tick_count
}

fn use_spell_start(
    me: &mut Entity,
    mob: &mut Mob,
    spell: &mut SpellcasterUseSpell,
    casting_time: i32,
    interval: i32,
    prepare_sound: &'static str,
    cast_spell: IllagerSpell,
) {
    spell.attack_warmup_delay = goals::reduced_tick_delay(20);
    mob.spellcaster.spell_casting_tick_count = casting_time;
    spell.next_attack_tick_count = me.tick_count + interval;
    living::play_sound(me, mob, prepare_sound, 1.0, 1.0);
    mob.spellcaster.set_spell(cast_spell);
}

/// `Illusioner.registerGoals`：父链的巡逻和四个袭击目标，然后是自己的八个。
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        (1, Goal::Illusioner(IllusionerGoal::Casting)),
        (
            3,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Creaking, 8.0, 1.0, 1.2)),
        ),
        (
            4,
            Goal::Illusioner(IllusionerGoal::MirrorSpell {
                spell: SpellcasterUseSpell::default(),
            }),
        ),
        (
            5,
            Goal::Illusioner(IllusionerGoal::BlindnessSpell {
                spell: SpellcasterUseSpell::default(),
                last_target_id: None,
            }),
        ),
        (6, Goal::Bow(BowGoal::illusioner())),
        (
            8,
            Goal::Stroll {
                speed: 0.6,
                interval: 120,
                probability: super::goals::PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            9,
            Goal::LookAtPlayer {
                look_distance: 3.0,
                probability: 1.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            10,
            Goal::Raider(RaiderGoal::LookAtMob {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            }),
        ),
    ]);
    navigation.flags.can_float = true;
    GoalSelector::from_ordered_pairs(goals)
}

/// `Illusioner` 的目标选择器：受伤反击、玩家、村民、铁傀儡（后三个都记 300 刻）。
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (
            1,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        (2, Goal::nearest_target(TargetType::Player)),
        (3, Goal::nearest_target_without_sight(TargetType::Villager)),
        // `NearestAttackableTargetGoal(IronGolem.class, false)`：铁傀儡不记视线丢失。
        // `NearestAttackableTargetGoal(IronGolem.class, false)`: golems keep no sight memory.
        (3, Goal::nearest_target_without_sight(TargetType::IronGolem)),
    ])
}

/// `Illusioner.finalizeSpawn`：主手放弓，然后 `SpellcasterIllager`（`Raider` + `Mob`）。
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn crate::random::RandomSource) {
    entity.with_mob(|_, mob| {
        mob.equipment.set(
            EquipmentSlot::MainHand,
            ItemStack::new(mc_bamboo_data::items::BOW, 1),
        );
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, false, true);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Illusioner.addAdditionalSaveData` 没有自己的键；`SpellTicks` 由施法层写。
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "SpellTicks".to_owned(),
        Tag::Int(mob.spellcaster.spell_casting_tick_count),
    ));
}

/// `Illusioner.readAdditionalSaveData` 的施法键。
pub(super) fn load(mob: &mut Mob, tag: &Tag) {
    mob.spellcaster.spell_casting_tick_count = crate::archive::tag::int_or(tag, "SpellTicks", 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 32.0);
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), f64::from(0.5_f32));
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 18.0);
    }

    #[test]
    fn goal_order_matches_vanilla() {
        let mut navigation = Navigation::new(18.0);
        let selector = goals(&mut navigation);
        let names: Vec<&str> = selector
            .goals
            .iter()
            .map(|wrapped| wrapped.goal.name())
            .collect();
        assert_eq!(names[0], "LongDistancePatrolGoal");
        assert_eq!(names[5], "FloatGoal");
        assert_eq!(names[6], "SpellcasterCastingSpellGoal");
        assert_eq!(names[7], "AvoidEntityGoal");
        assert_eq!(names[8], "IllusionerMirrorSpellGoal");
        assert_eq!(names[9], "IllusionerBlindnessSpellGoal");
        assert_eq!(names[10], "RangedBowAttackGoal");
    }

    /// 三个 `NearestAttackableTargetGoal` 的 `mustSee` 照字节码：玩家真（记 300 刻），
    /// 村民和铁傀儡假。
    ///
    /// The three `NearestAttackableTargetGoal` `mustSee` values match the bytecode:
    /// the player goal is true (300 ticks) while villager and golem are false.
    #[test]
    fn target_must_see_matches_vanilla() {
        let entries: Vec<_> = targets()
            .goals
            .iter()
            .filter_map(|wrapped| match &wrapped.goal {
                Goal::NearestAttackableTarget { kind, must_see, .. } => {
                    Some((wrapped.priority, *kind, *must_see))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            entries,
            vec![
                (2, TargetType::Player, true),
                (3, TargetType::Villager, false),
                (3, TargetType::IronGolem, false),
            ]
        );
    }
}
