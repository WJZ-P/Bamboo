//! `/time`。语法和反馈跟官方 `TimeCommand` 一致。
//!
//! 时间标记和时间线是 `Timelines.bootstrap` 里写死的那几条，生成器没有导出。
//!
//! `/time`. The grammar and feedback match official `TimeCommand`.
//!
//! The time markers and timelines are the ones hardcoded in `Timelines.bootstrap`. The
//! generator does not export them.

use mc_bamboo_command::{ParsedCommand, Value, argument::suggest_identifier};
use mc_bamboo_data::registries::world_clock;
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::feedback::{self, CommandFail, fail, fail_with},
    global::{Clock, Global},
    inbound::PlayerId,
    region::Region,
};

const ASK_SERVER: &str = "minecraft:ask_server";
const OVERWORLD: &str = "minecraft:overworld";
const THE_END: &str = "minecraft:the_end";
const DAY_PERIOD: i64 = 24_000;

/// 官方 `Timelines` 里挂在主世界时钟上、能执行的时间标记。
///
/// Time markers from official `Timelines` that can be executed on the overworld clock.
struct Marker {
    name: &'static str,
    ticks: i64,
    /// `show_in_commands`。不显示的标记仍然可以执行。
    ///
    /// `show_in_commands`. Hidden markers can still be executed.
    show: bool,
}

const MARKERS: &[Marker] = &[
    Marker {
        name: "minecraft:day",
        ticks: 1000,
        show: true,
    },
    Marker {
        name: "minecraft:noon",
        ticks: 6000,
        show: true,
    },
    Marker {
        name: "minecraft:night",
        ticks: 13000,
        show: true,
    },
    Marker {
        name: "minecraft:midnight",
        ticks: 18000,
        show: true,
    },
    Marker {
        name: "minecraft:wake_up_from_sleep",
        ticks: 0,
        show: false,
    },
    Marker {
        name: "minecraft:roll_village_siege",
        ticks: 18000,
        show: false,
    },
];

struct Timeline {
    name: &'static str,
    /// `None` 表示没有周期，官方 `Optional.empty()`。
    ///
    /// `None` means no period, official `Optional.empty()`.
    period: Option<i64>,
}

/// 注册表顺序跟官方 `minecraft:timeline` 一致，全部挂在主世界时钟上。
///
/// Registry order matches official `minecraft:timeline`. All of them use the overworld clock.
const TIMELINES: &[Timeline] = &[
    Timeline {
        name: "minecraft:day",
        period: Some(DAY_PERIOD),
    },
    Timeline {
        name: "minecraft:early_game",
        period: None,
    },
    Timeline {
        name: "minecraft:moon",
        period: Some(DAY_PERIOD * 8),
    },
    Timeline {
        name: "minecraft:villager_schedule",
        period: Some(DAY_PERIOD),
    },
];

pub(crate) const SUGGESTION: &str = ASK_SERVER;

pub(crate) fn suggest_markers(arguments: &[(String, Value)], lower: &str, out: &mut Vec<String>) {
    if clock_name(arguments) != OVERWORLD {
        return;
    }
    for marker in MARKERS.iter().filter(|marker| marker.show) {
        suggest_identifier(marker.name, lower, out);
    }
}

pub(crate) fn suggest_timelines(arguments: &[(String, Value)], lower: &str, out: &mut Vec<String>) {
    if clock_name(arguments) != OVERWORLD {
        return;
    }
    for timeline in TIMELINES {
        suggest_identifier(timeline.name, lower, out);
    }
}

