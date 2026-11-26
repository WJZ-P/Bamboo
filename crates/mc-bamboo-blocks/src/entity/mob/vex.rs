//! 恼鬼：官方 `Monster` → `Vex`。穿墙飞行（`noPhysics` + `VexMoveControl`）、有限生命
//! （`limitedLife` 到时每刻扣 1 点饥饿伤害）、冲锋玩家、跟随主人目标。
//!
//! Vex: vanilla `Monster` → `Vex`. A wall-phasing flyer (`noPhysics` +
//! `VexMoveControl`), a limited lifetime (`limitedLife`, 1 starvation damage per
//! tick when it runs out), a charge attack and owner-target copying.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    attributes::Attributes,
    goals::{self, Flag, Goal, GoalSelector, TargetType},
    living,
    navigation::Navigation,
};
use crate::World;
use crate::entity::{
    Entity, EntityUid,
    ctx::{Command, EntityCtx},
};

/// 唤魔者召唤恼鬼时的一条召唤：位置和有限生命刻数。
///
/// One summon from `EvokerSummonSpellGoal`: the position and limited life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VexSummon {
    pub position: BlockPos,
    pub limited_life: i32,
}

/// `Vex` 自己的状态：主人、出生锚点、有限生命。
///
/// The vex-only state: its owner, bound origin and limited life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VexState {
    /// `owner`：主人的实体 uid；读盘后为空，按 `owner_uuid` 再找。
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `boundOrigin`。
    pub bound_origin: Option<BlockPos>,
    /// `hasLimitedLife`。
    pub has_limited_life: bool,
    /// `limitedLifeTicks`。
    pub limited_life_ticks: i32,
    /// `isCharging`（同步 `DATA_FLAGS_ID` 的第 1 位）。
    pub charging: bool,
}

/// `Vex.createAttributes()`：`Monster` 的缺省加 14 血、4 攻击。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MAX_HEALTH, 14.0);
    table.set_base(attribute::ATTACK_DAMAGE, 4.0);
    table
}

/// `Vex` 自己的目标。
///
/// The vex's own goals.
#[derive(Debug, Clone, PartialEq)]
pub enum VexGoal {
    /// `VexChargeAttackGoal`：冲过去，撞到就打。
    ChargeAttack,
    /// `VexRandomMoveGoal`：锚点附近随机飞。
    RandomMove,
    /// `VexCopyOwnerTargetGoal`：复制主人的目标。
    CopyOwnerTarget,
}

