//! `/summon`：官方 `SummonCommand`，支持已建模的生物和部分非生物实体。
//!
//! `/summon`, vanilla `SummonCommand`, for modeled mobs and supported non-mob entities.

use mc_bamboo_blocks::{
    World,
    entity::mob::{
        MobType,
        spawn::{SummonError, summon_mob},
    },
};
use mc_bamboo_command::ParsedCommand;
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        source,
    },
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
    let command_source = source::of(region, id);
    // 不带 `<pos>` 时用执行者自己的位置；`with_pos` 只区分这两棵命令树节点。
    // Without `<pos>`, use the executor's own position; `with_pos` only tells
    // the two command-tree nodes apart.
    let _ = with_pos;
    let position = source::coordinates(parsed, "pos", command_source, command_source.position)?;
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
    if entity == "minecraft:lightning_bolt" {
        spawn_for_execute(region, id, &entity, position)?;
        return Ok(());
    }
    if matches!(
        entity.as_str(),
        "minecraft:spawner_minecart" | "minecraft:command_block_minecart"
    ) {
        return summon_minecart(region, id, &entity, position, nbt);
    }
    let Some(kind) = MobType::by_name(&entity) else {
        return Err(fail("commands.summon.failed"));
    };
    let uid = spawn_mob(region, id, kind, position, nbt.as_ref())?;
    let display = region
        .entities
        .public_at(uid)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.custom_name.clone(),
            _ => None,
        })
        .map_or_else(
            || Component::translate(kind.translation_key(), []),
            Component::trusted,
        );
    feedback::success(
        region,
        id,
        Component::translate("commands.summon.success", [TranslateArg::from(display)]),
        true,
    );
    Ok(())
}

fn spawn_mob(
    region: &mut Region,
    id: PlayerId,
    kind: MobType,
    position: Vec3,
    nbt: Option<&Tag>,
) -> Result<i32, CommandFail> {
    let (result, effects) = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let result = summon_mob(&mut world, kind, position, nbt);
        (result, world.into_effects())
    };
    interaction::send_effects(region, Some(id), &effects);
    match result {
        Ok(uid) => Ok(uid),
        Err(SummonError::InvalidPosition) => Err(fail("commands.summon.invalidPosition")),
        Err(SummonError::Failed(_)) => Err(fail("commands.summon.failed")),
        Err(SummonError::DuplicateUuid) => Err(fail("commands.summon.failed.uuid")),
    }
}

/// `SummonCommand.createEntity(..., true)`：生成并返回实体，不发命令成功反馈。
///
/// `SummonCommand.createEntity(..., true)`: spawn and return the entity without command feedback.
pub(crate) fn spawn_for_execute(
    region: &mut Region,
    source: PlayerId,
    entity_type: &str,
    position: Vec3,
) -> Result<i32, CommandFail> {
    if let Some(kind) = MobType::by_name(entity_type) {
        return spawn_mob(region, source, kind, position, None);
    }
    check_position(position)?;
    let entity = if entity_type == "minecraft:lightning_bolt" {
        mc_bamboo_blocks::entity::lightning::entity(position, false)
    } else {
        read_entity(entity_type, position, None)?
    };
    let (mut level, ticks) = region.block_level();
    Ok(World::new(&mut level, ticks).spawn_entity(entity))
}

fn summon_minecart(
    region: &mut Region,
    id: PlayerId,
    entity_type: &str,
    position: Vec3,
    nbt: Option<Tag>,
) -> Result<(), CommandFail> {
    check_position(position)?;
    let entity = read_entity(entity_type, position, nbt)?;
    let (mut level, ticks) = region.block_level();
    let uid = World::new(&mut level, ticks).spawn_entity(entity);
    let display = region
        .entities
        .public_at(uid)
        .and_then(|public| crate::vehicle_display_name::of(&region.global.scoreboard, public))
        .unwrap_or_else(|| {
            Component::translate(format!("entity.{}", entity_type.replace(':', ".")), [])
        });
    feedback::success(
        region,
        id,
        Component::translate("commands.summon.success", [display.into()]),
        true,
    );
    Ok(())
}

fn check_position(position: Vec3) -> Result<(), CommandFail> {
    if !mc_bamboo_blocks::entity::mob::spawn::is_in_spawnable_bounds(BlockPos::containing(position))
    {
        return Err(fail("commands.summon.invalidPosition"));
    }
    Ok(())
}

fn read_entity(
    entity_type: &str,
    position: Vec3,
    nbt: Option<Tag>,
) -> Result<mc_bamboo_blocks::entity::Entity, CommandFail> {
    let mut fields = match nbt {
        Some(Tag::Compound(fields)) => fields,
        _ => Vec::new(),
    };
    fields.retain(|(key, _)| key != "id" && key != "Pos");
    fields.push(("id".to_owned(), Tag::String(entity_type.to_owned())));
    fields.push((
        "Pos".to_owned(),
        Tag::List(vec![
            Tag::Double(position.x),
            Tag::Double(position.y),
            Tag::Double(position.z),
        ]),
    ));
    mc_bamboo_blocks::archive::read_entity(&Tag::Compound(fields), 0)
        .ok()
        .flatten()
        .map(|entity| entity.into_entity())
        .ok_or_else(|| fail("commands.summon.failed"))
}
