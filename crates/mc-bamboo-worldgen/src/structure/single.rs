//! 四个「代码生成」单件结构的结构件布局：沙漠神殿、丛林神殿、女巫小屋、埋藏的宝藏。
//!
//! 官方这些结构的 `findGenerationPoint` 用 `onTopOfChunkCenter` 取一个起点（terrain 高度，
//! 属里程碑 1 的 `structure-validity` 对拍范围），但结构件本身（`generatePieces`）只由
//! `GenerationContext.random()`（`makeRandom(seed, chunkPos)`）、区块最小方块坐标和常量
//! 尺寸决定：起点高度不参与包围盒计算。因此这里把「结构件列表」当纯函数复刻，
//! 输入 `(kind, seed, cx, cz)`，输出 `Vec<Piece>`，和 `cb-wg-multipiece` 的
//! `structure::pieces` 共用同一套 `Piece` / `Box` / `HDir` 模型与 `canonical_line` 格式。
//!
//! 三个 `ScatteredFeaturePiece` 子类先消耗一次 `getRandomHorizontalDirection(random)`
//! （`Direction.Plane.HORIZONTAL.faces[nextInt(4)]`），再由 `makeBoundingBox` 按方向
//! 决定是否交换 width/depth；`setOrientation` 同时定下 rotation / mirror。
//! `BuriedTreasurePiece` 不消耗随机数，包围盒是固定高度 90 的单点。
//! `postProcess`（把方块写进世界）属里程碑 3，不在这里。
//!
//! Layout of the four code-built single-piece structures: desert pyramid, jungle temple,
//! swamp hut and buried treasure. Vanilla's `findGenerationPoint` obtains a start via
//! `onTopOfChunkCenter` (a terrain height, covered by the milestone-1 `structure-validity`
//! oracle), but the pieces themselves (`generatePieces`) depend only on
//! `GenerationContext.random()` (`makeRandom(seed, chunkPos)`), the chunk's min block coords
//! and constant dimensions: the start height never enters the bounding box. So the piece list
//! is a pure function of `(kind, seed, cx, cz)` returning `Vec<Piece>`, reusing the same
//! `Piece` / `Box` / `HDir` model and `canonical_line` format as `cb-wg-multipiece`.
//! The three `ScatteredFeaturePiece` subclasses first draw one
//! `getRandomHorizontalDirection(random)` (`faces[nextInt(4)]`), then `makeBoundingBox`
//! swaps width/depth by direction; `setOrientation` fixes rotation / mirror.
//! `BuriedTreasurePiece` draws no randomness and has a fixed single-point box at Y=90.
//! `postProcess` (writing blocks into the world) is milestone 3, not here.

use crate::structure::pieces::{Box, HDir, Piece, block_at, make_random, new_box};

/// `StructurePieceType` 的注册名：`setPieceId(..., "TeDP")` 等把短名小写后注册进
/// `BuiltInRegistries.STRUCTURE_PIECE`，`createTag` 写进 NBT 的 `id`。
///
/// `StructurePieceType` registry names: `setPieceId(..., "TeDP")` lowercases the short id
/// before registering; `createTag` writes it as the NBT `id`.
const ID_DESERT: &str = "minecraft:tedp";
const ID_JUNGLE: &str = "minecraft:tejp";
const ID_SWAMP: &str = "minecraft:tesh";
const ID_BURIED: &str = "minecraft:btp";

/// `ScatteredFeaturePiece` 子类构造器的固定基准 Y。
///
/// Fixed base Y of the `ScatteredFeaturePiece` subclass constructors.
const FLOOR_Y: i32 = 64;
/// `BuriedTreasureStructure.generatePieces` 里 `new BlockPos(..., 90, ...)` 的 Y。
///
/// The Y of `new BlockPos(..., 90, ...)` in `BuriedTreasureStructure.generatePieces`.
const BURIED_Y: i32 = 90;

