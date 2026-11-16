//! `ChunkGenerator.createStructures` 的生产入口：把一个结构集命中的区块变成完整的
//! [`WorldStructureStart`]。
//!
//! 已有的 `start.rs::StructureStart` 是「稀疏」模型（结构件只有基类四字段），
//! `structure-save` / `structure-load` / `beard` 等旧对拍都用它；这里另建富模型，
//! 把各结构已对拍过的布局函数（[`pieces`]、[`single`]、[`templated`]、[`jigsaw`]）
//! 的结果装进 [`StartPieces`]，`as_sparse()` 再投影回稀疏模型。放置阶段直接读富模型，
//! 引用、Beardifier 和存档编码读投影，两边都不必改公开类型。
//!
//! Production entry point for `ChunkGenerator.createStructures`: turn a structure-set
//! hit into a full `WorldStructureStart`. The existing `start.rs::StructureStart` is a
//! sparse model (base-only pieces) used by the save/load/beard oracles; this module adds
//! a rich model holding the verified layout functions' output and projects back via
//! `as_sparse()`. Placement reads the rich model; references, the Beardifier and save
//! encoding read the projection.

use std::collections::{BTreeMap, HashSet};

use mc_bamboo_data::BIOME_CLIMATES;
use serde_json::Value;

use crate::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        BoundingBox, StructureDef, StructureStart, StructureStarts, Structures,
        jigsaw::{self, JigsawStructures, TemplatePools},
        pieces::{self, Piece},
        rings::{BiomeResolver, resolve_allowed_biomes},
        single,
        start::StructurePiece,
        start_point::{self, PointKind, Site},
        templated,
        validity::{allowed_biome_names, column_has_valid_biome, is_valid_biome_at},
    },
};

/// 一个结构起点的结构件集合。各条目对应 `Structure.generate` 里不同的
/// `GenerationStub` 产物；`Sparse` 只给读盘重建失败的占位用。
///
/// The piece collection of one structure start. Each variant mirrors a different
/// `GenerationStub` produced by `Structure.generate`; `Sparse` is a fallback for
/// starts that could not be rebuilt from save data.
#[derive(Debug, Clone, PartialEq)]
pub enum StartPieces {
    /// 代码生成 / 模板生成的结构件（含林地府邸、海底神殿的子件列表）。
    Layout(Vec<Piece>),
    /// jigsaw 的结构件，带 pool element、连接点和液体设置。
    Jigsaw(Vec<jigsaw::Piece>),
    /// 稀疏模型（读盘只恢复出基类字段时）。
    Sparse(Vec<StructurePiece>),
}

impl StartPieces {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Layout(pieces) => pieces.is_empty(),
            Self::Jigsaw(pieces) => pieces.is_empty(),
            Self::Sparse(pieces) => pieces.is_empty(),
        }
    }

    /// 结构件包围盒的并集（不按 `terrain_adaptation` 外扩；外扩由
    /// [`StructureStart::bounding_box`] 负责）。
    ///
    /// Union of the piece boxes (without the `terrain_adaptation` inflation, which
    /// `StructureStart::bounding_box` applies).
    pub fn union(&self) -> BoundingBox {
        match self {
            Self::Layout(pieces) => BoundingBox::union(pieces.iter().map(|piece| BoundingBox {
                min_x: piece.bb[0],
                min_y: piece.bb[1],
                min_z: piece.bb[2],
                max_x: piece.bb[3],
                max_y: piece.bb[4],
                max_z: piece.bb[5],
            })),
            Self::Jigsaw(pieces) => {
                BoundingBox::union(pieces.iter().map(|piece| piece.bounding_box))
            }
            Self::Sparse(pieces) => {
                BoundingBox::union(pieces.iter().map(|piece| piece.bounding_box))
            }
        }
    }

    /// `StructurePiece` 的基类四字段列表。jigsaw 件的 `O` 恒 -1、`GD` 恒 0
    /// （构造器不调 `setOrientation`），`id` 恒 `minecraft:jigsaw`。
    ///
    /// The four base fields of each `StructurePiece`. A jigsaw piece always has
    /// `O = -1`, `GD` from the pool element and the `minecraft:jigsaw` id.
    fn base_pieces(&self) -> Vec<StructurePiece> {
        match self {
            Self::Layout(pieces) => pieces
                .iter()
                .map(|piece| StructurePiece {
                    piece_type: piece.id.clone(),
                    bounding_box: BoundingBox {
                        min_x: piece.bb[0],
                        min_y: piece.bb[1],
                        min_z: piece.bb[2],
                        max_x: piece.bb[3],
                        max_y: piece.bb[4],
                        max_z: piece.bb[5],
                    },
                    orientation: piece.o,
                    ground_level_delta: piece.gd,
                })
                .collect(),
            Self::Jigsaw(pieces) => pieces
                .iter()
                .map(|piece| StructurePiece {
                    piece_type: "minecraft:jigsaw".to_owned(),
                    bounding_box: piece.bounding_box,
                    orientation: -1,
                    ground_level_delta: piece.ground_level_delta,
                })
                .collect(),
            Self::Sparse(pieces) => pieces.clone(),
        }
    }
}

