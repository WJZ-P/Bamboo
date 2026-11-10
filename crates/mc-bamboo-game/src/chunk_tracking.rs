//! 区块跟踪：玩家应该有哪些区块，以及按什么速度发给它。
//!
//! 跟踪范围不是方形，也不是视距圈：沿坐标轴的半径是 `vd + 1`，四角是圆的。
//! 这样客户端渲染范围里的每个区块，8 个邻居都能到齐，否则它不编译这个区块。

use std::{collections::BTreeMap, ops::RangeInclusive};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_protocol::packet::play::{
    ChunkBatchFinished, ChunkBatchStart, ForgetLevelChunk, SetChunkCacheCenter,
};

use crate::{
    chunk_encoding::chunk_frame,
    inbound::PlayerId,
    links::Links,
    player::Player,
    profiler::Profiler,
    region::{Region, RegionTick},
};

/// 视距的下限。客户端请求的值和服务端的配置都不能比它小。
pub(crate) const MIN_VIEW_DISTANCE: i32 = 2;
/// 服务端视距的上限。
pub(crate) const MAX_VIEW_DISTANCE: i32 = 32;

const MIN_CHUNKS_PER_TICK: f32 = 0.01;
const MAX_CHUNKS_PER_TICK: f32 = 64.0;
const START_CHUNKS_PER_TICK: f32 = 9.0;
/// 收到第一次确认之后，最多有多少批在路上。在那之前只有一批。
const MAX_UNACKNOWLEDGED_BATCHES: i32 = 10;

/// 一名玩家的跟踪范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkView {
    pub(crate) center: ChunkPos,
    pub(crate) view_distance: i32,
}

impl ChunkView {
    fn is_within(self, pos: ChunkPos, buffer: i64) -> bool {
        let axis = |center: i32, value: i32| {
            ((i64::from(value) - i64::from(center)).abs() - buffer).max(0)
        };
        let dx = axis(self.center.x, pos.x);
        let dz = axis(self.center.z, pos.z);
        let radius = i64::from(self.view_distance);
        dx * dx + dz * dz < radius * radius
    }

    /// 集合 `T(vd)`：服务端跟踪的范围，也就是要发给客户端的区块。
    pub(crate) fn contains(self, pos: ChunkPos) -> bool {
        self.is_within(pos, 2)
    }

    /// 集合 `R(vd)`：客户端的渲染范围。`T` 正好是 `R` 的每个区块加上它的 8 个邻居。
    #[cfg(test)]
    pub(crate) fn is_in_view_distance(self, pos: ChunkPos) -> bool {
        self.is_within(pos, 1)
    }

    fn x_range(self) -> RangeInclusive<i32> {
        let radius = self.view_distance + 1;
        self.center.x - radius..=self.center.x + radius
    }

    fn z_range(self) -> RangeInclusive<i32> {
        let radius = self.view_distance + 1;
        self.center.z - radius..=self.center.z + radius
    }

    fn square_intersects(self, other: Self) -> bool {
        let overlaps = |a: RangeInclusive<i32>, b: RangeInclusive<i32>| {
            a.start() <= b.end() && a.end() >= b.start()
        };
        overlaps(self.x_range(), other.x_range()) && overlaps(self.z_range(), other.z_range())
    }

    /// `T(vd)` 里的全部区块，x 在外层，都是升序。
    fn chunks(self) -> impl Iterator<Item = ChunkPos> {
        self.x_range()
            .flat_map(move |x| self.z_range().map(move |z| ChunkPos::new(x, z)))
            .filter(move |&pos| self.contains(pos))
    }
}

/// 视图变化时进入和离开的区块，顺序和官方一样是 x 在外层、都是升序。
fn view_difference(previous: Option<ChunkView>, next: ChunkView) -> (Vec<ChunkPos>, Vec<ChunkPos>) {
    let Some(previous) = previous else {
        return (next.chunks().collect(), Vec::new());
    };
    if previous == next {
        return (Vec::new(), Vec::new());
    }
    // 跳得很远时包围盒的并集可以大到几千万格，所以不能遍历它。
    if !previous.square_intersects(next) {
        return (next.chunks().collect(), previous.chunks().collect());
    }

    let x_range = (*previous.x_range().start()).min(*next.x_range().start())
        ..=(*previous.x_range().end()).max(*next.x_range().end());
    let z_range = (*previous.z_range().start()).min(*next.z_range().start())
        ..=(*previous.z_range().end()).max(*next.z_range().end());
    let mut entered = Vec::new();
    let mut left = Vec::new();
    for x in x_range {
        for z in z_range.clone() {
            let pos = ChunkPos::new(x, z);
            match (previous.contains(pos), next.contains(pos)) {
                (false, true) => entered.push(pos),
                (true, false) => left.push(pos),
                _ => {}
            }
        }
    }
    (entered, left)
}

