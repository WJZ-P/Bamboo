//! 沉船（`ShipwreckPieces`）的结构件布局。
//!
//! 官方 `ShipwreckStructure.generatePieces`：`Rotation.getRandom(random)` 一次，
//! `ShipwreckPieces.addRandomPiece` 从「沙滩」或「海里」两张模板表里 `Util.getRandom`
//! （`nextInt(len)`）挑一个，起点是 `(minBlockX, 90, minBlockZ)`。结构件只有一件，
//! rotation pivot 固定 `(4, 0, 15)`，`makeSettings` 用 `Mirror.NONE`。
//! 模板尺寸大于世界生成区（32）时才在 `generatePieces` 里按高度图调整 Y，这一步要用
//! terrain 高度，属放置里程碑；这里的包围盒是「还没调整高度」的版本。
//! 写方块（`postProcess` 的高度校正、箱子战利品表）同样属放置里程碑。
//!
//! Layout of the shipwreck piece. Vanilla `ShipwreckStructure.generatePieces` draws
//! `Rotation.getRandom(random)` once and `ShipwreckPieces.addRandomPiece` picks a template
//! from the beached or ocean table via `Util.getRandom` (`nextInt(len)`), with the start at
//! `(minBlockX, 90, minBlockZ)`. There is a single piece with rotation pivot `(4, 0, 15)`
//! and `Mirror.NONE`. Only templates larger than the world-gen region (32) get their Y
//! adjusted from a heightmap inside `generatePieces`, which needs terrain heights and
//! belongs to the placement milestone; the box here is the unadjusted one. Writing blocks
//! (`postProcess` height adjustment, chest loot tables) is placement-milestone work too.

use crate::structure::{
    pieces::{Piece, make_random},
    template::StructureTemplate,
    templated::{MIRROR_NONE, rotation_name, template_bounds},
};

/// `ShipwreckPieces.PIVOT` 的 X/Z。
///
/// The X/Z of `ShipwreckPieces.PIVOT`.
const PIVOT: (i32, i32) = (4, 15);
/// 和官方 `generatePieces` 一样，Y 从 90 起（`postProcess` 再按高度学校正）。
///
/// Start Y is 90 as in vanilla `generatePieces` (corrected later by `postProcess`).
const START_Y: i32 = 90;

/// `STRUCTURE_LOCATION_BEACHED`，顺序和官方常量一致。
///
/// `STRUCTURE_LOCATION_BEACHED` in vanilla constant order.
const BEACHED: [&str; 11] = [
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];

/// `STRUCTURE_LOCATION_OCEAN`，顺序和官方常量一致（20 项）。
///
/// `STRUCTURE_LOCATION_OCEAN` in vanilla constant order (20 entries).
const OCEAN: [&str; 20] = [
    "minecraft:shipwreck/with_mast",
    "minecraft:shipwreck/upsidedown_full",
    "minecraft:shipwreck/upsidedown_fronthalf",
    "minecraft:shipwreck/upsidedown_backhalf",
    "minecraft:shipwreck/sideways_full",
    "minecraft:shipwreck/sideways_fronthalf",
    "minecraft:shipwreck/sideways_backhalf",
    "minecraft:shipwreck/rightsideup_full",
    "minecraft:shipwreck/rightsideup_fronthalf",
    "minecraft:shipwreck/rightsideup_backhalf",
    "minecraft:shipwreck/with_mast_degraded",
    "minecraft:shipwreck/upsidedown_full_degraded",
    "minecraft:shipwreck/upsidedown_fronthalf_degraded",
    "minecraft:shipwreck/upsidedown_backhalf_degraded",
    "minecraft:shipwreck/sideways_full_degraded",
    "minecraft:shipwreck/sideways_fronthalf_degraded",
    "minecraft:shipwreck/sideways_backhalf_degraded",
    "minecraft:shipwreck/rightsideup_full_degraded",
    "minecraft:shipwreck/rightsideup_fronthalf_degraded",
    "minecraft:shipwreck/rightsideup_backhalf_degraded",
];

