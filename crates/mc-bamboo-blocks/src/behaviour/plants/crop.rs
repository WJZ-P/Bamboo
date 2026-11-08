//! `CropBlock`、`BeetrootBlock`、`CarrotBlock`、`PotatoBlock`、`TorchflowerCropBlock`、`StemBlock`、
//! `AttachedStemBlock`：作物和茎的生长、骨粉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass, blocks, tags::block_tags};

use super::vegetation::Vegetation;
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, field_block, field_int, field_tag, vegetation_update_shape},
    },
    state::{AGE_1, AGE_3, AGE_7, BlockState, HORIZONTAL_FACING, MOISTURE, Property},
    world::{UPDATE_CLIENTS, World},
};

/// `CropBlock.hasSufficientLight` 要求的亮度。
const CROP_MIN_BRIGHTNESS: i32 = 8;
/// `randomTick` 里作物和茎要这么亮才长。
const GROWTH_MIN_BRIGHTNESS: i32 = 9;
/// `StemBlock.MAX_AGE`：瓜被摘走后，连着瓜的茎退回长满的茎。
const FULLY_GROWN_STEM: i32 = 7;
/// 骨粉一次长 2 到 5 级，`Mth.nextInt(random, 2, 5)`。
const BONEMEAL_MIN_GROWTH: i32 = 2;
const BONEMEAL_MAX_GROWTH: i32 = 5;
/// `BeetrootBlock.getBonemealAgeIncrease` 把上面的结果除以 3。
const BEETROOT_BONEMEAL_DIVISOR: i32 = 3;

/// `CropBlock.hasSufficientLight`。
pub(super) fn has_sufficient_light(world: &World, pos: BlockPos) -> bool {
    world.raw_brightness(pos) >= CROP_MIN_BRIGHTNESS
}

/// `getAgeProperty`：甜菜根是 0 到 3，火把花作物是 0 到 1，别的是 0 到 7。
fn age_property(class: BlockClass) -> Property<i32> {
    match class {
        BlockClass::BeetrootBlock => AGE_3,
        BlockClass::TorchflowerCropBlock => AGE_1,
        _ => AGE_7,
    }
}

/// `getMaxAge`。火把花作物的最大值 2 不是它的状态，长到 2 就换成火把花。
fn max_age(block: &Block) -> i32 {
    field_int(block, "getMaxAge()")
}

/// `getAge`。
fn age(state: BlockState) -> i32 {
    state.get(age_property(state.class()))
}

/// `getStateForAge`。
fn state_for_age(block: &'static Block, age: i32) -> BlockState {
    if block.class == BlockClass::TorchflowerCropBlock && age == max_age(block) {
        return BlockState(blocks::TORCHFLOWER);
    }
    default_state(block).with(age_property(block.class), age)
}

/// `CropBlock.getGrowthSpeed`：脚下和周围的耕地越多、越湿长得越快，同种作物挤在一起慢一半。
/// 浮点数的运算顺序和官方一样，`25 / speed` 取整的边界上差一位结果就变。
fn growth_speed(block: &Block, world: &World, pos: BlockPos) -> f32 {
    let below = pos.below();
    let mut speed = 1.0_f32;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let mut block_speed = soil_speed(world.block_state(below.offset(dx, 0, dz)));
            if dx != 0 || dz != 0 {
                block_speed /= 4.0;
            }
            speed += block_speed;
        }
    }
    if is_crowded(block, world, pos) {
        speed /= 2.0;
    }
    speed
}

/// 一格耕地对生长的贡献：湿的 3，干的 1，不是耕地 0。
fn soil_speed(soil: BlockState) -> f32 {
    if !soil.is_tagged(block_tags::GROWS_CROPS) {
        0.0
    } else if soil.try_get(MOISTURE).unwrap_or(0) > 0 {
        3.0
    } else {
        1.0
    }
}

/// 同种作物在两个轴上都挨着，或者斜对角有同种作物。
fn is_crowded(block: &Block, world: &World, pos: BlockPos) -> bool {
    let same = |at: BlockPos| world.block_state(at).is(block.default_state);
    let (north, south, west, east) = (pos.north(), pos.south(), pos.west(), pos.east());
    let along_x = same(west) || same(east);
    let along_z = same(north) || same(south);
    if along_x && along_z {
        return true;
    }
    same(west.north()) || same(east.north()) || same(east.south()) || same(west.south())
}

