//! 合成器的合成和吐出，官方 `CrafterBlock.dispenseFrom`。
//!
//! 计划刻到期时查找 3×3 配方。没有配方或产物是空的，只发失败事件 1050。
//! 成功时亮起 `crafting` 六刻，把产物和剩余物朝前吐出，九格里每个非空物品再减 1。
//! 前方是容器就按 `HopperBlockEntity.addItem` 放进去；放不下的部分走
//! [`spawn_dispensed_item`]。这里不保存官方 `RecipeCache` 的那张表，查找仍然是
//! 注册表里的第一条匹配，结果和有缓存时一样。
//!
//! `CrafterBlock.dispenseFrom`. A due tick looks up the 3x3 recipe. No recipe, or an empty
//! result, only plays fail event 1050. A success turns `crafting` on for six ticks, ejects the
//! result and the remainders forward, then shrinks every non-empty grid stack by one.
//! A container in front is filled with `HopperBlockEntity.addItem`. Whatever does not fit goes
//! through [`spawn_dispensed_item`]. The vanilla `RecipeCache` table is not stored; the lookup
//! is still the first registry match, so the result is the same as a cache hit.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, Recipe, RecipeType};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{CraftingInput, RecipeInput, RecipeManager, assemble, remaining_items};

use crate::{
    container::copy_with_count,
    hopper::{Added, add_item, apply_hopper_cooldown, set_changed},
    state::{BlockState, CRAFTING, FrontAndTop, ORIENTATION},
    world::{UPDATE_CLIENTS, World},
};

/// 合成格的边长。官方 `CrafterBlockEntity.getWidth`。
///
/// Width of the crafting grid. Vanilla `CrafterBlockEntity.getWidth`.
pub const GRID: usize = 3;
/// `CrafterBlock.MAX_CRAFTING_TICKS`。`crafting` 亮着的刻数。
///
/// How many ticks `crafting` stays on.
const MAX_CRAFTING_TICKS: i32 = 6;
/// `LevelEvent.SOUND_CRAFTER_FAIL`。
pub const SOUND_CRAFTER_FAIL: i32 = 1050;
/// `LevelEvent.SOUND_CRAFTER_CRAFT`。只有没放进容器、变成掉落物时才发。
///
/// Played only when a stack does not fit and becomes a dropped item.
const SOUND_CRAFTER_CRAFT: i32 = 1049;
/// `LevelEvent.PARTICLES_SHOOT_WHITE_SMOKE`。数据和朝向的 `get3DDataValue` 一样。
///
/// The data is the facing's `get3DDataValue`.
const PARTICLES_SHOOT: i32 = 2010;
/// 九格现在能做出的产物。空输入或没有配方时是空物品。
///
/// The result the nine slots would craft. Empty when the input or the recipe is empty.
pub fn potential_result(slots: &[ItemStack]) -> ItemStack {
    let Some(input) = craft_input(slots) else {
        return ItemStack::empty();
    };
    let Some(recipe) = find_recipe(&input) else {
        return ItemStack::empty();
    };
    assemble(recipe, input.slots())
        .filter(|stack| !stack.is_empty())
        .unwrap_or_else(ItemStack::empty)
}

/// `CrafterBlock.dispenseFrom`。这一格不是合成器，或者读不到九格时什么都不做。
///
/// Does nothing when the block is not a crafter or the nine slots cannot be read.
pub fn dispense_from(world: &mut World, pos: BlockPos, state: BlockState) {
    let Some(slots) = read_grid(world, pos) else {
        return;
    };
    let Some(input) = craft_input(&slots) else {
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
        return;
    };
    // 官方先 `getPotentialResults`，再 `assemble`。两步有一步是空的都只发 1050。
    //
    // Vanilla looks up the recipe, then `assemble`. Either step being empty only sends 1050.
    let Some(recipe) = find_recipe(&input) else {
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
        return;
    };
    let Some(result) = assemble(recipe, input.slots()).filter(|stack| !stack.is_empty()) else {
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
        return;
    };
    let front = front_of(state.get(ORIENTATION));
    // 倒计时先记下来。接着改 `crafting` 只换属性，不会把方块实体清掉。
    //
    // Record the countdown first. Turning `crafting` on only changes a property and does not
    // remove the block entity.
    world.set_crafter_crafting(pos, MAX_CRAFTING_TICKS);
    world.set_block(pos, state.with(CRAFTING, true), UPDATE_CLIENTS);
    // `ItemStack.onCraftedBySystem` 只有地图会改物品（`MapItem.onCraftedPostProcess`）。
    // 这里没有地图数据存储，那一步先不做。
    //
    // `onCraftedBySystem` only changes maps (`MapItem.onCraftedPostProcess`). There is no map
    // data store, so that step is skipped.
    dispense_item(world, pos, result, front);
    for remainder in remaining_items(recipe, input.slots()) {
        if !remainder.is_empty() {
            dispense_item(world, pos, remainder, front);
        }
    }
    let _ = world.with_slots(pos, GRID * GRID, &mut |slots| {
        for slot in slots.iter_mut() {
            if slot.is_empty() {
                continue;
            }
            slot.count -= 1;
            if slot.count <= 0 {
                *slot = ItemStack::empty();
            }
        }
    });
    set_changed(world, pos);
}

