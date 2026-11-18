//! 把 `ChunkStatus.FEATURES` 接进 Director 的 `Staged` 流水线。
//!
//! 官方这一步是 `ChunkGenerator.applyBiomeDecoration`：读中心和半径 1 的 8 个
//! 邻居（3×3）的 CARVERS 方块，写中心、也会写到邻居里。`mc_bamboo_worldgen::feature`
//! 的 [`Generator::features`] 是这段的纯函数；这里负责在游戏侧拼出 3×3 视图、
//! 调它（每个中心整个进程只跑一次，结果进全进程缓存），把本区块完工要的写增量
//! （自己的地物 + 8 个邻居溢进来的）交回世界线程。
//!
//! 世界线程把每个区块的半成品存成不可变 [`Arc<GeneratedChunk>`]：发出任务后这些
//! `Arc` 不再被改，合并增量时通过 `Arc::make_mut` 写时复制出新的 `Arc`（设计稿
//! 5.3.1、`worldgen-features-plan.md` 3.2）。计算池只读这些快照、不碰世界状态。
//!
//! Wire `ChunkStatus.FEATURES` into the Director's `Staged` pipeline. Vanilla's
//! `ChunkGenerator.applyBiomeDecoration` reads a 3×3 of CARVERS chunks and
//! writes the center plus neighbor overflow. `mc_bamboo_worldgen::feature`'s
//! `Generator::features` is the pure function; this module assembles the 3×3
//! view, calls it (once per center per process, cached), and returns the writes
//! this chunk needs (its own features + the 8 neighbors' overflow).
//! The world thread keeps each chunk's half-built column as an immutable
//! `Arc<GeneratedChunk>` and merges deltas copy-on-write via `Arc::make_mut`.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, light::generate::GeneratedLight};
use mc_bamboo_worldgen::{
    Worldgen,
    feature::{
        CENTER, DECORATION_STEPS, FeatureChunks, FeatureResult, Heightmaps, world::prime_heightmaps,
    },
    structure::{
        pipeline::StartPieces,
        place_step::{StartInput, StartPieces as PlacePieces, StepStructure, StructurePlacement},
        processor::ProcessorLists,
    },
};

use crate::{
    region::RegionColumn,
    world_options::WorldGenConfig,
    worldgen::{
        GeneratedChunk, build_region_column, cached_beardified_chunk, cached_generate_chunk,
        with_generator,
    },
    worldgen_profile::{
        BUILD_VIEW_CALLS, BUILD_VIEW_CELLS, BUILD_VIEW_NS, BUILD_VIEW_TERRAIN_MISSES,
        FEATURE_RUN_CALLS, FEATURE_RUN_HITS, FEATURE_RUN_MISSES, FEATURES_CALLS, FEATURES_NS,
        RUN_FEATURES_CALLS, RUN_FEATURES_NS, SNAPSHOT_NONE, SNAPSHOT_SOME, TERRAIN_CALLS,
        TERRAIN_HITS, TERRAIN_LOOKUPS, TERRAIN_MISSES, TERRAIN_NS,
    },
};

/// 一次 `setBlock` 写增量：世界坐标 `(x, y, z, 状态 ID)`。
///
/// One feature `setBlock` write in world coordinates `(x, y, z, state id)`.
pub(crate) type BlockDelta = (i32, i32, i32, u32);

/// 一个来源中心写进目标区块的全部地物写。`slot` 是来源中心相对目标区块的
/// 3×3 下标 `(dz+1)*3+(dx+1)`；中心自己是 4，其余按 `slot` 升序后到。
/// 只有 `features` 阶段产生这些写，完工时 `assemble_feature_column` 按 `slot`
/// 的先后合，同一格由先写的那个中心定下最终状态。
///
/// All feature writes from one source center into a target chunk. `slot` is the
/// source center's 3×3 index `(dz+1)*3+(dx+1)` relative to the target: 4 for the
/// target itself, then neighbors in ascending slot order. Only the `features`
/// stage produces these; `assemble_feature_column` merges by `slot` order, so
/// the first writer of a cell determines its final state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureWrites {
    /// 来源中心的 3×3 下标；`assemble_feature_column` 按它升序排先后。
    /// The source center's 3×3 index; assembly orders writers by it.
    pub slot: u8,
    /// 这一中心的写增量（同一格最多一条，见 `feature::world::last_writes`）。
    /// The center's writes (at most one per cell, see `feature::world::last_writes`).
    pub deltas: Vec<BlockDelta>,
}

/// 按中心的固定先后合并一组写：中心自己（`CENTER`）最先，其余 8 个来源按
/// `slot` 升序，即 `(dz,dx)` 升序；同一格取最先写下的那条状态。
///
/// 游戏完工组装和 `feature-merge` 的 `isolated` 比法都调这里，保证两边合成规则
/// 只有一份。各来源之间的先后只由 `slot` 决定，与分组传入顺序无关；同一来源对
/// 同一格写了多次时，以它最后写下的那条为该中心的最终状态（真实数据里每个来源
/// 每格最多一条，见 `feature::world::last_writes`）。越界的 y 由 `apply_deltas`
/// 忽略。
///
/// Merge writes by fixed source order: the target itself (`CENTER`) first, then
/// the eight neighbors in ascending slot / `(dz,dx)` order; the first write to a
/// cell wins. Both assembly and the `isolated` oracle comparison call this, so
/// the rule lives in one place. The order between sources depends only on `slot`,
/// not on group order; repeated writes to a cell from one source resolve to its
/// last write (real data has at most one per source per cell, see
/// `feature::world::last_writes`). Out-of-range y values are ignored by
/// `apply_deltas`.
pub fn merge_feature_writes(
    chunk: &mut GeneratedChunk,
    groups: impl IntoIterator<Item = FeatureWrites>,
) {
    // 先按来源收敛：同一来源对同一格的多次写留下最后一条，作为该中心的最终状态。
    // First collapse each source: its last write to a cell is its final state.
    let mut per_slot: std::collections::BTreeMap<
        u8,
        std::collections::HashMap<(i32, i32, i32), u32>,
    > = std::collections::BTreeMap::new();
    for group in groups {
        let writes = per_slot.entry(group.slot).or_default();
        for &(x, y, z, state) in &group.deltas {
            if (MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
                writes.insert((x, y, z), state);
            }
        }
    }
    // 固定先后：中心自己最先，其余按 slot 升序。从后往前覆盖，先写的留下。
    // Fixed order: the center first, then ascending slots. Walk backwards so
    // earlier sources overwrite later ones and the first writer survives.
    let mut order: Vec<u8> = per_slot.keys().copied().collect();
    order.sort_by_key(|&slot| (u8::from(slot != CENTER as u8), slot));
    let mut winners: std::collections::HashMap<(i32, i32, i32), u32> =
        std::collections::HashMap::new();
    for slot in order.into_iter().rev() {
        for ((x, y, z), state) in per_slot.remove(&slot).unwrap_or_default() {
            winners.insert((x, y, z), state);
        }
    }
    let mut out: Vec<BlockDelta> = winners
        .into_iter()
        .map(|((x, y, z), state)| (x, y, z, state))
        .collect();
    // 每格只留一条，排序只为让落盘顺序也确定，与哈希表迭代次序无关。
    // One entry per cell; sorted so even the apply order is deterministic.
    out.sort_unstable();
    apply_deltas(chunk, &out);
}

