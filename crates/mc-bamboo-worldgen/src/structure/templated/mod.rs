//! 需要 `.nbt` 模板的单件结构（雪屋、沉船、海底遗迹、废弃传送门）的结构件布局。
//!
//! 这些结构的结构件都继承官方 `TemplateStructurePiece`：包围盒由模板尺寸、
//! rotation、mirror 和 rotation pivot 算出（`StructureTemplate.getBoundingBox`），
//! `addAdditionalSaveData` 写 `Template` / `TPX` / `TPY` / `TPZ` 和各自的参数。
//! 这里只复刻「结构件列表」这一层——模板尺寸从导出的 `.nbt` 读，写方块（`postProcess`）
//! 属放置里程碑。每个结构一个子模块，入口都是 `generate(seed, cx, cz, ...)` 纯函数。
//!
//! Layout generation for the single-piece structures that need `.nbt` templates
//! (igloo, shipwreck, ocean ruin, ruined portal). Every piece extends vanilla
//! `TemplateStructurePiece`: the bounding box comes from the template size, rotation,
//! mirror and rotation pivot (`StructureTemplate.getBoundingBox`), and
//! `addAdditionalSaveData` writes `Template` / `TPX` / `TPY` / `TPZ` plus per-piece
//! parameters. Only the piece list is reproduced here; template sizes are read from the
//! exported `.nbt`, and block placement (`postProcess`) belongs to the placement
//! milestone. Each structure is one submodule exposing a pure `generate(seed, cx, cz, ..)`.

pub mod igloo;
pub mod ocean_ruin;
pub mod ruined_portal;
pub mod shipwreck;

use crate::structure::pieces::{Box, new_box};

/// 官方 `Rotation` 的序号，和 `Rotation.values()` 顺序一致。
///
/// Ordinals of vanilla `Rotation`, matching the `Rotation.values()` order.
pub const ROT_NONE: i32 = 0;
pub const ROT_CLOCKWISE_90: i32 = 1;
pub const ROT_CLOCKWISE_180: i32 = 2;
pub const ROT_COUNTERCLOCKWISE_90: i32 = 3;

/// 官方 `Mirror` 的序号：`NONE` / `LEFT_RIGHT` / `FRONT_BACK`。
///
/// Ordinals of vanilla `Mirror`: `NONE` / `LEFT_RIGHT` / `FRONT_BACK`.
pub const MIRROR_NONE: i32 = 0;

/// `Rotation.LEGACY_CODEC` 写进 NBT 的字符串：枚举常量名。
///
/// The string `Rotation.LEGACY_CODEC` writes to NBT: the enum constant name.
pub fn rotation_name(rotation: i32) -> &'static str {
    match rotation {
        ROT_CLOCKWISE_90 => "CLOCKWISE_90",
        ROT_CLOCKWISE_180 => "CLOCKWISE_180",
        ROT_COUNTERCLOCKWISE_90 => "COUNTERCLOCKWISE_90",
        _ => "NONE",
    }
}

/// `rotation_name` 的反向：从 NBT 字符串读回 `Rotation` 的序号。
///
/// The inverse of `rotation_name`: read the `Rotation` ordinal back from the NBT string.
pub fn rotation_ordinal(name: &str) -> Result<i32, String> {
    Ok(match name {
        "NONE" => ROT_NONE,
        "CLOCKWISE_90" => ROT_CLOCKWISE_90,
        "CLOCKWISE_180" => ROT_CLOCKWISE_180,
        "COUNTERCLOCKWISE_90" => ROT_COUNTERCLOCKWISE_90,
        other => return Err(format!("bad rotation {other}")),
    })
}