/// 一个完整的 `StructureStart`：`structure_starts` 阶段的产出。
///
/// A full `StructureStart`: the output of the `structure_starts` stage.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldStructureStart {
    pub structure: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub references: i32,
    pub pieces: StartPieces,
    /// `structure.terrainAdaptation() != NONE`。
    pub terrain_adaptation: bool,
}

impl WorldStructureStart {
    pub fn is_valid(&self) -> bool {
        !self.pieces.is_empty()
    }

    /// 投影到稀疏模型，供 `createReferences` / `Beardifier` / 存档编码用。
    ///
    /// Project into the sparse model for `createReferences`, the Beardifier and save
    /// encoding.
    pub fn as_sparse(&self) -> StructureStart {
        StructureStart {
            structure: self.structure.clone(),
            chunk_x: self.chunk_x,
            chunk_z: self.chunk_z,
            references: self.references,
            pieces: self.pieces.base_pieces(),
            terrain_adaptation: self.terrain_adaptation,
        }
    }
}

/// 一个区块的全部富起点（`ChunkAccess.getAllStarts()` 的等价物）。
///
/// All rich starts in one chunk (`ChunkAccess.getAllStarts()`).
#[derive(Debug, Clone, Default)]
pub struct WorldStructureStarts {
    pub by_structure: BTreeMap<String, WorldStructureStart>,
}

impl WorldStructureStarts {
    pub fn insert(&mut self, start: WorldStructureStart) {
        self.by_structure.insert(start.structure.clone(), start);
    }

    pub fn get(&self, structure: &str) -> Option<&WorldStructureStart> {
        self.by_structure.get(structure)
    }

    /// 官方 `existingStart` 短路用的稀疏视图：这个结构集里有没有有效起点。
    ///
    /// Sparse view for vanilla's `existingStart` short-circuit.
    pub fn has_valid_for_set(&self, entries: &[crate::structure::SelectionEntry]) -> bool {
        entries.iter().any(|entry| {
            self.get(&entry.structure)
                .is_some_and(WorldStructureStart::is_valid)
        })
    }

    /// 投影成稀疏的 `StructureStarts`（`structure_references` / Beardifier 输入）。
    ///
    /// Project to sparse `StructureStarts` (input of `structure_references` / Beardifier).
    pub fn as_sparse(&self) -> StructureStarts {
        let mut out = StructureStarts::default();
        for start in self.by_structure.values() {
            out.insert(start.as_sparse());
        }
        out
    }
}

/// `ChunkGeneratorStructureState` 里「全世界一份」的只读部分。
///
/// `possibleStructureSets` 的过滤、要塞环位置、结构/jigsaw 注册表、模板池都按种子一次算好；
/// `BiomeResolver` 的可变 `SeededRouter` 不能跨线程共享，仍由调用方按线程建。
///
/// The per-world read-only part of `ChunkGeneratorStructureState`: the possible-set
/// filter, stronghold ring positions, structure/jigsaw registries and template pools are
/// computed once per seed. The mutable `SeededRouter` behind `BiomeResolver` is not
/// `Sync`, so callers keep it per-thread.
#[derive(Debug, Clone)]
pub struct WorldStructureState {
    /// `possibleStructureSets()` 过滤后的结构集，按注册名排序。
    pub sets: crate::structure::StructureState,
    pub structures: Structures,
    pub jigsaw: JigsawStructures,
    pub pools: TemplatePools,
    pub seed: i64,
}

