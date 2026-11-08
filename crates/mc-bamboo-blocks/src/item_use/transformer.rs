//! `BlockTransformer.transformBlock`：斧子、锄头、锹右键时按数据驱动的规则改方块。
//!
//! 规则是从官方注册表生成的，在 `mc_bamboo_data::block_transformers`。官方锄头翻生根泥土时还会掉出垂根，
//! 掉落物不在范围里。

use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{
    BlockAncestor, BlockPredicate, BlockTransform, BlockTransformer, StateProvider, StateRule,
};

use crate::{
    behaviour::{common::item_of, machines::chest::connected_direction},
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, CHEST_TYPE, ChestType},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `transformBlock`：第一项给出新状态的规则生效。
pub(super) fn transform_block(
    world: &mut World,
    transformer: &BlockTransformer,
    ctx: &UseContext,
) -> InteractionResult {
    if has_blocking_item_use_intent(ctx) {
        return InteractionResult::Pass;
    }
    let pos = ctx.clicked_pos();
    let face = ctx.clicked_face().id();
    let found = transformer
        .transforms
        .iter()
        .filter(|transform| !transform.disallowed_faces.contains(&face))
        .find_map(|transform| Some((transform, optional_state(&transform.provider, world, pos)?)));
    let Some((transform, state)) = found else {
        return InteractionResult::Pass;
    };
    let old = world.block_state(pos);
    let state = fitted_to_neighbours(transform, world, state, pos);
    world.set_block(pos, state, UPDATE_ALL_IMMEDIATE);
    // `transformBlock`：写方块之后按这一项配的音效和粒子出声，都跳过动手的玩家。
    if let Some(sound) = transform.sound {
        world.play_block_sound_except(pos, sound, 1.0, 1.0);
    }
    if transform.particle != 0 {
        world.level_event_except(transform.particle, pos, 0);
        // `transformType == COPPER_CHEST`：涂蜡和刮削要把粒子也发给大箱子的另一半。
        if transform.copper_chest
            && old.is_a(BlockAncestor::CopperChestBlock)
            && old.get(CHEST_TYPE) != ChestType::Single
        {
            let neighbor = pos.relative(connected_direction(old));
            world.level_event_except(transform.particle, neighbor, 0);
        }
    }
    InteractionResult::Success
}

/// `playerHasBlockingItemUseIntent`：副手拿着盾、没有蹲下时，主手的右键是举盾。
fn has_blocking_item_use_intent(ctx: &UseContext) -> bool {
    ctx.hand == Hand::Main
        && item_of(ctx.actor.off_hand).is_some_and(|item| item.blocks_attacks)
        && !ctx.is_secondary_use_active()
}

/// `updateFromNeighbors` 为真时按六个邻居更新一遍。铜门和铜箱子不更新，另一半由写入后的形状更新跟上。
fn fitted_to_neighbours(
    transform: &BlockTransform,
    world: &mut World,
    state: BlockState,
    pos: BlockPos,
) -> BlockState {
    if transform.update_from_neighbors {
        world.update_from_neighbour_shapes(state, pos)
    } else {
        state
    }
}

/// `BlockStateProvider.getOptionalState`：只有按规则选的提供器会什么也不给。
fn optional_state(provider: &StateProvider, world: &World, pos: BlockPos) -> Option<BlockState> {
    match provider {
        StateProvider::RuleBased { rules, fallback } => rule_based(rules, *fallback, world, pos),
        _ => Some(state_of(provider, world, pos)),
    }
}

/// `BlockStateProvider.getState`。
fn state_of(provider: &StateProvider, world: &World, pos: BlockPos) -> BlockState {
    match provider {
        StateProvider::Simple(state) => BlockState(*state),
        StateProvider::CopyProperties(source) => {
            state_of(source, world, pos).with_properties_of(world.block_state(pos))
        }
        StateProvider::RuleBased { .. } => {
            optional_state(provider, world, pos).unwrap_or_else(|| world.block_state(pos))
        }
    }
}

/// `RuleBasedStateProvider.getOptionalState`：成立的规则给不出状态时接着试下一条。
fn rule_based(
    rules: &[StateRule],
    fallback: Option<&StateProvider>,
    world: &World,
    pos: BlockPos,
) -> Option<BlockState> {
    rules
        .iter()
        .filter(|rule| test(&rule.if_true, world, pos))
        .find_map(|rule| optional_state(&rule.then, world, pos))
        .or_else(|| optional_state(fallback?, world, pos))
}

/// `BlockPredicate.test`。
fn test(predicate: &BlockPredicate, world: &World, pos: BlockPos) -> bool {
    match predicate {
        BlockPredicate::MatchingBlocks { offset, blocks } => {
            let id = world.block_state(offset_of(pos, *offset)).block_id();
            blocks.iter().any(|&block| block as usize == id)
        }
        BlockPredicate::MatchingBlockTag { offset, tag } => {
            world.block_state(offset_of(pos, *offset)).is_tagged(*tag)
        }
        BlockPredicate::AllOf(predicates) => predicates.iter().all(|part| test(part, world, pos)),
    }
}

