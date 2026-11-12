//! 熔炉、高炉、烟熏炉的方块实体：`AbstractFurnaceBlockEntity` 和它的三个子类。
//!
//! ticker 逐行照官方 `serverTick`：燃料、烹饪进度、`lit` 方块状态和配方统计都在这里。
//! 取走结果时官方会给经验，这个世界没有经验球，但 `createExperience` 和
//! `ExperienceOrb.award` 抽的世界随机数一个都不能少，否则后面的随机刻就错位了。
//!
//! 数据槽（`ContainerData`）给菜单读，顺序是剩余燃烧时间、总燃烧时间、
//! 烹饪进度、总烹饪时间。

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockClass, CookingKind, Recipe, RecipeKind, RecipeType, items};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{
    RecipeManager, assemble, cooking_fuel, crafting_remainder, max_stack_size,
    same_item_same_components,
};

use crate::{
    block_entity::{BlockEntity, block_entity_clock, drop_item_stack},
    state::{BlockState, LIT},
    world::World,
};

/// 原料格。
pub const SLOT_INPUT: usize = 0;
/// 燃料格。
pub const SLOT_FUEL: usize = 1;
/// 结果格。
pub const SLOT_RESULT: usize = 2;
/// 熔炉系列的格数。
pub const SLOTS: usize = 3;
/// 数据槽个数，官方 `NUM_DATA_VALUES`。
pub const NUM_DATA_VALUES: usize = 4;
/// 找不到配方时的烹饪时间，官方 `BURN_TIME_STANDARD`。
const BURN_TIME_STANDARD: i32 = 200;
/// 没在烧时每刻回落的进度，官方 `BURN_COOL_SPEED`。
const BURN_COOL_SPEED: i32 = 2;
/// `Container.getMaxStackSize()` 的默认值。
const CONTAINER_STACK_LIMIT: i32 = 99;

/// `ExperienceOrb.getExperienceValue` 的分段上限。
const EXPERIENCE_VALUES: [i32; 10] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3];

/// `AbstractFurnaceBlockEntity`。
///
/// 官方有三个子类，区别只在 `RecipeType` 和默认名字；这里用 [`CookingKind`] 区分。
pub struct FurnaceEntity {
    pos: BlockPos,
    kind: CookingKind,
    /// 三格内容，下标见 [`SLOT_INPUT`]、[`SLOT_FUEL`]、[`SLOT_RESULT`]。
    items: [ItemStack; SLOTS],
    lit_time_remaining: i32,
    lit_total_time: i32,
    cooking_timer: i32,
    cooking_total_time: i32,
    speed_multiplier: f32,
    /// 官方 `recipesUsed`：配方 ID 和做过的次数。
    recipes_used: Vec<(&'static str, i32)>,
    /// 官方 `quickCheck` 缓存的上一份配方。
    last_recipe: Option<&'static Recipe>,
    /// 一段没有可见事件的安静刻：`since` 之后每刻只按 `lit_step` / `cook_step`
    /// 线性推进两个数值，直到 `wake_at` 那一刻必须真跑 `server_tick`。
    sleep: Option<Sleep>,
}

/// 熔炉安静刻的线性公式。数值在 `since` 那一刻已经是真跑完的值。
///
/// `cook_step` 为 -2 时按官方 `Mth.clamp(timer - 2 * steps, 0, cooking_total_time)` 算。
#[derive(Clone, Copy)]
struct Sleep {
    since: i64,
    wake_at: i64,
    lit_step: i32,
    cook_step: i32,
}

impl FurnaceEntity {
    /// 新建一个空熔炉。`kind` 决定查哪种烹饪配方。
    pub fn new(pos: BlockPos, kind: CookingKind) -> Self {
        Self {
            pos,
            kind,
            items: std::array::from_fn(|_| ItemStack::empty()),
            lit_time_remaining: 0,
            lit_total_time: 0,
            cooking_timer: 0,
            cooking_total_time: 0,
            speed_multiplier: 1.0,
            recipes_used: Vec::new(),
            last_recipe: None,
            sleep: None,
        }
    }

