//! `PumpkinBlock`：剪刀把南瓜刻成雕刻南瓜，并按 `CARVE_PUMPKIN` 掉 4 个种子。
//!
//! Shears carve a pumpkin and drop 4 seeds from `CARVE_PUMPKIN`.

use mc_bamboo_core::{Axis, Vec3};
use mc_bamboo_data::{blocks, items, sounds::sound_event};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{Behaviour, Plain},
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `PumpkinBlock`。
pub(super) struct Pumpkin;

impl Behaviour for Pumpkin {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item != items::SHEARS {
            return Plain.use_item_on(state, world, ctx);
        }
        let face = ctx.clicked_face();
        let facing = if face.axis() == Axis::Y {
            ctx.horizontal_direction().opposite()
        } else {
            face
        };
        let carved = BlockState(blocks::CARVED_PUMPKIN).with(HORIZONTAL_FACING, facing);
        // 表是常量 4，不取序列随机数。构造物品时实体随机速度随后被 `level.random` 的两个 double 盖掉。
        // 雕刻声在写方块之前，剪刀耐久在写方块之后。
        //
        // The table is a constant 4 and draws no sequence. The entity's random velocity is
        // then replaced by two `level.random` doubles. The carve sound comes before the block
        // write, and the shears spend durability after it.
        let pos = ctx.clicked_pos();
        let (step_x, _, step_z) = facing.normal();
        let id = world.spawn_at_location(
            Vec3::new(
                f64::from(pos.x) + 0.5 + f64::from(step_x) * 0.65,
                f64::from(pos.y) + 0.1,
                f64::from(pos.z) + 0.5 + f64::from(step_z) * 0.65,
            ),
            ItemStack::new(items::PUMPKIN_SEEDS, 4),
        );
        let delta_x = 0.05 * f64::from(step_x) + world.random().next_double() * 0.02;
        let delta_z = 0.05 * f64::from(step_z) + world.random().next_double() * 0.02;
        if let Some(id) = id
            && let Some(entity) = world.entity_mut(id)
        {
            entity.delta = Vec3::new(delta_x, 0.05, delta_z);
        }
        world.play_block_sound(pos, sound_event("minecraft:block.pumpkin.carve"), 1.0, 1.0);
        world.set_block(pos, carved, UPDATE_ALL_IMMEDIATE);
        crate::durability::hurt_hand(world, ctx.hand, 1);
        InteractionResult::Success
    }
}
