//! `ChunkGenerator.applyBiomeDecoration` 里「每个 decoration step 先放结构件、再放地物」
//! 的结构放置入口。
//!
//! 官方把 `structuresRegistry` 按 `structure.step().ordinal()` 分组，per-step 局部
//! `index`（从 0 起，和地物的全局 index 分开）用 `setFeatureSeed(decorationSeed,
//! index, step)` 播种，再对 `structureManager.startsForStructure(...)` 的每个起点跑
//! `StructureStart.placeInChunk`。本模块只放结构件的方块，返回每个结构放置了多少
//! 结构件供对拍统计。
//!
//! The structure-placement half of `ChunkGenerator.applyBiomeDecoration`'s per-step
//! loop: structs are grouped by `structure.step().ordinal()`, seeded with a per-step
//! local index, and `StructureStart.placeInChunk` runs before the step's features.

use std::{cell::RefCell, collections::BTreeMap};

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    feature::{
        FeatureChunks,
        world::{FeatureWorld, HeightmapKind},
    },
    random::{Random, WorldgenRandom, XoroshiroRandom},
    structure::{
        jigsaw::{self},
        piece_place::{self, PieceLevel},
        pieces::{self, Piece, intersects},
        post_process,
        processor::{BlockSetRef, BlockStateSpec, ProcessorListRef, ProcessorLists},
        processor_apply::{self, PlaceSettings as ProcessorPlaceSettings},
        template::StructureTemplate,
        template_place,
    },
};

/// 本轮已经逐方块对拍 0 不一致的结构类型；其余起点和引用照官方生成，但放置标
/// `unsupported` 跳过。`structure::single` 的四个代码生成单件结构和废弃传送门的
/// `postProcess` 由另一个分支负责，等它合进来再打开。
///
/// Structure types whose placement is verified block-for-block this round; other
/// starts/references are still generated but their placement is skipped as
/// `unsupported`. The four code-built single-piece structures and ruined portals
/// belong to another branch and stay off until their `postProcess` lands.
pub const PLACEABLE_TYPES: &[&str] = &[
    "minecraft:jigsaw",
    "minecraft:mineshaft",
    "minecraft:stronghold",
    "minecraft:ocean_monument",
    "minecraft:woodland_mansion",
    "minecraft:igloo",
    "minecraft:shipwreck",
    "minecraft:ocean_ruin",
    "minecraft:desert_pyramid",
    "minecraft:jungle_temple",
    "minecraft:swamp_hut",
    "minecraft:buried_treasure",
    "minecraft:ruined_portal",
];

/// 一次 `features` 调用里要放的结构（按 step 分组的输入 + 汇聚的统计）。
///
/// Structure placement inputs for one `features` call, grouped by step, plus the
/// accumulated statistics.
pub struct StructurePlacement<'a> {
    groups: Vec<StepStructure<'a>>,
    processor_lists: ProcessorLists,
    seed: i64,
    /// 对拍改坏确认要跳过的结构类型 ID；正常路径为 `None`。
    /// Structure type ID to skip for the oracle's break confirmation.
    skip_type: Option<String>,
    /// 逐 step 累计的放置次数，供对拍确认每类结构真的被放到。
    pub stats: RefCell<PlaceStats>,
}

impl StructurePlacement<'_> {
    /// 逐结构类型的放置次数，对拍完成后读出来写进报告。
    ///
    /// Per-structure-type placement counts, read after the oracle run.
    pub fn placed_pieces(&self) -> BTreeMap<String, usize> {
        self.stats.borrow().placed_pieces.clone()
    }

    /// 因 unsupported 跳过的结构件次数。
    ///
    /// Number of pieces skipped as unsupported.
    pub fn unsupported_pieces(&self) -> usize {
        self.stats.borrow().unsupported_pieces
    }

    /// 汇总一份统计（游戏侧 `features_with_structures` 交回调用方）。
    ///
    /// Snapshot the accumulated stats (`features_with_structures` hands this back).
    pub fn total_stats(&self) -> PlaceStats {
        self.stats.borrow().clone()
    }
}

