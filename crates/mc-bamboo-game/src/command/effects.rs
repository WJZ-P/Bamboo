//! `/playsound` 和 `/particle` 共用的位置与反馈工具。
//!
//! 两个命令都用 `Vec3Argument.vec3()`（没有小数点的绝对整数会居中 +0.5），
//! 成功消息都由 `CommandResponseTracker` 按「只有一个目标 / 多个目标」选翻译键。

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::text::Component;

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        text::level_of,
    },
    inbound::PlayerId,
    region::Region,
};

/// 官方 `CommandSourceStack.getPosition`：执行者脚底的位置。
pub(crate) fn source_position(region: &Region, id: PlayerId) -> Vec3 {
    region
        .players
        .get(&id)
        .map(|player| player.position())
        .unwrap_or(Vec3::new(0.0, 0.0, 0.0))
}

/// 把参数节点里的坐标按执行者位置和朝向算成世界坐标。
pub(crate) fn coordinates(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
    default: Vec3,
) -> Result<Vec3, CommandFail> {
    let Some(value) = parsed.get(name) else {
        return Ok(default);
    };
    let coords = value.as_vec3().ok_or_else(|| fail("command.failed"))?;
    let (yaw, pitch) = region
        .players
        .get(&id)
        .map(|player| (player.yaw, player.pitch))
        .unwrap_or((0.0, 0.0));
    Ok(coords.position(source_position(region, id), yaw, pitch))
}

/// `EntityArgument.getPlayers` 的空结果在 `select::players` 里已经报错。
pub(crate) fn targets(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<Vec<PlayerId>, CommandFail> {
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    crate::command::select::players(region, id, level_of(region, id), selector)
}

/// 官方 `CommandResponseTracker.sendFeedback`：只有一个目标时用 `.single`（参数是显示名），
/// 否则用 `.multiple`（参数是目标数）。`sendSuccess(..., true)` 还会广播给其他 OP。
pub(crate) fn report(
    region: &mut Region,
    source: PlayerId,
    targets: &[PlayerId],
    single: &'static str,
    multiple: &'static str,
    argument: Option<Component>,
) {
    let message = match (targets, argument) {
        ([only], Some(argument)) => {
            let name = display_name(region, *only);
            Component::translate(single, [argument.into(), name.into()])
        }
        ([only], None) => Component::translate(single, [display_name(region, *only).into()]),
        (_, Some(argument)) => {
            Component::translate(multiple, [argument.into(), (targets.len() as i32).into()])
        }
        (_, None) => Component::translate(multiple, [(targets.len() as i32).into()]),
    };
    feedback::success(region, source, message, true);
}

pub(crate) fn display_name(region: &Region, id: PlayerId) -> Component {
    Component::text(
        region
            .players
            .get(&id)
            .map(|player| player.name.clone())
            .unwrap_or_default(),
    )
}
