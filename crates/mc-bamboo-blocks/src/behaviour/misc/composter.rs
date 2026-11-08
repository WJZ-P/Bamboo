//! `ComposterBlock`。
//!
//! 放一个物品加几层由物品的 `compostable` 组件决定，抽签用世界的随机数，和官方逐位相同。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{CompostLayers, sounds::sound_event};

use crate::{
    behaviour::{Behaviour, common::item_of},
    context::UseContext,
    interact::InteractionResult,
    random::LegacyRandom,
    state::{BlockState, LEVEL_COMPOSTER},
    world::World,
};

/// `ComposterBlock`。
pub(super) struct Composter;

/// `READY`：可以取骨粉了。
const READY: i32 = 8;
/// `MAX_LEVEL`：满了，等计划刻变成 [`READY`]。
const MAX_LEVEL: i32 = 7;
/// 满了之后等多久变成 [`READY`]。
const READY_DELAY: i32 = 20;

/// `WeightedList.getRandomOrThrow`：`nextInt(总权重)` 落在哪一项。
fn pick_weighted(distribution: &[(i32, i32)], random: &LegacyRandom) -> i32 {
    let total = distribution.iter().map(|&(_, weight)| weight).sum();
    let mut selection = random.next_int(total);
    for &(layers, weight) in distribution {
        selection -= weight;
        if selection < 0 {
            return layers;
        }
    }
    // 抽到的数小于总权重，一定落在上面某一项里。
    0
}

/// `compostable.layers().get(...)`。战利品上下文没有给种子，用的是世界的随机数。
fn layers_to_add(layers: CompostLayers, state: BlockState, world: &World) -> i32 {
    match layers {
        CompostLayers::Constant(layers) => layers,
        CompostLayers::Weighted { when_empty, .. } if state.get(LEVEL_COMPOSTER) == 0 => when_empty,
        CompostLayers::Weighted { distribution, .. } => pick_weighted(distribution, world.random()),
    }
}

/// `addLayer`：没抽中时什么也不改。返回值是方块有没有变，等级事件要看它。
fn add_layer(state: BlockState, world: &mut World, pos: BlockPos, layers: CompostLayers) -> bool {
    let added = layers_to_add(layers, state, world);
    if added <= 0 {
        return false;
    }
    let level = (state.get(LEVEL_COMPOSTER) + added).clamp(0, MAX_LEVEL);
    world.set_block_and_update(pos, state.with(LEVEL_COMPOSTER, level));
    if level == MAX_LEVEL {
        world.schedule_tick(pos, state.block(), READY_DELAY);
    }
    true
}

impl Behaviour for Composter {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if state.get(LEVEL_COMPOSTER) == MAX_LEVEL {
            world.schedule_tick(pos, state.block(), READY_DELAY);
        }
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let level = state.get(LEVEL_COMPOSTER);
        let compostable = item_of(ctx.item).and_then(|item| item.compostable);
        let Some(layers) = compostable.filter(|_| level < READY) else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        if level < MAX_LEVEL {
            let pos = ctx.clicked_pos();
            let changed = add_layer(state, world, pos, layers);
            // `1500`：填充的声音和碎屑都在客户端那边，数据是这一下有没有加层。
            world.level_event(1500, pos, i32::from(changed));
        }
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(LEVEL_COMPOSTER) != READY {
            return InteractionResult::Pass;
        }
        // `extractProduce` 先给掉出来的骨粉算落点，`Vec3.offsetRandomXZ` 取两个 `nextFloat`。
        // 世界里没有物品实体，这两个数照样要取。
        world.random().next_float();
        world.random().next_float();
        let pos = ctx.clicked_pos();
        world.set_block_and_update(pos, state.with(LEVEL_COMPOSTER, 0));
        // `extractProduce` 在 `empty` 之后出声，`null` 作 `except`。
        world.play_block_sound(
            pos,
            sound_event("minecraft:block.composter.empty"),
            1.0,
            1.0,
        );
        InteractionResult::Success
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(LEVEL_COMPOSTER) == MAX_LEVEL {
            world.set_block_and_update(pos, state.with(LEVEL_COMPOSTER, READY));
            world.play_block_sound(
                pos,
                sound_event("minecraft:block.composter.ready"),
                1.0,
                1.0,
            );
        }
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        state.get(LEVEL_COMPOSTER)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::misc::test_support::tick,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const COMPOSTER: BlockPos = BlockPos::new(0, 64, 0);

    fn compost(level: &mut TestLevel, item: i32) -> InteractionResult {
        use_item(level, &actor_holding(item), top_hit(COMPOSTER)).result
    }

    #[test]
    fn an_empty_composter_always_takes_a_layer() {
        let mut level = TestLevel::new();
        level.set(COMPOSTER, BlockState(blocks::COMPOSTER));
        assert_eq!(
            compost(&mut level, items::WHEAT_SEEDS),
            InteractionResult::Success
        );
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), 1);
    }

    /// 种子是 `compostable/low`：不空的时候 `nextInt(100) < 30` 才加一层。
    #[test]
    fn seeds_add_a_layer_on_a_thirty_percent_draw() {
        let mut level = TestLevel::new();
        let expected = LegacyRandom::new(0).next_int(100) < 30;
        level.set(
            COMPOSTER,
            BlockState(blocks::COMPOSTER).with(LEVEL_COMPOSTER, 3),
        );
        assert_eq!(
            compost(&mut level, items::WHEAT_SEEDS),
            InteractionResult::Success
        );
        assert_eq!(
            level.get(COMPOSTER).get(LEVEL_COMPOSTER),
            3 + i32::from(expected)
        );
    }

    #[test]
    fn a_cake_always_adds_a_layer() {
        let mut level = TestLevel::new();
        level.set(
            COMPOSTER,
            BlockState(blocks::COMPOSTER).with(LEVEL_COMPOSTER, 6),
        );
        compost(&mut level, items::CAKE);
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), MAX_LEVEL);
    }

    #[test]
    fn stone_is_not_compostable() {
        // 堆肥桶不认石头，官方接着让物品自己试，石头是方块物品，会在上面放下来。
        // 要点是堆肥桶没有加层。
        let mut level = TestLevel::new();
        level.set(COMPOSTER, BlockState(blocks::COMPOSTER));
        assert_eq!(
            compost(&mut level, items::STONE),
            InteractionResult::Success
        );
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), 0);
    }

    #[test]
    fn a_full_composter_turns_ready_on_its_tick() {
        let mut level = TestLevel::new();
        let full = BlockState(blocks::COMPOSTER).with(LEVEL_COMPOSTER, MAX_LEVEL);
        level.set(COMPOSTER, full);
        tick(&mut level, COMPOSTER);
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), READY);
    }
}