/// `StructurePiece.makeBoundingBox(x, y, z, direction, width, height, depth)`。
/// 方向轴为 Z（NORTH/SOUTH）时用 width 撑 X、depth 撑 Z；否则交换。
///
/// `StructurePiece.makeBoundingBox`: a Z-axis direction (NORTH/SOUTH) spans X by width and
/// Z by depth; an X-axis direction swaps them.
fn bounding_box(x: i32, y: i32, z: i32, dir: HDir, width: i32, height: i32, depth: i32) -> Box {
    if dir.axis_z() {
        new_box(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
    } else {
        new_box(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
    }
}

/// `ScatteredFeaturePiece.addAdditionalSaveData` 写的四个键；`heightPosition` 初值 -1。
///
/// The four keys written by `ScatteredFeaturePiece.addAdditionalSaveData`; `heightPosition`
/// starts at -1.
fn scattered_extras(piece: &mut Piece, width: i32, height: i32, depth: i32) {
    piece.put_int("Width", width);
    piece.put_int("Height", height);
    piece.put_int("Depth", depth);
    piece.put_int("HPos", -1);
}

/// 一个 `ScatteredFeaturePiece` 子类的常量：注册名、尺寸、子类的布尔键。
///
/// Constants of one `ScatteredFeaturePiece` subclass: registry id, dimensions, boolean keys.
struct ScatteredSpec {
    id: &'static str,
    width: i32,
    height: i32,
    depth: i32,
    false_flags: &'static [&'static str],
}

/// 三个 `ScatteredFeaturePiece` 子类的公共编排：`makeRandom` → 一次方向取数 →
/// `makeBoundingBox` → `setOrientation` → 子类的布尔 `addAdditionalSaveData` 键。
///
/// Shared orchestration of the three `ScatteredFeaturePiece` subclasses: `makeRandom` → one
/// direction draw → `makeBoundingBox` → `setOrientation` → the subclass boolean save keys.
fn scattered(seed: i64, cx: i32, cz: i32, spec: &ScatteredSpec) -> Vec<Piece> {
    let random = make_random(seed, cx, cz);
    let dir = HDir::random(&random);
    let bb = bounding_box(
        block_at(cx, 0),
        FLOOR_Y,
        block_at(cz, 0),
        dir,
        spec.width,
        spec.height,
        spec.depth,
    );
    let mut piece = Piece::oriented(spec.id, bb, 0, dir);
    scattered_extras(&mut piece, spec.width, spec.height, spec.depth);
    for flag in spec.false_flags {
        piece.put_bool(flag, false);
    }
    vec![piece]
}

/// `DesertPyramidPiece(random, west, north)`：`21×15×21`，四个 `hasPlacedChestN`。
///
/// `DesertPyramidPiece(random, west, north)`: `21×15×21`, four `hasPlacedChestN` flags.
const DESERT: ScatteredSpec = ScatteredSpec {
    id: ID_DESERT,
    width: 21,
    height: 15,
    depth: 21,
    false_flags: &[
        "hasPlacedChest0",
        "hasPlacedChest1",
        "hasPlacedChest2",
        "hasPlacedChest3",
    ],
};

pub fn desert_pyramid(seed: i64, cx: i32, cz: i32) -> Vec<Piece> {
    scattered(seed, cx, cz, &DESERT)
}

/// `JungleTemplePiece(random, west, north)`：`12×10×15`，四个陷阱 / 箱子布尔键。
///
/// `JungleTemplePiece(random, west, north)`: `12×10×15`, four trap / chest boolean keys.
const JUNGLE: ScatteredSpec = ScatteredSpec {
    id: ID_JUNGLE,
    width: 12,
    height: 10,
    depth: 15,
    false_flags: &[
        "placedMainChest",
        "placedHiddenChest",
        "placedTrap1",
        "placedTrap2",
    ],
};

pub fn jungle_temple(seed: i64, cx: i32, cz: i32) -> Vec<Piece> {
    scattered(seed, cx, cz, &JUNGLE)
}

/// `SwampHutPiece(random, west, north)`：`7×7×9`，`Witch` / `Cat` 两个布尔键。
///
/// `SwampHutPiece(random, west, north)`: `7×7×9`, `Witch` / `Cat` boolean keys.
const SWAMP: ScatteredSpec = ScatteredSpec {
    id: ID_SWAMP,
    width: 7,
    height: 7,
    depth: 9,
    false_flags: &["Witch", "Cat"],
};

pub fn swamp_hut(seed: i64, cx: i32, cz: i32) -> Vec<Piece> {
    scattered(seed, cx, cz, &SWAMP)
}

/// `BuriedTreasurePieces.BuriedTreasurePiece(BlockPos(getBlockX(9), 90, getBlockZ(9)))`：
/// 不取随机数、无朝向、无子类参数，包围盒是单个点。
///
/// `BuriedTreasurePieces.BuriedTreasurePiece(BlockPos(getBlockX(9), 90, getBlockZ(9)))`:
/// no randomness, no orientation, no subclass params; the box is a single point.
pub fn buried_treasure(seed: i64, cx: i32, cz: i32) -> Vec<Piece> {
    let _ = seed;
    let x = block_at(cx, 9);
    let z = block_at(cz, 9);
    vec![Piece::plain(
        ID_BURIED,
        new_box(x, BURIED_Y, z, x, BURIED_Y, z),
        0,
    )]
}

/// 按 kind 分发的纯函数入口；未知 kind 报错。
///
/// Pure entry point dispatching by kind; unknown kinds are an error.
pub fn generate(kind: &str, seed: i64, cx: i32, cz: i32) -> Result<Vec<Piece>, String> {
    match kind {
        "desert_pyramid" => Ok(desert_pyramid(seed, cx, cz)),
        "jungle_temple" => Ok(jungle_temple(seed, cx, cz)),
        "swamp_hut" => Ok(swamp_hut(seed, cx, cz)),
        "buried_treasure" => Ok(buried_treasure(seed, cx, cz)),
        other => Err(format!("no code-built single-piece layout for {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scattered_pieces_are_deterministic_and_single() {
        for seed in [0, 42, -7] {
            for kind in [
                "desert_pyramid",
                "jungle_temple",
                "swamp_hut",
                "buried_treasure",
            ] {
                let a = generate(kind, seed, 3, -5).unwrap();
                let b = generate(kind, seed, 3, -5).unwrap();
                assert_eq!(a, b, "{kind}");
                assert_eq!(a.len(), 1, "{kind}");
            }
        }
    }

    #[test]
    fn buried_treasure_is_a_fixed_single_point() {
        let p = buried_treasure(0, 7, -9).pop().unwrap();
        let x = block_at(7, 9);
        let z = block_at(-9, 9);
        assert_eq!(p.bb, [x, BURIED_Y, z, x, BURIED_Y, z]);
        assert_eq!(p.o, -1);
        assert_eq!(p.rot, -1);
        assert_eq!(p.mirror, -1);
        assert!(p.extras.is_empty());
    }
}
