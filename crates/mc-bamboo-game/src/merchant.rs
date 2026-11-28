//! 村民交易：`AbstractVillager` 的懒生成交易表、`MerchantMenu` 的三格和一次真实交易。
//!
//! 一名玩家右键有职业的村民时打开菜单。玩家的 36 格物品栏由 [`crate::menu`]
//! 的点击逻辑管理，这里只管付款格 0 / 1 和结果格 2：按选中的交易算出结果堆，
//! 取出结果时扣两格付款、累加使用次数、发经验和升级。4 格距离之外或村民死了
//! 菜单就失效。

use mc_bamboo_blocks::InteractionResult;
use mc_bamboo_blocks::RandomSource;
use mc_bamboo_blocks::entity::mob::MobType;
use mc_bamboo_blocks::entity::mob::villager::{self, Villager};
use mc_bamboo_core::Vec3;
use mc_bamboo_data::{registries, villager as data};
use mc_bamboo_loot::{
    Position, RandomSequences,
    trade::{GeneratedOffer, TradeParams, TradeSet, add_offers_from_trade_set},
};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, PatchEntry,
        archive::{components, read},
        cursor::Registry,
    },
    item::ItemStack,
    merchant::{ItemCost, MerchantOffer},
    packet::play::{MerchantOffersPacket, OpenScreen, Sound},
    text::Component,
};
use serde_json::Value;

use crate::{
    inbound::PlayerId,
    loot_stacks,
    menu::{ClickPacket, Menu},
    region::Region,
};

/// `AbstractVillager.stillValid` 的额外交互距离。
const INTERACTION_BUFFER: f64 = 4.0;

/// 一个商人菜单的全部状态，存在 [`Menu`] 里。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MerchantMenu {
    pub(crate) villager: i32,
    pub(crate) selection_hint: i32,
    /// 两个付款格；结果格由 `slots_changed` 每次重算。
    pub(crate) payment: [ItemStack; 2],
    pub(crate) result: ItemStack,
    pub(crate) active: Option<usize>,
}

/// 生成（或取回）村民的交易表。官方 `getOffers()` 在 `offers == null` 时
/// 调 `updateTrades(level)`，等级从 1 开始逐级补齐到当前等级。
pub(crate) fn ensure_offers(
    villager: &mut mc_bamboo_blocks::entity::mob::villager::Villager,
    kind: MobType,
    position: Vec3,
    sequences: &RandomSequences,
) {
    if villager.offers.is_none() {
        let mut offers = Vec::new();
        // `WanderingTrader.updateTrades`：buying → uncommon → common 三张交易集。
        // `WanderingTrader.updateTrades`: the buying, uncommon and common sets.
        if kind == MobType::WanderingTrader {
            for name in WANDERING_TRADE_SETS {
                append_trade_set(&mut offers, name, villager, position, sequences);
            }
        } else {
            for level in 1..=villager.level.clamp(1, 5) {
                let Some(profession) = data::profession(villager.profession) else {
                    break;
                };
                let name = profession.trade_sets_by_level[(level - 1) as usize];
                if name.is_empty() {
                    continue;
                }
                let Ok(set) = TradeSet::parse(name) else {
                    continue;
                };
                let params = TradeParams {
                    world_seed: 0,
                    origin: Position::new(position.x, position.y, position.z),
                    villager_type: data::type_name(villager.kind).unwrap_or(data::DEFAULT_TYPE),
                };
                let fallback = mc_bamboo_blocks::random::LegacyRandom::new(0);
                let generated = generate_offers(&set, &params, sequences, &fallback);
                for offer in &generated {
                    if let Some(offer) = generated_offer_to_merchant(offer) {
                        offers.push(offer);
                    }
                }
            }
        }
        villager.offers = Some(offers);
    }
}

/// 官方 `TradeSets.WANDERING_TRADER_*` 的注册名，按 `updateTrades` 的追加顺序。
///
/// The `TradeSets.WANDERING_TRADER_*` names in `updateTrades` order.
pub const WANDERING_TRADE_SETS: [&str; 3] = [
    "minecraft:wandering_trader/buying",
    "minecraft:wandering_trader/uncommon",
    "minecraft:wandering_trader/common",
];

/// 求一张 `trade_set` 并把生成的交易追加到协议层的交易表。
fn append_trade_set(
    offers: &mut Vec<MerchantOffer>,
    name: &str,
    villager: &mc_bamboo_blocks::entity::mob::villager::Villager,
    position: Vec3,
    sequences: &RandomSequences,
) {
    let Ok(set) = TradeSet::parse(name) else {
        return;
    };
    let params = TradeParams {
        world_seed: 0,
        origin: Position::new(position.x, position.y, position.z),
        villager_type: data::type_name(villager.kind).unwrap_or(data::DEFAULT_TYPE),
    };
    let fallback = mc_bamboo_blocks::random::LegacyRandom::new(0);
    for offer in generate_offers(&set, &params, sequences, &fallback) {
        if let Some(offer) = generated_offer_to_merchant(&offer) {
            offers.push(offer);
        }
    }
}

/// 一个 `trade_set` 求值成网络包里的交易表。
fn generate_offers(
    set: &TradeSet,
    params: &TradeParams,
    sequences: &RandomSequences,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
) -> Vec<GeneratedOffer> {
    let mut offers = Vec::new();
    let _ = add_offers_from_trade_set(set, params, Some(sequences), fallback, &mut offers);
    offers
}

/// 把 loot 侧求值的交易转成协议层的 `MerchantOffer`。
fn generated_offer_to_merchant(offer: &GeneratedOffer) -> Option<MerchantOffer> {
    Some(MerchantOffer {
        base_cost_a: generated_cost(&offer.cost_a)?,
        cost_b: offer.cost_b.as_ref().and_then(generated_cost),
        result: loot_stacks::loot_stack(&offer.result),
        uses: 0,
        max_uses: offer.max_uses,
        reward_exp: true,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: offer.price_multiplier,
        xp: offer.xp,
    })
}

fn generated_cost(price: &mc_bamboo_loot::trade::TradePrice) -> Option<ItemCost> {
    let mut components = Vec::new();
    for (name, value) in &price.components {
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, name)?;
        let tag = json_to_tag(value).to_tag();
        let value = components::decode_value(kind, &tag, 0).ok()?;
        components.push(mc_bamboo_protocol::components::predicate::TypedComponent { kind, value });
    }
    Some(ItemCost {
        item: price.item,
        count: price.count,
        components,
    })
}

