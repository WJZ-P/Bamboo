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
//! （`feature::shape_update` 已有等价实现）。
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
//! caller (`feature::shape_update` already has an equivalent).

use mc_bamboo_data::BlockAncestor;

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
/// 带 `nbt` 的方块官方先写一次 `BARRIER`，再写真实状态；之后读
/// `level.getBlockEntity(pos)`，是 `RandomizableContainer` 时取
/// `random.nextLong()` 作 `LootTableSeed`。`WorldGenRegion.getBlockEntity` 会把
/// `setBlock` 留下的 `DUMMY` 占位换成真的方块实体，所以箱子、木桶这类**每个都取一次**。
///
/// The write loop of `StructureTemplate.placeInWorld`. Vanilla writes block by block in
/// palette order; each goes through `boundingBox.isInside` (the caller's `chunkBB`), then
/// `mirror` / `rotate`, then `setBlock`.
///
/// Blocks with `nbt` get a `BARRIER` written first, then the real state; vanilla then
/// reads `level.getBlockEntity(pos)` and draws `random.nextLong()` as the `LootTableSeed`
/// of a `RandomizableContainer`. `WorldGenRegion.getBlockEntity` turns the `DUMMY`
/// placeholder `setBlock` left into a real block entity, so every chest, barrel and the
/// like draws once.
pub fn place_blocks(
    level: &mut dyn PieceLevel,
    chunk_bb: &[i32; 6],
    blocks: &[PlacedBlock],
    settings: PlaceSettings,
    random: &crate::random::WorldgenRandom,
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
        if entry.has_nbt {
            level.set_block(
                entry.pos.0,
                entry.pos.1,
                entry.pos.2,
                crate::structure::piece_place::block("minecraft:barrier"),
            );
        }
        level.set_block(entry.pos.0, entry.pos.1, entry.pos.2, state);
        placed.push(entry.pos);
        if entry.has_nbt && is_randomizable_container(state) {
            random.next_long();
        }

        let Some(previous) = previous else {
            continue;
        };
        // `state.getFluidState().isSource()` → `lockedFluids.add(blockPos)`。
        // `state.getFluidState().isSource()` → `lockedFluids.add(blockPos)`.
        if is_source(state) {
            locked.push(entry.pos);
            continue;
        }
        // `state.getBlock() instanceof LiquidBlockContainer` → `placeLiquid(...)`，不看它
        // 成不成功；旧流体不是源（包括空流体）就进 `toFill`。
        // `state.getBlock() instanceof LiquidBlockContainer` → `placeLiquid(...)` whatever
        // it returns; a previous fluid that is not a source (empty included) joins `toFill`.
        if !is_liquid_container(state) {
            continue;
        }
        if !is_empty_fluid(previous) {
            place_liquid(level, entry.pos, state, previous);
        }
        if !is_source(previous) {
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
            // 官方只要是 `LiquidBlockContainer` 就算填过、移出 `toFill`，不看 `placeLiquid`
            // 的结果。
            // Vanilla counts any `LiquidBlockContainer` as filled and drops it from
            // `toFill`, whatever `placeLiquid` returns.
            if is_liquid_container(current) {
                place_liquid(level, pos, current, fluid);
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
    has_ancestor(state, &[BlockAncestor::LiquidBlockContainer])
}

/// 方块实体是不是 `RandomizableContainer`（箱子一族、木桶、合成器、饰纹陶罐、发射器 /
/// 投掷器、漏斗、潜影盒）。
///
/// Whether the block entity is a `RandomizableContainer` (the chest family, barrel,
/// crafter, decorated pot, dispenser / dropper, hopper, shulker box).
fn is_randomizable_container(state: u32) -> bool {
    has_ancestor(
        state,
        &[
            BlockAncestor::ChestBlock,
            BlockAncestor::BarrelBlock,
            BlockAncestor::CrafterBlock,
            BlockAncestor::DecoratedPotBlock,
            BlockAncestor::DispenserBlock,
            BlockAncestor::HopperBlock,
            BlockAncestor::ShulkerBoxBlock,
        ],
    )
}

/// 方块类（含自身）是否继承或实现了其中之一。
///
/// Whether the block class (itself included) extends or implements any of these.
fn has_ancestor(state: u32, wanted: &[BlockAncestor]) -> bool {
    mc_bamboo_data::block_of_state(state)
        .is_some_and(|block| block.class.ancestors().iter().any(|a| wanted.contains(a)))
}

/// 在 `WorldGenRegion` 上调一次 `LiquidBlockContainer.placeLiquid`，返回它是否放进了流体。
///
/// 所有会放的实现（`SimpleWaterloggedBlock`、台阶非双层、营火、蜡烛、干涸恶魂）都要求
/// 方块还没含水、流体是水源（`is(Fluids.WATER)`，流动的水不算），然后
/// `setBlockAndUpdate` 写一次含水状态；营火和点着的蜡烛顺带熄灭。`WorldGenRegion` 的
/// `setBlock` 不做邻居形状更新，声音、粒子、计划刻都不改方块。海带、海草恒返回 false。
///
/// One `LiquidBlockContainer.placeLiquid` on a `WorldGenRegion`, returning whether the
/// fluid went in. Every implementation that accepts (`SimpleWaterloggedBlock`, non-double
/// slabs, campfire, candle, dried ghast) requires the block not to be waterlogged yet and
/// the fluid to be a water source (`is(Fluids.WATER)`; flowing water does not count), then
/// writes the waterlogged state once through `setBlockAndUpdate`; campfires and lit candles
/// are put out as well. `WorldGenRegion.setBlock` runs no neighbour shape updates, and
/// sounds, particles and scheduled ticks change no block. Kelp and seagrass always refuse.
fn place_liquid(level: &mut dyn PieceLevel, pos: (i32, i32, i32), state: u32, fluid: u32) -> bool {
    use crate::structure::state_transform::property_of;
    if property_of(state, "waterlogged") != Some("false")
        || property_of(state, "type") == Some("double")
        || !mc_bamboo_blocks::BlockState(fluid)
            .fluid()
            .is(mc_bamboo_data::registries::fluid::WATER)
    {
        return false;
    }
    let mut new_state =
        crate::structure::piece_place::with_property(state, "waterlogged", "true").unwrap_or(state);
    if property_of(new_state, "lit").is_some() {
        new_state = crate::structure::piece_place::with_property(new_state, "lit", "false")
            .unwrap_or(new_state);
    }
    level.set_block(pos.0, pos.1, pos.2, new_state);
    true
}

/// `BlockStateParser.parseForBlock(..., stateString, true)`：解析
/// `minecraft:oak_planks[axis=y]` 这种拼图方块的 `final_state`。
///
/// 官方在 `BlockStateParser` 里把 `name[props]` 拆开、按 `StateDefinition` 逐属性校验；
/// 这里复用 `mc-bamboo-data` 的方块表与 `feature::state::with_property`，行为相同。
///
/// `BlockStateParser.parseForBlock(..., stateString, true)`: parse a jigsaw block's
/// `final_state` such as `minecraft:oak_planks[axis=y]`. Vanilla splits `name[props]` and
/// validates each property against the `StateDefinition`; this reuses `mc-bamboo-data`'s
/// block table and `feature::state::with_property`, which behaves the same.
pub fn parse_block_state(text: &str) -> Result<u32, String> {
    let (name, rest) = match text.split_once('[') {
        Some((name, rest)) => (name, Some(rest.strip_suffix(']').unwrap_or(rest))),
        None => (text, None),
    };
    let mut state = mc_bamboo_data::block_by_name(name)
        .ok_or_else(|| format!("unknown block {name}"))?
        .default_state;
    let Some(props) = rest else {
        return Ok(state);
    };
    for pair in props.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("bad block state property {pair}"))?;
        state = crate::feature::state::with_property(state, key.trim(), value.trim())
            .ok_or_else(|| format!("{name} has no property {key}={value}"))?;
    }
    Ok(state)
}

