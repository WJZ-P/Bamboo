//! 按水平位置把玩家分进 64×64 的格子，广播时只看附近几格里的人。
//!
//! 官方 `PlayerList.broadcast` 对每个包都遍历全服玩家再算距离。这里先按格子挑出候选，
//! 再做和官方一样的距离判断，所以收到包的人一个不差，只是不用再看远处的玩家。
//! 每名玩家有自己的出站缓冲区，候选的先后不影响任何人收到的字节和顺序。
//!
//! 网格只有跟玩家的真实位置一致才可靠，所以位置只能经 [`Player::set_position`](crate::player::Player::set_position) 改，
//! 进入和离开世界分别经 `Region::insert_player` 和 [`PlayerGrid::remove`]。
//!
//! Players bucketed into 64×64 horizontal cells, so a broadcast only looks at the cells nearby.
//!
//! Vanilla `PlayerList.broadcast` walks every player on the server for every packet and checks
//! the distance. Here the grid picks candidates first and the same distance check runs on them,
//! so exactly the same players receive the packet; distant players are simply never looked at.
//! Each player has its own outbound buffer, so the order of candidates changes nobody's bytes
//! or their order.
//!
//! The grid is only as good as its agreement with real positions: positions change only through
//! [`Player::set_position`](crate::player::Player::set_position), and players enter and leave
//! through `Region::insert_player` and [`PlayerGrid::remove`].

use std::collections::{BTreeMap, HashMap};

use mc_bamboo_core::Vec3;

use crate::inbound::PlayerId;

/// 格子的边长。和世界事件、方块事件的广播半径一样，查一次最多看 3×3 格。
///
/// Cell edge length. It equals the level event and block event broadcast radius, so a query
/// looks at no more than 3×3 cells.
const CELL_SIZE: f64 = 64.0;

type Cell = (i64, i64);

/// 一个坐标落在第几格。移动包已经把坐标限制在 ±3×10⁷ 以内，转成 `i64` 不会溢出。
///
/// Which cell a coordinate falls in. Movement already clamps coordinates to ±3×10⁷, so the
/// `i64` conversion cannot overflow.
fn cell_index(coordinate: f64) -> i64 {
    (coordinate / CELL_SIZE).floor() as i64
}

fn cell_at(position: Vec3) -> Cell {
    (cell_index(position.x), cell_index(position.z))
}

/// 每名玩家在哪一格。
///
/// Which cell each player is in.
#[derive(Default)]
pub(crate) struct PlayerGrid {
    cells: HashMap<Cell, Vec<PlayerId>>,
    placed: HashMap<PlayerId, Cell>,
}

impl PlayerGrid {
    /// 记下玩家现在的位置。还在同一格时什么也不做。
    ///
    /// Records the player's current position. Does nothing while it stays in the same cell.
    pub(crate) fn place(&mut self, id: PlayerId, position: Vec3) {
        let cell = cell_at(position);
        match self.placed.insert(id, cell) {
            Some(old) if old == cell => return,
            Some(old) => self.take_out(id, old),
            None => {}
        }
        self.cells.entry(cell).or_default().push(id);
    }

    /// 玩家离开世界。
    ///
    /// The player leaves the world.
    pub(crate) fn remove(&mut self, id: PlayerId) {
        if let Some(cell) = self.placed.remove(&id) {
            self.take_out(id, cell);
        }
    }

