//! 区块 NBT 顶层 `structures` 键的存档编码（`ChunkStatus.FULL` 落盘）。
//!
//! 对应官方 `SerializableChunkData.packStructureData` →
//! `StructureStart.createTag` → `PiecesContainer.save` →
//! `StructurePiece.createTag`。这里只做纯数据编码：输入结构起点和引用，输出
//! 一个 `Tag`，由存档层原样写进 `ChunkEncodeExtra.structures`（见 `chunk_nbt.rs`）。
//!
//! `starts` 是「结构 ID → 起点标签」，起点标签含 `id / ChunkX / ChunkZ /
//! references / Children`，无结构件时只写 `id = "INVALID"`。`Children` 是
//! 结构件标签的列表，每条含 `id / BB / O / GD` 加子类 `addAdditionalSaveData`
//! 字段。`References` 是「结构 ID → packed 区块键的 LongArray」，空列表不写。
//!
//! Save encoding of the chunk NBT top-level `structures` key (`ChunkStatus.FULL`).
//! Mirrors vanilla `SerializableChunkData.packStructureData` →
//! `StructureStart.createTag` → `PiecesContainer.save` →
//! `StructurePiece.createTag`. Pure data only: structure starts and references go
//! in, one `Tag` comes out, written verbatim by the storage layer through
//! `ChunkEncodeExtra.structures`. `starts` maps structure id to a start tag
//! (`id / ChunkX / ChunkZ / references / Children`, or just `id = "INVALID"`
//! when empty); `Children` is a list of piece tags (`id / BB / O / GD` plus
//! subclass `addAdditionalSaveData` fields); `References` maps structure id to
//! a LongArray of packed chunk keys, skipping empty lists.

use std::collections::BTreeMap;

use mc_bamboo_nbt::Tag;

use crate::structure::start::{
    BoundingBox, References, StructurePiece, StructureStart, StructureStarts,
};

/// `BoundingBox.CODEC`（`Codec.INT_STREAM`）在 NBT 里是 6 个 int 的 IntArrayTag。
///
/// `BoundingBox.CODEC` (`Codec.INT_STREAM`) is a 6-int IntArrayTag in NBT.
fn bounding_box_tag(bb: &BoundingBox) -> Tag {
    Tag::IntArray(vec![
        bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z,
    ])
}

/// 一条结构件的存档数据，对应官方 `StructurePiece.createTag`。
///
/// `extras` 是 `addAdditionalSaveData` 写的子类字段（`Width`/`Height`/`Depth`/
/// `HPos` 是 Int，布尔标记是 Byte，模板件还有 `Rotation`/`Mirror` 等）；按任意
/// 顺序给出即可，`CompoundTag` 是 HashMap，规范比较会按键排序。
///
/// Save data of one `StructurePiece`, matching vanilla `StructurePiece.createTag`.
/// `extras` are the subclass `addAdditionalSaveData` fields (`Width`/`Height`/
/// `Depth`/`HPos` as Int, boolean flags as Byte, template pieces add
/// `Rotation`/`Mirror`, ...). Order is irrelevant: `CompoundTag` is a HashMap
/// and canonical comparison sorts keys.
#[derive(Debug, Clone, PartialEq)]
pub struct SavePiece {
    pub id: String,
    pub bb: BoundingBox,
    pub orientation: i32,
    pub gen_depth: i32,
    pub extras: Vec<(String, Tag)>,
}

impl SavePiece {
    /// 从流水线稀疏模型转一条结构件；稀疏模型只有四个基类字段，没有子类参数。
    ///
    /// Convert from the pipeline's sparse model; it has only the four base
    /// fields and no subclass parameters.
    pub fn from_piece(piece: &StructurePiece) -> Self {
        Self {
            id: piece.piece_type.clone(),
            bb: piece.bounding_box,
            orientation: piece.orientation,
            gen_depth: piece.ground_level_delta,
            extras: Vec::new(),
        }
    }

