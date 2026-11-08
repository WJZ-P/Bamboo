//! `BucketItem` 和 `MobBucketItem` 的 `use`：用空桶舀，用装满的桶倒。
//!
//! 桶不覆盖 `useOn`，点方块时方块不接，官方的客户端接着发 `use_item`，服务端从眼睛看出去找目标，
//! 所以这里只有 [`use_item`](crate::use_item) 一个入口。细雪桶是方块物品，放置在 `placement` 里。
//! 全按创造模式写：倒完手里还是满桶，舀完手里还是空桶。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockAncestor, BlockClass, items, sounds, sounds::sound_event};

use super::Fluid;
use crate::{
    context::{Actor, BlockHit},
    interact::InteractionResult,
    level::Effect,
    state::BlockState,
    world::{ClipFluid, UPDATE_ALL_IMMEDIATE, UPDATE_LIMIT, World},
};

/// 一种桶。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket {
    /// `content`：桶里的流体，空桶和装着硫磺史莱姆的桶是 `None`。
    pub content: Option<Fluid>,
    /// `MobBucketItem`：桶里还有一只生物。这个世界没有实体，倒出来的只有水。
    pub mob: bool,
}

impl Bucket {
    /// 这个物品是哪种桶，不是 `BucketItem` 时是 `None`。
    pub const fn of(item: i32) -> Option<Self> {
        let (content, mob) = match item {
            items::BUCKET => (None, false),
            items::WATER_BUCKET => (Some(Fluid::Water), false),
            items::LAVA_BUCKET => (Some(Fluid::Lava), false),
            items::PUFFERFISH_BUCKET
            | items::SALMON_BUCKET
            | items::COD_BUCKET
            | items::TROPICAL_FISH_BUCKET
            | items::AXOLOTL_BUCKET
            | items::TADPOLE_BUCKET => (Some(Fluid::Water), true),
            items::SULFUR_CUBE_BUCKET => (None, true),
            _ => return None,
        };
        Some(Self { content, mob })
    }

    /// `getFluidContext`：空桶的视线停在源头上，别的桶穿过流体。
    const fn clip_fluid(self) -> ClipFluid {
        if self.content.is_none() && !self.mob {
            ClipFluid::SourceOnly
        } else {
            ClipFluid::None
        }
    }
}

/// `BucketItem.use`。
pub(crate) fn use_bucket(world: &mut World, actor: &Actor, bucket: Bucket) -> InteractionResult {
    let (from, to) = actor.view_segment();
    let Some(hit) = world.clip(from, to, bucket.clip_fluid(), actor.main_hand) else {
        return InteractionResult::Pass;
    };
    let pos = hit.pos;
    // `mayInteract`：没有出生点保护，只看世界边界。创造模式的 `mayUseItemAt` 总是真。
    if !world.is_within_world_border(pos) {
        return InteractionResult::Fail;
    }
    let clicked = world.block_state(pos);
    let into_clicked =
        clicked.is_a(BlockAncestor::LiquidBlockContainer) && bucket.content == Some(Fluid::Water);
    let place_pos = if into_clicked {
        pos
    } else {
        pos.relative(hit.face)
    };
    if empty_contents(world, actor, bucket, place_pos, Some(hit)) {
        return InteractionResult::Success;
    }
    if bucket.content.is_none() {
        return pick_up(world, actor, pos);
    }
    InteractionResult::Fail
}

/// `getPickupSound`：水、岩浆和细雪各有各的舀起声音。
fn pickup_sound(item: i32) -> &'static str {
    if item == items::LAVA_BUCKET {
        "minecraft:item.bucket.fill_lava"
    } else if item == items::POWDER_SNOW_BUCKET {
        "minecraft:item.bucket.fill_powder_snow"
    } else {
        "minecraft:item.bucket.fill"
    }
}

/// 空桶舀起 `pos` 的东西。
fn pick_up(world: &mut World, actor: &Actor, pos: BlockPos) -> InteractionResult {
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::BucketPickup) {
        return InteractionResult::Fail;
    }
    let taken = state.pickup_block(world, pos, true);
    if taken == items::AIR {
        return InteractionResult::Fail;
    }
    world.push_effect(Effect::FilledBucket { item: taken });
    // 官方的 `player.playSound`：`PLAYERS` 来源，在玩家脚下，跳过这名玩家。
    world.play_sound(
        actor.position,
        sound_event(pickup_sound(taken)),
        sounds::PLAYERS,
        1.0,
        1.0,
        true,
    );
    InteractionResult::Success
}

