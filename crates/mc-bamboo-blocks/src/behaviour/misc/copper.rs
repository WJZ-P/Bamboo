//! `CopperBulbBlock`、`WeatheringCopperBulbBlock`、`CopperGolemStatueBlock`、
//! `WeatheringCopperGolemStatueBlock`。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    Block, items,
    sounds::{self, sound_event},
    tags::item_tags,
};

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour,
        common::{
            default_state, field_name, is_water_source, tick_water_if_waterlogged,
            update_neighbours_after_destroy,
        },
    },
    context::{PlaceContext, UseContext},
    entity::{Entity, mob::MobType},
    interact::InteractionResult,
    state::{
        BlockState, COPPER_GOLEM_POSE, CopperGolemPose, HORIZONTAL_FACING, LIT, POWERED,
        WATERLOGGED, item_is_tagged,
    },
    world::World,
};

/// `CopperBulbBlock`。
pub(super) struct CopperBulb;

/// `checkAndFlip`。
fn check_and_flip(state: BlockState, world: &mut World, pos: BlockPos) {
    let signal = world.has_neighbor_signal(pos);
    if signal == state.get(POWERED) {
        return;
    }
    let flipped = if state.get(POWERED) {
        state
    } else {
        let flipped = state.cycle(LIT);
        let sound = if flipped.get(LIT) {
            sound_event("minecraft:block.copper_bulb.turn_on")
        } else {
            sound_event("minecraft:block.copper_bulb.turn_off")
        };
        world.play_block_sound(pos, sound, 1.0, 1.0);
        flipped
    };
    world.set_block_and_update(pos, flipped.with(POWERED, signal));
}

/// `WeatheringCopperFullBlock`：只有氧化。
pub(super) struct WeatheringCopperFull;

impl Behaviour for WeatheringCopperFull {
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }
}

impl Behaviour for CopperBulb {
    /// `WeatheringCopperBulbBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            check_and_flip(state, world, pos);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_and_flip(state, world, pos);
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(LIT) { 15 } else { 0 }
    }
}

/// `CopperGolemStatueBlock`。
pub(super) struct CopperGolemStatue;

/// `updatePose`：先响再换姿势。
fn update_pose(state: BlockState, world: &mut World, pos: BlockPos) {
    world.play_block_sound(
        pos,
        sound_event("minecraft:entity.copper_golem_become_statue"),
        1.0,
        1.0,
    );
    world.set_block_and_update(pos, state.cycle(COPPER_GOLEM_POSE));
}

/// `getAnalogOutputSignal`：姿势的序号加一，顺序和官方的 `Pose` 一样。
fn pose_signal(state: BlockState) -> i32 {
    match state.get(COPPER_GOLEM_POSE) {
        CopperGolemPose::Standing => 1,
        CopperGolemPose::Sitting => 2,
        CopperGolemPose::Running => 3,
        CopperGolemPose::Star => 4,
    }
}

impl Behaviour for CopperGolemStatue {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite())
                .with(WATERLOGGED, waterlogged),
        )
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        state
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if item_is_tagged(ctx.item, item_tags::AXES) {
            return InteractionResult::Pass;
        }
        update_pose(state, world, ctx.clicked_pos());
        InteractionResult::Success
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        pose_signal(state)
    }
}

/// `WeatheringCopperGolemStatueBlock`：会氧化的雕像。
pub(super) struct WeatheringCopperGolemStatue;

/// 没有氧化的雕像被斧头点中时变回铜傀儡。
///
/// `removeStatue`：在方块中心生成铜傀儡，朝向取雕像的 `FACING`，播 `spawn` 声，
/// 然后移除方块。方块实体的自定义名这里没有建模，不复制。
///
/// An unaffected statue hit with an axe turns back into a copper golem.
///
/// `removeStatue`: spawn a golem at the block centre facing the statue's `FACING`,
/// play the spawn sound and remove the block. The block entity's custom name is
/// not modelled here, so it is not copied.
fn revive_statue(state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult {
    if field_name(state.block(), "getAge()") != "unaffected" {
        return InteractionResult::Pass;
    }
    let pos = ctx.clicked_pos();
    crate::durability::hurt_hand(world, ctx.hand, 1);
    let position = Vec3 {
        x: f64::from(pos.x) + 0.5,
        y: f64::from(pos.y),
        z: f64::from(pos.z) + 0.5,
    };
    // `removeStatue` 把雕像组件表里的自定义名搬回铜傀儡；要在移除方块之前读。
    // `removeStatue` copies the statue's custom name back onto the golem; read it
    // before the block (and its block entity) are removed.
    let custom_name = world.statue_custom_name(pos);
    // `prepare_entity` 会调 `finish_mob_random` 覆写朝向，朝向要在之后设。
    // `prepare_entity` calls `finish_mob_random`, which overwrites the facing, so
    // set it afterwards.
    let facing = state.get(HORIZONTAL_FACING).to_yaw();
    let mut entity = world.prepare_entity(Entity::by_mob_type(MobType::CopperGolem, position));
    entity.old_position = position;
    entity.y_rot = facing;
    entity.with_mob(|_, mob| {
        mob.x_rot = 0.0;
        mob.y_head_rot = facing;
        mob.y_body_rot = facing;
        mob.y_head_rot_o = facing;
        mob.y_body_rot_o = facing;
        mob.custom_name = custom_name.clone();
    });
    world.add_prepared_entity(entity);
    world.play_sound(
        position,
        sound_event("minecraft:entity.copper_golem.spawn"),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    world.remove_block(pos);
    InteractionResult::Success
}

impl Behaviour for WeatheringCopperGolemStatue {
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        CopperGolemStatue.state_for_placement(block, ctx, world)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        CopperGolemStatue.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        CopperGolemStatue.affect_neighbors_after_removal(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        if item_is_tagged(ctx.item, item_tags::AXES) {
            return revive_statue(state, world, ctx);
        }
        if ctx.item == items::HONEYCOMB {
            return InteractionResult::Pass;
        }
        update_pose(state, world, pos);
        InteractionResult::Success
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        pose_signal(state)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::{
        state::BlockState,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    /// `removeStatue` 把雕像组件表里的自定义名搬回新生成的铜傀儡。
    #[test]
    fn revive_copies_the_statue_custom_name_onto_the_golem() {
        let mut level = TestLevel::new();
        let pos = BlockPos::new(1, 64, 2);
        level.set(pos, BlockState(blocks::COPPER_GOLEM_STATUE));
        let name = Tag::String("Copper Nick".to_owned());
        level.statue_names.insert(pos, name.clone());

        let actor = actor_holding(items::DIAMOND_AXE);
        let _outcome = use_item(&mut level, &actor, top_hit(pos));

        let golems: Vec<_> = level
            .entities()
            .iter()
            .filter(|entity| {
                entity
                    .kind
                    .mob()
                    .is_some_and(|mob| mob.mob_type == MobType::CopperGolem)
            })
            .collect();
        assert_eq!(golems.len(), 1);
        assert_eq!(golems[0].kind.mob().unwrap().custom_name, Some(name));
        // 方块没了就看不到名字了（和 `Level::sign` 一样）。
        assert!(level.get(pos).is_air());
    }
}
