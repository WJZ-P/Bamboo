//! 酿造台的方块实体，对应官方 `BrewingStandBlockEntity`。
//!
//! 槽位 0 到 2 是药水瓶、3 是材料、4 是燃料。每刻照官方 `serverTick`：先补燃料，
//! 再看能不能酿、倒计时，最后把三个瓶子空不空写回 `has_bottle_*` 方块状态。
//!
//! 数据槽给菜单读，顺序是酿造进度、剩余燃料、总酿造时间、总燃料。

use std::any::Any;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{Recipe, RecipeType};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{
    BrewingInput, RecipeManager, assemble, brewing_fuel, crafting_remainder, is_brewing_reagent,
    potion_id,
};

use crate::{
    block_entity::{BlockEntity, block_entity_clock, drop_item_stack},
    sleep_clock::{Sleep, materialize_one, value_at},
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
    /// 燃料的 `setChanged` 已经发出，后面的酿造还没跑。
    ///
    /// Fuel's `setChanged` has run; the brew after it has not.
    yield_tick: bool,
    /// `0` 从燃料开始，`1` 燃料已经通知过（`resume_tick` 的第二段）。
    ///
    /// `0` starts at fuel. `1` is the second segment after the fuel notification.
    tick_phase: u8,
    /// 一段没有可见事件的安静刻：`brew_time` 每刻 -1，到点真跑一次 `serverTick`。
    sleep: Option<Sleep>,
}