pub(crate) fn set_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let ticks = required_time(parsed)?;
    {
        let clock = clock_mut(global, name)?;
        if clock.total_ticks == i64::from(ticks) {
            return Err(fail_with(
                "commands.time.set.already_at_time",
                [name.into(), TranslateArg::from(ticks)],
            ));
        }
        clock.set_total_ticks(i64::from(ticks));
    }
    publish(global, region, name);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.set.absolute",
            [TranslateArg::from(name), ticks.into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn add_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let ticks = required_time(parsed)?;
    let total = {
        let clock = clock_mut(global, name)?;
        clock.add_ticks(i64::from(ticks));
        clock.total_ticks
    };
    publish(global, region, name);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.set.absolute",
            [TranslateArg::from(name), total.into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn set_marker(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let marker_name = required_text(parsed, "timemarker")?;
    let marker = MARKERS.iter().find(|marker| marker.name == marker_name);
    let Some(marker) = marker.filter(|_| name == OVERWORLD) else {
        return Err(fail_with(
            "commands.time.no_time_marker_found",
            [marker_name.into(), name.into()],
        ));
    };
    {
        let clock = clock_mut(global, name)?;
        if occurs_at(clock.total_ticks, marker.ticks) {
            return Err(fail_with(
                "commands.time.set.already_at_time_marker",
                [name.into(), marker_name.into()],
            ));
        }
        clock.set_total_ticks(resolve_marker(clock.total_ticks, marker.ticks));
    }
    publish(global, region, name);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.set.time_marker",
            [TranslateArg::from(name), marker_name.into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn pause(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    set_paused(global, region, id, parsed, true)
}

pub(crate) fn resume(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    set_paused(global, region, id, parsed, false)
}

pub(crate) fn set_rate(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let rate = parsed
        .get("rate")
        .and_then(Value::as_float)
        .ok_or_else(|| fail("command.failed"))?;
    {
        let clock = clock_mut(global, name)?;
        if clock.rate.to_bits() == rate.to_bits() {
            return Err(fail_with(
                "commands.time.rate.already_same",
                [name.into(), TranslateArg::from(rate)],
            ));
        }
        clock.set_rate(rate);
    }
    publish(global, region, name);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.rate",
            [TranslateArg::from(name), rate.into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn query_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let total = clock_ref(global, name)?.total_ticks;
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.query.absolute",
            [TranslateArg::from(name), total.into()],
        ),
        false,
    );
    Ok(())
}

pub(crate) fn query_gametime(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    feedback::success(
        region,
        id,
        Component::translate("commands.time.query.gametime", [global.time().0.into()]),
        false,
    );
    Ok(())
}

pub(crate) fn query_timeline(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let (timeline, total) = timeline_total(global, parsed)?;
    let current = match timeline.period {
        Some(period) => total.rem_euclid(period),
        None => total,
    };
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.query.timeline",
            [TranslateArg::from(timeline.name), current.into()],
        ),
        false,
    );
    Ok(())
}

pub(crate) fn query_repetition(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let (timeline, total) = timeline_total(global, parsed)?;
    let count = timeline
        .period
        .map(|period| (total / period) as i32)
        .unwrap_or(0);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.time.query.timeline.repetitions",
            [
                TranslateArg::from(timeline.name),
                TranslateArg::from(i64::from(count)),
            ],
        ),
        false,
    );
    Ok(())
}

fn timeline_total(
    global: &Global,
    parsed: &ParsedCommand,
) -> Result<(&'static Timeline, i64), CommandFail> {
    let name = clock_name(&parsed.arguments);
    let timeline_name = required_text(parsed, "timeline")?;
    let timeline = TIMELINES
        .iter()
        .find(|timeline| timeline.name == timeline_name)
        .ok_or_else(|| fail("command.failed"))?;
    if name != OVERWORLD {
        return Err(fail_with(
            "commands.time.wrong_timeline_for_clock",
            [timeline.name.into(), name.into()],
        ));
    }
    let total = clock_ref(global, name)?.total_ticks;
    Ok((timeline, total))
}

fn set_paused(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    paused: bool,
) -> Result<(), CommandFail> {
    let name = clock_name(&parsed.arguments);
    {
        let clock = clock_mut(global, name)?;
        if clock.paused == paused {
            let key = if paused {
                "commands.time.pause.already_paused"
            } else {
                "commands.time.pause.already_running"
            };
            return Err(fail_with(key, [TranslateArg::from(name)]));
        }
        clock.set_paused(paused);
    }
    publish(global, region, name);
    let key = if paused {
        "commands.time.pause"
    } else {
        "commands.time.resume"
    };
    feedback::success(
        region,
        id,
        Component::translate(key, [TranslateArg::from(name)]),
        true,
    );
    Ok(())
}

fn publish(global: &Global, region: &mut Region, name: &str) {
    let Some(clock) = global.clock_named(name).copied() else {
        return;
    };
    if name == THE_END {
        region.broadcast_clock(global.time(), world_clock::THE_END, clock);
    } else {
        region.broadcast_time(global.time(), clock);
    }
}

fn clock_name(arguments: &[(String, Value)]) -> &str {
    arguments
        .iter()
        .find(|(name, _)| name == "clock")
        .and_then(|(_, value)| value.as_str())
        .unwrap_or(OVERWORLD)
}

fn clock_mut<'a>(global: &'a mut Global, name: &str) -> Result<&'a mut Clock, CommandFail> {
    global
        .clock_named_mut(name)
        .ok_or_else(|| fail("command.failed"))
}

fn clock_ref<'a>(global: &'a Global, name: &str) -> Result<&'a Clock, CommandFail> {
    global
        .clock_named(name)
        .ok_or_else(|| fail("command.failed"))
}

fn required_time(parsed: &ParsedCommand) -> Result<i32, CommandFail> {
    parsed
        .get("time")
        .and_then(Value::as_time)
        .ok_or_else(|| fail("command.failed"))
}

fn required_text<'a>(parsed: &'a ParsedCommand, name: &str) -> Result<&'a str, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))
}

/// 官方 `ClockTimeMarker.occursAt`，这些标记都有 24000 的周期。
///
/// Official `ClockTimeMarker.occursAt`. These markers all have a period of 24000.
fn occurs_at(total: i64, ticks: i64) -> bool {
    ticks == total.rem_euclid(DAY_PERIOD)
}

/// 官方 `ClockTimeMarker.resolveTimeToMoveTo`。
///
/// Official `ClockTimeMarker.resolveTimeToMoveTo`.
fn resolve_marker(total: i64, ticks: i64) -> i64 {
    let from = total.rem_euclid(DAY_PERIOD);
    let duration = ticks - from;
    let step = if duration > 0 {
        duration
    } else {
        DAY_PERIOD + duration
    };
    total + step
}
