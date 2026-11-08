//! 工作台菜单：玩家 2×2、工作台、切石机、锻造台、熔炉系列、酿造台。
//!
//! 点击顺序照官方 `AbstractContainerMenu.doClick`。每种菜单的 `quickMoveStack`
//! 各自翻译，不共用。`slotsChanged` 查配方时不带提示：官方 `CraftingMenu.slotsChanged`
//! 传的就是 `null`，提示只给 `place_recipe` 用，而那条不在这次的范围里。
//!
//! Workstation menus: the player 2×2, crafting table, stonecutter, smithing table,
//! furnaces, and brewing stand.
//!
//! Clicks follow `AbstractContainerMenu.doClick`. Each menu has its own `quickMoveStack`.
//! Recipe lookup from `slotsChanged` passes no hint: vanilla `CraftingMenu.slotsChanged`
//! passes `null`. The hint is only for `place_recipe`, which is out of scope here.

use mc_bamboo_blocks::{
    BlockState, World,
    behaviour::machines::{BrewingStandEntity, FurnaceEntity},
};
use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{CookingKind, Recipe, RecipeType, blocks, items, registries::menu};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        ContainerInput, ContainerSetContent, ContainerSetData, ContainerSetSlot, GameMode,
        HashedStack, OpenScreen, SetCursorItem,
    },
    text::Component,
};
use mc_bamboo_recipes::{
    CraftingInput, RecipeManager, SmithingInput, assemble, equipment_slot_for_item,
    is_brewing_fuel, is_brewing_potion_input, is_brewing_reagent, is_cooking_fuel,
    is_cooking_input, is_equippable_in_slot, is_smithing_addition, is_smithing_base,
    is_smithing_template, remaining_items, stonecutter_recipes,
};
use tracing::debug;

use super::{ClickPacket, ContainerKind, Menu, RemoteSlot, Target};
use crate::{
    container::CONTAINER_BUFFER, inbound::PlayerId, interaction::BLOCK_INTERACTION_RANGE,
    inventory::Inventory, player::Player, region::Region,
};

const STATE_ID_MASK: i32 = 32767;
const CONTAINER_STACK_LIMIT: i32 = 99;
/// 玩家物品栏菜单的槽位数，和 `Inventory.slots` 一样长。
const INVENTORY_SLOTS: usize = 46;
/// 装备槽的网络 ID。脚、腿、胸、头、副手。
const SLOT_FEET: i32 = 1;
const SLOT_OFFHAND: i32 = 5;

/// 没有方块菜单时，2×2 合成的光标和远程槽。
///
/// Cursor and remote slots for the 2×2 grid while no block menu is open.
pub(crate) struct InventoryCursor {
    carried: ItemStack,
    remote_slots: Vec<RemoteSlot>,
    remote_carried: RemoteSlot,
    quickcraft_status: i32,
    quickcraft_type: i32,
    quickcraft_slots: Vec<usize>,
}

impl InventoryCursor {
    pub(crate) fn new() -> Self {
        Self {
            carried: ItemStack::empty(),
            remote_slots: vec![RemoteSlot::synchronized(); INVENTORY_SLOTS],
            remote_carried: RemoteSlot::synchronized(),
            quickcraft_status: 0,
            quickcraft_type: 0,
            quickcraft_slots: Vec::new(),
        }
    }

    fn force_slot(&mut self, slot: usize, stack: &ItemStack) {
        if let Some(remote) = self.remote_slots.get_mut(slot) {
            remote.force(stack);
        }
    }
}

/// `send_slot` 已经把这一格发给客户端了，远程槽记成一致。
///
/// `send_slot` already told the client. Mark the remote slot as matching.
pub(crate) fn note_inventory_slot(player: &mut Player, slot: usize) {
    if let Some(stack) = player.inventory.slots.get(slot) {
        let stack = stack.clone();
        player.inventory_cursor.force_slot(slot, &stack);
    }
}

/// 打开的是哪一种菜单。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Inventory,
    Crafting,
    Stonecutter,
    Smithing,
    Furnace,
    BlastFurnace,
    Smoker,
    Brewing,
}

impl Kind {
    fn from_block(state: u32) -> Option<Self> {
        let id = BlockState(state).block_id();
        let is = |block: u32| BlockState(block).block_id() == id;
        if is(blocks::CRAFTING_TABLE) {
            Some(Self::Crafting)
        } else if is(blocks::STONECUTTER) {
            Some(Self::Stonecutter)
        } else if is(blocks::SMITHING_TABLE) {
            Some(Self::Smithing)
        } else if is(blocks::FURNACE) {
            Some(Self::Furnace)
        } else if is(blocks::BLAST_FURNACE) {
            Some(Self::BlastFurnace)
        } else if is(blocks::SMOKER) {
            Some(Self::Smoker)
        } else if is(blocks::BREWING_STAND) {
            Some(Self::Brewing)
        } else {
            None
        }
    }

    fn slot_count(self) -> usize {
        match self {
            Self::Inventory | Self::Crafting => 46,
            Self::Stonecutter => 38,
            Self::Smithing => 40,
            Self::Furnace | Self::BlastFurnace | Self::Smoker => 39,
            Self::Brewing => 41,
        }
    }

    fn data_len(self) -> usize {
        match self {
            Self::Stonecutter | Self::Smithing => 1,
            Self::Furnace | Self::BlastFurnace | Self::Smoker | Self::Brewing => 4,
            _ => 0,
        }
    }

    /// 玩家背包第一格在菜单里的下标。
    fn player_start(self) -> usize {
        match self {
            Self::Inventory => 9,
            Self::Crafting => 10,
            Self::Stonecutter => 2,
            Self::Smithing => 4,
            Self::Furnace | Self::BlastFurnace | Self::Smoker => 3,
            Self::Brewing => 5,
        }
    }

    fn machine_len(self) -> usize {
        match self {
            Self::Furnace | Self::BlastFurnace | Self::Smoker => 3,
            Self::Brewing => 5,
            _ => 0,
        }
    }

    fn cooking(self) -> Option<CookingKind> {
        match self {
            Self::Furnace => Some(CookingKind::Smelting),
            Self::BlastFurnace => Some(CookingKind::Blasting),
            Self::Smoker => Some(CookingKind::Smoking),
            _ => None,
        }
    }

    fn is_furnace(self) -> bool {
        self.cooking().is_some()
    }

    fn menu_type(self) -> i32 {
        match self {
            Self::Crafting | Self::Inventory => menu::CRAFTING,
            Self::Furnace => menu::FURNACE,
            Self::BlastFurnace => menu::BLAST_FURNACE,
            Self::Smoker => menu::SMOKER,
            Self::Stonecutter => menu::STONECUTTER,
            Self::Smithing => menu::SMITHING,
            Self::Brewing => menu::BREWING_STAND,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Crafting | Self::Inventory => "container.crafting",
            Self::Furnace => "container.furnace",
            Self::BlastFurnace => "container.blast_furnace",
            Self::Smoker => "container.smoker",
            Self::Stonecutter => "container.stonecutter",
            Self::Smithing => "container.upgrade",
            Self::Brewing => "container.brewing",
        }
    }
}

enum Outgoing {
    Content {
        state_id: i32,
        items: Vec<ItemStack>,
        carried: ItemStack,
    },
    Slot {
        state_id: i32,
        slot: i16,
        stack: ItemStack,
    },
    Data {
        id: i16,
        value: i16,
    },
    Carried(ItemStack),
}

enum Role {
    Craft,
    CraftResult,
    StoneInput,
    StoneResult,
    SmithInput,
    SmithResult,
    Machine,
    Armor(i32),
    Player,
}

enum MoveResult {
    /// 官方在清理格子之前 `return EMPTY`。`write_back` 时来源格的数量已经变了。
    ///
    /// Vanilla `return EMPTY` before the slot cleanup. `write_back` means the source count changed.
    Stop {
        write_back: bool,
    },
    Moved {
        crafted: bool,
    },
}

/// 一个工作台菜单的槽位、光标、数据槽和远程槽。
pub(crate) struct Station {
    kind: Kind,
    pos: BlockPos,
    container_id: i32,
    slots: Vec<ItemStack>,
    /// 菜单里看不见的盔甲和副手。顺序是脚、腿、胸、头、副手。
    ///
    /// Armor and offhand hidden from this menu, in order: feet, legs, chest, head, offhand.
    hidden: [ItemStack; 5],
    carried: ItemStack,
    state_id: i32,
    remote_slots: Vec<RemoteSlot>,
    remote_carried: RemoteSlot,
    data: Vec<i32>,
    remote_data: Vec<i32>,
    quickcraft_status: i32,
    quickcraft_type: i32,
    quickcraft_slots: Vec<usize>,
    suppress: bool,
    creative: bool,
    selected: u8,
    /// 切石机上次看到的输入物品。只比物品种类，不比数量。
    ///
    /// Last stonecutter input item. Compared by item id, not count.
    stone_input: ItemStack,
    stone_recipes: Vec<&'static Recipe>,
    /// 这一格走过 `setItem`。熔炉原料格会因此重算时间。
    ///
    /// This slot went through `setItem`. The furnace input slot recomputes cook time.
    machine_set: [bool; 5],
    /// 这一格只改了数量，不走 `setItem`。
    ///
    /// This slot only changed count, not through `setItem`.
    machine_content: [bool; 5],
    /// 这次点击要调用几次 `award_experience`。Shift 取出结果是两次。
    ///
    /// How many times this click calls `award_experience`. Shift-clicking the result is twice.
    furnace_awards: i32,
    packets: Vec<Outgoing>,
}

