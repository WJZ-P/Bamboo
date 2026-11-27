//! 命令参数类型。每种类型对应官方一个 `ArgumentType`。
//!
//! 这里只做解析，不碰网络。参数类型在协议里的数字 ID 由生成器从官方注册表导出，
//! [`ArgumentType::parser_name`] 返回的是官方注册名，网络层按它查 ID。

pub mod block;
pub mod component;
pub mod coordinates;
pub mod entity;
pub mod item;
pub mod message;
pub mod particle;
pub mod primitives;
pub mod scoreboard;

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
    /// `Vec3Argument.vec3(false)`：不把绝对整数居中。`/particle` 的 delta 用它。
    Vec3Uncentered,
    /// 官方 `Vec2Argument.vec2()`：两个水平坐标，绝对整数居中。`/worldborder center` 用它。
    ///
    /// Vanilla `Vec2Argument.vec2()`: two horizontal coordinates, centring absolute
    /// integers. `/worldborder center` uses it.
    Vec2,
    Rotation,
    BlockState,
    BlockPredicate,
    Item,
    ItemPredicate,
    Message,
    /// 官方 `ComponentArgument`：一个 SNBT 文本组件。
    Component,
    /// 官方 `ParticleArgument`：粒子类型名，后面可以跟一段 SNBT 选项。
    Particle,
    /// 官方 `IdentifierArgument`。时间标记用它，不查注册表。
    ///
    /// Official `IdentifierArgument`. Time markers use it and do not look up a registry.
    ResourceLocation,
    /// 官方 `DimensionArgument`：任意合法标识符，执行时再查维度。
    ///
    /// Official `DimensionArgument`: any identifier, resolved to a dimension at execution.
    Dimension,
    /// 官方 `RangeArgument.Ints`：`5`、`5..`、`..5` 或 `5..10`。
    ///
    /// Official `RangeArgument.Ints`: `5`, `5..`, `..5` or `5..10`.
    IntRange,
    /// 官方 `LootTableArgument`：战利品表的标识符。
    ///
    /// Official `LootTableArgument`: a loot-table identifier.
    LootTable,
    /// 官方 `SlotArgument`：物品栏槽位名。
    ///
    /// Official `SlotArgument`: an inventory slot name.
    ItemSlot,
    /// 官方 `ResourceArgument`。`registry` 是注册表键，比如 `minecraft:world_clock`。
    ///
    /// Official `ResourceArgument`. `registry` is the registry key, such as `minecraft:world_clock`.
    Resource {
        registry: &'static str,
    },
    /// 官方 `ObjectiveArgument`。执行时才查这个目标在不在。
    ///
    /// Official `ObjectiveArgument`. Whether the objective exists is checked at execution.
    Objective,
    /// 官方 `TeamArgument`。
    ///
    /// Official `TeamArgument`.
    Team,
    /// 官方 `ObjectiveCriteriaArgument`。
    ///
    /// Official `ObjectiveCriteriaArgument`.
    Criteria,
    /// 官方 `OperationArgument`。
    ///
    /// Official `OperationArgument`.
    Operation,
    /// 官方 `ScoreboardSlotArgument`。
    ///
    /// Official `ScoreboardSlotArgument`.
    DisplaySlot,
    /// 官方 `ScoreHolderArgument`。`multiple` 为真时允许 `@a` 和默认的 `*`。
    ///
    /// Official `ScoreHolderArgument`. `multiple` allows `@a` and the default `*`.
    ScoreHolder {
        multiple: bool,
    },
    /// 官方 `StyleArgument`。
    ///
    /// Official `StyleArgument`.
    Style,
    /// 官方 `TeamColorArgument`。
    ///
    /// Official `TeamColorArgument`.
    TeamColor,
    /// 官方 `CompoundTagArgument`：一个 SNBT 复合标签。
    ///
    /// Official `CompoundTagArgument`: one SNBT compound.
    CompoundTag,
    /// 官方 `SwizzleArgument`：`/execute align` 的轴集合（只解析）。
    ///
    /// Official `SwizzleArgument`: the axis set of `/execute align` (parse only).
    Swizzle,
    /// 官方 `NbtPathArgument`：只解析路径文本，执行期再求值。
    ///
    /// Official `NbtPathArgument`: parses the path text; evaluated at run time.
    NbtPath,
    /// 官方 `ResourceOrTagArgument`（本命令树里只用于生物群系）。
    ResourceOrTag {
        registry: &'static str,
    },
    /// 官方 `HeightmapTypeArgument`：只解析名字。
    Heightmap,
    /// 官方 `SlotSourceArgument`：只解析槽位来源文本。
    SlotSource,
    /// 官方 `LootPredicateArgument`：只解析谓词 ID。
    LootPredicate,
    /// 官方 `FloatRangeArgument`：只解析范围文本。
    FloatRange,
    /// 官方 `FunctionArgument`：只解析函数名。
    Function,
    /// 官方 `NbtTagArgument`：任意 SNBT 值。
    NbtTag,
    /// 官方 `ContextIntProvider` / `ContextFloatProvider`：本树只保留文本。
    ContextIntProvider,
    ContextFloatProvider,
    /// 官方 `ItemSlotsArgument`（`/data modify` 的 compute 用不到，树里预留）。
    ItemSlots,
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
            Self::Vec3 | Self::Vec3Uncentered => "minecraft:vec3",
            Self::Vec2 => "minecraft:vec2",
            Self::Rotation => "minecraft:rotation",
            Self::BlockState => "minecraft:block_state",
            Self::BlockPredicate => "minecraft:block_predicate",
            Self::Item => "minecraft:item_stack",
            Self::ItemPredicate => "minecraft:item_predicate",
            Self::Message => "minecraft:message",
            Self::Component => "minecraft:component",
            Self::Particle => "minecraft:particle",
            Self::ResourceLocation => "minecraft:resource_location",
            Self::Resource { .. } => "minecraft:resource",
            Self::Dimension => "minecraft:dimension",
            Self::IntRange => "minecraft:int_range",
            Self::LootTable => "minecraft:loot_table",
            Self::ItemSlot => "minecraft:item_slot",
            Self::Objective => "minecraft:objective",
            Self::Team => "minecraft:team",
            Self::Criteria => "minecraft:objective_criteria",
            Self::Operation => "minecraft:operation",
            Self::DisplaySlot => "minecraft:scoreboard_slot",
            Self::ScoreHolder { .. } => "minecraft:score_holder",
            Self::Style => "minecraft:style",
            Self::TeamColor => "minecraft:team_color",
            Self::CompoundTag => "minecraft:nbt_compound_tag",
            Self::Swizzle => "minecraft:swizzle",
            Self::NbtPath => "minecraft:nbt_path",
            Self::ResourceOrTag { .. } => "minecraft:resource_or_tag",
            Self::Heightmap => "minecraft:heightmap",
            Self::SlotSource => "minecraft:slot_source",
            Self::LootPredicate => "minecraft:loot_predicate",
            Self::FloatRange => "minecraft:float_range",
            Self::Function => "minecraft:function",
            Self::NbtTag => "minecraft:nbt_tag",
            Self::ContextIntProvider => "minecraft:context_int_provider",
            Self::ContextFloatProvider => "minecraft:context_float_provider",
            Self::ItemSlots => "minecraft:item_slots",
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
            Self::Vec3 | Self::Vec3Uncentered => registry::VEC3,
            Self::Vec2 => registry::VEC2,
            Self::Rotation => registry::ROTATION,
            Self::BlockState => registry::BLOCK_STATE,
            Self::BlockPredicate => registry::BLOCK_PREDICATE,
            Self::Item => registry::ITEM_STACK,
            Self::ItemPredicate => registry::ITEM_PREDICATE,
            Self::Message => registry::MESSAGE,
            Self::Component => registry::COMPONENT,
            Self::Particle => registry::PARTICLE,
            Self::ResourceLocation => registry::RESOURCE_LOCATION,
            Self::Resource { .. } => registry::RESOURCE,
            Self::Dimension => registry::DIMENSION,
            Self::IntRange => registry::INT_RANGE,
            Self::LootTable => registry::LOOT_TABLE,
            Self::ItemSlot => registry::ITEM_SLOT,
            Self::GameProfile => registry::GAME_PROFILE,
            Self::Objective => registry::OBJECTIVE,
            Self::Team => registry::TEAM,
            Self::Criteria => registry::OBJECTIVE_CRITERIA,
            Self::Operation => registry::OPERATION,
            Self::DisplaySlot => registry::SCOREBOARD_SLOT,
            Self::ScoreHolder { .. } => registry::SCORE_HOLDER,
            Self::Style => registry::STYLE,
            Self::TeamColor => registry::TEAM_COLOR,
            Self::CompoundTag => registry::NBT_COMPOUND_TAG,
            Self::Swizzle => registry::SWIZZLE,
            Self::NbtPath => registry::NBT_PATH,
            Self::ResourceOrTag { .. } => registry::RESOURCE_OR_TAG,
            Self::Heightmap => registry::HEIGHTMAP,
            Self::SlotSource => registry::SLOT_SOURCE,
            Self::LootPredicate => registry::LOOT_PREDICATE,
            Self::FloatRange => registry::FLOAT_RANGE,
            Self::Function => registry::FUNCTION,
            Self::NbtTag => registry::NBT_TAG,
            Self::ContextIntProvider => registry::CONTEXT_INT_PROVIDER,
            Self::ContextFloatProvider => registry::CONTEXT_FLOAT_PROVIDER,
            Self::ItemSlots => registry::ITEM_SLOTS,
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
            Self::Vec3Uncentered => coordinates::parse_vec3_without_centering(reader),
            Self::Vec2 => coordinates::parse_vec2(reader),
            Self::Rotation => coordinates::parse_rotation(reader),
            Self::BlockState => block::parse_block_state(reader),
            Self::BlockPredicate => block::parse_block_predicate(reader),
            Self::Item => item::parse_item(reader),
            Self::ItemPredicate => item::parse_item_predicate(reader),
            Self::Message => message::parse_message(reader, allow_selectors),
            Self::Component => component::parse_component(reader),
            Self::Particle => particle::parse_particle(reader),
            Self::ResourceLocation => parse_resource_location(reader),
            Self::Resource { registry } => parse_resource(reader, registry),
            Self::Dimension => parse_resource_location(reader),
            Self::IntRange => primitives::parse_int_range(reader),
            Self::LootTable => parse_resource_location(reader),
            Self::ItemSlot => primitives::parse_slot(reader),
            Self::Objective => scoreboard::parse_objective(reader),
            Self::Team => scoreboard::parse_team(reader),
            Self::Criteria => scoreboard::parse_criteria(reader),
            Self::Operation => scoreboard::parse_operation(reader),
            Self::DisplaySlot => scoreboard::parse_display_slot(reader),
            Self::ScoreHolder { multiple } => {
                scoreboard::parse_score_holder(reader, *multiple, allow_selectors)
            }
            Self::Style => scoreboard::parse_style(reader),
            Self::TeamColor => scoreboard::parse_team_color(reader),
            Self::CompoundTag => parse_compound_tag(reader),
            Self::Swizzle => parse_swizzle(reader),
            Self::NbtPath => Ok(Value::String(reader.read_unquoted_string())),
            Self::ResourceOrTag { registry } => parse_resource_or_tag(reader, registry),
            Self::Heightmap => Ok(Value::String(reader.read_unquoted_string())),
            Self::SlotSource => Ok(Value::String(reader.read_unquoted_string())),
            Self::LootPredicate => parse_loot_predicate(reader),
            Self::FloatRange => Ok(Value::String(reader.read_unquoted_string())),
            Self::Function => parse_function(reader),
            Self::NbtTag => parse_nbt_tag(reader),
            Self::ContextIntProvider | Self::ContextFloatProvider => {
                Ok(Value::String(reader.read_unquoted_string()))
            }
            Self::ItemSlots => Ok(Value::String(reader.read_unquoted_string())),
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
            Self::Criteria => scoreboard::suggest_criteria(lower, out),
            Self::Operation => scoreboard::suggest_operations(lower, out),
            Self::DisplaySlot => scoreboard::suggest_slots(lower, out),
            Self::TeamColor => scoreboard::suggest_colors(lower, out),
            Self::ScoreHolder { .. } => scoreboard::suggest_holders(lower, player_names, out),
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

