//! `EnderEyeItem.useOn`：把末影之眼装进末地传送门框架，一圈框架都装满时生成传送门。
//!
//! 官方装眼时会把站在框架上的实体往上推，这里没有实体。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::blocks;

use crate::{
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, EYE, HORIZONTAL_FACING},
    world::{UPDATE_CLIENTS, UPDATE_LIMIT, World},
};

/// `EndPortalFrameBlock.getOrCreatePortalShape` 的图样，只有一层。一行从左到右，行从上到下。
/// `?` 什么都行，箭头是装了眼、朝某个方向的框架，见 [`frame_facing`]。
const PORTAL_PATTERN: [&[u8; 5]; 5] = [b"?vvv?", b">???<", b">???<", b">???<", b"?^^^?"];

/// `BlockPattern.find` 从点中的方块往三个正方向找图样的角，范围是图样最长的一边。
const SEARCH_SIZE: i32 = 5;

/// `useOn`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    let state = world.block_state(pos);
    if !state.is(blocks::END_PORTAL_FRAME) || state.get(EYE) {
        return InteractionResult::Pass;
    }
    world.set_block(pos, state.with(EYE, true), UPDATE_CLIENTS);
    world.update_neighbour_for_output_signal(pos, state.block());
    world.level_event(1503, pos, 0);
    if let Some(corner) = find_portal_frame(world, pos) {
        fill_portal(world, corner.offset(-3, 0, -3));
    }
    InteractionResult::Success
}

/// 官方按 `getFrontTopLeft` 往西北偏三格填一块 3×3，竖着的一圈框架也照样这么填。
fn fill_portal(world: &mut World, start: BlockPos) {
    for x in 0..3 {
        for z in 0..3 {
            let pos = start.offset(x, 0, z);
            world.destroy_block(pos, UPDATE_LIMIT);
            world.set_block(pos, BlockState(blocks::END_PORTAL), UPDATE_CLIENTS);
        }
    }
}

/// `BlockPattern.find`：图样对上时返回它的 `frontTopLeft`。
fn find_portal_frame(world: &World, origin: BlockPos) -> Option<BlockPos> {
    search_area(origin).find(|&corner| {
        orientations().any(|(forwards, up)| matches_at(world, corner, forwards, up))
    })
}

/// `BlockPos.betweenClosed` 的顺序：x 变化最快，然后 y，最后 z。
fn search_area(origin: BlockPos) -> impl Iterator<Item = BlockPos> {
    (0..SEARCH_SIZE).flat_map(move |z| {
        (0..SEARCH_SIZE).flat_map(move |y| (0..SEARCH_SIZE).map(move |x| origin.offset(x, y, z)))
    })
}

/// `find` 试的朝向：`up` 不能和 `forwards` 在同一根轴上。
fn orientations() -> impl Iterator<Item = (Direction, Direction)> {
    Direction::ALL.into_iter().flat_map(|forwards| {
        Direction::ALL
            .into_iter()
            .filter(move |up| up.axis() != forwards.axis())
            .map(move |up| (forwards, up))
    })
}

/// `matches`：图样的每一格都对上。
fn matches_at(world: &World, corner: BlockPos, forwards: Direction, up: Direction) -> bool {
    PORTAL_PATTERN.iter().zip(0..).all(|(row, down)| {
        row.iter().zip(0..).all(|(&symbol, right)| {
            let pos = translate(corner, forwards, up, right, down);
            frame_facing(symbol)
                .is_none_or(|facing| is_filled_frame(world.block_state(pos), facing))
        })
    })
}

/// 图样里箭头要求的框架朝向，`?` 是 `None`。
fn frame_facing(symbol: u8) -> Option<Direction> {
    match symbol {
        b'^' => Some(Direction::South),
        b'>' => Some(Direction::West),
        b'v' => Some(Direction::North),
        b'<' => Some(Direction::East),
        _ => None,
    }
}

fn is_filled_frame(state: BlockState, facing: Direction) -> bool {
    state.is(blocks::END_PORTAL_FRAME) && state.get(EYE) && state.get(HORIZONTAL_FACING) == facing
}

/// `translateAndRotate`：图样只有一层，沿 `forwards` 的深度是 0。右手方向是 `forwards` 叉乘 `up`。
fn translate(
    corner: BlockPos,
    forwards: Direction,
    up: Direction,
    right: i32,
    down: i32,
) -> BlockPos {
    let (fx, fy, fz) = forwards.normal();
    let (ux, uy, uz) = up.normal();
    let (rx, ry, rz) = (fy * uz - fz * uy, fz * ux - fx * uz, fx * uy - fy * ux);
    corner.offset(
        rx * right - ux * down,
        ry * right - uy * down,
        rz * right - uz * down,
    )
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        level::Effect,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    /// 平放的一圈框架，中间 3×3 从 (1, 64, 1) 到 (3, 64, 3)，都朝里。
    fn ring(level: &mut TestLevel, eyes: bool) -> Vec<BlockPos> {
        let frame = |facing| {
            BlockState(blocks::END_PORTAL_FRAME)
                .with(HORIZONTAL_FACING, facing)
                .with(EYE, eyes)
        };
        let mut placed = Vec::new();
        for i in 1..=3 {
            for (pos, facing) in [
                (BlockPos::new(i, 64, 0), Direction::South),
                (BlockPos::new(i, 64, 4), Direction::North),
                (BlockPos::new(0, 64, i), Direction::East),
                (BlockPos::new(4, 64, i), Direction::West),
            ] {
                level.set(pos, frame(facing));
                placed.push(pos);
            }
        }
        placed
    }

    #[test]
    fn the_twelfth_eye_opens_the_portal() {
        let mut level = TestLevel::new();
        let frames = ring(&mut level, true);
        let last = frames[5];
        level.set(last, level.get(last).with(EYE, false));
        level.set(BlockPos::new(2, 64, 2), BlockState(blocks::STONE));

        let eye = actor_holding(items::ENDER_EYE);
        let outcome = use_item(&mut level, &eye, top_hit(last));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(last).get(EYE));
        for x in 1..=3 {
            for z in 1..=3 {
                assert!(level.get(BlockPos::new(x, 64, z)).is(blocks::END_PORTAL));
            }
        }
        // 官方先报 1503，再填传送门，中间的石英方块在这一步被替换掉。
        assert_eq!(
            outcome.effects.first(),
            Some(&Effect::LevelEvent {
                event: 1503,
                pos: last,
                data: 0,
                except_initiator: false,
            })
        );
        assert_eq!(
            outcome.effects.len(),
            2,
            "the stone in the middle is destroyed"
        );
    }

    #[test]
    fn an_incomplete_ring_only_gets_the_eye() {
        let mut level = TestLevel::new();
        let frames = ring(&mut level, false);
        let eye = actor_holding(items::ENDER_EYE);
        assert_eq!(
            use_item(&mut level, &eye, top_hit(frames[0])).result,
            InteractionResult::Success
        );
        assert!(level.get(BlockPos::new(2, 64, 2)).is_air());
        assert_eq!(
            use_item(&mut level, &eye, top_hit(frames[0])).result,
            InteractionResult::Pass
        );
    }
}
