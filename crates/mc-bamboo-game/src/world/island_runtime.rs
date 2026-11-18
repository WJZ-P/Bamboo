//! 多岛在运行时的拆合和跨岛（岛的实施计划第 6 节第 10 步、3.2、4.1）。
//!
//! 分区器在主控的 `pump` 里定下合并和拆分（[`TopologyOp`]），宿主先扣住卷进去的岛的信；
//! 这里在全部岛停放的时点（齐步：每次宿主调用之后；分离：岛交回之后）照着做：
//!
//! 1. 按先后照做每一条拆合：合并把被吸收方整个倒进吸收方（时钟差平移），拆分按 section
//!    把区块、信箱里的信、玩家分到新岛（随机源由世界种子和新岛按（x，z）最小的 section
//!    派生，计划 3.3「岛的随机源」第 1 条）。邮筒里还没收的信跟着改投，Director 里的
//!    `Owner::Region` 改记。拆分要等两边的视野和配对都分得开（有人正在跨岛时晚一点拆）。
//! 2. 站到别的岛的区块上的玩家（传送、`/tp`）等原来的岛把他的视野、实体配对、菜单清干净
//!    （通常就是传送之后那一刻），再整个搬到新岛（计划 4.1「离境和到达」）；久等不干净就
//!    强制清掉再搬。
//! 3. 拆掉空了的岛，放出扣着的信（齐步当场收）。
//! 4. 调试构建断言计划 6.2 第 3 条的不变量。
//!
//! Splits and merges at run time and island crossings (island plan, step 10,
//! 3.2, 4.1). The partitioner decides merges and splits inside the master's
//! pump and the host holds the affected islands' mail; here, at a point where
//! every island is parked, the master replays them, moves players standing
//! on another island's chunk once their old island has cleared their view
//! and pairings, drops empty islands, releases the held mail and (in debug
//! builds) asserts the invariants of plan 6.2 item 3.

use std::collections::BTreeSet;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::director::{HOME_REGION, RegionId};
use tracing::warn;

use super::{World, island_access};
use crate::{
    inbound::PlayerId,
    island_partition::{SectionPos, TopologyOp},
    profiler::Metric,
};

/// 跨岛的玩家最多等这么多刻，原来的岛还没把他清干净就强制清掉再搬。正常情况下传送
/// 之后那一刻就清干净了。
///
/// How many ticks a crossing player may wait for the old island to clear
/// them before being cleared forcibly. Normally the tick after the teleport
/// is enough.
const TRANSIT_PATIENCE: u64 = 40;

/// 一次照做里最多来回几轮（放信可能让分区器又定下新的合并）。
const SETTLE_ROUNDS: usize = 8;

/// 测试专用：拆合之后故意弄坏一个新生成的实体（改坏确认）。正式构建里不存在。
///
/// Test only: corrupt one freshly spawned entity after a split or merge
/// (break-it check). Absent in production builds.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshEntityBreak {
    /// 拆出来的岛里丢掉一个下落方块实体。
    LoseEntity,
    /// 拆出来的岛里清掉一个新实体的追踪配对。
    LoseTracking,
}

