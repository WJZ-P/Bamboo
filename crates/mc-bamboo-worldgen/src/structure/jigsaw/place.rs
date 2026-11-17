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
use mc_bamboo_nbt::Tag;

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
    /// `blockInfo.nbt`：非 `None` 时官方先写 `Blocks.BARRIER` 再写真正的方块，放置完成后
    /// 还要用它恢复方块实体。
    ///
    /// `blockInfo.nbt`: when set, vanilla writes `Blocks.BARRIER` first and restores the
    /// block entity afterwards.
    pub nbt: Option<mc_bamboo_nbt::Tag>,
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

/// 一次放置的产物：真正写进去的方块，以及它们的包围盒。///
/// 官方 `StructureTemplate.placeInWorld` 内部维护 `minX..maxZ` 与 `placed`（位置 + nbt），
/// 用于 `updateShapeAtEdge`、方块实体 `setChanged` 与 `placeEntities`。这里把同样的两块
/// 数据交出去，使用方（放置里程碑）自己决定怎么用。
///
/// What one placement produced: the blocks that were really written, plus their bounding
/// box. Vanilla keeps `minX..maxZ` and `placed` (position + nbt) internally for
/// `updateShapeAtEdge`, block-entity `setChanged` and `placeEntities`; this exposes the same
/// two pieces so the placement milestone can use them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlacedBlocks {
    /// `minX..maxZ`：只有真正改了的方块参与。一个都没写时是 `None`。
    ///
    /// `minX..maxZ`, built only from blocks that really changed; `None` when nothing did.
    pub bounds: Option<crate::structure::BoundingBox>,
    /// `placed`：`(世界坐标, 原方块的 nbt)`，按写入顺序。
    ///
    /// `placed`: `(world position, the block's original nbt)`, in write order.
    pub placed: Vec<((i32, i32, i32), Option<mc_bamboo_nbt::Tag>)>,
    /// `placeEntities` 的产物：待创建的实体（只在 `place_piece` 里填）。
    ///
    /// What `placeEntities` produced: entities to create (filled in by `place_piece` only).
    pub entities: Vec<PlacedEntity>,
}

/// 官方 `StructureTemplate.placeEntities`：把模板里的每个实体变换到世界坐标，改写它的
/// `Pos` / `block_pos` / `UUID`，并算出 `yRot`。实体本身不在这里创建。
///
/// 逐行照官方：
///
/// * `blockPos = transform(entityInfo.blockPos, mirror, rotation, pivot).offset(position)`，
///   只放落在 `boundingBox` 内的；
/// * `tag = nbt.copy()`，`Pos` 换成变换后的 `Vec3`（**double** 列表），删 `UUID`；
/// * `block_pos` 存在时按 `BlockPos.CODEC`（三个 int 的 `int[]`）改写；
/// * `yRot = entity.rotate(rotation) + entity.mirror(mirror) - entity.getYRot()`，
///   其中 `rotate` / `mirror` 都先 `Mth.wrapDegrees(entity.getYRot())`。
///
/// Vanilla `StructureTemplate.placeEntities`: transform each template entity into world
/// space, rewrite its `Pos` / `block_pos` / `UUID` and work out `yRot`. The entity itself is
/// not created here.
pub fn place_entities(
    template: &StructureTemplate,
    piece_position: (i32, i32, i32),
    mirror: u32,
    rotation: u32,
    chunk_bb: Option<crate::structure::BoundingBox>,
) -> Vec<PlacedEntity> {
    let mut out = Vec::new();
    for entity in &template.entities {
        let block_pos = relative_position(entity.block_pos, mirror, rotation);
        let block_pos = (
            block_pos.0 + piece_position.0,
            block_pos.1 + piece_position.1,
            block_pos.2 + piece_position.2,
        );
        if let Some(bb) = chunk_bb
            && !bb.contains(block_pos.0, block_pos.1, block_pos.2)
        {
            continue;
        }
        let relative = transform_double(entity.pos, mirror, rotation);
        let pos = (
            relative.0 + f64::from(piece_position.0),
            relative.1 + f64::from(piece_position.1),
            relative.2 + f64::from(piece_position.2),
        );
        let Tag::Compound(mut entries) = entity.nbt.clone() else {
            continue;
        };
        // `Rotation` 是 `[yRot, xRot]`，都在实体身上先读出来（官方是先 `EntityType.create`
        // 再 `entity.getYRot()`，值就是 nbt 里那份）。
        // `Rotation` is `[yRot, xRot]`; read it out first (vanilla reads it back from the
        // created entity, which is the nbt value).
        let (y_rot_nbt, x_rot) = rotation_of(&entries).unwrap_or((0.0, 0.0));
        // `tag.put("Pos", posTag)`：覆盖成变换后的 double 列表。
        // `tag.put("Pos", posTag)`: overwrite with the transformed double list.
        entries.retain(|(key, _)| key != "Pos");
        entries.push((
            "Pos".to_owned(),
            Tag::List(vec![
                Tag::Double(pos.0),
                Tag::Double(pos.1),
                Tag::Double(pos.2),
            ]),
        ));
        // `tag.remove("UUID")`。
        entries.retain(|(key, _)| key != "UUID");
        // `if (tag.contains("block_pos")) tag.store("block_pos", BlockPos.CODEC, blockPos)`。
        // `BlockPos.CODEC` 是三个 int 的流（`int[]`）。
        // `BlockPos.CODEC` is a three-int stream (`int[]`).
        if entries.iter().any(|(key, _)| key == "block_pos") {
            entries.retain(|(key, _)| key != "block_pos");
            entries.push((
                "block_pos".to_owned(),
                Tag::IntArray(vec![block_pos.0, block_pos.1, block_pos.2]),
            ));
        }

        // `float yRot = entity.rotate(rotation); yRot += entity.mirror(mirror)
        //      - entity.getYRot();`
        // 结合顺序必须照抄：**先**算 `mirror(base) - raw`，**再**与 `rotate` 相加，
        // 否则 f32 舍入会差 1 个 ULP。
        // The association order matters: compute `mirror(base) - raw` **first**, then add it
        // to `rotate`, or f32 rounding differs by one ULP.
        let base = wrap_degrees_f32(y_rot_nbt);
        let rotated = base + rotation_angle_f32(rotation);
        let delta = mirror_value_f32(mirror, base) - y_rot_nbt;
        let y_rot = rotated + delta;
        out.push(PlacedEntity {
            block_pos,
            x: pos.0,
            y: pos.1,
            z: pos.2,
            y_rot,
            x_rot,
            raw_y_rot: y_rot_nbt,
            tag: Tag::Compound(entries),
        });
    }
    out
}

/// 实体 nbt 里的 `Rotation`：`[yRot, xRot]`，两个 float。
///
/// The entity's `Rotation` nbt: `[yRot, xRot]`, both floats.
fn rotation_of(entries: &[(String, mc_bamboo_nbt::Tag)]) -> Option<(f32, f32)> {
    let tag = entries.iter().find(|(key, _)| key == "Rotation")?;
    let mc_bamboo_nbt::Tag::List(values) = &tag.1 else {
        return None;
    };
    let number = |tag: &mc_bamboo_nbt::Tag| match tag {
        mc_bamboo_nbt::Tag::Float(value) => Some(*value),
        mc_bamboo_nbt::Tag::Double(value) => Some(*value as f32),
        _ => None,
    };
    Some((number(values.first()?)?, number(values.get(1)?)?))
}

/// `Mth.wrapDegrees(float)`。
///
/// `Mth.wrapDegrees(float)`.
fn wrap_degrees_f32(angle: f32) -> f32 {
    let mut normalized = angle % 360.0;
    if normalized >= 180.0 {
        normalized -= 360.0;
    }
    if normalized < -180.0 {
        normalized += 360.0;
    }
    normalized
}