/// `playEmptySound`：倒岩浆用岩浆的声音，倒水用普通的水声。
fn play_empty_sound(world: &mut World, pos: BlockPos, fluid: Fluid) {
    let sound = if fluid == Fluid::Lava {
        "minecraft:item.bucket.empty_lava"
    } else {
        "minecraft:item.bucket.empty"
    };
    world.play_block_sound_except(pos, sound_event(sound), 1.0, 1.0);
}

/// `emptyContents`：把桶里的东西倒在 `pos`。`hit` 是视线的结果，倒不进去时改倒在它的面外面一格，
/// 那次调用 `hit` 是空的。
fn empty_contents(
    world: &mut World,
    actor: &Actor,
    bucket: Bucket,
    pos: BlockPos,
    hit: Option<BlockHit>,
) -> bool {
    // 空桶不是 `FlowingFluid`，倒不出东西；`MobBucketItem` 里没有流体时只放出生物，算成功。
    let Some(fluid) = bucket.content else {
        return bucket.mob;
    };
    let state = world.block_state(pos);
    let may_replace = can_be_replaced_by_fluid(state);
    let into_container = state.is_a(BlockAncestor::LiquidBlockContainer)
        && state.can_place_liquid(world, pos, fluid.source(), true);
    let place_liquid = may_replace || into_container;
    let place_inside = state.is_air() || place_liquid && (!actor.secondary_use || hit.is_none());
    if !place_inside {
        return hit.is_some_and(|hit| {
            let beside = hit.pos.relative(hit.face);
            empty_contents(world, actor, bucket, beside, None)
        });
    }
    // 主世界的水不会蒸发。
    if state.is_a(BlockAncestor::LiquidBlockContainer) && fluid == Fluid::Water {
        state.place_liquid(world, pos, fluid.source_state());
        play_empty_sound(world, pos, fluid);
        return true;
    }
    if may_replace && !state.is_liquid() {
        world.destroy_block(pos, UPDATE_LIMIT);
    }
    let placed = fluid.source_state().legacy_block();
    let placed_now =
        world.set_block(pos, placed, UPDATE_ALL_IMMEDIATE) || state.fluid().is_source();
    if placed_now {
        play_empty_sound(world, pos, fluid);
    }
    placed_now
}