    /// 方块类对应的烹饪种类，不是熔炉系列时是 `None`。
    pub fn kind_of(class: BlockClass) -> Option<CookingKind> {
        match class {
            BlockClass::FurnaceBlock => Some(CookingKind::Smelting),
            BlockClass::BlastFurnaceBlock => Some(CookingKind::Blasting),
            BlockClass::SmokerBlock => Some(CookingKind::Smoking),
            _ => None,
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

    /// 官方 `ContainerData.get`：`0` 剩余燃烧时间、`1` 总燃烧时间、
    /// `2` 烹饪进度、`3` 总烹饪时间。
    pub fn data(&self, id: usize) -> i32 {
        let (lit, timer, total) = self.values_at(block_entity_clock());
        match id {
            0 => lit,
            1 => self.lit_total_time,
            2 => timer,
            3 => total,
            _ => 0,
        }
    }

    /// 把安静刻的线性推进按公式算到 `now`，不改实体本身。
    /// 只用于 `&self` 的只读访问；到点那一刻的值由真正跑 `server_tick` 给出。
    fn values_at(&self, now: i64) -> (i32, i32, i32) {
        let Some(sleep) = self.sleep else {
            return (
                self.lit_time_remaining,
                self.cooking_timer,
                self.cooking_total_time,
            );
        };
        // 读发生在方块实体阶段之后，官方在 `now` 这一刻也跑过一次，所以要把
        // `now` 的线性增量算进去。到点那一刻会先真跑 `server_tick` 再重新登记，
        // 因此读路径不会看到 `now >= wake_at` 的旧睡眠。
        let steps = now - sleep.since;
        if steps <= 0 {
            return (
                self.lit_time_remaining,
                self.cooking_timer,
                self.cooking_total_time,
            );
        }
        let steps = i32::try_from(steps).unwrap_or(i32::MAX);
        let lit = self
            .lit_time_remaining
            .saturating_add(sleep.lit_step.saturating_mul(steps));
        let timer = if sleep.cook_step < 0 {
            self.cooking_timer
                .saturating_sub((-sleep.cook_step).saturating_mul(steps))
                .clamp(0, self.cooking_total_time)
        } else {
            self.cooking_timer
                .saturating_add(sleep.cook_step.saturating_mul(steps))
                .clamp(0, self.cooking_total_time)
        };
        (lit, timer, self.cooking_total_time)
    }

    /// 菜单读到的全部数据槽，官方 `AbstractFurnaceMenu.addDataSlots`。
    pub fn data_values(&self) -> [i32; NUM_DATA_VALUES] {
        std::array::from_fn(|id| self.data(id))
    }

    /// 录制层要看到的、数据槽之外的内部状态：配方缓存和配方统计。
    pub fn recipe_facts(&self) -> (Option<&'static str>, Vec<(&'static str, i32)>) {
        (
            self.last_recipe.map(|recipe| recipe.id),
            self.recipes_used.clone(),
        )
    }

    /// 存档读回时直接写官方的五个数值槽，不重算配方缓存。
    pub fn set_archive_values(&mut self, speed: f32, recipes_used: Vec<(&'static str, i32)>) {
        self.materialize(block_entity_clock().saturating_sub(1));
        self.speed_multiplier = speed;
        self.recipes_used = recipes_used;
        self.last_recipe = None;
    }

    /// 存档 `RecipesUsed` 的配方名和次数。
    pub fn speed_multiplier(&self) -> f32 {
        self.speed_multiplier
    }

    /// 官方 `ContainerData.set`。存档回读和菜单写数据槽都走这里，不经过 `setItem`。
    ///
    /// Vanilla `ContainerData.set`. Loading and menu data writes use this and do not go through `setItem`.
    pub fn set_data(&mut self, id: usize, value: i32) {
        self.materialize(block_entity_clock().saturating_sub(1));
        match id {
            0 => self.lit_time_remaining = value,
            1 => self.lit_total_time = value,
            2 => self.cooking_timer = value,
            3 => self.cooking_total_time = value,
            _ => {}
        }
    }

    /// 官方 `setItem`：写一格，数量按 `getMaxStackSize(stack)` 截断，
    /// 也就是 `min(99, 物品自身的上限)`。
    ///
    /// 原料格换了内容时重算总烹饪时间并把进度归零。菜单点击也走这里。
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        self.materialize(block_entity_clock().saturating_sub(1));
        let old = self.items[slot].clone();
        let same = !stack.is_empty() && same_item_same_components(&old, &stack);
        let mut stack = stack;
        let limit = CONTAINER_STACK_LIMIT.min(max_stack_size(&stack));
        if stack.count > limit {
            stack.count = limit;
        }
        self.items[slot] = stack;
        if slot == SLOT_INPUT && !same {
            self.cooking_total_time = self.total_cook_time();
            self.cooking_timer = 0;
        }
    }

    /// 不经过 `setItem` 的写回。`removeItem` 和原地改数量不重算烹饪时间。
    ///
    /// Writes a slot without `setItem`. `removeItem` and in-place count edits do not recompute cook time.
    pub fn write_item(&mut self, slot: usize, stack: ItemStack) {
        self.materialize(block_entity_clock().saturating_sub(1));
        self.items[slot] = if stack.is_empty() {
            ItemStack::empty()
        } else {
            stack
        };
    }

    /// 官方 `Container.removeItem`：拿走一格里的几个。它不经过 `setItem`。
    pub fn remove_item(&mut self, slot: usize, amount: i32) -> ItemStack {
        self.materialize(block_entity_clock().saturating_sub(1));
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

    /// 官方 `canPlaceItem`。
    pub fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        crate::machine_container::furnace_can_place(&self.items, slot, stack)
    }

    fn recipe_type(&self) -> RecipeType {
        match self.kind {
            CookingKind::Smelting => RecipeType::Smelting,
            CookingKind::Blasting => RecipeType::Blasting,
            CookingKind::Smoking => RecipeType::Smoking,
            CookingKind::Campfire => RecipeType::CampfireCooking,
        }
    }

    /// 官方 `RecipeManager.CachedCheck.getRecipeFor`：先试上一份配方，再按注册表顺序找。
    fn recipe_for(&mut self, stack: &ItemStack) -> Option<&'static Recipe> {
        let recipe =
            RecipeManager::global().get_recipe_for(self.recipe_type(), stack, self.last_recipe);
        if let Some(recipe) = recipe {
            self.last_recipe = Some(recipe);
        }
        recipe
    }

