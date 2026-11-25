//! 村民交易：`AbstractVillager` 的懒生成交易表、`MerchantMenu` 的三格和一次真实交易。
//!
//! 一名玩家右键有职业的村民时打开菜单。玩家的 36 格物品栏由 [`crate::menu`]
//! 的点击逻辑管理，这里只管付款格 0 / 1 和结果格 2：按选中的交易算出结果堆，
//! 取出结果时扣两格付款、累加使用次数、发经验和升级。4 格距离之外或村民死了
//! 菜单就失效。

use mc_bamboo_blocks::entity::mob::MobType;
use mc_bamboo_blocks::entity::mob::villager::Villager;
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
    packet::play::{MerchantOffersPacket, OpenScreen},
    text::Component,
};
use serde_json::Value;

use crate::{
    inbound::PlayerId,
    loot_stacks,
    menu::{ClickPacket, Menu},
    region::Region,
};

/// `MerchantContainer` 的结果格。
const RESULT_SLOT: usize = 2;
/// `AbstractVillager.stillValid` 的额外交互距离。
const INTERACTION_BUFFER: f64 = 4.0;
/// `Villager.restock` 的冷却和新的一天。
const RESTOCK_COOLDOWN: i64 = 2400;
const LONG_OVERDUE: i64 = 12000;

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
    position: Vec3,
    sequences: &RandomSequences,
) {
    if villager.offers.is_none() {
        let mut offers = Vec::new();
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
        villager.offers = Some(offers);
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

/// 空手右键有职业的成年村民：生成交易、发 `open_screen` 和交易列表、装好菜单。
pub(crate) fn open(region: &mut Region, player: PlayerId, villager_id: i32) -> bool {
    if !region.players.contains_key(&player) {
        return false;
    }
    let Some(public) = region.entities.public_at(villager_id) else {
        return false;
    };
    if public.removed || !matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_)) {
        return false;
    }
    let position = public.position;
    // 懒生成：借一次实体表就够了，`random_sequence` 走区域的序列缓存。
    let generated = {
        let Some(entity) = region.entities.get(villager_id) else {
            return false;
        };
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind else {
            return false;
        };
        if mob.mob_type != MobType::Villager || mob.villager.profession <= 0 {
            return false;
        }
        let mut villager = mob.villager.clone();
        ensure_offers(&mut villager, position, &region.loot_sequences);
        Some((villager, position))
    };
    let Some((villager, position)) = generated else {
        return false;
    };
    let level = villager.level;
    let xp = villager.xp;
    let title = {
        let name = data::profession(villager.profession)
            .map(|p| p.name)
            .unwrap_or("minecraft:none");
        Component::translate(format!("entity.minecraft.villager.{name}"), [])
    };
    let offers = villager.offers.clone().unwrap_or_default();
    if offers.is_empty() {
        return false;
    }
    let _ = position;
    let written = region.entities.with_mut(villager_id, |entity| {
        if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind {
            mob.villager.offers = villager.offers.clone();
            // `startTrading` 记下正在交易，村民降频豁免看它。
            mob.villager.trading = true;
        }
    });
    if written.is_none() {
        return false;
    }
    if region.menus.contains_key(&player) {
        crate::menu::close_menu(region, player);
    }
    let container_id = {
        let Some(p) = region.players.get_mut(&player) else {
            return false;
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
            villager_level: level,
            villager_xp: xp,
            show_progress: true,
            can_restock: true,
        },
    );
    crate::menu::broadcast_merchant_state(region, player);
    true
}

/// `ServerboundSelectTradePacket`：记下客户端选的是第几条，然后重算结果格。
pub(crate) fn on_select_trade(region: &mut Region, player: PlayerId, slot: i32) {
    let Some(mut menu) = region.menus.remove(&player) else {
        return;
    };
    if let Some(state) = menu.merchant.as_mut() {
        state.selection_hint = slot.max(0);
        slots_changed(region, player, &mut menu);
    }
    region.menus.insert(player, menu);
}

/// 玩家点击了商人菜单的槽位。付款格用普通点击逻辑改，结果格只在能交易时取出。
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
    if packet.slot as usize == RESULT_SLOT {
        let Some(mut menu) = region.menus.remove(&player) else {
            return false;
        };
        let traded = take_result(region, player, &mut menu);
        crate::menu::broadcast_merchant_state(region, player);
        region.menus.insert(player, menu);
        let _ = traded;
        return true;
    }
    // 付款 / 玩家物品栏点击：交给 menu 的通用逻辑，它自己会重算并同步。
    crate::menu::on_click_default(region, player, packet);
    true
}

