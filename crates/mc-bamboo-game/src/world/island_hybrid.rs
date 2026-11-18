//! 混合驱动（岛的实施计划 5.5，第 10 步第三段）。
//!
//! 多岛开着、分离驱动时，主控每一刻把停放着的岛派给 tick 线程池，只等到这一刻的截止
//! 时间；到点没回来的岛这一刻不等，主控和别的岛照常进下一刻，慢岛欠着刻、回来以后连续
//! 追赶。所有岛都按时回来时，每一步的先后和以前「全部等齐」一行不差。
//!
//! 这里放的是只属于混合驱动的东西：
//!
//! - 一刻里的收岛、派出、等待、落地、追赶（计划 5.5.1）；
//! - 岛没停放时主控的事怎么排队（计划 5.5.2）：玩家的入站消息按来源排、等全部岛停放的
//!   事整件排，能做的时候按原来的先后做；
//! - 给不在主控手里的岛的出站投递（计划 5.5.3）；
//! - 存盘名单留给不在的岛、关服时把岛收回来、岛 panic 时收齐再抛（计划 5.5.5）；
//! - 测试专用的「某岛第 N 到 M 刻不回来」（计划 5.5.7）。
//!
//! 别的驱动下这里的队列和表一直是空的，[`World::accept_message`]、[`World::run_or_hold`]、
//! [`World::land_island`] 都退化成当场做。
//!
//! The hybrid drive (island plan 5.5, step 10 part three). With multi-island
//! mode on and the detached drive, the master hands the parked islands to the
//! tick threads every tick and waits only until that tick's deadline; an
//! island that is not back is not waited for, the master and the other
//! islands go on, and the slow island owes ticks and catches up back to back
//! once it returns. When every island is on time, each step happens in the
//! same order as the old wait-for-all drive.
//!
//! This module holds what belongs to the hybrid drive alone: taking islands
//! in, dispatching, waiting, landing and catching up (5.5.1); queueing the
//! master's work while islands are away (5.5.2); deferred outbound delivery
//! for islands that are away (5.5.3); save lists kept for absent islands,
//! recalling islands at shutdown and raising an island panic once the others
//! are back (5.5.5); and the test-only "island X stays away for ticks N to M"
//! (5.5.7). Under the other drives its queues stay empty and
//! [`World::accept_message`], [`World::run_or_hold`] and
//! [`World::land_island`] simply act at once.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::ControlFlow,
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_protocol::packet::play::PlayRequest;
use mc_bamboo_storage::player_data::PlayerData;
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp};
use tracing::{error, warn};

use super::{ISLAND_SLOW_REPORT, ISLAND_WAIT_SLICE, World};
use crate::{
    inbound::{Inbound, JoinRequest, PlayerId, Sequenced},
    island::{Borrow, IslandAccess, IslandDrive, SavePlan, SaveReport},
    island_pool::{HandBack, Wait},
    links::FanOut,
    profiler::{Metric, Phase},
    world_state::RespawnData,
};

/// 岛最多欠这么多刻（2 秒，同 `MAX_CATCH_UP`）；回来时欠得更多就全部丢掉（计划 5.5.1）。
///
/// The most ticks an island may owe (2 seconds, as `MAX_CATCH_UP`); owing
/// more when it returns drops them all (plan 5.5.1).
pub(crate) const MAX_DEBT: u32 = 40;

/// 一件等着做的事是谁发起的：同一来源的事严格按先后做（计划 5.5.2）。
///
/// Who started a piece of held work; work of one source runs strictly in
/// order (plan 5.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// 主控自己：控制台命令、白名单重载、`/save-all` 和它的反馈、自动保存。
    Master,
    /// 一名玩家：他的进服、请求和离开。
    Player(PlayerId),
}

/// 主控发起、要等全部岛停放的一件事。
///
/// A piece of master work that waits for every island to be parked.
pub(crate) type Work = Box<dyn FnOnce(&mut World) + Send>;

/// 一件排着队的事（计划 5.5.2）。
///
/// One piece of held work (plan 5.5.2).
pub(crate) enum Held {
    /// 一条入站消息，要的岛还没停放。
    Message(Box<Inbound>),
    /// 票已经挂上、等目标岛停放的进服。
    Join(Box<PendingJoin>),
    /// 主控自己的一件事，等全部岛停放。
    Work(Work),
}

/// 进服的前一半做完以后留下的东西（见 `World::begin_join`）。
///
/// What the first half of a join leaves behind (see `World::begin_join`).
pub(crate) struct PendingJoin {
    pub(crate) request: JoinRequest,
    pub(crate) permission_level: u8,
    pub(crate) is_op: bool,
    pub(crate) restored: Option<PlayerData>,
    pub(crate) game_mode: mc_bamboo_protocol::packet::play::GameMode,
    pub(crate) join_spawn: RespawnData,
    pub(crate) difficulty_frames: Vec<u8>,
    pub(crate) spawn_chunk: ChunkPos,
}