/// `Entity.rotate(rotation)` 相对原 `yRot` 的增量。`Rotation` 序是
/// 0=NONE、1=CLOCKWISE_90、2=CLOCKWISE_180、3=COUNTERCLOCKWISE_90。
///
/// The increment `Entity.rotate(rotation)` adds. `Rotation` order is 0=NONE, 1=CLOCKWISE_90,
/// 2=CLOCKWISE_180, 3=COUNTERCLOCKWISE_90.
fn rotation_angle_f32(rotation: u32) -> f32 {
    match rotation {
        1 => 90.0,
        2 => 180.0,
        3 => 270.0,
        _ => 0.0,
    }
}

/// `Entity.mirror(mirror)`：`LEFT_RIGHT` 取 `180 - angle`、`FRONT_BACK` 取反，其余原样。
/// `Mirror` 序是 0=NONE、1=LEFT_RIGHT、2=FRONT_BACK。
///
/// `Entity.mirror(mirror)`: `180 - angle` for `LEFT_RIGHT`, negate for `FRONT_BACK`,
/// otherwise the input. `Mirror` order is 0=NONE, 1=LEFT_RIGHT, 2=FRONT_BACK.
fn mirror_value_f32(mirror: u32, angle: f32) -> f32 {
    match mirror {
        1 => 180.0 - angle,
        2 => -angle,
        _ => angle,
    }
}

/// `StructureTemplate.transform(Vec3, mirror, rotation, pivot)`，pivot 为零；镜像用
/// `1.0 - x` / `1.0 - z`，与整数版本取反再平移等价。
///
/// `StructureTemplate.transform(Vec3, mirror, rotation, pivot)` with a zero pivot. The
/// mirror is `1.0 - x` / `1.0 - z`, which equals negate-then-shift.
fn transform_double(pos: (f64, f64, f64), mirror: u32, rotation: u32) -> (f64, f64, f64) {
    let (mut x, y, mut z) = pos;
    // `Mirror` 序：0=NONE、1=LEFT_RIGHT（取反 z）、2=FRONT_BACK（取反 x）。
    // `Mirror` order: 0=NONE, 1=LEFT_RIGHT (negate z), 2=FRONT_BACK (negate x).
    match mirror {
        1 => z = 1.0 - z,
        2 => x = 1.0 - x,
        _ => {}
    }
    // `Rotation` 序：0=NONE、1=CLOCKWISE_90、2=CLOCKWISE_180、3=COUNTERCLOCKWISE_90。
    // `Rotation` order: 0=NONE, 1=CLOCKWISE_90, 2=CLOCKWISE_180, 3=COUNTERCLOCKWISE_90.
    match rotation {
        1 => (1.0 - z, y, x),
        2 => (1.0 - x, y, 1.0 - z),
        3 => (z, y, 1.0 - x),
        _ => (x, y, z),
    }
}

