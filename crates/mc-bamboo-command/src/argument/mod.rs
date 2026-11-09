//! 命令参数类型。每种类型对应官方一个 `ArgumentType`。
//!
//! 这里只做解析，不碰网络。参数类型在协议里的数字 ID 由生成器从官方注册表导出，
//! [`ArgumentType::parser_name`] 返回的是官方注册名，网络层按它查 ID。

pub mod block;
pub mod coordinates;
pub mod entity;
pub mod item;
pub mod message;
pub mod primitives;

use crate::{
    ErrorArg, ParseError, Value,
    identifier::Identifier,
    reader::StringReader,
    value::{EntityAnchor, GameMode, GameProfileInput},
};

pub use primitives::StringKind;

/// 命令树里的参数节点类型。
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentType {
    Bool,
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
    String(StringKind),
    /// `EntityArgument`。`single` 是只能选一个，`players_only` 是只能选玩家。
    Entity {
        single: bool,
        players_only: bool,
    },
    /// 官方 `GameProfileArgument`。选择器按玩家解析，普通名字在执行时查档案。
    ///
    /// Official `GameProfileArgument`. Selectors resolve to players, plain names look up profiles.
    GameProfile,
    GameMode,
    EntityAnchor,
    Time {
        minimum: i32,
    },
    BlockPos,
    Vec3,
    Rotation,
    BlockState,
    BlockPredicate,
    Item,
    ItemPredicate,
    Message,
    /// 官方 `IdentifierArgument`。时间标记用它，不查注册表。
    ///
    /// Official `IdentifierArgument`. Time markers use it and do not look up a registry.
    ResourceLocation,
    /// 官方 `ResourceArgument`。`registry` 是注册表键，比如 `minecraft:world_clock`。
    ///
    /// Official `ResourceArgument`. `registry` is the registry key, such as `minecraft:world_clock`.
    Resource {
        registry: &'static str,
    },
}