/// 官方 `CrafterBlockEntity.serverTick`：每刻减一次倒计时，归零时关掉 `crafting`。
/// 方块 oracle 不登记合成器的 ticker，所以比较时 `crafting` 保持亮着；只有游戏循环和
/// 测试用的世界刻会调用这里。
///
/// ticker 由 `LevelChunk$BoundTickingBlockEntity.tick` 包着：先判 `LevelChunk.isTicking(pos)`
/// （世界边界内、区块能 tick），不满足就整刻不跑，倒计时停在原值，恢复后接着倒数。
/// 边界判断和方块实体阶段用同一个整数方块盒。
///
/// `CrafterBlockEntity.serverTick`: decrement the countdown each tick and clear `crafting` when
/// it reaches zero. The block oracle does not register a crafter ticker, so `crafting` stays on
/// while a case is compared; only the game loop and test world ticks call this. Like every
/// ticker it is skipped while the chunk cannot tick or the block is outside the world border,
/// so the countdown pauses there and resumes later.
pub(crate) fn tick_crafting_clocks(world: &mut World) {
    let clocks = world.crafter_crafting();
    if clocks.is_empty() {
        return;
    }
    let border = world.world_border().block_box();
    for (pos, _) in clocks {
        if !world.chunk_ticks_blocks(pos.chunk()) || !border.contains(pos) {
            continue;
        }
        if !is_crafter(world, pos) {
            // 方块已经换掉，官方的方块实体和 ticker 一起没了。
            world.set_crafter_crafting(pos, 0);
            continue;
        }
        // 前面那台关 `crafting` 时的邻居更新也可能碰到这一台，按表里的当前值减。
        let next = world.crafter_crafting_at(pos) - 1;
        if next < 0 {
            continue;
        }
        world.set_crafter_crafting(pos, next);
        if next == 0 {
            let state = world.block_state(pos);
            world.set_block_and_update(pos, state.with(CRAFTING, false));
        }
    }
}

/// `CrafterBlock.dispenseItem` 的 `DefaultDispenseItemBehavior.spawnItem`。
///
/// 落点是 `Vec3.atCenterOf(pos).relative(front, 0.7)`（和发射器/投掷器的
/// `getDispensePosition` 一样），竖直朝向把 Y 减 0.125，其它朝向减 0.15625。
/// 速度由 `spawn_item_random` 取一个 `nextDouble` 和三个 `triangle`（精度 6），
/// 这套随机数只取一遍，物品实体用自己的随机源，不碰 `level.random`。
///
/// `DefaultDispenseItemBehavior.spawnItem` from `CrafterBlock.dispenseItem`.
/// The position is `Vec3.atCenterOf(pos).relative(front, 0.7)`, the same as a dispenser's
/// `getDispensePosition`; vertical facing lowers Y by 0.125, others by 0.15625.
/// `spawn_item_random` takes one `nextDouble` and three `triangle` draws (accuracy 6)
/// exactly once. The item entity uses its own random source, not `level.random`.
pub fn spawn_dispensed_item(world: &mut World, pos: BlockPos, stack: &ItemStack, front: Direction) {
    if stack.is_empty() {
        return;
    }
    let velocity = crate::dispenser::spawn_item_random(world, front);
    world.spawn_dispensed_item_with_velocity(
        stack.clone(),
        front,
        crate::dispense::dispense_position(pos, front),
        velocity,
    );
}