/// 酿造台里录制层要看的状态（数据槽之外）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrewingFacts {
    pub last_potion_count: Option<[bool; 3]>,
    pub ingredient: Option<i32>,
    pub speed_multiplier: u32,
    pub last_recipe: Option<&'static str>,
    pub yield_tick: bool,
    pub tick_phase: u8,
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
            yield_tick: false,
            tick_phase: 0,
            sleep: None,
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
            0 => self.brew_time_at(block_entity_clock()),
            1 => self.fuel,
            2 => self.total_brew_time,
            3 => self.total_fuel,
            _ => 0,
        }
    }

    /// 安静刻的倒计时按公式算到 `now`，不改实体本身。
    fn brew_time_at(&self, now: i64) -> i32 {
        value_at(self.sleep, self.brew_time, -1, now)
    }

    /// 外部写入、存档回读、装盒之前结束睡眠，数值按公式算到「第 `tick` 刻结束时」。
    fn materialize(&mut self, tick: i64) {
        materialize_one(&mut self.sleep, &mut self.brew_time, -1, tick);
    }

    /// 菜单读到的全部数据槽。
    pub fn data_values(&self) -> [i32; NUM_DATA_VALUES] {
        std::array::from_fn(|id| self.data(id))
    }

    /// 录制层要看到的、数据槽之外的内部状态。
    pub fn observed_facts(&self) -> BrewingFacts {
        // 安静刻里配方缓存不变；`yield_tick` / `tick_phase` 只在真跑的两段里变。
        BrewingFacts {
            last_potion_count: self.last_potion_count,
            ingredient: self.ingredient,
            speed_multiplier: self.speed_multiplier.to_bits(),
            last_recipe: self.last_recipe.map(|recipe| recipe.id),
            yield_tick: self.yield_tick,
            tick_phase: self.tick_phase,
        }
    }

    /// 存档读回 `speed_multiplier`；`ingredient` 由官方 `loadAdditional` 在
    /// `BrewTime > 0` 时从材料格物品重算，这里保持一致。
    pub fn set_speed_multiplier(&mut self, speed: f32) {
        self.materialize(block_entity_clock().saturating_sub(1));
        self.speed_multiplier = speed;
        if self.brew_time > 0 {
            self.ingredient = Some(self.items[SLOT_INGREDIENT].item);
        }
    }

    /// 官方 `ContainerData.set`。存档回读和菜单写数据槽都走这里。
    ///
    /// Vanilla `ContainerData.set`. Loading and menu data writes use this.
    pub fn set_data(&mut self, id: usize, value: i32) {
        self.materialize(block_entity_clock().saturating_sub(1));
        match id {
            0 => self.brew_time = value,
            1 => self.fuel = value,
            2 => self.total_brew_time = value,
            3 => self.total_fuel = value,
            _ => {}
        }
    }

    /// 官方 `BaseContainerBlockEntity.setItem` 的写入：数量按 `getMaxStackSize` 截断。
    ///
    /// `setChanged` 不在这里。漏斗那条路在容器包装结束时通知，菜单在写回之后通知。
    ///
    /// The write half of vanilla `BaseContainerBlockEntity.setItem`: the count is limited by
    /// `getMaxStackSize`. `setChanged` is not here. The hopper path notifies at the end of the
    /// container wrapper, and a menu notifies after it writes the slots back.
    pub fn set_item(&mut self, slot: usize, stack: ItemStack) {
        self.materialize(block_entity_clock().saturating_sub(1));
        let mut stack = stack;
        let limit = mc_bamboo_recipes::max_stack_size(&stack).min(CONTAINER_STACK_LIMIT);
        if stack.count > limit {
            stack.count = limit;
        }
        self.items[slot] = stack;
    }

    /// 官方 `Container.removeItem`。
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

    /// 官方 `canPlaceItem`。`level` 一定有；材料格查 `BREWING_REAGENTS`，
    /// 药水格要求是药水输入并且这一格现在是空的。
    pub fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        crate::machine_container::brewing_can_place(&self.items, slot, stack)
    }

    /// 官方 `RecipeManager.CachedCheck.getRecipeFor`。
    fn recipe_for(&mut self, input: BrewingInput) -> Option<&'static Recipe> {
        let recipe =
            RecipeManager::global().get_recipe_for(RecipeType::Brewing, &input, self.last_recipe);
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
                self.items[slot] = assemble(recipe, &[input.input.clone(), input.reagent.clone()])
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

    /// 燃料用完时补一格，并按官方立刻 `setChanged`。真的消耗了才返回真。
    ///
    /// Refills fuel when it is empty and calls `setChanged` immediately, as vanilla does.
    /// Returns true only when a fuel item was consumed.
    fn consume_fuel(&mut self, world: &mut World) -> bool {
        if self.fuel > 0 {
            return false;
        }
        let Some((uses, speed)) = brewing_fuel(&self.items[SLOT_FUEL]) else {
            return false;
        };
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
        crate::machine_container::mark_container_changed(world, self.pos);
        true
    }

    /// 官方 `serverTick` 里前半段的燃料通知。两段式 `resume_tick` 的第二段
    /// 从这里接着跑；中间的比较器读取已经在驱动里按官方顺序跑过。
    fn fuel_tick(&mut self, world: &mut World) {
        let first = self.tick_phase == 0;
        if first {
            self.materialize(block_entity_clock().saturating_sub(1));
            self.sleep = None;
            self.yield_tick = false;
        }
        if first && self.consume_fuel(world) && world.block_entity_is_taken(self.pos) {
            self.tick_phase = 1;
            self.yield_tick = true;
            return;
        }
        self.tick_phase = 0;
        self.brew_and_update_bottles(world);
    }

    /// 官方 `serverTick`。
    ///
    /// 消耗燃料会 `setChanged`，同一刻后面的 `doBrew` 还可能改药水。实体正被拿着时这两次
    /// 比较器读到的物品不一样，所以燃料通知之后先停，等实体放回表、比较器读完再继续。
    /// 实体不在表里被拿着时（机器对照直接调用）一次跑完，随机数顺序不变。
    ///
    /// Vanilla `serverTick`. Consuming fuel calls `setChanged`, and `doBrew` later in the same
    /// tick can still change the potions. While the entity is out of the map those two
    /// comparator reads see different items, so the tick pauses after the fuel notification
    /// until the entity is put back and the queued reads have run. A direct call that does not
    /// hold the entity runs the whole tick at once, so the random draws stay in order.
    pub fn server_tick(&mut self, world: &mut World) {
        self.fuel_tick(world);
    }

    fn brew_and_update_bottles(&mut self, world: &mut World) {
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
            // 官方酿造倒计时的每一刻都 `setChanged`；只有 `doBrew` 那一才改物品。
            // Vanilla calls `setChanged` on every brewing tick; only `doBrew` changes items.
            crate::machine_container::quiet_mark_container_changed(
                world,
                self.pos,
                !done || !brewable,
            );
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
            // 官方开始酿造时 `setChanged`。
            // Vanilla calls `setChanged` when a brew starts.
            crate::machine_container::mark_container_changed(world, self.pos);
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

    /// 一次 `serverTick` 之后算下一次必须真跑的时刻。
    ///
    /// 官方每刻都调 `isBrewable`（会更新配方缓存）、补燃料、改 `brewTime`、
    /// 跑 `doBrew`、再把三个瓶子的空不空写回方块状态。物品不变时这些都不产生
    /// 可见效果，只有 `brewTime` 每刻 -1；到点的那一刻才必须真跑 `doBrew`。
    /// 没在酿、且不会下一刻开酿或补燃料时睡到外部叫醒。
    fn arm_sleep(&mut self, now: i64) -> Option<i64> {
        if self.yield_tick || self.tick_phase != 0 {
            // 两段式 `resume_tick` 还没跑完。
            return None;
        }
        // 和官方下一刻一致地刷新配方缓存；安静刻里它不会再变。
        let brewable = self.is_brewable();
        if self.brew_time > 1
            && brewable
            && self.ingredient == Some(self.items[SLOT_INGREDIENT].item)
        {
            // 第 `now + brew_time` 刻减到 0 并 `doBrew`，不能在中间醒来。
            return Some(now.saturating_add(i64::from(self.brew_time)));
        }
        if self.brew_time > 0 {
            // 下一刻就要结束这一轮，或材料已经变了要提前清零。
            return None;
        }
        let fuel_will_be_consumed =
            self.fuel <= 0 && brewing_fuel(&self.items[SLOT_FUEL]).is_some();
        if !brewable && !fuel_will_be_consumed {
            return Some(i64::MAX);
        }
        // 下一刻会开一轮或消耗燃料（余料掉落要取随机数），必须真跑。
        None
    }
}

