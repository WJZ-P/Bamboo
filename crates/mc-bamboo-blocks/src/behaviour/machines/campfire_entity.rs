//! `CampfireBlockEntity`：四格营火烧炼。
//!
//! 点燃时 `cookTick` 推进进度，到时间就把结果按 `Containers.dropItemStack` 弹出。
//! 熄灭时 `cooldownTick` 每刻把进度减 2。客户端粒子那条 `particleTick` 不在服务端跑。
//! 物品实体没有，弹出的东西在取完随机数之后消失。
//!
//! `CampfireBlockEntity`: four campfire cooking slots.
//!
//! While lit, `cookTick` advances the progress and pops the result through `Containers.dropItemStack`.
//! While unlit, `cooldownTick` subtracts 2 from the progress. The client `particleTick` does not run
//! on the server. There is no item entity, so the popped stack vanishes after its random draws.

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{RecipeKind, RecipeType, registries};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{RecipeManager, assemble};

use crate::{
    block_entity::{BlockEntity, drop_item_stack},
    hopper::{quiet_set_changed, set_changed},
    level::Effect,
    state::{BlockState, LIT},
    world::World,
};

/// 官方 `NUM_SLOTS`。
pub const SLOTS: usize = 4;
/// 官方 `BURN_COOL_SPEED`。
const BURN_COOL_SPEED: i32 = 2;

/// 营火方块实体。
///
/// The campfire block entity.
pub struct CampfireEntity {
    pos: BlockPos,
    items: [ItemStack; SLOTS],
    progress: [i32; SLOTS],
    cooking_time: [i32; SLOTS],
    /// `RecipeManager.CachedCheck` 记住的上一份配方。`placeFood` 不用它。
    ///
    /// The previous recipe remembered by `RecipeManager.CachedCheck`. `placeFood` does not use it.
    last_recipe: Option<&'static mc_bamboo_data::Recipe>,
    /// 上一刻看到的方块状态。状态一变，官方就会换一个新的 `CachedCheck`。
    ///
    /// The block state seen last tick. Vanilla replaces the `CachedCheck` whenever the state changes.
    last_state: BlockState,
}

