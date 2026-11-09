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
        AttributeSnapshot, ChatVisibility, DisguisedChat, GameEvent, GameMode, PlayerAbilities,
        UpdateAttributes,
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
    let raw = required_message(parsed, "message")?;
    let rendered = render(region, id, raw)?;
    publish_command_chat(
        region,
        id,
        "message",
        &rendered,
        chat_type::SAY_COMMAND,
        None,
    );
    Ok(())
}

pub(crate) fn me(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let raw = required_message(parsed, "action")?;
    let rendered = render(region, id, raw)?;
    publish_command_chat(
        region,
        id,
        "action",
        &rendered,
        chat_type::EMOTE_COMMAND,
        None,
    );
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
    let raw = required_message(parsed, "message")?;
    let rendered = render(region, id, raw)?;
    let source_name = player_name(region, id);
    let sender = Component::text(source_name);
    let signed = crate::secure_chat::take_argument(region, id, "message");
    let unsigned = signed.as_ref().and_then(|message| {
        (message.content != rendered).then(|| Component::text(rendered.clone()))
    });
    for target in targets {
        let target_name = Component::text(player_name(region, target));
        deliver_one(
            region,
            id,
            signed.as_ref(),
            &DirectChat {
                rendered: &rendered,
                unsigned: unsigned.as_ref(),
                chat_type: chat_type::MSG_COMMAND_OUTGOING,
                name: &sender,
                target: Some(&target_name),
            },
        );
        deliver_one(
            region,
            target,
            signed.as_ref(),
            &DirectChat {
                rendered: &rendered,
                unsigned: unsigned.as_ref(),
                chat_type: chat_type::MSG_COMMAND_INCOMING,
                name: &sender,
                target: None,
            },
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
        Component::text(render(region, id, message)?)
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

/// `GameModeCommand.setGameMode`：F3+N / F3+F4 发来的模式。模式没变时没有消息。
///
/// `GameModeCommand.setGameMode` for the mode from F3+N / F3+F4. An unchanged mode sends nothing.
pub(crate) fn change_own_mode(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    protocol: GameMode,
) {
    let mode = match protocol {
        GameMode::Survival => CommandMode::Survival,
        GameMode::Creative => CommandMode::Creative,
        GameMode::Adventure => CommandMode::Adventure,
        GameMode::Spectator => CommandMode::Spectator,
    };
    apply_mode(global, region, id, id, mode, protocol);
}

pub(crate) fn apply_mode(
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
        // `GameModeCommand.logGamemodeChange`：给目标的 `gameMode.changed` 受
        // `GameRules.SEND_COMMAND_FEEDBACK` 限制。
        //
        // `GameModeCommand.logGamemodeChange`: the target's `gameMode.changed` is gated by
        // `GameRules.SEND_COMMAND_FEEDBACK`.
        if region.send_command_feedback {
            let changed = Component::translate("gameMode.changed", [mode_name.clone().into()]);
            region.links.send(
                target,
                &mc_bamboo_protocol::packet::play::SystemChat {
                    content: &changed,
                    overlay: false,
                },
            );
        }
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

fn render(region: &Region, source: PlayerId, message: &Message) -> Result<String, CommandFail> {
    if message.parts.is_empty() {
        return Ok(message.text.clone());
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
    Ok(text)
}

/// `/say` 和 `/me`。发送者不是全零 UUID 时发 `player_chat`，否则发 `disguised_chat`。
///
/// `/say` and `/me`. A sender other than the nil UUID goes out as `player_chat`.
/// A nil sender goes out as `disguised_chat`.
fn publish_command_chat(
    region: &mut Region,
    sender: PlayerId,
    argument: &str,
    rendered: &str,
    chat_type: i32,
    target: Option<&Component>,
) {
    let signed = crate::secure_chat::take_argument(region, sender, argument);
    let speaker = player_name(region, sender);
    let trusted = signed
        .as_ref()
        .is_some_and(|message| message.is_trusted(region.now_millis()));
    crate::chat::log_player_broadcast(trusted, chat_type, &speaker, rendered);
    let name = Component::text(speaker);
    let unsigned = signed
        .as_ref()
        .filter(|message| message.content != rendered)
        .map(|_| Component::text(rendered));
    if let Some(message) = signed.as_ref().filter(|message| !message.sender.is_nil()) {
        crate::secure_chat::broadcast_signed(
            region,
            message,
            chat_type,
            &name,
            target,
            unsigned.as_ref(),
        );
        return;
    }
    let message = Component::text(rendered);
    broadcast_chat(region, sender, &message, chat_type, target);
}

fn broadcast_chat(
    region: &mut Region,
    sender: PlayerId,
    message: &Component,
    chat_type: i32,
    target: Option<&Component>,
) {
    let name = Component::text(player_name(region, sender));
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.client.chat_visibility == ChatVisibility::Full)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        region.links.send(
            id,
            &DisguisedChat {
                message,
                chat_type,
                name: &name,
                target_name: target,
            },
        );
    }
}

struct DirectChat<'a> {
    rendered: &'a str,
    unsigned: Option<&'a Component>,
    chat_type: i32,
    name: &'a Component,
    target: Option<&'a Component>,
}

fn deliver_one(
    region: &mut Region,
    to: PlayerId,
    signed: Option<&crate::secure_chat::SignedMessage>,
    chat: &DirectChat<'_>,
) {
    if let Some(message) = signed.filter(|message| !message.sender.is_nil()) {
        crate::secure_chat::send_signed_to(
            region,
            to,
            message,
            chat.chat_type,
            chat.name,
            chat.target,
            chat.unsigned,
        );
        return;
    }
    let visible = region
        .players
        .get(&to)
        .is_some_and(|player| player.client.chat_visibility == ChatVisibility::Full);
    if !visible {
        return;
    }
    let message = Component::text(chat.rendered);
    send_chat(region, to, &message, chat.chat_type, chat.name, chat.target);
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

pub(crate) fn protocol_mode(mode: CommandMode) -> GameMode {
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