impl Station {
    fn create(kind: Kind, container_id: i32, pos: BlockPos) -> Self {
        let len = kind.slot_count();
        Self {
            kind,
            pos,
            container_id,
            slots: vec![ItemStack::empty(); len],
            hidden: std::array::from_fn(|_| ItemStack::empty()),
            carried: ItemStack::empty(),
            state_id: 0,
            remote_slots: vec![RemoteSlot::synchronized(); len],
            remote_carried: RemoteSlot::synchronized(),
            data: vec![0; kind.data_len()],
            remote_data: vec![0; kind.data_len()],
            quickcraft_status: 0,
            quickcraft_type: 0,
            quickcraft_slots: Vec::new(),
            suppress: false,
            creative: false,
            selected: 0,
            stone_input: ItemStack::empty(),
            stone_recipes: Vec::new(),
            machine_set: [false; 5],
            machine_content: [false; 5],
            furnace_awards: 0,
            packets: Vec::new(),
        }
    }

    fn from_player(player: &Player) -> Self {
        let mut station = Self::create(Kind::Inventory, 0, BlockPos::new(0, 0, 0));
        station.creative = player.game_mode == GameMode::Creative;
        station.load_player(&player.inventory.slots, player.inventory.selected);
        station.state_id = player.inventory.state_id();
        station.carried = player.inventory_cursor.carried.clone();
        station
            .remote_slots
            .clone_from(&player.inventory_cursor.remote_slots);
        station.remote_carried = player.inventory_cursor.remote_carried.clone();
        station.quickcraft_status = player.inventory_cursor.quickcraft_status;
        station.quickcraft_type = player.inventory_cursor.quickcraft_type;
        station
            .quickcraft_slots
            .clone_from(&player.inventory_cursor.quickcraft_slots);
        station
    }

    fn store_cursor(&self, cursor: &mut InventoryCursor) {
        cursor.carried = self.carried.clone();
        cursor.remote_slots.clone_from(&self.remote_slots);
        cursor.remote_carried = self.remote_carried.clone();
        cursor.quickcraft_status = self.quickcraft_status;
        cursor.quickcraft_type = self.quickcraft_type;
        cursor.quickcraft_slots.clone_from(&self.quickcraft_slots);
    }

    fn load_player(&mut self, slots: &[ItemStack], selected: u8) {
        self.selected = selected;
        if self.kind == Kind::Inventory {
            self.slots.clone_from_slice(slots);
            return;
        }
        let start = self.kind.player_start();
        self.slots[start..start + 27].clone_from_slice(&slots[9..36]);
        self.slots[start + 27..start + 36].clone_from_slice(&slots[36..45]);
        self.hidden = [
            slots[8].clone(),
            slots[7].clone(),
            slots[6].clone(),
            slots[5].clone(),
            slots[45].clone(),
        ];
    }

    fn write_player(&self, inventory: &mut Inventory) {
        if self.kind == Kind::Inventory {
            inventory.slots.clone_from_slice(&self.slots);
            return;
        }
        let start = self.kind.player_start();
        inventory.slots[9..36].clone_from_slice(&self.slots[start..start + 27]);
        inventory.slots[36..45].clone_from_slice(&self.slots[start + 27..start + 36]);
        inventory.slots[8] = self.hidden[0].clone();
        inventory.slots[7] = self.hidden[1].clone();
        inventory.slots[6] = self.hidden[2].clone();
        inventory.slots[5] = self.hidden[3].clone();
        inventory.slots[45] = self.hidden[4].clone();
    }

    pub(crate) fn still_valid(&self, region: &Region, id: PlayerId) -> bool {
        let Some(player) = region.players.get(&id) else {
            return false;
        };
        let Some(state) = region.chunks.block(self.pos) else {
            return false;
        };
        if Kind::from_block(state) != Some(self.kind) {
            return false;
        }
        let eye = Vec3::new(
            player.position().x,
            player.position().y + player.eye_height(),
            player.position().z,
        );
        let range = BLOCK_INTERACTION_RANGE + CONTAINER_BUFFER;
        let x = f64::from(self.pos.x);
        let y = f64::from(self.pos.y);
        let z = f64::from(self.pos.z);
        Aabb::new(x, y, z, x + 1.0, y + 1.0, z + 1.0).distance_to_sqr(eye) < range * range
    }

    fn valid_slot(&self, slot: i16) -> bool {
        let slot = i32::from(slot);
        slot == -1 || slot == -999 || slot < self.slots.len() as i32
    }

    fn bump(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & STATE_ID_MASK;
        self.state_id
    }

    fn role(&self, index: usize) -> Role {
        match self.kind {
            Kind::Inventory => match index {
                0 => Role::CraftResult,
                1..=4 => Role::Craft,
                5 => Role::Armor(4),
                6 => Role::Armor(3),
                7 => Role::Armor(2),
                8 => Role::Armor(1),
                _ => Role::Player,
            },
            Kind::Crafting => match index {
                0 => Role::CraftResult,
                1..=9 => Role::Craft,
                _ => Role::Player,
            },
            Kind::Stonecutter => match index {
                0 => Role::StoneInput,
                1 => Role::StoneResult,
                _ => Role::Player,
            },
            Kind::Smithing => match index {
                0..=2 => Role::SmithInput,
                3 => Role::SmithResult,
                _ => Role::Player,
            },
            Kind::Furnace | Kind::BlastFurnace | Kind::Smoker | Kind::Brewing => {
                if index < self.kind.machine_len() {
                    Role::Machine
                } else {
                    Role::Player
                }
            }
        }
    }

    fn takes_all(&self, index: usize) -> bool {
        matches!(
            self.role(index),
            Role::CraftResult | Role::StoneResult | Role::SmithResult
        )
    }

    fn may_pickup(&self, _index: usize) -> bool {
        // 绑定诅咒（`PREVENT_ARMOR_CHANGE`）还没做，盔甲一律能拿下来。
        // Binding curse (`PREVENT_ARMOR_CHANGE`) is not implemented, so armor can always be taken.
        true
    }

    fn may_place(&self, index: usize, stack: &ItemStack) -> bool {
        match self.role(index) {
            Role::CraftResult | Role::StoneResult | Role::SmithResult => false,
            Role::Armor(slot) => is_equippable_in_slot(stack, slot),
            Role::SmithInput => match index {
                0 => is_smithing_template(stack.item),
                1 => is_smithing_base(stack.item),
                _ => is_smithing_addition(stack.item),
            },
            Role::Machine => self.machine_may_place(index, stack),
            Role::Craft | Role::StoneInput | Role::Player => true,
        }
    }

    fn machine_may_place(&self, index: usize, stack: &ItemStack) -> bool {
        if self.kind.is_furnace() {
            return match index {
                2 => false,
                1 => is_cooking_fuel(stack) || stack.item == items::BUCKET,
                _ => true,
            };
        }
        match index {
            0..=2 => is_brewing_potion_input(stack.item),
            3 => is_brewing_reagent(stack.item),
            4 => is_brewing_fuel(stack),
            _ => true,
        }
    }

    fn slot_max(&self, index: usize, stack: &ItemStack) -> i32 {
        let bucket_fuel = self.kind.is_furnace()
            && index == 1
            && !stack.is_empty()
            && stack.item == items::BUCKET;
        let potion = self.kind == Kind::Brewing && index < 3;
        let armor = matches!(self.role(index), Role::Armor(_));
        let cap = if bucket_fuel || potion || armor {
            1
        } else {
            CONTAINER_STACK_LIMIT
        };
        cap.min(stack.max_stack_size())
    }

    fn limits_on_set(&self, index: usize) -> bool {
        matches!(
            self.role(index),
            Role::StoneInput | Role::SmithInput | Role::Machine
        )
    }

    fn allow_modification(&self, index: usize) -> bool {
        self.may_pickup(index) && self.may_place(index, &self.slots[index])
    }

    fn store(&mut self, index: usize, mut stack: ItemStack) {
        if stack.is_empty() {
            self.slots[index] = ItemStack::empty();
            return;
        }
        if self.limits_on_set(index) {
            let limit = CONTAINER_STACK_LIMIT.min(stack.max_stack_size());
            if stack.count > limit {
                stack.count = limit;
            }
        }
        self.slots[index] = stack;
    }