fn json_to_tag(value: &Value) -> mc_bamboo_protocol::components::archive::Dyn {
    use mc_bamboo_protocol::components::archive::Dyn;
    match value {
        Value::Null => Dyn::Byte(0),
        Value::Bool(value) => Dyn::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                if let Ok(value) = i8::try_from(value) {
                    return Dyn::Byte(value);
                }
                if let Ok(value) = i16::try_from(value) {
                    return Dyn::Short(value);
                }
                if let Ok(value) = i32::try_from(value) {
                    return Dyn::Int(value);
                }
                return Dyn::Long(value);
            }
            number.as_f64().map_or(Dyn::Double(0.0), |value| {
                let as_f32 = value as f32;
                if f64::from(as_f32) == value {
                    Dyn::Float(as_f32)
                } else {
                    Dyn::Double(value)
                }
            })
        }
        Value::String(value) => Dyn::String(value.clone()),
        Value::Array(values) => Dyn::List(values.iter().map(json_to_tag).collect()),
        Value::Object(entries) => Dyn::Map(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), json_to_tag(value)))
                .collect(),
        ),
    }
}

/// `Villager.mobInteract`。返回 `None` 表示这次点击不消费（非村民、睡觉、已在交易），
/// 走通用 `mobInteract`。幼年摇头、`NONE` 职业摇头 + 消耗、有职业打开交易界面。
///
/// `Villager.mobInteract`. `None` means the click is not consumed (not a villager,
/// sleeping, or already trading) and falls through to the generic mob interaction.
/// A baby shakes its head; a `NONE` villager shakes its head and consumes the click;
/// a professional villager opens the trading screen.
pub(crate) fn interact(
    region: &mut Region,
    player: PlayerId,
    villager_id: i32,
    main_hand: bool,
) -> Option<InteractionResult> {
    if !region.players.contains_key(&player) {
        return None;
    }
    let public = region.entities.public_at(villager_id)?;
    if public.removed || !matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_)) {
        return None;
    }
    let position = public.position;
    // 懒生成交易表，顺便拿到幼年标志。睡觉 / 交易中走通用路径。
    let (villager, offers, baby, kind) = {
        let entity = region.entities.get(villager_id)?;
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind else {
            return None;
        };
        let kind = mob.mob_type;
        if !matches!(kind, MobType::Villager | MobType::WanderingTrader) {
            return None;
        }
        if mob.villager.is_trading() || (kind == MobType::Villager && mob.villager.sleeping) {
            return None;
        }
        let baby = mob.is_baby();
        // `WanderingTrader.mobInteract` 在幼年时落到 `AbstractVillager.mobInteract`，
        // 不能开交易界面；村民幼年由下面 `setUnhappy` 摇头。
        // `WanderingTrader.mobInteract` falls through on a baby; baby villagers shake
        // their heads through `setUnhappy` below.
        if baby && kind == MobType::WanderingTrader {
            return None;
        }
        let mut villager = mob.villager.clone();
        ensure_offers(&mut villager, kind, position, &region.loot_sequences);
        let offers = villager.offers.clone().unwrap_or_default();
        (villager, offers, baby, kind)
    };
    if baby {
        set_unhappy(region, villager_id, position);
        return Some(InteractionResult::Success);
    }
    // `Stats.TALKED_TO_VILLAGER` 只在主手交互时记，村民与流浪商人共用。
    // `Stats.TALKED_TO_VILLAGER` is awarded on a main-hand interaction by both kinds.
    if main_hand {
        crate::item_use_stats::award_custom(
            region,
            player,
            registries::custom_stat::TALKED_TO_VILLAGER,
            1,
        );
    }
    if offers.is_empty() {
        if kind == MobType::Villager {
            set_unhappy(region, villager_id, position);
        }
        return Some(InteractionResult::Consume);
    }
    open_screen(region, player, villager_id, villager, kind);
    Some(InteractionResult::Success)
}

/// `Villager.setUnhappy`：`setUnhappyCounter(40)` + `makeSound(VILLAGER_NO)`。
/// 摇头动画由客户端按 `DATA_UNHAPPY_COUNTER` 播放；计数递减在村民大脑里做。
///
/// `Villager.setUnhappy`: `setUnhappyCounter(40)` plus `makeSound(VILLAGER_NO)`.
/// The client plays the head-shake from `DATA_UNHAPPY_COUNTER`; the countdown
/// runs in the villager brain.
fn set_unhappy(region: &mut Region, villager_id: i32, position: Vec3) {
    region.entities.with_mut(villager_id, |entity| {
        if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
            mob.villager.unhappy = 40;
        }
    });
    crate::interaction::send_sound(
        region,
        None,
        &Sound {
            sound: i32::from(mc_bamboo_data::sounds::sound_event(
                "minecraft:entity.villager.no",
            )),
            inline: None,
            source: mc_bamboo_data::sounds::NEUTRAL,
            x: position.x,
            y: position.y,
            z: position.z,
            volume: 1.0,
            pitch: 1.0,
            seed: region.sound_seed_generator.source().next_long(),
        },
    );
}