/// 官方 `TagParser.parseCompoundAsArgument`：读一个 SNBT 值，不是复合标签就报
/// `argument.nbt.expected.compound`。
///
/// Official `TagParser.parseCompoundAsArgument`: one SNBT value that must be a compound.
fn parse_compound_tag(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let rest = reader.remaining();
    let (tag, used) = mc_bamboo_nbt::parse_snbt_argument(&rest)
        .map_err(|error| ParseError::text("argument.nbt.invalid", error.message))?;
    if !matches!(tag, mc_bamboo_nbt::Tag::Compound(_)) {
        return Err(ParseError::new("argument.nbt.expected.compound"));
    }
    reader.set_cursor(start + used);
    Ok(Value::Nbt(tag))
}

/// 官方 `ResourceArgument.parse`：读一个标识符，不在注册表里就报 `argument.resource.not_found`。
///
/// Official `ResourceArgument.parse`: read an identifier, or `argument.resource.not_found`.
/// 官方 `SwizzleArgument`：字母 x/y/z 各最多一次，至少一个。
fn parse_swizzle(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    while reader.can_read() && matches!(reader.peek(), 'x' | 'y' | 'z') {
        reader.skip();
    }
    if reader.cursor() == start {
        return Err(ParseError::new("argument.swizzle.invalid"));
    }
    let text = reader.slice(start, reader.cursor()).to_owned();
    for axis in ['x', 'y', 'z'] {
        if text.chars().filter(|c| *c == axis).count() > 1 {
            return Err(ParseError::new("argument.swizzle.duplicate"));
        }
    }
    Ok(Value::String(text))
}