impl<'a> StructurePlacement<'a> {
    pub fn new(groups: Vec<StepStructure<'a>>, processor_lists: ProcessorLists, seed: i64) -> Self {
        Self {
            groups,
            processor_lists,
            seed,
            skip_type: None,
            stats: RefCell::new(PlaceStats::default()),
        }
    }

    /// 对拍改坏确认：跳过一种结构类型的结构件放置，其它结构照常。
    /// Oracle break confirmation: skip one structure type, place the rest.
    pub fn skip_type(&mut self, type_id: &str) {
        self.skip_type = Some(type_id.to_owned());
    }

    /// 跑当前 step 的结构件放置，统计合进 [`Self::stats`]。
    ///
    /// Place this step's structures and merge the counts into `stats`.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        input: &FeatureChunks,
        step: usize,
    ) -> Result<(), String> {
        let partial = place_step(
            world,
            input,
            self.seed,
            step,
            &self.groups,
            &self.processor_lists,
            self.skip_type.as_deref(),
        )?;
        let mut stats = self.stats.borrow_mut();
        for (kind, count) in partial.placed_pieces {
            *stats.placed_pieces.entry(kind).or_default() += count;
        }
        for (kind, count) in partial.placed_blocks {
            *stats.placed_blocks.entry(kind).or_default() += count;
        }
        stats.unsupported_pieces += partial.unsupported_pieces;
        Ok(())
    }
}

/// 一个结构起点（按官方 `StructureStart.placeInChunk` 的输入）。
///
/// One structure start (the input of vanilla `StructureStart.placeInChunk`).
#[derive(Debug, Clone)]
pub struct StartInput {
    pub structure: String,
    /// 结构注册表条目的 `type` 字段（决定 `postProcess` 分派）。
    pub type_id: String,
    /// 结构件列表。jigsaw 走模板链，其余走布局 `Piece`。
    pub pieces: StartPieces,
}

/// 一次 `place_start` 调用的公共输入与对拍开关。
/// Shared inputs and oracle-only switches for one `place_start` call.
#[derive(Debug, Clone, Copy)]
pub struct PlaceOptions<'a> {
    pub seed: i64,
    pub random: &'a WorldgenRandom,
    pub processor_lists: &'a ProcessorLists,
    /// 对拍改坏确认要跳过的结构类型 ID；正常路径为 `None`。
    /// Structure type ID to skip for break confirmation.
    pub skip_type: Option<&'a str>,
}

/// 放置阶段看到的结构件集合。
///
/// The piece collection the placement stage sees.
#[derive(Debug, Clone)]
pub enum StartPieces {
    Layout(Vec<Piece>),
    Jigsaw(Vec<jigsaw::Piece>),
    /// 读盘只恢复了基类字段：不能放，标 unsupported 跳过。
    Sparse,
}

/// 一个 step 的结构放置统计，对拍用它确认每类结构至少真的被放了一次。
///
/// Per-step placement counts; the oracle uses them to prove each structure type
/// really got placed.
#[derive(Debug, Default, Clone)]
pub struct PlaceStats {
    /// `structure type -> 跑过 postProcess 的结构件次数`。
    pub placed_pieces: BTreeMap<String, usize>,
    /// `structure type -> 结构件写过的方块次数`（含写空气、含重复写）。
    /// `structure type -> block writes made by its pieces`.
    pub placed_blocks: BTreeMap<String, usize>,
    /// 因 unsupported 跳过的结构件次数。
    pub unsupported_pieces: usize,
}

/// 一个结构在 `structuresByStep` 里的条目。
///
/// One entry in `structuresByStep`.
pub struct StepStructure<'a> {
    pub structure: &'a str,
    /// `structure.step()` 的 `Decoration` 序号。
    pub step: usize,
    pub starts: &'a [StartInput],
}