    fn set_by_player(&mut self, index: usize, stack: ItemStack) {
        self.store(index, stack);
        self.on_set_item(index);
        self.on_set_changed(index);
    }

    fn write_changed(&mut self, index: usize, stack: ItemStack) {
        self.store(index, stack);
        self.on_set_changed(index);
    }

    fn on_set_item(&mut self, index: usize) {
        match self.role(index) {
            Role::Craft => self.refresh_craft(),
            Role::StoneInput => self.stone_changed(),
            Role::SmithInput => self.refresh_smithing(),
            Role::Machine => self.mark_set(index),
            _ => {}
        }
    }

    fn on_set_changed(&mut self, index: usize) {
        match self.role(index) {
            Role::StoneInput => self.stone_changed(),
            Role::SmithInput => self.refresh_smithing(),
            Role::Machine => self.mark_content(index),
            _ => {}
        }
    }

    fn mark_set(&mut self, index: usize) {
        if index < self.machine_set.len() {
            self.machine_set[index] = true;
        }
    }

    fn mark_content(&mut self, index: usize) {
        if index < self.machine_content.len() {
            self.machine_content[index] = true;
        }
    }

    fn is_craft_grid(&self, index: usize) -> bool {
        matches!(
            (self.kind, index),
            (Kind::Inventory, 1..=4) | (Kind::Crafting, 1..=9)
        )
    }

    fn remove_amount(&mut self, index: usize, amount: i32) -> ItemStack {
        if self.takes_all(index) {
            return std::mem::take(&mut self.slots[index]);
        }
        let taken = split(&mut self.slots[index], amount);
        if taken.is_empty() {
            return taken;
        }
        if self.is_craft_grid(index) {
            self.refresh_craft();
        } else {
            self.on_set_changed(index);
        }
        taken
    }

    fn try_remove(&mut self, index: usize, mut amount: i32, max_amount: i32) -> Option<ItemStack> {
        let current = self.slots[index].clone();
        if current.is_empty() || !self.may_pickup(index) {
            return None;
        }
        if !self.allow_modification(index) && max_amount < current.count {
            return None;
        }
        amount = amount.min(max_amount);
        if !self.takes_all(index) && amount <= 0 {
            return None;
        }
        let taken = self.remove_amount(index, amount);
        if taken.is_empty() {
            return None;
        }
        if self.slots[index].is_empty() {
            self.set_by_player(index, ItemStack::empty());
        }
        Some(taken)
    }

    fn safe_take(&mut self, index: usize, amount: i32, max_amount: i32) -> ItemStack {
        match self.try_remove(index, amount, max_amount) {
            Some(taken) => {
                self.on_take(index, &taken);
                taken
            }
            None => ItemStack::empty(),
        }
    }

    fn safe_insert(&mut self, index: usize, mut input: ItemStack, amount: i32) -> ItemStack {
        if input.is_empty() || !self.may_place(index, &input) {
            return input;
        }
        let slot_stack = self.slots[index].clone();
        let room = self.slot_max(index, &input) - slot_stack.count;
        let transferable = amount.min(input.count).min(room);
        if transferable <= 0 {
            return input;
        }
        if slot_stack.is_empty() {
            let placed = split(&mut input, transferable);
            self.set_by_player(index, placed);
        } else if slot_stack.is_same_item_same_components(&input) {
            input.count -= transferable;
            if input.count <= 0 {
                input = ItemStack::empty();
            }
            let mut grown = slot_stack;
            grown.count += transferable;
            self.set_by_player(index, grown);
        }
        input
    }

    fn click(&mut self, slot: i32, button: i32, input: ContainerInput) {
        self.suppress = true;
        self.do_click(slot, button, input);
        self.suppress = false;
    }

    fn do_click(&mut self, slot_index: i32, button: i32, input: ContainerInput) {
        if input == ContainerInput::QuickCraft {
            self.quick_craft(slot_index, button);
        } else if self.quickcraft_status != 0 {
            self.reset_quick_craft();
        } else if matches!(input, ContainerInput::Pickup | ContainerInput::QuickMove)
            && (button == 0 || button == 1)
        {
            self.pickup_or_quick_move(slot_index, button, input == ContainerInput::QuickMove);
        } else if input == ContainerInput::Swap && ((0..9).contains(&button) || button == 40) {
            self.swap_with(slot_index, button);
        } else if input == ContainerInput::Clone
            && self.creative
            && self.carried.is_empty()
            && slot_index >= 0
        {
            if let Some(index) = self.positive(slot_index) {
                self.clone_slot(index);
            }
        } else if input == ContainerInput::Throw && self.carried.is_empty() && slot_index >= 0 {
            if let Some(index) = self.positive(slot_index) {
                self.throw_slot(index, button);
            }
        } else if input == ContainerInput::PickupAll && slot_index >= 0 {
            self.pickup_all(slot_index, button);
        }
    }

    fn positive(&self, slot: i32) -> Option<usize> {
        usize::try_from(slot)
            .ok()
            .filter(|&index| index < self.slots.len())
    }

    fn pickup_or_quick_move(&mut self, slot_index: i32, button: i32, quick: bool) {
        if slot_index == -999 {
            if !self.carried.is_empty() {
                if button == 0 {
                    let carried = std::mem::take(&mut self.carried);
                    self.discard(carried);
                } else {
                    let dropped = split(&mut self.carried, 1);
                    self.discard(dropped);
                }
            }
            return;
        }
        let Some(index) = self.positive(slot_index) else {
            return;
        };
        if quick {
            if !self.may_pickup(index) {
                return;
            }
            let mut clicked = self.quick_move(index);
            while !clicked.is_empty() && self.slots[index].is_same_item(&clicked) {
                clicked = self.quick_move(index);
            }
        } else {
            self.pickup(index, button == 0);
        }
    }

    fn pickup(&mut self, index: usize, primary: bool) {
        let clicked = self.slots[index].clone();
        let carried = self.carried.clone();
        if clicked.is_empty() {
            if !carried.is_empty() {
                let amount = if primary { carried.count } else { 1 };
                self.carried = self.safe_insert(index, carried, amount);
            }
        } else if self.may_pickup(index) {
            if carried.is_empty() {
                let amount = if primary {
                    clicked.count
                } else {
                    (clicked.count + 1) / 2
                };
                if let Some(taken) = self.try_remove(index, amount, i32::MAX) {
                    self.carried = taken.clone();
                    self.on_take(index, &taken);
                }
            } else if self.may_place(index, &carried) {
                if clicked.is_same_item_same_components(&carried) {
                    let amount = if primary { carried.count } else { 1 };
                    self.carried = self.safe_insert(index, carried, amount);
                } else if carried.count <= self.slot_max(index, &carried) {
                    self.carried = clicked;
                    self.set_by_player(index, carried);
                }
            } else if clicked.is_same_item_same_components(&carried) {
                let room = carried.max_stack_size() - carried.count;
                if let Some(taken) = self.try_remove(index, clicked.count, room) {
                    self.carried.count += taken.count;
                    self.on_take(index, &taken);
                }
            }
        }
        self.on_set_changed(index);
    }

    fn swap_with(&mut self, slot_index: i32, button: i32) {
        let Some(index) = self.positive(slot_index) else {
            return;
        };
        let official = button as usize;
        let mut source = self.official_get(official);
        let target = self.slots[index].clone();
        if source.is_empty() && target.is_empty() {
            return;
        }
        if source.is_empty() {
            if self.may_pickup(index) {
                self.official_set(official, target.clone());
                self.set_by_player(index, ItemStack::empty());
                self.on_take(index, &target);
            }
        } else if target.is_empty() {
            if self.may_place(index, &source) {
                let max = self.slot_max(index, &source);
                if source.count > max {
                    let placed = split(&mut source, max);
                    self.official_set(official, source);
                    self.set_by_player(index, placed);
                } else {
                    self.official_set(official, ItemStack::empty());
                    self.set_by_player(index, source);
                }
            }
        } else if self.may_pickup(index) && self.may_place(index, &source) {
            let max = self.slot_max(index, &source);
            if source.count > max {
                let placed = split(&mut source, max);
                self.official_set(official, source);
                self.set_by_player(index, placed);
                self.on_take(index, &target);
                let mut leftover = target;
                if !self.inventory_add(&mut leftover) {
                    self.discard(leftover);
                }
            } else {
                self.official_set(official, target.clone());
                self.set_by_player(index, source);
                self.on_take(index, &target);
            }
        }
    }

    fn clone_slot(&mut self, index: usize) {
        let current = &self.slots[index];
        if current.is_empty() {
            return;
        }
        self.carried = copy_count(current, current.max_stack_size());
    }