    /// 官方 `getBurnDuration`：燃料的燃烧时间，快速炉把引用除以 2。
    fn burn_duration(&self, fuel: &ItemStack) -> i32 {
        cooking_fuel(fuel, self.fast_cooking()).map_or(0, |(time, _)| time)
    }

    /// 官方 `getSpeedMultiplier`。
    fn speed_of(&self, fuel: &ItemStack) -> f32 {
        cooking_fuel(fuel, self.fast_cooking()).map_or(1.0, |(_, speed)| speed)
    }

    /// 官方 `FAST_FURNACE` 谓词：高炉和烟熏炉。
    fn fast_cooking(&self) -> bool {
        matches!(self.kind, CookingKind::Blasting | CookingKind::Smoking)
    }

    /// 官方 `getTotalCookTime(recipe, entity)`。
    fn cook_time_of(&self, recipe: &Recipe) -> i32 {
        let time = match recipe.kind {
            RecipeKind::Cooking { time, .. } => time,
            _ => BURN_TIME_STANDARD,
        };
        if self.speed_multiplier > 0.0 {
            (time as f32 / self.speed_multiplier).ceil() as i32
        } else {
            time
        }
    }

    /// 官方 `getTotalCookTime(level, entity)`：原料格没有配方时是 200。
    ///
    /// 它走的是 `quickCheck`，所以也会更新缓存。
    fn total_cook_time(&mut self) -> i32 {
        let input = self.items[SLOT_INPUT].clone();
        self.recipe_for(&input)
            .map_or(BURN_TIME_STANDARD, |recipe| self.cook_time_of(recipe))
    }

    /// 官方 `canBurn`。
    fn can_burn(&self, burn_result: &ItemStack) -> bool {
        let result = &self.items[SLOT_RESULT];
        if result.is_empty() {
            return true;
        }
        if !same_item_same_components(result, burn_result) {
            return false;
        }
        let result_count = result.count + burn_result.count;
        let max_result_count = CONTAINER_STACK_LIMIT.min(max_stack_size(burn_result));
        result_count <= max_result_count
    }

