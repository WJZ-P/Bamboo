//! 物品放置方块，对应官方的 `BlockItem` 和它的子类。
//!
//! 官方每个物品类覆盖 `BlockItem` 的不同步骤，这里用 [`ItemKind`] 在这几个步骤上分支：
//!
//! | 官方类 | 不同的步骤 |
//! |---|---|
//! | `StandingAndWallBlockItem` 和子类 | `getPlacementState` |
//! | `HangingSignItem` | `canPlace` 多一个判定 |
//! | `DoubleHighBlockItem` | `placeBlock` 先清空上方一格 |
//! | `PlaceOnWaterBlockItem` | `useOn` 什么也不做，放置走 `use` |
//! | `GameMasterBlockItem` | `getPlacementState` 要权限 |
//! | `ScaffoldingBlockItem` | `updatePlacementContext`、`mustSurvive` |
//! | `SolidBucketItem` | `useOn` 之后换成空桶，创造模式不换 |

use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::{Item, ItemKind, blocks};

use crate::{
    behaviour::{
        behaviour_of,
        common::{SCAFFOLDING_MAX_DISTANCE, block_by_id, item_of, scaffolding_distance},
        state_for_placement,
    },
    context::{Actor, BlockHit, PlaceContext, UseContext},
    interact::InteractionResult,
    shape::CollisionContext,
    state::BlockState,
    world::{UPDATE_ALL_IMMEDIATE, UPDATE_KNOWN_SHAPE, World},
};

/// 脚手架横着最多长这么远。
const SCAFFOLDING_MAX_HORIZONTAL: i32 = 7;

/// `Item.useOn`：这个 crate 只认识方块物品，别的物品什么也不做。
pub(crate) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    match item_of(ctx.item) {
        Some(item) if !matches!(item.kind, ItemKind::Other | ItemKind::PlaceOnWater) => {
            block_item_use_on(world, item, *ctx)
        }
        _ => InteractionResult::Pass,
    }
}

/// `Item.use` 里方块物品的部分：只有 `PlaceOnWaterBlockItem` 用它放置。
pub(crate) fn use_in_air(world: &mut World, actor: &Actor, hand: Hand) -> InteractionResult {
    let Some(item) = item_of(actor.item_in(hand)) else {
        return InteractionResult::Pass;
    };
    if item.kind != ItemKind::PlaceOnWater {
        return InteractionResult::Pass;
    }
    let from = actor.eye_position();
    let view = Vec3::view_vector(actor.yaw, actor.pitch);
    let to = from.add(view.scale(actor.block_interaction_range));
    let hit = world.clip_source_fluids(from, to);
    let above = BlockHit {
        pos: hit.pos.above(),
        ..hit
    };
    block_item_use_on(world, item, UseContext::new(actor, hand, above))
}

/// 方块物品放置的方块。
fn block_of(item: &Item) -> &'static mc_bamboo_data::Block {
    block_by_id(item.block.expect("这里只处理方块物品"))
}

/// `BlockItem.useOn`。
fn block_item_use_on(world: &mut World, item: &Item, ctx: UseContext) -> InteractionResult {
    let place_ctx = PlaceContext::new(ctx, world);
    let result = place(world, item, place_ctx);
    if result.consumes_action() {
        return result;
    }
    // 官方放置失败后的 `Item.use`：能吃的方块物品开始吃。创造模式总能吃，结果是 `CONSUME`。
    if item.consumable {
        return InteractionResult::Consume;
    }
    result
}

/// `BlockItem.place`。
fn place(world: &mut World, item: &Item, ctx: PlaceContext) -> InteractionResult {
    if !ctx.can_place(world) {
        return InteractionResult::Fail;
    }
    let Some(ctx) = update_placement_context(world, item, ctx) else {
        return InteractionResult::Fail;
    };
    let Some(state) = placement_state(world, item, &ctx) else {
        return InteractionResult::Fail;
    };
    if !place_block(world, item, &ctx, state) {
        return InteractionResult::Fail;
    }

    let pos = ctx.clicked_pos();
    let placed = world.block_state(pos);
    if placed.is_same_block(state) {
        // `updateBlockEntityComponents` 的 `setChanged`：没有模拟输出的方块也会告诉比较器。
        if placed.has_block_entity() {
            world.update_neighbour_for_output_signal(pos, placed.block());
        }
        placed.set_placed_by(world, pos, ctx.actor);
    }
    InteractionResult::Success
}

