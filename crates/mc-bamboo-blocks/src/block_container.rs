//! 雕纹书架、唱片机、饰纹陶罐和合成器的物品栏。
//!
//! 这四种都实现了官方的 `Container`，但不是 `WorldlyContainer`：漏斗和投掷器用整张槽位表。
//! 副作用（书架的 `updateState`、唱片机的 `setTheItem`、合成器禁用槽被写入时清掉禁用位）
//! 记在回调里，等槽位借用结束再写回世界，和堆肥桶同一套做法。
//!
//! Inventories for the chiseled bookshelf, jukebox, decorated pot and crafter.
//!
//! All four implement vanilla `Container` and none of them is a `WorldlyContainer`, so hoppers
//! and droppers use the whole slot list. Side effects (`ChiseledBookShelfBlockEntity.updateState`,
//! `JukeboxBlockEntity.setTheItem`, and a crafter clearing a disabled slot on `setItem`) are
//! recorded during the callback and applied after the slots are no longer borrowed, the same
//! pattern as the composter.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    BlockAncestor, jukebox_songs, registries::data_component_type as kinds, tags::item_tags,
};
use mc_bamboo_protocol::{
    components::{ComponentValue, cursor::Holder, defaults},
    item::ItemStack,
};

use crate::{
    container::{
        CONTAINER_STACK_LIMIT, Container, helper_remove_item, item_max, redstone_signal,
        same_item_same_components,
    },
    state::{
        BlockState, HAS_RECORD, Property, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED, SLOT_2_OCCUPIED,
        SLOT_3_OCCUPIED, SLOT_4_OCCUPIED, SLOT_5_OCCUPIED, item_is_tagged,
    },
    world::{UPDATE_CLIENTS, World},
};

/// `ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES`。
const SLOT_OCCUPIED: [Property<bool>; 6] = [
    SLOT_0_OCCUPIED,
    SLOT_1_OCCUPIED,
    SLOT_2_OCCUPIED,
    SLOT_3_OCCUPIED,
    SLOT_4_OCCUPIED,
    SLOT_5_OCCUPIED,
];

/// 这四种容器。槽位数和 `getMaxStackSize()` 跟官方的方块实体一样。
///
/// The four containers. Slot count and `getMaxStackSize()` match the vanilla block entities.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bookshelf,
    Jukebox,
    Pot,
    Crafter,
}

impl Kind {
    const fn len(self) -> usize {
        match self {
            Self::Bookshelf => 6,
            Self::Jukebox | Self::Pot => 1,
            Self::Crafter => 9,
        }
    }

    const fn max_stack(self) -> i32 {
        match self {
            Self::Bookshelf | Self::Jukebox => 1,
            Self::Pot | Self::Crafter => CONTAINER_STACK_LIMIT,
        }
    }
}

/// 这一格是不是这四种容器之一。
///
/// Whether this block is one of the four containers.
pub(crate) fn is_block_container(state: BlockState) -> bool {
    kind_of(state).is_some()
}

fn kind_of(state: BlockState) -> Option<Kind> {
    if state.is_a(BlockAncestor::ChiseledBookShelfBlock) {
        Some(Kind::Bookshelf)
    } else if state.is_a(BlockAncestor::JukeboxBlock) {
        Some(Kind::Jukebox)
    } else if state.is_a(BlockAncestor::DecoratedPotBlock) {
        Some(Kind::Pot)
    } else if state.is_a(BlockAncestor::CrafterBlock) {
        Some(Kind::Crafter)
    } else {
        None
    }
}

/// 一次 `with_container` 里要等借用结束才写进世界的变化。
///
/// Changes from one `with_container` call that wait until the slots are no longer borrowed.
#[derive(Default)]
struct Pending {
    /// `ChiseledBookShelfBlockEntity.updateState` 要刷新的那一格。
    ///
    /// The slot `ChiseledBookShelfBlockEntity.updateState` should refresh.
    bookshelf: Option<usize>,
    /// 唱片槽被 `setTheItem` 写过。
    ///
    /// The record slot was written by `setTheItem`.
    jukebox: bool,
    /// 合成器禁用位被 `setItem` 清掉之后的掩码。
    ///
    /// The crafter disabled-slot mask after `setItem` cleared a bit.
    crafter_disabled: Option<u16>,
}

