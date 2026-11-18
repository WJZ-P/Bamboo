//! `/experience`（`/xp`）：`add` / `set` / `query` × `points` / `levels`。
//!
//! 语法、检查、翻译键和反馈的广播都照官方 `ExperienceCommand` 及
//! `CommandResponseTracker` 的字节码；目标选择后按玩家表顺序改动经验。
//!
//! `/experience` (`/xp`): `add` / `set` / `query` × `points` / `levels`.
//! Grammar, checks, translation keys and feedback follow the official
//! `ExperienceCommand` and `CommandResponseTracker` bytecode.

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        players, select,
    },
    global_port::GlobalPort,
    inbound::PlayerId,
    region::Region,
};

/// `set points` 越界时官方抛的错。
///
/// The official error when `set points` exceeds the current level's maximum.
const SET_POINTS_INVALID: &str = "commands.experience.set.points.invalid";

/// 官方 `Type.addResponse` / `setResponse` 的 six 个翻译键。
///
/// The six translation keys of the official `Type.addResponse` / `setResponse`.
const ADD_POINTS: &str = "commands.experience.add.points.success";
const ADD_LEVELS: &str = "commands.experience.add.levels.success";
const SET_POINTS: &str = "commands.experience.set.points.success";
const SET_LEVELS: &str = "commands.experience.set.levels.success";

pub(crate) fn add_points(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let (targets, amount) = many(region, id, parsed)?;
    for target in &targets {
        if let Some(player) = region.players.get_mut(target) {
            player.give_experience_points(amount);
        }
    }
    respond(region, id, ADD_POINTS, amount, &targets);
    Ok(())
}

pub(crate) fn add_levels(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let (targets, amount) = many(region, id, parsed)?;
    for target in &targets {
        if let Some(player) = region.players.get_mut(target) {
            player.give_experience_levels(amount);
        }
    }
    respond(region, id, ADD_LEVELS, amount, &targets);
    Ok(())
}

pub(crate) fn set_points(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let (targets, amount) = many(region, id, parsed)?;
    // 官方对每个目标单独判定 `amount < getXpNeededForNextLevel()`；只要没有人成功就报错，
    // 部分成功时反馈只数成功的那几个。
    //
    // Official checks `amount < getXpNeededForNextLevel()` per target. It fails
    // only when nobody succeeded; partial success counts just the successes.
    let mut succeeded = Vec::new();
    for target in &targets {
        if let Some(player) = region.players.get_mut(target)
            && amount < player.experience.needed_for_next_level()
        {
            // 官方的 BiPredicate 无论进度有没有变都返回 true。
            // The official BiPredicate returns true even if progress is unchanged.
            let _ = player.set_experience_points(amount);
            succeeded.push(*target);
        }
    }
    if succeeded.is_empty() {
        return Err(fail(SET_POINTS_INVALID));
    }
    respond(region, id, SET_POINTS, amount, &succeeded);
    Ok(())
}

pub(crate) fn set_levels(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let (targets, amount) = many(region, id, parsed)?;
    for target in &targets {
        if let Some(player) = region.players.get_mut(target) {
            player.set_experience_levels(amount);
        }
    }
    respond(region, id, SET_LEVELS, amount, &targets);
    Ok(())
}

pub(crate) fn query_points(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let target = one(region, id, parsed)?;
    let value = region
        .players
        .get(&target)
        .map(|player| player.experience.points_in_level())
        .unwrap_or(0);
    query(
        region,
        id,
        target,
        "commands.experience.query.points",
        value,
    );
    Ok(())
}

pub(crate) fn query_levels(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let target = one(region, id, parsed)?;
    let value = region
        .players
        .get(&target)
        .map(|player| player.experience.level)
        .unwrap_or(0);
    query(
        region,
        id,
        target,
        "commands.experience.query.levels",
        value,
    );
    Ok(())
}

/// `EntityArgument.getPlayers` + `IntegerArgumentType.getInteger`。
fn many(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(Vec<PlayerId>, i32), CommandFail> {
    let selector = parsed
        .get("target")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let amount = parsed
        .get("amount")
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, players::level_of(region, id), selector)?;
    Ok((targets, amount))
}

/// `query` 的两个分支用 `EntityArgument.getPlayer`（单个目标）。
fn one(region: &Region, id: PlayerId, parsed: &ParsedCommand) -> Result<PlayerId, CommandFail> {
    let selector = parsed
        .get("target")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, players::level_of(region, id), selector)?;
    match targets.as_slice() {
        [only] => Ok(*only),
        _ => Err(fail("argument.player.toomany")),
    }
}

/// 官方 `CommandResponseTracker.sendFeedback`：一个目标用 `.single`，多个用 `.multiple`。
///
/// Official `CommandResponseTracker.sendFeedback`: `.single` for one element,
/// `.multiple` for several.
fn respond(region: &mut Region, id: PlayerId, base: &str, amount: i32, targets: &[PlayerId]) {
    let key: &'static str = match (base, targets.len() == 1) {
        (ADD_POINTS, true) => "commands.experience.add.points.success.single",
        (ADD_POINTS, false) => "commands.experience.add.points.success.multiple",
        (ADD_LEVELS, true) => "commands.experience.add.levels.success.single",
        (ADD_LEVELS, false) => "commands.experience.add.levels.success.multiple",
        (SET_POINTS, true) => "commands.experience.set.points.success.single",
        (SET_POINTS, false) => "commands.experience.set.points.success.multiple",
        (SET_LEVELS, true) => "commands.experience.set.levels.success.single",
        (SET_LEVELS, false) => "commands.experience.set.levels.success.multiple",
        _ => return,
    };
    let second = if targets.len() == 1 {
        region
            .players
            .get(&targets[0])
            .map(|player| TranslateArg::from(player.name.as_str()))
            .unwrap_or_else(|| TranslateArg::from(String::new()))
    } else {
        (targets.len() as i32).into()
    };
    feedback::success(
        region,
        id,
        Component::translate(key, [amount.into(), second]),
        true,
    );
}

/// 官方 `queryExperience`：`sendSuccess` 不广播，参数依次是显示名和值。
///
/// Official `queryExperience`: `sendSuccess` without a broadcast, with the
/// display name and the value as arguments.
fn query(region: &mut Region, id: PlayerId, target: PlayerId, key: &'static str, value: i32) {
    let name = region
        .players
        .get(&target)
        .map(|player| TranslateArg::from(player.name.as_str()))
        .unwrap_or_else(|| TranslateArg::from(String::new()));
    feedback::success(
        region,
        id,
        Component::translate(key, [name, value.into()]),
        false,
    );
}

#[cfg(test)]
mod tests {
    use crate::command::test_commands;

    #[test]
    fn every_experience_form_parses_and_the_alias_redirects() {
        let (graph, _) = test_commands();
        for text in [
            "experience add @s 7 points",
            "experience add @s 7 levels",
            "experience add @s 7",
            "experience set @s 3 points",
            "experience set @s 3 levels",
            "experience set @s 3",
            "experience query @s points",
            "experience query @s levels",
            "xp add @s 2 levels",
            "xp set @s 5 points",
            "xp query @s levels",
        ] {
            assert!(graph.parse(text, 2).is_ok(), "{text}");
        }
        assert!(graph.parse("xp query @s", 2).is_err());
        assert!(graph.parse("experience add @s 7 points", 1).is_err());
    }
}
