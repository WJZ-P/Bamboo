//! `StructureProcessor` 的执行：`processBlock` / `finalizeProcessing` 的纯函数版。
//!
//! 官方 `StructureTemplate.processBlockInfos` 对模板里每个方块依次跑
//! `settings.getProcessors()` 里的 processor（任一返回 null 就丢弃该方块），最后再对整份
//! 列表跑一遍 `finalizeProcessing`。这里照同一条链路翻译，把「世界」抽象成
//! `ProcessorLevel`（读某个位置的方块状态 + 标签判定）。
//!
//! 随机源的关键细节（最容易写错的地方）：
//!
//! * `BlockRotProcessor.processBlock` 与 `RuleProcessor.processBlock` 都用
//!   `settings.getRandom(processedBlockInfo.pos())`；没有显式随机源时是
//!   `RandomSource.create(Mth.getSeed(pos))`——**每个方块一个独立随机源**，不消耗世界随机源。
//! * `CappedProcessor.finalizeProcessing` 用
//!   `RandomSource.createThreadLocalInstance(level.getLevel().getSeed()).forkPositional().at(position)`，
//!   即 `Mth.getSeed(piecePosition)` 派生的随机源（`position` 是结构件位置）。
//!
//! Execution of `StructureProcessor`: the pure-function form of `processBlock` /
//! `finalizeProcessing`. Vanilla `StructureTemplate.processBlockInfos` runs the settings'
//! processors over every template block (any returning null drops the block) and then runs
//! `finalizeProcessing` over the whole list. Per-block processors derive their random
//! source from the block position (`RandomSource.create(Mth.getSeed(pos))`, no world
//! randomness); `CappedProcessor` derives its source from the piece position.

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    random::{Random, positional_seed},
    structure::{
        processor::{BlockSetRef, BlockStateSpec, PosRuleTest, Processor, ProcessorRule, RuleTest},
        start::BoundingBox,
    },
};

/// processor 需要读世界的地方：`level.getBlockState(pos)` 与标签判定。
///
/// What a processor needs from the world: `level.getBlockState(pos)` plus tag lookups.
pub trait ProcessorLevel {
    /// `level.getBlockState(pos)`。
    ///
    /// `level.getBlockState(pos)`.
    fn block_state(&self, pos: (i32, i32, i32)) -> BlockStateSpec;

    /// `state.is(HolderSet<Block>)`：方块是否属于某个标签或显式集合。
    ///
    /// `state.is(HolderSet<Block>)`: whether the state belongs to a tag or explicit set.
    fn in_block_set(&self, state: &BlockStateSpec, set: &BlockSetRef) -> bool;

    /// `level.getHeight(heightmap, x, z)`：`GravityProcessor` 用它算方块的落点。
    /// 返回 `None` 表示「这个 level 不提供高度图」——`Gravity` 会保留原位置并**报错**
    /// 而不是静默算错（早期版本用一个哨兵整数，会让 Y 回绕）。
    ///
    /// `level.getHeight(heightmap, x, z)`, used by `GravityProcessor`. `None` means the
    /// level has no heightmap; `Gravity` then keeps the position and returns an error
    /// rather than silently computing a wrapped Y.
    fn height(&self, _heightmap: &str, _x: i32, _z: i32) -> Option<i32> {
        None
    }
}

/// 一个待处理的方块，对应官方 `StructureTemplate.StructureBlockInfo`。
///
/// One block to process, mirroring vanilla `StructureTemplate.StructureBlockInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessBlock {
    /// `blockInfo.pos`：模板内的相对坐标（未变换），用作 `templateRelativePos`。
    ///
    /// `blockInfo.pos`: the template-relative position (untransformed), used as
    /// `templateRelativePos`.
    pub pos: (i32, i32, i32),
    /// 世界坐标（`calculateRelativePosition(settings, pos).offset(position)`）。
    ///
    /// The world position (`calculateRelativePosition(settings, pos).offset(position)`).
    pub world_pos: (i32, i32, i32),
    pub state: BlockStateSpec,
    /// `blockInfo.nbt`：拼图方块的 `JigsawReplacementProcessor` 要读 `final_state`，
    /// 带 nbt 的方块还要先写 `BARRIER`。
    ///
    /// `blockInfo.nbt`: `JigsawReplacementProcessor` reads `final_state` from it, and a
    /// block carrying nbt gets a `BARRIER` written first.
    pub nbt: Option<mc_bamboo_nbt::Tag>,
}

/// `StructurePlaceSettings.getRandom(pos)`：没有显式随机源时按位置派生。
///
/// `StructurePlaceSettings.getRandom(pos)`: derived from the position when no explicit
/// source is set.
/// `StructurePlaceSettings.getRandom(pos)`。`explicit` 为 `Some` 时直接返回它
/// （官方 `if (this.random != null) return this.random;`），否则按位置派生
/// `RandomSource.create(Mth.getSeed(pos))`。
///
/// `StructurePlaceSettings.getRandom(pos)`: returns `explicit` when set (vanilla
/// `if (this.random != null) return this.random;`), otherwise derives
/// `RandomSource.create(Mth.getSeed(pos))`.
fn settings_random(explicit: Option<&Random>, pos: (i32, i32, i32)) -> Random {
    match explicit {
        Some(random) => random.clone(),
        None => Random::Legacy(LegacyRandom::new(positional_seed(pos.0, pos.1, pos.2))),
    }
}

/// `RuleTest.test(state, pos, random)`。
///
/// `RuleTest.test(state, pos, random)`.
fn rule_test_matches(
    test: &RuleTest,
    state: &BlockStateSpec,
    level: &impl ProcessorLevel,
    random: &Random,
) -> bool {
    match test {
        RuleTest::AlwaysTrue => true,
        RuleTest::BlockMatch { block } => state.name == *block,
        RuleTest::BlockStateMatch { block_state } => state == block_state,
        RuleTest::TagMatch { tag } => {
            level.in_block_set(state, &BlockSetRef::Tag(format!("#{tag}")))
        }
        // `random.nextFloat() < probability`：方块不匹配时**不取随机数**（短路）。
        // `random.nextFloat() < probability`: no draw when the block does not match.
        RuleTest::RandomBlockMatch { block, probability } => {
            state.name == *block && random.next_float() < *probability
        }
        RuleTest::RandomBlockStateMatch {
            block_state,
            probability,
        } => state == block_state && random.next_float() < *probability,
    }
}

