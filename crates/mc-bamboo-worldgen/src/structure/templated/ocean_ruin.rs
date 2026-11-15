//! 海底遗迹（`OceanRuinPieces`）的结构件布局。
//!
//! 官方 `OceanRuinStructure.generatePieces`：起点 `(minBlockX, 90, minBlockZ)`，
//! `Rotation.getRandom(random)` 一次，然后 `OceanRuinPieces.addPieces`：
//! `nextFloat() <= largeProbability` 决定大遗迹（完好度 0.9）还是小遗迹（0.8）；
//! 大遗迹再 `nextFloat() <= clusterProbability` 时追加一圈小遗迹。暖水遗迹从
//! `WARM_RUINS` / `BIG_WARM_RUINS` 里抽一张模板；冷水遗迹一次 `nextInt(8)` 同时选中
//! brick / cracked / mossy 三张同号模板，完好度分别是调用方给的、0.7、0.5。
//! 所有结构件的 rotation pivot 都是 `BlockPos.ZERO`、`Mirror.NONE`，包围盒直接由模板
//! 尺寸和 rotation 得出。写方块（`postProcess` 的海床高度、processor、溺尸、箱子）
//! 属放置里程碑。
//!
//! Layout of the ocean ruin pieces. Vanilla `OceanRuinStructure.generatePieces` starts at
//! `(minBlockX, 90, minBlockZ)`, draws `Rotation.getRandom(random)` once, then calls
//! `OceanRuinPieces.addPieces`: `nextFloat() <= largeProbability` selects the large ruin
//! (integrity 0.9) or the small one (0.8), and a large ruin adds a ring of small ruins
//! when `nextFloat() <= clusterProbability`. Warm ruins pick one template from
//! `WARM_RUINS` / `BIG_WARM_RUINS`; cold ruins draw `nextInt(8)` once and use the same
//! index for the brick / cracked / mossy templates with integrity as given, 0.7 and 0.5.
//! Every piece uses rotation pivot `BlockPos.ZERO` and `Mirror.NONE`, so the box follows
//! the template size and rotation directly. Writing blocks (the `postProcess` sea-floor
//! height, processors, drowned, chests) belongs to the placement milestone.

use crate::structure::{
    pieces::{Box, Piece, intersects, make_random, new_box},
    template::StructureTemplate,
    templated::{MIRROR_NONE, rotation_name, template_bounds},
};

#[cfg(test)]
use crate::structure::pieces::Extra;

/// `OceanRuinStructure.Type`：暖水 / 冷水。
///
/// `OceanRuinStructure.Type`: warm or cold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiomeTemp {
    Warm,
    Cold,
}

impl BiomeTemp {
    /// `OceanRuinStructure.Type.LEGACY_CODEC` 写进 NBT 的字符串。
    ///
    /// The string `OceanRuinStructure.Type.LEGACY_CODEC` writes to NBT.
    pub fn name(self) -> &'static str {
        match self {
            Self::Warm => "WARM",
            Self::Cold => "COLD",
        }
    }
}

/// `OceanRuinStructure` 的三个配置字段。
///
/// The three config fields of `OceanRuinStructure`.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub biome_temp: BiomeTemp,
    pub large_probability: f32,
    pub cluster_probability: f32,
}

impl Config {
    /// 从 `structure` 条目的原始 JSON 解析 `biome_temp` / `large_probability` /
    /// `cluster_probability`，和 `OceanRuinStructure.CODEC` 的字段名一致。
    ///
    /// Parse `biome_temp` / `large_probability` / `cluster_probability` from a `structure`
    /// entry's raw JSON, matching the `OceanRuinStructure.CODEC` field names.
    pub fn parse(raw: &serde_json::Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("ocean ruin must be an object")?;
        let biome_temp = match object.get("biome_temp").and_then(serde_json::Value::as_str) {
            Some("warm") => BiomeTemp::Warm,
            Some("cold") => BiomeTemp::Cold,
            _ => return Err("ocean ruin has no valid biome_temp".to_owned()),
        };
        let float = |key: &str| -> Result<f32, String> {
            object
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .map(|value| value as f32)
                .ok_or_else(|| format!("ocean ruin has no {key}"))
        };
        Ok(Self {
            biome_temp,
            large_probability: float("large_probability")?,
            cluster_probability: float("cluster_probability")?,
        })
    }
}

