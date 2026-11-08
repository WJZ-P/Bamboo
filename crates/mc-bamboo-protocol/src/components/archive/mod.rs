//! 物品组件和物品堆的存档（NBT）编解码器。
//!
//! 和 [`super`] 里的网络编解码器是同一个类型系统，区别只在序列化格式：
//!
//! - 官方用 `NbtOps` 把值编成 NBT，用 `HashOps` 算组件哈希，两个 `DynamicOps` 对
//!   布尔和数值的表示不一样，所以中间加了一层 [`dynamic::Dyn`] 同时支持两种输出；
//! - 官方 `Codec` 的三种可选字段写法在编码时行为不同，[`write::Fields`] 各给一个方法；
//! - 解码要按 `NbtOps` 的宽松规则接受任意数值标签，[`read`] 集中处理。
//!
//! 读写函数分模块放：`read` 从 NBT 读值，`write` 写中间值，`text` 是文本组件的完整
//! 编解码，`components` 是 122 种组件各自的字段表，`stack` 是物品堆和组件补丁。

pub mod components;
pub mod dynamic;
pub mod read;
pub mod stack;
pub mod text;
pub mod write;

pub use dynamic::Dyn;
pub use read::{Error, Result};
