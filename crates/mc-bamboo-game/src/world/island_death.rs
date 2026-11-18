//! 岛卡死的兜底：判死（岛的实施计划 5.6）。
//!
//! 混合驱动让慢岛只拖慢自己，但再也不回来的岛会让等全部岛的事永远做不了、它的出站队列
//! 无限增长。这里：
//!
//! - 判定（[`World::bury_overdue`]）：一个岛的同一刻在 tick 线程上连续跑了超过
//!   `island_dead_timeout`，或者主控给它攒的出站投递超过上限，或者关服时到点没回来；
//! - 判死的那一刻（[`World::bury`]）：线程池放弃那条线程并补一条，槽位从岛表里拿掉
//!   （家岛换一个空的），经进服时留下的第二个出口把它的玩家踢掉，丢掉主控手里只属于它
//!   的东西；
//! - 后事（[`World::hold_funerals`]，在岛都落地了的时点做）：撤它的玩家的票，Director 里
//!   它拿着的区块按「盒子没了」处理、票还要的重新读盘，写错误日志，把「离开了游戏」和
//!   Tab 名单的移除排给全部玩家；
//! - 死岛后来回来了：派出号对不上任何槽位，整个丢弃（`World::seat`）。
//!
//! The fallback for a stuck island: declaring it dead (island plan 5.6). The
//! hybrid drive lets a slow island slow only itself, but one that never
//! returns blocks all work that waits for every island and grows its outbound
//! queue without bound. This module holds the verdict
//! ([`World::bury_overdue`]: a tick running past `island_dead_timeout` on its
//! thread, an outbound queue past its limit, or not back in time at
//! shutdown), what happens at once ([`World::bury`]: abandon and replace the
//! thread, drop the slot or swap in an empty home, kick its players through
//! the second exits kept at join, drop what the master held for it alone) and
//! the rest done once the islands have landed ([`World::hold_funerals`]: drop
//! its players' tickets, treat its chunks as lost boxes so wanted ones read
//! from disk again, log the error, announce the departures). A dead island
//! that comes back later matches no slot by ticket and is discarded
//! (`World::seat`).

use std::time::Duration;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_protocol::text::Component;
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp};
use tracing::{error, warn};

use super::{SERVER_SHUTDOWN, World, island_access, island_hybrid::Source};
use crate::{
    global_port::GlobalEffect,
    inbound::PlayerId,
    island::{Borrow, Buried},
};

/// 主控给一个不在手里的岛攒的出站投递最多这么多条（岛的实施计划 5.6.5）。
///
/// The most outbound deliveries the master queues for one island that is away
/// (island plan 5.6.5).
pub(crate) const MAX_FAN_OUT_ITEMS: usize = 16_384;

/// 同上，按帧的总字节数。
///
/// The same limit by total frame bytes.
pub(crate) const MAX_FAN_OUT_BYTES: usize = 16 * 1024 * 1024;

/// 主控记着的判死记录最多留这么多条（只给诊断和测试看）。
const DEATHS_KEPT: usize = 64;

/// 被判死的岛上的玩家看到的断开原因。
///
/// The disconnect reason shown to the players of an island declared dead.
pub(crate) const DEAD_ISLAND_REASON: &str = "The part of the world you were in stopped responding and was unloaded. \
     Changes there since the last save were lost. Please reconnect.";

/// 为什么判死。
///
/// Why an island was declared dead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeathCause {
    /// 同一刻在 tick 线程上跑了超过 `island_dead_timeout`。
    ///
    /// One tick ran on its thread for longer than `island_dead_timeout`.
    Timeout,
    /// 主控给它攒的出站投递超过了上限。
    ///
    /// The outbound deliveries queued for it passed the limit.
    OutboundFull,
    /// 关服请求之后到点没回来。
    ///
    /// It was not back in time after the shutdown request.
    Shutdown,
}

/// 一次判死的记录（日志里写的那些）。
///
/// The record of one death (what the log says).
#[derive(Clone, Debug)]
pub(crate) struct Death {
    pub(crate) island: RegionId,
    pub(crate) cause: DeathCause,
    /// 这一刻在线程上已经跑了多久；还没开跑（排着队）时是 `None`。
    pub(crate) running_for: Option<Duration>,
    pub(crate) ticks_owed: u32,
    pub(crate) players: Vec<PlayerId>,
    /// 它拿着、因此丢了上次存盘以来改动的区块，升序。
    pub(crate) chunks: Vec<ChunkPos>,
}