/// `OceanRuinPieces.WARM_RUINS`。
///
/// `OceanRuinPieces.WARM_RUINS`.
const WARM_RUINS: [&str; 8] = [
    "minecraft:underwater_ruin/warm_1",
    "minecraft:underwater_ruin/warm_2",
    "minecraft:underwater_ruin/warm_3",
    "minecraft:underwater_ruin/warm_4",
    "minecraft:underwater_ruin/warm_5",
    "minecraft:underwater_ruin/warm_6",
    "minecraft:underwater_ruin/warm_7",
    "minecraft:underwater_ruin/warm_8",
];
/// `OceanRuinPieces.BIG_WARM_RUINS`。
///
/// `OceanRuinPieces.BIG_WARM_RUINS`.
const BIG_WARM_RUINS: [&str; 4] = [
    "minecraft:underwater_ruin/big_warm_4",
    "minecraft:underwater_ruin/big_warm_5",
    "minecraft:underwater_ruin/big_warm_6",
    "minecraft:underwater_ruin/big_warm_7",
];
/// `OceanRuinPieces.RUINS_BRICK` / `BIG_RUINS_BRICK` 的公共后缀。
///
/// The shared suffixes of `OceanRuinPieces.RUINS_BRICK` / `BIG_RUINS_BRICK`.
const RUINS_BRICK: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];
const BIG_RUINS_BRICK: [&str; 4] = ["1", "2", "3", "8"];

/// 冷水遗迹的三类模板名：`underwater_ruin/{brick, cracked, mossy}_N`。
///
/// The three cold-ruin template names: `underwater_ruin/{brick, cracked, mossy}_N`.
fn cold_template(kind: &str, big: bool, suffix: &str) -> String {
    let prefix = if big { "big_" } else { "" };
    format!("minecraft:underwater_ruin/{prefix}{kind}_{suffix}")
}

/// `Mth.nextInt(random, min, max)`：`min >= max` 时直接返回 `min`。
///
/// `Mth.nextInt(random, min, max)`: returns `min` outright when `min >= max`.
fn next_int_between(random: &crate::random::WorldgenRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random.next_int(max - min + 1) + min
    }
}

/// `new OceanRuinPiece(manager, template, position, rotation, integrity, biomeTemp, isLarge)`。
///
/// `new OceanRuinPiece(manager, template, position, rotation, integrity, biomeTemp, isLarge)`.
fn ruin_piece(
    template_name: &str,
    position: (i32, i32, i32),
    rotation: i32,
    integrity: f32,
    biome_temp: BiomeTemp,
    is_large: bool,
) -> Result<Piece, String> {
    let template = StructureTemplate::load(template_name)?;
    let bb = template_bounds(template.size, rotation, MIRROR_NONE, (0, 0), position);
    let mut piece = Piece::plain("minecraft:orp", bb, 0);
    piece.o = 2;
    piece.rot = rotation;
    piece.mirror = MIRROR_NONE;
    piece.put_str("Template", template_name);
    piece.put_int("TPX", position.0);
    piece.put_int("TPY", position.1);
    piece.put_int("TPZ", position.2);
    piece.put_str("Rot", rotation_name(rotation));
    piece.put_float("Integrity", integrity);
    piece.put_str("BiomeType", biome_temp.name());
    piece.put_bool("IsLarge", is_large);
    Ok(piece)
}

/// `StructureTemplate.transform(pos, Mirror.NONE, rotation, BlockPos.ZERO).offset(base)`
/// 的 XZ 结果（`OceanRuinPieces` 只用 `(15,0,15)` 和 `(5,0,6)` 这两个点算包围盒）。
///
/// The XZ result of
/// `StructureTemplate.transform(pos, Mirror.NONE, rotation, BlockPos.ZERO).offset(base)`,
/// used by `OceanRuinPieces` for the `(15,0,15)` and `(5,0,6)` corners only.
fn corner_of(pos: (i32, i32), rotation: i32, base: (i32, i32, i32)) -> (i32, i32) {
    let (x, z) = pos;
    let (dx, dz) = match rotation {
        1 => (-z, x),
        2 => (-x, -z),
        3 => (z, -x),
        _ => (x, z),
    };
    (base.0 + dx, base.2 + dz)
}

