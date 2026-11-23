//! 结构刷怪覆盖的场景：官方 `ChunkGenerator.getMobsAt` 和 Bamboo 的
//! [`mc_bamboo_game::structure_spawn::resolve_override`] 对同一批结构起点和位置求表。
//!
//! Structure spawn override scenes: vanilla `ChunkGenerator.getMobsAt` and Bamboo's
//! [`mc_bamboo_game::structure_spawn::resolve_override`] resolve the same starts and
//! positions.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    registries::biome,
    spawning::{self, MobCategory},
};
use mc_bamboo_worldgen::structure::{
    pipeline::{StartPieces, WorldStructureStart},
    start::{BoundingBox, StructurePiece},
};
use serde::Deserialize;

use super::Report;

/// Java 一侧发来的一个起点：结构名、是否外扩 12、每件的包围盒。
///
/// One start from the Java side: structure name, whether it inflates by 12, piece boxes.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    structure: String,
    adaptation: bool,
    pieces: Vec<[i32; 6]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Query {
    category: String,
    pos: [i32; 3],
    /// 官方解析出的 `(类型, 权重, 最少, 最多)`；空表示空表。
    ///
    /// The official `(type, weight, min, max)` list; empty for an empty list.
    expected: Vec<[i32; 4]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    name: String,
    biome: String,
    starts: Vec<Start>,
    queries: Vec<Query>,
}

#[derive(Default)]
pub(super) struct Coverage {
    pub(super) cases: usize,
    queries: usize,
    overrides: usize,
}

impl Coverage {
    pub(super) fn print(&self) {
        if self.cases > 0 {
            println!(
                "STRUCTURE SPAWN: {} cases, {} queries, {} override hits",
                self.cases, self.queries, self.overrides
            );
        }
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if self.cases > 0 && self.overrides == 0 {
            return Err("the structure spawn fixture never hit an override".to_owned());
        }
        Ok(())
    }
}

impl Report {
    pub(super) fn replay_structure_spawn(&mut self, case: &Case) -> Result<(), String> {
        self.cases += 1;
        self.structure_spawns.cases += 1;
        let biome = biome_id(&case.biome)?;
        let starts: Vec<WorldStructureStart> = case
            .starts
            .iter()
            .map(|start| WorldStructureStart {
                structure: start.structure.clone(),
                chunk_x: 0,
                chunk_z: 0,
                references: 1,
                pieces: StartPieces::Sparse(
                    start
                        .pieces
                        .iter()
                        .map(|box_| StructurePiece {
                            piece_type: "minecraft:test".to_owned(),
                            bounding_box: BoundingBox {
                                min_x: box_[0],
                                min_y: box_[1],
                                min_z: box_[2],
                                max_x: box_[3],
                                max_y: box_[4],
                                max_z: box_[5],
                            },
                            orientation: -1,
                            ground_level_delta: 0,
                        })
                        .collect(),
                ),
                terrain_adaptation: start.adaptation,
            })
            .collect();
        for query in &case.queries {
            self.structure_spawns.queries += 1;
            let category = category(&query.category)?;
            let pos = BlockPos::new(query.pos[0], query.pos[1], query.pos[2]);
            let resolved = mc_bamboo_game::structure_spawn::resolve_override(
                starts.iter().map(|start| (start.structure.as_str(), start)),
                pos,
                category,
            );
            let ours: Vec<[i32; 4]> = match resolved {
                Some(table) => {
                    self.structure_spawns.overrides += 1;
                    table
                        .iter()
                        .map(|entry| [entry.entity_type, entry.weight, entry.min, entry.max])
                        .collect()
                }
                None => spawning::for_biome_id(biome)
                    .mobs_to_spawn(category)
                    .iter()
                    .map(|entry| [entry.entity_type, entry.weight, entry.min, entry.max])
                    .collect(),
            };
            if ours != query.expected {
                self.mismatch(format!(
                    "{} {} at {:?}: official {:?}, Bamboo {ours:?}",
                    case.name, query.category, query.pos, query.expected
                ));
            }
        }
        self.steps += 1;
        Ok(())
    }
}

fn category(name: &str) -> Result<MobCategory, String> {
    MobCategory::ALL
        .into_iter()
        .find(|category| category.facts().name == name)
        .ok_or_else(|| format!("unknown mob category {name}"))
}

fn biome_id(name: &str) -> Result<i32, String> {
    biome::NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as i32)
        .ok_or_else(|| format!("unknown biome {name}"))
}
