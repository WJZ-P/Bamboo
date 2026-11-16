//! 玩家加载需求：排队优先级和每名玩家的在途上限（设计稿 5.4）。
//!
//! 这里只描述「一个区块任务对玩家有多急」，不认识区块内容、线程和存档。
//! 同一个排名函数给 Director 的待派队列、宿主的读盘队列、计算池待办和交付信箱用，
//! 免得三处各排各的、互相把对方的优先级抵消掉。
//!
//! Player load demand: queue priority and per-player in-flight caps. This module
//! only ranks chunk tasks; it knows nothing about chunk contents, threads or IO.
//! The same rank is used by the Director's dispatch queue, the host's read queue,
//! compute backlog and delivery mailbox, so they cannot work against each other.

use mc_bamboo_core::ChunkPos;

/// 前方最多抵掉几个区块的距离。方向只用来打破远近的平局，不能让远处的
/// 区块抢到近处前面（`rank` 永远不小于 `distance - 2`）。
///
/// How many chunks of distance a forward position may discount. Direction only
/// breaks ties between nearby rings; a far chunk never outranks a nearer one.
pub const MAX_DIRECTION_BONUS: i64 = 2;
/// 每名玩家同时允许的「加载存档」任务数（`Read` / `Decode`）。
pub const DEFAULT_MAX_LOADS_PER_PLAYER: usize = 16;
/// 每名玩家同时允许的「新生成」任务数（`Generate`）。
pub const DEFAULT_MAX_GENERATES_PER_PLAYER: usize = 16;
/// 看得见这个任务的玩家半径：视距上限加一圈。超出的人不为它付费。
const OWNER_RADIUS: i64 = 33;

/// 一名玩家当前的加载需求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerDemand {
    /// 和票表里的玩家编号一致。
    pub id: u64,
    /// 玩家所在的区块。
    pub center: ChunkPos,
    /// 移动方向：每跨一个区块的 `(dx, dz)`，只取符号，模长不影响结果。
    /// `(0, 0)` 表示站着或方向未知，此时排名退化成纯距离。
    pub direction: (i32, i32),
}

/// 一组玩家需求。快照由宿主每刻更新，玩家跨格才变。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DemandSnapshot {
    players: Vec<PlayerDemand>,
    max_loads: usize,
    max_generates: usize,
}

impl DemandSnapshot {
    /// 按当前玩家位置建一份快照。上限为 0 表示不限（测试可以关掉限速）。
    pub fn new(players: Vec<PlayerDemand>) -> Self {
        Self::with_caps(
            players,
            DEFAULT_MAX_LOADS_PER_PLAYER,
            DEFAULT_MAX_GENERATES_PER_PLAYER,
        )
    }

    /// 指定每名玩家的在途上限。
    pub fn with_caps(
        mut players: Vec<PlayerDemand>,
        max_loads: usize,
        max_generates: usize,
    ) -> Self {
        // 按编号排，遍历顺序不依赖哈希图；同编号只留第一个。
        players.sort_by_key(|player| player.id);
        players.dedup_by_key(|player| player.id);
        Self {
            players,
            max_loads,
            max_generates,
        }
    }

    pub fn players(&self) -> &[PlayerDemand] {
        &self.players
    }

    pub const fn max_loads(&self) -> usize {
        self.max_loads
    }

    pub const fn max_generates(&self) -> usize {
        self.max_generates
    }

    /// 这个区块归哪名玩家：视距内离得最近的那个，同距离按编号小的。
    /// 只归一个人，每名玩家的在途配额才真的互相独立；多个人都看得见的
    /// 区块也只占一个人的额度。没有玩家时返回 `None`。
    ///
    /// Which player owns this chunk: the nearest one within view range,
    /// ties broken by id. Exactly one owner keeps per-player quotas disjoint.
    pub fn owner(&self, pos: ChunkPos) -> Option<u64> {
        self.players
            .iter()
            .map(|player| (chebyshev(pos, player.center), player.id))
            .filter(|&(distance, _)| distance <= OWNER_RADIUS)
            .min()
            .map(|(_, id)| id)
    }

    /// 排队用的 rank：越小越急。没有玩家或方向未知时是纯切比雪夫距离。
    ///
    /// 前方加权按「朝玩家移动方向投多少区块」线性折算，最多抵 `MAX_DIRECTION_BONUS`；
    /// 背后的区块同样最多加这么多。多名玩家取各自 rank 的最小值。
    /// 排名用 `i64`，坐标差先转宽类型，客户端能到的坐标也不会溢出。
    pub fn rank(&self, pos: ChunkPos) -> i64 {
        let mut best = i64::MAX;
        for player in &self.players {
            let distance = chebyshev(pos, player.center);
            let dx = i64::from(pos.x) - i64::from(player.center.x);
            let dz = i64::from(pos.z) - i64::from(player.center.z);
            let (vx, vz) = (i64::from(player.direction.0), i64::from(player.direction.1));
            // 投影落在 [-distance, distance]，只有朝前时才减 rank。用比较而不是
            // 归一化，避免除零和浮点；`vx*dx + vz*dz` 不会溢出 i64（坐标到 ±3e7）。
            let ahead = vx * dx + vz * dz;
            let bonus = if ahead > 0 && (vx != 0 || vz != 0) {
                // 方向向量只取符号，所以 |vx| + |vz| <= 2；投影除以它是保守的
                // 前方距离，钳到上限再折成最多 2 个区块的加权。
                let denom = vx.abs() + vz.abs();
                -(ahead / denom.max(1)).min(MAX_DIRECTION_BONUS)
            } else if ahead < 0 && (vx != 0 || vz != 0) {
                let denom = vx.abs() + vz.abs();
                (-ahead / denom.max(1)).min(MAX_DIRECTION_BONUS)
            } else {
                0
            };
            best = best.min((distance + bonus).max(0));
        }
        if best == i64::MAX { 0 } else { best }
    }
}