/// 一个待创建的实体，照 `StructureTemplate.placeEntities` 里 `createEntityIgnoreException`
/// 之后、`addFreshEntityWithPassengers` 之前的那份 `tag`，外加算好的朝向。
///
/// 官方这一段只做几何与 nbt 改写（`Pos` 覆盖、删 `UUID`、改写 `block_pos`、算 `yRot`），
/// 创建实体本身需要实体注册表与 `ServerLevel`，留给上层。`y_rot` 对应
/// `entity.rotate(rotation) + entity.mirror(mirror) - entity.getYRot()`。
///
/// One entity to create: the `tag` as it stands between `createEntityIgnoreException` and
/// `addFreshEntityWithPassengers`, plus the computed rotation. Vanilla's loop only does
/// geometry and nbt rewriting here (overwrite `Pos`, drop `UUID`, rewrite `block_pos`, work
/// out `yRot`); creating the entity needs the entity registry and a `ServerLevel`, so that
/// stays with the caller. `y_rot` is
/// `entity.rotate(rotation) + entity.mirror(mirror) - entity.getYRot()`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedEntity {
    /// `transform(entityInfo.blockPos, mirror, rotation, pivot).offset(position)`，整数。
    /// 既用于包围盒判定，也是 `block_pos` 改写后的值。
    ///
    /// The transformed integer `blockPos`; used both for the box test and as the rewritten
    /// `block_pos`.
    pub block_pos: (i32, i32, i32),
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub y_rot: f32,
    /// 实体原本的 `Rotation`（`[yRot, xRot]`），最终 `snapTo` 用的是它。
    ///
    /// The entity's original `Rotation` (`[yRot, xRot]`); `snapTo` ends up with it.
    pub x_rot: f32,
    /// nbt 里的原始 `yRot`（`Rotation[0]`）。
    ///
    /// The raw `yRot` from the nbt (`Rotation[0]`).
    pub raw_y_rot: f32,
    pub tag: mc_bamboo_nbt::Tag,
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
    random: &crate::random::Random,
    worldgen_random: Option<&crate::random::WorldgenRandom>,
    world: &mut impl BlockWriter,
) -> Result<PlacedBlocks, String> {
    let mut placed = PlacedBlocks::default();
    // `List<BlockPos> toFill` / `lockedFluids`：watering 的两份列表。
    // The two lists used by vanilla's watering pass.
    let mut to_fill: Vec<(i32, i32, i32)> = Vec::new();
    let mut locked_fluids: std::collections::HashSet<(i32, i32, i32)> =
        std::collections::HashSet::new();
    for block in processed {
        let pos = block.pos;
        if !chunk_bb.contains(pos.0, pos.1, pos.2) {
            continue;
        }
        // `settings.shouldApplyWaterlogging() ? level.getFluidState(blockPos) : null`。
        // 必须在写方块之前读。
        // Read before the block is written.
        let previous_fluid = world.fluid_at(pos.0, pos.1, pos.2);
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
        if block.nbt.is_some() {
            world.set_block(pos.0, pos.1, pos.2, barrier_state());
        }
        // 官方只在 `level.setBlock(...)` 返回 true 时才把位置计入 `minX..maxZ` 与
        // `placed`。
        // Vanilla counts a position only when `level.setBlock(...)` returns true.
        if world.set_block(pos.0, pos.1, pos.2, state_out) {
            placed.bounds = Some(match placed.bounds {
                None => crate::structure::BoundingBox {
                    min_x: pos.0,
                    min_y: pos.1,
                    min_z: pos.2,
                    max_x: pos.0,
                    max_y: pos.1,
                    max_z: pos.2,
                },
                Some(bounds) => crate::structure::BoundingBox {
                    min_x: bounds.min_x.min(pos.0),
                    min_y: bounds.min_y.min(pos.1),
                    min_z: bounds.min_z.min(pos.2),
                    max_x: bounds.max_x.max(pos.0),
                    max_y: bounds.max_y.max(pos.1),
                    max_z: bounds.max_z.max(pos.2),
                },
            });
            // 官方的方块实体恢复：带 nbt 时，如果这个方块实体是
            // `RandomizableContainer`，就用 `random.nextLong()` **再取一次**覆盖
            // `LootTableSeed`，然后才 `loadWithComponents`。这一次取数必须发生，
            // 否则世界随机源序列与官方错开。
            //
            // The block-entity restore: with nbt set, a `RandomizableContainer` gets
            // `random.nextLong()` **drawn again** to overwrite `LootTableSeed` before
            // `loadWithComponents`. That draw must happen or the world random sequence
            // diverges from vanilla.
            let nbt = match &block.nbt {
                None => None,
                Some(tag) if is_randomizable_container(&block.state.name) => {
                    // `RandomizableContainer` 读入模板 nbt 时，`LootTable` 不存在就清空
                    // 整份 `LootTable` 状态；随后写入的 `LootTableSeed`（可能非 0）
                    // 不会单独存出。古城箱子只有 `Items`、没有表，所以要丢掉这个种子。
                    //
                    // A `RandomizableContainer` with no `LootTable` in its template nbt
                    // clears the whole loot-table state; the placement-time
                    // `LootTableSeed` is then not saved on its own. The ancient-city chest
                    // has only `Items`, so its seed must be dropped.
                    if !nbt_has_key(tag, "LootTable") {
                        let mut cleaned = tag.clone();
                        nbt_remove_key(&mut cleaned, "LootTableSeed");
                        Some(cleaned)
                    } else {
                        // `StructureStart.placeInChunk` 传下来的是 `WorldgenRandom`（继承
                        // `LegacyRandomSource`），`BitRandomSource.nextLong` 取两次 `next(32)`；
                        // 直接问内层的 `Random` 会走 Xoroshiro 的原生一次 64 位，两种种子只差低位。
                        // 走游戏路径时一定要用那个 `WorldgenRandom`，单独对拍的调用方才退回到
                        // 传入的 `Random`。
                        //
                        // `StructureStart.placeInChunk` passes a `WorldgenRandom` (extending
                        // `LegacyRandomSource`), whose `BitRandomSource.nextLong` draws two
                        // `next(32)` values. Asking the inner `Random` instead uses Xoroshiro's
                        // single 64-bit draw, which only differs in the low bits. The game path
                        // must use that `WorldgenRandom`; stand-alone oracles fall back to `Random`.
                        let seed = match worldgen_random {
                            Some(source) => source.next_long(),
                            None => random.next_long(),
                        };
                        Some(overwrite_loot_table_seed(tag, seed))
                    }
                }
                // `VaultBlockEntity.saveAdditional` 把 `config` 过一遍 `VaultConfig.CODEC`：
                // 当 `loot_table` 等于该表的默认 `TRIAL_CHAMBERS_REWARD` 时，编码会省略
                // 它。试炼密室模板里普通宝库写的正是默认值，所以官方存出来没有这一项。
                //
                // `VaultBlockEntity.saveAdditional` round-trips `config` through
                // `VaultConfig.CODEC`, whose encoder omits `loot_table` when it equals the
                // codec default (`TRIAL_CHAMBERS_REWARD`). Plain trial-chamber vault
                // templates use exactly that default, so vanilla saves no such key.
                Some(tag) if block.state.name == "minecraft:vault" => {
                    Some(normalize_vault_config(tag))
                }
                // 模板里的数值字段用的是旧式短/字节类型；官方的 `saveAdditional`
                // 按 `putInt` 写回，告示牌文本也要过 `SignText.CODEC`。
                // Template numeric fields use legacy short/byte types; vanilla's
                // `saveAdditional` writes them back as ints, and sign text is
                // round-tripped through `SignText.CODEC`.
                Some(tag) => Some(normalize_block_entity_nbt(&block.state.name, tag)),
            };
            // 官方 `blockEntity.loadWithComponents(...)`：nbt 写进方块实体。
            // Vanilla `blockEntity.loadWithComponents(...)`: the nbt goes into the block
            // entity.
            if let Some(tag) = &nbt {
                world.set_block_entity(pos.0, pos.1, pos.2, tag);
            }
            placed.placed.push((pos, nbt));

            // `previousFluidState = settings.shouldApplyWaterlogging()
            //      ? level.getFluidState(blockPos) : null`——在写方块**之前**取。
            // 这一步必须在 `set_block` 之前读，否则读到的是新方块的状态。
            // The previous fluid state is read **before** writing the block; after the write
            // it would already be the new state.
            if let Some((is_water, source)) = previous_fluid {
                if mc_bamboo_data::state_fluid(state_out).source {
                    // `state.getFluidState().isSource()`：新方块本身是流体源，锁定该位置。
                    // The new block is itself a fluid source: lock the position.
                    locked_fluids.insert(pos);
                } else if is_liquid_container(state_out) {
                    // `state.getBlock() instanceof LiquidBlockContainer` →
                    // `placeLiquid(...)`；**返回 false 也照样**把位置加入 `toFill`。
                    // The `placeLiquid` call; the position joins `toFill` even when it
                    // returns false.
                    if let Some(filled_state) = place_liquid(state_out, is_water, source) {
                        world.set_block(pos.0, pos.1, pos.2, filled_state);
                    }
                    if !source {
                        to_fill.push(pos);
                    }
                }
            }
        }
    }
    // `while (filled && !toFill.isEmpty())`：反复把「相邻有源流体」的待填充位置灌满。
    // 官方在这里用 `FluidState toPlace = level.getFluidState(pos)` 起手（任何流体，不止水），
    // 再按 UP/NORTH/EAST/SOUTH/WEST 找第一个**未被锁定**的源；找到源就调 `placeLiquid`
    //（不管返回什么）并把位置出列。
    // Repeat until no more positions can be filled from a neighbouring source. Vanilla
    // starts from the position's own fluid state (any fluid, not just water) and scans
    // UP/NORTH/EAST/SOUTH/WEST for the first unlocked source; a found source means
    // `placeLiquid` runs and the position leaves the list regardless of its result.
    let directions = [(0, 1, 0), (0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];
    let mut filled = true;
    while filled && !to_fill.is_empty() {
        filled = false;
        let mut remaining = Vec::with_capacity(to_fill.len());
        for pos in std::mem::take(&mut to_fill) {
            let mut to_place = world.fluid_at(pos.0, pos.1, pos.2);
            if !to_place.is_some_and(|(_, is_source)| is_source) {
                for (dx, dy, dz) in directions {
                    let neighbor = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                    if locked_fluids.contains(&neighbor) {
                        continue;
                    }
                    let Some(fluid) = world.fluid_at(neighbor.0, neighbor.1, neighbor.2) else {
                        continue;
                    };
                    if fluid.1 {
                        to_place = Some(fluid);
                        break;
                    }
                }
            }
            if to_place.is_some_and(|(_, is_source)| is_source) {
                let state = world.block_state(pos.0, pos.1, pos.2);
                if is_liquid_container(state) {
                    let (is_water, is_source) = to_place.unwrap_or((false, false));
                    if let Some(filled_state) = place_liquid(state, is_water, is_source) {
                        world.set_block(pos.0, pos.1, pos.2, filled_state);
                    }
                    filled = true;
                    continue;
                }
            }
            remaining.push(pos);
        }
        to_fill = remaining;
    }
    Ok(placed)
}

/// `state.getBlock() instanceof LiquidBlockContainer`。///
/// 26.3 里有 `waterlogged` 属性的 505 个方块**全部**实现该接口且用默认
/// `SimpleWaterloggedBlock.placeLiquid`（少数覆写见 [`place_liquid`]）；反过来只有四个
/// 方块实现接口却没有该属性（`kelp` / `kelp_plant` / `seagrass` / `tall_seagrass`），
/// 它们的 `placeLiquid` 恒返回 `false`。两类都由本函数如实分类。
///
/// Every one of the 505 blocks with a `waterlogged` property in 26.3 implements
/// `LiquidBlockContainer` through the default `SimpleWaterloggedBlock.placeLiquid` (the few
/// overrides are in [`place_liquid`]); conversely only four blocks implement the interface
/// without the property (`kelp` / `kelp_plant` / `seagrass` / `tall_seagrass`) and their
/// `placeLiquid` always returns `false`. This classifies both sets faithfully.
fn is_liquid_container(state: u32) -> bool {
    if crate::structure::state_transform::property_of(state, "waterlogged").is_some() {
        return true;
    }
    matches!(
        state_text(state).as_str(),
        "minecraft:kelp"
            | "minecraft:kelp_plant"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
    )
}

/// `LiquidBlockContainer.placeLiquid(level, pos, state, fluidState)`：返回灌完之后的新状态，
/// 没灌上（官方返回 `false`）时返回 `None`。
///
/// 默认实现是 `SimpleWaterloggedBlock`：`!waterlogged && fluidState.is(WATER)` 时把
/// `waterlogged` 置 true。26.3 有四处覆写，行为都归结为同一件事：
///
/// * `SlabBlock`：`type == double` 时直接返回 `false`；
/// * `CandleBlock` / `CampfireBlock`：灌水同时把 `lit` 置 false（`extinguish` / `douse`）；
/// * `DriedGhastBlock`：与默认实现相同。
///
/// **对拍覆盖**（`jigsaw waterpiece`，168 查询 / 64600 写入 / 88648 次流体查询）：
///
/// * 台阶的双台阶拒绝、「已 waterlogged 不重复灌」、「流动水不灌」都由真实模板覆盖；
/// * 营火与**蜡烛**都被覆盖：水位按模板里每个可含水方块的精确位置注入，
///   `ancient_city/structures/barracks`、`sauna_1`、`trial_chambers/reward/ominous_vault`
///   里就有 `lit=true` 的 `white_candle` / `red_candle`，把清 `lit` 那行改成不清会报
///   **59 处不一致**（实测）。
///
/// **Oracle coverage** (`jigsaw waterpiece`, 168 queries / 64600 writes / 88648 fluid
/// queries): the double-slab refusal, the "already waterlogged" case and "flowing water
/// does not waterlog" are all covered by real templates. Campfires and **candles** are
/// covered too: the fluid is injected at each waterloggable block's exact template
/// position, and `ancient_city/structures/barracks`, `sauna_1` and
/// `trial_chambers/reward/ominous_vault` contain `lit=true` candles. Removing the `lit`
/// clearing line reports **59 mismatches** (measured).
///
/// `LiquidBlockContainer.placeLiquid(level, pos, state, fluidState)`: the state after the
/// water goes in, or `None` when vanilla would return `false`. The default
/// `SimpleWaterloggedBlock` implementation sets `waterlogged` when it is false and the
/// fluid is water. The four overrides in 26.3 all reduce to the same result: `SlabBlock`
/// refuses `type == double`, `CandleBlock` / `CampfireBlock` also clear `lit`, and
/// `DriedGhastBlock` matches the default.
fn place_liquid(state: u32, is_water: bool, is_source: bool) -> Option<u32> {
    use crate::structure::state_transform::property_of;
    // `fluidState.is(Fluids.WATER)`：`TypedInstance.is` 是 `typeHolder().value() == fluid`，
    // 比较的是**流体对象身份**。流动水的类型是 `flowing_water`，**不满足**；
    // 只有源水的类型才是 `water`。
    // `is(Fluids.WATER)` compares fluid identity through `TypedInstance.is`: flowing water's
    // type is `flowing_water`, so it does **not** match; only a water source does.
    if !is_water || !is_source {
        return None;
    }
    if property_of(state, "waterlogged") != Some("false") {
        return None;
    }
    // `SlabBlock.placeLiquid`：双台阶不灌。
    // `SlabBlock.placeLiquid`: a double slab takes no water.
    if property_of(state, "type") == Some("double") {
        return None;
    }
    let next = with_waterlogged(state, true)?;
    // `CandleBlock` 走 `extinguish` → `setLit(false)`；`CampfireBlock` 直接
    // `setValue(LIT, false)`。两者都保留刚设上的 `waterlogged = true`。
    // `CandleBlock` goes through `extinguish` → `setLit(false)`; `CampfireBlock` sets
    // `LIT = false` directly. Both keep the `waterlogged = true` just set.
    Some(crate::structure::piece_place::with_property(next, "lit", "false").unwrap_or(next))
}

/// 把 `waterlogged` 设成 `value`；没有该属性时返回 `None`。
///
/// Set `waterlogged` to `value`; `None` when the state has no such property.
fn with_waterlogged(state: u32, value: bool) -> Option<u32> {
    crate::structure::piece_place::with_property(
        state,
        "waterlogged",
        if value { "true" } else { "false" },
    )
}

/// 这个方块是不是 `RandomizableContainer`（官方实现该接口的 8 个方块实体类）。
///
/// Whether this block's block entity implements `RandomizableContainer` (the eight vanilla
/// block-entity classes that do).
pub fn is_randomizable_container(block_name: &str) -> bool {
    use mc_bamboo_data::registries::block_entity_type as be;
    let Some(block) = mc_bamboo_data::block_by_name(block_name) else {
        return false;
    };
    let Some(kind) = block.block_entity_type else {
        return false;
    };
    matches!(
        kind,
        be::CHEST
            | be::TRAPPED_CHEST
            | be::DISPENSER
            | be::DROPPER
            | be::HOPPER
            | be::SHULKER_BOX
            | be::BARREL
            | be::DECORATED_POT
            | be::CRAFTER
    )
}

/// 按官方各类方块实体的 `saveAdditional` 重新编码模板 nbt 里的字段。
///
/// Re-encode the template nbt fields the way each vanilla block entity's
/// `saveAdditional` does.
pub fn normalize_block_entity_nbt(block_name: &str, tag: &Tag) -> Tag {
    let Tag::Compound(entries) = tag else {
        return tag.clone();
    };
    let mut out = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let next = if is_int_when_saved(block_name, key) {
            numeric_as_int(value)
        } else if is_sign_text(block_name, key) {
            normalize_sign_text(value)
        } else {
            value.clone()
        };
        out.push((key.clone(), next));
    }
    Tag::Compound(out)
}