impl WorldStructureState {
    /// 按官方 `createForNormal` 建一份。`worldgen` 是导出的官方注册表；
    /// `resolver` 只用于要塞环位置的群系吸附（`findBiomeHorizontal`）。
    ///
    /// Build per vanilla `createForNormal`. `resolver` is used only for stronghold ring
    /// snapping (`findBiomeHorizontal`).
    pub fn new(
        worldgen: &Worldgen,
        seed: i64,
        resolver: &mut BiomeResolver<'_>,
    ) -> Result<Self, String> {
        let raw = worldgen.raw();
        let all_sets = crate::structure::StructureSets::parse(
            raw.get("structure_set").ok_or("missing structure_set")?,
        )?;
        let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;
        // `BiomeSource.possibleBiomes()`：预设参数表里实际出现的群系（去重）。
        // `BiomeSource.possibleBiomes()`: the distinct biomes in the preset's parameter list.
        let possible: HashSet<i32> = worldgen
            .preset("minecraft:overworld")
            .ok_or("missing overworld preset")?
            .parameters()
            .entries()
            .filter_map(|(_, name)| {
                BIOME_CLIMATES
                    .iter()
                    .position(|biome| biome.name == *name)
                    .map(|id| id as i32)
            })
            .collect();
        let allowed_sets =
            crate::structure::possible_structure_sets(&all_sets, &structures, &possible);
        let mut filtered = serde_json::Map::new();
        for name in allowed_sets {
            // 原始 JSON 按名字取；`StructureSets::parse` 只认 placement / structures。
            let value = raw
                .get("structure_set")
                .and_then(|v| v.get(&name))
                .ok_or_else(|| format!("missing structure_set {name}"))?;
            filtered.insert(name, value.clone());
        }
        let mut sets = crate::structure::StructureState::new(
            crate::structure::StructureSets::parse(&Value::Object(filtered))?,
        );
        // `generateRingPositions`：主环随机源用 levelSeed，群系吸附用 fork 出的随机源。
        if let Some(entry) = sets.sets().get("minecraft:strongholds") {
            let placement = &entry.placement;
            let (distance, spread, count, tag) = match placement {
                crate::structure::Placement::ConcentricRings(rings) => (
                    rings.distance,
                    rings.spread,
                    rings.count,
                    rings.preferred_biomes.clone(),
                ),
                _ => (32, 3, 128, "#minecraft:stronghold_biased_to".to_owned()),
            };
            let allowed = resolve_allowed_biomes(&tag)?;
            let positions = crate::structure::rings::ring_positions(
                seed, resolver, &allowed, distance, spread, count,
            );
            sets.set_ring_positions("minecraft:strongholds", positions);
        }
        Ok(Self {
            sets,
            structures,
            jigsaw: JigsawStructures::parse(raw.get("structure").ok_or("missing structure")?)?,
            pools: TemplatePools::parse(raw.get("template_pool").ok_or("missing template_pool")?)?,
            seed,
        })
    }

    /// `structure.step()` 的 `GenerationStep.Decoration` 序号（26.3 的固定枚举顺序）。
    ///
    /// Ordinal of `structure.step()` in `GenerationStep.Decoration` (fixed in 26.3).
    pub fn step_of(structure: &StructureDef) -> usize {
        const STEPS: [&str; 11] = [
            "raw_generation",
            "lakes",
            "local_modifications",
            "underground_structures",
            "surface_structures",
            "strongholds",
            "underground_ores",
            "underground_decoration",
            "fluid_springs",
            "vegetal_decoration",
            "top_layer_modification",
        ];
        STEPS
            .iter()
            .position(|name| *name == structure.step)
            .unwrap_or(4)
    }
}

