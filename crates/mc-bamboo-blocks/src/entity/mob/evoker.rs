//! 唤魔者：官方 `PatrollingMonster` → `Raider` → `AbstractIllager` →
//! `SpellcasterIllager` → `Evoker`。
//!
//! 三个法术目标（尖牙、召唤恼鬼、wololo）和施法目标按 26.3 字节码：属性、存档键
//! `SpellTicks`、目标注册顺序、每个法术的暖机 / 施法时长 / 间隔和随机取数顺序。
//!
//! Evoker: vanilla `PatrollingMonster` → `Raider` → `AbstractIllager` →
//! `SpellcasterIllager` → `Evoker`. The three spells (fangs, summon vexes, wololo)
//! and the casting goal follow the 26.3 bytecode for attributes, the `SpellTicks`
//! save key, goal order, and each spell's warmup / casting time / interval and
//! random draw order.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;

use super::{
    Mob,
    attributes::Attributes,
    avoid::{AvoidClass, AvoidEntity},
    goals::{self, Flag, Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
    raider::RaiderGoal,
    spellcaster::{IllagerSpell, SpellcasterUseSpell},
};
use crate::entity::{
    Entity, EntityUid, KindPublic,
    ctx::{Command, EntityCtx},
};

/// `Evoker` 自己的状态：wololo 法术选中的蓝羊。
///
/// The evoker-only state: the blue sheep picked by the wololo spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvokerState {
    pub wololo_target: Option<EntityUid>,
}

/// `Evoker.createAttributes()`：`Monster` 的缺省加 0.5 移速、12 跟随距离、24 血。
///
/// `Evoker.createAttributes()`: the monster defaults plus 0.5 speed, a follow
/// range of 12 and 24 health.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.5_f32));
    table.set_base(attribute::FOLLOW_RANGE, 12.0);
    table.set_base(attribute::MAX_HEALTH, 24.0);
    table
}

/// `Evoker` 自己的目标：施法、三个法术、躲玩家 / 嘎枝、闲逛、看玩家 / 看生物。
///
/// The evoker's own goals: casting, the three spells, avoiding players and
/// creakings, strolling, and the two look goals.
#[derive(Debug, Clone, PartialEq)]
pub enum EvokerGoal {
    /// `EvokerCastingSpellGoal`：施法期间锁 MOVE|LOOK 并看向目标（或 wololo 目标）。
    Casting,
    /// `EvokerAttackSpellGoal`：放尖牙（FANGS）。
    AttackSpell { spell: SpellcasterUseSpell },
    /// `EvokerSummonSpellGoal`：召唤 3 只恼鬼（SUMMON_VEX）。
    SummonSpell { spell: SpellcasterUseSpell },
    /// `EvokerWololoSpellGoal`：把蓝羊变红（WOLOLO）。
    WololoSpell { spell: SpellcasterUseSpell },
}

