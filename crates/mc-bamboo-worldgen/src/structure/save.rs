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

use crate::structure::pieces::{Box as LayoutBox, HDir, Piece};
use crate::structure::start::{
    BoundingBox, References, StructurePiece, StructureStart, StructureStarts,
};

/// `BoundingBox.CODEC` 在列表里的元素：6 个 int 的 IntArrayTag。
///
/// One `BoundingBox.CODEC` list element: a 6-int IntArrayTag.
fn entrances_tag(bb: &LayoutBox) -> Tag {
    Tag::IntArray(vec![bb[0], bb[1], bb[2], bb[3], bb[4], bb[5]])
}

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
    /// 从布局 `Piece` 转一条结构件：基类四字段加子类 `extras`（保留官方 NBT 类型），
    /// `Entrances` 按当前包围盒写成 `BoundingBox.CODEC.listOf()` 的 IntArray 列表。
    ///
    /// Convert from a layout `Piece`: the four base fields plus subclass `extras` with
    /// their vanilla NBT types; `Entrances` becomes a list of IntArrays like
    /// `BoundingBox.CODEC.listOf()`.
    pub fn from_layout(piece: &Piece) -> Self {
        let mut extras: Vec<(String, Tag)> = piece
            .extras
            .iter()
            .map(|(key, value)| (key.clone(), value.to_tag()))
            .collect();
        if !piece.entrances.is_empty() {
            extras.push((
                "Entrances".to_owned(),
                Tag::List(piece.entrances.iter().map(entrances_tag).collect()),
            ));
        }
        Self {
            id: piece.id.clone(),
            bb: BoundingBox {
                min_x: piece.bb[0],
                min_y: piece.bb[1],
                min_z: piece.bb[2],
                max_x: piece.bb[3],
                max_y: piece.bb[4],
                max_z: piece.bb[5],
            },
            orientation: piece.o,
            gen_depth: piece.gd,
            extras,
        }
    }

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

/// 一个结构件从存档读回的结果：官方 `StructurePiece` 构造器能恢复的字段。
///
/// One piece read back from save data: the fields a vanilla `StructurePiece`
/// constructor recovers.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPiece {
    /// `BuiltInRegistries.STRUCTURE_PIECE` 注册名（已按 `RENAMES` 归一）。
    pub id: String,
    pub bb: BoundingBox,
    /// `tag.getIntOr("O", 0)` 解出的 2D 值；-1 表示无朝向。
    pub orientation: i32,
    pub gen_depth: i32,
    /// 子类 `addAdditionalSaveData` 的键（读回原样，供放置里程碑使用）。
    pub extras: BTreeMap<String, Tag>,
}

/// `PiecesContainer.load` 的 `RENAMES`：老的 jigsaw 结构件 id 统一成 `minecraft:jigsaw`。
///
/// The `RENAMES` table of `PiecesContainer.load`: old jigsaw piece ids all map to
/// `minecraft:jigsaw`.
const RENAMES: [(&str, &str); 4] = [
    ("minecraft:nvi", "minecraft:jigsaw"),
    ("minecraft:pcp", "minecraft:jigsaw"),
    ("minecraft:bastionremnant", "minecraft:jigsaw"),
    ("minecraft:runtime", "minecraft:jigsaw"),
];

fn compound_field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn int_field(tag: &Tag, key: &str) -> Option<i32> {
    match compound_field(tag, key) {
        Some(Tag::Int(v)) => Some(*v),
        Some(Tag::Byte(v)) => Some(i32::from(*v)),
        Some(Tag::Short(v)) => Some(i32::from(*v)),
        _ => None,
    }
}

fn string_field(tag: &Tag, key: &str) -> Option<String> {
    match compound_field(tag, key) {
        Some(Tag::String(v)) => Some(v.clone()),
        _ => None,
    }
}

/// `tag.read("BB", BoundingBox.CODEC)`：6 个 int 的 IntArray。
///
/// `tag.read("BB", BoundingBox.CODEC)`: a 6-int IntArray.
fn bounding_box_field(tag: &Tag) -> Option<BoundingBox> {
    let Tag::IntArray(values) = compound_field(tag, "BB")? else {
        return None;
    };
    if values.len() != 6 {
        return None;
    }
    Some(BoundingBox {
        min_x: values[0],
        min_y: values[1],
        min_z: values[2],
        max_x: values[3],
        max_y: values[4],
        max_z: values[5],
    })
}

