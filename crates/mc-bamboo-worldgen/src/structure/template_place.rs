//! `StructureTemplate.placeInWorld` 的等价实现，以及它用到的 `StructurePlaceSettings` 与
//! 几种 `StructureProcessor`（`BlockIgnoreProcessor` / `BlockRotProcessor` /
//! `RuleProcessor` / `BlockAgeProcessor` / `BlackstoneReplaceProcessor` /
//! `LavaSubmergedBlockProcessor` / `ProtectedBlockProcessor`）。
//!
//! 逐条照官方 26.3 翻译：
//!
//! * `getRandomPalette(palettes, pos)`：`palettes.get(getRandom(pos).nextInt(size))`，
//!   `getRandom(pos)` 是 `RandomSource.create(Mth.getSeed(pos))`，不消耗世界随机源；
//! * `buildInfoList` 的方块顺序：整方块、其它方块、带 nbt 的方块各自按 `(y, x, z)` 稳定
//!   排序后拼接（这个顺序决定写入顺序，也决定 processor 的调用顺序）；
//! * `processBlockInfos`：每个方块按 `settings.getProcessors()` 的顺序过一次，任一
//!   processor 返回 null 就丢掉这个方块；`chunkBB` 之外的方块不处理（除非有
//!   `evaluatesEntirePieceState` 的 processor，这四个结构都没用到）；
//! * 写入：先 `mirror` 再 `rotate`，带 nbt 的方块先写一次 `BARRIER`，再写真实状态。
//!
//! Port of vanilla `StructureTemplate.placeInWorld` plus the `StructurePlaceSettings` and
//! `StructureProcessor` flavours it needs. The palette choice, the `buildInfoList` block
//! order, the processor pipeline and the final `mirror` → `rotate` state transform are
//! copied step by step from 26.3.

use mc_bamboo_data::{blocks, facts, state_facts};

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    random::WorldgenRandom,
    structure::{pieces::Box, template::StructureTemplate, templated::transform},
};

/// `Mirror` 的序号。
///
/// Ordinals of vanilla `Mirror`.
pub const MIRROR_NONE: i32 = 0;
pub const MIRROR_LEFT_RIGHT: i32 = 1;
pub const MIRROR_FRONT_BACK: i32 = 2;

/// `LiquidSettings`。
///
/// Vanilla `LiquidSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidSettings {
    ApplyWaterlogging,
    IgnoreWaterlogging,
}

/// `StructureProcessor` 的几种，字段直接对应官方构造参数。
///
/// The `StructureProcessor` flavours, with fields matching the vanilla constructors.
#[derive(Debug, Clone)]
pub enum Processor {
    /// `BlockIgnoreProcessor`：方块是列表里的就丢掉。
    ///
    /// `BlockIgnoreProcessor`: drop the block when it is one of the listed ones.
    Ignore(&'static [&'static str]),
    /// `BlockRotProcessor`（没有 `rottable_blocks`）：`nextFloat() <= integrity` 才保留。
    ///
    /// `BlockRotProcessor` without `rottable_blocks`: keep when
    /// `nextFloat() <= integrity`.
    Rot(f32),
    /// `RuleProcessor`：按顺序试每条规则，第一条命中的替换输出状态。
    ///
    /// `RuleProcessor`: try every rule in order; the first match replaces the state.
    Rule(Vec<Rule>),
    /// `BlockAgeProcessor`。
    Age(f32),
    /// `BlackstoneReplaceProcessor.INSTANCE`。
    Blackstone,
    /// `LavaSubmergedBlockProcessor.INSTANCE`。
    LavaSubmerged,
    /// `ProtectedBlockProcessor(FEATURES_CANNOT_REPLACE)`。
    Protected,
}

/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`。
///
/// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
pub const IGNORE_STRUCTURE_BLOCK: &[&str] = &["minecraft:structure_block"];
/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR`。
///
/// `BlockIgnoreProcessor.STRUCTURE_AND_AIR`.
pub const IGNORE_STRUCTURE_AND_AIR: &[&str] = &["minecraft:air", "minecraft:structure_block"];

/// `RuleProcessor` 的一条 `ProcessorRule`。这里只覆盖这四个结构用到的两种输入谓词：
/// `BlockMatchTest` 与 `RandomBlockMatchTest`；位置谓词都是 `PosAlwaysTrueTest`。
///
/// One `ProcessorRule` as used by these four structures: `BlockMatchTest` or
/// `RandomBlockMatchTest` on the input state, `AlwaysTrueTest` on the world state and
/// `PosAlwaysTrueTest` on the position.
#[derive(Debug, Clone)]
pub struct Rule {
    pub input: InputTest,
    pub output: u32,
}

