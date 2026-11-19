//! 「盘上半成品区块读回 → 按 `Status` 续跑 → 完工」的对拍入口。
//!
//! 正式加载走 `Director` 的 `TaskResult::Decoded { column, status }`，这条重放把
//! 同一批阶段纯函数按官方 `ChunkStep.apply` 的顺序跑一遍：只跑盘上状态之后的
//! 阶段，盘上已有的数据按状态恢复。只在 `oracle-samples` 下编进库里，
//! `mc-bamboo-vanilla` 的 `resume-world` 用它和官方一口气生成的 `full` 区块逐格比。
//!
//! Replay of "read a non-full chunk from disk → resume from its status → finish".
//! The real load path uses `Director`'s `TaskResult::Decoded { column, status }`;
//! this runs the same stage pure functions in vanilla `ChunkStep.apply` order,
//! from the stage after the on-disk one, restoring intermediate results by stage.
//! Gated behind `oracle-samples` for the `resume-world` oracle.

use std::{collections::BTreeMap, sync::Arc};

use mc_bamboo_core::{ChunkPos, LightLayer};
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::chunk_nbt::{self, ChunkEncodeExtra};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT, SECTION_SIZE, WORLD_HEIGHT,
    chunk::{ChunkArchiveMeta, HeightmapKind},
    light::{DataLayer, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos},
    ticket::ChunkStatus,
};

use crate::{
    region::{RegionColumn, chunk_content},
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{
        GenerateOutput, assemble_feature_column, generate_features_stage, generate_terrain_stage,
        passthrough_stage,
    },
    worldgen_light::generate_light_stage,
    worldgen_structures::{install_disk_structures, set_structures_tag},
};

/// 一条对拍用例：一个区块的盘上 NBT（base64 解码后的未压缩字节）和钉住的状态。
#[derive(Debug, Clone)]
pub struct ResumeCase {
    pub pos: ChunkPos,
    /// 官方 `SerializableChunkData.write()` 输出的未压缩 NBT 字节。
    pub chunk_nbt: Vec<u8>,
    /// 钉在盘上的 `Status`（官方写盘时用的阶段）。
    pub stage: ChunkStatus,
}

/// 完工区块里要逐格比较的数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedChunk {
    /// 逐层方块状态 ID，`WORLD_HEIGHT × 256`，层内 `x*16+z`。
    pub blocks: Vec<Vec<u32>>,
    /// 每段 64 个 quart 群系，顺序 `x → y → z`。
    pub biomes: Vec<Vec<u32>>,
    /// 四张 FINAL 高度图，每张 256 列的世界高度（`getFirstAvailable`）。
    pub heightmaps: BTreeMap<&'static str, Vec<i32>>,
    /// 天空光 / 方块光每段的 2048 字节；`None` 表示官方不存这一段。
    pub sky: Vec<Option<Vec<u8>>>,
    pub block_light: Vec<Option<Vec<u8>>>,
    pub light_on: bool,
    /// `(世界坐标, SNBT)`，按坐标排序。
    pub block_entities: Vec<(String, String)>,
    /// 顶层 `structures`（`starts` + `References`）的 SNBT。
    pub structures: String,
    /// 下一次写盘写出的 `Status`。
    pub saved_status: ChunkStatus,
    /// 下一次写盘写出的未压缩 NBT 字节，供「Bamboo 写 → Bamboo 读」往返用。
    pub saved_nbt: Vec<u8>,
}

/// 故意改坏续跑：把读回的状态当成 `full` 直接交付，或者丢掉邻居写进中心的地物增量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeBreak {
    /// 把盘上 `Status` 当成 `full`，不续跑后面的阶段。
    TreatAsFull,
    /// 丢掉 8 个邻居中心溢进中心的写增量（只留中心自己的）。
    DropNeighborWrites,
}

