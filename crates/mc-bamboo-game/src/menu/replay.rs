//! 不碰网络和世界的菜单重放，给 `mc-bamboo-vanilla menu` 和官方菜单对比用。
//!
//! 这里跑的和 [`super::on_click`] 是同一套点击逻辑，只是把发包换成了记账：
//! 每一步之后返回菜单里的全部槽位、光标和 `state_id`。
//!
//! 客户端按“从不预测”的模型模拟：每次点击都声称自己没有改过任何槽、光标是空的。
//! 官方客户端会预测，但那是另一条路径；两边用同一个模型才比得了。

use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::{
    components::{ComponentPatch, archive::stack::component_hashes},
    item::ItemStack,
    packet::play::{ContainerInput, HashedItem},
};

use super::{Ctx, HashedStack, Menu, Target};
use crate::{
    container::ContainerKind,
    inventory::{HOTBAR_START, Inventory, OFF_HAND},
};

/// 菜单对应的方块种类。箱子要带行数，双箱是 6 行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayShape {
    Chest(usize),
    Hopper,
    Dispenser,
    ShulkerBox,
    /// 玩家物品栏里的 2×2。
    ///
    /// The player's 2×2 crafting grid.
    Inventory,
    Crafting,
    /// 合成器：九格材料、玩家 36 格、一个只读结果格。
    ///
    /// A crafter: nine grid slots, the player's 36 slots, and a read-only result slot.
    Crafter,
    Stonecutter,
    Smithing,
    Furnace,
    BlastFurnace,
    Smoker,
    Brewing,
    Loom,
    Grindstone,
}

impl ReplayShape {
    fn kind(self) -> ContainerKind {
        match self {
            Self::Chest(_) => ContainerKind::Chest,
            Self::Hopper => ContainerKind::Hopper,
            Self::Dispenser => ContainerKind::Dispenser,
            Self::ShulkerBox => ContainerKind::ShulkerBox,
            Self::Inventory
            | Self::Crafting
            | Self::Stonecutter
            | Self::Smithing
            | Self::Furnace
            | Self::BlastFurnace
            | Self::Smoker
            | Self::Brewing
            | Self::Loom
            | Self::Grindstone
            | Self::Crafter => ContainerKind::Chest,
        }
    }

    fn workstation(self) -> Option<super::station::WorkKind> {
        use super::station::WorkKind;
        match self {
            Self::Inventory => Some(WorkKind::Inventory),
            Self::Crafting => Some(WorkKind::Crafting),
            Self::Crafter => Some(WorkKind::Crafter),
            Self::Stonecutter => Some(WorkKind::Stonecutter),
            Self::Smithing => Some(WorkKind::Smithing),
            Self::Furnace => Some(WorkKind::Furnace),
            Self::BlastFurnace => Some(WorkKind::BlastFurnace),
            Self::Smoker => Some(WorkKind::Smoker),
            Self::Brewing => Some(WorkKind::Brewing),
            Self::Loom => Some(WorkKind::Loom),
            Self::Grindstone => Some(WorkKind::Grindstone),
            _ => None,
        }
    }

    /// 工作台、熔炉、酿造台这一类，格子不在单独的容器里。
    ///
    /// Crafting, furnaces, and the brewing stand keep their grid on the menu itself.
    pub fn is_workstation(self) -> bool {
        self.workstation().is_some()
    }

    /// 合成格的材料放进菜单时要往后挪一格，结果格是算出来的。
    ///
    /// Crafting ingredients land one slot later. The result slot is computed.
    fn seed_slot(self, index: usize) -> Option<usize> {
        match self {
            Self::Inventory | Self::Crafting => Some(index + 1),
            Self::Stonecutter
            | Self::Smithing
            | Self::Furnace
            | Self::BlastFurnace
            | Self::Smoker
            | Self::Brewing
            | Self::Loom
            | Self::Grindstone
            | Self::Crafter => Some(index),
            _ => None,
        }
    }

    fn slots(self) -> usize {
        match self {
            Self::Chest(rows) => rows * 9,
            _ => self.kind().slots(),
        }
    }
}

/// 一次点击之后菜单的样子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// 菜单里的全部槽位，前几格是容器的，后面是玩家的 36 格。
    pub slots: Vec<ItemStack>,
    pub carried: ItemStack,
    pub state_id: i32,
    /// 进度条和切石机选中项。箱子没有。
    ///
    /// Progress bars and the stonecutter selection. Chests have none.
    pub data: Vec<i32>,
}

