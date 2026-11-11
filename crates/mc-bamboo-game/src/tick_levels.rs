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

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{
    director::Desired,
    ticket::{BLOCK_TICKING_LEVEL, ENTITY_TICKING_LEVEL, LevelChange},
};

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

/// 方块逻辑和实体分别要求半径 1 / 2 的邻居都已交付。
const BLOCK_RADIUS: i32 = 1;
const ENTITY_RADIUS: i32 = 2;

/// 每个已交付区块的档位，以及增量维护出来的三个集合。
#[derive(Debug, Default)]
pub(crate) struct TickLevels {
    /// 已交付区块的档位。`tiers` 里的键就是「已交付」这一事实。
    tiers: BTreeMap<ChunkPos, ChunkTiers>,
    /// 能跑方块逻辑的区块。
    blocks: BTreeSet<ChunkPos>,
    /// 能跑实体的区块。
    entities: BTreeSet<ChunkPos>,
    /// 逐区块 tick 的区块：属于 `blocks`，且模拟等级 ≤31。
    random: BTreeSet<ChunkPos>,
    /// 已加载模式才启用；无限超平坦模式的查询恒为真。
    enabled: bool,
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
        self.entities.clear();
        self.random.clear();
    }

    pub(crate) fn chunk_ticks_blocks(&self, pos: ChunkPos) -> bool {
        !self.enabled || self.blocks.contains(&pos)
    }

    pub(crate) fn chunk_ticks_entities(&self, pos: ChunkPos) -> bool {
        !self.enabled || self.entities.contains(&pos)
    }

    /// 逐区块 tick 的范围，按坐标排序。无限模式返回空集合，调用方走自己的老路径。
    pub(crate) fn random_chunks(&self) -> &BTreeSet<ChunkPos> {
        &self.random
    }

    /// 测试用的只读视图。
    #[cfg(test)]
    pub(crate) fn block_chunks(&self) -> &BTreeSet<ChunkPos> {
        &self.blocks
    }

    #[cfg(test)]
    pub(crate) fn entity_chunks(&self) -> &BTreeSet<ChunkPos> {
        &self.entities
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

    /// 只重算受影响的中心：一个位置换档或改了「已交付」状态，只会影响它自己
    /// 和把它当邻居的那些位置，方块集合半径 1、实体集合半径 2。
    fn recompute_around(&mut self, positions: Vec<ChunkPos>) {
        let mut block_centers = BTreeSet::new();
        let mut entity_centers = BTreeSet::new();
        for pos in positions {
            for center in neighborhood(pos, BLOCK_RADIUS) {
                block_centers.insert(center);
            }
            for center in neighborhood(pos, ENTITY_RADIUS) {
                entity_centers.insert(center);
            }
        }
        for center in block_centers {
            let eligible = self.blocks_eligible(center);
            let random = eligible && self.ticks_randomly(center);
            set_member(&mut self.blocks, center, eligible);
            set_member(&mut self.random, center, random);
        }
        for center in entity_centers {
            let eligible = self.entities_eligible(center);
            set_member(&mut self.entities, center, eligible);
        }
    }

    fn blocks_eligible(&self, pos: ChunkPos) -> bool {
        let Some(tiers) = self.tiers.get(&pos) else {
            return false;
        };
        if !matches!(tiers.loading, TickTier::Entity | TickTier::Block)
            || !matches!(tiers.simulation, TickTier::Entity | TickTier::Block)
        {
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
        for &pos in self.tiers.keys() {
            if self.blocks_eligible(pos) {
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
        assert_eq!(self.blocks, blocks, "方块集合和定义不一致");
        assert_eq!(self.entities, entities, "实体集合和定义不一致");
        assert_eq!(self.random, random, "逐区块 tick 集合和定义不一致");
    }
}

fn set_member(set: &mut BTreeSet<ChunkPos>, pos: ChunkPos, member: bool) {
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