impl EvokerGoal {
    pub(super) const fn flags(&self) -> &'static [Flag] {
        // 施法目标锁 MOVE|LOOK；三个法术目标不设标志位（官方没调 `setFlags`）。
        // The casting goal locks MOVE|LOOK; the spells set none.
        match self {
            Self::Casting => &[Flag::Move, Flag::Look],
            Self::AttackSpell { .. } | Self::SummonSpell { .. } | Self::WololoSpell { .. } => &[],
        }
    }

    pub(super) const fn every_tick(&self) -> bool {
        false
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Casting => "EvokerCastingSpellGoal",
            Self::AttackSpell { .. } => "EvokerAttackSpellGoal",
            Self::SummonSpell { .. } => "EvokerSummonSpellGoal",
            Self::WololoSpell { .. } => "EvokerWololoSpellGoal",
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
            Self::AttackSpell { spell } => use_spell_can_use(me, mob, spell),
            Self::SummonSpell { spell } => {
                if !use_spell_can_use(me, mob, spell) {
                    return false;
                }
                // 16 格内的恼鬼数不超过 `nextInt(8) + 1` 才召唤。
                // Summon only while the vexes within 16 blocks do not exceed
                // `nextInt(8) + 1`.
                let count = count_nearby_vexes(me, mob, ctx);
                count <= me.random.next_int_bounded(8) + 1
            }
            Self::WololoSpell { spell } => {
                if mob.target.is_some()
                    || mob.spellcaster.spell_casting_tick_count > 0
                    || me.tick_count < spell.next_attack_tick_count
                {
                    return false;
                }
                let view = ctx.blocks();
                if !view.block_game_rules().mob_griefing {
                    return false;
                }
                // 16×4×16 内找蓝羊，随机挑一只。
                let sheep = nearby_blue_sheep(me, ctx);
                let Some(sheep) = sheep else {
                    return false;
                };
                let index = me.random.next_int_bounded(sheep.len() as i32) as usize;
                mob.evoker.wololo_target = Some(sheep[index]);
                true
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
            Self::AttackSpell { spell } | Self::SummonSpell { spell } => {
                target_alive(mob, ctx) && spell.attack_warmup_delay > 0
            }
            Self::WololoSpell { spell } => {
                mob.evoker.wololo_target.is_some() && spell.attack_warmup_delay > 0
            }
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::Casting => {
                mob.navigation.stop();
            }
            Self::AttackSpell { spell } => use_spell_start(
                me,
                mob,
                spell,
                40,
                100,
                "minecraft:entity.evoker.prepare_attack",
                IllagerSpell::Fangs,
            ),
            Self::SummonSpell { spell } => use_spell_start(
                me,
                mob,
                spell,
                100,
                340,
                "minecraft:entity.evoker.prepare_summon",
                IllagerSpell::SummonVex,
            ),
            Self::WololoSpell { spell } => {
                // `getCastWarmupTime()` 覆盖成 40（不是基类的 20）。
                spell.attack_warmup_delay = goals::reduced_tick_delay(40);
                spell.next_attack_tick_count = me.tick_count + 140;
                mob.spellcaster.spell_casting_tick_count = 60;
                living::play_sound(me, mob, "minecraft:entity.evoker.prepare_wololo", 1.0, 1.0);
                mob.spellcaster.set_spell(IllagerSpell::Wololo);
            }
        }
    }

    pub(super) fn stop(&mut self, _me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::Casting => {
                mob.spellcaster.set_spell(IllagerSpell::None);
            }
            Self::WololoSpell { .. } => {
                mob.evoker.wololo_target = None;
            }
            Self::AttackSpell { .. } | Self::SummonSpell { .. } => {}
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
                } else if let Some(sheep) = mob.evoker.wololo_target {
                    look_at_mob(me, mob, ctx, sheep);
                }
            }
            Self::AttackSpell { spell } => {
                spell.attack_warmup_delay -= 1;
                if spell.attack_warmup_delay == 0 {
                    perform_attack(me, mob, ctx);
                    living::play_sound(me, mob, "minecraft:entity.evoker.cast_spell", 1.0, 1.0);
                }
            }
            Self::SummonSpell { spell } => {
                spell.attack_warmup_delay -= 1;
                if spell.attack_warmup_delay == 0 {
                    perform_summon(me, mob, ctx);
                    living::play_sound(me, mob, "minecraft:entity.evoker.cast_spell", 1.0, 1.0);
                }
            }
            Self::WololoSpell { spell } => {
                spell.attack_warmup_delay -= 1;
                if spell.attack_warmup_delay == 0 {
                    if let Some(sheep) = mob.evoker.wololo_target {
                        ctx.emit(
                            Command::SheepSetColor {
                                target: sheep,
                                color: 14,
                            },
                            me,
                        );
                    }
                    living::play_sound(me, mob, "minecraft:entity.evoker.cast_spell", 1.0, 1.0);
                }
            }
        }
    }
}

/// `SpellcasterUseSpellGoal.canUse`：目标活着、没在施法、间隔已到。
fn use_spell_can_use(me: &mut Entity, mob: &mut Mob, spell: &SpellcasterUseSpell) -> bool {
    mob.target.is_some()
        && !mob.spellcaster.is_casting_spell()
        && me.tick_count >= spell.next_attack_tick_count
}

/// `SpellcasterUseSpellGoal.canContinueToUse`：目标活着且暖机还没结束。
fn target_alive(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    mob.target
        .and_then(|uid| goals::target_view(ctx, uid))
        .is_some_and(|(_, alive)| alive)
}

/// `SpellcasterUseSpellGoal.start`：暖机、施法时长、下次间隔、准备声和同步法术。
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

