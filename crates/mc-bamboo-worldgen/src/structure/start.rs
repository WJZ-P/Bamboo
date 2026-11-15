//! `ChunkGenerator.createStructures` / `createReferences` 的纯数据部分。
//!
//! 本模块只移植「哪些结构被尝试、按什么顺序、起点区块的引用怎么写」这些不需要
//! `Structure.generate`（结构件生成）就能确定的部分。`Structure.generate` 里的
//! `findValidGenerationPoint`（群系检查 + 高度检查）和结构件生成属于下一步，
//! 所以这里产出的是「尝试序列」而不是最终的 `StructureStart`。
//!
//! Pure-data part of `ChunkGenerator.createStructures` / `createReferences`.
//! Only the parts decidable without `Structure.generate` (piece generation) are
//! ported here: which structures are tried in which order, and how start
//! references are written. `findValidGenerationPoint` (biome + height checks)
//! and piece generation come in a later milestone, so this yields an attempt
//! sequence rather than a final `StructureStart`.

use std::collections::BTreeMap;

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    random::{Random, WorldgenRandom},
    structure::registry::SelectionEntry,
};

/// 官方 `net.minecraft.world.level.levelgen.structure.BoundingBox`（六个 int）。
///
/// Vanilla `net.minecraft.world.level.levelgen.structure.BoundingBox` (six ints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    /// 空包围盒的起点，等价于官方 `PiecesContainer.calculateBoundingBox()` 的空值。
    ///
    /// The starting value of an empty union, like an empty `PiecesContainer`.
    pub const EMPTY: Self = Self {
        min_x: i32::MAX,
        min_y: i32::MAX,
        min_z: i32::MAX,
        max_x: i32::MIN,
        max_y: i32::MIN,
        max_z: i32::MIN,
    };

    /// `BoundingBox.encapsulating(a, b)`：取两盒的并集。
    ///
    /// `BoundingBox.encapsulating(a, b)`: union of two boxes.
    pub fn encapsulating(a: Self, b: Self) -> Self {
        Self {
            min_x: a.min_x.min(b.min_x),
            min_y: a.min_y.min(b.min_y),
            min_z: a.min_z.min(b.min_z),
            max_x: a.max_x.max(b.max_x),
            max_y: a.max_y.max(b.max_y),
            max_z: a.max_z.max(b.max_z),
        }
    }

    /// `BoundingBox.inflatedBy(n)`。
    pub fn inflated_by(&self, n: i32) -> Self {
        Self {
            min_x: self.min_x - n,
            min_y: self.min_y - n,
            min_z: self.min_z - n,
            max_x: self.max_x + n,
            max_y: self.max_y + n,
            max_z: self.max_z + n,
        }
    }

    /// `BoundingBox.intersects(minX, minZ, maxX, maxZ)`：只比 XZ 两轴。
    ///
    /// `BoundingBox.intersects(minX, minZ, maxX, maxZ)`: XZ-only overlap.
    pub fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max_x >= min_x && self.min_x <= max_x && self.max_z >= min_z && self.min_z <= max_z
    }

    /// `BoundingBox.isInside(Vec3i)`：六轴都含端点。
    ///
    /// `BoundingBox.isInside(Vec3i)`: inclusive on all six axes.
    pub const fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        self.min_x <= x
            && x <= self.max_x
            && self.min_y <= y
            && y <= self.max_y
            && self.min_z <= z
            && z <= self.max_z
    }

    /// 把一批结构件的包围盒并起来（对应 `PiecesContainer.calculateBoundingBox()`）。
    ///
    /// Union of a set of piece boxes (`PiecesContainer.calculateBoundingBox()`).
    pub fn union(boxes: impl IntoIterator<Item = Self>) -> Self {
        boxes.into_iter().fold(Self::EMPTY, Self::encapsulating)
    }
}

/// 一条结构件（`StructurePiece`）的存档字段，不含各子类特有数据。
///
/// Save fields of a `StructurePiece`, without subclass-specific data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructurePiece {
    /// `BuiltInRegistries.STRUCTURE_PIECE` 的注册名，写进 NBT 的 `id`。
    pub piece_type: String,
    pub bounding_box: BoundingBox,
    /// NBT `O`：`Rotation.get2DDataValue()`，null 是 -1。
    pub orientation: i32,
    /// NBT `GD`：`groundLevelDelta`。
    pub ground_level_delta: i32,
}

/// 官方 `StructureStart`（`StructureStart.java`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureStart {
    pub structure: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub references: i32,
    pub pieces: Vec<StructurePiece>,
    /// `structure.terrainAdaptation() != NONE`：`getBoundingBox()` 要 inflatedBy(12)。
    pub terrain_adaptation: bool,
}