/// `RuleTest` 的两种。
///
/// The two `RuleTest` flavours.
#[derive(Debug, Clone)]
pub enum InputTest {
    /// `BlockMatchTest`：方块相同（不看属性）。不消耗随机数。
    ///
    /// `BlockMatchTest`: same block, properties ignored. Draws nothing.
    Block(&'static str),
    /// `RandomBlockMatchTest`：方块相同后 `nextFloat() < probability`。
    ///
    /// `RandomBlockMatchTest`: same block, then `nextFloat() < probability`.
    RandomBlock(&'static str, f32),
    /// `AlwaysTrueTest`。
    Always,
}

/// `BlockAgeProcessor` 用到的方块状态，按名字取默认状态。
///
/// The block states `BlockAgeProcessor` uses, by name.
const CRACKED_STONE_BRICKS: &str = "minecraft:cracked_stone_bricks";
const MOSSY_STONE_BRICKS: &str = "minecraft:mossy_stone_bricks";
const STONE_BRICK_STAIRS: &str = "minecraft:stone_brick_stairs";
const MOSSY_STONE_BRICK_STAIRS: &str = "minecraft:mossy_stone_brick_stairs";
const MOSSY_STONE_BRICK_SLAB: &str = "minecraft:mossy_stone_brick_slab";
const STONE_SLAB: &str = "minecraft:stone_slab";
const STONE_BRICK_SLAB: &str = "minecraft:stone_brick_slab";
const MOSSY_STONE_BRICK_WALL: &str = "minecraft:mossy_stone_brick_wall";
const CRYING_OBSIDIAN: &str = "minecraft:crying_obsidian";

/// `StructurePlaceSettings` 里放置阶段用到的字段。
///
/// The `StructurePlaceSettings` fields the placement stage needs.
#[derive(Debug, Clone)]
pub struct PlaceSettings {
    pub mirror: i32,
    pub rotation: i32,
    /// `rotationPivot` 的 X/Z（`StructureTemplate.transform` 只用这两轴）。
    ///
    /// The X/Z of `rotationPivot` (`StructureTemplate.transform` ignores Y).
    pub pivot: (i32, i32),
    pub liquid: LiquidSettings,
    pub processors: Vec<Processor>,
    /// `setBoundingBox(chunkBB)`：只处理盒内的方块。
    ///
    /// `setBoundingBox(chunkBB)`: process only blocks inside the box.
    pub chunk_bb: Option<Box>,
}

impl PlaceSettings {
    pub fn new(mirror: i32, rotation: i32, pivot: (i32, i32)) -> Self {
        Self {
            mirror,
            rotation,
            pivot,
            liquid: LiquidSettings::ApplyWaterlogging,
            processors: Vec::new(),
            chunk_bb: None,
        }
    }

    pub fn with_processors(mut self, processors: Vec<Processor>) -> Self {
        self.processors = processors;
        self
    }

    pub fn with_liquid(mut self, liquid: LiquidSettings) -> Self {
        self.liquid = liquid;
        self
    }

    /// `StructureTemplate.calculateRelativePosition(settings, pos)`。
    ///
    /// `StructureTemplate.calculateRelativePosition(settings, pos)`.
    pub fn relative(&self, pos: (i32, i32, i32)) -> (i32, i32, i32) {
        transform(pos, self.mirror, self.rotation, self.pivot)
    }
}

/// 按 `buildInfoList` 的顺序给出方块下标：整方块、其它方块、带 nbt 的方块，各自按
/// `(y, x, z)` 稳定排序。
///
/// Indices in `buildInfoList` order: full blocks, other blocks, then nbt-carrying blocks,
/// each stably sorted by `(y, x, z)`.
pub fn ordered_blocks(template: &StructureTemplate, palette: usize) -> Vec<usize> {
    let mut full = Vec::new();
    let mut other = Vec::new();
    let mut with_nbt = Vec::new();
    for (index, block) in template.blocks.iter().enumerate() {
        if block.nbt.is_some() {
            with_nbt.push(index);
            continue;
        }
        let Some(entry) = template.palettes[palette].get(block.state) else {
            continue;
        };
        let Some(block_def) = mc_bamboo_data::block_by_name(&entry.name) else {
            continue;
        };
        let is_full = state_facts(block_def.default_state) & facts::COLLISION_FULL_BLOCK != 0;
        if is_full {
            full.push(index);
        } else {
            other.push(index);
        }
    }
    let by_pos = |a: &usize, b: &usize| {
        let pa = template.blocks[*a].pos;
        let pb = template.blocks[*b].pos;
        (pa.1, pa.0, pa.2).cmp(&(pb.1, pb.0, pb.2))
    };
    full.sort_by(by_pos);
    other.sort_by(by_pos);
    with_nbt.sort_by(by_pos);
    full.into_iter().chain(other).chain(with_nbt).collect()
}

/// 一次放置的结果。
///
/// The outcome of one placement.
pub struct Placed {
    /// 真正写下的方块，给对拍用；`None` 表示不记录。
    ///
    /// The blocks actually written, for the oracle; `None` disables recording.
    pub writes: Option<Vec<(i32, i32, i32, u32)>>,
    /// 带方块实体的位置（模板里带 nbt 的方块写完就是方块实体）。
    ///
    /// Positions holding a block entity (a template block with nbt becomes one).
    pub block_entities: Vec<(i32, i32, i32)>,
}

/// `StructureTemplate.placeInWorld(level, position, referencePos, settings, random, 2)`。
///
/// 返回 false 表示模板没有 palette（官方的 `palettes.isEmpty()`）。
///
/// `StructureTemplate.placeInWorld(...)` with update mode 2. Returns false when the
/// template has no palette (`palettes.isEmpty()`).
pub fn place_in_world(
    world: &mut FeatureWorld<'_>,
    template: &StructureTemplate,
    position: (i32, i32, i32),
    settings: &PlaceSettings,
    random: &WorldgenRandom,
    record: bool,
) -> Option<Placed> {
    if template.palettes.is_empty() {
        return None;
    }
    let palette = template.palette_index(position)?;
    let mut writes = Vec::new();
    let mut block_entities = Vec::new();
    let blocks = ordered_blocks(template, palette);
    if blocks.is_empty() || template.size.0 < 1 || template.size.1 < 1 || template.size.2 < 1 {
        return Some(Placed {
            writes: record.then_some(writes),
            block_entities,
        });
    }

    // 先过 processor，再统一写入：官方先构造 `processedBlockInfoList`，然后才写方块，
    // 所以 processor 读到的世界是「本结构件还什么都没写」的状态。
    // Processors run first, then the writes: vanilla builds `processedBlockInfoList`
    // before writing anything, so processors see the world without this piece's writes.
    let mut processed: Vec<((i32, i32, i32), u32, bool)> = Vec::new();
    for &index in &blocks {
        let block = &template.blocks[index];
        let mut pos = settings.relative(block.pos);
        pos = (pos.0 + position.0, pos.1 + position.1, pos.2 + position.2);
        if let Some(bb) = &settings.chunk_bb
            && !inside(bb, pos)
        {
            continue;
        }
        let Some(entry) = template.palettes[palette].get(block.state) else {
            continue;
        };
        let Some(state) = state_of(entry) else {
            continue;
        };
        let mut current = state;
        let mut keep = true;
        for processor in &settings.processors {
            match processor.process(world, pos, block.pos, current, settings) {
                Some(next) => current = next,
                None => {
                    keep = false;
                    break;
                }
            }
        }
        if keep {
            processed.push((pos, current, block.nbt.is_some()));
        }
    }

    let mut locked_fluids: Vec<(i32, i32, i32)> = Vec::new();
    let mut to_fill: Vec<(i32, i32, i32)> = Vec::new();
    let mut placed_positions: Vec<(i32, i32, i32)> = Vec::new();
    for (pos, state, has_nbt) in processed {
        let previous_fluid = (settings.liquid == LiquidSettings::ApplyWaterlogging)
            .then(|| *mc_bamboo_data::state_fluid(world.get_block(pos.0, pos.1, pos.2)));
        let state = mirror_rotate(state, settings);
        if has_nbt {
            // `level.setBlock(blockPos, Blocks.BARRIER, 820)`：给方块实体占位。
            // `level.setBlock(blockPos, Blocks.BARRIER, 820)`: a placeholder for the
            // block entity.
            write(world, pos, blocks::BARRIER, &mut writes, record);
        }
        if !write(world, pos, state, &mut writes, record) {
            continue;
        }
        placed_positions.push(pos);
        if has_nbt {
            block_entities.push(pos);
        }
        if let Some(previous) = previous_fluid {
            let new_fluid = mc_bamboo_data::state_fluid(state);
            if new_fluid.source {
                locked_fluids.push(pos);
            } else if can_hold_liquid(state) {
                if previous.source {
                    if let Some(filled) = waterlog(state, &previous) {
                        write(world, pos, filled, &mut writes, record);
                    }
                } else {
                    to_fill.push(pos);
                }
            }
        }
    }

    fill_fluids(world, &mut to_fill, &locked_fluids, &mut writes, record);
    // `updateShapeAtEdge`：对写入形状的每个外表面，让两侧方块各做一次 `updateShape`。
    // `updateShapeAtEdge`: run `updateShape` on both sides of every outer face of the
    // written shape.
    if !placed_positions.is_empty() {
        let shape: std::collections::HashSet<(i32, i32, i32)> =
            placed_positions.iter().copied().collect();
        let min = (
            placed_positions.iter().map(|pos| pos.0).min().unwrap_or(0),
            placed_positions.iter().map(|pos| pos.1).min().unwrap_or(0),
            placed_positions.iter().map(|pos| pos.2).min().unwrap_or(0),
        );
        let max = (
            placed_positions.iter().map(|pos| pos.0).max().unwrap_or(0),
            placed_positions.iter().map(|pos| pos.1).max().unwrap_or(0),
            placed_positions.iter().map(|pos| pos.2).max().unwrap_or(0),
        );
        crate::feature::shape_update::update_shape_at_edge(world, &shape, min, max);
        // 再对每个写入位置跑一次 `Block.updateFromNeighbourShapes`。
        // Then run `Block.updateFromNeighbourShapes` on every written position.
        for pos in &placed_positions {
            let state = world.get_block(pos.0, pos.1, pos.2);
            let new_state = crate::feature::shape_update::update_from_neighbour_shapes(world, *pos);
            if new_state != state {
                write(world, *pos, new_state, &mut writes, record);
            }
        }
    }
    let _ = random;
    Some(Placed {
        writes: record.then_some(writes),
        block_entities,
    })
}

/// 官方的流体回填循环：反复把相邻水源放进还能装水的方块，直到一轮没有变化。
///
/// Vanilla's fluid fill loop: repeatedly pour neighbouring source fluid into blocks that
/// can hold liquid until a pass changes nothing.
fn fill_fluids(
    world: &mut FeatureWorld<'_>,
    to_fill: &mut Vec<(i32, i32, i32)>,
    locked: &[(i32, i32, i32)],
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    const ORDER: [(i32, i32, i32); 5] = [(0, 1, 0), (0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];
    let mut filled = true;
    while filled && !to_fill.is_empty() {
        filled = false;
        let mut index = 0;
        while index < to_fill.len() {
            let pos = to_fill[index];
            let mut placed = false;
            for (dx, dy, dz) in ORDER {
                let neighbour = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                let fluid = mc_bamboo_data::state_fluid(world.get_block(
                    neighbour.0,
                    neighbour.1,
                    neighbour.2,
                ));
                if fluid.source && !locked.contains(&neighbour) {
                    let state = world.get_block(pos.0, pos.1, pos.2);
                    if let Some(wet) = waterlog(state, fluid) {
                        write(world, pos, wet, writes, record);
                        placed = true;
                    }
                    break;
                }
            }
            if placed {
                filled = true;
                to_fill.remove(index);
            } else {
                index += 1;
            }
        }
    }
}

/// `PlaceSettings.getHeightmapType`：`ON_OCEAN_FLOOR` 用 `OCEAN_FLOOR_WG`。
///
/// `PlaceSettings.getHeightmapType`: `ON_OCEAN_FLOOR` uses `OCEAN_FLOOR_WG`.
pub fn heightmap_type(ocean_floor: bool) -> HeightmapKind {
    if ocean_floor {
        HeightmapKind::OceanFloorWg
    } else {
        HeightmapKind::WorldSurfaceWg
    }
}

/// 一个 `TemplateStructurePiece.postProcess` 的结果。
///
/// The result of one `TemplateStructurePiece.postProcess`.
pub struct PieceOutcome {
    /// 放置结果；模板没有 palette 时是 `None`（官方的 `placeInWorld` 返回 false）。
    ///
    /// The placement outcome; `None` when the template has no palette (vanilla
    /// `placeInWorld` returns false).
    pub placed: Option<Placed>,
    /// 数据标记：`(metadata, 位置)`，顺序照 `filterBlocks`。
    ///
    /// Data markers as `(metadata, position)`, in `filterBlocks` order.
    pub markers: Vec<(String, (i32, i32, i32))>,
    /// 拼图方块被替换成的状态（`(位置, 状态)`），写完即生效。
    ///
    /// The states the jigsaw blocks were replaced with as `(position, state)`.
    pub jigsaw_writes: Vec<((i32, i32, i32), u32)>,
    /// 放置后的包围盒（官方 `placeInWorld` 之前重算的那一次）。
    ///
    /// The bounding box recomputed just before `placeInWorld`.
    pub bounding_box: Box,
    /// 放置后的模板位置（`postProcess` 之后 `templatePosition` 的值）。
    ///
    /// The template position after `postProcess`.
    pub template_position: (i32, i32, i32),
}

/// `TemplateStructurePiece.postProcess`：重算包围盒、放模板、跑数据标记和拼图替换。
///
/// 官方顺序：`placeSettings.setBoundingBox(chunkBB)` → 重算 `boundingBox` →
/// `template.placeInWorld(...)`；只有放置成功（有 palette）才处理数据标记和拼图方块。
///
/// Vanilla order: `placeSettings.setBoundingBox(chunkBB)`, recompute the box, then
/// `template.placeInWorld(...)`; data markers and jigsaw replacement only run when the
/// placement succeeded (the template has a palette).
pub fn template_piece_post_process(
    world: &mut FeatureWorld<'_>,
    template: &StructureTemplate,
    template_position: (i32, i32, i32),
    settings: &PlaceSettings,
    chunk_bb: Box,
    record: bool,
) -> PieceOutcome {
    let mut settings = settings.clone();
    settings.chunk_bb = Some(chunk_bb);
    let bounding_box = template_box(template, &settings, template_position);
    let random = position_random(template_position);
    let placed = place_in_world(
        world,
        template,
        template_position,
        &settings,
        &random,
        record,
    );

    let mut markers = Vec::new();
    let mut jigsaw_writes = Vec::new();
    if placed.is_some() {
        for (metadata, pos) in data_markers(template, template_position, &settings) {
            markers.push((metadata, pos));
        }
        for (pos, entry) in
            filter_blocks(template, template_position, &settings, "minecraft:jigsaw")
        {
            let Some(entry) = entry else { continue };
            let final_state =
                string_field(entry, "final_state").unwrap_or_else(|| "minecraft:air".to_owned());
            if let Some(state) = state_from_text(&final_state) {
                world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
                jigsaw_writes.push((pos, state));
            }
        }
    }

    PieceOutcome {
        placed,
        markers,
        jigsaw_writes,
        bounding_box,
        template_position,
    }
}

/// `TemplateStructurePiece.postProcess` 里数据标记的方块效果。
///
/// 官方在 `placeInWorld` 成功之后遍历 `filterBlocks(STRUCTURE_BLOCK)`，对
/// `mode == data` 的标记调 `handleDataMarker`。这里把标记位置和 metadata 交回调用方，
/// 由各结构决定写什么方块。
///
/// The block effects of data markers inside `TemplateStructurePiece.postProcess`. After a
/// successful `placeInWorld` vanilla walks `filterBlocks(STRUCTURE_BLOCK)` and, for
/// markers with `mode == data`, calls `handleDataMarker`. The marker positions and
/// metadata are handed back here so each structure decides what to write.
pub fn data_markers(
    template: &StructureTemplate,
    position: (i32, i32, i32),
    settings: &PlaceSettings,
) -> Vec<(String, (i32, i32, i32))> {
    let mut markers = Vec::new();
    for (pos, entry) in filter_blocks(template, position, settings, "minecraft:structure_block") {
        let Some(entry) = entry else { continue };
        if string_field(entry, "mode").as_deref() != Some("data") {
            continue;
        }
        let metadata = string_field(entry, "metadata").unwrap_or_default();
        markers.push((metadata, pos));
    }
    markers
}

/// `StructureTemplate.getBoundingBox(settings, position)`：`(0,0,0)` 和 `size - 1` 两个角
/// 变换后取包围盒再平移到 `position`。
///
/// `StructureTemplate.getBoundingBox(settings, position)`: transform the two corners
/// `(0,0,0)` and `size - 1`, take their box and translate it by `position`.
pub fn template_box(
    template: &StructureTemplate,
    settings: &PlaceSettings,
    position: (i32, i32, i32),
) -> Box {
    let first = settings.relative((0, 0, 0));
    let second = settings.relative((
        template.size.0 - 1,
        template.size.1 - 1,
        template.size.2 - 1,
    ));
    [
        first.0.min(second.0) + position.0,
        first.1.min(second.1) + position.1,
        first.2.min(second.2) + position.2,
        first.0.max(second.0) + position.0,
        first.1.max(second.1) + position.1,
        first.2.max(second.2) + position.2,
    ]
}

/// 一个被 `filterBlocks` 选中的方块：位置和它的 nbt。
///
/// One block picked by `filterBlocks`: its position and nbt.
pub type FilteredBlock<'a> = ((i32, i32, i32), Option<&'a mc_bamboo_nbt::Tag>);

/// `StructureTemplate.filterBlocks(position, settings, block)`：按 `buildInfoList` 的顺序
/// 给出方块是 `block` 的项，位置变换后落在 `chunkBB` 内才算。
///
/// `StructureTemplate.filterBlocks(position, settings, block)`: the blocks matching
/// `block` in `buildInfoList` order, with positions transformed and filtered by the box.
fn filter_blocks<'a>(
    template: &'a StructureTemplate,
    position: (i32, i32, i32),
    settings: &PlaceSettings,
    block: &str,
) -> Vec<FilteredBlock<'a>> {
    let Some(palette) = template.palette_index(position) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in ordered_blocks(template, palette) {
        let entry = &template.blocks[index];
        let Some(palette_entry) = template.palettes[palette].get(entry.state) else {
            continue;
        };
        if palette_entry.name != block {
            continue;
        }
        let relative = settings.relative(entry.pos);
        let pos = (
            relative.0 + position.0,
            relative.1 + position.1,
            relative.2 + position.2,
        );
        if let Some(bb) = &settings.chunk_bb
            && !inside(bb, pos)
        {
            continue;
        }
        out.push((pos, entry.nbt.as_ref()));
    }
    out
}

