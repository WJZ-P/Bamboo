//! `ExecuteCommand.createRelationOperations` 查询当前岛上的实体关系。
//!
//! `ExecuteCommand.createRelationOperations` resolves entities in the current island.

use mc_bamboo_blocks::entity::{
    EntityKind, FIRST_ENTITY_ID, KindPublic,
    mob::{MobType, brain::memory::MemoryType},
};
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
        (KindPublic::Boat(boat), "controller") => {
            return by_id(region, *boat.passengers.first()?)
                .filter(|target| matches!(target, Target::Player(_)));
        }
        (KindPublic::Mob(_), "controller") => mob_controller(region, uid)?,
        (KindPublic::Mob(mob), "leasher") => mob.leash_holder?,
        (KindPublic::Mob(mob), "target") => match mob.mob_type {
            MobType::Axolotl
            | MobType::Frog
            | MobType::Breeze
            | MobType::Creaking
            | MobType::Warden => {
                let entity = region.entities.get(uid)?;
                entity
                    .kind
                    .mob()?
                    .brain
                    .as_ref()?
                    .memories
                    .entity(MemoryType::AttackTarget)?
            }
            _ => mob.target?,
        },
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
}

fn mob_controller(region: &Region, uid: i32) -> Option<i32> {
    let entity = region.entities.get(uid)?;
    let mob = entity.kind.mob()?;
    let passenger = mob.first_passenger()?;
    if let Some(player) = region.players.get(&PlayerId(passenger)) {
        if mob.controlling_passenger() != Some(passenger) {
            return None;
        }
        return (mob.mob_type != MobType::Pig
            || [Hand::Main, Hand::Off].into_iter().any(|hand| {
                player.inventory.held(hand).item == mc_bamboo_data::items::CARROT_ON_A_STICK
            }))
        .then_some(passenger);
    }
    // `Mob.getControllingPassenger`：未禁用 AI，且首位生物允许接管。
    // `Mob.getControllingPassenger`: AI is enabled and the first mob can take control.
    let public = region.entities.public_at(passenger)?;
    (!mob.no_ai
        && matches!(&public.kind, KindPublic::Mob(rider) if rider.mob_type.can_control_vehicle()))
    .then_some(passenger)
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

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::Vec3;

    use super::*;

    #[test]
    fn player_and_boat_resolve_both_sides_of_riding() {
        let (_, mut region, id) = crate::command::tests::world_with("Kitten", 2);
        let mut boat = Entity::boat(
            mc_bamboo_data::registries::entity_type::OAK_BOAT,
            Vec3::default(),
        );
        let EntityKind::Boat(state) = &mut boat.kind else {
            unreachable!()
        };
        state.passengers.push(id.0);
        let boat = region.entities.spawn(boat);
        region.players.get_mut(&id).unwrap().riding.vehicle = Some(boat);
        assert_eq!(
            targets(&region, Target::Player(id), "vehicle"),
            [Target::World(boat)]
        );
        assert_eq!(
            targets(&region, Target::World(boat), "passengers"),
            [Target::Player(id)]
        );
        assert_eq!(
            targets(&region, Target::World(boat), "controller"),
            [Target::Player(id)]
        );
    }
}
