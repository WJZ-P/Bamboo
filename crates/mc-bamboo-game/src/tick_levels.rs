//! 按区块的加载/模拟等级档位决定每个区块参与哪些 tick 阶段。
//!
//! 只在已加载模式启用；无限超平坦模式的所有位置都算能 tick（[`TickLevels::new`]
//! 用 `enabled = false` 关掉整套判断，查询恒为真）。等级只从 Director 发来的消息来：
//! `Deliver` 的 `desired`、`LevelChanged` 的新值、收回。region 不查票表。
//!
//! 两个集合按下面的定义增量维护，查询是常数时间：
//!
//! - 方块逻辑：两套等级都 ≤32，且自己和周围 8 个区块都已交付；
//! - 实体：两套等级都 ≤31，且自己和周围两圈（5×5）都已交付。
//!
//! 逐区块 tick 还要模拟等级 ≤31，单独存一个集合。
//!
//! 另有一组只看加载等级的「可运行」状态，对应官方 `prepareTickingChunk`：加载等级 ≤32
//! 且周围 8 个区块都已交付时进「待处理」，岛线程做完完工后处理（见
//! [`crate::generation_post_process`]）后进「可发送」，跌出这一条件时两边都清掉。
//! 玩家视距内、模拟距离外的区块也会走到这一步，所以不能复用方块逻辑集合。

#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::{BTreeSet, HashMap, HashSet};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{
    director::Desired,
    ticket::{BLOCK_TICKING_LEVEL, ChunkHasherBuilder, ENTITY_TICKING_LEVEL, LevelChange},
};

/// 成员查询用的快速哈希集合。「这个区块能不能 tick」每条到期的刻、每个方块
/// 实体、每个实体每刻都要问一次；有序集合留给测试和逐区块遍历。
type ChunkSet = HashSet<ChunkPos, ChunkHasherBuilder>;
/// 档位表：换档、邻居查询都按坐标单点查，用便宜哈希；`random_chunks` 需要
/// 有序遍历，单独维护一份有序集合。
type TierMap = HashMap<ChunkPos, ChunkTiers, ChunkHasherBuilder>;

/// 一套等级里和 tick 有关的三档。跨档才会收到通知，所以记档位就够了。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TickTier {
    Entity,
    Block,
    Loaded,
}

impl TickTier {
    const fn of_level(level: i32) -> Self {
        if level <= ENTITY_TICKING_LEVEL {
            Self::Entity
        } else if level <= BLOCK_TICKING_LEVEL {
            Self::Block
        } else {
            Self::Loaded
        }
    }
}

/// 一个区块两套等级的档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkTiers {
    pub(crate) loading: TickTier,
    pub(crate) simulation: TickTier,
}

impl ChunkTiers {
    fn of_desired(desired: Desired) -> Self {
        Self {
            loading: TickTier::of_level(desired.loading_level),
            simulation: TickTier::of_level(desired.simulation_level),
        }
    }

    fn of_change(change: LevelChange) -> Self {
        Self {
            loading: TickTier::of_level(change.loading.1),
            simulation: TickTier::of_level(change.simulation.1),
        }
    }

    /// 逐区块 tick（随机刻）除了能跑方块逻辑，还要求模拟等级 ≤31。
    const fn ticks_randomly(self) -> bool {
        matches!(self.simulation, TickTier::Entity)
    }
}

/// 从一个有序集合里拆出 `chunks` 里的那些：要搬的不到一半时逐个取（按升序收进新集合是
/// 整批建树），否则扫一遍整批重建两边。两种做法结果相同。
///
/// Split the members in `chunks` off an ordered set: one by one when less
/// than half moves (collecting in ascending order builds the tree in bulk), otherwise
/// one pass rebuilding both sides in bulk. Same result either way.
fn split_ordered(set: &mut BTreeSet<ChunkPos>, chunks: &BTreeSet<ChunkPos>) -> BTreeSet<ChunkPos> {
    if chunks.len().saturating_mul(2) < set.len() {
        chunks
            .iter()
            .filter(|pos| set.remove(pos))
            .copied()
            .collect()
    } else {
        let (moved, kept) = std::mem::take(set)
            .into_iter()
            .partition(|pos| chunks.contains(pos));
        *set = kept;
        moved
    }
}