/// `StructurePiece(type, tag)` 基类构造器：`GD`（缺省 0）、`BB`（必需）、`O`（缺省 0，
/// -1 表示无朝向）。子类字段留待 `apply_subclass_load`。
///
/// The `StructurePiece(type, tag)` base constructor: `GD` (default 0), `BB`
/// (required) and `O` (default 0, -1 for none). Subclass fields are left to
/// `apply_subclass_load`.
pub fn load_piece(tag: &Tag) -> Result<LoadedPiece, String> {
    let bb = bounding_box_field(tag).ok_or("structure piece has no valid BB")?;
    // 官方 `Identifier.parse`：没有命名空间的 id 补 `minecraft:` 前缀。
    // Vanilla `Identifier.parse`: ids without a namespace get the `minecraft:` prefix.
    let raw_id = string_field(tag, "id").unwrap_or_default().to_lowercase();
    let namespaced = if raw_id.contains(':') {
        raw_id
    } else {
        format!("minecraft:{raw_id}")
    };
    let id = RENAMES
        .iter()
        .find(|(old, _)| *old == namespaced)
        .map_or(namespaced.clone(), |(_, new)| (*new).to_owned());
    let mut extras = BTreeMap::new();
    if let Tag::Compound(entries) = tag {
        for (key, value) in entries {
            if !matches!(key.as_str(), "id" | "BB" | "O" | "GD") {
                extras.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(LoadedPiece {
        id,
        bb,
        orientation: int_field(tag, "O").unwrap_or(0),
        gen_depth: int_field(tag, "GD").unwrap_or(0),
        extras,
    })
}

/// 已知的结构件类型 id（`BuiltInRegistries.STRUCTURE_PIECE`），用于判「官方能不能读」。
/// `PiecesContainer.load` 对未知 id 只打日志并跳过该结构件。
///
/// **版本相关的硬编码表**：`worldgen.json` 目前没有导出 `structure_piece` 注册表，
/// 所以这张表只能手写；换 jar 版本要重新核对（`BuiltInRegistries.STRUCTURE_PIECE`
/// 的键）。
///
/// Known structure piece type ids (`BuiltInRegistries.STRUCTURE_PIECE`), used to
/// tell whether vanilla could read a piece. `PiecesContainer.load` logs and skips
/// unknown ids.
///
/// **Hard-coded version-dependent table**: `worldgen.json` does not export the
/// `structure_piece` registry yet, so this list is written by hand; re-check it
/// against `BuiltInRegistries.STRUCTURE_PIECE` when the jar version changes.
pub const KNOWN_PIECE_IDS: &[&str] = &[
    // 废弃矿井 / 要塞 / 海底神殿 / 林地府邸：本里程碑负责的四个多件式结构。
    // Mineshaft / stronghold / ocean monument / woodland mansion: the four
    // multi-piece structures owned by this milestone.
    "minecraft:mscorridor",
    "minecraft:mscrossing",
    "minecraft:msroom",
    "minecraft:msstairs",
    "minecraft:shcc",
    "minecraft:shfc",
    "minecraft:sh5c",
    "minecraft:shlt",
    "minecraft:shli",
    "minecraft:shpr",
    "minecraft:shph",
    "minecraft:shrt",
    "minecraft:shrc",
    "minecraft:shsd",
    "minecraft:shstart",
    "minecraft:shs",
    "minecraft:shssd",
    "minecraft:omb",
    "minecraft:omcr",
    "minecraft:omdxr",
    "minecraft:omdxyr",
    "minecraft:omdyr",
    "minecraft:omdyzr",
    "minecraft:omdzr",
    "minecraft:omentry",
    "minecraft:ompenthouse",
    "minecraft:omsimple",
    "minecraft:omsimplet",
    "minecraft:omwr",
    "minecraft:wmp",
    // 结构队员已做的四个代码生成单件结构 + jigsaw。
    // The four code-built single-piece structures and jigsaw, owned by others.
    "minecraft:tedp",
    "minecraft:tejp",
    "minecraft:tesh",
    "minecraft:btp",
    "minecraft:jigsaw",
];

/// `PiecesContainer.load(children, context)`：逐条读结构件，未知 id 跳过（官方打日志
/// 后跳过），坏标签也跳过（官方 `catch` 后跳过）。返回能读出的结构件列表。
///
/// `PiecesContainer.load(children, context)`: read each piece, skipping unknown ids
/// (vanilla logs and skips) and malformed tags (vanilla catches and skips). Returns
/// the pieces that could be read.
pub fn load_pieces(children: &[Tag]) -> Vec<LoadedPiece> {
    let mut pieces = Vec::with_capacity(children.len());
    for tag in children {
        let Ok(piece) = load_piece(tag) else {
            continue;
        };
        if !KNOWN_PIECE_IDS.contains(&piece.id.as_str()) {
            continue;
        }
        pieces.push(piece);
    }
    pieces
}

/// `StructureStart.loadStaticStart(context, tag, seed)` 的纯数据部分：读 `id` /
/// `ChunkX` / `ChunkZ` / `references` / `Children`，再把 `Children` 读成结构件列表。
/// `id = "INVALID"` 返回 `None`（官方返回 `StructureStart.INVALID_START`）。
///
/// Pure-data part of `StructureStart.loadStaticStart(context, tag, seed)`: read `id`
/// / `ChunkX` / `ChunkZ` / `references` / `Children`, then load `Children` into
/// pieces. `id = "INVALID"` yields `None` (vanilla returns
/// `StructureStart.INVALID_START`).
pub fn load_start(tag: &Tag) -> Option<LoadedStart> {
    let id = string_field(tag, "id").unwrap_or_default();
    if id == "INVALID" {
        return None;
    }
    let children = match compound_field(tag, "Children") {
        Some(Tag::List(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    Some(LoadedStart {
        structure: id,
        chunk_x: int_field(tag, "ChunkX").unwrap_or(0),
        chunk_z: int_field(tag, "ChunkZ").unwrap_or(0),
        references: int_field(tag, "references").unwrap_or(0),
        pieces: load_pieces(&children),
    })
}

/// 一个读回的结构起点。
///
/// A structure start read back from save data.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedStart {
    pub structure: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub references: i32,
    pub pieces: Vec<LoadedPiece>,
}

/// `StructureStart.loadStaticStart` 里 `OceanMonumentStructure.regeneratePiecesAfterLoad`
/// 的移植：读回的海底神殿只有存档 `BB` / 朝向，房间图要从世界种子 + 区块坐标确定性重建。
///
/// Port of `OceanMonumentStructure.regeneratePiecesAfterLoad` inside
/// `StructureStart.loadStaticStart`: a reloaded monument keeps only the saved `BB`
/// and orientation, so the room graph is rebuilt deterministically from the world
/// seed and the chunk position.
pub fn regenerate_monument(chunk_x: i32, chunk_z: i32, seed: i64, saved: &[Piece]) -> Vec<Piece> {
    use crate::structure::pieces::ocean_monument;

    if saved.is_empty() {
        return Vec::new();
    }
    let random = crate::structure::pieces::make_random(seed, chunk_x, chunk_z);
    let old = &saved[0];
    let west = old.bb[0];
    let north = old.bb[2];
    // 官方即便用存档朝向也要先取一次 getRandomDirection，保持取数位置对齐。
    // Vanilla draws getRandomDirection even when the saved orientation wins, so the
    // random source is aligned with the original MonumentBuilding construction.
    let default_dir = HDir::random(&random);
    let dir = if old.o < 0 {
        default_dir
    } else {
        HDir::from_2d(old.o)
    };
    ocean_monument::monument_layout(&random, west, north, dir).pieces()
}

/// 把 `LoadedPiece` 映射成布局 `Piece`，供 `regenerate_monument` 用（只需要 `bb` / `o`）。
///
/// Map a `LoadedPiece` into a layout `Piece`, which is all `regenerate_monument`
/// needs (`bb` and `o`).
pub fn loaded_piece_as_layout(piece: &LoadedPiece) -> Piece {
    let mut layout = Piece::plain(&piece.id, [0; 6], piece.gen_depth);
    layout.bb = [
        piece.bb.min_x,
        piece.bb.min_y,
        piece.bb.min_z,
        piece.bb.max_x,
        piece.bb.max_y,
        piece.bb.max_z,
    ];
    layout.o = piece.orientation;
    layout
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

    #[test]
    fn layout_extras_keep_official_nbt_types() {
        use crate::structure::pieces::{Extra, HDir, Piece};
        let mut piece = Piece::plain("minecraft:mscrossing", [0, 50, 0, 2, 52, 4], 0);
        piece.put_bool("tf", true);
        // 官方 `D` 走 `Direction.LEGACY_ID_CODEC_2D`（`Codec.BYTE`）。
        // Vanilla writes `D` through `Direction.LEGACY_ID_CODEC_2D` (a `Codec.BYTE`).
        piece.put_byte("D", HDir::North.to_2d());
        piece.put_int("MST", 1);
        piece.put_str("Template", "1x1_a1");

        let save = SavePiece::from_layout(&piece);
        let fields = |key: &str| {
            save.extras
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(piece.extras["tf"], Extra::Byte(1));
        assert_eq!(fields("tf"), Tag::Byte(1));
        assert_eq!(fields("D"), Tag::Byte(2));
        assert_eq!(fields("MST"), Tag::Int(1));
        assert_eq!(fields("Template"), Tag::String("1x1_a1".to_owned()));
    }

    #[test]
    fn entrances_become_a_list_of_int_arrays() {
        use crate::structure::pieces::Piece;
        let mut piece = Piece::plain("minecraft:msroom", [0, 50, 0, 7, 54, 7], 0);
        piece.entrances = vec![[1, 51, 0, 3, 53, 1], [4, 51, 6, 6, 53, 7]];
        let save = SavePiece::from_layout(&piece);
        let entrances = save
            .extras
            .iter()
            .find(|(k, _)| k == "Entrances")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(
            entrances,
            Tag::List(vec![
                Tag::IntArray(vec![1, 51, 0, 3, 53, 1]),
                Tag::IntArray(vec![4, 51, 6, 6, 53, 7]),
            ])
        );
    }

    #[test]
    fn load_piece_recovers_base_fields_and_extras() {
        let tag = Tag::Compound(vec![
            ("id".to_owned(), Tag::String("MSCorridor".to_owned())),
            ("BB".to_owned(), Tag::IntArray(vec![1, 64, 2, 21, 78, 22])),
            ("O".to_owned(), Tag::Int(2)),
            ("GD".to_owned(), Tag::Int(3)),
            ("hr".to_owned(), Tag::Byte(1)),
            ("Num".to_owned(), Tag::Int(4)),
        ]);
        let piece = load_piece(&tag).unwrap();
        assert_eq!(piece.id, "minecraft:mscorridor");
        assert_eq!(piece.bb, bb());
        assert_eq!(piece.orientation, 2);
        assert_eq!(piece.gen_depth, 3);
        assert_eq!(piece.extras["hr"], Tag::Byte(1));
        assert_eq!(piece.extras["Num"], Tag::Int(4));
        assert!(!piece.extras.contains_key("BB"));
    }

    #[test]
    fn load_piece_applies_the_jigsaw_renames() {
        let tag = |id: &str| {
            Tag::Compound(vec![
                ("id".to_owned(), Tag::String(id.to_owned())),
                ("BB".to_owned(), Tag::IntArray(vec![0, 0, 0, 1, 1, 1])),
            ])
        };
        for old in ["minecraft:nvi", "minecraft:pcp", "minecraft:runtime"] {
            assert_eq!(load_piece(&tag(old)).unwrap().id, "minecraft:jigsaw");
        }
    }

    #[test]
    fn load_pieces_skips_unknown_and_malformed_entries() {
        let good = Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:shstart".to_owned())),
            ("BB".to_owned(), Tag::IntArray(vec![0, 0, 0, 1, 1, 1])),
        ]);
        let unknown = Tag::Compound(vec![
            (
                "id".to_owned(),
                Tag::String("minecraft:no_such_piece".to_owned()),
            ),
            ("BB".to_owned(), Tag::IntArray(vec![0, 0, 0, 1, 1, 1])),
        ]);
        // 没有 BB：官方 `orElseThrow` 后 `PiecesContainer.load` 的 catch 跳过。
        // No BB: vanilla `orElseThrow`s and `PiecesContainer.load` catches and skips.
        let malformed = Tag::Compound(vec![(
            "id".to_owned(),
            Tag::String("minecraft:shs".to_owned()),
        )]);
        let pieces = load_pieces(&[good, unknown, malformed]);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].id, "minecraft:shstart");
    }

    #[test]
    fn load_start_returns_none_for_invalid_and_reads_children() {
        let invalid = Tag::Compound(vec![("id".to_owned(), Tag::String("INVALID".to_owned()))]);
        assert!(load_start(&invalid).is_none());

        let valid = Tag::Compound(vec![
            (
                "id".to_owned(),
                Tag::String("minecraft:stronghold".to_owned()),
            ),
            ("ChunkX".to_owned(), Tag::Int(3)),
            ("ChunkZ".to_owned(), Tag::Int(-9)),
            ("references".to_owned(), Tag::Int(1)),
            (
                "Children".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    ("id".to_owned(), Tag::String("minecraft:shpr".to_owned())),
                    ("BB".to_owned(), Tag::IntArray(vec![0, 0, 0, 1, 1, 1])),
                    ("O".to_owned(), Tag::Int(1)),
                    ("GD".to_owned(), Tag::Int(2)),
                    ("Mob".to_owned(), Tag::Byte(0)),
                ])]),
            ),
        ]);
        let start = load_start(&valid).unwrap();
        assert_eq!(start.structure, "minecraft:stronghold");
        assert_eq!((start.chunk_x, start.chunk_z, start.references), (3, -9, 1));
        assert_eq!(start.pieces.len(), 1);
        assert_eq!(start.pieces[0].id, "minecraft:shpr");
    }

    #[test]
    fn regenerate_monument_matches_generation_from_save_fields() {
        use crate::structure::pieces::ocean_monument;

        for seed in [0_i64, 42, -7, 12_345_678_901] {
            for (cx, cz) in [(0, 0), (1, -1), (34, 50)] {
                let generated = ocean_monument::generate(seed, cx, cz);
                // 存档只保留第 0 项（MonumentBuilding）。
                // Saving keeps only index 0 (the MonumentBuilding).
                let saved: Vec<Piece> = vec![generated[0].clone()];
                let rebuilt = regenerate_monument(cx, cz, seed, &saved);
                assert_eq!(
                    rebuilt, generated,
                    "seed {seed} chunk {cx},{cz}: rebuilt room graph differs"
                );
            }
        }
    }

    #[test]
    fn regenerate_monument_uses_the_saved_orientation() {
        use crate::structure::pieces::{HDir, ocean_monument};

        let generated = ocean_monument::generate(7, 3, -5);
        let mut saved = vec![generated[0].clone()];
        // 存档朝向换成相反方向：官方用存档朝向建 MonumentBuilding，但取数位置不变。
        // Swap the saved orientation: vanilla builds with it while the draw position
        // stays aligned.
        saved[0].o = HDir::from_2d(saved[0].o).opposite().to_2d();
        let rebuilt = regenerate_monument(3, -5, 7, &saved);
        assert_eq!(rebuilt[0].o, saved[0].o);
        assert_ne!(rebuilt[0].o, generated[0].o);
        // 房间图仍按同一随机序列重建，件数不变，但包围盒跟着朝向变。
        // The room graph is still rebuilt from the same draws: same piece count, boxes follow.
        assert_eq!(rebuilt.len(), generated.len());
        assert_ne!(rebuilt[1].bb, generated[1].bb);
    }

    #[test]
    fn regenerate_monument_falls_back_when_the_saved_orientation_is_missing() {
        use crate::structure::pieces::ocean_monument;

        let generated = ocean_monument::generate(11, 2, 4);
        let mut saved = vec![generated[0].clone()];
        saved[0].o = -1;
        let rebuilt = regenerate_monument(2, 4, 11, &saved);
        // `O = -1` 时官方 `getOrientation()` 为 null，退回本次取到的方向。
        // With `O = -1` vanilla's `getOrientation()` is null, so the drawn direction wins.
        assert!((0..4).contains(&rebuilt[0].o));
        assert_eq!(rebuilt.len(), generated.len());
    }

    #[test]
    fn regenerate_monument_of_nothing_stays_empty() {
        assert!(regenerate_monument(0, 0, 0, &[]).is_empty());
    }
}
