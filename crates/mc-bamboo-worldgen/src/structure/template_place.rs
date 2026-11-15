//! 结构模板的逐方块放置：`StructureTemplate.placeInWorld` + `processBlockInfos` 的移植。
//!
//! 本轮只覆盖林地府邸用到的组合：`StructurePlaceSettings` 里 rotation / mirror、
//! `BlockIgnoreProcessor.STRUCTURE_BLOCK`、`setIgnoreEntities(true)`、`setBoundingBox(chunkBB)`。
//!
//! 逐条照官方翻译（`StructureTemplate.java`）：`getRandomPalette` 的多 palette 按
//! `Mth.getSeed(position)` 选（不消耗世界随机源）、`calculateRelativePosition`
//! （`transform(pos, mirror, rotation, pivot)`）、`processBlockInfos` 只处理落在 `chunkBB`
//! 里的方块并按 `structure_block` 身份过滤、写入循环的 `boundingBox.isInside` →
//! `mirror` / `rotate` → `setBlock`，以及 `RandomizableContainer` 的 `nextLong()`。
//!
//! 已知限制：官方在 `placeInWorld` 之后还会跑 `updateShapeAtEdge` /
//! `updateFromNeighbourShapes`（林地府邸没有 `setKnownShape`），这两步留给调用方
//! （`feature::shape_update` 已有等价实现）。带 `nbt` 的方块官方先写 `BARRIER` 再写真实
//! 状态，那一步只为移除旧方块实体，不影响最终方块，这里省掉。
//!
//! Per-block structure template placement: a port of `StructureTemplate.placeInWorld` and
//! `processBlockInfos`, covering the combination the woodland mansion uses (rotation /
//! mirror, `BlockIgnoreProcessor.STRUCTURE_BLOCK`, `setIgnoreEntities(true)`,
//! `setBoundingBox(chunkBB)`).
//!
//! Translated point by point from vanilla `StructureTemplate.java`: `getRandomPalette`
//! picks a multi-palette by `Mth.getSeed(position)` without touching the worldgen random,
//! `calculateRelativePosition` is `transform(pos, mirror, rotation, pivot)`,
//! `processBlockInfos` keeps only blocks inside `chunkBB` and filters `structure_block`,
//! and the write loop goes `boundingBox.isInside` → `mirror` / `rotate` → `setBlock`, with
//! one `nextLong()` per `RandomizableContainer`.
//!
//! Known limits: vanilla also runs `updateShapeAtEdge` / `updateFromNeighbourShapes` after
//! `placeInWorld` (the mansion never calls `setKnownShape`); those steps are left to the
//! caller (`feature::shape_update` already has an equivalent). Blocks with `nbt` get a
//! `BARRIER` written first in vanilla only to drop the old block entity, which does not
//! affect the final block, so it is skipped here.

use crate::structure::piece_place::{PieceLevel, box_is_inside, is_block};
use crate::structure::state_transform::{Mirror, Rotation, transform};
use crate::structure::template::{PaletteEntry, StructureTemplate};

/// `StructurePlaceSettings` 里本轮要用的部分。
///
/// The part of `StructurePlaceSettings` this round needs.
#[derive(Debug, Clone, Copy)]
pub struct PlaceSettings {
    pub mirror: Mirror,
    pub rotation: Rotation,
    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`：模板里的结构方块不写。
    ///
    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`: structure blocks are not written.
    pub ignore_structure_blocks: bool,
    /// `LiquidSettings.APPLY_WATERLOGGING`（官方的默认值）。
    ///
    /// `LiquidSettings.APPLY_WATERLOGGING`, vanilla's default.
    pub apply_waterlogging: bool,
}

impl PlaceSettings {
    /// 林地府邸的 `makeSettings`：`setIgnoreEntities(true)` + rotation + mirror +
    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`。
    ///
    /// The mansion's `makeSettings`: `setIgnoreEntities(true)`, rotation, mirror and
    /// `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
    pub fn mansion(rotation: Rotation, mirror: Mirror) -> Self {
        Self {
            mirror,
            rotation,
            ignore_structure_blocks: true,
            apply_waterlogging: true,
        }
    }
}

/// `StructureTemplate.transform(pos, mirror, rotation, pivot)`，pivot 恒为 `(0, 0, 0)`。
///
/// `StructureTemplate.transform(pos, mirror, rotation, pivot)` with a zero pivot.
pub fn transform_pos(pos: (i32, i32, i32), mirror: Mirror, rotation: Rotation) -> (i32, i32, i32) {
    let (mut x, y, mut z) = pos;
    let mirrored = match mirror {
        Mirror::LeftRight => {
            z = -z;
            true
        }
        Mirror::FrontBack => {
            x = -x;
            true
        }
        Mirror::None => false,
    };
    match rotation {
        Rotation::CounterClockwise90 => (z, y, -x),
        Rotation::Clockwise90 => (-z, y, x),
        Rotation::Clockwise180 => (-x, y, -z),
        Rotation::None => {
            if mirrored {
                (x, y, z)
            } else {
                pos
            }
        }
    }
}