/// 把一个有序集合并进另一个：小的逐个插进大的，差不多大时整批归并。结果相同。
///
/// Merge an ordered set into another: the smaller is inserted one by one
/// into the larger, similar sizes are merged in bulk. Same result.
fn absorb_ordered(into: &mut BTreeSet<ChunkPos>, mut from: BTreeSet<ChunkPos>) {
    if from.len() > into.len() {
        std::mem::swap(into, &mut from);
    }
    if from.len().saturating_mul(16) < into.len() {
        into.extend(from);
    } else {
        into.append(&mut from);
    }
}

/// 方块逻辑和实体分别要求半径 1 / 2 的邻居都已交付。
const BLOCK_RADIUS: i32 = 1;
const ENTITY_RADIUS: i32 = 2;

/// 每个已交付区块的档位，以及增量维护出来的三个集合。
#[derive(Debug, Default)]
pub(crate) struct TickLevels {
    /// 已交付区块的档位。`tiers` 里的键就是「已交付」这一事实。
    tiers: TierMap,
    /// 能跑方块逻辑的区块，供成员查询。（以前另有一份有序的同样内容，只有测试读，
    /// 岛拆合时却每个区块都要搬；测试现在按需从这一份排出有序的。）
    ///
    /// Chunks that run block logic, for membership queries. (An ordered copy
    /// used to be kept as well, read only by tests yet moved chunk by chunk
    /// on every island split and merge; tests now sort this one on demand.)
    blocks: ChunkSet,
    /// 加载档达到 BLOCK_TICKING 且 3×3 已交付，等岛线程执行完工后处理。
    /// Loading-tier promotions awaiting generation post-processing on the island.
    preparing: BTreeSet<ChunkPos>,
    /// 同一加载档内已完成后处理，可以向玩家发首包。
    /// Prepared chunks in the current loading tier, eligible for sending.
    ready: ChunkSet,
    /// 能跑实体的区块，同上。
    entities: ChunkSet,
    /// 逐区块 tick 的区块：属于 `blocks`，且模拟等级 ≤31。
    random: BTreeSet<ChunkPos>,
    /// 已加载模式才启用；无限超平坦模式的查询恒为真。
    enabled: bool,
    /// 「能跑方块逻辑」集合的成员变化，等方块实体阶段开头一次性交给
    /// `BlockEntities::set_chunk_ticking`（冻结时把睡着的日程暂停）。
    block_tick_changes: Vec<(ChunkPos, bool)>,
}

impl TickLevels {
    /// `enabled` 由 [`mc_bamboo_world::store::ChunkStore::is_loaded_mode`] 决定。
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    /// 切到无限超平坦模式：丢掉全部记账，之后的查询恒为真。
    pub(crate) fn disable(&mut self) {
        self.enabled = false;
        self.tiers.clear();
        self.blocks.clear();
        self.preparing.clear();
        self.ready.clear();
        self.entities.clear();
        self.random.clear();
    }

    /// 取走这一刻之前新跨入可运行、等完工后处理的区块，按坐标升序。
    /// Take the chunks awaiting generation post-processing, in position order.
    pub(crate) fn take_preparing(&mut self) -> BTreeSet<ChunkPos> {
        std::mem::take(&mut self.preparing)
    }

    /// 后处理做完：在当前加载档内算可发送，直到跌出可运行。
    /// Post-processing done: sendable until the chunk drops out of ticking.
    pub(crate) fn finish_preparing(&mut self, pos: ChunkPos) {
        if self.loading_ready(pos) {
            self.ready.insert(pos);
        }
    }

    /// 区块做完完工后处理、可以发给玩家（官方 `onChunkReadyToSend` 之后）。
    /// Whether the chunk finished generation post-processing and may be sent.
    pub(crate) fn ready_to_send(&self, pos: ChunkPos) -> bool {
        !self.enabled || self.ready.contains(&pos) || crate::generation_post_process::send_early()
    }

    /// 后处理已经做完（不看改坏开关）。对拍用它判断处理发生了没有。
    /// Post-processing has run (ignores the break switch); the oracle uses it.
    pub(crate) fn prepared(&self, pos: ChunkPos) -> bool {
        self.ready.contains(&pos)
    }