/// `BlockItem.updatePlacementContext`。
fn update_placement_context<'a>(
    world: &World,
    item: &Item,
    ctx: PlaceContext<'a>,
) -> Option<PlaceContext<'a>> {
    if item.kind == ItemKind::Scaffolding {
        scaffolding_placement_context(world, ctx)
    } else {
        Some(ctx)
    }
}

/// `ScaffoldingBlockItem.updatePlacementContext`：点在脚手架上时，新的一格长在这一串的末端。
fn scaffolding_placement_context<'a>(
    world: &World,
    ctx: PlaceContext<'a>,
) -> Option<PlaceContext<'a>> {
    let pos = ctx.clicked_pos();
    if !world.block_state(pos).is(blocks::SCAFFOLDING) {
        let reachable = scaffolding_distance(world, pos) != SCAFFOLDING_MAX_DISTANCE;
        return reachable.then_some(ctx);
    }

    let direction = scaffolding_growth_direction(&ctx);
    let mut horizontal_distance = 0;
    let mut placement_pos = pos.relative(direction);
    while horizontal_distance < SCAFFOLDING_MAX_HORIZONTAL {
        // 官方在这里给太高的情况发提示，提示不影响世界。
        if !world.is_in_world_bounds(placement_pos) {
            return None;
        }
        let replaced = world.block_state(placement_pos);
        if !replaced.is(blocks::SCAFFOLDING) {
            return replaced
                .can_be_replaced(&ctx, world)
                .then(|| ctx.at(placement_pos, direction, world));
        }
        placement_pos = placement_pos.relative(direction);
        if direction.axis().is_horizontal() {
            horizontal_distance += 1;
        }
    }
    None
}

fn scaffolding_growth_direction(ctx: &PlaceContext) -> Direction {
    if ctx.is_secondary_use_active() {
        if ctx.is_inside() {
            ctx.clicked_face().opposite()
        } else {
            ctx.clicked_face()
        }
    } else if ctx.clicked_face() == Direction::Up {
        ctx.horizontal_direction()
    } else {
        Direction::Up
    }
}

/// `BlockItem.getPlacementState`。
fn placement_state(world: &mut World, item: &Item, ctx: &PlaceContext) -> Option<BlockState> {
    match item.kind {
        ItemKind::StandingAndWall
        | ItemKind::Banner
        | ItemKind::HangingSign
        | ItemKind::PlayerHead => standing_or_wall_state(world, item, ctx),
        // Bamboo 没有权限系统，官方的 `canUseGameMasterBlocks` 一律是假。
        ItemKind::GameMaster => None,
        _ => {
            let block = block_of(item);
            behaviour_of(block.class).before_placement(block, ctx, world);
            let state = state_for_placement(block, ctx, world)?;
            can_place(world, item, ctx, state).then_some(state)
        }
    }
}

/// `BlockItem.canPlace`。
fn can_place(world: &World, item: &Item, ctx: &PlaceContext, state: BlockState) -> bool {
    let pos = ctx.clicked_pos();
    // `ScaffoldingBlockItem.mustSurvive` 是假：能放就放，放不住的下一刻自己掉下去。
    let must_survive = item.kind != ItemKind::Scaffolding;
    (!must_survive || state.can_survive(world, pos))
        && world.is_unobstructed(state, pos, CollisionContext::Placement)
}

/// `StandingAndWallBlockItem.getPlacementState`：按视线找第一个放得住的朝向。
fn standing_or_wall_state(world: &World, item: &Item, ctx: &PlaceContext) -> Option<BlockState> {
    let wall_block = block_by_id(item.wall_block.expect("这种物品一定有挂墙变体"));
    let attachment = Direction::from_id(item.attachment.expect("这种物品一定有贴附方向"));
    let wall_state = state_for_placement(wall_block, ctx, world);
    let pos = ctx.clicked_pos();

    let state = ctx
        .nearest_looking_directions()
        .into_iter()
        .filter(|&direction| direction != attachment.opposite())
        .filter_map(|direction| {
            if direction == attachment {
                state_for_placement(block_of(item), ctx, world)
            } else {
                wall_state
            }
        })
        .find(|&candidate| can_place_on_wall_item(world, item, candidate, pos))?;
    world
        .is_unobstructed(state, pos, CollisionContext::Empty)
        .then_some(state)
}

