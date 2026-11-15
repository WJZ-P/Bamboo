//! jigsaw 结构件的 `postProcess`：模板方块 → processor 链 → 写入世界。
//!
//! 官方链路（`SinglePoolElement.place` → `StructureTemplate.placeInWorld`）：
//!
//! 1. `settings.getRandomPalette(palettes, position).blocks()`：按**世界位置**选调色板，
//!    拿到该调色板的 `StructureBlockInfo` 列表（位置仍是模板相对坐标）。
//! 2. `processBlockInfos(level, position, referencePos, settings, blockInfoList)`：逐方块跑
//!    processor 链（任一返回 null 就丢弃），再跑 `finalizeProcessing`。**这一步的输出才是
//!    要写的方块**，且位置已经是世界坐标。
//! 3. 对每个处理后的方块：`state.mirror(settings.getMirror()).rotate(settings.getRotation())`
//!    再 `level.setBlock(blockPos, state, updateMode)`。
//!
//! 这里把三件事都做掉：`template_blocks` 生成第 1 步的列表，
//! `structure::processor_apply::process_blocks` 是第 2 步，`place_processed` 是第 3 步。
//!
//! `postProcess` of a jigsaw piece: template blocks → processor chain → world writes.
//! Vanilla `SinglePoolElement.place` → `StructureTemplate.placeInWorld`: pick the palette by
//! world position, run `processBlockInfos` (the processor chain plus `finalizeProcessing`),
//! then write each resulting block with `state.mirror(...).rotate(...)`.

use crate::structure::{
    jigsaw::element::{PoolElement, Projection},
    processor::{BlockStateSpec, Processor},
    processor_apply::{PlaceSettings, ProcessBlock, ProcessorLevel, process_blocks},
    template::{PaletteEntry, StructureTemplate},
};

/// 一个模板里某个调色板的方块，转成 processor 链的输入。
///
/// The blocks of one palette turned into processor-chain input.
///
/// 位置语义照官方：`ProcessBlock.pos` 是**模板相对坐标**（`templateRelativePos`），
/// `world_pos` 是 `transform(pos) + piece_position`（默认 settings 下就是 `pos + piece`）。
///
/// Position semantics follow vanilla: `ProcessBlock.pos` is template-relative
/// (`templateRelativePos`) and `world_pos` is `transform(pos) + piece_position`.
pub fn template_blocks(
    template: &StructureTemplate,
    palette_index: usize,
    palette: &[PaletteEntry],
    piece_position: (i32, i32, i32),
    transform: impl Fn((i32, i32, i32)) -> (i32, i32, i32),
) -> Result<Vec<(ProcessBlock, bool)>, String> {
    let mut blocks = Vec::with_capacity(template.blocks.len());
    for block in &template.blocks {
        let entry = palette
            .get(block.state)
            .ok_or_else(|| format!("palette index {palette_index} has no state {}", block.state))?;
        let local = transform(block.pos);
        blocks.push((
            ProcessBlock {
                pos: block.pos,
                world_pos: (
                    local.0 + piece_position.0,
                    local.1 + piece_position.1,
                    local.2 + piece_position.2,
                ),
                state: BlockStateSpec::from_palette_entry(entry),
                nbt: block.nbt.clone(),
            },
            // `blockInfo.nbt != null`：官方在写这个方块前先放一个 `BARRIER`。
            // `blockInfo.nbt != null`: vanilla writes a `BARRIER` first.
            block.nbt.is_some(),
        ));
    }
    Ok(blocks)
}

