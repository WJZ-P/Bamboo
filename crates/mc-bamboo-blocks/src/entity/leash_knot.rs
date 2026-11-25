//! 拴绳结实体，官方的 `LeashFenceKnotEntity`。
//!
//! 只有挂着的栅栏位置，没有别的状态。每刻检查脚下栅栏还在不在；被拆掉或所有绳子
//! 都断了就消失。
//!
//! A fence leash knot (`LeashFenceKnotEntity`): just the fence position. Each tick it
//! checks the fence still exists; it vanishes when the fence breaks or no rope remains.

use mc_bamboo_core::BlockPos;

use super::{Entity, EntityKind, EntityUid, LeashKnot, ctx::EntityCtx, mob::leash};
use crate::{InteractionResult, world::World};

/// `LeashFenceKnotEntity` 挂着的世界位置（栅栏那一格的上方 0.375 格处）。
pub fn knot_position(pos: BlockPos) -> mc_bamboo_core::Vec3 {
    mc_bamboo_core::Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.375,
        f64::from(pos.z) + 0.5,
    )
}

/// 新建一个拴绳结，放在 `pos` 那一格上。
pub fn create(world: &mut World, pos: BlockPos) -> EntityUid {
    world.spawn_entity(Entity::leash_knot(pos))
}

/// `getKnot`：在 `pos` 附近一格里找已经存在的拴绳结。
pub fn get(world: &World, pos: BlockPos) -> Option<EntityUid> {
    for entity in world.live_entities() {
        if let EntityKind::LeashKnot(knot) = &entity.kind
            && knot.pos == pos
        {
            return Some(entity.id);
        }
    }
    None
}

/// `getOrCreateKnot`：有就用，没有就造一个。
pub fn get_or_create(world: &mut World, pos: BlockPos) -> EntityUid {
    get(world, pos).unwrap_or_else(|| create(world, pos))
}

/// `BlockAttachedEntity.tick`：栅栏不在了就消失。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::LeashKnot(knot) = &me.kind else {
        return;
    };
    if !survives(ctx, knot.pos) {
        let position = me.position;
        me.discard();
        // `LeashFenceKnotEntity.dropItem`：`playSound(LEAD_UNTIED, 1, 1)`（只放音不掉拴绳）。
        ctx.emit(
            crate::entity::ctx::Command::LeashKnotUntied { position },
            me,
        );
    }
}

/// `survives()`：`blockState(pos).is(#fences)`。
pub fn survives(ctx: &dyn EntityCtx, pos: BlockPos) -> bool {
    ctx.blocks()
        .block_state(pos)
        .is_tagged(mc_bamboo_data::tags::block_tags::FENCES)
}

/// `Entity.leash_knot` 构造：位置是 `pos` 那一格的上方 0.375 格。
impl Entity {
    pub fn leash_knot(pos: BlockPos) -> Self {
        Self::new(EntityKind::LeashKnot(LeashKnot { pos }), knot_position(pos))
    }
}

/// `LeadItem.bindPlayerMobs`：把玩家牵着的生物都转到 `pos` 这一格的拴绳结上。
pub fn bind_player_mobs(world: &mut World, player: EntityUid, pos: BlockPos) -> InteractionResult {
    let center = mc_bamboo_core::Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    );
    // `leashableInArea(level, center, holder == player)`。
    let mut mobs = Vec::new();
    for entity in world.live_entities() {
        if let Some(mob) = entity.kind.mob()
            && leash::leash_holder(mob) == Some(player)
            && leash::in_leashable_area(entity.bounding_box(), center)
        {
            mobs.push(entity.id);
        }
    }
    if mobs.is_empty() {
        return InteractionResult::Pass;
    }
    let existing = get(world, pos);
    let knot = existing.unwrap_or_else(|| create(world, pos));
    let now = world.game_time();
    let mut tied = false;
    for uid in mobs {
        let mut attached = false;
        world.with_entity_mut(uid, &mut |entity| {
            let position = entity.position;
            let height = entity.dimensions.1;
            entity.with_mob(|_, mob| {
                if leash::can_have_a_leash_attached_to(
                    mob,
                    position,
                    height,
                    knot_position(pos),
                    0.5,
                    now,
                ) {
                    leash::set_leashed_to(mob, knot, leash::LeashDelayed::Knot(pos));
                    attached = true;
                }
            });
        });
        tied |= attached;
    }
    if tied {
        // `playPlacementSound`：`playSound(LEAD_TIED, 1, 1)`。
        world.play_sound(
            knot_position(pos),
            mc_bamboo_data::sound_event("minecraft:item.lead.tied"),
            mc_bamboo_data::sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
        InteractionResult::SuccessServer
    } else {
        // 一个都没拴上，而且这个结是新建的：拆掉。
        if existing.is_none() {
            world.remove_entity(knot);
        }
        InteractionResult::Pass
    }
}