/// 有职业的成年村民：把生成好的交易表写回、发 `open_screen` 和交易列表、装好菜单。
///
/// `Villager.startTrading` + `openTradingScreen`: write back the generated offers,
/// send the screen and offers, and install the menu.
fn open_screen(
    region: &mut Region,
    player: PlayerId,
    villager_id: i32,
    mut villager: Villager,
    kind: MobType,
) {
    let hero_amplifier = region.players.get(&player).and_then(|p| {
        p.effects
            .get(registries::mob_effect::HERO_OF_THE_VILLAGE)
            .map(|instance| instance.amplifier)
    });
    let level = villager.level;
    let xp = villager.xp;
    let trader = kind == MobType::WanderingTrader;
    let title = if trader {
        // `WanderingTrader.mobInteract` 传 `getDisplayName()`，翻译键是实体名。
        // `WanderingTrader.mobInteract` passes `getDisplayName()`, the entity name.
        Component::translate("entity.minecraft.wandering_trader", [])
    } else {
        let name = data::profession(villager.profession)
            .map(|p| p.name)
            .unwrap_or("none");
        Component::translate(format!("entity.minecraft.villager.{name}"), [])
    };
    // `Villager.startTrading`：`updateSpecialPrices(player)` 在 `setTradingPlayer` 之前。
    // `Villager.startTrading`: `updateSpecialPrices(player)` precedes `setTradingPlayer`.
    // 流浪商人没有声望，`updateSpecialPrices` 是村民的覆写。
    // Wandering traders have no reputation; `updateSpecialPrices` is a villager override.
    if !trader {
        villager.update_special_prices(hero_amplifier);
    }
    villager.set_trading_player(Some(player.0));
    let offers = villager.offers.clone().unwrap_or_default();
    region.entities.with_mut(villager_id, |entity| {
        if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
            mob.villager = villager;
        }
    });
    if region.menus.contains_key(&player) {
        crate::menu::close_menu(region, player);
    }
    let container_id = {
        let Some(p) = region.players.get_mut(&player) else {
            return;
        };
        p.container_counter = p.container_counter % 100 + 1;
        p.container_counter
    };
    let menu = Menu::merchant(container_id, villager_id);
    region.menus.insert(player, menu);
    region.links.send(
        player,
        &OpenScreen {
            container_id,
            menu_type: registries::menu::MERCHANT,
            title: &title,
        },
    );
    region.links.send(
        player,
        &MerchantOffersPacket {
            container_id,
            offers: &offers,
            villager_level: if trader { 1 } else { level },
            villager_xp: if trader { 0 } else { xp },
            // `WanderingTrader.showProgressBar()` = false；`MerchantMenu.canRestock`
            // 由 `Merchant.canRestock()` 给，流浪商人不继承村民的补货。
            // `WanderingTrader.showProgressBar()` is false; it has no restocking.
            show_progress: !trader,
            can_restock: !trader,
        },
    );
    crate::menu::broadcast_merchant_state(region, player);
}

/// `ServerboundSelectTradePacket`：记下选中的交易、重算结果，再把背包里匹配的
/// 付款物品自动填进付款格（`MerchantMenu.tryMoveItems`）。
///
/// `ServerboundSelectTradePacket`: record the selection, recompute the result, then
/// auto-fill the payment slots from the inventory (`MerchantMenu.tryMoveItems`).
pub(crate) fn on_select_trade(region: &mut Region, player: PlayerId, slot: i32) {
    let Some(mut menu) = region.menus.remove(&player) else {
        return;
    };
    if let Some(state) = menu.merchant.as_mut() {
        state.selection_hint = slot.max(0);
        try_move_items(region, player, &mut menu, slot);
        slots_changed(region, player, &mut menu);
    }
    region.menus.insert(player, menu);
}

/// `MerchantMenu.tryMoveItems(slot)`：先把旧的付款搬回背包，两格都空时按选中交易的
/// A / B 成本从背包自动填付款格。
///
/// `MerchantMenu.tryMoveItems(slot)`: move any existing payment back to the inventory,
/// then auto-fill both payment slots from the inventory when they are empty.
fn try_move_items(region: &mut Region, player: PlayerId, menu: &mut Menu, slot: i32) {
    if slot < 0 {
        return;
    }
    let Some(villager_id) = menu.target.villager() else {
        return;
    };
    let Some(offers) = villager_offers(region, villager_id) else {
        return;
    };
    if slot as usize >= offers.len() {
        return;
    }
    let Some(player_data) = region.players.get_mut(&player) else {
        return;
    };
    let inventory = &mut player_data.inventory;
    let Some(state) = menu.merchant.as_mut() else {
        return;
    };
    // 旧的付款搬回背包；搬不动就停下，不自动填。
    for payment in [0, 1] {
        if state.payment[payment].is_empty() {
            continue;
        }
        if !move_payment_to_inventory(inventory, &mut state.payment[payment]) {
            return;
        }
    }
    if state.payment[0].is_empty() && state.payment[1].is_empty() {
        let offer = &offers[slot as usize];
        move_from_inventory_to_payment(inventory, &mut state.payment[0], &offer.base_cost_a);
        if let Some(cost) = &offer.cost_b {
            move_from_inventory_to_payment(inventory, &mut state.payment[1], cost);
        }
    }
}

/// `moveItemStackTo(stack, 3, 39, true)`：把付款格的一堆搬进玩家 36 格，从后往前。
/// 返回是否搬动了任何东西；没搬动时 `tryMoveItems` 停下，不自动填。
///
/// `moveItemStackTo(stack, 3, 39, true)`: move a payment stack into the player's 36
/// slots, back to front. Returns whether anything moved; a failure stops `tryMoveItems`.
pub(crate) fn move_payment_to_inventory(
    inventory: &mut crate::inventory::Inventory,
    stack: &mut ItemStack,
) -> bool {
    let mut moved = false;
    if stack.is_stackable() {
        for index in (crate::menu::INVENTORY_MENU_START..crate::inventory::OFF_HAND).rev() {
            let target = &mut inventory.slots[index];
            if target.is_empty() || !target.is_same_item_same_components(stack) {
                continue;
            }
            let max = target.max_stack_size();
            let total = target.count + stack.count;
            if total <= max {
                target.count = total;
                *stack = ItemStack::empty();
                return true;
            }
            if target.count < max {
                let added = max - target.count;
                stack.count -= added;
                target.count = max;
                moved = true;
            }
        }
    }
    if !stack.is_empty() {
        for index in (crate::menu::INVENTORY_MENU_START..crate::inventory::OFF_HAND).rev() {
            if inventory.slots[index].is_empty() {
                inventory.slots[index] = std::mem::take(stack);
                return true;
            }
        }
    }
    moved
}

/// `moveFromInventoryToPaymentSlot`：在背包里找匹配 `cost` 的堆，填进付款格。
pub(crate) fn move_from_inventory_to_payment(
    inventory: &mut crate::inventory::Inventory,
    payment: &mut ItemStack,
    cost: &ItemCost,
) {
    for index in crate::menu::INVENTORY_MENU_START..crate::inventory::OFF_HAND {
        let stack = &mut inventory.slots[index];
        if stack.is_empty() || !cost_matches(cost, stack) {
            continue;
        }
        if !payment.is_empty() && !payment.is_same_item_same_components(stack) {
            continue;
        }
        let max = stack.max_stack_size();
        let amount = (max - payment.count).min(stack.count);
        let mut combined = stack.clone();
        combined.count = payment.count + amount;
        stack.count -= amount;
        if stack.count <= 0 {
            *stack = ItemStack::empty();
        }
        let filled = combined.count;
        *payment = combined;
        if filled < max {
            continue;
        }
        return;
    }
}