/// 一个生成阶段的产物。
///
/// Output of one generation stage.
pub(crate) struct GenerateOutput {
    /// 中心推进到这一阶段后的盒子。
    pub(crate) column: Box<RegionColumn>,
    /// `terrain` 阶段的 CARVERS 纯数据，供世界线程的邻居快照缓存用。
    /// 与全进程地形缓存共用同一个 `Arc`，回执搬回世界线程时不深拷贝。
    /// Clean CARVERS data from the `terrain` stage, for the world-thread
    /// neighbor snapshot cache; shares the global cache's `Arc` without a copy.
    pub(crate) terrain: Option<Arc<GeneratedChunk>>,
    /// 给邻居（和中心自身）的地物写，按目标区块坐标分组，组内按来源中心分组。
    /// Feature writes grouped by target chunk and, inside, by source center.
    pub(crate) deltas: Vec<(ChunkPos, Vec<FeatureWrites>)>,
    /// `light` 阶段算出的生成期光照（中心柱的天空光 + 方块光）。`None` 表示
    /// 这一阶段不算光；世界线程在完工组装时装进交付盒子。
    /// Generation-time light for the center column from the `light` stage (sky +
    /// block layers). `None` means this stage computes no light; the world
    /// thread installs it into the delivered column at final assembly.
    pub(crate) light: Option<GeneratedLight>,
    /// `structure_references` 阶段算好的 `structures` 顶层标签（`starts` + `References`）。
    /// 世界线程在完工组装时把它装进交付盒子，下一次写盘原样带回。
    /// The `structures` tag from `structure_references` (starts + References);
    /// the world thread installs it into the delivered column at assembly.
    pub(crate) structures: Option<Tag>,
}

impl GenerateOutput {
    /// 没有增量、没有快照、没有光照的阶段（透传）。
    /// A pass-through stage: no snapshot, no deltas, no light.
    fn passthrough(column: Box<RegionColumn>) -> Self {
        Self {
            column,
            terrain: None,
            deltas: Vec::new(),
            light: None,
            structures: None,
        }
    }
}

/// 邻居只读视图：目标是哪些区块、各自不可变的 CARVERS 快照。
/// `None` 表示世界线程还没有这个位置的快照，计算线程用纯函数现算 —— 现算的
/// 必须和 `terrain` 阶段交回的快照是同一份（带本格 Beardifier），否则有没有
/// 快照（调度先后）会改变结果。
///
/// Read-only neighbor view: target positions and their immutable CARVERS
/// snapshots; `None` means the compute thread regenerates it purely, and the
/// regenerated cell must equal the `terrain` stage snapshot (with the cell's
/// Beardifier), otherwise snapshot presence (scheduling) changes the result.
pub(crate) type SnapshotView<'a> = &'a [(ChunkPos, Option<Arc<GeneratedChunk>>)];

/// `ChunkGenerator.applyBiomeDecoration`，纯函数：只依赖配置、坐标和 3×3 视图。
/// 视图是只读的 `Arc`；函数内部为了给地物一份可写方块层才复制一次。
///
/// `ChunkGenerator.applyBiomeDecoration` as a pure function of config, position
/// and the 3×3 view.
pub(crate) fn run_features(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    view: [Arc<GeneratedChunk>; 9],
) -> Result<FeatureResult, String> {
    let _profile_timer = crate::worldgen_profile::timer(&RUN_FEATURES_NS);
    crate::worldgen_profile::count(&RUN_FEATURES_CALLS);
    run_features_filtered(
        config,
        cx,
        cz,
        view,
        None,
        (1 << DECORATION_STEPS) - 1,
        None,
    )
    .map(|(result, _)| result)
}