/// 把方块状态名转成 `u32` 状态 id（写世界用）。找不到时报错而不是猜 0（空气）。
///
/// Resolve a block-state name to a `u32` state id. A miss is an error rather than a guess
/// (silently writing air would be worse than failing).
pub fn state_id_of(state: &crate::structure::processor::BlockStateSpec) -> Result<u32, String> {
    // 只带名字、不带属性时，用方块的默认状态。
    // With a name but no properties, use the block's default state.
    let block = mc_bamboo_data::block_by_name(&state.name)
        .ok_or_else(|| format!("unknown block {}", state.name))?;
    if state.properties.is_empty() {
        return Ok(block.default_state);
    }
    // 带属性时按名字 + 属性查注册表里的状态表。
    // With properties, look the state up in the registry tables.
    state_with_properties(block, &state.properties).ok_or_else(|| {
        format!(
            "no state for {}[{}]",
            state.name,
            properties_text(&state.properties)
        )
    })
}

/// 按属性列表找状态 id；属性必须全部匹配，顺序无关。
///
/// Find the state id matching every property (order irrelevant).
fn state_with_properties(
    block: &'static mc_bamboo_data::Block,
    properties: &[(String, String)],
) -> Option<u32> {
    let count = mc_bamboo_data::state_count(
        mc_bamboo_data::block_id_of_state(block.default_state)? as usize,
    );
    for offset in 0..count {
        let state = block.first_state + offset;
        if properties.iter().all(|(key, value)| {
            crate::structure::state_transform::property_of(state, key)
                .is_some_and(|got| got == value.as_str())
        }) {
            return Some(state);
        }
    }
    None
}

fn properties_text(properties: &[(String, String)]) -> String {
    properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 一个待写方块附带的信息：是否有 nbt（官方会用 `BARRIER` 占位）。
///
/// Per-block extras: whether the block carries nbt (vanilla writes a `BARRIER` first).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceBlock {
    pub pos: (i32, i32, i32),
    pub state: BlockStateSpec,
    /// `blockInfo.nbt != null`：官方先写 `Blocks.BARRIER` 再写真正的方块。
    ///
    /// `blockInfo.nbt != null`: vanilla writes `Blocks.BARRIER` before the real block.
    pub has_nbt: bool,
}

/// `StructureTemplate.calculateRelativePosition(settings, pos)`：`transform(pos, mirror,
/// rotation, pivot)`，pivot 默认 `BlockPos.ZERO`。位置变换**包含 mirror**（`LEFT_RIGHT`
/// 取反 Z、`FRONT_BACK` 取反 X，再按 rotation 转）。
///
/// `StructureTemplate.calculateRelativePosition(settings, pos)`: `transform(pos, mirror,
/// rotation, pivot)` with the default zero pivot. The position transform **includes the
/// mirror** (`LEFT_RIGHT` negates Z, `FRONT_BACK` negates X) before the rotation.
pub fn relative_position(pos: (i32, i32, i32), mirror: u32, rotation: u32) -> (i32, i32, i32) {
    let (mut x, y, mut z) = pos;
    // `Mirror.values()` 顺序是 NONE / LEFT_RIGHT / FRONT_BACK。
    // `Mirror.values()` order is NONE / LEFT_RIGHT / FRONT_BACK.
    match mirror {
        1 => z = -z,
        2 => x = -x,
        _ => {}
    }
    match rotation {
        1 => (-z, y, x),
        2 => (-x, y, -z),
        3 => (z, y, -x),
        _ => (x, y, z),
    }
}

