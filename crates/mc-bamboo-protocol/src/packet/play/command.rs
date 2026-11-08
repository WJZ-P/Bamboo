//! 命令树、命令补全和权限等级变化。
//!
//! 命令树的编码规则来自官方的 `ClientboundCommandsPacket`：节点按任意顺序排列，
//! 根节点的下标单独写在最后，子节点和重定向都用下标引用。这里只负责写字节，
//! 树的结构和权限过滤由 `mc-bamboo-game` 决定。

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_i32, write_string, write_varint},
    packet::ClientboundPacket,
    text::Component,
};

/// 节点标志：低两位是节点类型。
const FLAG_EXECUTABLE: u8 = 4;
const FLAG_REDIRECT: u8 = 8;
const FLAG_CUSTOM_SUGGESTIONS: u8 = 16;
/// 无权限的来源用不了这个节点。官方 `ClientboundCommandsPacket.FLAG_RESTRICTED`。
///
/// The node fails for a source with no permissions. Official `ClientboundCommandsPacket.FLAG_RESTRICTED`.
const FLAG_RESTRICTED: u8 = 32;
const TYPE_ROOT: u8 = 0;
const TYPE_LITERAL: u8 = 1;
const TYPE_ARGUMENT: u8 = 2;

/// 权限等级实体事件，值来自官方 `EntityEvent`。
pub const PERMISSION_LEVEL_ALL: u8 = 24;
pub const PERMISSION_LEVEL_MODERATORS: u8 = 25;
pub const PERMISSION_LEVEL_GAMEMASTERS: u8 = 26;
pub const PERMISSION_LEVEL_ADMINS: u8 = 27;
pub const PERMISSION_LEVEL_OWNERS: u8 = 28;

/// 命令树里的一个节点。下标就是它在 [`CommandTree::nodes`] 里的位置。
#[derive(Debug, Clone, PartialEq)]
pub struct CommandNode<'a> {
    pub kind: CommandNodeKind<'a>,
    /// 节点本身能直接执行。
    pub executable: bool,
    /// 重定向目标的下标。
    pub redirect: Option<i32>,
    /// 官方 `NodeInspector.isRestricted`：无权限来源通不过这个节点自己的要求。
    ///
    /// Official `NodeInspector.isRestricted`: a source with no permissions fails this node's own requirement.
    pub restricted: bool,
    pub children: &'a [i32],
}

/// 节点的类型。
#[derive(Debug, Clone, PartialEq)]
pub enum CommandNodeKind<'a> {
    Root,
    Literal(&'a str),
    Argument(ArgumentNode<'a>),
}

/// 参数节点：名字、官方注册表里的参数类型 ID 和序列化属性。
#[derive(Debug, Clone, PartialEq)]
pub struct ArgumentNode<'a> {
    pub name: &'a str,
    /// `minecraft:command_argument_type` 注册表里的 ID，由 `mc-bamboo-data` 导出。
    pub parser: i32,
    pub properties: ArgumentProperties,
    /// 自定义补全的标识符，官方 `suggestionId`。现在的命令都用客户端的默认补全。
    pub suggestion: Option<&'a str>,
}

/// 参数类型跟着 `commands` 包一起发的属性。
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentProperties {
    /// 没有附加内容的类型，比如 `bool`、`block_pos`。
    None,
    Integer {
        min: i32,
        max: i32,
    },
    Long {
        min: i64,
        max: i64,
    },
    Float {
        min: f32,
        max: f32,
    },
    Double {
        min: f64,
        max: f64,
    },
    String(StringProperty),
    Entity {
        single: bool,
        players_only: bool,
    },
    Time {
        minimum: i32,
    },
    /// `minecraft:resource` 附带的注册表键，比如 `minecraft:world_clock`。
    ///
    /// Registry key carried by `minecraft:resource`, such as `minecraft:world_clock`.
    Registry(&'static str),
}

/// 官方 `StringArgumentType.StringType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringProperty {
    SingleWord = 0,
    QuotablePhrase = 1,
    GreedyPhrase = 2,
}

/// 整棵命令树。
#[derive(Debug, Clone, PartialEq)]
pub struct CommandTree<'a> {
    pub nodes: &'a [CommandNode<'a>],
    pub root_index: i32,
}

impl ClientboundPacket for CommandTree<'_> {
    const ID: i32 = clientbound::COMMANDS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.nodes.len() as i32);
        for node in self.nodes {
            write_node(out, node);
        }
        write_varint(out, self.root_index);
    }
}