/// 读回一份盘上的区块，按它的 `Status` 续跑到 `full`，返回完工数据。
///
/// `neighbor_tags` 是邻居区块的盘上 `structures` 标签（键是坐标），模拟结构起点 /
/// 引用跨区块恢复；`break_kind` 只给改坏确认用，正常路径传 `None`。
///
/// Decode a chunk, resume generation from its on-disk `Status` to `full`, and
/// return the finished data. `neighbor_tags` supplies neighbouring chunks' on-disk
/// `structures` tags (cross-chunk starts/references). `break_kind` is only for
/// break confirmation.
pub fn resume_chunk(
    config: &WorldGenConfig,
    case: &ResumeCase,
    neighbor_tags: &BTreeMap<ChunkPos, Tag>,
    break_kind: Option<ResumeBreak>,
) -> Result<ResumedChunk, String> {
    let bytes = &case.chunk_nbt;
    let decoded = chunk_nbt::decode_bytes(bytes)
        .map_err(|error| format!("decode {:?}: {error}", case.pos))?;
    let disk_status = decoded.persisted_status;
    // 官方 `SerializableChunkData.read` 把盘上状态装进 ProtoChunk；这里和
    // `TaskResult::Decoded` 一样把它交给流水线，只有 full 直接交付。
    // Vanilla's `read` installs the persisted status into the ProtoChunk;
    // this hands it to the pipeline just like `TaskResult::Decoded`.
    if disk_status != case.stage {
        return Err(format!(
            "{:?}: disk status {:?} disagrees with oracle stage {:?}",
            case.pos, disk_status, case.stage
        ));
    }
    // 读盘的两份 NBT 一起走正式的 `chunk_content::decode` 组装，拿到
    // `RegionColumn`（含方块实体 / 计划刻 / 实体），和 Director 的加载路径一致。
    // Use the real `chunk_content::decode` for both NBT files so upper-layer
    // content matches the Director's load path.
    let (_, entries) =
        mc_bamboo_nbt::read_named(bytes, mc_bamboo_nbt::Accounter::uncompressed_quota())
            .map_err(|error| format!("read named NBT {:?}: {error}", case.pos))?;
    let root = Tag::Compound(entries);
    let decoded_chunk = chunk_content::decode(case.pos, &root, None)
        .map_err(|error| format!("decode {:?}: {error}", case.pos))?;
    let mut column = *decoded_chunk.column;
    // 读盘带来的 `structures` 挂进结构阶段的线程本地覆盖：纯函数重算地形时
    // 要从盘上的起点恢复 Beardifier 和引用，而不是按种子另造一份。
    // Restore the disk structures tag so pure terrain/feature stages rebuild
    // Beardifier and references from the saved starts.
    install_disk_structures(config, case.pos.x, case.pos.z, Some(&decoded.structures));
    for (&pos, tag) in neighbor_tags {
        install_disk_structures(config, pos.x, pos.z, Some(tag));
    }

    // 按阶段恢复中间结果：terrain 之前的盘上数据只是结构 / 群系，不需要
    // CARVERS 快照；terrain 及之后把盘上方块还原成 `GeneratedChunk`，供
    // `features` / `light` 的输入使用。
    // Restore stage intermediates: before terrain only structures/biomes exist;
    // from terrain on, rebuild the CARVERS-like `GeneratedChunk` from the disk
    // blocks so features/light can consume them.
    let mut partial = if disk_status.is_after_terrain() {
        Some(Arc::new(generated_from_column(&column)))
    } else {
        None
    };
    let mut deltas: Vec<crate::worldgen_features::FeatureWrites> = Vec::new();
    // 官方 `SerializableChunkData.read`：`Status >= LIGHT` 且 `isLightOn` 为真时
    // 盘上的光照段灌进光引擎，`light` 步也不重算（`isLighted` 返回真）。
    // 这里把盘上的两层光照原样留到完工组装。
    // Vanilla `read` reuses saved light for `Status >= LIGHT`; keep those layers.
    let mut light: Option<mc_bamboo_world::light::generate::GeneratedLight> =
        if (disk_status as usize) >= (ChunkStatus::Light as usize) && decoded.light_on {
            Some(light_from_column(&column))
        } else {
            None
        };
    // 盘上的 structures 是起点 / 引用的权威来源：无论阶段都要原样带回。
    // The disk tag is authoritative for saved starts/references at every stage.
    let mut structures: Option<Tag> = Some(decoded.structures.clone());

    let stages = ChunkStatus::ALL;
    let stopped = break_kind == Some(ResumeBreak::TreatAsFull);
    for stage in stages.into_iter().filter(|stage| {
        (*stage as usize) > (disk_status as usize)
            && (*stage as usize) <= (ChunkStatus::Full as usize)
    }) {
        if stopped {
            break;
        }
        let center = Some(Box::new(column));
        let output = match stage {
            // 官方 LOADING_PYRAMID 的 `structure_starts` 是 `loadStructureStarts`：
            // 只复用盘上起点，不重算；其余结构阶段由纯函数按需重算。
            // LOADING_PYRAMID's structure_starts step only reuses saved starts.
            ChunkStatus::StructureStarts
            | ChunkStatus::StructureReferences
            | ChunkStatus::Biomes => passthrough_stage(center),
            ChunkStatus::Terrain => generate_terrain_stage(config, case.pos.x, case.pos.z)?,
            ChunkStatus::Features => {
                let empty: Vec<(ChunkPos, Option<Arc<GeneratedChunk>>)> = Vec::new();
                generate_features_stage(config, case.pos.x, case.pos.z, partial.clone(), &empty)?
            }
            ChunkStatus::Light => {
                let empty: Vec<(ChunkPos, Option<Arc<GeneratedChunk>>)> = Vec::new();
                let generated =
                    generate_light_stage(config, case.pos.x, case.pos.z, partial.clone(), &empty)?;
                let mut output = passthrough_stage(center);
                output.light = Some(generated);
                output
            }
            ChunkStatus::Empty
            | ChunkStatus::InitializeLight
            | ChunkStatus::Spawn
            | ChunkStatus::Full => passthrough_stage(center),
        };
        column = *apply_stage_output(
            &mut partial,
            &mut deltas,
            &mut light,
            &mut structures,
            output,
        );
    }

    // 完工组装：官方 `full` 步把 ProtoChunk 变成 LevelChunk，Bamboo 把
    // CARVERS + 地物增量 + 生成期光照 + structures 装进交付盒子。
    // Final assembly as `full`: install terrain + feature writes + light + tag.
    if !stopped {
        if let Some(snapshot) = partial.as_ref() {
            let mut groups = std::mem::take(&mut deltas);
            if break_kind == Some(ResumeBreak::DropNeighborWrites) {
                groups.retain(|group| group.slot == 4);
            }
            // 盘上状态已经跑到 FEATURES 时，盘上的方块是「地物写完之后」的
            // 视图，不能拿 CARVERS + 新算的写再覆盖一遍（官方 `ChunkStep.apply`
            // 会跳过已完成的阶段）。只把续跑阶段交回的新增写增量合进去。
            // A chunk saved at/after FEATURES already contains the merged feature
            // writes; do not rebuild it from CARVERS. Only newly produced writes
            // from resumed stages are merged here.
            if (disk_status as usize) >= (ChunkStatus::Features as usize) {
                let mut assembled = (*snapshot.as_ref()).clone();
                crate::worldgen_features::merge_feature_writes(&mut assembled, groups.drain(..));
                column = *crate::worldgen::build_region_column(&assembled)?;
            } else {
                column = *assemble_feature_column(snapshot, &mut groups)?;
            }
        }
        if let Some(generated) = light.take() {
            column.set_generated_light(generated);
        }
        if let Some(tag) = structures.take() {
            set_structures_tag(&mut column, &tag);
        }
        set_persisted_status(&mut column, ChunkStatus::Full);
    }
    let saved_nbt = chunk_nbt::encode_bytes(
        column.chunk_column(),
        &encode_extra_from_disk(case.pos, &decoded),
    )
    .map_err(|error| format!("encode {:?}: {error}", case.pos))?;
    Ok(snapshot_resumed(case.pos, &column, &saved_nbt))
}

