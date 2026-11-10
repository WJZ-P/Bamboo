//! 世界层：区块、方块、光照和世界生成。
//!
//! 这一层只描述游戏世界本身，不知道网络的存在。
//! 把区块编码成数据包的工作在 `mc-bamboo-game` 里完成。

pub mod chunk;
pub mod column;
pub mod flat;
pub mod light;
pub mod palette;
pub mod store;

/// 主世界的最低方块高度。
pub const MIN_Y: i32 = -64;
/// 主世界的总高度。
pub const WORLD_HEIGHT: i32 = 384;
/// 一个区块段的边长。
pub const SECTION_SIZE: i32 = 16;
/// 一个区块里的区块段数量。
pub const SECTION_COUNT: usize = (WORLD_HEIGHT / SECTION_SIZE) as usize;