impl ArgumentType {
    /// 官方注册表里的参数类型名。版本相关的数字 ID 在 `mc-bamboo-data` 里。
    pub const fn parser_name(&self) -> &'static str {
        match self {
            Self::Bool => "brigadier:bool",
            Self::Integer { .. } => "brigadier:integer",
            Self::Long { .. } => "brigadier:long",
            Self::Float { .. } => "brigadier:float",
            Self::Double { .. } => "brigadier:double",
            Self::String(_) => "brigadier:string",
            Self::Entity { .. } => "minecraft:entity",
            Self::GameProfile => "minecraft:game_profile",
            Self::GameMode => "minecraft:gamemode",
            Self::EntityAnchor => "minecraft:entity_anchor",
            Self::Time { .. } => "minecraft:time",
            Self::BlockPos => "minecraft:block_pos",
            Self::Vec3 => "minecraft:vec3",
            Self::Rotation => "minecraft:rotation",
            Self::BlockState => "minecraft:block_state",
            Self::BlockPredicate => "minecraft:block_predicate",
            Self::Item => "minecraft:item_stack",
            Self::ItemPredicate => "minecraft:item_predicate",
            Self::Message => "minecraft:message",
            Self::ResourceLocation => "minecraft:resource_location",
            Self::Resource { .. } => "minecraft:resource",
        }
    }

    /// 官方 `minecraft:command_argument_type` 注册表里的数字 ID。由生成器导出。
    pub const fn parser_id(&self) -> i32 {
        use mc_bamboo_data::command_argument_types::argument_type as registry;
        match self {
            Self::Bool => registry::BRIGADIER_BOOL,
            Self::Integer { .. } => registry::BRIGADIER_INTEGER,
            Self::Long { .. } => registry::BRIGADIER_LONG,
            Self::Float { .. } => registry::BRIGADIER_FLOAT,
            Self::Double { .. } => registry::BRIGADIER_DOUBLE,
            Self::String(_) => registry::BRIGADIER_STRING,
            Self::Entity { .. } => registry::ENTITY,
            Self::GameMode => registry::GAMEMODE,
            Self::EntityAnchor => registry::ENTITY_ANCHOR,
            Self::Time { .. } => registry::TIME,
            Self::BlockPos => registry::BLOCK_POS,
            Self::Vec3 => registry::VEC3,
            Self::Rotation => registry::ROTATION,
            Self::BlockState => registry::BLOCK_STATE,
            Self::BlockPredicate => registry::BLOCK_PREDICATE,
            Self::Item => registry::ITEM_STACK,
            Self::ItemPredicate => registry::ITEM_PREDICATE,
            Self::Message => registry::MESSAGE,
            Self::ResourceLocation => registry::RESOURCE_LOCATION,
            Self::Resource { .. } => registry::RESOURCE,
            Self::GameProfile => registry::GAME_PROFILE,
        }
    }

    /// 按官方规则解析。实体选择器和消息里的 `@` 都允许。
    ///
    /// Parses with the official rules. Entity selectors and `@` inside messages are allowed.
    pub fn parse(&self, reader: &mut StringReader) -> Result<Value, ParseError> {
        self.parse_with(reader, true)
    }

    /// 和 [`parse`](Self::parse) 相同。`allow_selectors` 为假时，实体参数见到 `@` 就失败，
    /// 消息整段当文字，对应权限不足 `COMMANDS_ENTITY_SELECTORS` 的来源。
    ///
    /// Same as [`parse`](Self::parse). When `allow_selectors` is false, an entity argument
    /// fails at `@` and a message stays literal text, matching a source without
    /// `COMMANDS_ENTITY_SELECTORS`.
    pub fn parse_with(
        &self,
        reader: &mut StringReader,
        allow_selectors: bool,
    ) -> Result<Value, ParseError> {
        match self {
            Self::Bool => primitives::parse_bool(reader),
            Self::Integer { min, max } => primitives::parse_integer(reader, *min, *max),
            Self::Long { min, max } => primitives::parse_long(reader, *min, *max),
            Self::Float { min, max } => primitives::parse_float(reader, *min, *max),
            Self::Double { min, max } => primitives::parse_double(reader, *min, *max),
            Self::String(kind) => primitives::parse_string(reader, *kind),
            Self::GameProfile => parse_game_profile(reader, allow_selectors),
            Self::Entity {
                single,
                players_only,
            } => {
                // 官方 `EntitySelectorParser.parse`：不允许选择器时，`@` 处抛
                // `argument.entity.selector.not_allowed`，游标停在 `@` 上。
                //
                // Official `EntitySelectorParser.parse`: without selector permission, `@`
                // throws `argument.entity.selector.not_allowed` and the cursor stays on `@`.
                if !allow_selectors && reader.can_read() && reader.peek() == '@' {
                    return Err(ParseError::new("argument.entity.selector.not_allowed"));
                }
                entity::parse_entity(reader, *single, *players_only)
            }
            Self::GameMode => primitives::parse_game_mode(reader),
            Self::EntityAnchor => parse_entity_anchor(reader),
            Self::Time { minimum } => primitives::parse_time(reader, *minimum),
            Self::BlockPos => coordinates::parse_block_pos(reader),
            Self::Vec3 => coordinates::parse_vec3(reader),
            Self::Rotation => coordinates::parse_rotation(reader),
            Self::BlockState => block::parse_block_state(reader),
            Self::BlockPredicate => block::parse_block_predicate(reader),
            Self::Item => item::parse_item(reader),
            Self::ItemPredicate => item::parse_item_predicate(reader),
            Self::Message => message::parse_message(reader, allow_selectors),
            Self::ResourceLocation => parse_resource_location(reader),
            Self::Resource { registry } => parse_resource(reader, registry),
        }
    }

    /// 补全建议。只实现官方的字面量和玩家名部分，方块和物品的补全以后再加。
    pub fn suggest(&self, lower: &str, player_names: &[&str], out: &mut Vec<String>) {
        match self {
            Self::Bool => suggest_all(&["true", "false"], lower, out),
            Self::GameMode => {
                for mode in GameMode::ALL {
                    suggest_word(mode.name(), lower, out);
                }
            }
            Self::EntityAnchor => {
                for anchor in EntityAnchor::ALL {
                    suggest_word(anchor.name(), lower, out);
                }
            }
            Self::Entity { .. } | Self::GameProfile => {
                for name in player_names {
                    suggest_word(name, lower, out);
                }
            }
            Self::Resource { registry } => {
                let Some(entries) = registry_entries(registry) else {
                    return;
                };
                for entry in entries {
                    suggest_identifier(entry, lower, out);
                }
            }
            _ => {}
        }
    }
}