/// `ShipwreckStructure.findGenerationPoint` → `generatePieces` 的结构件列表。
///
/// 官方在 `generatePieces` 里只在模板尺寸超过 32 时调整 Y（要 heightmap）；这里返回
/// 「未调整」的包围盒，并用 `height_adjusted = false` 标识，由放置里程碑补齐。
///
/// The piece list of `ShipwreckStructure.findGenerationPoint` → `generatePieces`.
/// Vanilla only adjusts Y inside `generatePieces` when the template exceeds 32 blocks
/// (needs a heightmap); we return the unadjusted box, marked `height_adjusted = false`
/// so the placement milestone can fill it in.
pub fn generate(seed: i64, cx: i32, cz: i32, is_beached: bool) -> Result<Vec<Piece>, String> {
    let random = make_random(seed, cx, cz);
    let rotation = random.next_int(4);
    let table: &[&str] = if is_beached { &BEACHED } else { &OCEAN };
    let template_name = table[random.next_int(table.len() as i32) as usize];
    let template = StructureTemplate::load(template_name)?;
    let position = (cx * 16, START_Y, cz * 16);
    let bb = template_bounds(template.size, rotation, MIRROR_NONE, PIVOT, position);
    let mut piece = Piece::plain("minecraft:shipwreck", bb, 0);
    piece.o = 2;
    piece.rot = rotation;
    piece.mirror = MIRROR_NONE;
    piece.put_str("Template", template_name);
    piece.put_int("TPX", position.0);
    piece.put_int("TPY", position.1);
    piece.put_int("TPZ", position.2);
    piece.put_bool("isBeached", is_beached);
    piece.put_str("Rot", rotation_name(rotation));
    // `heightAdjusted` 在构造器里是 false，只有 `generatePieces` 调
    // `adjustPositionHeight` 才会变 true，而那是 `isTooBigToFitInWorldGenRegion()` 的分支。
    // 现有 31 个沉船模板最大 9×21×28，都放得进世界生成区，所以这里恒 false、Y 恒 90。
    // `heightAdjusted` starts false and only becomes true through `adjustPositionHeight`
    // in `generatePieces`, which sits behind `isTooBigToFitInWorldGenRegion()`. All 31
    // shipped shipwreck templates are at most 9x21x28 and fit the world-gen region, so it
    // stays false and Y stays 90.
    if too_big_for_world_gen(template.size) {
        return Err(format!(
            "{template_name} is too big for the world-gen region; its height adjustment \
             needs terrain heights and is not ported yet"
        ));
    }
    piece.put_bool("height_adjusted", false);
    Ok(vec![piece])
}

/// `ShipwreckPiece.isTooBigToFitInWorldGenRegion()`：模板 X 或 Y 超过 32。
///
/// `ShipwreckPiece.isTooBigToFitInWorldGenRegion()`: template X or Y above 32.
fn too_big_for_world_gen(size: (i32, i32, i32)) -> bool {
    size.0 > 32 || size.1 > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两次调用结果一致，且模板名来自对应的那张表。
    ///
    /// Two calls agree, and the template comes from the matching table.
    #[test]
    fn templates_come_from_the_right_table() {
        for seed in 0..24 {
            let ocean = generate(seed, 5, -3, false).unwrap();
            let beached = generate(seed, 5, -3, true).unwrap();
            assert_eq!(ocean, generate(seed, 5, -3, false).unwrap());
            let ocean_name = ocean[0].extras["Template"].as_str().unwrap();
            let beached_name = beached[0].extras["Template"].as_str().unwrap();
            assert!(OCEAN.contains(&ocean_name), "{ocean_name}");
            assert!(BEACHED.contains(&beached_name), "{beached_name}");
        }
    }
}