/// 混合驱动的状态，全部只由主控线程读写（计划 5.5.6）。
///
/// The hybrid drive's state, read and written by the master thread only
/// (plan 5.5.6).
#[derive(Default)]
pub(crate) struct HybridState {
    /// 这一刻等岛等到什么时候。`None`（直接调 `World::tick`）等全部岛回来。
    ///
    /// How long this tick waits for islands. `None` (a direct `World::tick`)
    /// waits for every island.
    pub(crate) deadline: Option<Instant>,
    /// 混合驱动跑到第几刻（从 1 起）。
    pub(crate) tick: u64,
    /// 每个不在主控手里的岛的出站队列（计划 5.5.3）。停放着的岛没有条目。
    ///
    /// The outbound queue of each island that is away (plan 5.5.3). Parked
    /// islands have no entry.
    pub(crate) fan_out: BTreeMap<RegionId, Vec<FanOut>>,
    /// 排着队的事，按来源分；每条带着排进来的先后。
    ///
    /// Held work by source, each entry with its arrival order.
    held: BTreeMap<Source, VecDeque<(u64, Held)>>,
    held_order: u64,
    /// 正在进服的玩家：票已经挂上，人还没放进岛。
    ///
    /// Players still joining: ticket placed, not on an island yet.
    pub(crate) joining: BTreeSet<PlayerId>,
    /// 留给不在主控手里的岛的存盘名单，落地时并进它的名单（计划 5.5.5）。
    ///
    /// Save lists kept for islands that are away, merged in when they land
    /// (plan 5.5.5).
    pub(crate) plans: BTreeMap<RegionId, SavePlan>,
    /// 落后的岛落地时交出来的存盘报告，下一次排名单时用。
    ///
    /// Save reports handed over when lagging islands landed, used by the next
    /// save planning.
    pub(crate) reports: SaveReport,
    /// 落后的岛落地时取走的名单改动，这一刻末尾一起发布。
    pub(crate) admin: crate::admin::AdminChanges,
    /// 有岛在 tick 线程上 panic 了：载荷，收齐别的岛以后在主控线程上抛出。
    ///
    /// An island panicked on its tick thread: the payload, re-raised on the
    /// master thread once the other islands are back.
    panic: Option<Box<dyn Any + Send>>,
    /// 等岛的时候已经等够了关服的超时：关服不用再等一遍。
    gave_up: bool,
    /// 登录包里的人数上限（家岛不在主控手里时进服复查名单要用）。
    pub(crate) max_players: i32,
    /// 测试专用：哪个岛在哪几刻不回来（计划 5.5.7）。
    #[cfg(test)]
    pub(crate) stalls: Vec<Stall>,
    /// 测试专用：故意改坏的开关（改坏确认）。
    #[cfg(test)]
    pub(crate) breaks: Breaks,
}

impl HybridState {
    /// 取出正在进服（票已经挂上、还在等目标岛）的玩家的队列里已经到达的消息。他们的队列
    /// 还没挂到任何岛上，不取的话他的离开永远到不了，等着的进服也就撤不掉。
    ///
    /// Drain the queues of players still joining (ticket placed, waiting for
    /// the target island). Their queues hang on no island yet; without this a
    /// leave would never arrive and the waiting join could not be cancelled.
    pub(super) fn drain_joining(&self, out: &mut Vec<Sequenced>) {
        for queue in self.held.values() {
            for (_, held) in queue {
                if let Held::Join(pending) = held {
                    pending.request.inbox.drain_into(out);
                }
            }
        }
    }

    /// 排着队的事的件数。
    #[cfg(test)]
    pub(crate) fn held_len(&self) -> usize {
        self.held.values().map(VecDeque::len).sum()
    }
}

/// 测试专用：岛 `island` 在混合驱动的第 `from` 到 `to` 刻（含）派出去以后不回来。
///
/// Test only: island `island`, once dispatched during hybrid ticks `from` to
/// `to` (inclusive), does not come back.
#[cfg(test)]
pub(crate) struct Stall {
    pub(crate) island: RegionId,
    pub(crate) from: u64,
    pub(crate) to: u64,
    /// 已经卡在 tick 线程上：开闸用的标志。
    gate: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// 测试专用：故意改坏混合驱动的某一处，确认对应的测试会因为它失败。正式构建里不存在。
///
/// Test only: break one part of the hybrid drive on purpose to confirm the
/// matching test fails because of it. Absent in production builds.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Breaks {
    /// 不给投给不存在的岛编号的信建岛（上一任的疑点 3 原样）。
    pub(crate) no_islands_for_mail: bool,
    /// 命令不等全部岛停放，只作用于停放着的岛。
    pub(crate) commands_do_not_wait: bool,
    /// 丢掉给不在的岛的出站投递。
    pub(crate) drop_fan_out: bool,
    /// 给不在的岛的出站投递落地时倒着发。
    pub(crate) reverse_fan_out: bool,
    /// 慢岛回来以后不追赶。
    pub(crate) no_catch_up: bool,
    /// 排着队的事倒着做。
    pub(crate) held_backwards: bool,
    /// 排着队的事排进来就丢掉。
    pub(crate) drop_held: bool,
    /// 给不在的岛的出站投递抄两份（发两遍）。
    pub(crate) fan_out_twice: bool,
    /// 卷进不在的岛的拆合直接丢掉，不留着。
    pub(crate) forget_blocked_topology: bool,
}

impl World {
    /// 是不是混合驱动：多岛开着并且分离驱动（计划 5.5）。
    ///
    /// Whether the hybrid drive is in charge: multi-island mode with the
    /// detached drive (plan 5.5).
    pub(crate) fn hybrid(&self) -> bool {
        self.master.islands_mode && self.master.drive == IslandDrive::Detached
    }

    /// 未签名消息和过期判断用的现在（照停放着的岛的；测试可以钉死）。
    pub(super) fn clock_millis(&self) -> i64 {
        match self.islands.template() {
            Some(id) => self.islands.get(id).expect("停放着").region.now_millis(),
            None => crate::secure_chat::now_millis(),
        }
    }

    /// 登录包里的人数上限。
    pub(super) fn max_players(&self) -> i32 {
        match self.islands.template() {
            Some(id) => self.islands.get(id).expect("停放着").region.max_players(),
            None => self.master.hybrid.max_players,
        }
    }

    /// 给这名玩家发反馈时在哪个岛上做：他所在的岛，不然是家岛（或者别的停放着的岛）。
    ///
    /// The island to run feedback for this player on: theirs, otherwise home
    /// (or another parked island).
    pub(super) fn island_for_feedback(&self, source: PlayerId) -> RegionId {
        self.islands
            .island_of_player(source)
            .filter(|&id| self.islands.is_parked(id))
            .or_else(|| self.islands.template())
            .unwrap_or(HOME_REGION)
    }