fn write_node(out: &mut Vec<u8>, node: &CommandNode<'_>) {
    let type_flag = match node.kind {
        CommandNodeKind::Root => TYPE_ROOT,
        CommandNodeKind::Literal(_) => TYPE_LITERAL,
        CommandNodeKind::Argument(_) => TYPE_ARGUMENT,
    };
    let mut flags = type_flag;
    if node.executable {
        flags |= FLAG_EXECUTABLE;
    }
    if node.redirect.is_some() {
        flags |= FLAG_REDIRECT;
    }
    if node.restricted {
        flags |= FLAG_RESTRICTED;
    }
    if matches!(
        &node.kind,
        CommandNodeKind::Argument(ArgumentNode {
            suggestion: Some(_),
            ..
        })
    ) {
        flags |= FLAG_CUSTOM_SUGGESTIONS;
    }

    out.push(flags);
    write_varint(out, node.children.len() as i32);
    for &child in node.children {
        write_varint(out, child);
    }
    if let Some(redirect) = node.redirect {
        write_varint(out, redirect);
    }

    match &node.kind {
        CommandNodeKind::Root => {}
        CommandNodeKind::Literal(literal) => write_string(out, literal),
        CommandNodeKind::Argument(argument) => {
            write_string(out, argument.name);
            write_varint(out, argument.parser);
            write_argument_properties(out, &argument.properties);
            if let Some(suggestion) = argument.suggestion {
                write_string(out, suggestion);
            }
        }
    }
}

fn write_argument_properties(out: &mut Vec<u8>, properties: &ArgumentProperties) {
    match properties {
        ArgumentProperties::None => {}
        // 官方 `ArgumentUtils.createNumberFlags`：第 0 位有最小值，第 1 位有最大值。
        // 缺省端点不写。整数的缺省是 `MIN`/`MAX`，浮点的缺省是 ±`MAX_VALUE`。
        //
        // Official `ArgumentUtils.createNumberFlags`: bit 0 is a minimum, bit 1 is a maximum.
        // A default endpoint is omitted. Integers default to `MIN`/`MAX`, floats to ±`MAX_VALUE`.
        ArgumentProperties::Integer { min, max } => {
            write_number_flags(out, *min != i32::MIN, *max != i32::MAX);
            if *min != i32::MIN {
                write_i32(out, *min);
            }
            if *max != i32::MAX {
                write_i32(out, *max);
            }
        }
        ArgumentProperties::Long { min, max } => {
            write_number_flags(out, *min != i64::MIN, *max != i64::MAX);
            if *min != i64::MIN {
                out.extend_from_slice(&min.to_be_bytes());
            }
            if *max != i64::MAX {
                out.extend_from_slice(&max.to_be_bytes());
            }
        }
        ArgumentProperties::Float { min, max } => {
            write_number_flags(out, *min != -f32::MAX, *max != f32::MAX);
            if *min != -f32::MAX {
                out.extend_from_slice(&min.to_be_bytes());
            }
            if *max != f32::MAX {
                out.extend_from_slice(&max.to_be_bytes());
            }
        }
        ArgumentProperties::Double { min, max } => {
            write_number_flags(out, *min != -f64::MAX, *max != f64::MAX);
            if *min != -f64::MAX {
                out.extend_from_slice(&min.to_be_bytes());
            }
            if *max != f64::MAX {
                out.extend_from_slice(&max.to_be_bytes());
            }
        }
        ArgumentProperties::String(kind) => write_varint(out, *kind as i32),
        ArgumentProperties::Entity {
            single,
            players_only,
        } => {
            // 官方 `EntityArgumentInfo`：第 0 位只选一个，第 1 位只选玩家。
            let flags = u8::from(*single) | u8::from(*players_only) << 1;
            out.push(flags);
        }
        ArgumentProperties::Time { minimum } => write_i32(out, *minimum),
        // 官方 `ResourceArgument.Info.serializeToNetwork` 写注册表键的标识符。
        //
        // Official `ResourceArgument.Info.serializeToNetwork` writes the registry key's identifier.
        ArgumentProperties::Registry(registry) => write_string(out, registry),
    }
}

/// 官方 `ArgumentUtils.createNumberFlags`。
///
/// Official `ArgumentUtils.createNumberFlags`.
fn write_number_flags(out: &mut Vec<u8>, has_min: bool, has_max: bool) {
    let mut flags = 0;
    if has_min {
        flags |= 1;
    }
    if has_max {
        flags |= 2;
    }
    out.push(flags);
}

/// 一条补全建议。
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion<'a> {
    pub text: &'a str,
    pub tooltip: Option<&'a Component>,
}

/// 回复 [`PlayRequest::CommandSuggestion`](crate::packet::play::PlayRequest::CommandSuggestion)。
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSuggestions<'a> {
    /// 请求里的 ID，原样返回。
    pub id: i32,
    /// 被替换的起点，按字符计。
    pub start: i32,
    pub length: i32,
    pub suggestions: &'a [Suggestion<'a>],
}

impl ClientboundPacket for CommandSuggestions<'_> {
    const ID: i32 = clientbound::COMMAND_SUGGESTIONS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.id);
        write_varint(out, self.start);
        write_varint(out, self.length);
        write_varint(out, self.suggestions.len() as i32);
        for suggestion in self.suggestions {
            write_string(out, suggestion.text);
            match suggestion.tooltip {
                Some(tooltip) => {
                    out.push(1);
                    tooltip.write(out);
                }
                None => out.push(0),
            }
        }
    }
}

/// 实体事件。命令系统只用来同步权限等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityEvent {
    pub entity_id: i32,
    pub event: u8,
}

