//! 游戏模式、聊天、名单、踢人和帮助。
//!
//! Game mode, chat, the player list, kicking, and help.

use mc_bamboo_blocks::BlockState;
use mc_bamboo_command::{
    EntitySelector, GameMode as CommandMode, Message, ParsedCommand, Value, permission,
};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::registries::{attribute, attribute_defaults};
use mc_bamboo_protocol::{
    packet::play::{
        AttributeSnapshot, DisguisedChat, GameEvent, GameMode, PlayerAbilities, UpdateAttributes,
    },
    text::{Component, TranslateArg},
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select,
    },
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    player::Player,
    region::{self, Region},
};

use mc_bamboo_data::registries::chat_type;

const FLYING_SPEED: f32 = 0.05;
const WALKING_SPEED: f32 = 0.1;

const PLAIN_ATTRIBUTES: [AttributeSnapshot<'static>; 2] = [
    AttributeSnapshot {
        attribute: attribute::BLOCK_INTERACTION_RANGE,
        base: attribute_defaults::BLOCK_INTERACTION_RANGE,
        modifiers: &[],
    },
    AttributeSnapshot {
        attribute: attribute::ENTITY_INTERACTION_RANGE,
        base: attribute_defaults::ENTITY_INTERACTION_RANGE,
        modifiers: &[],
    },
];

pub(crate) fn gamemode(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let mode = parsed
        .get("gamemode")
        .and_then(Value::as_game_mode)
        .ok_or_else(|| fail("command.failed"))?;
    let level = level_of(region, id);
    let targets = if let Some(selector) = parsed.get("target").and_then(Value::as_entity) {
        select::players(region, id, level, selector)?
    } else {
        vec![id]
    };
    let protocol = protocol_mode(mode);
    for target in targets {
        apply_mode(global, region, id, target, mode, protocol);
    }
    Ok(())
}

pub(crate) fn say(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let message = render(region, id, required_message(parsed, "message")?)?;
    broadcast_chat(region, id, &message, chat_type::SAY_COMMAND, None);
    Ok(())
}

pub(crate) fn me(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let message = render(region, id, required_message(parsed, "action")?)?;
    broadcast_chat(region, id, &message, chat_type::EMOTE_COMMAND, None);
    Ok(())
}

pub(crate) fn msg(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = required_players(parsed, "targets")?;
    let targets = select::players(region, id, level_of(region, id), selector)?;
    let message = render(region, id, required_message(parsed, "message")?)?;
    let source_name = player_name(region, id);
    let sender = Component::text(source_name);
    for target in targets {
        let target_name = Component::text(player_name(region, target));
        send_chat(
            region,
            id,
            &message,
            chat_type::MSG_COMMAND_OUTGOING,
            &sender,
            Some(&target_name),
        );
        send_chat(
            region,
            target,
            &message,
            chat_type::MSG_COMMAND_INCOMING,
            &sender,
            None,
        );
    }
    Ok(())
}

pub(crate) fn list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    // `list` 只有字面量节点，`list uuids` 多一个。
    //
    // `list` is one literal node. `list uuids` adds a second.
    let uuids = parsed.nodes.len() >= 2;
    let entries: Vec<Component> = region
        .players
        .values()
        .map(|player| {
            if uuids {
                Component::translate(
                    "commands.list.nameAndId",
                    [
                        TranslateArg::from(player.name.as_str()),
                        player.uuid.to_string().into(),
                    ],
                )
            } else {
                Component::text(player.name.clone())
            }
        })
        .collect();
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.list.players",
            [
                TranslateArg::from(region.players.len() as i32),
                region.max_players().into(),
                format_list(&entries).into(),
            ],
        ),
        false,
    );
    Ok(())
}

pub(crate) fn kick(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = required_players(parsed, "targets")?;
    let targets = select::players(region, id, level_of(region, id), selector)?;
    let reason = if let Some(message) = parsed.get("reason").and_then(Value::as_message) {
        render(region, id, message)?
    } else {
        Component::translate("multiplayer.disconnect.kicked", [])
    };
    for target in targets {
        let name = Component::text(player_name(region, target));
        region.links.kick(target, &reason);
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.kick.success",
                [name.into(), reason.clone().into()],
            ),
            true,
        );
    }
    Ok(())
}

pub(crate) fn help(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let level = level_of(region, id);
    let graph = &crate::command::commands().graph;
    let (node, prefix) = if let Some(command) = parsed.get("command").and_then(Value::as_str) {
        let Some(node) = graph.parse_last_node(command, level) else {
            return Err(fail("commands.help.failed"));
        };
        (node, format!("/{command} "))
    } else {
        (graph.root(), "/".to_owned())
    };
    for line in graph.smart_usage(node, level) {
        feedback::success(
            region,
            id,
            Component::text(format!("{prefix}{line}")),
            false,
        );
    }
    Ok(())
}