impl World {
    /// 照做分区器定下的拆合、搬跨岛的玩家、拆掉空岛、放出扣着的信（见模块说明）。
    /// 只在多岛开着、全部岛都停放时做；别的时候什么都不做。
    ///
    /// Replay the partitioner's splits and merges, move crossing players, drop
    /// empty islands and release held mail (see the module docs). Only with
    /// multi-island mode on and every island parked.
    pub(crate) fn settle_islands(&mut self) {
        if !self.master.islands_mode || !self.islands.all_parked() {
            return;
        }
        let mut crossings = 0;
        for _ in 0..SETTLE_ROUNDS {
            let mut changed = false;
            while let Some(op) = self.master.host.peek_topology().cloned() {
                let started = std::time::Instant::now();
                let (done, metric) = match &op {
                    TopologyOp::Merge { into, from } => {
                        self.apply_merge(*into, *from);
                        (true, Metric::IslandMerge)
                    }
                    TopologyOp::Split { from, to, sections } => {
                        (self.apply_split(*from, *to, sections), Metric::IslandSplit)
                    }
                };
                if !done {
                    break;
                }
                let (_, waited) = self.master.host.pop_topology().expect("刚看过");
                let profiler = &mut self.islands.home_mut().region.profiler;
                profiler.record_duration(metric, started.elapsed());
                profiler.record(Metric::IslandTopologyWait, waited);
                changed = true;
            }
            let moved = self.relocate_players();
            crossings += moved;
            #[cfg(test)]
            if changed && let Some(break_kind) = self.master.split_break {
                self.apply_fresh_entity_break(break_kind);
            }
            self.islands.drop_empty();
            if !changed && moved == 0 {
                break;
            }
            self.master
                .host
                .release_mail(&mut island_access(self.master.drive, &mut self.islands));
            if !self.master.host.has_topology() {
                break;
            }
        }
        let count = self.islands.len() as u64;
        let profiler = &mut self.islands.home_mut().region.profiler;
        profiler.record(Metric::IslandCount, count);
        profiler.record(Metric::IslandCrossings, crossings as u64);
        #[cfg(debug_assertions)]
        self.assert_island_invariants();
    }

    /// 改坏确认：拆合之后在刚变化的岛上丢掉一个新生成的下落方块实体，或者清掉它的追踪
    /// 配对，用来确认「含新实体」的对照会抓到。正式构建里不存在。
    ///
    /// Break-it check: after a structural change, drop one fresh falling-block
    /// entity or clear its tracking entry on the changed island, to confirm
    /// the fresh-entity comparison catches it. Absent in production builds.
    #[cfg(test)]
    fn apply_fresh_entity_break(&mut self, kind: FreshEntityBreak) {
        use mc_bamboo_blocks::entity::KindPublic;

        let fresh: Vec<(RegionId, i32)> = self
            .islands
            .iter()
            .flat_map(|island| {
                island
                    .region
                    .entities
                    .iter_public()
                    .filter(|public| matches!(public.kind, KindPublic::FallingBlock { .. }))
                    .map(|public| (island.id, public.uid))
                    .collect::<Vec<_>>()
            })
            .collect();
        let Some(&(id, uid)) = fresh.first() else {
            return;
        };
        let island = self.islands.get_mut(id).expect("在表里");
        match kind {
            FreshEntityBreak::LoseEntity => island.region.entities.remove(uid),
            FreshEntityBreak::LoseTracking => {
                // 清掉配对：这一刻它的观察者名单空了，和对照世界的名单对不上。
                // Clear the pairing: its watcher list is now empty and differs
                // from the control world's.
                island.region.entity_tracking.forget_watchers(uid);
            }
        }
    }

    /// 把岛 `from` 整个倒进 `into`（`into` 不在就先建一个空的）。被吸收方的绝对时刻按
    /// 两岛时钟差平移（计划 5.1），邮筒跟着并，Director 里改记归属。
    ///
    /// Pour island `from` into `into` (creating an empty `into` if missing),
    /// rebasing the absorbed clock (plan 5.1); the postbox and the Director's
    /// owners follow.
    fn apply_merge(&mut self, into: RegionId, from: RegionId) {
        if into == from {
            return;
        }
        assert_ne!(
            from, HOME_REGION,
            "家岛总是吸收方（分区器并进编号最小的岛）"
        );
        let Some(other) = self.islands.remove(from) else {
            self.master.host.merge_posts(into, from, &[]);
            return;
        };
        let moved = other.region.chunks.loaded_positions();
        let origin = self
            .master
            .host
            .island_origin(into, moved.first().copied().unwrap_or(ChunkPos::new(0, 0)));
        let target = self.islands.ensure(into, origin);
        let delta = target.region.tick.game_time() - other.region.tick.game_time();
        target.absorb_island_boxed(other, delta);
        self.master.host.merge_posts(into, from, &moved);
    }

