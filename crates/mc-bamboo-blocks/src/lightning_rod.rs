//! 雷击落点里的避雷针查找，官方 `ServerLevel.findLightningRod`。
//!
//! 官方查 `PoiManager` 里 `PoiTypes.LIGHTNING_ROD` 的记录；Bamboo 还没有 POI 系统，
//! 这里按方块状态和 `WORLD_SURFACE` 高度图直接判断，语义和 POI 记录一致。以后村民那期
//! 做成正式 POI 查询后，把 [`Level::lightning_rod_near`] 改成查索引即可。

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::tags::block_tags;

use crate::{Level, state::BlockState};

/// `findLightningRod` 的搜索半径，官方传入 128。
pub const LIGHTNING_ROD_RADIUS: i32 = 128;
/// 官方 `PoiManager.sectionsToVillage` 用的段边长。
const SECTION_SIZE: i32 = 16;

/// 这一格是不是 POI 里登记的那种避雷针（含涂蜡八种状态）。
pub fn is_lightning_rod(state: BlockState) -> bool {
    state.is_tagged(block_tags::LIGHTNING_RODS)
}

/// 默认实现：按官方 POI 流的顺序逐格扫，返回距离最近的避雷针上方一格。
///
/// 遍历顺序是 `ChunkPos.rangeClosed(中心区块, floorDiv(radius,16)+1)`：x 外层、z 内层，
/// 每个区块按 section y 从低到高，段内按 (y,z,x) 扫描。`Stream.min` 保留并列时的第一个。
/// 官方同一段内多根避雷针的顺序由 `HashMap` 决定、不可复现，这里取确定顺序。
pub fn nearest_lightning_rod(level: &dyn Level, center: BlockPos, radius: i32) -> Option<BlockPos> {
    fn scan(level: &dyn Level, center: BlockPos, radius: i32) -> Option<BlockPos> {
        scan_impl(
            center,
            radius,
            |chunk| level.chunk_is_loaded(chunk),
            |pos| level.block_state(pos),
            |pos| level.world_surface_y(pos),
            level.min_y(),
            level.max_y(),
        )
    }
    scan(level, center, radius)
}

/// `Level` 默认实现用的入口：trait 对象不能拿 `&Self`，这里把每个查询拆成闭包。
pub fn nearest_lightning_rod_from(
    level: &(impl Level + ?Sized),
    center: BlockPos,
    radius: i32,
) -> Option<BlockPos> {
    scan_impl(
        center,
        radius,
        |chunk| level.chunk_is_loaded(chunk),
        |pos| level.block_state(pos),
        |pos| level.world_surface_y(pos),
        level.min_y(),
        level.max_y(),
    )
}

fn scan_impl(
    center: BlockPos,
    radius: i32,
    loaded: impl Fn(ChunkPos) -> bool,
    block: impl Fn(BlockPos) -> BlockState,
    surface: impl Fn(BlockPos) -> i32,
    min_y: i32,
    max_y: i32,
) -> Option<BlockPos> {
    if radius < 0 {
        return None;
    }
    let centre_chunk = ChunkPos::new(center.x >> 4, center.z >> 4);
    let chunks = radius.div_euclid(SECTION_SIZE) + 1;
    let radius_sq = i64::from(radius) * i64::from(radius);
    let mut best: Option<(i64, BlockPos)> = None;
    for chunk_x in (centre_chunk.x - chunks)..=(centre_chunk.x + chunks) {
        for chunk_z in (centre_chunk.z - chunks)..=(centre_chunk.z + chunks) {
            if !loaded(ChunkPos::new(chunk_x, chunk_z)) {
                continue;
            }
            let base_x = chunk_x * SECTION_SIZE;
            let base_z = chunk_z * SECTION_SIZE;
            for section_y in (min_y >> 4)..=(max_y >> 4) {
                let base_y = section_y * SECTION_SIZE;
                for y in 0..SECTION_SIZE {
                    for z in 0..SECTION_SIZE {
                        for x in 0..SECTION_SIZE {
                            let pos = BlockPos::new(base_x + x, base_y + y, base_z + z);
                            let dx = i64::from(pos.x - center.x);
                            let dz = i64::from(pos.z - center.z);
                            if dx.abs() > i64::from(radius) || dz.abs() > i64::from(radius) {
                                continue;
                            }
                            let dy = i64::from(pos.y - center.y);
                            let distance = dx * dx + dy * dy + dz * dz;
                            if distance > radius_sq {
                                continue;
                            }
                            if best.is_some_and(|(old, _)| distance >= old) {
                                continue;
                            }
                            if !is_lightning_rod(block(pos)) {
                                continue;
                            }
                            // `PoiManager.findClosest` 的第二个过滤：`WORLD_SURFACE` 高度 - 1。
                            if pos.y != surface(pos) - 1 {
                                continue;
                            }
                            best = Some((distance, pos.above()));
                        }
                    }
                }
            }
        }
    }
    best.map(|(_, pos)| pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestLevel;
    use mc_bamboo_data::blocks;

    #[test]
    fn nearest_tie_follows_poi_stream_order() {
        // 中心和两根等距避雷针，断言取 POI 流顺序（低 x 优先）里的第一根。
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        level.set(BlockPos::new(-3, 63, 0), BlockState(blocks::LIGHTNING_ROD));
        level.set(BlockPos::new(3, 63, 0), BlockState(blocks::LIGHTNING_ROD));
        // 两根都在 63 层的空气之上，`WORLD_SURFACE` 是 64。
        assert_eq!(
            nearest_lightning_rod(&level, center, LIGHTNING_ROD_RADIUS),
            Some(BlockPos::new(-3, 64, 0))
        );
    }

    #[test]
    fn buried_rod_is_not_a_target() {
        let mut level = TestLevel::new();
        let center = BlockPos::new(0, 64, 0);
        level.set(BlockPos::new(2, 63, 0), BlockState(blocks::LIGHTNING_ROD));
        level.set(BlockPos::new(2, 64, 0), BlockState(blocks::STONE));
        assert_eq!(
            nearest_lightning_rod(&level, center, LIGHTNING_ROD_RADIUS),
            None
        );
    }
}