    fn throw_slot(&mut self, index: usize, button: i32) {
        let amount = if button == 0 {
            1
        } else {
            self.slots[index].count
        };
        let mut item = self.safe_take(index, amount, i32::MAX);
        self.discard(item.clone());
        if button == 1 {
            while !item.is_empty() && self.slots[index].is_same_item(&item) {
                item = self.safe_take(index, amount, i32::MAX);
                self.discard(item.clone());
            }
        }
    }

    fn pickup_all(&mut self, slot_index: i32, button: i32) {
        let Some(index) = self.positive(slot_index) else {
            return;
        };
        if self.carried.is_empty() || !self.slots[index].is_empty() {
            return;
        }
        let start: i64 = if button == 0 {
            0
        } else {
            self.slots.len() as i64 - 1
        };
        let step: i64 = if button == 0 { 1 } else { -1 };
        for pass in 0..2 {
            let mut cursor = start;
            while cursor >= 0
                && (cursor as usize) < self.slots.len()
                && self.carried.count < self.carried.max_stack_size()
            {
                let slot = cursor as usize;
                let item = self.slots[slot].clone();
                if !item.is_empty()
                    && self.can_quick_replace(slot, &self.carried, true)
                    && self.may_pickup(slot)
                    && !self.takes_all(slot)
                    && (pass != 0 || item.count != item.max_stack_size())
                {
                    let room = self.carried.max_stack_size() - self.carried.count;
                    let removed = self.safe_take(slot, item.count, room);
                    self.carried.count += removed.count;
                }
                cursor += step;
            }
        }
    }

    fn can_quick_replace(&self, index: usize, stack: &ItemStack, ignore_size: bool) -> bool {
        let target = &self.slots[index];
        if target.is_empty() {
            return true;
        }
        stack.is_same_item_same_components(target)
            && target.count + if ignore_size { 0 } else { stack.count } <= stack.max_stack_size()
    }

    fn reset_quick_craft(&mut self) {
        self.quickcraft_status = 0;
        self.quickcraft_slots.clear();
    }

    fn quick_craft(&mut self, slot_index: i32, button: i32) {
        let expected = self.quickcraft_status;
        let status = button & 3;
        self.quickcraft_status = status;
        if (expected != 1 || status != 2) && expected != status || self.carried.is_empty() {
            self.reset_quick_craft();
        } else if status == 0 {
            let kind = (button >> 2) & 3;
            self.quickcraft_type = kind;
            if kind == 0 || kind == 1 || (kind == 2 && self.creative) {
                self.quickcraft_status = 1;
                self.quickcraft_slots.clear();
            } else {
                self.reset_quick_craft();
            }
        } else if status == 1 {
            let Some(index) = self.positive(slot_index) else {
                return;
            };
            let carried = self.carried.clone();
            if self.can_quick_replace(index, &carried, true)
                && self.may_place(index, &carried)
                && (self.quickcraft_type == 2 || carried.count > self.quickcraft_slots.len() as i32)
                && !self.quickcraft_slots.contains(&index)
            {
                self.quickcraft_slots.push(index);
            }
        } else if status == 2 {
            self.finish_quick_craft();
        } else {
            self.reset_quick_craft();
        }
    }

    fn finish_quick_craft(&mut self) {
        if self.quickcraft_slots.is_empty() {
            self.reset_quick_craft();
            return;
        }
        if self.quickcraft_slots.len() == 1 {
            let index = self.quickcraft_slots[0];
            let button = self.quickcraft_type;
            self.reset_quick_craft();
            self.do_click(index as i32, button, ContainerInput::Pickup);
            return;
        }
        let mut source = self.carried.clone();
        if source.is_empty() {
            self.reset_quick_craft();
            return;
        }
        let size = self.quickcraft_slots.len();
        let mut remaining = self.carried.count;
        let slots = self.quickcraft_slots.clone();
        for index in slots {
            let carried = self.carried.clone();
            if self.can_quick_replace(index, &carried, true)
                && self.may_place(index, &carried)
                && (self.quickcraft_type == 2 || carried.count >= size as i32)
            {
                let carry = if self.slots[index].is_empty() {
                    0
                } else {
                    self.slots[index].count
                };
                let max_size = source.max_stack_size().min(self.slot_max(index, &source));
                let new_count =
                    (place_count(size, self.quickcraft_type, &source) + carry).min(max_size);
                remaining -= new_count - carry;
                self.set_by_player(index, copy_count(&source, new_count));
            }
        }
        source.count = remaining;
        self.carried = normalize(source);
        self.reset_quick_craft();
    }

    fn quick_move(&mut self, index: usize) -> ItemStack {
        let clicked = self.slots[index].clone();
        if clicked.is_empty() {
            return ItemStack::empty();
        }
        let mut stack = clicked.clone();
        let crafted = match self.route(index, &mut stack, &clicked) {
            MoveResult::Stop { write_back } => {
                if write_back {
                    self.slots[index] = normalize(stack);
                }
                return ItemStack::empty();
            }
            MoveResult::Moved { crafted } => crafted,
        };
        if crafted {
            self.on_quick_craft(index, &stack, clicked.count);
        }
        if stack.is_empty() {
            self.set_by_player(index, ItemStack::empty());
            if self.kind == Kind::Stonecutter {
                self.on_set_changed(index);
            }
        } else {
            self.write_changed(index, stack.clone());
        }
        if stack.count == clicked.count {
            return ItemStack::empty();
        }
        let taken = if self.kind == Kind::Brewing {
            clicked.clone()
        } else {
            stack.clone()
        };
        self.on_take(index, &taken);
        if self.drops_remainder(index) {
            self.discard(stack);
        }
        if self.kind == Kind::Stonecutter {
            self.broadcast();
        }
        clicked
    }

    fn drops_remainder(&self, index: usize) -> bool {
        matches!(
            (self.kind, index),
            (Kind::Inventory | Kind::Crafting, 0) | (Kind::Stonecutter, 1)
        )
    }

    fn route(&mut self, index: usize, stack: &mut ItemStack, clicked: &ItemStack) -> MoveResult {
        match self.kind {
            Kind::Inventory => self.route_inventory(index, stack, clicked),
            Kind::Crafting => self.route_crafting(index, stack),
            Kind::Stonecutter => self.route_stone(index, stack),
            Kind::Smithing => self.route_smithing(index, stack),
            Kind::Furnace | Kind::BlastFurnace | Kind::Smoker => self.route_furnace(index, stack),
            Kind::Brewing => self.route_brewing(index, stack, clicked),
        }
    }

