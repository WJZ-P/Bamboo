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
//! | `scoreboard` | 计分板、队伍和自定义 Boss 栏 |
//! | `profiler` | 每刻各阶段的耗时直方图和报告 |
//! | `pacing` | 用 `thread::sleep` 睡到下一刻 |

pub mod admin;
mod block_drops;
mod block_items;
mod book;
mod campfire;
mod chat;
mod chunk_broadcast;
pub mod chunk_codec;
mod chunk_encoding;
mod chunk_tracking;
pub mod command;
mod command_block;
mod container;
mod container_loot;
mod debug_stick;
mod director_host;
mod drops;
mod durability;
mod entity_sync;
mod entity_tracking;
mod entity_view;
mod gamerules;
mod global;
mod inbound;
mod interaction;
mod inventory;
mod lectern;
mod light_updates;
mod links;
mod lock;
mod loot_stacks;
mod menu;
mod movement;
mod outgoing;
mod pacing;
mod player;
mod player_grid;
mod player_move;
mod player_save;
mod profiler;
pub mod region;
mod save_data;
pub mod save_session;
mod scoreboard;
mod secure_chat;
mod sign;
mod tick_levels;
mod weather;
mod world;
mod world_state;

pub use command::test_commands;
pub use director_host::MailBudget;
pub use inbound::{InFlight, Inbound, JoinRequest, PlayerId, PlayerSink};
pub use inventory::receive_like_player;
pub use lectern::{LecternReplay, LecternSnapshot};
pub use loot_stacks::loot_stack;
pub use menu::replay::{Replay, ReplayShape, ReplayWorld, Snapshot};
pub use outgoing::{Outgoing, Segment, SharedFrames};
pub use player_move::{MoveReplay, MoveReplayPacket, MoveReplayReport, replay_movement};
pub use secure_chat::ServiceProfileKeys;
pub use world::{MAX_ENTITY_THREADS, MAX_MAIL_BUDGET, World, WorldConfig};

/// 供 `mc-bamboo-vanilla saved-data` 和官方 codec 对拍：返回 Bamboo 编码器写出的
/// 两份示例 `data` 复合标签（游戏规则、世界边界）。只暴露公开的 [`Tag`]，
/// 不把运行时类型泄漏到 crate 外。值固定，方便对拍期望值。
/// 只在 `oracle-samples` feature 下编进库里，正式构建不含这段对拍代码。
/// Samples of Bamboo's saved-data encoders for the `mc-bamboo-vanilla
/// saved-data` official-codec comparison. Exposes only public [`Tag`]s with
/// fixed values so the tool can assert the expected decoded values. Gated
/// behind the `oracle-samples` feature so production builds omit it.
#[cfg(feature = "oracle-samples")]
pub fn saved_data_samples() -> (mc_bamboo_nbt::Tag, mc_bamboo_nbt::Tag) {
    let mut rules = gamerules::GameRuleSet::vanilla();
    let index = mc_bamboo_data::gamerules::find("minecraft:random_tick_speed").unwrap();
    let _ = rules.assign(index, "7");
    let fire = mc_bamboo_data::gamerules::find("minecraft:fire_damage").unwrap();
    let _ = rules.assign(fire, "false");
    let border = mc_bamboo_blocks::world_border::BorderSettings {
        center_x: 12.5,
        center_z: -8.0,
        damage_per_block: 0.3,
        safe_zone: 4.0,
        warning_blocks: 7,
        warning_time: 42,
        size: 123.0,
        lerp_time: 99,
        lerp_target: 200.0,
    };
    (
        save_data::game_rules_tag(&rules),
        save_data::world_border_tag(&border),
    )
}