    fn take_out(&mut self, id: PlayerId, cell: Cell) {
        if let Some(ids) = self.cells.get_mut(&cell) {
            ids.retain(|&other| other != id);
            if ids.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// 岛拆分：把 `players` 的格子登记搬进一份新网格。格子里的先后不影响任何人收到的包。
    ///
    /// Island split: move the cells of `players` into a new grid.
    pub(crate) fn split_off(&mut self, players: &std::collections::BTreeSet<PlayerId>) -> Self {
        let mut out = Self::default();
        for &id in players {
            if let Some(cell) = self.placed.remove(&id) {
                self.take_out(id, cell);
                out.placed.insert(id, cell);
                out.cells.entry(cell).or_default().push(id);
            }
        }
        out
    }

    /// 岛合并：按玩家编号把另一份网格的登记并进来。
    ///
    /// Island merge: add the other grid's players in id order.
    pub(crate) fn absorb(&mut self, other: Self) {
        let mut placed: Vec<(PlayerId, Cell)> = other.placed.into_iter().collect();
        placed.sort_unstable();
        for (id, cell) in placed {
            debug_assert!(!self.placed.contains_key(&id), "玩家在两个岛上");
            self.placed.insert(id, cell);
            self.cells.entry(cell).or_default().push(id);
        }
    }

    /// 登记过的玩家，按编号升序。测试核对拆合用。
    #[cfg(test)]
    pub(crate) fn placed_players(&self) -> Vec<PlayerId> {
        let mut out: Vec<PlayerId> = self.placed.keys().copied().collect();
        out.sort_unstable();
        out
    }

    /// 水平方向上 x、z 各自离得不到 `radius` 的玩家都在里面。
    ///
    /// 结果可能多出几个远一点的人，调用方要再按真实距离筛一遍。
    ///
    /// Every player whose x and z are each less than `radius` away is included. The result may
    /// contain a few players further away, so callers filter again by the real distance.
    pub(crate) fn near(&self, x: f64, z: f64, radius: f64) -> impl Iterator<Item = PlayerId> + '_ {
        let (min_x, max_x) = (cell_index(x - radius), cell_index(x + radius));
        let (min_z, max_z) = (cell_index(z - radius), cell_index(z + radius));
        (min_x..=max_x)
            .flat_map(move |cell_x| (min_z..=max_z).map(move |cell_z| (cell_x, cell_z)))
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
            .copied()
    }

    /// 水平距离不超过 `radius` 的玩家，按 uid 升序；距离用玩家表里的真实位置算，
    /// 网格只负责给出候选。这个查询不依赖视距，也不要求区块已经发到客户端。
    ///
    /// Players within `radius` horizontally, in ascending uid order. The grid
    /// only supplies candidates; the real positions from the player table give
    /// the exact distance. No view-distance or tracked-chunk condition applies.
    /// 第一批生物接进降频 / 刷怪阶段之前没有调用方，允许暂时未被使用。
    /// The first mob batch is not wired yet, so this has no caller for now.
    #[allow(dead_code)]
    pub(crate) fn players_within<'a>(
        &'a self,
        x: f64,
        z: f64,
        radius: f64,
        players: &'a BTreeMap<PlayerId, crate::player::Player>,
    ) -> impl Iterator<Item = (PlayerId, f64)> + 'a {
        let radius_sq = radius * radius;
        let mut found: Vec<(PlayerId, f64)> = self
            .near(x, z, radius)
            .filter_map(|id| players.get(&id))
            .filter_map(|player| {
                let dx = player.position().x - x;
                let dz = player.position().z - z;
                let distance_sq = dx * dx + dz * dz;
                (distance_sq <= radius_sq).then_some((player.id, distance_sq.sqrt()))
            })
            .collect();
        found.sort_unstable_by_key(|(id, _)| *id);
        found.into_iter()
    }

    /// 离得最近的水平距离；没有玩家时是 `None`。`radius` 限制网格候选范围，
    /// 超出这个距离的一律不算（设计稿的休眠档阈值由调用方给）。
    ///
    /// Distance to the nearest player, or `None` when there is none. `radius`
    /// bounds the grid candidate search; the caller supplies the design's
    /// sleep-tier threshold. Also unused until the first mob batch arrives.
    #[allow(dead_code)]
    pub(crate) fn nearest_distance(
        &self,
        x: f64,
        z: f64,
        radius: f64,
        players: &BTreeMap<PlayerId, crate::player::Player>,
    ) -> Option<f64> {
        let mut best = f64::INFINITY;
        for id in self.near(x, z, radius) {
            let Some(player) = players.get(&id) else {
                continue;
            };
            let dx = player.position().x - x;
            let dz = player.position().z - z;
            best = best.min((dx * dx + dz * dz).sqrt());
        }
        (best <= radius).then_some(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near_ids(grid: &PlayerGrid, x: f64, z: f64) -> Vec<i32> {
        let mut ids: Vec<i32> = grid.near(x, z, 64.0).map(|id| id.0).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn players_within_the_radius_are_always_candidates() {
        let mut grid = PlayerGrid::default();
        // 格子边界两侧、负坐标，都要在 64 格以内被找到。
        // Both sides of a cell border, and negative coordinates, are found within 64 blocks.
        grid.place(PlayerId(1), Vec3::new(63.9, 0.0, 0.0));
        grid.place(PlayerId(2), Vec3::new(-0.1, 0.0, -63.9));
        grid.place(PlayerId(3), Vec3::new(-127.9, 0.0, 0.0));
        grid.place(PlayerId(4), Vec3::new(500.0, 0.0, 500.0));

        assert_eq!(near_ids(&grid, 0.0, 0.0), [1, 2]);
        // 候选可以多：1 号离得超过 64 格，但和 2、3 号在相邻的格子里。
        // Candidates may be extra: player 1 is over 64 blocks away but shares the neighbouring cells.
        assert_eq!(near_ids(&grid, -64.0, 0.0), [1, 2, 3]);
        assert_eq!(near_ids(&grid, 480.0, 480.0), [4]);
    }

    #[test]
    fn moving_and_leaving_update_the_cells() {
        let mut grid = PlayerGrid::default();
        grid.place(PlayerId(1), Vec3::new(0.0, 0.0, 0.0));
        grid.place(PlayerId(1), Vec3::new(1000.0, 0.0, 0.0));

        assert!(near_ids(&grid, 0.0, 0.0).is_empty());
        assert_eq!(near_ids(&grid, 1000.0, 0.0), [1]);

        grid.remove(PlayerId(1));
        assert!(near_ids(&grid, 1000.0, 0.0).is_empty());
        assert!(grid.cells.is_empty());
    }
    /// 「某个区块若干格内有哪些玩家、最近的玩家多远」的查询接口：
    /// 网格给候选，玩家表给真实坐标，结果按 uid 升序。
    #[test]
    fn nearby_player_queries_use_real_positions_and_uid_order() {
        use mc_bamboo_core::Vec3;
        use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
        use uuid::Uuid;

        fn player(id: i32, x: f64, z: f64) -> crate::player::Player {
            crate::player::Player::new(
                PlayerId(id),
                Uuid::nil(),
                format!("P{id}"),
                GameMode::Creative,
                ClientInformation::default(),
                Vec3::new(x, 64.0, z),
            )
        }

        let mut grid = PlayerGrid::default();
        let mut players = BTreeMap::new();
        for (id, x, z) in [
            (9, 30.0, 40.0), // 距离 50
            (2, 3.0, 4.0),   // 距离 5
            (7, 100.0, 0.0), // 100，在 64 格外
            (5, -64.1, 0.0), // 64.1，网格相邻但精确距离超了
        ] {
            let p = player(id, x, z);
            grid.place(PlayerId(id), p.position());
            players.insert(PlayerId(id), p);
        }
        let found: Vec<_> = grid.players_within(0.0, 0.0, 64.0, &players).collect();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, PlayerId(2));
        assert_eq!(found[1].0, PlayerId(9));
        assert!((found[0].1 - 5.0).abs() < 1.0e-9);
        assert!((found[1].1 - 50.0).abs() < 1.0e-9);
        assert_eq!(grid.nearest_distance(0.0, 0.0, 64.0, &players), Some(5.0));
        assert_eq!(grid.nearest_distance(0.0, 0.0, 4.0, &players), None);
        assert_eq!(grid.nearest_distance(1000.0, 1000.0, 64.0, &players), None);

        // 玩家跨格子移动后，查询跟着真实位置走；离开世界后不再出现。
        let moved = player(2, 300.0, 300.0);
        grid.place(PlayerId(2), moved.position());
        players.insert(PlayerId(2), moved);
        assert_eq!(grid.nearest_distance(0.0, 0.0, 64.0, &players), Some(50.0));
        grid.remove(PlayerId(9));
        players.remove(&PlayerId(9));
        assert!(
            grid.players_within(0.0, 0.0, 64.0, &players)
                .next()
                .is_none()
        );
    }

    /// 改坏确认：网格里留着旧格子、或者玩家表给了错的位置，查询都必须看得出来。
    #[test]
    fn stale_grid_cells_would_fail_the_nearby_player_queries() {
        use mc_bamboo_core::Vec3;
        use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
        use uuid::Uuid;

        let id = PlayerId(1);
        let mut grid = PlayerGrid::default();
        let mut players = BTreeMap::new();
        grid.place(id, Vec3::new(0.0, 64.0, 0.0));
        let player = crate::player::Player::new(
            id,
            Uuid::nil(),
            "P1".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(0.0, 64.0, 0.0),
        );
        players.insert(id, player);
        assert_eq!(grid.nearest_distance(0.0, 0.0, 64.0, &players), Some(0.0));

        // 把格子改坏（玩家其实已经走远，但网格没更新），精确距离会排除这个候选。
        grid.place(id, Vec3::new(0.0, 64.0, 0.0));
        let moved = crate::player::Player::new(
            id,
            Uuid::nil(),
            "P1".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(500.0, 64.0, 0.0),
        );
        players.insert(id, moved);
        assert_eq!(grid.nearest_distance(0.0, 0.0, 64.0, &players), None);
        assert!(
            grid.players_within(0.0, 0.0, 64.0, &players)
                .next()
                .is_none()
        );
    }
}
