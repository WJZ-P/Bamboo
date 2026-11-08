//! `RedStoneOreBlock`：点一下就亮，随机刻时熄灭。

use mc_bamboo_core::{Axis, BlockPos, Direction};

use crate::{
    behaviour::Behaviour,
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, LIT, block_of_item},
    world::World,
};

/// `RedStoneOreBlock`。
pub(super) struct RedstoneOre;

/// `spawnParticles`：不画粒子，但取随机数的次数要和官方一样，后面的计划刻延迟才不会错开。
///
/// 每个不挡光的面各取两个 `nextFloat`，分别是这个面不沿轴的两个坐标。
fn consume_particle_random(world: &World, pos: BlockPos) {
    let random = world.random();
    for direction in Direction::ALL {
        if world.block_state(pos.relative(direction)).is_solid_render() {
            continue;
        }
        let axis = direction.axis();
        if axis != Axis::X {
            random.next_float();
        }
        if axis != Axis::Y {
            random.next_float();
        }
        if axis != Axis::Z {
            random.next_float();
        }
    }
}

impl Behaviour for RedstoneOre {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        consume_particle_random(world, ctx.clicked_pos());
        if !state.get(LIT) {
            world.set_block_and_update(ctx.clicked_pos(), state.with(LIT, true));
        }
        let is_block_item = !block_of_item(ctx.item).is_air();
        if is_block_item && PlaceContext::new(*ctx, world).can_place(world) {
            InteractionResult::Pass
        } else {
            InteractionResult::Success
        }
    }

    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(LIT) {
            world.set_block_and_update(pos, state.with(LIT, false));
        }
    }
}