/// `LeashFenceKnotEntity.interact`：先试把玩家牵着的生物拴到结上；一个都没拴上且没
/// 潜行时，把结上的生物转回玩家。`shears` 分支由调用方（生物交互）处理。
pub fn interact(
    world: &mut World,
    knot: EntityUid,
    pos: BlockPos,
    player: EntityUid,
    player_uuid: [u8; 16],
    sneaking: bool,
) -> InteractionResult {
    // 第一圈：玩家牵着的生物 → 结。
    let mut tied = false;
    for entity in world.live_entities() {
        let Some(mob) = entity.kind.mob() else {
            continue;
        };
        if leash::leash_holder(mob) != Some(player) {
            continue;
        }
        let position = entity.position;
        let height = entity.dimensions.1;
        let can = leash::can_have_a_leash_attached_to(
            mob,
            position,
            height,
            knot_position(pos),
            0.5,
            world.game_time(),
        );
        if can {
            world.with_entity_mut(entity.id, &mut |e| {
                e.with_mob(|_, m| leash::set_leashed_to(m, knot, leash::LeashDelayed::Knot(pos)));
            });
            tied = true;
        }
    }
    let mut untied = false;
    if !tied && !sneaking {
        // 第二圈：结上的生物 → 玩家。
        for entity in world.live_entities() {
            let Some(mob) = entity.kind.mob() else {
                continue;
            };
            if leash::leash_holder(mob) != Some(knot) {
                continue;
            }
            world.with_entity_mut(entity.id, &mut |e| {
                e.with_mob(|_, m| {
                    leash::set_leashed_to(m, player, leash::LeashDelayed::Entity(player_uuid));
                });
            });
            untied = true;
        }
    }
    if untied {
        // `setLeashedTo` 的 `notifyLeasheeRemoved`：都解走后结消失。
        notify_leashee_removed(world, knot);
    }
    if tied || untied {
        world.play_sound(
            knot_position(pos),
            mc_bamboo_data::sound_event("minecraft:item.lead.tied"),
            mc_bamboo_data::sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
        InteractionResult::Success
    } else {
        InteractionResult::Pass
    }
}

/// `notifyLeasheeRemoved`：`holder` 是拴绳结且没有生物还拴在它身上时，结消失。
pub fn notify_leashee_removed(world: &mut World, holder: EntityUid) {
    if !is_knot(world, holder) {
        return;
    }
    let empty = !world.live_entities().iter().any(|entity| {
        entity
            .kind
            .mob()
            .is_some_and(|mob| leash::leash_holder(mob) == Some(holder))
    });
    if empty {
        world.remove_entity(holder);
    }
}

/// `holder` 是不是拴绳结。
fn is_knot(world: &World, uid: EntityUid) -> bool {
    world
        .live_entities()
        .iter()
        .any(|entity| entity.id == uid && matches!(entity.kind, EntityKind::LeashKnot(_)))
}

/// `Leashable.leashableLeashedTo(holder)` 里的每个生物 `dropLeash()`，返回有没有掉。
/// 剪刀剪结时用它。
pub fn drop_all_leashed_to(world: &mut World, holder: EntityUid) -> bool {
    let mut mobs = Vec::new();
    for entity in world.live_entities() {
        if let Some(mob) = entity.kind.mob()
            && leash::leash_holder(mob) == Some(holder)
        {
            mobs.push(entity.id);
        }
    }
    let dropped = !mobs.is_empty();
    for uid in mobs {
        world.with_entity_mut(uid, &mut |entity| {
            entity.with_mob(|_, mob| {
                leash::drop_leash(mob);
            });
        });
    }
    dropped
}