/// 玩家点击了商人菜单的槽位。每一格、每一种点击都走 `AbstractContainerMenu.doClick`
/// 那套通用逻辑：结果格在那里按 `MerchantResultSlot` 的规则取出并成交，别的点击方式
/// （双击、拖动、Shift、数字键、收纳袋）也就绕不过付款。
///
/// A click in the merchant menu. Every slot and every click type runs the shared
/// `AbstractContainerMenu.doClick` logic, where the result slot follows
/// `MerchantResultSlot` and completes the trade, so no click type (double click, drag,
/// shift, number key, bundle) gets around paying.
pub(crate) fn on_click(region: &mut Region, player: PlayerId, packet: ClickPacket) -> bool {
    let Some(menu) = region.menus.get(&player) else {
        return false;
    };
    if menu.merchant.is_none() {
        return false;
    }
    if menu.container_id != packet.container_id {
        return false;
    }
    crate::menu::on_click_default(region, player, packet);
    true
}

/// `MerchantContainer.updateSellItem`：付款格 0 空就用 1；按 `getRecipeFor(a, b, hint)`
/// 找交易，不中或卖光再试 `getRecipeFor(b, a, hint)`（付款两格对调）。
///
/// `MerchantContainer.updateSellItem`: fall back to slot 1 when slot 0 is empty;
/// look up `getRecipeFor(a, b, hint)` and retry with the two payments swapped when
/// the hint misses or is out of stock.
pub(crate) fn slots_changed(region: &Region, _player: PlayerId, menu: &mut Menu) {
    let Some(state) = menu.merchant.as_mut() else {
        return;
    };
    state.active = None;
    state.result = ItemStack::empty();
    let Some(villager_id) = menu.target.villager() else {
        return;
    };
    let Some(offers) = villager_offers(region, villager_id) else {
        return;
    };
    let a = state.payment[0].clone();
    let b = state.payment[1].clone();
    let Some(index) = active_offer_index(&offers, &a, &b, state.selection_hint) else {
        return;
    };
    state.active = Some(index);
    state.result = offers[index].result.clone();
}

/// `MerchantContainer.updateSellItem` 的纯逻辑：付款格 0 空就用 1，按
/// `getRecipeFor(a, b, hint)` 找交易，不中或卖光再试对调的两格。返回应显示的交易下标。
///
/// The pure `MerchantContainer.updateSellItem`: fall back to slot 1 when slot 0 is
/// empty, look up `getRecipeFor(a, b, hint)`, and retry with the payments swapped.
/// Returns the offer index whose result should be shown.
pub(crate) fn active_offer_index(
    offers: &[MerchantOffer],
    a: &ItemStack,
    b: &ItemStack,
    hint: i32,
) -> Option<usize> {
    let (a, b) = if a.is_empty() {
        (b, &ItemStack::empty())
    } else {
        (a, b)
    };
    if a.is_empty() || offers.is_empty() {
        return None;
    }
    let hint = hint as usize;
    let mut index = get_recipe_for(offers, a, b, hint);
    if index.is_none_or(|i| offers[i].is_out_of_stock()) {
        index = get_recipe_for(offers, b, a, hint);
    }
    index.filter(|&i| !offers[i].is_out_of_stock())
}

/// `MerchantOffers.getRecipeFor(a, b, hint)`：hint 有效时只看那一条，不中直接 `None`。
pub(crate) fn get_recipe_for(
    offers: &[MerchantOffer],
    a: &ItemStack,
    b: &ItemStack,
    hint: usize,
) -> Option<usize> {
    if hint > 0 && hint < offers.len() {
        return satisfied_by(&offers[hint], a, b).then_some(hint);
    }
    offers.iter().position(|offer| satisfied_by(offer, a, b))
}

/// 只读地取出村民的交易表。
pub(crate) fn villager_offers(region: &Region, villager_id: i32) -> Option<Vec<MerchantOffer>> {
    region.entities.public_at(villager_id)?;
    // 实体表的公开行和完整行都在同一个 store；这里走只读快照。
    let mut store: Option<Vec<MerchantOffer>> = None;
    let _ = &mut store;
    if let Some(entity) = region.entities.get(villager_id)
        && let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind
    {
        store = mob.villager.offers.clone();
    }
    store
}

/// 一次点击里成交的每一笔（交易下标和卖出的物品）在村民这边的后果，最后把新的
/// 交易表发给客户端。付款和结果物品已经由点击逻辑搬完了。
///
/// The villager side of every trade one click completed (offer index and item
/// sold), followed by one refreshed offer list for the client. The click logic has
/// already moved the payment and the result.
pub(crate) fn complete_trades(
    region: &mut Region,
    player: PlayerId,
    menu: &Menu,
    trades: &[(usize, ItemStack)],
) {
    let Some(villager_id) = menu.target.villager() else {
        return;
    };
    if trades.is_empty() {
        return;
    }
    for (index, result) in trades {
        complete_trade(region, player, villager_id, *index, result);
    }
    // 交易完成后把新的使用次数 / 等级 / 交易表发给客户端，`uses` 变化会改「售罄」状态。
    // After trading, resend the updated offers: `uses` changes the out-of-stock state
    // and a level-up appends a new tier.
    send_offers(region, player, menu);
}