/// 两个区块的切比雪夫距离，先转 i64 再相减，坐标到 i32 边界也不会溢出。
pub fn chebyshev(a: ChunkPos, b: ChunkPos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dz = i64::from(a.z) - i64::from(b.z);
    dx.abs().max(dz.abs())
}

/// 任务属于哪种每玩家限速。`Save` / `Assemble` 返回 `None`：它们是收尾路径，
/// 卡住反而会让在途任务永远不释放。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemandKind {
    Load,
    Generate,
    /// 不受每玩家上限约束。
    Unbounded,
}

impl DemandKind {
    pub const fn cap(self, snapshot: &DemandSnapshot) -> usize {
        match self {
            Self::Load => snapshot.max_loads,
            Self::Generate => snapshot.max_generates,
            Self::Unbounded => usize::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demand(x: i32, z: i32, dx: i32, dz: i32) -> PlayerDemand {
        PlayerDemand {
            id: 1,
            center: ChunkPos::new(x, z),
            direction: (dx, dz),
        }
    }

    #[test]
    fn rank_is_distance_without_a_direction() {
        let snapshot = DemandSnapshot::new(vec![demand(0, 0, 0, 0)]);
        assert_eq!(snapshot.rank(ChunkPos::new(3, -4)), 4);
        assert_eq!(snapshot.rank(ChunkPos::new(-9, 2)), 9);
        assert_eq!(DemandSnapshot::default().rank(ChunkPos::new(7, 7)), 0);
    }

    #[test]
    fn forward_chunks_get_at_most_a_two_chunk_bonus() {
        let snapshot = DemandSnapshot::new(vec![demand(0, 0, 1, 0)]);
        assert_eq!(snapshot.rank(ChunkPos::new(2, 0)), 0);
        assert_eq!(snapshot.rank(ChunkPos::new(5, 0)), 3);
        assert_eq!(snapshot.rank(ChunkPos::new(5, 5)), 3);
        assert_eq!(snapshot.rank(ChunkPos::new(-5, 0)), 7);
        assert_eq!(snapshot.rank(ChunkPos::new(0, 5)), 5);
        // 方向最多抵 2，永远不会让 5 格外的前方胜过 2 格外的侧面。
        assert!(snapshot.rank(ChunkPos::new(5, 0)) > snapshot.rank(ChunkPos::new(2, 2)));
    }

    #[test]
    fn nearest_player_wins_and_owners_are_deterministic() {
        let snapshot = DemandSnapshot::new(vec![
            PlayerDemand {
                id: 7,
                center: ChunkPos::new(10, 0),
                direction: (1, 0),
            },
            demand(0, 0, 0, 0),
            demand(0, 0, 1, 0),
        ]);
        assert_eq!(snapshot.players().len(), 2, "重复的玩家编号只留一个");
        // 离 7 号玩家 2 格但在它身后（+2 罚），离 1 号玩家 8 格；取较小的 rank。
        assert_eq!(snapshot.rank(ChunkPos::new(8, 0)), 4);
        assert_eq!(snapshot.owner(ChunkPos::new(0, 0)), Some(1));
        assert_eq!(snapshot.owner(ChunkPos::new(10, 0)), Some(7));
        let mut many: Vec<PlayerDemand> = (0..8)
            .map(|i| PlayerDemand {
                id: i,
                center: ChunkPos::new(0, 0),
                direction: (0, 0),
            })
            .collect();
        many.reverse();
        let many = DemandSnapshot::new(many);
        assert_eq!(many.owner(ChunkPos::new(0, 0)), Some(0));
        assert_eq!(many.owner(ChunkPos::new(100, 100)), None);
    }

    #[test]
    fn coordinates_at_the_i32_limit_do_not_overflow() {
        let snapshot = DemandSnapshot::new(vec![demand(i32::MIN, i32::MIN, -1, -1)]);
        let _ = snapshot.rank(ChunkPos::new(i32::MAX, i32::MAX));
        assert_eq!(
            chebyshev(ChunkPos::new(i32::MIN, 0), ChunkPos::new(i32::MAX, 0)),
            4_294_967_295
        );
    }
}