/// 看向另一只生物（wololo 目标 / 恼鬼主人都用）。位置取脚底，眼高按目标种类。
fn look_at_mob(_me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx, uid: EntityUid) {
    let Some((target, _)) = goals::target_view(ctx, uid) else {
        return;
    };
    mob.look.set_look_at(
        target.position.x,
        target.eye_y(),
        target.position.z,
        super::control::MAX_HEAD_Y_ROT as f32,
        goals::max_head_x_rot(mob),
    );
}

/// `EvokerSummonSpellGoal.canUse` 里的恼鬼计数：16 格盒子里活的 `Vex`。
fn count_nearby_vexes(me: &Entity, _mob: &Mob, ctx: &dyn EntityCtx) -> i32 {
    let area = me.bounding_box().inflate(16.0, 16.0, 16.0);
    let mut count = 0;
    ctx.for_each_other_in(area, &mut |other| {
        if !other.removed
            && matches!(
                &other.kind,
                KindPublic::Mob(public) if public.mob_type == super::MobType::Vex
            )
        {
            count += 1;
        }
    });
    count
}

/// `EvokerWololoSpellGoal.canUse` 里找蓝羊：16×4×16 盒子里活的羊，毛色是蓝。
fn nearby_blue_sheep(me: &Entity, ctx: &dyn EntityCtx) -> Option<Vec<EntityUid>> {
    let area = me.bounding_box().inflate(16.0, 4.0, 16.0);
    let mut found = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if other.removed {
            return;
        }
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if public.mob_type == super::MobType::Sheep && public.is_alive() && public.wool & 0xF == 11
        {
            found.push(other.uid);
        }
    });
    (!found.is_empty()).then_some(found)
}

/// `EvokerAttackSpellGoal.performSpellCasting`：按距离放尖牙。近距离是半径 1.5 的圆
/// 加半径 2.5 的环，远距离是 16 颗直线排开的尖牙。
fn perform_attack(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(target) = mob.target.and_then(|uid| goals::target_view(ctx, uid)) else {
        return;
    };
    let (target, _) = target;
    let min_y = target.position.y.min(me.position.y);
    let max_y = target.position.y.max(me.position.y) + 1.0;
    let yaw = (mth::atan2(
        target.position.z - me.position.z,
        target.position.x - me.position.x,
    )) as f32;
    let distance = me.position.distance_to_sqr(target.position);
    if distance < 9.0 {
        for i in 0..5 {
            let angle = yaw + (i as f32) * std::f32::consts::PI * 0.4;
            let x = me.position.x + f64::from(mth::cos(f64::from(angle))) * 1.5;
            let z = me.position.z + f64::from(mth::sin(f64::from(angle))) * 1.5;
            spawn_fangs(me, mob, ctx, x, min_y, max_y, z, angle, 0);
        }
        for i in 0..8 {
            let angle = yaw + (i as f32) * std::f32::consts::PI * 2.0 / 8.0 + 1.256_637_1;
            let x = me.position.x + f64::from(mth::cos(f64::from(angle))) * 2.5;
            let z = me.position.z + f64::from(mth::sin(f64::from(angle))) * 2.5;
            spawn_fangs(me, mob, ctx, x, min_y, max_y, z, angle, 3);
        }
    } else {
        for i in 0..16 {
            let step = 1.25 * f64::from(i as f32 + 1.0);
            let x = me.position.x + f64::from(mth::cos(f64::from(yaw))) * step;
            let z = me.position.z + f64::from(mth::sin(f64::from(yaw))) * step;
            spawn_fangs(me, mob, ctx, x, min_y, max_y, z, yaw, i);
        }
    }
}