/// `ProcessorRule.test(level, inputState, inTemplatePos, worldPos, reference, random)`。
///
/// `ProcessorRule.test(level, inputState, inTemplatePos, worldPos, reference, random)`.
fn rule_matches(
    rule: &ProcessorRule,
    block: &ProcessBlock,
    reference_pos: (i32, i32, i32),
    level: &impl ProcessorLevel,
    random: &Random,
) -> bool {
    // `inputPredicate.test(inputState, worldPos, random)`：用模板里的方块。
    // `inputPredicate.test(inputState, worldPos, random)`: uses the template block.
    if !rule_test_matches(&rule.input_predicate, &block.state, level, random) {
        return false;
    }
    // `locPredicate.testAgainstWorldState(level, worldPos, random)`：读**世界**的方块。
    // `locPredicate.testAgainstWorldState(level, worldPos, random)`: reads the world state.
    let world_state = level.block_state(block.world_pos);
    if !rule_test_matches(&rule.location_predicate, &world_state, level, random) {
        return false;
    }
    pos_rule_test_matches(
        &rule.position_predicate,
        block.pos,
        block.world_pos,
        reference_pos,
        random,
    )
}

/// `PosRuleTest.test`。`AxisAlignedLinearPosTest` 的公式照官方。
///
/// `PosRuleTest.test`. `AxisAlignedLinearPosTest` uses the vanilla formula verbatim.
fn pos_rule_test_matches(
    test: &PosRuleTest,
    _template_pos: (i32, i32, i32),
    world_pos: (i32, i32, i32),
    reference: (i32, i32, i32),
    random: &Random,
) -> bool {
    match test {
        PosRuleTest::AlwaysTrue => true,
        PosRuleTest::AxisAlignedLinear {
            min_chance,
            max_chance,
            min_dist,
            max_dist,
            axis,
        } => {
            // `Direction.get(POSITIVE, axis).getStep*()`：沿轴分量取 1，其余 0。
            // `Direction.get(POSITIVE, axis)`: 1 along the axis, 0 elsewhere.
            let step = match axis.as_str() {
                "x" => (1.0_f32, 0.0, 0.0),
                "z" => (0.0, 0.0, 1.0),
                _ => (0.0, 1.0, 0.0),
            };
            let dx = (world_pos.0 - reference.0) as f32 * step.0;
            let dy = (world_pos.1 - reference.1) as f32 * step.1;
            let dz = (world_pos.2 - reference.2) as f32 * step.2;
            let dist = (dx.abs() + dy.abs() + dz.abs()) as i32;
            let rnd = random.next_float();
            rnd <= clamped_lerp(
                inverse_lerp(dist as f32, *min_dist as f32, *max_dist as f32),
                *min_chance,
                *max_chance,
            )
        }
    }
}