    /// 把岛 `from` 里属于 `sections` 的区块（已加载的和信箱里的）和站在那里的玩家拆成
    /// 新岛 `to`（`to` 已经在、比如空着的家岛，就并进去）。两边的视野、实体配对还没分开
    /// （有人正在跨岛）时返回 `false`，下次再拆。
    ///
    /// Split the chunks of `sections` (loaded and in the mailbox) and the
    /// players standing there off island `from` into `to` (merged into `to`
    /// when it already exists, e.g. an empty home). Returns `false` while the
    /// two sides' views or entity pairings still overlap (someone crossing).
    fn apply_split(
        &mut self,
        from: RegionId,
        to: RegionId,
        sections: &BTreeSet<SectionPos>,
    ) -> bool {
        let inside = |pos: ChunkPos| sections.contains(&SectionPos::of(pos));
        let Some(island) = self.islands.get_mut(from) else {
            self.master.host.split_posts(from, to, sections, &[]);
            return true;
        };
        let mut chunks: BTreeSet<ChunkPos> = island
            .region
            .chunks
            .loaded_positions()
            .into_iter()
            .filter(|&pos| inside(pos))
            .collect();
        chunks.extend(
            island
                .mailbox
                .positions()
                .into_iter()
                .filter(|&pos| inside(pos)),
        );
        let players: BTreeSet<PlayerId> = island
            .region
            .players
            .values()
            .filter(|player| inside(player.chunk_pos()))
            .map(|player| player.id)
            .collect();
        if !split_ready(&island.region, &players, &inside) {
            return false;
        }
        let origin = *sections.first().expect("拆出来的部分不为空");
        let random = island
            .region
            .derive_island_random(island.region.global.seed, origin);
        #[cfg(test)]
        let skip = self.master.split_skip;
        #[cfg(not(test))]
        let skip = None;
        let Ok(child) = island.split_off_island_skipping(to, &chunks, &players, random, skip)
        else {
            return false;
        };
        let moved = child.region.chunks.loaded_positions();
        if let Some(target) = self.islands.get_mut(to) {
            let delta = target.region.tick.game_time() - child.region.tick.game_time();
            target.absorb_island_boxed(child, delta);
        } else {
            self.islands.insert(child);
        }
        self.master.host.split_posts(from, to, sections, &moved);
        true
    }

    /// 站到别的岛的区块上的玩家：原来的岛把他清干净了（或者等太久了）就搬过去。返回
    /// 搬了几个。
    ///
    /// Move players standing on another island's chunk once their old island
    /// has cleared them (or they waited too long). Returns how many moved.
    fn relocate_players(&mut self) -> usize {
        let mut crossing: Vec<(RegionId, PlayerId, ChunkPos)> = Vec::new();
        for island in self.islands.iter() {
            for player in island.region.players.values() {
                crossing.push((island.id, player.id, player.chunk_pos()));
            }
        }
        let mut moved = 0;
        for (from, id, chunk) in crossing {
            let Some(to) = self.master.host.island_of(chunk) else {
                continue;
            };
            if to == from {
                self.master.transit.remove(&id);
                continue;
            }
            let now = self.master.director_tick;
            let waited = now.saturating_sub(*self.master.transit.entry(id).or_insert(now));
            let region = &self.islands.get(from).expect("在表里").region;
            let clean = player_clear_of(region, id);
            if !clean {
                if waited <= TRANSIT_PATIENCE {
                    continue;
                }
                warn!(
                    player = id.0,
                    from, to, "a crossing player was not cleared in time; clearing forcibly"
                );
                let region = &mut self.islands.get_mut(from).expect("在表里").region;
                // 视野先跟到新位置（发中心和 forget），再把配对、菜单清掉。
                // Move the view first (centre and forgets), then drop pairings and menus.
                let view_distance = region.view_distance;
                if let Some(player) = region.players.get_mut(&id) {
                    crate::chunk_tracking::update_player_view(
                        player,
                        &mut region.links,
                        view_distance,
                    );
                }
                crate::entity_sync::on_leave(region, id);
                crate::entity_tracking::on_leave(region, id);
                crate::menu::on_leave(region, id);
                crate::lectern::on_leave(region, id);
            }
            self.move_player(from, to, id, chunk);
            self.master.transit.remove(&id);
            moved += 1;
        }
        moved
    }

