//! Minecraft 命令解析。这是 Brigadier 的可读 Rust 移植，没有任何网络依赖。
//!
//! - [`CommandGraph`] 是命令树，支持字面量、参数、可执行节点、重定向和权限等级。
//! - [`CommandGraph::parse`] 按官方 `CommandDispatcher.parseNodes` 的规则解析。
//! - [`CommandGraph::suggestions`] 返回补全建议，目前覆盖字面量、玩家名和游戏模式。
//! - [`Value`] 是参数解析结果，命令执行时从这里取值。
//!
//! 翻译键、参数顺序和游标位置都尽量和官方一致，客户端能把错误直接显示出来。

pub mod argument;
pub mod dispatcher;
pub mod error;
pub mod graph;
pub mod identifier;
pub mod reader;
pub mod value;

pub use argument::{ArgumentType, StringKind};
pub use dispatcher::{ParseFailure, ParsedCommand, SuggestionList};
pub use error::{ErrorArg, ParseError};
pub use graph::{CommandGraph, Node, NodeKind};
pub use identifier::Identifier;
pub use reader::StringReader;
pub use value::{
    BlockPredicateInput, BlockStateInput, Coordinates, DisplaySlot, EntityAnchor, EntitySelector,
    GameMode, ItemInput, ItemPredicateInput, Message, MessagePart, NumberBounds, ObjectiveCriteria,
    ParticleInput, Rotation, ScoreHolderSpec, ScoreOperation, SortOrder, TeamColorId, Value,
    WorldCoordinate, short_name, wrap_degrees,
};

/// 官方权限等级：所有玩家、管理员、游戏管理员、管理员、服主。
pub mod permission {
    pub const ALL: u8 = 0;
    pub const MODERATORS: u8 = 1;
    pub const GAMEMASTERS: u8 = 2;
    pub const ADMINS: u8 = 3;
    pub const OWNERS: u8 = 4;
}