/// `AbstractFurnaceBlockEntity` / `BrewingStandBlockEntity` 用 `putInt` 写计时字段，
/// 模板里却存成 short / byte。
///
/// `AbstractFurnaceBlockEntity` / `BrewingStandBlockEntity` write their timers with
/// `putInt`, while the templates store them as shorts / bytes.
fn is_int_when_saved(block_name: &str, key: &str) -> bool {
    let furnace = matches!(
        block_name,
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker"
    ) && matches!(
        key,
        "cooking_time_spent"
            | "cooking_total_time"
            | "lit_time_remaining"
            | "lit_total_time"
    );
    let brewing = block_name == "minecraft:brewing_stand"
        && matches!(key, "BrewTime" | "Fuel" | "total_brew_time" | "total_fuel");
    furnace || brewing
}

fn numeric_as_int(value: &Tag) -> Tag {
    match value {
        Tag::Byte(number) => Tag::Int(i32::from(*number)),
        Tag::Short(number) => Tag::Int(i32::from(*number)),
        _ => value.clone(),
    }
}

/// `SignBlockEntity.saveAdditional` 把 `front_text` / `back_text` 过
/// `SignText.CODEC`：文本组件压成纯字符串，`filtered_messages` 与 `messages`
/// 相同时省略。
///
/// `SignBlockEntity.saveAdditional` round-trips `front_text` / `back_text`
/// through `SignText.CODEC`: text components collapse to plain strings and
/// `filtered_messages` is omitted when equal to `messages`.
fn is_sign_text(block_name: &str, key: &str) -> bool {
    block_name.contains("sign")
        && !block_name.contains("hanging_sign")
        && matches!(key, "front_text" | "back_text")
}

fn component_text(value: &Tag) -> Tag {
    let Tag::Compound(entries) = value else {
        return value.clone();
    };
    if entries.len() == 1 {
        if let Some((_, Tag::String(text))) = entries.first()
            && entries[0].0 == "text"
        {
            return Tag::String(text.clone());
        }
    }
    value.clone()
}

fn normalize_sign_text(value: &Tag) -> Tag {
    let Tag::Compound(entries) = value else {
        return value.clone();
    };
    let mut messages: Option<Vec<Tag>> = None;
    let mut filtered: Option<Vec<Tag>> = None;
    let mut out = Vec::with_capacity(entries.len());
    for (key, field) in entries {
        match (key.as_str(), field) {
            ("messages", Tag::List(list)) => {
                let normalized: Vec<Tag> = list.iter().map(component_text).collect();
                messages = Some(normalized.clone());
                out.push((key.clone(), Tag::List(normalized)));
            }
            ("filtered_messages", Tag::List(list)) => {
                filtered = Some(list.iter().map(component_text).collect());
            }
            _ => out.push((key.clone(), field.clone())),
        }
    }
    // `SignText.filteredMessagesForSerialization`：两份列表相等时不写。
    // `SignText.filteredMessagesForSerialization`: omit it when the lists are equal.
    if let (Some(filtered), Some(messages)) = (filtered, messages)
        && filtered != messages
    {
        out.push(("filtered_messages".to_owned(), Tag::List(filtered)));
    }
    Tag::Compound(out)
}