/// 把一个阶段的产物合并进世界线程的每区块记账（和 `DirectorHost::merge_generated` 同形）。
fn apply_stage_output(
    partial: &mut Option<Arc<GeneratedChunk>>,
    deltas: &mut Vec<crate::worldgen_features::FeatureWrites>,
    light: &mut Option<mc_bamboo_world::light::generate::GeneratedLight>,
    structures: &mut Option<Tag>,
    output: GenerateOutput,
) -> Box<RegionColumn> {
    if let Some(terrain) = output.terrain {
        *partial = Some(terrain);
    }
    for (_, groups) in output.deltas {
        deltas.extend(groups);
    }
    if output.light.is_some() {
        *light = output.light;
    }
    if output.structures.is_some() {
        *structures = output.structures;
    }
    output.column
}

/// 设置这一柱当前生成阶段（官方 `ChunkAccess.setPersistedStatus`）。
fn set_persisted_status(column: &mut RegionColumn, status: ChunkStatus) {
    let mut archive = column.chunk_column().chunk().archive().clone();
    archive.status = status;
    column.set_archive(archive);
}

/// 编码时按盘上的元数据原样带回 `LastUpdate` / `InhabitedTime` / 不认识的键等。
fn encode_extra_from_disk(
    pos: ChunkPos,
    decoded: &chunk_nbt::DecodedChunk,
) -> ChunkEncodeExtra<'_> {
    ChunkEncodeExtra {
        chunk_pos: pos,
        status: Some(decoded.persisted_status),
        last_update: decoded.last_update,
        inhabited_time: decoded.inhabited_time,
        preserved: decoded.preserved.clone(),
        structures: Some(decoded.structures.clone()),
        post_processing: Some(decoded.post_processing.clone()),
        raw_block_entities: None,
        raw_heightmaps: None,
        block_ticks: decoded.block_ticks.clone(),
        fluid_ticks: decoded.fluid_ticks.clone(),
        ..Default::default()
    }
}