impl StructureStart {
    /// `StructureStart.INVALID_START`：没有结构件。
    pub fn invalid(structure: &str, chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            structure: structure.to_owned(),
            chunk_x,
            chunk_z,
            references: 0,
            pieces: Vec::new(),
            terrain_adaptation: false,
        }
    }

    /// `StructureStart.isValid()` = 结构件非空。
    pub fn is_valid(&self) -> bool {
        !self.pieces.is_empty()
    }

    /// `StructureStart.getBoundingBox()`：结构件并集，terrainAdaptation != NONE 时外扩 12。
    ///
    /// `StructureStart.getBoundingBox()`: union of pieces, inflated by 12 when
    /// `terrainAdaptation != NONE` (`Structure.adjustBoundingBox`).
    pub fn bounding_box(&self) -> BoundingBox {
        let union = BoundingBox::union(self.pieces.iter().map(|piece| piece.bounding_box));
        if self.terrain_adaptation {
            union.inflated_by(12)
        } else {
            union
        }
    }
}

/// 一个区块里已有的全部起点（`ChunkAccess.getAllStarts()` 的等价物）。
///
/// All starts in one chunk (counterpart of `ChunkAccess.getAllStarts()`).
#[derive(Debug, Clone, Default)]
pub struct StructureStarts {
    pub by_structure: BTreeMap<String, StructureStart>,
}

impl StructureStarts {
    /// `StructureManager.getStartForStructure(structure, chunk)`。
    pub fn get(&self, structure: &str) -> Option<&StructureStart> {
        self.by_structure.get(structure)
    }

    /// `StructureManager.setStartForStructure(structure, start, chunk)`。
    pub fn insert(&mut self, start: StructureStart) {
        self.by_structure.insert(start.structure.clone(), start);
    }

    /// `existingStart` 短路：这个结构集里有没有已经生成好的有效起点。
    ///
    /// The `existingStart` short-circuit: does this set already have a valid start?
    pub fn has_valid_for_set(&self, entries: &[SelectionEntry]) -> bool {
        entries.iter().any(|entry| {
            self.get(&entry.structure)
                .is_some_and(StructureStart::is_valid)
        })
    }
}

/// `createStructures` 对一个结构集的产出：结构集名字 + 按官方顺序尝试的结构序列。
/// 最终选中哪个还要等 `Structure.generate` 判定有效（`structure_starts` 里程碑）。
///
/// Output of `createStructures` for one set: the set name plus the structure ids
/// tried in vanilla order. Which one is finally selected needs `Structure.generate`
/// validity checks (a later milestone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPlan {
    pub set: String,
    pub attempts: Vec<String>,
}

/// 区块 NBT `structures.References` 的形状：结构 ID → packed 区块键列表。
///
/// Shape of the `structures.References` NBT: structure id -> packed chunk keys.
pub type References = BTreeMap<String, Vec<i64>>;

/// `ChunkPos.pack(x, z) = (x as u32) | ((z as u32) << 32)`，负数按 32 位补码。
///
/// `ChunkPos.pack(x, z) = (x as u32) | ((z as u32) << 32)`; negatives wrap to u32.
pub fn chunk_pos_pack(x: i32, z: i32) -> i64 {
    (x as u32 as i64) | ((z as u32 as i64) << 32)
}

/// `createStructures` 的加权选择：返回按官方顺序尝试的结构 ID 序列。
///
/// `setLargeFeatureSeed(seed, chunkX, chunkZ)` 播种，每次 `nextInt(total)` 选一个，
/// 命中后从 options 移除、total 减权，直到 options 空。官方在第一个「生成成功」时
/// 提前返回，这里没有结构件生成，所以给出完整序列，调用方取前缀即可。
///
/// Weighted selection of `createStructures`: the structure ids tried in vanilla
/// order. Seed via `setLargeFeatureSeed(seed, chunkX, chunkZ)`, then repeatedly
/// `nextInt(total)`, removing the chosen entry and reducing `total` until empty.
/// Vanilla returns early on the first successful generation; without piece
/// generation we emit the full sequence and let the caller take a prefix.
pub fn selection_sequence(entries: &[SelectionEntry], seed: i64, x: i32, z: i32) -> Vec<String> {
    let mut options: Vec<&SelectionEntry> = entries.iter().collect();
    let mut total: i32 = options.iter().map(|entry| entry.weight).sum();
    let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    random.set_large_feature_seed(seed, x, z);
    let mut sequence = Vec::with_capacity(options.len());
    while !options.is_empty() {
        let mut choice = random.next_int(total);
        let mut index = 0usize;
        for (i, option) in options.iter().enumerate() {
            choice -= option.weight;
            if choice < 0 {
                break;
            }
            index = i + 1;
        }
        // 官方没有显式边界检查，数据合法时 index 一定落在范围内。
        // Vanilla has no explicit bounds check; with valid data index is in range.
        let selected = options[index.min(options.len() - 1)];
        sequence.push(selected.structure.clone());
        total -= selected.weight;
        options.remove(index.min(options.len() - 1));
    }
    sequence
}