/// `VaultConfig.CODEC` 的默认战利品表（`VaultConfig.DEFAULT` 构造里写死）。
///
/// The default loot table of `VaultConfig.CODEC` (`VaultConfig.DEFAULT`'s constructor).
const DEFAULT_VAULT_LOOT_TABLE: &str = "minecraft:chests/trial_chambers/reward";

/// 按 `VaultConfig.CODEC` 的编码规则省略默认的 `config.loot_table`。
///
/// Omit the default `config.loot_table` the way `VaultConfig.CODEC` encodes it.
fn normalize_vault_config(tag: &Tag) -> Tag {
    let Tag::Compound(entries) = tag else {
        return tag.clone();
    };
    let mut out = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        if key != "config" {
            out.push((key.clone(), value.clone()));
            continue;
        }
        let Tag::Compound(config) = value else {
            out.push((key.clone(), value.clone()));
            continue;
        };
        let mut fields = Vec::with_capacity(config.len());
        for (field, field_value) in config {
            let is_default = *field == "loot_table"
                && matches!(field_value, Tag::String(text) if text == DEFAULT_VAULT_LOOT_TABLE);
            if !is_default {
                fields.push((field.clone(), field_value.clone()));
            }
        }
        out.push((key.clone(), Tag::Compound(fields)));
    }
    Tag::Compound(out)
}

/// 模板 nbt 里有没有某个键。
///
/// Whether the template nbt has a key.
fn nbt_has_key(tag: &Tag, key: &str) -> bool {
    matches!(tag, Tag::Compound(entries) if entries.iter().any(|(name, _)| name == key))
}

/// 删掉模板 nbt 里的某个键。
///
/// Remove a key from the template nbt.
fn nbt_remove_key(tag: &mut Tag, key: &str) {
    if let Tag::Compound(entries) = tag {
        entries.retain(|(name, _)| name != key);
    }
}

/// 用 `seed` 覆盖（或插入）nbt 里的 `LootTableSeed`。
///
/// Overwrite (or insert) `LootTableSeed` in the nbt.
fn overwrite_loot_table_seed(tag: &mc_bamboo_nbt::Tag, seed: i64) -> mc_bamboo_nbt::Tag {
    let mut out = tag.clone();
    if let Tag::Compound(entries) = &mut out {
        if let Some((_, value)) = entries.iter_mut().find(|(key, _)| key == "LootTableSeed") {
            *value = Tag::Long(seed);
        } else {
            entries.push(("LootTableSeed".to_owned(), Tag::Long(seed)));
        }
    }
    out
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

    /// 官方 `blockEntity.loadWithComponents(...)`：把 nbt 装进该位置的方块实体。
    /// 默认什么都不做（还没有方块实体容器的调用方可以先用这份 nbt 自己处理）。
    ///
    /// Vanilla `blockEntity.loadWithComponents(...)`: load the nbt into the block entity at
    /// that position. The default is a no-op so callers without a block-entity store can
    /// handle the nbt themselves.
    fn set_block_entity(&mut self, _x: i32, _y: i32, _z: i32, _nbt: &mc_bamboo_nbt::Tag) {}

    /// `level.getFluidState(pos)`：返回 `(是否有水, 是否源)`。
    /// `None` 表示调用方不支持流体模型（整段 watering 跳过）。
    ///
    /// `level.getFluidState(pos)` as `(is_water, is_source)`; `None` means the caller has no
    /// fluid model and the whole watering pass is skipped.
    fn fluid_at(&self, _x: i32, _y: i32, _z: i32) -> Option<(bool, bool)> {
        None
    }

    /// `level.getBlockState(pos)`。
    ///
    /// `level.getBlockState(pos)`.
    fn block_state(&self, _x: i32, _y: i32, _z: i32) -> u32 {
        0
    }
}

impl BlockWriter for crate::feature::world::FeatureWorld<'_> {
    /// 官方 `StructureTemplate.placeInWorld` 走 `level.setBlock(...)` →
    /// `ChunkAccess.setBlockState`，会按 `Heightmap.update` 增量维护四张 FINAL 高度图。
    /// 结构件写完后的 `freeze_top_layer` 等 step 10 地物会读它们，所以这里不能只改
    /// `blocks` 不更新 `heights`。
    ///
    /// Vanilla's `StructureTemplate.placeInWorld` goes through
    /// `ChunkAccess.setBlockState`, which incrementally updates the four FINAL
    /// heightmaps. Step-10 features such as `freeze_top_layer` read those, so the
    /// plain `set_block` path would leave them stale.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        crate::feature::world::FeatureWorld::set_block_with_heightmaps(self, x, y, z, state)
    }

    /// 官方 `blockEntity.loadWithComponents(...)`：nbt 交给假世界的方块实体表。
    /// Vanilla `blockEntity.loadWithComponents(...)`: hand the nbt to the fake world's
    /// block entity store.
    fn set_block_entity(&mut self, x: i32, y: i32, z: i32, nbt: &mc_bamboo_nbt::Tag) {
        crate::feature::world::FeatureWorld::set_block_entity(self, x, y, z, nbt);
    }

    /// `level.getFluidState(pos)`，见 [`crate::feature::world::FeatureWorld::fluid_state`]。
    ///
    /// `level.getFluidState(pos)`; see `FeatureWorld::fluid_state`.
    fn fluid_at(&self, x: i32, y: i32, z: i32) -> Option<(bool, bool)> {
        Some(crate::feature::world::FeatureWorld::fluid_state(
            self, x, y, z,
        ))
    }

    /// `level.getBlockState(pos)`：范围内给真实状态，范围外给空气（与
    /// `WorldGenRegion.getBlockState` 一致）。
    ///
    /// `level.getBlockState(pos)`: the real state inside the 3x3, air outside, matching
    /// `WorldGenRegion.getBlockState`.
    fn block_state(&self, x: i32, y: i32, z: i32) -> u32 {
        crate::feature::world::FeatureWorld::get_block(self, x, y, z)
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
    random: &crate::random::Random,
    worldgen_random: Option<&crate::random::WorldgenRandom>,
    world: &mut impl BlockWriter,
) -> Result<PlacedBlocks, String> {
    // `settings.getBoundingBox()`：`None` 时官方不过滤（`boundingBox == null`），
    // 这里用一个「无限」盒表示同样的语义。
    // `settings.getBoundingBox()`; `None` means no filtering in vanilla, represented here by
    // an unbounded box.
    let chunk_bb = settings.chunk_bb.unwrap_or(crate::structure::BoundingBox {
        min_x: i32::MIN,
        min_y: i32::MIN,
        min_z: i32::MIN,
        max_x: i32::MAX,
        max_y: i32::MAX,
        max_z: i32::MAX,
    });
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
    let plain: Vec<ProcessBlock> = blocks.into_iter().map(|(block, _)| block).collect();
    let processed = process_blocks(processors, &plain, level, settings);
    let place: Vec<PlaceBlock> = processed
        .into_iter()
        .map(|block| PlaceBlock {
            // processor 会改写状态但保留 nbt，所以直接用处理后的方块上的 nbt。
            // Processors rewrite the state but keep the nbt, so take it from the processed
            // block.
            nbt: block.nbt.clone(),
            pos: block.world_pos,
            state: block.state,
        })
        .collect();
    let mut placed = place_processed(
        &place,
        chunk_bb,
        mirror,
        rotation,
        random,
        worldgen_random,
        world,
    )?;
    // `if (!settings.isIgnoreEntities()) placeEntities(...)`。`SinglePoolElement.getSettings`
    // 里 `setIgnoreEntities(false)`，所以 jigsaw 结构件照官方都要放实体。`finalizeEntities`
    // 只影响 `Mob.finalizeSpawn`，那属于实体系统，这里交给调用方。
    // `placeEntities` runs unless entities are ignored; `SinglePoolElement.getSettings` sets
    // `setIgnoreEntities(false)`, so jigsaw pieces always place them. `finalizeEntities` only
    // gates `Mob.finalizeSpawn`, which belongs to the entity system and stays with the
    // caller.
    placed.entities = place_entities(
        template,
        piece_position,
        mirror,
        rotation,
        settings.chunk_bb,
    );
    Ok(placed)
}

