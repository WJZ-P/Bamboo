//! 世界线程：所有游戏状态的唯一所有者。
//!
//! 网络层在 `mc-bamboo-server` 里。连接任务把解码好的请求放进 [`Inbound`] 队列，
//! 世界线程每 tick 处理一次，再把要发给各玩家的字节交给 [`PlayerSink`]。
//! 这个 crate 不依赖 tokio，世界线程也不会等待任何 IO。
//!
//! 模块按领域划分，tick 的各个阶段都从 [`World`] 调用：
//!
//! | 模块 | 负责 |
//! |---|---|
//! | `world` | 主循环、消息分发、进服和离开的顺序 |
//! | `global` | 世界时间和名单。碰不到玩家和链路，只返回要广播的帧 |
//! | `region` | 区块、玩家和链路的归属 |
//! | `player` | 玩家的状态 |
//! | `links` | 每名玩家的出站缓冲 |
//! | `movement` | 移动包和传送确认 |
//! | `chunk_tracking` | 跟踪范围和发送速度 |
//! | `chunk_encoding` | 区块的网络格式 |
//! | `chunk_broadcast` | 每 tick 逐个区块发光照和方块的变化 |
//! | `light_updates` | 算光照，发 `light_update` |
//! | `sign` | 告示牌的方块实体数据 |
//! | `block_items` | 方块实体从物品上带来的组件和它的 `getUpdateTag` |
//! | `book` | 书与笔的编辑和署名 |
//! | `entity_sync`、`interaction`、`inventory`、`chat` | 各领域的游戏逻辑 |

mod block_items;
mod book;
mod chat;
mod chunk_broadcast;
mod chunk_encoding;
mod chunk_tracking;
mod command;
mod container;
mod entity_sync;
mod entity_tracking;
mod global;
mod inbound;
mod interaction;
mod inventory;
mod lectern;
mod light_updates;
mod links;
mod menu;
mod movement;
mod player;
mod player_grid;
mod region;
mod sign;
mod world;

pub use inbound::{InFlight, Inbound, JoinRequest, PlayerId, PlayerSink};
pub use lectern::{LecternReplay, LecternSnapshot};
pub use menu::replay::{Replay, ReplayShape, Snapshot};
pub use world::{World, WorldConfig};
