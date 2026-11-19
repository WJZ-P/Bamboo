//! `/effect give|clear`：官方 `EffectCommands`。
//!
//! `give` 的语法：`<targets> <effect> [seconds 1..1000000 | infinite]
//! [amplifier 0..255] [hideParticles]`；秒数乘 20，缺省 600 刻（瞬间效果 1 刻）。
//! `clear [targets] [effect]`。非玩家实体走生物层的效果入口，玩家走
//! [`crate::player_effects`]。反馈按官方 `CommandResponseTracker` 的单 / 多目标翻译键。
//!
//! `/effect give|clear`, vanilla `EffectCommands`. Seconds become ticks;
//! the default is 600 ticks (1 for instantaneous effects). Non-player mobs
//! use the shared effect core, players use `player_effects`.

use mc_bamboo_blocks::{
    World,
    entity::mob::effects::{EffectInstance, INFINITE, is_instantaneous},
};
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_data::registries::mob_effect;
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

/// 官方 `EffectCommands.computeDurationInTicks`：瞬间效果秒数不乘 20、
/// `-1` 保持无限；没给秒数时瞬间效果 1 刻、其余 600 刻。
///
/// Vanilla `EffectCommands.computeDurationInTicks`.
fn duration(seconds: Option<i32>, effect: i32) -> i32 {
    match seconds {
        Some(-1) => INFINITE,
        Some(seconds) if is_instantaneous(effect) => seconds,
        Some(seconds) => seconds.saturating_mul(20),
        None if is_instantaneous(effect) => 1,
        None => 600,
    }
}

fn effect_id(parsed: &ParsedCommand) -> Result<i32, CommandFail> {
    parsed
        .get("effect")
        .and_then(Value::as_str)
        .and_then(mob_effect::by_name)
        .ok_or_else(|| fail("argument.resource.not_found"))
}

fn display(effect: i32) -> Component {
    Component::translate(
        mob_effect::name_of(effect)
            .unwrap_or("minecraft:speed")
            .replace("minecraft:", "effect.minecraft."),
        [],
    )
}

fn targets(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<Vec<Target>, CommandFail> {
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    select::targets(region, id, source::level(region, id), selector)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the official feedback keys and arguments are all passed separately"
)]
fn report(
    region: &mut Region,
    source: PlayerId,
    successes: usize,
    failed: &'static str,
    single: &'static str,
    multiple: &'static str,
    args: Vec<TranslateArg>,
    name: Option<String>,
) -> Result<(), CommandFail> {
    if successes == 0 {
        return Err(fail(failed));
    }
    let message = if successes == 1 {
        let mut all = args.clone();
        if let Some(name) = name {
            all.push(Component::text(name).into());
        }
        Component::translate(single, all)
    } else {
        let mut all = args;
        all.push(TranslateArg::from(successes as i32));
        Component::translate(multiple, all)
    };
    feedback::success(region, source, message, true);
    Ok(())
}

/// `effect give <targets> <effect> [seconds 1..1000000] [amplifier] [hideParticles]`。
pub(crate) fn give(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    run_give(port, region, source, parsed, false)
}

/// `effect give <targets> <effect> infinite [amplifier] [hideParticles]`。
/// 官方的 `infinite` 分支给 `-1` 秒（`computeDurationInTicks` 直接返回 `-1`）。
pub(crate) fn give_infinite(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    run_give(port, region, source, parsed, true)
}

fn run_give(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
    infinite: bool,
) -> Result<(), CommandFail> {
    let _ = port;
    let effect = effect_id(parsed)?;
    let seconds = parsed.get("seconds").and_then(Value::as_integer);
    let ticks = if infinite {
        INFINITE
    } else {
        duration(seconds, effect)
    };
    let amplifier = parsed
        .get("amplifier")
        .and_then(Value::as_integer)
        .unwrap_or(0)
        .clamp(0, 255);
    let hide = parsed
        .get("hideParticles")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let targets = targets(region, source, parsed)?;
    let mut successes = 0;
    let mut only_name = None;
    for target in &targets {
        let ok = match *target {
            Target::Player(id) => crate::player_effects::give(
                region,
                id,
                EffectInstance::with_flags(effect, ticks, amplifier, false, !hide),
            ),
            Target::World(uid) => {
                let (mut level, ticks_now) = region.block_level();
                let mut world = World::new(&mut level, ticks_now);
                world.add_mob_effect(
                    uid,
                    EffectInstance::with_flags(effect, ticks, amplifier, false, !hide),
                )
            }
        };
        if ok {
            successes += 1;
            only_name = source::entity_name(region, *target);
        }
    }
    report(
        region,
        source,
        successes,
        "commands.effect.give.failed",
        "commands.effect.give.success.single",
        "commands.effect.give.success.multiple",
        vec![display(effect).into(), TranslateArg::from(ticks / 20)],
        only_name,
    )
}

/// `effect clear [targets] [effect]`。
pub(crate) fn clear(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    let effect = match parsed.get("effect") {
        Some(value) => Some(
            value
                .as_str()
                .and_then(mob_effect::by_name)
                .ok_or_else(|| fail("argument.resource.not_found"))?,
        ),
        None => None,
    };
    let targets = match parsed.get("targets") {
        Some(_) => targets(region, source, parsed)?,
        None => vec![Target::Player(source)],
    };
    let mut successes = 0;
    let mut only_name = None;
    for target in &targets {
        let ok = match *target {
            Target::Player(id) => crate::player_effects::clear(region, id, effect),
            Target::World(uid) => {
                let (mut level, ticks_now) = region.block_level();
                let mut world = World::new(&mut level, ticks_now);
                world.clear_mob_effects(uid, effect)
            }
        };
        if ok {
            successes += 1;
            only_name = source::entity_name(region, *target);
        }
    }
    let (failed, single, multiple) = if effect.is_some() {
        (
            "commands.effect.clear.specific.failed",
            "commands.effect.clear.specific.success.single",
            "commands.effect.clear.specific.success.multiple",
        )
    } else {
        (
            "commands.effect.clear.everything.failed",
            "commands.effect.clear.everything.success.single",
            "commands.effect.clear.everything.success.multiple",
        )
    };
    let mut args = Vec::new();
    if let Some(effect) = effect {
        args.push(display(effect).into());
    }
    report(
        region, source, successes, failed, single, multiple, args, only_name,
    )
}