/// `AbstractVillager.notifyTrade` 加 `MerchantResultSlot.onTake` 里和村民、统计有关的
/// 部分：增加使用次数、给经验、可能升级、进度和统计。
///
/// `AbstractVillager.notifyTrade` plus the villager and stat parts of
/// `MerchantResultSlot.onTake`: count the use, reward experience, maybe level up, and
/// record the advancement and the stat.
fn complete_trade(
    region: &mut Region,
    player: PlayerId,
    villager_id: i32,
    index: usize,
    result: &ItemStack,
) {
    let mut reward_exp = false;
    let mut orb_amount = 0;
    let mut advancement_villager = "minecraft:villager";
    region.entities.with_mut(villager_id, |entity| {
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind else {
            return;
        };
        // `CriteriaTriggers.TRADE` 里的 `type` 是实体的注册表 ID。
        // `CriteriaTriggers.TRADE`'s `type` is the entity's registry id.
        let trader = mob.mob_type == MobType::WanderingTrader;
        advancement_villager = if trader {
            "minecraft:wandering_trader"
        } else {
            "minecraft:villager"
        };
        let offer_xp = {
            let Some(offers) = mob.villager.offers.as_mut() else {
                return;
            };
            let Some(offer) = offers.get_mut(index) else {
                return;
            };
            // `AbstractVillager.notifyTrade` → `MerchantOffer.increaseUses()`。
            offer.uses += 1;
            reward_exp = offer.reward_exp;
            offer.xp
        };
        // `rewardTradeXp` 先按村民自己的随机源抽经验球数量。
        // `rewardTradeXp` draws the orb count from the villager's own random first.
        let random = mc_bamboo_blocks::entity::EntityRandom(std::cell::RefCell::new(entity.random));
        let orb_base = 3 + random.next_int(4);
        entity.random = random.0.into_inner();
        let profession = mob.villager.profession;
        let kind = mob.villager.kind;
        let position = entity.position;
        let next_level = mob.villager.level + 1;
        // `increaseMerchantCareer` 追加的是新等级这一套交易；只在升级时求值（blocks 不依赖 loot）。
        // `increaseMerchantCareer` appends the new tier's offers; evaluated only on a
        // level-up (blocks does not depend on loot).
        let new_offers = || {
            generate_level_offers(
                &region.loot_sequences,
                profession,
                kind,
                position,
                next_level,
            )
        };
        if trader {
            // `WanderingTrader.rewardTradeXp`: 3 + nextInt(4) 经验球，不升级、不加
            // 经验也不改 `VillagerData`；`AbstractVillager.notifyTrade` 仍压住环境音。
            // `WanderingTrader.rewardTradeXp`: 3 + nextInt(4) orbs, no levelling.
            mob.ambient_sound_time = -mob.mob_type.ambient_sound_interval();
            orb_amount = orb_base;
        } else {
            let outcome = villager::notify_trade(mob, offer_xp, orb_base, new_offers);
            orb_amount = outcome.orb_amount;
        }
    });
    crate::advancement_triggers::record(
        region,
        player,
        "minecraft:villager_trade",
        serde_json::json!({
            "item": mc_bamboo_loot::item_name(result.item).unwrap_or(""),
            "villager": {"type": advancement_villager}
        }),
    );
    spawn_trade_xp(region, villager_id, orb_amount, reward_exp);
    // `Stats.TRADED_WITH_VILLAGER`：自定义统计，随 `RequestStats` 一起发。
    crate::item_use_stats::award_custom(
        region,
        player,
        registries::custom_stat::TRADED_WITH_VILLAGER,
        1,
    );
}

/// `Villager.updateTrades` 里 `addOffersFromTradeSet(..., profession.getTrades(level))`：
/// 按新等级的交易集生成要追加的 `MerchantOffer`。只在升级时调用，避免非升级交易推进
/// loot 随机序列。
///
/// The `addOffersFromTradeSet(..., profession.getTrades(level))` half of
/// `Villager.updateTrades`: generate the `MerchantOffer`s to append for the new tier.
/// Called only on a level-up so non-levelling trades do not advance loot RNG.
fn generate_level_offers(
    sequences: &RandomSequences,
    profession: i32,
    kind: i32,
    position: Vec3,
    level: i32,
) -> Vec<MerchantOffer> {
    let Some(prof) = data::profession(profession) else {
        return Vec::new();
    };
    let Some(name) = prof.trade_sets_by_level.get((level - 1) as usize) else {
        return Vec::new();
    };
    if name.is_empty() {
        return Vec::new();
    }
    let Ok(set) = TradeSet::parse(name) else {
        return Vec::new();
    };
    let params = TradeParams {
        world_seed: 0,
        origin: Position::new(position.x, position.y, position.z),
        villager_type: data::type_name(kind).unwrap_or(data::DEFAULT_TYPE),
    };
    let fallback = mc_bamboo_blocks::random::LegacyRandom::new(0);
    let mut generated = Vec::new();
    let _ = add_offers_from_trade_set(&set, &params, Some(sequences), &fallback, &mut generated);
    generated
        .iter()
        .filter_map(generated_offer_to_merchant)
        .collect()
}