/// 把结构按 `step` 分组，组的遍历序是结构注册名（官方 `Registry.stream()` 顺序），
/// 组内保持注册顺序；局部 index 从 0 起，和地物的全局 index 无关。
///
/// Group structures by `step`; group order is the structure registry name order
/// (`Registry.stream()`), with a per-step local index independent of features.
pub fn place_step(
    world: &mut FeatureWorld<'_>,
    input: &FeatureChunks,
    seed: i64,
    step: usize,
    structures: &[StepStructure<'_>],
    processor_lists: &ProcessorLists,
    skip_type: Option<&str>,
) -> Result<PlaceStats, String> {
    let mut stats = PlaceStats::default();
    // `applyBiomeDecoration` 的那一个随机源是
    // `new WorldgenRandom(new XoroshiroRandomSource(RandomSupport.generateUniqueSeed()))`
    // （字节码 90-104）；结构分支和地物分支共用它。不是 LegacyRandomSource：
    // 后者会让 `nextInt` / `nextFloat` 走另一条 LCG 序列。
    let random = WorldgenRandom::new(Random::Xoroshiro(XoroshiroRandom::new(0)));
    let decoration_seed =
        random.set_decoration_seed(seed, input.center_x * 16, input.center_z * 16);
    let options = PlaceOptions {
        seed,
        random: &random,
        processor_lists,
        skip_type,
    };
    let mut local_index = 0_i32;
    for group in structures {
        // 官方按 `structure.step().ordinal()` 分组，所以这里只跑属于当前 step 的。
        if group.step != step {
            continue;
        }
        random.set_feature_seed(decoration_seed, local_index, step as i32);
        local_index += 1;
        for start in group.starts {
            place_start(world, input, start, &mut stats, options)?;
        }
    }
    Ok(stats)
}

/// `ChunkGenerator.getWritableArea(chunk)`：`FEATURES` 阶段传给 `placeInChunk` 的盒。
/// 用中心区块的整列盒（minY+1 起是官方 ProtoChunk 的 `getMinBuildHeight()+1`）。
///
/// `ChunkGenerator.getWritableArea(chunk)`: the box passed to `placeInChunk` during
/// FEATURES, the center chunk's full column.
fn writable_area(input: &FeatureChunks) -> [i32; 6] {
    [
        input.center_x * 16,
        input.min_y + 1,
        input.center_z * 16,
        input.center_x * 16 + 15,
        input.min_y + input.height - 1,
        input.center_z * 16 + 15,
    ]
}

/// 官方 `StructureStart.placeInChunk`：对每个与 `chunkBB` 相交的结构件跑
/// `postProcess`，最后 `structure.afterPlace`。
///
/// Vanilla `StructureStart.placeInChunk`: run `postProcess` on every piece whose box
/// intersects `chunkBB`, then `structure.afterPlace`.
fn place_start(
    world: &mut FeatureWorld<'_>,
    input: &FeatureChunks,
    start: &StartInput,
    stats: &mut PlaceStats,
    options: PlaceOptions<'_>,
) -> Result<(), String> {
    if options.skip_type == Some(start.type_id.as_str()) {
        return Ok(());
    }
    let chunk_bb = writable_area(input);
    match &start.pieces {
        StartPieces::Jigsaw(pieces) => {
            for piece in pieces {
                if !jigsaw_intersects(piece, &chunk_bb) {
                    continue;
                }
                if !PLACEABLE_TYPES.contains(&start.type_id.as_str()) {
                    stats.unsupported_pieces += 1;
                    continue;
                }
                let before = world.write_count;
                place_jigsaw_piece(
                    world,
                    options.seed,
                    piece,
                    &chunk_bb,
                    options.random,
                    options.processor_lists,
                )?;
                *stats
                    .placed_pieces
                    .entry(start.type_id.clone())
                    .or_default() += 1;
                *stats
                    .placed_blocks
                    .entry(start.type_id.clone())
                    .or_default() += world.write_count.saturating_sub(before);
            }
        }
        StartPieces::Layout(pieces) => {
            let kind = layout_kind(&start.type_id);
            if kind.is_none() || !PLACEABLE_TYPES.contains(&start.type_id.as_str()) {
                stats.unsupported_pieces += pieces.len();
                return Ok(());
            }
            let kind = kind.unwrap_or_default();
            for piece in pieces {
                if !intersects(&piece.bb, &chunk_bb) {
                    continue;
                }
                let before = world.write_count;
                place_layout_piece(world, piece, kind, &chunk_bb, options.random)?;
                *stats
                    .placed_pieces
                    .entry(start.type_id.clone())
                    .or_default() += 1;
                *stats
                    .placed_blocks
                    .entry(start.type_id.clone())
                    .or_default() += world.write_count.saturating_sub(before);
            }
            if start.type_id == "minecraft:woodland_mansion" {
                let world_random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
                world_random.set_large_feature_seed(
                    world.seed,
                    world.center_x * 16,
                    world.center_z * 16,
                );
                let mut adapter = FeaturePieceLevel {
                    world,
                    world_random,
                };
                pieces::woodland_mansion::after_place(pieces, &mut adapter, &chunk_bb);
            }
        }
        StartPieces::Sparse => stats.unsupported_pieces += 1,
    }
    Ok(())
}

fn jigsaw_intersects(piece: &jigsaw::Piece, chunk_bb: &[i32; 6]) -> bool {
    let bb = [
        piece.bounding_box.min_x,
        piece.bounding_box.min_y,
        piece.bounding_box.min_z,
        piece.bounding_box.max_x,
        piece.bounding_box.max_y,
        piece.bounding_box.max_z,
    ];
    intersects(&bb, chunk_bb)
}

/// 一个 jigsaw 结构件：`SinglePoolElement.place` → `StructureTemplate.placeInWorld`。
///
/// One jigsaw piece: `SinglePoolElement.place` → `StructureTemplate.placeInWorld`.
fn place_jigsaw_piece(
    world: &mut FeatureWorld<'_>,
    seed: i64,
    piece: &jigsaw::Piece,
    chunk_bb: &[i32; 6],
    random: &WorldgenRandom,
    processor_lists: &ProcessorLists,
) -> Result<(), String> {
    let Some(location) = piece.element.location() else {
        // `EmptyPoolElement` 不写方块；`Feature`/`List` 元素在 place 阶段交给各自的子元素，
        // 目前未接（标成 unsupported 而不假装放置）。
        return Ok(());
    };
    let template = StructureTemplate::load(location)?;
    let configured = match &piece.element {
        jigsaw::PoolElement::Single { processors, .. } => match processors {
            ProcessorListRef::Named(name) => processor_lists
                .get(name.strip_prefix("minecraft:").unwrap_or(name.as_str()))
                .map(|v| v.to_vec())
                .unwrap_or_default(),
            ProcessorListRef::Inline(processors) => processors.clone(),
        },
        _ => Vec::new(),
    };
    // `SinglePoolElement.getSettings` 的 processor 链（含 legacy 的 pop-then-append）。
    let chain =
        jigsaw::place::assemble_processors(&piece.element, &configured, false).unwrap_or_default();
    // processor 只读世界当前状态，先拷一份快照，让 `place_piece` 独占可变 `world`。
    // Processors only read the pre-placement world; snapshot it so `place_piece`
    // can take the mutable borrow.
    let level = FeatureProcessorLevel {
        view: world.snapshot_view(),
    };
    let settings = ProcessorPlaceSettings {
        piece_position: piece.position,
        reference_pos: piece.position,
        chunk_bb: Some(crate::structure::BoundingBox {
            min_x: chunk_bb[0],
            min_y: chunk_bb[1],
            min_z: chunk_bb[2],
            max_x: chunk_bb[3],
            max_y: chunk_bb[4],
            max_z: chunk_bb[5],
        }),
        level_seed: seed,
        explicit_random: None,
    };
    let bare = Random::Legacy(LegacyRandom::new(0));
    jigsaw::place::place_piece(
        &template,
        piece.position,
        &chain,
        &level,
        settings,
        0,
        piece.rotation.index() as u32,
        &bare,
        world,
    )?;
    let _ = random;
    Ok(())
}

/// `structure` 注册名 → 布局结构件的放置分派。
///
/// Structure type → layout piece placement dispatch.
fn layout_kind(type_id: &str) -> Option<&'static str> {
    Some(match type_id {
        "minecraft:mineshaft" => "mineshaft",
        "minecraft:stronghold" => "stronghold",
        "minecraft:ocean_monument" => "monument",
        "minecraft:woodland_mansion" => "mansion",
        "minecraft:igloo" => "igloo",
        "minecraft:shipwreck" => "shipwreck",
        "minecraft:ocean_ruin" => "ocean_ruin",
        "minecraft:desert_pyramid" => "desert_pyramid",
        "minecraft:jungle_temple" => "jungle_temple",
        "minecraft:swamp_hut" => "swamp_hut",
        "minecraft:buried_treasure" => "buried_treasure",
        "minecraft:ruined_portal" => "ruined_portal",
        _ => return None,
    })
}