/// `StructureTemplate.transform(BlockPos, Mirror, Rotation, BlockPos)`（整数版）。
/// mirror 先把 z（`LEFT_RIGHT`）或 x（`FRONT_BACK`）取反，rotation 再绕 pivot 转。
///
/// Vanilla `StructureTemplate.transform(BlockPos, Mirror, Rotation, BlockPos)` (the
/// integer overload). The mirror negates z (`LEFT_RIGHT`) or x (`FRONT_BACK`) first,
/// then the rotation turns the result around the pivot.
pub fn transform(
    pos: (i32, i32, i32),
    mirror: i32,
    rotation: i32,
    pivot: (i32, i32),
) -> (i32, i32, i32) {
    let (mut x, y, mut z) = pos;
    let was_mirrored = match mirror {
        1 => {
            z = -z;
            true
        }
        2 => {
            x = -x;
            true
        }
        _ => false,
    };
    let (pivot_x, pivot_z) = pivot;
    match rotation {
        ROT_COUNTERCLOCKWISE_90 => (pivot_x - pivot_z + z, y, pivot_x + pivot_z - x),
        ROT_CLOCKWISE_90 => (pivot_x + pivot_z - z, y, pivot_z - pivot_x + x),
        ROT_CLOCKWISE_180 => (pivot_x + pivot_x - x, y, pivot_z + pivot_z - z),
        _ if was_mirrored => (x, y, z),
        _ => pos,
    }
}

/// `StructureTemplate.getBoundingBox(BlockPos, Rotation, pivot, Mirror, Vec3i)`：
/// 把 `(0,0,0)` 和 `size - 1` 两个角变换后取包围盒，再平移到 `position`。
///
/// Vanilla `StructureTemplate.getBoundingBox(BlockPos, Rotation, pivot, Mirror, Vec3i)`:
/// transform the two corners `(0,0,0)` and `size - 1`, take their bounding box, then
/// translate by `position`.
pub fn template_bounds(
    size: (i32, i32, i32),
    rotation: i32,
    mirror: i32,
    pivot: (i32, i32),
    position: (i32, i32, i32),
) -> Box {
    let corner1 = transform((0, 0, 0), mirror, rotation, pivot);
    let corner2 = transform(
        (size.0 - 1, size.1 - 1, size.2 - 1),
        mirror,
        rotation,
        pivot,
    );
    let (min_x, max_x) = (corner1.0.min(corner2.0), corner1.0.max(corner2.0));
    let (min_y, max_y) = (corner1.1.min(corner2.1), corner1.1.max(corner2.1));
    let (min_z, max_z) = (corner1.2.min(corner2.2), corner1.2.max(corner2.2));
    new_box(
        min_x + position.0,
        min_y + position.1,
        min_z + position.2,
        max_x + position.0,
        max_y + position.1,
        max_z + position.2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CLOCKWISE_90` 不带 mirror 时，模板的 X/Y/Z 尺寸在包围盒里交换 X/Z。
    ///
    /// Without a mirror, `CLOCKWISE_90` swaps the template X/Z sizes in the box.
    #[test]
    fn clockwise_90_swaps_x_and_z_spans() {
        let bb = template_bounds(
            (7, 5, 9),
            ROT_CLOCKWISE_90,
            MIRROR_NONE,
            (3, 4),
            (100, 60, -7),
        );
        assert_eq!(bb[3] - bb[0] + 1, 9);
        assert_eq!(bb[4] - bb[1] + 1, 5);
        assert_eq!(bb[5] - bb[2] + 1, 7);
        // 角点按 pivot 定位：corner1 = (pivotX+pivotZ, 0, pivotZ-pivotX) = (7, 0, 1)。
        // Corners follow the pivot: corner1 = (pivotX+pivotZ, 0, pivotZ-pivotX) = (7, 0, 1).
        assert_eq!((bb[0], bb[1], bb[2]), (100 + 7 - 8, 60, -7 + 1));
    }

    /// `NONE` 不带 mirror 时包围盒就是从 `position` 起的一个尺寸盒。
    ///
    /// Without a mirror, `NONE` yields the plain size box anchored at `position`.
    #[test]
    fn none_is_the_plain_size_box() {
        let bb = template_bounds((7, 5, 9), ROT_NONE, MIRROR_NONE, (3, 4), (100, 60, -7));
        assert_eq!(bb, [100, 60, -7, 106, 64, 1]);
    }
}
