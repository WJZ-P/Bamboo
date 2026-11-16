//! 丛林神殿（`JungleTemplePiece.postProcess`）的方块部分。
//!
//! 官方先 `updateAverageGroundHeight(level, chunkBB, 0)`，再按固定顺序摆墙体、地板、
//! 楼梯、两个陷阱的红石机关和两只箱子。所有 `generateBox(..., STONE_SELECTOR)` 走
//! `MossStoneSelector`：每格取一次 `postProcess` 随机源的 `nextFloat()`，
//! `< 0.4` 放圆石，否则放苔石；循环顺序是官方 `y → x → z`。
//!
//! 指令表由 `mc-bamboo-vanilla generate` / `extract` 跑官方
//! `JungleTemplePiece.postProcess` 的 `generateBox` / `generateAirBox` / `placeBlock` /
//! `createChest` / `createDispenser` 调用序列导出，写进同目录的
//! `jungle_temple_ops.rs`（不要手动改）；`StructurePiecePlaceOracle.java` 负责逐方块对拍。
//!
//! The block part of `JungleTemplePiece.postProcess`. Vanilla first runs
//! `updateAverageGroundHeight(level, chunkBB, 0)`, then lays out walls, floors, stairs,
//! the two redstone traps and the two chests. Every `generateBox(..., STONE_SELECTOR)`
//! uses `MossStoneSelector`: one `nextFloat()` per cell from the postProcess random,
//! cobblestone below `0.4` and mossy cobblestone otherwise, in vanilla's `y → x → z`
//! order. The instruction table is generated from the official helper-call sequence by
//! `mc-bamboo-vanilla generate` / `extract` and lives in `jungle_temple_ops.rs`.

use mc_bamboo_data::blocks;

use crate::{
    feature::world::FeatureWorld,
    random::WorldgenRandom,
    structure::{
        place::{PieceContext, WriteLog},
        post_process::create_chest,
        scattered::average_ground_height,
    },
};

/// 一条绘制指令。
///
/// One paint instruction.
enum Op {
    /// `generateBox(..., skipAir, random, STONE_SELECTOR)`。
    Moss {
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        skip_air: bool,
    },
    /// `generateAirBox(from, to)`。
    Air {
        from: (i32, i32, i32),
        to: (i32, i32, i32),
    },
    /// `placeBlock(state, x, y, z)`。
    One { state: u32, at: (i32, i32, i32) },
    /// `createChest(level, chunkBB, random, x, y, z, table)`。
    Chest {
        at: (i32, i32, i32),
        table: &'static str,
    },
    /// `createDispenser(level, chunkBB, random, x, y, z, facing, table)`。
    Dispenser {
        at: (i32, i32, i32),
        facing: &'static str,
        table: &'static str,
    },
}

/// 按属性名/值列表设方块状态；属性缺失时报错（数据版本变更立刻暴露）。
///
/// Set a block state from property pairs; a missing property is an error.
fn state_with(state: u32, props: &[(&str, &str)]) -> u32 {
    let mut result = state;
    for &(name, value) in props {
        result = crate::feature::state::with_property(result, name, value)
            .unwrap_or_else(|| panic!("missing block property {name}={value}"));
    }
    result
}

/// `MossStoneSelector.next`：每格一次 `nextFloat()`。
///
/// `MossStoneSelector.next`: one `nextFloat()` per cell.
fn moss_selector(random: &WorldgenRandom) -> u32 {
    if random.next_float() < 0.4 {
        blocks::COBBLESTONE
    } else {
        blocks::MOSSY_COBBLESTONE
    }
}

include!("jungle_temple_ops.rs");

/// `JungleTemplePiece.postProcess` 的方块部分。
///
/// `random` 是 `applyBiomeDecoration` 的 `setFeatureSeed` 随机源，供
/// `MossStoneSelector`、箱子和发射器的战利品表种子取数。
///
/// The block part of `JungleTemplePiece.postProcess`. `random` is
/// `applyBiomeDecoration`'s `setFeatureSeed` source, used by `MossStoneSelector` and
/// the chest/dispenser loot table seeds.
#[allow(clippy::too_many_arguments)]
pub fn jungle_temple(
    world: &mut FeatureWorld<'_>,
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    orientation: Option<crate::structure::pieces::HDir>,
    rotation: i32,
    mirror: i32,
    random: &WorldgenRandom,
    writes: WriteLog,
) -> crate::structure::place::Placed {
    let Some((height_position, new_bb)) = average_ground_height(world, piece_bb, chunk_bb, 0)
    else {
        return (None, writes);
    };
    let mut ctx = PieceContext {
        world,
        chunk_bb: *chunk_bb,
        piece_bb: new_bb,
        orientation,
        rotation,
        mirror,
        writes,
    };
    for op in ops() {
        match op {
            Op::Moss { from, to, skip_air } => {
                let (x0, y0, z0) = from;
                let (x1, y1, z1) = to;
                // 官方 `generateBox(..., skipAir, random, selector)` 的三重循环顺序。
                // Vanilla `generateBox(..., skipAir, random, selector)` loop order.
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        for z in z0..=z1 {
                            if skip_air && ctx.block(x, y, z) == blocks::AIR {
                                continue;
                            }
                            let state = moss_selector(random);
                            ctx.place_block(state, x, y, z);
                        }
                    }
                }
            }
            Op::Air { from, to } => ctx.generate_air_box(from, to),
            Op::One { state, at } => ctx.place_block(state, at.0, at.1, at.2),
            Op::Chest { at, table } => {
                let pos = ctx.world_pos(at.0, at.1, at.2);
                if let Some(state) =
                    create_chest(ctx.world, &ctx.chunk_bb, pos, None, random, Some(table))
                    && let Some(log) = ctx.writes.as_mut()
                {
                    log.push((pos.0, pos.1, pos.2, state));
                }
            }
            Op::Dispenser { at, facing, table } => {
                // `createDispenser` 内部走 `placeBlock`（状态按 mirror / rotation 变换），
                // 随后拿发射器方块实体取一次 `nextLong()` 当战利品表种子。
                // Vanilla `createDispenser` goes through `placeBlock` (state transformed by
                // mirror / rotation), then draws one `nextLong()` for the loot table.
                let state = state_with(
                    blocks::DISPENSER,
                    &[("facing", facing), ("triggered", "false")],
                );
                ctx.place_block(state, at.0, at.1, at.2);
                let pos = ctx.world_pos(at.0, at.1, at.2);
                let seed = random.next_long();
                let nbt = mc_bamboo_nbt::Tag::Compound(vec![
                    (
                        "LootTable".to_owned(),
                        mc_bamboo_nbt::Tag::String(table.to_owned()),
                    ),
                    ("LootTableSeed".to_owned(), mc_bamboo_nbt::Tag::Long(seed)),
                ]);
                ctx.world.set_block_entity(pos.0, pos.1, pos.2, &nbt);
            }
        }
    }
    (Some((height_position, new_bb)), ctx.writes.take())
}