impl BlockEntity for BrewingStandEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.fuel_tick(world);
    }

    fn resume_tick(&self) -> bool {
        self.yield_tick
    }

    /// 官方没有覆盖 `preRemoveSideEffects`，基类只把五格内容掉出来。
    fn pre_remove_side_effects(&mut self, world: &mut World, _pos: BlockPos, _state: BlockState) {
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

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        use crate::archive::tag::put;
        let mut out = Vec::new();
        put(
            &mut out,
            "BrewTime",
            Tag::Int(self.brew_time_at(block_entity_clock())),
        );
        put(&mut out, "total_brew_time", Tag::Int(self.total_brew_time));
        put(
            &mut out,
            "Items",
            crate::archive::block_entity::items_tag(self.items())?,
        );
        put(&mut out, "Fuel", Tag::Int(self.fuel));
        put(&mut out, "total_fuel", Tag::Int(self.total_fuel));
        put(
            &mut out,
            "speed_multiplier",
            Tag::Float(self.speed_multiplier),
        );
        Ok(out)
    }

    fn sleep_from(&mut self, now: i64) -> Option<i64> {
        let wake_at = self.arm_sleep(now)?;
        self.sleep = Some(Sleep::new(now, wake_at));
        Some(wake_at)
    }

    fn materialize(&mut self, tick: i64) {
        BrewingStandEntity::materialize(self, tick);
    }

    fn resleep(&mut self, since: i64, wake_at: i64) {
        // 装盒时数值已按公式算到源表的最后一刻；目标表从这一刻起算。
        self.sleep = Some(Sleep::new(since, wake_at));
    }
}
