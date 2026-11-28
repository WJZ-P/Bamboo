//! 商人菜单的点击：结果格只能通过付款取走，别的点击方式不能绕过，也不能把东西吞掉。
//! 每条都对着官方 `MerchantMenu` / `MerchantResultSlot` / `AbstractContainerMenu.doClick`。
//!
//! Merchant menu clicks: the result only leaves by paying, no click type gets around
//! that, and none of them swallows items. Each case follows vanilla `MerchantMenu`,
//! `MerchantResultSlot` and `AbstractContainerMenu.doClick`.

use mc_bamboo_blocks::entity::{Entity, EntityKind, mob::MobType};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::{
    components::bundle,
    packet::play::{ClientInformation, ContainerInput, GameMode, HashedStack},
};
use mc_bamboo_world::flat;
use uuid::Uuid;

use super::*;
use crate::player::Player;

/// 菜单槽位：0 和 1 是付款格，2 是结果格，3 起是背包 27 格，30 起是快捷栏。
///
/// Menu slots: 0 and 1 pay, 2 is the result, 3.. the 27 inventory slots, 30.. the hotbar.
const PAY_A: i16 = 0;
const PAY_B: i16 = 1;
const RESULT: i16 = 2;
const FIRST_INVENTORY: i16 = 3;

struct Scene {
    region: Region,
    id: PlayerId,
    villager: i32,
}

/// 一颗绿宝石换六个面包，最多 16 次。
///
/// One emerald for six bread, at most 16 times.
fn bread_offer() -> MerchantOffer {
    MerchantOffer {
        base_cost_a: ItemCost::new(items::EMERALD, 1),
        cost_b: None,
        result: ItemStack::new(items::BREAD, 6),
        uses: 0,
        max_uses: 16,
        reward_exp: false,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: 0.05,
        xp: 1,
    }
}

impl Scene {
    /// 玩家和一个只卖面包的村民，交易界面已经打开。
    ///
    /// A player and a villager selling only bread, with the trade screen open.
    fn open() -> Self {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let position = Vec3::new(0.5, f64::from(flat::surface_y()), 0.5);
        region.insert_player(Player::new(
            id,
            Uuid::from_u128(1),
            "trader".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            position,
        ));
        let villager = region
            .entities
            .spawn(Entity::by_mob_type(MobType::Villager, position));
        region.entities.with_mut(villager, |entity| {
            if let EntityKind::Mob(mob) = &mut entity.kind {
                mob.villager.profession = data::profession_id("minecraft:farmer").unwrap();
                mob.villager.level = 1;
                mob.villager.kind = data::type_id("minecraft:plains").unwrap();
                mob.villager.offers = Some(vec![bread_offer()]);
            }
        });
        crate::interaction::on_interact_entity(&mut region, id, villager, Hand::Main, false);
        assert!(region.menus.contains_key(&id), "交易界面应该打开了");
        Self {
            region,
            id,
            villager,
        }
    }

    fn click(&mut self, slot: i16, button: i8, input: ContainerInput) {
        let menu = &self.region.menus[&self.id];
        let packet = ClickPacket {
            container_id: menu.container_id,
            state_id: menu.state_id,
            slot,
            button,
            input,
            changed_slots: Default::default(),
            carried: HashedStack::Empty,
        };
        crate::menu::on_click(&mut self.region, self.id, packet);
    }

    /// 菜单槽位 `slot`（3 起）对应的物品栏格。
    ///
    /// The inventory stack behind menu slot `slot` (3 and up).
    fn inventory(&mut self, slot: i16) -> &mut ItemStack {
        let index = slot as usize - 3 + crate::menu::INVENTORY_MENU_START;
        &mut self
            .region
            .players
            .get_mut(&self.id)
            .expect("玩家")
            .inventory
            .slots[index]
    }