    /// 主控在岛 `from` 上替所有玩家执行完一件事之后：落到别的岛已加载区块上的实体（`/give`
    /// 掉在远处玩家脚下的东西、被 `/tp` 送走的实体）整块搬到那个岛（计划 6.5「掉出范围的
    /// 实体」走同一条离境 / 到达）。哪个岛都没加载的位置不管，和以前一样留在原地。
    ///
    /// After the master ran something for every player on island `from`:
    /// entities that landed on another island's loaded chunk (a `/give` drop at
    /// a distant player's feet, an entity sent off by `/tp`) move to that
    /// island (plan 6.5's departure and arrival). Positions no island has
    /// loaded stay as before.
    pub(crate) fn move_stray_entities(&mut self, from: RegionId) {
        let Some(island) = self.islands.get(from) else {
            return;
        };
        let region = &island.region;
        let mut strays: std::collections::BTreeMap<RegionId, BTreeSet<ChunkPos>> =
            std::collections::BTreeMap::new();
        for public in region.entities.iter_public() {
            let chunk = public.chunk_position();
            if region.chunks.is_loaded(chunk) {
                continue;
            }
            if let Some(to) = self.islands.island_of_chunk(chunk) {
                strays.entry(to).or_default().insert(chunk);
            }
        }
        for (to, chunks) in strays {
            let source = self.islands.get_mut(from).expect("在表里");
            let origin = SectionPos::of(*chunks.first().expect("不为空"));
            let random = source.region.derive_island_random(0, origin);
            let Ok(carrier) = source.split_off_island_boxed(to, &chunks, &BTreeSet::new(), random)
            else {
                continue;
            };
            let target = self.islands.get_mut(to).expect("刚找到");
            let delta = target.region.tick.game_time() - carrier.region.tick.game_time();
            target.absorb_island_boxed(carrier, delta);
        }
    }

    /// 把一名玩家（和跟着他走的表）从岛 `from` 搬到岛 `to`（离境再到达）。
    ///
    /// Move one player (and the tables that follow players) from island
    /// `from` to island `to`: a departure and an arrival.
    fn move_player(&mut self, from: RegionId, to: RegionId, id: PlayerId, chunk: ChunkPos) {
        let origin = self.master.host.island_origin(to, chunk);
        let source = self.islands.get_mut(from).expect("在表里");
        // 只装这名玩家的小岛：随机源用不上（合进目标岛时丢掉）。
        // A one-player carrier; its random sources are dropped on the merge.
        let random = source.region.derive_island_random(0, SectionPos::of(chunk));
        let carrier = source
            .split_off_island_boxed(to, &BTreeSet::new(), &BTreeSet::from([id]), random)
            .unwrap_or_else(|_| panic!("tick 边界上搬不走玩家 {id:?}"));
        let target = self.islands.ensure(to, origin);
        let delta = target.region.tick.game_time() - carrier.region.tick.game_time();
        target.absorb_island_boxed(carrier, delta);
    }

