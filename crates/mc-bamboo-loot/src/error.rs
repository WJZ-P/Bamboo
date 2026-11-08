//! 战利品表的错误。

use thiserror::Error;

/// 战利品表求值或解析时可能出现的错误。
///
/// 官方的数据包在加载时就已经校验过，所以这里的错误只有三种来源：
/// 引用不存在的表、本实现标记为不支持的节点、以及生成数据本身坏掉。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LootError {
    #[error("unknown loot table: {0}")]
    UnknownTable(String),
    /// 这个节点依赖本实现没有的数据或世界查询，比如配方、附魔定义。
    #[error("unsupported loot feature: {0}")]
    Unsupported(String),
    #[error("invalid loot data: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LootError>;