/// `OceanRuinPieces.addPiece`：按 `biome_temp` 决定模板与完好度，往列表里追加 1 或 3 件。
///
/// `OceanRuinPieces.addPiece`: pick templates and integrity by `biome_temp` and append
/// one or three pieces.
fn add_piece(
    pieces: &mut Vec<Piece>,
    random: &crate::random::WorldgenRandom,
    position: (i32, i32, i32),
    rotation: i32,
    config: Config,
    is_large: bool,
    base_integrity: f32,
) -> Result<(), String> {
    match config.biome_temp {
        BiomeTemp::Warm => {
            let table: &[&str] = if is_large {
                &BIG_WARM_RUINS
            } else {
                &WARM_RUINS
            };
            let template = table[random.next_int(table.len() as i32) as usize];
            pieces.push(ruin_piece(
                template,
                position,
                rotation,
                base_integrity,
                config.biome_temp,
                is_large,
            )?);
        }
        BiomeTemp::Cold => {
            let suffix = if is_large {
                BIG_RUINS_BRICK[random.next_int(4) as usize]
            } else {
                RUINS_BRICK[random.next_int(8) as usize]
            };
            for (kind, integrity) in [("brick", base_integrity), ("cracked", 0.7), ("mossy", 0.5)] {
                let template = cold_template(kind, is_large, suffix);
                pieces.push(ruin_piece(
                    &template,
                    position,
                    rotation,
                    integrity,
                    config.biome_temp,
                    is_large,
                )?);
            }
        }
    }
    Ok(())
}

/// `OceanRuinPieces.allPositions(random, origin)`：八个小遗迹候选位置，取数顺序照官方。
/// 每项是 `(x 基值, x 下界, x 上界, z 基值, z 下界, z 上界)`，逐项照抄官方的
/// `Mth.nextInt(random, 下界, 上界)`。
///
/// `OceanRuinPieces.allPositions(random, origin)`: eight candidate positions for the
/// small ruins, drawing in vanilla order. Each entry is
/// `(x base, x min, x max, z base, z min, z max)`, copying the vanilla
/// `Mth.nextInt(random, min, max)` bounds per entry.
const CLUSTER_POSITIONS: [(i32, i32, i32, i32, i32, i32); 8] = [
    (-16, 1, 8, 16, 1, 7),
    (-16, 1, 8, 0, 1, 7),
    (-16, 1, 8, -16, 4, 8),
    (0, 1, 7, 16, 1, 7),
    (0, 1, 7, -16, 4, 6),
    (16, 1, 7, 16, 3, 8),
    (16, 1, 7, 0, 1, 7),
    (16, 1, 7, -16, 4, 8),
];

fn all_positions(
    random: &crate::random::WorldgenRandom,
    origin: (i32, i32, i32),
) -> Vec<(i32, i32, i32)> {
    let mut positions = Vec::with_capacity(8);
    for (x_base, x_min, x_max, z_base, z_min, z_max) in CLUSTER_POSITIONS {
        // `origin.offset(a + Mth.nextInt(random, b, c), 0, d + Mth.nextInt(random, e, f))`：
        // Java 先算 X 再算 Z，两个 `nextInt` 的顺序不能交换。
        // `origin.offset(a + Mth.nextInt(random, b, c), 0, d + Mth.nextInt(random, e, f))`:
        // Java draws X first and Z second; the two `nextInt` calls must not be swapped.
        let x_offset = x_base + next_int_between(random, x_min, x_max);
        let z_offset = z_base + next_int_between(random, z_min, z_max);
        positions.push((origin.0 + x_offset, origin.1, origin.2 + z_offset));
    }
    positions
}

/// `OceanRuinPieces.addClusterRuins`。
///
/// `OceanRuinPieces.addClusterRuins`.
fn add_cluster_ruins(
    pieces: &mut Vec<Piece>,
    random: &crate::random::WorldgenRandom,
    rotation: i32,
    position: (i32, i32, i32),
    config: Config,
) -> Result<(), String> {
    let parent_pos = (position.0, 90, position.2);
    let parent_corner = corner_of((15, 15), rotation, parent_pos);
    let parent_bb = from_corners(parent_pos, parent_corner);
    let parent_bottom_left = (
        parent_pos.0.min(parent_corner.0),
        parent_pos.1,
        parent_pos.2.min(parent_corner.1),
    );
    let mut all = all_positions(random, parent_bottom_left);
    let ruins = next_int_between(random, 4, 8);
    for _ in 0..ruins {
        if all.is_empty() {
            break;
        }
        let index = random.next_int(all.len() as i32) as usize;
        let pos = all.remove(index);
        let next_rotation = random.next_int(4);
        let next_corner = corner_of((5, 6), next_rotation, pos);
        let next_bb = from_corners(pos, next_corner);
        if !intersects(&next_bb, &parent_bb) {
            add_piece(pieces, random, pos, next_rotation, config, false, 0.8)?;
        }
    }
    Ok(())
}