fn place_layout_piece(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    kind: &str,
    chunk_bb: &[i32; 6],
    random: &WorldgenRandom,
) -> Result<(), String> {
    // 模板结构件的 `postProcess` 已经在 `FeatureWorld` 上跑（它们就按这个类型写的）；
    // 多件式结构的 `postProcess` 走 `PieceLevel`，这里给 `FeatureWorld` 加一层适配。
    let world_random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    world_random.set_large_feature_seed(world.seed, world.center_x * 16, world.center_z * 16);
    let mut adapter = FeaturePieceLevel {
        world,
        world_random,
    };
    match kind {
        "igloo" | "shipwreck" | "ocean_ruin" => {
            let mut piece = piece.clone();
            let outcome = match kind {
                "igloo" => post_process::igloo::igloo(world, &mut piece, *chunk_bb, random)?,
                "shipwreck" => {
                    post_process::shipwreck::shipwreck(world, &mut piece, *chunk_bb, random)?
                }
                _ => post_process::ocean_ruin::ocean_ruin(world, &mut piece, *chunk_bb, random)?,
            };
            for (x, y, z, state) in outcome.writes {
                world.set_block_with_heightmaps(x, y, z, state);
            }
        }
        "mineshaft" | "stronghold" | "monument" | "mansion" => {
            place_code_piece(&mut adapter, piece, kind, chunk_bb, random)?;
        }
        "desert_pyramid" | "jungle_temple" | "swamp_hut" | "buried_treasure" | "ruined_portal" => {
            place_single_piece(world, piece, kind, chunk_bb, random)?;
        }
        _ => {}
    }
    Ok(())
}