/// `Villager.rewardTradeXp`：`rewardExp` 为真时在村民头顶 `(x, y+0.5, z)` 撒
/// `amount` 点经验球。数量已经由调用方按村民自己的随机源算好（升级再 +5）。
///
/// `Villager.rewardTradeXp`: if `rewardExp`, drop `amount` experience at the villager's
/// head. The amount was already drawn from the villager's own random by the caller
/// (plus 5 on a level-up).
fn spawn_trade_xp(region: &mut Region, villager_id: i32, amount: i32, reward_exp: bool) {
    if !reward_exp {
        return;
    }
    let Some(public) = region.entities.public_at(villager_id) else {
        return;
    };
    let position = Vec3::new(
        public.position.x,
        public.position.y + 0.5,
        public.position.z,
    );
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        mc_bamboo_blocks::entity::award(&mut world, position, amount);
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

/// `MerchantOffer.getModifiedCostCount`。
pub(crate) fn cost_count(offer: &MerchantOffer, cost: &ItemCost) -> i32 {
    let demand = (cost.count * offer.demand) as f32 * offer.price_multiplier;
    let extra = demand.floor().max(0.0) as i32;
    (cost.count + extra + offer.special_price_diff).clamp(1, cost_stack_max(cost))
}

fn cost_stack_max(cost: &ItemCost) -> i32 {
    mc_bamboo_data::items::ITEMS
        .get(cost.item as usize)
        .map_or(99, |item| item.max_stack_size)
}

/// `MerchantOffer.satisfiedBy`：A 按 `getModifiedCostCount` 比数量，B 按原始 `count`。
/// 官方 B 的价格不打折（`getCostB` 是 `costB.itemStack()`，数量不变）。
///
/// `MerchantOffer.satisfiedBy`: A uses `getModifiedCostCount`; B uses the raw
/// `count` — vanilla does not discount the second cost (`getCostB` is `costB.itemStack()`).
pub(crate) fn satisfied_by(offer: &MerchantOffer, a: &ItemStack, b: &ItemStack) -> bool {
    if !cost_matches(&offer.base_cost_a, a) || a.count < cost_count(offer, &offer.base_cost_a) {
        return false;
    }
    match &offer.cost_b {
        Some(cost) => cost_matches(cost, b) && b.count >= cost.count,
        None => b.is_empty(),
    }
}

/// `MerchantOffer.take(a, b)`：先 `satisfiedBy`，通过就从 `a` 扣 A、从 `b` 扣 B。
///
/// `MerchantOffer.take(a, b)`: check `satisfiedBy`, then deduct A from `a` and B from `b`.
pub(crate) fn offer_take(offer: &MerchantOffer, a: &mut ItemStack, b: &mut ItemStack) -> bool {
    if !satisfied_by(offer, a, b) {
        return false;
    }
    a.count -= cost_count(offer, &offer.base_cost_a);
    if a.count <= 0 {
        *a = ItemStack::empty();
    }
    if let Some(cost) = &offer.cost_b {
        b.count -= cost.count;
        if b.count <= 0 {
            *b = ItemStack::empty();
        }
    }
    true
}

/// `ItemCost.test`：同种物品且要求的组件完全相等。
pub(crate) fn cost_matches(cost: &ItemCost, stack: &ItemStack) -> bool {
    if stack.is_empty() || stack.item != cost.item {
        return false;
    }
    if cost.components.is_empty() {
        return true;
    }
    let patch = stack.patch();
    for required in &cost.components {
        match patch.get(required.kind) {
            Some(value) if *value == required.value => {}
            _ => return false,
        }
    }
    true
}

// 补货（`shouldRestock` / `restock` / `catchUpDemand` / `updateDemand`）与
// `updateSpecialPrices` 的状态转移都在 `mc-bamboo-blocks` 的 `Villager` 上，村民大脑
// `WorkAtPoi` 直接调用；这一侧只保留菜单与封包逻辑，不留第二份实现。
//
// Restock (`shouldRestock` / `restock` / `catchUpDemand` / `updateDemand`) and the
// `updateSpecialPrices` state transition live on `mc-bamboo-blocks`' `Villager`; the
// `WorkAtPoi` brain behaviour calls them directly, so this side keeps only menu/packet
// logic and no second copy.

/// `stillValid`：交易玩家相同、村民活着、4 格距离内。
pub(crate) fn still_valid(region: &Region, player: PlayerId, menu: &Menu) -> bool {
    let Some(state) = menu.merchant.as_ref() else {
        return false;
    };
    let Some(p) = region.players.get(&player) else {
        return false;
    };
    let Some(public) = region.entities.public_at(state.villager) else {
        return false;
    };
    if public.removed {
        return false;
    }
    // `AbstractVillager.stillValid(Player)`：`getTradingPlayer() == player`。
    let same_trader = region
        .entities
        .get(state.villager)
        .and_then(|entity| match &entity.kind {
            mc_bamboo_blocks::entity::EntityKind::Mob(mob) => {
                Some(mob.villager.still_valid_trading(player.0))
            }
            _ => None,
        })
        .unwrap_or(false);
    if !same_trader {
        return false;
    }
    let eye = Vec3::new(
        p.position().x,
        p.position().y + p.eye_height(),
        p.position().z,
    );
    let reach = p
        .attributes
        .value(registries::attribute::ENTITY_INTERACTION_RANGE)
        + INTERACTION_BUFFER;
    let delta = Vec3::new(
        public.position.x - eye.x,
        public.position.y - eye.y,
        public.position.z - eye.z,
    );
    delta.length_sqr() < reach * reach
}

/// `ClientboundMerchantOffersPacket` 在升级和补货后重发。
pub(crate) fn send_offers(region: &mut Region, player: PlayerId, menu: &Menu) {
    let Some(state) = menu.merchant.as_ref() else {
        return;
    };
    let Some(entity) = region.entities.get(state.villager) else {
        return;
    };
    let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind else {
        return;
    };
    let trader = mob.mob_type == MobType::WanderingTrader;
    let (level, xp, offers) = (
        if trader { 1 } else { mob.villager.level },
        if trader { 0 } else { mob.villager.xp },
        mob.villager.offers.clone().unwrap_or_default(),
    );
    region.links.send(
        player,
        &MerchantOffersPacket {
            container_id: menu.container_id,
            offers: &offers,
            villager_level: level,
            villager_xp: xp,
            show_progress: !trader,
            can_restock: !trader,
        },
    );
}

/// 组件补丁构造（对拍和存档转换用）。
#[allow(dead_code)]
fn patch_of(components: &[(i32, ComponentValue)]) -> ComponentPatch {
    let mut patch = ComponentPatch::new();
    for (kind, value) in components {
        patch.put(*kind, PatchEntry::Set(value.clone()));
    }
    patch
}

#[cfg(test)]
mod click_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::items;

    fn empty_menu() -> Menu {
        Menu::merchant(1, 7)
    }

    #[test]
    fn cost_count_follows_the_vanilla_formula() {
        let mut offer = MerchantOffer {
            base_cost_a: ItemCost::new(items::EMERALD, 4),
            cost_b: None,
            result: ItemStack::new(items::IRON_BOOTS, 1),
            uses: 0,
            max_uses: 12,
            reward_exp: true,
            special_price_diff: 0,
            demand: 2,
            price_multiplier: 0.2,
            xp: 2,
        };
        assert_eq!(cost_count(&offer, &offer.base_cost_a), 5);
        offer.special_price_diff = -2;
        assert_eq!(cost_count(&offer, &offer.base_cost_a), 3);
        let _ = empty_menu();
    }

    /// `moveFromInventoryToPaymentSlot` 把整堆匹配物品搬进付款格（不只 `cost.count`）。
    #[test]
    fn auto_fill_moves_the_whole_matching_stack() {
        let mut inventory = crate::inventory::Inventory::default();
        inventory.slots[crate::menu::INVENTORY_MENU_START] = ItemStack::new(items::EMERALD, 4);
        let mut payment = ItemStack::empty();
        move_from_inventory_to_payment(
            &mut inventory,
            &mut payment,
            &ItemCost::new(items::EMERALD, 3),
        );
        assert_eq!(payment.item, items::EMERALD);
        assert_eq!(payment.count, 4);
        assert!(inventory.slots[crate::menu::INVENTORY_MENU_START].is_empty());
    }

    /// `moveItemStackTo(stack, 3, 39, true)` 从后往前搬回背包的最后一格。
    #[test]
    fn payment_moves_back_to_the_last_inventory_slot() {
        let mut inventory = crate::inventory::Inventory::default();
        let mut payment = ItemStack::new(items::EMERALD, 4);
        assert!(move_payment_to_inventory(&mut inventory, &mut payment));
        assert!(payment.is_empty());
        let slot = crate::inventory::OFF_HAND - 1;
        assert_eq!(inventory.slots[slot].item, items::EMERALD);
        assert_eq!(inventory.slots[slot].count, 4);
    }
}