/// `StandingAndWallBlockItem.canPlace`。
fn can_place_on_wall_item(world: &World, item: &Item, state: BlockState, pos: BlockPos) -> bool {
    if item.kind == ItemKind::HangingSign
        && !state
            .behaviour()
            .can_place_from_hanging_sign_item(state, world, pos)
    {
        return false;
    }
    state.can_survive(world, pos)
}

/// `BlockItem.placeBlock`。
fn place_block(world: &mut World, item: &Item, ctx: &PlaceContext, state: BlockState) -> bool {
    let pos = ctx.clicked_pos();
    if item.kind == ItemKind::DoubleHigh {
        clear_above(world, pos);
    }
    world.set_block(pos, state, UPDATE_ALL_IMMEDIATE)
}

/// `DoubleHighBlockItem.placeBlock`：先把上方一格清空，放下半时 `setPlacedBy` 才放得进上半。
fn clear_above(world: &mut World, pos: BlockPos) {
    let above = pos.above();
    let cleared = if world.is_water_at(above) {
        BlockState::WATER
    } else {
        BlockState::AIR
    };
    world.set_block(above, cleared, UPDATE_ALL_IMMEDIATE | UPDATE_KNOWN_SHAPE);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockTicks,
        state::{SLAB_TYPE, SlabType, WATERLOGGED},
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);

    fn level() -> TestLevel {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level
    }

    #[test]
    fn plain_block_goes_next_to_the_clicked_face() {
        let mut level = level();
        let outcome = use_item(&mut level, &actor_holding(items::DIRT), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR.above()), BlockState(blocks::DIRT));
    }

    #[test]
    fn replaceable_block_is_replaced_in_place() {
        let mut level = level();
        let grass = FLOOR.above();
        level.set(grass, BlockState(blocks::SHORT_GRASS));
        use_item(&mut level, &actor_holding(items::DIRT), top_hit(grass));
        assert_eq!(level.get(grass), BlockState(blocks::DIRT));
        assert!(level.get(grass.above()).is_air());
    }

    #[test]
    fn same_item_does_not_replace_its_own_block() {
        // 默认的 `canBeReplaced`：手上拿的就是这个方块的物品时不替换，放到相邻的一格。
        let mut level = level();
        let grass = FLOOR.above();
        level.set(grass, BlockState(blocks::SHORT_GRASS));
        use_item(
            &mut level,
            &actor_holding(items::SHORT_GRASS),
            top_hit(grass),
        );
        assert_eq!(level.get(grass), BlockState(blocks::SHORT_GRASS));
    }

    #[test]
    fn occupied_target_fails() {
        let mut level = level();
        level.set(FLOOR.above(), BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::DIRT), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Fail);
    }

    #[test]
    fn entities_in_the_way_block_solid_blocks_only() {
        let mut level = level();
        level.obstructed = true;
        let outcome = use_item(&mut level, &actor_holding(items::DIRT), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(FLOOR.above()).is_air());

        // 火把没有碰撞形状，不看实体。
        let outcome = use_item(&mut level, &actor_holding(items::TORCH), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
    }

    #[test]
    fn game_master_items_need_permissions_nobody_has() {
        let mut level = level();
        let outcome = use_item(
            &mut level,
            &actor_holding(items::COMMAND_BLOCK),
            top_hit(FLOOR),
        );
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(FLOOR.above()).is_air());
    }

    #[test]
    fn other_items_do_nothing() {
        let mut level = level();
        let outcome = use_item(&mut level, &actor_holding(items::DIAMOND), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn food_that_cannot_be_placed_is_eaten_instead() {
        let mut level = level();
        level.set(FLOOR.above(), BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::CARROT), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Consume);
    }

    #[test]
    fn lily_pad_ignores_block_clicks() {
        let mut level = level();
        let outcome = use_item(&mut level, &actor_holding(items::LILY_PAD), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn hand_held_lily_pad_looks_for_a_water_source() {
        let mut level = TestLevel::new();
        let water = BlockPos::new(0, 63, 3);
        level.set(water, BlockState::WATER);
        let mut actor = actor_holding(items::LILY_PAD);
        actor.position = Vec3::new(0.5, 64.0, 3.5);
        actor.pitch = 90.0;
        let outcome = crate::testing::use_in_air(&mut level, &actor);
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(water.above()), BlockState(blocks::LILY_PAD));
    }

    /// 不真的放下去，直接看 `updatePlacementContext` 的结果。
    fn scaffolding_target(
        level: &mut TestLevel,
        hit: BlockHit,
        crouching: bool,
    ) -> Option<BlockPos> {
        let mut actor = actor_holding(items::SCAFFOLDING);
        actor.secondary_use = crouching;
        let mut ticks = BlockTicks::new();
        let world = World::new(level, &mut ticks);
        let ctx = PlaceContext::new(UseContext::new(&actor, Hand::Main, hit), &world);
        scaffolding_placement_context(&world, ctx).map(|ctx| ctx.clicked_pos())
    }

    #[test]
    fn scaffolding_grows_from_the_end_of_the_chain() {
        let mut level = level();
        let first = FLOOR.above();
        level.set(first, BlockState(blocks::SCAFFOLDING));

        // 点在顶面时朝玩家的方向长，玩家的 `yaw` 是 0，朝南。
        assert_eq!(
            scaffolding_target(&mut level, top_hit(FLOOR), false),
            Some(first.south())
        );
        // 这个方向上已经有脚手架时长在它后面。
        level.set(first.south(), BlockState(blocks::SCAFFOLDING));
        assert_eq!(
            scaffolding_target(&mut level, top_hit(FLOOR), false),
            Some(first.south().south())
        );

        // 点在旁边方块的侧面时往上长。
        level.set(first.west(), BlockState(blocks::STONE));
        let side = BlockHit {
            pos: first.west(),
            face: Direction::East,
            location: Vec3::new(0.0, 65.5, 0.5),
            inside: false,
        };
        assert_eq!(
            scaffolding_target(&mut level, side, false),
            Some(first.above())
        );
        // 蹲着时朝点击的面长。
        assert_eq!(
            scaffolding_target(&mut level, side, true),
            Some(first.east())
        );
    }

    #[test]
    fn scaffolding_stops_after_seven_blocks_sideways() {
        let mut level = level();
        let first = FLOOR.above();
        for distance in 0..=7 {
            level.set(
                first.relative_by(Direction::South, distance),
                BlockState(blocks::SCAFFOLDING),
            );
        }
        assert_eq!(scaffolding_target(&mut level, top_hit(FLOOR), false), None);
    }

    #[test]
    fn scaffolding_needs_support_within_reach() {
        let mut level = TestLevel::new();
        let actor = actor_holding(items::SCAFFOLDING);
        let wall = BlockPos::new(0, 64, 0);
        level.set(wall, BlockState(blocks::STONE));
        let outcome = use_item(
            &mut level,
            &actor,
            BlockHit {
                pos: wall,
                face: Direction::East,
                location: Vec3::new(1.0, 64.5, 0.5),
                inside: false,
            },
        );
        assert_eq!(outcome.result, InteractionResult::Fail);
    }

    /// `DoubleHighBlockItem.placeBlock` 的第一步，门紧接着把上半放进去，所以单独看它。
    fn cleared_above(above: BlockState) -> BlockState {
        let mut level = level();
        level.set(FLOOR.above(), above);
        let mut ticks = BlockTicks::new();
        clear_above(&mut World::new(&mut level, &mut ticks), FLOOR);
        level.get(FLOOR.above())
    }

    #[test]
    fn double_high_items_clear_the_block_above_first() {
        assert!(cleared_above(BlockState(blocks::SHORT_GRASS)).is_air());
    }

    #[test]
    fn double_high_items_keep_water_above() {
        let flowing = BlockState::WATER.with(crate::state::LEVEL, 3);
        assert_eq!(cleared_above(flowing), BlockState::WATER);
    }

    #[test]
    fn placing_reports_the_new_state_to_the_neighbours() {
        // 放下方块时邻居收到形状更新：含水台阶这条路径不改世界，这里只验证不会 panic。
        let mut level = level();
        let wet = BlockState(blocks::OAK_SLAB)
            .with(SLAB_TYPE, SlabType::Top)
            .with(WATERLOGGED, true);
        level.set(FLOOR.above().east(), wet);
        use_item(&mut level, &actor_holding(items::DIRT), top_hit(FLOOR));
        assert_eq!(level.get(FLOOR.above().east()), wet);
    }

    #[test]
    fn standing_items_skip_the_side_opposite_to_the_attachment() {
        // 悬挂告示牌贴在天花板上，脚下有方块也不能当支撑。
        let mut level = level();
        let outcome = use_item(
            &mut level,
            &actor_holding(items::OAK_HANGING_SIGN),
            top_hit(FLOOR),
        );
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(FLOOR.above()).is_air());
    }
}
