//! `Creaking`：和嘎枝之心绑定的敌对生物。
//!
//! 这里有官方构造函数的属性、元数据缺省、`home_pos` 存档、被看着时的冻结、
//! 不掉血而放树脂的受伤路径，以及心没了之后的拆解死亡。
//!
//! `Creaking`: the hostile bound to a creaking heart. This module ports the
//! constructor, state, `home_pos` key, freeze-on-gaze, no-damage hurt path and
//! the tear-down death flow.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    DamageKind, DamageSource, Mob, attributes::Attributes, brain::memory::MemoryType, goals, living,
};
use crate::{
    entity::{Entity, EntityUid, ctx::EntityCtx},
    shape::CollisionContext,
};

/// `Creaking.createAttributes()`：怪物属性上覆盖五个基础值。
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::monster();
    attributes.set_base(attribute::MAX_HEALTH, entity_types::CREAKING_MAX_HEALTH);
    attributes.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::CREAKING_MOVEMENT_SPEED,
    );
    attributes.set_base(
        attribute::ATTACK_DAMAGE,
        entity_types::CREAKING_ATTACK_DAMAGE,
    );
    attributes.set_base(attribute::FOLLOW_RANGE, entity_types::CREAKING_FOLLOW_RANGE);
    attributes.set_base(attribute::STEP_HEIGHT, entity_types::CREAKING_STEP_HEIGHT);
    attributes
}

/// `Creaking` 的运行时状态。四个同步项都按官方缺省值构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CreakingState {
    /// `CAN_MOVE`，缺省真。
    pub can_move: bool,
    /// `IS_ACTIVE`，缺省假。
    pub active: bool,
    /// `IS_TEARING_DOWN`，缺省假。
    pub tearing_down: bool,
    /// `HOME_POS`；没有绑定心时是空。
    pub home_pos: Option<BlockPos>,
    /// `playerStuckCounter`：连续几刻有玩家眼睛在 AABB 里。
    pub player_stuck_counter: i32,
    /// `invulnerabilityAnimationRemainingTicks`。
    pub invulnerability_animation: i32,
    /// `attackAnimationRemainingTicks`。
    pub attack_animation: i32,
    /// `deathTime` 超过 45 且拆解时真正移除。
    pub tear_down_time: i32,
}

impl CreakingState {
    /// 官方 `defineSynchedData` 的缺省值；`Default` 给别的生物用 false 填满。
    pub fn new() -> Self {
        Self {
            can_move: true,
            ..Self::default()
        }
    }
}

/// `setTransient(homePos)`：绑定心。
pub fn bind(mob: &mut Mob, home: BlockPos) {
    mob.creaking.home_pos = Some(home);
}

/// `isHeartBound()`。
pub fn is_heart_bound(mob: &Mob) -> bool {
    mob.creaking.home_pos.is_some()
}

/// `home_pos` 的 NBT 形状是三个 int 的数组。
pub fn home_tag(home: BlockPos) -> mc_bamboo_nbt::Tag {
    mc_bamboo_nbt::Tag::IntArray(vec![home.x, home.y, home.z])
}

/// 坏数据一律不绑定，不让客户端 / 命令传来的值触发 panic。
pub fn home_from_tag(tag: &mc_bamboo_nbt::Tag) -> Option<BlockPos> {
    let mc_bamboo_nbt::Tag::IntArray(values) = tag else {
        return None;
    };
    (values.len() == 3).then(|| BlockPos::new(values[0], values[1], values[2]))
}

/// `hurtServer`：绑定心、非 bypass、不在无敌动画里时返回心坐标；调用方发树脂命令。
pub(super) fn hurt_server(me: &Entity, mob: &mut Mob, source: &DamageSource) -> Option<BlockPos> {
    let home = mob.creaking.home_pos?;
    if source.kind.bypasses_invulnerability()
        || living::is_invulnerable_to(me, mob, source)
        || mob.creaking.invulnerability_animation > 0
        || mob.dead
    {
        return None;
    }
    let player_credit = source.attacker_is_player || matches!(source.kind, DamageKind::Arrow);
    if !player_credit && source.direct.is_none() && source.attacker.is_none() {
        return None;
    }
    mob.creaking.invulnerability_animation = 8;
    mob.events.push(super::MobEvent::EntityEvent { event: 66 });
    mob.creaking.invulnerability_animation = 8;
    if is_heart_bound(mob) {
        living::play_sound(me, mob, "minecraft:entity.creaking.sway", 1.0, 1.0);
    }
    Some(home)
}

/// `Creaking.aiStep`：被玩家看着时 `CAN_MOVE=false`，看着的玩家走开后解冻。
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.creaking.invulnerability_animation > 0 {
        mob.creaking.invulnerability_animation -= 1;
    }
    if mob.creaking.attack_animation > 0 {
        mob.creaking.attack_animation -= 1;
    }
    let was = mob.creaking.can_move;
    let can_move = check_can_move(me, mob, ctx);
    if was != can_move {
        mob.events.push(super::MobEvent::Sound {
            sound: if can_move {
                "minecraft:entity.creaking.unfreeze"
            } else {
                "minecraft:entity.creaking.freeze"
            },
            volume: 1.0,
            pitch: 1.0,
            position: me.position,
            hostile: true,
            seed: None,
        });
        if !can_move {
            mob.xxa = 0.0;
            mob.zza = 0.0;
            mob.jumping = false;
            mob.speed = 0.0;
            mob.navigation.stop();
        }
    }
    mob.creaking.can_move = can_move;
    if let Some(home) = mob.creaking.home_pos {
        // `BlockView` 的并行快照没有方块实体表；精确视图用 `has_block_entity` 认出心还在。
        // 拆解流程由 `removeProtector(null)` 自己置位，这里不误判成心消失。
        if !ctx.blocks().has_block_entity(home) && !mob.creaking.tearing_down {
            mob.health = 0.0;
            mob.dead = true;
        }
    }
}