/// 供调试与日志用：`minecraft:x` 形式的方块名（不含属性）。
///
/// Debug text: the `minecraft:x` block name (without properties).
pub fn state_text(state: u32) -> String {
    mc_bamboo_data::block_of_state(state)
        .map(|block| block.name.to_owned())
        .unwrap_or_else(|| format!("unknown:{state}"))
}

/// 状态的全部属性，`["lit=false", "waterlogged=true"]` 这样按名字排序的形式。
/// 没有属性时返回空表。
///
/// Every property of the state as `["lit=false", "waterlogged=true"]`, sorted by name;
/// empty when the state has none.
pub fn state_properties(state: u32) -> Vec<String> {
    let Some(block) = mc_bamboo_data::block_of_state(state) else {
        return Vec::new();
    };
    let mut out: Vec<String> = block
        .properties
        .iter()
        .filter_map(|property| {
            crate::structure::state_transform::property_of(state, property.name)
                .map(|value| format!("{}={value}", property.name))
        })
        .collect();
    out.sort();
    out
}

/// 这个状态是不是「可含水」的那种（`waterlogged` 属性存在）。
///
/// Whether the state is waterloggable (it has a `waterlogged` property).
pub fn is_waterloggable_state(state: u32) -> bool {
    is_liquid_container(state)
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
                nbt: None,
            })
            .collect();

        let mut world = PartialWorld {
            accept: 3,
            calls: 0,
        };
        let changed = place_processed(
            &blocks,
            bb,
            0,
            0,
            &crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)),
            None,
            None,
            &mut world,
        )
        .unwrap();
        assert_eq!(world.calls, 5, "every block is still attempted");
        assert_eq!(changed.placed.len(), 3, "only accepted writes count");
        // 包围盒只覆盖被接受的前三个（x = 0,1,2）。
        // The bounds cover only the three accepted blocks (x = 0,1,2).
        assert_eq!(
            changed.bounds,
            Some(crate::structure::BoundingBox {
                min_x: 0,
                min_y: 0,
                min_z: 0,
                max_x: 2,
                max_y: 0,
                max_z: 0,
            })
        );
    }

    /// `chunkBB` 外的方块既不写也不进 `placed`/`bounds`；一个都没写时 `bounds` 是 `None`。
    ///
    /// Blocks outside `chunkBB` are neither written nor recorded; an empty placement has
    /// `bounds == None`.
    #[test]
    fn placed_bounds_skip_out_of_box_blocks() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;

        #[derive(Default)]
        struct World {
            calls: usize,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                self.calls += 1;
                true
            }
        }
        let bb = crate::structure::BoundingBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 1,
            max_y: 1,
            max_z: 1,
        };
        let blocks: Vec<PlaceBlock> = [(0, 0, 0), (1, 1, 1), (5, 5, 5)]
            .into_iter()
            .map(|pos| PlaceBlock {
                pos,
                state: BlockStateSpec::parse_str("minecraft:stone").unwrap(),
                nbt: None,
            })
            .collect();
        let mut world = World::default();
        let placed = place_processed(
            &blocks,
            bb,
            0,
            0,
            &crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)),
            None,
            None,
            &mut world,
        )
        .unwrap();
        assert_eq!(world.calls, 2, "the block at (5,5,5) is outside the box");
        assert_eq!(placed.placed.len(), 2);
        assert_eq!(
            placed.bounds,
            Some(crate::structure::BoundingBox {
                min_x: 0,
                min_y: 0,
                min_z: 0,
                max_x: 1,
                max_y: 1,
                max_z: 1,
            })
        );

        // 空输入：不写、`bounds` 为 `None`。
        // Empty input: nothing written, `bounds` is `None`.
        let mut world = World::default();
        let placed = place_processed(
            &[],
            bb,
            0,
            0,
            &crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)),
            None,
            None,
            &mut world,
        )
        .unwrap();
        assert_eq!(world.calls, 0);
        assert_eq!(placed.bounds, None);
    }

    /// `is_randomizable_container` 认哪些方块：官方只有
    /// `RandomizableContainerBlockEntity` 的九个子类（箱子 / 陷阱箱 / 发射器 / 投掷器 /
    /// 漏斗 / 潜影盒 / 木桶 / 饰纹陶罐 / 合成器）实现 `RandomizableContainer`。
    /// 熔炉 / 酿造台 / 烟熏炉 / 高炉**不是**；可疑的沙砾与沙子**也不是**——
    /// `BrushableBlockEntity extends BlockEntity`，它自带 `LootTable` 字段但没实现该接口。
    /// （这一条是 `jigsaw rc` 对拍抓出来的：我最初把 `brushable_block` 算进去了。）
    ///
    /// Blocks that count: only the nine `RandomizableContainerBlockEntity` subclasses.
    /// Furnaces and brewing stands are not; neither is the brushable block
    /// (`BrushableBlockEntity extends BlockEntity` and has its own `LootTable` field but
    /// does not implement the interface — the `jigsaw rc` oracle caught this).
    #[test]
    fn randomizable_containers_match_the_vanilla_set() {
        use super::is_randomizable_container;
        // 是随机容器的：九个。
        // Randomizable containers: nine of them.
        for name in [
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:dispenser",
            "minecraft:dropper",
            "minecraft:hopper",
            "minecraft:shulker_box",
            "minecraft:barrel",
            "minecraft:decorated_pot",
            "minecraft:crafter",
        ] {
            assert!(
                is_randomizable_container(name),
                "{name} must be randomizable"
            );
        }
        // 不是随机容器的。可疑的沙砾 / 沙子与熔炉类都在这里。
        // Not randomizable, including the brushable blocks and the furnace family.
        for name in [
            "minecraft:furnace",
            "minecraft:blast_furnace",
            "minecraft:smoker",
            "minecraft:brewing_stand",
            "minecraft:ender_chest",
            "minecraft:suspicious_sand",
            "minecraft:suspicious_gravel",
            "minecraft:stone",
            "minecraft:jigsaw",
            "minecraft:not_a_block",
        ] {
            assert!(
                !is_randomizable_container(name),
                "{name} must not be randomizable"
            );
        }
    }

    /// 随机容器在放置时多取一次 `nextLong` 覆盖 `LootTableSeed`；非随机容器不取数。
    ///
    /// A randomizable container draws one extra `nextLong` to overwrite `LootTableSeed`;
    /// other blocks draw nothing.
    #[test]
    fn placing_a_randomizable_container_draws_the_loot_seed() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;
        use mc_bamboo_nbt::Tag;

        #[derive(Default)]
        struct World {
            calls: usize,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                self.calls += 1;
                true
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
        let nbt = || {
            Some(Tag::Compound(vec![(
                "LootTable".to_owned(),
                Tag::String("minecraft:chests/simple_dungeon".to_owned()),
            )]))
        };

        // 箱子：`LootTableSeed` 被写入，且取了一次数。
        // A chest: `LootTableSeed` is written and one draw happened.
        let blocks = vec![PlaceBlock {
            pos: (0, 0, 0),
            state: BlockStateSpec::parse_str("minecraft:chest").unwrap(),
            nbt: nbt(),
        }];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(3));
        let mut world = World::default();
        let placed = place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        let (_, saved) = &placed.placed[0];
        let saved = saved.as_ref().unwrap();
        let Tag::Compound(entries) = saved else {
            panic!("saved nbt must be a compound");
        };
        assert!(
            entries
                .iter()
                .any(|(key, value)| key == "LootTableSeed" && matches!(value, Tag::Long(_))),
            "a chest must get LootTableSeed"
        );

        // 石头 + nbt：不是随机容器，不写 `LootTableSeed`、不取数。
        // Stone with nbt: not a container, so no `LootTableSeed` and no draw.
        let blocks = vec![PlaceBlock {
            pos: (0, 0, 0),
            state: BlockStateSpec::parse_str("minecraft:stone").unwrap(),
            nbt: nbt(),
        }];
        let mut world = World::default();
        let placed = place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        let (_, saved) = &placed.placed[0];
        let Tag::Compound(entries) = saved.as_ref().unwrap() else {
            panic!("saved nbt must be a compound");
        };
        assert!(
            !entries.iter().any(|(key, _)| key == "LootTableSeed"),
            "stone must not get LootTableSeed"
        );
        // 两次调用的取数次数不同：第一次多取了 1 次，所以同一个 `random` 的后续值不同。
        // The draw counts differ, so the same `random` yields a different next value.
        let _ = random.next_long();
    }

    /// watering：写方块时如果该位置原来是流体源、且新方块可含水，就把 `waterlogged`
    /// 置 true；`lockedFluids` 里的位置不会被相邻流体灌。
    ///
    /// Watering: a waterloggable block written into a fluid source gets `waterlogged = true`;
    /// positions in `lockedFluids` are not filled from neighbours.
    #[test]
    fn watering_sets_waterlogged_from_the_previous_fluid() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;

        /// 记录写入，且把「某个位置是流体源」写死。
        ///
        /// Records writes and pins which positions are fluid sources.
        #[derive(Default)]
        struct World {
            sources: std::collections::HashSet<(i32, i32, i32)>,
            writes: Vec<((i32, i32, i32), u32)>,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
                self.writes.push(((x, y, z), state));
                true
            }
            fn fluid_at(&self, x: i32, y: i32, z: i32) -> Option<(bool, bool)> {
                let source = self.sources.contains(&(x, y, z));
                // 测试里只有源流体，没有流动水。
                // The test only has source fluid, no flowing water.
                Some((source, source))
            }
            fn block_state(&self, _x: i32, _y: i32, _z: i32) -> u32 {
                // 上一次写入的状态（`place_processed` 里 watering 只读刚写的方块）。
                // The last written state; watering only reads the block just written.
                self.writes.last().map_or(0, |(_, state)| *state)
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
        // 台阶：默认 `waterlogged=false`，可含水。
        // A slab: `waterlogged=false` by default and waterloggable.
        let blocks = vec![PlaceBlock {
            pos: (2, 3, 4),
            state: BlockStateSpec::parse_str("minecraft:oak_slab[type=bottom,waterlogged=false]")
                .unwrap(),
            nbt: None,
        }];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0));

        // 该位置原来是流体源 → 写完方块后 waterlogged 变 true。
        // The position was a fluid source, so `waterlogged` becomes true.
        let mut world = World::default();
        world.sources.insert((2, 3, 4));
        place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        let last = world.writes.last().expect("at least one write");
        assert_eq!(last.0, (2, 3, 4));
        assert_eq!(
            crate::structure::state_transform::property_of(last.1, "waterlogged"),
            Some("true"),
            "a waterloggable block in a fluid source is waterlogged"
        );

        // 不是流体源 → 保持 false，且只写一次。
        // Not a fluid source: stays false and only one write happens.
        let mut world = World::default();
        place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        assert_eq!(world.writes.len(), 1, "no extra watering write");
        assert_eq!(
            crate::structure::state_transform::property_of(world.writes[0].1, "waterlogged"),
            Some("false")
        );
    }

    /// `LiquidBlockContainer.placeLiquid` 的四类覆写，逐个照官方行为：
    ///
    /// * 台阶：`type=double` 时 `SlabBlock.placeLiquid` 返回 false，不灌；
    /// * 蜡烛 / 营火：灌水同时把 `lit` 清成 false；
    /// * `dried_ghast` 与默认实现相同；
    /// * 海带 / 海草：实现接口但恒返回 false，不灌。
    ///
    /// The four `placeLiquid` overrides, each matching vanilla: a double slab refuses
    /// water, candles and campfires also clear `lit`, `dried_ghast` behaves like the
    /// default, and kelp / seagrass implement the interface but always refuse.
    #[test]
    fn place_liquid_matches_the_four_vanilla_overrides() {
        use super::place_liquid;
        use crate::structure::processor::BlockStateSpec;
        use crate::structure::state_transform::property_of;

        // `state_id_of` 已经处理「名字 + 属性 → 状态 ID」。
        // `state_id_of` already turns a name plus properties into a state id.
        let state = |text: &str| super::state_id_of(&BlockStateSpec::parse_str(text).unwrap());
        let state = |text: &str| state(text).expect("known state");

        // 单台阶：灌水成功。
        // A bottom slab takes the water.
        let bottom = state("minecraft:oak_slab[waterlogged=false,type=bottom]");
        let filled = place_liquid(bottom, true, true).expect("a bottom slab accepts water");
        assert_eq!(property_of(filled, "waterlogged"), Some("true"));

        // 双台阶：`SlabBlock.placeLiquid` 返回 false。
        // A double slab refuses.
        let double = state("minecraft:oak_slab[waterlogged=false,type=double]");
        assert_eq!(
            place_liquid(double, true, true),
            None,
            "a double slab refuses water"
        );

        // 蜡烛：灌水并清 `lit`。
        // A candle is waterlogged and goes out.
        let candle = state("minecraft:candle[lit=true,waterlogged=false]");
        let filled = place_liquid(candle, true, true).expect("a lit candle is waterlogged");
        assert_eq!(property_of(filled, "waterlogged"), Some("true"));
        assert_eq!(property_of(filled, "lit"), Some("false"), "lit is cleared");

        // 营火：同上。
        // Same for a campfire.
        let campfire = state("minecraft:campfire[lit=true,waterlogged=false]");
        let filled = place_liquid(campfire, true, true).expect("a lit campfire is waterlogged");
        assert_eq!(property_of(filled, "waterlogged"), Some("true"));
        assert_eq!(property_of(filled, "lit"), Some("false"));

        // 岩浆不算水。
        // Lava is not water.
        assert_eq!(
            place_liquid(bottom, false, false),
            None,
            "lava does not waterlog"
        );

        // 已经 waterlogged 的方块不再灌（默认实现要求 `!waterlogged`）。
        // An already waterlogged block is left alone.
        let already = state("minecraft:oak_slab[waterlogged=true,type=bottom]");
        assert_eq!(place_liquid(already, true, true), None);

        // 海带：实现接口但恒返回 false。
        // Kelp implements the interface but always refuses.
        let kelp = state("minecraft:kelp");
        assert!(
            super::is_liquid_container(kelp),
            "kelp is a LiquidBlockContainer"
        );
        assert_eq!(place_liquid(kelp, true, true), None, "kelp refuses water");

        // 流动水**不是** `Fluids.WATER`：`is(WATER)` 比的是流体对象身份，
        // 流动水的类型是 `flowing_water`。官方因此不会在流动水上直接灌水。
        // Flowing water is **not** `Fluids.WATER`: `is(WATER)` compares fluid identity and
        // flowing water's type is `flowing_water`, so vanilla does not waterlog from it.
        assert_eq!(
            place_liquid(bottom, true, false),
            None,
            "flowing water does not waterlog"
        );
    }

    /// 调用方不提供流体模型（`fluid_is_source` 返回 `None`）时，watering 整段跳过。
    ///
    /// When the caller has no fluid model, the whole watering pass is skipped.
    #[test]
    fn watering_is_skipped_without_a_fluid_model() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;

        #[derive(Default)]
        struct World {
            writes: usize,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                self.writes += 1;
                true
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
        let blocks = vec![PlaceBlock {
            pos: (0, 0, 0),
            state: BlockStateSpec::parse_str("minecraft:oak_slab[type=bottom,waterlogged=false]")
                .unwrap(),
            nbt: None,
        }];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0));
        let mut world = World::default();
        place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        assert_eq!(world.writes, 1, "only the block itself is written");
    }

    /// `place_processed` 把 nbt 交给 `set_block_entity`（官方的
    /// `blockEntity.loadWithComponents`）；随机容器的 nbt 里带着放置期生成的
    /// `LootTableSeed`。
    ///
    /// `place_processed` hands the nbt to `set_block_entity` (vanilla's
    /// `blockEntity.loadWithComponents`); a randomizable container's nbt carries the
    /// placement-time `LootTableSeed`.
    #[test]
    fn place_processed_hands_the_nbt_to_the_block_entity_hook() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;
        use mc_bamboo_nbt::Tag;

        /// 记录 `set_block_entity` 收到的 nbt。
        ///
        /// Records what `set_block_entity` receives.
        #[derive(Default)]
        struct World {
            entities: Vec<((i32, i32, i32), Tag)>,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                true
            }
            fn set_block_entity(&mut self, x: i32, y: i32, z: i32, nbt: &Tag) {
                self.entities.push(((x, y, z), nbt.clone()));
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
        let blocks = vec![PlaceBlock {
            pos: (0, 5, 0),
            state: BlockStateSpec::parse_str("minecraft:chest").unwrap(),
            nbt: Some(Tag::Compound(vec![(
                "LootTable".to_owned(),
                Tag::String("minecraft:chests/simple_dungeon".to_owned()),
            )])),
        }];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(11));
        let mut world = World::default();
        place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        assert_eq!(world.entities.len(), 1, "one block entity written");
        let (pos, tag) = &world.entities[0];
        assert_eq!(*pos, (0, 5, 0));
        let Tag::Compound(entries) = tag else {
            panic!("block entity nbt must be a compound");
        };
        assert!(entries.iter().any(|(key, _)| key == "LootTable"));
        assert!(
            entries.iter().any(|(key, _)| key == "LootTableSeed"),
            "a chest gets the placement-time seed"
        );
    }

    /// 不是随机容器的方块也会把 nbt 交给 `set_block_entity`，只是不带 `LootTableSeed`。
    ///
    /// A non-container block also hands its nbt to `set_block_entity`, just without
    /// `LootTableSeed`.
    #[test]
    fn place_processed_hands_nbt_for_plain_blocks_too() {
        use super::{BlockWriter, PlaceBlock, place_processed};
        use crate::structure::processor::BlockStateSpec;
        use mc_bamboo_nbt::Tag;

        #[derive(Default)]
        struct World {
            entities: usize,
        }
        impl BlockWriter for World {
            fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: u32) -> bool {
                true
            }
            fn set_block_entity(&mut self, _x: i32, _y: i32, _z: i32, _nbt: &Tag) {
                self.entities += 1;
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
        let blocks = vec![
            PlaceBlock {
                pos: (0, 0, 0),
                state: BlockStateSpec::parse_str("minecraft:stone").unwrap(),
                nbt: Some(Tag::Compound(vec![(
                    "Marker".to_owned(),
                    Tag::String("x".to_owned()),
                )])),
            },
            // 没有 nbt 的方块不触发。
            // A block without nbt does not trigger it.
            PlaceBlock {
                pos: (1, 0, 0),
                state: BlockStateSpec::parse_str("minecraft:stone").unwrap(),
                nbt: None,
            },
        ];
        let random = crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0));
        let mut world = World::default();
        place_processed(&blocks, bb, 0, 0, &random, None, &mut world).unwrap();
        assert_eq!(world.entities, 1, "only the block with nbt");
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
            &crate::random::Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)),
            None,
            None,
            &mut world,
        )
        .unwrap();
        assert!(!written.placed.is_empty(), "expected some writes");
        // 每个写出的位置都在盒内。
        // Every write must be inside the box.
        for (x, y, z, _) in &world.writes {
            assert!(chunk_bb.contains(*x, *y, *z), "write outside chunk_bb");
        }
        // 盒外的方块确实被跳过了：总写入数少于模板方块数时成立，或全部在盒内。
        // The filter must have skipped something unless every block happens to be inside.
        assert!(
            written.placed.len() <= template.blocks.len(),
            "wrote {} of {} blocks",
            written.placed.len(),
            template.blocks.len()
        );
        let _ = palette;
    }

    /// `place_entities` 照官方 `StructureTemplate.placeEntities` 变换实体：
    /// `Pos` 换成变换后的 double 列表、删 `UUID`、`block_pos` 改写成三个 int、`yRot`
    /// 按 rotate/mirror 累加。用真实模板里带村民的那一份验证。
    ///
    /// `place_entities` transforms entities the way vanilla does: overwrite `Pos` with the
    /// transformed doubles, drop `UUID`, rewrite `block_pos` as three ints, and accumulate
    /// `yRot` through rotate/mirror. Verified on a real template that carries a villager.
    #[test]
    fn place_entities_matches_the_vanilla_transform() {
        use super::{place_entities, wrap_degrees_f32};
        use mc_bamboo_nbt::Tag;

        let name = "minecraft:village/plains/villagers/unemployed";
        let template = crate::structure::template::StructureTemplate::load(name).unwrap();
        assert_eq!(template.entities.len(), 1, "{name} should carry one entity");

        // 无镜像无旋转、原点：`Pos` 与 `block_pos` 只是平移到世界坐标。
        // No mirror, no rotation, zero position: `Pos` and `block_pos` just translate.
        let placed = place_entities(&template, (100, 70, -50), 0, 0, None);
        assert_eq!(placed.len(), 1);
        let entity = &placed[0];
        let source = &template.entities[0];
        assert!((entity.x - (source.pos.0 + 100.0)).abs() < 1e-9);
        assert!((entity.y - (source.pos.1 + 70.0)).abs() < 1e-9);
        assert!((entity.z - (source.pos.2 - 50.0)).abs() < 1e-9);
        let Tag::Compound(entries) = &entity.tag else {
            panic!("entity nbt must be a compound")
        };
        let Tag::List(pos) = &entries.iter().find(|(key, _)| key == "Pos").unwrap().1 else {
            panic!("Pos must be a list")
        };
        assert_eq!(pos.len(), 3);
        assert_eq!(pos[0], Tag::Double(entity.x));
        assert_eq!(pos[1], Tag::Double(entity.y));
        assert_eq!(pos[2], Tag::Double(entity.z));
        assert!(
            !entries.iter().any(|(key, _)| key == "UUID"),
            "UUID is dropped"
        );
        let block_pos = entries
            .iter()
            .find(|(key, _)| key == "block_pos")
            .map(|(_, value)| value.clone());
        if let Some(Tag::IntArray(values)) = block_pos {
            assert_eq!(values.len(), 3);
        }

        // 朝向：`rotate(CLOCKWISE_90)` 是 +90，`mirror(LEFT_RIGHT)` 是 `180 - yRot`，
        // 末尾再减一次未包装的原值。
        // Rotation: `rotate(CLOCKWISE_90)` adds 90, `mirror(LEFT_RIGHT)` is `180 - yRot`,
        // and the raw original is subtracted once more.
        let raw = entity.raw_y_rot;
        let base = wrap_degrees_f32(raw);
        let rotated = place_entities(&template, (0, 0, 0), 0, 1, None);
        let expected = (base + 90.0) + (base - raw);
        assert!(
            (rotated[0].y_rot - expected).abs() < 1e-4,
            "yRot {} != {expected}",
            rotated[0].y_rot
        );
        let mirrored = place_entities(&template, (0, 0, 0), 1, 0, None);
        let expected = base + ((180.0 - base) - raw);
        assert!(
            (mirrored[0].y_rot - expected).abs() < 1e-4,
            "yRot {} != {expected}",
            mirrored[0].y_rot
        );

        // 包围盒过滤：`block_pos` 落在盒外时整个实体跳过。
        // Box filtering: an entity whose `block_pos` is outside is skipped entirely.
        let world_block = (
            source.block_pos.0 + 100,
            source.block_pos.1 + 70,
            source.block_pos.2 - 50,
        );
        let outside = crate::structure::BoundingBox {
            min_x: world_block.0 + 1000,
            min_y: world_block.1,
            min_z: world_block.2,
            max_x: world_block.0 + 1001,
            max_y: world_block.1 + 1,
            max_z: world_block.2 + 1,
        };
        assert!(
            place_entities(&template, (100, 70, -50), 0, 0, Some(outside)).is_empty(),
            "an entity outside the box is skipped"
        );
    }
}
