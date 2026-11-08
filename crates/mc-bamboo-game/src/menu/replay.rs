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
    inbound::PlayerId,
    inventory::{HOTBAR_START, Inventory, OFF_HAND},
};

/// 重放里只有一名玩家，容器里的东西都算它放的。
const VIEWER: PlayerId = PlayerId(0);

/// 菜单对应的方块种类。箱子要带行数，双箱是 6 行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayShape {
    Chest(usize),
    Hopper,
    Dispenser,
    ShulkerBox,
}

impl ReplayShape {
    fn kind(self) -> ContainerKind {
        match self {
            Self::Chest(_) => ContainerKind::Chest,
            Self::Hopper => ContainerKind::Hopper,
            Self::Dispenser => ContainerKind::Dispenser,
            Self::ShulkerBox => ContainerKind::ShulkerBox,
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
}

/// 一个打开着的菜单，只有点击逻辑，没有世界。
pub struct Replay {
    menu: Menu,
    container: Vec<ItemStack>,
    /// 每个容器格的组件来自哪名玩家。重放里只有一名玩家，所以都是它。
    sources: Vec<Option<PlayerId>>,
    inventory: Inventory,
    creative: bool,
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
            sources: vec![Some(VIEWER); size],
            inventory: player,
            creative: true,
        }
    }

    /// 非创造模式时中键克隆什么也不做。
    pub fn set_creative(&mut self, creative: bool) {
        self.creative = creative;
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
    pub fn open(&mut self) -> Snapshot {
        let slots = self.menu_slots();
        self.menu.take_full_state(&slots);
        self.snapshot()
    }

    /// 点一下。槽位不合法时和官方一样什么也不做。
    pub fn click(&mut self, slot: i16, button: i8, input: ContainerInput) -> Snapshot {
        if !self.menu.valid_slot_index(slot) {
            return self.snapshot();
        }
        let mut ctx = Ctx {
            kind: self.menu.kind,
            container_size: self.menu.container_size,
            viewer: VIEWER,
            container: &mut self.container,
            sources: &mut self.sources,
            inventory: &mut self.inventory,
            carried: &mut self.menu.carried,
            quickcraft_status: &mut self.menu.quickcraft_status,
            quickcraft_type: &mut self.menu.quickcraft_type,
            quickcraft_slots: &mut self.menu.quickcraft_slots,
            creative: self.creative,
        };
        super::do_click(&mut ctx, i32::from(slot), i32::from(button), input);
        // 客户端从不预测，回传的总是空光标。
        self.menu.remote_carried.receive(HashedStack::Empty);
        let slots = self.menu_slots();
        self.menu.take_slot_changes(&slots);
        self.menu.take_carried_change();
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
        let mut ctx = Ctx {
            kind: self.menu.kind,
            container_size: self.menu.container_size,
            viewer: VIEWER,
            container: &mut self.container,
            sources: &mut self.sources,
            inventory: &mut self.inventory,
            carried: &mut self.menu.carried,
            quickcraft_status: &mut self.menu.quickcraft_status,
            quickcraft_type: &mut self.menu.quickcraft_type,
            quickcraft_slots: &mut self.menu.quickcraft_slots,
            creative: self.creative,
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
        Snapshot {
            slots: self.menu_slots(),
            carried: self.menu.carried.clone(),
            state_id: self.menu.state_id,
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
}
