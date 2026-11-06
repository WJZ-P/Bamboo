//! Minecraft: Java Edition network protocol.
//!
//! Everything here works on byte slices and never touches a socket, so the code can be
//! tested with plain bytes and reused by any transport.

pub mod codec;
pub mod frame;
pub mod packet;

/// Protocol number of the only game version Bamboo supports.
pub const PROTOCOL_VERSION: i32 = 777;

/// Name of the only game version Bamboo supports.
pub const GAME_VERSION: &str = "26.3";
