//! 只有含水逻辑的方块：`BarrierBlock`、`HeavyCoreBlock`、`MangroveRootsBlock`、
//! `WaterloggedTransparentBlock`、`WeatheringCopperGrateBlock`、`ConduitBlock`、`DriedGhastBlock`、
//! `LightBlock`。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{Block, items, sounds::sound_event};

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_full_water, is_water_source, tick_water_if_waterlogged},
    },
    context::{Actor, PlaceContext, UseContext},
    entity::{
        Entity,
        mob::{MobType, living, spawn},
    },
    game_event::{GameEvent, dispatch::EventContext},
    interact::InteractionResult,
    state::{BlockState, DRIED_GHAST_HYDRATION_LEVELS, FluidState, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

/// 放进水里就含水的方块。
pub(super) struct Waterlogged;

impl Behaviour for Waterlogged {
    /// `WeatheringCopperGrateBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(default_state(block).with(WATERLOGGED, waterlogged))
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
}

/// `BarrierBlock`：只有创造模式的玩家能往里倒水、舀水，流体和海绵都不行。
pub(super) struct Barrier;

impl Behaviour for Barrier {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Waterlogged.state_for_placement(block, ctx, world)
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
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn can_place_liquid(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        fluid: i32,
        by_player: bool,
    ) -> bool {
        by_player && Plain.can_place_liquid(state, world, pos, fluid, by_player)
    }

    fn pickup_block(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        by_player: bool,
    ) -> i32 {
        if by_player {
            Plain.pickup_block(state, world, pos, by_player)
        } else {
            items::AIR
        }
    }
}

/// `ConduitBlock`：放在流动的水里也算含水，只要那一格是满的。
pub(super) struct Conduit;

impl Behaviour for Conduit {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_full_water(world.fluid_state(ctx.clicked_pos()));
        Some(default_state(block).with(WATERLOGGED, waterlogged))
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
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `DriedGhastBlock`。含水的方块，泡水后按计划刻推进水合阶段，吸满孵出幼年快乐恶魂。
pub(super) struct DriedGhast;

/// `DriedGhastBlock.MAX_HYDRATION_LEVEL`：水合到这一级就孵出恶魂。
const MAX_HYDRATION_LEVEL: i32 = 3;
/// `AgeableMob.BABY_START_AGE`：`setBaby(true)` 落到的年龄。
const BABY_START_AGE: i32 = -24_000;

impl Behaviour for DriedGhast {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = Waterlogged.state_for_placement(block, ctx, world)?;
        Some(state.with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite()))
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
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// `setPlacedBy`：泡着水放下时是另一个声音。
    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let sound = if state.get(WATERLOGGED) {
            "minecraft:block.dried_ghast.place_in_water"
        } else {
            "minecraft:block.dried_ghast.place"
        };
        world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
    }

    /// `placeLiquid`：换完方块、排好流体刻之后放换水的声音。
    fn place_liquid(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        fluid: FluidState,
    ) -> bool {
        if !Plain.place_liquid(state, world, pos, fluid) {
            return false;
        }
        world.play_block_sound(
            pos,
            sound_event("minecraft:block.dried_ghast.place_in_water"),
            1.0,
            1.0,
        );
        true
    }

    /// 泡着水或者已经吸了水时排一个吸水的计划刻。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let hydrating = state.get(WATERLOGGED) || state.get(DRIED_GHAST_HYDRATION_LEVELS) > 0;
        if hydrating && !world.has_scheduled_tick(pos, state.block()) {
            world.schedule_tick(pos, state.block(), DRIED_GHAST_HYDRATION_DELAY);
        }
    }

    /// `DriedGhastBlock.tick`：含水时推进一级水合（吸满则孵出幼年快乐恶魂），
    /// 脱水时退一级水合。每次变状态都发 `BLOCK_CHANGE` 游戏事件。
    ///
    /// `DriedGhastBlock.tick`: while waterlogged, advance hydration (or hatch a baby
    /// happy ghast when saturated); while dry, decay hydration. Every state change
    /// emits a `BLOCK_CHANGE` game event.
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let hydration = state.get(DRIED_GHAST_HYDRATION_LEVELS);
        let center = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        );
        if state.get(WATERLOGGED) {
            if hydration < MAX_HYDRATION_LEVEL {
                world.play_block_sound(
                    pos,
                    sound_event("minecraft:block.dried_ghast.transition"),
                    1.0,
                    1.0,
                );
                world.set_block(
                    pos,
                    state.with(DRIED_GHAST_HYDRATION_LEVELS, hydration + 1),
                    2,
                );
                world.game_event(
                    GameEvent::BlockChange,
                    center,
                    EventContext {
                        affected_state: Some(state),
                        ..Default::default()
                    },
                );
            } else {
                spawn_ghastling(state, world, pos);
            }
            return;
        }
        if hydration > 0 {
            world.set_block(
                pos,
                state.with(DRIED_GHAST_HYDRATION_LEVELS, hydration - 1),
                2,
            );
            world.game_event(
                GameEvent::BlockChange,
                center,
                EventContext {
                    affected_state: Some(state),
                    ..Default::default()
                },
            );
        }
    }
}

/// `DriedGhastBlock.spawnGhastling`：拿掉方块，孵出一只朝 `FACING` 的幼年快乐恶魂，
/// 落点在方块底心，放孵化声。
///
/// `DriedGhastBlock.spawnGhastling`: remove the block and spawn a baby happy ghast
/// facing `FACING` at the block's bottom centre, with the spawn sound.
fn spawn_ghastling(state: BlockState, world: &mut World, pos: BlockPos) {
    world.remove_block(pos);
    let yaw = state.get(HORIZONTAL_FACING).to_yaw();
    let mut ghastling =
        world.prepare_entity(Entity::by_mob_type(MobType::HappyGhast, Vec3::default()));
    ghastling.with_mob(|me, mob| {
        living::set_age(me, mob, BABY_START_AGE);
        mob.y_head_rot = yaw;
    });
    spawn::snap_to(
        &mut ghastling,
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        ),
        yaw,
        0.0,
    );
    world.add_prepared_entity(ghastling);
    world.play_block_sound(
        pos,
        sound_event("minecraft:entity.ghastling.spawn"),
        1.0,
        1.0,
    );
}

/// `DriedGhastBlock.HYDRATION_TICK_DELAY`：吸一次水要等的刻数。
const DRIED_GHAST_HYDRATION_DELAY: i32 = 5000;

/// `LightBlock`：放置不含水。调亮度要权限，这个世界没有权限，所以只吃掉点击。
pub(super) struct Light;

impl Behaviour for Light {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Consume
    }
}