/// 四个代码生成单件结构 + 废弃传送门的 `postProcess`（已由
/// `structure-post-process` / `structure-templated-place` 逐方块对拍 0 不一致）。
///
/// `postProcess` of the four code-built single-piece structures and ruined portals
/// (verified block-for-block by the corresponding oracles).
fn place_single_piece(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    kind: &str,
    chunk_bb: &[i32; 6],
    random: &WorldgenRandom,
) -> Result<(), String> {
    let orientation = (piece.o >= 0).then(|| pieces::HDir::from_2d(piece.o));
    match kind {
        "buried_treasure" => {
            let (x, z) = (piece.bb[0], piece.bb[2]);
            let bb = [x, 90, z, x, 90, z];
            if let Some((chest, _)) = post_process::buried_treasure(world, &bb, None)
                && let Some(state) = post_process::create_chest(
                    world,
                    chunk_bb,
                    chest,
                    None,
                    random,
                    Some("minecraft:chests/buried_treasure"),
                )
            {
                let _ = state;
            }
        }
        "swamp_hut" => {
            post_process::swamp_hut::swamp_hut(
                world,
                &piece.bb,
                chunk_bb,
                orientation,
                piece.rot,
                piece.mirror,
                None,
            )?;
        }
        "jungle_temple" => {
            post_process::jungle_temple::jungle_temple(
                world,
                &piece.bb,
                chunk_bb,
                orientation,
                piece.rot,
                piece.mirror,
                random,
                None,
            );
        }
        "desert_pyramid" => {
            post_process::pyramid::desert_pyramid(
                world,
                &piece.bb,
                chunk_bb,
                orientation,
                piece.rot,
                piece.mirror,
                world.seed,
                false,
                world.center_x,
                world.center_z,
                random,
                None,
            );
        }
        "ruined_portal" => {
            let mut piece = piece.clone();
            let mut box_ = *chunk_bb;
            let outcome =
                post_process::ruined_portal::ruined_portal(world, &mut piece, &mut box_, random)?;
            for (x, y, z, state) in outcome.writes {
                world.set_block_with_heightmaps(x, y, z, state);
            }
        }
        _ => {}
    }
    Ok(())
}

