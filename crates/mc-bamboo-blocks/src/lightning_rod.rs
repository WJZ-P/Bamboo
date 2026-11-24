//! 雷击落点里的避雷针查找，官方 `ServerLevel.findLightningRod`。
//!
//! 查询本身走 `PoiManager` 里的 `LIGHTNING_ROD` POI（见
//! [`Level::lightning_rod_near`](crate::Level::lightning_rod_near)）；这里只保留
//! 官方传入的搜索半径和一组逐格结果的单测。

/// `findLightningRod` 的搜索半径，官方传入 128。
pub const LIGHTNING_ROD_RADIUS: i32 = 128;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Level, state::BlockState, testing::TestLevel};
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    fn rod(level: &mut TestLevel, at: BlockPos) {
        level.set(at, BlockState(blocks::LIGHTNING_ROD));
    }

    #[test]
    fn nearest_of_multiple_and_tie_follow_poi_order() {
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        // 两根等距：POI 遍历序里 chunk x = -1 的那根先，取它上方一格。
        rod(&mut level, BlockPos::new(-3, 63, 0));
        rod(&mut level, BlockPos::new(3, 63, 0));
        assert_eq!(
            level.lightning_rod_near(center, LIGHTNING_ROD_RADIUS),
            Some(BlockPos::new(-3, 64, 0))
        );
        // 后来放一根更近的：距离平方更小者胜出。
        rod(&mut level, BlockPos::new(1, 63, 0));
        assert_eq!(
            level.lightning_rod_near(center, LIGHTNING_ROD_RADIUS),
            Some(BlockPos::new(1, 64, 0))
        );
    }

    #[test]
    fn buried_rod_is_not_a_target() {
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        rod(&mut level, BlockPos::new(2, 63, 0));
        // 上面盖一格石头，针就不在 `WORLD_SURFACE - 1` 上。
        level.set(BlockPos::new(2, 64, 0), BlockState(blocks::STONE));
        assert_eq!(level.lightning_rod_near(center, LIGHTNING_ROD_RADIUS), None);
    }

    #[test]
    fn rod_beyond_radius_is_ignored() {
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        rod(&mut level, BlockPos::new(200, 63, 0));
        assert_eq!(level.lightning_rod_near(center, LIGHTNING_ROD_RADIUS), None);
    }

    #[test]
    fn blocked_rod_does_not_shadow_a_farther_exposed_one() {
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        // 近处一根被埋，远处一根露天：官方只认高度图上的那一格。
        rod(&mut level, BlockPos::new(2, 63, 0));
        level.set(BlockPos::new(2, 64, 0), BlockState(blocks::STONE));
        rod(&mut level, BlockPos::new(6, 63, 0));
        assert_eq!(
            level.lightning_rod_near(center, LIGHTNING_ROD_RADIUS),
            Some(BlockPos::new(6, 64, 0))
        );
    }
}
