//! 命令的分发、命令树和补全。
//!
//! 解析在 `mc-bamboo-command`。这里按官方把解析结果做成世界状态的改动，
//! 并在进服时把玩家用得了的命令树发给客户端。
//!
//! Command dispatch, the command tree, and suggestions.
//!
//! Parsing lives in `mc-bamboo-command`. This module turns a parsed command into world
//! changes and sends each player the command tree they can use when they join.

pub(crate) use players::change_own_mode;

mod blocks;
mod catalog;
mod feedback;
mod gamerule;
mod items;
mod players;
mod scoreboard;
mod select;
mod teleport;
mod time;

use std::sync::OnceLock;

use mc_bamboo_command::{
    ArgumentType, CommandGraph, NodeKind, ParsedCommand, StringKind, permission,
};
use mc_bamboo_protocol::packet::play::{
    ArgumentNode, ArgumentProperties, CommandNode, CommandNodeKind, CommandSuggestions,
    CommandTree, EntityEvent, PERMISSION_LEVEL_ALL, SignedCommand, StringProperty, Suggestion,
    SystemChat,
};

use crate::{
    chat, command::feedback::CommandFail, global::Global, inbound::PlayerId, links::Links,
    region::Region, secure_chat,
};

/// 一个可执行节点对应的函数。下标是命令树节点的 ID。
///
/// The function for one executable node. The index is the command-tree node id.
pub(crate) type CommandFn =
    fn(&mut Global, &mut Region, PlayerId, &ParsedCommand) -> Result<(), CommandFail>;

pub(crate) struct Commands {
    pub(crate) graph: CommandGraph,
    actions: Vec<Option<CommandFn>>,
    /// 和 `actions` 对齐。`/gamerule` 的节点记下规则下标，别的命令是 `None`。
    ///
    /// Parallel to `actions`. `/gamerule` nodes store the rule index; other commands are `None`.
    pub(crate) rules: Vec<Option<u16>>,
}

fn reject_unsigned_signable(region: &mut Region, id: PlayerId) {
    let content = mc_bamboo_protocol::text::Component::translate(
        "chat.disabled.invalid_command_signature",
        [],
    )
    .color(mc_bamboo_protocol::text::Color::Red);
    region.links.send(
        id,
        &SystemChat {
            content: &content,
            overlay: false,
        },
    );
}

pub(crate) fn commands() -> &'static Commands {
    static COMMANDS: OnceLock<Commands> = OnceLock::new();
    COMMANDS.get_or_init(catalog::build)
}

/// 进服时告诉客户端权限等级，并发送这名玩家用得了的命令树。
///
/// 官方 `PlayerList.sendPlayerPermissionLevel` 紧挨在 `player_abilities` 后面、传送前面。
///
/// On join, tell the client its permission level and send the command tree it can use.
///
/// Official `PlayerList.sendPlayerPermissionLevel` sits right after `player_abilities` and before the teleport.
pub(crate) fn send_session(links: &mut Links, id: PlayerId, level: u8, entity_id: i32) {
    let level = level.min(permission::OWNERS);
    links.send(
        id,
        &EntityEvent {
            entity_id,
            event: PERMISSION_LEVEL_ALL + level,
        },
    );
    let owned = filter_tree(&commands().graph, level);
    let nodes: Vec<CommandNode<'_>> = owned.iter().map(view_node).collect();
    links.send(
        id,
        &CommandTree {
            nodes: &nodes,
            root_index: 0,
        },
    );
}

/// 官方 `chat_command` / `chat_command_signed`。
///
/// Official `chat_command` / `chat_command_signed`.
pub(crate) fn on_command(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    command: &str,
    signed: Option<&SignedCommand>,
) {
    if let Some(signed) = signed
        && !secure_chat::apply_last_seen(region, id, &signed.last_seen)
    {
        return;
    }
    if chat::reject_illegal_command(region, id, command) {
        return;
    }
    let level = level_of(region, id);
    match commands().graph.parse(command, level) {
        Ok(parsed) => {
            if signed.is_none()
                && secure_chat::enforces(region)
                && secure_chat::has_signable_argument(&parsed.arguments)
            {
                reject_unsigned_signable(region, id);
            } else if signed
                .is_some_and(|signed| !secure_chat::collect_command(region, id, &parsed, signed))
            {
                // 签名对不上时系统消息已经发出去了，命令不执行。
                //
                // A signature mismatch already sent the system message. The command does not run.
            } else {
                let action = commands()
                    .actions
                    .get(parsed.action)
                    .and_then(|action| *action);
                if let Some(action) = action {
                    if let Err(failure) = action(global, region, id, &parsed) {
                        feedback::report(region, id, command, &failure);
                    }
                } else {
                    feedback::report(region, id, command, &feedback::fail("command.failed"));
                }
            }
        }
        Err(failure) => {
            // 官方 `performSignedChatCommand` 先对解析结果 `collectSignedArguments`，
            // 对不上才决定要不要执行。解析失败也一样，不能跳过。
            //
            // Official `performSignedChatCommand` calls `collectSignedArguments` on the
            // parse result before it decides whether to run the command. A failed parse
            // still goes through that step.
            let expected = commands().graph.signable_messages(command, level);
            if let Some(signed) = signed {
                if secure_chat::collect_arguments(region, id, &expected, signed) {
                    feedback::report(
                        region,
                        id,
                        command,
                        &feedback::fail_parse(command, &failure.error, failure.cursor),
                    );
                }
            } else if secure_chat::enforces(region) && !expected.is_empty() {
                reject_unsigned_signable(region, id);
            } else {
                feedback::report(
                    region,
                    id,
                    command,
                    &feedback::fail_parse(command, &failure.error, failure.cursor),
                );
            }
        }
    }
    chat::detect_command_rate_spam(region, id);
}