/// 按客户端的接收速度发区块，和官方的 `PlayerChunkSender` 一致。
///
/// 这是背压：接收方反过来限制发送方。收不到确认时只是停发，区块留在 `pending` 里。
#[derive(Debug)]
pub(crate) struct PlayerChunkSender {
    /// 值为进入待发送集合的那一刻，用来算区块延迟。
    pending: BTreeMap<ChunkPos, RegionTick>,
    tick: RegionTick,
    desired_chunks_per_tick: f32,
    batch_quota: f32,
    unacknowledged_batches: i32,
    max_unacknowledged_batches: i32,
}

impl Default for PlayerChunkSender {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            tick: RegionTick::default(),
            desired_chunks_per_tick: START_CHUNKS_PER_TICK,
            batch_quota: 0.0,
            unacknowledged_batches: 0,
            max_unacknowledged_batches: 1,
        }
    }
}

impl PlayerChunkSender {
    /// 世界线程在视图变化之前把当前刻写进来。
    pub(crate) fn set_tick(&mut self, tick: RegionTick) {
        self.tick = tick;
    }

    fn mark_pending(&mut self, pos: ChunkPos) {
        self.pending.entry(pos).or_insert(self.tick);
    }

    /// 这个区块离开了视图，返回客户端是否已经有它，有的话要发 `forget_level_chunk`。
    fn drop_chunk(&mut self, pos: ChunkPos) -> bool {
        self.pending.remove(&pos).is_none()
    }

    fn is_pending(&self, pos: ChunkPos) -> bool {
        self.pending.contains_key(&pos)
    }

    /// 在区块离开 `pending` 之前，把这一刻写进出站缓冲的区块延迟记进剖析器。
    fn record_batch(&self, batch: &[ChunkPos], profiler: &mut Profiler) {
        for pos in batch {
            if let Some(&entered) = self.pending.get(pos) {
                profiler.record_chunk_latency(self.tick.since(entered));
            }
        }
    }

    /// 处理客户端的 `chunk_batch_received`。
    fn on_batch_received(&mut self, desired_chunks_per_tick: f32) {
        self.unacknowledged_batches = (self.unacknowledged_batches - 1).max(0);
        self.desired_chunks_per_tick = if desired_chunks_per_tick.is_nan() {
            MIN_CHUNKS_PER_TICK
        } else {
            desired_chunks_per_tick.clamp(MIN_CHUNKS_PER_TICK, MAX_CHUNKS_PER_TICK)
        };
        if self.unacknowledged_batches == 0 {
            self.batch_quota = 1.0;
        }
        self.max_unacknowledged_batches = MAX_UNACKNOWLEDGED_BATCHES;
    }

    /// 取出这一 tick 要发的区块，近的在前。`center` 是玩家此刻所在的区块。
    fn next_batch(&mut self, center: ChunkPos, profiler: &mut Profiler) -> Vec<ChunkPos> {
        if self.unacknowledged_batches >= self.max_unacknowledged_batches {
            return Vec::new();
        }
        let max_batch_size = self.desired_chunks_per_tick.max(1.0);
        self.batch_quota = (self.batch_quota + self.desired_chunks_per_tick).min(max_batch_size);
        if self.batch_quota < 1.0 || self.pending.is_empty() {
            return Vec::new();
        }

        let chunks = self.take_nearest(center, self.batch_quota.floor() as usize, profiler);
        self.unacknowledged_batches += 1;
        self.batch_quota -= chunks.len() as f32;
        chunks
    }

