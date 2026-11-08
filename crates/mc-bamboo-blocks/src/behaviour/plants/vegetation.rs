//! `VegetationBlock` 和只换了支撑条件的子类：花、草、树苗、灌木、菌类、下界疣、荷叶、茎，
//! 以及多覆盖了几个方法的 `MushroomBlock`、`MangrovePropaguleBlock`、`SeagrassBlock`、
//! `SweetBerryBushBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, BlockClass, blocks, items,
    registries::fluid,
    sounds::sound_event,
    tags::{block_tags, fluid_tags},
};

use super::can_spread;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, field_block, field_tag, is_full_water, is_water_source,
            may_place_vegetation_on, tick_water_if_waterlogged, vegetation_update_shape,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        AGE_3, AGE_4, BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, FluidState, HANGING, STAGE,
        SupportType, WATERLOGGED,
    },
    world::{UPDATE_CLIENTS, UPDATE_NONE, World},
};

/// `MushroomBlock.canSurvive` 要求亮度低于这个值。
const MUSHROOM_MAX_BRIGHTNESS: i32 = 13;
/// 蘑菇每次随机刻有二十五分之一的机会蔓延。
const MUSHROOM_SPREAD_CHANCE: i32 = 25;
/// 蔓延时往外试走的步数。
const MUSHROOM_SPREAD_STEPS: i32 = 4;
/// `MangrovePropaguleBlock.MAX_AGE`：放下时就是长成的样子。
const FULLY_GROWN_PROPAGULE: i32 = 4;
/// `SaplingBlock.BRIGHTNESS_FOR_SAPLING_GROWTH`。
const SAPLING_MIN_BRIGHTNESS: i32 = 9;
/// `TICK_CHANCE_FOR_SAPLING_GROWTH`：每次随机刻七分之一的机会长一步。
const SAPLING_GROWTH_CHANCE: i32 = 7;
/// `SaplingBlock.isBonemealSuccess` 的概率。官方拿 `nextFloat()` 和 double 的 0.45 比，要在 double 里比。
const SAPLING_BONEMEAL_CHANCE: f64 = 0.45;
/// `NetherFungusBlock.isBonemealSuccess` 的概率。
const FUNGUS_BONEMEAL_CHANCE: f64 = 0.4;
/// `advanceTree` 写 `stage` 的标记：`UPDATE_NONE`。
const SAPLING_STAGE_FLAGS: u32 = UPDATE_NONE;
/// `SweetBerryBushBlock.MAX_AGE`。
const RIPE_BUSH_AGE: i32 = 3;
/// 摘完果子后退回的 `age`，比它大才有果子。
const PICKED_BUSH_AGE: i32 = 1;
/// 浆果丛每次随机刻五分之一的机会长一级。
const BUSH_GROWTH_CHANCE: i32 = 5;
/// 浆果丛上面一格要这么亮才长。
const BUSH_MIN_BRIGHTNESS: i32 = 9;
/// `NetherWartBlock.MAX_AGE`。
const RIPE_WART_AGE: i32 = 3;
/// 下界疣每次随机刻十分之一的机会长一级。
const WART_GROWTH_CHANCE: i32 = 10;