/// `worldgen.json` 的 `processor_list` 只随数据版本变，和种子、世界类型、区块坐标
/// 都无关。解析整份 JSON（`Worldgen::load`）实测约 24 ms，若每个区块的 `features`
/// 调用都重来一次，它会盖过地物本身。进程里缓存一份解析结果，之后只克隆这张表
/// （每条计算线程各持一份，结构放置只读它）。
///
/// `processor_list` depends only on the bundled data version, not on the seed,
/// world type or chunk position. Parsing the whole JSON costs ~24 ms, which would
/// dwarf the features work if repeated per chunk; cache it process-wide.
fn processor_lists() -> Result<&'static ProcessorLists, String> {
    static LISTS: OnceLock<Result<ProcessorLists, String>> = OnceLock::new();
    LISTS
        .get_or_init(|| {
            let worldgen = Worldgen::load()?;
            let raw = worldgen
                .raw()
                .get("processor_list")
                .ok_or("missing processor_list")?;
            ProcessorLists::parse(raw)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// [`run_features`] 加一个地物白名单；游戏里恒为 `None`，只有对拍工具会传。
///
/// [`run_features`] with a feature allow list; the game always passes `None`,
/// only the oracle tool narrows it.
fn run_features_filtered(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    view: [Arc<GeneratedChunk>; 9],
    allowed_kinds: Option<&[String]>,
    steps_mask: i32,
    skip_structure: Option<&str>,
) -> Result<
    (
        FeatureResult,
        Option<mc_bamboo_worldgen::structure::place_step::PlaceStats>,
    ),
    String,
> {
    if config.level_type.is_flat() {
        // flat 世界没有噪声地物；流水线也不是 Staged，这里只是防御。
        // Flat worlds have no noise features; the pipeline is not Staged either.
        return Ok((
            FeatureResult {
                blocks: std::array::from_fn(|_| {
                    vec![vec![mc_bamboo_data::blocks::AIR; 256]; WORLD_HEIGHT as usize]
                }),
                deltas: std::array::from_fn(|_| Vec::new()),
                block_entities: Vec::new(),
                unsupported: false,
            },
            None,
        ));
    }
    // 结构阶段 / 邻居起点重算改写了共享的群系 R 树 `lastResult`；地物的
    // `BiomeFilter` 走单点查询，从确定状态起步才与计算池的区块到达顺序无关。
    // Structure stages / neighbour-start recomputation mutated the shared biome
    // R-tree `lastResult`; features' BiomeFilter uses single-point lookups, so
    // start clean to stay independent of compute-pool arrival order.
    crate::worldgen_structures::reset_generator_biome_state(config)?;
    let blocks: [Vec<Vec<u32>>; 9] = view.each_ref().map(|chunk| chunk.blocks.clone());
    let biomes: [Vec<Vec<u32>>; 9] = view.each_ref().map(|chunk| chunk.biomes.clone());
    let heights: [Heightmaps; 9] = blocks
        .each_ref()
        .map(|chunk| prime_heightmaps(chunk, MIN_Y, WORLD_HEIGHT));
    let input = FeatureChunks {
        center_x: cx,
        center_z: cz,
        min_y: MIN_Y,
        height: WORLD_HEIGHT,
        blocks,
        biomes,
        heights,
    };
    // `applyBiomeDecoration` 的结构分支：按中心区块的 `References` 反查每个起点，
    // 按 `structure.step()` 分组；`generate-structures=false` 时整段跳过。
    // Vanilla's structure branch: resolve the starts referenced by the center,
    // group by `structure.step()`; skipped when `generate-structures=false`.
    let references =
        crate::worldgen_structures::structure_references_stage(config, cx, cz).unwrap_or_default();
    let mut starts: Vec<StartInput> = Vec::new();
    if config.generate_structures {
        for (structure, keys) in &references {
            let Some(type_id) = crate::worldgen_structures::structure_type(config, structure)
            else {
                continue;
            };
            for &packed in keys {
                let (sx, sz) = crate::worldgen_structures::unpack_chunk_pos(packed);
                let rich = crate::worldgen_structures::structure_starts(config, sx, sz)?;
                let Some(start) = rich.get(structure) else {
                    continue;
                };
                let pieces = match &start.pieces {
                    StartPieces::Layout(pieces) => PlacePieces::Layout(pieces.clone()),
                    StartPieces::Jigsaw(pieces) => PlacePieces::Jigsaw(pieces.clone()),
                    StartPieces::Sparse(_) => PlacePieces::Sparse,
                };
                starts.push(StartInput {
                    structure: structure.clone(),
                    type_id: type_id.clone(),
                    source_x: sx,
                    source_z: sz,
                    pieces,
                });
            }
        }
    }
    // 官方 `structuresByStep` 是**整张** `STRUCTURE` 注册表按 `step().ordinal()`
    // 分组：per-step 局部 index 数的是该 step 的所有结构，不管这个区块有没有引用它。
    // 因此这里给每个注册结构都建一组（没有引用的组 starts 为空）。
    // Vanilla's `structuresByStep` groups the **whole** STRUCTURE registry by
    // `step().ordinal()`: the local index counts every structure at the step,
    // referenced or not, so every registry structure gets a group.
    let mut by_name: std::collections::BTreeMap<String, Vec<StartInput>> =
        std::collections::BTreeMap::new();
    for start in starts {
        by_name
            .entry(start.structure.clone())
            .or_default()
            .push(start);
    }
    let all_structures = crate::worldgen_structures::structure_names(config);
    let mut owned: Vec<(String, Vec<StartInput>)> = Vec::new();
    for name in all_structures {
        owned.push((name.clone(), by_name.remove(&name).unwrap_or_default()));
    }
    let groups: Vec<StepStructure<'_>> = owned
        .iter()
        .map(|(name, group)| StepStructure {
            structure: name.as_str(),
            step: crate::worldgen_structures::structure_step(config, name).unwrap_or(4),
            starts: group.as_slice(),
        })
        .collect();
    let mut placement = StructurePlacement::new(groups, processor_lists()?.clone(), config.seed);
    // 对拍改坏确认：跳过某个结构类型的结构件放置（`None` 是正常路径）。
    // Oracle break confirmation: drop one structure type's pieces.
    if let Some(skip) = skip_structure {
        placement.skip_type(skip);
    }
    // 上面反查起点 / 引用会改写一个群的系 R 树 `lastResult`；`BiomeManager`
    // 在 features 里按同样顺序查它，进主循环前清到确定状态。
    // Resolving starts/references above mutates the shared biome R-tree
    // `lastResult`; clear it before the feature loop reads it.
    crate::worldgen_structures::reset_generator_biome_state(config)?;
    with_generator(config, |generator| {
        generator.features_with_structures(&input, steps_mask, allowed_kinds, Some(&placement))
    })
}

/// `gen-world --through features` 对拍入口：只用配置和坐标，把 3×3 CARVERS
/// 视图拼好、调游戏里同一份 [`run_features`]，返回 9 宫格结果给工具逐方块比较。
///
/// `gen-world --through features` oracle entry: from config + position alone,
/// build the 3×3 CARVERS view and call the game's own [`run_features`], returning
/// the 9-chunk result for block-by-block comparison.
///
/// `allowed_kinds` 和 Java 侧 `--feature-types` 是同一份白名单：官方那边只留
/// 这些地物时，这里也只跑这些，两边的集合才一样。
/// `allowed_kinds` is the same allow list as Java's `--feature-types`, so both
/// sides run the same set of features.
pub fn game_features(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    allowed_kinds: Option<&[String]>,
) -> Result<FeatureResult, String> {
    game_features_with_mask(config, cx, cz, allowed_kinds, (1 << DECORATION_STEPS) - 1)
}

/// [`game_features`] 加上过渡期的 `--steps` 掩码，只有对拍工具用；
/// 游戏里恒为全部 11 步。
///
/// [`game_features`] with the transitional `--steps` mask; only the oracle uses
/// a narrowed mask, the game always runs all eleven steps.
pub fn game_features_with_mask(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    allowed_kinds: Option<&[String]>,
    steps_mask: i32,
) -> Result<FeatureResult, String> {
    game_features_with_mask_skipping(config, cx, cz, allowed_kinds, steps_mask, None)
}

/// 对拍改坏确认用：额外跳过一个结构类型的结构件放置，确认逐方块比较会报差异。
/// 正常路径（游戏和干净对拍）永远传 `None`。
///
/// Break confirmation for the oracle: skip one structure type's piece placement
/// to prove the block comparator detects it. The game and clean runs pass `None`.
pub fn game_features_with_mask_skipping(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    allowed_kinds: Option<&[String]>,
    steps_mask: i32,
    skip_structure: Option<&str>,
) -> Result<FeatureResult, String> {
    let view = build_view(config, cx, cz)?;
    run_features_filtered(
        config,
        cx,
        cz,
        view,
        allowed_kinds,
        steps_mask,
        skip_structure,
    )
    .map(|(result, _)| result)
}

/// 一次「游戏里的完整生成流水线」结果，给 `gen-world --through structures` 对拍用。
/// 走的是游戏里同一批阶段纯函数：`structure_starts` / `structure_references` /
/// `terrain`（带 Beardifier）/ `features`（先结构后地物），最后按 Director 完工时的
/// 来源固定先后合并增量（先写的赢）。
///
/// Result of the game's full generation pipeline for `gen-world --through structures`.
/// It runs the same stage pure functions the game uses, then merges deltas the way
/// the Director does at assembly.
#[derive(Debug, Clone)]
pub struct GamePipelineResult {
    /// 9 宫格最终方块（每格 `WORLD_HEIGHT × 256`）。
    pub blocks: [Vec<Vec<u32>>; 9],
    /// 中心区块的 `structures` 标签（`starts` + `References`）。
    pub structures: mc_bamboo_nbt::Tag,
    /// 中心区块引用里的 `(结构名, 起点 packed 键)` 列表，对拍逐项比。
    pub references: Vec<(String, i64)>,
    /// 中心区块起点里的 `(结构名, 结构件数)` 列表，对拍逐项比。
    pub starts: Vec<(String, usize)>,
    /// 各结构类型实际放置的结构件次数（每个 step 累计）。
    pub placed_pieces: std::collections::BTreeMap<String, usize>,
    /// 各结构类型的结构件写过的方块次数。
    pub placed_blocks: std::collections::BTreeMap<String, usize>,
    /// 对拍没涉及但游戏里标 unsupported 的放置次数。
    pub unsupported_pieces: usize,
}

/// `gen-world` 端到端入口：按 Director 的 `ChunkStatus` 顺序走同一批纯函数，
/// 返回 3×3 九宫格跑完 FEATURES 之后的方块与结构数据。
///
/// End-to-end `gen-world` entry: follow the Director's `ChunkStatus` order through
/// the same pure functions, returning the 3x3 blocks after FEATURES.
pub fn game_pipeline(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<GamePipelineResult, String> {
    use mc_bamboo_worldgen::feature::CENTER;

    // 1. `STRUCTURE_STARTS` / `STRUCTURE_REFERENCES`（纯函数重算 + 缓存）。
    let data = crate::worldgen_structures::structure_data(config, cx, cz)?;
    // 2. `BIOMES` / `TERRAIN`：3×3 每个区块都按自己的 references 注入 Beardifier。
    let mut blocks: [Vec<Vec<u32>>; 9] = std::array::from_fn(|_| Vec::new());
    let mut biomes: [Vec<Vec<u32>>; 9] = std::array::from_fn(|_| Vec::new());
    for dz in -1..=1 {
        for dx in -1..=1 {
            let x = cx + dx;
            let z = cz + dz;
            let index = ((dz + 1) * 3 + dx + 1) as usize;
            let refs = crate::worldgen_structures::structure_references_stage(config, x, z)?;
            let beard = crate::worldgen_structures::beardifier_for_chunk(config, x, z, &refs)?;
            let chunk =
                crate::worldgen::generate_chunk_with_beardifier(config, x, z, beard.as_ref())?;
            blocks[index] = chunk.blocks;
            biomes[index] = chunk.biomes;
        }
    }
    // 3. `FEATURES`：先结构后地物，中心这一遍要拿到写增量统计。
    let view: [Arc<GeneratedChunk>; 9] = std::array::from_fn(|index| {
        Arc::new(GeneratedChunk {
            blocks: blocks[index].clone(),
            biomes: biomes[index].clone(),
            heightmaps: Default::default(),
        })
    });
    let (result, stats) = run_features_filtered(
        config,
        cx,
        cz,
        view,
        None,
        (1 << DECORATION_STEPS) - 1,
        None,
    )?;
    let mut final_blocks = blocks;
    // 4. Director 完工时的合并规则：中心自己的写 + 8 个邻居中心溢进来的写，
    //    按中心固定先后合、同格先写的赢（和正式组装路径用同一个函数）。
    //    单中心 3×3 跑出的 `result.deltas[CENTER]` 是给邻居的增量，中心格要用
    //    「相对干净地形变了哪些格」那一份；邻居的溢出由纯函数重算（带缓存）。
    // 4. Assembly rule: the center's own writes plus 8 neighbor centers' spills,
    //    merged by fixed center order with first-writer-wins (same function as the
    //    real assembly path). `result.deltas[CENTER]` from this single 3×3 run is
    //    the center's writes into other chunks; the center cell needs the diff
    //    against clean terrain. Neighbor spills come from the cached pure runs.
    let terrain = cached_generate_chunk_with_structures(config, cx, cz)?;
    let mut chunk = (*terrain).clone();
    merge_feature_writes(&mut chunk, feature_writes_into(config, cx, cz)?);
    final_blocks[CENTER] = chunk.blocks;
    // 这一步还交回单中心 3×3 的结构放置统计，方块取上面 9 个中心合出来的结果。
    // This run also supplies the structure placement stats; blocks come from the
    // merged 9-center result above.
    let _ = &result;
    let references: Vec<(String, i64)> = data
        .references
        .iter()
        .flat_map(|(name, keys)| keys.iter().map(move |&key| (name.clone(), key)))
        .collect();
    let starts: Vec<(String, usize)> = data
        .starts
        .by_structure
        .iter()
        .map(|(name, start)| {
            let count = match &start.pieces {
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Layout(p) => p.len(),
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Jigsaw(p) => p.len(),
                mc_bamboo_worldgen::structure::pipeline::StartPieces::Sparse(p) => p.len(),
            };
            (name.clone(), count)
        })
        .collect();
    Ok(GamePipelineResult {
        blocks: final_blocks,
        structures: data.tag,
        references,
        starts,
        placed_pieces: stats
            .as_ref()
            .map(|s| s.placed_pieces.clone())
            .unwrap_or_default(),
        placed_blocks: stats
            .as_ref()
            .map(|s| s.placed_blocks.clone())
            .unwrap_or_default(),
        unsupported_pieces: stats.as_ref().map_or(0, |s| s.unsupported_pieces),
    })
}

/// 一次 `applyBiomeDecoration` 里需要复用的紧凑结果：给 3×3 九宫格各格的
/// 写增量。中心那格是中心自己的地物写（[`center_writes`]，相对干净 CARVERS 地形
/// 变了的格子），8 个邻居格是溢出去的写。
///
/// Compact reusable result of one `applyBiomeDecoration`: write deltas for each
/// of the 9 grid cells. The center cell holds the center's own feature writes
/// ([`center_writes`], cells changed against clean CARVERS terrain); the other
/// eight hold the overflow into the neighbors.
struct FeatureRun {
    deltas: [Vec<BlockDelta>; 9],
}

type FeatureCacheKey = (i64, Option<&'static str>, i32, i32, bool);
/// 一个中心的地物结果，或正在算它的线程占位。等待方在 `OnceLock` 上等，
/// 拿到的是计算方放进来的同一个 `Arc`。
/// A center's feature result, or the in-flight placeholder. Waiters block on
/// the same `OnceLock` and receive the computing thread's very same `Arc`.
type FeatureSlot = Arc<OnceLock<Result<Arc<FeatureRun>, String>>>;
/// 全进程一份，按分片加锁。条目和占位一起按插入顺序淘汰，字节预算按实测
/// 写增量大小算。结果只有写增量（中心自己的写 + 8 份溢出），比一整份 CARVERS
/// 地形小得多。
///
/// One process-wide cache with sharded locks. Entries and placeholders are
/// evicted together in insertion order under a measured byte budget. A result
/// holds only write deltas (the center's own + 8 overflow lists), far smaller
/// than a full CARVERS chunk.
#[derive(Default)]
struct FeatureShard {
    entries: HashMap<FeatureCacheKey, Arc<FeatureRun>>,
    inflight: HashMap<FeatureCacheKey, FeatureSlot>,
    order: std::collections::VecDeque<FeatureCacheKey>,
    bytes: usize,
}

const FEATURE_CACHE_SHARDS: usize = 16;
const FEATURE_CACHE_BYTE_LIMIT: usize = 64 * 1024 * 1024;
static FEATURE_CACHE: OnceLock<Vec<std::sync::Mutex<FeatureShard>>> = OnceLock::new();

fn feature_shards() -> &'static [std::sync::Mutex<FeatureShard>] {
    FEATURE_CACHE.get_or_init(|| {
        (0..FEATURE_CACHE_SHARDS)
            .map(|_| std::sync::Mutex::new(FeatureShard::default()))
            .collect()
    })
}

fn feature_shard_index(key: &FeatureCacheKey) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as usize) % FEATURE_CACHE_SHARDS
}

/// 一条地物结果的实测堆字节数（9 份写增量的容量之和）。
/// Measured heap bytes held by one feature result (sum of 9 delta capacities).
fn feature_run_heap_bytes(run: &FeatureRun) -> usize {
    run.deltas
        .iter()
        .map(|deltas| deltas.capacity() * size_of::<BlockDelta>())
        .sum()
}

/// 测量 / 冷启动用：清掉整份地物结果缓存。
/// Clear the whole feature-result cache (measurements / cold starts).
pub fn clear_feature_cache() {
    for shard in feature_shards() {
        if let Ok(mut shard) = shard.lock() {
            *shard = FeatureShard::default();
        }
    }
}

/// 以 `(cx, cz)` 为中心造 3×3 的 CARVERS 视图（纯函数，带全进程共享缓存）。
/// Build a 3×3 CARVERS view centered on `(cx, cz)` (pure, globally cached).
fn build_view(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<[Arc<GeneratedChunk>; 9], String> {
    let _profile_timer = crate::worldgen_profile::timer(&BUILD_VIEW_NS);
    crate::worldgen_profile::count(&BUILD_VIEW_CALLS);
    crate::worldgen_profile::count_by(&BUILD_VIEW_CELLS, 9);
    let mut view: [Option<Arc<GeneratedChunk>>; 9] = std::array::from_fn(|_| None);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let index = ((dz + 1) * 3 + (dx + 1)) as usize;
            // 每个格子都按「官方 createNoiseChunk」注入自己的 Beardifier。
            // `terrain` 阶段的依赖表已保证半径 8 的 `structure_starts` 与中心的
            // `structure_references` 完成，所以这里读到的起点数据是定稿的；
            // 计算池任务完成先后不再影响结果。
            // Every cell injects its own Beardifier like vanilla's
            // `createNoiseChunk`. The dependency table guarantees the radius-8
            // starts and center references, so this data is final and task
            // completion order cannot change the result.
            let x = cx + dx;
            let z = cz + dz;
            let instrumented = crate::worldgen_profile::enabled();
            let counts_before = instrumented
                .then(crate::worldgen::terrain_lookup_counts)
                .unwrap_or((0, 0));
            let chunk = cached_generate_chunk_with_structures(config, x, z)?;
            if instrumented {
                crate::worldgen_profile::count(&TERRAIN_LOOKUPS);
                let (hits, misses) = crate::worldgen::terrain_lookup_counts();
                if hits > counts_before.0 {
                    crate::worldgen_profile::count(&TERRAIN_HITS);
                } else if misses > counts_before.1 {
                    crate::worldgen_profile::count(&TERRAIN_MISSES);
                    crate::worldgen_profile::count(&BUILD_VIEW_TERRAIN_MISSES);
                }
            }
            view[index] = Some(chunk);
        }
    }
    Ok(view.map(Option::unwrap))
}