/// 漏斗、投掷器和玩家共用的入口。
///
/// Shared entry for hoppers, droppers and players.
pub(crate) fn with_container(
    world: &mut World,
    pos: BlockPos,
    f: &mut dyn FnMut(&mut dyn Container),
) -> bool {
    let Some(kind) = kind_of(world.block_state(pos)) else {
        return false;
    };
    let disabled = if kind == Kind::Crafter {
        world.redstone_memory().crafter_disabled(pos)
    } else {
        0
    };
    let mut pending = Pending::default();
    let ran = world.with_slots(pos, kind.len(), &mut |slots| {
        let mut view = View {
            kind,
            slots,
            disabled,
            pending: &mut pending,
        };
        f(&mut view);
    });
    if !ran {
        return false;
    }
    apply(world, pos, &pending);
    true
}

fn apply(world: &mut World, pos: BlockPos, pending: &Pending) {
    if let Some(slot) = pending.bookshelf {
        apply_bookshelf(world, pos, slot);
    }
    if pending.jukebox {
        apply_jukebox(world, pos);
    }
    if let Some(mask) = pending.crafter_disabled {
        world.redstone_memory_mut().set_crafter_disabled(pos, mask);
    }
}

/// `ChiseledBookShelfBlockEntity.updateState`：记下最近动过的格子，再按物品刷新全部 `slot_N_occupied`。
///
/// `ChiseledBookShelfBlockEntity.updateState`: remember the slot, then mirror every item into
/// `slot_N_occupied`.
fn apply_bookshelf(world: &mut World, pos: BlockPos, slot: usize) {
    if slot >= SLOT_OCCUPIED.len() {
        return;
    }
    world
        .redstone_memory_mut()
        .set_last_bookshelf_slot(pos, slot);
    let slots = world.slots_at(pos);
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::ChiseledBookShelfBlock) {
        return;
    }
    let mut updated = state;
    for (index, property) in SLOT_OCCUPIED.iter().enumerate() {
        let occupied = slots.get(index).is_some_and(|stack| !stack.is_empty());
        updated = updated.with(*property, occupied);
    }
    world.set_block_and_update(pos, updated);
}

/// `JukeboxBlockEntity.setTheItem` 写完物品之后的部分。
///
/// `notifyItemChangedInJukebox` 用标志 2（只更新客户端，不通知邻居）改 `has_record`。
/// 有 `jukebox_playable` 才 `play`，否则正在放的歌 `stop`。`play` / `stop` 自己会 `onSongChanged`。
///
/// The part of `JukeboxBlockEntity.setTheItem` that follows the assignment.
///
/// `notifyItemChangedInJukebox` writes `has_record` with flag 2 (clients only, no neighbor
/// updates). A stack with `jukebox_playable` calls `play`; otherwise a song that is already
/// playing `stop`s. `play` and `stop` call `onSongChanged` themselves.
fn apply_jukebox(world: &mut World, pos: BlockPos) {
    let slots = world.slots_at(pos);
    let has_item = slots.first().is_some_and(|stack| !stack.is_empty());
    let state = world.block_state(pos);
    if state.is_a(BlockAncestor::JukeboxBlock) && state.get(HAS_RECORD) != has_item {
        world.set_block(pos, state.with(HAS_RECORD, has_item), UPDATE_CLIENTS);
    }
    let playable = has_item && slots.first().is_some_and(has_jukebox_playable);
    let was_playing = world.redstone_memory().is_jukebox_playing(pos);
    if playable {
        world.redstone_memory_mut().set_jukebox_playing(pos, true);
        on_jukebox_song_changed(world.block_state(pos), world, pos);
    } else if was_playing {
        world.redstone_memory_mut().set_jukebox_playing(pos, false);
        on_jukebox_song_changed(world.block_state(pos), world, pos);
    }
}

/// `JukeboxBlockEntity.onSongChanged`：邻居和比较器都重读。
///
/// `JukeboxBlockEntity.onSongChanged`: neighbors and comparators both re-read.
pub(crate) fn on_jukebox_song_changed(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbors_at(pos, state.block());
    world.update_neighbour_for_output_signal(pos, state.block());
}

/// 玩家右键放进唱片。官方 `JukeboxPlayable.tryInsertIntoJukebox` 在 `has_record` 为假时 `setTheItem`。
///
/// A player inserting a disc. Vanilla `JukeboxPlayable.tryInsertIntoJukebox` calls `setTheItem`
/// while `has_record` is false.
pub(crate) fn set_jukebox_item(world: &mut World, pos: BlockPos, stack: ItemStack) {
    with_container(world, pos, &mut |container| container.set(0, stack.clone()));
}

