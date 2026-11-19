//! 岛对 `Global` 的写（岛的实施计划第 6 节第 5b 步）。
//!
//! 岛里读全局状态都读 [`GlobalView`](crate::global::GlobalView) 快照；要改全局状态时
//! 不直接拿 `&mut Global`，而是把改动做成一条 [`GlobalEffect`] 交给 [`GlobalPort`]：
//!
//! - **直接**：当场在权威的 `Global` 上应用并重发快照。只有一个岛、精确模式用它，
//!   行为和以前直接改 `Global` 逐位相同。
//! - **排队**：先攒在岛上，岛这一刻的活做完、交回时由主控按提交顺序应用。多岛时用它；
//!   现在只在测试里打开（`BAMBOO_TEST_GLOBAL_PORT=queued`），验证只有反馈晚到、内容不变。
//!
//! Island writes to `Global` (island plan, section 6, step 5b). The island reads
//! global state from the [`GlobalView`](crate::global::GlobalView) snapshot; to
//! change it, it hands a [`GlobalEffect`] to a [`GlobalPort`] instead of taking
//! `&mut Global`:
//!
//! - **Direct** applies it at once to the authoritative `Global` and republishes
//!   the snapshot. Used with one island and in exact mode; bit for bit what the
//!   direct writes did before.
//! - **Queued** keeps it on the island; the master applies the queue in order
//!   when the island hands back after its tick. Multi-island mode will use it;
//!   for now only tests turn it on (`BAMBOO_TEST_GLOBAL_PORT=queued`) to check
//!   that only feedback arrives later and nothing else changes.

use mc_bamboo_command::ParsedCommand;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::{
    command::{CommandFn, feedback},
    global::Global,
    inbound::PlayerId,
    region::Region,
    secure_chat::SessionUpdate,
};

/// 岛交给主控的一条全局改动。
///
/// One change to global state the island hands to the master.
pub(crate) enum GlobalEffect {
    /// 全局类命令（计划 4.4）：主控用权威的 `Global` 执行，失败反馈也在那时发出。
    /// 解析、签名校验和刷屏计数都已经在岛上做完。
    ///
    /// A global command (plan 4.4). The master runs it on the authoritative
    /// `Global` and reports a failure then. Parsing, signature checks and spam
    /// counting already happened on the island.
    Command {
        source: PlayerId,
        command: String,
        parsed: ParsedCommand,
        action: CommandFn,
    },
    /// 客户端的 `set_game_rule` 包。权限已经在岛上查过。
    ///
    /// A client `set_game_rule` packet. The island already checked permission.
    GameRules {
        name: String,
        entries: Vec<(String, String)>,
    },
    /// Tab 名单里的游戏模式（`/gamemode`、F3+F4）。
    ///
    /// The game mode on the tab list (`/gamemode`, F3+F4).
    GameMode { player: PlayerId, mode: GameMode },
    /// Tab 名单里的帽子层（客户端设置）。
    ///
    /// The hat layer on the tab list (client settings).
    Hat { player: PlayerId, show: bool },
    /// Tab 名单里的签名聊天会话。
    ///
    /// The signed-chat session on the tab list.
    ChatSession {
        player: PlayerId,
        update: SessionUpdate,
    },
    /// 官方 `ServerPlayer.doTick` 的六个只读准则这一刻变了的值：岛算，主控写计分板。
    ///
    /// This tick's changed values of the six read-only criteria from official
    /// `ServerPlayer.doTick`: the island computes, the master writes the scoreboard.
    Criteria {
        name: String,
        changes: Vec<(usize, i32)>,
    },
    /// 玩家离开：广播「离开了游戏」。
    ///
    /// A player left: broadcast "left the game".
    LeaveMessage { player: PlayerId },
    /// 官方 `ServerLevel.tick` 的睡眠跳过：人数够了跳到早上并清除天气。
    ///
    /// Official `ServerLevel.tick`'s sleep skip: enough players asleep, so
    /// advance the overworld clock to morning and clear the weather.
    SkipNight,
    /// 玩家离开：从 Boss 栏和 Tab 名单里去掉。
    ///
    /// A player left: drop them from boss bars and the tab list.
    Left { player: PlayerId },
}

/// 岛写全局状态的口子。
///
/// The island's port for writing global state.
pub(crate) enum GlobalPort<'a> {
    /// 当场应用。
    ///
    /// Apply at once.
    Direct(&'a mut Global),
    /// 攒起来，交回时应用。
    ///
    /// Keep for the hand-back.
    Queued(&'a mut Vec<GlobalEffect>),
}

impl GlobalPort<'_> {
    /// 提交一条改动。直接模式下当场应用；`region` 是发起改动的岛。
    ///
    /// Submit one change. Direct mode applies it at once; `region` is the
    /// submitting island.
    pub(crate) fn submit(&mut self, region: &mut Region, effect: GlobalEffect) {
        match self {
            Self::Direct(global) => apply(global, region, effect),
            Self::Queued(queue) => queue.push(effect),
        }
    }
}

