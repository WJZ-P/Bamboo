//! `/summon`：官方 `SummonCommand`。这一轮只能召唤牛；别的实体类型报
//! `commands.summon.failed`（官方能召唤，Bamboo 还没有这些实体）。
//!
//! `/summon`, vanilla `SummonCommand`. Only cows this round; other entity types fail
//! with `commands.summon.failed` because Bamboo does not have them yet.

use mc_bamboo_blocks::{
    World,
    entity::mob::spawn::{SummonError, summon_cow},
};
use mc_bamboo_command::ParsedCommand;
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::feedback::{self, CommandFail, fail},
    global_port::GlobalPort,
    inbound::PlayerId,
    interaction,
    region::Region,
};

/// 补全的名字，客户端自己按实体类型注册表算（官方 `SuggestionProviders.SUMMONABLE_ENTITIES`）。
pub(crate) const SUGGESTION: &str = "minecraft:summonable_entities";

/// 服务端这边的补全：全部实体类型（客户端用自己的那份过滤可召唤的）。
pub(crate) fn suggest_entities(
    _arguments: &[(String, mc_bamboo_command::Value)],
    lower: &str,
    out: &mut Vec<String>,
) {
    for name in mc_bamboo_data::registries::entity_type::NAMES {
        mc_bamboo_command::argument::suggest_identifier(name, lower, out);
    }
}

/// `/summon <entity>`：在执行者的位置，跑 `finalizeSpawn`。
pub(crate) fn summon_here(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    summon(region, id, parsed, false, false)
}

/// `/summon <entity> <pos>`：跑 `finalizeSpawn`。
pub(crate) fn summon_at(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    summon(region, id, parsed, true, false)
}

/// `/summon <entity> <pos> <nbt>`：不跑 `finalizeSpawn`。
pub(crate) fn summon_with_nbt(
    port: &mut GlobalPort<'_>,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = port;
    summon(region, id, parsed, true, true)
}

fn summon(
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    with_pos: bool,
    with_nbt: bool,
) -> Result<(), CommandFail> {
    let entity = parsed
        .get("entity")
        .and_then(|value| value.as_str())
        .ok_or_else(|| fail("command.failed"))?
        .to_owned();
    let position = {
        let player = region
            .players
            .get(&id)
            .ok_or_else(|| fail("permissions.requires.player"))?;
        match parsed.get("pos").and_then(|value| value.as_vec3()) {
            Some(coords) if with_pos => {
                coords.position(player.position(), player.yaw, player.pitch)
            }
            _ => player.position(),
        }
    };
    let nbt = if with_nbt {
        Some(
            parsed
                .get("nbt")
                .and_then(|value| value.as_nbt())
                .cloned()
                .ok_or_else(|| fail("command.failed"))?,
        )
    } else {
        None
    };
    if entity != "minecraft:cow" {
        return Err(fail("commands.summon.failed"));
    }
    let (result, effects, name) = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let result = summon_cow(&mut world, position, nbt.as_ref());
        let mut name = None;
        if let Ok(uid) = result {
            world.with_entity_mut(uid, &mut |entity| {
                name = entity.kind.mob().and_then(|mob| mob.custom_name.clone());
            });
        }
        let effects = world.into_effects();
        (result, effects, name)
    };
    interaction::send_effects(region, Some(id), &effects);
    match result {
        Ok(_) => {
            // `Entity.getDisplayName()`：有自定义名字时用它，否则是类型名。
            let display = match name {
                Some(tag) => Component::trusted(tag),
                None => Component::translate("entity.minecraft.cow", []),
            };
            feedback::success(
                region,
                id,
                Component::translate("commands.summon.success", [TranslateArg::from(display)]),
                true,
            );
            Ok(())
        }
        Err(SummonError::InvalidPosition) => Err(fail("commands.summon.invalidPosition")),
        Err(SummonError::Failed(_)) => Err(fail("commands.summon.failed")),
        Err(SummonError::DuplicateUuid) => Err(fail("commands.summon.failed.uuid")),
    }
}