fn apply_mode(
    global: &mut Global,
    region: &mut Region,
    source: PlayerId,
    target: PlayerId,
    mode: CommandMode,
    protocol: GameMode,
) {
    let (near, was_flying, name) = {
        let Some(player) = region.players.get(&target) else {
            return;
        };
        if player.game_mode == protocol {
            return;
        }
        (
            near_ground(&region.chunks, player),
            player.sync.flying,
            player.name.clone(),
        )
    };
    let flying = match protocol {
        GameMode::Spectator => true,
        GameMode::Creative => was_flying && !near,
        GameMode::Survival | GameMode::Adventure => false,
    };
    if let Some(player) = region.players.get_mut(&target) {
        player.game_mode = protocol;
        player.sync.flying = flying;
    }
    let abilities = PlayerAbilities {
        invulnerable: matches!(protocol, GameMode::Creative | GameMode::Spectator),
        flying,
        can_fly: matches!(protocol, GameMode::Creative | GameMode::Spectator),
        instant_build: protocol == GameMode::Creative,
        flying_speed: FLYING_SPEED,
        walking_speed: WALKING_SPEED,
    };
    // 官方 `setGameMode` 先 `onUpdateAbilities`，再发 `CHANGE_GAME_MODE`，然后再发一次能力。
    //
    // Official `setGameMode` calls `onUpdateAbilities`, then `CHANGE_GAME_MODE`, then abilities again.
    region.links.send(target, &abilities);
    region.links.send(
        target,
        &GameEvent {
            event: GameEvent::CHANGE_GAME_MODE,
            param: mode.id() as f32,
        },
    );
    region.links.send(target, &abilities);
    let attributes: &[AttributeSnapshot<'_>] = if protocol == GameMode::Creative {
        &region::CREATIVE_ATTRIBUTES
    } else {
        &PLAIN_ATTRIBUTES
    };
    region.links.send(
        target,
        &UpdateAttributes {
            entity_id: target.0,
            attributes,
        },
    );
    if let Some(frames) = global.set_game_mode(target, protocol) {
        region.links.broadcast(&frames);
    }
    let mode_name = Component::translate(format!("gameMode.{}", mode.name()), []);
    if source == target {
        feedback::success(
            region,
            source,
            Component::translate("commands.gamemode.success.self", [mode_name.into()]),
            true,
        );
    } else {
        let changed = Component::translate("gameMode.changed", [mode_name.clone().into()]);
        region.links.send(
            target,
            &mc_bamboo_protocol::packet::play::SystemChat {
                content: &changed,
                overlay: false,
            },
        );
        feedback::success(
            region,
            source,
            Component::translate(
                "commands.gamemode.success.other",
                [Component::text(name).into(), mode_name.into()],
            ),
            true,
        );
    }
}

/// 官方 `Entity.isInRangeOfGround` 的近似：已经着地，或者脚下一格不是空气。
///
/// An approximation of official `Entity.isInRangeOfGround`: already on the ground, or the
/// block under the feet is not air.
fn near_ground(chunks: &mc_bamboo_world::store::ChunkStore, player: &Player) -> bool {
    if player.on_ground {
        return true;
    }
    let below = BlockPos::containing(player.position()).below();
    chunks
        .block(below)
        .is_some_and(|state| !BlockState(state).is_air())
}

fn render(region: &Region, source: PlayerId, message: &Message) -> Result<Component, CommandFail> {
    if message.parts.is_empty() {
        return Ok(Component::text(message.text.clone()));
    }
    let chars: Vec<char> = message.text.chars().collect();
    let mut text = String::new();
    let mut cursor = 0;
    let level = level_of(region, source);
    for part in &message.parts {
        let start = part.start.min(chars.len());
        if start > cursor {
            text.extend(chars[cursor..start].iter().copied());
        }
        let ids = select::mention(region, source, level, &part.selector)?;
        let names: Vec<String> = ids.iter().map(|id| player_name(region, *id)).collect();
        text.push_str(&names.join(", "));
        cursor = part.end.min(chars.len());
    }
    if cursor < chars.len() {
        text.extend(chars[cursor..].iter().copied());
    }
    Ok(Component::text(text))
}

fn broadcast_chat(
    region: &mut Region,
    sender: PlayerId,
    message: &Component,
    chat_type: i32,
    target: Option<&Component>,
) {
    let name = Component::text(player_name(region, sender));
    let frames = encode_frames(&DisguisedChat {
        message,
        chat_type,
        name: &name,
        target_name: target,
    });
    region.links.broadcast(&frames);
}

fn send_chat(
    region: &mut Region,
    to: PlayerId,
    message: &Component,
    chat_type: i32,
    name: &Component,
    target: Option<&Component>,
) {
    region.links.send(
        to,
        &DisguisedChat {
            message,
            chat_type,
            name,
            target_name: target,
        },
    );
}

/// 官方 `ComponentUtils.formatList`：各项之间是 `", "`。
///
/// Official `ComponentUtils.formatList`: entries are separated by `", "`.
fn format_list(entries: &[Component]) -> Component {
    let mut list = Component::text("");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            list = list.extra(Component::text(", "));
        }
        list = list.extra(entry.clone());
    }
    list
}

fn protocol_mode(mode: CommandMode) -> GameMode {
    match mode {
        CommandMode::Survival => GameMode::Survival,
        CommandMode::Creative => GameMode::Creative,
        CommandMode::Adventure => GameMode::Adventure,
        CommandMode::Spectator => GameMode::Spectator,
    }
}

fn required_message<'a>(parsed: &'a ParsedCommand, name: &str) -> Result<&'a Message, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_message)
        .ok_or_else(|| fail("command.failed"))
}

fn required_players<'a>(
    parsed: &'a ParsedCommand,
    name: &str,
) -> Result<&'a EntitySelector, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL)
}

fn player_name(region: &Region, id: PlayerId) -> String {
    region
        .players
        .get(&id)
        .map(|player| player.name.clone())
        .unwrap_or_default()
}
