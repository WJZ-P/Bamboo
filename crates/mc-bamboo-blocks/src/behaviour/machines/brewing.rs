//! 酿造台的方块实体，对应官方 `BrewingStandBlockEntity`。
//!
//! 槽位 0 到 2 是药水瓶、3 是材料、4 是燃料。每刻照官方 `serverTick`：先补燃料，
//! 再看能不能酿、倒计时，最后把三个瓶子空不空写回 `has_bottle_*` 方块状态。
//!
//! 数据槽给菜单读，顺序是酿造进度、剩余燃料、总酿造时间、总燃料。

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{Block, Recipe, RecipeType};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{
    BrewingInput, RecipeManager, assemble, brewing_fuel, crafting_remainder, is_brewing_fuel,
    is_brewing_potion_input, is_brewing_reagent, potion_id,
};

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, update_neighbours_after_destroy},
    },
    block_entity::{BlockEntity, drop_item_stack},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HAS_BOTTLE_0, HAS_BOTTLE_1, HAS_BOTTLE_2},
    world::{UPDATE_CLIENTS, World},
};

/// 第一格药水。
pub const SLOT_FIRST_POTION: usize = 0;
/// 材料格，官方 `INGREDIENT_SLOT`。
pub const SLOT_INGREDIENT: usize = 3;
/// 燃料格，官方 `FUEL_SLOT`。
pub const SLOT_FUEL: usize = 4;
/// 酿造台一共五格。
pub const SLOTS: usize = 5;
/// 数据槽个数，官方 `NUM_DATA_VALUES`。
pub const NUM_DATA_VALUES: usize = 4;

/// `BREWING_TIME_SECONDS`：速度 1 时酿一次要 400 刻。
const BREWING_TIME_TICKS: f32 = 400.0;
/// 燃料或者速度组件缺失时的默认速度。
const DEFAULT_SPEED_MULTIPLIER: f32 = 1.0;
/// `Container.getMaxStackSize()` 的默认值。
const CONTAINER_STACK_LIMIT: i32 = 99;

/// `BrewingStandBlock.HAS_BOTTLE` 的三个属性。
const HAS_BOTTLE: [crate::state::Property<bool>; 3] = [HAS_BOTTLE_0, HAS_BOTTLE_1, HAS_BOTTLE_2];

/// `BrewingStandBlockEntity`。
pub struct BrewingStandEntity {
    pos: BlockPos,
    items: [ItemStack; SLOTS],
    brew_time: i32,
    total_brew_time: i32,
    /// 上一次看到的瓶子状态，官方 `lastPotionCount`。
    last_potion_count: Option<[bool; 3]>,
    /// 开始酿造时材料格里的物品，官方 `ingredient`。空槽的 `air` 不会等于它。
    ingredient: Option<i32>,
    fuel: i32,
    total_fuel: i32,
    speed_multiplier: f32,
    /// 官方 `quickCheck` 缓存的上一份酿造配方。
    last_recipe: Option<&'static Recipe>,
}