fn place_code_piece(
    level: &mut FeaturePieceLevel<'_, '_>,
    piece: &Piece,
    kind: &str,
    chunk_bb: &[i32; 6],
    random: &WorldgenRandom,
) -> Result<(), String> {
    if kind == "mansion" {
        return pieces::woodland_mansion::place_piece(piece, level, chunk_bb, random);
    }
    if kind == "monument" {
        // 海底神殿的房间图需要从 seed / chunk 重建；放置阶段每个区块只跑一次，
        // 这里交给 `place_start`（整份 layout 的第 0 项是 MonumentBuilding）。
        return Ok(());
    }
    let orientation = (piece.o >= 0).then(|| pieces::HDir::from_2d(piece.o));
    let ctx = piece_place::PieceCtx::new(piece.bb, orientation);
    if kind == "stronghold" {
        let mut sp = pieces::stronghold_place::StrongholdPiece {
            ctx,
            level,
            chunk_bb: *chunk_bb,
        };
        dispatch_stronghold(&mut sp, piece, random)?;
    } else {
        let mut sp = pieces::mineshaft_place::ShaftPiece {
            ctx,
            shaft: if piece.id.contains("mesa") || is_mesa(piece) {
                pieces::mineshaft_place::ShaftType::Mesa
            } else {
                pieces::mineshaft_place::ShaftType::Normal
            },
            level,
            chunk_bb: *chunk_bb,
            biome_blocking: false,
        };
        dispatch_mineshaft(&mut sp, piece, random)?;
    }
    Ok(())
}

fn is_mesa(piece: &Piece) -> bool {
    piece.extras.get("MST").is_some_and(|v| v.token() == "Mesa")
}