/// 端到端：空手右键有职业的村民 → 打开交易界面 → 放入付款 → 拿到结果。
#[cfg(test)]
mod e2e_tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_blocks::entity::{Entity, mob::MobType};
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::{items, packets::play::clientbound};
    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::play::{ClientInformation, ContainerInput, GameMode},
    };
    use mc_bamboo_world::flat;
    use uuid::Uuid;

    use super::*;
    use crate::{
        Outgoing, PlayerSink,
        menu::{ClickPacket, Menu},
        player::Player,
    };

    #[derive(Default)]
    struct SinkState {
        bytes: Vec<u8>,
        closed: bool,
    }

    #[derive(Clone, Default)]
    struct TestSink(Arc<Mutex<SinkState>>);

    impl PlayerSink for TestSink {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0.lock().unwrap().bytes.extend(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {
            self.0.lock().unwrap().closed = true;
        }
    }

    /// 建一个只有玩家和有职业村民的区域。村民就在玩家旁边。
    fn fixture() -> (Region, PlayerId, i32, TestSink) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let sink = TestSink::default();
        region.links.add(id, Box::new(sink.clone()));
        let position = Vec3::new(0.5, f64::from(flat::surface_y()), 0.5);
        let player = Player::new(
            id,
            Uuid::from_u128(1),
            "trader".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            position,
        );
        region.insert_player(player);
        let villager = region
            .entities
            .spawn(Entity::by_mob_type(MobType::Villager, position));
        region.entities.with_mut(villager, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
                mob.villager.profession = data::profession_id("minecraft:armorer").unwrap();
                mob.villager.level = 1;
                mob.villager.kind = data::type_id("minecraft:plains").unwrap();
            }
        });
        (region, id, villager, sink)
    }

    /// 发出去的包 ID，按 `links.flush` 之后 sink 里的字节解帧。
    fn packet_ids(sink: &TestSink) -> Vec<i32> {
        let bytes = std::mem::take(&mut sink.0.lock().unwrap().bytes);
        let mut rest = bytes.as_slice();
        let mut ids = Vec::new();
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN).unwrap().unwrap();
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            ids.push(reader.varint().unwrap());
            rest = &rest[end..];
        }
        ids
    }

    #[test]
    fn a_player_can_complete_a_real_trade() {
        let (mut region, id, villager_id, sink) = fixture();
        // 空手右键：打开交易界面，发 open_screen + merchant_offers + 槽位内容。
        crate::interaction::on_interact_entity(
            &mut region,
            id,
            villager_id,
            mc_bamboo_core::Hand::Main,
            false,
        );
        assert!(region.menus.contains_key(&id), "右键应该已经打开了交易界面");
        region.links.flush();
        let ids = packet_ids(&sink);
        assert!(
            ids.contains(&clientbound::OPEN_SCREEN),
            "没有 open_screen：{ids:?}"
        );
        assert!(
            ids.contains(&clientbound::MERCHANT_OFFERS),
            "没有 merchant_offers：{ids:?}"
        );
        let Some(menu) = region.menus.get(&id) else {
            panic!("菜单没打开");
        };
        assert!(menu.merchant.is_some());
        let offers = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => mob.villager.offers.clone(),
                _ => None,
            })
            .expect("村民应该懒生成了交易表");
        assert!(!offers.is_empty());
        let offer = offers[0].clone();
        // 直接把付款放进商人的两格，重算结果，再点结果格取出。
        let container_id = region.menus[&id].container_id;
        if let Some(state) = region.menus.get_mut(&id).and_then(|m| m.merchant.as_mut()) {
            state.payment[0] = ItemStack::new(offer.base_cost_a.item, offer.base_cost_a.count);
            if let Some(cost) = &offer.cost_b {
                state.payment[1] = ItemStack::new(cost.item, cost.count);
            }
        }
        crate::menu::broadcast_merchant_state(&mut region, id);
        {
            let state = region.menus[&id].merchant.as_ref().unwrap();
            assert_eq!(state.result, offer.result, "结果格应该显示交易结果");
            assert!(state.active.is_some());
        }
        let packet = ClickPacket {
            container_id,
            state_id: region.menus[&id].state_id,
            slot: 2,
            button: 0,
            input: ContainerInput::Pickup,
            changed_slots: Default::default(),
            carried: mc_bamboo_protocol::packet::play::HashedStack::Empty,
        };
        crate::menu::on_click(&mut region, id, packet);
        // 玩家拿到结果（官方 `doClick` 的 PICKUP 把它放到光标上），付款被扣光，
        // 村民的使用次数加一。
        // The player holds the result (vanilla PICKUP puts it on the cursor), the
        // payment is gone and the offer counts one use.
        let carried = region.menus[&id].carried();
        assert!(
            carried.item == offer.result.item && carried.count >= offer.result.count,
            "玩家没拿到交易结果"
        );
        let after = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => mob.villager.offers.clone(),
                _ => None,
            })
            .unwrap();
        assert_eq!(after[0].uses, 1, "交易没有被记一次");
        let state = region.menus[&id].merchant.as_ref().unwrap();
        assert!(state.payment[0].is_empty(), "付款没扣");
        let _: Option<&Menu> = region.menus.get(&id);
        let _ = items::AIR;
    }

    /// 流浪商人：懒生成 buying→uncommon→common 共 9 笔交易，界面不带进度条，
    /// 完成一笔不升级、不加村民经验（只加使用次数）。
    #[test]
    fn a_wandering_trader_shares_the_merchant_menu_without_villager_progression() {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let sink = TestSink::default();
        region.links.add(id, Box::new(sink.clone()));
        let position = Vec3::new(0.5, f64::from(flat::surface_y()), 0.5);
        region.insert_player(Player::new(
            id,
            Uuid::from_u128(2),
            "customer".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            position,
        ));
        let trader = region
            .entities
            .spawn(Entity::by_mob_type(MobType::WanderingTrader, position));
        region.entities.with_mut(trader, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
                mob.wandering_trader.despawn_delay = 48000;
            }
        });
        crate::interaction::on_interact_entity(
            &mut region,
            id,
            trader,
            mc_bamboo_core::Hand::Main,
            false,
        );
        assert!(region.menus.contains_key(&id), "右键应该已经打开了交易界面");
        let offers = region
            .entities
            .get(trader)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => mob.villager.offers.clone(),
                _ => None,
            })
            .expect("流浪商人应该懒生成了交易表");
        assert_eq!(offers.len(), 9, "三张交易集分别是 2 / 2 / 5 笔");
        // 界面包按 `links.flush` 之后才能从 sink 里读到，交易表本身已生成好。
        region.links.flush();
        assert!(!packet_ids(&sink).is_empty(), "应该发了 open_screen 等包");

        let offer = offers[0].clone();
        let container_id = region.menus[&id].container_id;
        if let Some(state) = region.menus.get_mut(&id).and_then(|m| m.merchant.as_mut()) {
            state.payment[0] = ItemStack::new(offer.base_cost_a.item, offer.base_cost_a.count);
            if let Some(cost) = &offer.cost_b {
                state.payment[1] = ItemStack::new(cost.item, cost.count);
            }
        }
        crate::menu::broadcast_merchant_state(&mut region, id);
        let click = ClickPacket {
            container_id,
            state_id: region.menus[&id].state_id,
            slot: 2,
            button: 0,
            input: ContainerInput::Pickup,
            changed_slots: Default::default(),
            carried: mc_bamboo_protocol::packet::play::HashedStack::Empty,
        };
        crate::menu::on_click(&mut region, id, click);
        let entity = region.entities.get(trader).unwrap();
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind else {
            panic!("流浪商人应该是生物");
        };
        assert_eq!(mob.villager.offers.as_ref().unwrap()[0].uses, 1);
        assert_eq!(mob.villager.level, 1, "流浪商人不升级");
        assert_eq!(mob.villager.xp, 0, "流浪商人不加村民经验");
        assert_eq!(mob.villager.offers.as_ref().unwrap().len(), 9);
        assert_eq!(mob.wandering_trader.despawn_delay, 48000);
    }

    /// 交易经验跨过阈值后升级：追加新等级交易、等级 +1、挂 200 刻再生、经验累加。
    #[test]
    fn a_trade_that_reaches_the_xp_threshold_levels_up() {
        use mc_bamboo_data::registries::mob_effect;

        let (mut region, id, villager_id, _sink) = fixture();
        region.entities.with_mut(villager_id, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
                mob.villager.xp = 9;
            }
        });
        crate::interaction::on_interact_entity(
            &mut region,
            id,
            villager_id,
            mc_bamboo_core::Hand::Main,
            false,
        );
        let offers = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => mob.villager.offers.clone(),
                _ => None,
            })
            .expect("村民应该懒生成了交易表");
        let offer = offers[0].clone();
        let container_id = region.menus[&id].container_id;
        if let Some(state) = region.menus.get_mut(&id).and_then(|m| m.merchant.as_mut()) {
            state.payment[0] = ItemStack::new(offer.base_cost_a.item, offer.base_cost_a.count);
            if let Some(cost) = &offer.cost_b {
                state.payment[1] = ItemStack::new(cost.item, cost.count);
            }
        }
        crate::menu::broadcast_merchant_state(&mut region, id);
        let packet = ClickPacket {
            container_id,
            state_id: region.menus[&id].state_id,
            slot: 2,
            button: 0,
            input: ContainerInput::Pickup,
            changed_slots: Default::default(),
            carried: mc_bamboo_protocol::packet::play::HashedStack::Empty,
        };
        crate::menu::on_click(&mut region, id, packet);
        let (level, xp, offers_len, regen) = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => Some((
                    mob.villager.level,
                    mob.villager.xp,
                    mob.villager.offers.as_ref().map_or(0, Vec::len),
                    mob.effects
                        .get(mob_effect::REGENERATION)
                        .map(|instance| instance.duration),
                )),
                _ => None,
            })
            .unwrap();
        assert_eq!(level, 2, "经验过阈值应升到 2 级");
        assert_eq!(xp, 9 + offer.xp, "经验累加");
        assert!(offers_len > offers.len(), "升级应追加新等级交易");
        assert_eq!(regen, Some(200), "升级应挂 200 刻再生");
    }

    /// 幼年村民右键摇头，不打开交易界面。
    #[test]
    fn a_baby_villager_does_not_open_the_trade() {
        let (mut region, id, villager_id, _sink) = fixture();
        region.entities.with_mut(villager_id, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
                mob.age = -100;
            }
        });
        crate::interaction::on_interact_entity(
            &mut region,
            id,
            villager_id,
            mc_bamboo_core::Hand::Main,
            false,
        );
        assert!(
            !region.menus.contains_key(&id),
            "幼年村民不应该打开交易界面"
        );
        let unhappy = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => Some(mob.villager.unhappy),
                _ => None,
            })
            .unwrap();
        assert_eq!(unhappy, 40, "幼年村民应该摇头（unhappy 计数 40）");
    }

    /// 睡觉的村民右键走通用 mobInteract，不打开交易界面也不摇头。
    #[test]
    fn a_sleeping_villager_does_not_open_the_trade() {
        let (mut region, id, villager_id, _sink) = fixture();
        region.entities.with_mut(villager_id, |entity| {
            if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
                mob.villager.sleeping = true;
            }
        });
        crate::interaction::on_interact_entity(
            &mut region,
            id,
            villager_id,
            mc_bamboo_core::Hand::Main,
            false,
        );
        assert!(
            !region.menus.contains_key(&id),
            "睡觉的村民不应该打开交易界面"
        );
        let unhappy = region
            .entities
            .get(villager_id)
            .and_then(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::Mob(mob) => Some(mob.villager.unhappy),
                _ => None,
            })
            .unwrap();
        assert_eq!(unhappy, 0, "睡觉的村民不应该摇头");
    }
}
