//! `BlobFoliagePlacer`：橡树的球状树冠。
//!
//! 官方 `FoliagePlacer.createFoliage` 先采样 `offset`，再逐层算半径、逐行放树叶；
//! `BlobFoliagePlacer.shouldSkipLocation` 只在四个角上取一次 `nextInt(2)`（短路顺序
//! 决定了即使 `y == 0` 也会取数），这决定了随机数消耗次数，必须逐行照抄。
//!
//! Official `BlobFoliagePlacer`: `createFoliage` samples `offset`, then places leaves
//! row by row. `shouldSkipLocation` consumes one `nextInt(2)` only at the four corners;
//! short-circuit order means it is consumed even when `y == 0`.

use serde_json::Value;

use crate::{feature::provider::IntProvider, random::WorldgenRandom};

use super::{FoliageAttachment, TreeConfig, TreePlace, try_place_leaf};

/// `blob_foliage_placer` 的 radius / offset / height。
///
/// The radius / offset / height of `blob_foliage_placer`.
pub struct BlobFoliagePlacer {
    radius: IntProvider,
    offset: IntProvider,
    height: i32,
}

impl BlobFoliagePlacer {
    /// 解析配置；类型不是 `minecraft:blob_foliage_placer` 返回 `None`。
    ///
    /// Parse the config; `None` for a different type.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        if object.get("type").and_then(Value::as_str) != Some("minecraft:blob_foliage_placer") {
            return None;
        }
        Some(Self {
            radius: IntProvider::parse(object.get("radius")?).ok()?,
            offset: IntProvider::parse(object.get("offset")?).ok()?,
            height: object.get("height")?.as_i64()? as i32,
        })
    }

    /// `BlobFoliagePlacer.foliageHeight`：固定返回 `height`，不取随机数。
    ///
    /// Official `foliageHeight`: returns `height`, consuming no random.
    pub fn foliage_height(&self) -> i32 {
        self.height
    }

    /// `FoliagePlacer.foliageRadius(random, trunkHeight)`。
    ///
    /// Official `FoliagePlacer.foliageRadius(random, trunkHeight)`.
    pub fn foliage_radius(&self, random: &WorldgenRandom) -> i32 {
        self.radius.sample(random)
    }

    /// 官方 9 参数的 `createFoliage`：`offset` 在这里采样，再逐层放。
    ///
    /// Official 9-argument `createFoliage`: `offset` is sampled here, then rows are
    /// placed from top to bottom.
    pub(crate) fn create_foliage(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        attachment: &FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
    ) {
        let offset = self.offset.sample(random);
        let foliage_height_with_offset = foliage_height + attachment.foliage_height_offset;
        let mut yo = offset;
        while yo >= offset - foliage_height_with_offset {
            // Java 的 `yo / 2` 向零截断；Rust 的整数除法一样。
            // Java's `yo / 2` truncates toward zero, same as Rust integer division.
            let current_radius = (leaf_radius + attachment.radius_offset_xz - 1 - yo / 2).max(0);
            self.place_leaves_row(place, config, random, attachment, current_radius, yo);
            yo -= 1;
        }
    }

    /// `FoliagePlacer.placeLeavesRow` + `shouldSkipLocationSigned`。
    ///
    /// Official `FoliagePlacer.placeLeavesRow` + `shouldSkipLocationSigned`.
    fn place_leaves_row(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        attachment: &FoliageAttachment,
        radius: i32,
        y: i32,
    ) {
        let origin = attachment.pos;
        let double_trunk = attachment.double_trunk;
        let offset = if double_trunk { 1 } else { 0 };
        for dx in -radius..=radius + offset {
            for dz in -radius..=radius + offset {
                let (min_dx, min_dz) = if double_trunk {
                    (
                        (dx.abs()).min((dx - 1).abs()),
                        (dz.abs()).min((dz - 1).abs()),
                    )
                } else {
                    (dx.abs(), dz.abs())
                };
                // 角上才取 `nextInt(2)`；`||` 左操作数先求值，所以 y==0 时也取数。
                // Only corners consume `nextInt(2)`; the left operand is evaluated
                // first, so it is consumed even when y==0.
                let skip = if min_dx == radius && min_dz == radius {
                    let roll = random.next_int(2) == 0;
                    roll || y == 0
                } else {
                    false
                };
                if !skip {
                    let pos = (origin.0 + dx, origin.1 + y, origin.2 + dz);
                    try_place_leaf(place, config, random, pos);
                }
            }
        }
    }
}
