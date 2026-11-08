//! `BoneMealItem.useOn`：催熟点中的方块，不行时在点中的面前的水里长海草。
//!
//! 创造模式不消耗骨粉。世界只有平原，平原的水里不长珊瑚，只长海草，所以点中的是哪个面不影响长出来的东西。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockAncestor, blocks};

use crate::{
    context::UseContext,
    interact::InteractionResult,
    level::Effect,
    state::{BlockState, SupportType},
    world::World,
};

/// `growWaterPlant` 试多少次。
const WATER_PLANT_ATTEMPTS: i32 = 128;
/// 每过这么多次多走一步，越往后长得越远。
const ATTEMPTS_PER_STEP: i32 = 16;
/// 已经有海草的地方十分之一的机会让它长高。
const SEAGRASS_GROWTH_CHANCE: i32 = 10;
/// 满格流体的 `getAmount`。
const FULL_FLUID: i32 = 8;

/// `BoneMealItem.useOn`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    if grow_crop(world, pos) {
        world.push_effect(Effect::BoneMealUsed { pos });
        return InteractionResult::SuccessServer;
    }
    let face = ctx.clicked_face();
    let water = pos.relative(face);
    if world.is_face_sturdy(pos, face, SupportType::Full) && grow_water_plant(world, water) {
        world.push_effect(Effect::BoneMealUsed { pos: water });
        return InteractionResult::Success;
    }
    InteractionResult::Pass
}

/// `growCrop`：点中的方块能催熟时催熟，成不成功都算用掉了骨粉。
fn grow_crop(world: &mut World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::BonemealableBlock) || !state.is_valid_bonemeal_target(world, pos)
    {
        return false;
    }
    if state.is_bonemeal_success(world, pos) {
        state.perform_bonemeal(world, pos);
    }
    true
}

/// 满格的水源方块。
fn is_full_water_block(world: &World, pos: BlockPos) -> bool {
    world.block_state(pos).is(blocks::WATER) && world.fluid_state(pos).amount() == FULL_FLUID
}

/// `growWaterPlant`：从这一格水出发随机走几步，在走到的水里长海草。
fn grow_water_plant(world: &mut World, pos: BlockPos) -> bool {
    if !is_full_water_block(world, pos) {
        return false;
    }
    for attempt in 0..WATER_PLANT_ATTEMPTS {
        if let Some(target) = random_walk(world, pos, attempt / ATTEMPTS_PER_STEP) {
            grow_seagrass_at(world, target);
        }
    }
    true
}

/// 随机走 `steps` 步，碰到完整的方块就放弃这一次。
fn random_walk(world: &World, start: BlockPos, steps: i32) -> Option<BlockPos> {
    let mut pos = start;
    for _ in 0..steps {
        let random = world.random();
        let dx = random.next_int(3) - 1;
        let dy = (random.next_int(3) - 1) * random.next_int(3) / 2;
        let dz = random.next_int(3) - 1;
        pos = pos.offset(dx, dy, dz);
        if world.block_state(pos).is_collision_shape_full_block() {
            return None;
        }
    }
    Some(pos)
}

/// 满格的水里放海草；已经是海草时偶尔让它长高。
fn grow_seagrass_at(world: &mut World, pos: BlockPos) {
    let seagrass = BlockState(blocks::SEAGRASS);
    if !seagrass.can_survive(world, pos) {
        return;
    }
    let state = world.block_state(pos);
    if is_full_water_block(world, pos) {
        world.set_block_and_update(pos, seagrass);
    } else if state.is(blocks::SEAGRASS)
        && state.is_valid_bonemeal_target(world, pos)
        && world.random().next_int(SEAGRASS_GROWTH_CHANCE) == 0
    {
        state.perform_bonemeal(world, pos);
    }
}