/// 判死之后还没办的后事。
struct Funeral {
    island: RegionId,
    cause: DeathCause,
    running_for: Option<Duration>,
    buried: Buried,
    /// 玩家的名字（判死时从全局名单里抄下来，日志用）。
    names: Vec<String>,
}

/// 判死这一部分的状态，只由主控线程读写。
///
/// The state of this module, read and written by the master thread only.
#[derive(Default)]
pub(crate) struct DeathState {
    /// 每名在线玩家的第二个出口（`PlayerSink::second_handle`）：他的出站链路跟着他在岛
    /// 上，岛卡死时主控够不着，靠这一个把他断开。只有混合驱动留。
    ///
    /// Every online player's second exit (`PlayerSink::second_handle`). The
    /// player's link lives on their island and is out of reach when that
    /// island is stuck; this is how the master disconnects them. Kept under
    /// the hybrid drive only.
    pub(crate) exits: std::collections::BTreeMap<PlayerId, Box<dyn crate::inbound::PlayerSink>>,
    /// 出站队列超了上限、等下一个判定点判死的岛。
    ///
    /// Islands whose outbound queue passed the limit, to be declared dead at
    /// the next check.
    pub(crate) doomed: std::collections::BTreeSet<RegionId>,
    /// 每个不在手里的岛的出站队列里帧的总字节数。
    pub(crate) fan_out_bytes: std::collections::BTreeMap<RegionId, usize>,
    /// 已经判死、后事还没办的岛。
    funerals: Vec<Funeral>,
    /// 判死记录，最近的在后。
    pub(crate) deaths: Vec<Death>,
    /// 死岛后来回来、被丢弃的次数。
    pub(crate) zombies: u64,
    /// 测试专用：出站队列的上限（条数，字节）。
    #[cfg(test)]
    pub(crate) fan_out_limit: Option<(usize, usize)>,
    /// 测试专用：被判死的岛的闸（[`World::release_stalls_for_test`] 开闸让它们回来）。
    #[cfg(test)]
    pub(crate) zombie_gates: Vec<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl DeathState {
    /// 出站队列的上限（条数，字节）。
    pub(crate) fn fan_out_limit(&self) -> (usize, usize) {
        #[cfg(test)]
        if let Some(limit) = self.fan_out_limit {
            return limit;
        }
        (MAX_FAN_OUT_ITEMS, MAX_FAN_OUT_BYTES)
    }
}

impl World {
    /// 判定点（岛的实施计划 5.6.1）：同一刻在线程上跑得超过 `island_dead_timeout` 的岛，
    /// 和出站队列超了上限、还没回来的岛，判死。只在混合驱动下做。调用之前要先收下已经
    /// 交回的岛，刚好跑完的不会被误判（跑完的那一刻线程池已经不把它算作「正在跑」）。
    ///
    /// The check point (island plan 5.6.1): islands whose tick has run past
    /// `island_dead_timeout` on its thread, and islands whose outbound queue
    /// passed the limit and that are still away, are declared dead. Hybrid
    /// drive only. Islands already handed back are taken in first, so one that
    /// just finished is never misjudged (the pool stops counting a tick as
    /// running the moment it ends).
    pub(super) fn bury_overdue(&mut self) {
        if !self.hybrid() {
            return;
        }
        let Some(pool) = self.master.pool.as_ref() else {
            return;
        };
        for ticket in pool.overdue(self.master.island_dead_timeout) {
            if let Some(id) = self.islands.island_of_ticket(ticket) {
                self.bury(id, DeathCause::Timeout);
            }
        }
        for id in std::mem::take(&mut self.master.hybrid.death.doomed) {
            self.bury(id, DeathCause::OutboundFull);
        }
    }

