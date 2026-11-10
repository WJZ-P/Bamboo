//! 存档的编解码层：区块 NBT 和 Anvil 区域文件。
//!
//! 这一层只负责把区块束和磁盘字节互相转换，不接进游戏，不启动任何线程，
//! 磁盘上也不会因为引入它多出文件。见 `docs/m3-storage.md`。

pub mod anvil;
#[cfg(test)]
mod bench;
pub mod chunk_nbt;
