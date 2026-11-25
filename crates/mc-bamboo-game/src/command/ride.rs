//! `/ride`：官方 `RideCommand` 的 `mount` / `dismount`。
//!
//! `mount` 走 `startRiding(vehicle, force=true, emitEvent=true)`：不看潜行、冷却
//! 和已有乘客数，只保留防环与「不能骑玩家」的前置判定。`dismount` 走共用的
//! `Entity.stopRiding`，玩家和方块层的生物乘客从同一份名单里摘掉。
//!
//! `/ride`: vanilla `RideCommand`'s `mount` / `dismount`. Mounting uses
//! `startRiding(vehicle, true, true)`, bypassing the sneak/cooldown/passenger
//! checks but keeping the no-cycle and no-player-vehicle rules. Dismounting
//! uses the shared `Entity.stopRiding` path.

use mc_bamboo_blocks::entity::{EntityUid, FIRST_ENTITY_ID};
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select::{self, Target},
        source,
    },
    global_port::GlobalPort,
    inbound::PlayerId,
    region::Region,
};

fn display(region: &Region, target: Target) -> Option<TranslateArg> {
    source::entity_name(region, target).map(|text| Component::text(text).into())
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(0)
}

fn entity_arg<'a>(
    parsed: &'a ParsedCommand,
    name: &str,
) -> Option<&'a mc_bamboo_command::EntitySelector> {
    parsed.get(name).and_then(Value::as_entity)
}

/// 官方 `RideCommand.register`：`<target> dismount` 或 `<target> mount <vehicle>`。
pub(crate) fn ride(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let level = level_of(region, id);
    let Some(selector) = entity_arg(parsed, "target") else {
        return Err(fail("command.failed"));
    };
    let target = select::one_target(region, id, level, selector)?;
    if let Some(vehicle_selector) = entity_arg(parsed, "vehicle") {
        let vehicle = select::one_target(region, id, level, vehicle_selector)?;
        // 官方 `RideCommand.mount` 先查「已经骑着别的」「目标不能是玩家」「不能成环」。
        if let Target::Player(_) = vehicle {
            return Err(fail("commands.ride.mount.failure.cant_ride_players"));
        }
        let already = match target {
            Target::Player(player) => region
                .players
                .get(&player)
                .is_some_and(|p| p.riding.is_passenger()),
            Target::World(uid) => region
                .entities
                .public_at(uid)
                .is_some_and(|public| public.vehicle.is_some()),
        };
        if already {
            return Err(fail("commands.ride.already_riding"));
        }
        let vehicle_uid = match vehicle {
            Target::World(uid) => uid,
            _ => return Err(fail("commands.ride.mount.failure.cant_ride_players")),
        };
        if would_loop(region, target, vehicle_uid) {
            return Err(fail("commands.ride.mount.failure.loop"));
        }
        if !force_mount(region, target, vehicle_uid) {
            return Err(fail("commands.ride.mount.failure.generic"));
        }
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.ride.mount.success",
                [display(region, target), display(region, vehicle)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
            ),
            true,
        );
    } else {
        let riding = match target {
            Target::Player(player) => region.players.get(&player).and_then(|p| p.riding.vehicle),
            Target::World(uid) => region
                .entities
                .public_at(uid)
                .and_then(|public| public.vehicle),
        };
        let Some(vehicle) = riding else {
            return Err(fail("commands.ride.not_riding"));
        };
        force_dismount(region, target, vehicle);
        let vehicle_name = region
            .entities
            .public_at(vehicle)
            .and_then(|public| source::entity_name(region, Target::World(public.uid)))
            .map(|text| Component::text(text).into());
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.ride.dismount.success",
                [display(region, target), vehicle_name]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
            ),
            true,
        );
    }
    Ok(())
}

/// `Entity.startRiding` 的防环判定：从载具沿 `vehicle` 往上走，遇到目标就拒绝。
fn would_loop(region: &Region, target: Target, vehicle: EntityUid) -> bool {
    let target_uid = match target {
        Target::Player(id) => id.0,
        Target::World(uid) => uid,
    };
    let mut at = Some(vehicle);
    let mut seen = 0;
    while let Some(uid) = at {
        if uid == target_uid {
            return true;
        }
        seen += 1;
        if seen > 64 {
            return true;
        }
        at = region
            .entities
            .public_at(uid)
            .and_then(|public| public.vehicle);
    }
    false
}