    // ---- 排队（计划 5.5.2） / Holding (plan 5.5.2) ----

    /// 处理一条入站消息：现在能做就做；它要的岛没停放，或者同一来源前面还有事排着，就
    /// 排进这个来源的队列，保持先后。关服消息由调用方处理。
    ///
    /// Handle one inbound message: run it now if possible; if an island it
    /// needs is not parked, or its source already has held work, queue it
    /// behind that source to keep the order. Shutdown is the caller's.
    pub(super) fn accept_message(&mut self, message: Inbound) {
        let source = match &message {
            Inbound::Join(request) => Source::Player(request.id),
            Inbound::Request { player, .. } | Inbound::Leave { player } => Source::Player(*player),
            Inbound::Latency { player, millis } => {
                self.master.global.set_latency(*player, *millis);
                return;
            }
            Inbound::WhitelistReload(_) | Inbound::ConsoleCommand(_) | Inbound::Shutdown => {
                Source::Master
            }
        };
        if self
            .master
            .hybrid
            .held
            .get(&source)
            .is_some_and(|queue| !queue.is_empty())
        {
            if let Inbound::Leave { player } = &message
                && self.cancel_pending_join(*player)
            {
                return;
            }
            self.hold(source, Held::Message(Box::new(message)));
            return;
        }
        if let Err(held) = self.try_message(message) {
            self.hold(source, held);
        }
    }

    /// 主控自己的一件要全部岛停放的事：全部停放着、前面也没有排着的就当场做，不然排队。
    ///
    /// A piece of master work that needs every island parked: run now if they
    /// all are and nothing of the master's is held, otherwise queue it.
    pub(super) fn run_or_hold(&mut self, work: Work) {
        let queued = self
            .master
            .hybrid
            .held
            .get(&Source::Master)
            .is_some_and(|queue| !queue.is_empty());
        if !queued && self.islands.all_parked() {
            work(self);
        } else {
            self.hold(Source::Master, Held::Work(work));
        }
    }

    fn hold(&mut self, source: Source, held: Held) {
        #[cfg(test)]
        if self.master.hybrid.breaks.drop_held {
            return;
        }
        let state = &mut self.master.hybrid;
        state.held_order += 1;
        state
            .held
            .entry(source)
            .or_default()
            .push_back((state.held_order, held));
    }

    /// 这名玩家的进服还在等目标岛、他就离开了：撤票，整条队列作废。返回是不是这种情况。
    ///
    /// The player left while their join was still waiting for its island:
    /// drop the ticket and the whole queue. Returns whether that was the case.
    fn cancel_pending_join(&mut self, player: PlayerId) -> bool {
        let source = Source::Player(player);
        let waiting = self
            .master
            .hybrid
            .held
            .get(&source)
            .and_then(VecDeque::front)
            .is_some_and(|(_, held)| matches!(held, Held::Join(_)));
        if !waiting {
            return false;
        }
        self.master.hybrid.held.remove(&source);
        self.master.hybrid.joining.remove(&player);
        self.master
            .refused_inboxes
            .retain(|(refused, _)| *refused != player);
        self.apply_ticket_ops([TicketOp::PlayerMoved {
            id: u64::from(player.0 as u32),
            to: None,
        }]);
        true
    }

    /// 试着做一件事。它要的岛没停放时原样退回（计划 5.5.2 的三种等法）。
    ///
    /// Try one piece of work; it is handed back when an island it needs is
    /// not parked (the three kinds of waiting in plan 5.5.2).
    fn try_held(&mut self, held: Held) -> Result<(), Held> {
        match held {
            Held::Message(message) => self.try_message(*message),
            Held::Join(pending) => self.finish_join(pending).map_err(Held::Join),
            Held::Work(work) => {
                if !self.islands.all_parked() {
                    return Err(Held::Work(work));
                }
                work(self);
                Ok(())
            }
        }
    }

    /// 试着处理一条入站消息（见 [`Self::try_held`]）。
    ///
    /// Try one inbound message (see [`Self::try_held`]).
    fn try_message(&mut self, message: Inbound) -> Result<(), Held> {
        let wait = |message: Inbound| Err(Held::Message(Box::new(message)));
        match message {
            Inbound::Join(request) => match self.begin_join(*request) {
                Some(pending) => self.finish_join(pending).map_err(Held::Join),
                None => Ok(()),
            },
            Inbound::Request {
                player,
                request,
                in_flight,
            } => {
                let island = self.islands.island_of_player(player);
                let blocked = match island {
                    // 他自己的岛不在主控手里：等它停放。
                    // Their own island is away: wait for it.
                    Some(island) if !self.islands.is_parked(island) => true,
                    Some(_) => self.needs_everyone(&request) && !self.islands.all_parked(),
                    None => false,
                };
                if blocked {
                    return wait(Inbound::Request {
                        player,
                        request,
                        in_flight,
                    });
                }
                self.on_request(player, request);
                drop(in_flight);
                Ok(())
            }
            Inbound::Leave { player } => {
                if self
                    .islands
                    .island_of_player(player)
                    .is_some_and(|island| !self.islands.is_parked(island))
                {
                    return wait(Inbound::Leave { player });
                }
                self.on_leave(player);
                Ok(())
            }
            Inbound::Latency { player, millis } => {
                self.master.global.set_latency(player, millis);
                Ok(())
            }
            Inbound::WhitelistReload(reload) => {
                if !self.islands.all_parked() {
                    return wait(Inbound::WhitelistReload(reload));
                }
                self.master.global.admin.whitelist = reload.entries;
                let home = self.islands.template().unwrap_or(HOME_REGION);
                self.on_master(home, Borrow::All, |global, region| {
                    crate::command::admin::after_reload(global, region);
                });
                Ok(())
            }
            // 连接层的控制台线程读到的命令，在世界线程里用服务器控制台来源执行。
            // 控制台没有岛，在主控上执行，别的岛上的玩家借到家岛上（计划 4.4）。
            // A console line read by the connection layer runs here with the
            // server-console source, on the master with every player borrowed (plan 4.4).
            Inbound::ConsoleCommand(line) => {
                if !self.islands.all_parked() {
                    return wait(Inbound::ConsoleCommand(line));
                }
                let borrow = crate::command::borrow_for(&line);
                self.on_master(HOME_REGION, borrow, |global, region| {
                    crate::command::console::on_console_command(global, region, &line);
                });
                Ok(())
            }
            Inbound::Shutdown => Ok(()),
        }
    }

