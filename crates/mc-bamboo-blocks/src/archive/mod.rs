//! 区块里上层内容的官方存档 NBT，见 `docs/m3-storage.md` 5.4 / 5.5 和
//! `docs/world-model-design.md` 5.5。
//!
//! 原则：谁拥有这个类型，谁负责把它写成 NBT、从 NBT 读回来。方块实体自己的键由
//! [`crate::BlockEntity::write_additional`] 写（像官方 `saveAdditional`），按 `id` 造回具体类型
//! 的工厂在 [`block_entity`]。游戏层四张表里的内容由 `mc-bamboo-game` 补进同一个方块实体
//! 条目；`mc-bamboo-storage` 只负责跨类型的合并。
//!
//! 输入当成不可信数据：所有解析返回 [`Error`] 或者用官方缺省值，不 panic。

pub mod block_entity;
pub mod redstone;
pub mod tag;

#[cfg(test)]
mod tests;

pub use block_entity::{BlockEntityKind, create_block_entity, write_block_entity_additional};
pub use redstone::{read_redstone, write_redstone};
pub use tag::{Error, Result, field, put, remove_known, with_defaults};