/// `JukeboxBlockEntity.popOutTheItem`：物品非空才拿走、取两个 `nextFloat`，然后再 `onSongChanged`。
///
/// 物品实体不生成。`setTheItem(EMPTY)` 的停歌通知发生在取数之前。
///
/// `JukeboxBlockEntity.popOutTheItem`: only a non-empty item is removed, then two `nextFloat`s,
/// then another `onSongChanged`.
///
/// No item entity is spawned. The stop notification inside `setTheItem(EMPTY)` happens before
/// the random calls.
pub(crate) fn pop_jukebox_record(world: &mut World, pos: BlockPos) {
    let empty = world.slots_at(pos).first().is_none_or(ItemStack::is_empty);
    if empty {
        return;
    }
    with_container(world, pos, &mut |container| {
        container.set(0, ItemStack::empty());
    });
    world.random().next_float();
    world.random().next_float();
    on_jukebox_song_changed(world.block_state(pos), world, pos);
}

/// 玩家往书架的一格放进或拿走一本书。`setItem` 会走 `updateState`。
///
/// A player putting a book into a shelf slot or taking one out. `setItem` runs `updateState`.
pub(crate) fn set_bookshelf_slot(world: &mut World, pos: BlockPos, slot: usize, stack: ItemStack) {
    with_container(world, pos, &mut |container| {
        container.set(slot, stack.clone())
    });
}

/// 玩家往陶罐里放一件。能叠才返回真。漏斗不走这里，所以不摇晃。
///
/// Whether a player can insert one item into the pot. Hoppers do not use this, so they do not wobble.
pub(crate) fn pot_accepts(world: &World, pos: BlockPos, item: i32) -> bool {
    if ItemStack::new(item, 1).is_empty() {
        return false;
    }
    let current = world
        .slots_at(pos)
        .into_iter()
        .next()
        .unwrap_or_else(ItemStack::empty);
    let held = ItemStack::new(item, 1);
    current.is_empty()
        || same_item_same_components(&current, &held) && current.count < item_max(&current)
}

/// 玩家放进一件。空罐直接放，否则 `grow(1)`。调用方已经确认能叠。
///
/// A player inserts one item. An empty pot stores it; otherwise the stack `grow`s by one.
/// The caller has already checked that it fits.
pub(crate) fn insert_pot_item(world: &mut World, pos: BlockPos, item: i32) {
    with_container(world, pos, &mut |container| {
        let current = container.get(0);
        if current.is_empty() {
            container.set(0, ItemStack::new(item, 1));
        } else {
            let mut grown = current;
            grown.count += 1;
            container.set(0, grown);
        }
    });
}

/// `JukeboxBlockEntity.getComparatorOutput`。
///
/// `JukeboxBlockEntity.getComparatorOutput`.
pub(crate) fn jukebox_signal(world: &World, pos: BlockPos) -> i32 {
    world
        .slots_at(pos)
        .first()
        .map(jukebox_comparator_output)
        .unwrap_or(0)
}

/// `DecoratedPotBlock.getAnalogOutputSignal`：单格的 `getRedstoneSignalFromContainer`。
///
/// `DecoratedPotBlock.getAnalogOutputSignal`: `getRedstoneSignalFromContainer` on the one slot.
pub(crate) fn pot_signal(world: &World, pos: BlockPos) -> i32 {
    let slots = world.slots_at(pos);
    redstone_signal(slots.iter(), 1)
}

/// `CrafterBlockEntity.getRedstoneSignal`：非空或者被禁用的格子数，直接就是 0 到 9。
///
/// `CrafterBlockEntity.getRedstoneSignal`: how many slots are non-empty or disabled, already 0 to 9.
pub(crate) fn crafter_signal(world: &World, pos: BlockPos) -> i32 {
    let slots = world.slots_at(pos);
    let disabled = world.redstone_memory().crafter_disabled(pos);
    let mut count = 0;
    for slot in 0..Kind::Crafter.len() {
        let filled = slots.get(slot).is_some_and(|stack| !stack.is_empty());
        let off = slot < 16 && disabled & (1 << slot) != 0;
        if filled || off {
            count += 1;
        }
    }
    count
}