/// `BoundingBox.fromCorners(a, b)`（两个 XZ 角点、同一 Y）。
///
/// `BoundingBox.fromCorners(a, b)` for two XZ corners at the same Y.
fn from_corners(a: (i32, i32, i32), b: (i32, i32)) -> Box {
    new_box(
        a.0.min(b.0),
        a.1,
        a.2.min(b.1),
        a.0.max(b.0),
        a.1,
        a.2.max(b.1),
    )
}

/// `OceanRuinStructure.findGenerationPoint` → `OceanRuinPieces.addPieces` 的结构件列表。
///
/// The piece list of `OceanRuinStructure.findGenerationPoint` → `OceanRuinPieces.addPieces`.
pub fn generate(seed: i64, cx: i32, cz: i32, config: Config) -> Result<Vec<Piece>, String> {
    let random = make_random(seed, cx, cz);
    let position = (cx * 16, 90, cz * 16);
    let rotation = random.next_int(4);
    let is_large = random.next_float() <= config.large_probability;
    let base_integrity = if is_large { 0.9 } else { 0.8 };
    let mut pieces = Vec::new();
    add_piece(
        &mut pieces,
        &random,
        position,
        rotation,
        config,
        is_large,
        base_integrity,
    )?;
    if is_large && random.next_float() <= config.cluster_probability {
        add_cluster_ruins(&mut pieces, &random, rotation, position, config)?;
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WARM: Config = Config {
        biome_temp: BiomeTemp::Warm,
        large_probability: 0.3,
        cluster_probability: 0.9,
    };
    const COLD: Config = Config {
        biome_temp: BiomeTemp::Cold,
        large_probability: 0.3,
        cluster_probability: 0.9,
    };

    /// 暖水遗迹每件都是从暖水表里抽的，冷水遗迹的 brick / cracked / mossy 同号。
    ///
    /// Warm ruins take every template from the warm tables; cold ruins share one suffix
    /// across brick / cracked / mossy.
    #[test]
    fn templates_follow_the_biome_kind() {
        for seed in 0..32 {
            let warm = generate(seed, 4, 9, WARM).unwrap();
            let warm_template = warm[0].extras["Template"].as_str().unwrap();
            assert!(WARM_RUINS.contains(&warm_template) || BIG_WARM_RUINS.contains(&warm_template));
            let cold = generate(seed, 4, 9, COLD).unwrap();
            let big = cold[0].extras["IsLarge"] == Extra::Byte(1);
            let template = cold[0].extras["Template"].as_str().unwrap();
            let suffix = template.rsplit('_').next().unwrap().to_owned();
            for (i, kind) in ["brick", "cracked", "mossy"].iter().enumerate() {
                assert_eq!(
                    cold[i].extras["Template"],
                    Extra::Str(cold_template(kind, big, &suffix))
                );
            }
            assert_eq!(
                cold[0].extras["Integrity"],
                Extra::Float(if big { 0.9 } else { 0.8 })
            );
            assert_eq!(cold[1].extras["Integrity"], Extra::Float(0.7));
            assert_eq!(cold[2].extras["Integrity"], Extra::Float(0.5));
        }
    }

    /// 冷水大遗迹的三件包围盒完全相同（同位置、同朝向、同尺寸）。
    ///
    /// The three cold large-ruin pieces share one bounding box (same position, rotation
    /// and size).
    #[test]
    fn cold_ruins_stack_three_pieces_on_one_box() {
        for seed in 0..32 {
            let cold = generate(seed, -6, 2, COLD).unwrap();
            if cold[0].extras["IsLarge"] == Extra::Byte(1) {
                assert_eq!(cold[0].bb, cold[1].bb);
                assert_eq!(cold[0].bb, cold[2].bb);
            }
        }
    }
}
