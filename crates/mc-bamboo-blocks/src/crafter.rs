//! 合成器的合成和吐出，官方 `CrafterBlock.dispenseFrom`。
//!
//! 计划刻到期时查找 3×3 配方。没有配方或产物是空的，只发失败事件 1050。
//! 成功时亮起 `crafting` 六刻，把产物和剩余物朝前吐出，九格里每个非空物品再减 1。
//! 前方是容器就按 `HopperBlockEntity.addItem` 放进去；放不下的部分走
//! [`spawn_dispensed_item`]。这里不保存官方 `RecipeCache` 的那张表，查找仍然是
//! 注册表里的第一条匹配，结果和有缓存时一样。
//!
//! `CrafterBlock.dispenseFrom`. A due tick looks up the 3×3 recipe. No recipe, or an empty
//! result, only plays fail event 1050. A success turns `crafting` on for six ticks, ejects the
//! result and the remainders forward, then shrinks every non-empty grid stack by one.
//! A container in front is filled with `HopperBlockEntity.addItem`. Whatever does not fit goes
//! through [`spawn_dispensed_item`]. The vanilla `RecipeCache` table is not stored; the lookup
//! is still the first registry match, so the result is the same as a cache hit.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, RecipeType};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_recipes::{CraftingInput, RecipeManager, assemble, remaining_items};

use crate::{
    container::copy_with_count,
    hopper::{Added, add_item, apply_hopper_cooldown, set_changed},
    state::{BlockState, CRAFTING, FrontAndTop},
    world::{UPDATE_CLIENTS, World},
};

/// 合成格的边长。官方 `CrafterBlockEntity.getWidth`。
///
/// Width of the crafting grid. Vanilla `CrafterBlockEntity.getWidth`.
const GRID: usize = 3;
/// `CrafterBlock.MAX_CRAFTING_TICKS`。`crafting` 亮着的刻数。
///
/// How many ticks `crafting` stays on.
const MAX_CRAFTING_TICKS: i32 = 6;
/// `LevelEvent.SOUND_CRAFTER_FAIL`。
const SOUND_CRAFTER_FAIL: i32 = 1050;
/// `LevelEvent.SOUND_CRAFTER_CRAFT`。只有没放进容器、变成掉落物时才发。
///
/// Played only when a stack does not fit and becomes a dropped item.
const SOUND_CRAFTER_CRAFT: i32 = 1049;
/// `LevelEvent.PARTICLES_SHOOT_WHITE_SMOKE`。数据和朝向的 `get3DDataValue` 一样。
///
/// The data is the facing's `get3DDataValue`.
const PARTICLES_SHOOT: i32 = 2010;
/// `DefaultDispenseItemBehavior.spawnItem` 的精度参数。合成器传的是 6。
///
/// Accuracy passed to `spawnItem`. The crafter passes 6.
const SPAWN_ACCURACY: i32 = 6;

