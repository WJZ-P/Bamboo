//! `BlockBlobFeature`（森林岩石）：从候选位置往下找到可落脚的方块，连放三层苔石球。
//!
//! 照 `BlockBlobFeature.place`：`can_place_on` 命中下方方块后停下，取三次
//! `(2r+1)³` 盒子，按 `BlockPos.betweenClosed` 顺序把 `distSqr <= tr²` 的格子写成 `state`，
//! 每层结束把原点往随机方向挪一格。
//!
//! `BlockBlobFeature` (forest rock): walk down until `can_place_on` matches the block below,
//! then lay three `(2r+1)³` blobs of `state`, writing cells with `distSqr <= tr²` in
//! `BlockPos.betweenClosed` order and nudging the origin by one block after each blob.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{block_predicate::BlockPredicate, state::parse_state, world::FeatureWorld};

/// `BlockBlobFeature` 的配置。
///
/// Configuration of `BlockBlobFeature`.
#[derive(Debug, Clone)]
pub struct BlockBlobFeature {
    pub state: u32,
    pub can_place_on: BlockPredicate,
}

impl BlockBlobFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("block_blob feature must be an object")?;
        Ok(Self {
            state: parse_state(object.get("state").ok_or("block_blob has no state")?)?,
            can_place_on: BlockPredicate::parse(
                object
                    .get("can_place_on")
                    .ok_or("block_blob has no can_place_on")?,
            ),
        })
    }

    /// 嵌套谓词是否都已移植。
    ///
    /// Whether every nested predicate is ported.
    pub fn supported(&self) -> bool {
        self.can_place_on.is_supported()
    }
}

/// `BlockBlobFeature.place`。返回是否放置过（官方固定返回 true，只要落脚点有效）。
///
/// `BlockBlobFeature.place`. Returns whether placement ran (official returns true whenever
/// a valid footing exists).
pub fn place(
    blob: &BlockBlobFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (mut ox, mut oy, mut oz) = origin;
    // 往下找到落脚方块：只在这里读方块，不消费随机数。
    // Walk down to the footing block: reads only, no randomness consumed.
    while oy > world.min_y + 3 && !blob.can_place_on.test(world, (ox, oy - 1, oz)) {
        oy -= 1;
    }
    if oy <= world.min_y + 3 {
        return false;
    }

    for _ in 0..3 {
        let xr = random.next_int(2);
        let yr = random.next_int(2);
        let zr = random.next_int(2);
        let tr: f32 = (xr + yr + zr) as f32 * 0.333_f32 + 0.5_f32;
        let tr_sq = (tr * tr) as f64;
        // betweenClosed：x 变化最快，其次 y，z 最慢。
        // betweenClosed: x varies fastest, then y, z slowest.
        for dz in -zr..=zr {
            for dy in -yr..=yr {
                for dx in -xr..=xr {
                    let d2 = (dx * dx + dy * dy + dz * dz) as f64;
                    if d2 <= tr_sq {
                        // setBlockAndUpdate → ChunkAccess.setBlockState，要更新高度图。
                        // setBlockAndUpdate → ChunkAccess.setBlockState, so update heightmaps.
                        world.set_block_with_heightmaps(ox + dx, oy + dy, oz + dz, blob.state);
                    }
                }
            }
        }
        // 官方 `origin.offset(-1 + nextInt(2), -nextInt(2), -1 + nextInt(2))`。
        // Official `origin.offset(-1 + nextInt(2), -nextInt(2), -1 + nextInt(2))`.
        ox += -1 + random.next_int(2);
        oy -= random.next_int(2);
        oz += -1 + random.next_int(2);
    }
    true
}
