//! 实验场景和分析入口，供 `mc-bamboo-hit-rate` 二进制和诊断程序共用。

pub mod machines;
pub mod world;

pub use machines::*;
pub use world::*;
