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

use std::collections::HashMap;

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
}