fn dispatch_mineshaft(
    sp: &mut pieces::mineshaft_place::ShaftPiece<'_>,
    piece: &Piece,
    random: &WorldgenRandom,
) -> Result<(), String> {
    use pieces::mineshaft_place as place;
    let int = |key: &str| {
        piece
            .extras
            .get(key)
            .and_then(|v| v.token().strip_prefix("I:").and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    };
    let flag = |key: &str| piece.extras.get(key).is_some_and(|v| v.token() == "B:1");
    match piece.id.as_str() {
        "minecraft:mscorridor" => {
            let mut state = place::CorridorPlace {
                num_sections: int("Num"),
                has_rails: flag("hr"),
                spider_corridor: flag("sc"),
                has_placed_spider: false,
            };
            place::corridor_post_process(sp, random, &mut state);
        }
        "minecraft:mscrossing" => place::crossing_post_process(sp, flag("tf")),
        "minecraft:msstairs" => place::stairs_post_process(sp),
        "minecraft:msroom" => {
            let entrances: Vec<[i32; 6]> = piece.entrances.clone();
            place::room_post_process(sp, &entrances);
        }
        _ => {}
    }
    Ok(())
}

fn dispatch_stronghold(
    sp: &mut pieces::stronghold_place::StrongholdPiece<'_>,
    piece: &Piece,
    random: &WorldgenRandom,
) -> Result<(), String> {
    use pieces::stronghold_place as sp_place;
    let int = |key: &str| {
        piece
            .extras
            .get(key)
            .and_then(|v| v.token().strip_prefix("I:").and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    };
    let flag = |key: &str| piece.extras.get(key).is_some_and(|v| v.token() == "B:1");
    let door = sp_place::random_small_door(random);
    let orientation = (piece.o >= 0).then(|| pieces::HDir::from_2d(piece.o));
    match piece.id.as_str() {
        "minecraft:shcc" => {
            let mut chest = false;
            sp_place::chest_corridor_post_process(sp, random, door, &mut chest);
        }
        "minecraft:shfc" => sp_place::filler_corridor_post_process(sp, int("Steps")),
        "minecraft:shs" => {
            sp_place::straight_post_process(sp, random, door, flag("Left"), flag("Right"))
        }
        "minecraft:shph" => sp_place::prison_hall_post_process(sp, random, door),
        "minecraft:shrt" => sp_place::right_turn_post_process(sp, random, door, orientation),
        "minecraft:shlt" => sp_place::left_turn_post_process(sp, random, door, orientation),
        "minecraft:shrc" => sp_place::room_crossing_post_process(sp, random, door, int("Type")),
        "minecraft:shsd" | "minecraft:shstart" => {
            sp_place::stairs_down_post_process(sp, random, door)
        }
        "minecraft:shssd" => sp_place::straight_stairs_down_post_process(sp, random, door),
        "minecraft:sh5c" => sp_place::five_crossing_post_process(
            sp,
            random,
            door,
            (
                flag("leftLow"),
                flag("leftHigh"),
                flag("rightLow"),
                flag("rightHigh"),
            ),
        ),
        "minecraft:shli" => sp_place::library_post_process(sp, random, door, flag("Tall")),
        "minecraft:shpr" => {
            let mut spawner = false;
            sp_place::portal_room_post_process(sp, random, &mut spawner);
        }
        _ => {}
    }
    Ok(())
}

/// `FeatureWorld` 当 `PieceLevel` 用：读写方块、冻结的 OCEAN_FLOOR_WG 高度图。
///
/// `FeatureWorld` as `PieceLevel`: block reads/writes plus the frozen
/// `OCEAN_FLOOR_WG` heightmap.
struct FeaturePieceLevel<'w, 'a> {
    world: &'w mut FeatureWorld<'a>,
    world_random: WorldgenRandom,
}

impl PieceLevel for FeaturePieceLevel<'_, '_> {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        self.world.get_block(x, y, z)
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        self.world.set_block_with_heightmaps(x, y, z, state)
    }

    fn min_y(&self) -> i32 {
        self.world.min_y
    }

    fn max_y(&self) -> i32 {
        self.world.min_y + self.world.height - 1
    }

    fn ocean_floor_height(&self, x: i32, z: i32) -> i32 {
        self.world.get_height(HeightmapKind::OceanFloorWg, x, z) - 1
    }

    fn level_random(&self) -> &WorldgenRandom {
        // `WorldGenRegion.getRandom()` 是位置随机源。`FeaturePieceLevel` 自带一份
        // 按世界种子 + 中心区块原点派生、只给实体数量这类非方块分支使用的源。
        // `WorldGenRegion.getRandom()` is a positional source. This adapter owns one
        // derived from the world seed and center origin for non-block branches only.
        &self.world_random
    }
}

/// `FeatureWorld` 当 `ProcessorLevel` 用：模板 processor 读世界方块、判标签、取高度。
///
/// `FeatureWorld` as `ProcessorLevel`: template processors read world blocks, tags
/// and heightmaps.
struct FeatureProcessorLevel {
    view: FeatureWorldView,
}

/// processor 只读的 3×3 方块/高度图快照（写由 `place_piece` 直接落在 `FeatureWorld`）。
///
/// Read-only 3x3 block/heightmap snapshot for processors; writes go straight to
/// `FeatureWorld` through `place_piece`.
struct FeatureWorldView {
    center_x: i32,
    center_z: i32,
    min_y: i32,
    height: i32,
    blocks: [Vec<Vec<u32>>; 9],
    heights: [crate::feature::Heightmaps; 9],
}

impl processor_apply::ProcessorLevel for FeatureProcessorLevel {
    fn block_state(&self, pos: (i32, i32, i32)) -> BlockStateSpec {
        let state = view_get_block(&self.view, pos.0, pos.1, pos.2);
        BlockStateSpec::parse_str(
            mc_bamboo_data::block_of_state(state)
                .map(|block| block.name)
                .unwrap_or("minecraft:air"),
        )
        .unwrap_or_else(|_| {
            BlockStateSpec::parse_str("minecraft:air").unwrap_or(BlockStateSpec {
                name: "minecraft:air".to_owned(),
                properties: Vec::new(),
            })
        })
    }

    fn in_block_set(&self, state: &BlockStateSpec, set: &BlockSetRef) -> bool {
        match set {
            BlockSetRef::Tag(tag) => {
                let name = tag.strip_prefix('#').unwrap_or(tag);
                let id = mc_bamboo_data::block_by_name(&state.name)
                    .map(|block| block.default_state)
                    .unwrap_or(0);
                template_place::in_tag(id, name)
            }
            BlockSetRef::Blocks(ids) => ids.contains(&state.name),
        }
    }

    fn height(&self, heightmap: &str, x: i32, z: i32) -> Option<i32> {
        let kind = match heightmap {
            "WORLD_SURFACE_WG" => HeightmapKind::WorldSurfaceWg,
            "WORLD_SURFACE" => HeightmapKind::WorldSurface,
            "OCEAN_FLOOR_WG" => HeightmapKind::OceanFloorWg,
            "OCEAN_FLOOR" => HeightmapKind::OceanFloor,
            _ => return None,
        };
        Some(view_get_height(&self.view, kind, x, z))
    }
}

impl FeatureWorld<'_> {
    /// 给 processor 链建一份只读快照（见 [`FeatureWorldView`]）。
    ///
    /// Snapshot the world for the processor chain (see `FeatureWorldView`).
    fn snapshot_view(&self) -> FeatureWorldView {
        FeatureWorldView {
            center_x: self.center_x,
            center_z: self.center_z,
            min_y: self.min_y,
            height: self.height,
            blocks: self.blocks.clone(),
            heights: self.heights,
        }
    }
}