    /// 把一堆物品经过点击放进付款格。
    ///
    /// Put a stack into a payment slot through clicks.
    fn pay(&mut self, slot: i16, stack: ItemStack) {
        *self.inventory(FIRST_INVENTORY) = stack;
        self.click(FIRST_INVENTORY, 0, ContainerInput::Pickup);
        self.click(slot, 0, ContainerInput::Pickup);
    }

    fn state(&self) -> &MerchantMenu {
        self.region.menus[&self.id]
            .merchant
            .as_ref()
            .expect("商人菜单")
    }

    fn carried(&self) -> ItemStack {
        self.region.menus[&self.id].carried().clone()
    }

    fn uses(&self) -> i32 {
        villager_offers(&self.region, self.villager).expect("交易表")[0].uses
    }

    /// 玩家手里一共有多少个 `item`：物品栏、光标、付款格，加上地上的物品实体。
    /// 结果格是预览，不算。
    ///
    /// How many of `item` the player owns: inventory, cursor, payment slots and item
    /// entities on the ground. The result slot is a preview and does not count.
    fn owned(&self, item: i32) -> i32 {
        let player = &self.region.players[&self.id];
        let held = player.inventory.slots.iter();
        let state = self.state();
        let carried = self.carried();
        let ground: i32 = self
            .region
            .entities
            .iter()
            .filter_map(|entity| entity.ground().map(|ground| ground.stack.clone()))
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum();
        held.chain(state.payment.iter())
            .chain(std::iter::once(&carried))
            .filter(|stack| stack.item == item)
            .map(|stack| stack.count)
            .sum::<i32>()
            + ground
    }
}

/// 左键和右键点结果格都把整堆结果拿到光标上，扣一次付款。背包满了也一样，
/// 结果不会因为放不进背包而消失。
///
/// Left and right clicks on the result both put the whole stack on the cursor and
/// charge once. A full inventory changes nothing: the result must not vanish.
#[test]
fn clicking_the_result_puts_the_whole_stack_on_the_cursor() {
    for button in [0, 1] {
        let mut scene = Scene::open();
        scene.pay(PAY_A, ItemStack::new(items::EMERALD, 1));
        assert_eq!(scene.state().result, ItemStack::new(items::BREAD, 6));
        for slot in 3..39 {
            *scene.inventory(slot) = ItemStack::new(items::STONE, 64);
        }

        scene.click(RESULT, button, ContainerInput::Pickup);

        assert_eq!(scene.carried(), ItemStack::new(items::BREAD, 6));
        assert_eq!(scene.owned(items::EMERALD), 0, "付了一颗绿宝石");
        assert_eq!(scene.uses(), 1);
    }
}

/// 双击收集：官方 `MerchantMenu.canTakeItemForPickAll` 恒为假，什么也收不到。
/// 结果格里的预览不能被收进光标。
///
/// Double click: vanilla `MerchantMenu.canTakeItemForPickAll` is always false, so
/// nothing is collected. The result preview must not be swept onto the cursor.
#[test]
fn a_double_click_does_not_collect_the_result() {
    let mut scene = Scene::open();
    scene.pay(PAY_A, ItemStack::new(items::EMERALD, 1));
    *scene.inventory(FIRST_INVENTORY + 1) = ItemStack::new(items::BREAD, 1);
    scene.click(FIRST_INVENTORY + 1, 0, ContainerInput::Pickup);

    scene.click(FIRST_INVENTORY + 2, 0, ContainerInput::PickupAll);

    assert_eq!(scene.owned(items::BREAD), 1, "没付款，面包不能变多");
    assert_eq!(scene.owned(items::EMERALD), 1);
    assert_eq!(scene.uses(), 0);
}