impl VexGoal {
    pub(super) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::ChargeAttack | Self::RandomMove => &[Flag::Move],
            Self::CopyOwnerTarget => &[Flag::Target],
        }
    }

    pub(super) const fn every_tick(&self) -> bool {
        matches!(self, Self::ChargeAttack)
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::ChargeAttack => "VexChargeAttackGoal",
            Self::RandomMove => "VexRandomMoveGoal",
            Self::CopyOwnerTarget => "VexCopyOwnerTargetGoal",
        }
    }

    pub(super) fn can_use(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::ChargeAttack => {
                let Some((target, alive)) = mob.target.and_then(|uid| goals::target_view(ctx, uid))
                else {
                    return false;
                };
                if !alive || mob.mover.has_wanted() {
                    return false;
                }
                me.random.next_int_bounded(goals::reduced_tick_delay(7)) == 0
                    && me.position.distance_to_sqr(target.position) > 4.0
            }
            Self::RandomMove => {
                !mob.mover.has_wanted()
                    && me.random.next_int_bounded(goals::reduced_tick_delay(7)) == 0
            }
            Self::CopyOwnerTarget => copy_owner_target(mob, ctx).is_some(),
        }
    }

    pub(super) fn can_continue(
        &mut self,
        _me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::ChargeAttack => {
                mob.mover.has_wanted()
                    && mob.vex.charging
                    && mob
                        .target
                        .and_then(|uid| goals::target_view(ctx, uid))
                        .is_some_and(|(_, alive)| alive)
            }
            Self::RandomMove => false,
            Self::CopyOwnerTarget => mob.target.is_some(),
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::ChargeAttack => {
                let Some((target, _)) = mob.target.and_then(|uid| goals::target_view(ctx, uid))
                else {
                    return;
                };
                mob.mover.set_wanted_position(
                    target.position.x,
                    target.eye_y(),
                    target.position.z,
                    1.0,
                );
                mob.vex.charging = true;
                living::play_sound(me, mob, "minecraft:entity.vex.charge", 1.0, 1.0);
            }
            Self::RandomMove => {
                let anchor = mob.vex.bound_origin.unwrap_or_else(|| me.block_position());
                for _ in 0..3 {
                    let x = anchor.x - 7 + me.random.next_int_bounded(15);
                    let y = anchor.y - 5 + me.random.next_int_bounded(11);
                    let z = anchor.z - 7 + me.random.next_int_bounded(15);
                    let pos = BlockPos::new(x, y, z);
                    if ctx.blocks().block_state(pos).is_air() {
                        mob.mover.set_wanted_position(
                            f64::from(x) + 0.5,
                            f64::from(y) + 0.5,
                            f64::from(z) + 0.5,
                            0.25,
                        );
                        if mob.target.is_none() {
                            mob.look.set_look_at(
                                f64::from(x) + 0.5,
                                f64::from(y) + 0.5,
                                f64::from(z) + 0.5,
                                180.0,
                                20.0,
                            );
                        }
                        break;
                    }
                }
            }
            Self::CopyOwnerTarget => {
                let target = copy_owner_target(mob, ctx);
                mob.target = target;
            }
        }
    }

    pub(super) fn stop(&mut self, _me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::ChargeAttack => {
                mob.vex.charging = false;
            }
            Self::RandomMove | Self::CopyOwnerTarget => {}
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::ChargeAttack => {
                let Some(target) = mob.target.and_then(|uid| goals::target_view(ctx, uid)) else {
                    return;
                };
                let (target, _) = target;
                if me.bounding_box().intersects(target.hitbox()) {
                    // 撞到就打，然后停止冲锋。
                    ctx.emit(
                        Command::HurtMob {
                            target: target.uid,
                            source: melee_source(me, mob),
                            amount: mob.attributes.value(attribute::ATTACK_DAMAGE) as f32,
                            on_hit: None,
                        },
                        me,
                    );
                    mob.vex.charging = false;
                } else if me.position.distance_to_sqr(target.position) < 9.0 {
                    mob.mover.set_wanted_position(
                        target.position.x,
                        target.eye_y(),
                        target.position.z,
                        1.0,
                    );
                }
            }
            Self::RandomMove | Self::CopyOwnerTarget => {}
        }
    }
}

fn melee_source(me: &Entity, mob: &Mob) -> super::DamageSource {
    let mut source = super::DamageSource::environment(super::DamageKind::MobAttack);
    source.attacker = Some(me.id);
    source.direct = Some(me.id);
    source.attacker_uuid = Some(mob.uuid);
    source.attacker_type = Some(mob.mob_type.entity_type());
    source.position = Some(me.position);
    source
}

fn copy_owner_target(mob: &Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let owner = mob.vex.owner?;
    ctx.other(owner).and_then(|other| match &other.kind {
        crate::entity::KindPublic::Mob(public) => public.target,
        _ => None,
    })
}

/// `Vex.registerGoals`：漂浮、冲锋、随机飞、看玩家、看生物；目标复制主人的。
pub(super) fn goals(_navigation: &mut Navigation) -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (0, Goal::Float),
        (4, Goal::Vex(VexGoal::ChargeAttack)),
        (8, Goal::Vex(VexGoal::RandomMove)),
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
            Goal::Raider(super::raider::RaiderGoal::LookAtMob {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            }),
        ),
    ])
}

/// `Vex` 的目标选择器：受伤反击（不记袭击者的仇、会叫同伴）、复制主人、玩家。
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (
            1,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        (2, Goal::Vex(VexGoal::CopyOwnerTarget)),
        (3, Goal::nearest_target(TargetType::Player)),
    ])
}