/// 一个打开着的菜单，只有点击逻辑，没有世界。
pub struct Replay {
    menu: Menu,
    container: Vec<ItemStack>,
    inventory: Inventory,
    creative: bool,
    station: Option<super::station::Station>,
}

impl Replay {
    /// `container` 和 `inventory` 都是 `(物品 ID, 数量)`，空位用 `(0, 0)`。
    /// 物品栏按官方 `Inventory` 的下标给，0 到 8 是快捷栏，9 到 35 是背包，40 是副手。
    pub fn new(shape: ReplayShape, container: &[(i32, i32)], inventory: &[(i32, i32)]) -> Self {
        let size = shape.slots();
        let mut slots = vec![ItemStack::empty(); size];
        for (index, &(item, count)) in container.iter().enumerate().take(size) {
            slots[index] = stack(item, count);
        }
        let mut player = Inventory::default();
        for (index, &(item, count)) in inventory.iter().enumerate() {
            if let Some(slot) = official_slot(index) {
                player.slots[slot] = stack(item, count);
            }
        }
        Self {
            menu: Menu::new(
                1,
                shape.kind(),
                Target::Single(BlockPos::new(0, 0, 0)),
                size,
            ),
            container: slots,
            inventory: player,
            creative: true,
            station: None,
        }
    }

    /// 工作台菜单。`grid` 是菜单自己的材料格，空位用 `(0, 0)`，按顺序放进去。
    ///
    /// A workstation menu. `grid` is the menu's own ingredient slots, with `(0, 0)` for empty ones,
    /// applied in order.
    pub fn workstation(
        shape: ReplayShape,
        grid: &[(i32, i32)],
        inventory: &[(i32, i32)],
        data: &[i32],
        creative: bool,
    ) -> Self {
        let kind = shape.workstation().expect("workstation shape");
        let mut seeds = Vec::new();
        for (index, &(item, count)) in grid.iter().enumerate() {
            if item == 0 || count <= 0 {
                continue;
            }
            if let Some(slot) = shape.seed_slot(index) {
                seeds.push((slot, stack(item, count)));
            }
        }
        let mut replay = Self::new(ReplayShape::Chest(1), &[], &[]);
        replay.creative = creative;
        replay.station = Some(super::station::prepare_replay(
            kind,
            &bamboo_inventory(inventory),
            &seeds,
            data,
            creative,
        ));
        replay
    }

    /// 非创造模式时中键克隆什么也不做。
    pub fn set_creative(&mut self, creative: bool) {
        self.creative = creative;
        if let Some(station) = self.station.as_mut() {
            station.set_replay_creative(creative);
        }
    }

    /// 打开之前换掉工作台自己的一格。织布机和砂轮的材料补丁走这里。
    ///
    /// Replace one of the workstation's own slots before the menu opens. Loom and grindstone
    /// ingredient patches use this.
    pub fn reseed(&mut self, index: usize, stack: ItemStack) {
        if let Some(station) = self.station.as_mut() {
            station.replay_replace(index, stack);
        }
    }

    /// 打开之前换掉容器里的一格。组件点击题用它放上带补丁的物品。
    ///
    /// Replace one container slot before the menu opens.
    pub fn set_container(&mut self, index: usize, stack: ItemStack) {
        if index < self.container.len() {
            self.container[index] = stack;
        }
    }

    /// 模拟 `sendAllDataToRemote`，打开菜单后的第一个 state_id 是 1。
    /// 工作台如果事先放了合成材料，每次重算结果还会再加一次。
    ///
    /// Simulates `sendAllDataToRemote`. The first state id is 1.
    /// A workstation that was seeded with crafting ingredients increments once more per recompute.
    pub fn open(&mut self) -> Snapshot {
        if let Some(station) = self.station.as_mut() {
            let view = station.replay_open();
            return Self::from_station(view);
        }
        let slots = self.menu_slots();
        self.menu.take_full_state(&slots);
        self.snapshot()
    }