/// `mayPlaceOn`：`plant` 能不能长在 `support_pos` 这一格的 `support` 上。
///
/// 这是官方最常被子类覆盖的方法，所有覆盖都列在这里，只换了它的子类不用写结构体。
pub(super) fn may_place_on(
    plant: BlockState,
    support: BlockState,
    world: &World,
    support_pos: BlockPos,
) -> bool {
    use BlockClass as C;

    let block = plant.block();
    match plant.class() {
        C::AttachedStemBlock | C::NetherFungusBlock | C::NetherRootsBlock => {
            support.is_tagged(field_tag(block, "supportBlocks"))
        }
        C::AzaleaBlock => support.is_tagged(block_tags::SUPPORTS_AZALEA),
        C::BeetrootBlock
        | C::CarrotBlock
        | C::CropBlock
        | C::PitcherCropBlock
        | C::PotatoBlock
        | C::TorchflowerCropBlock => support.is_tagged(block_tags::SUPPORTS_CROPS),
        C::CactusFlowerBlock => {
            support.is_tagged(block_tags::SUPPORT_OVERRIDE_CACTUS_FLOWER)
                || world.is_face_sturdy(support_pos, Direction::Up, SupportType::Center)
        }
        C::DryVegetationBlock | C::ShortDryGrassBlock | C::TallDryGrassBlock => {
            support.is_tagged(block_tags::SUPPORTS_DRY_VEGETATION)
        }
        C::LilyPadBlock => lily_pad_may_place_on(support, world, support_pos),
        C::MangrovePropaguleBlock => support.is_tagged(block_tags::SUPPORTS_MANGROVE_PROPAGULE),
        C::MushroomBlock => support.is_solid_render(),
        C::NetherSproutsBlock => support.is_tagged(block_tags::SUPPORTS_NETHER_SPROUTS),
        C::NetherWartBlock => support.is_tagged(block_tags::SUPPORTS_NETHER_WART),
        C::SeagrassBlock | C::TallSeagrassBlock => {
            world.is_face_sturdy(support_pos, Direction::Up, SupportType::Full)
                && !support.is_tagged(block_tags::CANNOT_SUPPORT_SEAGRASS)
        }
        C::SmallDripleafBlock => {
            support.is_tagged(block_tags::SUPPORTS_SMALL_DRIPLEAF)
                || world.fluid_state(support_pos.above()).is(fluid::WATER)
                    && may_place_vegetation_on(support)
        }
        C::StemBlock => support.is_tagged(field_tag(block, "stemSupportBlocks")),
        C::WitherRoseBlock => support.is_tagged(block_tags::SUPPORTS_WITHER_ROSE),
        _ => may_place_vegetation_on(support),
    }
}

/// `LilyPadBlock.mayPlaceOn`：下面是水或者冰，上方不能有流体。
fn lily_pad_may_place_on(support: BlockState, world: &World, support_pos: BlockPos) -> bool {
    let on_fluid = world
        .fluid_state(support_pos)
        .is_tagged(fluid_tags::SUPPORTS_LILY_PAD);
    (on_fluid || support.is_tagged(block_tags::SUPPORTS_LILY_PAD))
        && world.fluid_state(support_pos.above()).is_empty()
}

/// `VegetationBlock`。
pub(super) struct Vegetation;

impl Behaviour for Vegetation {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        may_place_on(state, world.block_state(below), world, below)
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
        vegetation_update_shape(state, world, pos)
    }
}

/// `NetherWartBlock`：每次随机刻十分之一的机会长一级。
pub(super) struct NetherWart;

impl Behaviour for NetherWart {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
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
        vegetation_update_shape(state, world, pos)
    }

    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let age = state.get(AGE_3);
        if age < RIPE_WART_AGE && world.random().next_int(WART_GROWTH_CHANCE) == 0 {
            world.set_block(pos, state.with(AGE_3, age + 1), UPDATE_CLIENTS);
        }
    }
}

/// `NetherFungusBlock`：骨粉长成巨型菌要世界生成的地物，不做；能不能用骨粉、成不成功照官方判断。
pub(super) struct NetherFungus;

impl Behaviour for NetherFungus {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
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
        vegetation_update_shape(state, world, pos)
    }

    /// 长在对应的菌岩上，上面还在世界里。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let required = default_state(field_block(state.block(), "requiredBlock"));
        world.block_state(pos.below()).is_same_block(required)
            && world.is_inside_build_height(pos.above().y)
    }

    /// 官方拿 `nextFloat()` 和 double 的 0.4 比。
    fn is_bonemeal_success(&self, _state: BlockState, world: &World, _pos: BlockPos) -> bool {
        f64::from(world.random().next_float()) < FUNGUS_BONEMEAL_CHANCE
    }
}

