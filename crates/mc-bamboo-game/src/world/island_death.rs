//! 岛卡死的兜底：判死（岛的实施计划 5.6）。
//!
//! 混合驱动让慢岛只拖慢自己，但再也不回来的岛会让等全部岛的事永远做不了、它的出站队列
//! 无限增长。这里：
//!
//! - 判定（[`World::bury_overdue`]）：一个岛的同一刻在 tick 线程上连续跑了超过
//!   `island_dead_timeout`，或者主控给它攒的出站投递超过上限，或者关服时到点没回来；
//! - 判死的那一刻（[`World::bury`]）：写错误日志，线程池放弃那条线程并补一条，槽位从
//!   岛表里拿掉（家岛换一个空的），经进服时留下的第二个出口把它的玩家踢掉，丢掉主控手里
//!   只属于它的东西，撤它的玩家的票，Director 里它拿着的区块按「盒子没了」处理、票还要的
//!   重新读盘；
//! - 岛都落地以后（[`World::hold_funerals`]）：把「离开了游戏」和 Tab 名单的移除发给
//!   别的玩家；
//! - 死岛后来回来了：派出号对不上任何槽位，整个丢弃（`World::seat`）。
//!
//! The fallback for a stuck island: declaring it dead (island plan 5.6). The
//! hybrid drive lets a slow island slow only itself, but one that never
//! returns blocks all work that waits for every island and grows its outbound
//! queue without bound. This module holds the verdict
//! ([`World::bury_overdue`]: a tick running past `island_dead_timeout` on its
//! thread, an outbound queue past its limit, or not back in time at
//! shutdown), what happens at once ([`World::bury`]: log the error, abandon
//! and replace the thread, drop the slot or swap in an empty home, kick its
//! players through the second exits kept at join, drop what the master held
//! for it alone, drop its players' tickets and treat its chunks as lost boxes
//! so wanted ones read from disk again) and what waits until the islands have
//! landed ([`World::hold_funerals`]: announce the departures). A dead island
//! that comes back later matches no slot by ticket and is discarded
//! (`World::seat`).

use std::{collections::BTreeSet, time::Duration};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_protocol::text::Component;
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp, owner_is_held};
use tracing::{error, warn};

use super::{SERVER_SHUTDOWN, World, island_access, island_hybrid::Source};
use crate::{global_port::GlobalEffect, inbound::PlayerId, island::Borrow};

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
#[cfg_attr(not(test), allow(dead_code))]
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

/// 判死之后还没办的事：让别的玩家看到它的玩家离开。要借全部玩家，等岛都落地了再做。
///
/// What is left after a death: showing the others that its players left.
/// It borrows every player, so it waits until the islands have landed.
struct Funeral {
    /// 它上面的玩家。
    players: Vec<PlayerId>,
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