    /// 官方 `consumeFuel`。燃料的余料放不下时官方丢在地上，掉落要取世界随机数。
    fn consume_fuel(&mut self, world: &mut World) {
        let fuel_item = self.items[SLOT_FUEL].clone();
        let remainder = crafting_remainder(&fuel_item);
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

    /// 官方 `burn`：产物进结果格，湿海绵配空桶变成水桶，最后消耗一个原料。
    fn burn(&mut self, burn_result: &ItemStack) {
        if self.items[SLOT_RESULT].is_empty() {
            self.items[SLOT_RESULT] = burn_result.clone();
        } else {
            self.items[SLOT_RESULT].count += burn_result.count;
        }
        if self.items[SLOT_INPUT].item == items::WET_SPONGE
            && !self.items[SLOT_FUEL].is_empty()
            && self.items[SLOT_FUEL].item == items::BUCKET
        {
            self.items[SLOT_FUEL] = ItemStack::new(items::WATER_BUCKET, 1);
        }
        self.items[SLOT_INPUT].count -= 1;
        if self.items[SLOT_INPUT].count <= 0 {
            self.items[SLOT_INPUT] = ItemStack::empty();
        }
    }

    /// 官方 `setRecipeUsed`：记一次配方统计。
    fn set_recipe_used(&mut self, recipe: &'static Recipe) {
        if let Some((_, count)) = self
            .recipes_used
            .iter_mut()
            .find(|(id, _)| *id == recipe.id)
        {
            *count += 1;
        } else {
            self.recipes_used.push((recipe.id, 1));
        }
    }

    /// 官方 `getRecipesToAwardAndPopExperience`：每条统计过的配方给经验。
    ///
    /// 经验球不存在，但 `createExperience` 的 `nextFloat` 和
    /// `ExperienceOrb.award` 每次迭代的 `nextInt(40)` 都照抽。
    pub fn award_experience(&mut self, world: &mut World) {
        self.materialize(block_entity_clock().saturating_sub(1));
        let used = std::mem::take(&mut self.recipes_used);
        for (id, count) in used {
            let Some(recipe) = RecipeManager::global().recipe(id) else {
                continue;
            };
            let RecipeKind::Cooking { experience, .. } = recipe.kind else {
                continue;
            };
            create_experience(world, count, experience);
        }
    }

    /// 官方 `serverTick`。
    pub fn server_tick(&mut self, world: &mut World) {
        let now = block_entity_clock();
        self.materialize(now.saturating_sub(1));
        self.sleep = None;
        // 官方 `serverTick` 的 `changed`：只有燃料、成品或 `lit` 变了才 `setChanged`。
        //
        // Vanilla `serverTick`'s `changed`: `setChanged` runs only when fuel, the result, or `lit` changes.
        let mut changed = false;
        let was_lit;
        let mut is_lit;
        if self.lit_time_remaining > 0 {
            was_lit = true;
            self.lit_time_remaining -= 1;
            is_lit = self.lit_time_remaining > 0;
        } else {
            was_lit = false;
            is_lit = false;
        }

        let fuel = self.items[SLOT_FUEL].clone();
        let ingredient = self.items[SLOT_INPUT].clone();
        let has_ingredient = !ingredient.is_empty();
        let has_fuel = !fuel.is_empty();
        if is_lit || has_fuel && has_ingredient {
            if has_ingredient {
                let recipe = self.recipe_for(&ingredient);
                if let Some(recipe) = recipe {
                    let burn_result = assemble(recipe, std::slice::from_ref(&ingredient))
                        .filter(|stack| !stack.is_empty());
                    if let Some(burn_result) = burn_result
                        && self.can_burn(&burn_result)
                    {
                        if !is_lit {
                            let new_lit_time = self.burn_duration(&fuel);
                            let new_speed = self.speed_of(&fuel);
                            self.lit_time_remaining = new_lit_time;
                            self.lit_total_time = new_lit_time;
                            self.speed_multiplier = new_speed;
                            if self.cooking_total_time > 0
                                && self.cooking_timer < self.cooking_total_time
                            {
                                let ratio =
                                    self.cooking_timer as f32 / self.cooking_total_time as f32;
                                self.cooking_total_time = self.cook_time_of(recipe);
                                self.cooking_timer =
                                    (ratio * self.cooking_total_time as f32).ceil() as i32;
                            }
                            if new_lit_time > 0 {
                                self.consume_fuel(world);
                                is_lit = true;
                                changed = true;
                            }
                        }

                        if is_lit {
                            self.cooking_timer += 1;
                            if self.cooking_timer >= self.cooking_total_time {
                                self.cooking_timer = 0;
                                self.cooking_total_time = self.cook_time_of(recipe);
                                self.burn(&burn_result);
                                self.set_recipe_used(recipe);
                                changed = true;
                            }
                        } else {
                            self.cooking_timer = 0;
                        }
                    } else {
                        self.cooking_timer = 0;
                    }
                }
                // 官方 `recipe == null` 时不改 `cookingTimer`。找不到配方就留着原来的进度。
                // Vanilla leaves `cookingTimer` unchanged when `recipe == null`.
            } else {
                self.cooking_timer = 0;
            }
        } else if self.cooking_timer > 0 {
            self.cooking_timer =
                (self.cooking_timer - BURN_COOL_SPEED).clamp(0, self.cooking_total_time);
        }

        if was_lit != is_lit {
            changed = true;
            let state = world.block_state(self.pos);
            // 官方无条件 `setBlockAndUpdate(pos, state.setValue(LIT, isLit))`。
            // `LevelChunk.setBlockState` 在新旧状态相同时直接返回 null，`Level.setBlock`
            // 随后也直接返回，不发任何更新；这里提前判一次和它等价。
            //
            // Vanilla calls `setBlockAndUpdate` unconditionally. `LevelChunk.setBlockState`
            // returns null when the state is unchanged, and `Level.setBlock` then returns
            // without any update, so this guard is equivalent.
            if state.get(LIT) != is_lit {
                world.set_block_and_update(self.pos, state.with(LIT, is_lit));
            }
        }

        // 官方 `setChanged(level, pos, state)`：燃料烧起来、烧完一格或者 `lit` 变了才通知比较器。
        // 只增加 `cookingTimer` 不算变化。
        //
        // Vanilla `setChanged(level, pos, state)` runs when fuel is consumed, a cook finishes,
        // or `lit` changes. Incrementing `cookingTimer` alone does not.
        if changed {
            crate::machine_container::mark_container_changed(world, self.pos);
        }
        let _ = now;
    }

    /// 一次 `server_tick` 之后，把安静刻的线性公式记下来。
    ///
    /// 官方 `serverTick` 的安静刻只有两种：点着火且有原料、有配方、结果格放得下时
    /// `litTimeRemaining` 每刻 -1、`cookingTimer` 每刻 +1；熄火且进度 > 0 时每刻 -2
    /// （clamp 到 0）。其余分支要么不改数值、要么会查配方/点火，都照常每刻跑。
    ///
    /// 到点取 `min(这一物品烧完, 燃料烧完)`。燃料烧完那刻若还有燃料/原料，官方会点火
    /// 消耗燃料（余料掉落取世界随机数）；没有就熄火改 `LIT` 方块状态。两种都必须在
    /// 原刻原位置真跑，所以日程不能跨过它。
    fn arm_sleep(&mut self, now: i64) {
        let has_input = !self.items[SLOT_INPUT].is_empty();
        let is_lit = self.lit_time_remaining > 0;
        self.sleep = None;

        if is_lit && has_input {
            // 查配方和结果格都不读世界，只用静态配方表和三格物品。
            let recipe = RecipeManager::global().get_recipe_for(
                self.recipe_type(),
                &self.items[SLOT_INPUT],
                self.last_recipe,
            );
            let Some(recipe) = recipe else {
                return;
            };
            let burn_result = assemble(recipe, std::slice::from_ref(&self.items[SLOT_INPUT]))
                .filter(|stack| !stack.is_empty());
            if !burn_result.is_some_and(|result| self.can_burn(&result)) {
                return;
            }
            let cook_left = i64::from((self.cooking_total_time - self.cooking_timer).max(1));
            let fuel_left = i64::from(self.lit_time_remaining.max(1));
            self.sleep = Some(Sleep {
                since: now,
                wake_at: now.saturating_add(cook_left.min(fuel_left)),
                lit_step: -1,
                cook_step: 1,
            });
            return;
        }

        if is_lit && !has_input {
            // 点着火但没有原料：官方每刻只把 `cookingTimer` 归零（已经是 0），
            // 燃料每刻 -1。到燃料烧完那刻才要真跑（可能点火或熄火）。
            self.sleep = Some(Sleep {
                since: now,
                wake_at: now.saturating_add(i64::from(self.lit_time_remaining.max(1))),
                lit_step: -1,
                cook_step: 0,
            });
            return;
        }

        if !is_lit && !has_input {
            // 熄火且没有原料：官方 `isLit || hasFuel && hasIngredient` 为假，只把进度
            // 每刻 -2（clamp 到 0）；燃料格有没有东西都不消耗。进度到 0 之后每刻
            // 仍跑一次但没有可见效果，睡到外部叫醒。
            self.sleep = Some(Sleep {
                since: now,
                wake_at: i64::MAX,
                lit_step: 0,
                cook_step: if self.cooking_timer > 0 {
                    -BURN_COOL_SPEED
                } else {
                    0
                },
            });
        }
    }
}

/// 官方 `createExperience` 加 `ExperienceOrb.award`。
///
/// 有一段小数概率时抽一次 `nextFloat`；随后按 `getExperienceValue` 分段把总量拆成好几个球，
/// 每个球在 `tryMergeToExisting` 里抽一次 `nextInt(40)`。世界没有实体，合并永远失败，
/// 但随机数照抽。
fn create_experience(world: &mut World, amount: i32, value: f32) {
    let experience = amount as f32 * value;
    let floor = experience.floor();
    let mut reward = floor as i32;
    let fraction = experience - floor;
    if fraction != 0.0 && world.random().next_float() < fraction {
        reward += 1;
    }
    let mut remaining = reward;
    while remaining > 0 {
        remaining -= experience_value(remaining);
        world.random().next_int(40);
    }
}

/// 官方 `ExperienceOrb.getExperienceValue`。
fn experience_value(amount: i32) -> i32 {
    for limit in EXPERIENCE_VALUES {
        if amount >= limit {
            return limit;
        }
    }
    1
}

impl BlockEntity for FurnaceEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.server_tick(world);
    }