    /// 把不在主控手里的岛 `id` 判死，做当场能做的那一半（岛的实施计划 5.6.2 的第 2–5
    /// 步）；剩下的（票、区块、日志、让别的玩家看到他们离开）留给
    /// [`Self::hold_funerals`]，等岛都落地了再做。岛停放着时什么都不做。
    ///
    /// 岛还排在线程池的队列里没开跑时不判死：把它原样拿回来、落地（它没卡住，只是没
    /// 轮到线程）。
    ///
    /// Declare island `id`, which is away, dead and do the half that can be
    /// done at once (steps 2–5 of island plan 5.6.2); the rest (tickets,
    /// chunks, the log, showing the departures to others) waits for
    /// [`Self::hold_funerals`] once the islands have landed. A no-op for a
    /// parked island. An island still queued in the pool, not started, is not
    /// dead: it is taken back as it is and landed (it is not stuck, it just
    /// had no thread yet).
    pub(super) fn bury(&mut self, id: RegionId, cause: DeathCause) {
        let Some(ticket) = self.islands.away_ticket(id) else {
            return;
        };
        let pool = self.master.pool.as_mut().expect("分离驱动一定有线程池");
        if let Some(mut island) = pool.withdraw(ticket) {
            // 没开跑：拿回来放回槽位。它欠着派出时该跑的那一刻。
            // Never started: seat it again; it owes the tick it was dispatched for.
            island.park();
            let slot = self.islands.slot_mut(id).expect("岛在表里");
            slot.put(island);
            slot.debt = slot.debt.saturating_add(1);
            self.land_withdrawn(id);
            return;
        }
        let running_for = pool.running_for(ticket);
        pool.abandon(ticket);
        // 家岛要换一个空的：先给空白岛重发全局快照，新家岛的随机源按现在的游戏时间派生。
        // Home is replaced by an empty one: republish to the blank island
        // first so the new home's random sources derive from the current time.
        if id == HOME_REGION
            && let Some(region) = self.islands.blueprint_region_mut()
        {
            self.master.global.publish(region);
        }
        let Some(buried) = self.islands.bury(id) else {
            return;
        };
        let state = &mut self.master.hybrid;
        state.fan_out.remove(&id);
        state.plans.remove(&id);
        state.death.fan_out_bytes.remove(&id);
        state.death.doomed.remove(&id);
        #[cfg(test)]
        self.retire_stalls(id);
        let reason = match cause {
            DeathCause::Shutdown => Component::translate(SERVER_SHUTDOWN, []),
            DeathCause::Timeout | DeathCause::OutboundFull => Component::text(DEAD_ISLAND_REASON),
        };
        let mut names = Vec::with_capacity(buried.players.len());
        for &player in &buried.players {
            // 玩家的出站链路在死岛里；经进服时留下的第二个出口发断开包、关连接。连接层的
            // 出站队列收到关闭就结束，那条线程之后再写的帧都被丢掉。
            // The player's link is inside the dead island; the second exit
            // kept at join sends the disconnect and closes. The connection's
            // outbound queue ends at the close, so frames the stuck thread
            // writes later are dropped.
            if let Some(mut exit) = self.master.hybrid.death.exits.remove(&player) {
                crate::links::kick_through(exit.as_mut(), &reason);
            }
            names.push(
                self.master
                    .global
                    .roster_name(player)
                    .unwrap_or("?")
                    .to_owned(),
            );
            self.master.transit.remove(&player);
            // 他排着队的入站消息（连同占着的在途配额）作废。
            // Their held inbound messages (and the quota they hold) are void.
            self.master.hybrid.forget_source(Source::Player(player));
        }
        error!(
            island = id,
            ?cause,
            running_secs = running_for.map(|time| time.as_secs_f64()),
            ticks_owed = buried.debt,
            players = buried.players.len(),
            "an island stopped responding and was declared dead; its players are disconnected \
             and its chunks will be reloaded from the last save"
        );
        self.master.hybrid.death.funerals.push(Funeral {
            island: id,
            cause,
            running_for,
            buried,
            names,
        });
    }

    /// 从线程池的队列里拿回来的岛（没开跑）落地：和平常回来的岛一样先发排给它的出站投递、
    /// 收下留给它的存盘名单，再交回。它的出站队列因此清空，不用判死。
    ///
    /// Land an island taken back from the pool's queue before it started, as
    /// any returning island: its queued outbound deliveries go out, it takes
    /// the save list kept for it, then hands back. That empties its outbound
    /// queue, so it need not die.
    fn land_withdrawn(&mut self, id: RegionId) {
        self.master.hybrid.death.doomed.remove(&id);
        self.land_island(id);
        if let Some(island) = self.islands.get_mut(id) {
            let _ = island.region.links.flush();
        }
    }