/// 读 nbt 里的一个字符串字段（小写键名，和方块实体的 tag 一致）。
///
/// Read one string field from an nbt tag (lower-case keys, as block entity tags use).
pub fn string_field(tag: &mc_bamboo_nbt::Tag, key: &str) -> Option<String> {
    let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
        return None;
    };
    entries
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| {
            let mc_bamboo_nbt::Tag::String(text) = value else {
                return None;
            };
            Some(text.clone())
        })
}

/// 把 `BlockStateParser.parseForBlock(..., true)` 的字符串（`minecraft:x[a=b]`）解析成状态。
///
/// Parse the `BlockStateParser.parseForBlock(..., true)` text (`minecraft:x[a=b]`) into a
/// state id.
fn state_from_text(text: &str) -> Option<u32> {
    let (name, rest) = match text.split_once('[') {
        Some((name, rest)) => (name, Some(rest.trim_end_matches(']'))),
        None => (text, None),
    };
    let block = mc_bamboo_data::block_by_name(name)?;
    let mut state = block.default_state;
    if let Some(properties) = rest {
        for entry in properties.split(',') {
            let (key, value) = entry.split_once('=')?;
            state = crate::feature::state::with_property(state, key, value)?;
        }
    }
    Some(state)
}

fn write(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    state: u32,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) -> bool {
    let written = world.set_block(pos.0, pos.1, pos.2, state);
    if written && record {
        writes.push((pos.0, pos.1, pos.2, state));
    }
    written
}