/// `isLookingAtMe(player, 0.5, false, true, [eyeY, y+0.5, mid])` 的 Bamboo 版。
fn looking_at_me(
    me: &Entity,
    mob: &Mob,
    player: &crate::entity::ctx::PlayerView,
    view: &dyn crate::entity::ctx::BlockView,
) -> bool {
    if player.gaze_disguise {
        return false;
    }
    let from = player.eye_position();
    let eye = me.position.y + f64::from(mob.eye_height);
    for target_y in [eye, me.position.y + 0.5, (eye + me.position.y) / 2.0] {
        let to = Vec3::new(me.position.x, target_y, me.position.z);
        let offset = to.subtract(from);
        let length = offset.length();
        if length <= 0.0 {
            continue;
        }
        let direction = offset.normalize();
        let look = player.look_direction.normalize();
        if look.x * direction.x + look.y * direction.y + look.z * direction.z > 1.0 - 0.5 / length
            && view
                .clip_collider(from, to, CollisionContext::Empty)
                .is_none()
        {
            return true;
        }
    }
    false
}

/// `Creaking.checkCanMove`：读大脑的 `NEAREST_PLAYERS` 记忆，任一可攻击玩家盯着
/// 12 格内的嘎枝时冻结。
fn check_can_move(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    let view = ctx.blocks();
    let players = mob
        .brain
        .as_deref()
        .and_then(|brain| brain.memories.entities(MemoryType::NearestPlayers))
        .map(|list| list.to_vec())
        .unwrap_or_default();
    if players.is_empty() {
        if mob.creaking.active {
            deactivate(mob);
        }
        return true;
    }
    let mut any_attackable = false;
    for uid in players {
        let Some(player) = goals::player_by_uid(view, uid) else {
            continue;
        };
        if !player.alive || player.spectator || player.creative {
            continue;
        }
        any_attackable = true;
        if looking_at_me(me, mob, &player, view) {
            if mob.creaking.active {
                return false;
            }
            if player.position.distance_to_sqr(me.position) < 144.0 {
                activate(mob, uid);
                return false;
            }
        }
    }
    if !any_attackable && mob.creaking.active {
        deactivate(mob);
    }
    true
}

fn activate(mob: &mut Mob, player: EntityUid) {
    if let Some(brain) = mob.brain.as_mut() {
        brain.memories.set(
            MemoryType::AttackTarget,
            crate::entity::mob::brain::memory::MemoryValue::Entity(player),
        );
    }
    mob.creaking.active = true;
    mob.events.push(super::MobEvent::Sound {
        sound: "minecraft:entity.creaking.activate",
        volume: 1.0,
        pitch: 1.0,
        position: Vec3::default(),
        hostile: true,
        seed: None,
    });
}

fn deactivate(mob: &mut Mob) {
    if let Some(brain) = mob.brain.as_mut() {
        brain.memories.erase(MemoryType::AttackTarget);
    }
    mob.creaking.active = false;
    mob.events.push(super::MobEvent::Sound {
        sound: "minecraft:entity.creaking.deactivate",
        volume: 1.0,
        pitch: 1.0,
        position: Vec3::default(),
        hostile: true,
        seed: None,
    });
}

/// `Creaking.doHurtTarget`：非活体返回假；挥击动画 15 刻 + 事件 4，再走
/// `Monster.doHurtTarget`。
pub(super) fn swing_and_hurt(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: EntityUid,
) {
    mob.creaking.attack_animation = 15;
    mob.events.push(super::MobEvent::EntityEvent { event: 4 });
    living::do_hurt_target(me, mob, ctx, target);
}

/// `tickDeath` 的拆解分支：45 刻后播死亡声、广播 Poof 并移除，不走普通 20 刻死亡。
pub(super) fn tear_down_death(me: &mut Entity, mob: &mut Mob) -> bool {
    if !is_heart_bound(mob) || !mob.creaking.tearing_down {
        return false;
    }
    mob.creaking.tear_down_time += 1;
    if mob.creaking.tear_down_time <= 45 || me.removed {
        return true;
    }
    mob.events.push(super::MobEvent::Sound {
        sound: "minecraft:entity.creaking.death",
        volume: 1.0,
        pitch: 1.0,
        position: me.position,
        hostile: true,
        seed: None,
    });
    mob.events.push(super::MobEvent::Poof);
    me.discard();
    true
}

/// 逐刻对拍用的状态行。
impl Mob {
    pub fn describe_creaking(&self) -> String {
        let state = &self.creaking;
        format!(
            "can_move={} active={} tearing_down={} home={:?}",
            state.can_move, state.active, state.tearing_down, state.home_pos
        )
    }
}