    /// 办后事（岛的实施计划 5.6.2 的第 6–7 步和日志）。只在岛都落地了的时点调用
    /// （排着队的事开做之前）：这里会动票、推 Director、照做拆合。
    ///
    /// The rest of a death (steps 6–7 of island plan 5.6.2 and the log). Only
    /// called where the islands have landed (before held work runs): this
    /// moves tickets, drives the Director and replays structural changes.
    pub(super) fn hold_funerals(&mut self) {
        if self.master.hybrid.death.funerals.is_empty() {
            return;
        }
        for funeral in std::mem::take(&mut self.master.hybrid.death.funerals) {
            let Funeral {
                island,
                cause,
                running_for,
                buried,
                names,
            } = funeral;
            // 它拿着的区块：Director 记在它名下的，加上派出时还没回执的那些。
            // Its chunks: those the Director records under it, plus the ones
            // not yet acknowledged at dispatch.
            let mut positions = buried.away.unacked.clone();
            positions.extend(
                self.master
                    .host
                    .director()
                    .owner_records()
                    .into_iter()
                    .filter(|record| record.owner == Owner::Region(island))
                    .map(|record| record.pos),
            );
            let chunks = self.master.host.lose_chunks(
                &mut island_access(self.master.drive, &mut self.islands),
                &positions,
            );
            // 撤它的玩家的票（票中心一起清）；方向和需求快照跟着更新。
            // Drop its players' tickets (and centres); directions and the
            // demand snapshot follow.
            for player in &buried.players {
                self.master.player_directions.remove(player);
            }
            self.apply_ticket_ops(buried.players.iter().map(|player| TicketOp::PlayerMoved {
                id: u64::from(player.0 as u32),
                to: None,
            }));
            self.publish_player_demand();
            error!(
                island,
                ?cause,
                running_secs = running_for.map(|time| time.as_secs_f64()),
                ticks_owed = buried.debt,
                player_count = buried.players.len(),
                players = ?names,
                chunk_count = chunks.len(),
                ?chunks,
                "a dead island's chunks were dropped: every change to them since their last \
                 save is lost, and so is what its players did since their last save"
            );
            let deaths = &mut self.master.hybrid.death.deaths;
            if deaths.len() >= DEATHS_KEPT {
                deaths.remove(0);
            }
            deaths.push(Death {
                island,
                cause,
                running_for,
                ticks_owed: buried.debt,
                players: buried.players.clone(),
                chunks,
            });
            // 别的玩家要看到他们离开：和正常离开相同的两条全局改动，作为主控的一件事
            // 排队（和排着的控制台命令保持先后）。关服时不用（大家马上都被踢）。
            // The others must see them leave: the same two global changes as
            // a normal leave, queued as master work (keeping its order with
            // held console commands). Not at shutdown, where everyone is
            // about to be kicked.
            if cause != DeathCause::Shutdown && !buried.players.is_empty() {
                let players = buried.players;
                self.run_or_hold(Box::new(move |world: &mut Self| {
                    world.announce_departures(&players);
                }));
            }
        }
    }

    /// 让全部玩家看到 `players` 离开了：系统消息、Tab 名单、Boss 栏的在线集合。
    ///
    /// Show everyone that `players` left: the system message, the tab list
    /// and the boss bars' online sets.
    fn announce_departures(&mut self, players: &[PlayerId]) {
        let island = self.islands.template().unwrap_or(HOME_REGION);
        for &player in players {
            self.with_players(island, Borrow::All, |port, region| {
                port.submit(region, GlobalEffect::LeaveMessage { player });
                port.submit(region, GlobalEffect::Left { player });
            });
        }
    }

    /// 一个岛回来了但派出号对不上它的槽位：它被判死过（岛的实施计划 5.6.1）。整个丢弃，
    /// 不并回世界。
    ///
    /// An island came back but its ticket matches no slot: it was declared
    /// dead (island plan 5.6.1). It is discarded whole, never merged back.
    pub(super) fn discard_zombie(&mut self, island: Box<crate::island::Island>, ticket: u64) {
        self.master.hybrid.death.zombies += 1;
        warn!(
            island = island.id,
            ticket,
            chunks = island.region.chunks.loaded_count().unwrap_or(0),
            players = island.region.players.len(),
            "an island that was declared dead came back; discarding it"
        );
        drop(island);
    }

    /// 测试专用：把岛 `id` 这一刻在线程上的开跑时刻往前拨 `by`，下一次判定就当它已经跑了
    /// 这么久（岛的实施计划 5.6.7）。返回它是不是真的在跑。
    ///
    /// Test only: move the start of island `id`'s running tick back by `by`,
    /// so the next check sees it as running that long (island plan 5.6.7).
    /// Returns whether it really is running.
    #[cfg(test)]
    pub(crate) fn age_island(&mut self, id: RegionId, by: Duration) -> bool {
        let Some(ticket) = self.islands.away_ticket(id) else {
            return false;
        };
        let pool = self.master.pool.as_ref().expect("分离驱动一定有线程池");
        // 派出以后线程要过一小会儿才取到它：等它真的开跑。
        // The thread takes a moment to pick it up after dispatch.
        let waited = std::time::Instant::now();
        while !pool.is_running(ticket) {
            if waited.elapsed() > Duration::from_secs(60) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        pool.backdate(ticket, by)
    }
}