impl ClientboundPacket for EntityEvent {
    const ID: i32 = clientbound::ENTITY_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i32(out, self.entity_id);
        out.push(self.event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn a_small_tree_matches_the_official_layout() {
        let children_of_root = [1];
        let children_of_foo = [2];
        let argument = ArgumentNode {
            name: "x",
            parser: mc_bamboo_data::command_argument_types::argument_type::BRIGADIER_INTEGER,
            properties: ArgumentProperties::Integer { min: 0, max: 10 },
            suggestion: None,
        };
        let nodes = [
            CommandNode {
                kind: CommandNodeKind::Root,
                executable: false,
                redirect: None,
                restricted: false,
                children: &children_of_root,
            },
            CommandNode {
                kind: CommandNodeKind::Literal("foo"),
                executable: true,
                redirect: None,
                restricted: false,
                children: &children_of_foo,
            },
            CommandNode {
                kind: CommandNodeKind::Argument(argument),
                executable: false,
                redirect: None,
                restricted: false,
                children: &[],
            },
        ];
        let tree = CommandTree {
            nodes: &nodes,
            root_index: 0,
        };
        assert_eq!(
            body(&tree),
            hex("03
                 00  01 01
                 05  01 02  03 66 6f 6f
                 02  00  01 78  03  03 00 00 00 00  00 00 00 0a
                 00")
        );
    }

    #[test]
    fn redirect_and_suggestion_set_their_flags() {
        let children = [1];
        let argument = ArgumentNode {
            name: "name",
            parser: mc_bamboo_data::command_argument_types::argument_type::MESSAGE,
            properties: ArgumentProperties::None,
            suggestion: Some("minecraft:player"),
        };
        let nodes = [
            CommandNode {
                kind: CommandNodeKind::Root,
                executable: false,
                redirect: None,
                restricted: false,
                children: &children,
            },
            CommandNode {
                kind: CommandNodeKind::Argument(argument),
                executable: false,
                redirect: Some(0),
                restricted: false,
                children: &[],
            },
        ];
        let tree = CommandTree {
            nodes: &nodes,
            root_index: 0,
        };
        // 标志位：参数（2）、重定向（8）、自定义补全（16）合起来是 0x1a。
        let expected = hex("02
                               00  01 01
                               1a  00  00
                               04 6e 61 6d 65  14
                               10 6d 69 6e 65 63 72 61 66 74 3a 70 6c 61 79 65 72
                               00");
        assert_eq!(body(&tree), expected);
    }

    #[test]
    fn argument_properties_use_the_official_encodings() {
        let cases: [(ArgumentProperties, &str); 10] = [
            (
                ArgumentProperties::String(StringProperty::GreedyPhrase),
                "02",
            ),
            (
                ArgumentProperties::Entity {
                    single: true,
                    players_only: true,
                },
                "03",
            ),
            (ArgumentProperties::Time { minimum: 1 }, "00 00 00 01"),
            (
                ArgumentProperties::Float { min: 0.0, max: 1.0 },
                "03 00 00 00 00 3f 80 00 00",
            ),
            (
                ArgumentProperties::Double { min: 0.0, max: 1.0 },
                "03 00 00 00 00 00 00 00 00 3f f0 00 00 00 00 00 00",
            ),
            (
                ArgumentProperties::Long { min: -1, max: 1 },
                "03 ff ff ff ff ff ff ff ff 00 00 00 00 00 00 00 01",
            ),
            (
                ArgumentProperties::Integer {
                    min: i32::MIN,
                    max: i32::MAX,
                },
                "00",
            ),
            (
                ArgumentProperties::Integer {
                    min: 1,
                    max: i32::MAX,
                },
                "01 00 00 00 01",
            ),
            (ArgumentProperties::None, ""),
            (
                ArgumentProperties::Registry("minecraft:world_clock"),
                "15 6d 69 6e 65 63 72 61 66 74 3a 77 6f 72 6c 64 5f 63 6c 6f 63 6b",
            ),
        ];
        for (properties, expected) in cases {
            let mut out = Vec::new();
            write_argument_properties(&mut out, &properties);
            assert_eq!(out, hex(expected), "{properties:?}");
        }
    }

    #[test]
    fn suggestions_can_carry_a_tooltip() {
        let tooltip = Component::text("tip");
        let suggestions = [
            Suggestion {
                text: "hello",
                tooltip: None,
            },
            Suggestion {
                text: "world",
                tooltip: Some(&tooltip),
            },
        ];
        let packet = CommandSuggestions {
            id: 7,
            start: 1,
            length: 2,
            suggestions: &suggestions,
        };
        // 纯文本组件就是根上的一个 TAG_String。
        let expected =
            hex("07 01 02 02  05 68 65 6c 6c 6f  00  05 77 6f 72 6c 64  01  08 00 03 74 69 70");
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn entity_event_is_an_int_and_a_byte() {
        let packet = EntityEvent {
            entity_id: 42,
            event: PERMISSION_LEVEL_GAMEMASTERS,
        };
        assert_eq!(body(&packet), hex("00 00 00 2a 1a"));
    }
}