impl BrewingStandEntity {
    /// 新建一个空酿造台。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            items: std::array::from_fn(|_| ItemStack::empty()),
            brew_time: 0,
            total_brew_time: 0,
            last_potion_count: None,
            ingredient: None,
            fuel: 0,
            total_fuel: 0,
            speed_multiplier: DEFAULT_SPEED_MULTIPLIER,
            last_recipe: None,
        }
    }

    /// 官方 `getContainerSize`。
    pub const fn slots(&self) -> usize {
        SLOTS
    }

    /// `getItem(slot)`。
    pub fn item(&self, slot: usize) -> &ItemStack {
        &self.items[slot]
    }

    /// 全部内容，给菜单复制用。
    pub fn items(&self) -> &[ItemStack; SLOTS] {
        &self.items
    }

    /// 官方 `ContainerData.get`：`0` 酿造进度、`1` 剩余燃料、
    /// `2` 总酿造时间、`3` 总燃料。
    pub fn data(&self, id: usize) -> i32 {
        match id {
            0 => self.brew_time,
            1 => self.fuel,
            2 => self.total_brew_time,
            3 => self.total_fuel,
            _ => 0,
        }
    }

    /// 菜单读到的全部数据槽。
    pub fn data_values(&self) -> [i32; NUM_DATA_VALUES] {
        std::array::from_fn(|id| self.data(id))
    }

    /// 官方 `setItem`，也就是 `BaseContainerBlockEntity.setItem`：
    /// 写一格，数量按 `getMaxStackSize(stack)` 截断，然后通知。
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        let mut stack = stack;
        let limit = mc_bamboo_recipes::max_stack_size(&stack).min(CONTAINER_STACK_LIMIT);
        if stack.count > limit {
            stack.count = limit;
        }
        self.items[slot] = stack;
    }

    /// 官方 `Container.removeItem`。
    pub fn remove_item(&mut self, slot: usize, amount: i32) -> ItemStack {
        let current = &self.items[slot];
        if current.is_empty() || amount <= 0 {
            return ItemStack::empty();
        }
        let count = amount.min(current.count);
        let mut taken = current.clone();
        taken.count = count;
        self.items[slot].count -= count;
        if self.items[slot].count <= 0 {
            self.items[slot] = ItemStack::empty();
        }
        taken
    }

    /// 官方 `canPlaceItem`。`level` 一定有；材料格查 `BREWING_REAGENTS`，
    /// 药水格要求是药水输入并且这一格现在是空的。
    pub fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        if slot == SLOT_FUEL {
            return is_brewing_fuel(stack);
        }
        if slot == SLOT_INGREDIENT {
            is_brewing_reagent(stack.item)
        } else {
            is_brewing_potion_input(stack.item) && self.items[slot].is_empty()
        }
    }

    /// 官方 `RecipeManager.CachedCheck.getRecipeFor`。
    fn recipe_for(&mut self, input: BrewingInput) -> Option<&'static Recipe> {
        let recipe = RecipeManager::global().get_recipe_for(
            RecipeType::Brewing,
            &input,
            self.last_recipe,
        );
        if let Some(recipe) = recipe {
            self.last_recipe = Some(recipe);
        }
        recipe
    }

    /// 一格药水配当前材料的输入。
    fn input_for(&self, slot: usize) -> BrewingInput {
        let container = &self.items[slot];
        let reagent = &self.items[SLOT_INGREDIENT];
        BrewingInput::new(
            container.clone(),
            potion_id(container),
            reagent.clone(),
            potion_id(reagent),
        )
    }

    /// 官方 `getPotionBits`：三个药水格里有没有东西。
    fn potion_bits(&self) -> [bool; 3] {
        std::array::from_fn(|slot| !self.items[slot].is_empty())
    }

    /// 官方 `isBrewable`。每试一格都会过一遍 `quickCheck`，配方缓存跟着更新。
    fn is_brewable(&mut self) -> bool {
        let ingredient = &self.items[SLOT_INGREDIENT];
        if ingredient.is_empty() || !is_brewing_reagent(ingredient.item) {
            return false;
        }
        for slot in SLOT_FIRST_POTION..SLOT_INGREDIENT {
            if self.items[slot].is_empty() {
                continue;
            }
            let input = self.input_for(slot);
            if self.recipe_for(input).is_some() {
                return true;
            }
        }
        false
    }

    /// 官方 `doBrew`：三个格子各自找配方、组装，然后消耗一个材料。
    fn do_brew(&mut self, world: &mut World) {
        for slot in SLOT_FIRST_POTION..SLOT_INGREDIENT {
            let input = self.input_for(slot);
            if let Some(recipe) = self.recipe_for(input.clone()) {
                // 官方 `items.set(dest, recipe.assemble(input))`，`EMPTY` 也照样写回去。
                self.items[slot] =
                    assemble(recipe, &[input.input.clone(), input.reagent.clone()])
                        .unwrap_or_else(ItemStack::empty);
            }
        }

        let remainder = crafting_remainder(&self.items[SLOT_INGREDIENT]);
        self.items[SLOT_INGREDIENT].count -= 1;
        if self.items[SLOT_INGREDIENT].count <= 0 {
            self.items[SLOT_INGREDIENT] = ItemStack::empty();
        }
        if let Some(remainder) = remainder {
            if self.items[SLOT_INGREDIENT].is_empty() {
                self.items[SLOT_INGREDIENT] = remainder;
            } else {
                drop_item_stack(world, self.pos, &remainder);
            }
        }
        // `levelEvent(1035)` 是中客户端粒子，世界里没有声音和粒子，不发送。
        let _ = world;
    }

    /// 官方 `serverTick`。
    pub fn server_tick(&mut self, world: &mut World) {
        if self.fuel <= 0
            && let Some((uses, speed)) = brewing_fuel(&self.items[SLOT_FUEL])
        {
            self.fuel = uses;
            self.total_fuel = uses;
            self.speed_multiplier = speed;
            let remainder = crafting_remainder(&self.items[SLOT_FUEL]);
            self.items[SLOT_FUEL].count -= 1;
            if self.items[SLOT_FUEL].count <= 0 {
                self.items[SLOT_FUEL] = ItemStack::empty();
            }
            if let Some(remainder) = remainder {
                if self.items[SLOT_FUEL].is_empty() {
                    self.items[SLOT_FUEL] = remainder;
                } else {
                    drop_item_stack(world, self.pos, &remainder);
                }
            }
        }

        let brewable = self.is_brewable();
        let is_brewing = self.brew_time > 0;
        if is_brewing {
            self.brew_time -= 1;
            let done = self.brew_time == 0;
            if done && brewable {
                self.do_brew(world);
            } else if !brewable || self.ingredient != Some(self.items[SLOT_INGREDIENT].item) {
                self.brew_time = 0;
            }
        } else if brewable && self.fuel > 0 {
            let speed = if self.speed_multiplier > 0.0 {
                self.speed_multiplier
            } else {
                DEFAULT_SPEED_MULTIPLIER
            };
            self.fuel -= 1;
            self.brew_time = (BREWING_TIME_TICKS / speed).ceil() as i32;
            self.total_brew_time = self.brew_time;
            self.ingredient = Some(self.items[SLOT_INGREDIENT].item);
        }

        let bits = self.potion_bits();
        if self.last_potion_count != Some(bits) {
            self.last_potion_count = Some(bits);
            let state = world.block_state(self.pos);
            let mut new_state = state;
            for (index, property) in HAS_BOTTLE.into_iter().enumerate() {
                new_state = new_state.with(property, bits[index]);
            }
            if new_state != state {
                // 官方用 `flags = 2`，只通知客户端，不更新邻居也不跑 `onPlace` 的连锁。
                world.set_block(self.pos, new_state, UPDATE_CLIENTS);
            }
        }
    }
}

impl BlockEntity for BrewingStandEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    /// 官方没有覆盖 `preRemoveSideEffects`，基类只把五格内容掉出来。
    fn pre_remove_side_effects(
        &mut self,
        world: &mut World,
        _pos: BlockPos,
        _state: BlockState,
    ) {
        for stack in &self.items {
            drop_item_stack(world, self.pos, stack);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// `BrewingStandBlock`。没有朝向，放下时建一个空方块实体。
pub(super) struct BrewingStand;

impl Behaviour for BrewingStand {
    fn state_for_placement(
        &self,
        block: &'static Block,
        _ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block))
    }

    /// `newBlockEntity`：放下酿造台时建一个空实体。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) && !world.has_block_entity(pos) {
            world.set_block_entity(Box::new(BrewingStandEntity::new(pos)));
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}