/// `Vex.populateDefaultEquipmentSlots`：主手铁剑、掉落率 0。之后走 `Monster.finalizeSpawn`。
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn crate::random::RandomSource) {
    entity.with_mob(|_, mob| {
        mob.equipment.set(
            EquipmentSlot::MainHand,
            ItemStack::new(mc_bamboo_data::items::IRON_SWORD, 1),
        );
        mob.equipment.set_drop_chance(EquipmentSlot::MainHand, 0.0);
        // 恼鬼穿墙飞行、无重力。
        mob.no_physics = true;
        mob.no_gravity = true;
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Vex.addAdditionalSaveData`：`bound_pos`（可空）、`life_ticks`（仅有限生命）、`owner`。
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    if let Some(origin) = mob.vex.bound_origin {
        out.push((
            "bound_pos".to_owned(),
            Tag::IntArray(vec![origin.x, origin.y, origin.z]),
        ));
    }
    if mob.vex.has_limited_life {
        out.push((
            "life_ticks".to_owned(),
            Tag::Int(mob.vex.limited_life_ticks),
        ));
    }
    if let Some(uuid) = mob.vex.owner_uuid {
        out.push(("owner".to_owned(), super::nbt::uuid_tag(&uuid)));
    }
}

/// `Vex.readAdditionalSaveData`。
pub(super) fn load(mob: &mut Mob, tag: &Tag) {
    use crate::archive::tag;
    let pos = tag::int_array(tag, "bound_pos");
    mob.vex.bound_origin = (pos.len() == 3).then(|| BlockPos::new(pos[0], pos[1], pos[2]));
    match tag::field(tag, "life_ticks") {
        Some(Tag::Int(ticks)) => {
            mob.vex.has_limited_life = true;
            mob.vex.limited_life_ticks = *ticks;
        }
        _ => {
            mob.vex.has_limited_life = false;
            mob.vex.limited_life_ticks = 0;
        }
    }
    mob.vex.owner_uuid = super::nbt::uuid_field(tag, "owner");
}

/// `Vex.tick` 在 `super.tick()` 之后的那一段：`setNoGravity(true)` 和有限生命扣血。
/// 穿墙的 `noPhysics` 由 `move_living` 按 `mob.no_physics` 处理（恼鬼恒真）。
///
/// The part of `Vex.tick` after `super.tick()`: `setNoGravity(true)` and the limited
/// life. The wall-phasing `noPhysics` is handled by `move_living` via `mob.no_physics`
/// (always true for a vex).
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    mob.no_gravity = true;
    if mob.vex.has_limited_life {
        mob.vex.limited_life_ticks -= 1;
        if mob.vex.limited_life_ticks <= 0 {
            mob.vex.limited_life_ticks = 20;
            let source = super::DamageSource::environment(super::DamageKind::Starve);
            ctx.emit(
                Command::HurtMob {
                    target: me.id,
                    source,
                    amount: 1.0,
                    on_hit: None,
                },
                me,
            );
        }
    }
}

/// `VexMoveControl.tick`：MOVE_TO 时按速度逼近目标点，太近就 WAIT 并减速；
/// 有目标朝目标转，没有就朝速度方向转。
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.mover.operation != super::control::Operation::MoveTo {
        return;
    }
    let dx = mob.mover.wanted_x - me.position.x;
    let dy = mob.mover.wanted_y - me.position.y;
    let dz = mob.mover.wanted_z - me.position.z;
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    let box_size = f64::from(me.dimensions.0.max(me.dimensions.1));
    if length < box_size {
        mob.mover.operation = super::control::Operation::Wait;
        me.delta = me.delta.scale(0.5);
        return;
    }
    let speed = mob.mover.speed_modifier * 0.05 / length;
    me.delta = me.delta.add(Vec3::new(dx * speed, dy * speed, dz * speed));
    // 有目标朝目标转，没有就朝这一刻的速度方向转。
    let (tx, tz) = match mob.target.and_then(|uid| goals::target_view(ctx, uid)) {
        Some((target, _)) => (
            target.position.x - me.position.x,
            target.position.z - me.position.z,
        ),
        None => (me.delta.x, me.delta.z),
    };
    let yaw = -(mth::atan2(tz, tx) * 57.295_776_367_187_5) as f32;
    me.y_rot = yaw;
    mob.y_body_rot = yaw;
}

/// 落地 [`Command::SummonVex`]：生成三只恼鬼，设主人、锚点和有限生命。
pub(crate) fn summon(
    world: &mut World<'_>,
    summons: &[VexSummon],
    owner: EntityUid,
    owner_uuid: [u8; 16],
) {
    for summon in summons {
        let position = Vec3::new(
            f64::from(summon.position.x) + 0.5,
            f64::from(summon.position.y) + 0.5,
            f64::from(summon.position.z) + 0.5,
        );
        let mut entity = world.prepare_entity(Entity::by_mob_type(super::MobType::Vex, position));
        super::spawn::finalize_spawn(
            world,
            super::MobType::Vex,
            &mut entity,
            super::spawn::SpawnReason::Natural,
        );
        entity.with_mob(|_, mob| {
            mob.vex.owner = Some(owner);
            mob.vex.owner_uuid = Some(owner_uuid);
            mob.vex.bound_origin = Some(summon.position);
            mob.vex.has_limited_life = true;
            mob.vex.limited_life_ticks = summon.limited_life;
        });
        world.add_prepared_entity(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 14.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 4.0);
    }
}