fn find_recipe(input: &CraftingInput) -> Option<&'static Recipe> {
    RecipeManager::global().get_recipe_for(RecipeType::Crafting, input, None)
}

fn craft_input(slots: &[ItemStack]) -> Option<CraftingInput> {
    if slots.len() < GRID * GRID {
        return None;
    }
    let input = CraftingInput::of(GRID, GRID, &slots[..GRID * GRID]);
    if input.is_empty() { None } else { Some(input) }
}

fn read_grid(world: &mut World, pos: BlockPos) -> Option<Vec<ItemStack>> {
    if !is_crafter(world, pos) {
        return None;
    }
    let mut slots = None;
    let present = world.with_slots(pos, GRID * GRID, &mut |found| {
        slots = Some(found.to_vec());
    });
    present.then_some(slots).flatten()
}

fn is_crafter(world: &World, pos: BlockPos) -> bool {
    world.block_state(pos).is_a(BlockAncestor::CrafterBlock)
}

/// `CrafterBlock.dispenseItem`。前方没有方块容器时整堆都吐出去。
///
/// 官方接着还会找实体容器。列表是空的就不取 `nextInt`。这里没有实体容器，
/// 所以也不取。附近玩家的 `CRAFTER_RECIPE_CRAFTED` 成就同样只在吐出掉落物时触发，这里没做。
///
/// A missing block container ejects the whole stack.
///
/// Vanilla then looks for an entity container. An empty list does not draw `nextInt`. There are
/// no entity containers here, so that draw is skipped too. The `CRAFTER_RECIPE_CRAFTED`
/// advancement, which also runs only when something is ejected, is not awarded.
fn dispense_item(world: &mut World, pos: BlockPos, stack: ItemStack, front: Direction) {
    let dest = pos.relative(front);
    let into = front.opposite();
    let left = give_to_container(world, dest, stack, into);
    if left.is_empty() {
        return;
    }
    spawn_dispensed_item(world, pos, &left, front);
    world.level_event(SOUND_CRAFTER_CRAFT, pos, 0);
    world.level_event(PARTICLES_SHOOT, pos, front.id());
}

/// 前方是合成器，或者这堆比目标的 `getMaxStackSize(stack)` 还大，就一次放一个。
///
/// A crafter in front, or a stack larger than the destination's `getMaxStackSize(stack)`,
/// is inserted one item at a time.
fn give_to_container(
    world: &mut World,
    dest: BlockPos,
    stack: ItemStack,
    into: Direction,
) -> ItemStack {
    let into_crafter = is_crafter(world, dest);
    let mut limit = None;
    if !world.with_container(dest, &mut |container| {
        limit = Some(container.max_stack_size_of(&stack));
    }) {
        return stack;
    }
    let limit = limit.unwrap_or(0);
    if into_crafter || stack.count > limit {
        insert_one_by_one(world, dest, stack, into)
    } else {
        insert_whole(world, dest, stack, into)
    }
}

fn insert_one_by_one(
    world: &mut World,
    dest: BlockPos,
    stack: ItemStack,
    into: Direction,
) -> ItemStack {
    let mut remaining = stack;
    while !remaining.is_empty() {
        let one = copy_with_count(&remaining, 1);
        let Some(added) = place(world, dest, one, into) else {
            break;
        };
        if !added.remaining.is_empty() {
            break;
        }
        remaining = without_one(&remaining);
        // 官方每次 `addItem` 成功都 `setChanged`。一次放一个时，旁边的比较器看得到中间状态。
        //
        // Each successful `addItem` calls `setChanged`. One-at-a-time inserts let a neighbour
        // comparator see the intermediate counts.
        finish_insert(world, dest, true, added.was_empty);
    }
    remaining
}

fn insert_whole(world: &mut World, dest: BlockPos, stack: ItemStack, into: Direction) -> ItemStack {
    let mut remaining = stack;
    loop {
        if remaining.is_empty() {
            break;
        }
        let before = remaining.count;
        let Some(added) = place(world, dest, remaining.clone(), into) else {
            break;
        };
        let moved = added.remaining.count != before;
        remaining = added.remaining;
        if moved {
            finish_insert(world, dest, true, added.was_empty);
        }
        if !moved {
            break;
        }
    }
    remaining
}