/// palette 名 + 属性还原成状态 ID；未导出的方块报错。
///
/// Turn a palette name plus properties into a state id; unknown blocks are an error.
fn state_of(entry: &PaletteEntry) -> Result<u32, String> {
    let mut state = mc_bamboo_data::block_by_name(&entry.name)
        .ok_or_else(|| format!("unknown block {}", entry.name))?
        .default_state;
    for (name, value) in &entry.properties {
        state = crate::feature::state::with_property(state, name, value)
            .ok_or_else(|| format!("{} has no property {name}={value}", entry.name))?;
    }
    Ok(state)
}

/// 一条已算出世界坐标的模板方块。
///
/// One template block with its world position resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedBlock {
    pub pos: (i32, i32, i32),
    pub state: u32,
    /// 模板里带方块实体 NBT：战利品表种子要取一次 `nextLong()`。
    ///
    /// The template entry carries block-entity NBT, so the loot-table seed needs a
    /// `nextLong()` draw.
    pub has_nbt: bool,
}

/// `placeInWorld` 的「算出要写哪些方块」那一段（不含写入）。
///
/// The "which blocks to write" part of `placeInWorld` (writes excluded).
pub fn blocks_in_chunk(
    template: &StructureTemplate,
    position: (i32, i32, i32),
    settings: PlaceSettings,
    chunk_bb: &[i32; 6],
) -> Result<Vec<PlacedBlock>, String> {
    let Some(palette_index) = template.palette_index(position) else {
        return Ok(Vec::new());
    };
    let palette = &template.palettes[palette_index];
    let mut out = Vec::new();
    for entry in &template.blocks {
        // `calculateRelativePosition(settings, blockInfo.pos).offset(position)`。
        // `calculateRelativePosition(settings, blockInfo.pos).offset(position)`.
        let local = transform_pos(entry.pos, settings.mirror, settings.rotation);
        let world = (
            local.0 + position.0,
            local.1 + position.1,
            local.2 + position.2,
        );
        if !box_is_inside(chunk_bb, world.0, world.1, world.2) {
            continue;
        }
        let Some(palette_entry) = palette.get(entry.state) else {
            return Err(format!(
                "palette index {} out of range (palette has {} entries)",
                entry.state,
                palette.len()
            ));
        };
        let state = state_of(palette_entry)?;
        // `BlockIgnoreProcessor.STRUCTURE_BLOCK` 在 `processBlockInfos` 里丢方块。
        // `BlockIgnoreProcessor.STRUCTURE_BLOCK` drops blocks in `processBlockInfos`.
        if settings.ignore_structure_blocks && is_block(state, "minecraft:structure_block") {
            continue;
        }
        out.push(PlacedBlock {
            pos: world,
            state,
            has_nbt: entry.nbt.is_some(),
        });
    }
    Ok(out)
}

