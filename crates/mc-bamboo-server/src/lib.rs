//! Bamboo 服务端运行时。
//!
//! 这个 crate 不关心界面：命令行版和桌面版都通过 [`Server`] 启动服务端，
//! 再通过 [`ServerHandle`] 观察它的运行情况。
//!
//! 这里只有网络：accept、握手、登录、配置、连接任务和出站队列。
//! 游戏逻辑在世界线程上，见 `mc-bamboo-game`。

mod config;
mod connection;
mod event;
mod players;
mod server;

pub use config::{ForwardingMode, ServerConfig};
pub use event::{PlayerInfo, ServerEvent};
pub use mc_bamboo_protocol::{GAME_VERSION, PROTOCOL_VERSION};
pub use server::{Server, ServerHandle};
