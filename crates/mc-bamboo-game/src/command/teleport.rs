//! `/tp` 和 `/teleport`。相对轴、局部坐标和朝向按官方 `TeleportCommand` 计算。
//!
//! `/tp` and `/teleport`. Relative axes, local coordinates, and facing follow official
//! `TeleportCommand`.

use mc_bamboo_command::{Coordinates, EntityAnchor, ParsedCommand, Rotation, Value, wrap_degrees};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::{
    packet::play::relative,
    text::{Component, TranslateArg},
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select,
    },
    global::Global,
    inbound::PlayerId,
    movement::{self, CommandTeleport},
    player::Player,
    region::Region,
};

const SPAWN_HORIZONTAL: i32 = 30_000_000;
const SPAWN_VERTICAL: i32 = 20_000_000;

pub(crate) fn teleport(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let level = level_of(region, id);
    let (source_pos, source_yaw, source_pitch) = source_pose(region, id)?;
    let targets = if let Some(selector) = entity_arg(parsed, "targets") {
        select::entities(region, id, level, selector)?
    } else {
        vec![id]
    };
    if let Some(selector) = entity_arg(parsed, "destination") {
        let destination = select::one_entity(region, id, level, selector)?;
        teleport_to_entity(region, id, &targets, destination)
    } else {
        let location = parsed
            .get("location")
            .and_then(|value| value.as_vec3())
            .ok_or_else(|| fail("command.failed"))?;
        let absolute = location.position(source_pos, source_yaw, source_pitch);
        let rotation = parsed.get("rotation").and_then(|value| value.as_rotation());
        let facing_entity = entity_arg(parsed, "facingEntity")
            .map(|selector| select::one_entity(region, id, level, selector))
            .transpose()?;
        let facing_anchor = parsed
            .get("facingAnchor")
            .and_then(|value| value.as_entity_anchor())
            .unwrap_or(EntityAnchor::Feet);
        let facing_pos = parsed
            .get("facingLocation")
            .and_then(|value| value.as_vec3())
            .map(|coords| coords.position(source_pos, source_yaw, source_pitch));
        teleport_to_location(
            region,
            id,
            &targets,
            location,
            absolute,
            rotation,
            Facing {
                entity: facing_entity,
                anchor: facing_anchor,
                position: facing_pos,
            },
        )
    }
}

fn teleport_to_entity(
    region: &mut Region,
    source: PlayerId,
    targets: &[PlayerId],
    destination: PlayerId,
) -> Result<(), CommandFail> {
    let (position, yaw, pitch, name) = {
        let player = region
            .players
            .get(&destination)
            .ok_or_else(|| fail("argument.entity.notfound.entity"))?;
        (
            player.position(),
            player.yaw,
            player.pitch,
            player.name.clone(),
        )
    };
    ensure_spawnable(position)?;
    for target in targets {
        move_player(
            region,
            *target,
            position,
            RotationPlan::Absolute { yaw, pitch },
        );
    }
    let message = if targets.len() == 1 {
        Component::translate(
            "commands.teleport.success.entity.single",
            [
                Component::text(player_name(region, targets[0])).into(),
                Component::text(name).into(),
            ],
        )
    } else {
        Component::translate(
            "commands.teleport.success.entity.multiple",
            [
                TranslateArg::from(targets.len() as i32),
                Component::text(name).into(),
            ],
        )
    };
    feedback::success(region, source, message, true);
    Ok(())
}

struct Facing {
    entity: Option<PlayerId>,
    anchor: EntityAnchor,
    position: Option<Vec3>,
}

fn teleport_to_location(
    region: &mut Region,
    source: PlayerId,
    targets: &[PlayerId],
    location: Coordinates,
    absolute: Vec3,
    rotation: Option<Rotation>,
    facing: Facing,
) -> Result<(), CommandFail> {
    ensure_spawnable(absolute)?;
    let specified = rotation.map(|rotation| {
        let (yaw, pitch) = region
            .players
            .get(&source)
            .map(|player| rotation.resolve(player.yaw, player.pitch))
            .unwrap_or((0.0, 0.0));
        (yaw, pitch, rotation.yaw.relative, rotation.pitch.relative)
    });
    for target in targets {
        let plan = if let Some((yaw, pitch)) = facing_after_move(
            region,
            absolute,
            *target,
            facing.entity,
            facing.anchor,
            facing.position,
        ) {
            RotationPlan::Look {
                yaw,
                pitch,
                position_bits: relative_bits(location),
            }
        } else if let Some((yaw, pitch, yaw_relative, pitch_relative)) = specified {
            RotationPlan::Specified {
                yaw,
                pitch,
                yaw_relative,
                pitch_relative,
                position_bits: relative_bits(location),
            }
        } else {
            RotationPlan::Keep {
                position_bits: relative_bits(location),
            }
        };
        move_player(region, *target, absolute, plan);
    }
    let position = format_position(absolute);
    let message = if targets.len() == 1 {
        Component::translate(
            "commands.teleport.success.location.single",
            [
                Component::text(player_name(region, targets[0])).into(),
                position[0].clone().into(),
                position[1].clone().into(),
                position[2].clone().into(),
            ],
        )
    } else {
        Component::translate(
            "commands.teleport.success.location.multiple",
            [
                TranslateArg::from(targets.len() as i32),
                position[0].clone().into(),
                position[1].clone().into(),
                position[2].clone().into(),
            ],
        )
    };
    feedback::success(region, source, message, true);
    Ok(())
}

