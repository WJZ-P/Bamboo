//! `CreakingHeartBlock`。放下时看两头的苍白橡木，之后的状态变化在计划刻里，不在范围里。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::entity::Entity;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, update_neighbours_after_destroy},
        misc::creaking_heart_entity,
    },
    context::{Actor, PlaceContext},
    state::{AXIS, BlockState, CREAKING_HEART_STATE, CreakingHeartState, NATURAL},
    world::World,
};

/// `CreakingHeartBlock`。
pub(super) struct CreakingHeart;

/// `hasRequiredLogs`：这条轴两头都是同一条轴的苍白橡木。
fn has_required_logs(world: &World, pos: BlockPos, axis: Axis) -> bool {
    [axis.positive(), axis.negative()].into_iter().all(|side| {
        let neighbor = world.block_state(pos.relative(side));
        neighbor.is_tagged(block_tags::PALE_OAK_LOGS) && neighbor.get(AXIS) == axis
    })
}

impl Behaviour for CreakingHeart {
    /// `BaseEntityBlock.newBlockEntity`：放下心时挂一个空方块实体。
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if !world.has_block_entity(pos) {
            world.set_block_entity(creaking_heart_entity::create_block_entity(pos));
        }
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let axis = ctx.clicked_face().axis();
        let state = default_state(block).with(AXIS, axis);
        let active = world.creaking_active(ctx.clicked_pos());
        // `updateCreakingState`：缺两头木头时是 `UPROOTED`，否则按环境属性选。
        Some(if has_required_logs(world, ctx.clicked_pos(), axis) {
            state.with(
                CREAKING_HEART_STATE,
                if active {
                    CreakingHeartState::Awake
                } else {
                    CreakingHeartState::Dormant
                },
            )
        } else {
            state.with(CREAKING_HEART_STATE, CreakingHeartState::Uprooted)
        })
    }

    /// `CreakingHeartBlock.playerWillDestroy`：拆掉绑定的嘎枝，再按 NATURAL 给经验。
    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        let had_be = world
            .with_block_entity::<creaking_heart_entity::CreakingHeartEntity, _>(
                pos,
                |heart, world| heart.remove_protector(world),
            )
            .is_some();
        if had_be && !player.prevents_block_drops && state.try_get(NATURAL).unwrap_or(false) {
            let amount = world.random().next_int_between_inclusive(20, 24);
            world.spawn_entity(Entity::experience_orb(
                crate::world::block_center_public(pos),
                amount,
            ));
        }
        Plain.player_will_destroy(state, world, pos, player)
    }

    /// `getAnalogOutputSignal`：`UPROOTED` 恒 0，否则读方块实体。
    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(CREAKING_HEART_STATE) == CreakingHeartState::Uprooted {
            return 0;
        }
        world
            .block_entity::<creaking_heart_entity::CreakingHeartEntity>(pos)
            .map_or(
                0,
                creaking_heart_entity::CreakingHeartEntity::analog_output_signal,
            )
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }
}
