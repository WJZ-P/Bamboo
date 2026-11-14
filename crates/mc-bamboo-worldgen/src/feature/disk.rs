//! `DiskFeature`：在 `OCEAN_FLOOR_WG` 高度附近铺一层圆盘（沙/粘土/砾石/草/浮冰）。
//!
//! `DiskFeature.place` 先取一次 `radius.sample(random)`，再按 `BlockPos.betweenClosed`
//! 的顺序遍历以原点为中心的 (2r+1)² 列；落在 `xd²+zd² <= r²` 的列交给 `placeColumn`，
//! 从 `top` 往下到 `bottom+1` 逐格：`target` 命中就用 `state_provider` 取状态写进去。
//!
//! DiskFeature lays a disk near the ocean-floor height. It draws one `radius.sample(random)`,
//! walks the (2r+1)² columns in `BlockPos.betweenClosed` order, and for each column inside
//! `xd²+zd² <= r²` scans y from `top` down to `bottom+1`, writing blocks whose `target`
//! predicate matches via the `state_provider`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate, provider::IntProvider, state_provider::StateProvider,
    world::FeatureWorld,
};

/// `DiskFeature` 的配置。
///
/// Configuration of `DiskFeature`.
#[derive(Debug, Clone)]
pub struct DiskFeature {
    pub radius: IntProvider,
    pub half_height: i32,
    pub target: BlockPredicate,
    pub state_provider: StateProvider,
}

impl DiskFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config.as_object().ok_or("disk feature must be an object")?;
        Ok(Self {
            radius: IntProvider::parse(object.get("radius").ok_or("disk has no radius")?)?,
            half_height: object
                .get("half_height")
                .and_then(Value::as_i64)
                .ok_or("disk has no half_height")? as i32,
            target: BlockPredicate::parse(object.get("target").ok_or("disk has no target")?),
            state_provider: StateProvider::parse(
                object
                    .get("state_provider")
                    .ok_or("disk has no state_provider")?,
            )?,
        })
    }

    /// 配置里的谓词/状态提供者是否都移植了。
    ///
    /// Whether every predicate and state provider in the config is ported.
    pub fn supported(&self) -> bool {
        self.target.is_supported() && self.state_provider.supported()
    }
}

/// `DiskFeature.place`。返回是否至少写了一格。
///
/// `DiskFeature.place`. Returns whether at least one block was written.
pub fn place(
    disk: &DiskFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    // 只在这里消费一次随机数（uniform IntProvider）。
    // The only random draw here (a uniform IntProvider).
    let r = disk.radius.sample(random);
    let top = oy + disk.half_height;
    let bottom = oy - disk.half_height - 1;
    let mut placed_any = false;
    // betweenClosed(origin.offset(-r,0,-r), origin.offset(r,0,r))：x 变化最快，z 最慢。
    // betweenClosed(...): x varies fastest, z slowest (y is fixed at origin y).
    for dz in -r..=r {
        for dx in -r..=r {
            if dx * dx + dz * dz > r * r {
                continue;
            }
            let (x, z) = (ox + dx, oz + dz);
            // placeColumn：从 top 往下到 bottom+1。
            // placeColumn: from top down to bottom+1.
            let mut y = top;
            while y > bottom {
                if disk.target.test(world, (x, y, z))
                    && let Some(state) =
                        disk.state_provider
                            .get_optional_state(world, random, (x, y, z))
                {
                    // setBlock(pos, state, 2)：走 ChunkAccess.setBlockState，要更新高度图。
                    // setBlock(pos, state, 2): goes through ChunkAccess.setBlockState,
                    // so the heightmaps must be updated.
                    world.set_block_with_heightmaps(x, y, z, state);
                    placed_any = true;
                }
                y -= 1;
            }
        }
    }
    placed_any
}