/// 从完工方块重算四张 FINAL 高度图（和官方 `ChunkAccess.setBlockState` 的
/// `Heightmap.update` 终值一致，也等于 `Heightmap.primeHeightmaps` 的结果）。
///
/// Recompute the four FINAL heightmaps from the finished blocks; this is the
/// same value official `Heightmap.update` (or `primeHeightmaps`) settles on.
fn rebuild_final_heightmaps(column: &RegionColumn) -> BTreeMap<&'static str, Vec<i32>> {
    let chunk = column.chunk_column().chunk();
    let mut blocks = Vec::with_capacity(WORLD_HEIGHT as usize);
    for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
        let mut layer = vec![0_u32; 256];
        for x in 0..16 {
            for z in 0..16 {
                layer[x * 16 + z] = chunk.block(x, y, z);
            }
        }
        blocks.push(layer);
    }
    let generated = GeneratedChunk {
        blocks,
        biomes: vec![vec![0_u32; 64]; SECTION_COUNT],
        heightmaps: BTreeMap::new(),
    };
    crate::worldgen::compute_heightmaps_public(&generated)
}

/// 读盘柱子的两层光照段（官方 `SerializableChunkData.read` 恢复的内容）。
fn light_from_column(column: &RegionColumn) -> mc_bamboo_world::light::generate::GeneratedLight {
    use mc_bamboo_core::LightLayer;
    let layers = |layer: LightLayer| {
        (0..LIGHT_SECTION_COUNT)
            .map(|slot| {
                column
                    .chunk_column()
                    .section(
                        layer,
                        SectionPos::new(0, MIN_LIGHT_SECTION + slot as i32, 0),
                    )
                    .cloned()
            })
            .collect::<Vec<_>>()
    };
    mc_bamboo_world::light::generate::GeneratedLight {
        sky: layers(LightLayer::Sky),
        block: layers(LightLayer::Block),
    }
}

/// 把读盘柱子的方块 / 群系 / 高度图还原成 `GeneratedChunk`（CARVERS 形状的纯数据）。
fn generated_from_column(column: &RegionColumn) -> GeneratedChunk {
    let chunk = column.chunk_column().chunk();
    let mut blocks = Vec::with_capacity(usize::try_from(WORLD_HEIGHT).unwrap_or(384));
    for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
        let mut layer = vec![0_u32; 256];
        for x in 0..16 {
            for z in 0..16 {
                layer[x * 16 + z] = chunk.block(x, y, z);
            }
        }
        blocks.push(layer);
    }
    let mut biomes = Vec::with_capacity(SECTION_COUNT);
    for section in 0..SECTION_COUNT {
        let y = MIN_Y + section as i32 * SECTION_SIZE;
        let mut cells = vec![0_u32; 64];
        for qx in 0..4 {
            for qy in 0..4 {
                for qz in 0..4 {
                    cells[qx * 16 + qy * 4 + qz] =
                        chunk.biome_at(qx * 4, y + (qy * 4) as i32, qz * 4);
                }
            }
        }
        biomes.push(cells);
    }
    let mut heightmaps = BTreeMap::new();
    for (kind, key) in HeightmapKind::ALL.into_iter().zip([
        "WORLD_SURFACE",
        "OCEAN_FLOOR",
        "MOTION_BLOCKING",
        "MOTION_BLOCKING_NO_LEAVES",
    ]) {
        let mut columns = vec![0_i32; 256];
        for x in 0..16 {
            for z in 0..16 {
                columns[x + z * 16] = chunk.heightmap_y(kind, x, z);
            }
        }
        heightmaps.insert(key, columns);
    }
    GeneratedChunk {
        blocks,
        biomes,
        heightmaps,
    }
}

