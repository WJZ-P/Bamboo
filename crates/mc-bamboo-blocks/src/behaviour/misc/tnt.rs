//! `TntBlock`。点燃后的实体和爆炸不在范围里，方块和官方一样消失。
//! 实体不生成，但 `PrimedTnt` 构造时取的随机数照样取，后面的随机数才和官方对得上。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{Block, items, sounds, sounds::sound_event};

use crate::{
    behaviour::{Behaviour, Plain},
    context::UseContext,
    interact::InteractionResult,
    state::BlockState,
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `TntBlock`。
pub(super) struct Tnt;

/// `prime`：生成点燃的 TNT。实体还没有，只取它构造时那个随机朝向（`level.getRandom().nextDouble()`），
/// 然后照官方在实体位置放点燃的声音。
pub(super) fn prime(world: &mut World, pos: BlockPos) {
    world.random().next_double();
    let position = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    world.play_sound(
        position,
        sound_event("minecraft:entity.tnt.primed"),
        sounds::BLOCKS,
        1.0,
        1.0,
        false,
    );
}

/// `onPlace` 和 `neighborChanged`：有信号时点燃，方块消失。
fn prime_if_powered(world: &mut World, pos: BlockPos) {
    if world.has_neighbor_signal(pos) {
        prime(world, pos);
        world.remove_block(pos);
    }
}

impl Behaviour for Tnt {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            prime_if_powered(world, pos);
        }
    }

    fn neighbor_changed(
        &self,
        _state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        prime_if_powered(world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item != items::FLINT_AND_STEEL && ctx.item != items::FIRE_CHARGE {
            return Plain.use_item_on(state, world, ctx);
        }
        prime(world, ctx.clicked_pos());
        world.set_block(ctx.clicked_pos(), BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        InteractionResult::Success
    }
}
