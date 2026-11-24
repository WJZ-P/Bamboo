//! 给方块 / 实体逻辑用的 POI 只读查询。
//!
//! 这一层的接口和官方 `PoiManager` 的查询族对齐，调用方只传类型 ID 和占用口径，
//! 不需要碰 `PoiStore` 的内部形状。以后 `lightning`（避雷针）和 `bees`（蜂巢）
//! 两个分支从「按区块段调色板扫描」换成这里的查询即可，见 `docs/villager-plan.md`。
//!
//! `mc_bamboo_blocks::Level` / `entity::ctx::BlockView` 上的默认实现给空答案，
//! 精确驱动（`RegionLevel`）和并行驱动（`ReadOnlyWorld`）都转发到本岛的 `PoiStore`，
//! 两种驱动不会分叉。

use mc_bamboo_core::BlockPos;
use mc_bamboo_world::poi::Occupancy;

/// `PoiManager.Occupancy` 的 0/1/2 编码；坏值（客户端可控）返回 `None`。
pub(crate) fn occupancy(code: u8) -> Option<Occupancy> {
    match code {
        0 => Some(Occupancy::HasSpace),
        1 => Some(Occupancy::IsOccupied),
        2 => Some(Occupancy::Any),
        _ => None,
    }
}

/// `ServerLevel.findLightningRod`：半径 128、类型 `lightning_rod`、`ANY`，
/// 只认高度图 `WORLD_SURFACE - 1` 上的那一格，结果返回上方一格（官方的 `map(above())`）。
///
/// `position_filter` 由调用方按高度图给出；这里只负责把 POI 查询的口径钉死。
pub fn find_lightning_rod_near(
    poi: &mc_bamboo_world::poi::PoiStore,
    center: BlockPos,
    position_filter: impl Fn(BlockPos) -> bool,
) -> Option<BlockPos> {
    let kind = mc_bamboo_data::poi::poi_type_id("lightning_rod")?;
    poi.find_closest(
        |found| found == kind,
        position_filter,
        center,
        128,
        Occupancy::Any,
    )
    .map(|pos| pos.offset(0, 1, 0))
}

/// 蜂巢 / 蜂箱查询：`beehive` 与 `bee_nest` 是两种 POI 类型，这里一起查，半径
/// 与占用口径由调用方给；返回按官方遍历序排好的全部位置。
pub fn bee_homes_in_range(
    poi: &mc_bamboo_world::poi::PoiStore,
    center: BlockPos,
    radius: i32,
) -> Vec<(u8, BlockPos)> {
    let beehive = mc_bamboo_data::poi::poi_type_id("beehive");
    let bee_nest = mc_bamboo_data::poi::poi_type_id("bee_nest");
    let is_bee_home = |kind: u8| beehive == Some(kind) || bee_nest == Some(kind);
    poi.get_in_range(is_bee_home, center, radius, Occupancy::Any)
        .into_iter()
        .map(|record| (record.kind, record.pos))
        .collect()
}