/// 官方补全。协议里没有光标，光标算在字符串末尾。最多 1000 条。
///
/// Official suggestions. The packet has no cursor, so the cursor is the end of the string.
/// At most 1000 entries.
pub(crate) fn on_suggestion(region: &mut Region, id: PlayerId, transaction: i32, command: &str) {
    let level = level_of(region, id);
    let (body, slash) = command
        .strip_prefix('/')
        .map(|body| (body, 1))
        .unwrap_or((command, 0));
    let cursor = body.chars().count();
    let list = {
        let names: Vec<&str> = region
            .players
            .values()
            .map(|player| player.name.as_str())
            .collect();
        commands().graph.suggestions(body, cursor, level, &names)
    };
    let values: Vec<Suggestion<'_>> = list
        .values
        .iter()
        .take(1000)
        .map(|text| Suggestion {
            text,
            tooltip: None,
        })
        .collect();
    region.links.send(
        id,
        &CommandSuggestions {
            id: transaction,
            start: (list.start + slash) as i32,
            length: list.length as i32,
            suggestions: &values,
        },
    );
}

struct OwnedNode {
    kind: OwnedKind,
    executable: bool,
    redirect: Option<i32>,
    restricted: bool,
    children: Vec<i32>,
}

enum OwnedKind {
    Root,
    Literal(String),
    Argument {
        name: String,
        parser: i32,
        properties: ArgumentProperties,
        suggestion: Option<&'static str>,
    },
}

fn filter_tree(graph: &CommandGraph, level: u8) -> Vec<OwnedNode> {
    let mut owned = Vec::new();
    let mut map = vec![None; graph.nodes().len()];
    walk(graph, graph.root(), level, &mut map, &mut owned);
    for node in &mut owned {
        if let Some(target) = node.redirect {
            node.redirect = map.get(target as usize).copied().flatten();
        }
    }
    owned
}

fn walk(
    graph: &CommandGraph,
    id: usize,
    level: u8,
    map: &mut [Option<i32>],
    owned: &mut Vec<OwnedNode>,
) -> Option<i32> {
    if !graph.can_use(id, level) {
        return None;
    }
    let index = owned.len() as i32;
    map[id] = Some(index);
    let node = graph.node(id);
    owned.push(OwnedNode {
        kind: owned_kind(node),
        executable: node.executable,
        redirect: node.redirect.map(|target| target as i32),
        restricted: node.requires > 0,
        children: Vec::new(),
    });
    let children: Vec<usize> = node.children.clone();
    let mut child_indexes = Vec::new();
    for child in children {
        if let Some(child_index) = walk(graph, child, level, map, owned) {
            child_indexes.push(child_index);
        }
    }
    owned[index as usize].children = child_indexes;
    Some(index)
}

fn owned_kind(node: &mc_bamboo_command::Node) -> OwnedKind {
    match &node.kind {
        NodeKind::Root => OwnedKind::Root,
        NodeKind::Literal(literal) => OwnedKind::Literal(literal.clone()),
        NodeKind::Argument { name, kind } => OwnedKind::Argument {
            name: name.clone(),
            parser: kind.parser_id(),
            properties: argument_properties(kind),
            suggestion: node.suggestion,
        },
    }
}