/// `ChunkGenerator.createReferences`：中心区块周围 17×17 的邻域里，每个有效起点的
/// 包围盒若和中心区块的 XZ 范围相交，就把源区块的 packed 键记到该结构名下。
///
/// `ChunkGenerator.createReferences`: over the 17x17 neighborhood, any valid start
/// whose bounding box intersects the center chunk's XZ range contributes the
/// source chunk's packed key under its structure id.
pub fn structure_references(
    center_x: i32,
    center_z: i32,
    neighbors: &[(i32, i32, &StructureStarts)],
) -> References {
    let min_block_x = center_x * 16;
    let min_block_z = center_z * 16;
    let mut references = References::new();
    for &(source_x, source_z, starts) in neighbors {
        for start in starts.by_structure.values() {
            if start.is_valid()
                && start.bounding_box().intersects_xz(
                    min_block_x,
                    min_block_z,
                    min_block_x + 15,
                    min_block_z + 15,
                )
            {
                references
                    .entry(start.structure.clone())
                    .or_default()
                    .push(chunk_pos_pack(source_x, source_z));
            }
        }
    }
    references
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<SelectionEntry> {
        vec![
            SelectionEntry {
                structure: "minecraft:village_plains".to_owned(),
                weight: 1,
            },
            SelectionEntry {
                structure: "minecraft:village_desert".to_owned(),
                weight: 2,
            },
            SelectionEntry {
                structure: "minecraft:village_snowy".to_owned(),
                weight: 1,
            },
        ]
    }

    #[test]
    fn selection_is_a_permutation_and_deterministic() {
        let entries = entries();
        let first = selection_sequence(&entries, 42, 7, -9);
        assert_eq!(first, selection_sequence(&entries, 42, 7, -9));
        assert_eq!(first.len(), 3);
        let mut sorted = first.clone();
        sorted.sort();
        let mut expected: Vec<String> = entries.iter().map(|e| e.structure.clone()).collect();
        expected.sort();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn single_entry_is_just_that_structure() {
        let entries = vec![SelectionEntry {
            structure: "minecraft:stronghold".to_owned(),
            weight: 1,
        }];
        assert_eq!(
            selection_sequence(&entries, 1, 0, 0),
            vec!["minecraft:stronghold"]
        );
    }

    #[test]
    fn chunk_pos_pack_matches_vanilla() {
        assert_eq!(chunk_pos_pack(0, 0), 0);
        assert_eq!(chunk_pos_pack(1, 2), 1 | (2 << 32));
        assert_eq!(chunk_pos_pack(-1, -1), 0xFFFF_FFFF_FFFF_FFFF_u64 as i64);
    }

    #[test]
    fn bounding_box_union_and_intersect() {
        let a = BoundingBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 5,
            max_y: 5,
            max_z: 5,
        };
        let b = BoundingBox {
            min_x: 10,
            min_y: 2,
            min_z: -3,
            max_x: 12,
            max_y: 9,
            max_z: 2,
        };
        let union = BoundingBox::union([a, b]);
        assert_eq!((union.min_x, union.max_x), (0, 12));
        assert!(union.intersects_xz(-1, -1, 3, 3));
        assert!(!union.intersects_xz(20, 20, 25, 25));
    }

    #[test]
    fn references_only_include_intersecting_valid_starts() {
        let mut starts = StructureStarts::default();
        starts.insert(StructureStart {
            structure: "minecraft:village_plains".to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            references: 0,
            pieces: vec![StructurePiece {
                piece_type: "minecraft:jigsaw".to_owned(),
                bounding_box: BoundingBox {
                    min_x: -10,
                    min_y: 0,
                    min_z: -10,
                    max_x: 100,
                    max_y: 10,
                    max_z: 100,
                },
                orientation: 0,
                ground_level_delta: 0,
            }],
            terrain_adaptation: true,
        });
        let neighbors = vec![(0, 0, &starts), (3, 3, &starts)];
        let references = structure_references(0, 0, &neighbors);
        let keys = &references["minecraft:village_plains"];
        assert!(keys.contains(&chunk_pos_pack(0, 0)));
        assert!(keys.contains(&chunk_pos_pack(3, 3)));
    }
}