/// `SaplingBlock.advanceTree`：第一步只是 `stage` 加一，第二步长成树要世界生成，这里不做。
fn advance_tree(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.get(STAGE) == 0 {
        world.set_block(pos, state.cycle(STAGE), SAPLING_STAGE_FLAGS);
    }
}

/// `SaplingBlock`：长成树要世界生成的地物，骨粉判断能不能长也要用树的配置，都不做。
/// 随机刻的第一步不用地物，照官方做。
pub(super) struct Sapling;

impl Behaviour for Sapling {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
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
        vegetation_update_shape(state, world, pos)
    }

    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.max_local_raw_brightness(pos.above()) >= SAPLING_MIN_BRIGHTNESS
            && world.random().next_int(SAPLING_GROWTH_CHANCE) == 0
        {
            advance_tree(state, world, pos);
        }
    }
}

/// `MushroomBlock`：亮的地方只能长在菌丝这类方块上，暗的地方会往旁边蔓延。
/// 骨粉长成巨型蘑菇要世界生成的地物，不做。
pub(super) struct Mushroom;

impl Mushroom {
    /// `pos.offset(nextInt(3) - 1, nextInt(2) - nextInt(2), nextInt(3) - 1)`。
    fn random_offset(world: &World, pos: BlockPos) -> BlockPos {
        let random = world.random();
        let dx = random.next_int(3) - 1;
        let dy = random.next_int(2) - random.next_int(2);
        let dz = random.next_int(3) - 1;
        pos.offset(dx, dy, dz)
    }

    fn can_spread_to(state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_empty_block(pos) && state.can_survive(world, pos)
    }
}

impl Behaviour for Mushroom {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        let support = world.block_state(below);
        support.is_tagged(block_tags::OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT)
            || world.raw_brightness(pos) < MUSHROOM_MAX_BRIGHTNESS
                && may_place_on(state, support, world, below)
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
        vegetation_update_shape(state, world, pos)
    }

    /// 二十五分之一的机会往附近走几步，停在最后一个能长的空格上。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.random().next_int(MUSHROOM_SPREAD_CHANCE) != 0 || !can_spread(state, world, pos) {
            return;
        }
        let mut from = pos;
        let mut target = Self::random_offset(world, from);
        for _ in 0..MUSHROOM_SPREAD_STEPS {
            if Self::can_spread_to(state, world, target) {
                from = target;
            }
            target = Self::random_offset(world, from);
        }
        if Self::can_spread_to(state, world, target) {
            world.set_block(target, state, UPDATE_CLIENTS);
        }
    }
}

/// `MangrovePropaguleBlock`：红树胎生苗，可以挂在叶子下面。
pub(super) struct MangrovePropagule;

impl Behaviour for MangrovePropagule {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        let state = default_state(block)
            .with(WATERLOGGED, waterlogged)
            .with(AGE_4, FULLY_GROWN_PROPAGULE);
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        if state.get(HANGING) {
            world
                .block_state(pos.above())
                .is_tagged(block_tags::SUPPORTS_HANGING_MANGROVE_PROPAGULE)
        } else {
            Vegetation.can_survive(state, world, pos)
        }
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
        // 官方先单独判定了上面一格的更新，`VegetationBlock.updateShape` 判定的是同一个
        // `canSurvive`，结果一样。
        vegetation_update_shape(state, world, pos)
    }

    /// 挂着的长到 4 级；地上的和树苗一样，七分之一的机会往长成树走一步。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !state.get(HANGING) {
            if world.random().next_int(SAPLING_GROWTH_CHANCE) == 0 {
                advance_tree(state, world, pos);
            }
        } else if state.get(AGE_4) != FULLY_GROWN_PROPAGULE {
            world.set_block(pos, state.cycle(AGE_4), UPDATE_CLIENTS);
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        !state.get(HANGING) || state.get(AGE_4) != FULLY_GROWN_PROPAGULE
    }

    /// 地上的走 `SaplingBlock.isBonemealSuccess`。
    fn is_bonemeal_success(&self, state: BlockState, world: &World, _pos: BlockPos) -> bool {
        if state.get(HANGING) {
            state.get(AGE_4) != FULLY_GROWN_PROPAGULE
        } else {
            f64::from(world.random().next_float()) < SAPLING_BONEMEAL_CHANCE
        }
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(HANGING) && state.get(AGE_4) != FULLY_GROWN_PROPAGULE {
            world.set_block(pos, state.cycle(AGE_4), UPDATE_CLIENTS);
        } else {
            advance_tree(state, world, pos);
        }
    }
}