/// `random.nextInt((int) (25 / growthSpeed) + 1) == 0`：作物和茎每次随机刻长一级的机会。
pub(super) fn rolls_growth(block: &Block, world: &World, pos: BlockPos) -> bool {
    let speed = growth_speed(block, world, pos);
    world.random().next_int((25.0 / speed) as i32 + 1) == 0
}

/// `CropBlock.getBonemealAgeIncrease`。
fn bonemeal_increase(class: BlockClass, world: &World) -> i32 {
    let roll = || {
        world
            .random()
            .mth_next_int(BONEMEAL_MIN_GROWTH, BONEMEAL_MAX_GROWTH)
    };
    match class {
        BlockClass::TorchflowerCropBlock => 1,
        BlockClass::BeetrootBlock => roll() / BEETROOT_BONEMEAL_DIVISOR,
        _ => roll(),
    }
}

/// `CropBlock`。
pub(super) struct Crop;

impl Crop {
    /// `CropBlock.randomTick`。
    fn grow(state: BlockState, world: &mut World, pos: BlockPos) {
        if world.raw_brightness(pos) < GROWTH_MIN_BRIGHTNESS {
            return;
        }
        let block = state.block();
        let age = age(state);
        if age < max_age(block) && rolls_growth(block, world, pos) {
            world.set_block(pos, state_for_age(block, age + 1), UPDATE_CLIENTS);
        }
    }
}

impl Behaviour for Crop {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sufficient_light(world, pos) && Vegetation.can_survive(state, world, pos)
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

    /// 甜菜根和火把花先掷一次三分之一的骰子，掷中了就这一次不长。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let slower = matches!(
            state.class(),
            BlockClass::BeetrootBlock | BlockClass::TorchflowerCropBlock
        );
        if !slower || world.random().next_int(3) != 0 {
            Self::grow(state, world, pos);
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        age(state) < max_age(state.block())
    }

    /// `growCrops`。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let block = state.block();
        let grown = (age(state) + bonemeal_increase(block.class, world)).min(max_age(block));
        world.set_block(pos, state_for_age(block, grown), UPDATE_CLIENTS);
    }
}

/// `StemBlock`：长满以后在旁边结瓜，自己变成连着瓜的茎。
pub(super) struct Stem;

impl Stem {
    /// 长满的茎往随机一个水平方向结瓜：那一格是空气，下面是能长瓜的方块。
    fn bear_fruit(state: BlockState, world: &mut World, pos: BlockPos) {
        let direction = world.random().horizontal_direction();
        let fruit_pos = pos.relative(direction);
        let block = state.block();
        let ground = world.block_state(fruit_pos.below());
        if world.block_state(fruit_pos).is_air()
            && ground.is_tagged(field_tag(block, "fruitSupportBlocks"))
        {
            let fruit = default_state(field_block(block, "fruit"));
            world.set_block_and_update(fruit_pos, fruit);
            let attached = default_state(field_block(block, "attachedStem"));
            world.set_block_and_update(pos, attached.with(HORIZONTAL_FACING, direction));
        }
    }
}

impl Behaviour for Stem {
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
        if world.raw_brightness(pos) < GROWTH_MIN_BRIGHTNESS
            || !rolls_growth(state.block(), world, pos)
        {
            return;
        }
        let age = state.get(AGE_7);
        if age < FULLY_GROWN_STEM {
            world.set_block(pos, state.with(AGE_7, age + 1), UPDATE_CLIENTS);
        } else {
            Self::bear_fruit(state, world, pos);
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        state.get(AGE_7) != FULLY_GROWN_STEM
    }

    /// 长满时接着做一次随机刻，可能马上结瓜。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let growth = world
            .random()
            .mth_next_int(BONEMEAL_MIN_GROWTH, BONEMEAL_MAX_GROWTH);
        let age = (state.get(AGE_7) + growth).min(FULLY_GROWN_STEM);
        let grown = state.with(AGE_7, age);
        world.set_block(pos, grown, UPDATE_CLIENTS);
        if age == FULLY_GROWN_STEM {
            grown.random_tick(world, pos);
        }
    }
}

/// `AttachedStemBlock`：连着瓜的茎。
pub(super) struct AttachedStem;

impl Behaviour for AttachedStem {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Vegetation.can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let block = state.block();
        let fruit = default_state(field_block(block, "fruit"));
        if !neighbor.is_same_block(fruit) && direction == state.get(HORIZONTAL_FACING) {
            return default_state(field_block(block, "stem")).with(AGE_7, FULLY_GROWN_STEM);
        }
        vegetation_update_shape(state, world, pos)
    }
}