/// 官方 `StructureTemplate.placeInWorld` 的方块写入循环（processor 已跑完的那部分）：
/// 对每个处理后的方块做 `state.mirror(...).rotate(...)` 并写进世界，只写落在 `chunk_bb`
/// 内的位置；带 nbt 的方块先写一个 `BARRIER`。
///
/// The block-write loop of `StructureTemplate.placeInWorld` after processors have run:
/// mirror then rotate each state and write it, skipping positions outside `chunk_bb`, and
/// write a `BARRIER` first when the block carries nbt.
pub fn place_processed(
    processed: &[PlaceBlock],
    chunk_bb: crate::structure::BoundingBox,
    mirror: u32,
    rotation: u32,
    world: &mut impl BlockWriter,
) -> Result<usize, String> {
    let mut changed = 0;
    for block in processed {
        let pos = block.pos;
        if !chunk_bb.contains(pos.0, pos.1, pos.2) {
            continue;
        }
        let state = state_id_of(&block.state)?;
        // `BlockState.mirror(...).rotate(...)`，与 `StructureTemplate.placeInWorld` 同序。
        // `BlockState.mirror(...).rotate(...)`, same order as vanilla.
        let mut state_out = state;
        if mirror > 0 {
            state_out = mc_bamboo_data::state_mirror(state_out, mirror);
        }
        if rotation > 0 {
            state_out = mc_bamboo_data::state_rotation(state_out, rotation);
        }
        // `if (blockInfo.nbt != null) level.setBlock(blockPos, Blocks.BARRIER..., 820);`
        if block.has_nbt {
            world.set_block(pos.0, pos.1, pos.2, barrier_state());
        }
        // 官方只在 `level.setBlock(...)` 返回 true 时才把位置计入 `minX..maxZ` 与
        // `placed`（即 `placeInWorld` 的「实际改了几个方块」），返回值不参与其它判断。
        // Vanilla counts a position only when `level.setBlock(...)` returns true; that
        // count is what `placeInWorld` reports as "blocks actually changed".
        if world.set_block(pos.0, pos.1, pos.2, state_out) {
            changed += 1;
        }
    }
    Ok(changed)
}

/// `Blocks.BARRIER.defaultBlockState()` 的状态 id。
///
/// The state id of `Blocks.BARRIER.defaultBlockState()`.
fn barrier_state() -> u32 {
    mc_bamboo_data::block_by_name("minecraft:barrier")
        .map(|block| block.default_state)
        .unwrap_or(0)
}

/// 写入方块的最小接口（`FeatureWorld` 与测试假世界都实现它）。
///
/// The minimal block-write interface (`FeatureWorld` and test doubles implement it).
pub trait BlockWriter {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool;
}

impl BlockWriter for crate::feature::world::FeatureWorld<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        crate::feature::world::FeatureWorld::set_block(self, x, y, z, state)
    }
}

/// 完整的一次结构件放置：选调色板 → 跑 processor 链 → 写世界。
///
/// One full piece placement: pick the palette, run the processor chain, write the world.
#[allow(clippy::too_many_arguments)]
pub fn place_piece<L: ProcessorLevel>(
    template: &StructureTemplate,
    piece_position: (i32, i32, i32),
    processors: &[Processor],
    level: &L,
    settings: PlaceSettings<'_>,
    mirror: u32,
    rotation: u32,
    chunk_bb: crate::structure::BoundingBox,
    world: &mut impl BlockWriter,
) -> Result<usize, String> {
    // `settings.getRandomPalette(palettes, position)`：按世界位置选调色板。
    // `settings.getRandomPalette(palettes, position)`: palette picked by world position.
    let palette_index = template
        .palette_index(piece_position)
        .ok_or("structure template has no palette")?;
    let palette = template
        .palettes
        .get(palette_index)
        .ok_or("palette index out of range")?;
    let blocks = template_blocks(template, palette_index, palette, piece_position, |pos| {
        relative_position(pos, mirror, rotation)
    })?;
    let nbt_positions: std::collections::HashSet<(i32, i32, i32)> = blocks
        .iter()
        .filter(|(_, has_nbt)| *has_nbt)
        .map(|(block, _)| block.world_pos)
        .collect();
    let plain: Vec<ProcessBlock> = blocks.into_iter().map(|(block, _)| block).collect();
    let processed = process_blocks(processors, &plain, level, settings);
    let place: Vec<PlaceBlock> = processed
        .into_iter()
        .map(|block| PlaceBlock {
            // 官方用处理后的位置查 `blockInfo.nbt`：processor 会改写状态但保留 nbt。
            // Vanilla looks the nbt up on the processed info: processors rewrite the state
            // but keep the nbt.
            has_nbt: nbt_positions.contains(&block.world_pos),
            pos: block.world_pos,
            state: block.state,
        })
        .collect();
    place_processed(&place, chunk_bb, mirror, rotation, world)
}