fn offset_of(pos: BlockPos, [dx, dy, dz]: [i32; 3]) -> BlockPos {
    pos.offset(dx, dy, dz)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{Axis, BlockPos, Direction};
    use mc_bamboo_data::{blocks, items};

    use crate::{
        BlockHit, InteractionResult,
        state::{AXIS, BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, MOISTURE, WATERLOGGED},
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn side_hit(pos: BlockPos, face: Direction) -> BlockHit {
        BlockHit {
            face,
            ..top_hit(pos)
        }
    }

    #[test]
    fn an_axe_strips_a_log_and_keeps_its_axis() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::OAK_LOG).with(AXIS, Axis::X));
        let outcome = use_item(&mut level, &actor_holding(items::IRON_AXE), top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            level.get(POS),
            BlockState(blocks::STRIPPED_OAK_LOG).with(AXIS, Axis::X)
        );
    }

    #[test]
    fn an_axe_scrapes_and_unwaxes_copper() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::EXPOSED_COPPER));
        use_item(&mut level, &actor_holding(items::STONE_AXE), top_hit(POS));
        assert_eq!(level.get(POS), BlockState(blocks::COPPER_BLOCK));

        level.set(
            POS,
            BlockState(blocks::WAXED_CUT_COPPER_SLAB).with(WATERLOGGED, true),
        );
        use_item(&mut level, &actor_holding(items::STONE_AXE), top_hit(POS));
        assert_eq!(
            level.get(POS),
            BlockState(blocks::CUT_COPPER_SLAB).with(WATERLOGGED, true)
        );
    }

    #[test]
    fn scraping_one_half_of_a_door_changes_the_other_half_too() {
        let mut level = TestLevel::new();
        let lower = BlockState(blocks::WEATHERED_COPPER_DOOR);
        level.set(POS.below(), BlockState(blocks::STONE));
        level.set(POS, lower);
        level.set(
            POS.above(),
            lower.with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper),
        );
        // 不蹲着时铜门自己接住右键，开门。
        let mut actor = actor_holding(items::GOLDEN_AXE);
        actor.secondary_use = true;
        use_item(&mut level, &actor, top_hit(POS));
        assert_eq!(level.get(POS), BlockState(blocks::EXPOSED_COPPER_DOOR));
        assert_eq!(
            level.get(POS.above()),
            BlockState(blocks::EXPOSED_COPPER_DOOR).with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper)
        );
    }

    #[test]
    fn a_shovel_makes_a_path_only_from_above_with_air_on_top() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::GRASS_BLOCK));
        let below = use_item(
            &mut level,
            &actor_holding(items::WOODEN_SHOVEL),
            side_hit(POS, Direction::Down),
        );
        assert_eq!(below.result, InteractionResult::Pass);
        level.set(POS.above(), BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::WOODEN_SHOVEL),
            top_hit(POS),
        );
        assert!(level.get(POS).is(blocks::GRASS_BLOCK));

        level.set(POS.above(), BlockState::AIR);
        let side = use_item(
            &mut level,
            &actor_holding(items::WOODEN_SHOVEL),
            side_hit(POS, Direction::North),
        );
        assert_eq!(side.result, InteractionResult::Success);
        assert_eq!(level.get(POS), BlockState(blocks::DIRT_PATH));
    }

    #[test]
    fn a_hoe_tills_dirt_and_turns_coarse_and_rooted_dirt_into_dirt() {
        let mut level = TestLevel::new();
        let hoe = actor_holding(items::DIAMOND_HOE);
        level.set(POS, BlockState(blocks::DIRT));
        use_item(&mut level, &hoe, top_hit(POS));
        assert_eq!(
            level.get(POS),
            BlockState(blocks::FARMLAND).with(MOISTURE, 0)
        );

        level.set(POS, BlockState(blocks::COARSE_DIRT));
        use_item(&mut level, &hoe, top_hit(POS));
        assert_eq!(level.get(POS), BlockState(blocks::DIRT));

        // 生根泥土不看上面是不是空气，也不挑面。
        level.set(POS, BlockState(blocks::ROOTED_DIRT));
        level.set(POS.above(), BlockState(blocks::STONE));
        use_item(&mut level, &hoe, side_hit(POS, Direction::Down));
        assert_eq!(level.get(POS), BlockState(blocks::DIRT));
    }

    #[test]
    fn a_shield_in_the_off_hand_keeps_the_axe_from_stripping() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::BIRCH_WOOD));
        let mut actor = actor_holding(items::IRON_AXE);
        actor.off_hand = items::SHIELD;
        let outcome = use_item(&mut level, &actor, top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(level.get(POS).is(blocks::BIRCH_WOOD));

        // 蹲着的时候不举盾，斧子照常去皮。
        actor.secondary_use = true;
        use_item(&mut level, &actor, top_hit(POS));
        assert!(level.get(POS).is(blocks::STRIPPED_BIRCH_WOOD));
    }
}