fn argument_properties(kind: &ArgumentType) -> ArgumentProperties {
    match *kind {
        ArgumentType::Integer { min, max } => ArgumentProperties::Integer { min, max },
        ArgumentType::Long { min, max } => ArgumentProperties::Long { min, max },
        ArgumentType::Float { min, max } => ArgumentProperties::Float { min, max },
        ArgumentType::Double { min, max } => ArgumentProperties::Double { min, max },
        ArgumentType::String(StringKind::SingleWord) => {
            ArgumentProperties::String(StringProperty::SingleWord)
        }
        ArgumentType::String(StringKind::Quotable) => {
            ArgumentProperties::String(StringProperty::QuotablePhrase)
        }
        ArgumentType::String(StringKind::Greedy) => {
            ArgumentProperties::String(StringProperty::GreedyPhrase)
        }
        ArgumentType::Entity {
            single,
            players_only,
        } => ArgumentProperties::Entity {
            single,
            players_only,
        },
        ArgumentType::Time { minimum } => ArgumentProperties::Time { minimum },
        ArgumentType::Resource { registry } => ArgumentProperties::Registry(registry),
        ArgumentType::ScoreHolder { multiple } => ArgumentProperties::ScoreHolder { multiple },
        ArgumentType::Bool
        | ArgumentType::GameMode
        | ArgumentType::EntityAnchor
        | ArgumentType::BlockPos
        | ArgumentType::Vec3
        | ArgumentType::Rotation
        | ArgumentType::BlockState
        | ArgumentType::BlockPredicate
        | ArgumentType::Item
        | ArgumentType::ItemPredicate
        | ArgumentType::Message
        | ArgumentType::ResourceLocation
        | ArgumentType::Objective
        | ArgumentType::Team
        | ArgumentType::Criteria
        | ArgumentType::Operation
        | ArgumentType::DisplaySlot
        | ArgumentType::Component
        | ArgumentType::Style
        | ArgumentType::TeamColor => ArgumentProperties::None,
    }
}

fn view_node(node: &OwnedNode) -> CommandNode<'_> {
    let kind = match &node.kind {
        OwnedKind::Root => CommandNodeKind::Root,
        OwnedKind::Literal(literal) => CommandNodeKind::Literal(literal),
        OwnedKind::Argument {
            name,
            parser,
            properties,
            suggestion,
        } => CommandNodeKind::Argument(ArgumentNode {
            name,
            parser: *parser,
            properties: properties.clone(),
            suggestion: *suggestion,
        }),
    };
    CommandNode {
        kind,
        executable: node.executable,
        redirect: node.redirect,
        restricted: node.restricted,
        children: &node.children,
    }
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::BlockState;
    use mc_bamboo_core::{BlockPos, Vec3};
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use mc_bamboo_world::flat;
    use uuid::Uuid;

    use super::*;
    use crate::{global::Global, inbound::PlayerId, player::Player, region::Region};

    fn world_with(name: &str, level: u8) -> (Global, Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            name.to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(8.5, f64::from(flat::surface_y() + 1), 8.5),
        );
        player.permission_level = level;
        player.is_op = level > 0;
        region.insert_player(player);
        (Global::default(), region, id)
    }

    #[test]
    fn permission_zero_parses_list_and_rejects_selectors() {
        let commands = commands();
        assert!(commands.graph.parse("list", 0).is_ok());
        let error = commands.graph.parse("msg @a hi", 0).unwrap_err();
        assert_eq!(error.error.key, "argument.entity.selector.not_allowed");
        assert!(commands.graph.parse("gamemode creative", 0).is_err());
    }

    #[test]
    fn setblock_places_stone_above_the_surface() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        let y = flat::surface_y() + 1;
        on_command(
            &mut global,
            &mut region,
            id,
            &format!("setblock 8 {y} 8 stone"),
            None,
        );
        assert_eq!(
            region.chunks.block(BlockPos::new(8, y, 8)),
            Some(BlockState(blocks::STONE).0)
        );
    }

    #[test]
    fn time_set_keeps_partial_when_already_there_and_clears_it_otherwise() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        global
            .clock_named_mut("minecraft:overworld")
            .expect("overworld clock")
            .partial_tick = 0.5;
        on_command(&mut global, &mut region, id, "time set 6000", None);
        assert_eq!(global.clock().total_ticks, 6000);
        assert_eq!(global.clock().partial_tick, 0.5);
        on_command(&mut global, &mut region, id, "time set 1000", None);
        assert_eq!(global.clock().total_ticks, 1000);
        assert_eq!(global.clock().partial_tick, 0.0);
    }

    #[test]
    fn gamemode_survival_clears_creative_and_give_adds_stone() {
        let (mut global, mut region, id) = world_with("Kitten", 2);
        on_command(&mut global, &mut region, id, "gamemode survival", None);
        assert_eq!(region.players[&id].game_mode, GameMode::Survival);
        on_command(&mut global, &mut region, id, "give Kitten stone 3", None);
        let count: i32 = region.players[&id]
            .inventory
            .slots
            .iter()
            .filter(|stack| stack.item == mc_bamboo_data::items::STONE)
            .map(|stack| stack.count)
            .sum();
        assert_eq!(count, 3);
    }
}