    /// 点一下。槽位不合法时和官方一样什么也不做。
    pub fn click(&mut self, slot: i16, button: i8, input: ContainerInput) -> Snapshot {
        if let Some(station) = self.station.as_mut() {
            let view = station.replay_click(i32::from(slot), i32::from(button), input);
            return Self::from_station(view);
        }
        if !self.menu.valid_slot_index(slot) {
            return self.snapshot();
        }
        let mut thrown = Vec::new();
        let mut ctx = Ctx {
            kind: self.menu.kind,
            container_size: self.menu.container_size,
            container: &mut self.container,
            inventory: &mut self.inventory,
            carried: &mut self.menu.carried,
            quickcraft_status: &mut self.menu.quickcraft_status,
            quickcraft_type: &mut self.menu.quickcraft_type,
            quickcraft_slots: &mut self.menu.quickcraft_slots,
            creative: self.creative,
            thrown: &mut thrown,
        };
        super::do_click(&mut ctx, i32::from(slot), i32::from(button), input);
        // 客户端从不预测，回传的总是空光标。
        self.menu.remote_carried.receive(HashedStack::Empty);
        let slots = self.menu_slots();
        self.menu.take_slot_changes(&slots);
        self.menu.take_carried_change();
        self.snapshot()
    }

    /// 切石机的 `container_button_click`。别的菜单点了等于没点。
    ///
    /// The stonecutter's `container_button_click`. Other menus ignore it.
    pub fn button(&mut self, button: i8) -> Snapshot {
        if let Some(station) = self.station.as_mut() {
            let view = station.replay_button(i32::from(button));
            return Self::from_station(view);
        }
        self.snapshot()
    }

    /// 官方 `MultiPlayerGameMode.handleContainerInput` 的预测：变过的槽和光标都回传
    /// `HashedStack.create`。返回这次服务端补发的槽位，以及光标有没有被补发。
    ///
    /// The predicting client from `MultiPlayerGameMode.handleContainerInput`.
    /// Returns the slots the server resent, and whether the carried item was resent.
    pub fn click_predicting(
        &mut self,
        slot: i16,
        button: i8,
        input: ContainerInput,
    ) -> (Snapshot, Vec<i16>, bool) {
        if !self.menu.valid_slot_index(slot) {
            return (self.snapshot(), Vec::new(), false);
        }
        let before = self.menu_slots();
        let mut thrown = Vec::new();
        let mut ctx = Ctx {
            kind: self.menu.kind,
            container_size: self.menu.container_size,
            container: &mut self.container,
            inventory: &mut self.inventory,
            carried: &mut self.menu.carried,
            quickcraft_status: &mut self.menu.quickcraft_status,
            quickcraft_type: &mut self.menu.quickcraft_type,
            quickcraft_slots: &mut self.menu.quickcraft_slots,
            creative: self.creative,
            thrown: &mut thrown,
        };
        super::do_click(&mut ctx, i32::from(slot), i32::from(button), input);
        let after = self.menu_slots();
        for (index, (previous, current)) in before.iter().zip(&after).enumerate() {
            if !previous.matches(current) {
                self.menu.remote_slots[index].receive(hashed_stack(current));
            }
        }
        self.menu
            .remote_carried
            .receive(hashed_stack(&self.menu.carried));
        let resent = self
            .menu
            .take_slot_changes(&after)
            .iter()
            .map(|(_, index, _)| *index as i16)
            .collect();
        let carried_resent = self.menu.take_carried_change().is_some();
        (self.snapshot(), resent, carried_resent)
    }

    pub fn snapshot(&self) -> Snapshot {
        if let Some(station) = &self.station {
            return Self::from_station(station.replay_view());
        }
        Snapshot {
            slots: self.menu_slots(),
            carried: self.menu.carried.clone(),
            state_id: self.menu.state_id,
            data: Vec::new(),
        }
    }

    fn from_station(view: super::station::StationSnapshot) -> Snapshot {
        Snapshot {
            slots: view.slots,
            carried: view.carried,
            state_id: view.state_id,
            data: view.data,
        }
    }

    /// 菜单看到的槽位：容器的格加上玩家物品栏的 36 格，顺序和官方一样。
    fn menu_slots(&self) -> Vec<ItemStack> {
        let mut slots = self.container.clone();
        slots.extend_from_slice(&self.inventory.slots[super::INVENTORY_MENU_START..OFF_HAND]);
        slots
    }
}

/// 官方 `Inventory` 的下标换算成菜单槽位。盔甲的 36 到 39 不在菜单里，但重放时也放上，
/// 免得以后有人从这里取。
fn official_slot(index: usize) -> Option<usize> {
    match index {
        0..=8 => Some(HOTBAR_START + index),
        9..=35 => Some(index),
        36..=39 => Some(OFF_HAND - 1 - index),
        40 => Some(OFF_HAND),
        _ => None,
    }
}

fn bamboo_inventory(official: &[(i32, i32)]) -> Vec<ItemStack> {
    let mut slots = vec![ItemStack::empty(); 46];
    for (index, &(item, count)) in official.iter().enumerate() {
        if let Some(slot) = official_slot(index) {
            slots[slot] = stack(item, count);
        }
    }
    slots
}