    pub(crate) fn chunk_ticks_blocks(&self, pos: ChunkPos) -> bool {
        !self.enabled || self.blocks.contains(&pos)
    }

    /// 取走这一刻之前积累的「能跑方块逻辑」成员变化。
    pub(crate) fn take_block_tick_changes(&mut self) -> Vec<(ChunkPos, bool)> {
        std::mem::take(&mut self.block_tick_changes)
    }

    pub(crate) fn chunk_ticks_entities(&self, pos: ChunkPos) -> bool {
        !self.enabled || self.entities.contains(&pos)
    }

    /// 逐区块 tick 的范围，按坐标排序。无限模式返回空集合，调用方走自己的老路径。
    pub(crate) fn random_chunks(&self) -> &BTreeSet<ChunkPos> {
        &self.random
    }

    /// 测试用的只读视图（按坐标有序，口径不变）。
    #[cfg(test)]
    pub(crate) fn block_chunks(&self) -> BTreeSet<ChunkPos> {
        self.blocks.iter().copied().collect()
    }

    #[cfg(test)]
    pub(crate) fn entity_chunks(&self) -> BTreeSet<ChunkPos> {
        self.entities.iter().copied().collect()
    }

    #[cfg(test)]
    pub(crate) fn tiers(&self, pos: ChunkPos) -> Option<ChunkTiers> {
        self.tiers.get(&pos).copied()
    }

    /// Director 交付一批区块。一次只重算这批位置和它们半径 1 / 2 的邻居。
    pub(crate) fn deliver(&mut self, delivered: impl IntoIterator<Item = (ChunkPos, Desired)>) {
        if !self.enabled {
            return;
        }
        let mut positions = Vec::new();
        for (pos, desired) in delivered {
            self.tiers.insert(pos, ChunkTiers::of_desired(desired));
            positions.push(pos);
        }
        self.recompute_around(positions);
    }

    /// 一套等级跨了 31 / 32 / 33 边界。消息里也可能有还没交付、或已经收回的位置：
    /// 只有已交付的位置才有档位要换，其余忽略（下一次 `Deliver` 会带最新档位）。
    pub(crate) fn level_changed(&mut self, changes: &[LevelChange]) {
        if !self.enabled {
            return;
        }
        let mut positions = Vec::with_capacity(changes.len());
        for &change in changes {
            let pos = change.pos;
            if let Some(tiers) = self.tiers.get_mut(&pos) {
                *tiers = ChunkTiers::of_change(change);
                positions.push(pos);
            }
        }
        self.recompute_around(positions);
    }