/// 地形 + 本区块 Beardifier 的确定性快照。`structure_references_stage` 的结果
/// 按 (seed,x,z) 全局缓存，任何线程、任何调用顺序读到的都是同一份。
///
/// Deterministic terrain + Beardifier snapshot. References are cached globally
/// by (seed,x,z), so every thread and call order sees the same data.
pub(crate) fn cached_generate_chunk_with_structures(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
) -> Result<Arc<GeneratedChunk>, String> {
    if !config.generate_structures {
        return cached_generate_chunk(config, x, z);
    }
    let beardifier = crate::worldgen_structures::beardifier_for(config, x, z)?;
    match beardifier {
        Some(beardifier) => cached_beardified_chunk(config, x, z, &beardifier),
        None => cached_generate_chunk(config, x, z),
    }
}

/// 以某区块为中心跑一遍 `applyBiomeDecoration`，结果按中心缓存复用。
/// Run `applyBiomeDecoration` centered on a chunk, cached per center.
fn cached_feature_run(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Arc<FeatureRun>, String> {
    crate::worldgen_profile::count(&FEATURE_RUN_CALLS);
    crate::worldgen_profile::record_feature_center(config.seed, cx, cz);
    let key = (
        config.seed,
        config.level_type.settings_id(),
        cx,
        cz,
        config.generate_structures,
    );
    let shards = feature_shards();
    let index = feature_shard_index(&key);
    // 锁内只查表 / 放占位；地物在锁外算。同一个中心只有一条线程真正在算，
    // 其它线程在同一个 OnceLock 上等，拿到的是同一个 Arc。
    // Hold the lock only for lookup / placeholder insertion; features run
    // outside it. Only one thread computes a center; the rest wait on it.
    let slot = {
        let Ok(mut shard) = shards[index].lock() else {
            return compute_feature_run(config, cx, cz);
        };
        if let Some(run) = shard.entries.get(&key) {
            crate::worldgen_profile::count(&FEATURE_RUN_HITS);
            return Ok(Arc::clone(run));
        }
        if let Some(existing) = shard.inflight.get(&key) {
            crate::worldgen_profile::count(&FEATURE_RUN_HITS);
            Arc::clone(existing)
        } else {
            crate::worldgen_profile::count(&FEATURE_RUN_MISSES);
            let slot = Arc::new(OnceLock::new());
            shard.inflight.insert(key, Arc::clone(&slot));
            slot
        }
    };
    let result = slot.get_or_init(|| compute_feature_run(config, cx, cz));
    let run = match result {
        Ok(run) => Arc::clone(run),
        Err(error) => {
            // 失败的占位要移除，否则这个键以后一直拿旧的 Err。
            // Remove a failed placeholder or the key returns the stale Err forever.
            if let Ok(mut shard) = shards[index].lock()
                && shard
                    .inflight
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &slot))
            {
                shard.inflight.remove(&key);
            }
            return Err(error.clone());
        }
    };
    // 只有计算方提交条目；等待方直接返回，避免重复计字节。
    // Only the computing thread commits; waiters just return.
    if let Ok(mut shard) = shards[index].lock()
        && !shard.entries.contains_key(&key)
    {
        shard.order.push_back(key);
        shard.entries.insert(key, Arc::clone(&run));
        shard.bytes += feature_run_heap_bytes(&run);
        let target = FEATURE_CACHE_BYTE_LIMIT / FEATURE_CACHE_SHARDS * 3 / 4;
        if shard.bytes > FEATURE_CACHE_BYTE_LIMIT / FEATURE_CACHE_SHARDS {
            while shard.bytes > target {
                let Some(oldest) = shard.order.pop_front() else {
                    break;
                };
                let Some(removed) = shard.entries.remove(&oldest) else {
                    continue;
                };
                shard.bytes = shard.bytes.saturating_sub(feature_run_heap_bytes(&removed));
                shard.inflight.remove(&oldest);
            }
        }
    }
    Ok(run)
}

