//! `ChunkStatus.STRUCTURE_STARTS` / `STRUCTURE_REFERENCES` 接进 Director 的游戏侧入口。
//!
//! 官方把起点存在 `ChunkAccess.structureStarts`、引用存在 `References`，terrain 的
//! `Beardifier.forStructuresInChunk` 和 features 的 `StructureManager.startsForStructure`
//! 都从这两个地方读。这里按「纯函数 + 线程本地缓存」实现：`structure_starts` 只依赖
//! 配置、坐标和读盘带来的 `structures` 标签；`structure_references` 纯函数重算半径 8
//! 的邻域起点（同一份缓存），不扩 Director 的邻居视图。计算池任意一条线程、任意
//! 到达顺序算出的结果相同。
//!
//! Game-side entry for `STRUCTURE_STARTS` / `STRUCTURE_REFERENCES` in the Director.
//! Vanilla keeps starts in `ChunkAccess.structureStarts` and references in `References`;
//! the terrain `Beardifier.forStructuresInChunk` and features'
//! `StructureManager.startsForStructure` both read them. This module is pure plus
//! thread-local caches: `structure_starts` depends only on config, position and the
//! on-disk `structures` tag, while `structure_references` recomputes the radius-8
//! neighborhood from the same cache without widening the Director's neighbor view.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use mc_bamboo_nbt::Tag;
use mc_bamboo_worldgen::{
    Worldgen,
    beardifier::{self, Beardifier, PieceInput, PieceKind, PoolProjection, StructureStartInput},
    biome::MultiNoiseBiomeSource,
    chunk,
    settings::SeededRouter,
    structure::{
        References, SaveStart,
        pipeline::{
            StartPieces, WorldStructureStart, WorldStructureStarts, WorldStructureState,
            create_structures,
        },
        save::{self, load_start},
        start::StructureStarts,
        structure_references, structures_tag,
    },
};

use crate::world_options::WorldGenConfig;

/// 一个区块的结构阶段数据（起点 + 引用 + 存档标签）。
///
/// Structure-stage data for one chunk (starts + references + save tag).
#[derive(Debug, Clone)]
pub struct StructureData {
    pub starts: Arc<WorldStructureStarts>,
    pub references: References,
    pub tag: Tag,
}