    /// 这条请求要看得见全部岛上的玩家：命令（`@a`、玩家名可能指向任何岛上的人）和改规则包
    /// （给全部 OP 的反馈）。规则是「等」，不会只作用于停放的岛（计划 5.5.2）。
    ///
    /// This request must see every island's players: commands (`@a` and names
    /// may point at anyone) and game-rule packets (feedback to every op). The
    /// rule is to wait, never to act on the parked islands only (plan 5.5.2).
    fn needs_everyone(&self, request: &PlayRequest) -> bool {
        #[cfg(test)]
        if self.master.hybrid.breaks.commands_do_not_wait {
            return false;
        }
        matches!(
            request,
            PlayRequest::ChatCommand { .. } | PlayRequest::SetGameRule { .. }
        )
    }

    /// 有没有一件排在队头、要等全部岛停放的事（有的话第 ④ 段先不派岛出去追赶，留到下一刻
    /// 开头凑齐，计划 5.5.2）。
    ///
    /// Whether some queue head waits for every island (if so, part 4 holds
    /// back catch-up dispatches so the islands meet at the next tick start,
    /// plan 5.5.2).
    fn everyone_awaited(&self) -> bool {
        self.master.hybrid.held.values().any(|queue| {
            queue.front().is_some_and(|(_, held)| match held {
                Held::Work(_) => true,
                Held::Join(_) => false,
                Held::Message(message) => match &**message {
                    Inbound::ConsoleCommand(_) | Inbound::WhitelistReload(_) => true,
                    Inbound::Request {
                        player, request, ..
                    } => {
                        self.needs_everyone(request)
                            && self
                                .islands
                                .island_of_player(*player)
                                .is_some_and(|island| self.islands.is_parked(island))
                    }
                    _ => false,
                },
            })
        })
    }

    /// 把排着队的事里现在能做的做掉，按排进来的先后：每次在各来源的队头里挑最早排进来、
    /// 现在做得动的那一件做，做完重新挑，直到谁的队头都做不动。同一来源永远只看队头，
    /// 所以它的先后不变。别的驱动下队列一直是空的。
    ///
    /// Run whatever held work can run now, in arrival order: each step takes
    /// the earliest-arrived queue head that can run, then looks again, until
    /// no head can move. Only heads are considered, so each source keeps its
    /// order. The queues are always empty under the other drives.
    pub(super) fn drain_held(&mut self) {
        if self.master.hybrid.held.is_empty() {
            return;
        }
        loop {
            let mut heads: Vec<(u64, Source)> = self
                .master
                .hybrid
                .held
                .iter()
                .filter_map(|(source, queue)| self.head_order(queue).map(|order| (order, *source)))
                .collect();
            heads.sort_unstable();
            let mut progressed = false;
            for (_, source) in heads {
                let Some((order, held)) = self.pop_held(source) else {
                    continue;
                };
                match self.try_held(held) {
                    Ok(()) => {
                        // 做了一件可能改全局状态的事以后重发快照，下一件读到的是最新的。
                        // Republish after work that may change global state.
                        self.publish_global();
                        progressed = true;
                        break;
                    }
                    Err(held) => self.unpop_held(source, order, held),
                }
            }
            if !progressed {
                break;
            }
        }
        self.master.hybrid.held.retain(|_, queue| !queue.is_empty());
    }

    fn head_order(&self, queue: &VecDeque<(u64, Held)>) -> Option<u64> {
        #[cfg(test)]
        if self.master.hybrid.breaks.held_backwards {
            return queue.back().map(|(order, _)| *order);
        }
        queue.front().map(|(order, _)| *order)
    }

    fn pop_held(&mut self, source: Source) -> Option<(u64, Held)> {
        let queue = self.master.hybrid.held.get_mut(&source)?;
        #[cfg(test)]
        if self.master.hybrid.breaks.held_backwards {
            return queue.pop_back();
        }
        queue.pop_front()
    }

    fn unpop_held(&mut self, source: Source, order: u64, held: Held) {
        let queue = self.master.hybrid.held.entry(source).or_default();
        #[cfg(test)]
        if self.master.hybrid.breaks.held_backwards {
            queue.push_back((order, held));
            return;
        }
        queue.push_front((order, held));
    }

    // ---- 延后投递（计划 5.5.3） / Deferred delivery (plan 5.5.3) ----

    /// 主控当场做完一件事以后，给每个不在主控手里的岛各抄一份它那部分的投递，接在它的
    /// 出站队列后面。
    ///
    /// After the master did something at once, copy the absent islands' share
    /// of the deliveries onto each one's outbound queue.
    pub(super) fn post_fan_out(&mut self, away: &[RegionId], items: Vec<FanOut>) {
        if items.is_empty() {
            return;
        }
        #[cfg(test)]
        if self.master.hybrid.breaks.drop_fan_out {
            return;
        }
        for &id in away {
            let queue = self.master.hybrid.fan_out.entry(id).or_default();
            queue.extend(items.iter().cloned());
            #[cfg(test)]
            if self.master.hybrid.breaks.fan_out_twice {
                queue.extend(items.iter().cloned());
            }
        }
    }

