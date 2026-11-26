//! 唤魔者尖牙，官方 `EvokerFangs`（`Entity` 的子类）。
//!
//! 服务端一刻：`Entity.tick` 之后 `warmupDelayTicks--`；到 -8 时给碰撞盒
//! `inflate(0.2, 0, 0.2)` 里的活体打 6 点魔法伤害（跳过主人和盟友），然后发一次
//! 4 号实体事件，`lifeTicks--`，到 0 移除。尖牙不能受伤。
//!
//! Evoker fangs, vanilla `EvokerFangs` (an `Entity`). Server tick: after
//! `Entity.tick`, decrement `warmupDelayTicks`; at -8 deal 6 magic damage to the
//! living entities in the `inflate(0.2, 0, 0.2)` box (skipping the owner and its
//! allies), broadcast entity event 4, decrement `lifeTicks`, and discard at 0.
//! Fangs cannot be hurt.

use mc_bamboo_core::Vec3;

use super::mob::{DamageKind, DamageSource};
use super::{Entity, EntityUid};
use crate::World;
use crate::entity::ctx::{Command, EntityCtx};

/// `EvokerFangs.ATTACK_DURATION`：尖牙的总寿命。
pub const ATTACK_DURATION: i32 = 20;
/// 构造时 `lifeTicks` 的初值（`ATTACK_DURATION` + 2）。
const LIFE_TICKS: i32 = 22;

/// 尖牙自己的状态。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvokerFangs {
    /// `warmupDelayTicks`。
    pub warmup_delay_ticks: i32,
    /// `sentSpikeEvent`：服务端是否已经触发过 4 号事件（永久标志，不复位）。
    pub sent_spike_event: bool,
    /// 这一刻刚触发的 4 号事件，由实体表取走广播后清零。
    pub(crate) pending_spike_event: bool,
    /// `lifeTicks`，构造时为 22。
    pub life_ticks: i32,
    /// `owner`：召唤它的唤魔者。
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
}

impl EvokerFangs {
    pub const fn new(
        warmup_delay_ticks: i32,
        owner: Option<EntityUid>,
        owner_uuid: Option<[u8; 16]>,
    ) -> Self {
        Self {
            warmup_delay_ticks,
            sent_spike_event: false,
            pending_spike_event: false,
            life_ticks: LIFE_TICKS,
            owner,
            owner_uuid,
        }
    }
}

/// 生成一颗尖牙：`new EvokerFangs(level, x, y, z, yRot, warmupDelay, owner)`。
pub(crate) fn spawn(
    world: &mut World<'_>,
    position: Vec3,
    y_rot: f32,
    delay: i32,
    owner: EntityUid,
    owner_uuid: [u8; 16],
) {
    let mut entity = world.prepare_entity(Entity::new(
        super::EntityKind::EvokerFangs(Box::new(EvokerFangs::new(
            delay,
            Some(owner),
            Some(owner_uuid),
        ))),
        position,
    ));
    entity.y_rot = y_rot * 57.295_776;
    world.add_prepared_entity(entity);
}

/// `EvokerFangs.tick` 的服务端分支。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let mut hit = false;
    let mut remove = false;
    let (owner, owner_uuid) = {
        let super::EntityKind::EvokerFangs(fangs) = &mut me.kind else {
            return;
        };
        fangs.warmup_delay_ticks -= 1;
        if fangs.warmup_delay_ticks < 0 {
            if fangs.warmup_delay_ticks == -8 {
                hit = true;
            }
            if !fangs.sent_spike_event {
                fangs.sent_spike_event = true;
                fangs.pending_spike_event = true;
            }
            fangs.life_ticks -= 1;
            if fangs.life_ticks < 0 {
                remove = true;
            }
        }
        (fangs.owner, fangs.owner_uuid)
    };
    if hit {
        damage_nearby(me, ctx, owner, owner_uuid);
    }
    if remove {
        me.removed = true;
    }
}

/// `EvokerFangs.dealDamageTo`：bbox.inflate(0.2, 0, 0.2) 里活体、非主人、非盟友打 6 点。
fn damage_nearby(
    me: &mut Entity,
    ctx: &mut dyn EntityCtx,
    owner: Option<EntityUid>,
    owner_uuid: Option<[u8; 16]>,
) {
    let area = me.bounding_box().inflate(0.2, 0.0, 0.2);
    let owner_is_evoker = owner.and_then(|uid| ctx.other(uid)).is_some_and(|other| {
        matches!(
            &other.kind,
            super::KindPublic::Mob(public) if public.mob_type == super::mob::MobType::Evoker
        )
    });
    let mut targets = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if other.removed || other.uid == me.id || owner == Some(other.uid) {
            return;
        }
        let super::KindPublic::Mob(public) = &other.kind else {
            return;
        };
        // `Evoker.considersEntityAsAlly`：`#illager_friends` 和自己召唤的恼鬼都是盟友，不受尖牙伤。
        // `Evoker.considersEntityAsAlly`: `#illager_friends` and its vexes are allies, exempt from fangs.
        if owner_is_evoker
            && matches!(
                public.mob_type,
                super::mob::MobType::Evoker
                    | super::mob::MobType::Illusioner
                    | super::mob::MobType::Pillager
                    | super::mob::MobType::Vindicator
                    | super::mob::MobType::Vex
            )
        {
            return;
        }
        if public.is_alive() {
            targets.push(other.uid);
        }
    });
    let source = fangs_source(me, owner, owner_uuid);
    for target in targets {
        ctx.emit(
            Command::HurtMob {
                target,
                source,
                amount: 6.0,
                on_hit: None,
            },
            me,
        );
    }
    // 玩家目标：`dealDamageTo` 用 `hurt`（magic / indirectMagic）。
    let mut players = Vec::new();
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive
            && !player.creative
            && !player.spectator
            && player_box(player).intersects(area)
        {
            players.push(player.uid);
        }
    });
    for player in players {
        ctx.emit(
            Command::HurtPlayer {
                target: player,
                source,
                amount: 6.0,
            },
            me,
        );
    }
}

fn fangs_source(
    me: &Entity,
    owner: Option<EntityUid>,
    owner_uuid: Option<[u8; 16]>,
) -> DamageSource {
    // `EvokerFangs.dealDamageTo`：有主人是 `indirectMagic(owner, this)`，否则 `magic`。
    let mut source = DamageSource::environment(DamageKind::Magic);
    source.direct = Some(me.id);
    source.position = Some(me.position);
    if let Some(owner) = owner {
        source.attacker = Some(owner);
    }
    if let Some(uuid) = owner_uuid {
        source.attacker_uuid = Some(uuid);
    }
    source
}

fn player_box(player: &super::ctx::PlayerView) -> super::Aabb {
    super::arrow::player_box(player)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_ticks_starts_at_twenty_two() {
        let fangs = EvokerFangs::new(0, None, None);
        assert_eq!(fangs.life_ticks, 22);
        assert_eq!(ATTACK_DURATION, 20);
    }
}