/// 官方 `GameProfileArgument.parse`。`@` 开头的选择器只允许玩家，普通名字读一个单词，
/// 名字到 UUID 的解析留到执行时（离线 UUID 或 Mojang 查询）。
///
/// Official `GameProfileArgument.parse`. A leading `@` starts a players-only selector;
/// a plain name is one word, and resolving it to a UUID happens at execution time.
fn parse_game_profile(
    reader: &mut StringReader,
    allow_selectors: bool,
) -> Result<Value, ParseError> {
    if reader.can_read() && reader.peek() == '@' {
        if !allow_selectors {
            return Err(ParseError::new("argument.entity.selector.not_allowed"));
        }
        let selector = entity::parse_entity(reader, false, true)?;
        let Value::Entity(selector) = selector else {
            unreachable!("实体参数只会返回 Value::Entity");
        };
        return Ok(Value::GameProfile(GameProfileInput::Selector(selector)));
    }
    let cursor = reader.cursor();
    while reader.can_read() && reader.peek() != ' ' {
        reader.skip();
    }
    Ok(Value::GameProfile(GameProfileInput::Name(
        reader.slice(cursor, reader.cursor()),
    )))
}

/// 官方 `ResourceArgument.parse`：读一个标识符，不在注册表里就报 `argument.resource.not_found`。
///
/// Official `ResourceArgument.parse`: read an identifier, or `argument.resource.not_found`.
fn parse_resource(reader: &mut StringReader, registry: &str) -> Result<Value, ParseError> {
    let name = Identifier::read(reader)?.to_registry_name();
    let found = registry_entries(registry).is_some_and(|entries| entries.contains(&name.as_str()));
    if !found {
        return Err(ParseError::with_args(
            "argument.resource.not_found",
            [ErrorArg::Text(name), ErrorArg::Text(registry.to_owned())],
        ));
    }
    Ok(Value::String(name))
}

/// 官方 `IdentifierArgument.parse`。任何合法标识符都可以，执行时再查。
///
/// Official `IdentifierArgument.parse`. Any valid identifier is accepted; lookup happens later.
fn parse_resource_location(reader: &mut StringReader) -> Result<Value, ParseError> {
    Ok(Value::String(Identifier::read(reader)?.to_registry_name()))
}

fn registry_entries(registry: &str) -> Option<&'static [&'static str]> {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|synced| synced.name == registry)
        .map(|synced| synced.entries)
}

/// 官方 `SharedSuggestionProvider.filterResources`：没有冒号时命名空间和路径都能匹配。
///
/// Official `SharedSuggestionProvider.filterResources`: without a colon, the namespace and the path both match.
pub fn suggest_identifier(id: &str, lower: &str, out: &mut Vec<String>) {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    let matched = if lower.contains(':') {
        matches_sub_str(lower, id)
    } else {
        matches_sub_str(lower, namespace) || matches_sub_str(lower, path)
    };
    if matched {
        out.push(id.to_owned());
    }
}

/// 官方 `EntityAnchorArgument.parse`：读一个不带引号的词，不是 `feet`/`eyes` 时回到词首。
fn parse_entity_anchor(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let name = reader.read_unquoted_string();
    match EntityAnchor::by_name(&name) {
        Some(anchor) => Ok(Value::EntityAnchor(anchor)),
        None => {
            reader.set_cursor(start);
            Err(ParseError::text("argument.anchor.invalid", name))
        }
    }
}

fn suggest_all(candidates: &[&str], lower: &str, out: &mut Vec<String>) {
    for candidate in candidates {
        suggest_word(candidate, lower, out);
    }
}

fn suggest_word(candidate: &str, lower: &str, out: &mut Vec<String>) {
    if matches_sub_str(lower, &candidate.to_lowercase()) {
        out.push(candidate.to_owned());
    }
}

