//! `/particle`：给一定范围内的玩家发一个等级粒子包。
//!
//! 官方 `ParticleCommand.sendParticles` 对每个目标调 `ServerLevel.sendParticles`，
//! 只有玩家所在方块中心到粒子位置小于 32 格（force 时 512 格）才发包；
//! 一个都没发到报 `commands.particle.failed`，否则报 `commands.particle.success`。

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::{
    packet::play::{LevelParticles, Randomization},
    text::Component,
};

use crate::{
    command::{
        effects,
        feedback::{CommandFail, fail},
        text::level_of,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 官方 `/particle` 的 `delta` 是 `vec3(false)`；空参数用零向量。
fn delta(region: &Region, id: PlayerId, parsed: &ParsedCommand) -> Result<Vec3, CommandFail> {
    effects::coordinates(region, id, parsed, "delta", Vec3::new(0.0, 0.0, 0.0))
}

pub(crate) fn run(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    force: bool,
) -> Result<(), CommandFail> {
    let _ = global;
    let particle = parsed
        .get("name")
        .and_then(Value::as_particle)
        .ok_or_else(|| fail("command.failed"))?;
    let position = effects::coordinates(
        region,
        id,
        parsed,
        "pos",
        effects::source_position(region, id),
    )?;
    let spread = delta(region, id, parsed)?;
    let speed = optional_float(parsed, "speed", 0.0)?;
    let count = optional_int(parsed, "count", 0)?;
    let viewers = match parsed.get("viewers") {
        Some(_) => {
            let selector = parsed
                .get("viewers")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            crate::command::select::players(region, id, level_of(region, id), selector)?
        }
        None => region.players.keys().copied().collect(),
    };
    // 官方 `ServerLevel.sendParticles(ServerPlayer, ...)` 用方块中心比较距离。
    let limit = if force { 512.0 } else { 32.0 };
    let limit_squared = limit * limit;
    let mut delivered = 0usize;
    let packet = LevelParticles {
        options: &particle.bytes,
        override_limiter: force,
        always_show: false,
        x: position.x,
        y: position.y,
        z: position.z,
        x_dist: spread.x as f32,
        y_dist: spread.y as f32,
        z_dist: spread.z as f32,
        max_speed: speed,
        count,
        randomization: Randomization::Default,
    };
    for viewer in &viewers {
        let Some(player) = region.players.get(viewer) else {
            continue;
        };
        let block = BlockPos::containing(player.position());
        let center = Vec3::new(
            f64::from(block.x) + 0.5,
            f64::from(block.y) + 0.5,
            f64::from(block.z) + 0.5,
        );
        let dx = center.x - position.x;
        let dy = center.y - position.y;
        let dz = center.z - position.z;
        if dx * dx + dy * dy + dz * dz >= limit_squared {
            continue;
        }
        region.links.send(*viewer, &packet);
        delivered += 1;
    }
    if delivered == 0 {
        return Err(fail("commands.particle.failed"));
    }
    crate::command::feedback::success(
        region,
        id,
        Component::translate("commands.particle.success", [particle.name.as_str().into()]),
        true,
    );
    Ok(())
}

fn optional_float(parsed: &ParsedCommand, name: &str, default: f32) -> Result<f32, CommandFail> {
    match parsed.get(name) {
        Some(value) => value.as_float().ok_or_else(|| fail("command.failed")),
        None => Ok(default),
    }
}

fn optional_int(parsed: &ParsedCommand, name: &str, default: i32) -> Result<i32, CommandFail> {
    match parsed.get(name) {
        Some(value) => value.as_integer().ok_or_else(|| fail("command.failed")),
        None => Ok(default),
    }
}

pub(crate) fn normal(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    run(global, region, id, parsed, false)
}

pub(crate) fn force(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    run(global, region, id, parsed, true)
}