/// 读出完工区块里要对拍的全部字段。
fn snapshot_resumed(pos: ChunkPos, column: &RegionColumn, saved_nbt: &[u8]) -> ResumedChunk {
    let chunk = column.chunk_column().chunk();
    let mut blocks = Vec::with_capacity(usize::try_from(WORLD_HEIGHT).unwrap_or(384));
    for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
        let mut layer = vec![0_u32; 256];
        for x in 0..16 {
            for z in 0..16 {
                layer[x * 16 + z] = chunk.block(x, y, z);
            }
        }
        blocks.push(layer);
    }
    let mut biomes = Vec::with_capacity(SECTION_COUNT);
    for section in chunk.sections().iter().take(SECTION_COUNT) {
        let mut cells = vec![0_u32; 64];
        for qy in 0..4 {
            for qz in 0..4 {
                for qx in 0..4 {
                    cells[qx * 16 + qy * 4 + qz] = section.biomes().get(qy * 16 + qz * 4 + qx);
                }
            }
        }
        biomes.push(cells);
    }
    // 高度图按完工方块重算，和 `build_region_column` / 官方增量更新的终值一致。
    // Recompute from the finished blocks, matching assembly and official updates.
    let all = rebuild_final_heightmaps(column);
    let heightmaps = [
        "WORLD_SURFACE",
        "OCEAN_FLOOR",
        "MOTION_BLOCKING",
        "MOTION_BLOCKING_NO_LEAVES",
    ]
    .into_iter()
    .filter_map(|key| all.get(key).map(|values| (key, values.clone())))
    .collect::<BTreeMap<&'static str, Vec<i32>>>();
    let read_layers = |layer: LightLayer| {
        (0..LIGHT_SECTION_COUNT)
            .map(|slot| {
                column
                    .chunk_column()
                    .section(
                        layer,
                        SectionPos::new(0, MIN_LIGHT_SECTION + slot as i32, 0),
                    )
                    .map(|data: &DataLayer| data.bytes().to_vec())
            })
            .collect::<Vec<_>>()
    };
    // 官方 `getBlockEntityNbtForSaving` 的世界坐标 + SNBT：和 `gen-world`
    // 的方块实体比较器用同一种输出，逐项对账内容和位置。
    // World coordinates + SNBT like vanilla `getBlockEntityNbtForSaving`,
    // matching the `gen-world` block-entity comparator.
    let mut block_entities = Vec::new();
    for entity in chunk.block_entities() {
        let world_x = pos.x * 16 + entity.x as i32;
        let world_z = pos.z * 16 + entity.z as i32;
        let tag = chunk
            .preserved_block_entity(entity.x, entity.y, entity.z)
            .map(Tag::to_snbt)
            .unwrap_or_default();
        block_entities.push((format!("{world_x},{},{world_z}", entity.y), tag));
    }
    block_entities.sort();
    let structures = chunk
        .archive()
        .structures
        .as_ref()
        .map(Tag::to_snbt)
        .unwrap_or_default();
    ResumedChunk {
        blocks,
        biomes,
        heightmaps,
        sky: read_layers(LightLayer::Sky),
        block_light: read_layers(LightLayer::Block),
        light_on: column.chunk_column().light_on(),
        block_entities,
        structures,
        saved_status: chunk.archive().status,
        saved_nbt: saved_nbt.to_vec(),
    }
}

/// 清掉续跑对拍装进线程本地缓存的读盘结构覆盖。
pub fn clear_resume_overrides() {
    crate::worldgen_structures::clear_disk_structures();
}

/// `ChunkArchiveMeta` 用来确认读盘元数据带进了交付盒子。
#[allow(dead_code)]
fn _archive_is_send(archive: &ChunkArchiveMeta) -> &ChunkArchiveMeta {
    archive
}