fn inside(bb: &Box, pos: (i32, i32, i32)) -> bool {
    pos.0 >= bb[0]
        && pos.0 <= bb[3]
        && pos.1 >= bb[1]
        && pos.1 <= bb[4]
        && pos.2 >= bb[2]
        && pos.2 <= bb[5]
}

/// `StructureBlockInfo.state.mirror(settings.getMirror()).rotate(settings.getRotation())`。
///
/// `StructureBlockInfo.state.mirror(settings.getMirror()).rotate(settings.getRotation())`.
fn mirror_rotate(state: u32, settings: &PlaceSettings) -> u32 {
    let mut state = state;
    if settings.mirror > 0 {
        state = mc_bamboo_data::state_mirror(state, settings.mirror as u32);
    }
    if settings.rotation > 0 {
        state = mc_bamboo_data::state_rotation(state, settings.rotation as u32);
    }
    state
}

/// palette 里的一项换成状态 ID。
///
/// Turn one palette entry into a state id.
pub fn state_of(entry: &crate::structure::template::PaletteEntry) -> Option<u32> {
    let block = mc_bamboo_data::block_by_name(&entry.name)?;
    let mut state = block.default_state;
    for (key, value) in &entry.properties {
        state = crate::feature::state::with_property(state, key, value)?;
    }
    Some(state)
}

