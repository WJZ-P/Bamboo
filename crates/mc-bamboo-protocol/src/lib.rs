//! Minecraft Java 版网络协议。
//!
//! 这个 crate 只和字节打交道，不碰 socket，所以可以直接拿字节切片做测试，
//! 也能被任何传输层复用。

pub mod codec;
pub mod frame;
pub mod packet;

pub use mc_bamboo_data::version::{GAME_VERSION, PROTOCOL_VERSION};