    /// 官方 `preRemoveSideEffects`：先按容器掉落三格，再把攒下的经验发掉。
    fn pre_remove_side_effects(&mut self, world: &mut World, _pos: BlockPos, _state: BlockState) {
        for stack in &self.items {
            drop_item_stack(world, self.pos, stack);
        }
        self.award_experience(world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn sleep_from(&mut self, now: i64) -> Option<i64> {
        self.arm_sleep(now);
        self.sleep.map(|sleep| sleep.wake_at)
    }

    /// 把数值按公式算到「第 `tick` 刻结束时」，并结束这段睡眠。
    /// `server_tick` 在 `now` 这一刻真跑之前用 `now - 1`，好让这一刻的增量由
    /// `server_tick` 自己加；已经在游标前跑过的实体用 `now`。
    fn materialize(&mut self, tick: i64) {
        let Some(sleep) = self.sleep.take() else {
            return;
        };
        let steps = tick.min(sleep.wake_at.saturating_sub(1)) - sleep.since;
        if steps <= 0 {
            return;
        }
        let steps = i32::try_from(steps).unwrap_or(i32::MAX);
        self.lit_time_remaining = self
            .lit_time_remaining
            .saturating_add(sleep.lit_step.saturating_mul(steps))
            .max(0);
        if sleep.cook_step < 0 {
            self.cooking_timer = self
                .cooking_timer
                .saturating_sub((-sleep.cook_step).saturating_mul(steps))
                .clamp(0, self.cooking_total_time);
        } else {
            self.cooking_timer = self
                .cooking_timer
                .saturating_add(sleep.cook_step.saturating_mul(steps))
                .clamp(0, self.cooking_total_time);
        }
    }

    fn resleep(&mut self, since: i64, wake_at: i64) {
        // 数值已经在装盒时按公式算到源表的最后一刻；目标表的时钟从这一刻起算。
        self.sleep = Some(Sleep {
            since,
            wake_at,
            lit_step: if self.lit_time_remaining > 0 { -1 } else { 0 },
            cook_step: if self.lit_time_remaining > 0 && !self.items[SLOT_INPUT].is_empty() {
                1
            } else if self.cooking_timer > 0 {
                -BURN_COOL_SPEED
            } else {
                0
            },
        });
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        use crate::archive::tag::put;
        let mut out = Vec::new();
        let (lit, timer, _) = self.values_at(block_entity_clock());
        put(&mut out, "cooking_time_spent", Tag::Int(timer));
        put(
            &mut out,
            "cooking_total_time",
            Tag::Int(self.cooking_total_time),
        );
        put(&mut out, "lit_time_remaining", Tag::Int(lit));
        put(&mut out, "lit_total_time", Tag::Int(self.lit_total_time));
        put(
            &mut out,
            "speed_multiplier",
            Tag::Float(self.speed_multiplier),
        );
        put(
            &mut out,
            "Items",
            crate::archive::block_entity::items_tag(self.items())?,
        );
        // 官方 RECIPES_USED_CODEC 是 unboundedMap(Recipe.KEY_CODEC, Codec.INT)，
        // 空映射也写 `{}`。
        let mut used = Vec::new();
        for (id, count) in &self.recipes_used {
            put(&mut used, id, Tag::Int(*count));
        }
        put(&mut out, "RecipesUsed", Tag::Compound(used));
        Ok(out)
    }
}