/// `BlockState.hasBlockEntity()`：模板里带 nbt 的方块一定属于这一类，这里只用于
/// 判断 `RandomizableContainer`（箱子、木桶……）。
///
/// `BlockState.hasBlockEntity()`: used to recognise `RandomizableContainer` blocks
/// (chests, barrels, ...).
pub fn has_block_entity(state: u32) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.block_entity_type.is_some())
}

/// 方块能不能装液体（`LiquidBlockContainer` 的近似：属性表里带 `waterlogged`）。
///
/// Whether a block can hold liquid (an approximation of `LiquidBlockContainer`: the
/// block has a `waterlogged` property).
fn can_hold_liquid(state: u32) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| {
        block
            .properties
            .iter()
            .any(|property| property.name == "waterlogged")
    })
}

/// `SimpleWaterloggedBlock.canPlaceLiquid` + `placeLiquid`：只有水、且当前不是
/// `waterlogged=true` 时才置 true。
///
/// `SimpleWaterloggedBlock.canPlaceLiquid` + `placeLiquid`: only water, and only when the
/// state is not already `waterlogged=true`.
fn waterlog(state: u32, fluid: &mc_bamboo_data::FluidFacts) -> Option<u32> {
    if fluid.fluid != mc_bamboo_data::registries::fluid::WATER
        && fluid.fluid != mc_bamboo_data::registries::fluid::FLOWING_WATER
    {
        return None;
    }
    if property_of(state, "waterlogged").as_deref() == Some("true") {
        return None;
    }
    crate::feature::state::with_property(state, "waterlogged", "true")
}

