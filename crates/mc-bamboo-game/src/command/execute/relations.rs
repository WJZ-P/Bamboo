//! `ExecuteCommand.createRelationOperations` 查询当前岛上的实体关系。
//!
//! `ExecuteCommand.createRelationOperations` resolves entities in the current island.

use mc_bamboo_blocks::entity::{EntityKind, FIRST_ENTITY_ID, KindPublic, mob::MobType};
use mc_bamboo_core::Hand;

use crate::{
    command::select::{Target, entity_by_uuid},
    inbound::PlayerId,
    region::Region,
    riding,
};

pub(super) fn targets(region: &Region, entity: Target, relation: &str) -> Vec<Target> {
    match (entity, relation) {
        (Target::World(uid), "passengers") => riding::passengers(region, uid)
            .into_iter()
            .filter_map(|uid| by_id(region, uid))
            .collect(),
        _ => single(region, entity, relation).into_iter().collect(),
    }
}

fn single(region: &Region, entity: Target, relation: &str) -> Option<Target> {
    let uid = match entity {
        Target::Player(id) if relation == "vehicle" => region.players.get(&id)?.riding.vehicle?,
        Target::Player(_) => return None,
        Target::World(uid) => return world_relation(region, uid, relation),
    };
    by_id(region, uid)
}

fn world_relation(region: &Region, uid: i32, relation: &str) -> Option<Target> {
    let public = region.entities.public_at(uid)?;
    if relation == "origin" {
        return origin(region, &region.entities.get(uid)?.kind);
    }
    let related = match (&public.kind, relation) {
        (_, "vehicle") => public.vehicle?,
        (KindPublic::Boat(boat), "controller") => *boat.passengers.first()?,
        (KindPublic::Mob(_), "controller") => mob_controller(region, uid)?,
        (KindPublic::Mob(mob), "leasher") => mob.leash_holder?,
        (KindPublic::Mob(mob), "target") => mob.target?,
        (KindPublic::Mob(mob), "attacker") => mob.last_hurt_by_mob?,
        (KindPublic::Mob(mob), "owner") if mob.mob_type == MobType::Vex => {
            return origin(region, &region.entities.get(uid)?.kind).filter(|&owner| {
                matches!(owner, Target::Player(_))
                    || matches!(owner, Target::World(id) if region.entities.public_at(id).is_some_and(|p| matches!(p.kind, KindPublic::Mob(_))))
            });
        }
        (KindPublic::Mob(mob), "owner") => {
            return entity_by_uuid(region, uuid::Uuid::from_bytes(mob.owner?));
        }
        _ => return None,
    };
    by_id(region, related)
        .filter(|target| relation != "controller" || matches!(target, Target::Player(_)))
}

fn mob_controller(region: &Region, uid: i32) -> Option<i32> {
    let entity = region.entities.get(uid)?;
    let mob = entity.kind.mob()?;
    let passenger = mob.controlling_passenger()?;
    let player = region.players.get(&PlayerId(passenger))?;
    if mob.mob_type == MobType::Pig
        && ![Hand::Main, Hand::Off].into_iter().any(|hand| {
            player.inventory.held(hand).item == mc_bamboo_data::items::CARROT_ON_A_STICK
        })
    {
        return None;
    }
    Some(passenger)
}

/// `TraceableEntity.getOwner`：按实体类型读主人引用，而不是物品的捡取限制。
///
/// `TraceableEntity.getOwner` uses the type's owner reference, not an item's pickup restriction.
fn origin(region: &Region, kind: &EntityKind) -> Option<Target> {
    let (id, uuid) = match kind {
        EntityKind::Tnt(tnt) => (tnt.owner, None),
        EntityKind::Arrow(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::LlamaSpit(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::Egg(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::SplashPotion(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::EvokerFangs(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::WindCharge(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::WitherSkull(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::FireworkRocket(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::FishingHook(entity) => (entity.owner, entity.owner_uuid),
        EntityKind::Mob(mob) if mob.mob_type == MobType::Vex => (mob.vex.owner, mob.vex.owner_uuid),
        _ => return None,
    };
    id.and_then(|id| by_id(region, id))
        .or_else(|| entity_by_uuid(region, uuid::Uuid::from_bytes(uuid?)))
}

fn by_id(region: &Region, uid: i32) -> Option<Target> {
    if uid < FIRST_ENTITY_ID {
        let id = PlayerId(uid);
        region
            .players
            .contains_key(&id)
            .then_some(Target::Player(id))
    } else {
        let public = region.entities.public_at(uid)?;
        (!public.removed).then_some(Target::World(uid))
    }
}