/// 真正跑一遍 `applyBiomeDecoration`：拼视图、调纯函数、只保留 9 份写增量
/// （中心格换成中心自己的写）。
/// Actually run `applyBiomeDecoration`: build the view, call the pure function,
/// keep only the 9 delta lists (the center cell replaced by its own writes).
fn compute_feature_run(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Arc<FeatureRun>, String> {
    let view = build_view(config, cx, cz)?;
    let terrain = Arc::clone(&view[CENTER]);
    let result = run_features(config, cx, cz, view)?;
    let own = center_writes(cx, cz, &terrain, &result);
    let mut deltas = result.deltas;
    deltas[CENTER] = own;
    Ok(Arc::new(FeatureRun { deltas }))
}

/// 给 `feature-merge` 的 `isolated` 比法用的干净 CARVERS 地形：和 Director 的
/// `terrain` 阶段、[`build_view`] 用的是同一个缓存入口。
///
/// Clean CARVERS terrain for the `feature-merge` `isolated` comparison: the same
/// cache entry the Director's `terrain` stage and [`build_view`] use.
pub fn clean_feature_terrain(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<GeneratedChunk, String> {
    cached_generate_chunk_with_structures(config, cx, cz).map(|arc| (*arc).clone())
}

/// 测试用：只用纯函数（`build_view` 的 3×3 + 8 个邻居中心的溢出）算出一个区块
/// 完工后的方块，和 Director 交付的盒子逐格比。
/// Test-only: a chunk's finished blocks from pure functions alone, to compare
/// cell by cell with what the Director delivers.
#[cfg(test)]
pub(crate) fn pure_feature_blocks_for_test(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Vec<Vec<u32>>, String> {
    let terrain = cached_generate_chunk_with_structures(config, cx, cz)?;
    let mut chunk = (*terrain).clone();
    merge_feature_writes(&mut chunk, feature_writes_into(config, cx, cz)?);
    Ok(chunk.blocks)
}

/// 9 个来源中心各自写进目标区块 `(cx,cz)` 的地物写，按来源下标分组。
///
/// 官方里这些写来自各区块跑到 `FEATURES` 的 `WorldGenRegion`；Bamboo 的调度不保证
/// 邻居都在本区块组装前完成，所以在中心这次 `features` 里纯函数重算一遍（带缓存），
/// 保证本区块需要的写集合**从自己的 features 结果就齐**，与邻居任务落地先后无关。
/// 组装按中心的固定先后合，1/4 线程、缓存命中与否都逐字节相同。
///
/// The 9 source centers' writes into `(cx,cz)`, grouped by source index. Vanilla
/// gets these from each center's `WorldGenRegion`; Bamboo's scheduler does not
/// guarantee neighbors finish first, so this center's `features` task recomputes
/// them purely (cached) and the chunk's write set is complete from its own task.
/// Assembly merges in fixed center order, byte-identical for 1/4 threads and
/// cache hits.
pub fn feature_writes_into(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Vec<FeatureWrites>, String> {
    let own = cached_feature_run(config, cx, cz)?;
    let mut out = vec![FeatureWrites {
        slot: CENTER as u8,
        deltas: own.deltas[CENTER].clone(),
    }];
    for dz in -1..=1 {
        for dx in -1..=1 {
            let slot = ((dz + 1) * 3 + dx + 1) as usize;
            if slot == CENTER {
                continue;
            }
            let run = cached_feature_run(config, cx + dx, cz + dz)?;
            // 本区块在「以邻居为中心」的 3×3 里的格子下标。
            // This chunk's grid index in the neighbor-centered 3×3 view.
            let index = ((-dz + 1) * 3 + (-dx + 1)) as usize;
            out.push(FeatureWrites {
                slot: slot as u8,
                deltas: run.deltas[index].clone(),
            });
        }
    }
    Ok(out)
}

/// 中心「自己的地物」写增量：`blocks[CENTER]` 相对 CARVERS 地形变了的格子。
///
/// 官方 `applyBiomeDecoration` 把中心自己的地物写进中心 ProtoChunk；把它表示成
/// 增量，好和邻居溢出的增量一起按 `TaskId` 顺序合并（设计稿 5.3.1）。
///
/// The center's own feature writes: cells where `blocks[CENTER]` differs from
/// the clean CARVERS terrain. Expressed as deltas so they merge with neighbor
/// overflow in `TaskId` order (design 5.3.1).
fn center_writes(
    cx: i32,
    cz: i32,
    terrain: &GeneratedChunk,
    result: &FeatureResult,
) -> Vec<BlockDelta> {
    let after = &result.blocks[CENTER];
    let mut out = Vec::new();
    for (y, layer) in after.iter().enumerate() {
        let world_y = MIN_Y + y as i32;
        let before = &terrain.blocks[y];
        for column in 0..256 {
            if layer[column] != before[column] {
                let x = cx * 16 + (column / 16) as i32;
                let z = cz * 16 + (column % 16) as i32;
                out.push((x, world_y, z, layer[column]));
            }
        }
    }
    out
}

/// 跑一次 `features`：返回中心盒子（干净 CARVERS）+ 本区块完工需要的全部写增量。
///
/// 本区块的方块是纯函数 F(C) = CARVERS(C) + 固定先后合并 { C 自己的地物写，
/// 8 个邻居中心溢进 C 的写 }，同格先写的赢。每一项都以对应中心的干净 3×3 视图算出，
/// 取自全进程地物缓存 [`cached_feature_run`]：每个中心整个进程只算一次，不管它是作为
/// 本区块还是作为谁的邻居被问到。所以任务只交回 `(C, 按来源分组的写)` 一条，不再给
/// 邻居发增量（邻居自己的任务会从同一份缓存取到同样的写）。结果与线程数、回执先后、
/// 邻居是不是从盘上读回都无关（`docs/performance-optimizations.md` 18.6）。
///
/// Run one `features` stage: return the center column (clean CARVERS) plus every
/// write this chunk needs at assembly. The chunk's blocks are the pure function
/// F(C) = CARVERS(C) + fixed-order merge { C's own feature writes, the 8 neighbor
/// centers' spills into C }, first writer wins per cell. Each item is computed on
/// that center's clean 3×3 view and taken from the process-wide
/// [`cached_feature_run`], so every center runs once per process whether asked for
/// as itself or as someone's neighbor. The task returns only `(C, writes grouped
/// by source)` and no longer sends deltas to neighbors (their own tasks read the
/// same cached writes). The result does not depend on thread count, receipt order
/// or whether neighbors were loaded from disk.
pub(crate) fn generate_features_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    // 宿主照常附上 8 个邻居的快照（对应官方的依赖表），但 features 任务现在不读它：
    // 地物一律在干净的 3×3 视图上从全进程缓存取。
    // The host still attaches the 8 neighbor snapshots (vanilla's dependency
    // table), but the features task no longer reads them: features always run on
    // the clean 3×3 view from the process-wide cache.
    _neighbors: SnapshotView<'_>,
) -> Result<GenerateOutput, String> {
    let _profile_timer = crate::worldgen_profile::timer(&FEATURES_NS);
    crate::worldgen_profile::count(&FEATURES_CALLS);
    // 中心快照就是 terrain 阶段交回的那份（同一个缓存入口）；没有就现算同一份。
    // The center snapshot is the terrain stage's (same cache entry); recompute
    // the very same one if absent.
    let terrain = match center {
        Some(snapshot) => {
            crate::worldgen_profile::count(&SNAPSHOT_SOME);
            snapshot
        }
        None => {
            crate::worldgen_profile::count(&SNAPSHOT_NONE);
            cached_generate_chunk_with_structures(config, cx, cz)?
        }
    };

    // `structures` 顶层标签（`starts` + `References`）由本阶段交回世界线程，
    // 完工组装时写进交付盒子，下一次存盘原样带回。
    // The `structures` tag (starts + References) goes back to the world thread
    // here; assembly installs it into the delivered column.
    let structures = if config.generate_structures {
        let references = crate::worldgen_structures::structure_references_stage(config, cx, cz)?;
        let starts = crate::worldgen_structures::structure_starts(config, cx, cz)?;
        Some(crate::worldgen_structures::encode_structures(
            config,
            &starts,
            &references,
        )?)
    } else {
        None
    };

    #[allow(unused_mut)]
    let mut groups = feature_writes_into(config, cx, cz)?;
    #[cfg(feature = "oracle-samples")]
    oracle_merge::apply_break(config, cx, cz, &mut groups)?;

    // 中心盒子先回到 CARVERS 地形，写增量由世界线程按来源分组收集，完工再组装。
    // The center column returns to CARVERS terrain here; the world thread
    // collects the source-grouped writes and assembles them at completion.
    let column = build_region_column(&terrain)?;
    Ok(GenerateOutput {
        column,
        terrain: None,
        deltas: vec![(ChunkPos::new(cx, cz), groups)],
        light: None,
        structures,
    })
}

/// 邻居溢出合并对拍（`mc-bamboo-vanilla feature-merge`）用的入口和改坏开关，
/// 只在 `oracle-samples` 下编进来。
///
/// Entry points and break switches for the neighbor-spill merge oracle
/// (`mc-bamboo-vanilla feature-merge`), compiled only with `oracle-samples`.
#[cfg(feature = "oracle-samples")]
pub mod oracle_merge {
    use std::sync::Mutex;

    use super::{
        CENTER, FeatureWrites, WorldGenConfig, feature_writes_into as game_feature_writes_into,
    };

    /// 故意改坏合并，确认对拍抓得到。
    /// A deliberate merge corruption, to prove the oracle catches it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum MergeBreak {
        /// 丢掉 `(cx+dx, cz+dz)` 这个邻居溢进本区块的写。
        /// Drop the spill from the neighbor at `(cx+dx, cz+dz)`.
        DropNeighbor { dx: i32, dz: i32 },
        /// 同格冲突退回旧规则：状态 ID 最大的赢。
        /// Revert same-cell conflicts to the old rule: the largest state id wins.
        LargestStateWins,
    }

    /// 开关只对这一个种子生效，别的世界不受影响。
    /// The switch is scoped to one seed; other worlds are unaffected.
    static MERGE_BREAK: Mutex<Option<(i64, MergeBreak)>> = Mutex::new(None);

    /// 设置或清掉改坏开关。
    /// Set or clear the break switch.
    pub fn set_merge_break(setting: Option<(i64, MergeBreak)>) {
        if let Ok(mut slot) = MERGE_BREAK.lock() {
            *slot = setting;
        }
    }

    pub(super) fn apply_break(
        config: &WorldGenConfig,
        _cx: i32,
        _cz: i32,
        groups: &mut Vec<FeatureWrites>,
    ) -> Result<(), String> {
        let setting = MERGE_BREAK.lock().ok().and_then(|slot| *slot);
        let Some((seed, mode)) = setting else {
            return Ok(());
        };
        if seed != config.seed {
            return Ok(());
        }
        match mode {
            MergeBreak::DropNeighbor { dx, dz } => {
                let slot = ((dz + 1) * 3 + dx + 1) as u8;
                groups.retain(|group| group.slot != slot);
            }
            MergeBreak::LargestStateWins => {
                // 旧规则：按 `(x,y,z,state)` 排序后同格最后一条（状态 ID 最大）生效。
                // 把全部写混进一个来源组，组内后面的写覆盖前面的，效果与旧规则一致。
                // Old rule: after sorting `(x,y,z,state)` the last entry (largest
                // state id) wins. Mixing all writes into one group in that sorted
                // order gives the same result, since later in-group writes win.
                let mut all: Vec<super::BlockDelta> = groups
                    .iter()
                    .flat_map(|group| group.deltas.iter().copied())
                    .collect();
                all.sort_unstable();
                for group in groups.iter_mut() {
                    group.deltas.clear();
                }
                groups[CENTER].deltas = all;
            }
        }
        Ok(())
    }

    /// 写进 `(cx, cz)` 的全部地物写，按来源中心分 9 份：下标 `(dz+1)*3 + (dx+1)`
    /// 是以 `(cx+dx, cz+dz)` 为中心那一遍写进本区块的（中心格是本区块自己的）。
    /// 取自 Director 用的同一份全进程地物缓存。
    ///
    /// Every feature write into `(cx, cz)`, split by source center: index
    /// `(dz+1)*3 + (dx+1)` holds the writes of the run centered on
    /// `(cx+dx, cz+dz)` (the center cell is the chunk's own). Taken from the same
    /// process-wide feature cache the Director uses.
    pub fn feature_writes_into(
        config: &WorldGenConfig,
        cx: i32,
        cz: i32,
    ) -> Result<[Vec<super::BlockDelta>; 9], String> {
        let groups = game_feature_writes_into(config, cx, cz)?;
        let mut out: [Vec<super::BlockDelta>; 9] = std::array::from_fn(|_| Vec::new());
        for group in groups {
            out[group.slot as usize] = group.deltas;
        }
        Ok(out)
    }
}

/// `terrain` 阶段：算 CARVERS 方块，并把纯数据作为邻居快照交回世界线程。
///
/// `terrain` stage: compute CARVERS blocks and hand the clean data back for the
/// world-thread neighbor snapshot cache.
pub(crate) fn generate_terrain_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<GenerateOutput, String> {
    let _profile_timer = crate::worldgen_profile::timer(&TERRAIN_NS);
    crate::worldgen_profile::count(&TERRAIN_CALLS);
    // 官方 `createNoiseChunk` 把 `Beardifier.forStructuresInChunk` 注进
    // finalDensity。依赖表（`GENERATION[4].requires = [(S,8),(B,1)]`）保证半径 8
    // 的 `structure_starts` 与本区块的 `structure_references` 已完成；Beardifier
    // 只从这些定稿数据算，并按 (seed,x,z) 全局缓存，计算池的完成顺序不影响结果。
    // Vanilla `createNoiseChunk` injects `Beardifier.forStructuresInChunk`.
    // The dependency table guarantees the radius-8 starts and center references;
    // the Beardifier is derived only from those and cached by (seed,x,z).
    let chunk = cached_generate_chunk_with_structures(config, cx, cz)?;
    let column = build_region_column(&chunk)?;
    Ok(GenerateOutput {
        column,
        terrain: Some(chunk),
        deltas: Vec::new(),
        light: None,
        structures: None,
    })
}

/// 透传阶段（群系、光照、刷怪、完工）：盒子原样交回。
///
/// Pass-through stages: return the box unchanged.
pub(crate) fn passthrough_stage(center: Option<Box<RegionColumn>>) -> GenerateOutput {
    GenerateOutput::passthrough(center.unwrap_or_else(|| Box::new(RegionColumn::from_template())))
}

/// 把一组世界坐标的写增量落进 `GeneratedChunk` 的方块数组。
///
/// Apply world-coordinate deltas into a `GeneratedChunk`'s block arrays.
pub(crate) fn apply_deltas(chunk: &mut GeneratedChunk, deltas: &[BlockDelta]) {
    for &(x, y, z, state) in deltas {
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            continue;
        }
        let column = (x & 15) as usize * 16 + (z & 15) as usize;
        chunk.blocks[(y - MIN_Y) as usize][column] = state;
    }
}

/// 完工时用「CARVERS 地形 + 按来源分组的地物写」组装交付盒子。
///
/// [`merge_feature_writes`] 按来源中心的固定先后合、同格先写的赢：追加顺序
/// （`TaskId` 批处理顺序、1 线程还是 4 线程）不影响最终方块，结果逐字节确定。
///
/// Assemble the delivered box from CARVERS terrain + source-grouped feature
/// writes. [`merge_feature_writes`] merges in a fixed center order with
/// first-writer-wins, so append order (`TaskId` batching, 1 vs 4 threads) cannot
/// change the final blocks; results are byte-identical.
pub(crate) fn assemble_feature_column(
    terrain: &GeneratedChunk,
    groups: &mut Vec<FeatureWrites>,
) -> Result<Box<RegionColumn>, String> {
    let mut chunk = terrain.clone();
    merge_feature_writes(&mut chunk, groups.drain(..));
    build_region_column(&chunk)
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

    /// 缓存的 processor_list 与每次重新解析逐项相同，且解析只发生一次。
    /// The cached processor list matches a fresh parse item for item.
    #[test]
    fn cached_processor_lists_match_a_fresh_parse() {
        let fresh = ProcessorLists::parse(
            Worldgen::load()
                .unwrap()
                .raw()
                .get("processor_list")
                .unwrap(),
        )
        .unwrap();
        let cached = processor_lists().unwrap();
        assert_eq!(
            cached.names().collect::<Vec<_>>(),
            fresh.names().collect::<Vec<_>>()
        );
        for name in fresh.names() {
            assert_eq!(
                format!("{:?}", cached.get(name)),
                format!("{:?}", fresh.get(name)),
                "{name}"
            );
        }
        // 两次调用拿到同一个进程级实例。
        // Two calls return the same process-wide instance.
        assert!(std::ptr::eq(cached, processor_lists().unwrap()));
    }

    /// 同一输入跑两遍 features，输出逐字节一致（随机源只从种子和坐标派生）。
    /// Two runs of `features` on the same input are byte-identical.
    #[test]
    fn features_are_deterministic() {
        let config = normal(2026);
        let view: [Arc<GeneratedChunk>; 9] = std::array::from_fn(|i| {
            let dz = i as i32 / 3 - 1;
            let dx = i as i32 % 3 - 1;
            cached_generate_chunk(&config, dx, dz).unwrap()
        });
        let a = run_features(&config, 0, 0, view.clone()).unwrap();
        let b = run_features(&config, 0, 0, view).unwrap();
        assert_eq!(a.blocks, b.blocks);
        assert_eq!(a.deltas, b.deltas);
    }

    /// 结构区块跑完游戏里完整流水线不报错：起点/引用/Beardifier/features 全链路。
    /// The full game pipeline runs on a structure chunk without errors.
    #[test]
    fn game_pipeline_runs_on_a_structure_chunk() {
        let config = normal(42);
        let result = game_pipeline(&config, 4, 0).unwrap();
        assert!(
            !result.starts.is_empty(),
            "chunk (4,0) has a mineshaft start"
        );
        assert_eq!(result.blocks.len(), 9);
        assert_eq!(result.blocks[4].len(), WORLD_HEIGHT as usize);
    }

    /// 同一个中心，输入视图用 EMPTY 地形与真实 Beardifier 地形时，`run_features`
    /// 给出的写增量不同：所有视图都必须走同一个带 Beardifier 的地形入口，否则
    /// 哪一份先到（调度）会改变结果。
    #[test]
    fn neighbor_spills_depend_on_which_terrain_snapshot_arrives() {
        use mc_bamboo_worldgen::beardifier::Beardifier;
        let config = normal(42);
        // 一个有 trial_chambers 起点的区块，Beardifier 非空。
        let (cx, cz) = (17, 7);
        let refs = crate::worldgen_structures::structure_references_stage(&config, cx, cz).unwrap();
        let beard = crate::worldgen_structures::beardifier_for_chunk(&config, cx, cz, &refs)
            .unwrap()
            .unwrap_or_else(|| Beardifier::for_structures_in_chunk(cx, cz, &[]));
        let empty = crate::worldgen::generate_chunk(&config, cx, cz).unwrap();
        let real =
            crate::worldgen::generate_chunk_with_beardifier(&config, cx, cz, Some(&beard)).unwrap();
        let mut differing = 0usize;
        for (a, b) in empty.blocks.iter().zip(&real.blocks) {
            differing += a.iter().zip(b).filter(|(x, y)| x != y).count();
        }
        assert!(
            differing > 0,
            "the Beardifier must change terrain on a structure chunk"
        );
        // 若把真实地形喂给邻居重算、EMPTY 地形喂给中心，两侧的结果必然不同：
        // 这就是必须由依赖表固定下来的读写竞争。
        let view_empty = std::array::from_fn(|_| {
            Arc::new(GeneratedChunk {
                blocks: empty.blocks.clone(),
                biomes: empty.biomes.clone(),
                heightmaps: Default::default(),
            })
        });
        let view_real = std::array::from_fn(|_| {
            Arc::new(GeneratedChunk {
                blocks: real.blocks.clone(),
                biomes: real.biomes.clone(),
                heightmaps: Default::default(),
            })
        });
        let a = run_features(&config, cx, cz, view_empty).unwrap();
        let b = run_features(&config, cx, cz, view_real).unwrap();
        assert_ne!(
            a.deltas, b.deltas,
            "different terrain snapshots must produce different neighbor deltas"
        );
    }

    /// 全进程地物结果缓存：命中给的写增量和现算一遍逐条相同。种子只在这条测试里
    /// 用，别的测试并行跑也碰不到这些键。
    /// The shared feature cache returns exactly what a fresh run computes. The
    /// seed is private to this test, so parallel tests never touch these keys.
    #[test]
    fn feature_cache_hit_equals_a_fresh_run() {
        let config = normal(0x0fea_7c0c);
        let (cx, cz) = (5, -3);
        let fresh = run_features(&config, cx, cz, build_view(&config, cx, cz).unwrap()).unwrap();
        let first = cached_feature_run(&config, cx, cz).unwrap();
        let again = cached_feature_run(&config, cx, cz).unwrap();
        for index in 0..9 {
            if index == CENTER {
                continue;
            }
            assert_eq!(first.deltas[index], fresh.deltas[index], "cell {index}");
            assert_eq!(again.deltas[index], fresh.deltas[index], "cell {index}");
        }
        // 中心格是中心自己的地物写，等于对干净地形逐格比出来的那份。
        // The center cell holds the center's own writes against clean terrain.
        let terrain = cached_generate_chunk_with_structures(&config, cx, cz).unwrap();
        let own = center_writes(cx, cz, &terrain, &fresh);
        assert!(!own.is_empty(), "the test center must write into itself");
        assert_eq!(first.deltas[CENTER], own);
        assert_eq!(again.deltas[CENTER], own);
        assert!(
            (0..9).any(|index| index != CENTER && !fresh.deltas[index].is_empty()),
            "the test center must spill into some neighbor"
        );
    }

    /// 多条线程同时要同一个中心：只有一条真正去算，其余等它，拿到的是同一个
    /// `Arc`（各算各的会是不同的分配）。
    /// Threads asking for the same center at once share one computation: every
    /// caller gets the very same `Arc`.
    #[test]
    fn concurrent_feature_runs_share_one_computation() {
        let config = normal(0x0fea_7c0d);
        let barrier = std::sync::Barrier::new(6);
        let runs: Vec<Arc<FeatureRun>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        cached_feature_run(&config, -7, 11).unwrap()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for run in &runs[1..] {
            assert!(Arc::ptr_eq(run, &runs[0]), "a caller recomputed the center");
        }
    }

    /// 写增量落进方块数组后能读回来。
    /// Deltas round-trip through the block arrays.
    #[test]
    fn apply_deltas_writes_the_expected_cells() {
        let config = normal(1);
        let mut chunk = (*cached_generate_chunk(&config, 0, 0).unwrap()).clone();
        apply_deltas(&mut chunk, &[(3, 40, 5, 12345), (0, MIN_Y, 0, 999)]);
        assert_eq!(chunk.blocks[(40 - MIN_Y) as usize][3 * 16 + 5], 12345);
        assert_eq!(chunk.blocks[0][0], 999);
    }

    /// `features` 阶段真的会放地物：中心自己的方块相对 CARVERS 地形有变化，
    /// 写增量非空（否则整条 features 接入就是空转）。
    /// The `features` stage really places features: the center's blocks differ
    /// from CARVERS terrain and the writes are non-empty (guard against a no-op).
    #[test]
    fn features_stage_places_blocks() {
        let config = normal(2026);
        let view = build_view(&config, 0, 0).unwrap();
        let terrain = Arc::clone(&view[CENTER]);
        let result = run_features(&config, 0, 0, view).unwrap();
        let writes = center_writes(0, 0, &terrain, &result);
        assert!(
            !writes.is_empty(),
            "ores should change some center blocks (unsupported={})",
            result.unsupported
        );
    }

    /// 同一格多次写按来源中心的固定先后、先写的赢：中心自己最先，邻居按
    /// `(dz,dx)`；追加顺序不同不改变最终方块；同一来源对同一格写两次时以它
    /// 最后写下的那条为该中心的最终状态。
    /// First-writer-wins by source order: the center first, then neighbors in
    /// `(dz,dx)`; append order does not change the final blocks, and repeated
    /// writes from one source resolve to its last write.
    #[test]
    fn assemble_resolves_conflicts_by_first_writer() {
        let config = normal(7);
        let terrain = cached_generate_chunk(&config, 0, 0).unwrap();
        let groups = |order: &[u8]| {
            order
                .iter()
                .map(|&slot| FeatureWrites {
                    slot,
                    deltas: vec![(3, 0, 5, 100 + slot as u32), (slot as i32, 1, 6, 222)],
                })
                .collect::<Vec<_>>()
        };
        let mut a = groups(&[CENTER as u8, 0, 8]);
        let mut b = groups(&[8, 0, CENTER as u8]);
        let mut c = groups(&[0, 8, CENTER as u8]);
        // 一个中心对同一格写了两次：以它最后写下的为准。
        // One center wrote the same cell twice: its last write is the final one.
        c.push(FeatureWrites {
            slot: CENTER as u8,
            deltas: vec![(3, 0, 5, 999)],
        });
        let ca = assemble_feature_column(&terrain, &mut a).unwrap();
        let cb = assemble_feature_column(&terrain, &mut b).unwrap();
        let cc = assemble_feature_column(&terrain, &mut c).unwrap();
        assert_eq!(
            ca.parts().column.chunk().block(3, 0, 5),
            100 + CENTER as u32,
            "the center writes first even when appended last"
        );
        assert_eq!(
            cb.parts().column.chunk().block(3, 0, 5),
            100 + CENTER as u32,
            "group order does not matter"
        );
        assert_eq!(
            cc.parts().column.chunk().block(3, 0, 5),
            999,
            "a source's last write to a cell is its final state"
        );
        assert_eq!(
            ca.parts().column.chunk().block(3, 0, 5),
            cb.parts().column.chunk().block(3, 0, 5)
        );
        for (wa, wb) in [(ca.parts().column.chunk(), cb.parts().column.chunk())] {
            for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
                for x in 0..16 {
                    for z in 0..16 {
                        assert_eq!(wa.block(x, y, z), wb.block(x, y, z), "at ({x},{y},{z})");
                    }
                }
            }
        }
    }

    /// 同一个中心的写增量（中心自己的写、8 份溢出）每格最多一条：中心写由
    /// 逐格比对得出，溢出由 `feature::world::last_writes` 收敛成最后一次写。
    /// Each source center's deltas contain at most one write per cell.
    #[test]
    fn feature_writes_have_one_write_per_source_and_cell() {
        let config = normal(0x0fea_7c0e);
        for &(cx, cz) in &[(5, -3), (6, -3), (5, -2)] {
            let groups = feature_writes_into(&config, cx, cz).unwrap();
            assert_eq!(groups.len(), 9);
            for group in &groups {
                let mut cells: std::collections::HashSet<(i32, i32, i32)> =
                    std::collections::HashSet::new();
                for &(x, y, z, _) in &group.deltas {
                    assert!(
                        cells.insert((x, y, z)),
                        "slot {} wrote a cell twice",
                        group.slot
                    );
                }
            }
        }
        assert!(
            feature_writes_into(&config, 5, -3)
                .unwrap()
                .iter()
                .any(|group| !group.deltas.is_empty()),
            "the test area must have feature writes"
        );
    }

    /// 合并规则和「按来源先后逐条写、同格只在第一次写时落下」等价，
    /// 且对分组顺序、缓存命中与否都不敏感（真实数据每个来源每格只有一条写）。
    /// The merge equals a literal sequential first-writer-wins pass and is
    /// insensitive to group order and cache hits (real data has one write per
    /// source per cell).
    #[test]
    fn merge_feature_writes_matches_a_sequential_first_pass() {
        let config = normal(0x0fea_7c0f);
        let terrain = cached_generate_chunk(&config, 0, 0).unwrap();
        let groups = feature_writes_into(&config, 0, 0).unwrap();
        let mut chunk = (*terrain).clone();
        merge_feature_writes(&mut chunk, groups.iter().cloned());
        let mut expected = (*terrain).clone();
        let mut written = std::collections::HashSet::new();
        let mut ordered = groups.clone();
        ordered.sort_by_key(|group| (u8::from(group.slot != CENTER as u8), group.slot));
        for group in ordered {
            for &(x, y, z, state) in &group.deltas {
                if written.insert((x, y, z)) {
                    apply_deltas(&mut expected, &[(x, y, z, state)]);
                }
            }
        }
        assert_eq!(chunk.blocks, expected.blocks);
        let mut reversed = groups;
        reversed.reverse();
        let mut again = (*terrain).clone();
        merge_feature_writes(&mut again, reversed);
        assert_eq!(again.blocks, expected.blocks);
    }
}