/// `MerchantContainer.updateSellItem`：找到选中且能支付、没卖光的交易，算出结果堆。
pub(crate) fn slots_changed(region: &Region, _player: PlayerId, menu: &mut Menu) {
    let Some(state) = menu.merchant.as_mut() else {
        return;
    };
    state.active = None;
    state.result = ItemStack::empty();
    let Some(villager_id) = menu.target.villager() else {
        return;
    };
    let Some(offers) = region
        .entities
        .public_at(villager_id)
        .and_then(|_| villager_offers(region, villager_id))
    else {
        return;
    };
    let a = state.payment[0].clone();
    let b = state.payment[1].clone();
    if a.is_empty() {
        return;
    }
    // `MerchantOffers.getRecipeFor(a, b, hint)`：hint > 0 时先只看那一条，
    // 卖光或付不起就退回从第一条开始扫。
    let hint = state.selection_hint as usize;
    let matches = |candidates: &[MerchantOffer]| -> Option<usize> {
        candidates
            .iter()
            .enumerate()
            .find(|(_, offer)| !offer.is_out_of_stock() && satisfied_by(offer, &a, &b))
            .map(|(i, _)| i)
    };
    let index = if hint > 0 && hint < offers.len() {
        matches(&offers[hint..hint + 1])
            .map(|i| hint + i)
            .or_else(|| matches(&offers))
    } else {
        matches(&offers)
    };
    let Some(index) = index else {
        return;
    };
    state.active = Some(index);
    state.result = offers[index].result.clone();
}