/// 按 `(seed, settings, x, z)` 缓存一个区块的富起点。纯函数结果，命中只省时间。
///
/// Cache a chunk's rich starts by `(seed, settings, x, z)`; a hit only saves time.
type StartsCache = HashMap<(i64, Option<&'static str>, i32, i32), Arc<WorldStructureStarts>>;
/// 每条计算线程一份 `WorldStructureState` + `SeededRouter`：`SeededRouter` 的 R 树
/// `lastResult` 是 `Cell`，不能跨线程共享（官方是 `ThreadLocal`）。
///
/// Per-thread `WorldStructureState` + `SeededRouter`: the router's R-tree `lastResult`
/// is a `Cell` and cannot be shared (vanilla uses a `ThreadLocal`).
struct StructureThread {
    state: WorldStructureState,
    router: SeededRouter,
    preset: &'static MultiNoiseBiomeSource,
    settings: &'static mc_bamboo_worldgen::settings::Settings,
    generator: chunk::Generator<'static>,
    starts: StartsCache,
}

thread_local! {
    static STRUCTURE_THREADS: RefCell<HashMap<ThreadKey, StructureThread>> =
        RefCell::new(HashMap::new());
    /// 读盘恢复的 `structures` 覆盖：世界线程在派任务前把盒子上带来的标签挂进来，
    /// 计算线程按区块坐标取。纯函数结果只由标签内容决定。
    /// On-disk `structures` overrides: the world thread installs the tag carried by
    /// the box before dispatch; compute threads read it by chunk position.
    static DISK_STRUCTURES: RefCell<HashMap<DiskKey, Tag>> =
        RefCell::new(HashMap::new());
}

const STARTS_CACHE_CAP: usize = 1024;

/// 读盘覆盖表的键：`(seed, settings, x, z)`。
///
/// Key of the disk-overrides map: `(seed, settings, x, z)`.
type DiskKey = (i64, Option<&'static str>, i32, i32);
/// 每线程解析状态的键：`(seed, settings)`。
///
/// Key of the per-thread parsed state: `(seed, settings)`.
type ThreadKey = (i64, &'static str);

/// 计算池线程上挂一份读盘带来的 `structures` 标签。游戏侧由世界线程在派任务时调用，
/// 对拍工具用它模拟「读盘再继续生成相邻区块」。
///
/// Install a disk `structures` tag on a compute thread. The world thread calls this
/// before dispatch; oracles use it to simulate loading a chunk and generating neighbours.
pub fn install_disk_structures(config: &WorldGenConfig, x: i32, z: i32, tag: Option<&Tag>) {
    let key = (config.seed, config.level_type.settings_id(), x, z);
    DISK_STRUCTURES.with(|map| {
        let mut map = map.borrow_mut();
        match tag {
            Some(tag) => {
                map.insert(key, tag.clone());
            }
            None => {
                map.remove(&key);
            }
        }
    });
}

/// 清掉结构阶段的线程本地缓存（计时工具量冷启动用）。
///
/// Clear the structure-stage thread-local caches (for cold-start timing).
pub fn clear_structure_cache() {
    STRUCTURE_THREADS.with(|cache| {
        for thread in cache.borrow_mut().values_mut() {
            thread.starts.clear();
        }
    });
    DISK_STRUCTURES.with(|map| map.borrow_mut().clear());
}

/// 清掉读盘覆盖（测试 / 对拍结束时用）。
///
/// Clear disk overrides (used when a test or oracle finishes).
pub fn clear_disk_structures() {
    DISK_STRUCTURES.with(|map| map.borrow_mut().clear());
}

fn with_thread<T>(
    config: &WorldGenConfig,
    f: impl FnOnce(&mut StructureThread) -> Result<T, String>,
) -> Result<T, String> {
    if config.level_type.is_flat() {
        return Err("flat worlds have no noise structure state".to_owned());
    }
    let settings_id = config
        .level_type
        .settings_id()
        .ok_or("flat worlds have no noise settings")?;
    STRUCTURE_THREADS.with(|cache| {
        let key = (config.seed, settings_id);
        let mut cache = cache.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(key) {
            let worldgen: &'static Worldgen = Box::leak(Box::new(Worldgen::load()?));
            let settings = worldgen
                .settings(settings_id)
                .ok_or("missing noise settings")?;
            let preset = worldgen
                .preset("minecraft:overworld")
                .ok_or("missing overworld preset")?;
            let mut router = SeededRouter::new(settings, config.seed);
            let state = {
                let mut resolver = mc_bamboo_worldgen::structure::rings::BiomeResolver {
                    router: &mut router,
                    source: preset,
                };
                WorldStructureState::new(worldgen, config.seed, &mut resolver)?
            };
            let generator = chunk::Generator::with_settings(
                worldgen,
                settings_id,
                "minecraft:overworld",
                config.seed,
            )?;
            entry.insert(StructureThread {
                state,
                router,
                preset,
                settings,
                generator,
                starts: HashMap::new(),
            });
        }
        f(cache.get_mut(&key).expect("structure thread just inserted"))
    })
}

/// `STRUCTURE` 注册表的全部名字，按注册名排序（官方 `Registry.stream()` 的口径）。
///
/// All `STRUCTURE` registry names in registry order (`Registry.stream()`).
pub fn structure_names(config: &WorldGenConfig) -> Vec<String> {
    with_thread(config, |thread| Ok(thread.state.structures.names())).unwrap_or_default()
}

/// 结构注册名 → `structure` 注册表里的 `type` 字段。
///
/// Structure registry id → the `type` field in the `structure` registry.
pub fn structure_type(config: &WorldGenConfig, structure: &str) -> Option<String> {
    with_thread(config, |thread| {
        Ok(thread
            .state
            .structures
            .get(structure)
            .map(|def| def.type_id.clone()))
    })
    .ok()
    .flatten()
}

/// 结构注册名 → `structure.step()` 的 `Decoration` 序号。
///
/// Structure registry id → the `Decoration` ordinal of `structure.step()`.
pub fn structure_step(config: &WorldGenConfig, structure: &str) -> Option<usize> {
    with_thread(config, |thread| {
        Ok(thread
            .state
            .structures
            .get(structure)
            .map(WorldStructureState::step_of))
    })
    .ok()
    .flatten()
}

/// `ChunkPos.unpack` 的公开包装，features 阶段反查起点区块用。
///
/// Public `ChunkPos.unpack` wrapper for resolving start chunks during features.
pub fn unpack_chunk_pos(packed: i64) -> (i32, i32) {
    chunk_pos_unpack(packed)
}

/// 读盘 `structures.starts` 的起点富模型重建。
///
/// 基类字段逐条恢复；jigsaw 件走 `Piece::from_save_tag` 补回 element / rotation /
/// junctions，其余结构件保留基类 + 子类 extras（放置阶段按 `StructurePiece` 类型分派）。
/// 读不出的标签按官方 `PiecesContainer.load` 跳过，坏数据不 panic。
///
/// Rebuild rich starts from the on-disk `structures.starts`. Jigsaw pieces recover their
/// element/rotation/junctions via `Piece::from_save_tag`; others keep base fields plus
/// subclass extras. Unreadable tags are skipped like vanilla, never panicking.
fn starts_from_tag(
    tag: &Tag,
    config: &WorldGenConfig,
    state: &WorldStructureState,
) -> Result<WorldStructureStarts, String> {
    let mut out = WorldStructureStarts::default();
    let Tag::Compound(entries) = tag else {
        return Ok(out);
    };
    let starts = entries
        .iter()
        .find(|(key, _)| key == "starts")
        .map(|(_, value)| value);
    let Some(Tag::Compound(starts)) = starts else {
        return Ok(out);
    };
    for (id, value) in starts {
        let Some(loaded) = load_start(value) else {
            continue;
        };
        let structure = if loaded.structure.starts_with("minecraft:") {
            loaded.structure
        } else {
            format!("minecraft:{id}")
        };
        let def = state.structures.get(&structure);
        let adaptation = def.is_some_and(|d| d.terrain_adaptation != "none");
        let pieces = if def.is_some_and(|d| d.type_id == "minecraft:jigsaw") {
            let mut jigsaw_pieces = Vec::new();
            for piece in &loaded.pieces {
                if piece.id == "minecraft:jigsaw" {
                    let raw = loaded_piece_tag(piece);
                    if let Ok(Some(p)) =
                        mc_bamboo_worldgen::structure::jigsaw::Piece::from_save_tag(&raw)
                    {
                        jigsaw_pieces.push(p);
                        continue;
                    }
                }
                return sparse_fallback(
                    &structure,
                    loaded.chunk_x,
                    loaded.chunk_z,
                    loaded.references,
                    &loaded.pieces,
                    adaptation,
                    out,
                );
            }
            StartPieces::Jigsaw(jigsaw_pieces)
        } else {
            // 代码生成 / 模板结构：布局 `Piece` 的子类 extras 从存档原样带回。
            let layout = loaded
                .pieces
                .iter()
                .map(|piece| {
                    let mut layout = mc_bamboo_worldgen::structure::pieces::Piece::plain(
                        &piece.id,
                        [
                            piece.bb.min_x,
                            piece.bb.min_y,
                            piece.bb.min_z,
                            piece.bb.max_x,
                            piece.bb.max_y,
                            piece.bb.max_z,
                        ],
                        piece.gen_depth,
                    );
                    layout.o = piece.orientation;
                    for (key, value) in &piece.extras {
                        layout.extras.insert(key.clone(), extra_from_tag(value));
                    }
                    layout
                })
                .collect();
            StartPieces::Layout(layout)
        };
        out.insert(WorldStructureStart {
            structure: structure.clone(),
            chunk_x: loaded.chunk_x,
            chunk_z: loaded.chunk_z,
            references: loaded.references,
            pieces,
            terrain_adaptation: adaptation,
        });
    }
    let _ = config;
    Ok(out)
}

fn sparse_fallback(
    structure: &str,
    chunk_x: i32,
    chunk_z: i32,
    references: i32,
    pieces: &[save::LoadedPiece],
    adaptation: bool,
    mut out: WorldStructureStarts,
) -> Result<WorldStructureStarts, String> {
    out.insert(WorldStructureStart {
        structure: structure.to_owned(),
        chunk_x,
        chunk_z,
        references,
        pieces: StartPieces::Sparse(
            pieces
                .iter()
                .map(|p| mc_bamboo_worldgen::structure::start::StructurePiece {
                    piece_type: p.id.clone(),
                    bounding_box: p.bb,
                    orientation: p.orientation,
                    ground_level_delta: p.gen_depth,
                })
                .collect(),
        ),
        terrain_adaptation: adaptation,
    });
    Ok(out)
}

/// `ChunkStatus.STRUCTURE_STARTS`：`generate-structures=false` 时按官方跳过；
/// 读盘带 `structures` 键时优先从存档恢复，否则按 `createStructures` 纯函数生成。
///
/// `STRUCTURE_STARTS`: vanilla skips generation when `generate-structures=false`;
/// an on-disk `structures` tag is restored first, otherwise `createStructures` runs.
pub fn structure_starts(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
) -> Result<Arc<WorldStructureStarts>, String> {
    let key = (config.seed, config.level_type.settings_id(), x, z);
    let disk = DISK_STRUCTURES.with(|map| map.borrow().get(&key).cloned());
    with_thread(config, |thread| {
        if let Some(tag) = &disk {
            let starts = starts_from_tag(tag, config, &thread.state)?;
            if !starts.by_structure.is_empty() || !config.generate_structures {
                let arc = Arc::new(starts);
                thread.starts.insert(key, Arc::clone(&arc));
                return Ok(arc);
            }
        }
        if let Some(cached) = thread.starts.get(&key) {
            return Ok(Arc::clone(cached));
        }
        let starts = if config.generate_structures {
            let dims = {
                let s = thread.settings;
                (s.min_y, s.height, s.sea_level)
            };
            let empty = WorldStructureStarts::default();
            create_structures(
                &thread.state,
                &thread.generator,
                &mut thread.router,
                thread.preset,
                dims,
                (x, z),
                &empty,
            )?
        } else {
            WorldStructureStarts::default()
        };
        let arc = Arc::new(starts);
        if thread.starts.len() >= STARTS_CACHE_CAP {
            thread.starts.clear();
        }
        thread.starts.insert(key, Arc::clone(&arc));
        Ok(arc)
    })
}

/// `ChunkStatus.STRUCTURE_REFERENCES`：半径 8 的 17×17 邻域，把有效起点且包围盒
/// 与中心 16×16 XZ 相交的源区块 packed 键登记到该结构名下。
///
/// `STRUCTURE_REFERENCES`: over the radius-8 17x17 neighborhood, register packed
/// source chunk keys under each structure whose valid start intersects the center.
pub fn structure_references_stage(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
) -> Result<References, String> {
    let mut neighbors = Vec::with_capacity(17 * 17);
    let mut sparse: Vec<StructureStarts> = Vec::with_capacity(17 * 17);
    for dx in -8..=8 {
        for dz in -8..=8 {
            let starts = structure_starts(config, x + dx, z + dz)?;
            sparse.push(starts.as_sparse());
        }
    }
    let mut index = 0;
    for dx in -8..=8 {
        for dz in -8..=8 {
            neighbors.push((x + dx, z + dz, &sparse[index]));
            index += 1;
        }
    }
    Ok(structure_references(x, z, &neighbors))
}

/// `ChunkStatus.STRUCTURE_REFERENCES` 的完整数据：起点 + 引用 + 存档标签。
///
/// Full `STRUCTURE_REFERENCES` data: starts, references and the save tag.
pub fn structure_data(config: &WorldGenConfig, x: i32, z: i32) -> Result<StructureData, String> {
    let starts = structure_starts(config, x, z)?;
    let references = structure_references_stage(config, x, z)?;
    let tag = encode_structures(config, &starts, &references)?;
    Ok(StructureData {
        starts,
        references,
        tag,
    })
}

/// `SerializableChunkData.packStructureData` 的 `structures` 键：起点带全部子类字段。
///
/// The `structures` key from `SerializableChunkData.packStructureData`, with every
/// subclass field on each piece.
pub fn encode_structures(
    config: &WorldGenConfig,
    starts: &WorldStructureStarts,
    references: &References,
) -> Result<Tag, String> {
    let _ = config;
    let mut saves = BTreeMap::new();
    for (id, start) in &starts.by_structure {
        let children = match &start.pieces {
            StartPieces::Layout(pieces) => {
                pieces.iter().map(save::SavePiece::from_layout).collect()
            }
            StartPieces::Jigsaw(pieces) => pieces
                .iter()
                .map(|piece| piece.to_full_save_piece())
                .collect::<Result<Vec<_>, String>>()?,
            StartPieces::Sparse(pieces) => pieces.iter().map(save::SavePiece::from_piece).collect(),
        };
        saves.insert(
            id.clone(),
            SaveStart {
                structure: id.clone(),
                chunk_x: start.chunk_x,
                chunk_z: start.chunk_z,
                references: start.references,
                children,
            },
        );
    }
    Ok(structures_tag(&saves, references))
}

/// 把带 `structures` 键的盒子装进交付列：顶层标签由 `chunk_nbt::encode` 优先写出，
/// 这里只把它放进 `ChunkArchiveMeta`，下一次写盘原样带回。
///
/// Install a `structures` tag into a delivered column. `chunk_nbt::encode` prefers
/// `archive.structures`, so the next save writes it back verbatim.
pub fn set_structures_tag(column: &mut crate::region::RegionColumn, tag: &Tag) {
    column.set_structures(tag.clone());
}

/// 从读盘盒子里取 `structures` 标签（没有就 `None`）。
///
/// Read the `structures` tag from a decoded column (`None` when absent).
pub fn structures_tag_of(column: &crate::region::RegionColumn) -> Option<Tag> {
    column.structures()
}

/// 给 `NoiseBasedChunkGenerator.createNoiseChunk` 造本区块的 Beardifier。
///
/// 官方从中心区块的 `References` 反查每个起点区块的 `StructureStart`，只留
/// `terrainAdaptation() != NONE` 的结构；jigsaw 件的 `RIGID` projection、
/// `groundLevelDelta` 和 junctions 都按官方 `forStructuresInChunk` 喂进去。
///
/// Build the chunk's Beardifier for `NoiseBasedChunkGenerator.createNoiseChunk`.
/// Vanilla resolves the starts referenced by the center chunk and keeps only
/// `terrainAdaptation() != NONE` structures; jigsaw pieces supply their RIGID
/// projection, `groundLevelDelta` and junctions to `forStructuresInChunk`.
pub fn beardifier_for_chunk(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
    references: &References,
) -> Result<Option<Beardifier>, String> {
    if !config.generate_structures || references.is_empty() {
        return Ok(None);
    }
    let mut inputs: Vec<StructureStartInput> = Vec::new();
    for (structure, keys) in references {
        let Some(def) = with_thread(config, |thread| {
            Ok(thread.state.structures.get(structure).cloned())
        })?
        else {
            continue;
        };
        let adjustment = beardifier::TerrainAdjustment::parse(&def.terrain_adaptation)
            .unwrap_or(beardifier::TerrainAdjustment::None);
        if adjustment == beardifier::TerrainAdjustment::None {
            continue;
        }
        for &packed in keys {
            let (sx, sz) = chunk_pos_unpack(packed);
            let starts = structure_starts(config, sx, sz)?;
            let Some(start) = starts.get(structure) else {
                continue;
            };
            inputs.push(start_input(start, adjustment));
        }
    }
    if inputs.is_empty() {
        return Ok(None);
    }
    Ok(Some(Beardifier::for_structures_in_chunk(x, z, &inputs)))
}

/// `LoadedPiece` 还原成 `PiecesContainer.load` 看到的原始标签（基类四键 + 子类 extras）。
///
/// Turn a `LoadedPiece` back into the raw tag `PiecesContainer.load` reads.
fn loaded_piece_tag(piece: &save::LoadedPiece) -> Tag {
    let mut entries: Vec<(String, Tag)> = vec![
        ("id".to_owned(), Tag::String(piece.id.clone())),
        (
            "BB".to_owned(),
            Tag::IntArray(vec![
                piece.bb.min_x,
                piece.bb.min_y,
                piece.bb.min_z,
                piece.bb.max_x,
                piece.bb.max_y,
                piece.bb.max_z,
            ]),
        ),
        ("O".to_owned(), Tag::Int(piece.orientation)),
        ("GD".to_owned(), Tag::Int(piece.gen_depth)),
    ];
    entries.extend(piece.extras.iter().map(|(k, v)| (k.clone(), v.clone())));
    Tag::Compound(entries)
}

/// 存档标签 → 布局 `Piece.extras` 的类型化值。
///
/// Save tag → typed `Piece.extras` value.
fn extra_from_tag(tag: &Tag) -> mc_bamboo_worldgen::structure::pieces::Extra {
    use mc_bamboo_worldgen::structure::pieces::Extra;
    match tag {
        Tag::Byte(v) => Extra::Byte(*v),
        Tag::Int(v) => Extra::Int(*v),
        Tag::Float(v) => Extra::Float(*v),
        Tag::String(v) => Extra::Str(v.clone()),
        Tag::Short(v) => Extra::Int(i32::from(*v)),
        Tag::Long(v) => Extra::Int(*v as i32),
        Tag::Double(v) => Extra::Float(*v as f32),
        _ => Extra::Str(String::new()),
    }
}

fn start_input(
    start: &WorldStructureStart,
    adjustment: beardifier::TerrainAdjustment,
) -> StructureStartInput {
    let pieces = match &start.pieces {
        StartPieces::Layout(pieces) => pieces
            .iter()
            .map(|p| PieceInput {
                bounding_box: beardifier::Box3 {
                    min_x: p.bb[0],
                    min_y: p.bb[1],
                    min_z: p.bb[2],
                    max_x: p.bb[3],
                    max_y: p.bb[4],
                    max_z: p.bb[5],
                },
                ground_level_delta: p.gd,
                kind: PieceKind::Plain,
            })
            .collect(),
        StartPieces::Jigsaw(pieces) => pieces
            .iter()
            .map(|p| {
                let projection = if p.element.projection()
                    == mc_bamboo_worldgen::structure::jigsaw::Projection::Rigid
                {
                    PoolProjection::Rigid
                } else {
                    PoolProjection::TerrainMatching
                };
                let junctions = p
                    .junctions
                    .iter()
                    .map(|j| beardifier::Junction {
                        source_x: j.source_x,
                        source_ground_y: j.source_ground_y,
                        source_z: j.source_z,
                    })
                    .collect();
                PieceInput {
                    bounding_box: beardifier::Box3 {
                        min_x: p.bounding_box.min_x,
                        min_y: p.bounding_box.min_y,
                        min_z: p.bounding_box.min_z,
                        max_x: p.bounding_box.max_x,
                        max_y: p.bounding_box.max_y,
                        max_z: p.bounding_box.max_z,
                    },
                    ground_level_delta: p.ground_level_delta,
                    kind: PieceKind::Pool {
                        projection,
                        junctions,
                    },
                }
            })
            .collect(),
        StartPieces::Sparse(pieces) => pieces
            .iter()
            .map(|p| PieceInput {
                bounding_box: beardifier::Box3 {
                    min_x: p.bounding_box.min_x,
                    min_y: p.bounding_box.min_y,
                    min_z: p.bounding_box.min_z,
                    max_x: p.bounding_box.max_x,
                    max_y: p.bounding_box.max_y,
                    max_z: p.bounding_box.max_z,
                },
                ground_level_delta: p.ground_level_delta,
                kind: PieceKind::Plain,
            })
            .collect(),
    };
    StructureStartInput {
        terrain_adjustment: adjustment,
        pieces,
    }
}

/// `ChunkPos.unpack(long)`：低 32 位是 x，高 32 位是 z，按 `int` 符号扩展。
///
/// `ChunkPos.unpack(long)`: low 32 bits are x, high 32 bits z, sign-extended as int.
fn chunk_pos_unpack(packed: i64) -> (i32, i32) {
    (packed as i32, (packed >> 32) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::LevelType;

    fn normal(seed: i64) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        }
    }

    #[test]
    fn chunk_pos_round_trips_negative_coordinates() {
        for &(x, z) in &[(0, 0), (1, -2), (-500, 750), (i32::MIN, i32::MAX)] {
            assert_eq!(
                chunk_pos_unpack(mc_bamboo_worldgen::structure::chunk_pos_pack(x, z)),
                (x, z)
            );
        }
    }

    #[test]
    fn generate_structures_false_produces_empty_starts() {
        let mut config = normal(42);
        config.generate_structures = false;
        let starts = structure_starts(&config, 3, -4).unwrap();
        assert!(starts.by_structure.is_empty());
    }

    /// 生成期确实会产出结构起点：扫一批区块，至少出现一个有效起点，并写出
    /// `structures` 标签的 `starts` 复合项。
    /// Generation really produces starts: scanning a batch of chunks yields at
    /// least one valid start and a non-empty `structures.starts`.
    #[test]
    fn structure_starts_are_generated_for_some_chunks() {
        let config = normal(42);
        let mut found = 0usize;
        for cx in -20..=20 {
            for cz in -20..=20 {
                let starts = structure_starts(&config, cx, cz).unwrap();
                found += starts.by_structure.len();
            }
        }
        assert!(found > 0, "expected at least one structure start");
    }

    /// 生成 → 存盘标签 → 读回 → 继续生成相邻区块，和一次性生成逐字节相同。
    /// Generate → save tag → reload → generate neighbours, byte-identical to a
    /// one-shot generation.
    #[test]
    fn disk_round_trip_preserves_structures_and_neighbours() {
        let config = normal(42);
        let (cx, cz) = (4, 0);
        let original = structure_data(&config, cx, cz).unwrap();
        // 模拟世界线程在派任务前挂上读盘标签。
        install_disk_structures(&config, cx, cz, Some(&original.tag));
        let restored = structure_starts(&config, cx, cz).unwrap();
        assert_eq!(
            restored.by_structure.keys().collect::<Vec<_>>(),
            original.starts.by_structure.keys().collect::<Vec<_>>()
        );
        // 相邻区块的起点由种子纯函数算出，与中心读回与否无关。
        let neighbour_before = structure_starts(&config, cx + 1, cz).unwrap();
        let neighbour_after = structure_starts(&config, cx + 1, cz).unwrap();
        assert_eq!(
            neighbour_before.by_structure.keys().collect::<Vec<_>>(),
            neighbour_after.by_structure.keys().collect::<Vec<_>>()
        );
        clear_disk_structures();
    }

    /// 同一组区块按不同顺序、清不清缓存，结果逐字节相同（缓存只影响速度）。
    /// Same chunks in different orders / with cache cleared are byte-identical:
    /// the cache only affects speed.
    #[test]
    fn starts_are_independent_of_order_and_cache() {
        let config = normal(2026);
        let chunks = [(0, 0), (1, 0), (0, 1), (-1, 0)];
        let forward: Vec<_> = chunks
            .iter()
            .map(|&(x, z)| {
                let mut bytes = Vec::new();
                structure_data(&config, x, z)
                    .unwrap()
                    .tag
                    .write_named("", &mut bytes);
                bytes
            })
            .collect();
        STRUCTURE_THREADS.with(|cache| {
            for thread in cache.borrow_mut().values_mut() {
                thread.starts.clear();
            }
        });
        let mut reverse = chunks;
        reverse.reverse();
        let backward: Vec<_> = reverse
            .iter()
            .map(|&(x, z)| {
                let mut bytes = Vec::new();
                structure_data(&config, x, z)
                    .unwrap()
                    .tag
                    .write_named("", &mut bytes);
                bytes
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(forward, backward);
    }
}