    /// `StructurePiece.createTag`：`id / BB / O / GD` 加 `extras`。
    ///
    /// `StructurePiece.createTag`: `id / BB / O / GD` plus `extras`.
    pub fn to_tag(&self) -> Tag {
        let mut fields = vec![
            ("id".to_owned(), Tag::String(self.id.clone())),
            ("BB".to_owned(), bounding_box_tag(&self.bb)),
            ("O".to_owned(), Tag::Int(self.orientation)),
            ("GD".to_owned(), Tag::Int(self.gen_depth)),
        ];
        fields.extend(self.extras.iter().cloned());
        Tag::Compound(fields)
    }
}

/// 一个结构起点的存档数据，对应官方 `StructureStart.createTag`。
///
/// Save data of one structure start, matching vanilla `StructureStart.createTag`.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveStart {
    pub structure: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub references: i32,
    pub children: Vec<SavePiece>,
}

impl SaveStart {
    /// 从流水线稀疏 `StructureStart` 转换（结构件只有基类字段）。
    ///
    /// Convert from the pipeline's sparse `StructureStart` (base-only pieces).
    pub fn from_start(start: &StructureStart) -> Self {
        Self {
            structure: start.structure.clone(),
            chunk_x: start.chunk_x,
            chunk_z: start.chunk_z,
            references: start.references,
            children: start.pieces.iter().map(SavePiece::from_piece).collect(),
        }
    }

    /// `StructureStart.createTag`。`isValid()` = `Children` 非空；无效起点只写
    /// `id = "INVALID"`（`StructureStart.INVALID_START`）。
    ///
    /// `StructureStart.createTag`. `isValid()` = non-empty `Children`; an invalid
    /// start writes only `id = "INVALID"` (`StructureStart.INVALID_START`).
    pub fn to_tag(&self) -> Tag {
        if self.children.is_empty() {
            return Tag::Compound(vec![("id".to_owned(), Tag::String("INVALID".to_owned()))]);
        }
        let children = Tag::List(self.children.iter().map(SavePiece::to_tag).collect());
        Tag::Compound(vec![
            ("id".to_owned(), Tag::String(self.structure.clone())),
            ("ChunkX".to_owned(), Tag::Int(self.chunk_x)),
            ("ChunkZ".to_owned(), Tag::Int(self.chunk_z)),
            ("references".to_owned(), Tag::Int(self.references)),
            ("Children".to_owned(), children),
        ])
    }
}

/// `SerializableChunkData.packStructureData`：`starts` 复合标签和 `References`
/// 复合标签。空引用列表按官方跳过（`if (!entry.getValue().isEmpty())`）。
///
/// `SerializableChunkData.packStructureData`: the `starts` and `References`
/// compounds. Empty reference lists are skipped (`if (!...isEmpty())`).
pub fn structures_tag(starts: &BTreeMap<String, SaveStart>, references: &References) -> Tag {
    let starts_compound = Tag::Compound(
        starts
            .iter()
            .map(|(id, start)| (id.clone(), start.to_tag()))
            .collect(),
    );
    let references_compound = Tag::Compound(
        references
            .iter()
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(id, keys)| (id.clone(), Tag::LongArray(keys.clone())))
            .collect(),
    );
    Tag::Compound(vec![
        ("starts".to_owned(), starts_compound),
        ("References".to_owned(), references_compound),
    ])
}

