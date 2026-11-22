//! `ChunkGenerator.getMobsAt` 的结构刷怪覆盖：位置落在某结构的覆盖盒里时，用该结构
//! 的加权表替换群系表（掠夺者前哨站、女巫小屋等）。
//!
//! 官方先取该区块的 `STRUCTURE_REFERENCES`（`getAllStructuresAt`），再逐个引用区块读
//! `STRUCTURE_STARTS`，按 `StructureSpawnOverride.boundingBox` 判断位置：`STRUCTURE`
//! 用整个起点的包围盒（含 `terrainAdaptation` 外扩），`PIECE` 要求落在某个结构件里。
//! 命中就返回覆盖表，没命中回退群系表。
//!
//! The structure spawn override of `ChunkGenerator.getMobsAt`: inside a structure's
//! override box the structure's weighted list replaces the biome table (pillager
//! outposts, swamp huts, ...). Vanilla reads the chunk's `STRUCTURE_REFERENCES`
//! (`getAllStructuresAt`), then each referenced chunk's `STRUCTURE_STARTS`, and checks the
//! position against `StructureSpawnOverride.boundingBox`: `STRUCTURE` uses the start's
//! whole bounding box (with the `terrainAdaptation` inflation), `PIECE` requires a piece
//! box. A hit returns the override list, otherwise the biome table is used.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::spawning::{self, MobCategory, MobSpawnerDef, SpawnOverrideBox};
use mc_bamboo_worldgen::structure::pipeline::WorldStructureStart;

use crate::world_options::WorldGenConfig;

/// 按位置和类别查结构刷怪覆盖。结构数据由世界生成按种子重算（和交付区块时是同一份
/// 纯函数），所以不需要读区块里存的 `structures` 标签。
///
/// The structure override at a position and category. Structure data is recomputed
/// from the seed (the same pure function used when delivering chunks), so the
/// `structures` tag stored in the chunk is not needed.
pub(crate) fn override_at(
    config: &WorldGenConfig,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    if !config.generate_structures {
        return None;
    }
    let chunk = pos.chunk();
    let references =
        crate::worldgen_structures::structure_references_stage(config, chunk.x, chunk.z).ok()?;
    for (structure, keys) in &references {
        let Some(row) = spawning::structure_spawn_override_row(structure, category) else {
            continue;
        };
        for &packed in keys {
            let (x, z) = crate::worldgen_structures::unpack_chunk_pos(packed);
            let Ok(starts) = crate::worldgen_structures::structure_starts(config, x, z) else {
                continue;
            };
            let Some(start) = starts.get(structure) else {
                continue;
            };
            if start.is_valid() && contains(row.bounding, start, pos) {
                return Some(row.spawns);
            }
        }
    }
    None
}

/// 纯判定，供单测和对拍用：给定 `(结构名, 起点)` 对，返回命中的覆盖表。
///
/// Pure check for unit tests and the oracle: given `(structure, start)` pairs, return
/// the override list that covers the position.
pub fn resolve_override<'a>(
    starts: impl IntoIterator<Item = (&'a str, &'a WorldStructureStart)>,
    pos: BlockPos,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    for (structure, start) in starts {
        let Some(row) = spawning::structure_spawn_override_row(structure, category) else {
            continue;
        };
        if start.is_valid() && contains(row.bounding, start, pos) {
            return Some(row.spawns);
        }
    }
    None
}