/// 官方 `HashedStack.create`。算不出哈希时放一个对不上的值，服务端会把这格再发一次。
///
/// `HashedStack.create`. A value that cannot be hashed does not match, so the
/// server sends the slot again.
fn hashed_stack(stack: &ItemStack) -> HashedStack {
    if stack.is_empty() {
        return HashedStack::Empty;
    }
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return hashed_mismatch(stack);
    };
    match component_hashes(&patch) {
        Some(hashed) => HashedStack::Item(HashedItem {
            item: stack.item,
            count: stack.count,
            added: hashed.added,
            removed: hashed.removed,
        }),
        None => hashed_mismatch(stack),
    }
}

fn hashed_mismatch(stack: &ItemStack) -> HashedStack {
    HashedStack::Item(HashedItem {
        item: stack.item,
        count: stack.count,
        added: vec![(i32::MIN, 0)],
        removed: Vec::new(),
    })
}

fn stack(item: i32, count: i32) -> ItemStack {
    if item == 0 || count <= 0 {
        ItemStack::empty()
    } else {
        ItemStack::new(item, count)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;

    /// 打开后是全空的菜单，state_id 从 1 开始。
    #[test]
    fn opening_a_chest_starts_with_state_one() {
        let mut replay = Replay::new(ReplayShape::Chest(3), &[], &[]);
        let snapshot = replay.open();
        assert_eq!(snapshot.state_id, 1);
        assert_eq!(snapshot.slots.len(), 63);
        assert!(snapshot.slots.iter().all(ItemStack::is_empty));
    }

    /// 从物品栏拿起一格放进箱子，槽位和光标都对。
    #[test]
    fn a_pickup_moves_a_stack_into_the_container() {
        let mut inventory = vec![(0, 0); 41];
        inventory[0] = (items::STONE, 10);
        let mut replay = Replay::new(ReplayShape::Chest(3), &[], &inventory);
        replay.open();

        // 27 格容器之后是 27 格背包，再往后才是快捷栏，所以快捷栏第一格是 54。
        let empty = replay.click(54, 0, ContainerInput::Pickup);
        assert_eq!(empty.carried, ItemStack::new(items::STONE, 10));

        let placed = replay.click(0, 0, ContainerInput::Pickup);
        assert_eq!(placed.slots[0], ItemStack::new(items::STONE, 10));
        assert!(placed.carried.is_empty());
    }

    /// 拖动时同一个槽位经过两次只算一次，和官方的 Set<Slot> 一样。
    #[test]
    fn dragging_over_one_slot_twice_places_one_item() {
        let mut inventory = vec![(0, 0); 41];
        inventory[0] = (items::STONE, 10);
        let mut replay = Replay::new(ReplayShape::Chest(3), &[], &inventory);
        replay.open();

        replay.click(54, 0, ContainerInput::Pickup);
        replay.click(54, 4, ContainerInput::QuickCraft);
        replay.click(54, 5, ContainerInput::QuickCraft);
        replay.click(54, 5, ContainerInput::QuickCraft);
        let snapshot = replay.click(54, 6, ContainerInput::QuickCraft);

        assert_eq!(snapshot.slots[54], ItemStack::new(items::STONE, 1));
        assert_eq!(snapshot.carried, ItemStack::new(items::STONE, 9));
    }

    /// 官方物品栏下标和菜单槽位的换算。
    #[test]
    fn official_slots_map_to_the_menu_order() {
        assert_eq!(official_slot(0), Some(36));
        assert_eq!(official_slot(8), Some(44));
        assert_eq!(official_slot(9), Some(9));
        assert_eq!(official_slot(35), Some(35));
        assert_eq!(official_slot(36), Some(8));
        assert_eq!(official_slot(39), Some(5));
        assert_eq!(official_slot(40), Some(45));
        assert_eq!(official_slot(41), None);
    }

    /// 一格橡木原木放进工作台，打开之后结果是四块木板，state id 被重算和整包各推进一次。
    #[test]
    fn a_crafting_table_opens_with_planks() {
        let grid = [(items::OAK_LOG, 1)];
        let mut replay = Replay::workstation(ReplayShape::Crafting, &grid, &[], &[], false);
        let snapshot = replay.open();
        assert_eq!(snapshot.slots[0], ItemStack::new(items::OAK_PLANKS, 4));
        assert_eq!(snapshot.slots[1], ItemStack::new(items::OAK_LOG, 1));
        assert_eq!(snapshot.state_id, 2);
    }
}