/// `TemplateStructurePiece.postProcess` 里那段拼图方块替换。
///
/// 官方对 `filterBlocks(..., JIGSAW)` 的每个拼图方块读 `final_state`（缺省
/// `minecraft:air`）、`BlockStateParser.parseForBlock` 解析，然后
/// `level.setBlockAndUpdate(pos, targetState)`。解析失败只记日志。
///
/// The jigsaw-block replacement inside `TemplateStructurePiece.postProcess`. For every
/// jigsaw block from `filterBlocks(..., JIGSAW)`, vanilla reads `final_state` (default
/// `minecraft:air`), parses it with `BlockStateParser.parseForBlock` and calls
/// `level.setBlockAndUpdate(pos, targetState)`. A parse failure is only logged.
pub fn replace_jigsaws(
    template: &StructureTemplate,
    position: (i32, i32, i32),
    settings: PlaceSettings,
    chunk_bb: &[i32; 6],
    level: &mut dyn PieceLevel,
) {
    let Some(palette_index) = template.palette_index(position) else {
        return;
    };
    let Some(jigsaws) = template.palette_jigsaws.get(palette_index) else {
        return;
    };
    for jigsaw in jigsaws {
        let local = transform_pos(jigsaw.pos, settings.mirror, settings.rotation);
        let world = (
            local.0 + position.0,
            local.1 + position.1,
            local.2 + position.2,
        );
        if !box_is_inside(chunk_bb, world.0, world.1, world.2) {
            continue;
        }
        // 解析失败只记日志，不中断（官方 `catch (CommandSyntaxException)` 后保留默认 air）。
        // A parse failure is only logged; vanilla keeps the default air and continues.
        let Ok(state) = parse_block_state(&jigsaw.final_state) else {
            continue;
        };
        level.set_block(world.0, world.1, world.2, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_block_state_reads_a_name_and_its_properties() {
        let plain = parse_block_state("minecraft:stone").unwrap();
        assert_eq!(
            mc_bamboo_data::blocks::BLOCKS[mc_bamboo_data::block_id_of_state(plain).unwrap()].name,
            "minecraft:stone"
        );
        let with_props = parse_block_state("minecraft:oak_log[axis=x]").unwrap();
        let id = mc_bamboo_data::block_id_of_state(with_props).unwrap();
        assert_eq!(mc_bamboo_data::blocks::BLOCKS[id].name, "minecraft:oak_log");
        assert_ne!(
            with_props,
            parse_block_state("minecraft:oak_log[axis=y]").unwrap()
        );
    }

    #[test]
    fn parse_block_state_rejects_unknown_blocks_and_properties() {
        assert!(parse_block_state("minecraft:not_a_block").is_err());
        assert!(parse_block_state("minecraft:stone[nope=1]").is_err());
    }

    #[test]
    fn transform_pos_matches_the_vanilla_corner_cases() {
        // `StructureTemplate.transform` 的四条分支各取一个点（pivot 为 ZERO）：
        // CCW90 → `(-z, y, -x)`，CW90 → `(-z, y, x)`，CW180 → `(-x, y, -z)`。
        // One point per `StructureTemplate.transform` branch with a zero pivot:
        // CCW90 is `(-z, y, -x)`, CW90 `(-z, y, x)`, CW180 `(-x, y, -z)`.
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::None, Rotation::None),
            (3, 1, 5)
        );
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::None, Rotation::Clockwise180),
            (-3, 1, -5)
        );
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::None, Rotation::Clockwise90),
            (-5, 1, 3)
        );
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::None, Rotation::CounterClockwise90),
            (5, 1, -3)
        );
        // 镜像先作用，再旋转。
        // The mirror runs first, then the rotation.
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::LeftRight, Rotation::None),
            (3, 1, -5)
        );
        assert_eq!(
            transform_pos((3, 1, 5), Mirror::FrontBack, Rotation::None),
            (-3, 1, 5)
        );
    }
}