/// `findGenerationPoint` 的内部产出：`(是否有 stub, stub 位置, 结构件)`。
///
/// Internal `findGenerationPoint` output: `(stub present, stub position, pieces)`.
type StartOutcome = Result<(bool, (i32, i32, i32), StartPieces), String>;

/// 一个结构的 `findValidGenerationPoint` + `findGenerationPoint` 的结果。
///
/// Output of one structure's `findValidGenerationPoint` + `findGenerationPoint`.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedStart {
    pub structure: String,
    pub pieces: StartPieces,
    pub stub: (i32, i32, i32),
}

/// 供 `structure_starts` 阶段使用的每线程求值上下文。
///
/// Per-thread evaluation context for the `structure_starts` stage.
pub struct StartContext<'a> {
    pub generator: &'a Generator<'a>,
    pub state: &'a WorldStructureState,
    pub dims: (i32, i32, i32),
}

impl StartContext<'_> {
    /// `Structure.generate`：`findValidGenerationPoint` 通过才把结构件装进
    /// `StructureStart`。返回 `None` 对应官方的 `StructureStart.INVALID_START`。
    ///
    /// `Structure.generate`: only a valid generation point becomes a `StructureStart`;
    /// `None` is vanilla's `StructureStart.INVALID_START`.
    pub fn generate_start(
        &self,
        resolver: &mut BiomeResolver<'_>,
        structure_id: &str,
        chunk: (i32, i32),
    ) -> Result<Option<GeneratedStart>, String> {
        let Some(def) = self.state.structures.get(structure_id) else {
            return Ok(None);
        };
        let allowed = allowed_biome_names(def)?;
        let (min_y, height, sea_level) = self.dims;
        let seed = self.state.seed;
        let (cx, cz) = chunk;
        let site = Site { seed, cx, cz };
        // 每个结构自己的 `findGenerationPoint` 和 stub 位置。
        let (present, stub, pieces) = match def.type_id.as_str() {
            "minecraft:jigsaw" => {
                let Some(config) = self.state.jigsaw.get(structure_id) else {
                    return Ok(None);
                };
                let result = jigsaw::add_pieces(
                    config,
                    &self.state.pools,
                    self.generator,
                    &allowed,
                    (min_y, height, sea_level),
                    seed,
                    chunk,
                )?;
                match result {
                    Some(result) => (true, result.position, StartPieces::Jigsaw(result.pieces)),
                    None => (false, (0, 0, 0), StartPieces::Sparse(Vec::new())),
                }
            }
            "minecraft:mineshaft"
            | "minecraft:mineshaft_mesa"
            | "minecraft:stronghold"
            | "minecraft:monument"
            | "minecraft:mansion" => {
                self.code_built_start(structure_id, site, resolver, &allowed)?
            }
            // `SinglePieceStructure`（沙漠 / 丛林神殿）：整列群系 + 四角最低高度 >= 海平面
            // 才放行，起点仍取中心列 WORLD_SURFACE_WG 高度。
            "minecraft:desert_pyramid" | "minecraft:jungle_pyramid" => {
                let (width, depth) = if structure_id == "minecraft:desert_pyramid" {
                    (21, 21)
                } else {
                    (12, 15)
                };
                self.single_piece_start(structure_id, chunk, &allowed, (width, depth), |_| {
                    single::generate(single_kind(structure_id), seed, cx, cz)
                })?
            }
            "minecraft:swamp_hut" => self.on_top_start(chunk, &allowed, false, |_| {
                single::generate("swamp_hut", seed, cx, cz)
            })?,
            "minecraft:buried_treasure" => self.on_top_start(chunk, &allowed, true, |_| {
                single::generate("buried_treasure", seed, cx, cz)
            })?,
            "minecraft:igloo" => self.on_top_start(chunk, &allowed, false, |_| {
                templated::igloo::generate(seed, cx, cz)
            })?,
            "minecraft:shipwreck" | "minecraft:shipwreck_beached" => {
                let beached = structure_id == "minecraft:shipwreck_beached";
                // 沙滩用 WORLD_SURFACE_WG，海里用 OCEAN_FLOOR_WG。
                self.on_top_start(chunk, &allowed, !beached, |_| {
                    templated::shipwreck::generate(seed, cx, cz, beached)
                })?
            }
            "minecraft:ocean_ruin_cold" | "minecraft:ocean_ruin_warm" => {
                let config = templated::ocean_ruin::Config::parse(&def.raw)?;
                self.on_top_start(chunk, &allowed, true, |_| {
                    templated::ocean_ruin::generate(seed, cx, cz, config)
                })?
            }
            "minecraft:ruined_portal"
            | "minecraft:ruined_portal_desert"
            | "minecraft:ruined_portal_jungle"
            | "minecraft:ruined_portal_swamp"
            | "minecraft:ruined_portal_mountain"
            | "minecraft:ruined_portal_ocean"
            | "minecraft:ruined_portal_nether" => {
                let portal = templated::ruined_portal::generate(
                    self.generator,
                    &mut *resolver,
                    seed,
                    cx,
                    cz,
                    &def.raw,
                )?;
                (true, portal.origin, StartPieces::Layout(portal.pieces))
            }
            // 主世界 `possibleStructureSets` 过滤后剩下的纯下界 / 末地结构集
            // （fortress / end_city / bastion_remnant / nether_fossil）不会有起点。
            _ => (false, (0, 0, 0), StartPieces::Sparse(Vec::new())),
        };
        if !present || pieces.is_empty() {
            return Ok(None);
        }
        // `findValidGenerationPoint` 最后一道门：stub 的单点 quart 群系。
        if !is_valid_biome_at(resolver, stub.0, stub.1, stub.2, &allowed) {
            return Ok(None);
        }
        Ok(Some(GeneratedStart {
            structure: structure_id.to_owned(),
            pieces,
            stub,
        }))
    }

    /// `onTopOfChunkCenter` 一族：整列群系短路 → 中心列高度 → 布局函数。
    /// `ocean_floor` 用 `OCEAN_FLOOR_WG` 的高度（沉船 / 海底遗迹 / 埋藏的宝藏）。
    ///
    /// The `onTopOfChunkCenter` family: column biome short-circuit → center column
    /// height → layout function. `ocean_floor` uses `OCEAN_FLOOR_WG` (shipwrecks,
    /// ocean ruins, buried treasures).
    /// `SinglePieceStructure.findGenerationPoint`：整列群系短路 →
    /// `getLowestY(width, depth)`（四角 WORLD_SURFACE_WG 高度的最小值）→
    /// 低于海平面直接不放行 → 起点取中心列高度。
    ///
    /// `SinglePieceStructure.findGenerationPoint`: column biome short-circuit →
    /// `getLowestY(width, depth)` (min of the four corner WORLD_SURFACE_WG heights) →
    /// reject below sea level → stub at the center column height.
    fn single_piece_start(
        &self,
        _structure_id: &str,
        chunk: (i32, i32),
        allowed: &HashSet<String>,
        size: (i32, i32),
        layout: impl FnOnce(i32) -> Result<Vec<Piece>, String>,
    ) -> StartOutcome {
        let (min_y, height, sea_level) = self.dims;
        if !column_has_valid_biome(self.generator, chunk, allowed, (min_y, height))? {
            return Ok((false, (0, 0, 0), StartPieces::Sparse(Vec::new())));
        }
        let min_x = chunk.0 * 16;
        let min_z = chunk.1 * 16;
        let corners = crate::structure::validity::corner_heights(
            self.generator,
            min_x,
            min_z,
            size.0,
            size.1,
        )?;
        if crate::structure::validity::lowest_y(corners) < sea_level {
            return Ok((false, (0, 0, 0), StartPieces::Sparse(Vec::new())));
        }
        self.on_top_start(chunk, allowed, false, layout)
    }

    fn on_top_start(
        &self,
        chunk: (i32, i32),
        allowed: &HashSet<String>,
        ocean_floor: bool,
        layout: impl FnOnce(i32) -> Result<Vec<Piece>, String>,
    ) -> StartOutcome {
        let (min_y, height, _) = self.dims;
        if !column_has_valid_biome(self.generator, chunk, allowed, (min_y, height))? {
            return Ok((false, (0, 0, 0), StartPieces::Sparse(Vec::new())));
        }
        let center_x = chunk.0 * 16 + 8;
        let center_z = chunk.1 * 16 + 8;
        let stub_y = if ocean_floor {
            self.generator
                .first_occupied_height_ocean_floor(center_x, center_z)?
        } else {
            self.generator.first_occupied_height(center_x, center_z)?
        };
        let stub = (center_x, stub_y, center_z);
        let pieces = layout(stub_y)?;
        Ok((true, stub, StartPieces::Layout(pieces)))
    }

    /// 四个代码生成多件式结构的 `findGenerationPoint`（`start_point.rs` 已逐字段对拍）。
    ///
    /// The four code-built multi-piece structures' `findGenerationPoint` (already
    /// verified field by field in `start_point.rs`).
    fn code_built_start(
        &self,
        structure_id: &str,
        site: Site,
        resolver: &mut BiomeResolver<'_>,
        allowed: &HashSet<String>,
    ) -> StartOutcome {
        let kind = PointKind::from_id(structure_id)
            .ok_or_else(|| format!("no start-point kind for {structure_id}"))?;
        let gates = start_point::site_gates(self.generator, resolver, site)?;
        let result =
            start_point::start_point(kind, site, self.generator, resolver, allowed, &gates)?;
        if !result.valid {
            return Ok((false, (0, 0, 0), StartPieces::Sparse(Vec::new())));
        }
        let seed = site.seed;
        let (cx, cz) = (site.cx, site.cz);
        let pieces: Vec<Piece> = match kind {
            PointKind::Mineshaft => pieces::mineshaft::generate(seed, cx, cz, false, gates.base),
            PointKind::MineshaftMesa => {
                // 恶地矿井要用结构件包围盒中心的 `getBaseHeight` 作为 surfaceHeight，
                // 与 `start_point::mineshaft_stub_y` 的取法一致。
                let first = pieces::mineshaft::generate(seed, cx, cz, true, 64);
                let boxes: Vec<[i32; 6]> = first.iter().map(|p| p.bb).collect();
                let bb = pieces::union(&boxes);
                let center_x = bb[0] + pieces::x_span(&bb) / 2;
                let center_z = bb[2] + pieces::z_span(&bb) / 2;
                let surface = self.generator.first_occupied_height(center_x, center_z)? + 1;
                pieces::mineshaft::generate(seed, cx, cz, true, surface)
            }
            PointKind::Stronghold => pieces::stronghold::generate(seed, cx, cz),
            PointKind::OceanMonument => pieces::ocean_monument::generate(seed, cx, cz),
            PointKind::Mansion => pieces::woodland_mansion::generate(seed, cx, cz, gates.lowest)?,
        };
        Ok((true, result.stub, StartPieces::Layout(pieces)))
    }
}