    fn take_nearest(
        &mut self,
        center: ChunkPos,
        count: usize,
        profiler: &mut Profiler,
    ) -> Vec<ChunkPos> {
        let mut nearest: Vec<ChunkPos> = self.pending.keys().copied().collect();
        // 排序是稳定的，距离相同的区块保持 `pending` 里的顺序，结果是确定的。
        nearest.sort_by_key(|&pos| distance_squared(center, pos));
        nearest.truncate(count);
        self.record_batch(&nearest, profiler);
        for pos in &nearest {
            self.pending.remove(pos);
        }
        nearest
    }
}

fn distance_squared(a: ChunkPos, b: ChunkPos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dz = i64::from(a.z) - i64::from(b.z);
    dx * dx + dz * dz
}

/// 一名玩家的区块跟踪状态。
#[derive(Debug, Default)]
pub(crate) struct ChunkTracker {
    view: Option<ChunkView>,
    pub(crate) sender: PlayerChunkSender,
}

impl ChunkTracker {
    /// 换到新的视图：先发中心，再发 forget，最后把新区块放进 `pending`。
    ///
    /// 中心必须最先到：超出客户端范围的区块会被丢弃，也不会被重新请求。
    fn apply_view(&mut self, id: PlayerId, next: ChunkView, links: &mut Links) {
        if self.view == Some(next) {
            return;
        }
        if self.view.map(|view| view.center) != Some(next.center) {
            let ChunkPos { x, z } = next.center;
            links.send(id, &SetChunkCacheCenter { x, z });
        }

        let (entered, left) = view_difference(self.view, next);
        for pos in left {
            if self.sender.drop_chunk(pos) {
                let ChunkPos { x, z } = pos;
                links.send(id, &ForgetLevelChunk { x, z });
            }
        }
        for pos in entered {
            self.sender.mark_pending(pos);
        }
        self.view = Some(next);
    }

    /// 这个区块在跟踪范围里，而且已经发给客户端了。
    ///
    /// 方块更新和实体都只发给满足这个条件的玩家。还在 `pending` 的区块发送时就是最新内容。
    pub(crate) fn is_chunk_tracked(&self, pos: ChunkPos) -> bool {
        self.view.is_some_and(|view| view.contains(pos)) && !self.sender.is_pending(pos)
    }

    /// `ChunkMap.isChunkOnTrackedBorder`：这个区块已经发给客户端了，8 个邻居里却有没发的。
    ///
    /// `light_update` 只发给满足这个条件的玩家，客户端缺邻居时自己算不准这个区块的光照。
    pub(crate) fn is_chunk_on_tracked_border(&self, pos: ChunkPos) -> bool {
        self.is_chunk_tracked(pos)
            && (-1..=1).any(|dx| {
                (-1..=1).any(|dz| {
                    (dx, dz) != (0, 0)
                        && !self.is_chunk_tracked(ChunkPos::new(pos.x + dx, pos.z + dz))
                })
            })
    }
}

/// 让这名玩家的视图跟上它现在的位置和视距。位置和视距都没变时什么也不做。
pub(crate) fn update_player_view(
    player: &mut Player,
    links: &mut Links,
    server_view_distance: i32,
) {
    let next = ChunkView {
        center: player.chunk_pos(),
        view_distance: player.view_distance(server_view_distance),
    };
    player.chunks.apply_view(player.id, next, links);
}

/// tick 第 4 阶段：每名玩家比较一次 `(center, vd)`。
pub(crate) fn update_views(region: &mut Region) {
    let tick = region.tick;
    for player in region.players.values_mut() {
        player.chunks.sender.set_tick(tick);
        update_player_view(player, &mut region.links, region.view_distance);
    }
}

/// tick 第 10 阶段：每名玩家最多发一批，发完把没人跟踪的区块从编码缓存里丢掉。
pub(crate) fn send_chunks(region: &mut Region) {
    let tick = region.tick;
    for (&id, player) in &mut region.players {
        if !region.links.is_open(id) {
            continue;
        }
        player.chunks.sender.set_tick(tick);
        let batch = player
            .chunks
            .sender
            .next_batch(player.chunk_pos(), &mut region.profiler);
        if batch.is_empty() {
            continue;
        }

        region.links.send(id, &ChunkBatchStart);
        for &pos in &batch {
            let frames = chunk_frame(
                &mut region.chunk_cache,
                &region.chunks,
                &region.template_body,
                pos,
            );
            region.links.send_shared(id, frames);
        }
        let batch_size = batch.len() as i32;
        region.links.send(id, &ChunkBatchFinished { batch_size });
    }
    region.profiler.finish_chunk_sending();
    drop_untracked_bodies(region);
}