/// `EvokerAttackSpellGoal.createSpellEntity`：从目标 Y 往下找 sturdy 面，把尖牙落在上面。
#[allow(clippy::too_many_arguments, reason = "照官方 createSpellEntity 的参数")]
fn spawn_fangs(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    x: f64,
    min_y: f64,
    max_y: f64,
    z: f64,
    yaw: f32,
    delay: i32,
) {
    let view = ctx.blocks();
    let mut pos = BlockPos::containing(Vec3::new(x, min_y, z));
    let mut offset = 0.0;
    let mut found = false;
    loop {
        let below = pos.below();
        if view.is_face_sturdy(
            below,
            mc_bamboo_core::Direction::Up,
            crate::state::SupportType::Full,
        ) {
            let state = view.block_state(pos);
            if !state.is_air() {
                let boxes =
                    crate::shape::collision_boxes(state, pos, crate::CollisionContext::Empty);
                if let Some(bb) = boxes
                    .iter()
                    .max_by(|a, b| a.max_y.partial_cmp(&b.max_y).unwrap())
                {
                    offset = bb.max_y;
                }
            }
            found = true;
            break;
        }
        pos = pos.below();
        if pos.y < mth::floor(max_y - 1.0) {
            break;
        }
    }
    if !found {
        return;
    }
    let y = f64::from(pos.y) + offset;
    ctx.emit(
        Command::SpawnFangs {
            position: Vec3::new(x, y, z),
            y_rot: yaw * 57.295_776,
            delay,
            owner: me.id,
            owner_uuid: mob.uuid,
        },
        me,
    );
}

/// `EvokerSummonSpellGoal.performSpellCasting`：三只恼鬼，位置和寿命的随机数照官方顺序。
fn perform_summon(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let base = me.block_position();
    let mut summons = Vec::with_capacity(3);
    for _ in 0..3 {
        let x = base.x - 2 + me.random.next_int_bounded(5);
        let z = base.z - 2 + me.random.next_int_bounded(5);
        let limited_life = 20 * (30 + me.random.next_int_bounded(90));
        summons.push(crate::entity::mob::vex::VexSummon {
            position: BlockPos::new(x, base.y + 1, z),
            limited_life,
        });
    }
    ctx.emit(
        Command::SummonVex {
            summons,
            owner: me.id,
            owner_uuid: mob.uuid,
        },
        me,
    );
}

/// `Evoker.registerGoals`：父链的巡逻和四个袭击目标，然后是自己的九个。
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        (1, Goal::Evoker(EvokerGoal::Casting)),
        (
            2,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Player, 8.0, 0.6, 1.0)),
        ),
        (
            3,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Creaking, 8.0, 0.6, 1.0)),
        ),
        (
            4,
            Goal::Evoker(EvokerGoal::SummonSpell {
                spell: SpellcasterUseSpell::default(),
            }),
        ),
        (
            5,
            Goal::Evoker(EvokerGoal::AttackSpell {
                spell: SpellcasterUseSpell::default(),
            }),
        ),
        (
            6,
            Goal::Evoker(EvokerGoal::WololoSpell {
                spell: SpellcasterUseSpell::default(),
            }),
        ),
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

/// `Evoker` 的目标选择器：受伤反击（不记袭击者的仇、会叫同伴）、玩家、村民、铁傀儡。
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
        (3, Goal::nearest_target(TargetType::IronGolem)),
    ])
}

/// `Evoker.finalizeSpawn`：`Raider`（能加入袭击）、`PatrollingMonster`（队长 / 巡逻）和
/// `Mob` 的初始化，按官方顺序取随机数。唤魔者自己不刷装备。
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn crate::random::RandomSource) {
    entity.with_mob(|_, mob| {
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, false, true);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Evoker.addAdditionalSaveData` 没有自己的键；`SpellTicks` 由施法层写。
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "SpellTicks".to_owned(),
        Tag::Int(mob.spellcaster.spell_casting_tick_count),
    ));
}

/// `Evoker.readAdditionalSaveData` 的施法键。
pub(super) fn load(mob: &mut Mob, tag: &Tag) {
    mob.spellcaster.spell_casting_tick_count = crate::archive::tag::int_or(tag, "SpellTicks", 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 24.0);
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), f64::from(0.5_f32));
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 12.0);
    }

    #[test]
    fn goal_order_matches_vanilla() {
        let mut navigation = Navigation::new(12.0);
        let selector = goals(&mut navigation);
        let names: Vec<&str> = selector
            .goals
            .iter()
            .map(|wrapped| wrapped.goal.name())
            .collect();
        assert_eq!(names[0], "LongDistancePatrolGoal");
        assert_eq!(names[5], "FloatGoal");
        assert_eq!(names[6], "EvokerCastingSpellGoal");
        assert_eq!(names[7], "AvoidEntityGoal");
        assert_eq!(names[9], "EvokerSummonSpellGoal");
        assert_eq!(names[10], "EvokerAttackSpellGoal");
        assert_eq!(names[11], "EvokerWololoSpellGoal");
    }
}