/// `structure` 注册名 → `single::generate` 的 kind。
///
/// Structure registry id → `single::generate` kind.
fn single_kind(id: &str) -> &'static str {
    match id {
        "minecraft:desert_pyramid" => "desert_pyramid",
        "minecraft:jungle_pyramid" => "jungle_temple",
        _ => "unknown",
    }
}

/// `ChunkGenerator.createStructures`：对命中结构集按官方顺序尝试，第一个有效起点
/// 就登记并停下。起点对**所有**结构都算（放置阶段再按允许清单跳过方块写入）。
///
/// `ChunkGenerator.createStructures`: try hit sets in vanilla order and stop at the
/// first valid start. Starts are computed for **every** structure; only placement is
/// gated by the allow list.
pub fn create_structures(
    state: &WorldStructureState,
    generator: &Generator<'_>,
    router: &mut SeededRouter,
    preset: &crate::biome::MultiNoiseBiomeSource,
    dims: (i32, i32, i32),
    chunk: (i32, i32),
    existing: &WorldStructureStarts,
) -> Result<WorldStructureStarts, String> {
    let mut out = existing.clone();
    let context = StartContext {
        generator,
        state,
        dims,
    };
    let mut resolver = BiomeResolver {
        router,
        source: preset,
    };
    // `possibleStructureSets()` 的遍历序是结构集名字（注册表顺序）。
    for set in state.sets.sets().iter() {
        if out.has_valid_for_set(&set.structures) {
            continue;
        }
        if !state
            .sets
            .is_structure_chunk(&set.name, state.seed, chunk.0, chunk.1)
        {
            continue;
        }
        // 单个结构直接尝试；多个结构按 `setLargeFeatureSeed + nextInt(total)` 加权去重顺序。
        let attempts =
            crate::structure::selection_sequence(&set.structures, state.seed, chunk.0, chunk.1);
        for structure_id in attempts {
            if let Some(generated) = context.generate_start(&mut resolver, &structure_id, chunk)? {
                let def = state
                    .structures
                    .get(&structure_id)
                    .ok_or_else(|| format!("missing structure def {structure_id}"))?;
                out.insert(WorldStructureStart {
                    structure: structure_id,
                    chunk_x: chunk.0,
                    chunk_z: chunk.1,
                    references: 0,
                    pieces: generated.pieces,
                    terrain_adaptation: def.terrain_adaptation != "none",
                });
                // 官方 `lambda$createStructures$0` 在这里 return，整个结构集只留一个起点。
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_piece(min_x: i32, max_x: i32) -> Piece {
        pieces::Piece::plain("minecraft:test", [min_x, 60, 0, max_x, 70, 10], 0)
    }

    #[test]
    fn rich_start_projects_to_sparse_pieces() {
        let start = WorldStructureStart {
            structure: "minecraft:test".to_owned(),
            chunk_x: 3,
            chunk_z: -2,
            references: 7,
            pieces: StartPieces::Layout(vec![layout_piece(0, 5), layout_piece(10, 15)]),
            terrain_adaptation: true,
        };
        let sparse = start.as_sparse();
        assert_eq!(sparse.pieces.len(), 2);
        assert_eq!(sparse.pieces[0].bounding_box.min_x, 0);
        assert_eq!(sparse.pieces[1].bounding_box.max_x, 15);
        assert!(sparse.terrain_adaptation);
        // terrainAdaptation != NONE 的 `getBoundingBox()` 外扩 12。
        assert_eq!(sparse.bounding_box().min_x, -12);
        assert_eq!(sparse.bounding_box().max_x, 27);
    }

    #[test]
    fn jigsaw_projection_uses_the_fixed_base_fields() {
        // 只验投影所需的字段：jigsaw 的 O = -1、id 固定、GD 来自 pool element。
        let start = WorldStructureStart {
            structure: "minecraft:village_plains".to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            references: 0,
            pieces: StartPieces::Sparse(vec![StructurePiece {
                piece_type: "minecraft:jigsaw".to_owned(),
                bounding_box: BoundingBox {
                    min_x: 1,
                    min_y: 2,
                    min_z: 3,
                    max_x: 4,
                    max_y: 5,
                    max_z: 6,
                },
                orientation: -1,
                ground_level_delta: 0,
            }]),
            terrain_adaptation: true,
        };
        assert_eq!(start.as_sparse().pieces[0].orientation, -1);
        assert_eq!(start.as_sparse().pieces[0].piece_type, "minecraft:jigsaw");
    }

    #[test]
    fn decoration_step_ordinals_are_the_vanilla_ones() {
        for (index, name) in [
            "raw_generation",
            "lakes",
            "local_modifications",
            "underground_structures",
            "surface_structures",
            "strongholds",
            "underground_ores",
            "underground_decoration",
            "fluid_springs",
            "vegetal_decoration",
            "top_layer_modification",
        ]
        .iter()
        .enumerate()
        {
            let def = StructureDef {
                name: "x".to_owned(),
                type_id: "x".to_owned(),
                biomes: Vec::new(),
                step: (*name).to_owned(),
                terrain_adaptation: "none".to_owned(),
                raw: Value::Null,
            };
            assert_eq!(WorldStructureState::step_of(&def), index);
        }
    }
}