/// 只读地取出村民的交易表。
fn villager_offers(region: &Region, villager_id: i32) -> Option<Vec<MerchantOffer>> {
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

/// 取走结果：扣付款、增加使用次数、给经验、可能升级。
fn take_result(region: &mut Region, player: PlayerId, menu: &mut Menu) -> bool {
    let Some(state) = menu.merchant.as_mut() else {
        return false;
    };
    let Some(index) = state.active else {
        return false;
    };
    let Some(villager_id) = menu.target.villager() else {
        return false;
    };
    let Some(result) = region.players.get(&player).map(|_| state.result.clone()) else {
        return false;
    };
    if result.is_empty() {
        return false;
    }
    let Some(offer) =
        villager_offers(region, villager_id).and_then(|offers| offers.get(index).cloned())
    else {
        return false;
    };
    let a = state.payment[0].clone();
    let b = state.payment[1].clone();
    if !satisfied_by(&offer, &a, &b) {
        return false;
    }
    let cost_a = cost_count(&offer, &offer.base_cost_a);
    let cost_b = offer.cost_b.as_ref().map(|cost| cost_count(&offer, cost));
    state.payment[0].count -= cost_a;
    if state.payment[0].count <= 0 {
        state.payment[0] = ItemStack::empty();
    }
    if let Some(count) = cost_b {
        state.payment[1].count -= count;
        if state.payment[1].count <= 0 {
            state.payment[1] = ItemStack::empty();
        }
    }
    if let Some(player_data) = region.players.get_mut(&player) {
        let _ = player_data.inventory.receive(result.clone(), false);
    }
    crate::advancement_triggers::record(
        region,
        player,
        "minecraft:villager_trade",
        serde_json::json!({
            "item": {"id": mc_bamboo_loot::item_name(result.item).unwrap_or("")},
            "villager": {"type": "minecraft:villager"}
        }),
    );
    let mut level_up = false;
    let mut xp = 0;
    region.entities.with_mut(villager_id, |entity| {
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind else {
            return;
        };
        let Some(offers) = mob.villager.offers.as_mut() else {
            return;
        };
        let Some(offer) = offers.get_mut(index) else {
            return;
        };
        offer.uses += 1;
        mob.villager.xp += offer.xp;
        xp = offer.xp;
        if should_increase_level(mob.villager.level, mob.villager.xp)
            && let Some(profession) = data::profession(mob.villager.profession)
        {
            let next = mob.villager.level + 1;
            if let Some(name) = profession.trade_sets_by_level.get((next - 1) as usize)
                && !name.is_empty()
                && let Ok(set) = TradeSet::parse(name)
            {
                let params = TradeParams {
                    world_seed: 0,
                    origin: Position::new(entity.position.x, entity.position.y, entity.position.z),
                    villager_type: data::type_name(mob.villager.kind).unwrap_or(data::DEFAULT_TYPE),
                };
                let fallback = mc_bamboo_blocks::random::LegacyRandom::new(0);
                let mut generated = Vec::new();
                let _ = add_offers_from_trade_set(
                    &set,
                    &params,
                    Some(&region.loot_sequences),
                    &fallback,
                    &mut generated,
                );
                for generated in &generated {
                    if let Some(offer) = generated_offer_to_merchant(generated) {
                        offers.push(offer);
                    }
                }
                mob.villager.level = next;
                level_up = true;
            }
        }
    });
    let _ = xp;
    // 交易完成后把新的使用次数 / 等级 / 交易表发给客户端，`uses` 变化会改「售罄」状态。
    // After a trade, resend the updated offers: `uses` changes the out-of-stock state
    // and a level-up appends a new tier.
    spawn_trade_xp(region, villager_id, level_up, offer.reward_exp);
    send_offers(region, player, menu);
    // `Stats.TRADED_WITH_VILLAGER` 在 Bamboo 还没有通用统计表，
    // 触发器和进度用同一个事件；统计计数待期 6 的统计表接入。
    true
}

fn should_increase_level(level: i32, xp: i32) -> bool {
    let need = match level {
        1 => 10,
        2 => 70,
        3 => 150,
        4 => 250,
        _ => return false,
    };
    level < 5 && xp >= need
}

/// `Villager.rewardTradeXp`：`rewardExp` 为真时在村民头上撒 `3 + nextInt(4)` 点
/// 经验球，升级再 +5（再生 200 刻在升级分支里已经给了）。
///
/// `Villager.rewardTradeXp`: if `rewardExp`, drop `3 + nextInt(4)` experience at the
/// villager, plus 5 on a level-up (the 200-tick regeneration is applied in the level-up
/// branch already).
fn spawn_trade_xp(region: &mut Region, villager_id: i32, level_up: bool, reward_exp: bool) {
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
    let amount = 3 + region.random.source().next_int(4) + if level_up { 5 } else { 0 };
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        mc_bamboo_blocks::entity::award(&mut world, position, amount);
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

/// `MerchantOffer.getModifiedCostCount`。
fn cost_count(offer: &MerchantOffer, cost: &ItemCost) -> i32 {
    let demand = (cost.count * offer.demand) as f32 * offer.price_multiplier;
    let extra = demand.floor().max(0.0) as i32;
    (cost.count + extra + offer.special_price_diff).clamp(1, cost_stack_max(cost))
}

fn cost_stack_max(cost: &ItemCost) -> i32 {
    mc_bamboo_data::items::ITEMS
        .get(cost.item as usize)
        .map_or(99, |item| item.max_stack_size)
}

/// `MerchantOffer.satisfiedBy`：物品和精确组件一致，数量够；B 存在时第二格也要够。
fn satisfied_by(offer: &MerchantOffer, a: &ItemStack, b: &ItemStack) -> bool {
    if !cost_matches(&offer.base_cost_a, a) || a.count < cost_count(offer, &offer.base_cost_a) {
        return false;
    }
    match &offer.cost_b {
        Some(cost) => cost_matches(cost, b) && b.count >= cost_count(offer, cost),
        None => b.is_empty(),
    }
}

/// `ItemCost.test`：同种物品且要求的组件完全相等。
fn cost_matches(cost: &ItemCost, stack: &ItemStack) -> bool {
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

/// 官方 `Villager.updateSpecialPrices`：声望（期 6 接八卦）先按 0 算，
/// 英雄效果按 `0.3 + 0.0625 * amplifier` 打折。
#[allow(dead_code)]
pub(crate) fn update_special_prices(
    region: &mut Region,
    villager_id: i32,
    hero_amplifier: Option<i32>,
) {
    // 声望在第 6 期接八卦之前恒为 0。
    let reputation = 0_i32;
    region.entities.with_mut(villager_id, |entity| {
        let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &mut entity.kind else {
            return;
        };
        let Some(offers) = mob.villager.offers.as_mut() else {
            return;
        };
        for offer in offers.iter_mut() {
            offer.special_price_diff = 0;
            let factor = hero_amplifier.map_or(0.0, |level| 0.3 + 0.0625 * f64::from(level));
            let discount = if factor > 0.0 {
                (factor * f64::from(offer.base_cost_a.count))
                    .floor()
                    .max(1.0) as i32
            } else {
                0
            };
            offer.special_price_diff -=
                discount + (reputation as f32 * offer.price_multiplier).floor() as i32;
        }
    });
}

/// `Villager.shouldRestock(level)`：跨天或长时间没补货时先重置计数，
/// 再返回「今天还能补且需要补」。`day` 是 OVERWORLD_DAY 的天数（`now / 24000` 的近似）。
///
/// `Villager.shouldRestock(level)`: on a new day or after a long overdue gap, reset the
/// daily counter first, then report whether a restock is both allowed and needed. `day` is
/// the OVERWORLD_DAY day number.
#[allow(dead_code)] // 补货由村民大脑 WorkAtPoi 触发，交易这一侧先提供接口。
pub(crate) fn should_restock(v: &mut Villager, now: i64, day: i64) -> bool {
    let long_overdue = now > v.last_restock + LONG_OVERDUE;
    let new_day = v.last_restock_check_day > 0 && day > v.last_restock_check_day;
    v.last_restock_check_day = day;
    if long_overdue || new_day {
        v.last_restock = now;
        reset_number_of_restocks(v);
    }
    allowed_to_restock(v, now) && needs_restock(v)
}

/// `Villager.restock()`：把每笔交易按使用次数折算进需求，再清零使用次数。
/// 官方在这里还会对有交易玩家调 `updateSpecialPrices`，声望期 6 才接，先跳过。
///
/// `Villager.restock()`: fold each offer's uses into demand, then clear the uses.
/// Vanilla also recomputes special prices for the trading player here; that lands with
/// gossip in phase 6, so it is skipped for now.
#[allow(dead_code)] // 补货由村民大脑 WorkAtPoi 触发，交易这一侧先提供接口。
pub(crate) fn restock(v: &mut Villager, now: i64) {
    for offer in v.offers.iter_mut().flatten() {
        update_demand(offer);
        offer.uses = 0;
    }
    v.last_restock = now;
    v.restocks_today += 1;
}

/// `Villager.resetNumberOfRestocks()`。
fn reset_number_of_restocks(v: &mut Villager) {
    catch_up_demand(v);
    v.restocks_today = 0;
}

/// `Villager.catchUpDemand()`：把没补到的那几次的需求补上。
fn catch_up_demand(v: &mut Villager) {
    let misses = (2 - v.restocks_today).max(0);
    if misses > 0 {
        for offer in v.offers.iter_mut().flatten() {
            offer.uses = 0;
        }
    }
    for _ in 0..misses {
        for offer in v.offers.iter_mut().flatten() {
            update_demand(offer);
        }
    }
}

/// `MerchantOffer.updateDemand`：`demand += 2 * uses - maxUses`。
fn update_demand(offer: &mut MerchantOffer) {
    offer.demand += 2 * offer.uses - offer.max_uses;
}

fn allowed_to_restock(v: &Villager, now: i64) -> bool {
    v.restocks_today == 0 || (v.restocks_today < 2 && now > v.last_restock + RESTOCK_COOLDOWN)
}

fn needs_restock(v: &Villager) -> bool {
    v.offers
        .as_ref()
        .is_some_and(|offers| offers.iter().any(|offer| offer.uses > 0))
}

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
    let (level, xp, offers) = (
        mob.villager.level,
        mob.villager.xp,
        mob.villager.offers.clone().unwrap_or_default(),
    );
    region.links.send(
        player,
        &MerchantOffersPacket {
            container_id: menu.container_id,
            offers: &offers,
            villager_level: level,
            villager_xp: xp,
            show_progress: true,
            can_restock: true,
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

    /// `MerchantOffer.updateDemand` 的公式来自字节码：`demand += 2*uses - maxUses`。
    #[test]
    fn update_demand_follows_the_vanilla_formula() {
        let mut offer = MerchantOffer {
            base_cost_a: ItemCost::new(items::EMERALD, 1),
            cost_b: None,
            result: ItemStack::new(items::APPLE, 1),
            uses: 3,
            max_uses: 12,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier: 0.0,
            xp: 1,
        };
        update_demand(&mut offer);
        assert_eq!(offer.demand, 2 * 3 - 12);
    }

    /// 补货跨天重置计数，且只有「今天还能补」和「有使用过」同时成立才补。
    #[test]
    fn restock_only_fires_when_needed_and_allowed() {
        let mut v = Villager {
            offers: Some(vec![MerchantOffer {
                base_cost_a: ItemCost::new(items::EMERALD, 1),
                cost_b: None,
                result: ItemStack::new(items::APPLE, 1),
                uses: 1,
                max_uses: 12,
                reward_exp: true,
                special_price_diff: 0,
                demand: 0,
                price_multiplier: 0.0,
                xp: 1,
            }]),
            ..Villager::default()
        };
        // 第一次：需要补（uses>0）且允许（restocks_today==0）。
        assert!(should_restock(&mut v, 1000, 0));
        restock(&mut v, 1000);
        assert_eq!(v.restocks_today, 1);
        assert_eq!(v.offers.as_ref().unwrap()[0].uses, 0);
        // 刚补完还没冷却，不再补。
        assert!(!should_restock(&mut v, 2000, 0));
        // 跨天重置当天计数：先让检查日变成正数，再跨到第二天。
        v.last_restock_check_day = 1;
        assert!(
            !should_restock(&mut v, 3000, 2),
            "跨天重置后 uses 已经清零，不需要补"
        );
        assert_eq!(v.restocks_today, 0);
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
        // 玩家拿到结果，付款被扣光，村民的使用次数加一。
        let player = region.players.get(&id).unwrap();
        assert!(
            player
                .inventory
                .slots
                .iter()
                .any(|stack| stack.item == offer.result.item && stack.count >= offer.result.count),
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
}