    fn route_inventory(
        &mut self,
        index: usize,
        stack: &mut ItemStack,
        clicked: &ItemStack,
    ) -> MoveResult {
        let eq = equipment_slot_for_item(clicked);
        if index == 0 {
            return Self::moved_or_stop(self.move_items(stack, 9, 45, true), true);
        }
        if (1..9).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 9, 45, false), false);
        }
        if (SLOT_FEET..=4).contains(&eq) && self.slots[(9 - eq) as usize].is_empty() {
            let pos = (9 - eq) as usize;
            return Self::moved_or_stop(self.move_items(stack, pos, pos + 1, false), false);
        }
        if eq == SLOT_OFFHAND && self.slots[45].is_empty() {
            return Self::moved_or_stop(self.move_items(stack, 45, 46, false), false);
        }
        if (9..36).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 36, 45, false), false);
        }
        if (36..45).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 9, 36, false), false);
        }
        Self::moved_or_stop(self.move_items(stack, 9, 45, false), false)
    }

    fn route_crafting(&mut self, index: usize, stack: &mut ItemStack) -> MoveResult {
        if index == 0 {
            return Self::moved_or_stop(self.move_items(stack, 10, 46, true), true);
        }
        if (10..46).contains(&index) {
            if self.move_items(stack, 1, 10, false) {
                return MoveResult::Moved { crafted: false };
            }
            if index < 37 {
                return Self::moved_or_stop(self.move_items(stack, 37, 46, false), false);
            }
            return Self::moved_or_stop(self.move_items(stack, 10, 37, false), false);
        }
        Self::moved_or_stop(self.move_items(stack, 10, 46, false), false)
    }

    fn route_stone(&mut self, index: usize, stack: &mut ItemStack) -> MoveResult {
        if index == 1 {
            return Self::moved_or_stop(self.move_items(stack, 2, 38, true), true);
        }
        if index == 0 {
            return Self::moved_or_stop(self.move_items(stack, 2, 38, false), false);
        }
        if !stack.is_empty() && accepts_input(stack) {
            return Self::moved_or_stop(self.move_items(stack, 0, 1, false), false);
        }
        if (2..29).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 29, 38, false), false);
        }
        if (29..38).contains(&index) && !self.move_items(stack, 2, 29, false) {
            return MoveResult::Stop { write_back: false };
        }
        MoveResult::Moved { crafted: false }
    }

    fn route_smithing(&mut self, index: usize, stack: &mut ItemStack) -> MoveResult {
        if index == 3 {
            return Self::moved_or_stop(self.move_items(stack, 4, 40, true), true);
        }
        if index < 3 {
            return Self::moved_or_stop(self.move_items(stack, 4, 40, false), false);
        }
        if self.can_move_into_inputs(stack) && (4..40).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 0, 3, false), false);
        }
        if (4..31).contains(&index) {
            return Self::moved_or_stop(self.move_items(stack, 31, 40, false), false);
        }
        if (31..40).contains(&index) && !self.move_items(stack, 4, 31, false) {
            return MoveResult::Stop { write_back: false };
        }
        MoveResult::Moved { crafted: false }
    }

    fn route_furnace(&mut self, index: usize, stack: &mut ItemStack) -> MoveResult {
        let Some(kind) = self.kind.cooking() else {
            return MoveResult::Stop { write_back: false };
        };
        if index == 2 {
            return Self::moved_or_stop(self.move_items(stack, 3, 39, true), true);
        }
        if index != 0 && index != 1 {
            if !stack.is_empty() && is_cooking_input(kind, stack.item) {
                return Self::moved_or_stop(self.move_items(stack, 0, 1, false), false);
            }
            if is_cooking_fuel(stack) {
                return Self::moved_or_stop(self.move_items(stack, 1, 2, false), false);
            }
            if (3..30).contains(&index) {
                return Self::moved_or_stop(self.move_items(stack, 30, 39, false), false);
            }
            if (30..39).contains(&index) && !self.move_items(stack, 3, 30, false) {
                return MoveResult::Stop { write_back: false };
            }
            return MoveResult::Moved { crafted: false };
        }
        Self::moved_or_stop(self.move_items(stack, 3, 39, false), false)
    }

    fn route_brewing(
        &mut self,
        index: usize,
        stack: &mut ItemStack,
        clicked: &ItemStack,
    ) -> MoveResult {
        let machine_slot = index <= 2 || index == 3 || index == 4;
        if !machine_slot {
            if self.may_place(4, clicked) {
                if self.move_items(stack, 4, 5, false) {
                    return MoveResult::Stop { write_back: true };
                }
                if self.may_place(3, stack) && !self.move_items(stack, 3, 4, false) {
                    return MoveResult::Stop { write_back: true };
                }
                return MoveResult::Moved { crafted: false };
            }
            if self.may_place(3, stack) {
                return Self::moved_or_stop(self.move_items(stack, 3, 4, false), false);
            }
            if is_brewing_potion_input(clicked.item) {
                return Self::moved_or_stop(self.move_items(stack, 0, 3, false), false);
            }
            if (5..32).contains(&index) {
                return Self::moved_or_stop(self.move_items(stack, 32, 41, false), false);
            }
            if (32..41).contains(&index) {
                return Self::moved_or_stop(self.move_items(stack, 5, 32, false), false);
            }
            return Self::moved_or_stop(self.move_items(stack, 5, 41, false), false);
        }
        Self::moved_or_stop(self.move_items(stack, 5, 41, true), true)
    }

    fn moved_or_stop(moved: bool, crafted: bool) -> MoveResult {
        if moved {
            MoveResult::Moved { crafted }
        } else {
            MoveResult::Stop { write_back: false }
        }
    }

    fn can_move_into_inputs(&self, stack: &ItemStack) -> bool {
        if is_smithing_template(stack.item) && self.slots[0].is_empty() {
            return true;
        }
        if is_smithing_base(stack.item) && self.slots[1].is_empty() {
            return true;
        }
        is_smithing_addition(stack.item) && self.slots[2].is_empty()
    }

    fn move_items(
        &mut self,
        stack: &mut ItemStack,
        start: usize,
        end: usize,
        backwards: bool,
    ) -> bool {
        let mut changed = false;
        if stack.is_stackable() {
            let mut dest = if backwards {
                end as i64 - 1
            } else {
                start as i64
            };
            while !stack.is_empty() && in_range(dest, start, end, backwards) {
                let index = dest as usize;
                let target = self.slots[index].clone();
                if !target.is_empty() && stack.is_same_item_same_components(&target) {
                    let max = self.slot_max(index, &target);
                    let total = target.count + stack.count;
                    if total <= max {
                        *stack = ItemStack::empty();
                        let mut merged = target;
                        merged.count = total;
                        self.write_changed(index, merged);
                        changed = true;
                    } else if target.count < max {
                        let added = max - target.count;
                        stack.count -= added;
                        if stack.count <= 0 {
                            *stack = ItemStack::empty();
                        }
                        let mut merged = target;
                        merged.count = max;
                        self.write_changed(index, merged);
                        changed = true;
                    }
                }
                dest += if backwards { -1 } else { 1 };
            }
        }
        if !stack.is_empty() {
            let mut dest = if backwards {
                end as i64 - 1
            } else {
                start as i64
            };
            while in_range(dest, start, end, backwards) {
                let index = dest as usize;
                if self.slots[index].is_empty() && self.may_place(index, stack) {
                    let amount = stack.count.min(self.slot_max(index, stack));
                    let taken = split(stack, amount);
                    self.set_by_player(index, taken);
                    changed = true;
                    break;
                }
                dest += if backwards { -1 } else { 1 };
            }
        }
        changed
    }

    fn on_quick_craft(&mut self, index: usize, picked: &ItemStack, original: i32) {
        if original - picked.count <= 0 {
            return;
        }
        if self.kind.is_furnace() && index == 2 {
            self.furnace_awards += 1;
        }
    }

    fn on_take(&mut self, index: usize, _taken: &ItemStack) {
        match self.role(index) {
            Role::CraftResult => self.take_craft_result(),
            Role::StoneResult => self.take_stone_result(),
            Role::SmithResult => self.take_smith_result(),
            Role::Machine if self.kind.is_furnace() && index == 2 => self.furnace_awards += 1,
            _ => {}
        }
    }

    fn grid_width(&self) -> usize {
        match self.kind {
            Kind::Inventory => 2,
            Kind::Crafting => 3,
            _ => 0,
        }
    }

    fn refresh_craft(&mut self) {
        let width = self.grid_width();
        let len = width * width;
        let grid = self.slots[1..1 + len].to_vec();
        let input = CraftingInput::of(width, width, &grid);
        let result = RecipeManager::global()
            .get_recipe_for(RecipeType::Crafting, &input, None)
            .and_then(|recipe| assemble(recipe, input.slots()))
            .map(normalize)
            .unwrap_or_else(ItemStack::empty);
        self.slots[0] = result.clone();
        self.remote_slots[0].force(&result);
        let state_id = self.bump();
        self.packets.push(Outgoing::Slot {
            state_id,
            slot: 0,
            stack: result,
        });
    }

    fn take_craft_result(&mut self) {
        let width = self.grid_width();
        let len = width * width;
        let grid = self.slots[1..1 + len].to_vec();
        let positioned = CraftingInput::of_positioned(width, width, &grid);
        let recipe =
            RecipeManager::global().get_recipe_for(RecipeType::Crafting, &positioned.input, None);
        let remaining = match recipe {
            Some(recipe) => remaining_items(recipe, positioned.input.slots()),
            None => positioned.input.slots().to_vec(),
        };
        let cropped_width = positioned.input.width();
        for y in 0..positioned.input.height() {
            for x in 0..cropped_width {
                let slot = 1 + x + positioned.left + (y + positioned.top) * width;
                let mut item = self.slots[slot].clone();
                if !item.is_empty() {
                    self.remove_amount(slot, 1);
                    item = self.slots[slot].clone();
                }
                let mut replacement = remaining
                    .get(x + y * cropped_width)
                    .cloned()
                    .unwrap_or_else(ItemStack::empty);
                if replacement.is_empty() {
                    continue;
                }
                if item.is_empty() {
                    self.set_by_player(slot, replacement);
                } else if item.is_same_item_same_components(&replacement) {
                    replacement.count += item.count;
                    self.set_by_player(slot, replacement);
                } else if !self.inventory_add(&mut replacement) {
                    self.discard(replacement);
                }
            }
        }
    }

    fn stone_changed(&mut self) {
        let input = self.slots[0].clone();
        if input.is_same_item(&self.stone_input) {
            return;
        }
        self.stone_input = input.clone();
        if let Some(slot) = self.data.first_mut() {
            *slot = -1;
        }
        self.slots[1] = ItemStack::empty();
        self.stone_recipes = if input.is_empty() {
            Vec::new()
        } else {
            stonecutter_recipes(input.item).collect()
        };
    }

    fn setup_stone_result(&mut self, index: i32) {
        let result = if index >= 0 {
            self.stone_recipes
                .get(index as usize)
                .and_then(|recipe| assemble(recipe, std::slice::from_ref(&self.slots[0])))
                .map(normalize)
                .unwrap_or_else(ItemStack::empty)
        } else {
            ItemStack::empty()
        };
        self.slots[1] = result;
        self.broadcast();
    }

    fn take_stone_result(&mut self) {
        let removed = self.remove_amount(0, 1);
        if !removed.is_empty() {
            let index = self.data.first().copied().unwrap_or(-1);
            self.setup_stone_result(index);
        }
    }

    /// 按钮选结果。和当前编号相同就拒绝，编号越界也返回真。
    ///
    /// Selects a result. The same index is rejected. An out-of-range index still returns true.
    fn click_button(&mut self, button: i32) -> bool {
        if self.kind != Kind::Stonecutter {
            return false;
        }
        let current = self.data.first().copied().unwrap_or(0);
        if current == button {
            return false;
        }
        if button >= 0 && (button as usize) < self.stone_recipes.len() {
            self.data[0] = button;
            self.setup_stone_result(button);
        }
        true
    }

    fn refresh_smithing(&mut self) {
        self.broadcast();
        let template = self.slots[0].clone();
        let base = self.slots[1].clone();
        let addition = self.slots[2].clone();
        let input = SmithingInput::new(template.clone(), base.clone(), addition.clone());
        let result = RecipeManager::global()
            .get_recipe_for(RecipeType::Smithing, &input, None)
            .and_then(|recipe| assemble(recipe, &[template, base, addition]))
            .map(normalize)
            .unwrap_or_else(ItemStack::empty);
        self.slots[3] = result;
        let error = !self.slots[0].is_empty()
            && !self.slots[1].is_empty()
            && !self.slots[2].is_empty()
            && self.slots[3].is_empty();
        if let Some(slot) = self.data.first_mut() {
            *slot = i32::from(error);
        }
    }

    fn take_smith_result(&mut self) {
        for index in 0..3 {
            self.shrink_input(index);
        }
    }

    fn shrink_input(&mut self, index: usize) {
        let mut stack = self.slots[index].clone();
        if stack.is_empty() {
            return;
        }
        stack.count -= 1;
        self.set_by_player(index, normalize(stack));
    }

    fn official_get(&self, official: usize) -> ItemStack {
        match self.cell(official) {
            Cell::Menu(index) => self.slots[index].clone(),
            Cell::Hidden(index) => self.hidden[index].clone(),
        }
    }

    fn official_set(&mut self, official: usize, stack: ItemStack) {
        match self.cell(official) {
            Cell::Menu(index) => self.slots[index] = normalize(stack),
            Cell::Hidden(index) => self.hidden[index] = normalize(stack),
        }
    }

    fn cell(&self, official: usize) -> Cell {
        if self.kind == Kind::Inventory {
            let index = match official {
                0..=8 => 36 + official,
                9..=35 => official,
                36 => 8,
                37 => 7,
                38 => 6,
                39 => 5,
                _ => 45,
            };
            return Cell::Menu(index);
        }
        let start = self.kind.player_start();
        match official {
            0..=8 => Cell::Menu(start + 27 + official),
            9..=35 => Cell::Menu(start + official - 9),
            36..=40 => Cell::Hidden(official - 36),
            _ => Cell::Hidden(4),
        }
    }

    fn inventory_add(&mut self, stack: &mut ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        if stack_is_damaged(stack) {
            match self.free_slot() {
                Some(slot) => {
                    self.official_set(slot, std::mem::take(stack));
                    true
                }
                None if self.creative => {
                    *stack = ItemStack::empty();
                    true
                }
                None => false,
            }
        } else {
            let mut last;
            loop {
                last = stack.count;
                stack.count = self.add_resource(stack);
                *stack = normalize(stack.clone());
                if stack.is_empty() || stack.count >= last {
                    break;
                }
            }
            if stack.count == last && self.creative {
                *stack = ItemStack::empty();
                return true;
            }
            stack.count < last
        }
    }

    fn add_resource(&mut self, stack: &ItemStack) -> i32 {
        let slot = self.remaining_slot(stack).or_else(|| self.free_slot());
        match slot {
            Some(slot) => self.add_resource_to(slot, stack),
            None => stack.count,
        }
    }

    fn add_resource_to(&mut self, official: usize, stack: &ItemStack) -> i32 {
        let mut current = self.official_get(official);
        // 空格按进来的物品算上限。数量是 0 的副本在我们这里算空，不能拿它的上限。
        // An empty slot uses the incoming item's limit. A zero-count copy counts as empty here,
        // so its own limit cannot be used.
        let limit = if current.is_empty() {
            CONTAINER_STACK_LIMIT.min(stack.max_stack_size())
        } else {
            inventory_limit(&current)
        };
        let have = if current.is_empty() { 0 } else { current.count };
        let room = limit - have;
        let to_add = stack.count.min(room);
        if to_add <= 0 {
            return stack.count;
        }
        if current.is_empty() {
            current = stack.clone();
            current.count = to_add;
        } else {
            current.count += to_add;
        }
        self.official_set(official, normalize(current));
        stack.count - to_add
    }

    fn remaining_slot(&self, incoming: &ItemStack) -> Option<usize> {
        let selected = usize::from(self.selected);
        if self.has_room(selected, incoming) {
            return Some(selected);
        }
        if self.has_room(40, incoming) {
            return Some(40);
        }
        (0..36).find(|&slot| self.has_room(slot, incoming))
    }

    fn has_room(&self, official: usize, incoming: &ItemStack) -> bool {
        let slot = self.official_get(official);
        !slot.is_empty()
            && incoming.is_same_item_same_components(&slot)
            && slot.is_stackable()
            && slot.count < inventory_limit(&slot)
    }

    fn free_slot(&self) -> Option<usize> {
        (0..36).find(|&slot| self.official_get(slot).is_empty())
    }

    fn discard(&mut self, stack: ItemStack) {
        // 没有物品实体。丢掉的东西消失，不抽世界随机数：官方丢物品用的是实体自己的随机源。
        // No item entities. Dropped stacks vanish and do not draw level random: vanilla item
        // drops use the entity's own random source.
        let _ = stack;
    }

    fn broadcast(&mut self) {
        if self.suppress {
            return;
        }
        for index in 0..self.slots.len() {
            let current = self.slots[index].clone();
            if !self.remote_slots[index].matches(&current) {
                self.remote_slots[index].force(&current);
                let state_id = self.bump();
                self.packets.push(Outgoing::Slot {
                    state_id,
                    slot: index as i16,
                    stack: current,
                });
            }
        }
        if !self.remote_carried.matches(&self.carried) {
            let carried = self.carried.clone();
            self.remote_carried.force(&carried);
            self.packets.push(Outgoing::Carried(carried));
        }
        for (id, value) in self.data.iter().copied().enumerate() {
            if self.remote_data[id] != value {
                self.remote_data[id] = value;
                self.packets.push(Outgoing::Data {
                    id: id as i16,
                    value: value as i16,
                });
            }
        }
    }

    fn send_all(&mut self) {
        for (index, stack) in self.slots.iter().enumerate() {
            self.remote_slots[index].force(stack);
        }
        self.remote_carried.force(&self.carried);
        self.remote_data.clone_from(&self.data);
        let state_id = self.bump();
        self.packets.push(Outgoing::Content {
            state_id,
            items: self.slots.clone(),
            carried: self.carried.clone(),
        });
        for (id, value) in self.data.iter().copied().enumerate() {
            self.packets.push(Outgoing::Data {
                id: id as i16,
                value: value as i16,
            });
        }
    }

    fn receive(&mut self, slot: i16, hash: HashedStack) {
        if let Ok(index) = usize::try_from(slot)
            && index < self.remote_slots.len()
        {
            self.remote_slots[index].receive(hash);
        }
    }

    fn grids_to_return(&mut self) -> Vec<ItemStack> {
        let range = match self.kind {
            Kind::Crafting => 1..10,
            Kind::Stonecutter => 0..1,
            Kind::Smithing => 0..3,
            _ => 0..0,
        };
        range
            .map(|index| std::mem::take(&mut self.slots[index]))
            .collect()
    }

    /// 重放用的打开：材料已经放进格子，这里只做一次 `sendAllDataToRemote`。
    ///
    /// Opens a replay. Ingredients are already in the grid; this is the one `sendAllDataToRemote`.
    pub(crate) fn replay_open(&mut self) -> StationSnapshot {
        self.send_all();
        self.replay_view()
    }

    /// 和菜单 oracle 一样的点击：压住同步、点、光标按空的收回来、再 `broadcastChanges`。
    ///
    /// The menu oracle's click: suppress, click, receive an empty cursor, then `broadcastChanges`.
    pub(crate) fn replay_click(
        &mut self,
        slot: i32,
        button: i32,
        input: ContainerInput,
    ) -> StationSnapshot {
        self.suppress = true;
        self.do_click(slot, button, input);
        self.remote_carried.receive(HashedStack::Empty);
        self.suppress = false;
        self.broadcast();
        self.replay_view()
    }

    /// 切石机的按钮。返回假时官方不再广播。
    ///
    /// The stonecutter button. Vanilla does not broadcast when this returns false.
    pub(crate) fn replay_button(&mut self, button: i32) -> StationSnapshot {
        if self.click_button(button) {
            self.broadcast();
        }
        self.replay_view()
    }

    pub(crate) fn set_replay_creative(&mut self, creative: bool) {
        self.creative = creative;
    }

    pub(crate) fn replay_view(&self) -> StationSnapshot {
        StationSnapshot {
            slots: self.slots.clone(),
            carried: self.carried.clone(),
            state_id: self.state_id,
            data: self.data.clone(),
        }
    }
}