impl Processor {
    /// `StructureProcessor.processBlock`：返回 `None` 表示丢掉这个方块。
    ///
    /// `StructureProcessor.processBlock`: `None` drops the block.
    fn process(
        &self,
        world: &FeatureWorld<'_>,
        pos: (i32, i32, i32),
        template_pos: (i32, i32, i32),
        state: u32,
        settings: &PlaceSettings,
    ) -> Option<u32> {
        match self {
            Processor::Ignore(names) => {
                let name = block_name(state)?;
                if names.contains(&name) {
                    None
                } else {
                    Some(state)
                }
            }
            Processor::Rot(integrity) => {
                // `settings.getRandom(processedBlockInfo.pos())`：按世界位置派生，和
                // `getRandomPalette` 用同一套种子。
                let random = position_random(pos);
                if random.next_float() <= *integrity {
                    Some(state)
                } else {
                    None
                }
            }
            Processor::Rule(rules) => {
                let random = position_random(pos);
                for rule in rules {
                    if rule.input.matches(state, &random) {
                        return Some(rule.output);
                    }
                }
                Some(state)
            }
            Processor::Age(mossiness) => Some(age(state, *mossiness, &position_random(pos))),
            Processor::LavaSubmerged => {
                let was_lava = block_name(world.get_block(pos.0, pos.1, pos.2))
                    .is_some_and(|name| name == "minecraft:lava");
                let full = state_facts(state) & facts::COLLISION_FULL_BLOCK != 0;
                if was_lava && !full {
                    Some(block_state("minecraft:lava"))
                } else {
                    Some(state)
                }
            }
            Processor::Protected => {
                let world_state = world.get_block(pos.0, pos.1, pos.2);
                if protected_from_replacement(world_state) {
                    None
                } else {
                    Some(state)
                }
            }
            Processor::Blackstone => Some(blackstone(state)),
        }
        .inspect(|_| {
            let _ = (template_pos, settings);
        })
    }
}

impl InputTest {
    /// `RuleTest.test`：只做输入谓词的判定（位置谓词恒 true）。
    ///
    /// `RuleTest.test`: only the input predicate (the position predicate is always true).
    fn matches(&self, state: u32, random: &WorldgenRandom) -> bool {
        match self {
            InputTest::Always => true,
            InputTest::Block(name) => block_name(state) == Some(name),
            InputTest::RandomBlock(name, probability) => {
                block_name(state) == Some(name) && random.next_float() < *probability
            }
        }
    }
}

/// `RandomSource.create(Mth.getSeed(pos))`。
///
/// `RandomSource.create(Mth.getSeed(pos))`.
fn position_random(pos: (i32, i32, i32)) -> WorldgenRandom {
    let seed = crate::random::positional_seed(pos.0, pos.1, pos.2);
    WorldgenRandom::new(crate::random::Random::Legacy(
        mc_bamboo_blocks::LegacyRandom::new(seed),
    ))
}

fn block_name(state: u32) -> Option<&'static str> {
    let id = mc_bamboo_data::block_id_of_state(state)?;
    Some(blocks::BLOCKS[id].name)
}

fn block_state(name: &str) -> u32 {
    mc_bamboo_data::block_by_name(name)
        .map(|block| block.default_state)
        .unwrap_or(blocks::AIR)
}