    /// 计划 6.2 第 3 条的不变量（调试构建，每次照做完拆合之后）：不同岛的已交付区块不相邻；
    /// 每个已交付区块恰好在一个岛、Director 记的归属就是那个岛；每名玩家在一个岛上，不在跨岛
    /// 途中时就在他所在区块的岛里；玩家跟踪的区块都在他的岛里；实体网络 ID 全进程唯一；
    /// 停放着的岛没有被派出去。
    ///
    /// The invariants of plan 6.2 item 3 (debug builds, after each settle).
    #[cfg(debug_assertions)]
    pub(crate) fn assert_island_invariants(&mut self) {
        let mut owner: std::collections::HashMap<ChunkPos, RegionId> =
            std::collections::HashMap::new();
        let mut players: std::collections::HashMap<PlayerId, RegionId> =
            std::collections::HashMap::new();
        let mut entity_ids = std::collections::HashSet::new();
        for island in self.islands.iter() {
            assert_eq!(
                island.state,
                crate::island::IslandState::Parked,
                "停放着的岛 {} 的状态不对",
                island.id
            );
            for pos in island.region.chunks.loaded_positions() {
                if let Some(other) = owner.insert(pos, island.id) {
                    panic!("区块 {pos:?} 同时在岛 {other} 和 {} 上", island.id);
                }
            }
            for id in island.region.players.keys() {
                if let Some(other) = players.insert(*id, island.id) {
                    panic!("玩家 {id:?} 同时在岛 {other} 和 {} 上", island.id);
                }
            }
            for entity in island.region.entities.ids() {
                assert!(entity_ids.insert(entity), "实体网络 ID {entity} 重复");
            }
        }
        for (&pos, &id) in &owner {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let near = ChunkPos::new(pos.x + dx, pos.z + dz);
                    if let Some(&other) = owner.get(&near) {
                        assert_eq!(
                            other, id,
                            "岛 {id} 的 {pos:?} 和岛 {other} 的 {near:?} 相邻"
                        );
                    }
                }
            }
        }
        // Director 记成「在岛 x 上」的区块真在岛 x 上；还在等回执的交付 / 收回不算。
        // Chunks the Director records on island x really are there; deliveries
        // and reclaims still waiting for their ack do not count.
        for record in self.master.host.director().owner_records() {
            if let mc_bamboo_world::director::Owner::Region(id) = record.owner {
                assert_eq!(
                    owner.get(&record.pos),
                    Some(&id),
                    "Director 记的 {:?} 在岛 {id} 上，实际不在",
                    record.pos
                );
            }
        }
        for island in self.islands.iter() {
            for player in island.region.players.values() {
                if self.master.transit.contains_key(&player.id) {
                    continue;
                }
                let chunk = player.chunk_pos();
                if let Some(expected) = self
                    .master
                    .host
                    .islands()
                    .and_then(|partition| partition.island_of_chunk(chunk))
                    && !self.master.host.has_topology()
                {
                    assert_eq!(
                        expected, island.id,
                        "玩家 {:?} 站在岛 {expected} 的 {chunk:?} 上，却在岛 {}",
                        player.id, island.id
                    );
                }
                for (&pos, &id) in &owner {
                    if player.chunks.is_chunk_tracked(pos) {
                        assert_eq!(
                            id, island.id,
                            "玩家 {:?} 跟踪着别的岛 {id} 的区块 {pos:?}",
                            player.id
                        );
                    }
                }
            }
        }
    }
}

/// 拆分前两边分得开：每名玩家的视野中心、看得见他的人、他看得见的人都在他自己那一边，
/// 没有实体同时被两边的人看着。
///
/// Before a split the two sides are apart: every player's view centre, the
/// players who see them and the players they see are on their own side, and
/// no entity is watched from both sides.
fn split_ready(
    region: &crate::region::Region,
    players: &BTreeSet<PlayerId>,
    inside: &impl Fn(ChunkPos) -> bool,
) -> bool {
    for player in region.players.values() {
        let side = players.contains(&player.id);
        if player
            .chunks
            .view_center()
            .is_some_and(|center| inside(center) != side)
        {
            return false;
        }
        if player
            .sync
            .seen_by()
            .iter()
            .any(|viewer| players.contains(viewer) != side)
        {
            return false;
        }
    }
    !region.entity_tracking.straddles(players)
}

/// 跨岛的玩家在原来的岛上已经清干净：视野跟到了新位置（原来的岛没有那里的区块，什么都
/// 没跟踪），谁也看不见他、他也看不见谁，不看着任何实体，没开着菜单和讲台。
///
/// A crossing player is clear of the old island: the view followed the new
/// position (the old island has no chunks there, so nothing is tracked),
/// nobody sees them and they see nobody, they watch no entity, and no menu
/// or lectern is open.
fn player_clear_of(region: &crate::region::Region, id: PlayerId) -> bool {
    let Some(player) = region.players.get(&id) else {
        return true;
    };
    player.chunks.view_center() == Some(player.chunk_pos())
        && player.sync.seen_by().is_empty()
        && region
            .players
            .values()
            .all(|other| other.id == id || !other.sync.seen_by().contains(&id))
        && !region.entity_tracking.watched_by(id)
        && !region.menus.contains_key(&id)
        && !region.lecterns.has_view(id)
}