/// 朝向算进同一次传送包。协议里没有 `ClientboundPlayerLookAtPacket`。
/// 源锚点固定脚底：Bamboo 没有 `/execute anchored`。
///
/// Facing is folded into the same teleport packet. The protocol crate has no
/// `ClientboundPlayerLookAtPacket`. The source anchor is the feet: Bamboo has no
/// `/execute anchored`.
fn facing_after_move(
    region: &Region,
    absolute: Vec3,
    target: PlayerId,
    facing_entity: Option<PlayerId>,
    facing_anchor: EntityAnchor,
    facing_pos: Option<Vec3>,
) -> Option<(f32, f32)> {
    let to = if let Some(entity) = facing_entity {
        // 被看的人就是这次要传的目标时，官方先传送再 `lookAt`，眼睛和脚底都在落点上。
        //
        // When the looked-at player is the target, official teleports first and then
        // `lookAt`, so both anchors sit at the destination.
        if entity == target {
            Some(anchor_at(
                absolute,
                eye_height(region, entity),
                facing_anchor,
            ))
        } else {
            region
                .players
                .get(&entity)
                .map(|player| anchor_point(player, facing_anchor))
        }
    } else {
        facing_pos
    }?;
    Some(look_angles(absolute, to))
}

enum RotationPlan {
    /// 没有写旋转，每个目标保留自己的朝向，相对位是旋转。
    ///
    /// No rotation was written. Each target keeps its own facing, sent as a relative rotation.
    Keep {
        position_bits: i32,
    },
    Specified {
        yaw: f32,
        pitch: f32,
        yaw_relative: bool,
        pitch_relative: bool,
        position_bits: i32,
    },
    Look {
        yaw: f32,
        pitch: f32,
        position_bits: i32,
    },
    Absolute {
        yaw: f32,
        pitch: f32,
    },
}

fn move_player(region: &mut Region, id: PlayerId, absolute: Vec3, plan: RotationPlan) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let current = player.position();
    let (bits, packet_yaw, packet_pitch, stored_yaw, stored_pitch) = match plan {
        RotationPlan::Keep { position_bits } => (
            position_bits | relative::Y_ROT | relative::X_ROT,
            0.0,
            0.0,
            player.yaw,
            player.pitch,
        ),
        RotationPlan::Specified {
            yaw,
            pitch,
            yaw_relative,
            pitch_relative,
            position_bits,
        } => specified_rotation(
            player,
            position_bits,
            yaw,
            pitch,
            yaw_relative,
            pitch_relative,
        ),
        RotationPlan::Look {
            yaw,
            pitch,
            position_bits,
        } => (
            position_bits,
            yaw,
            pitch.clamp(-90.0, 90.0),
            yaw,
            pitch.clamp(-90.0, 90.0),
        ),
        RotationPlan::Absolute { yaw, pitch } => {
            let yaw = wrap_degrees(yaw);
            let pitch = pitch.clamp(-90.0, 90.0);
            (0, yaw, pitch, yaw, pitch)
        }
    };
    let motion = CommandTeleport {
        absolute,
        packet_x: if bits & relative::X != 0 {
            absolute.x - current.x
        } else {
            absolute.x
        },
        packet_y: if bits & relative::Y != 0 {
            absolute.y - current.y
        } else {
            absolute.y
        },
        packet_z: if bits & relative::Z != 0 {
            absolute.z - current.z
        } else {
            absolute.z
        },
        yaw: stored_yaw,
        pitch: stored_pitch,
        packet_yaw,
        packet_pitch,
        relative: bits,
    };
    let tick = region.tick;
    let Region {
        players,
        links,
        player_grid,
        ..
    } = region;
    let Some(player) = players.get_mut(&id) else {
        return;
    };
    movement::teleport_command(player, links, player_grid, tick, motion);
}