/// `BlockAgeProcessor.processBlock`。
///
/// `BlockAgeProcessor.processBlock`.
fn age(state: u32, mossiness: f32, random: &WorldgenRandom) -> u32 {
    let name = block_name(state).unwrap_or("");
    if matches!(
        name,
        "minecraft:stone_bricks" | "minecraft:stone" | "minecraft:chiseled_stone_bricks"
    ) {
        return age_full_stone_block(mossiness, random).unwrap_or(state);
    }
    if in_tag(state, "minecraft:stairs") {
        return age_stairs(state, mossiness, random).unwrap_or(state);
    }
    if in_tag(state, "minecraft:slabs") {
        return if random.next_float() < mossiness {
            with_properties_of(block_state(MOSSY_STONE_BRICK_SLAB), state)
        } else {
            state
        };
    }
    if in_tag(state, "minecraft:walls") {
        return if random.next_float() < mossiness {
            with_properties_of(block_state(MOSSY_STONE_BRICK_WALL), state)
        } else {
            state
        };
    }
    if name == "minecraft:obsidian" {
        return if random.next_float() < 0.15 {
            block_state(CRYING_OBSIDIAN)
        } else {
            state
        };
    }
    state
}

/// `BlockAgeProcessor.maybeReplaceFullStoneBlock`。两个候选数组在判定前就各自
/// `getRandomFacingStairs` 一次，所以随机数消耗和两个数组的构造顺序一致。
///
/// `BlockAgeProcessor.maybeReplaceFullStoneBlock`. Both candidate arrays build their
/// `getRandomFacingStairs` entry up front, so the draws follow that construction order.
fn age_full_stone_block(mossiness: f32, random: &WorldgenRandom) -> Option<u32> {
    if random.next_float() >= 0.5 {
        return None;
    }
    let non_mossy_stairs = random_facing_stairs(STONE_BRICK_STAIRS, random);
    let mossy_stairs = random_facing_stairs(MOSSY_STONE_BRICK_STAIRS, random);
    let non_mossy = [block_state(CRACKED_STONE_BRICKS), non_mossy_stairs];
    let mossy = [block_state(MOSSY_STONE_BRICKS), mossy_stairs];
    let pick = if random.next_float() < mossiness {
        mossy
    } else {
        non_mossy
    };
    Some(pick[random.next_int(2) as usize])
}

/// `BlockAgeProcessor.maybeReplaceStairs`。
///
/// `BlockAgeProcessor.maybeReplaceStairs`.
fn age_stairs(state: u32, mossiness: f32, random: &WorldgenRandom) -> Option<u32> {
    if random.next_float() >= 0.5 {
        return None;
    }
    let mossy = [
        with_properties_of(block_state(MOSSY_STONE_BRICK_STAIRS), state),
        block_state(MOSSY_STONE_BRICK_SLAB),
    ];
    let non_mossy = [block_state(STONE_SLAB), block_state(STONE_BRICK_SLAB)];
    let pick = if random.next_float() < mossiness {
        mossy
    } else {
        non_mossy
    };
    Some(pick[random.next_int(2) as usize])
}

/// `BlockAgeProcessor.getRandomFacingStairs`：
/// `Plane.HORIZONTAL.getRandomDirection(random)`（`nextInt(4)`）再
/// `Util.getRandom(Half.values(), random)`（`nextInt(2)`）。
///
/// `BlockAgeProcessor.getRandomFacingStairs`: one `nextInt(4)` for the facing and one
/// `nextInt(2)` for the half.
fn random_facing_stairs(block: &str, random: &WorldgenRandom) -> u32 {
    const FACING: [&str; 4] = ["north", "east", "south", "west"];
    let facing = FACING[random.next_int(4) as usize];
    let half = if random.next_int(2) == 0 {
        "top"
    } else {
        "bottom"
    };
    let mut state = block_state(block);
    if let Some(next) = crate::feature::state::with_property(state, "facing", facing) {
        state = next;
    }
    if let Some(next) = crate::feature::state::with_property(state, "half", half) {
        state = next;
    }
    state
}

/// `BlockBehaviour.BlockStateBase.withPropertiesOf(other)`：只搬两边都有的属性。
///
/// `withPropertiesOf(other)`: copy the properties both blocks have.
fn with_properties_of(state: u32, other: u32) -> u32 {
    let Some(target) = mc_bamboo_data::block_of_state(state) else {
        return state;
    };
    let Some(source) = mc_bamboo_data::block_of_state(other) else {
        return state;
    };
    let mut result = state;
    for property in target.properties {
        let Some(value) = source
            .properties
            .iter()
            .find(|candidate| candidate.name == property.name)
            .and_then(|_| property_of(other, property.name))
        else {
            continue;
        };
        if let Some(next) = crate::feature::state::with_property(result, property.name, &value) {
            result = next;
        }
    }
    result
}

/// 读一个状态上某个属性的字符串值。
///
/// Read one property of a state as a string.
fn property_of(state: u32, name: &str) -> Option<String> {
    let block = mc_bamboo_data::block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        if property.name == name {
            let index = (state - block.first_state) / stride % count;
            return Some(property.values[index as usize].to_owned());
        }
        stride *= count;
    }
    None
}