/// 从流水线稀疏 `StructureStarts`（`ChunkAccess.getAllStarts()`）+ 引用直接编出
/// `structures` 键。结构件只有基类四字段；子类 `addAdditionalSaveData` 参数要等
/// 放置里程碑把布局模型接进 `StructureStart` 后才带得全。
///
/// Encode the `structures` key from the pipeline's sparse `StructureStarts`
/// (`ChunkAccess.getAllStarts()`) plus references. Pieces carry only the four
/// base fields; subclass `addAdditionalSaveData` params need the placement
/// milestone to connect the layout model into `StructureStart`.
pub fn encode_structures(starts: &StructureStarts, references: &References) -> Tag {
    let saves: BTreeMap<String, SaveStart> = starts
        .by_structure
        .iter()
        .map(|(id, start)| (id.clone(), SaveStart::from_start(start)))
        .collect();
    structures_tag(&saves, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb() -> BoundingBox {
        BoundingBox {
            min_x: 1,
            min_y: 64,
            min_z: 2,
            max_x: 21,
            max_y: 78,
            max_z: 22,
        }
    }

    #[test]
    fn piece_tag_has_official_fields() {
        let piece = SavePiece {
            id: "minecraft:tedp".to_owned(),
            bb: bb(),
            orientation: 0,
            gen_depth: 0,
            extras: vec![
                ("Width".to_owned(), Tag::Int(21)),
                ("hasPlacedChest0".to_owned(), Tag::Byte(0)),
            ],
        };
        assert_eq!(
            piece.to_tag(),
            Tag::Compound(vec![
                ("id".to_owned(), Tag::String("minecraft:tedp".to_owned())),
                ("BB".to_owned(), Tag::IntArray(vec![1, 64, 2, 21, 78, 22])),
                ("O".to_owned(), Tag::Int(0)),
                ("GD".to_owned(), Tag::Int(0)),
                ("Width".to_owned(), Tag::Int(21)),
                ("hasPlacedChest0".to_owned(), Tag::Byte(0)),
            ])
        );
    }

    #[test]
    fn invalid_start_writes_only_invalid_id() {
        let start = SaveStart {
            structure: "minecraft:village_plains".to_owned(),
            chunk_x: 3,
            chunk_z: -9,
            references: 0,
            children: Vec::new(),
        };
        assert_eq!(
            start.to_tag(),
            Tag::Compound(vec![("id".to_owned(), Tag::String("INVALID".to_owned()))])
        );
    }

    #[test]
    fn empty_references_are_skipped() {
        let mut references = References::new();
        references.insert("minecraft:empty".to_owned(), Vec::new());
        references.insert("minecraft:stronghold".to_owned(), vec![5, -7]);
        let tag = structures_tag(&BTreeMap::new(), &references);
        let Tag::Compound(entries) = tag else {
            panic!("structures must be a compound");
        };
        let references_tag = entries
            .iter()
            .find(|(key, _)| key == "References")
            .map(|(_, value)| value)
            .unwrap();
        assert_eq!(
            references_tag,
            &Tag::Compound(vec![(
                "minecraft:stronghold".to_owned(),
                Tag::LongArray(vec![5, -7])
            )])
        );
    }

    #[test]
    fn encode_structures_roundtrips_sparse_model() {
        let mut starts = StructureStarts::default();
        starts.insert(StructureStart {
            structure: "minecraft:desert_pyramid".to_owned(),
            chunk_x: 3,
            chunk_z: -9,
            references: 1,
            pieces: vec![StructurePiece {
                piece_type: "minecraft:tedp".to_owned(),
                bounding_box: bb(),
                orientation: 2,
                ground_level_delta: 0,
            }],
            terrain_adaptation: false,
        });
        let tag = encode_structures(&starts, &References::new());
        let Tag::Compound(entries) = tag else {
            panic!("structures must be a compound");
        };
        let starts_tag = entries
            .iter()
            .find(|(key, _)| key == "starts")
            .map(|(_, value)| value)
            .unwrap();
        assert_eq!(
            starts_tag,
            &Tag::Compound(vec![(
                "minecraft:desert_pyramid".to_owned(),
                Tag::Compound(vec![
                    (
                        "id".to_owned(),
                        Tag::String("minecraft:desert_pyramid".to_owned())
                    ),
                    ("ChunkX".to_owned(), Tag::Int(3)),
                    ("ChunkZ".to_owned(), Tag::Int(-9)),
                    ("references".to_owned(), Tag::Int(1)),
                    (
                        "Children".to_owned(),
                        Tag::List(vec![Tag::Compound(vec![
                            ("id".to_owned(), Tag::String("minecraft:tedp".to_owned())),
                            ("BB".to_owned(), Tag::IntArray(vec![1, 64, 2, 21, 78, 22])),
                            ("O".to_owned(), Tag::Int(2)),
                            ("GD".to_owned(), Tag::Int(0)),
                        ])])
                    ),
                ])
            )])
        );
    }
}