/// 缓存只留下还在某名玩家跟踪范围里的区块，包括还在 `pending` 里等着发的。
fn drop_untracked_bodies(region: &mut Region) {
    let views: Vec<ChunkView> = region
        .players
        .values()
        .filter_map(|player| player.chunks.view)
        .collect();
    region
        .chunk_cache
        .retain(|pos| views.iter().any(|view| view.contains(pos)));
}

/// 处理客户端的 `chunk_batch_received`。
pub(crate) fn on_chunk_batch_received(region: &mut Region, id: PlayerId, desired: f32) {
    if let Some(player) = region.players.get_mut(&id) {
        player.chunks.sender.on_batch_received(desired);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mc_bamboo_data::packets::play::clientbound;
    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::ClientboundPacket,
    };

    use super::*;
    use crate::{
        chunk_encoding::{ChunkCache, encode_template},
        inbound::PlayerSink,
        outgoing::Outgoing,
        profiler::{Metric, Series},
        world::tests::{Harness, SPAWN_X, SPAWN_Z},
    };
    use std::sync::Arc;

    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::{frame::append_frame, packet::play::LevelChunkWithLight};
    use mc_bamboo_world::store::ChunkStore;

    const ORIGIN: ChunkPos = ChunkPos::new(0, 0);
    const PLAYER: PlayerId = PlayerId(1);

    fn view(x: i32, z: i32, view_distance: i32) -> ChunkView {
        ChunkView {
            center: ChunkPos::new(x, z),
            view_distance,
        }
    }

    fn chunk(x: i32, z: i32) -> ChunkPos {
        ChunkPos::new(x, z)
    }

    /// 包围盒之外再多看两圈，确认集合真的没有漏到外面。
    fn all_near(view_distance: i32) -> impl Iterator<Item = ChunkPos> {
        let radius = view_distance + 3;
        (-radius..=radius).flat_map(move |x| (-radius..=radius).map(move |z| chunk(x, z)))
    }

    fn sorted(mut positions: Vec<ChunkPos>) -> Vec<ChunkPos> {
        positions.sort_unstable();
        positions
    }

    #[test]
    fn tracking_set_boundary_vectors() {
        let tracked = view(0, 0, 8);
        for (x, z) in [(9, 0), (9, 2), (9, 3), (9, 5), (8, 7), (7, 7)] {
            assert!(tracked.contains(chunk(x, z)), "({x}, {z})");
        }
        for (x, z) in [(9, 6), (8, 8), (10, 0), (9, 9)] {
            assert!(!tracked.contains(chunk(x, z)), "({x}, {z})");
        }
    }

    #[test]
    fn render_set_boundary_vectors() {
        let rendered = view(0, 0, 8);
        for (x, z) in [(8, 0), (8, 1), (8, 4), (7, 6)] {
            assert!(rendered.is_in_view_distance(chunk(x, z)), "({x}, {z})");
        }
        for (x, z) in [(8, 5), (7, 7), (9, 0)] {
            assert!(!rendered.is_in_view_distance(chunk(x, z)), "({x}, {z})");
        }
    }

    #[test]
    fn set_sizes_match_the_table() {
        let sizes = [
            (2, 49, 25),
            (3, 81, 49),
            (4, 117, 77),
            (6, 213, 157),
            (8, 329, 257),
            (10, 473, 385),
            (12, 637, 533),
            (16, 1057, 921),
            (32, 3725, 3461),
        ];
        for (view_distance, tracked, rendered) in sizes {
            let current = view(0, 0, view_distance);
            let t = all_near(view_distance)
                .filter(|&pos| current.contains(pos))
                .count();
            let r = all_near(view_distance)
                .filter(|&pos| current.is_in_view_distance(pos))
                .count();
            assert_eq!((t, r), (tracked, rendered), "vd {view_distance}");
            assert_eq!(current.chunks().count(), tracked, "vd {view_distance}");
        }
    }

    #[test]
    fn stepping_changes_as_many_chunks_as_the_table_says() {
        let steps = [
            (2, 7, 13),
            (3, 9, 17),
            (4, 11, 19),
            (6, 15, 25),
            (8, 19, 31),
            (10, 23, 37),
            (12, 27, 41),
            (16, 35, 53),
            (32, 67, 99),
        ];
        for (view_distance, along_axis, diagonal) in steps {
            let start = view(0, 0, view_distance);
            for (target, expected) in [
                (view(1, 0, view_distance), along_axis),
                (view(1, 1, view_distance), diagonal),
            ] {
                let (entered, left) = view_difference(Some(start), target);
                assert_eq!(entered.len(), expected, "vd {view_distance}");
                assert_eq!(left.len(), expected, "vd {view_distance}");
            }
        }
    }

    #[test]
    fn tracking_set_is_the_render_set_plus_its_neighbours() {
        for view_distance in 2..=32 {
            let current = view(0, 0, view_distance);
            let mut expected = BTreeSet::new();
            for pos in all_near(view_distance).filter(|&pos| current.is_in_view_distance(pos)) {
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        expected.insert(chunk(pos.x + dx, pos.z + dz));
                    }
                }
            }
            let tracked: BTreeSet<ChunkPos> = current.chunks().collect();
            assert_eq!(tracked, expected, "vd {view_distance}");
        }
    }

    #[test]
    fn square_view_of_m1_misses_chunks_of_the_tracking_set() {
        let current = view(0, 0, 8);
        let square: BTreeSet<ChunkPos> = (-8..=8)
            .flat_map(|x| (-8..=8).map(move |z| chunk(x, z)))
            .collect();
        let tracked: BTreeSet<ChunkPos> = current.chunks().collect();

        assert_eq!(square.len(), 289);
        assert_eq!(tracked.difference(&square).count(), 44);
        assert_eq!(square.difference(&tracked).count(), 4);

        let missing_neighbours = all_near(8)
            .filter(|&pos| current.is_in_view_distance(pos))
            .filter(|pos| {
                (-1..=1)
                    .any(|dx| (-1..=1).any(|dz| !square.contains(&chunk(pos.x + dx, pos.z + dz))))
            })
            .count();
        assert_eq!(missing_neighbours, 36);
    }

    #[test]
    fn walking_one_chunk_at_view_distance_2() {
        let (entered, left) = view_difference(Some(view(0, 0, 2)), view(1, 0, 2));

        let column = |x| (-3..=3).map(move |z| chunk(x, z)).collect::<Vec<_>>();
        assert_eq!(entered, column(4));
        assert_eq!(left, column(-3));
    }

    #[test]
    fn walking_one_chunk_at_view_distance_8_is_rounded() {
        let (entered, left) = view_difference(Some(view(0, 0, 8)), view(1, 0, 8));

        let mut expected = vec![
            chunk(6, -9),
            chunk(6, 9),
            chunk(8, -8),
            chunk(8, 8),
            chunk(9, -7),
            chunk(9, 7),
            chunk(9, -6),
            chunk(9, 6),
        ];
        expected.extend((-5..=5).map(|z| chunk(10, z)));
        assert_eq!(sorted(entered), sorted(expected));
        assert_eq!(left.len(), 19);
    }

    #[test]
    fn first_view_enters_everything_and_leaves_nothing() {
        let (entered, left) = view_difference(None, view(0, 0, 2));
        assert_eq!(entered.len(), 49);
        assert!(left.is_empty());
    }

    #[test]
    fn same_view_changes_nothing() {
        let (entered, left) = view_difference(Some(view(5, 5, 4)), view(5, 5, 4));
        assert!(entered.is_empty() && left.is_empty());
    }

    #[test]
    fn view_distance_change_keeps_the_center_and_grows_outward() {
        let (entered, left) = view_difference(Some(view(0, 0, 2)), view(0, 0, 3));
        assert_eq!(entered.len(), 81 - 49);
        assert!(left.is_empty());
    }

    #[test]
    fn disjoint_views_swap_every_chunk_without_scanning_the_gap() {
        let far = 1_900_000;
        let (entered, left) = view_difference(Some(view(-far, -far, 2)), view(far, far, 2));

        assert_eq!(entered.len(), 49);
        assert_eq!(left.len(), 49);
        assert!(entered.iter().all(|pos| pos.x > 0));
        assert!(left.iter().all(|pos| pos.x < 0));
    }

    /// 让 `pending` 里有足够多的区块，都离原点很远不会被取完。
    fn sender_with_many_chunks() -> PlayerChunkSender {
        let mut sender = PlayerChunkSender::default();
        for x in -40..=40 {
            for z in -40..=40 {
                sender.mark_pending(chunk(x, z));
            }
        }
        sender
    }

    /// `chunks-movement.md` 7.4 的推演表。
    #[test]
    fn sender_follows_the_worked_example() {
        let mut sender = sender_with_many_chunks();
        let state =
            |sender: &PlayerChunkSender| (sender.batch_quota, sender.unacknowledged_batches);

        assert_eq!(sender.next_batch(ORIGIN, &mut Profiler::default()).len(), 9);
        assert_eq!(state(&sender), (0.0, 1));

        assert!(
            sender
                .next_batch(ORIGIN, &mut Profiler::default())
                .is_empty()
        );
        assert_eq!(state(&sender), (0.0, 1));

        sender.on_batch_received(3.5);
        assert_eq!(state(&sender), (1.0, 0));
        assert_eq!(sender.next_batch(ORIGIN, &mut Profiler::default()).len(), 3);
        assert_eq!(state(&sender), (0.5, 1));

        for unacknowledged in 2..=10 {
            assert_eq!(sender.next_batch(ORIGIN, &mut Profiler::default()).len(), 3);
            assert_eq!(state(&sender), (0.5, unacknowledged));
        }

        assert!(
            sender
                .next_batch(ORIGIN, &mut Profiler::default())
                .is_empty()
        );
        assert_eq!(state(&sender), (0.5, 10));
    }

    #[test]
    fn first_batch_is_the_three_by_three_nearest_first() {
        let mut sender = sender_with_many_chunks();

        let batch = sender.next_batch(ORIGIN, &mut Profiler::default());

        let distances: Vec<i64> = batch
            .iter()
            .map(|&pos| distance_squared(ORIGIN, pos))
            .collect();
        assert_eq!(distances, [0, 1, 1, 1, 1, 2, 2, 2, 2]);
    }

    #[test]
    fn distance_uses_the_position_at_send_time() {
        let mut sender = sender_with_many_chunks();

        let batch = sender.next_batch(chunk(30, 30), &mut Profiler::default());

        assert_eq!(batch[0], chunk(30, 30));
    }

    #[test]
    fn slow_clients_get_a_chunk_every_few_ticks() {
        let mut sender = sender_with_many_chunks();
        sender.next_batch(ORIGIN, &mut Profiler::default());
        sender.on_batch_received(0.5);
        sender.batch_quota = 0.0;

        assert!(
            sender
                .next_batch(ORIGIN, &mut Profiler::default())
                .is_empty()
        );
        assert_eq!(sender.batch_quota, 0.5);
        assert_eq!(sender.next_batch(ORIGIN, &mut Profiler::default()).len(), 1);
        assert_eq!(sender.batch_quota, 0.0);
    }

    #[test]
    fn fractions_of_the_desired_rate_do_not_accumulate() {
        let mut sender = sender_with_many_chunks();
        sender.next_batch(ORIGIN, &mut Profiler::default());
        sender.on_batch_received(3.5);

        for _ in 0..5 {
            assert_eq!(sender.next_batch(ORIGIN, &mut Profiler::default()).len(), 3);
        }
    }

    #[test]
    fn acknowledgement_values_are_clamped() {
        let desired_after = |value: f32| {
            let mut sender = PlayerChunkSender::default();
            sender.on_batch_received(value);
            sender.desired_chunks_per_tick
        };

        assert_eq!(desired_after(f32::NAN), 0.01);
        assert_eq!(desired_after(1000.0), 64.0);
        assert_eq!(desired_after(f32::INFINITY), 64.0);
        assert_eq!(desired_after(0.001), 0.01);
        assert_eq!(desired_after(f32::NEG_INFINITY), 0.01);
        assert_eq!(desired_after(5.0), 5.0);
    }

    #[test]
    fn acknowledgement_without_a_batch_keeps_the_count_at_zero() {
        let mut sender = PlayerChunkSender::default();
        sender.on_batch_received(1.0);
        assert_eq!(sender.unacknowledged_batches, 0);
    }

    #[test]
    fn quota_grows_even_when_nothing_is_pending() {
        let mut sender = PlayerChunkSender::default();

        assert!(
            sender
                .next_batch(ORIGIN, &mut Profiler::default())
                .is_empty()
        );

        assert_eq!(sender.batch_quota, 9.0);
        assert_eq!(sender.unacknowledged_batches, 0);
    }

    #[derive(Clone, Default)]
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl PlayerSink for Recorder {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0.lock().unwrap().extend(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {}
    }

    impl Recorder {
        fn take_ids(&self) -> Vec<i32> {
            let bytes = std::mem::take(&mut *self.0.lock().unwrap());
            let mut rest = bytes.as_slice();
            let mut ids = Vec::new();
            while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
                let end = bounds.header_len + bounds.payload_len;
                ids.push(Reader::new(&rest[bounds.header_len..end]).varint().unwrap());
                rest = &rest[end..];
            }
            ids
        }
    }

    fn tracker_with_links() -> (ChunkTracker, Links, Recorder) {
        let recorder = Recorder::default();
        let mut links = Links::default();
        links.add(PLAYER, Box::new(recorder.clone()));
        (ChunkTracker::default(), links, recorder)
    }

    #[test]
    fn pending_chunks_leave_the_view_silently() {
        let (mut tracker, mut links, recorder) = tracker_with_links();

        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        tracker.apply_view(PLAYER, view(1, 0, 2), &mut links);
        links.flush();

        // 什么都还没发，所以没有 forget。
        assert_eq!(
            recorder.take_ids(),
            [SetChunkCacheCenter::ID, SetChunkCacheCenter::ID]
        );
        assert_eq!(tracker.sender.pending.len(), 49);
        assert!(!tracker.sender.is_pending(chunk(-3, 0)));
        assert!(tracker.sender.is_pending(chunk(4, 0)));
    }

    #[test]
    fn sent_chunks_are_forgotten_after_the_center_moved() {
        let (mut tracker, mut links, recorder) = tracker_with_links();
        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        tracker.sender.pending.clear();
        links.flush();
        recorder.take_ids();

        tracker.apply_view(PLAYER, view(1, 0, 2), &mut links);
        links.flush();

        let mut expected = vec![clientbound::SET_CHUNK_CACHE_CENTER];
        expected.extend([clientbound::FORGET_LEVEL_CHUNK; 7]);
        assert_eq!(recorder.take_ids(), expected);
        assert_eq!(tracker.sender.pending.len(), 7);
    }

    #[test]
    fn unchanged_view_sends_nothing() {
        let (mut tracker, mut links, recorder) = tracker_with_links();
        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        links.flush();
        recorder.take_ids();

        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        links.flush();

        assert!(recorder.take_ids().is_empty());
    }

    #[test]
    fn view_distance_change_does_not_resend_the_center() {
        let (mut tracker, mut links, recorder) = tracker_with_links();
        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        links.flush();
        recorder.take_ids();

        tracker.apply_view(PLAYER, view(0, 0, 3), &mut links);
        links.flush();

        assert!(recorder.take_ids().is_empty());
        assert_eq!(tracker.sender.pending.len(), 81);
    }

    #[test]
    fn chunk_is_tracked_once_it_left_pending_and_stays_in_view() {
        let (mut tracker, mut links, _recorder) = tracker_with_links();

        assert!(!tracker.is_chunk_tracked(ORIGIN));
        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        assert!(!tracker.is_chunk_tracked(ORIGIN));

        tracker.sender.next_batch(ORIGIN, &mut Profiler::default());
        assert!(tracker.is_chunk_tracked(ORIGIN));
        assert!(!tracker.is_chunk_tracked(chunk(3, 0)));
        assert!(!tracker.is_chunk_tracked(chunk(4, 0)));
    }

    #[test]
    fn two_players_watching_one_chunk_get_the_same_shared_frame() {
        let mut server = Harness::new(2);
        let edited = ChunkPos::new(0, 0);
        let pos = BlockPos::new(1, -60, 1);
        server.region().chunks.set_block(pos, blocks::CHEST);
        let alice = server.join("Kitten");
        let bob = server.join("Mochi");
        server.tick();

        let first = server.shared_chunk_frame(edited).expect("区块帧已经做好");
        let other = server
            .shared_chunk_frame(ChunkPos::new(1, 0))
            .expect("邻居的帧也做好了");
        assert!(!Arc::ptr_eq(&first, &other));
        assert!(server.cached_frame_count() > 1, "模板区块按坐标各自缓存");

        // 两名玩家收到的线上字节包含同一个区块的同一份帧。
        let a = alice.take_frames();
        let b = bob.take_frames();
        let body_of = |frames: &[crate::world::tests::Frame]| {
            frames
                .iter()
                .find(|frame| frame.id == clientbound::LEVEL_CHUNK_WITH_LIGHT)
                .map(|frame| frame.body.clone())
        };
        assert!(body_of(&a).is_some());
        assert_eq!(body_of(&a), body_of(&b));

        server.move_to(&alice, SPAWN_X, SPAWN_Z);
        server.move_to(&bob, SPAWN_X, SPAWN_Z);
        server.acknowledge_batch(&alice, 64.0);
        server.acknowledge_batch(&bob, 64.0);
        server.tick();
        let same = server
            .shared_chunk_frame(edited)
            .expect("版本号没变，帧还在");
        assert!(Arc::ptr_eq(&first, &same), "两名玩家共用同一份 Arc");

        server.region().chunks.set_block(pos, blocks::STONE);
        // 区块改了，但玩家没有重新请求它，缓存里还是旧帧；把坐标重新排进 pending 再发一批。
        server
            .region()
            .players
            .get_mut(&alice.id)
            .expect("玩家在")
            .chunks
            .sender
            .mark_pending(edited);
        server.acknowledge_batch(&alice, 64.0);
        server.tick();
        let changed = server
            .shared_chunk_frame(edited)
            .expect("区块改过以后重做了帧");
        assert!(!Arc::ptr_eq(&first, &changed), "内容变了就换新的帧");
    }

    #[test]
    fn chunk_frames_keep_the_official_packet_layout() {
        let store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();
        let pos = ChunkPos::new(5, -3);
        let frames = chunk_frame(&mut cache, &store, &template_body, pos);

        let mut expected = Vec::new();
        append_frame(
            &mut expected,
            &LevelChunkWithLight {
                x: pos.x,
                z: pos.z,
                body: &template_body,
            },
        )
        .unwrap();
        assert_eq!(frames.raw(), expected);

        // 没改过的区块在不同坐标各有一份帧，包头里的坐标跟着变。
        let neighbour = chunk_frame(&mut cache, &store, &template_body, ChunkPos::new(6, -3));
        assert_ne!(frames.raw(), neighbour.raw());
        assert_eq!(cache.frame_count(), 2);
        cache.retain(|p| p == pos);
        assert_eq!(cache.frame_count(), 1);
        assert!(Arc::ptr_eq(
            &frames,
            &cache.frame(&store, pos).expect("保留下来的帧")
        ));
    }

    #[test]
    fn pending_chunks_record_how_many_ticks_they_waited() {
        let mut sender = PlayerChunkSender::default();
        let mut profiler = Profiler::default();
        sender.set_tick(RegionTick::at(10));
        sender.mark_pending(chunk(0, 0));
        sender.set_tick(RegionTick::at(13));
        let batch = sender.next_batch(ORIGIN, &mut profiler);
        assert_eq!(batch, [chunk(0, 0)]);

        let latencies = profiler.histogram(Series::Metric(Metric::ChunkLatencyTicks));
        assert_eq!(latencies.count(), 1);
        assert_eq!(latencies.sum(), 3);
        profiler.finish_chunk_sending();
        assert_eq!(
            profiler.histogram(Series::Metric(Metric::ChunksSent)).sum(),
            1
        );
    }

    #[test]
    fn border_chunks_have_an_untracked_neighbour() {
        let (mut tracker, mut links, _recorder) = tracker_with_links();
        tracker.apply_view(PLAYER, view(0, 0, 2), &mut links);
        tracker.sender.pending.clear();

        assert!(!tracker.is_chunk_on_tracked_border(ORIGIN));
        assert!(tracker.is_chunk_on_tracked_border(chunk(3, 0)));
        assert!(tracker.is_chunk_on_tracked_border(chunk(3, 3)));
        assert!(!tracker.is_chunk_on_tracked_border(chunk(2, 2)));
        assert!(!tracker.is_chunk_on_tracked_border(chunk(4, 0)));

        // 中间的区块旁边还有没发出去的区块时，它也算在边上。
        tracker.sender.mark_pending(chunk(1, 0));
        assert!(tracker.is_chunk_on_tracked_border(ORIGIN));
    }
}