/// 重放里菜单的一种。和 [`super::replay::ReplayShape`] 里工作台那几项对应。
///
/// One workstation menu in a replay. Matches the workstation variants of [`super::replay::ReplayShape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkKind {
    Inventory,
    Crafting,
    Stonecutter,
    Smithing,
    Furnace,
    BlastFurnace,
    Smoker,
    Brewing,
}

impl From<WorkKind> for Kind {
    fn from(kind: WorkKind) -> Self {
        match kind {
            WorkKind::Inventory => Self::Inventory,
            WorkKind::Crafting => Self::Crafting,
            WorkKind::Stonecutter => Self::Stonecutter,
            WorkKind::Smithing => Self::Smithing,
            WorkKind::Furnace => Self::Furnace,
            WorkKind::BlastFurnace => Self::BlastFurnace,
            WorkKind::Smoker => Self::Smoker,
            WorkKind::Brewing => Self::Brewing,
        }
    }
}

/// 重放看到的菜单。
///
/// What a replay observes.
pub(crate) struct StationSnapshot {
    pub slots: Vec<ItemStack>,
    pub carried: ItemStack,
    pub state_id: i32,
    pub data: Vec<i32>,
}

/// 造一个不连世界的菜单。`inventory` 是玩家那 46 格，合成格是空的。
/// `seeds` 按顺序用 `Slot.set` 放进菜单自己的格子，放的时候同步是压住的，
/// 只有合成结果那条官方本来就会推进的 state id 会动。
///
/// Builds a menu with no world. `inventory` is the player's 46 slots, with an empty crafting grid.
/// `seeds` are applied in order through `Slot.set` while updates are suppressed, so only the crafting
/// result's state id moves, which vanilla increments even before the synchronizer exists.
pub(crate) fn prepare_replay(
    kind: WorkKind,
    inventory: &[ItemStack],
    seeds: &[(usize, ItemStack)],
    data: &[i32],
    creative: bool,
) -> Station {
    let mut station = Station::create(kind.into(), 1, BlockPos::new(0, 0, 0));
    station.creative = creative;
    let mut slots = vec![ItemStack::empty(); 46];
    for (index, stack) in inventory.iter().enumerate().take(slots.len()) {
        slots[index] = stack.clone();
    }
    station.load_player(&slots, 0);
    for (index, value) in data.iter().enumerate() {
        if let Some(slot) = station.data.get_mut(index) {
            *slot = *value;
        }
    }
    station.suppress = true;
    for &(index, ref stack) in seeds {
        if index < station.slots.len() {
            station.set_by_player(index, stack.clone());
        }
    }
    station.suppress = false;
    station
}

