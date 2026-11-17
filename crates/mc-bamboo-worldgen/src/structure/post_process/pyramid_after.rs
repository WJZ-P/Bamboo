//! `DesertPyramidStructure.afterPlace`：可疑的沙子与塌陷屋顶。
//!
//! 字节码（`DesertPyramidStructure.afterPlace`）：
//! * 把每个 `DesertPyramidPiece.getPotentialSuspiciousSandWorldPositions()` 放进
//!   `SortedArraySet.create(Vec3i::compareTo)`（按 y、z、x 排序并去重）；
//! * 对每个件先 `placeSuspiciousSand(box, level, piece.getRandomCollapsedRoofPos())`；
//! * 用 `RandomSource.createThreadLocalInstance(level.getSeed()).forkPositional()
//!   .at(PiecesContainer.calculateBoundingBox().getCenter())` 造位置随机源，
//!   `Util.shuffle` 洗牌后 `nextIntBetweenInclusive(5,8)` 取数量 `n`；
//! * 依次：前 `n` 个位置写 `suspicious_sand`（带战利品表），其余若是
//!   `box.isInside(pos)` 就写 `sand`。
//!
//! `DesertPyramidStructure.afterPlace`: suspicious sand and the collapsed roof.
//! It runs after all pieces' `postProcess` with the same random sources, which is
//! why only the pyramid's start chunk (where the cellar positions fall inside the
//! writable area) used to differ.

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_data::blocks;

use crate::{feature::world::FeatureWorld, random::positional_seed};

type Pos = (i32, i32, i32);

/// `postProcess` 期间收集的数据，交给 `afterPlace`。
/// Data collected during `postProcess` and consumed by `afterPlace`.
#[derive(Debug, Default)]
pub struct PyramidAfterData {
    /// `placeSandBox` 记下的世界坐标（y、x、z 顺序）。
    pub potential_sand: Vec<Pos>,
    /// `getRandomCollapsedRoofPos()`。
    pub collapsed_roof_pos: Option<Pos>,
    /// `StructureStart.placeInChunk` 之后整份结构的包围盒（用于位置随机源中心）。
    pub structure_bb: Option<[i32; 6]>,
}

/// `SingleThreadedRandomSource(long seed).forkPositional()` 的等价物。
///
/// `SingleThreadedRandomSource` 用和 `LegacyRandomSource` 一样的 LCG，
/// `forkPositional()` 取一次 `nextLong()` 当 `LegacyPositionalRandomFactory` 的种子，
/// `at(x,y,z)` 再 `Mth.getSeed(x,y,z) ^ factorySeed`。
///
/// Equivalent of `SingleThreadedRandomSource(seed).forkPositional()`: the source
/// uses the same LCG as `LegacyRandomSource`, `forkPositional()` draws one
/// `nextLong()`, and `at` xors `Mth.getSeed(x,y,z)` with that factory seed.
fn positional_from_world_seed(world_seed: i64, at: Pos) -> LegacyRandom {
    let source = LegacyRandom::new(world_seed);
    let factory_seed = source.next_long();
    LegacyRandom::new(positional_seed(at.0, at.1, at.2) ^ factory_seed)
}

/// `DesertPyramidStructure.afterPlace`。
pub fn after_place(
    world: &mut FeatureWorld<'_>,
    world_seed: i64,
    chunk_bb: &[i32; 6],
    data: &PyramidAfterData,
) {
    // `SortedArraySet` 按 `Vec3i.compareTo`（y、z、x）排序去重。
    let mut positions = data.potential_sand.clone();
    positions.sort_by_key(|a| (a.1, a.2, a.0));
    positions.dedup();

    // 每个件先放塌陷屋顶那个可疑沙子（只在 box 内写）。
    if let Some(pos) = data.collapsed_roof_pos {
        place_suspicious_sand(world, chunk_bb, pos);
    }

    let Some(bb) = data.structure_bb else {
        return;
    };
    // `BoundingBox.getCenter()`：minX + (maxX - minX + 1)/2，y / z 同理。
    let center = (
        bb[0] + (bb[3] - bb[0] + 1) / 2,
        bb[1] + (bb[4] - bb[1] + 1) / 2,
        bb[2] + (bb[5] - bb[2] + 1) / 2,
    );
    let random = positional_from_world_seed(world_seed, center);
    // `Util.shuffle`：i 从 size 递减到 2，`swap(i-1, nextInt(i))`。
    for i in (2..=positions.len()).rev() {
        let j = random.next_int(i as i32) as usize;
        positions.swap(i - 1, j);
    }
    // 官方调的是 `RandomSource.nextInt(5, 8)`：上界**不含**，结果是 5..=7。
    // `bites`: vanilla calls `RandomSource.nextInt(5, 8)` with an exclusive bound (5..=7).
    let count = (random.next_int(3) + 5).max(0) as usize;
    let mut remaining = count.min(positions.len());
    for pos in positions {
        if remaining > 0 {
            remaining -= 1;
            place_suspicious_sand(world, chunk_bb, pos);
        } else if crate::structure::pieces::intersects(
            &[pos.0, pos.1, pos.2, pos.0, pos.1, pos.2],
            chunk_bb,
        ) {
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, blocks::SAND);
        }
    }
}

/// `placeSuspiciousSand`：只在 box 内写 `suspicious_sand`，并给刷扫方块实体设
/// `DESERT_PYRAMID_ARCHAEOLOGY` 与 `pos.asLong()` 作种子（字节码里的 lambda 参数）。
///
/// `placeSuspiciousSand`: write `suspicious_sand` inside the box, then set the
/// brushable block entity's loot table to `DESERT_PYRAMID_ARCHAEOLOGY` with
/// `pos.asLong()` as the seed (the lambda's argument in the bytecode).
fn place_suspicious_sand(world: &mut FeatureWorld<'_>, chunk_bb: &[i32; 6], pos: Pos) {
    if !crate::structure::pieces::intersects(&[pos.0, pos.1, pos.2, pos.0, pos.1, pos.2], chunk_bb)
    {
        return;
    }
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, blocks::SUSPICIOUS_SAND);
    let seed = crate::sampler::block_pos_as_long(pos.0, pos.1, pos.2);
    let nbt = mc_bamboo_nbt::Tag::Compound(vec![
        (
            "LootTable".to_owned(),
            mc_bamboo_nbt::Tag::String("minecraft:archaeology/desert_pyramid".to_owned()),
        ),
        ("LootTableSeed".to_owned(), mc_bamboo_nbt::Tag::Long(seed)),
    ]);
    world.set_block_entity(pos.0, pos.1, pos.2, &nbt);
}