/// 两种实现的选择。
///
/// Which implementation the world uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortMode {
    Direct,
    /// 现在只有单元测试用；多岛上线（第 10 步）后运行时也用。
    ///
    /// Only unit tests use it for now; multi-island mode (step 10) will use it at run time.
    #[cfg_attr(not(test), allow(dead_code))]
    Queued,
}

impl PortMode {
    /// 世界缺省用直接实现。单元测试可以用 `BAMBOO_TEST_GLOBAL_PORT=queued` 让整套测试
    /// 改走排队实现（同 `BAMBOO_TEST_LOADED`，作用于整个测试进程）。
    ///
    /// The world uses the direct implementation. Unit tests can switch the whole
    /// test process to the queued one with `BAMBOO_TEST_GLOBAL_PORT=queued` (like
    /// `BAMBOO_TEST_LOADED`).
    pub(crate) fn default_for_world() -> Self {
        #[cfg(test)]
        if std::env::var("BAMBOO_TEST_GLOBAL_PORT").is_ok_and(|value| value == "queued") {
            return Self::Queued;
        }
        Self::Direct
    }
}

/// 主控按提交顺序应用岛交回的改动。
///
/// The master applies the island's handed-back changes in submission order.
pub(crate) fn apply_all(global: &mut Global, region: &mut Region, effects: Vec<GlobalEffect>) {
    for effect in effects {
        apply(global, region, effect);
    }
}

/// 在权威的 `Global` 上应用一条改动，帧发给 `region` 的玩家。改了快照里的东西就重发快照。
///
/// 这是主控在执行：写给玩家的帧先投进岛的出站信箱，做完再由岛按顺序写进各自的缓冲
/// （岛的实施计划第 2 节第 28 行），齐步下时点和顺序都和直接写一样。
///
/// Apply one change to the authoritative `Global`, sending frames to `region`'s
/// players. Republish the snapshot when it covers what changed. This is the
/// master running: frames go to the island's outbound mailbox and the island
/// writes them in order when it is done (island plan, section 2, row 28), at
/// the same point and in the same order as direct writes in lockstep.
pub(crate) fn apply(global: &mut Global, region: &mut Region, effect: GlobalEffect) {
    region.links.open_mailbox();
    apply_on_master(global, region, effect);
    region.links.deliver_mailbox();
}

fn apply_on_master(global: &mut Global, region: &mut Region, effect: GlobalEffect) {
    match effect {
        GlobalEffect::Command {
            source,
            command,
            parsed,
            action,
        } => {
            if let Err(failure) = action(global, region, source, &parsed) {
                feedback::report(region, source, &command, &failure);
            }
            global.publish(region);
        }
        GlobalEffect::GameRules { name, entries } => {
            crate::gamerules::apply_set_packet(global, region, &name, entries);
        }
        GlobalEffect::GameMode { player, mode } => {
            if let Some(frames) = global.set_game_mode(player, mode) {
                region.links.broadcast(&frames);
            }
        }
        GlobalEffect::Hat { player, show } => {
            if let Some(frames) = global.set_hat(player, show) {
                region.links.broadcast(&frames);
            }
        }
        GlobalEffect::ChatSession { player, update } => {
            if let Some(frames) = global.initialize_chat(player, &update) {
                region.links.broadcast(&frames);
            }
        }
        GlobalEffect::Criteria { name, changes } => {
            if global.scoreboard().tracks_criteria(&changes) {
                global
                    .scoreboard_mut()
                    .write_criteria(&name, &changes, region);
                global.publish(region);
            }
        }
        GlobalEffect::LeaveMessage { player } => {
            if let Some(frames) = global.left_message(player) {
                // 官方 `PlayerList.broadcastSystemMessage` 逐玩家看可见性。
                //
                // Official `PlayerList.broadcastSystemMessage` checks visibility per player.
                crate::chat::broadcast_system_message(region, &frames, None);
            }
        }
        GlobalEffect::SkipNight => {
            crate::sleep::skip_night(global);
            global.publish(region);
            let time = global.time();
            let clocks = global.clocks();
            let advance = global.rules().bool(mc_bamboo_data::gamerules::ADVANCE_TIME);
            region.broadcast_all_clocks(time, &clocks, advance);
        }
        GlobalEffect::Left { player } => {
            // 官方玩家断开时 `entityRemoved` 直接 return，不广播 reset_score；
            // 分数和 Boss 栏的 UUID 集合都留着，下次进服还会补发 add 包。
            //
            // Official `entityRemoved` returns for players without broadcasting
            // reset_score; scores and boss-bar UUID sets stay so rejoining gets the
            // add packet again.
            global.boss_bars_mut().on_leave(player);
            if let Some(frames) = global.remove_player(player) {
                region.links.broadcast(&frames);
            }
        }
    }
}