fn place(world: &mut World, dest: BlockPos, stack: ItemStack, into: Direction) -> Option<Added> {
    let mut added = None;
    let mut stack = Some(stack);
    if !world.with_container(dest, &mut |container| {
        added = Some(add_item(
            container,
            stack.take().unwrap_or_default(),
            Some(into),
        ));
    }) {
        return None;
    }
    added
}

fn finish_insert(world: &mut World, dest: BlockPos, moved: bool, was_empty: bool) {
    if was_empty && world.block_state(dest).is_a(BlockAncestor::HopperBlock) {
        // 来源是合成器，不是漏斗，所以不减那一刻。
        //
        // The source is a crafter, not a hopper, so the cooldown is not shortened.
        apply_hopper_cooldown(world, dest, None);
    }
    if moved {
        set_changed(world, dest);
    }
}

fn without_one(stack: &ItemStack) -> ItemStack {
    if stack.count <= 1 {
        ItemStack::empty()
    } else {
        copy_with_count(stack, stack.count - 1)
    }
}

fn front_of(orientation: FrontAndTop) -> Direction {
    match orientation {
        FrontAndTop::DownEast
        | FrontAndTop::DownNorth
        | FrontAndTop::DownSouth
        | FrontAndTop::DownWest => Direction::Down,
        FrontAndTop::UpEast | FrontAndTop::UpNorth | FrontAndTop::UpSouth | FrontAndTop::UpWest => {
            Direction::Up
        }
        FrontAndTop::WestUp => Direction::West,
        FrontAndTop::EastUp => Direction::East,
        FrontAndTop::NorthUp => Direction::North,
        FrontAndTop::SouthUp => Direction::South,
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockState, BlockTicks, HopperEntity, LegacyRandom, LevelSideState,
        level::Effect,
        state::{ORIENTATION, TRIGGERED},
        testing::TestLevel,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn facing_north(level: &mut TestLevel, pos: BlockPos) {
        level.set(
            pos,
            BlockState(blocks::CRAFTER).with(ORIENTATION, FrontAndTop::NorthUp),
        );
    }

    fn put(level: &mut TestLevel, pos: BlockPos, index: usize, stack: ItemStack) {
        let slots = level
            .containers
            .entry(pos)
            .or_insert_with(|| vec![ItemStack::empty(); GRID * GRID]);
        slots[index] = stack;
    }

    fn tick(level: &mut TestLevel, pos: BlockPos) -> Vec<Effect> {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        let state = world.block_state(pos);
        state.tick(&mut world, pos);
        world.into_effects()
    }

    fn level_events(effects: &[Effect]) -> Vec<(i32, i32)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::LevelEvent { event, data, .. } => Some((*event, *data)),
                _ => None,
            })
            .collect()
    }

    fn contents(level: &TestLevel, pos: BlockPos) -> Vec<ItemStack> {
        level.containers.get(&pos).cloned().unwrap_or_default()
    }

    fn spawn_draws(front: Direction, random: &LegacyRandom) {
        let (step_x, _, step_z) = front.normal();
        let pow = random.next_double() * 0.1 + 0.2;
        let spread = 0.017_227_5 * crate::dispenser::SPAWN_ACCURACY;
        let _ = random.triangle(f64::from(step_x) * pow, spread);
        let _ = random.triangle(0.2, spread);
        let _ = random.triangle(f64::from(step_z) * pow, spread);
    }

    #[test]
    fn no_recipe_plays_only_the_fail_event() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::STONE, 1));
        put(&mut level, POS, 1, ItemStack::new(items::DIRT, 1));
        let seed = level.random.source().seed();

        let effects = tick(&mut level, POS);

        assert_eq!(level_events(&effects), vec![(SOUND_CRAFTER_FAIL, 0)]);
        assert_eq!(contents(&level, POS)[0], ItemStack::new(items::STONE, 1));
        assert!(!level.get(POS).get(CRAFTING));
        assert_eq!(level.random.source().seed(), seed);
    }

    #[test]
    fn an_empty_grid_plays_only_the_fail_event() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        level.set_container(POS, vec![ItemStack::empty(); GRID * GRID]);
        let seed = level.random.source().seed();

        let effects = tick(&mut level, POS);

        assert_eq!(level_events(&effects), vec![(SOUND_CRAFTER_FAIL, 0)]);
        assert_eq!(level.random.source().seed(), seed);
    }

    #[test]
    fn a_log_in_front_of_a_chest_becomes_four_planks() {
        let mut level = TestLevel::new();
        let chest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        level.set(chest, BlockState(blocks::CHEST));
        level.set_container(chest, vec![ItemStack::empty(); 27]);
        let seed = level.random.source().seed();

        let effects = tick(&mut level, POS);

        assert_eq!(
            contents(&level, chest)[0],
            ItemStack::new(items::OAK_PLANKS, 4)
        );
        assert!(contents(&level, POS)[0].is_empty());
        assert!(level.get(POS).get(CRAFTING));
        assert!(level_events(&effects).is_empty());
        assert_eq!(level.random.source().seed(), seed);
    }

    #[test]
    fn a_log_ejected_into_air_draws_seven_doubles_and_plays_both_events() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        let seed = level.random.source().seed();

        let effects = tick(&mut level, POS);

        assert!(contents(&level, POS)[0].is_empty());
        assert!(level.get(POS).get(CRAFTING));
        assert_eq!(
            level_events(&effects),
            vec![
                (SOUND_CRAFTER_CRAFT, 0),
                (PARTICLES_SHOOT, Direction::North.id())
            ]
        );
        // `TestLevel` 的 `LevelRandom::new(0)` 和这里同一条序列。
        //
        // `TestLevel`'s `LevelRandom::new(0)` is the same sequence as this one.
        let twin = LegacyRandom::new(0);
        assert_eq!(twin.seed(), seed);
        spawn_draws(Direction::North, &twin);
        assert_eq!(level.random.source().seed(), twin.seed());
    }

    /// 朝空气吐出的产物是位置、速度都按官方 `spawnItem` 算好的物品实体。
    #[test]
    fn an_ejected_result_becomes_a_ground_item() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        let _ = tick(&mut level, POS);
        // 朝北：落点是方块中心往北 0.7 格，再按非竖直朝向下移 0.15625。
        let items: Vec<_> = level
            .entities()
            .iter()
            .filter_map(|entity| {
                entity
                    .ground()
                    .map(|item| (entity.position, entity.delta, item.stack.clone()))
            })
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].2, ItemStack::new(items::OAK_PLANKS, 4));
        assert!((items[0].0.x - 0.5).abs() < 1.0e-9);
        assert!((items[0].0.y - 64.5 + 0.15625).abs() < 1.0e-9);
        assert!((items[0].0.z + 0.2).abs() < 1.0e-9);
        assert!(items[0].1.y > 0.0);
    }

    #[test]
    fn a_full_chest_leaves_the_result_to_the_air() {
        let mut level = TestLevel::new();
        let chest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        level.set(chest, BlockState(blocks::CHEST));
        level.set_container(chest, vec![ItemStack::new(items::STONE, 64); 27]);

        let effects = tick(&mut level, POS);

        assert_eq!(contents(&level, chest)[0], ItemStack::new(items::STONE, 64));
        assert!(contents(&level, POS)[0].is_empty());
        assert_eq!(
            level_events(&effects),
            vec![
                (SOUND_CRAFTER_CRAFT, 0),
                (PARTICLES_SHOOT, Direction::North.id())
            ]
        );
    }

    #[test]
    fn cake_remainders_are_three_buckets_in_the_chest() {
        let mut level = TestLevel::new();
        let chest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        for slot in 0..3 {
            put(&mut level, POS, slot, ItemStack::new(items::MILK_BUCKET, 1));
        }
        put(&mut level, POS, 3, ItemStack::new(items::SUGAR, 1));
        put(&mut level, POS, 4, ItemStack::new(items::EGG, 1));
        put(&mut level, POS, 5, ItemStack::new(items::SUGAR, 1));
        for slot in 6..9 {
            put(&mut level, POS, slot, ItemStack::new(items::WHEAT, 1));
        }
        level.set(chest, BlockState(blocks::CHEST));
        level.set_container(chest, vec![ItemStack::empty(); 27]);

        let effects = tick(&mut level, POS);

        let chest_slots = contents(&level, chest);
        assert_eq!(chest_slots[0], ItemStack::new(items::CAKE, 1));
        assert_eq!(chest_slots[1], ItemStack::new(items::BUCKET, 3));
        assert!(contents(&level, POS).iter().all(ItemStack::is_empty));
        assert!(level_events(&effects).is_empty());
    }

    #[test]
    fn a_honey_bottle_leaves_a_glass_bottle_in_the_chest() {
        let mut level = TestLevel::new();
        let chest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::HONEY_BOTTLE, 1));
        level.set(chest, BlockState(blocks::CHEST));
        level.set_container(chest, vec![ItemStack::empty(); 27]);

        let effects = tick(&mut level, POS);

        let chest_slots = contents(&level, chest);
        assert_eq!(chest_slots[0], ItemStack::new(items::SUGAR, 3));
        assert_eq!(chest_slots[1], ItemStack::new(items::GLASS_BOTTLE, 1));
        assert!(contents(&level, POS)[0].is_empty());
        assert!(level_events(&effects).is_empty());
    }

    #[test]
    fn four_planks_enter_an_empty_crafter_one_at_a_time() {
        let mut level = TestLevel::new();
        let dest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        facing_north(&mut level, dest);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));

        tick(&mut level, POS);

        let dest_slots = contents(&level, dest);
        for stack in dest_slots.iter().take(4) {
            assert_eq!(*stack, ItemStack::new(items::OAK_PLANKS, 1));
        }
        assert!(dest_slots[4].is_empty());
    }

    #[test]
    fn a_disabled_empty_slot_stays_disabled_while_another_slot_crafts() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 1, ItemStack::new(items::OAK_LOG, 1));
        level.set_crafter_disabled(POS, 1);

        tick(&mut level, POS);

        assert!(contents(&level, POS)[1].is_empty());
        assert_eq!(level.crafter_disabled(POS), 1);
        assert!(level.get(POS).get(CRAFTING));
    }

    #[test]
    fn an_empty_hopper_in_front_waits_eight_ticks() {
        let mut level = TestLevel::new();
        let hopper = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        level.set(hopper, BlockState(blocks::HOPPER));
        level.set_container(hopper, vec![ItemStack::empty(); 5]);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block_entity(Box::new(HopperEntity::new(hopper)));
        let state = world.block_state(POS);
        state.tick(&mut world, POS);

        assert_eq!(
            world
                .block_entity::<HopperEntity>(hopper)
                .unwrap()
                .cooldown_time(),
            8
        );
        assert_eq!(
            crate::hopper::query(&mut world, hopper, &mut |container| container.get(0)).unwrap(),
            ItemStack::new(items::OAK_PLANKS, 4)
        );
    }

    #[test]
    fn six_block_entity_ticks_clear_crafting_and_scheduled_ticks_do_not() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        tick(&mut level, POS);
        assert!(level.get(POS).get(CRAFTING));

        let mut ticks = BlockTicks::new();
        // 计划刻那一段不减倒计时，只有方块实体段的 ticker 减。
        ticks.set_time(1);
        crate::interact::run_scheduled_ticks(&mut level, &mut ticks, 1);
        assert!(level.get(POS).get(CRAFTING));

        for now in 1..=5 {
            ticks.set_time(now);
            crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        }
        assert!(level.get(POS).get(CRAFTING));
        ticks.set_time(6);
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        assert!(!level.get(POS).get(CRAFTING));
    }

    #[test]
    fn dropping_the_signal_clears_the_flag_but_the_clock_still_runs() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        let powered = level.get(POS).with(TRIGGERED, true).with(CRAFTING, true);
        level.set(POS, powered);
        level.set_crafter_crafting(POS, 2);
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            let state = world.block_state(POS);
            let block = state.block();
            state.neighbor_changed(&mut world, POS, block);
            assert!(!world.block_state(POS).get(CRAFTING));
            assert!(!world.block_state(POS).get(TRIGGERED));
        }
        ticks.set_time(1);
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        ticks.set_time(2);
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        assert!(!level.get(POS).get(CRAFTING));
    }

    /// 官方 ticker 的逐刻模型：区块能 tick 且在边界内才减 1，减到 0 关 `crafting`。
    struct ClockModel {
        remaining: i32,
        crafting: bool,
    }

    /// 一排合成器跨过边界和两个区块，倒计时反复亮起，同时冻结 / 恢复区块、
    /// 缩小 / 扩大边界、装盒再换个时钟放回。每一刻和官方 ticker 的模型比较。
    #[test]
    fn countdowns_pause_while_frozen_or_outside_the_border() {
        use mc_bamboo_core::ChunkPos;
        // x = 14..=17 跨过区块 0 / 1 的分界；边界中心在 x = 16。
        let row: Vec<BlockPos> = (12..=19).map(|x| BlockPos::new(x, 64, 0)).collect();
        let chunk = ChunkPos::new(0, 0);
        for kind in 0..4 {
            let mut level = TestLevel::new();
            let mut ticks = BlockTicks::new();
            let mut models: Vec<ClockModel> = Vec::new();
            for &pos in &row {
                facing_north(&mut level, pos);
                models.push(ClockModel {
                    remaining: 0,
                    crafting: false,
                });
            }
            level.border.set_center(16.0, 0.5);
            match kind {
                // 缩小渐变：从全包住缩到只剩中间。
                0 => {
                    level.border.lerp_size_between(20.0, 1.0, 60, 0);
                }
                // 扩大渐变：从只剩中间到全包住。
                1 => {
                    level.border.lerp_size_between(1.0, 20.0, 60, 0);
                }
                // 一下子跳小再跳大。
                _ => {
                    level.border.set_size(20.0);
                }
            }
            let mut frozen = false;
            for now in 1..=160_i64 {
                ticks.set_time(now);
                level.border.tick();
                if kind >= 2 && now % 23 == 0 {
                    let size = if level.border.size() > 10.0 {
                        3.0
                    } else {
                        20.0
                    };
                    level.border.set_size(size);
                }
                if kind != 2 && now % 7 == 0 {
                    frozen = !frozen;
                    level.set_frozen(chunk, frozen);
                }
                if now % 19 == 0 {
                    // 区块束带着剩余刻数走，放回的表时钟差很多也不影响。
                    level.box_redstone(chunk, now, now + 1000);
                    level.box_redstone(ChunkPos::new(1, 0), now + 1000, now);
                }
                // 隔几刻让其中一台重新亮起，和 `dispenseFrom` 一样先记倒计时再写状态。
                let lit = (now as usize * 5) % row.len();
                if now % 3 == 0 {
                    let pos = row[lit];
                    level.set_crafter_crafting(pos, MAX_CRAFTING_TICKS);
                    let state = level.get(pos).with(CRAFTING, true);
                    level.set(pos, state);
                    models[lit].remaining = MAX_CRAFTING_TICKS;
                    models[lit].crafting = true;
                }
                crate::interact::run_block_entity_tick(&mut level, &mut ticks);
                let border = level.border.block_box();
                for (index, &pos) in row.iter().enumerate() {
                    let model = &mut models[index];
                    let ticking = !(frozen && pos.chunk() == chunk) && border.contains(pos);
                    if ticking && model.remaining > 0 {
                        model.remaining -= 1;
                        if model.remaining == 0 {
                            model.crafting = false;
                        }
                    }
                    assert_eq!(
                        level.get(pos).get(CRAFTING),
                        model.crafting,
                        "kind {kind} tick {now} {pos:?}"
                    );
                    assert_eq!(
                        level.crafter_crafting_at(pos),
                        model.remaining,
                        "kind {kind} tick {now} {pos:?}"
                    );
                }
            }
        }
    }

    /// 冻结期间一直亮着，恢复那一刻起接着倒数剩下的刻数。
    #[test]
    fn a_frozen_crafter_resumes_its_countdown() {
        use mc_bamboo_core::ChunkPos;
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));
        tick(&mut level, POS);
        let mut ticks = BlockTicks::new();
        for now in 1..=2 {
            ticks.set_time(now);
            crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        }
        assert_eq!(level.crafter_crafting_at(POS), 4);
        level.set_frozen(ChunkPos::new(0, 0), true);
        for now in 3..=50 {
            ticks.set_time(now);
            crate::interact::run_block_entity_tick(&mut level, &mut ticks);
            assert!(level.get(POS).get(CRAFTING));
        }
        assert_eq!(level.crafter_crafting_at(POS), 4);
        level.set_frozen(ChunkPos::new(0, 0), false);
        for now in 51..=53 {
            ticks.set_time(now);
            crate::interact::run_block_entity_tick(&mut level, &mut ticks);
            assert!(level.get(POS).get(CRAFTING));
        }
        ticks.set_time(54);
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        assert!(!level.get(POS).get(CRAFTING));
        assert_eq!(level.crafter_crafting_at(POS), 0);
    }
}