    /// 把不在主控手里的岛 `id` 判死，做当场能做的那一部分（岛的实施计划 5.6.2 的第 1–5
    /// 步）：写错误日志、放弃那条线程、拿掉槽位（家岛换一个空的）、踢它的玩家、丢掉主控
    /// 手里只属于它的东西，并且当场算好它拿着的区块；票和 Director 留给
    /// [`Self::hold_funerals`]，等岛都落地了再做。岛停放着时什么都不做。
    ///
    /// 岛还排在线程池的队列里没开跑时不判死：把它原样拿回来、落地（它没卡住，只是没
    /// 轮到线程）。这一刻已经跑完、交回还在路上时也不判死：它马上就落地了。
    ///
    /// Declare island `id`, which is away, dead and do what can be done at
    /// once (steps 1–5 of island plan 5.6.2): log the error, abandon the
    /// thread, drop the slot (an empty home replaces home), kick its players,
    /// drop what the master held for it alone, and work out the chunks it
    /// held right now; tickets and the Director wait for
    /// [`Self::hold_funerals`] once the islands have landed. A no-op for a
    /// parked island. An island still queued in the pool, not started, is not
    /// dead: it is taken back as it is and landed (it is not stuck, it just
    /// had no thread yet). Neither is one whose tick just finished and whose
    /// hand-back is on its way: it lands in a moment.
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
        if !pool.abandon(ticket) {
            // 已经跑完、交回在通道里：下一次收岛时照常落地（出站队列也就发掉了）。
            // Already finished, the hand-back is in the channel: it lands at
            // the next take-in as usual (and its outbound queue goes out).
            return;
        }
        // 它拿着的区块（计划 5.6.3）：Director 记在它名下的、派出时还没回执的、以及
        // 收回信还在邮筒里没取的。现在就算：家岛马上换成同编号的新岛，以后再按编号查会
        // 把新岛的区块算进来。
        // Its chunks (plan 5.6.3): those the Director records under it, those
        // not yet acknowledged at dispatch, and those whose reclaim letter is
        // still posted. Worked out now: home is replaced by a new island with
        // the same id at once, and a later lookup by id would include its chunks.
        let chunks = self.chunks_of_dead(id);
        // 家岛要换一个空的：先给空白岛重发全局快照。
        // Home is replaced by an empty one: republish to the blank island first.
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
            player_count = buried.players.len(),
            players = ?names,
            chunk_count = chunks.len(),
            ?chunks,
            "an island stopped responding and was declared dead: its players were disconnected, \
             its chunks will be reloaded from their last save, and every change to those chunks \
             and those players since their last save is lost"
        );
        let deaths = &mut self.master.hybrid.death.deaths;
        if deaths.len() >= DEATHS_KEPT {
            deaths.remove(0);
        }
        deaths.push(Death {
            island: id,
            cause,
            running_for,
            ticks_owed: buried.debt,
            players: buried.players.clone(),
            chunks: chunks.clone(),
        });
        // 撤它的玩家的票（票中心和方向一起清），再把它拿着的区块按「盒子没了」交给
        // Director：先撤票，只有这些玩家要的区块就不用再从磁盘读一遍。这两步只推 Director、
        // 往邮筒投信，有岛不在、刚交回的岛还没落地时也能做；当场做，Director 的记录和岛表
        // 就不会有一段对不上。拆合（分区器因此定下的）照常留到下一次 `settle_islands`。
        // Drop its players' tickets (centres and directions too), then hand
        // the chunks it held to the Director as lost boxes; tickets first, so
        // chunks only these players wanted are not read again. Both only drive
        // the Director and post mail, so they are safe while islands are away
        // or seated but not landed; done at once, the Director's records never
        // disagree with the island table. Structural changes the partitioner
        // decides because of it wait for the next `settle_islands` as usual.
        for player in &buried.players {
            self.master.player_directions.remove(player);
            self.master.ticket_centers.remove(player);
        }
        let ops: Vec<TicketOp> = buried
            .players
            .iter()
            .map(|player| TicketOp::PlayerMoved {
                id: u64::from(player.0 as u32),
                to: None,
            })
            .collect();
        let mut access = island_access(self.master.drive, &mut self.islands);
        if !ops.is_empty() {
            self.master.host.tickets(&mut access, ops);
        }
        self.master.host.lose_chunks(&mut access, &chunks);
        self.publish_player_demand();
        // 关服时不用让别人看到离开（大家马上都被踢）。
        // Not at shutdown, where everyone is about to be kicked.
        if cause != DeathCause::Shutdown && !buried.players.is_empty() {
            self.master.hybrid.death.funerals.push(Funeral {
                players: buried.players,
            });
        }
    }

    /// 刚判死的岛 `id` 拿着的区块（见 [`Self::bury`]），只留 Director 还记在 region 一侧的，
    /// 升序。
    ///
    /// The chunks island `id`, being declared dead, holds (see
    /// [`Self::bury`]), keeping those the Director still records on the
    /// region side, ascending.
    fn chunks_of_dead(&self, id: RegionId) -> Vec<ChunkPos> {
        let director = self.master.host.director();
        let mut chunks: BTreeSet<ChunkPos> = director
            .owner_records()
            .into_iter()
            .filter(|record| record.owner == Owner::Region(id))
            .map(|record| record.pos)
            .collect();
        if let Some(away) = self.islands.away(id) {
            chunks.extend(away.unacked.iter().copied());
        }
        chunks.extend(self.master.host.posted_reclaims(id));
        chunks.retain(|&pos| director.owner_at(pos).is_some_and(owner_is_held));
        chunks.into_iter().collect()
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
        self.flush_island(id);
    }

    /// 让别的玩家看到判死的岛上的玩家离开（岛的实施计划 5.6.2 第 6 步的后一半）：和正常
    /// 离开相同的两条全局改动。只在岛都落地了的时点调用（排着队的事开做之前）。有停放着
    /// 的岛就当场发（和正常离开一样，不在的岛经出站队列补发），同一账号马上重进时「离开」
    /// 一定排在他重进之前；一个停放的岛都没有时作为主控的一件事排队。
    ///
    /// Show the others that a dead island's players left (the second half of
    /// step 6 of island plan 5.6.2): the same two global changes as a normal
    /// leave. Only called where the islands have landed (before held work
    /// runs). With some island parked it happens at once (as a normal leave;
    /// absent islands get it through their outbound queues), so the leave
    /// always precedes the same account joining again; with none parked it
    /// is queued as master work.
    pub(super) fn hold_funerals(&mut self) {
        for Funeral { players } in std::mem::take(&mut self.master.hybrid.death.funerals) {
            if self.islands.template().is_some() {
                self.announce_departures(&players);
            } else {
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