/// `startRiding(vehicle, force=true, emitEvent=true)`：跳过 `canRide` /
/// `canAddPassenger`，但两边关系仍从唯一一份 `passengers` 名单建立。
fn force_mount(region: &mut Region, target: Target, vehicle: EntityUid) -> bool {
    if region.entities.public_at(vehicle).is_none() {
        return false;
    }
    match target {
        Target::Player(id) => {
            let Some(player) = region.players.get_mut(&id) else {
                return false;
            };
            if player.riding.is_passenger() {
                crate::riding::stop_riding(region, id);
            }
            let Some(player) = region.players.get_mut(&id) else {
                return false;
            };
            player.riding.vehicle = Some(vehicle);
            player.riding.last_vehicle = Some(vehicle);
            player.riding.boarding_cooldown = 0;
            if let Some(public) = region.entities.public_at(vehicle) {
                player.riding.first_good = public.position;
                player.riding.last_good = public.position;
            }
            let uid = id.0;
            region.entities.with_mut(vehicle, |entity| {
                entity.with_mob(|_, mob| {
                    mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, uid, true);
                })
            });
        }
        Target::World(uid) => {
            if uid < FIRST_ENTITY_ID {
                return false;
            }
            let already = region
                .entities
                .public_at(uid)
                .and_then(|public| public.vehicle);
            if let Some(old) = already {
                crate::riding::entity_stop(region, uid, old);
            }
            region.entities.with_mut(uid, |entity| {
                entity.vehicle = Some(vehicle);
            });
            region.entities.with_mut(vehicle, |entity| {
                entity.with_mob(|_, mob| {
                    mc_bamboo_blocks::entity::riding::add_passenger(
                        &mut mob.passengers,
                        uid,
                        false,
                    );
                })
            });
        }
    }
    true
}

/// `Entity.stopRiding`：玩家按官方 `Avatar.removeVehicle` 找落点，生物乘客停原地。
fn force_dismount(region: &mut Region, target: Target, vehicle: EntityUid) {
    match target {
        Target::Player(id) => crate::riding::stop_riding(region, id),
        Target::World(uid) => {
            crate::riding::entity_stop(region, uid, vehicle);
            region.entities.with_mut(vehicle, |entity| {
                entity.with_mob(|_, mob| mob.passengers.retain(|&p| p != uid));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::{Entity, mob::MobType};
    use mc_bamboo_core::Vec3;

    use crate::{
        command::{on_command, tests::world_with},
        inbound::PlayerId,
        region::Region,
    };

    fn scene() -> (crate::global::Global, Region, PlayerId, i32, i32) {
        let (global, mut region, id) = world_with("Rider", 4);
        let horse = region.entities.spawn(Entity::by_mob_type(
            MobType::Horse,
            Vec3::new(8.5, -60.0, 8.5),
        ));
        let cow = region
            .entities
            .spawn(Entity::cow(Vec3::new(9.5, -60.0, 8.5)));
        (global, region, id, horse, cow)
    }

    /// `/ride <player> mount <horse>` 和 `/ride <player> dismount` 走同一份名单：
    /// 上车不看冷却和乘客数（force），下车后玩家和载具两边都清干净。
    #[test]
    fn ride_mount_and_dismount_share_the_passenger_list() {
        let (mut global, mut region, id, horse, _) = scene();
        on_command(
            &mut global,
            &mut region,
            id,
            "ride @s mount @e[type=horse,limit=1]",
            None,
        );
        assert_eq!(region.players[&id].riding.vehicle, Some(horse));
        assert_eq!(crate::riding::passengers(&region, horse), vec![id.0]);
        assert!(crate::riding::check_consistency(&region).is_empty());

        on_command(&mut global, &mut region, id, "ride @s dismount", None);
        assert_eq!(region.players[&id].riding.vehicle, None);
        assert!(crate::riding::passengers(&region, horse).is_empty());
        assert!(crate::riding::check_consistency(&region).is_empty());
    }

    /// 生物乘客的 `/ride dismount` 同样只改这一份名单和自己的 `vehicle`，不挪位置。
    #[test]
    fn ride_dismount_drops_a_mob_passenger_without_moving_it() {
        let (mut global, mut region, id, horse, cow) = scene();
        region.entities.with_mut(horse, |entity| {
            entity.with_mob(|_, mob| {
                mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, cow, false);
            })
        });
        region.entities.with_mut(cow, |entity| {
            entity.vehicle = Some(horse);
        });
        let before = region.entities.public_at(cow).unwrap().position;
        on_command(
            &mut global,
            &mut region,
            id,
            "ride @e[type=cow,limit=1] dismount",
            None,
        );
        assert_eq!(region.entities.public_at(cow).unwrap().vehicle, None);
        assert_eq!(region.entities.public_at(cow).unwrap().position, before);
        assert!(crate::riding::passengers(&region, horse).is_empty());
    }

    /// 官方 `RideCommand.mount` 拒绝「已经在骑」和「骑玩家」。
    #[test]
    fn ride_mount_reports_the_official_failures() {
        let (mut global, mut region, id, horse, cow) = scene();
        on_command(
            &mut global,
            &mut region,
            id,
            "ride @s mount @e[type=horse,limit=1]",
            None,
        );
        on_command(
            &mut global,
            &mut region,
            id,
            "ride @s mount @e[type=cow,limit=1]",
            None,
        );
        assert_eq!(region.players[&id].riding.vehicle, Some(horse));
        // 上的还是第一只马，没有偷偷换车；牛的名单也是空的。
        assert_eq!(crate::riding::passengers(&region, horse), vec![id.0]);
        assert!(crate::riding::passengers(&region, cow).is_empty());
    }
}