/// 官方 `ResourceOrTagArgument`：`#` 前缀是标签，本命令树只把它当文本解析。
/// 官方 `NbtTagArgument`：任意 SNBT 值，存成 `Value::Nbt`。
fn parse_nbt_tag(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let rest = reader.remaining();
    let (tag, used) = mc_bamboo_nbt::parse_snbt_argument(&rest)
        .map_err(|error| ParseError::text("argument.nbt.invalid", error.message))?;
    reader.set_cursor(start + used);
    Ok(Value::Nbt(tag))
}

///
/// 返回的文本保留 `#`，执行时靠它区分标签和单个条目。条目或标签不存在时在解析期报错；
/// 官方的异常是读完标识符以后用 `createWithContext(reader)` 建的，所以错误位置在
/// 标识符后面，游标不退回。
///
/// Official `ResourceOrTagArgument.parse`. The `#` stays in the returned text so the
/// runner can tell a tag from an element. An unknown element or tag fails at parse time.
/// Vanilla builds the exception with `createWithContext(reader)` after reading the
/// identifier, so the error position is after it and the cursor is not moved back.
fn parse_resource_or_tag(reader: &mut StringReader, registry: &str) -> Result<Value, ParseError> {
    let is_tag = reader.can_read() && reader.peek() == '#';
    if is_tag {
        reader.skip();
    }
    let name = Identifier::read(reader)?.to_registry_name();
    let found = if is_tag {
        mc_bamboo_data::tags::TAG_GROUPS
            .iter()
            .find(|group| group.registry == registry)
            .is_some_and(|group| group.tags.iter().any(|tag| tag.name == name))
    } else {
        registry_entries(registry).is_some_and(|entries| entries.contains(&name.as_str()))
    };
    if !found {
        let key = if is_tag {
            "argument.resource_tag.not_found"
        } else {
            "argument.resource.not_found"
        };
        return Err(ParseError::with_args(
            key,
            [ErrorArg::Text(name), ErrorArg::Text(registry.to_owned())],
        ));
    }
    Ok(Value::String(if is_tag {
        format!("#{name}")
    } else {
        name
    }))
}