enum Cell {
    Menu(usize),
    Hidden(usize),
}

fn accepts_input(stack: &ItemStack) -> bool {
    mc_bamboo_recipes::accepts_stonecutter_input(stack.item)
}

fn in_range(dest: i64, start: usize, end: usize, backwards: bool) -> bool {
    if backwards {
        dest >= start as i64
    } else {
        dest < end as i64
    }
}

fn place_count(size: usize, kind: i32, stack: &ItemStack) -> i32 {
    match kind {
        0 => (stack.count as f32 / size as f32).floor() as i32,
        1 => 1,
        2 => stack.max_stack_size(),
        _ => stack.count,
    }
}

fn inventory_limit(stack: &ItemStack) -> i32 {
    CONTAINER_STACK_LIMIT.min(stack.max_stack_size())
}

fn copy_count(stack: &ItemStack, count: i32) -> ItemStack {
    if stack.is_empty() || count <= 0 {
        return ItemStack::empty();
    }
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

fn split(stack: &mut ItemStack, amount: i32) -> ItemStack {
    if stack.is_empty() || amount <= 0 {
        return ItemStack::empty();
    }
    let taken_count = amount.min(stack.count);
    let taken = copy_count(stack, taken_count);
    stack.count -= taken_count;
    if stack.count <= 0 {
        *stack = ItemStack::empty();
    }
    taken
}

fn normalize(stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        ItemStack::empty()
    } else {
        stack
    }
}

fn stack_is_damaged(stack: &ItemStack) -> bool {
    if stack.is_empty() {
        return false;
    }
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};
    let patch = stack.patch();
    let resolve = |kind| defaults::resolve(stack.item, &patch, kind);
    let int = |kind| match resolve(kind) {
        Some(ComponentValue::MaxDamage(value) | ComponentValue::Damage(value)) => Some(*value),
        _ => None,
    };
    let (Some(max_damage), Some(damage)) = (int(kinds::MAX_DAMAGE), int(kinds::DAMAGE)) else {
        return false;
    };
    if resolve(kinds::UNBREAKABLE).is_some() {
        return false;
    }
    damage.clamp(0, max_damage) > 0
}

fn spectator(region: &Region, id: PlayerId) -> bool {
    region
        .players
        .get(&id)
        .is_some_and(|player| player.game_mode == GameMode::Spectator)
}

fn send_outgoing(region: &mut Region, id: PlayerId, container_id: i32, packets: Vec<Outgoing>) {
    for packet in packets {
        match packet {
            Outgoing::Content {
                state_id,
                items,
                carried,
            } => region.links.send(
                id,
                &ContainerSetContent {
                    container_id,
                    state_id,
                    items: &items,
                    carried: &carried,
                },
            ),
            Outgoing::Slot {
                state_id,
                slot,
                stack,
            } => region.links.send(
                id,
                &ContainerSetSlot {
                    container_id,
                    state_id,
                    slot,
                    stack: &stack,
                },
            ),
            Outgoing::Data { id: data_id, value } => region.links.send(
                id,
                &ContainerSetData {
                    container_id,
                    id: data_id,
                    value,
                },
            ),
            Outgoing::Carried(stack) => {
                region.links.send(id, &SetCursorItem { stack: &stack });
            }
        }
    }
}

fn pull_machine(region: &mut Region, station: &mut Station) {
    match station.kind {
        Kind::Furnace | Kind::BlastFurnace | Kind::Smoker => {
            let (level, _) = region.block_level();
            let Some(entity) = level.block_entities.get::<FurnaceEntity>(station.pos) else {
                return;
            };
            for (index, stack) in entity.items().iter().enumerate() {
                station.slots[index] = stack.clone();
            }
            station.data = entity.data_values().to_vec();
        }
        Kind::Brewing => {
            let (level, _) = region.block_level();
            let Some(entity) = level.block_entities.get::<BrewingStandEntity>(station.pos) else {
                return;
            };
            for (index, stack) in entity.items().iter().enumerate() {
                station.slots[index] = stack.clone();
            }
            station.data = entity.data_values().to_vec();
        }
        _ => {}
    }
}

fn flush_machine(region: &mut Region, station: &mut Station) {
    let set = station.machine_set;
    let content = station.machine_content;
    let awards = station.furnace_awards;
    station.machine_set = [false; 5];
    station.machine_content = [false; 5];
    station.furnace_awards = 0;
    let dirty = set.iter().any(|flag| *flag) || content.iter().any(|flag| *flag) || awards > 0;
    if !dirty {
        return;
    }
    let pos = station.pos;
    match station.kind {
        Kind::Furnace | Kind::BlastFurnace | Kind::Smoker => {
            let stacks = station.slots[..3].to_vec();
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            let _ = world.with_block_entity::<FurnaceEntity, _>(pos, |entity, world| {
                for slot in 0..3 {
                    if set[slot] {
                        entity.set_item(slot, stacks[slot].clone());
                    } else if content[slot] {
                        entity.write_item(slot, stacks[slot].clone());
                    }
                }
                for _ in 0..awards {
                    entity.award_experience(world);
                }
            });
        }
        Kind::Brewing => {
            let stacks = station.slots[..5].to_vec();
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            let _ = world.with_block_entity::<BrewingStandEntity, _>(pos, |entity, _world| {
                for slot in 0..5 {
                    if set[slot] || content[slot] {
                        entity.set_item(slot, stacks[slot].clone());
                    }
                }
            });
        }
        _ => {}
    }
}