/// Shift 点背包里的物品：官方只在背包和快捷栏之间搬，不会搬进付款格或结果格。
/// 两个付款格都占着时，物品不能掉进结果格被重算冲掉。
///
/// Shift-clicking an inventory stack: vanilla only moves between the inventory and
/// the hotbar, never into the payment or result slots. With both payment slots taken
/// the stack must not land in the result slot and be wiped by the recompute.
#[test]
fn a_shift_click_in_the_inventory_never_enters_the_trade_slots() {
    let mut scene = Scene::open();
    scene.pay(PAY_A, ItemStack::new(items::STICK, 1));
    scene.pay(PAY_B, ItemStack::new(items::STONE, 1));
    *scene.inventory(FIRST_INVENTORY + 4) = ItemStack::new(items::DIAMOND, 5);

    scene.click(FIRST_INVENTORY + 4, 0, ContainerInput::QuickMove);

    assert_eq!(scene.owned(items::DIAMOND), 5);
    assert_eq!(*scene.inventory(30), ItemStack::new(items::DIAMOND, 5));
    assert_eq!(scene.state().payment[0], ItemStack::new(items::STICK, 1));
    assert_eq!(scene.state().payment[1], ItemStack::new(items::STONE, 1));
}

/// 拖动分配扫过结果格：`MerchantResultSlot.mayPlace` 为假，结果格不收。
///
/// A drag across the result slot: `MerchantResultSlot.mayPlace` is false, so the
/// result slot takes nothing.
#[test]
fn a_drag_does_not_place_into_the_result_slot() {
    let mut scene = Scene::open();
    *scene.inventory(FIRST_INVENTORY) = ItemStack::new(items::DIAMOND, 2);
    scene.click(FIRST_INVENTORY, 0, ContainerInput::Pickup);

    scene.click(-999, 0, ContainerInput::QuickCraft);
    scene.click(RESULT, 1, ContainerInput::QuickCraft);
    scene.click(FIRST_INVENTORY + 5, 1, ContainerInput::QuickCraft);
    scene.click(-999, 2, ContainerInput::QuickCraft);

    assert_eq!(scene.owned(items::DIAMOND), 2);
}

/// Shift 点结果格：`doClick` 的 QUICK_MOVE 循环一直交易到付款不够为止。
///
/// Shift-clicking the result: the QUICK_MOVE loop of `doClick` trades until the
/// payment runs out.
#[test]
fn a_shift_click_on_the_result_trades_until_the_payment_runs_out() {
    let mut scene = Scene::open();
    scene.pay(PAY_A, ItemStack::new(items::EMERALD, 3));

    scene.click(RESULT, 0, ContainerInput::QuickMove);

    assert_eq!(scene.owned(items::BREAD), 18);
    assert_eq!(scene.owned(items::EMERALD), 0);
    assert_eq!(scene.uses(), 3);
    assert!(scene.carried().is_empty());
}

/// 光标上拿着收纳袋点结果格：结果进袋子，照样要付款。
///
/// A bundle on the cursor clicking the result: the result enters the bundle and the
/// trade is still paid for.
#[test]
fn a_bundle_taking_the_result_pays_for_it() {
    let mut scene = Scene::open();
    scene.pay(PAY_A, ItemStack::new(items::EMERALD, 1));
    *scene.inventory(FIRST_INVENTORY + 1) = ItemStack::new(items::BUNDLE, 1);
    scene.click(FIRST_INVENTORY + 1, 0, ContainerInput::Pickup);

    scene.click(RESULT, 0, ContainerInput::Pickup);

    let inside: i32 = bundle::contents(&scene.carried())
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.item == items::BREAD)
        .map(|entry| entry.count)
        .sum();
    assert_eq!(inside, 6);
    assert_eq!(scene.owned(items::EMERALD), 0);
    assert_eq!(scene.uses(), 1);
}

/// Q 丢结果格：整堆丢到地上，扣一次付款。
///
/// Q on the result: the whole stack is thrown and the trade is charged once.
#[test]
fn throwing_the_result_drops_it_and_pays() {
    let mut scene = Scene::open();
    scene.pay(PAY_A, ItemStack::new(items::EMERALD, 1));

    scene.click(RESULT, 0, ContainerInput::Throw);

    assert_eq!(scene.owned(items::BREAD), 6);
    assert!(scene.carried().is_empty());
    assert_eq!(scene.owned(items::EMERALD), 0);
    assert_eq!(scene.uses(), 1);
}