    // ---- 落地（计划 5.5.1） / Landing (plan 5.5.1) ----

    /// 一个停放着的岛的落地：先按顺序发它不在期间排给它的出站投递，收下留给它的存盘
    /// 名单，再交回（回执给 Director、排队的全局改动由主控应用）。别的驱动下前两样一直
    /// 是空的，就是原来的交回。
    ///
    /// Landing of one parked island: first the outbound deliveries queued
    /// while it was away, in order, and the save list kept for it; then the
    /// hand-back (acks to the Director, queued global changes applied by the
    /// master). Under the other drives the first two are always empty and
    /// this is the old hand-back.
    pub(super) fn land_island(&mut self, id: RegionId) {
        #[cfg_attr(not(test), allow(unused_mut))]
        if let Some(mut items) = self.master.hybrid.fan_out.remove(&id) {
            #[cfg(test)]
            if self.master.hybrid.breaks.reverse_fan_out {
                items.reverse();
            }
            let region = &mut self.islands.get_mut(id).expect("停放着").region;
            for item in &items {
                match item {
                    FanOut::Run(task) => task(region),
                    other => region.links.apply_fan_out(other),
                }
            }
        }
        if let Some(plan) = self.master.hybrid.plans.remove(&id) {
            self.islands
                .get_mut(id)
                .expect("停放着")
                .save_plan
                .absorb(plan);
        }
        self.hand_back_island(id);
    }

    /// 落后的岛落地的最后一步：刷它的出站缓冲，存盘报告和名单改动先存到主控手里（岛可能
    /// 马上又派出去追赶）。
    ///
    /// The last step of a lagging island's landing: flush its outbound
    /// buffers and stash its save report and list changes with the master
    /// (the island may be dispatched again at once to catch up).
    fn flush_island(&mut self, id: RegionId) {
        let Some(island) = self.islands.get_mut(id) else {
            return;
        };
        let _ = island.region.links.flush();
        let report = std::mem::take(&mut island.save_report);
        let changes = crate::command::admin::take_changes(&mut island.region);
        self.master.hybrid.reports.absorb(report);
        self.master.hybrid.admin.merge(changes);
    }

    /// 岛交回来了：放回槽位（停放），记下跑它的线程和它这一刻的耗时。欠刻超过上限就
    /// 全部丢掉。它要是 panic 了，记下载荷。返回岛的编号。
    ///
    /// An island is back: seat it in its slot (parked), remember its thread
    /// and tick time, drop the ticks it owes if past the limit, and keep the
    /// payload if it panicked. Returns its id.
    fn seat(&mut self, back: HandBack) -> RegionId {
        let HandBack {
            mut island,
            worker,
            started,
            finished,
            panic,
        } = back;
        island.park();
        island.last_worker = worker;
        island.region.profiler.record_duration(
            Metric::IslandBusy,
            finished.saturating_duration_since(started),
        );
        let id = island.id;
        let slot = self.islands.slot_mut(id).expect("交出去的岛回到自己的槽位");
        slot.put(island);
        if slot.debt > MAX_DEBT {
            warn!(
                island = id,
                ticks = slot.debt,
                "an island can't keep up; dropping the ticks it owes"
            );
            slot.debt = 0;
        }
        if let Some(payload) = panic
            && self.master.hybrid.panic.is_none()
        {
            self.master.hybrid.panic = Some(payload);
        }
        id
    }

    /// 刚放回槽位的岛按编号落地、刷出，再把排着队的事里能做的做掉。有岛 panic 了就改成
    /// 收齐别的岛、抛出。
    ///
    /// Land and flush the islands just seated, by id, then run what held work
    /// can run. If an island panicked, recall the others and re-raise instead.
    fn land_seated(&mut self, mut ids: Vec<RegionId>) {
        self.raise_island_panic();
        if ids.is_empty() {
            return;
        }
        ids.sort_unstable();
        for &id in &ids {
            if self.islands.is_parked(id) {
                self.land_island(id);
            }
        }
        for &id in &ids {
            self.flush_island(id);
        }
        self.drain_held();
    }

    /// 不等待地收下已经交回的岛并落地。
    ///
    /// Take in, without waiting, the islands that have come back, and land them.
    fn take_in_returned(&mut self) {
        let mut ids = Vec::new();
        loop {
            let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
            match pool.wait(Duration::ZERO) {
                Wait::Back(back) => ids.push(self.seat(*back)),
                Wait::Lost => {
                    panic!("every island tick thread is gone; the islands cannot come back")
                }
                Wait::Pending => break,
            }
        }
        self.land_seated(ids);
    }

    /// 有岛在 tick 线程上 panic 了：不再派任何岛，把别的还在跑的岛收回来（最多等
    /// `island_stop_timeout`），然后在主控线程上原样抛出，走世界线程原来的崩溃处理。
    /// 别的岛的数据不受影响（计划 5.5.5）。没有 panic 时什么都不做。
    ///
    /// An island panicked on its tick thread: dispatch nothing more, recall
    /// the islands still running (waiting at most `island_stop_timeout`),
    /// then re-raise on the master thread, on the world thread's existing
    /// crash path. The other islands' data is untouched (plan 5.5.5). A no-op
    /// without a panic.
    fn raise_island_panic(&mut self) {
        let Some(payload) = self.master.hybrid.panic.take() else {
            return;
        };
        error!("an island tick thread panicked; taking the world thread's crash path");
        self.recall(self.master.island_stop_timeout);
        std::panic::resume_unwind(payload);
    }