fn apply_click(region: &mut Region, id: PlayerId, station: &mut Station, packet: &ClickPacket) {
    let (creative, selected, inv) = {
        let Some(player) = region.players.get(&id) else {
            return;
        };
        (
            player.game_mode == GameMode::Creative,
            player.inventory.selected,
            player.inventory.slots.clone(),
        )
    };
    station.creative = creative;
    station.load_player(&inv, selected);
    pull_machine(region, station);
    let before = station.state_id;
    station.click(
        i32::from(packet.slot),
        i32::from(packet.button),
        packet.input,
    );
    for (slot, hash) in &packet.changed_slots {
        station.receive(*slot, hash.clone());
    }
    station.remote_carried.receive(packet.carried.clone());
    if let Some(player) = region.players.get_mut(&id) {
        station.write_player(&mut player.inventory);
    }
    flush_machine(region, station);
    pull_machine(region, station);
    if packet.state_id != before {
        station.send_all();
    } else {
        station.broadcast();
    }
}

/// 右键工作台、切石机、锻造台、熔炉或酿造台。不是这些方块就返回 `false`。
///
/// Right-click a crafting table, stonecutter, smithing table, furnace, or brewing stand.
/// Returns `false` for every other block.
pub(crate) fn try_open(region: &mut Region, id: PlayerId, pos: BlockPos) -> bool {
    let Some(state) = region.chunks.block(pos) else {
        return false;
    };
    let Some(kind) = Kind::from_block(state) else {
        return false;
    };
    if !region.players.contains_key(&id) {
        return false;
    }
    if region.menus.contains_key(&id) {
        super::close_menu(region, id);
    }
    let Some((container_id, creative, selected, inv)) = region.players.get_mut(&id).map(|player| {
        player.container_counter = player.container_counter % 100 + 1;
        (
            player.container_counter,
            player.game_mode == GameMode::Creative,
            player.inventory.selected,
            player.inventory.slots.clone(),
        )
    }) else {
        return false;
    };
    let mut station = Station::create(kind, container_id, pos);
    station.creative = creative;
    station.load_player(&inv, selected);
    pull_machine(region, &mut station);
    station.send_all();
    let title = Component::translate(kind.title(), []);
    region.links.send(
        id,
        &OpenScreen {
            container_id,
            menu_type: kind.menu_type(),
            title: &title,
        },
    );
    let packets = std::mem::take(&mut station.packets);
    send_outgoing(region, id, container_id, packets);
    let mut menu = Menu::new(container_id, ContainerKind::Chest, Target::Single(pos), 0);
    menu.station = Some(station);
    region.menus.insert(id, menu);
    true
}

pub(crate) fn click_menu(region: &mut Region, id: PlayerId, packet: ClickPacket) {
    let matches = region
        .menus
        .get(&id)
        .is_some_and(|menu| menu.station.is_some() && menu.container_id == packet.container_id);
    if !matches {
        return;
    }
    let valid = region.menus.get(&id).is_some_and(|menu| {
        menu.station
            .as_ref()
            .is_some_and(|station| station.still_valid(region, id))
    });
    if !valid {
        debug!(
            player = id.0,
            "ignoring a click on a menu that is no longer valid"
        );
        return;
    }
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    if let Some(station) = menu.station.as_mut() {
        if !station.valid_slot(packet.slot) {
            debug!(
                player = id.0,
                slot = packet.slot,
                "ignoring a click on an unknown slot"
            );
        } else if spectator(region, id) {
            station.send_all();
            let packets = std::mem::take(&mut station.packets);
            let container_id = station.container_id;
            send_outgoing(region, id, container_id, packets);
        } else {
            apply_click(region, id, station, &packet);
            let packets = std::mem::take(&mut station.packets);
            let container_id = station.container_id;
            send_outgoing(region, id, container_id, packets);
        }
    }
    region.menus.insert(id, menu);
}

pub(crate) fn click_inventory(region: &mut Region, id: PlayerId, packet: ClickPacket) {
    if spectator(region, id) {
        let packets = {
            let Some(player) = region.players.get_mut(&id) else {
                return;
            };
            let mut station = Station::from_player(player);
            station.send_all();
            player.inventory.set_state_id(station.state_id);
            station.store_cursor(&mut player.inventory_cursor);
            std::mem::take(&mut station.packets)
        };
        send_outgoing(region, id, 0, packets);
        return;
    }
    let packets = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let mut station = Station::from_player(player);
        if !station.valid_slot(packet.slot) {
            debug!(
                player = id.0,
                slot = packet.slot,
                "ignoring a click on an unknown slot"
            );
            return;
        }
        let before = station.state_id;
        station.click(
            i32::from(packet.slot),
            i32::from(packet.button),
            packet.input,
        );
        for (slot, hash) in &packet.changed_slots {
            station.receive(*slot, hash.clone());
        }
        station.remote_carried.receive(packet.carried.clone());
        if packet.state_id != before {
            station.send_all();
        } else {
            station.broadcast();
        }
        station.write_player(&mut player.inventory);
        player.inventory.set_state_id(station.state_id);
        station.store_cursor(&mut player.inventory_cursor);
        std::mem::take(&mut station.packets)
    };
    send_outgoing(region, id, 0, packets);
}

pub(crate) fn on_button(region: &mut Region, id: PlayerId, container_id: i32, button: i32) {
    if spectator(region, id) {
        return;
    }
    let matches = region.menus.get(&id).is_some_and(|menu| {
        menu.station.is_some()
            && menu.container_id == container_id
            && menu
                .station
                .as_ref()
                .is_some_and(|station| station.still_valid(region, id))
    });
    if !matches {
        return;
    }
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    if let Some(station) = menu.station.as_mut() {
        if station.click_button(button) {
            station.broadcast();
        }
        let packets = std::mem::take(&mut station.packets);
        let container_id = station.container_id;
        send_outgoing(region, id, container_id, packets);
    }
    region.menus.insert(id, menu);
}

pub(crate) fn tick_menu(region: &mut Region, id: PlayerId) {
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    if let Some(station) = menu.station.as_mut() {
        if let Some(player) = region.players.get(&id) {
            let selected = player.inventory.selected;
            let slots = player.inventory.slots.clone();
            station.load_player(&slots, selected);
        }
        pull_machine(region, station);
        station.broadcast();
        let packets = std::mem::take(&mut station.packets);
        let container_id = station.container_id;
        send_outgoing(region, id, container_id, packets);
    }
    region.menus.insert(id, menu);
}

pub(crate) fn finish(region: &mut Region, id: PlayerId, menu: &mut Menu, return_items: bool) {
    let Some(mut station) = menu.station.take() else {
        return;
    };
    if return_items && !station.carried.is_empty() {
        super::place_item_back_in_inventory(region, id, std::mem::take(&mut station.carried));
    }
    for stack in station.grids_to_return() {
        super::place_item_back_in_inventory(region, id, stack);
    }
}

/// 玩家离开时，把 2×2 里的材料放回背包。结果预览丢掉。
///
/// On leave, put the 2×2 ingredients back into the inventory. The result preview is discarded.
pub(crate) fn return_player_grid(region: &mut Region, id: PlayerId) {
    let stacks = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let stacks: Vec<_> = (1..5)
            .map(|slot| std::mem::take(&mut player.inventory.slots[slot]))
            .collect();
        player.inventory.slots[0] = ItemStack::empty();
        player.inventory_cursor.carried = ItemStack::empty();
        player.inventory_cursor.quickcraft_status = 0;
        player.inventory_cursor.quickcraft_slots.clear();
        stacks
    };
    for stack in stacks {
        super::place_item_back_in_inventory(region, id, stack);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;

    fn stack(item: i32, count: i32) -> ItemStack {
        ItemStack::new(item, count)
    }

    #[test]
    fn crafting_table_makes_planks_from_one_log() {
        let mut station = Station::create(Kind::Crafting, 1, BlockPos::new(0, 0, 0));
        station.set_by_player(1, stack(items::OAK_LOG, 1));
        assert_eq!(station.slots[0].item, items::OAK_PLANKS);
        assert_eq!(station.slots[0].count, 4);
    }

    #[test]
    fn shift_click_crafts_every_log() {
        let mut station = Station::create(Kind::Crafting, 1, BlockPos::new(0, 0, 0));
        station.set_by_player(1, stack(items::OAK_LOG, 2));
        station.click(0, 0, ContainerInput::QuickMove);
        assert!(station.slots[0].is_empty());
        assert!(station.slots[1].is_empty());
        assert_eq!(station.slots[45].item, items::OAK_PLANKS);
        assert_eq!(station.slots[45].count, 8);
    }

    #[test]
    fn right_click_takes_the_whole_result() {
        let mut station = Station::create(Kind::Crafting, 1, BlockPos::new(0, 0, 0));
        station.set_by_player(1, stack(items::OAK_LOG, 1));
        station.click(0, 1, ContainerInput::Pickup);
        assert_eq!(station.carried.item, items::OAK_PLANKS);
        assert_eq!(station.carried.count, 4);
        assert!(station.slots[1].is_empty());
        assert!(station.slots[0].is_empty());
    }
}