/// `StructureTemplate.placeInWorld` 的写入循环。
///
/// 官方按 palette 顺序逐块写；每块先过 `boundingBox.isInside`（就是调用方传的
/// `chunkBB`），再做 `mirror` / `rotate`，然后 `setBlock`。
///
/// 带 `nbt` 的方块官方会读 `level.getBlockEntity(pos)`，是 `RandomizableContainer` 时
/// 才 `blockInfo.nbt.putLong("LootTableSeed", random.nextLong())`。世界生成期这里的
/// 区块是 `ProtoChunk`，`setBlock` 只把方块实体 NBT 放进 `pendingBlockEntities`，
/// `getBlockEntity` 读的是另一张表，返回 null，所以**一次 `nextLong()` 都不取**。
///
/// The write loop of `StructureTemplate.placeInWorld`. Vanilla writes block by block in
/// palette order; each goes through `boundingBox.isInside` (the caller's `chunkBB`), then
/// `mirror` / `rotate`, then `setBlock`.
///
/// For blocks with `nbt` vanilla reads `level.getBlockEntity(pos)` and only draws
/// `blockInfo.nbt.putLong("LootTableSeed", random.nextLong())` for a
/// `RandomizableContainer`. During chunk generation the chunk is a `ProtoChunk`, whose
/// `setBlock` only fills `pendingBlockEntities` while `getBlockEntity` reads another map
/// and returns null, so no `nextLong()` is drawn here.
pub fn place_blocks(
    level: &mut dyn PieceLevel,
    chunk_bb: &[i32; 6],
    blocks: &[PlacedBlock],
    settings: PlaceSettings,
) -> Vec<(i32, i32, i32)> {
    // `LiquidSettings` 的两段：写入前取该格的流体，写完把旧流体回填进含水方块。
    // The two `LiquidSettings` steps: read the cell's fluid before the write, then pour it
    // back into waterloggable blocks afterwards.
    let mut to_fill: Vec<(i32, i32, i32)> = Vec::new();
    let mut locked: Vec<(i32, i32, i32)> = Vec::new();
    let mut placed = Vec::with_capacity(blocks.len());
    for entry in blocks {
        if !box_is_inside(chunk_bb, entry.pos.0, entry.pos.1, entry.pos.2) {
            continue;
        }
        let previous = settings
            .apply_waterlogging
            .then(|| level.get_block(entry.pos.0, entry.pos.1, entry.pos.2));
        let state = transform(entry.state, settings.mirror, settings.rotation);
        level.set_block(entry.pos.0, entry.pos.1, entry.pos.2, state);
        placed.push(entry.pos);

        let Some(previous) = previous else {
            continue;
        };
        // `state.getFluidState().isSource()` → `lockedFluids.add(blockPos)`。
        // `state.getFluidState().isSource()` → `lockedFluids.add(blockPos)`.
        if is_source(state) {
            locked.push(entry.pos);
            continue;
        }
        // `state.getBlock() instanceof LiquidBlockContainer` → `placeLiquid(...)`。
        // `state.getBlock() instanceof LiquidBlockContainer` → `placeLiquid(...)`.
        if !is_liquid_container(state) || is_empty_fluid(previous) {
            continue;
        }
        if place_liquid(level, entry.pos, state, previous) && !is_source(previous) {
            to_fill.push(entry.pos);
        }
    }

    // `while (filled && !toFill.isEmpty())`：按 `[UP, NORTH, EAST, SOUTH, WEST]` 找相邻
    // 流体源再回填一次，`lockedFluids` 里的位置不参与。
    // `while (filled && !toFill.isEmpty())`: look for a neighbouring source along
    // `[UP, NORTH, EAST, SOUTH, WEST]` and pour again, skipping `lockedFluids`.
    const DIRECTIONS: [(i32, i32, i32); 5] =
        [(1, 0, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0)];
    loop {
        let mut filled = false;
        let mut index = 0;
        while index < to_fill.len() {
            let pos = to_fill[index];
            let current = level.get_block(pos.0, pos.1, pos.2);
            let mut fluid = is_source(current).then_some(current);
            if fluid.is_none() {
                for (dx, dy, dz) in DIRECTIONS {
                    let neighbor = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                    if locked.contains(&neighbor) {
                        continue;
                    }
                    let neighbor_state = level.get_block(neighbor.0, neighbor.1, neighbor.2);
                    if is_source(neighbor_state) {
                        fluid = Some(neighbor_state);
                        break;
                    }
                }
            }
            let Some(fluid) = fluid else {
                index += 1;
                continue;
            };
            if is_liquid_container(current) && place_liquid(level, pos, current, fluid) {
                filled = true;
                to_fill.remove(index);
                continue;
            }
            index += 1;
        }
        if !filled {
            break;
        }
    }
    placed
}

/// `state.getFluidState().isSource()`。
///
/// `state.getFluidState().isSource()`.
fn is_source(state: u32) -> bool {
    let fluid = mc_bamboo_blocks::BlockState(state).fluid();
    !fluid.is_empty() && fluid.is_source()
}

/// `state.getFluidState().isEmpty()`。
///
/// `state.getFluidState().isEmpty()`.
fn is_empty_fluid(state: u32) -> bool {
    mc_bamboo_blocks::BlockState(state).fluid().is_empty()
}

/// 方块是不是 `LiquidBlockContainer`。
///
/// Whether the block is a `LiquidBlockContainer`.
fn is_liquid_container(state: u32) -> bool {
    let Some(id) = mc_bamboo_data::block_id_of_state(state) else {
        return false;
    };
    // 本版本实现 `LiquidBlockContainer` 的都是 `SimpleWaterloggedBlock`（台阶、楼梯、
    // 栅栏、墙……），它们都有 `waterlogged` 属性；按属性判定与官方 `instanceof`
    // 在这批方块上等价。
    //
    // Every `LiquidBlockContainer` in this version is a `SimpleWaterloggedBlock` (slabs,
    // stairs, fences, walls, ...) and carries a `waterlogged` property, so testing the
    // property matches vanilla's `instanceof` on this block set.
    mc_bamboo_data::blocks::BLOCKS[id]
        .properties
        .iter()
        .any(|property| property.name == "waterlogged")
}

/// 调一次 `LiquidBlockContainer.placeLiquid`，返回它是否真的放进了流体。
///
/// One `LiquidBlockContainer.placeLiquid`, returning whether the fluid landed.
fn place_liquid(level: &mut dyn PieceLevel, pos: (i32, i32, i32), state: u32, fluid: u32) -> bool {
    crate::structure::template_shape_update::place_liquid(
        level,
        pos,
        mc_bamboo_blocks::BlockState(state),
        mc_bamboo_blocks::BlockState(fluid).fluid(),
    )
}