/// 官方 `SharedSuggestionProvider.matchesSubStr`：在任意一个词的开头匹配。
///
/// 分隔符是 `.`、`_` 和 `/`，所以 `oak_` 能匹配 `oak_logs`。
pub fn matches_sub_str(pattern: &str, input: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    for (index, _) in input.char_indices() {
        let at_word_start = index == 0
            || matches!(
                input.as_bytes().get(index - 1),
                Some(b'.') | Some(b'_') | Some(b'/')
            );
        if at_word_start && input[index..].starts_with(pattern) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestion_matching_happens_at_word_starts() {
        assert!(matches_sub_str("sto", "stone"));
        assert!(matches_sub_str("logs", "oak_logs"));
        assert!(matches_sub_str("oak_", "oak_logs"));
        assert!(!matches_sub_str("one", "stone"));
        assert!(matches_sub_str("", "anything"));
    }

    #[test]
    fn parser_ids_come_from_the_official_registry() {
        // 这里的数字是官方注册表的顺序，生成器导出的是同一份数据。
        assert_eq!(ArgumentType::Bool.parser_id(), 0);
        assert_eq!(
            ArgumentType::Entity {
                single: false,
                players_only: false
            }
            .parser_id(),
            6
        );
        assert_eq!(ArgumentType::BlockPos.parser_id(), 8);
        assert_eq!(ArgumentType::Item.parser_id(), 14);
        assert_eq!(ArgumentType::Message.parser_id(), 20);
        assert_eq!(ArgumentType::Rotation.parser_id(), 29);
        assert_eq!(ArgumentType::Time { minimum: 0 }.parser_id(), 43);
        assert_eq!(ArgumentType::ResourceLocation.parser_id(), 36);
        assert_eq!(
            ArgumentType::Resource {
                registry: "minecraft:world_clock"
            }
            .parser_id(),
            46
        );
        assert_eq!(ArgumentType::Item.parser_name(), "minecraft:item_stack");
    }

    #[test]
    fn selectors_are_rejected_without_consuming_the_at_sign() {
        let mut reader = crate::StringReader::new("@a");
        let error = ArgumentType::Entity {
            single: false,
            players_only: true,
        }
        .parse_with(&mut reader, false)
        .unwrap_err();
        assert_eq!(error.key, "argument.entity.selector.not_allowed");
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn entity_anchors_parse_and_report_the_bad_word() {
        assert_eq!(ArgumentType::EntityAnchor.parser_id(), 38);
        assert_eq!(
            ArgumentType::EntityAnchor.parser_name(),
            "minecraft:entity_anchor"
        );

        let mut reader = StringReader::new("eyes rest");
        assert_eq!(
            ArgumentType::EntityAnchor.parse(&mut reader).unwrap(),
            Value::EntityAnchor(crate::EntityAnchor::Eyes)
        );
        assert_eq!(reader.remaining(), " rest");

        let mut reader = StringReader::new("head");
        let error = ArgumentType::EntityAnchor.parse(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.anchor.invalid");
        assert_eq!(error.args, [crate::ErrorArg::Text("head".into())]);
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn game_profiles_parse_names_and_players_only_selectors() {
        assert_eq!(
            ArgumentType::GameProfile.parser_name(),
            "minecraft:game_profile"
        );
        let mut reader = StringReader::new("Kitten some reason");
        assert_eq!(
            ArgumentType::GameProfile.parse(&mut reader).unwrap(),
            Value::GameProfile(GameProfileInput::Name("Kitten".to_owned()))
        );
        assert_eq!(reader.remaining(), " some reason");

        let mut reader = StringReader::new("@a rest");
        let Value::GameProfile(GameProfileInput::Selector(selector)) =
            ArgumentType::GameProfile.parse(&mut reader).unwrap()
        else {
            panic!("选择器应该解析成 GameProfileInput::Selector");
        };
        assert!(!selector.includes_entities);
        assert!(selector.uses_selectors);
        assert_eq!(reader.remaining(), " rest");

        let mut reader = StringReader::new("@e");
        assert_eq!(
            ArgumentType::GameProfile
                .parse(&mut reader)
                .unwrap_err()
                .key,
            "argument.player.entities"
        );
    }

    #[test]
    fn suggestions_filter_by_prefix() {
        let mut out = Vec::new();
        ArgumentType::GameMode.suggest("cr", &[], &mut out);
        assert_eq!(out, ["creative"]);
        let mut out = Vec::new();
        ArgumentType::Entity {
            single: true,
            players_only: true,
        }
        .suggest("", &["Steve", "Alex"], &mut out);
        assert_eq!(out, ["Steve", "Alex"]);
    }
}