/// 官方 `ResourceOrIdArgument.lootPredicate`：先试标识符，不行再读一个内联 SNBT 值。
///
/// Bamboo 没有数据包谓词注册表，原版数据包里也没有内置谓词，所以引用形式一律报
/// `argument.resource_or_id.no_such_element`；内联值存成 `Value::Nbt`，执行时再解码。
///
/// Official `ResourceOrIdArgument.lootPredicate`: an identifier first, otherwise one inline
/// SNBT value. Bamboo has no data-pack predicate registry and vanilla ships no built-in
/// predicates, so a reference always fails with `argument.resource_or_id.no_such_element`.
/// The inline value is kept as `Value::Nbt` and decoded when the command runs.
fn parse_loot_predicate(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    if let Ok(id) = Identifier::read_non_empty(reader) {
        // 错误位置在标识符后面（官方 `createWithContext(reader)`）。
        // The error position is after the identifier (official `createWithContext(reader)`).
        return Err(ParseError::with_args(
            "argument.resource_or_id.no_such_element",
            [
                ErrorArg::Text(id.to_registry_name()),
                ErrorArg::Text("minecraft:predicate".to_owned()),
            ],
        ));
    }
    reader.set_cursor(start);
    parse_nbt_tag(reader)
}

/// 官方 `FunctionArgument.parse`：`#` 开头是函数标签，否则是函数名；存不存在留到执行时查。
///
/// Official `FunctionArgument.parse`: a leading `#` names a function tag, otherwise a
/// function. Whether it exists is checked when the command runs.
fn parse_function(reader: &mut StringReader) -> Result<Value, ParseError> {
    let is_tag = reader.can_read() && reader.peek() == '#';
    if is_tag {
        reader.skip();
    }
    let name = Identifier::read(reader)?.to_registry_name();
    Ok(Value::String(if is_tag {
        format!("#{name}")
    } else {
        name
    }))
}

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
    // 实体类型是内置注册表，不在同步给客户端的那一组里。
    // Entity types are a built-in registry, not one of the synced ones.
    if registry == "minecraft:entity_type" {
        return Some(mc_bamboo_data::registries::entity_type::NAMES);
    }
    // 药水效果注册表也不在同步给客户端的那一组里（`/effect` 的参数要查它）。
    // Mob effects are a built-in registry too, needed by `/effect`.
    if registry == "minecraft:mob_effect" {
        return Some(mc_bamboo_data::registries::mob_effect::NAMES);
    }
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

    /// 官方三个在解析期查注册表的参数：错误键、参数和错误位置（标识符后面）。
    ///
    /// The three arguments that look things up at parse time: error key, arguments and
    /// error position (after the identifier).
    #[test]
    fn registry_backed_arguments_fail_after_the_identifier() {
        let biome = ArgumentType::ResourceOrTag {
            registry: "minecraft:worldgen/biome",
        };
        let mut reader = StringReader::new("minecraft:plains rest");
        assert_eq!(
            biome.parse(&mut reader).unwrap(),
            Value::String("minecraft:plains".to_owned())
        );
        let mut reader = StringReader::new("#minecraft:is_overworld rest");
        assert_eq!(
            biome.parse(&mut reader).unwrap(),
            Value::String("#minecraft:is_overworld".to_owned())
        );
        for (input, key) in [
            ("minecraft:nope rest", "argument.resource.not_found"),
            ("#minecraft:nope rest", "argument.resource_tag.not_found"),
        ] {
            let mut reader = StringReader::new(input);
            let error = biome.parse(&mut reader).unwrap_err();
            assert_eq!(error.key, key);
            assert_eq!(
                error.args,
                [
                    ErrorArg::Text("minecraft:nope".to_owned()),
                    ErrorArg::Text("minecraft:worldgen/biome".to_owned())
                ]
            );
            assert_eq!(reader.cursor(), input.len() - " rest".len(), "{input}");
        }

        let mut reader = StringReader::new("minecraft:nope rest");
        let error = ArgumentType::LootPredicate.parse(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.resource_or_id.no_such_element");
        assert_eq!(reader.cursor(), "minecraft:nope".len());
        let mut reader = StringReader::new("{type:\"x\"} rest");
        assert!(matches!(
            ArgumentType::LootPredicate.parse(&mut reader),
            Ok(Value::Nbt(_))
        ));
        assert_eq!(reader.cursor(), "{type:\"x\"}".len());

        let mut reader = StringReader::new("#demo:tag rest");
        assert_eq!(
            ArgumentType::Function.parse(&mut reader).unwrap(),
            Value::String("#demo:tag".to_owned())
        );
    }

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