/// 九格现在能做出的产物。空输入或没有配方时是空物品。
///
/// The result the nine slots would craft. Empty when the input or the recipe is empty.
pub fn potential_result(slots: &[ItemStack]) -> ItemStack {
    let Some(input) = craft_input(slots) else {
        return ItemStack::empty();
    };
    crafted(&input).unwrap_or_else(ItemStack::empty)
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
    let Some(result) = crafted(&input) else {
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
        return;
    };
    let Some(recipe) = RecipeManager::global().get_recipe_for(RecipeType::Crafting, &input, None)
    else {
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
        return;
    };
    let front = front_of(state.get(ORIENTATION));
    // 倒计时先记下来。接着改 `crafting` 只换属性，不会把方块实体清掉。
    //
    // Record the countdown first. Turning `crafting` on only changes a property and does not
    // remove the block entity.
    world
        .redstone_memory_mut()
        .set_crafter_crafting(pos, MAX_CRAFTING_TICKS);
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

/// 游戏刻末尾减 `craftingTicksRemaining`。方块 oracle 不跑这段，所以比较时 `crafting` 保持亮着。
///
/// Decrements `craftingTicksRemaining` at the end of a game tick. The block oracle does not run
/// this, so `crafting` stays on while a case is compared.
pub(crate) fn tick_crafting_clocks(world: &mut World) {
    let due = world.redstone_memory_mut().take_crafter_crafting();
    let mut kept = Vec::new();
    for (pos, remaining) in due {
        let next = remaining - 1;
        if next < 0 || !is_crafter(world, pos) {
            continue;
        }
        if next == 0 {
            let state = world.block_state(pos);
            world.set_block_and_update(pos, state.with(CRAFTING, false));
        } else {
            kept.push((pos, next));
        }
    }
    kept.retain(|(pos, _)| is_crafter(world, *pos));
    world.redstone_memory_mut().restore_crafter_crafting(kept);
}

/// 官方 `DefaultDispenseItemBehavior.spawnItem` 里从 `level.random` 取的那 7 个数。
///
/// 竖直朝向把生成点的 Y 减 0.125，其它朝向减 0.15625。`ItemEntity` 用自己的
/// `RandomSource.create()` 取 `bobOffs`、朝向和初始速度，不碰世界随机源；
/// `spawnItem` 随后用世界随机源把速度盖掉。物品实体还没接上，这 7 个数照取，物品消失。
/// 物品实体接上以后在这里生成，不要再多取随机数。
///
/// The seven `level.random` draws inside `DefaultDispenseItemBehavior.spawnItem`.
///
/// Vertical facing lowers the spawn Y by 0.125, every other facing by 0.15625. The `ItemEntity`
/// draws `bobOffs`, yaw, and its first velocity from its own `RandomSource.create()`, not from
/// the level; `spawnItem` then overwrites the velocity from the level random. Item entities are
/// not implemented, so these seven numbers are still drawn and the item vanishes. When item
/// entities exist, spawn them here and do not draw any further randoms.
pub fn spawn_dispensed_item(world: &mut World, stack: &ItemStack, front: Direction) {
    if stack.is_empty() {
        return;
    }
    let (step_x, _, step_z) = front.normal();
    let pow = world.random().next_double() * 0.1 + 0.2;
    let spread = 0.017_227_5 * f64::from(SPAWN_ACCURACY);
    let _ = world
        .random()
        .triangle(f64::from(step_x) * pow, spread);
    let _ = world.random().triangle(0.2, spread);
    let _ = world
        .random()
        .triangle(f64::from(step_z) * pow, spread);
}

fn craft_input(slots: &[ItemStack]) -> Option<CraftingInput> {
    if slots.len() < GRID * GRID {
        return None;
    }
    let input = CraftingInput::of(GRID, GRID, &slots[..GRID * GRID]);
    if input.is_empty() { None } else { Some(input) }
}

fn crafted(input: &CraftingInput) -> Option<ItemStack> {
    RecipeManager::global()
        .get_recipe_for(RecipeType::Crafting, input, None)
        .and_then(|recipe| assemble(recipe, input.slots()))
        .filter(|stack| !stack.is_empty())
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
    world
        .block_state(pos)
        .is_a(BlockAncestor::CrafterBlock)
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
    spawn_dispensed_item(world, &left, front);
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

fn insert_whole(
    world: &mut World,
    dest: BlockPos,
    stack: ItemStack,
    into: Direction,
) -> ItemStack {
    let mut remaining = stack;
    loop {
        if remaining.is_empty() {
            break;
        }
        let before = remaining.count;
        let Some(added) = place(world, dest, remaining, into) else {
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
    if !world.with_container(dest, &mut |container| {
        added = Some(add_item(container, stack, Some(into)));
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
        BlockState, BlockTicks, HopperEntity, LegacyRandom,
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
        let spread = 0.017_227_5 * f64::from(SPAWN_ACCURACY);
        let _ = random.triangle(f64::from(step_x) * pow, spread);
        let _ = random.triangle(0.2, spread);
        let _ = random.triangle(f64::from(step_z) * pow, spread);
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

        assert_eq!(contents(&level, chest)[0], ItemStack::new(items::OAK_PLANKS, 4));
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
            vec![(SOUND_CRAFTER_CRAFT, 0), (PARTICLES_SHOOT, Direction::North.id())]
        );
        let twin = LegacyRandom::new(seed);
        spawn_draws(Direction::North, &twin);
        assert_eq!(level.random.source().seed(), twin.seed());
    }

    #[test]
    fn cake_remainders_are_three_buckets_in_the_chest() {
        let mut level = TestLevel::new();
        let chest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        for slot in 0..3 {
            put(
                &mut level,
                POS,
                slot,
                ItemStack::new(items::MILK_BUCKET, 1),
            );
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
    fn four_planks_enter_an_empty_crafter_one_at_a_time() {
        let mut level = TestLevel::new();
        let dest = POS.relative(Direction::North);
        facing_north(&mut level, POS);
        facing_north(&mut level, dest);
        put(&mut level, POS, 0, ItemStack::new(items::OAK_LOG, 1));

        tick(&mut level, POS);

        let dest_slots = contents(&level, dest);
        for slot in 0..4 {
            assert_eq!(dest_slots[slot], ItemStack::new(items::OAK_PLANKS, 1));
        }
        assert!(dest_slots[4].is_empty());
    }

    #[test]
    fn a_disabled_empty_slot_stays_disabled_while_another_slot_crafts() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        put(&mut level, POS, 1, ItemStack::new(items::OAK_LOG, 1));
        level.redstone_mut().set_crafter_disabled(POS, 1);

        tick(&mut level, POS);

        assert!(contents(&level, POS)[1].is_empty());
        assert_eq!(level.redstone().crafter_disabled(POS), 1);
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
            world.block_entity::<HopperEntity>(hopper).unwrap().cooldown_time(),
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
        crate::interact::run_scheduled_ticks(&mut level, &mut ticks, 0);
        assert!(level.get(POS).get(CRAFTING));

        for _ in 0..5 {
            crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        }
        assert!(level.get(POS).get(CRAFTING));
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        assert!(!level.get(POS).get(CRAFTING));
    }

    #[test]
    fn dropping_the_signal_clears_the_flag_but_the_clock_still_runs() {
        let mut level = TestLevel::new();
        facing_north(&mut level, POS);
        let powered = level
            .get(POS)
            .with(TRIGGERED, true)
            .with(CRAFTING, true);
        level.set(POS, powered);
        level.redstone_mut().set_crafter_crafting(POS, 2);
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            let state = world.block_state(POS);
            let block = state.block();
            state.neighbor_changed(&mut world, POS, block);
            assert!(!world.block_state(POS).get(CRAFTING));
            assert!(!world.block_state(POS).get(TRIGGERED));
        }
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        crate::interact::run_block_entity_tick(&mut level, &mut ticks);
        assert!(!level.get(POS).get(CRAFTING));
    }
}