fn specified_rotation(
    player: &Player,
    position_bits: i32,
    yaw: f32,
    pitch: f32,
    yaw_relative: bool,
    pitch_relative: bool,
) -> (i32, f32, f32, f32, f32) {
    let mut bits = position_bits;
    let packet_yaw = wrap_degrees(if yaw_relative { yaw - player.yaw } else { yaw });
    let packet_pitch = wrap_degrees(if pitch_relative {
        pitch - player.pitch
    } else {
        pitch
    });
    let stored_yaw = if yaw_relative {
        player.yaw + packet_yaw
    } else {
        packet_yaw
    };
    let stored_pitch = (if pitch_relative {
        player.pitch + packet_pitch
    } else {
        packet_pitch
    })
    .clamp(-90.0, 90.0);
    if yaw_relative {
        bits |= relative::Y_ROT;
    }
    if pitch_relative {
        bits |= relative::X_ROT;
    }
    (bits, packet_yaw, packet_pitch, stored_yaw, stored_pitch)
}

fn relative_bits(coords: Coordinates) -> i32 {
    let mut bits = 0;
    if coords.is_x_relative() {
        bits |= relative::X | relative::DELTA_X;
    }
    if coords.is_y_relative() {
        bits |= relative::Y | relative::DELTA_Y;
    }
    if coords.is_z_relative() {
        bits |= relative::Z | relative::DELTA_Z;
    }
    bits
}

fn look_angles(from: Vec3, to: Vec3) -> (f32, f32) {
    let xd = to.x - from.x;
    let yd = to.y - from.y;
    let zd = to.z - from.z;
    let sd = (xd * xd + zd * zd).sqrt();
    let pi = std::f64::consts::PI as f32;
    let yaw =
        wrap_degrees((f64::atan2(zd, xd) * f64::from(180.0f32) / f64::from(pi)) as f32 - 90.0);
    let pitch = wrap_degrees((-(f64::atan2(yd, sd) * f64::from(180.0f32) / f64::from(pi))) as f32);
    (yaw, pitch)
}

fn anchor_point(player: &Player, anchor: EntityAnchor) -> Vec3 {
    anchor_at(player.position(), player.eye_height(), anchor)
}

fn anchor_at(feet: Vec3, eye_height: f64, anchor: EntityAnchor) -> Vec3 {
    match anchor {
        EntityAnchor::Feet => feet,
        EntityAnchor::Eyes => Vec3::new(feet.x, feet.y + eye_height, feet.z),
    }
}

fn eye_height(region: &Region, id: PlayerId) -> f64 {
    region
        .players
        .get(&id)
        .map(|player| player.eye_height())
        .unwrap_or(1.62)
}

fn ensure_spawnable(position: Vec3) -> Result<(), CommandFail> {
    let pos = BlockPos::containing(position);
    let inside = (-SPAWN_HORIZONTAL..SPAWN_HORIZONTAL).contains(&pos.x)
        && (-SPAWN_HORIZONTAL..SPAWN_HORIZONTAL).contains(&pos.z)
        && (-SPAWN_VERTICAL..SPAWN_VERTICAL).contains(&pos.y);
    if inside {
        Ok(())
    } else {
        Err(fail("commands.teleport.invalidPosition"))
    }
}

fn format_position(position: Vec3) -> [String; 3] {
    [
        format_double(position.x),
        format_double(position.y),
        format_double(position.z),
    ]
}

/// 官方 `String.format(Locale.ROOT, "%f", value)`，固定六位小数。
///
/// Official `String.format(Locale.ROOT, "%f", value)`, six digits after the decimal.
fn format_double(value: f64) -> String {
    format!("{value:.6}")
}

fn entity_arg<'a>(
    parsed: &'a ParsedCommand,
    name: &str,
) -> Option<&'a mc_bamboo_command::EntitySelector> {
    parsed.get(name).and_then(Value::as_entity)
}

fn source_pose(region: &Region, id: PlayerId) -> Result<(Vec3, f32, f32), CommandFail> {
    region
        .players
        .get(&id)
        .map(|player| (player.position(), player.yaw, player.pitch))
        .ok_or_else(|| fail("permissions.requires.player"))
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(0)
}

fn player_name(region: &Region, id: PlayerId) -> String {
    region
        .players
        .get(&id)
        .map(|player| player.name.clone())
        .unwrap_or_default()
}