/// 方块是否属于某个方块标签。
///
/// Whether the state's block belongs to a block tag.
fn in_tag(state: u32, tag: &str) -> bool {
    let Some(ids) = mc_bamboo_data::block_tag(tag) else {
        return false;
    };
    let Some(block) = mc_bamboo_data::block_of_state(state) else {
        return false;
    };
    let index = mc_bamboo_data::block_id_of_state(block.default_state).unwrap_or(usize::MAX);
    ids.iter().any(|&id| id as usize == index)
}

/// `ProtectedBlockProcessor(FEATURES_CANNOT_REPLACE)`。
///
/// `ProtectedBlockProcessor(FEATURES_CANNOT_REPLACE)`.
fn protected_from_replacement(state: u32) -> bool {
    in_tag(state, "minecraft:features_cannot_replace")
}

/// `BlackstoneReplaceProcessor` 的映射表，属性只搬 `facing` / `half` / `type`。
///
/// The `BlackstoneReplaceProcessor` map; only `facing`, `half` and `type` carry over.
fn blackstone(state: u32) -> u32 {
    const REPLACEMENTS: [(&str, &str); 23] = [
        ("minecraft:cobblestone", "minecraft:blackstone"),
        ("minecraft:mossy_cobblestone", "minecraft:blackstone"),
        ("minecraft:stone", "minecraft:polished_blackstone"),
        (
            "minecraft:stone_bricks",
            "minecraft:polished_blackstone_bricks",
        ),
        (
            "minecraft:mossy_stone_bricks",
            "minecraft:polished_blackstone_bricks",
        ),
        (
            "minecraft:cobblestone_stairs",
            "minecraft:blackstone_stairs",
        ),
        (
            "minecraft:mossy_cobblestone_stairs",
            "minecraft:blackstone_stairs",
        ),
        (
            "minecraft:stone_stairs",
            "minecraft:polished_blackstone_stairs",
        ),
        (
            "minecraft:stone_brick_stairs",
            "minecraft:polished_blackstone_brick_stairs",
        ),
        (
            "minecraft:mossy_stone_brick_stairs",
            "minecraft:polished_blackstone_brick_stairs",
        ),
        ("minecraft:cobblestone_slab", "minecraft:blackstone_slab"),
        (
            "minecraft:mossy_cobblestone_slab",
            "minecraft:blackstone_slab",
        ),
        (
            "minecraft:smooth_stone_slab",
            "minecraft:polished_blackstone_slab",
        ),
        ("minecraft:stone_slab", "minecraft:polished_blackstone_slab"),
        (
            "minecraft:stone_brick_slab",
            "minecraft:polished_blackstone_brick_slab",
        ),
        (
            "minecraft:mossy_stone_brick_slab",
            "minecraft:polished_blackstone_brick_slab",
        ),
        (
            "minecraft:stone_brick_wall",
            "minecraft:polished_blackstone_brick_wall",
        ),
        (
            "minecraft:mossy_stone_brick_wall",
            "minecraft:polished_blackstone_brick_wall",
        ),
        ("minecraft:cobblestone_wall", "minecraft:blackstone_wall"),
        (
            "minecraft:mossy_cobblestone_wall",
            "minecraft:blackstone_wall",
        ),
        (
            "minecraft:chiseled_stone_bricks",
            "minecraft:chiseled_polished_blackstone",
        ),
        (
            "minecraft:cracked_stone_bricks",
            "minecraft:cracked_polished_blackstone_bricks",
        ),
        ("minecraft:iron_bars", "minecraft:iron_chain"),
    ];
    let Some(name) = block_name(state) else {
        return state;
    };
    let Some((_, target)) = REPLACEMENTS.iter().find(|(from, _)| *from == name) else {
        return state;
    };
    let mut result = block_state(target);
    for property in ["facing", "half", "type"] {
        if let Some(value) = property_of(state, property)
            && let Some(next) = crate::feature::state::with_property(result, property, &value)
        {
            result = next;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(0,0,0)` 起的模板按 `(y, x, z)` 排：整方块在前，带 nbt 的在后。
    ///
    /// Blocks are ordered by `(y, x, z)` with full blocks first and nbt blocks last.
    #[test]
    fn ordered_blocks_puts_nbt_blocks_last() {
        let template = StructureTemplate::load("minecraft:igloo/top").unwrap();
        let order = ordered_blocks(&template, 0);
        assert_eq!(order.len(), template.blocks.len());
        let last = order.last().copied().unwrap();
        let has_nbt = template.blocks[last].nbt.is_some();
        assert!(has_nbt, "the last block should carry nbt");
    }

    /// 属性搬运只搬两边都有的键。
    ///
    /// Property copying only takes keys present on both sides.
    #[test]
    fn with_properties_of_copies_shared_keys() {
        let stairs = block_state("minecraft:stone_brick_stairs");
        let facing = property_of(stairs, "facing").unwrap();
        let mirrored =
            with_properties_of(block_state("minecraft:mossy_stone_brick_stairs"), stairs);
        assert_eq!(property_of(mirrored, "facing"), Some(facing));
    }

    /// `features_cannot_replace` 标签里至少有基岩。
    ///
    /// The `features_cannot_replace` tag contains bedrock.
    #[test]
    fn protected_blocks_include_bedrock() {
        assert!(protected_from_replacement(block_state("minecraft:bedrock")));
        assert!(!protected_from_replacement(block_state("minecraft:stone")));
    }
}