/// 这件物品的默认组件或补丁里有没有 `jukebox_playable`。
///
/// Whether the stack's resolved components include `jukebox_playable`.
pub(crate) fn has_jukebox_playable(stack: &ItemStack) -> bool {
    matches!(
        defaults::resolve(stack.item, &stack.patch(), kinds::JUKEBOX_PLAYABLE),
        Some(ComponentValue::JukeboxPlayable(_))
    )
}

/// 玩家手上只有物品 ID。官方假玩家用的是默认组件，和 `Item.jukebox_playable` 一致。
///
/// A player's hand only carries an item id. The oracle's fake player uses default components,
/// which matches `Item.jukebox_playable`.
pub(crate) fn item_has_jukebox_song(item: i32) -> bool {
    has_jukebox_playable(&ItemStack::new(item, 1))
}

fn jukebox_comparator_output(stack: &ItemStack) -> i32 {
    let patch = stack.patch();
    let Some(ComponentValue::JukeboxPlayable(song)) =
        defaults::resolve(stack.item, &patch, kinds::JUKEBOX_PLAYABLE)
    else {
        return 0;
    };
    match song {
        Holder::Id(id) => jukebox_songs::get(*id)
            .map(|song| song.comparator_output)
            .unwrap_or(0),
        Holder::Inline(song) => song.comparator_output,
    }
}

struct View<'a> {
    kind: Kind,
    slots: &'a mut [ItemStack],
    disabled: u16,
    pending: &'a mut Pending,
}

impl View<'_> {
    fn disabled(&self, slot: usize) -> bool {
        slot < 16 && self.disabled & (1 << slot) != 0
    }

    /// `CrafterBlockEntity.smallerStackExist`：只看后面的、没禁用的格子。
    ///
    /// `CrafterBlockEntity.smallerStackExist`: only later slots that are not disabled.
    fn smaller_stack_exist(&self, base_size: i32, base: &ItemStack, base_slot: usize) -> bool {
        self.slots
            .iter()
            .enumerate()
            .skip(base_slot + 1)
            .take(Kind::Crafter.len().saturating_sub(base_slot + 1))
            .any(|(slot, stack)| {
                !self.disabled(slot)
                    && (stack.is_empty()
                        || stack.count < base_size && same_item_same_components(stack, base))
            })
    }
}

impl Container for View<'_> {
    fn size(&self) -> usize {
        self.kind.len()
    }

    fn get(&self, slot: usize) -> ItemStack {
        self.slots
            .get(slot)
            .cloned()
            .unwrap_or_else(ItemStack::empty)
    }

    fn max_stack_size(&self) -> i32 {
        self.kind.max_stack()
    }

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        match self.kind {
            Kind::Bookshelf => {
                item_is_tagged(stack.item, item_tags::BOOKSHELF_BOOKS)
                    && (self.get(slot).is_empty()
                        || self.get(slot).count < self.max_stack_size_of(stack))
            }
            Kind::Jukebox => slot == 0 && self.get(0).is_empty() && has_jukebox_playable(stack),
            Kind::Pot => true,
            Kind::Crafter => {
                if self.disabled(slot) {
                    return false;
                }
                let current = self.get(slot);
                if current.count >= item_max(&current) {
                    return false;
                }
                current.is_empty() || !self.smaller_stack_exist(current.count, &current, slot)
            }
        }
    }

    fn can_take_item_into(
        &self,
        _slot: usize,
        stack: &ItemStack,
        into: &[ItemStack],
        into_max: i32,
    ) -> bool {
        match self.kind {
            Kind::Bookshelf => into.iter().any(|to| {
                to.is_empty()
                    || same_item_same_components(stack, to)
                        && to.count + stack.count <= into_max.min(item_max(to))
            }),
            Kind::Jukebox => into.iter().any(ItemStack::is_empty),
            Kind::Pot | Kind::Crafter => true,
        }
    }

    /// 唱片机的 `splitTheItem` 忽略数量，整堆拿走并 `setTheItem(EMPTY)`。
    /// 书架的 `removeItem` 同样整格清空，空格不再 `updateState`。
    ///
    /// The jukebox `splitTheItem` ignores the count, takes the whole stack and calls
    /// `setTheItem(EMPTY)`. The bookshelf `removeItem` also clears the slot, and an empty
    /// slot does not `updateState`.
    fn remove_item(&mut self, slot: usize, count: i32) -> ItemStack {
        match self.kind {
            Kind::Jukebox => {
                if slot != 0 {
                    return ItemStack::empty();
                }
                let retrieved = self.get(0);
                self.set(0, ItemStack::empty());
                retrieved
            }
            Kind::Bookshelf => {
                let retrieved = self.get(slot);
                self.set(slot, ItemStack::empty());
                retrieved
            }
            Kind::Pot | Kind::Crafter => helper_remove_item(self, slot, count),
        }
    }

    fn set(&mut self, slot: usize, stack: ItemStack) {
        if slot >= self.slots.len() {
            return;
        }
        match self.kind {
            Kind::Bookshelf => {
                if stack.is_empty() {
                    if !self.slots[slot].is_empty() {
                        self.slots[slot] = ItemStack::empty();
                        self.pending.bookshelf = Some(slot);
                    }
                } else if item_is_tagged(stack.item, item_tags::BOOKSHELF_BOOKS) {
                    self.slots[slot] = stack;
                    self.pending.bookshelf = Some(slot);
                }
            }
            Kind::Jukebox => {
                self.slots[slot] = stack;
                self.pending.jukebox = true;
            }
            Kind::Pot => self.slots[slot] = stack,
            Kind::Crafter => {
                // `setItem`：禁用的空格被写入之前先 `setSlotState(slot, true)`。
                // 漏斗的 `canPlaceItem` 不会选中禁用格。菜单这次不做，所以没有第二次 `setChanged`。
                //
                // `setItem` calls `setSlotState(slot, true)` before writing a disabled empty slot.
                // A hopper's `canPlaceItem` never selects a disabled slot. Menus are out of scope,
                // so the extra `setChanged` from that path is not emitted here.
                if self.disabled(slot) && self.slots[slot].is_empty() {
                    self.disabled &= !(1 << slot);
                    self.pending.crafter_disabled = Some(self.disabled);
                }
                self.slots[slot] = limit_stack(self, stack);
            }
        }
    }
}