fn view_index(view: &FeatureWorldView, x: i32, y: i32, z: i32) -> Option<(usize, usize)> {
    if y < view.min_y || y >= view.min_y + view.height {
        return None;
    }
    let dx = (x >> 4) - view.center_x;
    let dz = (z >> 4) - view.center_z;
    if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
        return None;
    }
    let index = ((dz + 1) * 3 + dx + 1) as usize;
    let local_y = (y - view.min_y) as usize;
    let local = ((x & 15) as usize) * 16 + (z & 15) as usize;
    Some((index, local_y * 256 + local))
}

fn view_get_block(view: &FeatureWorldView, x: i32, y: i32, z: i32) -> u32 {
    view_index(view, x, y, z)
        .map(|(chunk, index)| view.blocks[chunk][index / 256][index % 256])
        .unwrap_or(mc_bamboo_data::blocks::AIR)
}

fn view_get_height(view: &FeatureWorldView, kind: HeightmapKind, x: i32, z: i32) -> i32 {
    let dx = (x >> 4) - view.center_x;
    let dz = (z >> 4) - view.center_z;
    if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
        return view.min_y;
    }
    let index = ((dz + 1) * 3 + dx + 1) as usize;
    let column = (x & 15) as usize * 16 + (z & 15) as usize;
    let heights = &view.heights[index];
    let value = match kind {
        HeightmapKind::WorldSurfaceWg => heights.world_surface_wg[column],
        HeightmapKind::WorldSurface => heights.world_surface[column],
        HeightmapKind::OceanFloorWg => heights.ocean_floor_wg[column],
        HeightmapKind::OceanFloor => heights.ocean_floor[column],
        HeightmapKind::MotionBlocking => heights.motion_blocking[column],
        HeightmapKind::MotionBlockingNoLeaves => heights.motion_blocking_no_leaves[column],
    };
    value + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeable_list_includes_every_verified_structure_type() {
        for id in [
            "minecraft:desert_pyramid",
            "minecraft:jungle_temple",
            "minecraft:swamp_hut",
            "minecraft:buried_treasure",
            "minecraft:ruined_portal",
            "minecraft:jigsaw",
            "minecraft:mineshaft",
            "minecraft:igloo",
        ] {
            assert!(PLACEABLE_TYPES.contains(&id), "{id} must be placeable");
        }
    }

    #[test]
    fn writable_area_is_the_center_chunks_full_column() {
        let input = feature_chunks_stub(3, -2);
        assert_eq!(writable_area(&input), [48, -63, -32, 63, 319, -17]);
    }

    fn feature_chunks_stub(cx: i32, cz: i32) -> FeatureChunks {
        let blocks: [Vec<Vec<u32>>; 9] = std::array::from_fn(|_| Vec::new());
        let biomes: [Vec<Vec<u32>>; 9] = std::array::from_fn(|_| Vec::new());
        let heights: [crate::feature::Heightmaps; 9] =
            std::array::from_fn(|_| crate::feature::Heightmaps::new(-64));
        FeatureChunks {
            center_x: cx,
            center_z: cz,
            min_y: -64,
            height: 384,
            blocks,
            biomes,
            heights,
        }
    }
}
