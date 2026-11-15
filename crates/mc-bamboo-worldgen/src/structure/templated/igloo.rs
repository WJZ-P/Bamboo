//! 雪屋（`IglooPieces`）的结构件布局。
//!
//! 官方 `IglooStructure.generatePieces`：起点 `(minBlockX, 90, minBlockZ)`，先
//! `Rotation.getRandom(random)`（一次 `nextInt(4)`）定模板朝向，再进
//! `IglooPieces.addPieces`——`nextDouble() < 0.5` 时生成地下室（`nextInt(8) + 4` 层），
//! 最后总是放地表的 `igloo/top`。三个模板的 rotation pivot 和相对偏移是官方写死的常量表。
//! 包围盒由模板尺寸 + rotation + pivot 算出，朝向固定 `Direction.NORTH`（`O = 2`）。
//! 写方块（`postProcess` 的地表高度校正、箱子战利品表）属放置里程碑。
//!
//! Layout of the igloo pieces. Vanilla `IglooStructure.generatePieces` uses the start
//! `(minBlockX, 90, minBlockZ)`, draws `Rotation.getRandom(random)` (one `nextInt(4)`) for
//! the template rotation, then `IglooPieces.addPieces` adds an underground lab
//! (`nextDouble() < 0.5`, depth `nextInt(8) + 4`) and always the surface `igloo/top`.
//! The per-template rotation pivot and offset are the hard-coded vanilla tables. The
//! bounding box comes from the template size + rotation + pivot, and the orientation is
//! always `Direction.NORTH` (`O = 2`). Writing blocks (the `postProcess` surface-height
//! adjustment and the chest loot table) belongs to the placement milestone.

use crate::structure::{
    pieces::{Piece, make_random},
    template::StructureTemplate,
    templated::{MIRROR_NONE, rotation_name, template_bounds},
};

/// `IglooPieces.GENERATION_HEIGHT`：起点的 Y。
///
/// `IglooPieces.GENERATION_HEIGHT`: the start Y.
const GENERATION_HEIGHT: i32 = 90;

/// 雪屋的三个模板：注册名、rotation pivot、相对起点的偏移。
///
/// The three igloo templates: registry name, rotation pivot, offset from the start.
struct Part {
    template: &'static str,
    pivot: (i32, i32),
    offset: (i32, i32, i32),
}

/// `IglooPieces.PIVOTS` / `IglooPieces.OFFSETS` 的合并表，顺序和官方常量一致。
/// pivot 是官方 `BlockPos` 的 X/Z 两轴（Y 不参与 `StructureTemplate.transform`）。
///
/// Combined `IglooPieces.PIVOTS` / `IglooPieces.OFFSETS` table in vanilla constant order.
/// The pivot is the X/Z of the vanilla `BlockPos` (Y never enters
/// `StructureTemplate.transform`).
const IGLOO: Part = Part {
    template: "minecraft:igloo/top",
    pivot: (3, 5),
    offset: (0, 0, 0),
};
const LADDER: Part = Part {
    template: "minecraft:igloo/middle",
    pivot: (1, 1),
    offset: (2, -3, 4),
};
const LABORATORY: Part = Part {
    template: "minecraft:igloo/bottom",
    pivot: (3, 7),
    offset: (0, -3, -2),
};

/// `new IglooPiece(manager, template, position, rotation, depth)`：包围盒、朝向和 NBT 键。
///
/// `new IglooPiece(manager, template, position, rotation, depth)`: box, orientation and
/// NBT keys.
fn igloo_piece(
    part: &Part,
    base: (i32, i32, i32),
    rotation: i32,
    depth: i32,
) -> Result<Piece, String> {
    let template = StructureTemplate::load(part.template)?;
    let position = (
        base.0 + part.offset.0,
        base.1 + part.offset.1 - depth,
        base.2 + part.offset.2,
    );
    let bb = template_bounds(template.size, rotation, MIRROR_NONE, part.pivot, position);
    let mut piece = igloo_base(bb, rotation);
    // `TemplateStructurePiece.addAdditionalSaveData` 再 `IglooPiece` 的 `Rot`。
    // `TemplateStructurePiece.addAdditionalSaveData`, then `IglooPiece`'s `Rot`.
    piece.put_str("Template", part.template);
    piece.put_int("TPX", position.0);
    piece.put_int("TPY", position.1);
    piece.put_int("TPZ", position.2);
    piece.put_str("Rot", rotation_name(rotation));
    Ok(piece)
}

/// `TemplateStructurePiece` 构造器里的 `setOrientation(Direction.NORTH)`：`O = 2`、
/// mirror `NONE`；`getRotation()` 被 `TemplateStructurePiece` 覆写成模板的 rotation。
///
/// `setOrientation(Direction.NORTH)` in the `TemplateStructurePiece` constructor sets
/// `O = 2` and mirror `NONE`; `getRotation()` is overridden by `TemplateStructurePiece`
/// to return the template rotation.
fn igloo_base(bb: crate::structure::pieces::Box, rotation: i32) -> Piece {
    let mut piece = Piece::plain("minecraft:iglu", bb, 0);
    piece.o = 2;
    piece.rot = rotation;
    piece.mirror = 0;
    piece
}

/// `IglooStructure.findGenerationPoint` → `IglooPieces.addPieces` 的结构件列表。
///
/// The piece list of `IglooStructure.findGenerationPoint` → `IglooPieces.addPieces`.
pub fn generate(seed: i64, cx: i32, cz: i32) -> Result<Vec<Piece>, String> {
    let random = make_random(seed, cx, cz);
    let rotation = random.next_int(4);
    let base = (cx * 16, GENERATION_HEIGHT, cz * 16);
    let mut pieces = Vec::new();
    if random.next_double() < 0.5 {
        let depth = random.next_int(8) + 4;
        pieces.push(igloo_piece(&LABORATORY, base, rotation, depth * 3)?);
        for i in 0..depth - 1 {
            pieces.push(igloo_piece(&LADDER, base, rotation, i * 3)?);
        }
    }
    pieces.push(igloo_piece(&IGLOO, base, rotation, 0)?);
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::pieces::Extra;

    /// 每次调用结果确定，且最后一件总是地表雪屋。
    ///
    /// The result is deterministic and the last piece is always the surface igloo.
    #[test]
    fn always_ends_with_the_surface_igloo() {
        for seed in [0, 42, -7] {
            let pieces = generate(seed, 3, -5).unwrap();
            let last = pieces.last().unwrap();
            assert_eq!(
                last.extras.get("Template").and_then(Extra::as_str),
                Some("minecraft:igloo/top")
            );
            assert_eq!(pieces, generate(seed, 3, -5).unwrap());
        }
    }

    /// 有地下室时，结构件数 = 1（实验室）+ (depth - 1) 段梯子 + 1（地表）。
    ///
    /// With a basement, the count is 1 (lab) + (depth - 1) ladders + 1 (surface).
    #[test]
    fn basement_depth_matches_the_piece_count() {
        let mut saw_basement = false;
        let mut saw_surface_only = false;
        for seed in 0..64 {
            let pieces = generate(seed, 11, 7).unwrap();
            if pieces.len() == 1 {
                saw_surface_only = true;
            } else {
                saw_basement = true;
                let lab = &pieces[0];
                assert_eq!(
                    lab.extras.get("Template").and_then(Extra::as_str),
                    Some("minecraft:igloo/bottom")
                );
                assert_eq!(pieces.len(), (pieces.len() - 2) + 2);
            }
        }
        assert!(saw_basement && saw_surface_only);
    }
}