/// `ItemStack.limitSize`。空堆不改。
///
/// `ItemStack.limitSize`. An empty stack is left alone.
fn limit_stack(container: &View<'_>, mut stack: ItemStack) -> ItemStack {
    if !stack.is_empty() {
        let limit = container.max_stack_size_of(&stack);
        if stack.count > limit {
            stack.count = limit;
        }
    }
    stack
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::{blocks, component_registries, items, jukebox_songs};

    use super::*;
    use crate::{
        BlockTicks, Effect, HopperEntity, InteractionResult, LegacyRandom,
        dispenser::dispense_from,
        hopper::query,
        state::FACING,
        testing::{TestLevel, actor_holding, top_hit, use_item},
        world::UPDATE_ALL,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn empty(count: usize) -> Vec<ItemStack> {
        vec![ItemStack::empty(); count]
    }

    fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
        query(world, pos, &mut |container| container.get(index)).expect("container")
    }

    fn place_hopper(level: &mut TestLevel, pos: BlockPos) {
        level.set(pos, BlockState(blocks::HOPPER));
    }

    fn tick_into(level: &mut TestLevel, pos: BlockPos) {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        world.set_block_entity(Box::new(HopperEntity::new(pos)));
        world.tick_block_entities();
    }

    #[test]
    fn song_outputs_follow_the_registry() {
        assert_eq!(
            jukebox_songs::SONGS.len(),
            component_registries::sizes::JUKEBOX_SONG as usize
        );
        assert_eq!(
            jukebox_comparator_output(&ItemStack::new(items::MUSIC_DISC_13, 1)),
            1
        );
        assert_eq!(
            jukebox_comparator_output(&ItemStack::new(items::MUSIC_DISC_CAT, 1)),
            2
        );
        assert_eq!(
            jukebox_comparator_output(&ItemStack::new(items::MUSIC_DISC_5, 1)),
            15
        );
        assert_eq!(
            jukebox_comparator_output(&ItemStack::new(items::STONE, 1)),
            0
        );
    }

    #[test]
    fn a_hopper_inserts_a_book_and_the_comparator_reads_that_slot() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::CHISELED_BOOKSHELF));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::BOOK, 1);
        level.set_container(POS, hopper);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::BOOK, 1));
        assert!(slot(&mut world, POS, 0).is_empty());
        let shelf = world.block_state(below);
        assert!(shelf.get(SLOT_0_OCCUPIED));
        assert!(!shelf.get(SLOT_1_OCCUPIED));
        assert_eq!(shelf.analog_output_signal(&world, below, Direction::Up), 1);
    }

    #[test]
    fn a_hopper_will_not_put_dirt_in_a_bookshelf() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::CHISELED_BOOKSHELF));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::DIRT, 4);
        level.set_container(POS, hopper);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 4));
        assert!(slot(&mut world, below, 0).is_empty());
        assert!(!world.block_state(below).get(SLOT_0_OCCUPIED));
    }

    #[test]
    fn a_hopper_pulls_the_book_it_previously_stored() {
        let above = POS.above();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set_container(POS, empty(5));
        let mut shelf = BlockState(blocks::CHISELED_BOOKSHELF);
        shelf = shelf.with(SLOT_2_OCCUPIED, true);
        level.set(above, shelf);
        let mut books = empty(6);
        books[2] = ItemStack::new(items::BOOK, 1);
        level.set_container(above, books);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::BOOK, 1));
        assert!(slot(&mut world, above, 2).is_empty());
        let shelf = world.block_state(above);
        assert!(!shelf.get(SLOT_2_OCCUPIED));
        assert_eq!(
            shelf.analog_output_signal(&world, above, Direction::Down),
            3
        );
    }

    #[test]
    fn a_full_hopper_leaves_the_book_where_it_is() {
        let above = POS.above();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set_container(POS, vec![ItemStack::new(items::STONE, 64); 5]);
        level.set(
            above,
            BlockState(blocks::CHISELED_BOOKSHELF).with(SLOT_0_OCCUPIED, true),
        );
        let mut books = empty(6);
        books[0] = ItemStack::new(items::BOOK, 1);
        level.set_container(above, books);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, above, 0), ItemStack::new(items::BOOK, 1));
        assert!(world.block_state(above).get(SLOT_0_OCCUPIED));
    }

    #[test]
    fn a_dropper_puts_one_book_into_the_shelf_it_faces() {
        let below = POS.below();
        let mut level = TestLevel::new();
        level.set(below, BlockState(blocks::CHISELED_BOOKSHELF));
        let mut dropper = empty(9);
        dropper[0] = ItemStack::new(items::BOOK, 2);
        level.set_container(POS, dropper);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.set_block(
            POS,
            BlockState(blocks::DROPPER).with(FACING, Direction::Down),
            UPDATE_ALL,
        );
        let state = world.block_state(POS);
        dispense_from(&mut world, POS, state);

        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::BOOK, 1));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::BOOK, 1));
        assert!(world.block_state(below).get(SLOT_0_OCCUPIED));
        assert_eq!(
            world
                .block_state(below)
                .analog_output_signal(&world, below, Direction::Up),
            1
        );
    }

    #[test]
    fn a_hopper_stores_the_disc_it_inserts_and_plays_it() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::JUKEBOX));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::MUSIC_DISC_CAT, 1);
        level.set_container(POS, hopper);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(
            slot(&mut world, below, 0),
            ItemStack::new(items::MUSIC_DISC_CAT, 1)
        );
        assert!(slot(&mut world, POS, 0).is_empty());
        let jukebox = world.block_state(below);
        assert!(jukebox.get(HAS_RECORD));
        assert!(world.redstone_memory().is_jukebox_playing(below));
        assert_eq!(jukebox.own_signal(&world, below), 15);
        assert_eq!(
            jukebox.analog_output_signal(&world, below, Direction::Up),
            2
        );
    }

    #[test]
    fn a_scene_jukebox_keeps_disc_13_without_playing() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::JUKEBOX).with(HAS_RECORD, true));
        level.set_container(POS, vec![ItemStack::new(items::MUSIC_DISC_13, 1)]);
        let mut ticks = BlockTicks::new();
        let world = World::new(&mut level, &mut ticks);
        let state = world.block_state(POS);
        assert!(!world.redstone_memory().is_jukebox_playing(POS));
        assert_eq!(state.own_signal(&world, POS), 0);
        assert_eq!(state.analog_output_signal(&world, POS, Direction::North), 1);
    }

    #[test]
    fn a_player_pops_the_same_disc_the_hopper_inserted() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::JUKEBOX));
        let actor = actor_holding(items::MUSIC_DISC_13);
        let inserted = use_item(&mut level, &actor, top_hit(POS));
        assert_eq!(inserted.result, InteractionResult::Success);
        assert!(level.get(POS).get(HAS_RECORD));

        let before = level.random.source().clone();
        let empty_hand = actor_holding(items::AIR);
        let popped = use_item(&mut level, &empty_hand, top_hit(POS));
        assert_eq!(popped.result, InteractionResult::Success);
        assert!(!level.get(POS).get(HAS_RECORD));
        assert!(
            level
                .containers
                .get(&POS)
                .is_none_or(|slots| slots[0].is_empty())
        );
        let expected = before;
        expected.next_float();
        expected.next_float();
        assert_eq!(level.random.source().seed(), expected.seed());
    }

    #[test]
    fn a_hopper_pulls_a_whole_jukebox_stack_in_one_transfer() {
        let above = POS.above();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set_container(POS, empty(5));
        level.set(above, BlockState(blocks::JUKEBOX).with(HAS_RECORD, true));
        // 场景直接写入的堆可以超过 `getMaxStackSize()`。`splitTheItem` 仍整堆拿走。
        //
        // A scene can write a stack larger than `getMaxStackSize()`. `splitTheItem` still takes
        // all of it.
        level.set_container(above, vec![ItemStack::new(items::IRON_INGOT, 64)]);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(
            slot(&mut world, POS, 0),
            ItemStack::new(items::IRON_INGOT, 64)
        );
        assert!(slot(&mut world, above, 0).is_empty());
        assert!(!world.block_state(above).get(HAS_RECORD));
    }

    #[test]
    fn a_hopper_pulls_the_inserted_disc_back_out() {
        let above = POS.above();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set_container(POS, empty(5));
        level.set(above, BlockState(blocks::JUKEBOX).with(HAS_RECORD, true));
        level.set_container(above, vec![ItemStack::new(items::MUSIC_DISC_13, 1)]);
        level.redstone_mut().set_jukebox_playing(above, true);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(
            slot(&mut world, POS, 0),
            ItemStack::new(items::MUSIC_DISC_13, 1)
        );
        assert!(slot(&mut world, above, 0).is_empty());
        assert!(!world.block_state(above).get(HAS_RECORD));
        assert!(!world.redstone_memory().is_jukebox_playing(above));
        assert_eq!(
            world
                .block_state(above)
                .analog_output_signal(&world, above, Direction::Down),
            0
        );
    }

    #[test]
    fn a_hopper_stacks_one_dirt_into_a_pot() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::DECORATED_POT));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::DIRT, 3);
        level.set_container(POS, hopper);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, below, 0), ItemStack::new(items::DIRT, 1));
        assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::DIRT, 2));
        assert_eq!(
            world
                .block_state(below)
                .analog_output_signal(&world, below, Direction::Up),
            1
        );
    }

    #[test]
    fn a_player_wobbles_a_pot_when_the_item_fits_and_when_it_does_not() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::DECORATED_POT));
        let dirt = actor_holding(items::DIRT);
        let inserted = use_item(&mut level, &dirt, top_hit(POS));
        assert_eq!(inserted.result, InteractionResult::Success);
        // 摇晃排进方块事件队列，要等这一刻的 `runBlockEvents`，不在点击的返回值里。
        //
        // The wobble is queued for this tick's `runBlockEvents`, not returned by the click.
        assert!(wobble(&mut level).is_some_and(|event| event == (1, 0)));

        let again = use_item(&mut level, &dirt, top_hit(POS));
        assert_eq!(again.result, InteractionResult::Success);
        assert_eq!(level.containers[&POS][0], ItemStack::new(items::DIRT, 2));
        assert!(wobble(&mut level).is_some_and(|event| event == (1, 0)));

        let stone = actor_holding(items::STONE);
        let rejected = use_item(&mut level, &stone, top_hit(POS));
        assert_eq!(rejected.result, InteractionResult::Success);
        assert!(rejected.used_without_item);
        assert!(wobble(&mut level).is_some_and(|event| event == (1, 1)));
        assert_eq!(level.containers[&POS][0].count, 2);
    }

    #[test]
    fn a_hopper_fills_the_first_empty_crafter_slot() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::CRAFTER));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::STONE, 4);
        level.set_container(POS, hopper);
        // 后面还有空格时，空格算 `smallerStackExist`，更小的那一堆收不了。物品进第一个空格。
        //
        // A later empty slot counts for `smallerStackExist`, so the smaller stack is skipped.
        // The item goes into the first empty slot.
        let mut crafter = empty(9);
        crafter[0] = ItemStack::new(items::STONE, 5);
        crafter[1] = ItemStack::new(items::STONE, 2);
        level.set_container(below, crafter);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, below, 0).count, 5);
        assert_eq!(slot(&mut world, below, 1).count, 2);
        assert_eq!(slot(&mut world, below, 2), ItemStack::new(items::STONE, 1));
        assert_eq!(
            world
                .block_state(below)
                .analog_output_signal(&world, below, Direction::Up),
            3
        );
    }

    #[test]
    fn a_hopper_adds_to_the_smaller_crafter_stack_when_nothing_later_is_smaller() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::CRAFTER));
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::STONE, 4);
        level.set_container(POS, hopper);
        let mut crafter = empty(9);
        crafter[0] = ItemStack::new(items::STONE, 5);
        crafter[1] = ItemStack::new(items::STONE, 2);
        for slot in &mut crafter[2..] {
            *slot = ItemStack::new(items::DIRT, 1);
        }
        level.set_container(below, crafter);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert_eq!(slot(&mut world, below, 0).count, 5);
        assert_eq!(slot(&mut world, below, 1).count, 3);
        assert_eq!(
            world
                .block_state(below)
                .analog_output_signal(&world, below, Direction::Up),
            9
        );
    }

    #[test]
    fn a_disabled_crafter_slot_is_skipped_and_still_counts_for_the_comparator() {
        let below = POS.below();
        let mut level = TestLevel::new();
        place_hopper(&mut level, POS);
        level.set(below, BlockState(blocks::CRAFTER));
        level.redstone_mut().set_crafter_disabled(below, 1);
        let mut hopper = empty(5);
        hopper[0] = ItemStack::new(items::DIRT, 1);
        level.set_container(POS, hopper);
        tick_into(&mut level, POS);

        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert!(slot(&mut world, below, 0).is_empty());
        assert_eq!(slot(&mut world, below, 1), ItemStack::new(items::DIRT, 1));
        assert_eq!(
            world
                .block_state(below)
                .analog_output_signal(&world, below, Direction::Up),
            2
        );
    }

    #[test]
    fn breaking_a_jukebox_takes_two_floats_only_when_it_holds_an_item() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::JUKEBOX).with(HAS_RECORD, true));
        level.set_container(POS, vec![ItemStack::new(items::MUSIC_DISC_13, 1)]);
        level.redstone_mut().set_jukebox_playing(POS, true);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let expected = world.random().clone();
        expected.next_float();
        expected.next_float();
        world.set_block(POS, BlockState::AIR, UPDATE_ALL);
        assert_eq!(world.random().seed(), expected.seed());

        let mut empty_level = TestLevel::new();
        empty_level.set(POS, BlockState(blocks::JUKEBOX));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut empty_level, &mut ticks);
        let expected = world.random().clone();
        world.set_block(POS, BlockState::AIR, UPDATE_ALL);
        assert_eq!(world.random().seed(), expected.seed());
    }

    #[test]
    fn breaking_a_bookshelf_drops_every_slot_including_the_empty_ones() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::CHISELED_BOOKSHELF));
        let mut books = empty(6);
        books[0] = ItemStack::new(items::BOOK, 1);
        level.set_container(POS, books);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let expected = world.random().clone();
        consume_stack(&expected, 1);
        for _ in 0..5 {
            consume_empty(&expected);
        }
        world.set_block(POS, BlockState::AIR, UPDATE_ALL);
        assert_eq!(world.random().seed(), expected.seed());
    }

    fn consume_empty(random: &LegacyRandom) {
        for _ in 0..3 {
            random.next_double();
        }
    }

    fn consume_stack(random: &LegacyRandom, count: i32) {
        consume_empty(random);
        let mut left = count;
        while left > 0 {
            let split = (random.next_int(21) + 10).min(left);
            left -= split;
            for _ in 0..6 {
                random.next_double();
            }
        }
    }

    fn wobble(level: &mut TestLevel) -> Option<(i32, i32)> {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        world.run_block_events();
        world
            .into_effects()
            .into_iter()
            .find_map(|effect| match effect {
                Effect::BlockEvent {
                    param_a, param_b, ..
                } => Some((param_a, param_b)),
                _ => None,
            })
    }
}