/// `StructureSpawnOverride` 的盒判定。
///
/// The box check of `StructureSpawnOverride`.
fn contains(bounding: SpawnOverrideBox, start: &WorldStructureStart, pos: BlockPos) -> bool {
    match bounding {
        SpawnOverrideBox::Structure => start.bounding_box().contains(pos.x, pos.y, pos.z),
        SpawnOverrideBox::Piece => start.pieces.has_piece_at(pos.x, pos.y, pos.z),
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::registries::entity_type;
    use mc_bamboo_worldgen::structure::{
        pipeline::{StartPieces, WorldStructureStart},
        start::{BoundingBox, StructurePiece},
    };

    use super::*;

    /// 一个只有一件、包围盒固定的起点。
    ///
    /// A one-piece start with a fixed box.
    fn start(structure: &str, bb: [i32; 6], adaptation: bool) -> WorldStructureStart {
        WorldStructureStart {
            structure: structure.to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            references: 1,
            pieces: StartPieces::Sparse(vec![StructurePiece {
                // 读盘重建只认注册表里的结构件 id（`load_pieces` 会跳过未知 id），
                // `minecraft:shstart` 是女巫小屋的起始件。
                // Save reload only keeps known piece ids (`load_pieces` drops unknown
                // ones); `minecraft:shstart` is the swamp-hut start piece.
                piece_type: "minecraft:shstart".to_owned(),
                bounding_box: BoundingBox {
                    min_x: bb[0],
                    min_y: bb[1],
                    min_z: bb[2],
                    max_x: bb[3],
                    max_y: bb[4],
                    max_z: bb[5],
                },
                orientation: -1,
                ground_level_delta: 0,
            }]),
            terrain_adaptation: adaptation,
        }
    }

    /// 前哨站用整个起点的盒，并且 `terrainAdaptation` 外扩 12；女巫小屋只认结构件。
    ///
    /// Outposts use the whole start box inflated by 12; swamp huts only piece boxes.
    #[test]
    fn structure_and_piece_boxes_differ() {
        let outpost = start("minecraft:pillager_outpost", [0, 60, 0, 10, 80, 10], true);
        let inside = BlockPos::new(5, 70, 5);
        let inflated = BlockPos::new(-11, 70, 5);
        assert_eq!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                inside,
                MobCategory::Monster
            )
            .map(|table| table[0].entity_type),
            Some(entity_type::PILLAGER)
        );
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                inflated,
                MobCategory::Monster
            )
            .is_some()
        );
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                BlockPos::new(-13, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
        // `PIECE` 只看结构件，不吃外扩。
        let hut = start("minecraft:swamp_hut", [0, 60, 0, 10, 80, 10], false);
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &hut)],
                BlockPos::new(5, 70, 5),
                MobCategory::Monster
            )
            .is_some()
        );
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &hut)],
                BlockPos::new(-11, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
    }

    /// 类别没有注册覆盖时回退群系表（这里返回 `None`）。
    ///
    /// A category without a registered override yields `None` (the biome table).
    #[test]
    fn category_without_override_falls_back() {
        let outpost = start("minecraft:pillager_outpost", [0, 60, 0, 10, 80, 10], true);
        assert!(
            resolve_override(
                [("minecraft:pillager_outpost", &outpost)],
                BlockPos::new(5, 70, 5),
                MobCategory::Creature
            )
            .is_none()
        );
        assert!(
            resolve_override(
                [("minecraft:swamp_hut", &outpost)],
                BlockPos::new(20, 70, 5),
                MobCategory::Monster
            )
            .is_none()
        );
    }

    /// 活路径：读盘来的 `structures` 标签 → 引用 → 起点 → 覆盖表。用一个人造的女巫
    /// 小屋起点，避免为测试扫几万个区块找真结构。
    ///
    /// The live path: an on-disk `structures` tag → references → starts → override. A
    /// synthetic swamp hut start avoids scanning tens of thousands of chunks for a real
    /// one in a test.
    #[test]
    fn override_at_follows_stored_structure_references() {
        use mc_bamboo_worldgen::structure::pipeline::WorldStructureStarts;

        use crate::world_options::{LevelType, WorldGenConfig};
        use crate::worldgen_structures::{
            clear_disk_structures, encode_structures, install_disk_structures,
        };

        let config = WorldGenConfig {
            seed: 20261122,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let hut = start("minecraft:swamp_hut", [2, 60, 2, 8, 70, 8], false);
        let mut starts = WorldStructureStarts::default();
        starts.insert(hut);
        let references = std::collections::BTreeMap::from([(
            "minecraft:swamp_hut".to_owned(),
            vec![mc_bamboo_worldgen::structure::start::chunk_pos_pack(0, 0)],
        )]);
        let tag = encode_structures(&config, &starts, &references).unwrap();
        install_disk_structures(&config, 0, 0, Some(&tag));
        assert_eq!(
            override_at(&config, BlockPos::new(5, 65, 5), MobCategory::Monster)
                .map(|table| table[0].entity_type),
            Some(entity_type::WITCH)
        );
        assert_eq!(
            override_at(&config, BlockPos::new(5, 65, 5), MobCategory::Creature)
                .map(|table| table[0].entity_type),
            Some(entity_type::CAT)
        );
        clear_disk_structures();
    }
}