/// `SeagrassBlock`。
pub(super) struct Seagrass;

impl Behaviour for Seagrass {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        is_full_water(world.fluid_state(ctx.clicked_pos())).then(|| default_state(block))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
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
        let updated = vegetation_update_shape(state, world, pos);
        if !updated.is_air() {
            world.schedule_fluid_tick(pos, fluid::WATER);
        }
        updated
    }

    /// 海草本来就泡在水里，装不了别的。
    fn can_place_liquid(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _fluid: i32,
        _by_player: bool,
    ) -> bool {
        false
    }

    fn place_liquid(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _fluid: FluidState,
    ) -> bool {
        false
    }

    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.block_state(pos.above()).is(blocks::WATER)
    }

    /// 长成两格高的海草。
    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let lower =
            BlockState(blocks::TALL_SEAGRASS).with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Lower);
        let upper = lower.with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper);
        world.set_block(pos, lower, UPDATE_CLIENTS);
        world.set_block(pos.above(), upper, UPDATE_CLIENTS);
    }
}

/// `SweetBerryBushBlock`。
pub(super) struct SweetBerryBush;

impl Behaviour for SweetBerryBush {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
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
        vegetation_update_shape(state, world, pos)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        // 没长满时拿着骨粉不摘果子，让点击落到骨粉自己的 `useOn`。
        if state.get(AGE_3) != RIPE_BUSH_AGE && ctx.item == items::BONE_MEAL {
            InteractionResult::Pass
        } else {
            Plain.use_item_on(state, world, ctx)
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(AGE_3) <= PICKED_BUSH_AGE {
            return Plain.use_without_item(state, world, ctx);
        }
        // `useWithoutItem` 放摘果子的声音，音高随机；不出声也要取这个数，后面的计划刻才不错开。
        let pitch = 0.8 + world.random().next_float() * 0.4;
        world.play_block_sound(
            ctx.clicked_pos(),
            sound_event("minecraft:block.sweet_berry_bush.pick_berries"),
            1.0,
            pitch,
        );
        let picked = state.with(AGE_3, PICKED_BUSH_AGE);
        world.set_block(ctx.clicked_pos(), picked, UPDATE_CLIENTS);
        InteractionResult::Success
    }

    /// 没长满时五分之一的机会长一级，上面要够亮。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let age = state.get(AGE_3);
        if age < RIPE_BUSH_AGE
            && world.random().next_int(BUSH_GROWTH_CHANCE) == 0
            && world.raw_brightness(pos.above()) >= BUSH_MIN_BRIGHTNESS
        {
            world.set_block(pos, state.with(AGE_3, age + 1), UPDATE_CLIENTS);
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        state.get(AGE_3) < RIPE_BUSH_AGE
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let age = (state.get(AGE_3) + 1).min(RIPE_BUSH_AGE);
        world.set_block(pos, state.with(AGE_3, age), UPDATE_CLIENTS);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::testing::{TestLevel, actor_holding, use_in_air};

    /// `PlaceOnWaterBlockItem` 走 `Item.use`，oracle 没有这种题。
    #[test]
    fn a_lily_pad_does_not_go_on_solid_ground() {
        let mut level = TestLevel::new();
        let ground = BlockPos::new(0, 63, 3);
        level.set(ground, BlockState(blocks::STONE));
        let mut actor = actor_holding(items::LILY_PAD);
        actor.position = Vec3::new(0.5, 64.0, 3.5);
        actor.pitch = 90.0;
        let outcome = use_in_air(&mut level, &actor);
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(ground.above()).is_air());
    }
}