/// `canBeReplaced(Fluid)`：能替换的方块和不算实心的方块。只有末地传送门和末地折跃门覆盖了它，永远不行。
fn can_be_replaced_by_fluid(state: BlockState) -> bool {
    if matches!(
        state.class(),
        BlockClass::EndPortalBlock | BlockClass::EndGatewayBlock
    ) {
        return false;
    }
    state.is_replaceable() || !state.is_solid()
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockTicks, Outcome,
        state::{LEVEL, SLAB_TYPE, SlabType, WATERLOGGED},
        testing::{TestLevel, actor_holding},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 63, 3);

    /// 站在原点上空往下看着 `FLOOR` 的玩家。
    fn looking_down(item: i32) -> Actor {
        let mut actor = actor_holding(item);
        actor.position = Vec3::new(0.5, 65.0, 3.5);
        actor.pitch = 90.0;
        actor
    }

    fn use_in_air(level: &mut TestLevel, actor: &Actor) -> Outcome {
        crate::use_item(level, &mut BlockTicks::new(), actor, Hand::Main)
    }

    #[test]
    fn every_bucket_item_is_known() {
        let water = Bucket::of(items::WATER_BUCKET).expect("水桶是桶");
        assert_eq!(water.content, Some(Fluid::Water));
        assert_eq!(
            Bucket::of(items::COD_BUCKET).map(|bucket| bucket.mob),
            Some(true)
        );
        assert_eq!(Bucket::of(items::POWDER_SNOW_BUCKET), None);
        assert_eq!(Bucket::of(items::MILK_BUCKET), None);
    }

    #[test]
    fn a_water_bucket_pours_a_source_on_the_floor() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        let outcome = use_in_air(&mut level, &looking_down(items::WATER_BUCKET));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR.above()), BlockState::WATER);
        // 倒水的声音在方块中心，跳过倒水的玩家。
        assert!(matches!(
            outcome.effects.as_slice(),
            [Effect::Sound {
                sound,
                source,
                volume,
                pitch,
                except_initiator: true,
                ..
            }] if *sound == i32::from(sound_event("minecraft:item.bucket.empty"))
                && *source == sounds::BLOCKS
                && *volume == 1.0
                && *pitch == 1.0
        ));
    }

    #[test]
    fn an_empty_bucket_scoops_up_a_source() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::LAVA));
        let outcome = use_in_air(&mut level, &looking_down(items::BUCKET));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(FLOOR).is_air());
        assert_eq!(
            outcome.effects[0],
            Effect::FilledBucket {
                item: items::LAVA_BUCKET
            }
        );
        // 舀岩浆的声音在玩家脚下，跳过这名玩家。
        assert!(matches!(
            outcome.effects.get(1),
            Some(Effect::Sound {
                sound,
                source,
                except_initiator: true,
                ..
            }) if *sound == i32::from(sound_event("minecraft:item.bucket.fill_lava"))
                && *source == sounds::PLAYERS
        ));
    }

    #[test]
    fn an_empty_bucket_ignores_flowing_water() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState::WATER.with(LEVEL, 3));
        level.set(FLOOR.below(), BlockState(blocks::STONE));
        let outcome = use_in_air(&mut level, &looking_down(items::BUCKET));
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert_eq!(level.get(FLOOR), BlockState::WATER.with(LEVEL, 3));
    }

    #[test]
    fn water_goes_into_a_waterloggable_block() {
        let mut level = TestLevel::new();
        let slab = BlockState(blocks::OAK_SLAB);
        level.set(FLOOR, slab);
        let outcome = use_in_air(&mut level, &looking_down(items::WATER_BUCKET));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR), slab.with(WATERLOGGED, true));

        // 双层台阶装不了水，倒在上面一格。
        let double = slab.with(SLAB_TYPE, SlabType::Double);
        level.set(FLOOR, double);
        use_in_air(&mut level, &looking_down(items::WATER_BUCKET));
        assert_eq!(level.get(FLOOR), double);
        assert_eq!(level.get(FLOOR.above()), BlockState::WATER);
    }

    #[test]
    fn a_waterlogged_block_can_be_drained() {
        let mut level = TestLevel::new();
        let wet = BlockState(blocks::OAK_SLAB).with(WATERLOGGED, true);
        level.set(FLOOR, wet);
        let outcome = use_in_air(&mut level, &looking_down(items::BUCKET));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR), wet.with(WATERLOGGED, false));
    }

    #[test]
    fn lava_washes_away_a_plant() {
        let mut level = TestLevel::new();
        let wall = BlockPos::new(0, 64, 3);
        let plant = wall.north();
        level.set(wall, BlockState(blocks::STONE));
        level.set(plant.below(), BlockState(blocks::GRASS_BLOCK));
        level.set(plant, BlockState(blocks::SHORT_GRASS));
        // 视线从草的顶上擦过去，碰到后面的墙，倒在墙前面，也就是草那一格。
        let mut actor = actor_holding(items::LAVA_BUCKET);
        actor.position = Vec3::new(0.5, 63.3, 0.5);
        let outcome = use_in_air(&mut level, &actor);
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(plant), BlockState(blocks::LAVA));
        assert!(outcome.effects.iter().any(|effect| matches!(
            effect,
            Effect::Destroyed {
                except_initiator: false,
                ..
            }
        )));
    }

    #[test]
    fn crouching_pours_next_to_a_container_instead_of_into_it() {
        let mut level = TestLevel::new();
        let slab = BlockState(blocks::OAK_SLAB);
        level.set(FLOOR, slab);
        let mut actor = looking_down(items::WATER_BUCKET);
        actor.secondary_use = true;
        // 视线点中的就是台阶本身，官方照样先试台阶，蹲着时改倒在上面一格。
        use_in_air(&mut level, &actor);
        assert_eq!(level.get(FLOOR), slab);
        assert_eq!(level.get(FLOOR.above()), BlockState::WATER);
    }

    #[test]
    fn nothing_in_sight_passes() {
        let mut level = TestLevel::new();
        let outcome = use_in_air(&mut level, &looking_down(items::WATER_BUCKET));
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn a_fish_bucket_pours_water_and_a_sulfur_cube_bucket_only_succeeds() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        let outcome = use_in_air(&mut level, &looking_down(items::SULFUR_CUBE_BUCKET));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(FLOOR.above()).is_air());
        use_in_air(&mut level, &looking_down(items::SALMON_BUCKET));
        assert_eq!(level.get(FLOOR.above()), BlockState::WATER);
    }
}