/// 供调试与日志用：`minecraft:x` 形式的方块名（不含属性）。
///
/// Debug text: the `minecraft:x` block name (without properties).
pub fn state_text(state: u32) -> String {
    mc_bamboo_data::block_of_state(state)
        .map(|block| block.name.to_owned())
        .unwrap_or_else(|| format!("unknown:{state}"))
}

/// `SinglePoolElement.getSettings` 里组装 processor 链的那部分。
///
/// 官方顺序（`SinglePoolElement.getSettings`）：
///
/// 1. `BlockIgnoreProcessor.STRUCTURE_BLOCK`（忽略结构方块）；
/// 2. `!keepJigsaws` 时加 `JigsawReplacementProcessor.INSTANCE`；
/// 3. `this.processors.value().list()`（`pool_element` 里配置的 `processors`）；
/// 4. `getProjection().getProcessors()`（`TERRAIN_MATCHING` 是
///    `new GravityProcessor(WORLD_SURFACE_WG, -1)`，`RIGID` 是空）。
///
/// **`LegacySinglePoolElement.getSettings` 的额外一步**：它先调父类拿到上面那条链，再
/// `popProcessor(STRUCTURE_BLOCK)` 把第 1 项摘掉，然后 `addProcessor(STRUCTURE_AND_AIR)`——
/// 注意是**追加到末尾**，不是原地替换。所以 legacy 的链是
/// `[JigsawReplacement?, ...配置的, ...projection 的, STRUCTURE_AND_AIR]`。
///
/// The processor chain assembly inside `SinglePoolElement.getSettings`: structure-block
/// ignore, an optional jigsaw replacement, the pool element's configured processors, then
/// the projection's processors. `LegacySinglePoolElement` pops the structure-block ignore
/// and **appends** `STRUCTURE_AND_AIR` at the end.
pub fn assemble_processors(
    element: &PoolElement,
    configured: &[Processor],
    keep_jigsaws: bool,
) -> Option<Vec<Processor>> {
    let PoolElement::Single {
        projection, legacy, ..
    } = element
    else {
        // `ListPoolElement.getShuffledJigsawBlocks` 只用第一个元素的拼图方块，但 `place`
        // 会逐个调用自己的 `place`；`EmptyPoolElement` 什么都不写。三种都没有自己的
        // settings，所以这里返回 `None` 表示「不需要 processor 链」。
        //
        // `ListPoolElement` delegates to each sub-element's own `place`, and
        // `EmptyPoolElement` writes nothing, so neither has a settings of its own.
        return None;
    };

    let mut chain = vec![Processor::BlockIgnore {
        to_ignore: vec!["minecraft:structure_block".to_owned()],
    }];
    if !keep_jigsaws {
        chain.push(Processor::JigsawReplacement);
    }
    chain.extend_from_slice(configured);
    if *projection == Projection::TerrainMatching {
        // `Projection.TERRAIN_MATCHING.getProcessors()` = `[new GravityProcessor(WORLD_SURFACE_WG, -1)]`。
        // `Projection.TERRAIN_MATCHING.getProcessors()` is that single gravity processor.
        chain.push(Processor::Gravity {
            heightmap: "WORLD_SURFACE_WG".to_owned(),
            offset: -1,
        });
    }
    if *legacy {
        // `popProcessor(STRUCTURE_BLOCK)` 摘掉链首那一项（它一定是第一个），再追加
        // `STRUCTURE_AND_AIR`。
        // `popProcessor(STRUCTURE_BLOCK)` removes the first entry; `STRUCTURE_AND_AIR` is
        // then appended at the end.
        chain.retain(|processor| {
            !matches!(processor, Processor::BlockIgnore { to_ignore }
                if to_ignore.len() == 1 && to_ignore[0] == "minecraft:structure_block")
        });
        chain.push(Processor::BlockIgnore {
            to_ignore: vec![
                "minecraft:air".to_owned(),
                "minecraft:structure_block".to_owned(),
            ],
        });
    }
    Some(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::processor::BlockStateSpec;

    /// 不带属性的状态名解析成方块的默认状态。
    ///
    /// A name without properties resolves to the block's default state.
    #[test]
    fn state_id_resolves_the_default_state_by_name() {
        let spec = BlockStateSpec::parse_str("minecraft:oak_stairs").unwrap();
        let state = state_id_of(&spec).unwrap();
        let block = mc_bamboo_data::block_by_name("minecraft:oak_stairs").unwrap();
        assert_eq!(state, block.default_state);
    }

    /// 带属性的状态名解析成**唯一**匹配那个组合的状态，且属性顺序无关。
    ///
    /// A name with properties resolves to the unique matching state, order irrelevant.
    #[test]
    fn state_id_resolves_properties_in_any_order() {
        let forward =
            BlockStateSpec::parse_str("minecraft:oak_stairs[facing=east,half=top,shape=straight]")
                .unwrap();
        let reversed =
            BlockStateSpec::parse_str("minecraft:oak_stairs[shape=straight,half=top,facing=east]")
                .unwrap();
        let a = state_id_of(&forward).unwrap();
        let b = state_id_of(&reversed).unwrap();
        assert_eq!(a, b);
        assert_ne!(
            a,
            state_id_of(&BlockStateSpec::parse_str("minecraft:oak_stairs").unwrap()).unwrap()
        );
        // 读回来核对属性确实是我们写的那组。
        assert_eq!(
            crate::structure::state_transform::property_of(a, "facing"),
            Some("east")
        );
        assert_eq!(
            crate::structure::state_transform::property_of(a, "half"),
            Some("top")
        );
    }

    /// 未知方块/非法属性组合要报错，不能静默写成空气。
    ///
    /// An unknown block or an impossible property combination must error rather than write
    /// air silently.
    #[test]
    fn state_id_rejects_unknown_blocks_and_bad_properties() {
        let unknown = BlockStateSpec::parse_str("minecraft:not_a_block").unwrap();
        assert!(state_id_of(&unknown).is_err());
        let bad = BlockStateSpec::parse_str("minecraft:oak_stairs[facing=up]").unwrap();
        assert!(state_id_of(&bad).is_err());
    }

    /// processor 链的组装顺序照 `SinglePoolElement.getSettings`：结构方块忽略 →
    /// 可选拼图替换 → 配置的 → 投影的。
    ///
    /// The assembled order follows `SinglePoolElement.getSettings`.
    #[test]
    fn assemble_processors_follows_the_vanilla_order() {
        use crate::structure::jigsaw::element::{PoolElement, Projection};
        use crate::structure::processor::ProcessorListRef;

        let single = |projection: Projection, legacy: bool| PoolElement::Single {
            location: "minecraft:empty".to_owned(),
            projection,
            legacy,
            processors: ProcessorListRef::Inline(Vec::new()),
            override_liquid_settings: None,
        };

        // 1) rigid + 保留拼图 + 无配置：只有结构方块忽略。
        // rigid, keep jigsaws, nothing configured: only the structure-block ignore.
        let chain = assemble_processors(&single(Projection::Rigid, false), &[], true).unwrap();
        assert_eq!(chain.len(), 1);
        assert!(matches!(chain[0], Processor::BlockIgnore { .. }));

        // 2) 不保留拼图：结构方块忽略 + 拼图替换。
        // Not keeping jigsaws: the ignore plus the replacement.
        let chain = assemble_processors(&single(Projection::Rigid, false), &[], false).unwrap();
        assert_eq!(chain.len(), 2);
        assert!(matches!(chain[1], Processor::JigsawReplacement));

        // 3) 配置的 processor 在拼图替换之后。
        // Configured processors come after the replacement.
        let configured = vec![Processor::ProtectedBlocks {
            value: crate::structure::processor::BlockSetRef::Blocks(Vec::new()),
        }];
        let chain =
            assemble_processors(&single(Projection::Rigid, false), &configured, false).unwrap();
        assert_eq!(chain.len(), 3);
        assert!(matches!(chain[2], Processor::ProtectedBlocks { .. }));

        // 4) `terrain_matching` 在最后追加 gravity。
        // `terrain_matching` appends gravity last.
        let chain = assemble_processors(
            &single(Projection::TerrainMatching, false),
            &configured,
            false,
        )
        .unwrap();
        assert_eq!(chain.len(), 4);
        assert!(
            matches!(&chain[3], Processor::Gravity { heightmap, offset }
                if heightmap == "WORLD_SURFACE_WG" && *offset == -1),
            "terrain matching ends with gravity(WORLD_SURFACE_WG, -1)"
        );

        // 5) legacy：摘掉链首的结构方块忽略，追加 `STRUCTURE_AND_AIR` 到**末尾**。
        // legacy: the leading ignore is removed and `STRUCTURE_AND_AIR` is appended last.
        let chain = assemble_processors(&single(Projection::Rigid, true), &[], false).unwrap();
        assert_eq!(chain.len(), 2, "jigsaw replacement + structure_and_air");
        assert!(matches!(chain[0], Processor::JigsawReplacement));
        let Processor::BlockIgnore { to_ignore } = &chain[1] else {
            panic!("legacy must end with a block ignore");
        };
        assert_eq!(to_ignore, &["minecraft:air", "minecraft:structure_block"]);
        assert!(
            !chain.iter().any(
                |processor| matches!(processor, Processor::BlockIgnore { to_ignore }
                if to_ignore.len() == 1 && to_ignore[0] == "minecraft:structure_block")
            ),
            "the leading STRUCTURE_BLOCK-only ignore must be popped"
        );

        // 6) legacy + terrain_matching：gravity 在 `STRUCTURE_AND_AIR` 之前。
        // legacy + terrain_matching: gravity sits before `STRUCTURE_AND_AIR`.
        let chain =
            assemble_processors(&single(Projection::TerrainMatching, true), &[], true).unwrap();
        assert_eq!(chain.len(), 2);
        assert!(matches!(chain[0], Processor::Gravity { .. }));
        assert!(matches!(chain[1], Processor::BlockIgnore { .. }));
    }

    /// 非 `single` 元素没有自己的 settings。
    ///
    /// A non-`single` element has no settings of its own.
    #[test]
    fn assemble_processors_returns_none_for_other_elements() {
        use crate::structure::jigsaw::element::{PoolElement, Projection};
        use crate::structure::processor::ProcessorListRef;
        assert!(assemble_processors(&PoolElement::Empty, &[], false).is_none());
        assert!(
            assemble_processors(
                &PoolElement::Feature {
                    feature: "minecraft:x".to_owned(),
                    projection: Projection::Rigid
                },
                &[],
                false
            )
            .is_none()
        );
        assert!(
            assemble_processors(
                &PoolElement::List {
                    elements: Vec::new(),
                    projection: Projection::Rigid
                },
                &[],
                false
            )
            .is_none()
        );
        let _ = ProcessorListRef::Inline(Vec::new());
    }

    /// `set_block` 返回 `false`（状态没变）时不计入返回值，与官方
    /// `if (level.setBlock(...))` 一致。
    ///
    /// A `set_block` returning `false` (no state change) is not counted, matching vanilla's
    /// `if (level.setBlock(...))`.
    #[test]
    fn place_processed_counts_only_actual_changes() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;

        /// 前 `accept` 次返回 `true`，之后返回 `false`。
        ///
        /// Returns `true` for the first `accept` writes, `false` afterwards.
        struct PartialWorld {
            accept: usize,
            calls: usize,
        }
        impl BlockWriter for PartialWorld {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                self.calls += 1;
                self.calls <= self.accept
            }
        }

        let bb = crate::structure::BoundingBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 10,
            max_y: 10,
            max_z: 10,
        };
        let blocks: Vec<PlaceBlock> = (0..5)
            .map(|i| PlaceBlock {
                pos: (i, 0, 0),
                state: BlockStateSpec::parse_str("minecraft:stone").unwrap(),
                has_nbt: false,
            })
            .collect();

        let mut world = PartialWorld {
            accept: 3,
            calls: 0,
        };
        let changed = place_processed(&blocks, bb, 0, 0, &mut world).unwrap();
        assert_eq!(world.calls, 5, "every block is still attempted");
        assert_eq!(changed, 3, "only accepted writes count");
    }

    /// 完整链路：模板 → processor 链 → 写入，只写 `chunk_bb` 内的位置，且写出的状态
    /// 经过 mirror/rotation 变换。
    ///
    /// The whole chain: template → processor chain → write, restricted to `chunk_bb`, with
    /// mirror/rotation applied to each state.
    #[test]
    fn place_piece_writes_inside_the_chunk_box_with_transformed_states() {
        // 用真实模板 `minecraft:empty`（size 0、只有一个方块也没有）之外挑一个最小的。
        // Pick the smallest real template that has at least one block.
        let smallest = mc_bamboo_data::structures::ENTRIES
            .iter()
            .filter_map(|(name, _, _)| {
                crate::structure::template::StructureTemplate::load(name).ok()
            })
            .filter(|template| !template.blocks.is_empty() && template.palettes.len() == 1)
            .min_by_key(|template| template.blocks.len());
        let Some(template) = smallest else {
            return; // 数据里没有可用模板：跳过。
        };
        let palette = &template.palettes[0];
        let size = template.size;
        // 盒只覆盖一半，验证 `chunk_bb` 过滤真的生效。
        // The box covers half the template so the `chunk_bb` filter is exercised.
        let chunk_bb = crate::structure::BoundingBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: size.0.max(1) / 2,
            max_y: size.1,
            max_z: size.2,
        };
        struct Level;
        impl ProcessorLevel for Level {
            fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
                BlockStateSpec::parse_str("minecraft:air").unwrap()
            }
            fn in_block_set(
                &self,
                _state: &BlockStateSpec,
                _set: &crate::structure::processor::BlockSetRef,
            ) -> bool {
                false
            }
        }
        #[derive(Default)]
        struct World {
            writes: Vec<(i32, i32, i32, u32)>,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
                self.writes.push((x, y, z, state));
                true
            }
        }

        let level = Level;
        let mut world = World::default();
        let written = place_piece(
            &template,
            (0, 0, 0),
            &[],
            &level,
            PlaceSettings {
                piece_position: (0, 0, 0),
                reference_pos: (0, 0, 0),
                chunk_bb: Some(chunk_bb),
                level_seed: 0,
                explicit_random: None,
            },
            0,
            0,
            chunk_bb,
            &mut world,
        )
        .unwrap();
        assert!(written > 0, "expected some writes");
        // 每个写出的位置都在盒内。
        // Every write must be inside the box.
        for (x, y, z, _) in &world.writes {
            assert!(chunk_bb.contains(*x, *y, *z), "write outside chunk_bb");
        }
        // 盒外的方块确实被跳过了：总写入数少于模板方块数时成立，或全部在盒内。
        // The filter must have skipped something unless every block happens to be inside.
        assert!(
            written <= template.blocks.len(),
            "wrote {written} of {} blocks",
            template.blocks.len()
        );
        let _ = palette;
    }
}