    /// Director 收回一批区块。
    pub(crate) fn reclaim<'a>(&mut self, positions: impl IntoIterator<Item = &'a ChunkPos>) {
        if !self.enabled {
            return;
        }
        let mut changed = Vec::new();
        for &pos in positions {
            self.tiers.remove(&pos);
            changed.push(pos);
        }
        self.recompute_around(changed);
    }

    /// 岛拆分（岛的实施计划 3.3）：`chunks` 的档位、三个集合和还没交出去的成员变化整批
    /// 搬进一份新表，不重算。方块逻辑看半径 1、实体看半径 2 的邻居是否已交付，所以要求
    /// 两部分的已交付区块切比雪夫距离至少 3（调用方断言；分区器保证至少隔 8 个区块），
    /// 这时没有一个中心的邻域跨两部分，拆前算好的集合两边都还对。
    ///
    /// Island split: move the tiers, the three sets and the pending membership
    /// changes of `chunks` into a new table without recomputing. Block logic
    /// looks at radius 1 and entities at radius 2, so the two parts' delivered
    /// chunks must be at least 3 apart (asserted by the caller), and then no
    /// neighbourhood spans both parts.
    pub(crate) fn split_off(&mut self, chunks: &BTreeSet<ChunkPos>) -> Self {
        let mut out = Self::new(self.enabled);
        for &pos in chunks {
            if let Some(tiers) = self.tiers.remove(&pos) {
                out.tiers.insert(pos, tiers);
            }
            for (from, to) in [
                (&mut self.blocks, &mut out.blocks),
                (&mut self.ready, &mut out.ready),
                (&mut self.entities, &mut out.entities),
            ] {
                if from.remove(&pos) {
                    to.insert(pos);
                }
            }
        }
        out.preparing = split_ordered(&mut self.preparing, chunks);
        out.random = split_ordered(&mut self.random, chunks);
        let (moved, kept) = std::mem::take(&mut self.block_tick_changes)
            .into_iter()
            .partition(|(pos, _)| chunks.contains(pos));
        self.block_tick_changes = kept;
        out.block_tick_changes = moved;
        out
    }

    /// 岛合并：[`Self::split_off`] 的逆操作，同样要求两边隔开至少 3 个区块。
    ///
    /// Island merge: the inverse of [`Self::split_off`], with the same gap.
    pub(crate) fn absorb(&mut self, other: Self) {
        debug_assert_eq!(self.enabled, other.enabled);
        self.tiers.extend(other.tiers);
        self.blocks.extend(other.blocks);
        self.ready.extend(other.ready);
        absorb_ordered(&mut self.preparing, other.preparing);
        self.entities.extend(other.entities);
        absorb_ordered(&mut self.random, other.random);
        self.block_tick_changes.extend(other.block_tick_changes);
    }

    /// 已交付（有档位）的区块，按坐标升序。测试核对拆合用。
    #[cfg(test)]
    pub(crate) fn delivered(&self) -> BTreeSet<ChunkPos> {
        self.tiers.keys().copied().collect()
    }

    /// 只重算受影响的中心：一个位置换档或改了「已交付」状态，只会影响它自己
    /// 和把它当邻居的那些位置，方块集合半径 1、实体集合半径 2。
    fn recompute_around(&mut self, positions: Vec<ChunkPos>) {
        // 受影响的中心按坐标去重、有序输出；集合本身也用快速哈希。
        let mut block_centers = ChunkSet::default();
        let mut entity_centers = ChunkSet::default();
        for pos in positions {
            for center in neighborhood(pos, BLOCK_RADIUS) {
                block_centers.insert(center);
            }
            for center in neighborhood(pos, ENTITY_RADIUS) {
                entity_centers.insert(center);
            }
        }
        let mut block_centers: Vec<ChunkPos> = block_centers.into_iter().collect();
        block_centers.sort_unstable();
        let mut entity_centers: Vec<ChunkPos> = entity_centers.into_iter().collect();
        entity_centers.sort_unstable();
        for center in block_centers {
            let full_ring = self.loading_ready(center);
            if !full_ring {
                self.ready.remove(&center);
                self.preparing.remove(&center);
            } else if !self.ready.contains(&center) {
                self.preparing.insert(center);
            }
            let eligible = full_ring && self.tiers[&center].simulation <= TickTier::Block;
            let random = eligible && self.ticks_randomly(center);
            let was = self.blocks.contains(&center);
            if was != eligible {
                self.block_tick_changes.push((center, eligible));
            }
            set_member(&mut self.blocks, center, eligible);
            set_member_ordered(&mut self.random, center, random);
        }
        for center in entity_centers {
            let eligible = self.entities_eligible(center);
            set_member(&mut self.entities, center, eligible);
        }
    }

    fn loading_ready(&self, pos: ChunkPos) -> bool {
        let Some(tiers) = self.tiers.get(&pos) else {
            return false;
        };
        if tiers.loading > TickTier::Block {
            return false;
        }
        neighborhood(pos, BLOCK_RADIUS).all(|neighbor| self.tiers.contains_key(&neighbor))
    }

    fn entities_eligible(&self, pos: ChunkPos) -> bool {
        let Some(tiers) = self.tiers.get(&pos) else {
            return false;
        };
        if !matches!(tiers.loading, TickTier::Entity)
            || !matches!(tiers.simulation, TickTier::Entity)
        {
            return false;
        }
        neighborhood(pos, ENTITY_RADIUS).all(|neighbor| self.tiers.contains_key(&neighbor))
    }

    fn ticks_randomly(&self, pos: ChunkPos) -> bool {
        self.tiers
            .get(&pos)
            .is_some_and(|tiers| tiers.ticks_randomly())
    }

    /// 从头按定义算一遍，给测试和调试用。不在每刻跑。
    #[cfg(test)]
    pub(crate) fn recompute_all(
        &self,
    ) -> (BTreeSet<ChunkPos>, BTreeSet<ChunkPos>, BTreeSet<ChunkPos>) {
        let mut blocks = BTreeSet::new();
        let mut entities = BTreeSet::new();
        let mut random = BTreeSet::new();
        let mut positions: Vec<ChunkPos> = self.tiers.keys().copied().collect();
        positions.sort_unstable();
        for pos in positions {
            if self.loading_ready(pos) && self.tiers[&pos].simulation <= TickTier::Block {
                blocks.insert(pos);
                if self.ticks_randomly(pos) {
                    random.insert(pos);
                }
            }
            if self.entities_eligible(pos) {
                entities.insert(pos);
            }
        }
        (blocks, entities, random)
    }

    /// 测试断言：增量维护出的三个集合和从头算的一致。
    #[cfg(test)]
    pub(crate) fn assert_invariants(&self) {
        let (blocks, entities, random) = self.recompute_all();
        assert_eq!(self.block_chunks(), blocks, "方块集合和定义不一致");
        assert_eq!(self.entity_chunks(), entities, "实体集合和定义不一致");
        assert_eq!(self.random, random, "逐区块 tick 集合和定义不一致");
    }
}