    /// 等不在主控手里的岛回来，最多等 `budget`；回来的只放回槽位。
    ///
    /// Wait up to `budget` for the islands that are away; those that return
    /// are only seated.
    fn recall(&mut self, budget: Duration) {
        let started = Instant::now();
        while !self.islands.all_parked() {
            let left = budget.saturating_sub(started.elapsed());
            if left.is_zero() {
                break;
            }
            let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
            match pool.wait(left.min(ISLAND_WAIT_SLICE)) {
                Wait::Back(back) => {
                    self.seat(*back);
                }
                Wait::Lost => break,
                Wait::Pending => {}
            }
        }
    }

    /// 关服前把不在主控手里的岛收回来（计划 5.5.5）：总共最多等 `island_stop_timeout`
    /// （等岛的时候已经等够了就不再等）。还没回来的列进日志，之后只处理停放着的岛。
    ///
    /// Before shutdown, recall the islands that are away (plan 5.5.5),
    /// waiting at most `island_stop_timeout` in total (not again if the wait
    /// for islands already used it up). Those still out are logged and only
    /// the parked islands are handled afterwards.
    pub(super) fn recall_for_shutdown(&mut self) {
        let budget = if std::mem::take(&mut self.master.hybrid.gave_up) {
            Duration::ZERO
        } else {
            self.master.island_stop_timeout
        };
        self.recall(budget);
        self.raise_island_panic();
        let away = self.islands.away_ids();
        if !away.is_empty() {
            error!(
                islands = ?away,
                timeout_secs = self.master.island_stop_timeout.as_secs_f64(),
                "islands did not hand back after the shutdown request; stopping without them, \
                 their chunks and players keep only what was saved before"
            );
        }
    }

    // ---- 一刻（计划 5.5.1） / One tick (plan 5.5.1) ----

    /// 第 ⓪ 段：收下已经交回的岛、落地，排着队的事能做的做掉。
    ///
    /// Part 0: take in the islands that are back, land them, and run what
    /// held work can run.
    pub(super) fn begin_hybrid_tick(&mut self) -> ControlFlow<()> {
        self.master.hybrid.tick += 1;
        #[cfg(test)]
        self.release_stalls();
        self.take_in_returned();
        // 这一刻没有岛刚回来时也试一遍：全部岛可能在上一刻的末尾就都停放了。
        // Try even when no island just came back: they may all have parked at
        // the end of the last tick.
        self.drain_held();
        ControlFlow::Continue(())
    }

    /// 把一个停放着的岛派给 tick 线程。
    ///
    /// Hand one parked island to a tick thread.
    fn dispatch_island(&mut self, id: RegionId) {
        #[cfg(test)]
        self.arm_stall(id);
        let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
        let slot = self.islands.slot_mut(id).expect("岛在表里");
        let mut island = slot.take();
        island.dispatch();
        let hint = island.last_worker;
        if let Err(mut island) = pool.dispatch(hint, island) {
            island.park();
            slot.put(island);
            panic!("the island tick threads are gone");
        }
    }

    /// 现在还在等的岛：不在主控手里、也不是测试故意卡住的。
    ///
    /// Whether some island is still waited for: away and not stalled on
    /// purpose by a test.
    fn awaited(&self) -> bool {
        self.islands
            .away_ids()
            .into_iter()
            .any(|id| !self.is_stalled(id))
    }

    fn is_stalled(&self, id: RegionId) -> bool {
        #[cfg(test)]
        {
            self.master
                .hybrid
                .stalls
                .iter()
                .any(|stall| stall.island == id && stall.gate.is_some())
        }
        #[cfg(not(test))]
        {
            let _ = id;
            false
        }
    }