impl CampfireEntity {
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            items: std::array::from_fn(|_| ItemStack::empty()),
            progress: [0; SLOTS],
            cooking_time: [0; SLOTS],
            last_recipe: None,
            last_state: BlockState(0),
        }
    }

    pub fn items(&self) -> &[ItemStack; SLOTS] {
        &self.items
    }

    /// 机器 oracle 的数据：四格进度，再四格总时间。
    ///
    /// Machine-oracle data: four progress values, then four total times.
    pub fn data_values(&self) -> [i32; SLOTS * 2] {
        let mut values = [0; SLOTS * 2];
        values[..SLOTS].copy_from_slice(&self.progress);
        values[SLOTS..].copy_from_slice(&self.cooking_time);
        values
    }

    /// 录制层要看到的、数据槽之外的内部状态：配方缓存和上一刻的方块状态。
    pub fn recipe_facts(&self) -> (Option<&'static str>, u32) {
        (self.last_recipe.map(|recipe| recipe.id), self.last_state.0)
    }

    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if let Some(stored) = self.items.get_mut(slot) {
            *stored = if stack.is_empty() {
                ItemStack::empty()
            } else {
                stack
            };
        }
    }

    pub fn set_data(&mut self, id: usize, value: i32) {
        if id < SLOTS {
            self.progress[id] = value;
        } else if id < SLOTS * 2 {
            self.cooking_time[id - SLOTS] = value;
        }
    }

    /// 官方 `placeFood`。配方用 `getRecipeFor`，不走 cook tick 的缓存。
    ///
    /// 手上那一格的消耗由调用方处理。这里只留下一份数量为 1 的副本。
    ///
    /// Vanilla `placeFood`. The recipe comes from `getRecipeFor`, not the cook-tick cache.
    ///
    /// The caller consumes the held stack. This keeps a count-1 copy.
    pub fn place_food(&mut self, world: &mut World, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        let Some(slot) = self.items.iter().position(ItemStack::is_empty) else {
            return false;
        };
        let Some(recipe) =
            RecipeManager::global().get_recipe_for(RecipeType::CampfireCooking, stack, None)
        else {
            return false;
        };
        let RecipeKind::Cooking { time, .. } = recipe.kind else {
            return false;
        };
        self.cooking_time[slot] = time;
        self.progress[slot] = 0;
        let mut placed = stack.clone();
        placed.count = 1;
        self.items[slot] = placed;
        world.push_effect(Effect::CampfireChanged { pos: self.pos });
        set_changed(world, self.pos);
        true
    }

    pub fn server_tick(&mut self, world: &mut World) {
        let state = world.block_state(self.pos);
        // 官方的 ticker 包装先看 `BlockEntityType.isValid`。不是营火就不跑。
        // Vanilla's ticker wrapper checks `BlockEntityType.isValid` first and skips anything else.
        if state.block().block_entity_type != Some(registries::block_entity_type::CAMPFIRE) {
            return;
        }
        // `getTicker` 在方块状态变化时重做，里面新建一个 `CachedCheck`。
        // `getTicker` runs again when the block state changes and builds a new `CachedCheck`.
        if state != self.last_state {
            self.last_recipe = None;
            self.last_state = state;
        }
        if state.try_get(LIT) == Some(true) {
            self.cook(world);
        } else {
            self.cooldown(world);
        }
    }

    fn cook(&mut self, world: &mut World) {
        let mut occupied = false;
        let mut finished = false;
        for slot in 0..SLOTS {
            if self.items[slot].is_empty() {
                continue;
            }
            occupied = true;
            self.progress[slot] = self.progress[slot].wrapping_add(1);
            if self.progress[slot] < self.cooking_time[slot] {
                continue;
            }
            let result = self.cooked_result(slot);
            // 功能开关没做。官方结果被关掉时会一直加进度、不弹出。
            // Item feature flags are not implemented. Vanilla keeps incrementing and does not pop
            // a result that `isItemEnabled` rejects.
            drop_item_stack(world, self.pos, &result);
            self.items[slot] = ItemStack::empty();
            finished = true;
        }
        if finished {
            world.push_effect(Effect::CampfireChanged { pos: self.pos });
        }
        if occupied {
            // 营火没有比较器输出。`setChanged` 仍然通知旁边的比较器；
            // 没烤熟的那些刻物品不变，通知只是安静刻通知。
            // A campfire has no comparator output. `setChanged` still notifies neighbouring
            // comparators. On ticks without a finished item it is a quiet notification.
            quiet_set_changed(world, self.pos, !finished);
        }
    }

    fn cooked_result(&mut self, slot: usize) -> ItemStack {
        let input = &self.items[slot];
        let recipe = RecipeManager::global().get_recipe_for(
            RecipeType::CampfireCooking,
            input,
            self.last_recipe,
        );
        let Some(recipe) = recipe else {
            return input.clone();
        };
        self.last_recipe = Some(recipe);
        assemble(recipe, std::slice::from_ref(input)).unwrap_or_else(|| input.clone())
    }

    fn cooldown(&mut self, world: &mut World) {
        let mut changed = false;
        for slot in 0..SLOTS {
            if self.progress[slot] > 0 {
                changed = true;
                let cooled = self.progress[slot] - BURN_COOL_SPEED;
                self.progress[slot] = cooled.clamp(0, self.cooking_time[slot]);
            }
        }
        if changed {
            // 冷却的物品一直没变，只是进度在退。
            quiet_set_changed(world, self.pos, true);
        }
    }
}