fn set_member(set: &mut ChunkSet, pos: ChunkPos, member: bool) {
    if member {
        set.insert(pos);
    } else {
        set.remove(&pos);
    }
}

fn set_member_ordered(set: &mut BTreeSet<ChunkPos>, pos: ChunkPos, member: bool) {
    if member {
        set.insert(pos);
    } else {
        set.remove(&pos);
    }
}

/// 以 `pos` 为中心、`radius` 为半径的方形（含中心）。坐标加减用饱和算术，
/// 客户端给到极端坐标时不会溢出。
fn neighborhood(pos: ChunkPos, radius: i32) -> impl Iterator<Item = ChunkPos> {
    (-radius..=radius).flat_map(move |dx| {
        (-radius..=radius)
            .map(move |dz| ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz)))
    })
}

#[cfg(test)]
mod tests {
    use mc_bamboo_world::ticket::UNLOADED_LEVEL;

    use super::*;

    fn desired(loading: i32, simulation: i32) -> Desired {
        Desired::new(loading, simulation)
    }

    fn change(pos: ChunkPos, loading: i32, simulation: i32) -> LevelChange {
        LevelChange {
            pos,
            loading: (0, loading),
            simulation: (0, simulation),
        }
    }

    #[test]
    fn sending_waits_for_preparation_even_outside_simulation_distance() {
        let center = ChunkPos::new(0, 0);
        let corner = ChunkPos::new(1, 1);
        let mut levels = TickLevels::new(true);
        levels.deliver(
            neighborhood(center, 1)
                .filter(|pos| *pos != corner)
                .map(|pos| {
                    (
                        pos,
                        desired(if pos == center { 32 } else { 33 }, UNLOADED_LEVEL),
                    )
                }),
        );
        assert!(levels.take_preparing().is_empty());
        assert!(!levels.ready_to_send(center));
        levels.deliver([(corner, desired(33, UNLOADED_LEVEL))]);
        assert_eq!(levels.take_preparing(), BTreeSet::from([center]));
        assert!(!levels.ready_to_send(center));
        levels.finish_preparing(center);
        assert!(levels.ready_to_send(center));
        assert!(!levels.chunk_ticks_blocks(center));

        levels.level_changed(&[change(center, 33, 31)]);
        assert!(!levels.ready_to_send(center));
        levels.level_changed(&[change(center, 32, 31)]);
        levels.reclaim([&corner]);
        assert!(levels.take_preparing().is_empty());
        assert!(!levels.ready_to_send(center));
    }