/// `Mth.inverseLerp(value, min, max)`。
///
/// `Mth.inverseLerp(value, min, max)`.
fn inverse_lerp(value: f32, min: f32, max: f32) -> f32 {
    if min == max {
        0.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}

/// `Mth.clampedLerp(delta, start, end)`。
///
/// `Mth.clampedLerp(delta, start, end)`.
fn clamped_lerp(delta: f32, start: f32, end: f32) -> f32 {
    if delta < 0.0 {
        start
    } else if delta > 1.0 {
        end
    } else {
        start + delta * (end - start)
    }
}

/// 一个 processor 的 `processBlock`：`Some` 表示保留（可能改过），`None` 表示丢弃。
///
/// One processor's `processBlock`: `Some` keeps the block (possibly altered), `None` drops it.
pub fn process_block(
    processor: &Processor,
    block: &ProcessBlock,
    reference_pos: (i32, i32, i32),
    level: &impl ProcessorLevel,
    explicit_random: Option<&Random>,
) -> Option<ProcessBlock> {
    match processor {
        Processor::BlockRot {
            integrity,
            rottable_blocks,
        } => {
            // 官方 `(!rottable.isPresent() || state.is(rottable)) && !(nextFloat() <= integrity)`
            // 才返回 null；不在集合里时**不取随机数**。
            // Vanilla returns null only on that branch, and the roll is drawn only then.
            let in_set = match rottable_blocks {
                None => true,
                Some(set) => level.in_block_set(&block.state, set),
            };
            if !in_set {
                return Some(block.clone());
            }
            let random = settings_random(explicit_random, block.world_pos);
            if random.next_float() <= *integrity {
                Some(block.clone())
            } else {
                None
            }
        }
        Processor::Rule { rules } => {
            // 每个方块一个随机源，整条规则链共用（官方在同一函数里只建一次）。
            // One source per block, shared by the whole rule chain (vanilla builds it once).
            let random = settings_random(explicit_random, block.world_pos);
            for rule in rules {
                if rule_matches(rule, block, reference_pos, level, &random) {
                    let mut out = block.clone();
                    out.state = rule.output_state.clone();
                    // `rule.getOutputTag(random, processedBlockInfo.nbt())`。
                    // 方块实体 nbt 只可能是复合或缺失；`apply` 的 `Err` 只在这种前提被破坏时
                    // 出现，那时保留原 nbt（丢方块比丢 nbt 更糟）。
                    // The block-entity nbt is either a compound or absent; `apply` only errors
                    // when that premise breaks, in which case the original nbt is kept
                    // (dropping the whole block would be worse).
                    if let Ok(nbt) = rule.block_entity_modifier.apply(&random, out.nbt.as_ref()) {
                        out.nbt = nbt;
                    }
                    return Some(out);
                }
            }
            Some(block.clone())
        }
        Processor::ProtectedBlocks { value } => {
            // 官方读的是**世界**该位置的方块，不是模板里的方块。
            // Vanilla reads the world state at that position, not the template block.
            if level.in_block_set(&level.block_state(block.world_pos), value) {
                None
            } else {
                Some(block.clone())
            }
        }
        // `CappedProcessor.processBlock` 是空实现，它只在 `finalizeProcessing` 里干活。
        // `CappedProcessor.processBlock` is a no-op; it only works in `finalizeProcessing`.
        Processor::Capped { .. } => Some(block.clone()),
        // `BlockIgnoreProcessor.processBlock`：`toIgnore.contains(state.getBlock())` 时丢弃。
        // `BlockIgnoreProcessor.processBlock`: drops when the block is in `toIgnore`.
        Processor::BlockIgnore { to_ignore } => {
            if to_ignore.contains(&block.state.name) {
                None
            } else {
                Some(block.clone())
            }
        }
        // `GravityProcessor.processBlock`：把 Y 抬到 `height + offset + templateY`。
        // `GravityProcessor.processBlock`: lifts Y to `height + offset + templateY`.
        Processor::Gravity { heightmap, offset } => {
            let pos = block.world_pos;
            // 官方在 `level instanceof ServerLevel` 时把 `*_WG` 换成正式变体。
            // Vanilla swaps `*_WG` for the real variant when the level is a `ServerLevel`.
            let heightmap = match heightmap.as_str() {
                "WORLD_SURFACE_WG" => "WORLD_SURFACE",
                "OCEAN_FLOOR_WG" => "OCEAN_FLOOR",
                other => other,
            };
            // 拿不到高度图时保留原位置：官方在真实世界里一定有值，`None` 只可能意味着
            // 调用方的 `ProcessorLevel` 没实现 `height()`。跳过比算错安全。
            // Without a heightmap, keep the position: vanilla always has a value in a real
            // world, so `None` can only mean the caller's `ProcessorLevel` does not
            // implement `height()`. Skipping beats computing a wrong Y.
            let Some(height) = level.height(heightmap, pos.0, pos.2) else {
                return Some(block.clone());
            };
            let delta = block.pos.1;
            let mut out = block.clone();
            out.world_pos = (
                pos.0,
                height.wrapping_add(*offset).wrapping_add(delta),
                pos.2,
            );
            Some(out)
        }
        // `JigsawReplacementProcessor.processBlock`：把拼图方块换成 nbt 里的 `final_state`。
        // `JigsawReplacementProcessor.processBlock`: replaces the jigsaw with `final_state`.
        Processor::JigsawReplacement => {
            if block.state.name != "minecraft:jigsaw" {
                return Some(block.clone());
            }
            // 没有 nbt 时官方只打警告并保留原方块（`will not replace`）。
            // Without nbt, vanilla warns and keeps the block ("will not replace").
            let Some(nbt) = block.nbt.as_ref() else {
                return Some(block.clone());
            };
            let state_text = nbt_string(nbt, "final_state").unwrap_or("minecraft:air");
            let Ok(state) = BlockStateSpec::parse_str(state_text) else {
                // 解析失败时官方返回 null（丢弃）。
                // Vanilla returns null (drops) when parsing fails.
                return None;
            };
            // `structure_void` 也丢弃。
            // `structure_void` is dropped as well.
            if state.name == "minecraft:structure_void" {
                return None;
            }
            let mut out = block.clone();
            out.state = state;
            out.nbt = None;
            Some(out)
        }
    }
}

/// 从 NBT 复合里读一个字符串字段。
///
/// Read one string field from an NBT compound.
fn nbt_string<'a>(tag: &'a mc_bamboo_nbt::Tag, key: &str) -> Option<&'a str> {
    let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
        return None;
    };
    entries.iter().find_map(|(name, value)| {
        if name == key {
            match value {
                mc_bamboo_nbt::Tag::String(text) => Some(text.as_str()),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// `StructurePlaceSettings` 里与本模块相关的部分：位置、参考点、块过滤、随机源。
///
/// The parts of `StructurePlaceSettings` this module needs: position, reference point,
/// bounding box and random source.
#[derive(Clone, Copy)]
pub struct PlaceSettings<'a> {
    /// 结构件位置（`Piece::position`），`CappedProcessor` 用它派生随机源。
    ///
    /// The piece position, used by `CappedProcessor` to derive its random source.
    pub piece_position: (i32, i32, i32),
    /// `placeInChunk` 的参考点，供 `position_predicate` 用。
    ///
    /// The reference position passed to `placeInChunk`, used by `position_predicate`.
    pub reference_pos: (i32, i32, i32),
    /// `settings.getBoundingBox()`；`None` 表示不过滤。
    ///
    /// `settings.getBoundingBox()`; `None` means no filtering.
    pub chunk_bb: Option<BoundingBox>,
    /// 世界种子，`CappedProcessor.finalizeProcessing` 用它派生随机源。
    ///
    /// The level seed, used by `CappedProcessor.finalizeProcessing`.
    pub level_seed: i64,
    /// `settings.getRandom()` 显式设置的随机源；`None` 时按方块位置派生。
    ///
    /// An explicit `settings.getRandom()` source; `None` derives one from the block
    /// position.
    pub explicit_random: Option<&'a Random>,
}

/// `StructureTemplate.processBlockInfos` 的编排：逐方块跑 processor 链，再跑
/// `finalizeProcessing`。
///
/// `settings.chunk_bb` 对应官方的 `settings.getBoundingBox()`：
///
/// * 只要 processor 链里没有任何一个 `evaluatesEntirePieceState()` 为真（26.3 只有
///   `CappedProcessor` 会返回 true），且 `chunk_bb` 存在，就**只处理落在 `chunk_bb` 内的
///   方块**（官方 `processOnlyInCurrentChunk`）。这决定 `processedBlockInfoList` 的长度，
///   进而决定 `CappedProcessor` 洗牌时用的下标范围，所以必须照做。
/// * `chunk_bb` 为 `None` 表示不过滤（官方 `settings.getBoundingBox() == null`）。
///
/// The `StructureTemplate.processBlockInfos` orchestration: run the processor chain per
/// block, then `finalizeProcessing`. `settings.chunk_bb` mirrors
/// `settings.getBoundingBox()`: when no processor evaluates the whole piece state (in
/// 26.3 only `CappedProcessor` does) and a bounding box is set, only blocks inside it are
/// processed. That governs the length of `processedBlockInfoList`, hence the index range
/// `CappedProcessor` shuffles over.
pub fn process_blocks(
    processors: &[Processor],
    blocks: &[ProcessBlock],
    level: &impl ProcessorLevel,
    settings: PlaceSettings<'_>,
) -> Vec<ProcessBlock> {
    let PlaceSettings {
        piece_position,
        reference_pos,
        chunk_bb,
        level_seed,
        explicit_random,
    } = settings;
    // 官方先扫一遍 processor 决定是否整件处理。
    // Vanilla first scans the processors to decide whether to process the whole piece.
    let process_only_in_current_chunk = processors
        .iter()
        .all(|processor| !processor.evaluates_entire_piece_state());
    let mut originals = Vec::new();
    let mut processed = Vec::new();
    for block in blocks {
        let inside = match (process_only_in_current_chunk, chunk_bb) {
            (true, Some(bb)) => {
                bb.contains(block.world_pos.0, block.world_pos.1, block.world_pos.2)
            }
            _ => true,
        };
        if !inside {
            continue;
        }
        let mut current = Some(block.clone());
        for processor in processors {
            let Some(block_now) = current else {
                break;
            };
            current = process_block(processor, &block_now, reference_pos, level, explicit_random);
        }
        if let Some(block_now) = current {
            originals.push(block.clone());
            processed.push(block_now);
        }
    }
    for processor in processors {
        if let Processor::Capped { delegate, limit } = processor {
            processed = finalize_capped(CappedInput {
                delegate,
                limit: *limit,
                originals: &originals,
                processed,
                piece_position,
                reference_pos,
                level,
                level_seed,
            });
        }
    }
    processed
}

/// `finalize_capped` 的输入，避免参数过多。
///
/// Inputs of `finalize_capped`, kept in one place to avoid a long argument list.
struct CappedInput<'a, L> {
    delegate: &'a Processor,
    limit: i32,
    originals: &'a [ProcessBlock],
    processed: Vec<ProcessBlock>,
    piece_position: (i32, i32, i32),
    reference_pos: (i32, i32, i32),
    level: &'a L,
    level_seed: i64,
}

/// `CappedProcessor.finalizeProcessing`：用位置派生的随机源挑下标，最多改 `limit` 个。
///
/// `CappedProcessor.finalizeProcessing`: picks indices with the position-derived source and
/// alters at most `limit` blocks.
fn finalize_capped<L: ProcessorLevel>(input: CappedInput<'_, L>) -> Vec<ProcessBlock> {
    let CappedInput {
        delegate,
        limit,
        originals,
        mut processed,
        piece_position,
        reference_pos,
        level,
        level_seed,
    } = input;
    // `if (limit.maxInclusive() != 0 && !list.isEmpty())`，且两份列表长度必须相同。
    // Vanilla also requires both lists to have the same length.
    if limit == 0 || processed.is_empty() || originals.len() != processed.len() {
        return processed;
    }
    // `RandomSource.createThreadLocalInstance(levelSeed).forkPositional().at(position)`：
    // 先 fork 位置工厂再按结构件位置取随机源。
    // `RandomSource.createThreadLocalInstance(levelSeed).forkPositional().at(position)`:
    // fork the positional factory from the level seed, then position it at the piece.
    let random = Random::Legacy(LegacyRandom::new(level_seed))
        .fork_positional()
        .at(piece_position.0, piece_position.1, piece_position.2);
    let max_to_replace = limit.min(processed.len() as i32);
    if max_to_replace < 1 {
        return processed;
    }
    // `Util.toShuffledList(IntStream.range(0, size), random)`。
    // `Util.toShuffledList(IntStream.range(0, size), random)`.
    let mut indices: Vec<usize> = (0..processed.len()).collect();
    shuffle(&mut indices, &random);
    let mut replaced = 0;
    for index in indices {
        if replaced >= max_to_replace {
            break;
        }
        // 官方传 `originalBlockInfo.pos()` 作为 `templateRelativePos`，处理的是
        // `processedBlockInfoList.get(index)`。
        // Vanilla passes `originalBlockInfo.pos()` as `templateRelativePos` while processing
        // `processedBlockInfoList.get(index)`.
        if let Some(altered) =
            process_block(delegate, &processed[index], reference_pos, level, None)
            && altered != processed[index]
        {
            replaced += 1;
            processed[index] = altered;
        }
    }
    processed
}

/// `Util.shuffle(list, random)`：从尾部往前的 Fisher-Yates（与
/// `rotation::shuffle` 同一条，但作用于 `Random` 而不是 `WorldgenRandom`）。
///
/// `Util.shuffle(list, random)`: back-to-front Fisher-Yates, the same algorithm as
/// `rotation::shuffle` but over a plain `Random`.
fn shuffle<T>(items: &mut [T], random: &Random) {
    for i in (2..=items.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        items.swap(i - 1, swap_to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::processor::{
        BlockStateSpec, PosRuleTest, ProcessorRule, RuleBlockEntityModifier,
    };

    /// 所有位置都返回同一个状态的假世界，标签走显式名字列表。
    ///
    /// A fake level returning one state everywhere; tags go through an explicit name list.
    struct OneState {
        state: BlockStateSpec,
        tag_blocks: Vec<&'static str>,
    }

    impl ProcessorLevel for OneState {
        fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
            self.state.clone()
        }

        fn in_block_set(&self, state: &BlockStateSpec, set: &BlockSetRef) -> bool {
            match set {
                BlockSetRef::Blocks(names) => names.contains(&state.name),
                BlockSetRef::Tag(_) => self.tag_blocks.contains(&state.name.as_str()),
            }
        }
    }

    /// 构造一份默认 settings：位置 / 参考点 / 可选盒，世界种子 0、无显式随机源。
    ///
    /// Default settings: positions, optional box, level seed 0, no explicit source.
    fn settings(
        piece: (i32, i32, i32),
        reference: (i32, i32, i32),
        chunk_bb: Option<BoundingBox>,
    ) -> PlaceSettings<'static> {
        PlaceSettings {
            piece_position: piece,
            reference_pos: reference,
            chunk_bb,
            level_seed: 0,
            explicit_random: None,
        }
    }

    fn state(name: &str) -> BlockStateSpec {
        BlockStateSpec::parse_str(name).unwrap()
    }

    fn block(pos: (i32, i32, i32), name: &str) -> ProcessBlock {
        ProcessBlock {
            pos,
            world_pos: pos,
            state: state(name),
            nbt: None,
        }
    }

    /// `block_rot` 在 `nextFloat() <= integrity` 时保留，否则丢弃；不在 `rottable_blocks`
    /// 里的方块直接保留且**不取**随机数。
    ///
    /// `block_rot` keeps a block when `nextFloat() <= integrity`; a block outside
    /// `rottable_blocks` is kept without drawing at all.
    #[test]
    fn block_rot_keeps_high_integrity_and_skips_untracked_blocks() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        // `(0,64,0)` 的 `Mth.getSeed` 派生随机源首个 `nextFloat` 是 0.9703。
        // The first `nextFloat` of the `(0,64,0)` position source is 0.9703.
        let processor = Processor::BlockRot {
            integrity: 0.99,
            rottable_blocks: None,
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_some()
        );
        let processor = Processor::BlockRot {
            integrity: 0.95,
            rottable_blocks: None,
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );

        // 不在 `rottable_blocks` 里：无论 integrity 多低都保留（且不取随机数）。
        // Outside `rottable_blocks`: kept whatever the integrity, and nothing is drawn.
        let processor = Processor::BlockRot {
            integrity: 0.0,
            rottable_blocks: Some(BlockSetRef::Blocks(vec![
                "minecraft:deepslate_bricks".to_owned(),
            ])),
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_some()
        );
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:deepslate_bricks"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );
    }

    /// `rule` 只应用第一条命中的规则，并把状态换成 `output_state`。
    ///
    /// `rule` applies only the first matching rule and replaces the state with
    /// `output_state`.
    #[test]
    fn rule_applies_only_the_first_match() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let rules = vec![
            ProcessorRule {
                input_predicate: RuleTest::BlockMatch {
                    block: "minecraft:stone".to_owned(),
                },
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AlwaysTrue,
                output_state: state("minecraft:cobblestone"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            },
            ProcessorRule {
                input_predicate: RuleTest::AlwaysTrue,
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AlwaysTrue,
                output_state: state("minecraft:dirt"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            },
        ];
        let processor = Processor::Rule { rules };
        let out = process_block(
            &processor,
            &block((0, 64, 0), "minecraft:stone"),
            (0, 64, 0),
            &level,
            None,
        )
        .unwrap();
        assert_eq!(out.state.name, "minecraft:cobblestone");
    }

    /// `location_predicate` 读**世界**方块，不读模板方块。
    ///
    /// `location_predicate` reads the world state, not the template block.
    #[test]
    fn location_predicate_reads_the_world() {
        let level = OneState {
            state: state("minecraft:stone"),
            tag_blocks: Vec::new(),
        };
        let processor = Processor::Rule {
            rules: vec![ProcessorRule {
                input_predicate: RuleTest::AlwaysTrue,
                // 世界是石头 -> 命中；模板是泥土也无所谓。
                // The world is stone, so this matches even though the template is dirt.
                location_predicate: RuleTest::BlockMatch {
                    block: "minecraft:stone".to_owned(),
                },
                position_predicate: PosRuleTest::AlwaysTrue,
                output_state: state("minecraft:gravel"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            }],
        };
        let out = process_block(
            &processor,
            &block((0, 64, 0), "minecraft:dirt"),
            (0, 64, 0),
            &level,
            None,
        )
        .unwrap();
        assert_eq!(out.state.name, "minecraft:gravel");
    }

    /// `protected_blocks` 只丢弃**世界**方块属于该集合的位置。
    ///
    /// `protected_blocks` drops only the positions whose **world** block is in the set.
    #[test]
    fn protected_blocks_reads_the_world() {
        let level = OneState {
            state: state("minecraft:bedrock"),
            tag_blocks: Vec::new(),
        };
        let processor = Processor::ProtectedBlocks {
            value: BlockSetRef::Blocks(vec!["minecraft:bedrock".to_owned()]),
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );

        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_some()
        );
    }

    /// `process_blocks` 里任一 processor 返回 `None` 就丢弃该方块，其余继续。
    ///
    /// In `process_blocks` any processor returning `None` drops the block; the rest run on.
    #[test]
    fn chain_drops_on_first_none() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let processors = vec![
            Processor::ProtectedBlocks {
                value: BlockSetRef::Blocks(Vec::new()),
            },
            Processor::Rule {
                rules: vec![ProcessorRule {
                    input_predicate: RuleTest::BlockMatch {
                        block: "minecraft:stone".to_owned(),
                    },
                    location_predicate: RuleTest::AlwaysTrue,
                    position_predicate: PosRuleTest::AlwaysTrue,
                    output_state: state("minecraft:cobblestone"),
                    block_entity_modifier: RuleBlockEntityModifier::Passthrough,
                }],
            },
        ];
        let blocks = vec![
            block((0, 0, 0), "minecraft:stone"),
            block((1, 0, 0), "minecraft:dirt"),
        ];
        let out = process_blocks(
            &processors,
            &blocks,
            &level,
            settings((0, 64, 0), (0, 64, 0), None),
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].state.name, "minecraft:cobblestone");
        assert_eq!(out[1].state.name, "minecraft:dirt");
    }

    /// `axis_aligned_linear_pos` 的距离按轴分量算：`min_dist` 以内用 `min_chance`。
    ///
    /// `axis_aligned_linear_pos` measures distance along the axis; inside `min_dist` the
    /// chance is `min_chance`.
    #[test]
    fn axis_aligned_linear_pos_uses_the_axis_distance() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let processor = Processor::Rule {
            rules: vec![ProcessorRule {
                input_predicate: RuleTest::AlwaysTrue,
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AxisAlignedLinear {
                    min_chance: 1.0,
                    max_chance: 1.0,
                    min_dist: 0,
                    max_dist: 100,
                    axis: "y".to_owned(),
                },
                output_state: state("minecraft:gold_block"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            }],
        };
        // chance 恒 1.0，`nextFloat() <= 1.0` 总是成立，所以无论多远都命中。
        // The chance is always 1.0, so `nextFloat() <= 1.0` always holds.
        let far = ProcessBlock {
            pos: (0, 0, 0),
            world_pos: (0, 200, 0),
            state: state("minecraft:stone"),
            nbt: None,
        };
        let out = process_block(&processor, &far, (0, 0, 0), &level, None).unwrap();
        assert_eq!(out.state.name, "minecraft:gold_block");

        // chance 恒 0.0 -> `nextFloat()` 几乎不会等于 0，所以不命中。
        // The chance is 0.0, so `nextFloat()` almost never equals 0 and nothing matches.
        let processor = Processor::Rule {
            rules: vec![ProcessorRule {
                input_predicate: RuleTest::AlwaysTrue,
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AxisAlignedLinear {
                    min_chance: 0.0,
                    max_chance: 0.0,
                    min_dist: 0,
                    max_dist: 100,
                    axis: "y".to_owned(),
                },
                output_state: state("minecraft:gold_block"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            }],
        };
        let out = process_block(&processor, &far, (0, 0, 0), &level, None).unwrap();
        assert_eq!(out.state.name, "minecraft:stone");
    }

    /// `chunkBB` 只让盒内方块进入处理，除非链里有 `capped`
    /// （`evaluatesEntirePieceState()` 为 true 会关掉过滤）。
    ///
    /// `chunkBB` lets only inside blocks through unless the chain has a `capped`
    /// (whose `evaluatesEntirePieceState()` turns the filter off).
    /// `settings.getRandom(pos)`：给了显式随机源就用它，否则按位置派生。
    /// 用 `block_rot`（每方块只取一次 `nextFloat`）验证两条路径取到不同的值。
    ///
    /// `settings.getRandom(pos)`: the explicit source wins; otherwise it is derived from
    /// the position. `block_rot` (one `nextFloat` per block) shows the two paths differ.
    #[test]
    fn explicit_random_overrides_the_positional_source() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        // `(0,64,0)` 的位置随机源首个 `nextFloat` 是 0.9703，所以 `integrity = 0.95` 会丢弃。
        // The `(0,64,0)` source's first `nextFloat` is 0.9703, so `integrity = 0.95` drops.
        let processor = Processor::BlockRot {
            integrity: 0.95,
            rottable_blocks: None,
        };
        let b = block((0, 64, 0), "minecraft:stone");
        assert!(
            process_block(&processor, &b, (0, 64, 0), &level, None).is_none(),
            "no explicit source: the positional roll discards"
        );

        // 显式随机源的首个 `nextFloat` 取一个确定的小值：换成 seed 0 的 LegacyRandom，
        // 它的首个 `nextFloat` 是 0.7309（< 0.95），所以保留。
        // An explicit source whose first `nextFloat` is small keeps the block.
        let explicit = Random::Legacy(LegacyRandom::new(0));
        assert!(
            process_block(&processor, &b, (0, 64, 0), &level, Some(&explicit)).is_some(),
            "explicit source overrides the positional roll"
        );
    }

    #[test]
    fn chunk_bb_filters_unless_capped_is_present() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let blocks = vec![
            block((0, 0, 0), "minecraft:stone"),
            block((5, 0, 0), "minecraft:stone"),
        ];
        let bb = BoundingBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 0,
            max_y: 0,
            max_z: 0,
        };
        let plain = vec![Processor::ProtectedBlocks {
            value: BlockSetRef::Blocks(Vec::new()),
        }];
        // 没有 capped：只处理盒内那一个。
        // No `capped`: only the inside block is processed.
        let out = process_blocks(
            &plain,
            &blocks,
            &level,
            settings((0, 0, 0), (0, 0, 0), Some(bb)),
        );
        assert_eq!(out.len(), 1);
        // `chunk_bb = None` 时不过滤。
        // `chunk_bb = None` does not filter.
        let out = process_blocks(
            &plain,
            &blocks,
            &level,
            settings((0, 0, 0), (0, 0, 0), None),
        );
        assert_eq!(out.len(), 2);
        // 有 capped（limit 0 也够，因为只看 `evaluatesEntirePieceState`）：不过滤。
        // With a `capped` even at limit 0 (only `evaluatesEntirePieceState` matters): no filter.
        let with_capped = vec![Processor::Capped {
            delegate: Box::new(Processor::ProtectedBlocks {
                value: BlockSetRef::Blocks(Vec::new()),
            }),
            limit: 0,
        }];
        let out = process_blocks(
            &with_capped,
            &blocks,
            &level,
            settings((0, 0, 0), (0, 0, 0), Some(bb)),
        );
        assert_eq!(out.len(), 2);
    }

    /// `block_ignore` 丢弃 `to_ignore` 里的方块，其余保留。
    ///
    /// `block_ignore` drops blocks in `to_ignore` and keeps the rest.
    #[test]
    fn block_ignore_drops_listed_blocks() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let processor = Processor::BlockIgnore {
            to_ignore: vec![
                "minecraft:structure_block".to_owned(),
                "minecraft:air".to_owned(),
            ],
        };
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:structure_block"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:air"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );
        assert!(
            process_block(
                &processor,
                &block((0, 64, 0), "minecraft:stone"),
                (0, 64, 0),
                &level,
                None
            )
            .is_some()
        );
    }

    /// `jigsaw_replacement` 把拼图方块换成 nbt 里的 `final_state`；`structure_void` 丢弃；
    /// 没有 nbt 时保留原方块。
    ///
    /// `jigsaw_replacement` turns a jigsaw into its `final_state` nbt; `structure_void`
    /// drops it; a jigsaw without nbt is kept.
    #[test]
    fn jigsaw_replacement_uses_final_state() {
        use mc_bamboo_nbt::Tag;
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let processor = Processor::JigsawReplacement;
        let with_final = |final_state: &str| ProcessBlock {
            pos: (0, 64, 0),
            world_pos: (0, 64, 0),
            state: state("minecraft:jigsaw"),
            nbt: Some(Tag::Compound(vec![(
                "final_state".to_owned(),
                Tag::String(final_state.to_owned()),
            )])),
        };

        let out = process_block(
            &processor,
            &with_final("minecraft:stone"),
            (0, 64, 0),
            &level,
            None,
        )
        .unwrap();
        assert_eq!(out.state.name, "minecraft:stone");
        assert!(out.nbt.is_none(), "the replacement drops the jigsaw nbt");

        assert!(
            process_block(
                &processor,
                &with_final("minecraft:structure_void"),
                (0, 64, 0),
                &level,
                None
            )
            .is_none()
        );

        // 没有 nbt：保留原方块。
        // No nbt: the block is kept.
        let bare = ProcessBlock {
            pos: (0, 64, 0),
            world_pos: (0, 64, 0),
            state: state("minecraft:jigsaw"),
            nbt: None,
        };
        let out = process_block(&processor, &bare, (0, 64, 0), &level, None).unwrap();
        assert_eq!(out.state.name, "minecraft:jigsaw");

        // 非拼图方块不受影响。
        // A non-jigsaw block is untouched.
        let other = block((0, 64, 0), "minecraft:stone");
        let out = process_block(&processor, &other, (0, 64, 0), &level, None).unwrap();
        assert_eq!(out.state.name, "minecraft:stone");
    }

    /// `gravity` 把 Y 抬到 `height + offset + templateRelativePos.y`。
    ///
    /// `gravity` lifts Y to `height + offset + templateRelativePos.y`.
    #[test]
    fn gravity_lifts_y_to_the_heightmap() {
        /// 高度图恒为 70 的假世界。
        ///
        /// A fake level whose heightmap is always 70.
        struct Height70;
        impl ProcessorLevel for Height70 {
            fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
                state("minecraft:air")
            }
            fn in_block_set(&self, _state: &BlockStateSpec, _set: &BlockSetRef) -> bool {
                false
            }
            fn height(&self, _heightmap: &str, _x: i32, _z: i32) -> Option<i32> {
                Some(70)
            }
        }
        let level = Height70;
        let processor = Processor::Gravity {
            heightmap: "WORLD_SURFACE_WG".to_owned(),
            offset: -1,
        };
        // `templateRelativePos.y` 是 3，高度 70，offset -1 -> 72。
        // `templateRelativePos.y` is 3, height 70, offset -1 -> 72.
        let b = ProcessBlock {
            pos: (5, 3, 7),
            world_pos: (5, 40, 7),
            state: state("minecraft:grass_block"),
            nbt: None,
        };
        let out = process_block(&processor, &b, (0, 0, 0), &level, None).unwrap();
        assert_eq!(out.world_pos, (5, 72, 7));
        // X/Z 不变，状态不变。
        // X/Z and the state are unchanged.
        assert_eq!(out.state.name, "minecraft:grass_block");
    }

    /// `Gravity` 的 `*_WG` 名字会换成正式变体；level 不提供高度图时保留原位置而不是
    /// 算出错误的 Y。
    ///
    /// `Gravity` swaps `*_WG` for the real variant, and keeps the position (instead of
    /// computing a wrong Y) when the level provides no heightmap.
    #[test]
    fn gravity_swaps_the_wg_heightmap_and_degrades_safely() {
        /// 记录被问到的 heightmap 名字。
        ///
        /// Records which heightmap names it was asked for.
        struct Recorder {
            asked: std::cell::RefCell<Vec<String>>,
            answer: Option<i32>,
        }
        impl ProcessorLevel for Recorder {
            fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
                state("minecraft:air")
            }
            fn in_block_set(&self, _state: &BlockStateSpec, _set: &BlockSetRef) -> bool {
                false
            }
            fn height(&self, heightmap: &str, _x: i32, _z: i32) -> Option<i32> {
                self.asked.borrow_mut().push(heightmap.to_owned());
                self.answer
            }
        }

        let b = ProcessBlock {
            pos: (0, 2, 0),
            world_pos: (0, 50, 0),
            state: state("minecraft:stone"),
            nbt: None,
        };

        // `WORLD_SURFACE_WG` 会被换成 `WORLD_SURFACE`。
        // `WORLD_SURFACE_WG` is swapped for `WORLD_SURFACE`.
        let level = Recorder {
            asked: std::cell::RefCell::new(Vec::new()),
            answer: Some(80),
        };
        let processor = Processor::Gravity {
            heightmap: "WORLD_SURFACE_WG".to_owned(),
            offset: 0,
        };
        let out = process_block(&processor, &b, (0, 0, 0), &level, None).unwrap();
        assert_eq!(out.world_pos.1, 82, "80 + 2");
        assert_eq!(level.asked.borrow().as_slice(), ["WORLD_SURFACE"]);

        // level 不给高度图：保留原位置。
        // No heightmap: the position is kept.
        let level = Recorder {
            asked: std::cell::RefCell::new(Vec::new()),
            answer: None,
        };
        let out = process_block(&processor, &b, (0, 0, 0), &level, None).unwrap();
        assert_eq!(
            out.world_pos,
            (0, 50, 0),
            "position kept when height is unknown"
        );
    }

    /// `RuleBlockEntityModifier.apply`：`append_loot` 写 `LootTable` + 取一次 `nextLong`；
    /// `clear` 清空；`passthrough` 原样返回；`append_static` 递归合并。
    ///
    /// `RuleBlockEntityModifier.apply`: `append_loot` writes `LootTable` plus one `nextLong`,
    /// `clear` empties, `passthrough` returns the input and `append_static` merges recursively.
    #[test]
    fn block_entity_modifiers_produce_the_vanilla_tags() {
        use crate::structure::processor::RuleBlockEntityModifier;
        use mc_bamboo_blocks::LegacyRandom;
        use mc_bamboo_nbt::Tag;

        let random = Random::Legacy(LegacyRandom::new(7));
        let existing = Tag::Compound(vec![
            ("Keep".to_owned(), Tag::Int(5)),
            (
                "LootTable".to_owned(),
                Tag::String("minecraft:old".to_owned()),
            ),
        ]);

        // `passthrough` 不改。
        // `passthrough` changes nothing.
        assert_eq!(
            RuleBlockEntityModifier::Passthrough
                .apply(&random, Some(&existing))
                .unwrap(),
            Some(existing.clone())
        );
        // 现有的 nbt 是 `None` 时 `passthrough` 仍返回 `None`。
        // With no existing nbt, `passthrough` still returns `None`.
        assert_eq!(
            RuleBlockEntityModifier::Passthrough
                .apply(&random, None)
                .unwrap(),
            None
        );

        // `clear` 得到空复合。
        // `clear` yields an empty compound.
        assert_eq!(
            RuleBlockEntityModifier::Clear
                .apply(&random, Some(&existing))
                .unwrap(),
            Some(Tag::Compound(Vec::new()))
        );

        // `append_loot`：保留 `Keep`、覆盖 `LootTable`、加 `LootTableSeed`。
        let appended = RuleBlockEntityModifier::AppendLoot {
            loot_table: "minecraft:archaeology/trail_ruins_common".to_owned(),
        }
        .apply(&random, Some(&existing))
        .unwrap()
        .unwrap();
        let Tag::Compound(entries) = &appended else {
            panic!("append_loot must yield a compound");
        };
        let get = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(get("Keep"), Some(Tag::Int(5)));
        assert_eq!(
            get("LootTable"),
            Some(Tag::String(
                "minecraft:archaeology/trail_ruins_common".to_owned()
            ))
        );
        assert!(
            matches!(get("LootTableSeed"), Some(Tag::Long(_))),
            "append_loot must write a long seed"
        );
        // 同一个 `random` 连续取两次应得到不同的种子（证明它真的取数了）。
        // Asking again must draw a different seed, proving it really draws.
        let again = RuleBlockEntityModifier::AppendLoot {
            loot_table: "minecraft:x".to_owned(),
        }
        .apply(&random, None)
        .unwrap()
        .unwrap();
        assert!(
            matches!(&again, Tag::Compound(_)),
            "append_loot must yield a compound"
        );
        let seed = |tag: &Tag| match tag {
            Tag::Compound(entries) => entries
                .iter()
                .find(|(name, _)| name == "LootTableSeed")
                .map(|(_, value)| value.clone()),
            _ => None,
        };
        assert_ne!(seed(&appended), seed(&again), "consecutive draws differ");

        // `append_static` 递归合并：同名复合合并，标量覆盖。
        // `append_static` merges recursively: same-name compounds merge, scalars overwrite.
        let merged = RuleBlockEntityModifier::AppendStatic {
            data: Tag::Compound(vec![
                ("Keep".to_owned(), Tag::Int(9)),
                (
                    "Nested".to_owned(),
                    Tag::Compound(vec![("A".to_owned(), Tag::Int(1))]),
                ),
            ]),
        }
        .apply(
            &random,
            Some(&Tag::Compound(vec![
                (
                    "Nested".to_owned(),
                    Tag::Compound(vec![("B".to_owned(), Tag::Int(2))]),
                ),
                ("Only".to_owned(), Tag::Int(3)),
            ])),
        )
        .unwrap()
        .unwrap();
        let Tag::Compound(entries) = &merged else {
            panic!("append_static must yield a compound");
        };
        let nested = entries
            .iter()
            .find(|(name, _)| name == "Nested")
            .map(|(_, value)| value.clone());
        // 嵌套里同时有 A 和 B。
        // The nested compound holds both A and B.
        assert_eq!(
            nested,
            Some(Tag::Compound(vec![
                ("B".to_owned(), Tag::Int(2)),
                ("A".to_owned(), Tag::Int(1)),
            ]))
        );
        assert!(
            entries.iter().any(|(name, _)| name == "Only"),
            "append_static must keep untouched keys"
        );
    }

    /// `capped` 最多改 `limit` 个方块，且逐块用 delegate 的效果。
    ///
    /// `capped` alters at most `limit` blocks using the delegate's effect.
    #[test]
    fn capped_respects_the_limit() {
        let level = OneState {
            state: state("minecraft:air"),
            tag_blocks: Vec::new(),
        };
        let delegate = Processor::Rule {
            rules: vec![ProcessorRule {
                input_predicate: RuleTest::AlwaysTrue,
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AlwaysTrue,
                output_state: state("minecraft:gold_block"),
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            }],
        };
        let processors = vec![Processor::Capped {
            delegate: Box::new(delegate),
            limit: 2,
        }];
        let blocks: Vec<ProcessBlock> = (0..6)
            .map(|i| block((i, 0, 0), "minecraft:stone"))
            .collect();
        let out = process_blocks(
            &processors,
            &blocks,
            &level,
            settings((0, 64, 0), (0, 64, 0), None),
        );
        let changed = out
            .iter()
            .filter(|b| b.state.name == "minecraft:gold_block")
            .count();
        assert_eq!(changed, 2);
    }
}