impl BlockEntity for CampfireEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    /// 官方 `preRemoveSideEffects`：四格都走 `dropItemStack`，空格也取掉落点。
    ///
    /// Vanilla `preRemoveSideEffects`: all four slots go through `dropItemStack`, and an empty
    /// slot still draws the drop point.
    fn pre_remove_side_effects(&mut self, world: &mut World, pos: BlockPos, _state: BlockState) {
        for stack in &self.items {
            drop_item_stack(world, pos, stack);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        use crate::archive::tag::put;
        let mut out = Vec::new();
        put(
            &mut out,
            "Items",
            crate::archive::block_entity::items_tag(self.items())?,
        );
        put(
            &mut out,
            "CookingTimes",
            Tag::IntArray(self.progress.to_vec()),
        );
        put(
            &mut out,
            "CookingTotalTimes",
            Tag::IntArray(self.cooking_time.to_vec()),
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::{
        BlockTicks, InteractionResult, ItemExchange, LegacyRandom, LevelRandom, World,
        interact::use_item_on_with_stack,
        state::{BlockState, LIT},
        testing::{TestLevel, actor_holding, top_hit},
        world::UPDATE_ALL,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn lit_campfire(level: &mut TestLevel) {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        world.set_block(POS, BlockState(blocks::CAMPFIRE), UPDATE_ALL);
    }

    #[test]
    fn a_potato_lands_in_the_first_empty_slot_and_cooks_for_600() {
        let mut level = TestLevel::new();
        lit_campfire(&mut level);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let placed = world
            .with_block_entity::<CampfireEntity, _>(POS, |entity, world| {
                entity.place_food(world, &ItemStack::new(items::POTATO, 3))
            })
            .expect("the placed campfire has an entity");
        assert!(placed);
        drop(world);
        let entity = level.block_entity::<CampfireEntity>(POS).unwrap();
        assert_eq!(entity.items[0].item, items::POTATO);
        assert_eq!(entity.items[0].count, 1);
        assert!(entity.items[1].is_empty());
        assert_eq!(entity.cooking_time[0], 600);
        assert_eq!(entity.progress[0], 0);
    }

    #[test]
    fn a_finished_potato_draws_one_dropped_stack_and_clears_the_slot() {
        let mut level = TestLevel::new();
        level.random = LevelRandom::new(9);
        lit_campfire(&mut level);
        level.random = LevelRandom::new(9);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.with_block_entity::<CampfireEntity, _>(POS, |entity, _| {
            entity.set_item(0, ItemStack::new(items::POTATO, 1));
            entity.set_data(0, 599);
            entity.set_data(4, 600);
        });
        world.tick_block_entities();
        drop(world);
        let entity = level.block_entity::<CampfireEntity>(POS).unwrap();
        assert!(entity.items[0].is_empty());
        assert_eq!(entity.progress[0], 600);
        let expected = LegacyRandom::new(9);
        for _ in 0..3 {
            expected.next_double();
        }
        let _ = expected.next_int(21) + 10;
        for _ in 0..6 {
            expected.next_double();
        }
        assert_eq!(level.random.source().seed(), expected.seed());
    }

    #[test]
    fn an_unlit_campfire_cools_by_two_without_drawing_random() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block(
            POS,
            BlockState(blocks::CAMPFIRE).with(LIT, false),
            UPDATE_ALL,
        );
        level.random = LevelRandom::new(4);
        let before = level.random.source().seed();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.with_block_entity::<CampfireEntity, _>(POS, |entity, _| {
            entity.set_item(0, ItemStack::new(items::POTATO, 1));
            entity.set_data(0, 3);
            entity.set_data(4, 600);
        });
        world.tick_block_entities();
        drop(world);
        let entity = level.block_entity::<CampfireEntity>(POS).unwrap();
        assert_eq!(entity.progress[0], 1);
        assert_eq!(entity.items[0].item, items::POTATO);
        assert_eq!(level.random.source().seed(), before);
    }

    #[test]
    fn using_a_potato_places_food_and_asks_to_shrink_the_hand() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::CAMPFIRE));
        let mut ticks = BlockTicks::new();
        let potato = ItemStack::new(items::POTATO, 2);
        let mut actor = actor_holding(items::POTATO);
        // 测试用的默认玩家是创造模式，不消耗手里的物品。
        // The default test actor is creative and does not consume the held item.
        actor.prevents_block_drops = false;
        let outcome = use_item_on_with_stack(
            &mut level,
            &mut ticks,
            &actor,
            Hand::Main,
            &top_hit(POS),
            &potato,
        );
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert_eq!(outcome.item_exchange, Some(ItemExchange::Shrink));
        let entity = level.block_entity::<CampfireEntity>(POS).unwrap();
        assert_eq!(entity.items[0].count, 1);
        assert_eq!(entity.cooking_time[0], 600);
    }
}