    /// 交付一整片，中心全是实体档，边缘留给调用方改档。
    fn field(radius: i32, level: i32) -> BTreeMap<ChunkPos, Desired> {
        let mut out = BTreeMap::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                out.insert(ChunkPos::new(dx, dz), desired(level, level));
            }
        }
        out
    }

    #[test]
    fn infinite_flat_mode_ticks_everything() {
        let mut levels = TickLevels::new(false);
        levels.deliver([(ChunkPos::new(0, 0), desired(31, 31))]);
        assert!(levels.chunk_ticks_blocks(ChunkPos::new(100, 100)));
        assert!(levels.chunk_ticks_entities(ChunkPos::new(100, 100)));
        assert!(levels.block_chunks().is_empty());
    }

    #[test]
    fn a_single_delivered_chunk_cannot_tick() {
        let mut levels = TickLevels::new(true);
        levels.deliver([(ChunkPos::new(0, 0), desired(31, 31))]);
        assert!(!levels.chunk_ticks_blocks(ChunkPos::new(0, 0)));
        assert!(!levels.chunk_ticks_entities(ChunkPos::new(0, 0)));
        levels.assert_invariants();
    }

    #[test]
    fn a_full_5x5_field_ticks_the_center_only() {
        let mut levels = TickLevels::new(true);
        let all = field(3, 31);
        // 半径 3 全交付：中心两圈齐了，离边两格以内的都能跑实体。
        levels.deliver(all);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let pos = ChunkPos::new(dx, dz);
                assert!(levels.chunk_ticks_entities(pos), "{pos:?} 该 tick 实体");
                assert!(levels.chunk_ticks_blocks(pos), "{pos:?} 该 tick 方块");
            }
        }
        assert!(
            !levels.chunk_ticks_entities(ChunkPos::new(2, 0)),
            "5×5 差一格的区块不能 tick 实体"
        );
        assert!(levels.chunk_ticks_blocks(ChunkPos::new(2, 0)));
        assert_eq!(levels.random_chunks().len(), 5 * 5);
        levels.assert_invariants();
    }

    #[test]
    fn simulation_and_loading_tiers_both_gate_ticks() {
        let mut levels = TickLevels::new(true);
        let all: Vec<_> = field(3, 31).into_iter().collect();
        levels.deliver(all);
        let center = ChunkPos::new(0, 0);
        // 模拟等级降到 32：方块还能跑，实体和随机刻不能跑。
        levels.level_changed(&[change(center, 31, 32)]);
        assert!(levels.chunk_ticks_blocks(center));
        assert!(!levels.chunk_ticks_entities(center));
        assert!(!levels.random_chunks().contains(&center));
        // 加载等级降到 33：这个区块方块也停了；邻居仍然能跑（它们自己的等级没变）。
        levels.level_changed(&[change(center, 33, 32)]);
        assert!(!levels.chunk_ticks_blocks(center));
        assert!(levels.chunk_ticks_blocks(ChunkPos::new(1, 0)));
        levels.assert_invariants();
    }

    #[test]
    fn reclaiming_one_chunk_disables_its_neighbors() {
        let mut levels = TickLevels::new(true);
        levels.deliver(field(2, 31).into_iter().collect::<Vec<_>>());
        assert!(levels.chunk_ticks_entities(ChunkPos::new(0, 0)));
        levels.reclaim([&ChunkPos::new(2, 2)]);
        assert!(
            !levels.chunk_ticks_entities(ChunkPos::new(0, 0)),
            "两圈里少一角就不能 tick 实体"
        );
        assert!(levels.chunk_ticks_blocks(ChunkPos::new(0, 0)));
        levels.assert_invariants();
    }

    #[test]
    fn a_level_change_for_an_undelivered_chunk_is_ignored() {
        let mut levels = TickLevels::new(true);
        levels.deliver(field(2, 31).into_iter().collect::<Vec<_>>());
        let far = ChunkPos::new(10, 10);
        levels.level_changed(&[change(far, 33, 33)]);
        assert_eq!(levels.tiers(far), None, "没交付的位置不该当已交付记账");
        assert!(!levels.chunk_ticks_blocks(far));
        levels.deliver([(far, desired(33, 33))]);
        assert_eq!(
            levels.tiers(far).map(|tiers| tiers.loading),
            Some(TickTier::Loaded),
            "Deliver 带的是最新档位"
        );
        levels.assert_invariants();
    }

    #[test]
    fn unloaded_tiers_are_just_loaded() {
        assert_eq!(TickTier::of_level(UNLOADED_LEVEL), TickTier::Loaded);
        assert_eq!(TickTier::of_level(33), TickTier::Loaded);
        assert_eq!(TickTier::of_level(32), TickTier::Block);
        assert_eq!(TickTier::of_level(31), TickTier::Entity);
    }
}