    /// 等岛交回：等到没有还在等的岛、或者到 `deadline`；`first` 为真时回来一个就返回。
    /// 回来的岛放回槽位，返回它们的编号。没有截止时间（直接调 `World::tick`）时一直等，
    /// 每隔一段报告一次；收到关服请求之后再等 `island_stop_timeout`，还不回来就关服
    /// （不带它们），返回 `Break`。tick 线程全没了当成崩溃。
    ///
    /// Wait for islands until none is waited for or `deadline` passes; with
    /// `first`, return as soon as one is back. Returned islands are seated
    /// and their ids returned. Without a deadline (a direct `World::tick`)
    /// the wait goes on, reporting periodically; after a shutdown request it
    /// lasts `island_stop_timeout` more and then shuts down without them,
    /// returning `Break`. Losing every tick thread is a crash.
    fn await_islands(
        &mut self,
        deadline: Option<Instant>,
        first: bool,
    ) -> ControlFlow<(), Vec<RegionId>> {
        let began = Instant::now();
        let mut next_report = ISLAND_SLOW_REPORT;
        let mut stop_requested: Option<Instant> = None;
        let mut ids = Vec::new();
        while self.awaited() && !(first && !ids.is_empty()) {
            let slice = match deadline {
                Some(at) => {
                    let now = Instant::now();
                    if now >= at {
                        break;
                    }
                    (at - now).min(ISLAND_WAIT_SLICE)
                }
                None => ISLAND_WAIT_SLICE,
            };
            let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
            match pool.wait(slice) {
                Wait::Back(back) => {
                    ids.push(self.seat(*back));
                    continue;
                }
                Wait::Lost => {
                    panic!("every island tick thread is gone; the islands cannot come back")
                }
                Wait::Pending => {}
            }
            if deadline.is_some() {
                continue;
            }
            let running = began.elapsed();
            if running >= next_report {
                warn!(
                    seconds = running.as_secs(),
                    islands = ?self.islands.away_ids(),
                    "island ticks are still running"
                );
                next_report += ISLAND_SLOW_REPORT;
            }
            if stop_requested.is_none() && self.master.shutdown_requested() {
                stop_requested = Some(Instant::now());
            }
            if let Some(since) = stop_requested
                && since.elapsed() >= self.master.island_stop_timeout
            {
                self.raise_island_panic();
                self.master.hybrid.gave_up = true;
                // 关服的消息留在 `held_inbound` 里；世界循环到此结束。
                // The shutdown message stays in `held_inbound`; the loop ends here.
                self.shut_down()?;
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(ids)
    }

    /// 第 ② 段：停放着的岛全部派出（不在的岛欠一刻），主控同时推进 Director，然后等到
    /// 截止时间或者全部回来。回来的岛只放回槽位，落地在第 ③ 段按编号做，和以前
    /// 「全部等齐」的先后相同。
    ///
    /// Part 2: dispatch every parked island (absent ones owe a tick), advance
    /// the Director meanwhile, then wait until the deadline or until all are
    /// back. Returned islands are only seated; they land in part 3 by id, in
    /// the old wait-for-all order.
    pub(super) fn run_hybrid_round(&mut self) -> ControlFlow<()> {
        let dispatched = Instant::now();
        for id in self.islands.away_ids() {
            if let Some(slot) = self.islands.slot_mut(id) {
                slot.debt = slot.debt.saturating_add(1);
            }
        }
        for id in self.islands.parked_ids() {
            self.dispatch_island(id);
        }
        // 岛在 tick 线程上跑；这里只碰主控自己的状态，信件只投进邮筒。
        // The islands run on tick threads; only master state is touched
        // here and mail is only posted.
        let master_started = Instant::now();
        self.master.director_step(&mut IslandAccess::Away);
        let master_done = Instant::now();
        let deadline = self.master.hybrid.deadline;
        let flow = self.await_islands(deadline, false);
        let resumed = Instant::now();
        if flow.is_break() {
            return ControlFlow::Break(());
        }
        self.raise_island_panic();
        self.master.island_wait = resumed.saturating_duration_since(master_done);
        let profiler = self.profiler();
        profiler.restart_lap();
        let director = master_done.saturating_duration_since(master_started);
        profiler.record_phase(
            Phase::Delivery,
            director.as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        // 交接：派出到主控自己的活开始，加上主控的活做完到接着干活之间多等的那一段里
        // 岛已经回来的部分算不出来（各岛各自回来），这里只记派出本身。
        // The hand-over: only the dispatch itself is recorded here (islands
        // come back one by one, so the return side has no single figure).
        profiler.record_duration(
            Metric::IslandHandoff,
            master_started.saturating_duration_since(dispatched),
        );
        self.finish_partition_tick();
        self.record_delivery_metrics();
        ControlFlow::Continue(())
    }

    /// 第 ④ 段：还欠刻的岛在这一刻剩下的时间里追赶（计划 5.5.1）。停放着、欠刻的岛做完
    /// 「开头」里属于它自己的几步就再派出去；回来就落地；直到谁都不欠、或者到点。有等全部
    /// 岛的事排着时不派，留到下一刻开头凑齐。
    ///
    /// Part 4: islands that owe ticks catch up in what is left of this tick
    /// (plan 5.5.1). A parked island owing ticks does its own share of the
    /// tick start and is dispatched again; when it returns it lands; until
    /// nobody owes or time is up. Nothing is dispatched while work waiting
    /// for every island is held, so the islands meet at the next tick start.
    pub(super) fn catch_up(&mut self) -> ControlFlow<()> {
        let deadline = self.master.hybrid.deadline;
        loop {
            self.take_in_returned();
            #[cfg(test)]
            let allowed = !self.master.hybrid.breaks.no_catch_up;
            #[cfg(not(test))]
            let allowed = true;
            if allowed && !self.everyone_awaited() {
                for id in self.islands.parked_ids() {
                    if self.islands.debt(id) > 0 {
                        self.turn_around(id);
                    }
                }
            }
            if !self.awaited() || deadline.is_some_and(|at| Instant::now() >= at) {
                return ControlFlow::Continue(());
            }
            let ids = self.await_islands(deadline, true)?;
            self.land_seated(ids);
        }
    }

    /// 追赶时把一个停放着的岛再派出去之前，做「开头」里属于它自己的几步：收信、处理它的
    /// 玩家的入站消息、按玩家的新位置挂票、重发全局快照。处理消息可能让它被并进别的岛，
    /// 那样就不派了。
    ///
    /// Before a parked island is dispatched again to catch up, do its own
    /// share of the tick start: take mail, handle its players' inbound
    /// messages, move tickets, republish the snapshot. Handling messages may
    /// merge it into another island, in which case it is not dispatched.
    fn turn_around(&mut self, id: RegionId) {
        let Some(island) = self.islands.get_mut(id) else {
            return;
        };
        island.mailbox.begin_tick();
        self.take_island_mail(id);
        let mut batch = Vec::new();
        for player in self
            .islands
            .get(id)
            .expect("停放着")
            .region
            .players
            .values()
        {
            player.inbox.drain_into(&mut batch);
        }
        crate::inbound::merge(&mut batch);
        for Sequenced { message, .. } in batch {
            self.accept_message(message);
        }
        self.sync_player_tickets();
        if !self.islands.is_parked(id) {
            return;
        }
        self.publish_global();
        if let Some(slot) = self.islands.slot_mut(id) {
            slot.debt = slot.debt.saturating_sub(1);
        }
        self.dispatch_island(id);
    }

    // ---- 存盘（计划 5.5.5） / Saving (plan 5.5.5) ----

    /// 自动保存：存全部在线玩家和 `level.dat` / `data/`（只入队）。等全部岛停放。
    ///
    /// Autosave: every online player plus `level.dat` and `data/` (enqueue
    /// only). Waits for every island.
    pub(super) fn autosave(&mut self) {
        // 自动保存照官方 autoSave：只入队，不 join、不排阻塞栅栏（R5 / F3）。
        // Autosave like official autoSave: enqueue only, never join or block.
        self.save_all_players();
        let files = self
            .master
            .save
            .as_mut()
            .map(|save| save.encode_saves(&self.master.global, crate::admin::now_millis()));
        match files {
            Some(Ok(files)) => self.master.host.submit_file_writes(files),
            Some(Err(error)) => warn!(%error, "could not encode level data for autosave"),
            None => {}
        }
    }

    /// 把一份存盘名单分给各岛。全部岛都停放着时和以前一样按「谁加载着这个位置」分；有岛
    /// 不在主控手里时，停放的岛都没加载的位置按 Director 记的归属找岛，归属的岛不在就留在
    /// 主控手里、等它落地（「放回改动集合」那份不会标到别的岛上去）。
    ///
    /// Hand one save list out to the islands. With every island parked this
    /// splits by who has each position loaded, as before; with islands away,
    /// a position no parked island has loaded goes to its owner per the
    /// Director, and waits with the master when that owner is away (the
    /// "put back" entries never land on the wrong island).
    pub(super) fn assign_save_plan(&mut self, plan: SavePlan) {
        let away = self.islands.away_ids();
        if away.is_empty() {
            let mut islands = self.islands.all_mut();
            let plans = crate::director_host::split_plan(&islands, plan);
            for (island, plan) in islands.iter_mut().zip(plans) {
                island.save_plan = plan;
            }
            return;
        }
        let SavePlan {
            remark,
            encode,
            metrics,
            drain,
            #[cfg(test)]
            verify,
        } = plan;
        let parked = self.islands.parked_ids();
        let mut plans: BTreeMap<RegionId, SavePlan> = parked
            .iter()
            .chain(&away)
            .map(|&id| {
                (
                    id,
                    SavePlan {
                        drain,
                        ..SavePlan::default()
                    },
                )
            })
            .collect();
        // 都没加载、归属也查不到的位置：给第一个停放着的岛（编出来是 `None`，下次重试）；
        // 一个停放的都没有就给第一个不在的岛。
        // Positions nobody has loaded and no owner is known for go to the
        // first parked island (they encode to `None` and are retried), or to
        // the first absent one when nothing is parked.
        let fallback = parked.first().copied().unwrap_or(away[0]);
        let owner = |world: &Self, pos: ChunkPos| -> RegionId {
            if let Some(id) = world.islands.island_of_chunk(pos) {
                return id;
            }
            match world.master.host.director().owner_at(pos) {
                Some(Owner::Region(id)) if away.contains(&id) => id,
                _ => fallback,
            }
        };
        for pos in remark {
            let id = owner(self, pos);
            plans.entry(id).or_default().remark.push(pos);
        }
        for pos in encode {
            let id = owner(self, pos);
            plans.entry(id).or_default().encode.push(pos);
        }
        #[cfg(test)]
        for (pos, revision) in verify {
            let id = owner(self, pos);
            plans.entry(id).or_default().verify.push((pos, revision));
        }
        plans.entry(fallback).or_default().metrics = metrics;
        for (id, plan) in plans {
            match self.islands.is_parked(id) {
                true => self
                    .islands
                    .get_mut(id)
                    .expect("停放着")
                    .save_plan
                    .absorb(plan),
                false => self.master.hybrid.plans.entry(id).or_default().absorb(plan),
            }
        }
    }

    // ---- 测试专用（计划 5.5.7） / Test only (plan 5.5.7) ----

    /// 测试专用：从现在起的第 `first` 到 `last` 个混合驱动的刻（1 = 下一刻），岛 `island`
    /// 派出去以后不回来；过了以后主控开闸、等它回来（计划 5.5.7）。
    ///
    /// Test only: for the `first`-th to `last`-th hybrid tick from now (1 is
    /// the next one), island `island` does not come back once dispatched;
    /// afterwards the master opens the gate and waits for it (plan 5.5.7).
    #[cfg(test)]
    pub(crate) fn stall_island(&mut self, island: RegionId, first: u64, last: u64) {
        assert!(first >= 1 && last >= first);
        let now = self.master.hybrid.tick;
        self.master.hybrid.stalls.push(Stall {
            island,
            from: now + first,
            to: now + last,
            gate: None,
        });
    }

    /// 测试专用：岛在登记的那几刻里派出时，让它在 tick 线程上这一刻开头等闸。
    #[cfg(test)]
    fn arm_stall(&mut self, id: RegionId) {
        let tick = self.master.hybrid.tick;
        let Some(stall) = self.master.hybrid.stalls.iter_mut().find(|stall| {
            stall.island == id && stall.gate.is_none() && (stall.from..=stall.to).contains(&tick)
        }) else {
            return;
        };
        let gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        stall.gate = Some(std::sync::Arc::clone(&gate));
        self.islands.get_mut(id).expect("停放着").test_hook =
            Some(crate::island::IslandTestHook::Stall(gate));
    }

    /// 测试专用：过了登记的那几刻的岛开闸，并等它回来（放回槽位，落地由调用方接着做）。
    #[cfg(test)]
    fn release_stalls(&mut self) {
        let tick = self.master.hybrid.tick;
        let (due, kept): (Vec<Stall>, Vec<Stall>) = std::mem::take(&mut self.master.hybrid.stalls)
            .into_iter()
            .partition(|stall| tick > stall.to);
        self.master.hybrid.stalls = kept;
        let mut ids = Vec::new();
        for stall in due {
            let Some(gate) = stall.gate else {
                continue;
            };
            gate.store(true, std::sync::atomic::Ordering::Release);
            while !self.islands.is_parked(stall.island) {
                let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
                match pool.wait(ISLAND_WAIT_SLICE) {
                    Wait::Back(back) => ids.push(self.seat(*back)),
                    Wait::Lost => panic!("every island tick thread is gone"),
                    Wait::Pending => {}
                }
            }
        }
        self.land_seated(ids);
    }
}
